//! The libpinyin-file user store — [`crate::UserStore`] over the pin's own
//! user-dir file set (drop-in task 9's wiring half).
//!
//! [`UserStore::open_libpinyin`] opens a user *directory* (not a store
//! file): [`crate::persistence`] reads the profile into the session's
//! state in process memory — the user library arenas, the pinyin index
//! and phrase table, the user bigram container, the live system items and
//! the libraries' stored-total offsets — and every value operation —
//! training, phrase adds, exports, masking — runs against that state.
//! [`UserStore::save_reporting`] then exports the session back through
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
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use oxpinyin_data::chunk_write::ChunkItem;
use oxpinyin_data::user_files::SystemVersions;
use oxpinyin_store::{DefaultUserBigramDb, UserBigramDb};

use crate::persistence::{self, PersistenceError, SystemOriginals, UserConfLaw, UserState};
use crate::phrase;
use crate::store::{StoreInner, UserStore, UserStoreError};

/// The persistence target a session store carries: the user dir, the
/// original system chunks (the `.dbin` diff base), the conformance
/// triple, and the `user.conf` law and open counter the session's saves
/// and its fini write.
#[derive(Debug)]
pub struct Target {
    /// The user directory holding the profile.
    pub(crate) dir: Option<PathBuf>,
    /// Live item arenas and independent pinyin-index membership.
    pub(crate) phrases: RefCell<crate::user_library::SessionPhrases>,
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
    pub(crate) fn phrases(&self) -> Ref<'_, crate::user_library::SessionPhrases> {
        self.phrases.borrow()
    }

    pub(crate) fn phrases_mut(&self) -> RefMut<'_, crate::user_library::SessionPhrases> {
        self.phrases.borrow_mut()
    }
}

/// The facade fini's `user.conf` write ([`persistence::fini`]): libpinyin
/// lowers the open counter its init raised and writes the marker, saved
/// or not (`pinyin_fini`, `pinyin.cpp:1194-1200`); libzhuyin writes
/// nothing. Armed on the handle [`UserStore::open_libpinyin`]
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
        // The two abort sites keep their own variants: neither is a
        // degraded store — one is a failed open (see `persistence::load`),
        // the other a failed save; the C adapters abort on both.
        match error {
            PersistenceError::UnknownDatabaseFormat => UserStoreError::UnknownDatabaseFormat,
            PersistenceError::ChunkHeaderWrite { field, .. } => {
                UserStoreError::ChunkHeaderWrite(field)
            }
            PersistenceError::LibraryChunkHeaderRead => UserStoreError::LibraryChunkHeaderRead,
            PersistenceError::SystemRemapFailed => UserStoreError::SaveRemapFailed,
            PersistenceError::SaveTooLarge => UserStoreError::SaveTooLarge,
            PersistenceError::FatalCopy => UserStoreError::FatalCopy,
            other @ (PersistenceError::Io(_)
            | PersistenceError::Store(_)
            | PersistenceError::Codec(_)) => UserStoreError::Persistence(other.to_string()),
        }
    }
}

impl UserStore {
    /// Open the user store on a libpinyin user directory: read the
    /// profile ([`crate::persistence::load`]) under `law`, seed an
    /// in-memory session store with its values, and carry the persistence target so
    /// [`UserStore::save_reporting`] writes the pin's files back. Dropping the
    /// returned owner is the facade's fini: it
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
    /// be re-written, and [`UserStoreError::SessionSeed`] when the session
    /// cannot be built from the loaded profile, where the pin dies. A
    /// corrupt or partial profile does not fail here — it degrades
    /// per-file, exactly as upstream's loader degrades.
    pub fn open_libpinyin(
        user_dir: &Path,
        mut originals: SystemOriginals,
        versions: SystemVersions,
        law: UserConfLaw,
    ) -> Result<Self, UserStoreError> {
        Self::open_session::<PinSteps>(Some(user_dir), &mut originals, versions, law, None)
    }

    /// Open the pin's NULL-user-directory session entirely in memory.
    /// Imports and lookup work; no profile is read, written or removed.
    /// The failed empty-filename marker diagnostics follow `law`.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError::SessionSeed`] if an in-memory container
    /// cannot be created or seeded with the system libraries' allocation
    /// bases, where the pin dies.
    pub fn open_transient(
        mut originals: SystemOriginals,
        versions: SystemVersions,
        law: UserConfLaw,
    ) -> Result<Self, UserStoreError> {
        Self::open_session::<PinSteps>(None, &mut originals, versions, law, None)
    }

    /// [`open_libpinyin`](Self::open_libpinyin) for a profile
    /// [`check_profile`](Self::check_profile) already judged, over `originals`
    /// that the library loop fills as it walks `rows`: the profile is read,
    /// not judged again.
    ///
    /// # Errors
    ///
    /// As [`open_libpinyin`](Self::open_libpinyin), less the `user.conf`
    /// judgement. The loop's first fault is
    /// [`UserStoreError::LibraryLoop`], with the counter left raised.
    pub fn open_libpinyin_checked(
        user_dir: &Path,
        originals: &mut SystemOriginals,
        rows: &crate::library_loop::LibraryLoopRows<'_>,
        versions: SystemVersions,
        law: UserConfLaw,
        check: crate::persistence::ProfileCheck,
    ) -> Result<Self, UserStoreError> {
        Self::open_session::<PinSteps>(
            Some(user_dir),
            originals,
            versions,
            law,
            Some((check, rows)),
        )
    }

    /// `check_format` alone, for an init that must judge the profile before
    /// it loads the system libraries, as the pin does: the `user.conf`
    /// diagnostic, the marker write and the wipe of a non-conform profile.
    ///
    /// # Errors
    ///
    /// [`UserStoreError::UnknownDatabaseFormat`] at the abort point.
    pub fn check_profile(
        user_dir: &Path,
        versions: &SystemVersions,
        layout: &oxpinyin_data::user_files::UserFileLayout,
        law: UserConfLaw,
    ) -> Result<crate::persistence::ProfileCheck, UserStoreError> {
        Ok(persistence::check_format(user_dir, versions, layout, law)?)
    }

    /// `check_format` for an init without a user dir, whose `user.conf` the
    /// pin names `""` ([`crate::persistence::check_format_unnamed`]): the
    /// pin's failed load and, under libpinyin's law, failed save.
    pub fn check_profile_unnamed(versions: &SystemVersions, law: UserConfLaw) {
        persistence::check_format_unnamed(versions, law);
    }

    // Keeps one shared copy of this function for its three callers
    // (`open_libpinyin`, `open_transient`, `open_libpinyin_checked`).
    // Generic over its steps, LLVM otherwise inlined it into each caller
    // the C libraries reach: `shipping-sizes` measured +65,536 B
    // (libpinyin) and +69,632 B (libzhuyin) stripped at 235438d.
    #[inline(never)]
    fn open_session<P: OpenSteps>(
        user_dir: Option<&Path>,
        originals: &mut SystemOriginals,
        versions: SystemVersions,
        law: UserConfLaw,
        checked: Option<(
            crate::persistence::ProfileCheck,
            &crate::library_loop::LibraryLoopRows<'_>,
        )>,
    ) -> Result<Self, UserStoreError> {
        let profile = if let Some(dir) = user_dir {
            match checked {
                // The library loop's fault: the open returns here, after
                // `check_format`'s writes and the rows before it, before the
                // fini guard is armed, so the raised counter stays on disk.
                Some((check, rows)) => {
                    let (profile, stop) =
                        persistence::load_session_in_loop(dir, originals, rows, &check)?;
                    stop.map_err(UserStoreError::LibraryLoop)?;
                    profile
                }
                None => persistence::load_profile_live(dir, originals, &versions, law)?,
            }
        } else {
            // Without a user dir the pin's `check_format` reads and writes
            // `""`, which the init runs where the pin does
            // ([`Self::check_profile_unnamed`]); there is no profile.
            persistence::LoadedProfile {
                replayed: originals.take_live_libraries(),
                ..Default::default()
            }
        };
        let mut profile = profile;
        P::profile(&mut profile);
        // A system library's log replay reached a pin assertion
        // (rows 91 and 108) on an open without the loop.
        if let Some(fault) = profile.loaded.journal_fault {
            return Err(UserStoreError::Journal(fault));
        }
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
        let mut seed_fault = |error: &dyn std::fmt::Display| {
            fini.abandon();
            UserStoreError::SessionSeed(error.to_string())
        };
        let seeded_totals = seed_totals(
            &loaded.state,
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
            for (&nibble, items) in &loaded.state.libraries {
                for (&slot, item) in items {
                    let bytes = P::encode(item).map_err(|e| seed_fault(&e))?;
                    if let Some(library) = phrases.libraries.get_or_default(nibble) {
                        P::replace(library, slot, &bytes).map_err(|e| seed_fault(&e))?;
                    }
                }
            }
            // An item a system library with an original lacks lives in that
            // library's live buffer below, its only copy. A library with no
            // original has no buffer; its items are held in an arena.
            for (&nibble, items) in &loaded.state.system_overrides {
                if originals.get(&nibble).is_some() {
                    continue;
                }
                for (&slot, item) in items {
                    if let Some(item) = item {
                        let bytes = P::encode(item).map_err(|e| seed_fault(&e))?;
                        if let Some(library) = phrases.libraries.get_or_default(nibble) {
                            P::replace(library, slot, &bytes).map_err(|e| seed_fault(&e))?;
                        }
                    }
                }
            }
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
        let bigram_db = P::bigram(target.dir.as_deref()).map_err(|error| {
            let seeded = seed_fault(&error);
            // `copy_tkrzwdb` / `copy_bdb` assert in `load_db`
            // (`tkrzwdb_utils.h:65`, `:72`; `bdb_utils.h:61`, `:67`): the
            // guard is abandoned as for any seed fault, and the copy keeps
            // its own typed error.
            if matches!(error, PersistenceError::FatalCopy) {
                UserStoreError::FatalCopy
            } else {
                seeded
            }
        })?;
        let has_bigram_file = target.dir.as_deref().is_some_and(|dir| {
            oxpinyin_input::path::build_path(
                dir,
                std::ffi::OsStr::new(&oxpinyin_data::user_files::UserDbm::Bigram.file_name()),
            )
            .exists()
        });
        let has_user_data = replayed
            .values()
            .any(crate::replay::ReplayedLibrary::has_added)
            || has_bigram_file
            || target
                .phrases()
                .libraries
                .values()
                .any(|library| !library.is_empty());

        // The replay's buffers are the sub-indices' items from here on: the
        // session owns them, and a removal is a slot the buffer holds no item
        // at.
        let has_system_items = !replayed.is_empty();
        let has_removed_system_items = replayed
            .values()
            .any(crate::replay::ReplayedLibrary::has_removed);
        let library_offsets = seeded_totals.offsets;
        let system_items = crate::system_items::SystemItems::new(replayed)
            .with_totals(seeded_totals.totals, seeded_totals.facade_extra);

        let inner = StoreInner {
            has_system_items: Cell::new(has_system_items),
            system_items: RefCell::new(system_items),
            has_removed_system_items: Cell::new(has_removed_system_items),
            count_cache: RefCell::new(None),
            offsets: RefCell::new(library_offsets),
            dirty: Cell::new(false),
            write_generation: Cell::new(0),
            phrase_table_generation: Cell::new(0),
            has_user_data: Cell::new(has_user_data),
            bigram_failed: oxpinyin_store::UserBigramDb::is_failed(&bigram_db),
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
                    oxpinyin_data::user_files::UserFileLayout::stock(),
                ),
            ),
        ))
    }
}

/// The steps of a session open, past the hand-over of the system
/// libraries, that can fail. Each default method is the real step, and the
/// open is generic over the steps, so a production open ([`PinSteps`])
/// calls them directly. A test type overrides one to inject its failure
/// (#1576's `load_with` form), since none can be made to fail from data.
trait OpenSteps {
    /// The loaded profile, before the session is built from it.
    #[inline]
    fn profile(_profile: &mut persistence::LoadedProfile) {}

    /// A loaded item's encoding ([`persistence::encode_persisted_item`]).
    #[inline]
    fn encode(item: &ChunkItem) -> Result<Vec<u8>, oxpinyin_data::chunk_write::ChunkWriteError> {
        persistence::encode_persisted_item(item)
    }

    /// The encoded item's placement in its library's arena.
    #[inline]
    fn replace(
        library: &mut crate::user_library::UserLibrary,
        slot: u32,
        bytes: &[u8],
    ) -> Result<(), String> {
        library.replace(slot, bytes)
    }

    /// `m_user_bigram->load_db(user_bigram.db)` (`pinyin.cpp:399-401`,
    /// `zhuyin.cpp:341-343`), or the empty container without a user dir.
    #[inline]
    fn bigram(dir: Option<&Path>) -> Result<DefaultUserBigramDb, PersistenceError> {
        match dir {
            Some(dir) => persistence::load_user_bigram_db(dir),
            None => Ok(DefaultUserBigramDb::empty()?),
        }
    }
}

/// The real [`OpenSteps`]: every production open.
struct PinSteps;

impl OpenSteps for PinSteps {}

/// The totals the loaded profile seeds the session with — the seed half of
/// the value mapping.
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
#[cfg(test)]
pub(crate) fn seed_totals_plain(
    state: &UserState,
    originals: &impl crate::persistence::OriginalLibraries,
    library_totals: &crate::LibraryTable<u32>,
) -> SeedTotals {
    seed_totals(
        state,
        originals,
        library_totals,
        &crate::LibraryTable::new(),
        &crate::LibraryTable::new(),
    )
}

/// The existing owned fixture values and the load's borrowed item share the
/// same row conversion; no session reader or writer uses these views yet.
enum SeedItem<'a> {
    Owned(&'a ChunkItem),
    View(oxpinyin_data::phrase_library::PhraseItemView<'a>),
}

impl SeedItem<'_> {
    fn unigram(&self) -> u32 {
        match self {
            Self::Owned(item) => item.unigram,
            Self::View(item) => item.unigram(),
        }
    }
}

/// The unigram mass the decoded overrides add to each system library on top of
/// its original: the sub-index total a library without a live buffer starts
/// from when no log total names it. A replayed library holds its own total.
fn system_override_mass(
    state: &UserState,
    originals: &impl crate::persistence::OriginalLibraries,
    seeded: &mut crate::LibraryTable<u32>,
) {
    for (&nibble, overrides) in &state.system_overrides {
        let Some(original) = originals.get(&nibble) else {
            continue;
        };
        for (&slot, item) in overrides {
            let Some(new_item) = item else {
                continue; // a replayed removal: the load's `skipped` list
                // carries it
            };
            if !original.is_readable(slot) {
                continue;
            }
            // `is_readable` held, so the item exists and this is its unigram.
            let base = original.unigram(slot).unwrap_or(0);
            if let Some(sum) = seeded.get_or_insert_with(nibble, || 0) {
                *sum = sum.wrapping_add(new_item.unigram.wrapping_sub(base));
            }
        }
    }
}

/// What the seed leaves the session's unigram model: the user libraries'
/// stored-total differences, the sub-index totals of the system libraries
/// without a live buffer, and what the facade total holds beyond the system
/// sub-index totals.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SeedTotals {
    pub(crate) offsets: crate::LibraryTable<u32>,
    pub(crate) totals: crate::LibraryTable<u32>,
    pub(crate) facade_extra: u64,
}

