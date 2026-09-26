//! The user directory in libpinyin's own file shapes — drop-in task 9's
//! persistence half.
//!
//! The store's durable representation is the pin's file set
//! (`docs/findings/user-store.md` §4 for the save cycle's semantics,
//! `oxpinyin_data::user_files` for the inventory and codecs):
//!
//! * load — `check_format` (user.conf conformance, the open counter,
//!   `_clean_user_files` on a non-conform profile), then the bigram
//!   hash, the `USER_FILE` chunk stores (`user.bin`, `addon.bin`,
//!   `network.bin`), and the `SYSTEM_FILE` `.dbin` logs replayed onto the
//!   original system chunks. The two index DBMs (`user_pinyin_index.bin`,
//!   `user_phrase_index.bin`) are pure derivatives of the `USER_FILE`
//!   items — every index row was added by the same `add_index` walk that
//!   inserted the item — so they are rebuilt at save and not read.
//! * save — the pin's `_write_files` + `_rename_files`: every file is
//!   written whole to a `.tmp` sibling, then all are renamed over their
//!   finals, so a crash mid-save leaves the previous profile intact.
//! * fini — `pinyin_fini`'s `user.conf` write: the open counter the load
//!   raised, lowered again.
//!
//! The two facades share the files and the `user.conf` codec but not its
//! lifecycle: [`UserConfLaw`] names which one a profile follows.
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
use oxpinyin_data::phrase_libraries::PhraseLibraries;
use oxpinyin_data::row_format::pinyin_index::PinyinIndexItem;
use oxpinyin_data::single_gram::{decode_single_gram, encode_single_gram};
use oxpinyin_data::table_entries::{phrase_index_entries, pinyin_index_entries};
use oxpinyin_data::user_files::{
    LogRecord, SYSTEM_LOG_FILES, SystemVersions, USER_LIBRARY_FILES, UserDbm, UserTableInfo,
    decode_log_records, encode_log_records, get_open_counter, read_chunk_payload,
};
use oxpinyin_store::{DefaultStore, RawReadStore, StoreError, WriteStore};

/// `USER_TABLE_INFO` (`pinyin_internal.h:56`).
const USER_CONF: &str = "user.conf";

/// Which facade's `user.conf` lifecycle a profile follows. libpinyin and
/// libzhuyin read and write the same marker through the same
/// `UserTableInfo`, but at different points and with different counters,
/// so the facade that opens a profile names its law and every write below
/// follows it ([`load`], [`save`]'s counter, [`fini`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UserConfLaw {
    /// libpinyin: every `pinyin_init` raises the open counter and writes
    /// the marker (`check_format`, `pinyin.cpp:185-187`); `pinyin_save`
    /// writes the context's counter back (`mark_version`, `:1143`,
    /// `:220-232`); `pinyin_fini` lowers it and writes again, with or
    /// without a save before it (`:1194-1200`). A process that dies
    /// between init and fini leaves the counter raised, and the init that
    /// reads it past `OPEN_COUNTER_LIMIT` wipes the profile.
    Pinyin,
    /// libzhuyin: `zhuyin_init` only reads the marker (`check_format`,
    /// `zhuyin.cpp:126-162`: conformance and the wipe, no counter step and
    /// no write); `zhuyin_save` writes a fresh `UserTableInfo`, so its
    /// counter is 0 (`mark_version`, `:164-176`, called at `:695`);
    /// `zhuyin_fini` writes nothing (`:741-757`).
    Zhuyin,
}

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

/// Builds the `SYSTEM_FILE` libraries' originals from the runtime's
/// opened chunks — the `.dbin` diff base, and the conformance source
/// for replaying a profile's logs.
///
/// Items whose UCS-4 text does not
/// decode are skipped (a malformed entry, never a panic).
#[must_use]
pub fn system_originals(libraries: &PhraseLibraries) -> BTreeMap<u8, SystemLibrary> {
    let mut out = BTreeMap::new();
    for &(nibble, _name) in SYSTEM_LOG_FILES {
        let Some(library) = libraries.library((u32::from(nibble) << 24) | 1) else {
            continue;
        };
        let mut items = BTreeMap::new();
        for (token, view) in library.items() {
            let Some(text) = view.phrase_text() else {
                continue;
            };
            let prons: Vec<(Vec<u16>, u32)> = view
                .pronunciations()
                .map(|pron| {
                    let packed = pron
                        .keys
                        .chunks_exact(2)
                        .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                        .collect();
                    (packed, pron.freq)
                })
                .collect();
            items.insert(
                token & PHRASE_MASK,
                ChunkItem {
                    phrase: text.chars().map(u32::from).collect(),
                    unigram: view.unigram(),
                    prons,
                },
            );
        }
        out.insert(
            nibble,
            SystemLibrary {
                total: library.total_freq(),
                items,
            },
        );
    }
    out
}

/// The user state, in the value shapes the file set carries.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UserState {
    /// The user bigram by previous token.
    pub bigram: BTreeMap<u32, Gram>,
    /// The `USER_FILE` sub-indexes (5, 6, 7) by nibble, items by slot.
    pub libraries: BTreeMap<u8, BTreeMap<u32, ChunkItem>>,
    /// The `SYSTEM_FILE` libraries' touched items by nibble and slot:
    /// `Some(item)` modifies the original, `None` removes it. A slot
    /// absent here still answers its original item.
    pub system_overrides: BTreeMap<u8, BTreeMap<u32, Option<ChunkItem>>>,
}

impl UserState {}

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

