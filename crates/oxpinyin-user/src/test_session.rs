//! Opens a test's session the way an init does: `check_format`, then the
//! library loop over the test's system libraries and its layout's user
//! libraries ([`UserStore::open_libpinyin_checked`]'s path).
//!
//! A test describes its system libraries as [`SystemOriginals`]. The loop
//! maps chunk files, so each library is written to a scratch system
//! directory first: a mapped library's own file, or a chunk built from a
//! fixture's inline items with the fixture's stored total.

use std::path::Path;

use oxpinyin_data::table_info::{DefaultRowStep, FileName};
use oxpinyin_data::user_files::SystemVersions;

use crate::persistence::{SystemOriginals, UserConfLaw};
use crate::store::{UserStore, UserStoreError};

thread_local! {
    /// The scratch system directories of this thread's sessions. A save maps
    /// each system chunk again by path, so a directory outlives its store.
    static SYSTEM_DIRS: std::cell::RefCell<Vec<tempfile::TempDir>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Puts `library`'s chunk at `target`. A mapped library's own file is
/// linked where the platform has links, so the session maps that file.
fn place_chunk(library: crate::persistence::SystemLibraryRef<'_>, target: &Path) {
    if let Some(path) = library.mapped.and_then(|mapped| mapped.source_path()) {
        #[cfg(unix)]
        std::os::unix::fs::symlink(path, target).expect("link the fixture's chunk file");
        #[cfg(not(unix))]
        std::fs::copy(path, target).expect("copy the fixture's chunk file");
        return;
    }
    let items: Vec<_> = library
        .items
        .iter()
        .map(|(&slot, item)| (slot, item.clone()))
        .collect();
    let file = oxpinyin_data::chunk_write::build_chunk(&items).expect("fixture chunk");
    // The chunk header is the length and the checksum; the payload opens
    // with the sub-index total.
    let mut payload = file[8..].to_vec();
    payload[..4].copy_from_slice(&library.total.to_le_bytes());
    let file =
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).expect("fixture chunk frame");
    std::fs::write(target, file).expect("write the fixture's chunk");
}

/// The scratch system directory and the loop's rows for `originals`.
struct Rows {
    system_dir: std::path::PathBuf,
    files: Vec<(u8, FileName)>,
    user: Vec<u8>,
}

impl Rows {
    fn new(originals: &SystemOriginals) -> Self {
        let system_dir = tempfile::tempdir().expect("scratch system directory");
        let mut files = Vec::new();
        for (&nibble, library) in originals.iter() {
            let name = format!("library{nibble}.bin");
            place_chunk(library, &system_dir.path().join(&name));
            files.push((nibble, FileName::from_bytes(name.as_bytes())));
        }
        let path = system_dir.path().to_path_buf();
        SYSTEM_DIRS.with_borrow_mut(|dirs| dirs.push(system_dir));
        Self {
            system_dir: path,
            files,
            user: originals
                .layout()
                .user_libraries()
                .iter()
                .map(|(library, _)| *library)
                .collect(),
        }
    }

    fn rows(&self) -> crate::library_loop::LibraryLoopRows<'_> {
        let mut steps = Vec::new();
        for nibble in 0..16_u8 {
            if let Some((_, file)) = self.files.iter().find(|(library, _)| *library == nibble) {
                steps.push((
                    nibble,
                    DefaultRowStep::System {
                        file,
                        null_user_file: false,
                    },
                ));
            } else if self.user.contains(&nibble) {
                steps.push((nibble, DefaultRowStep::User));
            }
        }
        crate::library_loop::LibraryLoopRows::from_steps(&self.system_dir, steps)
    }
}

/// A session on `user_dir` over `originals`' libraries and layout.
pub(crate) fn open(
    user_dir: &Path,
    originals: SystemOriginals,
    versions: SystemVersions,
    law: UserConfLaw,
) -> Result<UserStore, UserStoreError> {
    open_over(user_dir, &originals, versions, law)
}

fn open_over(
    user_dir: &Path,
    originals: &SystemOriginals,
    versions: SystemVersions,
    law: UserConfLaw,
) -> Result<UserStore, UserStoreError> {
    let layout = originals.layout().clone();
    let rows = Rows::new(originals);
    let check = crate::persistence::check_format(user_dir, &versions, &layout, law)?;
    let mut loaded = SystemOriginals::new(crate::LibraryTable::new(), layout);
    UserStore::open_libpinyin_checked(user_dir, &mut loaded, &rows.rows(), versions, law, check)
}

/// [`open`] with the open's fallible steps replaced by `P`'s, for a test
/// that injects a failure no data produces.
pub(crate) fn open_with<P: crate::store_libpinyin::OpenSteps>(
    user_dir: &Path,
    originals: SystemOriginals,
    versions: SystemVersions,
    law: UserConfLaw,
) -> Result<UserStore, UserStoreError> {
    let layout = originals.layout().clone();
    let rows = Rows::new(&originals);
    let check = crate::persistence::check_format(user_dir, &versions, &layout, law)?;
    let mut loaded = SystemOriginals::new(crate::LibraryTable::new(), layout);
    UserStore::open_session::<P>(
        Some((user_dir, check, &rows.rows())),
        &mut loaded,
        versions,
        law,
    )
}

/// What an init reads of the profile in `dir`, as values: `check_format`
/// and the library loop, with no session made of it, so the counter the
/// load raised stays on disk as it does when a process dies before its fini.
pub(crate) fn load(
    dir: &Path,
    originals: &SystemOriginals,
    versions: &SystemVersions,
    law: UserConfLaw,
) -> Result<crate::persistence::Loaded, crate::persistence::PersistenceError> {
    let check = crate::persistence::check_format(dir, versions, originals.layout(), law)?;
    load_checked(dir, originals, &check)
}

