//! The libpinyin-file user store — [`crate::user::UserStore`] over the pin's own
//! user-dir file set.
//!
//! `UserStore::open_session` opens a user *directory* (not a store
//! file): [`crate::user::persistence`] reads the profile into the session's
//! state in process memory — the user library arenas, the pinyin index
//! and phrase table, the user bigram container, the live system items and
//! the libraries' stored-total offsets — and every value operation —
//! training, phrase adds, exports, masking — runs against that state.
//! `UserStore::save_reporting` then exports the session back through
//! [`crate::user::persistence`] into the pin's files. The durability shape is
//! therefore the pin's own: in memory between saves, as the pin's
//! context holds its loaded profile (`pinyin.cpp:326-444` at 074a2219),
//! whole-file `.tmp`+rename at `pinyin_save`, and a crash loses the
//! time since the last save, as the pin's does.
//!
//! The session touches nothing outside the user dir: no temp directory,
//! no `TMPDIR` read, nothing left behind by a process that dies before
//! `pinyin_fini`, and no file description shared with a `fork`ed child.
//! The pin reads no temp variable and opens no temp file anywhere under
//! `src` at 074a2219.
//!
//! The value mapping, both directions:
//!
//! * bigram — not a value the session exports at all: the session keeps
//!   the pin's own in-memory container, filled from `user_bigram.db` once
//!   by `load_db` and written back by `save_db`, and every bigram
//!   operation runs the pin's `SingleGram` code over its raw grams
//! * user phrases and appended system items — each body lives once in an
//!   owned `UserLibrary` arena, in the pin's packed item format. The user
//!   pinyin index is an independent set; it never supplies item counts.
//! * system tokens — a `.dbin` MODIFY record is the original item with its
//!   new unigram; a replayed REMOVE enters the live removal set. The
//!   session retains replayed originals in a private mapped view, decoding
//!   an item on its first use. That item holds its reading counts, and
//!   training updates them in place, which lookup, export and save all
//!   observe.

use std::cell::{Cell, Ref, RefCell, RefMut};
#[cfg(test)]
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

#[cfg(test)]
use crate::data::phrase_library::ChunkItem;
use crate::data::user_files::SystemVersions;
use crate::store::{DefaultUserBigramDb, UserBigramDb};
#[cfg(test)]
use crate::user::UserState;

use crate::user::phrase;
use crate::user::store::{StoreInner, UserStore, UserStoreError};
use crate::user::{self as persistence, PersistenceError, SystemOriginals, UserConfLaw};

/// The persistence target a session store carries: the user dir, the
/// original system chunks (the `.dbin` diff base), the conformance
/// triple, and the `user.conf` law and open counter the session's saves
/// and its fini write.
#[derive(Debug)]
pub struct Target {
    /// The user directory holding the profile.
    pub(crate) dir: Option<PathBuf>,
    /// Live item arenas and independent pinyin-index membership.
    pub(crate) phrases: RefCell<crate::user::user_library::SessionPhrases>,
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
}

impl Target {
    pub(crate) fn phrases(&self) -> Ref<'_, crate::user::user_library::SessionPhrases> {
        self.phrases.borrow()
    }

    pub(crate) fn phrases_mut(&self) -> RefMut<'_, crate::user::user_library::SessionPhrases> {
        self.phrases.borrow_mut()
    }
}

/// The facade fini's `user.conf` write ([`persistence::fini`]): libpinyin
/// lowers the open counter its init raised and writes the marker, saved
/// or not (`pinyin_fini`, `pinyin.cpp:1194-1200`); libzhuyin writes
/// nothing. Armed on the handle `UserStore::open_session`
/// returns, so it runs once per open — every open runs its own
/// `check_format` — at explicit context fini, or on drop if unfinished. A process that dies before then leaves the raised
/// counter on disk, as upstream's does.

#[derive(Debug)]
pub(crate) struct FiniGuard(Option<Rc<Target>>);

impl FiniGuard {
    /// Leave without the fini write: an open that fails where the pin dies
    /// ([`UserStoreError::SessionSeed`]) never reaches `pinyin_fini`.
    fn abandon(&mut self) {
        self.0 = None;
    }
}

