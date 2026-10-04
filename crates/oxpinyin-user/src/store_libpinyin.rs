//! The libpinyin-file user store — [`crate::GenericUserStore`] over the
//! pin's own user-dir file set (drop-in task 9's wiring half).
//!
//! [`UserStore::open_libpinyin`] opens a user *directory* (not a store
//! file): [`crate::persistence`] reads the profile, the loaded values
//! seed a **session store in process memory** (the backend's own
//! on-memory ordered container, [`WriteStore::create_in_memory`]), and
//! every existing value operation — training, phrase adds, exports,
//! masking — runs against that session exactly as before.
//! [`UserStore::save`] then exports the session values back through
//! [`crate::persistence`] into the pin's files. The durability shape is
//! therefore the pin's own: in memory between saves, as the pin's
//! context holds its loaded profile (`pinyin.cpp:326-444` at 074a2219),
//! whole-file `.tmp`+rename at `pinyin_save`, and a crash loses the
//! sub-timer window exactly as upstream's does (the W6-T5 "reproduce the
//! call pattern, not the loss window" deviation is reverted by design).
//!
//! The session touches nothing outside the user dir: no temp directory,
//! no `TMPDIR` read, nothing left behind by a process that dies before
//! `pinyin_fini` (#546), and nothing a `fork`ed child can corrupt under
//! its parent through a shared file description (#531 — a file-backed
//! session was truncated by the child's save, and the parent's Kyoto
//! Cabinet handle then failed or spun on its stale view of the file).
//! The pin reads no temp variable and opens no temp file anywhere under
//! `src` at 074a2219.
//!
//! The value mapping, both directions:
//!
//! * bigram — `BIGRAM[(prev,cur)]` + `BIGRAM_TOTAL[prev]` rows are the
//!   `SingleGram` grams of `user_bigram.db`, one-for-one.
//! * user phrases — a `user.bin`/`addon.bin` item is the `PHRASE` text,
//!   the `PRONUNCIATION` rows (packed keys — one wire form in both
//!   schemas) and the `UNIGRAM` row: upstream's item `unigram` field is
//!   exactly this store's full accumulation for the token (`count·3` at
//!   `_add_phrase`, `seed·7` per training, the promotion unigram).
//! * system tokens — a `.dbin` MODIFY record is the original item with
//!   `unigram + UNIGRAM[token]`; the value model tracks no per-token
//!   removal (a replayed REMOVE degrades to a skip, noted at load), and
//!   no per-pronunciation delta for system tokens — the pre-existing
//!   engine-model gap, unchanged by this persistence.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oxpinyin_data::chunk_write::ChunkItem;
use oxpinyin_data::user_files::SystemVersions;
use oxpinyin_store::{DefaultStore, StoreError, WriteStore, WriteTxn};

use crate::codec;
use crate::persistence::{self, PersistenceError, SystemLibrary, UserConfLaw, UserState};
use crate::phrase::{self, phrase_index_library_index};
use crate::registry::StoreInner;
use crate::store::{
    ALLOC, ALLOC_CURSOR, BIGRAM, BIGRAM_TOTAL, GenericUserStore, PHRASE, PHRASE_BY_LIB_TEXT,
    PHRASE_BY_TEXT, PRONUNCIATION, PronValue, SYSTEM_BASE, Token, UNIGRAM, UNIGRAM_TOTAL,
    UNIGRAM_TOTAL_KEY, UserStoreError,
};

/// The persistence target a session store carries: the user dir, the
/// original system chunks (the `.dbin` diff base), the conformance
/// triple, and the `user.conf` law and open counter the session's saves
/// and its fini write.
#[derive(Clone, Debug)]
pub struct Target {
    /// The user directory holding the profile.
    pub(crate) dir: PathBuf,
    /// The system libraries by nibble, as loaded at open.
    pub(crate) originals: BTreeMap<u8, SystemLibrary>,
    /// This build's identity triple.
    pub(crate) versions: SystemVersions,
    /// Whose `user.conf` lifecycle the profile follows.
    pub(crate) law: UserConfLaw,
    /// The open counter the session's `user.conf` writes start from
    /// ([`persistence::Loaded::open_counter`]).
    pub(crate) open_counter: i32,
}

/// The facade fini's `user.conf` write ([`persistence::fini`]): libpinyin
/// lowers the open counter its init raised and writes the marker, saved
/// or not (`pinyin_fini`, `pinyin.cpp:1194-1200`); libzhuyin writes
/// nothing. Armed on the handle [`GenericUserStore::open_libpinyin`]
/// returns and never on a clone of it, so it runs once per open — every
/// open runs its own `check_format` — when that handle drops: the
/// context's fini. A process that dies before then leaves the raised
/// counter on disk, as upstream's does.
#[derive(Debug)]
pub(crate) struct FiniGuard(Option<Arc<Target>>);

impl FiniGuard {
    /// A guard that writes nothing: every handle but the one an open
    /// returned.
    pub(crate) const fn disarmed() -> Self {
        Self(None)
    }
}

impl Drop for FiniGuard {
    fn drop(&mut self) {
        if let Some(target) = self.0.take() {
            // `pinyin_fini` ignores `mark_version`'s result
            // (`pinyin.cpp:1200`): a fini has no caller to answer, only the
            // line `UserTableInfo::save` prints (`table_info.cpp:382`).
            if persistence::fini(
                &target.dir,
                &target.versions,
                target.law,
                target.open_counter,
            )
            .is_err()
            {
                crate::persistence::diagnostic(&[
                    b"write ",
                    target.dir.join("user.conf").as_os_str().as_encoded_bytes(),
                    b" failed.\n",
                ]);
            }
        }
    }
}