impl From<oxpinyin_data::chunk_format::ChunkFrameError> for PersistenceError {
    fn from(error: oxpinyin_data::chunk_format::ChunkFrameError) -> Self {
        PersistenceError::Codec(error.to_string())
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
    /// The open counter every later `user.conf` write of this session
    /// starts from: under [`UserConfLaw::Pinyin`] the value the load
    /// raised and wrote, which [`save`] writes back and [`fini`] lowers;
    /// under [`UserConfLaw::Zhuyin`] 0, the counter of the fresh
    /// `UserTableInfo` libzhuyin's `mark_version` saves.
    pub open_counter: u32,
    /// A non-conform profile was found and its files removed —
    /// `check_format`'s `_clean_user_files`, the ecosystem's own
    /// mechanism for "backend or model change discards user data".
    /// Read by the tests here; the runtime wiring logs it.
    #[cfg_attr(not(test), allow(dead_code))]
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
/// This reproduces the facade's init-time user-dir half under `law`.
/// libpinyin raises the open counter and writes `user.conf` at every
/// init, conform or not (`pinyin.cpp:185-187`); libzhuyin reads the
/// marker and writes nothing (`zhuyin.cpp:126-162`). Both wipe a
/// non-conform profile — a counter past the limit included — and keep
/// the marker file itself, which neither pin's wipe removes.
///
/// # Errors
///
/// Returns [`PersistenceError`] only when the `user.conf` write itself
/// fails; unparsable profile files degrade per-file (see
/// [`Loaded::skipped`]), as upstream's do.
pub fn load(
    dir: &Path,
    originals: &BTreeMap<u8, SystemLibrary>,
    versions: &SystemVersions,
    law: UserConfLaw,
) -> Result<Loaded, PersistenceError> {
    let conf_path = dir.join(USER_CONF);
    let existing = std::fs::read_to_string(&conf_path)
        .ok()
        .and_then(|text| UserTableInfo::parse(&text).ok());

    let conform = existing
        .as_ref()
        .is_some_and(|info| info.is_conform(versions));

    // libpinyin's raise, exactly `check_format`'s arithmetic
    // (`pinyin.cpp:185-186`): `get_open_counter() + 1`, where a missing
    // or unparsable marker reads 0 (`UserTableInfo::load` resets before
    // it parses, `table_info.cpp:325-326`). A conform profile raised to 7
    // fails `is_conform` at the next init, which wipes and writes 1.
    // libzhuyin keeps no counter of its own: its writes carry the fresh
    // `UserTableInfo`'s 0.
    let open_counter = match law {
        UserConfLaw::Pinyin => {
            get_open_counter(existing.as_ref().map_or(0, |info| info.open_counter)) + 1
        }
        UserConfLaw::Zhuyin => 0,
    };

    let mut loaded = Loaded {
        open_counter,
        wiped: !conform,
        ..Loaded::default()
    };

    if !conform {
        clean_user_files(dir);
        if law == UserConfLaw::Pinyin {
            write_marker(dir, versions, open_counter)?;
        }
        return Ok(loaded);
    }

    load_bigram(dir, &mut loaded);
    load_libraries(dir, &mut loaded);
    load_logs(dir, originals, &mut loaded);

    if law == UserConfLaw::Pinyin {
        write_marker(dir, versions, open_counter)?;
    }
    Ok(loaded)
}

/// `pinyin_fini`'s arithmetic (`pinyin.cpp:1196-1197`): the counter the
/// init raised, read through `get_open_counter`, lowered by one and
/// floored at 0 — `counter > 1 ? counter - 1 : 0`, which over an unsigned
/// counter is exactly a saturating decrement. A raised 7 therefore reads
/// 0 and lowers to 0, not to 6.
#[must_use]
pub const fn lowered_open_counter(open_counter: u32) -> u32 {
    get_open_counter(open_counter).saturating_sub(1)
}

/// The facade's fini-time `user.conf` write under `law`: libpinyin
/// lowers the counter the load raised and writes the conform marker
/// (`pinyin_fini`, `pinyin.cpp:1195-1200`, whose `mark_version` runs
/// whether or not a save came first); libzhuyin writes nothing
/// (`zhuyin_fini`, `zhuyin.cpp:741-757`). Nothing else of the profile
/// is touched.
///
/// A process that never reaches its fini leaves the load's raised value
/// on disk, as upstream's does.
///
/// # Errors
///
/// Returns [`PersistenceError`] when the marker cannot be written.
pub fn fini(
    dir: &Path,
    versions: &SystemVersions,
    law: UserConfLaw,
    open_counter: u32,
) -> Result<(), PersistenceError> {
    match law {
        UserConfLaw::Pinyin => write_marker(dir, versions, lowered_open_counter(open_counter)),
        UserConfLaw::Zhuyin => Ok(()),
    }
}

/// `mark_version`'s write: the conform marker (`make_conform`) with
/// `open_counter`, straight over `user.conf` as `UserTableInfo::save`'s
/// `fopen("w")` does (`table_info.cpp:377-397`).
fn write_marker(
    dir: &Path,
    versions: &SystemVersions,
    open_counter: u32,
) -> Result<(), PersistenceError> {
    let mut marker = UserTableInfo::conform_to(versions);
    marker.open_counter = open_counter;
    std::fs::write(dir.join(USER_CONF), marker.to_text())?;
    Ok(())
}

/// `_clean_user_files` + the fixed names: every data file of the profile
/// is removed; absence is not an error. `user.conf` is not among them —
/// neither pin's `check_format` unlinks it (`pinyin.cpp:194-215`,
/// `zhuyin.cpp:141-159`): libpinyin has already rewritten it by then, and
/// libzhuyin leaves the non-conform marker in place until a save.
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
    for name in names {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// The user bigram: the container's rows are the grams wholesale. The
/// container is the backend's *user*-bigram form — a Kyoto Cabinet
/// snapshot stream, a tkrzw hash file
/// (`RawReadStore::open_user_bigram`); the system `bigram.db` is a
/// different container on Kyoto Cabinet and must not share an open path.
fn load_bigram(dir: &Path, loaded: &mut Loaded) {
    let path = dir.join(UserDbm::Bigram.file_name());
    if !path.exists() {
        return;
    }
    // A backend that locks can leave a lock sidecar beside the *final*
    // file even on a read-only open; it is stale once the handle drops,
    // and the user dir must hold exactly the pin's names. Removed at the
    // end of this function, after the walk.
    let store = match DefaultStore::open_user_bigram(&path) {
        Ok(store) => store,
        Err(error) => {
            loaded
                .skipped
                .push(format!("{}: {error}", UserDbm::Bigram.file_name()));
            // The failed open can have created a lock sidecar (a
            // locking backend writes one even when it then rejects the
            // file): the inventory rule applies on this path too.
            remove_dbm_sidecars(dir, &path);
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
    drop(store);
    remove_dbm_sidecars(dir, &path);
}

/// The `USER_FILE` chunk stores.
fn load_libraries(dir: &Path, loaded: &mut Loaded) {
    for &(nibble, name) in USER_LIBRARY_FILES {
        let path = dir.join(name);
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            // Absent is the normal fresh-profile case. Any other error
            // is an unreadable *existing* file: loading as if it were
            // absent would let the next save rebuild it from empty and
            // destroy the user's data with no record, so it lands in
            // `skipped` for the caller to see.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                loaded.skipped.push(format!("{name}: {e}"));
                continue;
            }
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
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                loaded.skipped.push(format!("{name}: {e}"));
                continue;
            }
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
                overrides.get(&slot).cloned().unwrap_or_else(|| {
                    original
                        .and_then(|library| library.items.get(&slot))
                        .cloned()
                })
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
                        _ => {
                            // payload mismatch: merge's failure return,
                            // recorded so a clean load is distinguishable
                            // from a truncated replay.
                            loaded.skipped.push(format!(
                                "{name}: replay stopped at a mismatched REMOVE for {token:#010x}"
                            ));
                            break;
                        }
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
                        _ => {
                            // payload mismatch: merge's failure return,
                            // recorded (see the REMOVE note above).
                            loaded.skipped.push(format!(
                                "{name}: replay stopped at a mismatched MODIFY for {token:#010x}"
                            ));
                            break;
                        }
                    }
                }
                LogRecord::ModifyHeader {
                    old_total,
                    new_total: _,
                } => {
                    // The header must land on the current total; a
                    // mismatch is merge's failure return.
                    let fallback = SystemLibrary::default();
                    let now = system_new_total(original.unwrap_or(&fallback), &overrides);
                    if now != old_total {
                        loaded
                            .skipped
                            .push(format!("{name}: replay stopped at a mismatched header"));
                        break;
                    }
                }
                _ => {
                    // a record of another library's token: corrupt stream
                    loaded.skipped.push(format!(
                        "{name}: replay stopped at a foreign-library record"
                    ));
                    break;
                }
            }
        }
        // An empty override set and an absent one are the same state.
        if !overrides.is_empty() {
            loaded.state.system_overrides.insert(nibble, overrides);
        }
    }
}

