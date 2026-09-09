//! The libpinyin-file user store — [`crate::GenericUserStore`] over the
//! pin's own user-dir file set (drop-in task 9's wiring half).
//!
//! [`UserStore::open_libpinyin`] opens a user *directory* (not a store
//! file): [`crate::persistence`] reads the profile, the loaded values
//! seed a **session scratch store** (a temp file in this backend's
//! container, removed when the last handle drops), and every existing
//! value operation — training, phrase adds, exports, masking — runs
//! against that scratch exactly as before. [`UserStore::save`] then
//! exports the session values back through [`crate::persistence`] into
//! the pin's files. The durability shape is therefore the pin's own:
//! in-memory-equivalent between saves, whole-file `.tmp`+rename at
//! `pinyin_save`, and a crash loses the sub-timer window exactly as
//! upstream's does (the W6-T5 "reproduce the call pattern, not the loss
//! window" deviation is reverted by design).
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
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oxpinyin_data::chunk_write::ChunkItem;
use oxpinyin_data::user_files::SystemVersions;
use oxpinyin_store::{DefaultStore, StoreError, WriteStore, WriteTxn};

use crate::codec;
use crate::persistence::{self, PersistenceError, SystemLibrary, UserState};
use crate::phrase::{self, phrase_index_library_index};
use crate::registry::{self, StoreInner};
use crate::store::{
    ALLOC, ALLOC_CURSOR, BIGRAM, BIGRAM_TOTAL, GenericUserStore, PHRASE, PHRASE_BY_LIB_TEXT,
    PHRASE_BY_TEXT, PRONUNCIATION, Token, UNIGRAM, UNIGRAM_TOTAL, UNIGRAM_TOTAL_KEY,
    UserStoreError,
};

/// The persistence target a session store carries: the user dir, the
/// original system chunks (the `.dbin` diff base), the conformance
/// triple and the open counter `pinyin_save` re-writes.
#[derive(Clone, Debug)]
pub(crate) struct Target {
    /// The user directory holding the profile.
    pub(crate) dir: PathBuf,
    /// The system libraries by nibble, as loaded at open.
    pub(crate) originals: BTreeMap<u8, SystemLibrary>,
    /// This build's identity triple.
    pub(crate) versions: SystemVersions,
    /// The open counter recorded at `check_format` time.
    pub(crate) open_counter: u32,
}

impl From<PersistenceError> for UserStoreError {
    fn from(error: PersistenceError) -> Self {
        UserStoreError::Persistence(error.to_string())
    }
}

/// The session scratch file for `user_dir` — one per directory per
/// process, in the temp area, in the backend's own container.
fn scratch_path(user_dir: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    user_dir.hash(&mut hasher);
    let hash = hasher.finish();
    std::env::temp_dir().join(format!(
        "oxpinyin-user-{hash:016x}.{}",
        oxpinyin_store::DEFAULT_STORE_EXT
    ))
}

impl GenericUserStore<DefaultStore> {
    /// Open the user store on a libpinyin user directory: read the
    /// profile ([`crate::persistence::load`]), seed a session scratch
    /// store with its values, and carry the persistence target so
    /// [`GenericUserStore::save`] writes the pin's files back.
    ///
    /// A second live open of the same directory shares the session
    /// handle (one scratch, shared dirty flag), like [`UserStore::open`]
    /// does for a path.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the profile's `user.conf` cannot
    /// be re-written or the scratch store cannot be created. A corrupt
    /// or partial profile does not fail here — it degrades per-file,
    /// exactly as upstream's loader degrades.
    pub fn open_libpinyin(
        user_dir: &Path,
        originals: BTreeMap<u8, SystemLibrary>,
        versions: SystemVersions,
    ) -> Result<Self, UserStoreError> {
        let scratch = scratch_path(user_dir);
        let key = registry::registry_key(&scratch);
        let reg = registry::lock_registry();
        if let Some(inner) = reg.get(&key).and_then(std::sync::Weak::upgrade) {
            return Ok(Self::from_parts(inner, None));
        }
        drop(reg);

        let loaded = persistence::load(user_dir, &originals, &versions)?;

        let lease =
            Arc::new(registry::acquire_scratch(&scratch).ok_or(UserStoreError::AlreadyOpen)?);
        // A crash can leave the previous session's scratch behind; it is
        // not a store anyone re-opens, it is rebuilt from the profile.
        let _ = std::fs::remove_file(&scratch);
        let db = DefaultStore::create(&scratch)?;
        let has_user_data = db.write(|txn| {
            seed_txn(txn, &loaded.state, &originals)?;
            let total_rows = count_tables(txn)?;
            Ok(total_rows)
        })?;

        let inner = Arc::new(StoreInner {
            count_cache: std::sync::Mutex::new(None),
            db: std::sync::Mutex::new(db),
            dirty: std::sync::atomic::AtomicBool::new(false),
            write_generation: std::sync::atomic::AtomicU64::new(0),
            phrase_generation: std::sync::atomic::AtomicU64::new(0),
            has_user_data: std::sync::atomic::AtomicBool::new(has_user_data),
            libpinyin: Some(Target {
                dir: user_dir.to_path_buf(),
                originals,
                versions,
                open_counter: loaded.open_counter,
            }),
        });
        registry::lock_registry().insert(key, Arc::downgrade(&inner));
        Ok(Self::from_parts(inner, Some(lease)))
    }
}

/// `init_and_wrap`'s `has_user_data` probe, against a seeded scratch.
fn count_tables(txn: &mut dyn WriteTxn) -> Result<bool, StoreError> {
    Ok(!txn.is_empty(BIGRAM)? || !txn.is_empty(UNIGRAM)? || !txn.is_empty(PHRASE)?)
}

