//! The user directory in libpinyin's own file shapes — drop-in task 9's
//! persistence half.
//!
//! The store's durable representation is the pin's file set
//! (`docs/findings/user-store.md` §4 for the save cycle's semantics,
//! `oxpinyin_data::user_files` for the inventory and codecs):
//!
//! * load — `check_format` (user.conf conformance, the open counter,
//!   `_clean_user_files` on a non-conform profile), then the bigram
//!   hash, the USER_FILE chunk stores (`user.bin`, `addon.bin`,
//!   `network.bin`), and the SYSTEM_FILE `.dbin` logs replayed onto the
//!   original system chunks. The two index DBMs (`user_pinyin_index.bin`,
//!   `user_phrase_index.bin`) are pure derivatives of the USER_FILE
//!   items — every index row was added by the same `add_index` walk that
//!   inserted the item — so they are rebuilt at save and not read.
//! * save — the pin's `_write_files` + `_rename_files`: every file is
//!   written whole to a `.tmp` sibling, then all are renamed over their
//!   finals, so a crash mid-save leaves the previous profile intact.
//!
//! Nothing here holds a container open: the DBM files are created,
//! written and closed during save, and opened read-only during load —
//! the pin's own lifecycle (`Bigram::load_db` copies into memory,
//! `save_db` writes a fresh file).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oxpinyin_core::ChewingKey;
use oxpinyin_data::chunk_format::build_memory_chunk;
use oxpinyin_data::chunk_write::{
    ChunkItem, PHRASE_MASK, build_chunk, decode_phrase_item, decode_sub_phrase_index,
    encode_phrase_item,
};
use oxpinyin_data::single_gram::{decode_single_gram, encode_single_gram};
use oxpinyin_data::table_entries::{ParsedRow, phrase_index_entries, pinyin_index_entries};
use oxpinyin_data::user_files::{
    LogRecord, OPEN_COUNTER_LIMIT, SYSTEM_LOG_FILES, SystemVersions, USER_LIBRARY_FILES, UserDbm,
    UserTableInfo, decode_log_records, encode_log_records, read_chunk_payload,
};
use oxpinyin_store::{DefaultStore, RawReadStore, StoreError, WriteStore};

/// `USER_TABLE_INFO` (`pinyin_internal.h:56`).
const USER_CONF: &str = "user.conf";

/// One user-bigram gram: the previous token's `SingleGram`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Gram {
    /// The gram's `total_freq`.
    pub total: u32,
    /// `(next token, count)`, token-ascending (`insert_freq`'s order).
    pub items: BTreeMap<u32, u32>,
}

/// One system library's original state — the shipped chunk as loaded,
/// the diff base of its `.dbin` log.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SystemLibrary {
    /// The chunk's `total_freq`.
    pub total: u32,
    /// Items by slot (`token & PHRASE_MASK`).
    pub items: BTreeMap<u32, ChunkItem>,
}

/// The user state, in the value shapes the file set carries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UserState {
    /// The user bigram by previous token.
    pub bigram: BTreeMap<u32, Gram>,
    /// The USER_FILE sub-indexes (5, 6, 7) by nibble, items by slot.
    pub libraries: BTreeMap<u8, BTreeMap<u32, ChunkItem>>,
    /// The SYSTEM_FILE libraries' touched items by nibble and slot:
    /// `Some(item)` modifies the original, `None` removes it. A slot
    /// absent here still answers its original item.
    pub system_overrides: BTreeMap<u8, BTreeMap<u32, Option<ChunkItem>>>,
}

impl UserState {
    /// The current item for a system-library slot — the override when
    /// one exists, the original otherwise.
    #[must_use]
    pub fn system_item<'a>(
        &'a self,
        originals: &'a BTreeMap<u8, SystemLibrary>,
        nibble: u8,
        slot: u32,
    ) -> Option<&'a ChunkItem> {
        if let Some(item) = self
            .system_overrides
            .get(&nibble)
            .and_then(|slots| slots.get(&slot))
        {
            return item.as_ref();
        }
        originals
            .get(&nibble)
            .and_then(|library| library.items.get(&slot))
    }
}