impl From<PersistenceError> for UserStoreError {
    fn from(error: PersistenceError) -> Self {
        // The class-(c) refusal keeps its own variant: it is not a
        // degraded store but a failed open (see `persistence::load`).
        match error {
            PersistenceError::UnknownDatabaseFormat => UserStoreError::UnknownDatabaseFormat,
            other => UserStoreError::Persistence(other.to_string()),
        }
    }
}

impl GenericUserStore<DefaultStore> {
    /// Open the user store on a libpinyin user directory: read the
    /// profile ([`crate::persistence::load`]) under `law`, seed an
    /// in-memory session store with its values, and carry the persistence target so
    /// [`GenericUserStore::save`] writes the pin's files back. Dropping the
    /// returned handle — not a clone of it — is the facade's fini: it
    /// makes the law's fini-time `user.conf` write
    /// ([`crate::persistence::fini`]).
    ///
    /// Every open is a session of its own, as every `pinyin_init` /
    /// `zhuyin_init` is a context of its own upstream: nothing is held
    /// per directory (`pinyin.cpp:326-444` keeps the profile in the
    /// context, `:1132-1147` saves that context's state). A second open
    /// of a directory another live session has open runs `check_format`
    /// again, reads the profile as the files hold it, and learns and
    /// saves on its own session store — it does not see the first session's
    /// unsaved learning, and when both save, the later save's files are
    /// the profile.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the profile's `user.conf` cannot
    /// be re-written or the session store cannot be created. A corrupt
    /// or partial profile does not fail here — it degrades per-file,
    /// exactly as upstream's loader degrades.
    pub fn open_libpinyin(
        user_dir: &Path,
        originals: BTreeMap<u8, SystemLibrary>,
        versions: SystemVersions,
        law: UserConfLaw,
    ) -> Result<Self, UserStoreError> {
        // Create the session's own container before touching the
        // profile, so a failure here cannot follow a `check_format` that
        // already raised the open counter and wiped a non-conforming
        // profile. It is this session's alone: process memory, shared
        // with no other open and no other process.
        let db = DefaultStore::create_in_memory()?;

        let loaded = persistence::load(user_dir, &originals, &versions, law)?;
        // Armed as soon as the load has raised the counter: an open that
        // fails from here on drops it and lowers the counter again, so a
        // failed open reads as a finished session, not a crash.
        let target = Arc::new(Target {
            dir: user_dir.to_path_buf(),
            originals,
            versions,
            law,
            open_counter: loaded.open_counter,
        });
        let fini = FiniGuard(Some(Arc::clone(&target)));
        let has_user_data = db.write(|txn| {
            seed_txn(txn, &loaded.state, &target.originals)?;
            let total_rows = count_tables(txn)?;
            Ok(total_rows)
        })?;

        let inner = Arc::new(StoreInner {
            has_system_items: std::sync::atomic::AtomicBool::new(
                !loaded.state.system_overrides.is_empty(),
            ),
            system_items: std::sync::Mutex::new(
                loaded
                    .state
                    .system_overrides
                    .iter()
                    .flat_map(|(&nibble, items)| {
                        items.iter().filter_map(move |(&slot, item)| {
                            item.clone()
                                .map(|item| (((u32::from(nibble) << 24) | slot), item))
                        })
                    })
                    .collect(),
            ),
            count_cache: std::sync::Mutex::new(None),
            db: std::sync::Mutex::new(db),
            dirty: std::sync::atomic::AtomicBool::new(false),
            write_generation: std::sync::atomic::AtomicU64::new(0),
            phrase_generation: std::sync::atomic::AtomicU64::new(0),
            has_user_data: std::sync::atomic::AtomicBool::new(has_user_data),
            bigram_db: Some(std::sync::Mutex::new(persistence::load_user_bigram_db(
                &target.dir,
            )?)),
            libpinyin: Some(target),
        });
        Ok(Self::from_parts(inner, None, fini))
    }
}

/// `init_and_wrap`'s `has_user_data` probe, against a seeded session store.
fn count_tables(txn: &mut dyn WriteTxn) -> Result<bool, StoreError> {
    Ok(!txn.is_empty(BIGRAM)? || !txn.is_empty(UNIGRAM)? || !txn.is_empty(PHRASE)?)
}