impl FiniGuard {
    pub(crate) fn finish(&mut self) {
        if let Some(target) = self.0.take() {
            // `pinyin_fini` ignores `mark_version`'s result
            // (`pinyin.cpp:1200`); a save that fails has written
            // `UserTableInfo::save`'s line (`table_info.cpp:382`). Without a
            // user dir the pin saves to `""`, which fails the same way.
            persistence::fini(
                target.dir.as_deref(),
                &target.versions,
                target.law,
                target.open_counter,
            );
        }
    }
}

impl Drop for FiniGuard {
    fn drop(&mut self) {
        self.finish();
    }
}

impl From<PersistenceError> for UserStoreError {
    fn from(error: PersistenceError) -> Self {
        match error {
            PersistenceError::SystemRemapFailed => UserStoreError::SaveRemapFailed,
            PersistenceError::SaveTooLarge => UserStoreError::SaveTooLarge,
            PersistenceError::Io(_) | PersistenceError::Store(_) | PersistenceError::Codec => {
                UserStoreError::Persistence
            }
        }
    }
}

impl UserStore {
    /// Open the pin's NULL-user-directory session entirely in memory.
    /// Imports and lookup work; no profile is read, written or removed.
    /// The failed empty-filename marker diagnostics follow `law`.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError::SessionSeed`] if the user bigram container
    /// cannot be created.
    #[cfg(test)]
    pub(crate) fn open_transient(
        mut originals: SystemOriginals,
        versions: SystemVersions,
        law: UserConfLaw,
    ) -> Result<Self, UserStoreError> {
        let profile = persistence::LoadedProfile {
            replayed: originals.take_live_libraries(),
            ..Default::default()
        };
        Self::open_session(None, &mut originals, versions, law, profile)
    }

    /// Open the user store on a libpinyin user directory whose profile
    /// [`check_profile`](Self::check_profile) already judged, over `originals`
    /// that the library loop fills as it walks `rows`. The loop reads the
    /// profile under `law` into the session's state in process memory, and
    /// `UserStore::save_reporting` writes the pin's files back. Dropping
    /// the returned owner is the facade's fini: it makes the law's fini-time
    /// `user.conf` write ([`crate::user::fini`]).
    ///
    /// Every open is a session of its own, as every `pinyin_init` /
    /// `zhuyin_init` is a context of its own upstream: nothing is held per
    /// directory (`pinyin.cpp:326-444` keeps the profile in the context,
    /// `:1132-1147` saves that context's state).
    ///
    /// # Errors
    ///
    /// [`UserStoreError::SessionSeed`] when the session cannot be built from
    /// the loaded profile, where the pin dies. The loop's first fault is
    /// [`UserStoreError::LibraryLoop`], with the counter left raised. A
    /// corrupt or partial profile does not fail here: it degrades per file,
    /// as upstream's loader does.
    pub fn open_libpinyin_checked(
        user_dir: &Path,
        originals: &mut SystemOriginals,
        rows: &crate::user::library_loop::LibraryLoopRows<'_>,
        versions: SystemVersions,
        law: UserConfLaw,
        check: crate::user::ProfileCheck,
    ) -> Result<Self, UserStoreError> {
        let (profile, stop) = persistence::load_session_in_loop(user_dir, originals, rows, &check);
        stop.map_err(UserStoreError::LibraryLoop)?;
        Self::open_session(Some(user_dir), originals, versions, law, profile)
    }

    /// `check_format` alone, for an init that must judge the profile before
    /// it loads the system libraries, as the pin does: the `user.conf`
    /// diagnostic, the marker write and the wipe of a non-conform profile.
    ///
    /// # Aborts
    ///
    /// Unknown database format aborts at the read point.
    pub fn check_profile(
        user_dir: &Path,
        versions: &SystemVersions,
        layout: &crate::data::user_files::UserFileLayout,
        law: UserConfLaw,
    ) -> crate::user::ProfileCheck {
        persistence::check_format(user_dir, versions, layout, law)
    }