/// A persistence failure: I/O, a container, or a byte stream that does
/// not parse.
#[derive(Debug)]
pub enum PersistenceError {
    /// Filesystem failure.
    Io(std::io::Error),
    /// A store container failed to open, write or read.
    Store(StoreError),
    /// A byte stream did not parse under its frozen format.
    Codec(String),
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "user file io: {error}"),
            Self::Store(error) => write!(f, "user file store: {error}"),
            Self::Codec(message) => write!(f, "user file codec: {message}"),
        }
    }
}

impl std::error::Error for PersistenceError {}

impl From<std::io::Error> for PersistenceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<StoreError> for PersistenceError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<oxpinyin_data::chunk_write::ChunkWriteError> for PersistenceError {
    fn from(error: oxpinyin_data::chunk_write::ChunkWriteError) -> Self {
        Self::Codec(error.to_string())
    }
}

/// The result of [`load`].
#[derive(Clone, Debug, Default)]
pub struct Loaded {
    /// The decoded state (empty on a fresh or wiped profile).
    pub state: UserState,
    /// The open counter now recorded in `user.conf`.
    pub open_counter: u32,
    /// A non-conform profile was found and its files removed —
    /// `check_format`'s `_clean_user_files`, the ecosystem's own
    /// mechanism for "backend or model change discards user data".
    pub wiped: bool,
    /// Files whose bytes failed to parse; the profile continues without
    /// them (upstream's own degrade — `chunk->load` failure leaves an
    /// empty library, never a failed init).
    pub skipped: Vec<String>,
}

/// The library nibble of a token (`PHRASE_INDEX_LIBRARY_INDEX`).
const fn nibble_of(token: u32) -> u8 {
    ((token >> 24) & 0x0F) as u8
}

/// The full token of a system-library item.
const fn token_of(nibble: u8, slot: u32) -> u32 {
    ((nibble as u32) << 24) | slot
}

/// Reads the user dir: `check_format` first, then the profile.
///
/// This reproduces the pin's `pinyin_init` user-dir half, including the
/// `user.conf` write at init — the open counter the conformance check
/// ratchets on every open, upstream's periodic-rebuild heuristic.
///
/// # Errors
///
/// Returns [`PersistenceError`] only when the `user.conf` re-write
/// itself fails; unparsable profile files degrade per-file (see
/// [`Loaded::skipped`]), as upstream's do.
pub fn load(
    dir: &Path,
    originals: &BTreeMap<u8, SystemLibrary>,
    versions: &SystemVersions,
) -> Result<Loaded, PersistenceError> {
    let conf_path = dir.join(USER_CONF);
    let existing = std::fs::read_to_string(&conf_path)
        .ok()
        .and_then(|text| UserTableInfo::parse(&text).ok());

    let conform = existing
        .as_ref()
        .is_some_and(|info| info.is_conform(versions));

    // `check_format`'s counter ratchet: get (capped at the limit, so a
    // tired profile reads 0), +1, save — on every init, conform or not.
    // A missing user.conf is a fresh profile: 0 + 1.
    let counter = existing
        .as_ref()
        .map_or(1, |info| info.open_counter.min(OPEN_COUNTER_LIMIT) + 1);

    let mut loaded = Loaded {
        open_counter: counter,
        wiped: !conform,
        ..Loaded::default()
    };

    if !conform {
        clean_user_files(dir);
        let mut marker = UserTableInfo::conform_to(versions);
        marker.open_counter = counter;
        std::fs::write(&conf_path, marker.to_text())?;
        return Ok(loaded);
    }

    load_bigram(dir, &mut loaded);
    load_libraries(dir, &mut loaded);
    load_logs(dir, originals, &mut loaded);

    let mut marker = UserTableInfo::conform_to(versions);
    marker.open_counter = counter;
    std::fs::write(&conf_path, marker.to_text())?;
    Ok(loaded)
}