/// Writes the loaded profile's values into a fresh session store — the
/// seed half of the value mapping.
fn seed_txn(
    txn: &mut dyn WriteTxn,
    state: &UserState,
    originals: &BTreeMap<u8, SystemLibrary>,
) -> Result<(), StoreError> {
    let mut unigram_total = 0_u64;
    let mut alloc_cursor: BTreeMap<u8, Token> = BTreeMap::new();

    // ADD records own text/index rows just like USER_FILE items. MODIFY
    // records do not gain index rows (pinyin.cpp:566-607 at 074a2219).
    let mut libraries = state.libraries.clone();
    for (&nibble, original) in originals {
        txn.put(
            SYSTEM_BASE,
            &codec::encode_u8(nibble),
            &codec::encode_u64(u64::from(original.total)),
        )?;
        alloc_cursor.insert(
            nibble,
            (u32::from(nibble) << 24) | original.range_end.max(1).saturating_sub(1),
        );
    }
    for (&nibble, overrides) in &state.system_overrides {
        for (&slot, item) in overrides {
            let Some(item) = item else {
                continue;
            };
            if !originals
                .get(&nibble)
                .is_some_and(|original| original.contains_item(slot))
            {
                libraries
                    .entry(nibble)
                    .or_default()
                    .insert(slot, item.clone());
            }
        }
    }
    for (&nibble, items) in &libraries {
        for (&slot, item) in items {
            let token = (u32::from(nibble) << 24) | slot;
            let Some(text) = ucs4_to_string(&item.phrase) else {
                continue; // a phrase that is not UTF-32 text cannot be a
                // user phrase in this engine; leave it unseeded
            };
            txn.put(
                PHRASE,
                &codec::encode_token(token),
                codec::encode_str(&text),
            )?;
            txn.put(
                PHRASE_BY_LIB_TEXT,
                &codec::encode_u8_str(nibble, &text),
                &codec::encode_token(token),
            )?;
            if nibble == crate::phrase::USER_DICTIONARY {
                txn.put(
                    PHRASE_BY_TEXT,
                    codec::encode_str(&text),
                    &codec::encode_token(token),
                )?;
            }
            for (seq, (keys, freq)) in (0_u32..).zip(&item.prons) {
                // The store's pronunciation table keys on the dense
                // syllable id (`PinyinKey`), while the pin's file format
                // carries packed `ChewingKey` words — the conversion
                // belongs here, at the seam between the two models. A
                // key that names no syllable in this engine's frozen
                // inventory cannot be represented, so the reading is
                // dropped rather than stored as a wrong id. The item's
                // reading order is the insertion order; the user pinyin
                // index says which readings lookup finds.
                let Some(ids) = packed_to_pinyin_keys(keys) else {
                    continue;
                };
                let value = PronValue {
                    count: u64::from(*freq),
                    seq,
                    indexed: state.indexed.contains(&(token, keys.clone())),
                };
                txn.put(
                    PRONUNCIATION,
                    &codec::encode_token_bytes(token, &phrase::encode_keys(&ids)),
                    &value.encode(),
                )?;
            }
            txn.put(
                UNIGRAM,
                &codec::encode_token(token),
                &codec::encode_u64(u64::from(item.unigram)),
            )?;
            unigram_total = unigram_total.saturating_add(u64::from(item.unigram));
            let cursor = alloc_cursor.entry(nibble).or_insert(0);
            if token > *cursor {
                *cursor = token;
            }
        }
    }

    // System-token deltas: the training mass on top of the originals.
    for (&nibble, overrides) in &state.system_overrides {
        let Some(original) = originals.get(&nibble) else {
            continue;
        };
        for (&slot, item) in overrides {
            let Some(new_item) = item else {
                continue; // a replayed removal: not expressible in the
                // value model; the load's `skipped` list carries it
            };
            let Some(old) = original.item(slot) else {
                continue;
            };
            // Restore packed system-pronunciation deltas on reopen, so
            // train/save does not erase earlier matched-reading updates.
            let token = (u32::from(nibble) << 24) | slot;
            for (keys, frequency) in &new_item.prons {
                let base = old
                    .prons
                    .iter()
                    .find(|(base, _)| base == keys)
                    .map_or(0, |(_, frequency)| *frequency);
                let delta = frequency.wrapping_sub(base);
                if delta != 0 {
                    let value = crate::store::PronValue {
                        count: u64::from(delta),
                        seq: u32::MAX,
                        indexed: false,
                    };
                    txn.put(
                        PRONUNCIATION,
                        &codec::encode_token_bytes(token, &phrase::encode_keys(keys)),
                        &value.encode(),
                    )?;
                }
            }
            let base = old.unigram;
            let delta = u64::from(new_item.unigram.saturating_sub(base));
            if delta == 0 {
                continue;
            }
            let key = codec::encode_token(token);
            let prev = crate::store::txn_get_u64_or(txn, UNIGRAM, &key, 0)?;
            txn.put(
                UNIGRAM,
                &key,
                &codec::encode_u64(prev.saturating_add(delta)),
            )?;
            unigram_total = unigram_total.saturating_add(delta);
        }
    }

    // The bigram grams, rows wholesale.
    for (&prev, gram) in &state.bigram {
        for (&cur, &count) in &gram.items {
            txn.put(
                BIGRAM,
                &codec::encode_token_pair(prev, cur),
                &codec::encode_u64(u64::from(count)),
            )?;
        }
        txn.put(
            BIGRAM_TOTAL,
            &codec::encode_token(prev),
            &codec::encode_u64(u64::from(gram.total)),
        )?;
    }

    // The alloc cursors: one past the highest loaded token per library.
    for (nibble, max_token) in alloc_cursor {
        let next = phrase::next_library_token_after(nibble, max_token)
            .unwrap_or_else(|| phrase::first_library_token(nibble));
        txn.put(ALLOC, &codec::encode_u8(nibble), &codec::encode_token(next))?;
        if nibble == crate::phrase::USER_DICTIONARY {
            txn.put(
                ALLOC,
                &codec::encode_u8(ALLOC_CURSOR),
                &codec::encode_token(next),
            )?;
        }
    }
    txn.put(
        UNIGRAM_TOTAL,
        &codec::encode_u8(UNIGRAM_TOTAL_KEY),
        &codec::encode_u64(unigram_total),
    )?;
    Ok(())
}