    /// `check_format` for an init without a user dir, whose `user.conf` the
    /// pin names `""` ([`crate::user::check_format_unnamed`]): the
    /// pin's failed load and, under libpinyin's law, failed save.
    pub(crate) fn check_profile_unnamed(versions: &SystemVersions, law: UserConfLaw) {
        persistence::check_format_unnamed(versions, law);
    }

    /// Seed the session and load the user bigram after the system bigram attach.
    #[inline(never)]
    pub(crate) fn open_session(
        user_dir: Option<&Path>,
        originals: &mut SystemOriginals,
        versions: SystemVersions,
        law: UserConfLaw,
        profile: persistence::LoadedProfile,
    ) -> Result<Self, UserStoreError> {
        let persistence::LoadedProfile {
            loaded,
            replayed,
            user_libraries,
            overlong_index_keys,
            index,
            phrase_table,
        } = profile;
        // Armed as soon as the load has raised the counter. Every failure
        // from here on is the pin's death ([`UserStoreError::SessionSeed`]),
        // which abandons the guard: the pin dies without `pinyin_fini`, so
        // the raised counter stays on disk and nothing is written or saved.
        // Only an open that completes hands it to the store.
        let target = Rc::new(Target {
            dir: user_dir.map(Path::to_path_buf),
            phrases: RefCell::new(Default::default()),
            versions,
            law,
            open_counter: loaded.open_counter,
            overlong_index_keys,
        });
        let mut fini = FiniGuard(Some(Rc::clone(&target)));
        // From here the session owns the system libraries (`replayed`), so a
        // failure cannot leave a store without them: each is the pin's
        // death, never a degraded open, and leaves without finalizing.
        let seeded_totals = seed_totals(
            originals,
            &loaded.library_totals,
            &replayed,
            &user_libraries,
        );
        {
            let mut phrases = target.phrases_mut();
            phrases.libraries = user_libraries;
            phrases.index = index;
            phrases.phrase_table = phrase_table;
        }
        // `m_user_bigram->load_db(user_bigram.db)` (`pinyin.cpp:399-401`):
        // the user bigram's one read, after the profile check, so a wiped
        // profile loads empty. Any key counts as user data, an item-less
        // gram included: the pin merges its total (`ngram.cpp:298-306`).
        // The flag only gates a fast path, so a profile that has the file
        // takes the full read without a walk of the container here.
        //
        // Without a user dir the pin loads `g_build_filename(NULL, …)`,
        // `""`, the same way. The container answers only an unsupported
        // linked library, which the system tables' attach has already
        // refused; one the pin cannot create ends the process in the
        // backend, and a Berkeley DB one whose open failed is the pin's
        // failed container.
        let bigram_db = load_user_bigram(target.dir.as_deref()).map_err(|_| {
            fini.abandon();
            UserStoreError::SessionSeed
        })?;
        let has_user_data = holds_user_data(&target, &replayed);

        // The replay's buffers are the sub-indices' items from here on: the
        // session owns them, and a removal is a slot the buffer holds no item
        // at.
        let has_system_items = !replayed.is_empty();
        let has_removed_system_items = replayed
            .values()
            .any(crate::user::replay::ReplayedLibrary::has_removed);
        let library_offsets = seeded_totals.offsets;
        let system_items = crate::user::system_items::SystemItems::new(replayed)
            .with_totals(seeded_totals.totals, seeded_totals.facade_extra);

        let inner = StoreInner {
            has_system_items: Cell::new(has_system_items),
            system_items: RefCell::new(system_items),
            has_removed_system_items: Cell::new(has_removed_system_items),
            offsets: RefCell::new(library_offsets),
            dirty: Cell::new(false),
            write_generation: Cell::new(0),
            has_user_data: Cell::new(has_user_data),
            bigram_failed: crate::store::UserBigramDb::is_failed(&bigram_db),
            bigram_db,
            libpinyin: target,
        };
        Ok(Self::from_parts(
            inner,
            fini,
            std::mem::replace(
                originals,
                SystemOriginals::new(
                    Default::default(),
                    crate::data::user_files::UserFileLayout::stock(),
                ),
            ),
        ))
    }
}