fn seed_totals(
    state: &UserState,
    originals: &impl crate::persistence::OriginalLibraries,
    library_totals: &crate::LibraryTable<u32>,
    replayed: &crate::LibraryTable<crate::replay::ReplayedLibrary>,
    user_libraries: &crate::LibraryTable<crate::user_library::UserLibrary>,
) -> SeedTotals {
    // The unigrams seeded per library, as the facade would sum them.
    let mut seeded: crate::LibraryTable<u32> = crate::LibraryTable::new();

    // ADD records own text/index rows just like USER_FILE items. MODIFY
    // records do not gain index rows (pinyin.cpp:566-607 at 074a2219).
    let mut libraries: crate::LibraryTable<BTreeMap<u32, &ChunkItem>> = state
        .libraries
        .iter()
        .map(|(&nibble, items)| {
            (
                nibble,
                items.iter().map(|(&slot, item)| (slot, item)).collect(),
            )
        })
        .collect();
    for (&nibble, overrides) in &state.system_overrides {
        for (&slot, item) in overrides {
            let Some(item) = item else {
                continue;
            };
            if !originals
                .get(&nibble)
                .is_some_and(|original| original.contains_item(slot))
                && let Some(items) = libraries.get_or_default(nibble)
            {
                items.insert(slot, item);
            }
        }
    }
    let mut seed = |nibble: u8, item: SeedItem<'_>| {
        if let Some(sum) = seeded.get_or_insert_with(nibble, || 0) {
            *sum = sum.wrapping_add(item.unigram());
        }
    };
    for (&nibble, items) in &libraries {
        for item in items.values() {
            seed(nibble, SeedItem::Owned(item));
        }
    }
    for (&nibble, library) in user_libraries {
        for (_, item) in library.items() {
            seed(nibble, SeedItem::View(item));
        }
    }

    system_override_mass(state, originals, &mut seeded);

    // The facade total is the sum of the loaded libraries' totals
    // (`phrase_index.cpp:237-248`). A system library's is its own scalar; a
    // user library's is kept as the difference from its items' sum, and the
    // facade's share of it as `facade_extra`.
    let mut offsets = crate::LibraryTable::new();
    let mut totals = crate::LibraryTable::new();
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
/// export half of the value mapping, [`UserStore::save_reporting`]'s — with
/// the live system items as values, for the tests.
#[cfg(test)]
pub fn export_state(
    store: &UserStore,
    originals: &impl crate::persistence::OriginalLibraries,
) -> Result<UserState, UserStoreError> {
    let mut state = export_for_save(store)?;
    let collected = store
        .session_target()
        .phrases()
        .phrase_table
        .for_each_record::<std::convert::Infallible>(|key, value| {
            state.phrase_table.insert(key.to_vec(), value.to_vec());
            Ok(())
        });
    if let Err(infallible) = collected {
        match infallible {}
    }
    // A session's system items are its live buffers: a save diffs them
    // against the file on disk (`persistence`), and only this test-facing
    // export reads them as values.
    for (token, item) in store.live_system_changes(originals) {
        if let Some(overrides) = state
            .system_overrides
            .get_or_default(phrase::phrase_index_library_index(token))
        {
            overrides.insert(token & crate::phrase::PHRASE_MASK, item);
        }
    }
    Ok(state)
}

/// Reads the session's system items back into the persistence shape — the
/// export half of the value mapping, [`UserStore::save_reporting`]'s.
pub(crate) fn export_for_save(store: &UserStore) -> Result<UserState, UserStoreError> {
    let mut state = UserState::default();
    // A session save writes the index, the phrase-table arena and the user
    // libraries directly; the value model carries only system overrides.
    let phrases = store.session_target().phrases();
    for (&nibble, library) in &phrases.libraries {
        if !(1..=4).contains(&nibble) {
            continue;
        }
        for (slot, view) in library.items() {
            let item = oxpinyin_data::chunk_write::decode_phrase_item(view.as_bytes())
                .map_err(|e| UserStoreError::Persistence(e.to_string()))?;
            if let Some(overrides) = state.system_overrides.get_or_default(nibble) {
                overrides.insert(slot, Some(item));
            }
        }
    }
    Ok(state)
}

/// The store's dense syllable ids for a reading's packed `ChewingKey`
/// words, decoded from the words' fields; `None` when any word is one no
/// pin parser writes (the zero key, an invalid field combination, a tone
/// past 5). This is the boundary conversion the two key models need —
/// `SyllableKey::index()` is a dense id, `ChewingKey::to_packed()` the
/// 16-bit bit-field the pin's files carry.
pub(crate) fn packed_to_pinyin_keys(packed: &[u16]) -> Option<Vec<crate::phrase::PinyinKey>> {
    packed
        .iter()
        .map(|&bits| {
            let key = oxpinyin_core::ChewingKey::from_packed(bits);
            let syllable = oxpinyin_core::SyllableKey::from_chewing_key(key)?;
            phrase::toned_key(syllable.index(), key.tone)
        })
        .collect()
}