/// Reads the session values back into the persistence shape — the
/// export half of the value mapping, [`GenericUserStore::save`]'s
/// libpinyin branch.
pub fn export_state<S: WriteStore>(
    store: &GenericUserStore<S>,
    originals: &BTreeMap<u8, SystemLibrary>,
) -> Result<UserState, UserStoreError> {
    let db = store.database();

    let mut phrase_text: BTreeMap<Token, Vec<u32>> = BTreeMap::new();
    db.for_each(PHRASE, &mut |key, value| {
        let token = codec::decode_token(key)
            .map_err(|_| StoreError::Backend("corrupt phrase token".into()))?;
        let text = codec::decode_str(value)
            .map_err(|_| StoreError::Backend("corrupt phrase text".into()))?;
        phrase_text.insert(token, text.chars().map(u32::from).collect());
        Ok(())
    })?;

    let mut pronunciations: BTreeMap<Token, Vec<StoredReading>> = BTreeMap::new();
    db.for_each(PRONUNCIATION, &mut |key, value| {
        let (token, key_bytes) = codec::decode_token_bytes(key)
            .map_err(|_| StoreError::Backend("corrupt pronunciation key".into()))?;
        let value = PronValue::decode(value)?;
        // The reverse of the seed conversion: the store holds dense
        // syllable ids, the file format wants packed `ChewingKey`
        // words. A stored id outside the engine's inventory is skipped
        // (a corrupt row, never a panic).
        let keys = phrase::decode_keys(key_bytes);
        let packed = if !phrase_text.contains_key(&token)
            && originals.contains_key(&phrase_index_library_index(token))
        {
            // A system item's rows retain exact packed keys and their values
            // are deltas; a phrase added to a system library (it has a phrase
            // row) keeps the user store's dense syllable ids.
            keys
        } else {
            let Some(packed) = pinyin_keys_to_packed(&keys) else {
                return Ok(());
            };
            packed
        };
        pronunciations.entry(token).or_default().push((
            value.seq,
            packed,
            value.count,
            value.indexed,
        ));
        Ok(())
    })?;
    for rows in pronunciations.values_mut() {
        // Insertion order: the item's reading order in the file.
        rows.sort_by_key(|row| row.0);
    }

    let mut unigrams: BTreeMap<Token, u64> = BTreeMap::new();
    db.for_each(UNIGRAM, &mut |key, value| {
        let token = codec::decode_token(key)
            .map_err(|_| StoreError::Backend("corrupt unigram key".into()))?;
        let delta = codec::decode_u64(value)
            .map_err(|_| StoreError::Backend("corrupt unigram value".into()))?;
        unigrams.insert(token, delta);
        Ok(())
    })?;

    let mut bigram_pairs: BTreeMap<(Token, Token), u64> = BTreeMap::new();
    db.for_each(BIGRAM, &mut |key, value| {
        let (prev, cur) = codec::decode_token_pair(key)
            .map_err(|_| StoreError::Backend("corrupt bigram key".into()))?;
        let count = codec::decode_u64(value)
            .map_err(|_| StoreError::Backend("corrupt bigram count".into()))?;
        bigram_pairs.insert((prev, cur), count);
        Ok(())
    })?;
    let mut bigram_totals: BTreeMap<Token, u64> = BTreeMap::new();
    db.for_each(BIGRAM_TOTAL, &mut |key, value| {
        let prev = codec::decode_token(key)
            .map_err(|_| StoreError::Backend("corrupt bigram total key".into()))?;
        let total = codec::decode_u64(value)
            .map_err(|_| StoreError::Backend("corrupt bigram total".into()))?;
        bigram_totals.insert(prev, total);
        Ok(())
    })?;

    let system_items = store.stored_system_items();
    drop(db);

    let mut state = UserState::default();
    for (token, text) in &phrase_text {
        // An item whose unigram add the sub-index refused keeps 0
        // (`SubPhraseIndex::add_unigram_frequency`'s overflow guard).
        let delta = unigrams.get(token).copied().unwrap_or(0);
        let prons = pronunciations.get(token).map_or(Vec::new(), |rows| {
            rows.iter()
                .map(|(_, packed, count, indexed)| {
                    if *indexed {
                        state.indexed.insert((*token, packed.clone()));
                    }
                    (packed.clone(), u32::try_from(*count).unwrap_or(u32::MAX))
                })
                .collect()
        });
        let nibble = phrase_index_library_index(*token);
        state.libraries.entry(nibble).or_default().insert(
            token & crate::phrase::PHRASE_MASK,
            ChunkItem {
                phrase: text.clone(),
                unigram: u32::try_from(delta).unwrap_or(u32::MAX),
                prons,
            },
        );
    }

    for (token, delta) in &unigrams {
        let nibble = phrase_index_library_index(*token);
        if phrase_text.contains_key(token) || !originals.contains_key(&nibble) {
            continue; // user tokens landed above; unknown libraries skip
        }
        let Some(original) = originals.get(&nibble) else {
            continue;
        };
        let slot = token & crate::phrase::PHRASE_MASK;
        let Some(old) = original.item(slot) else {
            continue;
        };
        let mut new_item = old.clone();
        for (keys, frequency) in &mut new_item.prons {
            if let Some(delta) = pronunciations
                .get(token)
                .and_then(|rows| rows.iter().find(|row| &row.1 == keys))
                .map(|row| row.2)
            {
                *frequency = frequency.wrapping_add(delta as u32);
            }
        }
        new_item.unigram = old
            .unigram
            .saturating_add(u32::try_from(*delta).unwrap_or(u32::MAX));
        state
            .system_overrides
            .entry(nibble)
            .or_default()
            .insert(slot, Some(new_item));
    }

    let mut by_prev: BTreeMap<Token, BTreeMap<Token, u64>> = BTreeMap::new();
    for ((prev, cur), count) in bigram_pairs {
        by_prev.entry(prev).or_default().insert(cur, count);
    }
    for (prev, items) in by_prev {
        let total = bigram_totals.get(&prev).copied().unwrap_or(0);
        state.bigram.insert(
            prev,
            crate::persistence::Gram {
                total: u32::try_from(total).unwrap_or(u32::MAX),
                items: items
                    .into_iter()
                    .map(|(cur, count)| (cur, u32::try_from(count).unwrap_or(u32::MAX)))
                    .collect(),
            },
        );
    }
    for (token, mut item) in system_items {
        let nibble = phrase_index_library_index(token);
        let slot = token & crate::phrase::PHRASE_MASK;
        let base = originals
            .get(&nibble)
            .and_then(|original| original.unigram(slot))
            .unwrap_or(0);
        item.unigram = base.wrapping_add(unigrams.get(&token).copied().unwrap_or(0) as u32);
        // A payload carries the pronunciation counts it was saved with. For
        // an item the library originally holds, the stored rows are deltas
        // over the original counts (the loader writes them, training adds
        // to them), so a reading with a row is rebuilt from the original
        // count plus its delta. A phrase added to the library has no
        // original: its payload is the authority, as for its unigram.
        let original_item = originals
            .get(&nibble)
            .and_then(|original| original.item(slot));
        if let Some(original_item) = original_item.as_ref() {
            for (keys, frequency) in &mut item.prons {
                let Some(row) = pronunciations
                    .get(&token)
                    .and_then(|rows| rows.iter().find(|row| &row.1 == keys))
                else {
                    continue;
                };
                let base_count = original_item
                    .prons
                    .iter()
                    .find(|(original, _)| original == keys)
                    .map_or(0, |(_, count)| *count);
                *frequency = base_count.wrapping_add(u32::try_from(row.2).unwrap_or(u32::MAX));
            }
        }
        state
            .system_overrides
            .entry(nibble)
            .or_default()
            .insert(slot, Some(item));
    }
    state
        .libraries
        .retain(|nibble, _| !(1..=4).contains(nibble));
    Ok(state)
}