/// What the seed leaves the session's unigram model: the user libraries'
/// stored-total differences, the sub-index totals of the system libraries
/// without a live buffer, and what the facade total holds beyond the system
/// sub-index totals.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SeedTotals {
    pub(crate) offsets: crate::user::LibraryTable<u32>,
    pub(crate) totals: crate::user::LibraryTable<u32>,
    pub(crate) facade_extra: u64,
}

/// The user bigram's one read: the container in `dir`, or the empty one
/// without a user directory.
fn load_user_bigram(dir: Option<&Path>) -> Result<DefaultUserBigramDb, PersistenceError> {
    match dir {
        Some(dir) => persistence::load_user_bigram_db(dir),
        None => DefaultUserBigramDb::empty().map_err(PersistenceError::from),
    }
}

/// Whether the opened session holds user data: an item a replay added, the
/// user bigram's file, or a non-empty arena. The flag only gates a fast path.
fn holds_user_data(
    target: &Target,
    replayed: &crate::user::LibraryTable<crate::user::replay::ReplayedLibrary>,
) -> bool {
    let has_bigram_file = target.dir.as_deref().is_some_and(|dir| {
        crate::input::build_path(
            dir,
            std::ffi::OsStr::new(&crate::data::user_files::UserDbm::Bigram.file_name()),
        )
        .exists()
    });
    replayed
        .values()
        .any(crate::user::replay::ReplayedLibrary::has_added)
        || has_bigram_file
        || target
            .phrases()
            .libraries
            .values()
            .any(|library| !library.is_empty())
}

/// The unigram mass in the loaded USER_FILE arenas, used only to retain
/// the difference from each library's stored total.
fn seeded_mass(
    user_libraries: &crate::user::LibraryTable<crate::user::user_library::UserLibrary>,
) -> crate::user::LibraryTable<u32> {
    user_libraries
        .iter()
        .map(|(&nibble, library)| {
            let total = library
                .items()
                .fold(0_u32, |total, (_, item)| total.wrapping_add(item.unigram()));
            (nibble, total)
        })
        .collect()
}

fn seed_totals(
    originals: &impl crate::user::OriginalLibraries,
    library_totals: &crate::user::LibraryTable<u32>,
    replayed: &crate::user::LibraryTable<crate::user::replay::ReplayedLibrary>,
    user_libraries: &crate::user::LibraryTable<crate::user::user_library::UserLibrary>,
) -> SeedTotals {
    let seeded = seeded_mass(user_libraries);

    // The facade total is the sum of the loaded libraries' totals
    // (`phrase_index.cpp:237-248`). A system library's is its own scalar; a
    // user library's is kept as the difference from its items' sum, and the
    // facade's share of it as `facade_extra`.
    let mut offsets = crate::user::LibraryTable::new();
    let mut totals = crate::user::LibraryTable::new();
    let mut facade_extra = 0_u64;
    for (&nibble, &total) in library_totals {
        if originals.contains_key(&nibble) {
            continue;
        }
        let items = seeded.get(&nibble).copied().unwrap_or(0);
        let offset = total.wrapping_sub(items);
        if offset != 0 {
            offsets.insert(nibble, offset);
        }
        facade_extra = facade_extra.wrapping_add(u64::from(total));
    }
    for (&nibble, &items) in &seeded {
        if !originals.contains_key(&nibble) && !library_totals.contains_key(&nibble) {
            facade_extra = facade_extra.wrapping_add(u64::from(items));
        }
    }
    for (&nibble, original) in originals.iter() {
        if replayed.contains_key(&nibble) {
            continue;
        }
        let total = library_totals.get(&nibble).copied().unwrap_or_else(|| {
            original
                .total
                .wrapping_add(seeded.get(&nibble).copied().unwrap_or(0))
        });
        if total != original.total {
            totals.insert(nibble, total);
        }
    }
    SeedTotals {
        offsets,
        totals,
        facade_extra,
    }
}