/// The library's new `total_freq` — the original plus the overrides'
/// deltas.
///
/// `m_total_freq`'s incremental arithmetic recomputed deterministically
/// (saturating; upstream drops a delta that would overflow instead, an
/// edge no training run reaches).
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
/// `user.conf` is `mark_version`'s marker with `open_counter` — the
/// session's [`Loaded::open_counter`]: libpinyin's save writes back the
/// value its init raised (`pinyin.cpp:1143`), libzhuyin's a fresh 0
/// (`zhuyin.cpp:695`). Neither save moves the counter. It is written
/// after the renames, in place, not staged.
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
        stage_user_bigram(dir, &bigram_rows)?;

        // ---- the two index trees, rebuilt from the USER_FILE items ------
        let mut chewing_rows: Vec<PinyinIndexItem> = Vec::new();
        let mut phrase_rows: Vec<(Vec<u32>, u32)> = Vec::new();
        for (&nibble, items) in &state.libraries {
            for (&slot, item) in items {
                let token = token_of(nibble, slot);
                phrase_rows.push((item.phrase.clone(), token));
                for (packed, _freq) in &item.prons {
                    chewing_rows.push(PinyinIndexItem {
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
            let records = diff_records(nibble, original, &overrides)?;
            let payload =
                encode_log_records(&records).map_err(|e| PersistenceError::Codec(e.to_string()))?;
            let bytes = build_memory_chunk(&payload).map_err(PersistenceError::from)?;
            stage_chunk(dir, name, &bytes, &mut staged)?;
        }

        Ok(())
    })();

    match result {
        Ok(()) => {
            for (index, (tmp, final_path)) in staged.iter().enumerate() {
                if let Err(error) = std::fs::rename(tmp, final_path) {
                    // The rename pass is half-done: this `.tmp` and every
                    // one still staged behind it are orphans the profile
                    // does not own. Remove them so the user dir keeps
                    // exactly the pin's inventory — the already-renamed
                    // files are complete files and stay.
                    for (tmp, _) in &staged[index..] {
                        let _ = std::fs::remove_file(tmp);
                    }
                    return Err(error.into());
                }
            }
            // `user.conf` last, and in place: `mark_version` runs after
            // the renames and `fopen`s the marker over the existing file
            // (`pinyin.cpp:1141-1143`, `zhuyin.cpp:695`), so the file
            // keeps whatever mode it already had.
            write_marker(dir, versions, open_counter)
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
///
/// # Errors
///
/// Returns the store's error when a diff walk or record append fails.
pub fn diff_records(
    nibble: u8,
    original: &SystemLibrary,
    overrides: &BTreeMap<u32, Option<ChunkItem>>,
) -> Result<Vec<LogRecord>, PersistenceError> {
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
        // An override entry always wins — including `Some(None)`, an
        // explicit removal: flattening it into the original item here
        // would drop the Remove record and resurrect the phrase on
        // reopen.
        let current = overrides
            .get(&slot)
            .map(|item| item.as_ref())
            .unwrap_or_else(|| original.items.get(&slot));
        let token = token_of(nibble, slot);
        match (original.items.get(&slot), current) {
            (Some(old), Some(new)) => {
                if old != new {
                    records.push(LogRecord::Modify {
                        token,
                        old_item: encode_phrase_item(old).map_err(PersistenceError::from)?,
                        new_item: encode_phrase_item(new).map_err(PersistenceError::from)?,
                    });
                }
            }
            (Some(old), None) => {
                records.push(LogRecord::Remove {
                    token,
                    old_item: encode_phrase_item(old).map_err(PersistenceError::from)?,
                });
            }
            (None, Some(new)) => {
                records.push(LogRecord::Add {
                    token,
                    new_item: encode_phrase_item(new).map_err(PersistenceError::from)?,
                });
            }
            (None, None) => {}
        }
    }
    Ok(records)
}

/// Writes the user bigram to its `.tmp` sibling and registers the rename.
///
/// Routed through [`WriteStore::write_user_bigram`] rather than
/// [`stage_dbm`]'s container create, because the user bigram is not a
/// hash file on every backend: Kyoto Cabinet writes a snapshot stream of
/// an in-memory stash (`ngram_kyotodb.cpp:82-101`) where tkrzw
/// write a container at the path.
fn stage_user_bigram(dir: &Path, rows: &[(Vec<u8>, Vec<u8>)]) -> Result<(), PersistenceError> {
    // The seam stages its own sibling temporary and renames atomically
    // (S2's review fix), so this is a direct call on the final path —
    // no outer `.tmp`, and any lock sidecar of the *previous* file is
    // the seam's to handle.
    DefaultStore::write_user_bigram(&dir.join(UserDbm::Bigram.file_name()), rows)?;
    Ok(())
}

/// Writes one of the two index DBMs' rows to its `.tmp` sibling (a tree
/// container on every backend) and registers the rename. The user bigram
/// takes the separate [`stage_user_bigram`] route, since its container is
/// backend-specific.
fn stage_dbm(
    dir: &Path,
    dbm: UserDbm,
    rows: &[(Vec<u8>, Vec<u8>)],
    staged: &mut Vec<(PathBuf, PathBuf)>,
) -> Result<(), PersistenceError> {
    debug_assert!(!dbm.is_hash(), "stage_dbm serves the two index trees");
    let final_path = dir.join(dbm.file_name());
    let tmp = dir.join(format!("{}.tmp", dbm.file_name()));
    // A crashed save can leave this `.tmp` behind with rows the coming
    // write would not overwrite — `create` opens-or-creates, so stale
    // index rows would survive into the renamed file.
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    remove_dbm_sidecars(dir, &tmp);
    // A user table, created with the mode the backend's own `save_db`
    // uses for one (Berkeley DB's 0600, not `attach`'s 0644).
    let store = DefaultStore::create_user(&tmp)?;
    store.write(|txn| {
        for (key, value) in rows {
            txn.put_raw(key, value)?;
        }
        Ok(())
    })?;
    drop(store);
    // Some backends keep a lock sidecar beside the database (tkrzw writes
    // `<path>-lock`). It is stale once the handle drops, and the rename
    // below moves only the data file — without this the user dir would
    // accumulate one orphan per save, which no libpinyin install leaves.
    remove_dbm_sidecars(dir, &tmp);
    staged.push((tmp, final_path));
    Ok(())
}

/// The sidecar suffixes a DBM backend may keep beside a data file.
/// An explicit allowlist, not a prefix match: a prefix match would
/// delete any future or unrelated profile file that happens to share
/// the data file's stem.
const DBM_SIDECAR_SUFFIXES: [&str; 2] = ["-lock", "-shm"];

/// Removes the sidecar files a DBM backend created beside `dbm_path`.
/// Absence is not an error: most backends create no sidecar at all.
fn remove_dbm_sidecars(dir: &Path, dbm_path: &Path) {
    let Some(file_name) = dbm_path.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if DBM_SIDECAR_SUFFIXES
            .iter()
            .any(|suffix| name == format!("{file_name}{suffix}"))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
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
    write_chunk_file(&tmp, bytes)?;
    staged.push((tmp, final_path));
    Ok(())
}

/// `MemoryChunk::save` (`memory_chunk.h:536-537`): `open(O_CREAT |
/// O_WRONLY | O_TRUNC, 0644)` over `path`, then the bytes. The process
/// umask applies to 0644, and a file that already exists keeps its own
/// mode, as upstream's does — `std::fs::write` would ask for 0666.
fn write_chunk_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o644);
    std::io::Write::write_all(&mut options.open(path)?, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxpinyin_data::user_files::OPEN_COUNTER_LIMIT;
    use oxpinyin_store::ReadStore;

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
        // The user dir holds exactly the pin's eleven names — the three
        // DBMs under the backend's own names, the three USER_FILE chunks,
        // the four `.dbin` logs, and `user.conf` — and nothing else. This
        // is the assertion that catches an orphaned backend sidecar (a
        // locking backend writes a `-lock` file beside each DBM; the
        // rename pass moves
        // only the data file), which a literal `.tmp`-suffix check misses.
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .expect("readdir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let mut expected = vec![
            UserDbm::Bigram.file_name(),
            UserDbm::PinyinIndex.file_name(),
            UserDbm::PhraseIndex.file_name(),
            "user.conf".to_owned(),
        ];
        expected.extend(USER_LIBRARY_FILES.iter().map(|&(_, n)| n.to_owned()));
        expected.extend(SYSTEM_LOG_FILES.iter().map(|&(_, n)| n.to_owned()));
        expected.sort();
        assert_eq!(names, expected, "the save left an unexpected file");

        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
        assert!(!loaded.wiped);
        assert_eq!(loaded.open_counter, 2); // check_format's raise
        assert_eq!(loaded.state, state);

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn diff_records_emits_remove_for_an_explicit_removal() {
        // A `Some(None)` override is a removal, not "fall back to the
        // original item": collapsing the two lookups drops the Remove
        // record (review on #447).
        let originals = originals();
        let original = &originals[&1];
        let overrides: BTreeMap<u32, Option<ChunkItem>> = BTreeMap::from([(1_u32, None)]);

        let records = diff_records(1, original, &overrides).expect("diff");
        assert!(matches!(
            records.as_slice(),
            [
                LogRecord::ModifyHeader {
                    old_total: 150,
                    new_total: 50
                },
                LogRecord::Remove {
                    token: 0x0100_0001,
                    ..
                }
            ]
        ));
    }

    #[test]
    fn removed_system_phrase_stays_removed_across_save_and_reopen() {
        let dir = tempdir("removal-round-trip");
        let originals = originals();
        let mut state = state();
        state
            .system_overrides
            .get_mut(&1)
            .expect("library")
            .insert(2_u32, None);

        save(&dir, &state, &originals, &versions(), 1).expect("save");
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(loaded.skipped.is_empty(), "{:?}", loaded.skipped);
        assert_eq!(
            loaded.state.system_overrides[&1].get(&2),
            Some(&None),
            "the removal did not survive the save-then-reopen"
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn fresh_dir_saves_and_loads_empty_everywhere() {
        let dir = tempdir("fresh");
        let originals = originals();

        // A fresh load: no user.conf → non-conform → empty, counter 1.
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(loaded.wiped);
        assert_eq!(loaded.open_counter, 1);
        assert_eq!(loaded.state, UserState::default());

        // Saving the empty state writes the whole inventory, and it
        // loads back empty.
        save(&dir, &UserState::default(), &originals, &versions(), 1).expect("save");
        let reloaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
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

        // A cross-backend marker never conforms. `BerkeleyDB` was the
        // obvious foreign token while no backend carried it; the bdb
        // build IS BerkeleyDB, so its foreign marker is a different real
        // upstream token (any of the three that is not this build's
        // `DEFAULT_STORE_DB_FORMAT` would do).
        let foreign_token = match oxpinyin_store::DEFAULT_STORE_DB_FORMAT {
            "BerkeleyDB" => "KyotoCabinet",
            _ => "BerkeleyDB",
        };
        let foreign = UserTableInfo {
            binary_format_version: 7,
            model_data_version: 14,
            database_format: Some(foreign_token.to_owned()),
            open_counter: 0,
        };
        std::fs::write(dir.join(USER_CONF), foreign.to_text()).expect("write");

        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
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
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(!loaded.wiped);
        assert_eq!(loaded.open_counter, OPEN_COUNTER_LIMIT + 1);
        let next = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(next.wiped);
        assert_eq!(next.state, UserState::default());

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The counter `user.conf` holds, read back as the next init reads it.
    fn recorded_counter(dir: &Path) -> Option<u32> {
        let text = std::fs::read_to_string(dir.join(USER_CONF)).ok()?;
        Some(UserTableInfo::parse(&text).ok()?.open_counter)
    }

    #[test]
    fn the_lowering_is_pinyin_finis_arithmetic() {
        // `counter > 1 ? counter - 1 : 0` over `get_open_counter`
        // (pinyin.cpp:1196-1197): a raised 7 reads 0, so it lowers to 0.
        assert_eq!(lowered_open_counter(0), 0);
        assert_eq!(lowered_open_counter(1), 0);
        assert_eq!(lowered_open_counter(2), 1);
        assert_eq!(
            lowered_open_counter(OPEN_COUNTER_LIMIT),
            OPEN_COUNTER_LIMIT - 1
        );
        assert_eq!(lowered_open_counter(OPEN_COUNTER_LIMIT + 1), 0);
        assert_eq!(lowered_open_counter(u32::MAX), 0);
    }

    #[test]
    fn clean_pinyin_cycles_return_the_counter_and_keep_the_profile() {
        // #523: init raises, save writes the raised value back, fini
        // lowers — so a profile that is saved and finished every launch
        // never reaches the limit, however many launches it sees.
        let dir = tempdir("pinyin-cycles");
        let originals = originals();
        for launch in 1..=10 {
            let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
            assert_eq!(
                loaded.wiped,
                launch == 1,
                "launch {launch}: only the fresh dir wipes"
            );
            assert_eq!(loaded.open_counter, 1, "launch {launch}: raised from 0");
            assert_eq!(
                recorded_counter(&dir),
                Some(1),
                "launch {launch}: the init writes"
            );
            if launch > 1 {
                assert_eq!(
                    loaded.state,
                    state(),
                    "launch {launch}: the profile survived"
                );
            }
            save(&dir, &state(), &originals, &versions(), loaded.open_counter).expect("save");
            assert_eq!(
                recorded_counter(&dir),
                Some(1),
                "launch {launch}: the save writes back"
            );
            fini(&dir, &versions(), UserConfLaw::Pinyin, loaded.open_counter).expect("fini");
            assert_eq!(
                recorded_counter(&dir),
                Some(0),
                "launch {launch}: the fini lowers"
            );
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_pinyin_fini_without_a_save_still_lowers_the_counter() {
        // pinyin_fini's mark_version runs whether or not a save came
        // first (pinyin.cpp:1194-1200).
        let dir = tempdir("pinyin-fini-unsaved");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 0).expect("seed");
        let before = std::fs::read(dir.join("user.bin")).expect("user.bin");
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert_eq!(recorded_counter(&dir), Some(1));
        fini(&dir, &versions(), UserConfLaw::Pinyin, loaded.open_counter).expect("fini");
        assert_eq!(recorded_counter(&dir), Some(0));
        // user.conf is the only file a fini writes.
        assert_eq!(
            std::fs::read(dir.join("user.bin")).expect("user.bin"),
            before
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn pinyin_launches_that_die_before_fini_wipe_on_the_eighth() {
        // The crash path: every init raises and writes, nothing lowers.
        // From a counter of 0 the seventh killed launch leaves 7, and the
        // eighth init reads it past the limit, wipes and writes 1 — where
        // the pin wipes (table_info.cpp:409-410).
        let dir = tempdir("pinyin-crashes");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 0).expect("seed");
        for launch in 1..=7 {
            let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
            assert!(!loaded.wiped, "killed launch {launch} wiped early");
            assert_eq!(loaded.state, state());
            assert_eq!(recorded_counter(&dir), Some(launch));
        }
        let eighth = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(eighth.wiped);
        assert_eq!(eighth.state, UserState::default());
        assert_eq!(recorded_counter(&dir), Some(1));
        assert!(
            !dir.join("user.bin").exists(),
            "profile file survived the wipe"
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_raised_seven_that_finishes_restarts_from_zero() {
        // get_open_counter on both ends: an init that raises 6 to 7 and
        // then finishes writes 0 (a 7 reads as 0), not 6.
        let dir = tempdir("pinyin-seven");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), OPEN_COUNTER_LIMIT).expect("seed");
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(!loaded.wiped);
        assert_eq!(recorded_counter(&dir), Some(OPEN_COUNTER_LIMIT + 1));
        fini(&dir, &versions(), UserConfLaw::Pinyin, loaded.open_counter).expect("fini");
        assert_eq!(recorded_counter(&dir), Some(0));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn zhuyin_init_and_fini_never_write_the_marker() {
        // libzhuyin's check_format only reads user.conf and its fini writes
        // nothing (zhuyin.cpp:126-162, :741-757): a fresh dir has no marker
        // until the first save, which writes a counter of 0 (:164-176).
        let dir = tempdir("zhuyin-marker");
        let originals = originals();
        let fresh = load(&dir, &originals, &versions(), UserConfLaw::Zhuyin).expect("load");
        assert!(fresh.wiped);
        assert_eq!(fresh.open_counter, 0);
        assert!(
            !dir.join(USER_CONF).exists(),
            "a zhuyin init wrote user.conf"
        );
        fini(&dir, &versions(), UserConfLaw::Zhuyin, fresh.open_counter).expect("fini");
        assert!(
            !dir.join(USER_CONF).exists(),
            "a zhuyin fini wrote user.conf"
        );

        save(&dir, &state(), &originals, &versions(), fresh.open_counter).expect("save");
        let marker = std::fs::read(dir.join(USER_CONF)).expect("marker");
        assert_eq!(recorded_counter(&dir), Some(0));
        // Launches that die before fini, and clean ones, leave it alone:
        // no counter moves, so no wipe ever comes of them.
        for launch in 1..=10 {
            let loaded = load(&dir, &originals, &versions(), UserConfLaw::Zhuyin).expect("load");
            assert!(!loaded.wiped, "zhuyin launch {launch} wiped");
            assert_eq!(loaded.state, state());
            fini(&dir, &versions(), UserConfLaw::Zhuyin, loaded.open_counter).expect("fini");
            assert_eq!(std::fs::read(dir.join(USER_CONF)).expect("marker"), marker);
        }
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_zhuyin_wipe_keeps_the_non_conform_marker() {
        // A counter past the limit is non-conform under either law; the
        // zhuyin wipe removes the data files and leaves the marker as it
        // was (zhuyin.cpp:141-159 unlinks no user.conf), so every later
        // init wipes again until a save rewrites it.
        let dir = tempdir("zhuyin-tired");
        let originals = originals();
        save(
            &dir,
            &state(),
            &originals,
            &versions(),
            OPEN_COUNTER_LIMIT + 1,
        )
        .expect("seed");
        let marker = std::fs::read(dir.join(USER_CONF)).expect("marker");
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Zhuyin).expect("load");
        assert!(loaded.wiped);
        assert_eq!(loaded.state, UserState::default());
        assert!(
            !dir.join("user.bin").exists(),
            "profile file survived the wipe"
        );
        assert_eq!(std::fs::read(dir.join(USER_CONF)).expect("marker"), marker);
        assert!(
            load(&dir, &originals, &versions(), UserConfLaw::Zhuyin)
                .expect("load")
                .wiped
        );
        save(
            &dir,
            &UserState::default(),
            &originals,
            &versions(),
            loaded.open_counter,
        )
        .expect("save");
        assert_eq!(recorded_counter(&dir), Some(0));
        assert!(
            !load(&dir, &originals, &versions(), UserConfLaw::Zhuyin)
                .expect("load")
                .wiped
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The process umask, read without `unsafe`: a file created asking
    /// for 0777 keeps exactly the bits the umask lets through.
    #[cfg(unix)]
    fn current_umask(dir: &Path) -> u32 {
        use std::os::unix::fs::OpenOptionsExt;
        let probe = dir.join("umask-probe");
        let _ = std::fs::remove_file(&probe);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o777)
            .open(&probe)
            .expect("probe");
        let granted = mode_of(&probe);
        std::fs::remove_file(&probe).expect("probe");
        !granted & 0o777
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            .permissions()
            .mode()
            & 0o777
    }

    #[test]
    #[cfg(unix)]
    fn user_files_take_the_pins_creation_modes() {
        // #544: each file's mode is what upstream asks for, less the
        // umask — MemoryChunk::save's 0644 for the chunks
        // (memory_chunk.h:536-537), the backend's own save_db request for
        // the three DBMs (Berkeley DB 0600, Kyoto Cabinet's ofstream 0666,
        // tkrzw's 0644), fopen's 0666 for user.conf — and user.conf is
        // written in place, so a mode the user gave it survives a save
        // while the renamed files come back fresh.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir("modes");
        let originals = originals();
        let umask = current_umask(&dir);
        save(&dir, &state(), &originals, &versions(), 0).expect("save");

        let dbm_request = match oxpinyin_store::DEFAULT_STORE_DB_FORMAT {
            "BerkeleyDB" => 0o600,
            "KyotoCabinet" => 0o666,
            _ => 0o644,
        };
        let mut expected: Vec<(String, u32)> = vec![(USER_CONF.to_owned(), 0o666)];
        for dbm in [UserDbm::Bigram, UserDbm::PinyinIndex, UserDbm::PhraseIndex] {
            expected.push((dbm.file_name(), dbm_request));
        }
        for &(_, name) in USER_LIBRARY_FILES.iter().chain(SYSTEM_LOG_FILES) {
            expected.push((name.to_owned(), 0o644));
        }
        for (name, request) in &expected {
            assert_eq!(mode_of(&dir.join(name)), request & !umask, "{name}");
        }

        for name in [USER_CONF, "user.bin"] {
            std::fs::set_permissions(dir.join(name), std::fs::Permissions::from_mode(0o640))
                .expect("chmod");
        }
        save(&dir, &state(), &originals, &versions(), 0).expect("save again");
        assert_eq!(
            mode_of(&dir.join(USER_CONF)),
            0o640,
            "user.conf is rewritten in place"
        );
        assert_eq!(
            mode_of(&dir.join("user.bin")),
            0o644 & !umask,
            "user.bin is replaced"
        );

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
        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
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
                // A decodable-but-wrong old item (a real pron run; the
                // encoder rejects empty runs now).
                old_item: encode_phrase_item(&item(&[0x597d], 999, &[(vec![0x5678], 999)]))
                    .unwrap_or_else(|e| panic!("{e}")), // wrong old
                new_item: encode_phrase_item(&item(&[0x597d], 1, &[(vec![0x5679], 1)]))
                    .unwrap_or_else(|e| panic!("{e}")),
            },
            LogRecord::Modify {
                token: 0x0100_0001,
                old_item: encode_phrase_item(&item(&[0x4f60], 100, &[(vec![0x1234], 100)]))
                    .unwrap_or_else(|e| panic!("{e}")),
                new_item: encode_phrase_item(&item(&[0x4f60], 200, &[(vec![0x1235], 200)]))
                    .unwrap_or_else(|e| panic!("{e}")),
            },
        ];
        let framed = build_memory_chunk(&encode_log_records(&records).expect("records encode"));
        std::fs::write(dir.join("gb_char.dbin"), framed.expect("frame")).expect("write");
        let marker = UserTableInfo::conform_to(&versions());
        std::fs::write(dir.join(USER_CONF), marker.to_text()).expect("write");

        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(!loaded.wiped);
        // Slot 1 was never reached: the mismatch stopped the replay
        // before the second MODIFY (an absent map and an empty one are
        // the same state).
        assert!(
            loaded
                .state
                .system_overrides
                .get(&1)
                .is_none_or(std::collections::BTreeMap::is_empty)
        );

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn the_bigram_round_trips_through_the_user_bigram_container() {
        let dir = tempdir("bigram");
        let originals = originals();
        save(&dir, &state(), &originals, &versions(), 1).expect("save");

        // Point-read one gram through the *user-bigram* seam, which is
        // where the container differs by backend: a Kyoto Cabinet
        // snapshot stream of an in-memory stash, a tkrzw hash file, the
        // hash file. Opening it as a plain hash file
        // is the bug this assertion exists to catch — on Kyoto Cabinet
        // the user bigram is not a HashDB and a hash open answers
        // "missing magic data of the file".
        let path = dir.join(UserDbm::Bigram.file_name());
        let store = DefaultStore::open_user_bigram(&path).expect("open");
        let value = store.get_raw(&1_u32.to_le_bytes()).expect("get");
        let (total, records) = decode_single_gram(&value.expect("gram")).expect("decode");
        assert_eq!(total, 207);
        assert_eq!(
            records,
            vec![(1, 69), (0x0100_0001, 138), (0x0700_0001, 69)]
        );

        // The whole gram set walks back, not just one point read: the
        // loader takes `range_raw` over the same container.
        let mut seen = Vec::new();
        store
            .range_raw(
                std::ops::Bound::Unbounded,
                std::ops::Bound::Unbounded,
                &mut |key, _value| {
                    seen.push(u32::from_le_bytes([key[0], key[1], key[2], key[3]]));
                    Ok(())
                },
            )
            .expect("walk");
        assert_eq!(seen, vec![1], "the profile holds one gram");

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

        let loaded = load(&dir, &originals, &versions(), UserConfLaw::Pinyin).expect("load");
        assert!(!loaded.wiped, "user.conf is intact; only files skip");
        assert_eq!(loaded.skipped.len(), 2, "{:?}", loaded.skipped);
        assert!(loaded.state.bigram.is_empty());
        assert!(!loaded.state.libraries.contains_key(&7));
        // The untouched halves still load.
        assert!(loaded.state.system_overrides.contains_key(&1));

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The frozen cross-backend record golden, regenerated by
    /// `the_saved_records_are_backend_independent`'s own dump. Identical
    /// on Kyoto Cabinet, tkrzw and Berkeley DB (measured
    /// 2026-09-09).
    const RECORDS_GOLDEN: &str = r#"## user_bigram (2 rows)
  01000000 = cf0000000100000045000000020000018a0000000100000745000000
  01000007 = 450000000100000545000000
## user_pinyin_index (12 rows)
  0200 = 
  02000f00 = 0100000582008f03
  0300 = 
  03000b00 = 0200000783028b01
  1700 = 
  17000700 = 0100000797008706
  8200 = 
  82008f03 = 0100000582008f03
  8302 = 
  83028b01 = 0200000783028b01
  9700 = 
  97008706 = 0100000797008706
## user_phrase_index (6 rows)
  17530000 = 
  17530000ac4e0000 = 01000005
  4b6d0000 = 
  4b6d0000d58b0000 = 02000007
  604f0000 = 
  604f00007d590000 = 01000007
## user.bin (83 payload bytes)
  2d000000110000001e000000530000002300000000080000001e00000023000000000000000002010f000000604f00007d590000970087060f00000002011e0000004b6d0000d58b000083028b011e00000023
## addon.bin (57 payload bytes)
  09000000110000001a0000003900000023000000000800000023000000000000000002010900000017530000ac4e000082008f030900000023
## network.bin (19 payload bytes)
  00000000110000001200000013000000232323
## gb_char.dbin (62 payload bytes)
  040000000000000004006400000047020000030000000100000110001000010164000000604f0000970064000000010147020000604f00009700a9000000
## gbk_char.dbin (18 payload bytes)
  040000000000000004000000000000000000
## opengram.dbin (18 payload bytes)
  040000000000000004000000000000000000
## merged.dbin (18 payload bytes)
  040000000000000004000000000000000000"#;

    /// The records a fixed user state produces, byte-for-byte, on every
    /// backend. The peer builds cannot share a process (the
    /// exactly-one-backend invariant), so this golden is the
    /// cross-backend equivalence check the store crate uses elsewhere:
    /// each build asserts the same bytes, and CI runs them all. It is
    /// what makes "the same records in whichever container the build
    /// selected" a tested claim rather than an assertion in a doc — the
    /// objective's "as though it were libpinyin with a peer container".
    #[test]
    fn the_saved_records_are_backend_independent() {
        let dir = tempdir("records");
        let (state, originals) = cross_backend_state();
        save(&dir, &state, &originals, &versions(), 1).expect("save");
        let got = dump_user_dir(&dir);
        assert_eq!(
            got.trim_end(),
            RECORDS_GOLDEN,
            "the backend's records drifted from the frozen golden"
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// One fixed user state: two user phrases in library 7, one in the
    /// addon library 5, one system-token MODIFY in library 1, and two
    /// grams. Every table the file set carries is non-empty.
    fn cross_backend_state() -> (UserState, BTreeMap<u8, SystemLibrary>) {
        let k = |i: u8, m: u8, f: u8| ChewingKey::new(i, m, f, 0).to_packed();
        let mut libraries = BTreeMap::new();
        libraries.insert(
            7_u8,
            BTreeMap::from([
                (
                    1_u32,
                    ChunkItem {
                        phrase: vec![0x4f60, 0x597d],
                        unigram: 15,
                        prons: vec![(vec![k(23, 0, 1), k(7, 0, 13)], 15)],
                    },
                ),
                (
                    2_u32,
                    ChunkItem {
                        phrase: vec![0x6d4b, 0x8bd5],
                        unigram: 30,
                        prons: vec![(vec![k(3, 0, 5), k(11, 0, 3)], 30)],
                    },
                ),
            ]),
        );
        libraries.insert(
            5_u8,
            BTreeMap::from([(
                1_u32,
                ChunkItem {
                    phrase: vec![0x5317, 0x4eac],
                    unigram: 9,
                    prons: vec![(vec![k(2, 0, 1), k(15, 0, 7)], 9)],
                },
            )]),
        );
        let mut bigram = BTreeMap::new();
        bigram.insert(
            1_u32,
            Gram {
                total: 207,
                items: BTreeMap::from([(1_u32, 69), (0x0100_0002, 138), (0x0700_0001, 69)]),
            },
        );
        bigram.insert(
            0x0700_0001,
            Gram {
                total: 69,
                items: BTreeMap::from([(0x0500_0001, 69)]),
            },
        );
        let mut system_overrides = BTreeMap::new();
        system_overrides.insert(
            1_u8,
            BTreeMap::from([(
                1_u32,
                Some(ChunkItem {
                    phrase: vec![0x4f60],
                    unigram: 583,
                    prons: vec![(vec![k(23, 0, 1)], 169)],
                }),
            )]),
        );
        let originals = BTreeMap::from([(
            1_u8,
            SystemLibrary {
                total: 100,
                items: BTreeMap::from([(
                    1_u32,
                    ChunkItem {
                        phrase: vec![0x4f60],
                        unigram: 100,
                        prons: vec![(vec![k(23, 0, 1)], 100)],
                    },
                )]),
            },
        )]);
        (
            UserState {
                bigram,
                libraries,
                system_overrides,
            },
            originals,
        )
    }

    /// Every row of every DBM and every chunk payload in `dir`, as one
    /// canonical hex dump sorted by key — the golden's exact form.
    fn dump_user_dir(dir: &Path) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        for dbm in [UserDbm::Bigram, UserDbm::PinyinIndex, UserDbm::PhraseIndex] {
            let path = dir.join(dbm.file_name());
            let store = if dbm.is_hash() {
                DefaultStore::open_user_bigram(&path).expect("user bigram opens")
            } else {
                DefaultStore::open_read_only(&path).expect("index opens")
            };
            let mut rows = Vec::new();
            store
                .range_raw(
                    std::ops::Bound::Unbounded,
                    std::ops::Bound::Unbounded,
                    &mut |k, v| {
                        rows.push((k.to_vec(), v.to_vec()));
                        Ok(())
                    },
                )
                .expect("walk");
            rows.sort();
            let _ = writeln!(out, "## {} ({} rows)", dbm.stem(), rows.len());
            for (k, v) in rows {
                let _ = writeln!(out, "  {} = {}", hex(&k), hex(&v));
            }
        }
        for name in [
            "user.bin",
            "addon.bin",
            "network.bin",
            "gb_char.dbin",
            "gbk_char.dbin",
            "opengram.dbin",
            "merged.dbin",
        ] {
            let bytes = std::fs::read(dir.join(name)).expect("chunk file");
            let payload = read_chunk_payload(&bytes).expect("frame");
            let _ = writeln!(
                out,
                "## {name} ({} payload bytes)\n  {}",
                payload.len(),
                hex(payload)
            );
        }
        out
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }
}