/// One stored reading on its way to the file: insertion sequence, packed
/// `ChewingKey` words, count, and whether the user pinyin index carries it.
type StoredReading = (u32, Vec<u16>, u64, bool);

/// UCS-4 code points as text; `None` when any scalar is invalid.
fn ucs4_to_string(codes: &[u32]) -> Option<String> {
    codes.iter().copied().map(char::from_u32).collect()
}

/// The store's dense syllable ids for a reading's packed `ChewingKey`
/// words; `None` when any word names no syllable in the frozen
/// inventory. This is the boundary conversion the two key models need —
/// `SyllableKey::index()` is a dense id, `ChewingKey::to_packed()` the
/// 16-bit bit-field the pin's files carry.
fn packed_to_pinyin_keys(packed: &[u16]) -> Option<Vec<crate::phrase::PinyinKey>> {
    packed
        .iter()
        .map(|&bits| {
            let key = oxpinyin_core::ChewingKey::from_packed(bits);
            let syllable = oxpinyin_core::SyllableKey::from_text(key.pinyin_spelling())?;
            phrase::toned_key(syllable.index(), key.tone)
        })
        .collect()
}

/// 074a2219 storage/pinyin_phrase3.h:68-147: initial equality,
/// incomplete middle/final wildcard, and zero-tone wildcard on either side.
pub(crate) fn pronunciation_matches(reading: &[oxpinyin_core::ChewingKey], packed: &[u16]) -> bool {
    reading.len() == packed.len()
        && reading.iter().zip(packed).all(|(query, bits)| {
            let stored = oxpinyin_core::ChewingKey::from_packed(*bits);
            query.initial == stored.initial
                && ((query.middle == stored.middle && query.final_ == stored.final_)
                    || (query.middle == 0 && query.final_ == 0)
                    || (stored.middle == 0 && stored.final_ == 0))
                && (query.tone == stored.tone || query.tone == 0 || stored.tone == 0)
        })
}