/// Writes the loaded profile's values into a fresh scratch store — the
/// seed half of the value mapping.
fn seed_txn(
    txn: &mut dyn WriteTxn,
    state: &UserState,
    originals: &BTreeMap<u8, SystemLibrary>,
) -> Result<(), StoreError> {
    let mut unigram_total = 0_u64;
    let mut alloc_cursor: BTreeMap<u8, Token> = BTreeMap::new();

    for (&nibble, items) in &state.libraries {
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
            for (keys, freq) in &item.prons {
                txn.put(
                    PRONUNCIATION,
                    &codec::encode_token_bytes(token, &packed_to_bytes(keys)),
                    &codec::encode_u64(u64::from(*freq)),
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
            let base = original.items.get(&slot).map_or(0, |old| old.unigram);
            let delta = u64::from(new_item.unigram.saturating_sub(base));
            if delta == 0 {
                continue;
            }
            let token = (u32::from(nibble) << 24) | slot;
            let key = codec::encode_token(token);
            let prev = txn_get_u64_or(txn, UNIGRAM, &key)?;
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
pub(crate) fn export_state<S: WriteStore>(
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

    let mut pronunciations: BTreeMap<Token, Vec<(Vec<u16>, u64)>> = BTreeMap::new();
    db.for_each(PRONUNCIATION, &mut |key, value| {
        let (token, key_bytes) = codec::decode_token_bytes(key)
            .map_err(|_| StoreError::Backend("corrupt pronunciation key".into()))?;
        let count = codec::decode_u64(value)
            .map_err(|_| StoreError::Backend("corrupt pronunciation count".into()))?;
        let packed = phrase::decode_keys(key_bytes)
            .iter()
            .map(|key| u16::from_le_bytes(key.to_le_bytes()))
            .collect();
        pronunciations
            .entry(token)
            .or_default()
            .push((packed, count));
        Ok(())
    })?;

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

    drop(db);

    let mut state = UserState::default();
    for (token, text) in &phrase_text {
        let Some(delta) = unigrams.get(token) else {
            continue;
        };
        let prons = pronunciations.get(token).map_or(Vec::new(), |rows| {
            rows.iter()
                .map(|(packed, count)| {
                    (
                        packed
                            .iter()
                            .map(|&bits| u16::from_le_bytes(bits.to_le_bytes()))
                            .collect::<Vec<_>>(),
                        u32::try_from(*count).unwrap_or(u32::MAX),
                    )
                })
                .collect()
        });
        let nibble = phrase_index_library_index(*token);
        state.libraries.entry(nibble).or_default().insert(
            token & crate::phrase::PHRASE_MASK,
            ChunkItem {
                phrase: text.clone(),
                unigram: u32::try_from(*delta).unwrap_or(u32::MAX),
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
        let Some(old) = original.items.get(&slot) else {
            continue;
        };
        let mut new_item = old.clone();
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
    Ok(state)
}

/// `store.rs`'s read helper, at crate visibility for the seed path.
fn txn_get_u64_or(txn: &dyn WriteTxn, table: &str, key: &[u8]) -> Result<u64, StoreError> {
    txn.get(table, key)?.map_or(Ok(0), |bytes| {
        codec::decode_u64(&bytes).map_err(|_| StoreError::Backend("corrupt u64".into()))
    })
}

/// UCS-4 code points as text; `None` when any scalar is invalid.
fn ucs4_to_string(codes: &[u32]) -> Option<String> {
    codes.iter().copied().map(char::from_u32).collect()
}

/// Packed `u16` key values as the byte wire form.
fn packed_to_bytes(keys: &[u16]) -> Vec<u8> {
    keys.iter().flat_map(|bits| bits.to_le_bytes()).collect()
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
        BTreeMap::from([(1_u8, SystemLibrary { total: 100, items })])
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
    fn train_save_reopen_carries_the_learning() {
        let dir = tempdir("cycle");
        let keys = [
            crate::phrase::PinyinKey::from_le_bytes(
                ChewingKey::new(23, 0, 1, 0).to_packed().to_le_bytes(),
            ),
            crate::phrase::PinyinKey::from_le_bytes(
                ChewingKey::new(7, 0, 13, 0).to_packed().to_le_bytes(),
            ),
        ];

        {
            let mut store = UserStore::open_libpinyin(&dir, originals(), versions()).expect("open");
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
            let store = UserStore::open_libpinyin(&dir, originals(), versions()).expect("reopen");
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
            assert_eq!(item.unigram, crate::phrase::DEFAULT_PHRASE_COUNT as u32 * 3);
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
        };
        persistence::save(&dir, &state, &originals(), &versions(), 1).expect("save");

        let store = UserStore::open_libpinyin(&dir, originals(), versions()).expect("open");
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

    #[test]
    fn a_second_open_shares_the_session_handle() {
        let dir = tempdir("share");
        let first = UserStore::open_libpinyin(&dir, originals(), versions()).expect("open 1");
        let second = UserStore::open_libpinyin(&dir, originals(), versions()).expect("open 2");
        let mut second = second;
        second.observe_selection(1, 0x0100_0001).expect("train");
        // The shared handle sees it: one scratch, one dirty flag.
        assert_eq!(first.bigram_count(1, 0x0100_0001).expect("count"), 69);

        drop(first);
        drop(second);
        // The scratch file dies with the last handle; the profile is
        // whatever was last saved (nothing here).
        let scratch = scratch_path(&dir);
        assert!(!scratch.exists(), "scratch survived the last handle");

        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