/// The library loop's read of the profile in `dir`, which `check` judged.
pub(crate) fn load_profile_checked(
    dir: &Path,
    originals: &SystemOriginals,
    check: &crate::persistence::ProfileCheck,
) -> Result<crate::persistence::LoadedProfile, crate::persistence::PersistenceError> {
    let rows = Rows::new(originals);
    let mut mapped = SystemOriginals::new(crate::LibraryTable::new(), originals.layout().clone());
    let (mut profile, stop) =
        crate::persistence::load_session_in_loop(dir, &mut mapped, &rows.rows(), check)?;
    match stop {
        Ok(()) => {}
        Err(crate::library_loop::LibraryLoopStop::Journal(fault)) => {
            profile.loaded.journal_fault = Some(fault);
        }
        Err(stop) => panic!("the library loop stopped: {stop}"),
    }
    Ok(profile)
}

/// [`load_profile_checked`] after `check_format`.
pub(crate) fn load_profile(
    dir: &Path,
    originals: &SystemOriginals,
    versions: &SystemVersions,
    law: UserConfLaw,
) -> Result<crate::persistence::LoadedProfile, crate::persistence::PersistenceError> {
    let check = crate::persistence::check_format(dir, versions, originals.layout(), law)?;
    load_profile_checked(dir, originals, &check)
}

/// [`load`] for a profile `check` already judged.
pub(crate) fn load_checked(
    dir: &Path,
    originals: &SystemOriginals,
    check: &crate::persistence::ProfileCheck,
) -> Result<crate::persistence::Loaded, crate::persistence::PersistenceError> {
    use oxpinyin_store::RawReadStore as _;
    let crate::persistence::LoadedProfile {
        mut loaded,
        replayed,
        user_libraries,
        index,
        phrase_table,
        ..
    } = load_profile_checked(dir, originals, check)?;
    let state = &mut loaded.state;
    let bigram = oxpinyin_input::path::build_path(
        dir,
        std::ffi::OsStr::new(&oxpinyin_data::user_files::UserDbm::Bigram.file_name()),
    );
    if check.conform && bigram.exists() {
        match oxpinyin_store::DefaultStore::open_user_bigram(&bigram) {
            Ok(store) => {
                let rows = &mut state.bigram;
                if let Err(error) =
                    store.copy_raw(|key, value| rows.push((key.to_vec(), value.to_vec())))
                {
                    loaded.skipped.push(format!("user bigram: {error}"));
                }
            }
            Err(error) => loaded.skipped.push(format!("user bigram: {error}")),
        }
    }
    let state = &mut loaded.state;
    for (&nibble, library) in &user_libraries {
        let items: std::collections::BTreeMap<_, _> = library
            .items()
            .map(|(slot, item)| {
                (
                    slot,
                    oxpinyin_data::chunk_write::decode_phrase_item(item.as_bytes())
                        .expect("a stored item"),
                )
            })
            .collect();
        if !items.is_empty() {
            state.libraries.insert(nibble, items);
        }
    }
    state.indexed = index.indexed_set();
    let collected = phrase_table.for_each_record::<std::convert::Infallible>(|key, value| {
        state.phrase_table.insert(key.to_vec(), value.to_vec());
        Ok(())
    });
    if let Err(infallible) = collected {
        match infallible {}
    }
    let live = crate::system_items::SystemItems::new(replayed);
    for (token, item) in live.changed_items(originals) {
        state
            .system_overrides
            .get_or_default(crate::phrase::phrase_index_library_index(token))
            .expect("a library slot")
            .insert(token & crate::phrase::PHRASE_MASK, item);
    }
    Ok(loaded)
}

/// A modified session on `dir` that holds `state`. Nothing of an earlier
/// profile is read: its data files are removed first.
pub(crate) fn seeded(
    dir: &Path,
    state: &crate::persistence::UserState,
    originals: &SystemOriginals,
    versions: &SystemVersions,
) -> Result<UserStore, UserStoreError> {
    use oxpinyin_data::user_files::{UserDbm, UserTableInfo};
    // The session starts from a conforming, empty profile. The marker is
    // rewritten in place, as every marker write is.
    let marker = dir.join("user.conf");
    let layout = originals.layout();
    let stale = layout
        .user_libraries()
        .iter()
        .chain(layout.system_logs())
        .map(|(_, name)| name.display().into_owned())
        .chain(
            [UserDbm::Bigram, UserDbm::PinyinIndex, UserDbm::PhraseIndex].map(UserDbm::file_name),
        );
    for name in stale {
        let _ = std::fs::remove_file(dir.join(name));
    }
    assert!(UserTableInfo::conform_to(versions).save_path(&marker));
    let mut store = open_over(dir, originals, *versions, UserConfLaw::Pinyin)?;
    store.seed_from_state(state);
    store.mark_modified();
    Ok(store)
}

/// Writes the profile a session holding `state` saves into `dir`, with
/// `open_counter` in its marker.
pub(crate) fn save_state(
    dir: &Path,
    state: &crate::persistence::UserState,
    originals: &SystemOriginals,
    versions: &SystemVersions,
    open_counter: i32,
) -> Result<(), UserStoreError> {
    use oxpinyin_data::user_files::UserTableInfo;
    let marker = dir.join("user.conf");
    let mut store = seeded(dir, state, originals, versions)?;
    let saved = store.save_reporting();
    // No fini: the marker below is the one the caller asked for.
    std::mem::forget(store);
    saved?;
    let mut info = UserTableInfo::conform_to(versions);
    info.open_counter = open_counter;
    assert!(info.save_path(&marker), "write the marker");
    Ok(())
}