/// Reads the session's system items back into the persistence shape — the
/// export half of the value mapping, `UserStore::save_reporting`'s — with
/// the live system items as values, for the tests.
#[cfg(test)]
pub fn export_state(
    store: &UserStore,
    originals: &impl crate::user::OriginalLibraries,
) -> Result<UserState, UserStoreError> {
    let mut state = export_for_save(store)?;
    let Ok(()) = store
        .session_target()
        .phrases()
        .phrase_table
        .for_each_record::<std::convert::Infallible>(|key, value| {
            state.phrase_table.insert(key.to_vec(), value.to_vec());
            Ok(())
        });
    // A session's system items are its live buffers: a save diffs them
    // against the file on disk (`persistence`), and only this test-facing
    // export reads them as values.
    for (token, item) in store.live_system_changes(originals) {
        if let Some(overrides) = state
            .system_overrides
            .get_or_default(crate::vocab::phrase_index_library_index(token))
        {
            overrides.insert(token & crate::vocab::PHRASE_MASK, item);
        }
    }
    Ok(state)
}

/// Reads the session's system items back into the persistence shape — the
/// export half of the value mapping, `UserStore::save_reporting`'s.
#[cfg(test)]
pub(crate) fn export_for_save(store: &UserStore) -> Result<UserState, UserStoreError> {
    let mut state = UserState::default();
    // A session save writes the index, the phrase table and the user
    // libraries directly; the value model carries only system overrides.
    let phrases = store.session_target().phrases();
    for (&nibble, library) in &phrases.libraries {
        if !(1..=4).contains(&nibble) {
            continue;
        }
        for (slot, view) in library.items() {
            let item = crate::data::phrase_library::decode_phrase_item(view.as_bytes())
                .ok_or(UserStoreError::Persistence)?;
            if let Some(overrides) = state.system_overrides.get_or_default(nibble) {
                overrides.insert(slot, Some(item));
            }
        }
    }
    Ok(state)
}

/// The store's dense syllable ids for a reading's packed `ChewingKey`
/// words, decoded from the words' fields; `None` when any word is the zero
/// key, has a field outside the table dimensions or a tone past 5. A key
/// inside the dimensions with no syllable-table row keeps its packed
/// fields: the pin stores a matrix alternate as it stores any key
/// (`phonetic_key_matrix.cpp:285-306`, `pinyin.cpp:3594-3606` at 074a2219).
/// This is the boundary conversion the two key models need —
/// `SyllableKey::index()` is a dense id, `ChewingKey::to_packed()` the
/// 16-bit bit-field the pin's files carry.
pub(crate) fn packed_to_pinyin_keys(packed: &[u16]) -> Option<Vec<crate::user::PinyinKey>> {
    packed
        .iter()
        .map(|&bits| {
            let key = crate::parser::ChewingKey::from_packed(bits);
            if key.with_tone(0).to_packed() == 0 || !key.elements_in_dims() {
                return None;
            }
            let syllable = crate::vocab::SyllableKey::from_matrix_key(key);
            phrase::toned_key(syllable.index(), key.tone)
        })
        .collect()
}

/// 074a2219 storage/pinyin_phrase3.h:68-147: initial equality,
/// incomplete middle/final wildcard, and zero-tone wildcard on either side.
pub(crate) fn pronunciation_matches(
    reading: &[crate::parser::ChewingKey],
    packed: impl ExactSizeIterator<Item = u16>,
) -> bool {
    reading.len() == packed.len()
        && reading.iter().zip(packed).all(|(query, bits)| {
            let stored = crate::parser::ChewingKey::from_packed(bits);
            query.initial == stored.initial
                && ((query.middle == stored.middle && query.final_ == stored.final_)
                    || (query.middle == 0 && query.final_ == 0)
                    || (stored.middle == 0 && stored.final_ == 0))
                && (query.tone == stored.tone || query.tone == 0 || stored.tone == 0)
        })
}

/// The reverse: dense syllable ids, tone included, as packed `ChewingKey`
/// words.
pub(crate) fn pinyin_keys_to_packed(ids: &[crate::user::PinyinKey]) -> Option<Vec<u16>> {
    ids.iter()
        .map(|&id| phrase::key_chewing(id).map(crate::parser::ChewingKey::to_packed))
        .collect()
}

#[cfg(test)]
mod tests;
