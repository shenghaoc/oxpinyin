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
//! * bigram — not a value of the session store at all: the session keeps
//!   the pin's own in-memory container, filled from `user_bigram.db` once
//!   by `load_db` and written back by `save_db`, and every bigram
//!   operation runs the pin's `SingleGram` code over its raw grams
//!   ([`crate::store::BIGRAM_GRAM`] is the plain stores' equivalent).
//! * user phrases — a `user.bin`/`addon.bin` item is the `PHRASE` text,
//!   the `PRONUNCIATION` rows (packed keys — one wire form in both
//!   schemas) and the `UNIGRAM` row: upstream's item `unigram` field is
//!   exactly this store's full accumulation for the token (`count·3` at
//!   `_add_phrase`, `seed·7` per training, the promotion unigram).
//! * system tokens — a `.dbin` MODIFY record is the original item with
//!   `unigram + UNIGRAM[token]`; the value model tracks no per-token
//!   removal (a replayed REMOVE degrades to a skip, noted at load). The
//!   session holds a replayed item whole, readings included, in its live
//!   system-item view, and that item is where its readings' counts are read:
//!   opening writes no `PRONUNCIATION` row for it. Training writes the
//!   exact-reading delta over the original for the readings it moves (a row
//!   is that delta, kept for the tables that probe it), and the live view
//!   takes the new count when training commits, so lookup and export observe
//!   it before save.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oxpinyin_data::chunk_write::ChunkItem;
use oxpinyin_data::pin_stderr;
use oxpinyin_data::user_files::SystemVersions;
use oxpinyin_store::{
    DefaultStore, DefaultUserBigramDb, StoreError, UserBigramDb, WriteStore, WriteTxn,
};

use crate::codec;
use oxpinyin_data::row_format::pinyin_index::PinyinIndexItem;

use crate::persistence::{
    self, IndexKeys, PersistenceError, SystemOriginals, UserConfLaw, UserState,
};
use crate::phrase::{self, phrase_index_library_index};
use crate::registry::StoreInner;
use crate::store::{
    ALLOC, ALLOC_CURSOR, GenericUserStore, LIBRARY_OFFSET, PHRASE, PHRASE_BY_LIB_TEXT,
    PHRASE_BY_TEXT, PRONUNCIATION, PronValue, SYSTEM_BASE, Token, UNIGRAM, UNIGRAM_TOTAL,
    UNIGRAM_TOTAL_KEY, UserStoreError,
};

/// The user pinyin index's `(token, packed keys)` entries that no item
/// reading backs, by the token's library nibble ([`Target::kept_index`]).
pub(crate) type KeptIndex = std::collections::BTreeMap<u8, BTreeSet<(u32, Vec<u16>)>>;

/// Keep the stored count even when an item body cannot be decoded. The pin's
/// count getter checks only the two header bytes, not the text or keys.
pub(crate) fn loaded_pronunciation_counts(
    state: &UserState,
    dir: Option<&Path>,
    layout: &oxpinyin_data::user_files::UserFileLayout,
    only: Option<u8>,
) -> BTreeMap<Token, u32> {
    let mut counts = BTreeMap::new();
    for (library, file) in layout.user_libraries() {
        let library = *library;
        if only.is_some_and(|only| only != library) {
            continue;
        }
        if let Some(items) = state.libraries.get(&library) {
            counts.extend(
                items.iter().map(|(&slot, item)| {
                    ((u32::from(library) << 24) | slot, item.prons.len() as u32)
                }),
            );
        } else if let Some(chunk) = dir.and_then(|dir| {
            oxpinyin_data::phrase_library::PhraseLibrary::open(&file.under(dir)).ok()
        }) {
            // The value loader can reject a body the pin never reads here.
            for slot in 0..chunk.token_range().end {
                if let Some(count) = chunk.pronunciation_count(slot) {
                    counts.insert((u32::from(library) << 24) | slot, count);
                }
            }
        }
    }
    counts
}

/// The persistence target a session store carries: the user dir, the
/// original system chunks (the `.dbin` diff base), the conformance
/// triple, and the `user.conf` law and open counter the session's saves
/// and its fini write.
#[derive(Debug)]
pub struct Target {
    /// The user directory holding the profile.
    pub(crate) dir: Option<PathBuf>,
    /// The system libraries by nibble, refreshed when mask_out reloads them.
    pub(crate) originals: std::sync::RwLock<SystemOriginals>,
    /// The live USER_FILE items' stored counts. Writers publish after commit.
    pub(crate) pronunciation_counts: std::sync::Mutex<BTreeMap<Token, u32>>,
    /// This build's identity triple.
    pub(crate) versions: SystemVersions,
    /// Whose `user.conf` lifecycle the profile follows.
    pub(crate) law: UserConfLaw,
    /// The open counter the session's `user.conf` writes start from
    /// ([`persistence::Loaded::open_counter`]).
    pub(crate) open_counter: i32,
    /// The user pinyin index's readings past `MAX_PHRASE_LENGTH` syllables
    /// (`user_pinyin_index.bin`), as packed `ChewingKey` words — the pin's
    /// `ChewingLargeTable2` user table aborts on them at
    /// `chewing_large_table2_bdb.cpp:282`/`:529`, and the value model
    /// cannot carry them, so the two abort sites read them here
    /// ([`persistence::LoadedProfile::overlong_index_keys`]).
    pub(crate) overlong_index_keys: Vec<Vec<u16>>,
    /// Every raw key `user_pinyin_index.bin` carried, as packed
    /// `ChewingKey` words — the exact query key the pin's `m_db->Get` /
    /// `DB_SET` probe tests before it walks extensions
    /// ([`persistence::LoadedProfile::index_keys`]).
    pub(crate) index_keys: BTreeSet<Vec<u16>>,
    /// Every retained key the session's user pinyin index holds
    /// ([`persistence::IndexKeys`]): the loaded files' keys, plus the keys
    /// of every row a mask or a removal took out of the store since. The
    /// save writes back the ones no remaining row writes, empty.
    pub(crate) index_file_keys: std::sync::Mutex<IndexKeys>,
    /// A system journal's ADD payload was shorter than its unigram; the
    /// replay stopped at that record and the facade logs one warning
    /// (class (c), register row 108).
    pub(crate) short_add_payload: bool,
    /// The entries the user pinyin index holds without an item reading
    /// behind them: those of a dropped item (the pin's index tables keep
    /// them when `unload` frees the sub-index, `phrase_index.cpp:261-268`)
    /// and those a profile's `user_pinyin_index.bin` carries for a token
    /// whose item lacks the reading. They are found on whatever item holds
    /// the token, written by a save, and removed by a `mask_out`.
    pub(crate) kept_index: std::sync::Mutex<KeptIndex>,
}

impl Target {
    pub(crate) fn originals(&self) -> std::sync::RwLockReadGuard<'_, SystemOriginals> {
        self.originals
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn index_file_keys(&self) -> std::sync::MutexGuard<'_, IndexKeys> {
        self.index_file_keys
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// The pinyin index rows the save would write for some stored phrases,
/// read before a mask or
/// a removal drops them, so their keys stay in the session's tables
/// ([`Target::index_file_keys`]).
#[derive(Debug, Default)]
pub(crate) struct IndexRows {
    pinyin: Vec<PinyinIndexItem>,
}

impl IndexRows {
    /// The rows of every phrase `select` picks, as [`export_state`] turns
    /// them into the save's rows: each reading
    /// whose `indexed` flag is set, in packed `ChewingKey` words.
    pub(crate) fn read(
        txn: &impl WriteTxn,
        select: impl Fn(Token) -> bool,
    ) -> Result<Self, StoreError> {
        let mut rows = Self::default();
        let mut phrases: BTreeSet<Token> = BTreeSet::new();
        txn.for_each(PHRASE, |key, _value| {
            let token = codec::decode_token(key)
                .map_err(|_| StoreError::Backend("corrupt phrase token".into()))?;
            if select(token) {
                phrases.insert(token);
            }
            Ok(())
        })?;
        if phrases.is_empty() {
            return Ok(rows);
        }
        txn.for_each(PRONUNCIATION, |key, value| {
            let (token, key_bytes) = codec::decode_token_bytes(key)
                .map_err(|_| StoreError::Backend("corrupt pronunciation key".into()))?;
            if phrases.contains(&token) && PronValue::decode(value)?.indexed {
                rows.push_reading(token, key_bytes);
            }
            Ok(())
        })?;
        Ok(rows)
    }

    /// The rows of the one phrase `token`, given its text and readings.
    pub(crate) fn of_phrase(token: Token, readings: &[crate::store::PronRow]) -> Self {
        let mut rows = Self::default();
        for reading in readings.iter().filter(|reading| reading.value.indexed) {
            rows.push_reading(token, &reading.key_bytes);
        }
        rows
    }

    /// A stored reading's dense syllable ids as the save's packed row; a
    /// reading [`export_state`] cannot convert writes no row and is skipped.
    fn push_reading(&mut self, token: Token, key_bytes: &[u8]) {
        if let Some(packed) = pinyin_keys_to_packed(&phrase::decode_keys(key_bytes)) {
            self.pinyin.push(PinyinIndexItem {
                token,
                keys: packed
                    .iter()
                    .map(|&bits| oxpinyin_core::ChewingKey::from_packed(bits))
                    .collect(),
            });
        }
    }

    /// The rows' readings, in the order the pin removes them (#785).
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    pub(crate) fn readings(&self) -> impl Iterator<Item = &[oxpinyin_core::ChewingKey]> {
        self.pinyin.iter().map(|item| item.keys.as_slice())
    }

