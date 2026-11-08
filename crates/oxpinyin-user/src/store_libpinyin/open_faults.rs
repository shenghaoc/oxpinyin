//! A session open that fails after the library loop handed it the system
//! libraries.
//!
//! The pin never degrades there. Its user bigram's `load_db` result is
//! ignored (`pinyin.cpp:401`, `zhuyin.cpp:343`): a failed `db_create`
//! asserts (`ngram_bdb.cpp:51-52`), a failed in-memory `DB->open` leaves the
//! failed container the init carries on with (`:54-57`), and the
//! allocations of every other in-memory load end in `std::bad_alloc`. No
//! input makes these steps fail, so each test replaces one step of the open
//! ([`super::OpenSteps`]) with the failure, through `open_session` itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oxpinyin_core::ChewingKey;
use oxpinyin_data::chunk_write::{ChunkItem, encode_phrase_item};
use oxpinyin_data::user_files::{SystemVersions, UserTableInfo};
use oxpinyin_store::StoreError;

use super::{OpenSteps, PinSteps};
use crate::persistence::{PersistenceError, SystemLibrary, SystemOriginals, UserConfLaw};
use crate::store::{UserStore, UserStoreError};

fn versions() -> SystemVersions {
    SystemVersions::for_this_build(7, 14)
}

fn tempdir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "oxpinyin-open-faults-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("tmpdir");
    std::fs::write(
        dir.join("user.conf"),
        UserTableInfo::conform_to(&versions()).to_text(),
    )
    .expect("write marker");
    dir
}

fn item(code: u32, unigram: u32) -> ChunkItem {
    ChunkItem {
        phrase: vec![code],
        unigram,
        prons: vec![(vec![ChewingKey::new(23, 0, 1, 0).to_packed()], unigram)],
    }
}

/// Library 1 with one item at slot 1, unigram 100.
fn originals() -> SystemOriginals {
    BTreeMap::from([(
        1_u8,
        SystemLibrary {
            mapped: None,
            total: 100,
            range_end: 2,
            items: BTreeMap::from([(1_u32, item(0x4f60, 100))]),
        },
    )])
    .into()
}

fn open<P: OpenSteps>(
    dir: Option<&Path>,
    originals: SystemOriginals,
) -> Result<UserStore, UserStoreError> {
    UserStore::open_session::<P>(
        dir,
        &mut { originals },
        versions(),
        UserConfLaw::Pinyin,
        None,
    )
}

fn injected() -> StoreError {
    StoreError::Backend("injected".into())
}

/// A loaded item the open encodes and places in its library's arena: a
/// value-only `user.bin` item of library 7. A session load reads `user.bin`
/// as views and replays a library without an original into its own buffer,
/// so no profile on disk reaches this step; the profile step plants the item.
fn plant(profile: &mut crate::persistence::LoadedProfile) {
    profile
        .loaded
        .state
        .libraries
        .entry(7)
        .or_default()
        .insert(2, item(0x597d, 7));
}

/// The real steps over a profile with a planted item: the control for the
/// encoding and placement failures.
struct Planted;

impl OpenSteps for Planted {
    fn profile(profile: &mut crate::persistence::LoadedProfile) {
        plant(profile);
    }
}

/// The user bigram's container answers an error: what remains of one once
/// the backend has ended the process for a container it cannot create, an
/// unsupported library.
struct BigramFails;

impl OpenSteps for BigramFails {
    fn bigram(_: Option<&Path>) -> Result<oxpinyin_store::DefaultUserBigramDb, PersistenceError> {
        Err(PersistenceError::Store(injected()))
    }
}

/// The planted item's encoding fails: an item no decoder yields, with no
/// text and one reading of one key.
struct EncodeFails;

impl OpenSteps for EncodeFails {
    fn profile(profile: &mut crate::persistence::LoadedProfile) {
        plant(profile);
    }

    fn encode(_: &ChunkItem) -> Result<Vec<u8>, oxpinyin_data::chunk_write::ChunkWriteError> {
        encode_phrase_item(&ChunkItem {
            phrase: Vec::new(),
            unigram: 0,
            prons: vec![(vec![1], 0)],
        })
    }
}

/// The planted item's placement fails.
struct ReplaceFails;

impl OpenSteps for ReplaceFails {
    fn profile(profile: &mut crate::persistence::LoadedProfile) {
        plant(profile);
    }

    fn replace(_: &mut crate::user_library::UserLibrary, _: u32, _: &[u8]) -> Result<(), String> {
        Err("injected".into())
    }
}