/// 074a2219 storage/pinyin_phrase3.h:68-147: initial equality,
/// incomplete middle/final wildcard, and zero-tone wildcard on either side.
pub(crate) fn pronunciation_matches(
    reading: &[oxpinyin_core::ChewingKey],
    packed: impl ExactSizeIterator<Item = u16>,
) -> bool {
    reading.len() == packed.len()
        && reading.iter().zip(packed).all(|(query, bits)| {
            let stored = oxpinyin_core::ChewingKey::from_packed(bits);
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
    use oxpinyin_store::{DefaultStore, StoreError, WriteStore};

    /// The container's gram under `key`, owned: `None` on a miss.
    fn gram(raw: &DefaultUserBigramDb, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let mut value = Vec::new();
        Ok(raw.get(key, &mut value)?.then_some(value))
    }
    use crate::store::UserStore;
    use oxpinyin_core::ChewingKey;

    /// Register row 101: a phrase-table record with a partial token tail is
    /// one the pin's entry walk reads past, in `_add_phrase`'s search, the
    /// promote's `add_index` and the mask's `ProcessEach`
    /// (`phrase_large_table3.cpp:32-39`, `:57-65`, `:103-117` at 074a2219).
    /// Each operation refuses it before changing anything.
    #[test]
    fn a_partial_phrase_table_record_is_refused_before_any_change() {
        let keys = |text: &str| {
            packed_to_pinyin_keys(&[ChewingKey::from_pinyin(text).unwrap().to_packed()]).unwrap()
        };
        let dir = tempdir("partial-phrase-record");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin).unwrap();
        store
            .add_phrase_in(crate::USER_DICTIONARY, &"你".into(), &keys("ni"), Some(5))
            .unwrap();
        {
            let target = store.session_target();
            let mut phrases = target.phrases_mut();
            let words: Vec<u32> = "他".chars().map(u32::from).collect();
            phrases
                .phrase_table
                .push_record(&crate::phrase_table::key_ucs4(&words), &[1, 0, 0, 7, 9]);
            phrases.phrase_table.finish_load();
        }
        let snapshot = |store: &UserStore| format!("{:?}", store.session_target().phrases());
        let before = snapshot(&store);
        let added = store.add_phrase_in(crate::USER_DICTIONARY, &"他".into(), &keys("ta"), Some(5));
        assert!(
            matches!(added, Err(UserStoreError::Persistence(_))),
            "{added:?}"
        );
        assert_eq!(snapshot(&store), before);
        let promoted = store.promote_addon_phrase("他", &[(keys("ta"), 5)], 5);
        assert!(
            matches!(promoted, Err(UserStoreError::Persistence(_))),
            "{promoted:?}"
        );
        assert_eq!(snapshot(&store), before);
        let masked = store.mask_out(0xffff_ffff, 0x0700_0001);
        assert!(
            matches!(masked, Err(UserStoreError::Persistence(_))),
            "{masked:?}"
        );
        assert_eq!(snapshot(&store), before);
    }

    fn profile_snapshot(dir: &std::path::Path) -> BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    std::fs::read(entry.path()).unwrap_or_default(),
                )
            })
            .collect()
    }

    /// The files of a profile other than the staged `.tmp` ones.
    fn settled(snapshot: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
        snapshot
            .iter()
            .filter(|(name, _)| !name.ends_with(".tmp"))
            .map(|(name, bytes)| (name.clone(), bytes.clone()))
            .collect()
    }

    fn open_law(name: &str, law: UserConfLaw) -> (PathBuf, UserStore) {
        let dir = tempdir(name);
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), law).unwrap();
        (dir, store)
    }

    /// Register row 112, `USER_FILE` (maintainer amendment): the compacted
    /// content copy is checked where `SubPhraseIndex::store` makes it. The
    /// save stops there with the earlier files as written (pinyin staged
    /// them, zhuyin renamed them), writes and renames nothing later, leaves
    /// the marker alone and the session dirty; a retry under the limit saves.
    #[test]
    fn a_user_chunk_copy_at_the_limit_stops_the_save_where_it_is() {
        use crate::user_library::TestLimits;
        let keys = |text: &str| {
            packed_to_pinyin_keys(&[ChewingKey::from_pinyin(text).unwrap().to_packed()]).unwrap()
        };
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            let (dir, mut store) = open_law("save-limit-user", law);
            let first = store
                .add_phrase_in(crate::USER_DICTIONARY, &"你".into(), &keys("ni"), Some(5))
                .unwrap();
            store
                .add_phrase_in(crate::USER_DICTIONARY, &"他".into(), &keys("ta"), Some(5))
                .unwrap();
            store.mark_modified();
            let content = 8 + 2 * 16;
            let before = profile_snapshot(&dir);
            {
                let _limits = TestLimits::set(1 << 32, content);
                let result = store.save_reporting();
                assert!(
                    matches!(result, Err(UserStoreError::SaveTooLarge)),
                    "{law:?}: {result:?}"
                );
            }
            assert!(store.is_modified(), "the session stays dirty");
            let after = profile_snapshot(&dir);
            // The libraries before the user dictionary were written.
            if law == UserConfLaw::Pinyin {
                assert!(after.contains_key("gb_char.dbin.tmp"), "staged");
                assert_eq!(settled(&after), settled(&before), "no rename ran");
            } else {
                assert!(!after.contains_key("gb_char.dbin.tmp"));
                assert!(after.contains_key("gb_char.dbin"), "renamed at once");
            }
            // Nothing after the stop: no later library, index or marker file
            // was opened, staged or renamed.
            let created: Vec<_> = after
                .keys()
                .filter(|name| !before.contains_key(*name))
                .cloned()
                .collect();
            // Only the libraries before the user dictionary (nibbles 1 to 6).
            let earlier = [
                "gb_char.dbin",
                "gbk_char.dbin",
                "opengram.dbin",
                "merged.dbin",
                "addon.bin",
                "network.bin",
            ];
            for name in &created {
                let stem = name.strip_suffix(".tmp").unwrap_or(name);
                assert!(
                    earlier.contains(&stem),
                    "{law:?}: later file {name} was touched"
                );
            }
            assert_eq!(after.get("user.conf"), before.get("user.conf"));
            // Under the limit the same session saves.
            assert!(store.remove_user_phrase(first).unwrap());
            {
                let _limits = TestLimits::set(1 << 32, content);
                store.mark_modified();
                store.save_reporting().unwrap();
            }
            assert!(!store.is_modified());
            drop(store);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    fn largest_journal(dir: &std::path::Path) -> u64 {
        profile_snapshot(dir)
            .iter()
            .filter(|(name, _)| name.ends_with(".dbin"))
            .map(|(_, bytes)| bytes.len() as u64 - 8)
            .max()
            .unwrap()
    }

    fn trained(name: &str, law: UserConfLaw) -> (PathBuf, UserStore) {
        let (dir, mut store) = open_law(name, law);
        store
            .train_pronunciations(
                (1_u32 << 24) | 1,
                std::iter::once(vec![ChewingKey::new(23, 0, 1, 0)]),
                5,
            )
            .unwrap();
        store.mark_modified();
        (dir, store)
    }

    /// Register row 112, `SYSTEM_FILE`: the journal actually encoded is the
    /// copy that is checked, at J >= the limit, and J + 1 saves.
    #[test]
    fn a_system_journal_copy_at_the_limit_stops_the_save_and_retries() {
        use crate::user_library::TestLimits;
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            let (probe_dir, mut probe) = trained("save-limit-probe", law);
            probe.save_reporting().unwrap();
            let journal = largest_journal(&probe_dir);
            drop(probe);
            std::fs::remove_dir_all(probe_dir).unwrap();

            let (dir, mut store) = trained("save-limit-log", law);
            let marker = profile_snapshot(&dir).get("user.conf").cloned();
            {
                let _limits = TestLimits::set(1 << 32, journal);
                let result = store.save_reporting();
                assert!(
                    matches!(result, Err(UserStoreError::SaveTooLarge)),
                    "{law:?}: {result:?}"
                );
            }
            assert!(store.is_modified());
            assert_eq!(
                profile_snapshot(&dir).get("user.conf"),
                marker.as_ref(),
                "no marker was written"
            );
            {
                let _limits = TestLimits::set(1 << 32, journal + 1);
                store.save_reporting().unwrap();
            }
            assert!(!store.is_modified());
            assert_eq!(largest_journal(&dir), journal);
            drop(store);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    /// The journal is encoded once, against the baseline the hook reports:
    /// a changed file on disk whose journal fits saves although the journal
    /// against the open-time original reaches the limit.
    #[test]
    fn the_actual_baseline_decides_the_journal_threshold() {
        use crate::persistence::SaveBaseline;
        use crate::user_library::TestLimits;
        let live = || {
            let mut library = originals().remove(&1).unwrap();
            let item = library.items.get_mut(&1).unwrap();
            item.prons[0].1 = 105;
            library
        };
        let sizes = |changed: bool| {
            let (dir, mut store) = trained("save-limit-baseline-probe", UserConfLaw::Pinyin);
            store
                .save_reporting_with_baseline(&[], |nibble, _| match (changed, nibble) {
                    (true, 1) => SaveBaseline::Changed(live()),
                    _ => SaveBaseline::Keep,
                })
                .unwrap();
            let size = largest_journal(&dir);
            drop(store);
            std::fs::remove_dir_all(dir).unwrap();
            size
        };
        let against_original = sizes(false);
        let against_disk = sizes(true);
        assert!(
            against_disk < against_original,
            "{against_disk} {against_original}"
        );

        let (dir, mut store) = trained("save-limit-baseline", UserConfLaw::Pinyin);
        let _limits = TestLimits::set(1 << 32, against_original);
        store
            .save_reporting_with_baseline(&[], |nibble, _| match nibble {
                1 => SaveBaseline::Changed(live()),
                _ => SaveBaseline::Keep,
            })
            .unwrap();
        assert!(!store.is_modified());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A valid empty user chunk keeps the total its header stores
    /// (`phrase_index.cpp:237-248`), and the arena the first import creates
    /// starts from it: with `u32::MAX - 20` left of room, an import of count 5
    /// (unigram 15) fits, and the next is refused by the overflow check
    /// (`add_unigram_frequency` answers `ERROR_INTEGER_OVERFLOW`), so its
    /// item keeps unigram 0.
    #[test]
    fn the_first_import_into_an_empty_chunk_starts_from_its_stored_total() {
        use oxpinyin_data::chunk_format::build_memory_chunk;
        use oxpinyin_data::chunk_write::build_chunk;
        use oxpinyin_data::user_files::read_chunk_payload;
        let dir = tempdir("empty-chunk-total");
        // A profile the library saved once, so the open reads its chunks.
        let mut first =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin).unwrap();
        first.mark_modified();
        first.save_reporting().unwrap();
        drop(first);
        let mut payload = read_chunk_payload(&build_chunk(&[]).unwrap())
            .unwrap()
            .to_vec();
        payload[..4].copy_from_slice(&(u32::MAX - 20).to_le_bytes());
        std::fs::write(dir.join("user.bin"), build_memory_chunk(&payload).unwrap()).unwrap();
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin).unwrap();
        let keys = |text: &str| {
            packed_to_pinyin_keys(&[ChewingKey::from_pinyin(text).unwrap().to_packed()]).unwrap()
        };
        assert_eq!(
            store.unigram_library_total(crate::USER_DICTIONARY).unwrap(),
            u32::MAX - 20
        );
        let unigram = |store: &UserStore, token: u32| {
            store
                .session_target()
                .phrases()
                .item(token)
                .unwrap()
                .unigram()
        };
        let first = store
            .add_phrase_in(crate::USER_DICTIONARY, &"你".into(), &keys("ni"), Some(5))
            .unwrap();
        assert_eq!(unigram(&store, first), 15, "the first import is accepted");
        let second = store
            .add_phrase_in(crate::USER_DICTIONARY, &"他".into(), &keys("ta"), Some(5))
            .unwrap();
        assert_eq!(unigram(&store, second), 0, "the second overflows the total");
        assert_eq!(
            store.unigram_library_total(crate::USER_DICTIONARY).unwrap(),
            u32::MAX - 5
        );
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Ruling 3: a session's arena compacts at end-import, save and mask-out
    /// only. Replacements between them append, so the content grows with the
    /// history as the pin's does, and each point rebuilds it from the items.
    #[test]
    fn the_session_arena_compacts_only_at_the_pins_points() {
        let dir = tempdir("compaction-points");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin).unwrap();
        let keys =
            packed_to_pinyin_keys(&[ChewingKey::from_pinyin("ni").unwrap().to_packed()]).unwrap();
        let length = |store: &UserStore| {
            store
                .session_target()
                .phrases()
                .libraries
                .get(&crate::USER_DICTIONARY)
                .map_or(0, |library| library.content_len())
        };
        let token = store
            .add_phrase_in(crate::USER_DICTIONARY, &"你".into(), &keys, Some(5))
            .unwrap();
        let other = |text: &str| {
            packed_to_pinyin_keys(&[ChewingKey::from_pinyin(text).unwrap().to_packed()]).unwrap()
        };
        let readings = ["a", "e", "ba", "bi", "da", "de", "fa", "fu"];
        for reading in readings {
            store
                .add_phrase_in(
                    crate::USER_DICTIONARY,
                    &"你".into(),
                    &other(reading),
                    Some(0),
                )
                .unwrap();
        }
        let _ = token;
        let grown = length(&store);
        // Each new reading appended a larger copy: 24 + 32 + ... bytes.
        assert!(grown > 200, "dead bytes are not collected: {grown}");
        store.compact_libraries();
        let live = length(&store);
        assert_eq!(live, 8 + 6 + 4 + 9 * 6, "end-import compacts to the item");
        for reading in ["ga", "ge", "ha", "he", "ji", "ka", "ke", "la"] {
            store
                .add_phrase_in(
                    crate::USER_DICTIONARY,
                    &"你".into(),
                    &other(reading),
                    Some(0),
                )
                .unwrap();
        }
        let grown = length(&store);
        assert!(grown > 2 * live);
        store.mark_modified();
        store.save_reporting().unwrap();
        assert_eq!(length(&store), 8 + 6 + 4 + 17 * 6, "save compacts");
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn session_phrases_have_one_arena_owner_through_writes_and_reload() {
        let dir = tempdir("arena-owner");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin).unwrap();
        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let keys = packed_to_pinyin_keys(&[ni.to_packed()]).unwrap();
        let token = store
            .add_phrase_in(crate::USER_DICTIONARY, &"你".into(), &keys, Some(5))
            .unwrap();
        store
            .train_pronunciations(token, std::iter::once(vec![ni]), 7)
            .unwrap();
        assert_eq!(
            store.phrase(token).unwrap().unwrap().pronunciations()[0].count(),
            12
        );
        store.mark_modified();
        store.save_reporting().unwrap();
        store.unload_user_library(crate::USER_DICTIONARY);
        assert!(store.phrase(token).unwrap().is_none());
        store.load_user_library(crate::USER_DICTIONARY);
        assert_eq!(store.user_pronunciation_count(token), Some(1));
        assert_eq!(
            store.phrase(token).unwrap().unwrap().pronunciations()[0].count(),
            12
        );
        assert!(store.phrase_exists(token).unwrap());
        assert_eq!(store.phrase_length(token).unwrap(), Some(1));
        assert_eq!(
            packed_to_pinyin_keys(&store.nth_pronunciation_words(token, 0).unwrap().unwrap())
                .unwrap(),
            keys
        );
        assert_eq!(
            store.nth_pronunciation_words(token, usize::MAX).unwrap(),
            None
        );
        assert!(store.remove_user_phrase(token).unwrap());
        assert!(store.phrase(token).unwrap().is_none());
        assert!(!store.phrase_exists(token).unwrap());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn predicted_prefix_trains_an_arena_resident_user_item() {
        let dir = tempdir("arena-predicted-prefix");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin).unwrap();
        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let keys = packed_to_pinyin_keys(&[ni.to_packed()]).unwrap();
        let token = store
            .add_phrase_in(crate::USER_DICTIONARY, &"你".into(), &keys, Some(5))
            .unwrap();
        assert!(store.with_user_item(token, |_| ()).is_some());
        let delta = store.unigram_delta(token).unwrap();
        let library = store.unigram_library_total(crate::USER_DICTIONARY).unwrap();
        let total = store.unigram_total().unwrap();
        assert_eq!(store.observe_predicted_prefix(token).unwrap(), 69);
        // 3a's token read reaches the same arena item the predicted-prefix
        // write trained (`with_nth_pronunciation_words`), so the unigram
        // update and the getter cannot disagree about which item is live.
        let mut words = None;
        store.with_nth_pronunciation_words(token, 0, |length, reading| {
            words = Some((length, reading.map(<[u8]>::to_vec)));
        });
        assert_eq!(
            words,
            Some((1, Some(ni.to_packed().to_le_bytes().to_vec())))
        );
        // `add_unigram_frequency(token, 69 * 7)` reaches the live item
        // (`pinyin.cpp:2609`), and the item's library and the facade total
        // take the same 483.
        assert_eq!(store.unigram_delta(token).unwrap(), delta + 483);
        assert_eq!(
            store.unigram_library_total(crate::USER_DICTIONARY).unwrap(),
            library + 483
        );
        assert_eq!(store.unigram_total().unwrap(), total + 483);
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A stored word no parser writes is handed out verbatim by both arena
    /// readers, including the padding bit alone (`0x8000`). The pin copies
    /// the item's words (074a2219 phrase_index.cpp:33-50), while
    /// `packed_to_pinyin_keys` has no key for this one.
    #[test]
    fn foreign_stored_word_is_verbatim_through_the_arena_reader() {
        use crate::user_library::UserLibrary;
        use oxpinyin_data::chunk_write::build_chunk;

        let dir = tempdir("arena-foreign-word");
        let store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin).unwrap();
        let item = ChunkItem {
            phrase: vec![0x4f60],
            unigram: 5,
            prons: vec![(vec![0x8000], 7)],
        };
        let library = UserLibrary::load(&build_chunk(&[(1, item)]).unwrap()).unwrap();
        let target = store.session_target();
        let mut phrases = target.phrases_mut();
        phrases.libraries.insert(crate::USER_DICTIONARY, library);
        drop(phrases);
        let token = (u32::from(crate::USER_DICTIONARY) << 24) | 1;
        let mut borrowed = None;
        assert!(
            store
                .with_nth_pronunciation_words(token, 0, |length, words| {
                    borrowed = Some((length, words.map(<[u8]>::to_vec)));
                })
                .is_some()
        );
        assert_eq!(borrowed, Some((1, Some(vec![0x00, 0x80]))));
        assert_eq!(
            store.nth_pronunciation_words(token, 0).unwrap(),
            Some(vec![0x8000])
        );
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

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
        crate::LibraryTable::from([(
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

    #[test]
    fn unloaded_training_keeps_bigram_but_never_materializes_an_item() {
        for law in [UserConfLaw::Pinyin, UserConfLaw::Zhuyin] {
            let dir = tempdir("unloaded-training");
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), law).expect("session");
            let token = 0x0100_0001;
            store.reset_system_library(1);
            let before = store.unigram_total().expect("facade extra");
            assert!(!store.add_system_unigram_frequency(token, 7).unwrap());
            assert_eq!(
                store.add_unigram_outcome(token, 483).unwrap(),
                crate::UnigramAdd::Missing
            );
            assert_eq!(store.observe_selection(1, token).unwrap(), 69);
            store
                .train_pronunciations(
                    token,
                    std::iter::from_fn(|| panic!("unloaded pronunciation step consumed a reading")),
                    69,
                )
                .unwrap();
            assert_eq!(store.unigram_total().unwrap(), before);
            assert!(!store.has_system_items());
            assert_eq!(store.count_delta(Some(1), token).unwrap().bigram_count, 69);
            assert!(store.is_modified());
            store.load_system_libraries(originals()).unwrap();
            assert!(store.add_system_unigram_frequency(token, 7).unwrap());
            assert_eq!(store.system_unigram(token), Some(107));
            drop(store);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn failed_system_reload_keeps_the_unloaded_guard() {
        use oxpinyin_data::chunk_format::build_memory_chunk;
        use oxpinyin_data::user_files::{LogRecord, encode_log_records};

        let dir = tempdir("unloaded-failed-reload");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin).unwrap();
        store.reset_system_library(1);
        let mut log = encode_log_records(&[LogRecord::ModifyHeader {
            old_total: 100,
            new_total: 100,
        }])
        .unwrap();
        log[4..8].copy_from_slice(&1_u32.to_le_bytes());
        std::fs::write(dir.join("gb_char.dbin"), build_memory_chunk(&log).unwrap()).unwrap();
        assert!(matches!(
            store.load_system_libraries(originals()),
            Err(crate::JournalFault::HeaderToken {
                library: 1,
                token: 1
            })
        ));
        assert!(!store.add_system_unigram_frequency(0x0100_0001, 7).unwrap());
        assert!(!store.has_system_items());
        std::fs::remove_file(dir.join("gb_char.dbin")).unwrap();
        store.load_system_libraries(originals()).unwrap();
        assert!(store.add_system_unigram_frequency(0x0100_0001, 7).unwrap());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn user_library_and_add_views_seed_identical_rows_to_owned_items() {
        use crate::user_library::UserLibrary;
        use oxpinyin_data::chunk_write::{build_chunk, encode_phrase_item};
        use oxpinyin_data::user_files::LogRecord;

        let ni = ChewingKey::from_pinyin("ni").unwrap().to_packed();
        let eng = ChewingKey::from_pinyin("eng").unwrap().to_packed();
        let items = BTreeMap::from([
            (
                1,
                ChunkItem {
                    phrase: vec![0x4f60],
                    unigram: 11,
                    prons: vec![
                        (vec![ni], 3),
                        (vec![eng], 7),
                        (vec![ni], 9),
                        (vec![0xffff], 1),
                    ],
                },
            ),
            (
                1024,
                ChunkItem {
                    phrase: vec![0x8000_0000, 0],
                    unigram: 13,
                    prons: vec![(vec![ni, ni], 13)],
                },
            ),
        ]);
        let added = ChunkItem {
            phrase: vec![0x597d],
            unigram: 17,
            prons: vec![(vec![ni | 0x8000], 5), (vec![ni], 12)],
        };
        let originals = originals();
        let legacy = UserState {
            libraries: crate::LibraryTable::from([(5, items.clone()), (7, items.clone())]),
            system_overrides: crate::LibraryTable::from([(
                1,
                BTreeMap::from([(3, Some(added.clone()))]),
            )]),
            ..UserState::default()
        }
        .index_every_reading();
        let mut views = legacy.clone();
        views.libraries.clear();
        views.system_overrides.clear();
        let libraries: crate::LibraryTable<_> = [5, 7]
            .into_iter()
            .map(|nibble| {
                let file = build_chunk(&items.clone().into_iter().collect::<Vec<_>>()).unwrap();
                (nibble, UserLibrary::load(&file).unwrap())
            })
            .collect();
        let mut merge = crate::replay::Merge::new(originals.get(&1));
        assert!(
            merge
                .apply_owned(LogRecord::Add {
                    token: 0x0100_0003,
                    new_item: encode_phrase_item(&added).unwrap(),
                })
                .is_ok()
        );
        let mut skipped = Vec::new();
        let (_, owned, retained) = merge.finish_views(&mut Some(&mut skipped), &"test");
        assert!(
            owned.is_empty(),
            "ADD must remain a view until rows are seeded"
        );
        assert!(skipped.is_empty());
        let replayed = crate::LibraryTable::from([(1, retained.unwrap())]);
        let totals = crate::LibraryTable::from([(1, 117), (5, 1000), (7, 24)]);
        let before_offsets = seed_totals_plain(&legacy, &originals, &totals);
        let after_offsets = seed_totals(&views, &originals, &totals, &replayed, &libraries);
        // A replayed library's total lives in its buffer, not in the map.
        assert_eq!(before_offsets.offsets, after_offsets.offsets);
        assert_eq!(before_offsets.facade_extra, after_offsets.facade_extra);
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

    // Linux only: /proc/self/maps.
    #[cfg(target_os = "linux")]
    #[test]
    fn session_owns_one_writable_whole_file_mapping() {
        let dir = tempdir("one-live-mapping");
        let path = dir.join("whole-file.bin");
        let file =
            oxpinyin_data::chunk_write::build_chunk(&[(1, three_items()[0].clone())]).unwrap();
        let mut payload = file[8..].to_vec();
        payload.resize(16_451, 0);
        let file = oxpinyin_data::chunk_format::build_memory_chunk(&payload).unwrap();
        std::fs::write(&path, &file).unwrap();
        let mapped = oxpinyin_data::phrase_library::PhraseLibrary::open(&path).unwrap();
        let originals: SystemOriginals =
            crate::LibraryTable::from([(1, SystemLibrary::from_library(mapped))]).into();
        let mut store =
            UserStore::open_transient(originals, versions(), UserConfLaw::Pinyin).unwrap();
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
        let rows: Vec<_> = maps
            .lines()
            .filter(|line| line.ends_with(path.to_str().unwrap()))
            .collect();
        assert_eq!(rows.len(), 1, "one persistent mapping: {rows:?}");
        let fields: Vec<_> = rows[0].split_whitespace().collect();
        assert_eq!(fields[1], "rw-p");
        assert_eq!(fields[2], "00000000");
        let (start, end) = fields[0].split_once('-').unwrap();
        let extent =
            usize::from_str_radix(end, 16).unwrap() - usize::from_str_radix(start, 16).unwrap();
        assert!(extent >= file.len(), "whole file mapped");
        let has_provenance = |store: &UserStore| store.system_provenance_recorded(1);
        assert!(!has_provenance(&store), "transfer records no provenance");
        assert_eq!(store.system_unigram(0x0100_0001), Some(100));
        assert!(!has_provenance(&store), "reads record no provenance");
        assert!(
            !store
                .add_system_unigram_frequency(0x0100_0001, u32::MAX)
                .unwrap()
        );
        assert!(!has_provenance(&store), "a refused write records none");
        assert!(store.add_system_unigram_frequency(0x0100_0001, 2).unwrap());
        assert!(
            !has_provenance(&store),
            "a write into a present item's field records none"
        );
        assert_eq!(store.system_unigram(0x0100_0001), Some(102));
        drop(store);
        assert!(
            !std::fs::read_to_string("/proc/self/maps")
                .unwrap()
                .lines()
                .any(|line| line.ends_with(path.to_str().unwrap()))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn header_only_count_survives_session_transfer() {
        use crate::live_system::{LiveSystemItems, Lookup};
        let dir = tempdir("header-only-count");
        let path = dir.join("count.bin");
        let content = [vec![0; 8], vec![255, 17]].concat();
        let offsets = [0_u32, 8, 9, 10, 0];
        let index_two = 17 + offsets.len() as u32 * 4 + 1;
        let mut payload = Vec::new();
        for value in [0, 17, index_two, index_two + content.len() as u32 + 1] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.push(b'#');
        for offset in offsets {
            payload.extend_from_slice(&offset.to_le_bytes());
        }
        payload.push(b'#');
        payload.extend_from_slice(&content);
        payload.push(b'#');
        std::fs::write(
            &path,
            oxpinyin_data::chunk_format::build_memory_chunk(&payload).unwrap(),
        )
        .unwrap();
        let mapped = oxpinyin_data::phrase_library::PhraseLibrary::open(&path).unwrap();
        assert_eq!(mapped.pronunciation_count(1), Some(17));
        assert!(mapped.item(1).is_none());
        let originals: SystemOriginals =
            crate::LibraryTable::from([(1, SystemLibrary::from_library(mapped))]).into();
        let store = UserStore::open_transient(originals, versions(), UserConfLaw::Pinyin).unwrap();
        let live = LiveSystemItems::unmasked(store.originals().phrase_libraries(), &store);
        assert_eq!(live.pronunciation_count(0x0100_0001), Lookup::Found(17));
        assert!(live.read(0x0100_0001, |_| ()).found().is_none());
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    // Linux only: /proc/self/maps, and a 4 GiB sparse mapping on a 64-bit host.
    #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
    #[test]
    fn layout_failed_sparse_library_keeps_and_releases_whole_mapping() {
        let dir = tempdir("layout-failed-whole-map");
        let path = dir.join("sparse.bin");
        let file = std::fs::File::create(&path).unwrap();
        let file_len = (1_u64 << 32) + 8;
        file.set_len(file_len).unwrap();
        drop(file);
        let mapped =
            oxpinyin_data::phrase_library::PhraseLibrary::open_retaining_layout(&path).unwrap();
        assert!(mapped.is_layout_failed());
        let originals: SystemOriginals =
            crate::LibraryTable::from([(1, SystemLibrary::from_library(mapped))]).into();
        let store = UserStore::open_transient(originals, versions(), UserConfLaw::Pinyin).unwrap();
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
        let rows: Vec<_> = maps
            .lines()
            .filter(|line| line.ends_with(path.to_str().unwrap()))
            .collect();
        assert_eq!(
            rows.len(),
            1,
            "one mapping after the failed layout: {rows:?}"
        );
        let fields: Vec<_> = rows[0].split_whitespace().collect();
        assert_eq!(fields[1], "rw-p");
        let (start, end) = fields[0].split_once('-').unwrap();
        let extent =
            u64::from_str_radix(end, 16).unwrap() - u64::from_str_radix(start, 16).unwrap();
        assert!(extent >= file_len, "whole sparse file is mapped");
        assert_eq!(store.system_unigram(0x0100_0001), None);
        drop(store);
        assert!(
            !std::fs::read_to_string("/proc/self/maps")
                .unwrap()
                .lines()
                .any(|line| line.ends_with(path.to_str().unwrap()))
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Opens a profile whose `gb_char.dbin` holds `records` over [`originals`].
    fn open_with_log(
        name: &str,
        records: &[oxpinyin_data::user_files::LogRecord],
    ) -> (PathBuf, UserStore) {
        let payload = oxpinyin_data::user_files::encode_log_records(records).expect("log");
        open_with_payload(name, originals(), &payload)
    }

    /// Opens a profile whose `gb_char.dbin` holds the checksummed `payload`.
    fn open_with_payload(
        name: &str,
        originals: SystemOriginals,
        payload: &[u8],
    ) -> (PathBuf, UserStore) {
        let dir = tempdir(name);
        std::fs::write(
            dir.join("gb_char.dbin"),
            oxpinyin_data::chunk_format::build_memory_chunk(payload).expect("chunk"),
        )
        .expect("write log");
        std::fs::write(
            dir.join("user.conf"),
            oxpinyin_data::user_files::UserTableInfo::conform_to(&versions()).to_text(),
        )
        .expect("write marker");
        let store = UserStore::open_libpinyin(&dir, originals, versions(), UserConfLaw::Pinyin)
            .expect("open");
        (dir, store)
    }

    /// Three single-reading items in library 1, slots 1 to 3.
    fn three_items() -> [ChunkItem; 3] {
        let ni = ChewingKey::new(23, 0, 1, 0).to_packed();
        let make = |word: u32, unigram: u32, frequency: u32| ChunkItem {
            phrase: vec![word],
            unigram,
            prons: vec![(vec![ni], frequency)],
        };
        [
            make(0x4f60, 100, 100),
            make(0x597d, 50, 50),
            make(0x4e16, 30, 30),
        ]
    }

    fn three_item_originals() -> SystemOriginals {
        crate::LibraryTable::from([(
            1_u8,
            SystemLibrary {
                mapped: None,
                total: 180,
                range_end: 4,
                items: three_items()
                    .into_iter()
                    .zip(1_u32..)
                    .map(|(item, slot)| (slot, item))
                    .collect(),
            },
        )])
        .into()
    }

    #[test]
    fn transient_reload_replaces_the_unloaded_system_baseline() {
        let mut store =
            UserStore::open_transient(three_item_originals(), versions(), UserConfLaw::Pinyin)
                .expect("transient session");
        let token = (1_u32 << 24) | 2;
        assert_eq!(store.system_unigram(token), Some(50));
        let mut items = three_items();
        items[1].unigram = 57;
        let replacement = crate::LibraryTable::from([(
            1_u8,
            SystemLibrary {
                mapped: None,
                total: 187,
                range_end: 4,
                items: items
                    .into_iter()
                    .zip(1_u32..)
                    .map(|(item, slot)| (slot, item))
                    .collect(),
            },
        )]);
        store
            .load_system_libraries(replacement.into())
            .expect("reload");
        assert_eq!(store.system_unigram(token), Some(57));
        assert_eq!(store.system_library_total(1), Some(187));
    }

    /// An import after a transient reload merges into the replacement's
    /// item, not into the item the session loaded first: the reading the
    /// replacement holds takes the count, and no old reading comes back.
    #[test]
    fn an_import_after_a_transient_reload_merges_into_the_replacement() {
        let mut store =
            UserStore::open_transient(three_item_originals(), versions(), UserConfLaw::Pinyin)
                .expect("transient session");
        let token = (1_u32 << 24) | 2;
        let ni3 = ChewingKey::new(23, 0, 1, 0).with_tone(3).to_packed();
        let mut items = three_items();
        items[1].prons = vec![(vec![ni3], 50)];
        let replacement = crate::LibraryTable::from([(
            1_u8,
            SystemLibrary {
                mapped: None,
                total: 180,
                range_end: 4,
                items: items
                    .into_iter()
                    .zip(1_u32..)
                    .map(|(item, slot)| (slot, item))
                    .collect(),
            },
        )]);
        store
            .load_system_libraries(replacement.into())
            .expect("reload");
        let keys = packed_to_pinyin_keys(&[ni3]).expect("keys");
        store
            .add_system_phrase_in(1, Some(token), &"好".into(), &keys, Some(1))
            .expect("import");
        assert_eq!(
            store.with_effective_system_item(token, |item| item.prons.clone()),
            Some(vec![(vec![ni3], 51)])
        );
    }

    /// The `(origin, first reading's frequency)` of one token, or its state.
    fn frequency_of(store: &UserStore, token: u32) -> crate::Lookup<(crate::ItemOrigin, u32)> {
        let unloaded = std::cell::Cell::new(0);
        let libraries = store.originals().phrase_libraries();
        crate::LiveSystemItems::new(libraries, store, &unloaded).read(token, |item| {
            (
                item.origin(),
                item.pronunciation(0).map_or(0, |reading| reading.frequency),
            )
        })
    }

    #[test]
    fn an_over_read_item_answers_no_item_and_the_records_around_it_stay() {
        use crate::{ItemOrigin, Lookup};
        use oxpinyin_data::chunk_write::encode_phrase_item;
        use oxpinyin_data::user_files::LogRecord;
        let items = three_items();
        let encode = |item: &ChunkItem| encode_phrase_item(item).expect("item");
        let (mut first, mut third) = (items[0].clone(), items[2].clone());
        first.prons[0].1 += 7;
        third.prons[0].1 += 9;
        let mut fourth = items[0].clone();
        fourth.phrase = vec![0x754c];
        fourth.prons[0].1 = 11;
        // Row 107, option 1: a header naming 132,096 bytes ends past the
        // content buffer, so slot 2 answers no item. The records before and
        // after it still apply.
        let mut over = encode(&items[1]);
        over[0] = 255;
        over[1] = 255;
        let records = [
            LogRecord::ModifyHeader {
                old_total: 180,
                new_total: 180,
            },
            LogRecord::Modify {
                token: 1,
                old_item: encode(&items[0]),
                new_item: encode(&first),
            },
            LogRecord::Modify {
                token: 2,
                old_item: encode(&items[1]),
                new_item: over,
            },
            LogRecord::Modify {
                token: 3,
                old_item: encode(&items[2]),
                new_item: encode(&third),
            },
            // An ADD after it grows the content past the shipped area.
            LogRecord::Add {
                token: (1_u32 << 24) | 4,
                new_item: encode(&fourth),
            },
        ];
        let payload = oxpinyin_data::user_files::encode_log_records(&records).expect("log");
        let shipped_dir = tempdir("over-read-shipped");
        let shipped_path = shipped_dir.join("gb_char.bin");
        let shipped: Vec<_> = items
            .iter()
            .cloned()
            .zip(1_u32..)
            .map(|(item, slot)| (slot, item))
            .collect();
        std::fs::write(
            &shipped_path,
            oxpinyin_data::chunk_write::build_chunk(&shipped).expect("chunk"),
        )
        .expect("write shipped chunk");
        let library = oxpinyin_data::phrase_library::PhraseLibrary::open(&shipped_path)
            .expect("open shipped chunk");
        let originals = crate::LibraryTable::from([(
            1_u8,
            SystemLibrary {
                total: library.total_freq(),
                range_end: library.token_range().end,
                items: BTreeMap::new(),
                mapped: Some(library),
            },
        )])
        .into();
        let (dir, store) = open_with_payload("over-read-item", originals, &payload);
        let token = |slot: u32| (1_u32 << 24) | slot;
        assert_eq!(
            frequency_of(&store, token(1)),
            Lookup::Found((ItemOrigin::Overlay, 107))
        );
        assert_eq!(frequency_of(&store, token(2)), Lookup::Removed);
        assert!(store.system_item_removed(token(2)));
        // Exercise the live-buffer fallback independently of the removal
        // marker; the shipped original must not reappear for an over-read.
        store.test_forget_removal(token(2));
        let unloaded = std::cell::Cell::new(0);
        let live =
            crate::LiveSystemItems::new(store.originals().phrase_libraries(), &store, &unloaded);
        assert_eq!(live.read(token(2), |_| ()), Lookup::Absent);
        assert_eq!(live.pronunciation_count(token(2)), Lookup::Absent);
        assert_eq!(
            frequency_of(&store, token(3)),
            Lookup::Found((ItemOrigin::Overlay, 39))
        );
        assert_eq!(
            frequency_of(&store, token(4)),
            Lookup::Found((ItemOrigin::Overlay, 11))
        );
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
        std::fs::remove_dir_all(shipped_dir).expect("cleanup shipped chunk");
    }

    #[test]
    fn training_creates_a_live_buffer_with_the_saved_library_total() {
        let originals = three_item_originals();
        let original = originals.get(&1).expect("original");
        let token = (1_u32 << 24) | 1;
        let saved_total = u32::MAX - 3;
        let mut items = crate::system_items::SystemItems::default()
            .with_totals(crate::LibraryTable::from([(1, saved_total)]), 0);
        assert_eq!(items.library_total(1, Some(original)), Some(saved_total));
        assert!(items.train_live(
            Some(original),
            token,
            std::iter::once(vec![ChewingKey::new(23, 0, 1, 0)]),
            1
        ));
        assert_eq!(items.library_total(1, Some(original)), Some(saved_total));
        assert_eq!(
            items.add_unigram(Some(original), token, 4),
            Some(crate::replay::UnigramAdd::Refused)
        );
    }

    #[test]
    fn training_a_system_item_writes_the_live_buffer_and_decodes_nothing() {
        use crate::{ItemOrigin, Lookup};
        let (dir, mut store) = open_with_log("train-live-buffer", &[]);
        let token = (1_u32 << 24) | 1;
        // Untouched, the item is the shipped one.
        assert_eq!(
            frequency_of(&store, token),
            Lookup::Absent,
            "the inline fixture has no mapped chunk to read"
        );
        let reading = vec![ChewingKey::new(23, 0, 1, 0)];
        store
            .train_pronunciations(token, std::iter::once(reading), 5)
            .expect("train");
        // The count was written in place in the library's buffer: the read
        // sees it, and no decoded copy of the item was made.
        assert_eq!(
            frequency_of(&store, token),
            Lookup::Found((ItemOrigin::Overlay, 105))
        );
        assert!(store.has_system_items());
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn overlapping_training_is_saved_with_live_unigram_and_compacted_total() {
        use oxpinyin_data::chunk_write::encode_phrase_item;
        use oxpinyin_data::user_files::LogRecord;
        let items = three_items();
        let old = encode_phrase_item(&items[0]).expect("item");
        let mut new = old.clone();
        new[0] = 2;
        new[1] = 2;
        let payload = oxpinyin_data::user_files::encode_log_records(&[LogRecord::Modify {
            token: 1,
            old_item: old,
            new_item: new,
        }])
        .expect("log");
        for prior_add in [false, true] {
            let (dir, mut store) = open_with_payload(
                if prior_add {
                    "overlap-before-add"
                } else {
                    "overlap-only"
                },
                three_item_originals(),
                &payload,
            );
            let first = (1_u32 << 24) | 1;
            let second = (1_u32 << 24) | 2;
            let reading = {
                let unloaded = std::cell::Cell::new(0);
                let live = crate::LiveSystemItems::new(
                    store.originals().phrase_libraries(),
                    &store,
                    &unloaded,
                );
                live.read(first, |item| {
                    item.pronunciation(0)
                        .expect("reading")
                        .keys
                        .iter()
                        .map(ChewingKey::from_packed)
                        .collect::<Vec<_>>()
                })
                .found()
                .expect("item")
            };
            if prior_add {
                assert!(store.add_system_unigram_frequency(second, 2).expect("add"));
            }
            let total = store.system_library_total(1).expect("library total");
            let facade = 180_u32.wrapping_add(store.unigram_total_over(|_| true) as u32);
            store
                .train_pronunciations(first, std::iter::once(reading), 7)
                .expect("train");
            let expected = if prior_add { 59 } else { 57 };
            assert_eq!(store.system_unigram(second), Some(expected));
            assert_eq!(store.system_library_total(1), Some(total));
            assert_eq!(
                180_u32.wrapping_add(store.unigram_total_over(|_| true) as u32),
                facade
            );
            store.mark_modified();
            store.save_reporting().expect("save");
            assert_eq!(store.system_unigram(second), Some(expected));
            assert_eq!(store.system_library_total(1), Some(total + 7));
            assert_eq!(
                180_u32.wrapping_add(store.unigram_total_over(|_| true) as u32),
                facade
            );
            drop(store);
            let reopened = UserStore::open_libpinyin(
                &dir,
                three_item_originals(),
                versions(),
                UserConfLaw::Pinyin,
            )
            .expect("reopen");
            assert_eq!(reopened.system_unigram(second), Some(expected));
            assert_eq!(reopened.system_library_total(1), Some(total + 7));
            drop(reopened);
            std::fs::remove_dir_all(dir).expect("cleanup");
        }
    }

    #[test]
    fn overlapping_training_that_invalidates_another_item_saves_its_absence() {
        use oxpinyin_data::chunk_write::{build_chunk, encode_phrase_item};
        use oxpinyin_data::user_files::LogRecord;
        let items = three_items();
        let pairs: Vec<_> = items
            .iter()
            .cloned()
            .zip(1_u32..)
            .map(|(item, slot)| (slot, item))
            .collect();
        let file = build_chunk(&pairs).expect("chunk");
        let mut payload = file[8..].to_vec();
        let word = |bytes: &[u8], at: usize| {
            u32::from_le_bytes(bytes[at..at + 4].try_into().expect("word"))
        };
        let index = word(&payload, 4) as usize;
        let content = word(&payload, 8) as usize;
        let second_offset = word(&payload, index + 2 * 4) as usize;
        let third_offset = word(&payload, index + 3 * 4);
        let end = word(&payload, 12);
        payload.splice(content + second_offset..content + second_offset, [0, 0]);
        payload[12..16].copy_from_slice(&(end + 2).to_le_bytes());
        payload[index + 2 * 4..index + 3 * 4]
            .copy_from_slice(&((second_offset as u32) + 2).to_le_bytes());
        payload[index + 3 * 4..index + 4 * 4].copy_from_slice(&(third_offset + 2).to_le_bytes());
        let system_dir = tempdir("overlap-header-system");
        let path = system_dir.join("gb_char.bin");
        std::fs::write(
            &path,
            oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("frame"),
        )
        .expect("system chunk");
        let originals = || -> SystemOriginals {
            let mapped =
                oxpinyin_data::phrase_library::PhraseLibrary::open(&path).expect("library");
            crate::LibraryTable::from([(1_u8, SystemLibrary::from_library(mapped))]).into()
        };
        let old = encode_phrase_item(&items[0]).expect("old");
        let mut new = old.clone();
        new[0] = 2;
        new[1] = 2;
        let log = oxpinyin_data::user_files::encode_log_records(&[LogRecord::Modify {
            token: 1,
            old_item: old,
            new_item: new,
        }])
        .expect("log");
        let (dir, mut store) = open_with_payload("overlap-header-profile", originals(), &log);
        let first = (1_u32 << 24) | 1;
        let second = (1_u32 << 24) | 2;
        let reading = {
            let unloaded = std::cell::Cell::new(0);
            let live = crate::LiveSystemItems::new(
                store.originals().phrase_libraries(),
                &store,
                &unloaded,
            );
            live.read(first, |item| {
                item.pronunciation(0)
                    .expect("reading")
                    .keys
                    .iter()
                    .map(ChewingKey::from_packed)
                    .collect::<Vec<_>>()
            })
            .found()
            .expect("item")
        };
        assert_eq!(store.system_unigram(second), Some(50));
        let total = store.system_library_total(1);
        store
            .train_pronunciations(first, std::iter::once(reading), 7)
            .expect("train");
        assert_eq!(store.system_unigram(second), None);
        assert!(store.system_item_removed(second));
        assert_eq!(store.system_library_total(1), total);
        store.mark_modified();
        store.save_reporting().expect("save");
        assert_eq!(store.system_library_total(1), Some(130));
        drop(store);
        let reopened =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("reopen");
        assert_eq!(reopened.system_unigram(second), None);
        drop(reopened);
        std::fs::remove_dir_all(dir).expect("cleanup");
        std::fs::remove_dir_all(system_dir).expect("cleanup system");
    }

    #[test]
    fn a_payload_over_read_reverses_the_applied_records() {
        use crate::{ItemOrigin, Lookup};
        use oxpinyin_data::chunk_write::encode_phrase_item;
        use oxpinyin_data::user_files::LogRecord;
        let items = three_items();
        let encode = |item: &ChunkItem| encode_phrase_item(item).expect("item");
        let (mut first, mut third) = (items[0].clone(), items[2].clone());
        first.prons[0].1 += 7;
        third.prons[0].1 += 9;
        let records = [
            LogRecord::ModifyHeader {
                old_total: 180,
                new_total: 180,
            },
            LogRecord::Modify {
                token: 1,
                old_item: encode(&items[0]),
                new_item: encode(&first),
            },
            LogRecord::Modify {
                token: 3,
                old_item: encode(&items[2]),
                new_item: encode(&third),
            },
        ];
        let mut payload = oxpinyin_data::user_files::encode_log_records(&records).expect("log");
        // The last record's payload now runs past the end of the log.
        payload.truncate(payload.len() - 3);
        let (dir, store) = open_with_payload("over-read-tail", three_item_originals(), &payload);
        let token = |slot: u32| (1_u32 << 24) | slot;
        // The library keeps its pre-replay content: the first record was
        // applied before the walk reached the faulty one, then reversed. The
        // reversed buffer is the library's live buffer.
        assert_eq!(
            frequency_of(&store, token(1)),
            Lookup::Found((ItemOrigin::Overlay, 100))
        );
        assert_eq!(
            frequency_of(&store, token(3)),
            Lookup::Found((ItemOrigin::Overlay, 30))
        );
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn live_items_tell_shipped_overlay_removed_and_unloaded_apart() {
        use crate::{ItemOrigin, LiveSystemItems, Lookup};
        use oxpinyin_data::chunk_write::{build_chunk, encode_phrase_item};
        use oxpinyin_data::phrase_library::PhraseLibrary;
        use oxpinyin_data::user_files::LogRecord;
        use std::cell::Cell;

        let ni = ChewingKey::new(23, 0, 1, 0).to_packed();
        let item = |word: u32, unigram: u32, frequency: u32| ChunkItem {
            phrase: vec![word],
            unigram,
            prons: vec![(vec![ni], frequency)],
        };
        let shipped = [
            (1, item(0x4f60, 100, 100)),
            (2, item(0x597d, 50, 50)),
            (3, item(0x4e16, 30, 30)),
        ];
        let dir = tempdir("live-items");
        let path = dir.join("gb_char.bin");
        std::fs::write(&path, build_chunk(&shipped).expect("chunk")).expect("write chunk");
        let library = PhraseLibrary::open(&path).expect("open");
        let total = library.total_freq();
        let originals: SystemOriginals = crate::LibraryTable::from([(
            1_u8,
            SystemLibrary {
                total,
                range_end: library.token_range().end,
                items: BTreeMap::new(),
                mapped: Some(library),
            },
        )])
        .into();

        let mut modified = shipped[0].1.clone();
        modified.prons[0].1 += 7;
        let records = [
            LogRecord::ModifyHeader {
                old_total: total,
                new_total: total,
            },
            LogRecord::Modify {
                token: 1,
                old_item: encode_phrase_item(&shipped[0].1).expect("old"),
                new_item: encode_phrase_item(&modified).expect("new"),
            },
            LogRecord::Remove {
                token: 2,
                old_item: encode_phrase_item(&shipped[1].1).expect("removed"),
            },
        ];
        let payload = oxpinyin_data::user_files::encode_log_records(&records).expect("log");
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
        let store = UserStore::open_libpinyin(&dir, originals, versions(), UserConfLaw::Pinyin)
            .expect("open");

        let unloaded = Cell::new(0);
        let live = LiveSystemItems::new(store.originals().phrase_libraries(), &store, &unloaded);
        let (replayed, removed, untouched, absent) = (
            (1_u32 << 24) | 1,
            (1_u32 << 24) | 2,
            (1_u32 << 24) | 3,
            (1_u32 << 24) | 9,
        );
        let summary = |item: crate::SystemItem<'_>| {
            let reading = item.pronunciation(0).expect("reading");
            (
                item.origin(),
                item.phrase_length(),
                item.n_pronunciations(),
                reading.frequency,
                reading.keys.iter().collect::<Vec<_>>(),
            )
        };
        // Every item of a library with a live buffer is read from it: the
        // replayed one as the replay wrote it, an untouched one as the chunk
        // stored it.
        assert_eq!(
            live.read(replayed, summary),
            Lookup::Found((ItemOrigin::Overlay, 1, 1, 107, vec![ni]))
        );
        assert_eq!(
            live.read(untouched, summary),
            Lookup::Found((ItemOrigin::Overlay, 1, 1, 30, vec![ni]))
        );
        assert_eq!(
            live.read(untouched, |item| item.phrase_text())
                .found()
                .flatten(),
            oxpinyin_core::PhraseText::from_words([0x4e16])
        );
        assert_eq!(live.read(removed, summary), Lookup::Removed);
        assert_eq!(live.read(absent, summary), Lookup::Absent);
        assert_eq!(live.read((5_u32 << 24) | 1, summary), Lookup::Absent);
        assert!(live.has_overlay());
        assert!(live.filter_active());
        assert!(live.is_visible(untouched) && !live.is_visible(removed));

        // A session item is read under a borrow of its cell; accessors are
        // called after the callback rather than borrowing the cell again.
        assert_eq!(
            live.read(replayed, |_| store.system_items_borrowed()),
            Lookup::Found(true)
        );

        // The header-only count agrees with the view for every state.
        assert_eq!(live.pronunciation_count(replayed), Lookup::Found(1));
        assert_eq!(live.pronunciation_count(untouched), Lookup::Found(1));
        assert_eq!(live.pronunciation_count(removed), Lookup::Removed);
        assert_eq!(live.pronunciation_count(absent), Lookup::Absent);

        // The stored field and the live count are separate reads: the stored
        // one is the chunk's; the live one adds the store's delta once.
        assert_eq!(live.effective_unigram(untouched), Some(30));
        assert_eq!(live.effective_unigram(absent), None);
        assert_eq!(live.effective_unigram(untouched), Some(30));
        assert_eq!(
            live.effective_unigram(replayed),
            Some(u64::from(100_u32) + store.unigram_delta(replayed).expect("delta"))
        );
        assert_eq!(live.effective_unigram(removed), None);
        assert_eq!(live.effective_unigram(absent), None);
        assert_eq!(live.overlay_end(1), store.system_item_end(1));

        // The imported flag is an item the shipped chunk lacks, whatever the
        // mask or a removal says; a chunk item the replay modified is not
        // one.
        assert!(!live.is_imported(replayed).expect("imported"));
        assert!(!live.is_imported(untouched).expect("imported"));
        assert!(!live.is_imported(removed).expect("imported"));
        assert!(!live.is_imported(absent).expect("imported"));
        assert!(!live.is_imported((5_u32 << 24) | 1).expect("imported"));

        // The store's own accessors answer the same.
        assert_eq!(
            live.read(replayed, |item| item.pronunciation(0).map(|r| r.frequency))
                .found()
                .flatten(),
            store.with_effective_system_item(replayed, |item| item.prons[0].1)
        );
        assert!(store.system_item_removed(removed));

        // An unloaded library hides every token, whatever it holds.
        let unloaded = Cell::new(1 << 1);
        let hidden = LiveSystemItems::new(store.originals().phrase_libraries(), &store, &unloaded);
        for token in [replayed, removed, untouched, absent] {
            assert_eq!(hidden.read(token, summary), Lookup::Unloaded);
            assert_eq!(hidden.pronunciation_count(token), Lookup::Unloaded);
            assert!(!hidden.is_visible(token));
            assert_eq!(hidden.effective_unigram(token), None);
        }

        assert!(!hidden.is_imported(replayed).expect("imported"));

        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
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
        let live = store.live_system_changes(&originals());
        assert_eq!(live.len(), 1);
        assert_eq!(live.get(&token), Some(&Some(modified.clone())));
        assert!(store.has_system_items());
        // Saved (a clean session writes nothing) and reopened, the session
        // holds the same overrides.
        store.save_reporting().expect("save");
        drop(store);
        let reopened =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("reopen");
        assert_eq!(reopened.live_system_changes(&originals()), live);
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
        assert_eq!(
            store.live_system_changes(&originals()),
            BTreeMap::from([(token, None)])
        );
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
        assert!(store.live_system_changes(&originals()).is_empty());
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
        assert!(matches!(
            store.mask_out(u32::MAX, 0),
            Err(UserStoreError::PartialUserBigramTail)
        ));
        assert_eq!(store.bigram_total(1).expect("unchanged total"), 7);
        assert_eq!(store.bigram_count(1, 9).expect("unchanged item"), 7);
        let raw = persistence::load_user_bigram_db(&dir).expect("raw container");
        assert_eq!(gram(&raw, &key).expect("raw get"), Some(value));

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
        store.save_reporting().expect("save masked gram");
        let raw = persistence::load_user_bigram_db(&dir).expect("raw masked container");
        let persisted_after_mask = gram(&raw, &key).expect("raw masked get");
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
        assert!(store.has_oob_bigram_value_for(1));
        let raw = persistence::load_user_bigram_db(&dir).expect("raw itemless container");
        assert_eq!(gram(&raw, &key).expect("raw itemless get"), Some(itemless));
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
            assert!(matches!(
                refused,
                Err(UserStoreError::PartialUserBigramTail)
            ));
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
        assert_eq!(gram(&raw, &key).expect("raw get"), Some(value.clone()));
        drop(raw);
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("reopen");
        assert!(store.has_oob_bigram_value_for(1));
        assert_eq!(store.bigram_total(1).expect("reopened total"), 7);
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// N2: both training paths read the predecessor's total
    /// (`phonetic_lookup.h:884`, `pinyin.cpp:2630` at the pin `074a2219`),
    /// and a value too short for one is the pin's assert
    /// (`memory_chunk.h:390`, register row 85), which the C adapters
    /// reproduce by aborting on the fault. Training reads it before it
    /// changes anything; the predicted path makes its unigram add first
    /// (`pinyin.cpp:2609-2612`), in memory, and returns the fault after it.
    #[test]
    fn a_short_predecessor_value_is_the_fault_of_both_training_paths() {
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
        assert!(matches!(
            store.observe_selection(1, 0x0100_0001),
            Err(UserStoreError::ShortUserBigramValue)
        ));
        assert_eq!(store.generation(), generation, "training wrote nothing");
        assert_eq!(store.system_unigram(0x0100_0001), Some(100));
        assert!(matches!(
            store.observe_predicted(1, 0x0100_0001),
            Err(UserStoreError::ShortUserBigramValue)
        ));
        assert_eq!(
            store.system_unigram(0x0100_0001),
            Some(100 + 483),
            "the predicted unigram add precedes the bigram load"
        );
        assert_eq!(store.bigram_row(1).expect("row"), Some(short));
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
            .reload_system_libraries(originals(), 0xffff_ffff, 0x0100_0002)
            .expect("reload");
        assert_eq!(store.system_unigram(0x0100_0001), Some(100));
        assert_eq!(store.bigram_row(1).expect("row"), gram);
        assert_eq!(store.bigram_count(1, 0x0100_0001).expect("count"), 69);
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// Preflight replays training on copies. Whether an unpartitioned
    /// gram trains or refuses, applying the same step must agree and
    /// must not reorder or clean its existing records.
    #[test]
    fn training_preflight_and_apply_agree_without_reordering_stored_grams() {
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
        let generation = store.generation();
        let mut check = crate::TrainingCheck::default();
        store
            .check_training_step_with(&mut check, 3, token(1))
            .expect("a clean step");
        let preflight = store.check_training_step_with(&mut check, 2, token(5));
        let refused = matches!(preflight, Err(UserStoreError::TrainingSetFreqMiss));
        assert!(preflight.is_ok() || refused);
        assert_eq!(store.generation(), generation, "preflight writes nothing");
        assert_eq!(store.bigram_row(2).expect("row"), Some(gram.clone()));
        assert_eq!(store.system_unigram(token(1)), Some(100));
        assert_eq!(store.system_unigram(token(5)), None);
        assert!(!store.is_modified());
        let applied = store.observe_selection(2, token(5));
        assert_eq!(applied.is_ok(), preflight.is_ok());
        if refused {
            assert!(matches!(applied, Err(UserStoreError::TrainingSetFreqMiss)));
            assert_eq!(store.generation(), generation, "a refusal writes nothing");
            assert_eq!(store.system_unigram(token(1)), Some(100));
            assert_eq!(store.system_unigram(token(5)), None);
            assert_eq!(store.bigram_row(2).expect("row"), Some(gram));
            assert!(!store.is_modified());
        } else {
            let trained = store.bigram_row(2).expect("row").expect("stored gram");
            let original: Vec<_> = oxpinyin_core::single_gram::retrieve_all(&gram).collect();
            let retained: Vec<_> = oxpinyin_core::single_gram::retrieve_all(&trained)
                .filter(|&(item, _)| item != token(5))
                .collect();
            assert_eq!(retained, original);
            assert_eq!(store.bigram_count(2, token(5)).expect("trained count"), 69);
            assert!(store.is_modified());
        }
        drop(store);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    /// Every token in `[high, low]` is at least `low`, so the predicate
    /// is partitioned despite the stored order. Its first-not-less item
    /// is `high`; the training walk stops there and inserts a duplicate
    /// without sorting the row (`phonetic_lookup.h:888-903` at 074a2219).
    #[test]
    fn training_preserves_a_partitioned_unsorted_gram_and_its_duplicates() {
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
            Err(UserStoreError::PartialUserBigramTail)
        ));
        assert!(matches!(
            store.observe_predicted(1, 0x0100_0002),
            Err(UserStoreError::PartialUserBigramTail)
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
    fn a_removal_assert_comes_before_the_bigram_fault() {
        // `pinyin_remove_user_candidate` asserts on a missing item
        // (`pinyin.cpp:3743`) before its `mask_out` (`:3766`) reads any
        // gram, so a malformed gram does not answer for an absent token.
        let dir = tempdir("remove-assert-before-bigram");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        store.plant_raw_bigram(1, &[1, 0, 0]);
        let absent = (u32::from(crate::USER_DICTIONARY) << 24) | 0x0000_0042;
        assert!(matches!(store.remove_user_phrase(absent), Ok(false)));
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
        store.save_reporting().expect("save masked gram");
        let raw = persistence::load_user_bigram_db(&dir).expect("raw masked container");
        let persisted_after_mask = gram(&raw, &key).expect("raw masked get");
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
            gram(&raw, &key).expect("raw get"),
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
    fn mask_out_replays_the_log_it_reads_and_returns_its_faults() {
        use crate::JournalFault;
        use oxpinyin_data::chunk_format::build_memory_chunk;
        use oxpinyin_data::chunk_write::encode_phrase_item;
        use oxpinyin_data::user_files::{LogRecord, encode_log_records};

        let dir = tempdir("mask-faults");
        let original = originals();
        let old = original[&1].item(1).expect("original item");
        let mut new = old.clone();
        new.phrase = vec![0x9f98];
        let header = LogRecord::ModifyHeader {
            old_total: 100,
            new_total: 100,
        };
        let modify = LogRecord::Modify {
            token: 1,
            old_item: encode_phrase_item(&old).expect("old item"),
            new_item: encode_phrase_item(&new).expect("new item"),
        };
        let valid = encode_log_records(&[header.clone(), modify.clone()]).expect("valid log");
        let mut non_null_header =
            encode_log_records(&[modify.clone(), header.clone()]).expect("header");
        let at = non_null_header.len() - 18 + 4;
        non_null_header[at..at + 4].copy_from_slice(&1_u32.to_le_bytes());
        let duplicate_headers =
            encode_log_records(&[header.clone(), header.clone()]).expect("duplicate headers");
        let surviving_add = encode_log_records(&[
            header.clone(),
            LogRecord::Add {
                token: 2,
                new_item: encode_phrase_item(&new).expect("added item"),
            },
        ])
        .expect("surviving ADD");
        let surviving_remove = encode_log_records(&[
            header.clone(),
            LogRecord::Remove {
                token: 1,
                old_item: encode_phrase_item(&old).expect("removed item"),
            },
        ])
        .expect("surviving REMOVE");
        // `_compute_new_header` runs over every kept record before
        // `_mask_out_records`: a short kept MODIFY after a kept ADD faults
        // first.
        let short_kept = encode_log_records(&[
            header.clone(),
            LogRecord::Add {
                token: 2,
                new_item: encode_phrase_item(&new).expect("added item"),
            },
            LogRecord::Modify {
                token: 3,
                old_item: vec![0; 5],
                new_item: vec![0; 6],
            },
        ])
        .expect("short kept payload");
        let mut store = UserStore::open_libpinyin(
            &dir,
            original.fixture_copy(),
            versions(),
            UserConfLaw::Pinyin,
        )
        .expect("open");
        let path = dir.join("gb_char.dbin");
        let library = 1;
        for (log, fault) in [
            (
                non_null_header,
                JournalFault::HeaderToken { library, token: 1 },
            ),
            (duplicate_headers, JournalFault::MultipleHeaders { library }),
            (
                surviving_add,
                JournalFault::SurvivingRecord { library, token: 2 },
            ),
            (
                surviving_remove,
                JournalFault::SurvivingRecord { library, token: 1 },
            ),
            (
                short_kept,
                JournalFault::ShortMaskedItem { library, token: 3 },
            ),
        ] {
            std::fs::write(&path, build_memory_chunk(&log).expect("frame")).expect("log");
            assert!(matches!(
                store.reload_system_libraries(original.fixture_copy(), 0x00ff_ffff, 0),
                Err(found) if found == fault
            ));
        }
        std::fs::write(&path, build_memory_chunk(&valid).expect("frame")).expect("log");
        store.mask_out(0x00ff_ffff, 0).expect("mask tables");
        store
            .reload_system_libraries(original.fixture_copy(), 0x00ff_ffff, 0)
            .expect("replay");
        assert_eq!(
            store
                .system_item_override(0x0100_0001)
                .expect("item")
                .expect("override"),
            new,
        );
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    /// Library 1's ADD of slot 2 and MODIFY of slot 1 (你 to 龘), with the
    /// header first when `header` is true.
    fn add_and_modify_log(header: bool) -> Vec<u8> {
        use oxpinyin_data::chunk_format::build_memory_chunk;
        use oxpinyin_data::chunk_write::encode_phrase_item;
        use oxpinyin_data::user_files::{LogRecord, encode_log_records};

        let old = originals()[&1].item(1).expect("original item");
        let mut new = old.clone();
        new.phrase = vec![0x9f98];
        let mut records = Vec::new();
        if header {
            records.push(LogRecord::ModifyHeader {
                old_total: 100,
                new_total: 100,
            });
        }
        records.push(LogRecord::Add {
            token: 2,
            new_item: encode_phrase_item(&new).expect("added item"),
        });
        records.push(LogRecord::Modify {
            token: 1,
            old_item: encode_phrase_item(&old).expect("old item"),
            new_item: encode_phrase_item(&new).expect("new item"),
        });
        build_memory_chunk(&encode_log_records(&records).expect("log")).expect("frame")
    }

    /// The replayed phrase of slot 1 (`None` keeps the original 你) and the
    /// unigram of slot 2 in library 1.
    fn slot_state(store: &UserStore) -> (Option<Vec<u32>>, Option<u32>) {
        let item = store.system_item_override(0x0100_0001).expect("item");
        (
            item.map(|item| item.phrase),
            store.system_unigram(0x0100_0002),
        )
    }

    /// A masked log with no header is an empty log (`_peek_header`): the
    /// mask keeps the ADD but nothing aborts, and the MODIFY is not applied.
    #[test]
    fn a_masked_log_with_no_header_replays_nothing() {
        let dir = tempdir("mask-no-header");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        std::fs::write(dir.join("gb_char.dbin"), add_and_modify_log(false)).expect("log");
        store
            .reload_system_libraries(originals(), 0x00ff_ffff, 0)
            .expect("no fault");
        assert_eq!(slot_state(&store), (None, None));
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    /// A mask that selects the ADD's own token drops that ADD, and the walk
    /// keeps the MODIFY after it.
    #[test]
    fn a_mask_on_an_add_token_drops_the_add_and_keeps_the_records_after_it() {
        let dir = tempdir("mask-selected-add");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        std::fs::write(dir.join("gb_char.dbin"), add_and_modify_log(true)).expect("log");
        store
            .reload_system_libraries(originals(), 0x00ff_ffff, 2)
            .expect("the ADD is dropped");
        assert_eq!(slot_state(&store), (Some(vec![0x9f98]), None));
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    /// A mask whose library bits exclude library 1 reloads it without its
    /// log: the saved ADD is dropped and its token answers no item.
    #[test]
    fn a_library_the_mask_does_not_select_reloads_without_its_log() {
        let dir = tempdir("mask-unselected-library");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        std::fs::write(dir.join("gb_char.dbin"), add_and_modify_log(true)).expect("log");
        store.load_system_libraries(originals()).expect("merge");
        assert_eq!(slot_state(&store), (Some(vec![0x9f98]), Some(100)));
        store
            .reload_system_libraries(originals(), 0xffff_ffff, 0)
            .expect("the log is not read");
        assert_eq!(slot_state(&store), (None, None));
        assert_eq!(
            store.add_unigram_outcome(0x0100_0002, 7).expect("add"),
            crate::UnigramAdd::Missing
        );
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
            let mut store = UserStore::open_libpinyin(
                &dir,
                base.fixture_copy(),
                versions(),
                UserConfLaw::Pinyin,
            )
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
            store.save_reporting().expect("save");
        }
        {
            let mut store = UserStore::open_libpinyin(
                &dir,
                base.fixture_copy(),
                versions(),
                UserConfLaw::Pinyin,
            )
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
            store.save_reporting().expect("save");
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
            let mut store = UserStore::open_libpinyin(
                &dir,
                base.fixture_copy(),
                versions(),
                UserConfLaw::Pinyin,
            )
            .expect("open");
            store
                .add_system_phrase_in(1, Some(token.value()), &"你".into(), &dense(2), Some(20))
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
                .add_system_phrase_in(1, Some(token.value()), &"你".into(), &dense(2), Some(7))
                .expect("import after training");
            assert_eq!(counts(&store), [100, 96]);
            store.save_reporting().expect("save");
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
            store.save_reporting().expect("save again");
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
            UserStore::open_libpinyin(&dir, base.fixture_copy(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        store
            .add_system_phrase_in(1, Some(0x0100_0001), &"你".into(), &dense(2), Some(20))
            .expect("saved reading");
        store.mark_modified();
        store.save_reporting().expect("save");
        store
            .add_system_phrase_in(1, Some(0x0100_0001), &"你".into(), &dense(4), Some(30))
            .expect("unsaved reading");
        let saved = counts(&store);
        assert_eq!(saved.len(), 3, "the unsaved reading is live");
        // A reload merges only the saved log: the unsaved reading is gone.
        store
            .load_system_libraries(base.fixture_copy())
            .expect("reload from the saved log");
        assert_eq!(counts(&store).len(), 2);
        // An unload takes the whole library, the saved log included; the
        // next load puts the saved state back.
        store.unload_system_libraries(base.fixture_copy());
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

    /// ADD copies an empty body and adds its unigram after the log header
    /// (074a2219 phrase_index.cpp:453-459, :503-514). Reload must retain
    /// the same item, including empty-key readings, as initial raw replay.
    #[test]
    fn persisted_empty_add_survives_reload_save_and_reopen() {
        use oxpinyin_data::user_files::LogRecord;
        for prons in [vec![], vec![(vec![], 13), (vec![], 5)]] {
            let item = ChunkItem {
                phrase: vec![],
                unigram: 37,
                prons,
            };
            let records = [
                LogRecord::ModifyHeader {
                    old_total: 100,
                    new_total: 137,
                },
                LogRecord::Add {
                    token: 9,
                    new_item: persistence::journal_item_bytes(&item),
                },
            ];
            let (dir, mut store) = open_with_log("persisted-empty-add", &records);
            let token = 0x0100_0009;
            let observe = |store: &UserStore| {
                assert_eq!(store.phrase_text(token).unwrap(), Some(vec![]));
                assert_eq!(store.unigram_delta(token).unwrap(), 37);
                assert_eq!(
                    store.system_item_override(token).unwrap(),
                    Some(item.clone())
                );
                assert_eq!(
                    store.user_pronunciation_count(token),
                    Some(item.prons.len() as u32)
                );
                for (nth, _) in item.prons.iter().enumerate() {
                    assert_eq!(
                        store.nth_pronunciation_words(token, nth).unwrap(),
                        Some(vec![])
                    );
                }
            };
            // The facade total is the sub-index total plus what no sub-index
            // took: replay's second count lives in the sub-index until a
            // compact rebuilds it from the items.
            let facade = |store: &UserStore| {
                u64::from(store.system_library_total(1).unwrap())
                    .wrapping_add(store.unigram_total().unwrap())
            };
            observe(&store);
            assert_eq!(store.system_library_total(1), Some(174));
            assert_eq!(store.unigram_total().unwrap(), 0);
            store.unload_system_libraries(originals());
            assert!(store.with_user_item(token, |_| ()).is_none());
            assert!(store.system_item_override(token).unwrap().is_none());
            store.load_system_libraries(originals()).unwrap();
            observe(&store);
            assert_eq!(facade(&store), 174);
            store.mark_modified();
            store.save_reporting().unwrap();
            observe(&store);
            assert_eq!(
                store.system_library_total(1),
                Some(137),
                "save compacts the sub-index only"
            );
            assert_eq!(facade(&store), 174, "the facade keeps what it held");
            let exported = export_state(&store, &originals()).unwrap();
            assert_eq!(exported.system_overrides[&1][&9], Some(item.clone()));
            drop(store);
            let reopened =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .unwrap();
            observe(&reopened);
            drop(reopened);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }

    #[test]
    fn import_readings_refresh_after_reborrowing_the_owner() {
        let dir = tempdir("import-reading-memo");
        let readings = distinct_readings();
        let dense = |packed| packed_to_pinyin_keys(&[packed]).expect("dense");
        let keys = dense(readings[0]);
        let extra = dense(readings[1]);
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        let user = store
            .add_phrase_in(7, &"龘".into(), &keys, Some(5))
            .expect("new user item");
        store
            .add_phrase_in(7, &"龘".into(), &keys, Some(7))
            .expect("other user writer");
        store
            .add_phrase_in(7, &"龘".into(), &keys, Some(3))
            .expect("refresh user memo");
        assert_eq!(
            store
                .phrase(user)
                .expect("phrase")
                .expect("present")
                .pronunciations()[0]
                .count(),
            15
        );

        let system = store
            .add_system_phrase_in(1, None, &"拟".into(), &keys, Some(5))
            .expect("new system item");
        store
            .add_system_phrase_in(1, Some(system), &"拟".into(), &extra, Some(7))
            .expect("append cached row");
        store
            .add_system_phrase_in(1, Some(system), &"拟".into(), &extra, Some(11))
            .expect("other system writer");
        store
            .add_system_phrase_in(1, Some(system), &"拟".into(), &extra, Some(3))
            .expect("refresh system memo");
        assert_eq!(
            store.with_effective_system_item(system, |item| item.prons[1].1),
            Some(21)
        );
        assert_eq!(
            store
                .phrase(system)
                .expect("phrase")
                .expect("present")
                .pronunciations()[1]
                .count(),
            21
        );
        store.mark_modified();
        store.save_reporting().expect("save");
        drop(store);
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("reopen");
        assert_eq!(
            store
                .phrase(user)
                .expect("user")
                .expect("present")
                .pronunciations()[0]
                .count(),
            15
        );
        assert_eq!(
            store.with_effective_system_item(system, |item| item.prons[1].1),
            Some(21)
        );
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
                .add_phrase_in(7, &"龘".into(), &dense(readings[0]), Some(1))
                .expect("add");
            assert_eq!(store.user_pronunciation_count(token), Some(1));
            store
                .add_phrase_in(7, &"龘".into(), &dense(readings[0]), Some(9))
                .expect("merge");
            assert_eq!(store.user_pronunciation_count(token), Some(1));
            for (index, &reading) in readings[1..255].iter().enumerate() {
                store
                    .add_phrase_in(7, &"龘".into(), &dense(reading), Some(1))
                    .expect("append");
                assert_eq!(
                    store.user_pronunciation_count(token),
                    Some(index as u32 + 2)
                );
            }
            store
                .add_phrase_in(7, &"龘".into(), &dense(readings[255]), Some(1))
                .expect("wrap");
            assert_eq!(store.user_pronunciation_count(token), Some(0));
            store
                .add_phrase_in(7, &"龘".into(), &dense(readings[256]), Some(1))
                .expect("257th");
            assert_eq!(store.user_pronunciation_count(token), Some(1));
            store.mark_modified();
            store.save_reporting().expect("save");
            store.unload_user_library(7);
            assert_eq!(store.user_pronunciation_count(token), None);
            store.load_user_library(7);
            assert_eq!(store.user_pronunciation_count(token), Some(1));
        }
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("reopen");
        assert_eq!(store.user_pronunciation_count(token), Some(1));
        store.mask_out(0xffff_ffff, token).expect("mask");
        assert_eq!(store.user_pronunciation_count(token), None);
        let other = store
            .add_phrase_in(7, &"龖".into(), &dense(readings[0]), Some(1))
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
            .add_phrase_in(crate::USER_DICTIONARY, &"龘".into(), &dense(2), Some(20))
            .expect("unsaved item");
        store.unload_user_library(crate::USER_DICTIONARY);
        assert!(readings(&store).is_empty());
        store.load_user_library(crate::USER_DICTIONARY);
        assert!(readings(&store).is_empty());
        // A saved item returns with its reading still indexed; an unsaved
        // reading merged after the save does not.
        store
            .add_phrase_in(crate::USER_DICTIONARY, &"鱻".into(), &dense(2), Some(20))
            .expect("item");
        store.mark_modified();
        store.save_reporting().expect("save");
        store
            .add_phrase_in(crate::USER_DICTIONARY, &"鱻".into(), &dense(4), Some(30))
            .expect("unsaved reading");
        assert_eq!(readings(&store), [vec![true, false]]);
        store.unload_user_library(crate::USER_DICTIONARY);
        assert!(readings(&store).is_empty());
        store.load_user_library(crate::USER_DICTIONARY);
        assert_eq!(readings(&store), [vec![true]]);
        // A different phrase handed a dropped token is found under the
        // reading the index kept for it.
        let ni_key = || oxpinyin_core::SyllableKey::from_text("ni").expect("ni");
        store.unload_user_library(crate::USER_DICTIONARY);
        store.load_user_library(crate::USER_DICTIONARY);
        let reused = store
            .add_phrase_in(crate::USER_DICTIONARY, &"鱺".into(), &dense(3), Some(20))
            .expect("another phrase takes the dropped token");
        let found = store.lookup().lookup(&[ni_key()]);
        assert!(found.iter().any(|entry| entry.token().value() == reused));
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn library_transitions_keep_the_independent_index() {
        let dir = tempdir("library-transition-index");
        let ni = ChewingKey::from_pinyin("ni").expect("ni");
        let dense = |tone| packed_to_pinyin_keys(&[ni.with_tone(tone).to_packed()]).expect("dense");
        let kept = |dir: &std::path::Path| {
            persistence::load(dir, &originals(), &versions(), UserConfLaw::Pinyin)
                .expect("load")
                .state
                .indexed
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
            .add_phrase_in(crate::USER_DICTIONARY, &"龘".into(), &dense(2), Some(20))
            .expect("item");
        // `FacadePhraseIndex::unload` drops only the sub-index
        // (`phrase_index.cpp:261-268`): the item goes, the user pinyin index
        // keeps its entry.
        store.unload_user_library(crate::USER_DICTIONARY);
        assert!(readings(&store).is_empty());
        // The unloaded library's chunk is skipped; the index arena is not,
        // so the saved profile carries the reading with no item behind it.
        store.mark_modified();
        store
            .save_reporting()
            .expect("the index row is saved, the item's chunk is skipped");
        let indexed = kept(&dir);
        assert_eq!(indexed.len(), 1);
        // The load builds the empty sub-index (`create_sub_phrase`,
        // `pinyin.cpp:316`) and the index entry stays.
        store.load_user_library(crate::USER_DICTIONARY);
        assert!(readings(&store).is_empty());
        drop(store);
        assert_eq!(kept(&dir), indexed);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    /// `_write_files` and `_rename_files` skip a library whose sub-index
    /// `unload` freed (074a2219 `pinyin.cpp:929-932`, `:1032-1035`;
    /// `zhuyin.cpp:562-565`): its saved chunk stays as it was, and the next
    /// load brings it back.
    #[test]
    fn a_save_skips_an_unloaded_user_library() {
        let dir = tempdir("save-unloaded-user-library");
        let ni = ChewingKey::from_pinyin("ni").expect("ni");
        let dense = |tone| packed_to_pinyin_keys(&[ni.with_tone(tone).to_packed()]).expect("dense");
        let chunk = dir.join("user.bin");
        let mut store =
            UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                .expect("open");
        let saved = store
            .add_phrase_in(crate::USER_DICTIONARY, &"龘".into(), &dense(2), Some(20))
            .expect("saved item");
        store.mark_modified();
        store.save_reporting().expect("save");
        let saved_chunk = std::fs::read(&chunk).expect("chunk");
        let unsaved = store
            .add_phrase_in(crate::USER_DICTIONARY, &"鱻".into(), &dense(4), Some(30))
            .expect("unsaved item");
        store.unload_user_library(crate::USER_DICTIONARY);
        store.mark_modified();
        store.save_reporting().expect("save while unloaded");
        assert_eq!(std::fs::read(&chunk).expect("chunk"), saved_chunk);
        assert!(
            !dir.join("user.bin.tmp").exists(),
            "nothing staged for the unloaded library"
        );
        store.load_user_library(crate::USER_DICTIONARY);
        assert!(store.phrase(saved).expect("read").is_some());
        assert!(store.phrase(unsaved).expect("read").is_none());
        // Loaded again, it is saved again.
        store.mark_modified();
        store.save_reporting().expect("save");
        assert_eq!(std::fs::read(&chunk).expect("chunk"), saved_chunk);
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    /// Every key the pin's parsers can write is a `content_table` row
    /// (`pinyin_parser2.cpp:114`, `:146`, `zhuyin_parser2.cpp:95`), so each
    /// of the 440 syllable keys decodes from its packed fields and renders
    /// back to the same packed word at every tone the pin allows (0..=5,
    /// `chewing_enum.h:92-99`) — the option-only `eng`/`nun` and the
    /// zhuyin-only `den` included.
    #[test]
    fn every_pin_producible_key_decodes_and_renders() {
        use oxpinyin_core::{SYLLABLE_KEY_COUNT, SyllableKey};
        for index in 0..SYLLABLE_KEY_COUNT {
            let syllable = SyllableKey::from_index(index).expect("index");
            let chewing = ChewingKey::from_pinyin(syllable.text()).expect("content row");
            for tone in 0..=5 {
                let packed = chewing.with_tone(tone).to_packed();
                let ids = packed_to_pinyin_keys(&[packed])
                    .unwrap_or_else(|| panic!("{} tone {tone} dropped", syllable.text()));
                assert_eq!(ids, [phrase::toned_key(index, tone).expect("toned")]);
                assert_eq!(pinyin_keys_to_packed(&ids), Some(vec![packed]));
            }
        }
        for (spelling, tone) in [("eng", 0), ("eng", 5), ("nun", 1), ("den", 4)] {
            let packed = ChewingKey::from_pinyin(spelling)
                .expect("content row")
                .with_tone(tone)
                .to_packed();
            let ids = packed_to_pinyin_keys(&[packed]).expect("decoded");
            assert_eq!(phrase::key_tone(ids[0]), tone);
            assert_eq!(
                SyllableKey::from_index(phrase::key_syllable(ids[0])).map(SyllableKey::text),
                Some(spelling)
            );
        }
    }

    /// A word no pin parser writes (a foreign file) has no key and must not
    /// panic: the zero key, a field combination `chewing_key_table` marks
    /// invalid, a tone past 5, and the padding bit alone.
    #[test]
    fn foreign_packed_words_decode_to_none_without_panic() {
        let invalid = ChewingKey::new(31, 3, 31, 0).to_packed();
        for packed in [0_u16, invalid, 0xffff, 0x8000] {
            assert_eq!(packed_to_pinyin_keys(&[packed]), None, "{packed:#06x}");
        }
        let ni = ChewingKey::from_pinyin("ni").expect("ni");
        for tone in [6, 7] {
            assert_eq!(
                packed_to_pinyin_keys(&[ni.with_tone(tone).to_packed()]),
                None
            );
        }
    }

    /// The pin's import writes `eng` (from `egn`, `emg`), `nun` (from
    /// `nuen`) and `den` (zhuyin `ㄉㄣ`); a save and reopen keeps each
    /// reading, as the pin's does.
    #[test]
    fn option_and_zhuyin_only_readings_survive_save_and_reopen() {
        use oxpinyin_core::SyllableKey;
        let dir = tempdir("option-only-reopen");
        let cases = [("嗯", "eng"), ("嫩", "nun"), ("扽", "den")];
        let mut tokens = Vec::new();
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            for (text, spelling) in cases {
                let index = SyllableKey::from_canonical_text(spelling)
                    .expect("key")
                    .index();
                let keys = vec![phrase::toned_key(index, 0).expect("key")];
                tokens.push(
                    store
                        .add_phrase_in(crate::USER_DICTIONARY, &text.into(), &keys, Some(5))
                        .expect("add"),
                );
            }
            // `pinyin_end_add_phrases` arms the save (`pinyin.cpp:657-658`).
            store.mark_modified();
            store.save_reporting().expect("save");
        }
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("reopen");
        for (token, (_, spelling)) in tokens.into_iter().zip(cases) {
            let phrase = store.phrase(token).expect("phrase").expect("present");
            let spellings: Vec<_> = phrase
                .pronunciations()
                .iter()
                .map(|reading| {
                    reading
                        .keys()
                        .iter()
                        .map(|&key| phrase::key_pinyin_string(key))
                        .collect::<Vec<_>>()
                })
                .collect();
            assert_eq!(spellings, [vec![Some(spelling.to_owned())]]);
        }
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
                .add_system_phrase_in(1, None, &"拟".into(), &dense(readings[0]), Some(5))
                .expect("append");
            for &packed in &readings[1..255] {
                store
                    .add_system_phrase_in(1, Some(token), &"拟".into(), &dense(packed), Some(5))
                    .expect("reading within the 255 limit");
            }
            assert_eq!(store.user_pronunciation_count(token), Some(255));
            unigram = store.unigram_delta(token).expect("unigram");
            assert_eq!(unigram, 15);
            store
                .add_system_phrase_in(1, Some(token), &"拟".into(), &dense(readings[255]), Some(5))
                .expect("the 256th reading is accepted");
            assert_eq!(
                store.with_effective_system_item(token, |item| item.prons.len()),
                Some(0)
            );
            assert!(readings_of(&store, token).is_empty());
            assert_eq!(store.unigram_delta(token).expect("unigram"), unigram);
            store.mark_modified();
            store.save_reporting().expect("save");
        }
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("reopen");
            assert!(readings_of(&store, token).is_empty());
            assert_eq!(store.unigram_delta(token).expect("unigram"), unigram);
            store
                .add_system_phrase_in(1, Some(token), &"拟".into(), &dense(readings[256]), Some(9))
                .expect("the 257th reading");
            assert_eq!(readings_of(&store, token), [9]);
            assert_eq!(
                store.with_effective_system_item(token, |item| item.prons.clone()),
                Some(vec![(vec![readings[256]], 9)])
            );
            store.mark_modified();
            store.save_reporting().expect("save");
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
                    .add_system_phrase_in(1, Some(token), &"你".into(), &dense(packed), Some(5))
                    .expect("reading within the 255 limit");
            }
            assert_eq!(prons_of(&store).len(), 255);
            unigram = unigram_of(&store);
            store
                .add_system_phrase_in(1, Some(token), &"你".into(), &dense(readings[254]), Some(5))
                .expect("the 256th reading is accepted");
            assert!(prons_of(&store).is_empty());
            assert_eq!(unigram_of(&store), unigram);
            store.mark_modified();
            store.save_reporting().expect("a wrapped original saves");
        }
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("reopen");
            assert!(prons_of(&store).is_empty());
            assert_eq!(unigram_of(&store), unigram);
            store
                .add_system_phrase_in(1, Some(token), &"你".into(), &dense(readings[255]), Some(9))
                .expect("the 257th reading");
            assert_eq!(prons_of(&store), [(vec![readings[255]], 9)]);
            store.mark_modified();
            store.save_reporting().expect("save");
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
                .add_phrase_in(
                    crate::USER_DICTIONARY,
                    &"龘".into(),
                    &dense(readings[0]),
                    Some(5),
                )
                .expect("append");
            for &packed in &readings[1..255] {
                store
                    .add_phrase_in(
                        crate::USER_DICTIONARY,
                        &"龘".into(),
                        &dense(packed),
                        Some(5),
                    )
                    .expect("reading within the 255 limit");
            }
            assert_eq!(readings_of(&store, token).len(), 255);
            unigram = store.unigram_delta(token).expect("unigram");
            store
                .add_phrase_in(
                    crate::USER_DICTIONARY,
                    &"龘".into(),
                    &dense(readings[255]),
                    Some(5),
                )
                .expect("the 256th reading is accepted");
            assert!(readings_of(&store, token).is_empty());
            assert_eq!(store.unigram_delta(token).expect("unigram"), unigram);
            store.mark_modified();
            store.save_reporting().expect("a wrapped item saves");
        }
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("reopen");
            assert!(readings_of(&store, token).is_empty());
            assert_eq!(store.unigram_delta(token).expect("unigram"), unigram);
            store
                .add_phrase_in(
                    crate::USER_DICTIONARY,
                    &"龘".into(),
                    &dense(readings[256]),
                    Some(9),
                )
                .expect("the 257th reading");
            assert_eq!(readings_of(&store, token), [9]);
            store.mark_modified();
            store.save_reporting().expect("save");
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
            let mut store = UserStore::open_libpinyin(
                &dir,
                base.fixture_copy(),
                versions(),
                UserConfLaw::Pinyin,
            )
            .expect("open");
            token = store
                .add_system_phrase_in(1, None, &"拟".into(), &keys, Some(5))
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
                .add_system_phrase_in(1, Some(token), &"拟".into(), &keys, Some(7))
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
            store.save_reporting().expect("save");
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
    fn mapped_system_append_accepts_second_reading_after_save_and_reopen() {
        use oxpinyin_data::chunk_write::build_chunk;
        use oxpinyin_data::phrase_library::PhraseLibrary;

        let dir = tempdir("mapped-append-second-reading");
        let shipped_dir = tempdir("mapped-append-shipped");
        let path = shipped_dir.join("gb_char.bin");
        let shipped = ChunkItem {
            phrase: vec![0x4f60],
            unigram: 100,
            prons: vec![(
                vec![ChewingKey::from_pinyin("ni").expect("ni").to_packed()],
                100,
            )],
        };
        std::fs::write(&path, build_chunk(&[(1, shipped)]).expect("chunk")).expect("write");
        let originals = || {
            SystemOriginals::from(crate::LibraryTable::from([(
                1_u8,
                SystemLibrary::from_library(PhraseLibrary::open(&path).expect("map")),
            )]))
        };
        let first = packed_to_pinyin_keys(&[ChewingKey::from_pinyin("ni").unwrap().to_packed()])
            .expect("first reading");
        let second = packed_to_pinyin_keys(&[ChewingKey::from_pinyin("ni")
            .unwrap()
            .with_tone(2)
            .to_packed()])
        .expect("second reading");
        let token;
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            token = store
                .add_system_phrase_in(1, None, &"拟".into(), &first, Some(5))
                .expect("append");
            assert_eq!(token & 0x00ff_ffff, 2, "beyond mapped offset table");
            assert_eq!(
                store
                    .phrase(token)
                    .expect("read appended")
                    .unwrap()
                    .pronunciations()
                    .len(),
                1
            );
            assert_eq!(
                store
                    .add_system_phrase_in(1, Some(token), &"拟".into(), &second, Some(7))
                    .expect("second import"),
                token
            );
            assert_eq!(
                store
                    .phrase(token)
                    .expect("read merged")
                    .unwrap()
                    .pronunciations()
                    .len(),
                2
            );
            store.mark_modified();
            store.save_reporting().expect("save");
        }
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("reopen");
        assert_eq!(
            store
                .phrase(token)
                .expect("read reopened")
                .unwrap()
                .pronunciations()
                .len(),
            2
        );
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
        std::fs::remove_dir_all(shipped_dir).expect("cleanup shipped");
    }

    /// Two loaded slots sharing one offset (a crafted chunk; both builders
    /// emit distinct offsets). A unigram added through the first slot is
    /// read through the second, as the pin's shared `m_phrase_content`
    /// shows it (074a2219 `phrase_index.cpp:150-178`, `:180-205`); the
    /// save diffs the file on disk again and emits a MODIFY for each slot
    /// (`:392-437`); the reload applies the first MODIFY in place and stops
    /// at the second's mismatch (`:487-493`, ignored at `pinyin.cpp:276`),
    /// dropping every later record. The second slot's baseline is captured
    /// after the first slot's write, which only the store's internal
    /// `unigram_delta` sees.
    #[test]
    fn aliased_offset_slots_share_live_reads_and_the_reload_prefix() {
        use oxpinyin_data::chunk_format::chunk_checksum;
        use oxpinyin_data::chunk_write::build_chunk;
        use oxpinyin_data::phrase_library::PhraseLibrary;
        use oxpinyin_data::user_files::{LogRecord, load_chunk_payload, log_records};

        let dir = tempdir("aliased-offset");
        let shipped_dir = tempdir("aliased-offset-shipped");
        let path = shipped_dir.join("gb_char.bin");
        let item = |code: u32, pinyin: &str, unigram: u32| ChunkItem {
            phrase: vec![code],
            unigram,
            prons: vec![(
                vec![ChewingKey::from_pinyin(pinyin).expect("key").to_packed()],
                unigram,
            )],
        };
        let mut chunk = build_chunk(&[
            (1, item(0x4f60, "ni", 100)),
            (2, item(0x597d, "hao", 50)),
            (3, item(0x6211, "wo", 70)),
        ])
        .expect("chunk");
        // Alias slot 2 to slot 1: the offset word of slot `s` is at
        // `8 + 17 + 4 * s` (chunk header, four words, separator), then
        // reseal the payload checksum.
        let word = |slot: usize| 8 + 17 + 4 * slot;
        let first: [u8; 4] = chunk[word(1)..word(1) + 4].try_into().unwrap();
        chunk[word(2)..word(2) + 4].copy_from_slice(&first);
        let checksum = chunk_checksum(&chunk[8..]).to_le_bytes();
        chunk[4..8].copy_from_slice(&checksum);
        std::fs::write(&path, &chunk).expect("write");
        let originals = || {
            SystemOriginals::from(crate::LibraryTable::from([(
                1_u8,
                SystemLibrary::from_library(PhraseLibrary::open(&path).expect("map")),
            )]))
        };
        let (a, b, c) = (0x0100_0001, 0x0100_0002, 0x0100_0003);
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("open");
            assert_eq!(
                (
                    store.system_unigram(a),
                    store.system_unigram(b),
                    store.system_unigram(c)
                ),
                (Some(100), Some(100), Some(70))
            );
            assert!(
                store
                    .add_system_unigram_frequency(a, 5)
                    .expect("add through a")
            );
            // Live reads: the second slot shows the first slot's write.
            assert_eq!(
                (
                    store.system_unigram(a),
                    store.system_unigram(b),
                    store.system_unigram(c)
                ),
                (Some(105), Some(105), Some(70))
            );
            assert!(
                store
                    .add_system_unigram_frequency(c, 7)
                    .expect("add through c")
            );
            assert_eq!(store.system_unigram(c), Some(77));
            // Field writes into present items record no provenance, shared
            // offset or not.
            assert!(!store.system_provenance_recorded(1));
            store.mark_modified();
            store.save_reporting().expect("save");
        }
        // Save output: the diff against the file on disk emits one MODIFY per
        // slot, the aliased pair with identical bytes, then slot 3
        // (`phrase_index.cpp:392-437`).
        let payload = load_chunk_payload(&dir.join("gb_char.dbin")).expect("journal");
        let records: Vec<LogRecord<Vec<u8>>> = log_records(&payload)
            .map(|record| record.expect("record").into_owned())
            .collect();
        assert_eq!(records.len(), 4, "{records:?}");
        let LogRecord::ModifyHeader {
            old_total,
            new_total,
        } = &records[0]
        else {
            panic!("{:?}", records[0]);
        };
        // The pin compacts before it writes (`pinyin.cpp:1139-1141`), so the
        // header's new total is the live items summed slot by slot, with the
        // aliased pair counted twice: 105 + 105 + 77.
        assert_eq!((*old_total, *new_total), (220, 287));
        let modify = |record: &LogRecord<Vec<u8>>| match record {
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            } => (*token, old_item.clone(), new_item.clone()),
            other => panic!("{other:?}"),
        };
        let (token_a, old_a, new_a) = modify(&records[1]);
        let (token_b, old_b, new_b) = modify(&records[2]);
        let (token_c, old_c, new_c) = modify(&records[3]);
        assert_eq!((token_a, token_b, token_c), (1, 2, 3));
        assert_eq!((old_a.clone(), new_a.clone()), (old_b, new_b));
        assert_eq!(u32::from_le_bytes(old_a[2..6].try_into().unwrap()), 100);
        assert_eq!(u32::from_le_bytes(new_a[2..6].try_into().unwrap()), 105);
        assert_eq!(u32::from_le_bytes(old_c[2..6].try_into().unwrap()), 70);
        assert_eq!(u32::from_le_bytes(new_c[2..6].try_into().unwrap()), 77);
        // Reload prefix: the header and slot 1 apply, slot 2's old item no
        // longer matches the shared bytes, and slot 3's record is dropped.
        let store = UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
            .expect("reopen");
        assert_eq!(
            (
                store.system_unigram(a),
                store.system_unigram(b),
                store.system_unigram(c)
            ),
            (Some(105), Some(105), Some(70))
        );
        drop(store);
        std::fs::remove_dir_all(dir).expect("cleanup");
        std::fs::remove_dir_all(shipped_dir).expect("cleanup shipped");
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
            store.save_reporting().expect("save gate");
            store.observe_selection(1, 0x0100_0001).expect("train");
            store.save_reporting().expect("save");
            store.save_reporting().expect("second save is a no-op");

            // A user phrase under the user dictionary. `_add_phrase`
            // does not arm the §4 gate (the pin sets `m_modified` only
            // in `pinyin_train` and `pinyin_end_add_phrases`); the
            // import trio's `mark_modified` is what arms a phrase save.
            let token = store
                .add_phrase(&"你好".into(), &keys[..], None)
                .expect("add phrase");
            assert_eq!(token & 0x0F00_0000, 0x0700_0000);
            store.save_reporting().expect("gate holds");
            store.mark_modified();
            store.save_reporting().expect("save 2");
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
            // seed * 7 on the system token's live unigram.
            assert_eq!(store.system_unigram(0x0100_0001), Some(100 + 69 * 7));

            // The user phrase round-trips with its reading and base
            // count (DEFAULT_PHRASE_COUNT * ADD_PHRASE_UNIGRAM_FACTOR).
            let token = store
                .token_for_phrase("你好")
                .expect("token")
                .expect("present");
            let phrase = store.phrase(token).expect("phrase").expect("row");
            assert_eq!(phrase.phrase_text().unwrap(), "你好");
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
                .add_phrase(&"你好".into(), &[id("ni"), id("hao")][..], None)
                .expect("add");
            let removed = store
                .add_phrase(&"我们".into(), &[id("wo"), id("men")][..], None)
                .expect("add");
            store.mark_modified();
            store.save_reporting().expect("save");
            store.mask_out(0xffff_ffff, masked).expect("mask");
            assert!(store.remove_user_phrase(removed).expect("remove"));
            store
                .add_phrase(&"他".into(), &[id("ta")][..], None)
                .expect("add");
            store.mark_modified();
            store.save_reporting().expect("save after");
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
                .index
                .keys();
        // Both key spaces of both two-syllable readings and their prefixes,
        // and 他's one-syllable reading in each space.
        assert_eq!(pinyin_keys.len(), 2 * 2 * 2 + 2, "{pinyin_keys:?}");

        // A later session keeps them through its own save.
        {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("reopen");
            store.mark_modified();
            store.save_reporting().expect("save again");
        }
        assert_eq!(phrase_rows(), rows);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    #[test]
    fn loaded_words_survive_lookup_and_native_save() {
        let dir = tempdir("stored-words-1198");
        let mut state = UserState::default();
        for library in [5, 6, 7] {
            for (slot, middle) in (1..).zip([0x4e01, 0, 0xd800, 0x110000]) {
                state.libraries.get_or_default(library).unwrap().insert(
                    slot,
                    ChunkItem {
                        phrase: vec![0x7532, middle, 0x4e59],
                        unigram: 100,
                        prons: vec![(vec![0x1084; 3], 10)],
                    },
                );
            }
        }
        let state = state.index_every_reading();
        persistence::save_state_reporting(&dir, &state, &originals(), &versions(), 1)
            .expect("native fixture");
        for _ in 0..2 {
            let mut store =
                UserStore::open_libpinyin(&dir, originals(), versions(), UserConfLaw::Pinyin)
                    .expect("load");
            for (&library, items) in &state.libraries {
                for (&slot, expected) in items {
                    let token = u32::from(library) << 24 | slot;
                    let phrase = store.phrase(token).expect("read").expect("retained item");
                    assert_eq!(phrase.words(), expected.phrase);
                    assert_eq!(phrase.pronunciations()[0].count(), 10);
                    assert_eq!(store.unigram_delta(token).expect("unigram"), 100);
                    let text = phrase.phrase_text().expect("outward text");
                    assert_eq!(text.phrase_length(), 3);
                    let outward = expected
                        .phrase
                        .iter()
                        .copied()
                        .take_while(|&word| word != 0)
                        .collect::<Vec<_>>();
                    assert_eq!(text.outward_words().collect::<Vec<_>>(), outward);
                    if expected.phrase[1] == 0 {
                        let scalar = oxpinyin_core::PhraseText::from("甲\0乙");
                        assert_eq!(scalar.stored_words(), Some(expected.phrase.as_slice()));
                        assert_eq!(scalar.phrase_length(), 3);
                        assert_eq!(scalar, text);
                    }
                }
            }
            store.mark_modified();
            store.save_reporting().expect("save");
        }
        std::fs::remove_dir_all(dir).expect("cleanup");
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
            libraries: crate::LibraryTable::from([(
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
            system_overrides: crate::LibraryTable::new(),
            indexed: std::collections::BTreeSet::new(),
        }
        .index_every_reading();
        persistence::save_state_reporting(&dir, &state, &originals(), &versions(), 1)
            .expect("save");

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
        Some(
            oxpinyin_data::user_files::UserTableInfo::load_path(&dir.join("user.conf"))
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

        second.save_reporting().expect("save 2");
        first.save_reporting().expect("save 1");
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
        first.save_reporting().expect("save 1");
        second.save_reporting().expect("save 2");
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
        zhuyin.save_reporting().expect("save zhuyin");
        assert_eq!(recorded_counter(&dir), Some(0));
        pinyin
            .observe_selection(1, 0x0100_0001)
            .expect("train pinyin");
        pinyin.save_reporting().expect("save pinyin");
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
        pinyin.save_reporting().expect("save pinyin");
        assert_eq!(recorded_counter(&dir), Some(1));
        // The later save is libzhuyin's: its fresh 0 replaces libpinyin's
        // raised counter.
        zhuyin.save_reporting().expect("save zhuyin");
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
mod open_faults;
#[cfg(test)]
mod provenance;
#[cfg(test)]
mod replayed_readings;
#[cfg(test)]
mod unloaded;