    /// Adds the rows' keys to `keys`.
    pub(crate) fn keep_in(&self, keys: &mut IndexKeys) {
        keys.keep_rows(&self.pinyin);
    }
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
            let Some(dir) = target.dir.as_deref() else {
                // mark_version still attempts the empty filename at NULL
                // (074a2219 pinyin.cpp:1194-1200); reproduce its diagnostic
                // without issuing filesystem operations.
                if target.law == UserConfLaw::Pinyin {
                    pin_stderr::emit(&[b"write  failed.\n"]);
                }
                return;
            };
            // `pinyin_fini` ignores `mark_version`'s result
            // (`pinyin.cpp:1200`): a fini has no caller to answer, only the
            // line `UserTableInfo::save` prints (`table_info.cpp:382`).
            if persistence::fini(dir, &target.versions, target.law, target.open_counter).is_err() {
                pin_stderr::emit(&[
                    b"write ",
                    pin_stderr::path_bytes(&dir.join("user.conf")),
                    b" failed.\n",
                ]);
            }
        }
    }
}

impl From<PersistenceError> for UserStoreError {
    fn from(error: PersistenceError) -> Self {
        // The two class-(c) sites keep their own variants: neither is a
        // degraded store — one is a failed open (see `persistence::load`),
        // the other a failed save the facade must log.
        match error {
            PersistenceError::UnknownDatabaseFormat => UserStoreError::UnknownDatabaseFormat,
            PersistenceError::ChunkHeaderWrite { field, .. } => {
                UserStoreError::ChunkHeaderWrite(field)
            }
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
        originals: impl Into<SystemOriginals>,
        versions: SystemVersions,
        law: UserConfLaw,
    ) -> Result<Self, UserStoreError> {
        Self::open_session(Some(user_dir), originals, versions, law, None)
    }

    /// Open the pin's NULL-user-directory session entirely in memory.
    /// Imports and lookup work; no profile is read, written or removed.
    /// The failed empty-filename marker diagnostics follow `law`.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] if an in-memory container cannot be created
    /// or seeded with the system libraries' allocation bases.
    pub fn open_transient(
        originals: impl Into<SystemOriginals>,
        versions: SystemVersions,
        law: UserConfLaw,
    ) -> Result<Self, UserStoreError> {
        Self::open_session(None, originals, versions, law, None)
    }

    /// [`UserStore::open_libpinyin`] for a profile [`UserStore::check_profile`]
    /// already judged: the profile is read, not judged again.
    ///
    /// # Errors
    ///
    /// As [`UserStore::open_libpinyin`], less the `user.conf` judgement.
    pub fn open_libpinyin_checked(
        user_dir: &Path,
        originals: impl Into<SystemOriginals>,
        versions: SystemVersions,
        law: UserConfLaw,
        check: crate::persistence::ProfileCheck,
    ) -> Result<Self, UserStoreError> {
        Self::open_session(Some(user_dir), originals, versions, law, Some(check))
    }

    /// `check_format` alone, for an init that must judge the profile before
    /// it loads the system libraries, as the pin does: the `user.conf`
    /// diagnostic, the wipe of a non-conform profile and the marker write.
    ///
    /// # Errors
    ///
    /// [`UserStoreError::UnknownDatabaseFormat`] at the abort point, or the
    /// marker write failing.
    pub fn check_profile(
        user_dir: &Path,
        versions: &SystemVersions,
        layout: &oxpinyin_data::user_files::UserFileLayout,
        law: UserConfLaw,
    ) -> Result<crate::persistence::ProfileCheck, UserStoreError> {
        Ok(persistence::check_format(user_dir, versions, layout, law)?)
    }

    fn open_session(
        user_dir: Option<&Path>,
        originals: impl Into<SystemOriginals>,
        versions: SystemVersions,
        law: UserConfLaw,
        checked: Option<crate::persistence::ProfileCheck>,
    ) -> Result<Self, UserStoreError> {
        let originals: SystemOriginals = originals.into();
        // Create the session's own container before touching the
        // profile, so a failure here cannot follow a `check_format` that
        // already raised the open counter and wiped a non-conforming
        // profile. It is this session's alone: process memory, shared
        // with no other open and no other process.
        let db = DefaultStore::create_in_memory()?;

        let profile = if let Some(dir) = user_dir {
            match checked {
                Some(check) => persistence::load_checked(dir, &originals, &check),
                None => persistence::load_profile(dir, &originals, &versions, law)?,
            }
        } else {
            // check_format reads ""; pinyin also tries to write "".
            // Neither operation can succeed. Do not redirect into cwd/temp.
            pin_stderr::emit(&[b"open  failed."]);
            if law == UserConfLaw::Pinyin {
                pin_stderr::emit(&[b"write  failed.\n"]);
            }
            persistence::LoadedProfile::default()
        };
        // A non-null-`MODIFY_HEADER` token is the pin's
        // `assert(token == null_token)` during the library load's `merge`
        // (`phrase_index_logger.h:202`): the session open fails here, with
        // the profile's open counter left raised as the pin leaves it.
        if profile.loaded.strict_log_fault.is_some() {
            return Err(UserStoreError::SystemLogHeaderToken);
        }
        let persistence::LoadedProfile {
            mut loaded,
            overlong_index_keys,
            index_keys,
            index_file_keys,
        } = profile;
        // Armed as soon as the load has raised the counter: an open that
        // fails from here on drops it and lowers the counter again, so a
        // failed open reads as a finished session, not a crash.
        let kept_index = unbacked_index(&loaded.state, &originals);
        let pronunciation_counts =
            loaded_pronunciation_counts(&loaded.state, user_dir, originals.layout(), None);
        let target = Arc::new(Target {
            dir: user_dir.map(Path::to_path_buf),
            originals: std::sync::RwLock::new(originals),
            pronunciation_counts: std::sync::Mutex::new(pronunciation_counts),
            versions,
            law,
            open_counter: loaded.open_counter,
            short_add_payload: loaded.short_add_payload,
            overlong_index_keys,
            index_keys,
            index_file_keys: std::sync::Mutex::new(index_file_keys),
            kept_index: std::sync::Mutex::new(kept_index),
        });
        let fini = FiniGuard(Some(Arc::clone(&target)));
        let (has_user_data, library_offsets) = db.write(|txn| {
            let offset = seed_txn(
                txn,
                &loaded.state,
                &target.originals(),
                &loaded.library_totals,
            )?;
            Ok((count_tables(txn)?, offset))
        })?;
        // `m_user_bigram->load_db(user_bigram.db)` (`pinyin.cpp:399-401`):
        // the user bigram's one read, after the profile check, so a wiped
        // profile loads empty. Any key counts as user data, an item-less
        // gram included: the pin merges its total (`ngram.cpp:298-306`).
        // The flag only gates a fast path, so a profile that has the file
        // takes the full read without a walk of the container here.
        let (bigram_db, has_bigram_file) = match target.dir.as_deref() {
            Some(dir) => (
                persistence::load_user_bigram_db(dir)?,
                dir.join(oxpinyin_data::user_files::UserDbm::Bigram.file_name())
                    .exists(),
            ),
            None => (DefaultUserBigramDb::empty()?, false),
        };
        let has_user_data = has_user_data || has_bigram_file;

        // The seed and the kept index are done with the loaded overrides, which
        // the session now owns: move them into the live map rather than
        // cloning every item and dropping the originals with `loaded`.
        let has_system_items = !loaded.state.system_overrides.is_empty();
        // One pass over the replayed overrides: an item is kept, a removal has
        // none to keep and the session records its token.
        let mut kept_items = Vec::new();
        let mut removed_system_items = std::collections::BTreeSet::new();
        for (nibble, items) in std::mem::take(&mut loaded.state.system_overrides) {
            for (slot, item) in items {
                let token = (u32::from(nibble) << 24) | slot;
                match item {
                    Some(item) => kept_items.push((token, item)),
                    None => {
                        removed_system_items.insert(token);
                    }
                }
            }
        }
        let system_items = kept_items.into_iter().collect();

        let inner = Arc::new(StoreInner {
            system_bumps_live: std::sync::atomic::AtomicBool::new(false),
            system_bumps: std::sync::Mutex::new(crate::registry::SystemUnigramBumps::default()),
            has_system_items: std::sync::atomic::AtomicBool::new(has_system_items),
            system_items: std::sync::Mutex::new(system_items),
            has_removed_system_items: std::sync::atomic::AtomicBool::new(
                !removed_system_items.is_empty(),
            ),
            removed_system_items: std::sync::Mutex::new(removed_system_items),
            count_cache: std::sync::Mutex::new(None),
            db: std::sync::Mutex::new(db),
            dirty: std::sync::atomic::AtomicBool::new(false),
            write_generation: std::sync::atomic::AtomicU64::new(0),
            phrase_generation: std::sync::atomic::AtomicU64::new(0),
            phrase_table_generation: std::sync::atomic::AtomicU64::new(0),
            #[cfg(test)]
            fail_replace: std::sync::atomic::AtomicBool::new(false),
            has_user_data: std::sync::atomic::AtomicBool::new(has_user_data),
            facade_offset: std::sync::atomic::AtomicU32::new(
                library_offsets
                    .values()
                    .fold(0_u32, |sum, &offset| sum.wrapping_add(offset)),
            ),
            has_library_offsets: std::sync::atomic::AtomicBool::new(!library_offsets.is_empty()),
            bigram_db: Some(std::sync::Mutex::new(bigram_db)),
            libpinyin: Some(target),
        });
        Ok(Self::from_parts(inner, None, fini))
    }
}

/// `init_and_wrap`'s `has_user_data` probe, against a seeded session
/// store; the user bigram lives in the container, which the caller asks.
fn count_tables(txn: &mut impl WriteTxn) -> Result<bool, StoreError> {
    Ok(!txn.is_empty(UNIGRAM)? || !txn.is_empty(PHRASE)?)
}

/// The `(token, packed keys)` readings the items of `state` carry: the
/// user libraries' items and the items added to a system library.
pub(crate) fn backed_readings(
    state: &UserState,
    originals: &SystemOriginals,
) -> BTreeSet<(u32, Vec<u16>)> {
    let mut backed = BTreeSet::new();
    let mut add = |nibble: u8, slot: u32, item: &ChunkItem| {
        let token = (u32::from(nibble) << 24) | slot;
        backed.extend(item.prons.iter().map(|(packed, _)| (token, packed.clone())));
    };
    for (&nibble, items) in &state.libraries {
        for (&slot, item) in items {
            add(nibble, slot, item);
        }
    }
    for (&nibble, overrides) in &state.system_overrides {
        for (&slot, item) in overrides {
            let appended = !originals
                .get(&nibble)
                .is_some_and(|original| original.contains_item(slot));
            if let (true, Some(item)) = (appended, item) {
                add(nibble, slot, item);
            }
        }
    }
    backed
}

/// The index entries of `state` that no item reading backs, by library.
pub(crate) fn unbacked_index(state: &UserState, originals: &SystemOriginals) -> KeptIndex {
    let backed = backed_readings(state, originals);
    let mut kept = KeptIndex::new();
    for entry in state.indexed.difference(&backed) {
        kept.entry(phrase_index_library_index(entry.0))
            .or_default()
            .insert(entry.clone());
    }
    kept
}

/// Writes the loaded profile's values into a fresh session store — the
/// seed half of the value mapping.
///
/// A replayed MODIFY of a shipped item seeds its unigram delta and no reading
/// row: the session holds the item whole (`StoreInner::system_items`) and its
/// readings' counts are read there, as a list of entries.
///
/// `state.phrase_table` is the pin's raw `m_phrase_table` read from
/// `user_phrase_index.bin` ([`UserState::phrase_table`]). It is seeded verbatim,
/// not derived from `state.libraries`, so a profile whose phrase index
/// disagrees with `user.bin` keeps the disagreement and
/// `pinyin_remove_user_candidate` reproduces the pin's `:3750` assert
/// against it. Nothing else reads the rows.
///
/// `library_totals` is the total each loaded library holds in the pin
/// ([`persistence::Loaded`]'s `library_totals`). Answers, per library, how
/// far that total lies from the one the seeded rows make — the library's
/// original total plus its seeded unigrams — as a wrapping `guint32`: the
/// facade keeps that difference until the library is loaded again, since
/// `compact` rebuilds the libraries' totals and never the facade's
/// (074a2219 `phrase_index.cpp:648-675`).
pub(crate) fn seed_txn(
    txn: &mut impl WriteTxn,
    state: &UserState,
    originals: &SystemOriginals,
    library_totals: &BTreeMap<u8, u32>,
) -> Result<BTreeMap<u8, u32>, StoreError> {
    let mut unigram_total = 0_u64;
    // The unigrams seeded per library, as the facade would sum them.
    let mut seeded: BTreeMap<u8, u32> = BTreeMap::new();
    let mut alloc_cursor: BTreeMap<u8, Token> = BTreeMap::new();

    // ADD records own text/index rows just like USER_FILE items. MODIFY
    // records do not gain index rows (pinyin.cpp:566-607 at 074a2219).
    let mut libraries = state.libraries.clone();
    for (&nibble, original) in originals.iter() {
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
            let sum = seeded.entry(nibble).or_insert(0);
            *sum = sum.wrapping_add(item.unigram);
            let cursor = alloc_cursor.entry(nibble).or_insert(0);
            if token > *cursor {
                *cursor = token;
            }
        }
    }