#[test]
fn a_planted_item_is_placed() {
    // The control: the real steps place the item, so the encoding and
    // placement failures fail at a step the open reaches.
    let dir = tempdir("place-control");
    let store = open::<Planted>(Some(&dir), originals()).expect("open");
    let placed = store
        .session_target()
        .phrases()
        .libraries
        .get(&7)
        .is_some_and(|library| !library.is_empty());
    assert!(placed);
    drop(store);
    std::fs::remove_dir_all(dir).unwrap();
}

/// The variable naming the one test a child copy of this binary runs.
const CHILD: &str = "OXPINYIN_USER_OPEN_FAULTS_CHILD";

/// The variable naming the profile the parent made for its child.
const CHILD_DIR: &str = "OXPINYIN_USER_OPEN_FAULTS_DIR";

/// Run `body` over the profile `dir` in a child copy of this test binary
/// that runs only the test `name`, so its stderr (fd 2, where the fini's
/// C-stdio line goes) is the child's alone. The child re-enters the test
/// from its start, so it is handed the parent's `dir`, not one of its own.
/// The child exits 0 once `body` returns; answers its status and stderr.
fn in_child(
    name: &str,
    dir: &Path,
    body: impl FnOnce(&Path),
) -> (std::process::ExitStatus, String) {
    if std::env::var_os(CHILD).is_some_and(|value| value == name) {
        let dir = PathBuf::from(std::env::var_os(CHILD_DIR).expect("the parent's profile"));
        body(&dir);
        std::process::exit(0);
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args(["--exact", name, "--test-threads=1"])
        .env(CHILD, name)
        .env(CHILD_DIR, dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .expect("re-run the test binary");
    (
        output.status,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The parent's profile, made once: a child re-entering the test leaves
/// the parent's in place.
fn parent_dir(name: &str) -> PathBuf {
    if std::env::var_os(CHILD).is_some() {
        return PathBuf::new();
    }
    tempdir(name)
}

/// The `open counter:` the profile's `user.conf` holds.
fn open_counter(dir: &Path) -> String {
    let text = std::fs::read_to_string(dir.join("user.conf")).expect("user.conf");
    text.lines()
        .find_map(|line| line.strip_prefix("open counter:"))
        .expect("open counter line")
        .to_owned()
}

/// The names in `dir`, sorted.
fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// One injected failure through the whole open, with and without a user
/// dir. The open answers [`UserStoreError::SessionSeed`] and leaves as the
/// pin dies, without `pinyin_fini`: `check_format`'s raised counter stays in
/// `user.conf`, nothing else is written or saved, and stderr is empty (no
/// `write %s failed.` from a fini that writes `""`).
fn leaves_without_finalizing<P: OpenSteps>(test: &str, name: &str) {
    let dir = parent_dir(name);
    let (status, stderr) = in_child(test, &dir, |dir| {
        assert!(matches!(
            open::<P>(Some(dir), originals()),
            Err(UserStoreError::SessionSeed(_))
        ));
        assert!(matches!(
            open::<P>(None, originals()),
            Err(UserStoreError::SessionSeed(_))
        ));
    });
    assert!(status.success(), "{test}: {status:?} {stderr}");
    assert_eq!(stderr, "", "{test}: no finalization output");
    assert_eq!(open_counter(&dir), "1", "{test}: the raised counter stays");
    assert_eq!(listing(&dir), ["user.conf"], "{test}: nothing saved");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_completed_open_finalizes() {
    // The control for the checks below: the real steps finalize at drop,
    // lowering the counter, and without a user dir the fini writes `""` and
    // prints the pin's `write  failed.` line.
    let test = "store_libpinyin::open_faults::a_completed_open_finalizes";
    let dir = parent_dir("finalizes");
    let (status, stderr) = in_child(test, &dir, |dir| {
        drop(open::<PinSteps>(Some(dir), originals()).expect("open"));
        drop(open::<PinSteps>(None, originals()).expect("open"));
    });
    assert!(status.success(), "{status:?} {stderr}");
    let newline = oxpinyin_input::TEXT_NEWLINE;
    assert_eq!(stderr, format!("write  failed.{newline}"));
    assert_eq!(open_counter(&dir), "0");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_failed_user_bigram_container_leaves_without_finalizing() {
    leaves_without_finalizing::<BigramFails>(
        "store_libpinyin::open_faults::a_failed_user_bigram_container_leaves_without_finalizing",
        "bigram",
    );
}

#[test]
fn a_failed_item_encoding_leaves_without_finalizing() {
    leaves_without_finalizing::<EncodeFails>(
        "store_libpinyin::open_faults::a_failed_item_encoding_leaves_without_finalizing",
        "encode",
    );
}

#[test]
fn a_failed_item_placement_leaves_without_finalizing() {
    leaves_without_finalizing::<ReplaceFails>(
        "store_libpinyin::open_faults::a_failed_item_placement_leaves_without_finalizing",
        "replace",
    );
}

/// Berkeley DB's failed container (`ngram_bdb.cpp:54-57`): the init carries
/// on with it, its libraries in place, and every later call answers as the
/// pin's call on the failed handle.
#[cfg(feature = "bdb")]
mod failed_container {
    use oxpinyin_store::{BdbUserBigramDb, UserBigramDb as _};

    use super::{OpenSteps, open, originals, tempdir};
    use crate::store::{SENTENCE_START, TrainingCheck, TrainingFault, UserStoreError};

    /// The open is given the failed container.
    struct Failed;

    impl OpenSteps for Failed {
        fn bigram(
            _: Option<&std::path::Path>,
        ) -> Result<oxpinyin_store::DefaultUserBigramDb, crate::persistence::PersistenceError>
        {
            Ok(BdbUserBigramDb::failed_container())
        }
    }
    const NI: u32 = 0x0100_0001;

    /// A profile whose `user_bigram.db` holds a gram under the sentence start.
    fn profile(name: &str) -> std::path::PathBuf {
        let dir = tempdir(name);
        let path = dir.join("user_bigram.db");
        let db = BdbUserBigramDb::load_db(&path).expect("container");
        db.store(
            &SENTENCE_START.to_le_bytes(),
            &[69, 0, 0, 0, 1, 0, 0, 1, 69, 0, 0, 0],
        )
        .expect("store");
        db.save_db(&path).expect("save");
        dir
    }

    #[test]
    fn the_open_keeps_its_libraries_and_reads_no_gram() {
        for dir in [Some(profile("keeps")), None] {
            let store = open::<Failed>(dir.as_deref(), originals()).expect("open");
            // The system library is the session's: its item answers.
            assert_eq!(store.system_unigram(NI), Some(100));
            // `load` misses, though the file holds the gram.
            assert_eq!(store.bigram_row(SENTENCE_START).expect("read"), None);
            drop(store);
            if let Some(dir) = dir {
                std::fs::remove_dir_all(dir).unwrap();
            }
        }
    }

    #[test]
    fn training_meets_the_store_assert_and_predicted_training_carries_on() {
        let dir = profile("training");
        let mut store = open::<Failed>(Some(&dir), originals()).expect("open");
        // `phonetic_lookup.h:904`: the first trained step's store fails.
        let mut check = TrainingCheck::default();
        assert!(matches!(
            store.check_training_step_with(&mut check, SENTENCE_START, NI),
            Err(UserStoreError::FailedUserBigramStore)
        ));
        assert_eq!(
            check.fault(),
            Some(TrainingFault::FailedUserBigramStore(SENTENCE_START))
        );
        assert!(matches!(
            store.observe_selection(SENTENCE_START, NI),
            Err(UserStoreError::FailedUserBigramStore)
        ));
        // `pinyin.cpp:2638` ignores the store's answer: the call succeeds,
        // and no gram is learnt.
        store
            .observe_predicted(SENTENCE_START, NI)
            .expect("predicted");
        assert_eq!(store.bigram_row(SENTENCE_START).expect("read"), None);
        drop(store);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_save_replaces_the_profiles_bigram_with_an_empty_file() {
        for law in [super::UserConfLaw::Pinyin, super::UserConfLaw::Zhuyin] {
            let dir = profile("save");
            let mut store = crate::store::UserStore::open_session::<Failed>(
                Some(&dir),
                &mut originals(),
                super::versions(),
                law,
                None,
            )
            .expect("open");
            store.mark_modified();
            // `save_db` answers false after creating the empty hash file,
            // and both facades rename it into place (`pinyin.cpp:1121`,
            // `zhuyin.cpp:687`).
            let report = store.save_reporting().expect("save");
            assert!(report.renames_failed.is_empty(), "{law:?}");
            let path = dir.join("user_bigram.db");
            let saved = BdbUserBigramDb::load_db(&path).expect("reload");
            assert!(saved.keys().expect("walk").is_empty(), "{law:?}");
            assert!(!dir.join("user_bigram.db.tmp").exists(), "{law:?}");
            drop((saved, store));
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