/// `_clean_user_files` + the fixed names: every file of the profile is
/// removed; absence is not an error.
fn clean_user_files(dir: &Path) {
    let mut names: Vec<String> = [
        UserDbm::Bigram.file_name(),
        UserDbm::PinyinIndex.file_name(),
        UserDbm::PhraseIndex.file_name(),
    ]
    .into_iter()
    .collect();
    names.extend(USER_LIBRARY_FILES.iter().map(|&(_, name)| name.to_owned()));
    names.extend(SYSTEM_LOG_FILES.iter().map(|&(_, name)| name.to_owned()));
    names.push(USER_CONF.to_owned());
    for name in names {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// The user bigram: the hash container's rows are the grams wholesale.
fn load_bigram(dir: &Path, loaded: &mut Loaded) {
    let path = dir.join(UserDbm::Bigram.file_name());
    if !path.exists() {
        return;
    }
    let store = match DefaultStore::open_hash_read_only(&path) {
        Ok(store) => store,
        Err(error) => {
            loaded
                .skipped
                .push(format!("{}: {error}", UserDbm::Bigram.file_name()));
            return;
        }
    };
    let mut visit = |key: &[u8], value: &[u8]| -> Result<(), StoreError> {
        if key.len() != 4 {
            return Ok(()); // not a token key; upstream never writes one
        }
        let prev = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        match decode_single_gram(value) {
            Ok((total, records)) => {
                let gram = Gram {
                    total,
                    items: records.into_iter().collect(),
                };
                loaded.state.bigram.insert(prev, gram);
            }
            Err(_) => loaded.skipped.push(format!(
                "{}: gram {prev:#010x} does not parse",
                UserDbm::Bigram.file_name()
            )),
        }
        Ok(())
    };
    if let Err(error) = store.range_raw(
        std::ops::Bound::Unbounded,
        std::ops::Bound::Unbounded,
        &mut visit,
    ) {
        loaded
            .skipped
            .push(format!("{}: {error}", UserDbm::Bigram.file_name()));
    }
}

/// The USER_FILE chunk stores.
fn load_libraries(dir: &Path, loaded: &mut Loaded) {
    for &(nibble, name) in USER_LIBRARY_FILES {
        let path = dir.join(name);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let payload = match read_chunk_payload(&bytes) {
            Ok(payload) => payload,
            Err(error) => {
                loaded.skipped.push(format!("{name}: {error}"));
                continue;
            }
        };
        match decode_sub_phrase_index(payload) {
            Ok((_total, items)) => {
                // An absent library and an empty one are the same state;
                // keep the map absent so save/load are byte- and
                // value-stable round trips.
                if !items.is_empty() {
                    loaded
                        .state
                        .libraries
                        .insert(nibble, items.into_iter().collect());
                }
            }
            Err(error) => loaded.skipped.push(format!("{name}: {error}")),
        }
    }
}

/// The `.dbin` logs, replayed onto the original system chunks —
/// `FacadePhraseIndex::merge`. A record whose old payload does not
/// match the current item stops that library's replay (upstream's
/// `merge` returns false mid-log and `_load_phrase_library` keeps the
/// partially-merged index).
fn load_logs(dir: &Path, originals: &BTreeMap<u8, SystemLibrary>, loaded: &mut Loaded) {
    for &(nibble, name) in SYSTEM_LOG_FILES {
        let path = dir.join(name);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let payload = match read_chunk_payload(&bytes) {
            Ok(payload) => payload,
            Err(error) => {
                loaded.skipped.push(format!("{name}: {error}"));
                continue;
            }
        };
        let records = match decode_log_records(payload) {
            Ok(records) => records,
            Err(error) => {
                loaded.skipped.push(format!("{name}: {error}"));
                continue;
            }
        };

        let original = originals.get(&nibble);
        let mut overrides: BTreeMap<u32, Option<ChunkItem>> = BTreeMap::new();
        let current =
            |overrides: &BTreeMap<u32, Option<ChunkItem>>, slot: u32| -> Option<ChunkItem> {
                match overrides.get(&slot) {
                    Some(item) => item.clone(),
                    None => original
                        .and_then(|library| library.items.get(&slot))
                        .cloned(),
                }
            };

        for record in records {
            match record {
                LogRecord::Add { token, new_item } if nibble_of(token) == nibble => {
                    match decode_phrase_item(&new_item) {
                        Ok(item) => {
                            overrides.insert(token & PHRASE_MASK, Some(item));
                        }
                        Err(error) => {
                            loaded.skipped.push(format!("{name}: {error}"));
                            break;
                        }
                    }
                }
                LogRecord::Remove { token, old_item } if nibble_of(token) == nibble => {
                    let slot = token & PHRASE_MASK;
                    match (decode_phrase_item(&old_item), current(&overrides, slot)) {
                        (Ok(old), Some(cur)) if old == cur => {
                            overrides.insert(slot, None);
                        }
                        (Ok(_), None) => {
                            overrides.insert(slot, None); // already gone; a no-op removal
                        }
                        _ => break, // payload mismatch: merge's failure return
                    }
                }
                LogRecord::Modify {
                    token,
                    old_item,
                    new_item,
                } if nibble_of(token) == nibble => {
                    let slot = token & PHRASE_MASK;
                    match (
                        decode_phrase_item(&old_item),
                        decode_phrase_item(&new_item),
                        current(&overrides, slot),
                    ) {
                        (Ok(old), Ok(new), Some(cur)) if old == cur => {
                            overrides.insert(slot, Some(new));
                        }
                        _ => break, // payload mismatch: merge's failure return
                    }
                }
                LogRecord::ModifyHeader {
                    old_total,
                    new_total: _,
                } => {
                    // The header must land on the current total; a
                    // mismatch is merge's failure return.
                    let now =
                        system_new_total(original.unwrap_or(&SystemLibrary::default()), &overrides);
                    if now != old_total {
                        break;
                    }
                }
                _ => break, // a record of another library's token: corrupt stream
            }
        }
        // An empty override set and an absent one are the same state.
        if !overrides.is_empty() {
            loaded.state.system_overrides.insert(nibble, overrides);
        }
    }
}

/// The library's new `total_freq` — the original plus the overrides'
/// deltas, `m_total_freq`'s incremental arithmetic recomputed
/// deterministically (saturating; upstream drops a delta that would
/// overflow instead, an edge no training run reaches).
#[must_use]
pub fn system_new_total(
    original: &SystemLibrary,
    overrides: &BTreeMap<u32, Option<ChunkItem>>,
) -> u32 {
    let mut total = original.total;
    for (slot, override_item) in overrides {
        match (original.items.get(slot), override_item) {
            (Some(old), Some(new)) => {
                total = total.saturating_sub(old.unigram);
                total = total.saturating_add(new.unigram);
            }
            (Some(old), None) => {
                total = total.saturating_sub(old.unigram);
            }
            (None, Some(new)) => {
                total = total.saturating_add(new.unigram);
            }
            (None, None) => {}
        }
    }
    total
}

/// Writes the whole profile: `_write_files` (every file to its `.tmp`
/// sibling) then `_rename_files` (all renames), so a crash between the
/// passes leaves the previous profile intact.
///
/// # Errors
///
/// Returns [`PersistenceError`] on the first write or rename failure;
/// the `.tmp` files of a failed save are removed, leaving the previous
/// profile untouched.
pub fn save(
    dir: &Path,
    state: &UserState,
    originals: &BTreeMap<u8, SystemLibrary>,
    versions: &SystemVersions,
    open_counter: u32,
) -> Result<(), PersistenceError> {
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| -> Result<(), PersistenceError> {
        // ---- the user bigram hash --------------------------------------
        let bigram_rows: Vec<(Vec<u8>, Vec<u8>)> = state
            .bigram
            .iter()
            .map(|(prev, gram)| {
                let records: Vec<(u32, u32)> = gram
                    .items
                    .iter()
                    .map(|(&next, &count)| (next, count))
                    .collect();
                (
                    prev.to_le_bytes().to_vec(),
                    encode_single_gram(gram.total, &records),
                )
            })
            .collect();
        stage_dbm(dir, UserDbm::Bigram, &bigram_rows, &mut staged)?;

        // ---- the two index trees, rebuilt from the USER_FILE items ------
        let mut chewing_rows: Vec<ParsedRow> = Vec::new();
        let mut phrase_rows: Vec<(Vec<u32>, u32)> = Vec::new();
        for (&nibble, items) in &state.libraries {
            for (&slot, item) in items {
                let token = token_of(nibble, slot);
                phrase_rows.push((item.phrase.clone(), token));
                for (packed, _freq) in &item.prons {
                    chewing_rows.push(ParsedRow {
                        token,
                        keys: packed
                            .iter()
                            .map(|&bits| ChewingKey::from_packed(bits))
                            .collect(),
                    });
                }
            }
        }
        stage_dbm(
            dir,
            UserDbm::PinyinIndex,
            &pinyin_index_entries(&chewing_rows),
            &mut staged,
        )?;
        stage_dbm(
            dir,
            UserDbm::PhraseIndex,
            &phrase_index_entries(&phrase_rows),
            &mut staged,
        )?;

        // ---- the USER_FILE chunk stores ---------------------------------
        for &(nibble, name) in USER_LIBRARY_FILES {
            let pairs: Vec<(u32, ChunkItem)> =
                state.libraries.get(&nibble).map_or(Vec::new(), |slots| {
                    slots
                        .iter()
                        .map(|(&slot, item)| (slot, item.clone()))
                        .collect()
                });
            let bytes = build_chunk(&pairs)?;
            stage_chunk(dir, name, &bytes, &mut staged)?;
        }

        // ---- the SYSTEM_FILE diff logs -----------------------------------
        let empty_library = SystemLibrary::default();
        for &(nibble, name) in SYSTEM_LOG_FILES {
            let original = originals.get(&nibble).unwrap_or(&empty_library);
            let overrides = state
                .system_overrides
                .get(&nibble)
                .cloned()
                .unwrap_or_default();
            let records = diff_records(nibble, original, &overrides);
            let bytes = build_memory_chunk(&encode_log_records(&records));
            stage_chunk(dir, name, &bytes, &mut staged)?;
        }

        // ---- user.conf ----------------------------------------------------
        let mut marker = UserTableInfo::conform_to(versions);
        marker.open_counter = open_counter;
        let tmp = dir.join(format!("{USER_CONF}.tmp"));
        std::fs::write(&tmp, marker.to_text())?;
        staged.push((tmp, dir.join(USER_CONF)));
        Ok(())
    })();

    match result {
        Ok(()) => {
            for (tmp, final_path) in &staged {
                std::fs::rename(tmp, final_path)?;
            }
            Ok(())
        }
        Err(error) => {
            for (tmp, _) in &staged {
                let _ = std::fs::remove_file(tmp);
            }
            Err(error)
        }
    }
}

/// The minimal log a save leaves for one system library — the header
/// record plus a record per changed slot, in ascending token walk
/// (`SubPhraseIndex::diff`'s order).
#[must_use]
pub fn diff_records(
    nibble: u8,
    original: &SystemLibrary,
    overrides: &BTreeMap<u32, Option<ChunkItem>>,
) -> Vec<LogRecord> {
    let mut records = vec![LogRecord::ModifyHeader {
        old_total: original.total,
        new_total: system_new_total(original, overrides),
    }];

    let mut slots: Vec<u32> = original
        .items
        .keys()
        .copied()
        .chain(overrides.keys().copied())
        .collect::<Vec<_>>();
    slots.sort_unstable();
    slots.dedup();

    for slot in slots {
        let current = match overrides.get(&slot) {
            Some(item) => item.as_ref(),
            None => original.items.get(&slot),
        };
        let token = token_of(nibble, slot);
        match (original.items.get(&slot), current) {
            (Some(old), Some(new)) => {
                if old != new {
                    records.push(LogRecord::Modify {
                        token,
                        old_item: encode_phrase_item(old),
                        new_item: encode_phrase_item(new),
                    });
                }
            }
            (Some(old), None) => {
                records.push(LogRecord::Remove {
                    token,
                    old_item: encode_phrase_item(old),
                });
            }
            (None, Some(new)) => {
                records.push(LogRecord::Add {
                    token,
                    new_item: encode_phrase_item(new),
                });
            }
            (None, None) => {}
        }
    }
    records
}

/// Writes one DBM file's rows to its `.tmp` sibling (hash for the
/// bigram, tree for the indexes) and registers the rename.
fn stage_dbm(
    dir: &Path,
    dbm: UserDbm,
    rows: &[(Vec<u8>, Vec<u8>)],
    staged: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), PersistenceError> {
    let final_path = dir.join(dbm.file_name());
    let tmp = dir.join(format!("{}.tmp", dbm.file_name()));
    let store = if dbm.is_hash() {
        DefaultStore::create_hash(&tmp)?
    } else {
        DefaultStore::create(&tmp)?
    };
    store.write(|txn| {
        for (key, value) in rows {
            txn.put_raw(key, value)?;
        }
        Ok(())
    })?;
    drop(store);
    staged.push((tmp, final_path));
    Ok(())
}

/// Writes one chunk file to its `.tmp` sibling and registers the rename.
fn stage_chunk(
    dir: &Path,
    name: &str,
    bytes: &[u8],
    staged: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), PersistenceError> {
    let final_path = dir.join(name);
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, bytes)?;
    staged.push((tmp, final_path));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(phrase: &[u32], unigram: u32, prons: &[(Vec<u16>, u32)]) -> ChunkItem {
        ChunkItem {
            phrase: phrase.to_vec(),
            unigram,
            prons: prons.to_vec(),
        }
    }

    fn originals() -> BTreeMap<u8, SystemLibrary> {
        let mut merged = BTreeMap::new();
        merged.insert(
            1,
            item(
                &[0x4f60],
                100,
                &[(vec![ChewingKey::new(23, 0, 1, 0).to_packed()], 100)],
            ),
        );
        merged.insert(
            2,
            item(
                &[0x597d],
                50,
                &[(vec![ChewingKey::new(7, 0, 13, 0).to_packed()], 50)],
            ),
        );
        BTreeMap::from([(
            1_u8,
            SystemLibrary {
                total: 150,
                items: merged,
            },
        )])
    }

    fn state() -> UserState {
        let mut bigram = BTreeMap::new();
        let mut gram = Gram {
            total: 207,
            items: BTreeMap::from([(1, 69), (0x0100_0001, 138)]),
        };
        gram.items.insert(0x0700_0001, 69);
        bigram.insert(1, gram);

        let user_item = item(
            &[0x4f60, 0x597d],
            15,
            &[(
                vec![
                    ChewingKey::new(23, 0, 1, 0).to_packed(),
                    ChewingKey::new(7, 0, 13, 0).to_packed(),
                ],
                15,
            )],
        );

        UserState {
            bigram,
            libraries: BTreeMap::from([(7_u8, BTreeMap::from([(1_u32, user_item)]))]),
            // 你 trained twice: 100 → 100 + 69*7
            system_overrides: BTreeMap::from([(
                1_u8,
                BTreeMap::from([(
                    1_u32,
                    Some(item(
                        &[0x4f60],
                        100 + 69 * 7,
                        &[(vec![ChewingKey::new(23, 0, 1, 0).to_packed()], 100 + 69)],
                    )),
                )]),
            )]),
        }
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-user-persist-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tmpdir");
        dir
    }

    fn versions() -> SystemVersions {
        SystemVersions::for_this_build(7, 14)
    }

    #[test]
    fn save_then_load_round_trips_the_state() {
        let dir = tempdir("round-trip");
        let originals = originals();
        let state = state();

        save(&dir, &state, &originals, &versions(), 1).expect("save");
        // No `.tmp` siblings survive the rename pass.
        for entry in std::fs::read_dir(&dir).expect("readdir") {
            let name = entry.expect("entry").file_name();
            assert!(
                !name.to_string_lossy().ends_with(".tmp"),
                "stray tmp: {name:?}"
            );
        }

        let loaded = load(&dir, &originals, &versions()).expect("load");
        assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
        assert!(!loaded.wiped);
        assert_eq!(loaded.open_counter, 2); // the ratchet
        assert_eq!(loaded.state, state);

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn fresh_dir_saves_and_loads_empty_everywhere() {
        let dir = tempdir("fresh");
        let originals = originals();

        // A fresh load: no user.conf → non-conform → empty, counter 1.
        let loaded = load(&dir, &originals, &versions()).expect("load");
        assert!(loaded.wiped);
        assert_eq!(loaded.open_counter, 1);
        assert_eq!(loaded.state, UserState::default());

        // Saving the empty state writes the whole inventory, and it
        // loads back empty.
        save(&dir, &UserState::default(), &originals, &versions(), 1).expect("save");
        let reloaded = load(&dir, &originals, &versions()).expect("load");
        assert!(!reloaded.wiped);
        assert!(reloaded.skipped.is_empty(), "{:?}", reloaded.skipped);
        assert_eq!(reloaded.state, UserState::default());
        // The header-only diff log: the frame decodes to one header
        // record with old == new totals.
        let log = std::fs::read(dir.join("merged.dbin")).expect("absent lib log");
        let records =
            decode_log_records(read_chunk_payload(&log).expect("frame")).expect("records");
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0],
            LogRecord::ModifyHeader {
                old_total: 0,
                new_total: 0
            }
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn non_conform_profile_wipes_and_resets() {
        let dir = tempdir("wipe");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 3).expect("save");
        assert!(dir.join("user.bin").exists());

        // A cross-backend marker never conforms.
        let foreign = UserTableInfo {
            binary_format_version: 7,
            model_data_version: 14,
            database_format: Some("BerkeleyDB".to_owned()),
            open_counter: 0,
        };
        std::fs::write(dir.join(USER_CONF), foreign.to_text()).expect("write");

        let loaded = load(&dir, &originals, &versions()).expect("load");
        assert!(loaded.wiped);
        assert_eq!(loaded.state, UserState::default());
        assert_eq!(loaded.open_counter, 1); // get_open_counter caps, +1
        assert!(
            !dir.join("user.bin").exists(),
            "profile file survived the wipe"
        );
        assert!(!dir.join("gb_char.dbin").exists());
        // The marker is rewritten conform.
        let marker =
            UserTableInfo::parse(&std::fs::read_to_string(dir.join(USER_CONF)).expect("read"))
                .expect("parse");
        assert!(marker.is_conform(&versions()));

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_tired_counter_wipes_on_the_next_open() {
        let dir = tempdir("tired");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), OPEN_COUNTER_LIMIT).expect("save");
        // The limit itself still conforms; one more open crosses it.
        let loaded = load(&dir, &originals, &versions()).expect("load");
        assert!(!loaded.wiped);
        assert_eq!(loaded.open_counter, OPEN_COUNTER_LIMIT + 1);
        let next = load(&dir, &originals, &versions()).expect("load");
        assert!(next.wiped);
        assert_eq!(next.state, UserState::default());

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_diff_log_is_stable_across_save_cycles() {
        let dir = tempdir("stable");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 1).expect("save");
        let first = std::fs::read(dir.join("gb_char.dbin")).expect("log");

        // Load back and save again: the loaded state reproduces the same
        // bytes — the replay recomputes the same overrides the diff
        // emitted.
        let loaded = load(&dir, &originals, &versions()).expect("load");
        save(&dir, &loaded.state, &originals, &versions(), 2).expect("save 2");
        let second = std::fs::read(dir.join("gb_char.dbin")).expect("log 2");
        assert_eq!(first, second);

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn replay_stops_at_a_mismatched_record() {
        let dir = tempdir("mismatch");
        let originals = originals();

        // A hand-built log: a MODIFY whose old payload is not the
        // original item. The replay stops at it, keeping the prefix.
        let records = vec![
            LogRecord::ModifyHeader {
                old_total: 150,
                new_total: 150 + 69,
            },
            LogRecord::Modify {
                token: 0x0100_0002,
                old_item: encode_phrase_item(&item(&[0x597d], 999, &[])), // wrong old
                new_item: encode_phrase_item(&item(&[0x597d], 1, &[])),
            },
            LogRecord::Modify {
                token: 0x0100_0001,
                old_item: encode_phrase_item(&item(&[0x4f60], 100, &[])),
                new_item: encode_phrase_item(&item(&[0x4f60], 200, &[])),
            },
        ];
        let framed = build_memory_chunk(&encode_log_records(&records));
        std::fs::write(dir.join("gb_char.dbin"), framed).expect("write");
        let marker = UserTableInfo::conform_to(&versions());
        std::fs::write(dir.join(USER_CONF), marker.to_text()).expect("write");

        let loaded = load(&dir, &originals, &versions()).expect("load");
        assert!(!loaded.wiped);
        // Slot 1 was never reached: the mismatch stopped the replay
        // before the second MODIFY (an absent map and an empty one are
        // the same state).
        assert!(
            loaded
                .state
                .system_overrides
                .get(&1)
                .is_none_or(|slots| slots.is_empty())
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_bigram_hash_round_trips_through_the_container() {
        let dir = tempdir("bigram");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 1).expect("save");

        // Point-read one gram through the backend, the way the runtime
        // reads the system bigram.
        let path = dir.join(UserDbm::Bigram.file_name());
        let store = DefaultStore::open_hash_read_only(&path).expect("open");
        let value = store.get_raw(&1_u32.to_le_bytes()).expect("get");
        let (total, records) = decode_single_gram(&value.expect("gram")).expect("decode");
        assert_eq!(total, 207);
        assert_eq!(
            records,
            vec![(1, 69), (0x0100_0001, 138), (0x0700_0001, 69)]
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn hostile_profile_files_degrade_per_file() {
        let dir = tempdir("hostile");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 1).expect("save");

        // A corrupted user.bin checksum and a garbage bigram container.
        let mut chunk = std::fs::read(dir.join("user.bin")).expect("read");
        let last = chunk.len() - 1;
        chunk[last] ^= 0xFF;
        std::fs::write(dir.join("user.bin"), chunk).expect("write");
        std::fs::write(dir.join(UserDbm::Bigram.file_name()), b"not a dbm").expect("write");

        let loaded = load(&dir, &originals, &versions()).expect("load");
        assert!(!loaded.wiped, "user.conf is intact; only files skip");
        assert_eq!(loaded.skipped.len(), 2, "{:?}", loaded.skipped);
        assert!(loaded.state.bigram.is_empty());
        assert!(!loaded.state.libraries.contains_key(&7));
        // The untouched halves still load.
        assert!(loaded.state.system_overrides.contains_key(&1));

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