    // load_db copies records without normalizing tokens or recreating prefixes.
    for (key, value) in &state.phrase_table {
        txn.put(crate::phrase_table::TABLE, key, value)?;
    }

    // System-token deltas: the training mass on top of the originals. Only
    // the unigram is a stored delta here (the count reads and the library
    // totals consult it); the readings' counts stay in the live items.
    for (&nibble, overrides) in &state.system_overrides {
        let Some(original) = originals.get(&nibble) else {
            continue;
        };
        for (&slot, item) in overrides {
            let Some(new_item) = item else {
                continue; // a replayed removal: not expressible in the
                // value model; the load's `skipped` list carries it
            };
            if !original.is_readable(slot) {
                continue;
            }
            // The replayed readings write no row: the session's live item
            // ([`StoreInner::system_items`]) holds their counts. Training
            // writes the row for the readings it moves, as the delta over the
            // original.
            let token = (u32::from(nibble) << 24) | slot;
            // `is_readable` held, so the item exists and this is its unigram.
            let base = original.unigram(slot).unwrap_or(0);
            let delta = u64::from(new_item.unigram.wrapping_sub(base));
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
            let sum = seeded.entry(nibble).or_insert(0);
            *sum = sum.wrapping_add(delta as u32);
        }
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
    let offsets: BTreeMap<u8, u32> = library_totals
        .iter()
        .map(|(&nibble, &total)| {
            let base = originals.get(&nibble).map_or(0, |original| original.total);
            let modelled = base.wrapping_add(seeded.get(&nibble).copied().unwrap_or(0));
            (nibble, total.wrapping_sub(modelled))
        })
        .filter(|&(_, offset)| offset != 0)
        .collect();
    for (&nibble, &offset) in &offsets {
        txn.put(
            LIBRARY_OFFSET,
            &codec::encode_u8(nibble),
            &codec::encode_u64(u64::from(offset)),
        )?;
    }
    Ok(offsets)
}

/// Reads the session values back into the persistence shape — the
/// export half of the value mapping, [`GenericUserStore::save`]'s
/// libpinyin branch.
pub fn export_state<S: WriteStore>(
    store: &GenericUserStore<S>,
    originals: &SystemOriginals,
) -> Result<UserState, UserStoreError> {
    store.flush_system_unigrams()?;
    let db = store.database();

    let mut phrase_text: BTreeMap<Token, Vec<u32>> = BTreeMap::new();
    db.for_each(PHRASE, |key, value| {
        let token = codec::decode_token(key)
            .map_err(|_| StoreError::Backend("corrupt phrase token".into()))?;
        let text = codec::decode_str(value)
            .map_err(|_| StoreError::Backend("corrupt phrase text".into()))?;
        phrase_text.insert(token, text.chars().map(u32::from).collect());
        Ok(())
    })?;

    let mut pronunciations: BTreeMap<Token, Vec<StoredReading>> = BTreeMap::new();
    db.for_each(PRONUNCIATION, |key, value| {
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
    db.for_each(UNIGRAM, |key, value| {
        let token = codec::decode_token(key)
            .map_err(|_| StoreError::Backend("corrupt unigram key".into()))?;
        let delta = codec::decode_u64(value)
            .map_err(|_| StoreError::Backend("corrupt unigram value".into()))?;
        unigrams.insert(token, delta);
        Ok(())
    })?;

    let system_items = store.stored_system_items();
    let removed_items = store.stored_removed_system_items();
    let mut state = UserState::default();
    state.indexed.extend(store.kept_index_entries());
    db.for_each(crate::phrase_table::TABLE, |key, value| {
        state.phrase_table.insert(key.to_vec(), value.to_vec());
        Ok(())
    })?;
    drop(db);
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
        new_item.unigram = old.unigram.wrapping_add(*delta as u32);
        state
            .system_overrides
            .entry(nibble)
            .or_default()
            .insert(slot, Some(new_item));
    }

    for (token, mut item) in system_items {
        let nibble = phrase_index_library_index(token);
        let slot = token & crate::phrase::PHRASE_MASK;
        let base = originals
            .get(&nibble)
            .and_then(|original| original.unigram(slot))
            .unwrap_or(0);
        item.unigram = base.wrapping_add(unigrams.get(&token).copied().unwrap_or(0) as u32);
        // The live item is the authority for its readings' counts, as it is
        // for pricing: replay, an import and training each leave it holding
        // the count the save writes. The rows training writes are the delta
        // over the original, kept for the tables that probe them; the entries
        // are a list, so a count is never rebuilt from a row found by keys.
        state
            .system_overrides
            .entry(nibble)
            .or_default()
            .insert(slot, Some(item));
    }
    // A removal is recorded last: it wins over any override a slot gathered
    // from the rows, as the sub-index no longer holds the item to modify.
    for token in removed_items {
        state
            .system_overrides
            .entry(phrase_index_library_index(token))
            .or_default()
            .insert(token & crate::phrase::PHRASE_MASK, None);
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
pub(crate) fn packed_to_pinyin_keys(packed: &[u16]) -> Option<Vec<crate::phrase::PinyinKey>> {
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
    use crate::persistence::SystemLibrary;
    use crate::store::UserStore;
    use oxpinyin_core::ChewingKey;

    fn originals() -> SystemOriginals {
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
        .into()
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

    /// Opens a profile whose `gb_char.dbin` holds `records` over [`originals`].
    fn open_with_log(
        name: &str,
        records: &[oxpinyin_data::user_files::LogRecord],
    ) -> (PathBuf, UserStore) {
        let dir = tempdir(name);
        let payload = oxpinyin_data::user_files::encode_log_records(records).expect("log");
        std::fs::write(
            dir.join("gb_char.dbin"),
            oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("chunk"),
        )
        .expect("write log");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("open");
        (dir, store)
    }

    #[test]
    fn session_owns_the_replayed_overrides_it_was_opened_with() {
        use oxpinyin_data::chunk_write::encode_phrase_item;
        use oxpinyin_data::user_files::LogRecord;
        let original = ChunkItem {
            phrase: vec![0x4f60],
            unigram: 100,
            prons: vec![(vec![ChewingKey::new(23, 0, 1, 0).to_packed()], 100)],
        };
        let mut modified = original.clone();
        modified.prons[0].1 += 7;
        let modify = [
            LogRecord::ModifyHeader {
                old_total: 100,
                new_total: 100,
            },
            LogRecord::Modify {
                token: 1,
                old_item: encode_phrase_item(&original).expect("old"),
                new_item: encode_phrase_item(&modified).expect("new"),
            },
        ];
        let (dir, mut store) = open_with_log("owned-overrides-modify", &modify);
        let token = (1_u32 << 24) | 1;
        let live = store.stored_system_items();
        assert_eq!(live.len(), 1);
        assert_eq!(live.get(&token), Some(&modified));
        assert!(store.has_system_items());
        // Saved (a clean session writes nothing) and reopened, the session
        // holds the same overrides.
        store.save().expect("save");
        drop(store);
        let reopened =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("reopen");
        assert_eq!(reopened.stored_system_items(), live);
        assert!(reopened.has_system_items());
        drop(reopened);
        std::fs::remove_dir_all(dir).expect("cleanup");

        // A replayed removal is an override with no payload: the library
        // counts as overridden, and there is no item to keep.
        let remove = [
            LogRecord::ModifyHeader {
                old_total: 100,
                new_total: 0,
            },
            LogRecord::Remove {
                token: 1,
                old_item: encode_phrase_item(&original).expect("old"),
            },
        ];
        let (dir, store) = open_with_log("owned-overrides-remove", &remove);
        assert!(store.stored_system_items().is_empty());
        assert!(store.has_system_items());
        // The session remembers the removal: the shipped chunk still holds
        // the item, but `get_phrase_item` no longer finds it.
        assert!(store.has_removed_system_items());
        assert!(store.system_item_removed(token));
        assert!(!store.system_item_removed(token + 1));
        // What a save diffs from the session is the removal, not the item.
        let state = export_state(&store, &originals()).expect("export");
        assert_eq!(state.system_overrides[&1].get(&1), Some(&None));
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");

        // No journal, no overrides.
        let dir = tempdir("owned-overrides-none");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("open");
        assert!(store.stored_system_items().is_empty());
        assert!(!store.has_system_items());
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn partial_bigram_reads_complete_items_and_refuses_unsafe_mask() {
        let dir = tempdir("partial-gram-mask");
        let key = 1_u32.to_le_bytes();
        let mut value = 7_u32.to_le_bytes().to_vec();
        value.extend_from_slice(&9_u32.to_le_bytes());
        value.extend_from_slice(&7_u32.to_le_bytes());
        value.extend_from_slice(&[0xa5, 0x5a, 0x01]);
        DefaultStore::write_user_bigram(
            &dir.join("user_bigram.db"),
            &[(key.to_vec(), value.clone())],
        )
        .expect("write gram");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");

        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        assert_eq!(store.bigram_total(1).expect("total"), 7);
        assert_eq!(store.bigram_count(1, 9).expect("complete item"), 7);
        assert_eq!(store.bigram_count(1, 10).expect("absent item"), 0);
        assert!(store.has_oob_bigram_value_for(1));
        assert!(!store.has_empty_bigram_gram_for(1));
        assert!(matches!(
            store.mask_out(u32::MAX, 0),
            Err(UserStoreError::ShortUserBigramValue)
        ));
        assert_eq!(store.bigram_total(1).expect("unchanged total"), 7);
        assert_eq!(store.bigram_count(1, 9).expect("unchanged item"), 7);
        let raw = persistence::load_user_bigram_db(&dir).expect("raw container");
        assert_eq!(raw.get(&key).expect("raw get"), Some(value));

        drop(raw);
        // Deleting the predecessor wholesale never loads its value.
        store.mask_out(u32::MAX, 1).expect("whole-key mask");
        assert_eq!(store.bigram_total(1).expect("masked total"), 0);
        assert_eq!(store.bigram_count(1, 9).expect("masked item"), 0);
        // Raw Remove may have different backend results. The helper must
        // immediately reflect whichever value the in-memory DBM kept.
        let live_after_mask = store.has_oob_bigram_value_for(1);
        // This fixture explicitly exercises serialization after the raw mask;
        // the reporting helper now enforces the pin's modified guard too.
        store.mark_modified();
        let report = store.save_reporting().expect("save masked gram");
        assert!(report.renames_failed.is_empty());
        assert!(report.user_conf_write_failed.is_none());
        let raw = persistence::load_user_bigram_db(&dir).expect("raw masked container");
        let persisted_after_mask = raw.get(&key).expect("raw masked get");
        assert_eq!(
            live_after_mask,
            persisted_after_mask
                .as_ref()
                .is_some_and(|bytes| bytes.len() >= 4 && !(bytes.len() - 4).is_multiple_of(8))
        );
        drop(raw);
        drop(store);
        let itemless = [7_u32.to_le_bytes().as_slice(), &[0xa5, 0x5a]].concat();
        DefaultStore::write_user_bigram(
            &dir.join("user_bigram.db"),
            &[(key.to_vec(), itemless.clone())],
        )
        .expect("replace gram");
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("reopen");
        assert_eq!(store.bigram_total(1).expect("itemless total"), 7);
        assert_eq!(store.bigram_count(1, 9).expect("itemless count"), 0);
        assert!(store.has_empty_bigram_gram_for(1));
        assert!(store.has_oob_bigram_value_for(1));
        let raw = persistence::load_user_bigram_db(&dir).expect("raw itemless container");
        assert_eq!(raw.get(&key).expect("raw itemless get"), Some(itemless));
        drop(raw);
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn training_over_a_partial_tail_refuses_and_keeps_the_raw_value() {
        let dir = tempdir("partial-gram-train");
        let key = 1_u32.to_le_bytes();
        let mut value = 7_u32.to_le_bytes().to_vec();
        value.extend_from_slice(&9_u32.to_le_bytes());
        value.extend_from_slice(&7_u32.to_le_bytes());
        value.extend_from_slice(&[0xa5, 0x5a, 0x01]);
        DefaultStore::write_user_bigram(
            &dir.join("user_bigram.db"),
            &[(key.to_vec(), value.clone())],
        )
        .expect("write gram");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");

        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        assert!(store.has_oob_bigram_value_for(1));
        for refused in [
            store.observe_selection(1, 0x0100_0001),
            store.observe_predicted(1, 0x0100_0001),
        ] {
            assert!(matches!(refused, Err(UserStoreError::ShortUserBigramValue)));
        }
        // Nothing moved: the decoded counts are as loaded.
        assert_eq!(store.bigram_total(1).expect("total"), 7);
        assert_eq!(store.bigram_count(1, 9).expect("item"), 7);
        assert_eq!(store.bigram_count(1, 0x0100_0001).expect("cur"), 0);
        // A predecessor without a partial tail still trains, both ways.
        store
            .observe_selection(2, 0x0100_0001)
            .expect("clean predecessor trains");
        store
            .observe_predicted(2, 0x0100_0001)
            .expect("clean predecessor accepts a predicted candidate");
        store.save_reporting().expect("save");
        drop(store);

        let raw = persistence::load_user_bigram_db(&dir).expect("raw container");
        assert_eq!(raw.get(&key).expect("raw get"), Some(value.clone()));
        drop(raw);
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("reopen");
        assert!(store.has_oob_bigram_value_for(1));
        assert_eq!(store.bigram_total(1).expect("reopened total"), 7);
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// N2: both training paths read the predecessor's total first
    /// (`phonetic_lookup.h:884`, `pinyin.cpp:2630` at the pin `074a2219`),
    /// and a value too short for one is the pin's assert
    /// (`memory_chunk.h:390`). Both refuse before anything changes: the
    /// predicted path's unigram add, which the pin makes first
    /// (`pinyin.cpp:2609`), dies with the process there, so it is not
    /// made either.
    #[test]
    fn a_short_predecessor_value_refuses_both_training_paths() {
        let dir = tempdir("short-gram-train");
        let short = vec![0xa5, 0x5a, 0x01];
        DefaultStore::write_user_bigram(
            &dir.join("user_bigram.db"),
            &[(1_u32.to_le_bytes().to_vec(), short.clone())],
        )
        .expect("write gram");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        let generation = store.generation();
        let total = store.unigram_total().expect("unigram total");
        for refused in [
            store.observe_selection(1, 0x0100_0001),
            store.observe_predicted(1, 0x0100_0001),
        ] {
            assert!(matches!(refused, Err(UserStoreError::ShortUserBigramValue)));
        }
        assert_eq!(store.generation(), generation, "nothing was written");
        assert_eq!(store.unigram_total().expect("unigram total"), total);
        assert_eq!(store.unigram_delta(0x0100_0001).expect("delta"), 0);
        assert_eq!(store.bigram_row(1).expect("row"), Some(short));
        assert!(!store.is_modified());
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// The system-library reload after a mask rebuilds the session tables,
    /// but the session's grams live in the container: a gram left there is
    /// still user data, so it stays readable even when no table row is.
    #[test]
    fn grams_stay_readable_after_a_system_reload_empties_the_tables() {
        let dir = tempdir("reload-keeps-grams");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        store
            .observe_selection(1, 0x0100_0001)
            .expect("train a system token");
        let gram = store.bigram_row(1).expect("row");
        assert!(gram.is_some());
        // The reload drops library 1's rows: the trained unigram was the
        // only table row.
        store
            .reload_system_libraries(
                originals(),
                0xffff_ffff,
                0x0100_0002,
                persistence::SystemLogSnapshot::default(),
            )
            .expect("reload");
        assert_eq!(store.unigram_delta(0x0100_0001).expect("delta"), 0);
        assert_eq!(store.bigram_row(1).expect("row"), gram);
        assert_eq!(store.bigram_count(1, 0x0100_0001).expect("count"), 69);
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// Q1's abort: on `[1, 2, 3, 6, 4]` training 5 misses, appends, and
    /// `set_freq` misses the appended item, where the pin's
    /// `check_result(user->set_freq(token, freq + seed))` dies
    /// (`phonetic_lookup.h:903`). The store refuses before anything is
    /// written, and the step check names the predecessor.
    #[test]
    fn a_set_freq_miss_refuses_training_and_observes_nothing() {
        let dir = tempdir("set-freq-miss");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        let token = |n: u32| 0x0100_0000 | n;
        let gram = oxpinyin_data::single_gram::encode_single_gram(
            5,
            &[
                (token(1), 1),
                (token(2), 1),
                (token(3), 1),
                (token(6), 1),
                (token(4), 1),
            ],
        );
        store.plant_raw_bigram(2, &gram);
        // The step checks replay the call's steps on copies and name the
        // predecessor whose step misses.
        store.begin_training_steps();
        store
            .check_training_step(3, token(1))
            .expect("a clean step");
        assert!(matches!(
            store.check_training_step(2, token(5)),
            Err(UserStoreError::TrainingSetFreqMiss)
        ));
        assert_eq!(
            store.take_training_fault(),
            Some(crate::TrainingFault::SetFreqMiss(2))
        );
        store.begin_training_steps();
        store
            .check_training_step(2, token(6))
            .expect("a hit trains");
        assert_eq!(store.take_training_fault(), None);
        let generation = store.generation();
        assert!(matches!(
            store.observe_selection(2, token(5)),
            Err(UserStoreError::TrainingSetFreqMiss)
        ));
        assert_eq!(store.generation(), generation, "nothing was written");
        assert_eq!(store.unigram_delta(token(5)).expect("delta"), 0);
        assert_eq!(store.bigram_row(2).expect("row"), Some(gram));
        assert!(!store.is_modified());
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// Q1: training searches the stored items as the pin does. On
    /// `[high, low]` `get_freq(low)` misses from the libstdc++ lower bound
    /// and `insert_freq(low, 0)` lands at the same place, so the gram
    /// gains a duplicate seeded with 69 (`phonetic_lookup.h:888-903`).
    #[test]
    fn training_over_an_unsorted_gram_inserts_where_the_pin_does() {
        let dir = tempdir("unsorted-gram-train");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        let (high, low) = (0x0100_0009_u32, 0x0100_0001_u32);
        let gram = oxpinyin_data::single_gram::encode_single_gram;
        store.plant_raw_bigram(2, &gram(10, &[(high, 4), (low, 6)]));
        assert_eq!(store.bigram_count(2, low).expect("miss"), 0);
        assert_eq!(store.observe_selection(2, low).expect("train"), 69);
        assert_eq!(
            store.bigram_row(2).expect("row"),
            Some(gram(79, &[(low, 69), (high, 4), (low, 6)]))
        );
        // The repeat finds the first match and doubles from it.
        assert_eq!(store.observe_selection(2, low).expect("train"), 138);
        assert_eq!(
            store.bigram_row(2).expect("row"),
            Some(gram(217, &[(low, 207), (high, 4), (low, 6)]))
        );
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_partial_tail_created_in_session_refuses_training() {
        let dir = tempdir("partial-gram-in-session");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        // Nothing loaded: the predecessor is clean and trains.
        assert!(!store.has_oob_bigram_value_for(1));
        store
            .observe_selection(1, 0x0100_0001)
            .expect("clean predecessor trains");
        let total = store.bigram_total(1).expect("total");
        // A partial-tail value appears after the open (a retained removal
        // record on a backend that keeps them has this shape).
        let mut value = (total as u32).to_le_bytes().to_vec();
        value.extend_from_slice(&0x0100_0001_u32.to_le_bytes());
        value.extend_from_slice(&(total as u32).to_le_bytes());
        value.push(0x01);
        store.plant_raw_bigram(1, &value);
        assert!(store.has_oob_bigram_value_for(1));
        assert!(matches!(
            store.observe_selection(1, 0x0100_0002),
            Err(UserStoreError::ShortUserBigramValue)
        ));
        assert!(matches!(
            store.observe_predicted(1, 0x0100_0002),
            Err(UserStoreError::ShortUserBigramValue)
        ));
        assert_eq!(store.bigram_total(1).expect("unchanged"), total);
        assert_eq!(store.bigram_count(1, 0x0100_0002).expect("cur"), 0);
        // Overwriting the value with a whole gram clears the refusal.
        store.plant_raw_bigram(1, &value[..value.len() - 1]);
        assert!(!store.has_oob_bigram_value_for(1));
        store
            .observe_selection(1, 0x0100_0002)
            .expect("trains again");
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn partial_tail_guard_tracks_an_in_session_mask_result() {
        let dir = tempdir("partial-gram-live-mask");
        let key = 1_u32.to_le_bytes();
        let value = [
            7_u32.to_le_bytes().as_slice(),
            9_u32.to_le_bytes().as_slice(),
            7_u32.to_le_bytes().as_slice(),
        ]
        .concat();
        DefaultStore::write_user_bigram(&dir.join("user_bigram.db"), &[(key.to_vec(), value)])
            .expect("write valid gram");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");

        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        assert!(!store.has_oob_bigram_value_for(1));
        assert_eq!(store.bigram_count(1, 9).expect("initial item"), 7);
        store.mask_out(u32::MAX, 1).expect("whole-key mask");
        // There is one copy: every read answers from the value the
        // backend's container kept after its `remove`.
        let live = store.bigram_row(1).expect("live row");
        let live_after_mask = store.has_oob_bigram_value_for(1);
        let partial = |gram: &[u8]| gram.len() >= 4 && !(gram.len() - 4).is_multiple_of(8);
        assert_eq!(live_after_mask, live.as_deref().is_some_and(partial));
        assert_eq!(
            store.bigram_count(1, 9).expect("masked item"),
            live.as_deref()
                .and_then(|gram| oxpinyin_core::single_gram::get_freq(gram, 9))
                .map_or(0, u64::from)
        );
        // This fixture explicitly exercises serialization after the raw mask;
        // the reporting helper now enforces the pin's modified guard too.
        store.mark_modified();
        let report = store.save_reporting().expect("save masked gram");
        assert!(report.renames_failed.is_empty());
        assert!(report.user_conf_write_failed.is_none());
        let raw = persistence::load_user_bigram_db(&dir).expect("raw masked container");
        let persisted_after_mask = raw.get(&key).expect("raw masked get");
        match persisted_after_mask {
            Some(bytes) => {
                assert!(bytes.len() >= 4 && !(bytes.len() - 4).is_multiple_of(8));
                assert!(
                    live_after_mask,
                    "retained partial value was missed immediately"
                );
            }
            None => assert!(!live_after_mask, "removed value stayed marked"),
        }
        drop(raw);

        // The hook updates that value in place (Q5): the item is inserted,
        // the total moves by the difference, and a partial tail the
        // backend kept stays.
        let total = live
            .as_deref()
            .and_then(oxpinyin_core::single_gram::get_total_freq)
            .unwrap_or(0);
        store.set_bigram_count(1, 10, 5).expect("overwrite gram");
        assert_eq!(store.has_oob_bigram_value_for(1), live_after_mask);
        assert_eq!(
            store.bigram_total(1).expect("new total"),
            (u64::from(total) + 5).min(u64::from(u32::MAX))
        );
        assert_eq!(store.bigram_count(1, 10).expect("new item"), 5);
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn short_bigram_guard_reads_the_live_raw_value() {
        let dir = tempdir("short-gram-live-guard");
        let key = 1_u32.to_le_bytes();
        DefaultStore::write_user_bigram(
            &dir.join("user_bigram.db"),
            &[(key.to_vec(), vec![0xa5, 0x5a, 0x01])],
        )
        .expect("write short gram");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");

        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        // The one copy is the raw value: no total can be read from it.
        assert!(store.has_short_bigram_value_for(1));
        assert_eq!(
            store.bigram_row(1).expect("raw row"),
            Some(vec![0xa5, 0x5a, 0x01])
        );
        assert_eq!(store.bigram_total(1).expect("no total"), 0);
        assert!(matches!(
            store.mask_out(u32::MAX, 0),
            Err(UserStoreError::ShortUserBigramValue)
        ));
        let raw = persistence::load_user_bigram_db(&dir).expect("raw container");
        assert_eq!(
            raw.get(&key).expect("raw get"),
            Some(vec![0xa5, 0x5a, 0x01])
        );
        drop(raw);

        // The test hook replaces a value too short for a total with a fresh
        // gram (Q5). The guard follows the live container.
        store.set_bigram_count(1, 9, 7).expect("replace gram");
        assert!(!store.has_short_bigram_value_for(1));
        assert_eq!(store.bigram_total(1).expect("new total"), 7);
        assert_eq!(store.bigram_count(1, 9).expect("new item"), 7);
        store.mask_out(u32::MAX, 0).expect("safe mask");
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn mask_out_replays_only_the_validated_system_log_snapshot() {
        use oxpinyin_data::chunk_format::build_memory_chunk;
        use oxpinyin_data::chunk_write::encode_phrase_item;
        use oxpinyin_data::user_files::{LogRecord, encode_log_records};

        let dir = tempdir("mask-snapshot");
        let original = originals();
        let old = original[&1].item(1).expect("original item");
        let mut new = old.clone();
        new.phrase = vec![0x9f98];
        let header = LogRecord::ModifyHeader {
            old_total: 100,
            new_total: 100,
        };
        let payload = encode_log_records(&[
            header.clone(),
            LogRecord::Modify {
                token: 1,
                old_item: encode_phrase_item(&old).expect("old item"),
                new_item: encode_phrase_item(&new).expect("new item"),
            },
        ])
        .expect("valid log");
        let valid = build_memory_chunk(&payload).expect("valid frame");
        let mut non_null_header =
            encode_log_records(std::slice::from_ref(&header)).expect("header");
        non_null_header[4..8].copy_from_slice(&1_u32.to_le_bytes());
        let duplicate_headers =
            encode_log_records(&[header.clone(), header.clone()]).expect("duplicate headers");
        let surviving_add = encode_log_records(&[
            header,
            LogRecord::Add {
                token: 2,
                new_item: encode_phrase_item(&new).expect("added item"),
            },
        ])
        .expect("surviving ADD");
        let mut store =
            UserStore::open_libpinyin(&dir, original.clone(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        let path = dir.join("gb_char.dbin");
        for replacement in [non_null_header, duplicate_headers, surviving_add] {
            std::fs::write(&path, &valid).expect("validated log");
            let logs = store
                .validate_system_logs(|_| true, 0x00ff_ffff, 0)
                .expect("preflight");
            // A concurrent atomic save changes the path after preflight.
            // Replay must consume the snapshot, never this abort-shaped log.
            let tmp = dir.join("gb_char.dbin.tmp");
            std::fs::write(
                &tmp,
                build_memory_chunk(&replacement).expect("replacement frame"),
            )
            .expect("replacement");
            std::fs::rename(tmp, &path).expect("atomic replacement");
            store.mask_out(0x00ff_ffff, 0).expect("mask tables");
            store
                .reload_system_libraries(original.clone(), 0x00ff_ffff, 0, logs)
                .expect("snapshot replay");
            assert_eq!(
                store
                    .system_item_override(0x0100_0001)
                    .expect("item")
                    .expect("override"),
                new,
            );
            assert!(store.phrase(0x0100_0002).expect("absent ADD").is_none());
        }
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
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
            assert_eq!(
                store.with_effective_system_item(token.value(), |item| item
                    .prons
                    .iter()
                    .map(|(_, count)| *count)
                    .collect::<Vec<_>>()),
                Some([169, 200, 300].to_vec())
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
            assert_eq!(
                store.with_effective_system_item(token.value(), |item| item
                    .prons
                    .iter()
                    .map(|(_, count)| *count)
                    .collect::<Vec<_>>()),
                Some([445, 476, 300].to_vec())
            );
            assert!(store.save().expect("save"));
        }
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn live_system_readings_train_import_and_replay_without_double_counting() {
        use oxpinyin_core::{PhraseToken, UserModel};
        let dir = tempdir("live-system-readings");
        let mut base = originals();
        let ni = ChewingKey::from_pinyin("ni").expect("ni");
        base.get_mut(&1)
            .expect("library")
            .items
            .get_mut(&1)
            .expect("item")
            .prons = vec![(vec![ni.with_tone(3).to_packed()], 100)];
        let token = PhraseToken::new(0x0100_0001);
        let dense = |tone| packed_to_pinyin_keys(&[ni.with_tone(tone).to_packed()]).expect("dense");
        let counts = |store: &UserStore| {
            store
                .with_effective_system_item(token.value(), |item| {
                    item.prons
                        .iter()
                        .map(|(_, count)| *count)
                        .collect::<Vec<_>>()
                })
                .expect("live")
        };
        {
            let mut store =
                UserStore::open_libpinyin(&dir, base.clone(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            store
                .add_system_phrase_in(1, Some(token.value()), "你", &dense(2), Some(20))
                .expect("extra reading");
            UserModel::observe_with_keys(
                &mut store,
                &[],
                &token,
                &mut [vec![ni.with_tone(2)]].into_iter(),
            )
            .expect("train extra");
            assert_eq!(counts(&store), [100, 89]);
            store
                .add_system_phrase_in(1, Some(token.value()), "你", &dense(2), Some(7))
                .expect("import after training");
            assert_eq!(counts(&store), [100, 96]);
            assert!(store.save().expect("save"));
        }
        {
            let mut store = UserStore::open_libpinyin(&dir, base, versions(), UserConfLaw::Pinyin)
                .expect("reopen");
            assert_eq!(counts(&store), [100, 96]);
            UserModel::observe_with_keys(
                &mut store,
                &[],
                &token,
                &mut [vec![ni.with_tone(2)]].into_iter(),
            )
            .expect("train replay");
            assert_eq!(counts(&store), [100, 234]);
            assert!(store.save().expect("save again"));
        }
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn reloading_a_system_library_drops_the_unsaved_session() {
        let dir = tempdir("reload-drops-session");
        let mut base = originals();
        let ni = ChewingKey::from_pinyin("ni").expect("ni");
        base.get_mut(&1)
            .expect("library")
            .items
            .get_mut(&1)
            .expect("item")
            .prons = vec![(vec![ni.with_tone(3).to_packed()], 100)];
        let dense = |tone| packed_to_pinyin_keys(&[ni.with_tone(tone).to_packed()]).expect("dense");
        let counts = |store: &UserStore| {
            store
                .with_effective_system_item(0x0100_0001, |item| {
                    item.prons
                        .iter()
                        .map(|(_, count)| *count)
                        .collect::<Vec<_>>()
                })
                .expect("live")
        };
        let mut store =
            UserStore::open_libpinyin(&dir, base.clone(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        store
            .add_system_phrase_in(1, Some(0x0100_0001), "你", &dense(2), Some(20))
            .expect("saved reading");
        store.mark_modified();
        assert!(store.save().expect("save"));
        store
            .add_system_phrase_in(1, Some(0x0100_0001), "你", &dense(4), Some(30))
            .expect("unsaved reading");
        let saved = counts(&store);
        assert_eq!(saved.len(), 3, "the unsaved reading is live");
        // A reload merges only the saved log: the unsaved reading is gone.
        store
            .load_system_libraries(base.clone())
            .expect("reload from the saved log");
        assert_eq!(counts(&store).len(), 2);
        // An unload takes the whole library, the saved log included; the
        // next load puts the saved state back.
        store.unload_system_libraries(base.clone()).expect("unload");
        assert!(
            store
                .system_item_override(0x0100_0001)
                .expect("item")
                .is_none()
        );
        store
            .load_system_libraries(base)
            .expect("reload after unload");
        assert_eq!(counts(&store).len(), 2);
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn stored_pronunciation_counts_follow_committed_item_changes() {
        let dir = tempdir("stored-pronunciation-counts");
        let readings = distinct_readings();
        let dense = |packed| packed_to_pinyin_keys(&[packed]).expect("dense");
        let token;
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            token = store
                .add_phrase_in(7, "龘", &dense(readings[0]), Some(1))
                .expect("add");
            assert_eq!(store.user_pronunciation_count(token), Some(1));
            store
                .add_phrase_in(7, "龘", &dense(readings[0]), Some(9))
                .expect("merge");
            assert_eq!(store.user_pronunciation_count(token), Some(1));
            for (index, &reading) in readings[1..255].iter().enumerate() {
                store
                    .add_phrase_in(7, "龘", &dense(reading), Some(1))
                    .expect("append");
                assert_eq!(
                    store.user_pronunciation_count(token),
                    Some(index as u32 + 2)
                );
            }
            store
                .add_phrase_in(7, "龘", &dense(readings[255]), Some(1))
                .expect("wrap");
            assert_eq!(store.user_pronunciation_count(token), Some(0));
            store
                .add_phrase_in(7, "龘", &dense(readings[256]), Some(1))
                .expect("257th");
            assert_eq!(store.user_pronunciation_count(token), Some(1));
            store.mark_modified();
            assert!(store.save().expect("save"));
            store.fail_next_replace();
            assert!(store.unload_user_library(7).is_err());
            assert_eq!(store.user_pronunciation_count(token), Some(1));
            store.unload_user_library(7).expect("unload");
            assert_eq!(store.user_pronunciation_count(token), None);
            store.load_user_library(7).expect("reload");
            assert_eq!(store.user_pronunciation_count(token), Some(1));
        }
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("reopen");
        assert_eq!(store.user_pronunciation_count(token), Some(1));
        store.mask_out(0xffff_ffff, token).expect("mask");
        assert_eq!(store.user_pronunciation_count(token), None);
        let other = store
            .add_phrase_in(7, "龖", &dense(readings[0]), Some(1))
            .expect("new");
        assert_eq!(store.user_pronunciation_count(other), Some(1));
        assert!(store.remove_user_phrase(other).expect("remove"));
        assert_eq!(store.user_pronunciation_count(other), None);
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn reloading_the_user_library_keeps_only_the_saved_chunk() {
        let dir = tempdir("reload-user-library");
        let ni = ChewingKey::from_pinyin("ni").expect("ni");
        let dense = |tone| packed_to_pinyin_keys(&[ni.with_tone(tone).to_packed()]).expect("dense");
        let readings = |store: &UserStore| {
            store
                .phrases()
                .expect("phrases")
                .iter()
                .map(|phrase| {
                    phrase
                        .pronunciations()
                        .iter()
                        .map(|reading| reading.indexed())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        // Nothing saved: the library comes back empty.
        store
            .add_phrase_in(crate::USER_DICTIONARY, "龘", &dense(2), Some(20))
            .expect("unsaved item");
        store
            .unload_user_library(crate::USER_DICTIONARY)
            .expect("unload");
        assert!(readings(&store).is_empty());
        store
            .load_user_library(crate::USER_DICTIONARY)
            .expect("load");
        assert!(readings(&store).is_empty());
        // A saved item returns with its reading still indexed; an unsaved
        // reading merged after the save does not.
        store
            .add_phrase_in(crate::USER_DICTIONARY, "鱻", &dense(2), Some(20))
            .expect("item");
        store.mark_modified();
        assert!(store.save().expect("save"));
        store
            .add_phrase_in(crate::USER_DICTIONARY, "鱻", &dense(4), Some(30))
            .expect("unsaved reading");
        assert_eq!(readings(&store), [vec![true, false]]);
        store
            .unload_user_library(crate::USER_DICTIONARY)
            .expect("unload");
        assert!(readings(&store).is_empty());
        store
            .load_user_library(crate::USER_DICTIONARY)
            .expect("load");
        assert_eq!(readings(&store), [vec![true]]);
        // A different phrase handed a dropped token is found under the
        // reading the index kept for it.
        let ni_key = || oxpinyin_core::SyllableKey::from_text("ni").expect("ni");
        store
            .unload_user_library(crate::USER_DICTIONARY)
            .expect("unload again");
        store
            .load_user_library(crate::USER_DICTIONARY)
            .expect("load again");
        let reused = store
            .add_phrase_in(crate::USER_DICTIONARY, "鱺", &dense(3), Some(20))
            .expect("another phrase takes the dropped token");
        let found = crate::UserLookup::from_store(&store)
            .expect("lookup")
            .lookup(&[ni_key()]);
        assert!(found.iter().any(|entry| entry.token().value() == reused));
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_refused_library_write_leaves_the_kept_index_and_the_rows_alone() {
        let dir = tempdir("refused-library-write");
        let ni = ChewingKey::from_pinyin("ni").expect("ni");
        let dense = |tone| packed_to_pinyin_keys(&[ni.with_tone(tone).to_packed()]).expect("dense");
        let kept = |store: &UserStore| {
            let mut entries = store.kept_index_entries();
            entries.sort();
            entries
        };
        let readings = |store: &UserStore| {
            store
                .phrases()
                .expect("phrases")
                .iter()
                .map(|phrase| {
                    phrase
                        .pronunciations()
                        .iter()
                        .map(|reading| reading.indexed())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        };
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        store
            .add_phrase_in(crate::USER_DICTIONARY, "龘", &dense(2), Some(20))
            .expect("item");
        assert!(kept(&store).is_empty());
        // A refused unload keeps the index entries where they were: none
        // set aside, the item and its indexed reading still there.
        store.fail_next_replace();
        assert!(store.unload_user_library(crate::USER_DICTIONARY).is_err());
        assert!(kept(&store).is_empty());
        assert_eq!(readings(&store), [vec![true]]);
        // The unload that is not refused sets the entry aside; a refused
        // load leaves it there and the library empty.
        store
            .unload_user_library(crate::USER_DICTIONARY)
            .expect("unload");
        let set_aside = kept(&store);
        assert_eq!(set_aside.len(), 1);
        store.fail_next_replace();
        assert!(store.load_user_library(crate::USER_DICTIONARY).is_err());
        assert_eq!(kept(&store), set_aside);
        assert!(readings(&store).is_empty());
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    /// 256 or more distinct toned one-syllable readings, as packed words.
    fn distinct_readings() -> Vec<u16> {
        let mut readings: Vec<u16> = Vec::new();
        for initial in [
            "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x", "zh", "ch", "sh",
            "r", "z", "c", "s", "y", "w", "",
        ] {
            for final_ in [
                "a", "o", "e", "ai", "ei", "ao", "ou", "an", "en", "ang", "eng", "i", "u", "ia",
                "ie", "iao", "iu", "ian", "in", "iang", "ing", "ua", "uo", "uai", "ui", "uan",
                "un", "uang", "ong",
            ] {
                let Some(key) = ChewingKey::from_pinyin(&format!("{initial}{final_}")) else {
                    continue;
                };
                for tone in 1..=4 {
                    let packed = key.with_tone(tone).to_packed();
                    if packed_to_pinyin_keys(&[packed]).is_some() && !readings.contains(&packed) {
                        readings.push(packed);
                    }
                }
            }
        }
        assert!(readings.len() >= 258, "need 258 distinct readings");
        readings
    }

    fn pronunciation_rows(store: &UserStore) -> usize {
        use oxpinyin_store::ReadStore as _;
        let mut rows = 0_usize;
        let db = store.database();
        db.for_each(PRONUNCIATION, |_, _| {
            rows += 1;
            Ok(())
        })
        .expect("rows");
        rows
    }

    /// The pin stores `npron + 1` in a `guint8` (`phrase_index.cpp:28`,
    /// `:88`), so the 256th reading leaves a count of 0 and the call still
    /// succeeds. Reads size the item from that count (`:203-204`) and the
    /// next add copies only that view (`:228`): the item has no readings,
    /// keeps its unigram, survives save and reload, and the 257th add
    /// leaves exactly one reading, the new one.
    #[test]
    fn a_256th_system_reading_wraps_the_count_to_an_item_without_readings() {
        let dir = tempdir("import-256th-system-reading");
        let readings = distinct_readings();
        let dense = |packed| packed_to_pinyin_keys(&[packed]).expect("dense");
        let readings_of = |store: &UserStore, token| {
            store
                .phrase(token)
                .expect("phrase")
                .expect("present")
                .pronunciations()
                .iter()
                .map(|pronunciation| pronunciation.count())
                .collect::<Vec<_>>()
        };
        let token;
        let unigram;
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            token = store
                .add_system_phrase_in(1, None, "拟", &dense(readings[0]), Some(5))
                .expect("append");
            for &packed in &readings[1..255] {
                store
                    .add_system_phrase_in(1, Some(token), "拟", &dense(packed), Some(5))
                    .expect("reading within the 255 limit");
            }
            assert_eq!(pronunciation_rows(&store), 255);
            unigram = store.unigram_delta(token).expect("unigram");
            assert_eq!(unigram, 15);
            store
                .add_system_phrase_in(1, Some(token), "拟", &dense(readings[255]), Some(5))
                .expect("the 256th reading is accepted");
            assert_eq!(pronunciation_rows(&store), 0);
            assert_eq!(
                store.with_effective_system_item(token, |item| item.prons.len()),
                Some(0)
            );
            assert!(readings_of(&store, token).is_empty());
            assert_eq!(store.unigram_delta(token).expect("unigram"), unigram);
            store.mark_modified();
            assert!(store.save().expect("save"));
        }
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("reopen");
            assert!(readings_of(&store, token).is_empty());
            assert_eq!(store.unigram_delta(token).expect("unigram"), unigram);
            store
                .add_system_phrase_in(1, Some(token), "拟", &dense(readings[256]), Some(9))
                .expect("the 257th reading");
            assert_eq!(readings_of(&store, token), [9]);
            assert_eq!(
                store.with_effective_system_item(token, |item| item.prons.clone()),
                Some(vec![(vec![readings[256]], 9)])
            );
            store.mark_modified();
            assert!(store.save().expect("save"));
        }
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("reopen");
        assert_eq!(readings_of(&store, token), [9]);
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    /// The same wrap on an item the system library ships (token 0x0100_0001,
    /// one reading), which persists as a modified original: the empty item
    /// must survive save and reopen, and the 257th reading must then leave
    /// exactly one reading.
    #[test]
    fn a_256th_reading_wraps_an_original_system_item_through_save_and_reopen() {
        let dir = tempdir("import-256th-original-reading");
        let token = 0x0100_0001;
        let original = ChewingKey::new(23, 0, 1, 0).to_packed();
        let readings: Vec<u16> = distinct_readings()
            .into_iter()
            .filter(|&packed| packed != original)
            .collect();
        let dense = |packed| packed_to_pinyin_keys(&[packed]).expect("dense");
        let prons_of = |store: &UserStore| {
            store
                .with_effective_system_item(token, |item| item.prons.clone())
                .expect("live item")
        };
        let unigram_of = |store: &UserStore| {
            store
                .system_item_override(token)
                .expect("override")
                .expect("present")
                .unigram
        };
        let unigram;
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            for &packed in &readings[..254] {
                store
                    .add_system_phrase_in(1, Some(token), "你", &dense(packed), Some(5))
                    .expect("reading within the 255 limit");
            }
            assert_eq!(prons_of(&store).len(), 255);
            unigram = unigram_of(&store);
            store
                .add_system_phrase_in(1, Some(token), "你", &dense(readings[254]), Some(5))
                .expect("the 256th reading is accepted");
            assert!(prons_of(&store).is_empty());
            assert_eq!(unigram_of(&store), unigram);
            store.mark_modified();
            assert!(store.save().expect("a wrapped original saves"));
        }
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("reopen");
            assert!(prons_of(&store).is_empty());
            assert_eq!(unigram_of(&store), unigram);
            store
                .add_system_phrase_in(1, Some(token), "你", &dense(readings[255]), Some(9))
                .expect("the 257th reading");
            assert_eq!(prons_of(&store), [(vec![readings[255]], 9)]);
            store.mark_modified();
            assert!(store.save().expect("save"));
        }
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("reopen");
        assert_eq!(prons_of(&store), [(vec![readings[255]], 9)]);
        assert_eq!(unigram_of(&store), unigram);
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    /// The same wrap on a `USER_FILE` library, where the item is its rows.
    #[test]
    fn a_256th_user_reading_wraps_the_count_to_an_item_without_readings() {
        let dir = tempdir("import-256th-user-reading");
        let readings = distinct_readings();
        let dense = |packed| packed_to_pinyin_keys(&[packed]).expect("dense");
        let readings_of = |store: &UserStore, token| {
            store
                .phrase(token)
                .expect("phrase")
                .expect("present")
                .pronunciations()
                .iter()
                .map(|pronunciation| pronunciation.count())
                .collect::<Vec<_>>()
        };
        let token;
        let unigram;
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            token = store
                .add_phrase_in(crate::USER_DICTIONARY, "龘", &dense(readings[0]), Some(5))
                .expect("append");
            for &packed in &readings[1..255] {
                store
                    .add_phrase_in(crate::USER_DICTIONARY, "龘", &dense(packed), Some(5))
                    .expect("reading within the 255 limit");
            }
            assert_eq!(readings_of(&store, token).len(), 255);
            unigram = store.unigram_delta(token).expect("unigram");
            store
                .add_phrase_in(crate::USER_DICTIONARY, "龘", &dense(readings[255]), Some(5))
                .expect("the 256th reading is accepted");
            assert!(readings_of(&store, token).is_empty());
            assert_eq!(store.unigram_delta(token).expect("unigram"), unigram);
            store.mark_modified();
            assert!(store.save().expect("a wrapped item saves"));
        }
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("reopen");
            assert!(readings_of(&store, token).is_empty());
            assert_eq!(store.unigram_delta(token).expect("unigram"), unigram);
            store
                .add_phrase_in(crate::USER_DICTIONARY, "龘", &dense(readings[256]), Some(9))
                .expect("the 257th reading");
            assert_eq!(readings_of(&store, token), [9]);
            store.mark_modified();
            assert!(store.save().expect("save"));
        }
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("reopen");
        assert_eq!(readings_of(&store, token), [9]);
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn appended_system_readings_keep_absolute_training_counts() {
        use oxpinyin_core::{PhraseToken, UserModel};
        let dir = tempdir("live-appended-readings");
        let ni = ChewingKey::from_pinyin("ni").expect("ni");
        let keys = packed_to_pinyin_keys(&[ni.to_packed()]).expect("dense");
        let base = originals();
        let token;
        {
            let mut store =
                UserStore::open_libpinyin(&dir, base.clone(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            token = store
                .add_system_phrase_in(1, None, "拟", &keys, Some(5))
                .expect("append");
            UserModel::observe_with_keys(
                &mut store,
                &[],
                &PhraseToken::new(token),
                &mut [vec![ni]].into_iter(),
            )
            .expect("train appended");
            assert_eq!(
                store.with_effective_system_item(token, |item| item.prons[0].1),
                Some(74)
            );
            store
                .add_system_phrase_in(1, Some(token), "拟", &keys, Some(7))
                .expect("reimport");
            assert_eq!(
                store.with_effective_system_item(token, |item| item.prons[0].1),
                Some(81)
            );
            assert_eq!(
                store
                    .phrase(token)
                    .expect("phrase")
                    .expect("present")
                    .pronunciations()[0]
                    .count(),
                81
            );
            assert!(store.save().expect("save"));
        }
        let store =
            UserStore::open_libpinyin(&dir, base, versions(), UserConfLaw::Pinyin).expect("reopen");
        assert_eq!(
            store.with_effective_system_item(token, |item| item.prons[0].1),
            Some(81)
        );
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn live_pronunciation_overflow_keeps_earlier_updates_and_stops_at_guard() {
        use oxpinyin_core::{PhraseToken, UserModel};
        let dir = tempdir("live-pronunciation-overflow");
        let mut base = originals();
        let ni = ChewingKey::from_pinyin("ni").expect("ni");
        base.get_mut(&1)
            .expect("library")
            .items
            .get_mut(&1)
            .expect("item")
            .prons = vec![
            (vec![ni.with_tone(2).to_packed()], u32::MAX - 75),
            (vec![ni.with_tone(3).to_packed()], 0),
        ];
        let mut store =
            UserStore::open_libpinyin(&dir, base, versions(), UserConfLaw::Pinyin).expect("open");
        let token = PhraseToken::new(0x0100_0001);
        UserModel::observe_with_keys(&mut store, &[], &token, &mut [vec![ni]].into_iter())
            .expect("train guarded rows");
        assert_eq!(
            store.with_effective_system_item(token.value(), |item| item
                .prons
                .iter()
                .map(|(_, count)| *count)
                .collect::<Vec<_>>()),
            Some(vec![u32::MAX - 6, 0])
        );
        drop(store);
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
            // the phrase; the container the raw gram the pin's training
            // wrote.
            assert_eq!(
                store.bigram_row(1).expect("row"),
                Some(oxpinyin_data::single_gram::encode_single_gram(
                    69,
                    &[(0x0100_0001, 69)]
                ))
            );
            let state = export_state(&store, &originals()).expect("export");
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

    /// #784/#785: masks/removals retain keys, including KC/tkrzw mask tails,
    /// and raw records outlive a reopen.
    #[test]
    fn masked_and_removed_phrases_preserve_raw_index_records() {
        use oxpinyin_store::RawReadStore;

        let dir = tempdir("index-keys");
        let id = |text: &str| {
            u16::try_from(
                oxpinyin_core::SyllableKey::from_text(text)
                    .expect("frozen syllable")
                    .index(),
            )
            .expect("id fits u16")
        };
        let ucs4 = |text: &str| -> Vec<u8> {
            text.chars()
                .flat_map(|c| u32::from(c).to_le_bytes())
                .collect()
        };
        let phrase_rows = || {
            let store = DefaultStore::open_user_index(
                &dir.join(oxpinyin_data::user_files::UserDbm::PhraseIndex.file_name()),
            )
            .expect("opens");
            let mut rows = BTreeMap::new();
            store
                .range_raw(
                    std::ops::Bound::Unbounded,
                    std::ops::Bound::Unbounded,
                    |k, v| {
                        rows.insert(k.to_vec(), v.to_vec());
                        Ok(())
                    },
                )
                .expect("walk");
            rows
        };
        let removed = {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            let masked = store
                .add_phrase("你好", &[id("ni"), id("hao")][..], None)
                .expect("add");
            let removed = store
                .add_phrase("我们", &[id("wo"), id("men")][..], None)
                .expect("add");
            store.mark_modified();
            assert!(store.save().expect("save"));
            store.mask_out(0xffff_ffff, masked).expect("mask");
            assert!(store.remove_user_phrase(removed).expect("remove"));
            store.add_phrase("他", &[id("ta")][..], None).expect("add");
            store.mark_modified();
            assert!(store.save().expect("save after"));
            removed
        };
        let rows = phrase_rows();
        for text in ["你", "你好", "我", "我们"] {
            // Raw UCS-4 walk: 我, 我们, 你, 你好. KC/tkrzw carry
            // 我们's post-mask token through the empty 你 prefix marker.
            // Removing 我们 later edits that record alone.
            let expected = if !cfg!(feature = "bdb") && text == "你" {
                removed.to_le_bytes().to_vec()
            } else {
                Vec::new()
            };
            assert_eq!(rows.get(&ucs4(text)), Some(&expected), "{text}");
        }
        assert!(rows.get(&ucs4("他")).is_some_and(|value| value.len() == 4));
        let pinyin_keys =
            persistence::load_profile(&dir, &originals(), &versions(), UserConfLaw::Pinyin)
                .expect("load")
                .index_file_keys
                .pinyin;
        // Both key spaces of both two-syllable readings and their prefixes,
        // and 他's one-syllable reading in each space.
        assert_eq!(pinyin_keys.len(), 2 * 2 * 2 + 2, "{pinyin_keys:?}");

        // A later session keeps them through its own save.
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("reopen");
            store.mark_modified();
            assert!(store.save().expect("save again"));
        }
        assert_eq!(phrase_rows(), rows);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn a_preexisting_libpinyin_profile_seeds_the_session() {
        let dir = tempdir("seed");
        // Write a profile directly through the persistence layer: a gram
        // and a user phrase, as a same-backend libpinyin would have.
        let state = UserState {
            bigram: vec![(
                1_u32.to_le_bytes().to_vec(),
                oxpinyin_data::single_gram::encode_single_gram(
                    207,
                    &[(0x0100_0001, 138), (0x0700_0001, 69)],
                ),
            )],
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
            phrase_table: BTreeMap::new(),
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

#[cfg(test)]
mod replayed_readings;