/// The reverse: dense syllable ids, tone included, as packed `ChewingKey`
/// words.
pub(crate) fn pinyin_keys_to_packed(ids: &[crate::phrase::PinyinKey]) -> Option<Vec<u16>> {
    ids.iter()
        .map(|&id| {
            let syllable = oxpinyin_core::SyllableKey::from_index(phrase::key_syllable(id))?;
            oxpinyin_core::ChewingKey::from_pinyin(syllable.text())
                .map(|key| key.with_tone(phrase::key_tone(id)).to_packed())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::Gram;
    use crate::store::UserStore;
    use oxpinyin_core::ChewingKey;

    fn originals() -> BTreeMap<u8, SystemLibrary> {
        let mut items = BTreeMap::new();
        items.insert(
            1_u32,
            ChunkItem {
                phrase: vec![0x4f60],
                unigram: 100,
                prons: vec![(vec![ChewingKey::new(23, 0, 1, 0).to_packed()], 100)],
            },
        );
        BTreeMap::from([(
            1_u8,
            SystemLibrary {
                mapped: None,
                total: 100,
                range_end: 2,
                items,
            },
        )])
    }

    fn versions() -> SystemVersions {
        SystemVersions::for_this_build(7, 14)
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-libpinyin-store-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("tmpdir");
        dir
    }

    #[test]
    fn trained_pronunciations_reopen_with_tones_and_path_multiplicity() {
        use oxpinyin_core::{PhraseToken, UserModel};
        let dir = tempdir("trained-prons");
        let mut base = originals();
        let item = base
            .get_mut(&1)
            .expect("library")
            .items
            .get_mut(&1)
            .expect("item");
        let key = ChewingKey::from_packed(item.prons[0].0[0]);
        let other = ChewingKey::from_pinyin("jin").expect("key");
        item.prons = vec![
            (vec![key.with_tone(2).to_packed()], 100),
            (vec![key.with_tone(3).to_packed()], 200),
            (vec![other.to_packed()], 300),
        ];
        let token = PhraseToken::new(0x0100_0001);
        {
            let mut store =
                UserStore::open_libpinyin(&dir, base.clone(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            let mut paths = vec![vec![key.with_tone(2)]].into_iter();
            UserModel::observe_with_keys(&mut store, &[], &token, &mut paths).expect("train");
            let state = export_state(&store, &base).expect("state");
            assert_eq!(
                state.system_overrides[&1][&1]
                    .as_ref()
                    .expect("item")
                    .prons
                    .iter()
                    .map(|p| p.1)
                    .collect::<Vec<_>>(),
                [169, 200, 300]
            );
            assert!(store.save().expect("save"));
        }
        {
            let mut store =
                UserStore::open_libpinyin(&dir, base.clone(), versions(), UserConfLaw::Pinyin)
                    .expect("reopen");
            let mut paths = vec![vec![key.with_tone(0)], vec![key.with_tone(0)]].into_iter();
            UserModel::observe_with_keys(&mut store, &[], &token, &mut paths).expect("retrain");
            let state = export_state(&store, &base).expect("state");
            assert_eq!(
                state.system_overrides[&1][&1]
                    .as_ref()
                    .expect("item")
                    .prons
                    .iter()
                    .map(|p| p.1)
                    .collect::<Vec<_>>(),
                [445, 476, 300]
            );
            assert!(store.save().expect("save"));
        }
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    /// The pin's save never stops at a failure (`pinyin.cpp:1132-1147`): with
    /// the user dir gone every rename and the marker write fail, each one is
    /// reported in the pin's order, and the save has still run. A save that
    /// works reports nothing.
    #[test]
    fn save_reporting_names_each_failure_in_the_pins_order() {
        let dir = tempdir("report");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        store.observe_selection(1, 0x0100_0001).expect("train");
        let clean = store.save_reporting().expect("save");
        assert_eq!(clean, crate::SaveReport::default());

        store.observe_selection(1, 0x0100_0001).expect("train");
        std::fs::remove_dir_all(&dir).expect("remove the user dir");
        let report = store
            .save_reporting()
            .expect("a missing dir is not an error");
        assert!(!store.is_modified(), "the store is clean after the save");
        let finals: Vec<&str> = report
            .renames_failed
            .iter()
            .map(|(_, target)| target.file_name().and_then(|n| n.to_str()).expect("name"))
            .collect();
        assert_eq!(
            finals,
            [
                "gb_char.dbin",
                "gbk_char.dbin",
                "opengram.dbin",
                "merged.dbin",
                "addon.bin",
                "network.bin",
                "user.bin",
                "user_pinyin_index.bin",
                "user_phrase_index.bin",
                "user_bigram.db",
            ]
        );
        for (tmp, target) in &report.renames_failed {
            assert_eq!(tmp.parent(), target.parent());
            assert_eq!(
                tmp.file_name().and_then(|n| n.to_str()),
                Some(
                    format!(
                        "{}.tmp",
                        target.file_name().and_then(|n| n.to_str()).expect("name")
                    )
                    .as_str()
                )
            );
        }
        assert_eq!(report.user_conf_write_failed, Some(dir.join("user.conf")));
    }

    #[test]
    fn train_save_reopen_carries_the_learning() {
        let dir = tempdir("cycle");
        // The store's `PinyinKey` is a dense syllable id, not packed
        // ChewingKey bits — using `to_packed()` here would encode the
        // exact confusion the persistence seam's conversion exists to
        // undo (and would round-trip self-consistently, hiding it).
        let keys = [
            u16::try_from(
                oxpinyin_core::SyllableKey::from_text("ni")
                    .expect("frozen syllable")
                    .index(),
            )
            .expect("id fits u16"),
            u16::try_from(
                oxpinyin_core::SyllableKey::from_text("hao")
                    .expect("frozen syllable")
                    .index(),
            )
            .expect("id fits u16"),
        ];

        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            // A fresh profile: no counts yet.
            assert_eq!(store.bigram_count(1, 0x0100_0001).expect("count"), 0);

            // §2.1 training: the seed arithmetic runs in the session
            // store; §4 gating: save only after a modification.
            assert!(!store.save().expect("save gate"));
            store.observe_selection(1, 0x0100_0001).expect("train");
            assert!(store.save().expect("save"));
            assert!(!store.save().expect("second save is a no-op"));

            // A user phrase under the user dictionary. `_add_phrase`
            // does not arm the §4 gate (the pin sets `m_modified` only
            // in `pinyin_train` and `pinyin_end_add_phrases`); the
            // import trio's `mark_modified` is what arms a phrase save.
            let token = store
                .add_phrase("你好", &keys[..], None)
                .expect("add phrase");
            assert_eq!(token & 0x0F00_0000, 0x0700_0000);
            assert!(!store.save().expect("gate holds"));
            store.mark_modified();
            assert!(store.save().expect("save 2"));
        }

        // The user dir now holds the pin's files, not user_store.<ext>.
        assert!(dir.join("user.conf").exists());
        assert!(
            !dir.join(format!("user_store.{}", oxpinyin_store::DEFAULT_STORE_EXT))
                .exists()
        );

        {
            let store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("reopen");
            // First training: seed 69 on the pair and the total.
            assert_eq!(store.bigram_count(1, 0x0100_0001).expect("count"), 69);
            assert_eq!(store.bigram_total(1).expect("total"), 69);
            // seed * 7 on the system token's unigram delta.
            assert_eq!(store.unigram_delta(0x0100_0001).expect("delta"), 69 * 7);

            // The user phrase round-trips with its reading and base
            // count (DEFAULT_PHRASE_COUNT * ADD_PHRASE_UNIGRAM_FACTOR).
            let token = store
                .token_for_phrase("你好")
                .expect("token")
                .expect("present");
            let phrase = store.phrase(token).expect("phrase").expect("row");
            assert_eq!(phrase.text(), "你好");
            let pron = &phrase.pronunciations()[0];
            assert_eq!(pron.count(), crate::phrase::DEFAULT_PHRASE_COUNT);
            assert_eq!(
                store.unigram_delta(token).expect("delta"),
                crate::phrase::DEFAULT_PHRASE_COUNT * crate::phrase::ADD_PHRASE_UNIGRAM_FACTOR
            );

            // The .dbin carries the system token's MODIFY; the user.bin
            // the phrase; the bigram the gram.
            let state = export_state(&store, &originals()).expect("export");
            let gram = state.bigram.get(&1).expect("gram");
            assert_eq!(gram.total, 69);
            assert_eq!(gram.items.get(&0x0100_0001), Some(&69));
            let user_items = state.libraries.get(&7).expect("user lib");
            let item = user_items
                .get(&(token & crate::phrase::PHRASE_MASK))
                .expect("item");
            assert_eq!(
                item.unigram,
                u32::try_from(crate::phrase::DEFAULT_PHRASE_COUNT * 3)
                    .expect("small default count")
            );
            let overrides = state.system_overrides.get(&1).expect("overrides");
            let modified = overrides
                .get(&(0x0100_0001 & crate::phrase::PHRASE_MASK))
                .and_then(Option::as_ref)
                .expect("modified item");
            assert_eq!(modified.unigram, 100 + 69 * 7);
        }

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_preexisting_libpinyin_profile_seeds_the_session() {
        let dir = tempdir("seed");
        // Write a profile directly through the persistence layer: a gram
        // and a user phrase, as a same-backend libpinyin would have.
        let mut gram = Gram {
            total: 138,
            items: BTreeMap::from([(0x0100_0001_u32, 138)]),
        };
        gram.items.insert(0x0700_0001, 69);
        let state = UserState {
            bigram: BTreeMap::from([(
                1_u32,
                Gram {
                    total: 207,
                    items: gram.items.clone(),
                },
            )]),
            libraries: BTreeMap::from([(
                7_u8,
                BTreeMap::from([(
                    1_u32,
                    ChunkItem {
                        phrase: vec![0x4f60, 0x597d],
                        unigram: 15,
                        prons: vec![(
                            vec![
                                ChewingKey::new(23, 0, 1, 0).to_packed(),
                                ChewingKey::new(7, 0, 13, 0).to_packed(),
                            ],
                            15,
                        )],
                    },
                )]),
            )]),
            system_overrides: BTreeMap::new(),
            indexed: std::collections::BTreeSet::new(),
        }
        .index_every_reading();
        persistence::save(&dir, &state, &originals(), &versions(), 1).expect("save");

        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("open");
        assert_eq!(store.bigram_total(1).expect("total"), 207);
        assert_eq!(store.bigram_count(1, 0x0700_0001).expect("count"), 69);
        let token = store
            .token_for_phrase("你好")
            .expect("token")
            .expect("present");
        assert_eq!(token, 0x0700_0001);
        assert_eq!(store.unigram_delta(token).expect("delta"), 15);
        // Training continues from the loaded counts: the next seed
        // doubles the stored count (§2.1's repeat rule).
        let mut store = store;
        let seed = store.observe_selection(1, 0x0700_0001).expect("train");
        assert_eq!(seed, 138); // max(prev 69, 69) * 2
        assert_eq!(store.bigram_count(1, 0x0700_0001).expect("count"), 69 + 138);

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The open counter `user.conf` records, as the next init reads it.
    fn recorded_counter(dir: &Path) -> Option<i32> {
        let bytes = std::fs::read(dir.join("user.conf")).ok()?;
        Some(
            oxpinyin_data::user_files::UserTableInfo::parse(&bytes)
                .ok()?
                .open_counter,
        )
    }

    #[test]
    fn a_second_open_is_a_session_of_its_own() {
        // #538: two opens of one user dir are two contexts, as two
        // pinyin_init calls are upstream. Each runs check_format, so the
        // counter reaches 2 (pinyin.cpp:185-187); each learns on its own
        // session store, so the second does not see the first's unsaved
        // learning; each save answers for its own modifications; each
        // fini lowers its own copy of the counter (:1194-1200).
        let dir = tempdir("two-sessions");
        let mut first =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open 1");
        let mut second =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open 2");
        assert_eq!(recorded_counter(&dir), Some(2));

        first.observe_selection(1, 0x0100_0001).expect("train");
        assert_eq!(first.bigram_count(1, 0x0100_0001).expect("count"), 69);
        assert_eq!(second.bigram_count(1, 0x0100_0001).expect("count"), 0);

        assert!(
            !second.save().expect("save 2"),
            "the second session changed nothing"
        );
        assert!(first.save().expect("save 1"));
        assert_eq!(
            recorded_counter(&dir),
            Some(1),
            "the first save writes its own counter"
        );

        drop(first);
        assert_eq!(recorded_counter(&dir), Some(0));
        drop(second);
        assert_eq!(recorded_counter(&dir), Some(1));

        // The profile holds the first session's saved learning.
        let third = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("open 3");
        assert_eq!(third.bigram_count(1, 0x0100_0001).expect("count"), 69);
        drop(third);

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn when_two_sessions_save_the_later_save_is_the_profile() {
        // Both sessions learn and both save: each writes its own whole
        // state (pinyin.cpp:1132-1147), so the profile is the later one's
        // and the earlier session's learning is gone from it.
        let dir = tempdir("two-saves");
        let mut first =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open 1");
        let mut second =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open 2");
        // The first session trains the gram once, the second twice: the
        // counts (69 against 69 + 138) tell whose save the profile kept.
        first.observe_selection(1, 0x0100_0001).expect("train 1");
        second.observe_selection(1, 0x0100_0001).expect("train 2");
        second
            .observe_selection(1, 0x0100_0001)
            .expect("train 2 again");
        assert_eq!(first.bigram_count(1, 0x0100_0001).expect("count"), 69);
        assert_eq!(second.bigram_count(1, 0x0100_0001).expect("count"), 207);
        assert!(first.save().expect("save 1"));
        assert!(second.save().expect("save 2"));
        drop(first);
        drop(second);

        let third = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("open 3");
        assert_eq!(third.bigram_count(1, 0x0100_0001).expect("count"), 207);
        drop(third);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_pinyin_open_after_a_zhuyin_open_keeps_libpinyins_law() {
        // #578's review: a session follows the law it was opened with, not
        // the law of whatever already has the dir open. libzhuyin's init
        // only reads user.conf (zhuyin.cpp:126-162); libpinyin's raises and
        // writes the counter whatever else is live (pinyin.cpp:185-187).
        let dir = tempdir("zhuyin-then-pinyin");
        let mut zhuyin =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Zhuyin)
                .expect("open zhuyin");
        assert_eq!(recorded_counter(&dir), None);
        let mut pinyin =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open pinyin");
        assert_eq!(recorded_counter(&dir), Some(1));

        zhuyin
            .observe_selection(1, 0x0100_0001)
            .expect("train zhuyin");
        assert_eq!(pinyin.bigram_count(1, 0x0100_0001).expect("count"), 0);
        // libzhuyin's save writes a fresh counter of 0 (zhuyin.cpp:164-176,
        // :695); libpinyin's writes back its own raised one
        // (pinyin.cpp:1143, :220-232).
        assert!(zhuyin.save().expect("save zhuyin"));
        assert_eq!(recorded_counter(&dir), Some(0));
        pinyin
            .observe_selection(1, 0x0100_0001)
            .expect("train pinyin");
        assert!(pinyin.save().expect("save pinyin"));
        assert_eq!(recorded_counter(&dir), Some(1));

        // libpinyin's fini lowers its copy (pinyin.cpp:1194-1200);
        // libzhuyin's writes nothing (zhuyin.cpp:741-757).
        drop(pinyin);
        assert_eq!(recorded_counter(&dir), Some(0));
        drop(zhuyin);
        assert_eq!(recorded_counter(&dir), Some(0));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_zhuyin_open_after_a_pinyin_open_keeps_libzhuyins_law() {
        // The other order: the live libpinyin session lends the libzhuyin
        // open neither its law nor its unsaved learning.
        let dir = tempdir("pinyin-then-zhuyin");
        let mut pinyin =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open pinyin");
        assert_eq!(recorded_counter(&dir), Some(1));
        let mut zhuyin =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Zhuyin)
                .expect("open zhuyin");
        assert_eq!(recorded_counter(&dir), Some(1));

        pinyin
            .observe_selection(1, 0x0100_0001)
            .expect("train pinyin");
        assert_eq!(zhuyin.bigram_count(1, 0x0100_0001).expect("count"), 0);
        zhuyin
            .observe_selection(1, 0x0100_0001)
            .expect("train zhuyin");
        assert!(pinyin.save().expect("save pinyin"));
        assert_eq!(recorded_counter(&dir), Some(1));
        // The later save is libzhuyin's: its fresh 0 replaces libpinyin's
        // raised counter.
        assert!(zhuyin.save().expect("save zhuyin"));
        assert_eq!(recorded_counter(&dir), Some(0));

        // The finis in the reverse order: libzhuyin's writes nothing, and
        // libpinyin's lowers its own 1.
        drop(zhuyin);
        assert_eq!(recorded_counter(&dir), Some(0));
        drop(pinyin);
        assert_eq!(recorded_counter(&dir), Some(0));
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
