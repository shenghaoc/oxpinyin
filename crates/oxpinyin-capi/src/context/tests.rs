#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::test_support::alloc;
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::test_support::init_at;
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::test_support::parse;

use crate::test_support::token_of;

use crate::candidates::{pinyin_choose_candidate, pinyin_train};

#[cfg(unix)]
use crate::config::pinyin_mask_out;

#[cfg(unix)]
use crate::context::pinyin_init;

use crate::context::pinyin_save;

use crate::e2e_tests::with_store;

use crate::instance::pinyin_reset;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::instance::pinyin_free_instance;

use crate::sentence::pinyin_guess_sentence;

use crate::state::instance_mut;

use crate::test_support::TempUserDir;
use crate::test_support::candidate;
use crate::test_support::open;

#[cfg(unix)]
use crate::test_support::cstr;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::test_support::plant_bigram;
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::test_support::system_dir;

use crate::types::PinyinInstance;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use crate::types::PinyinContext;

use oxpinyin_user::SENTENCE_START;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
use std::path::Path;

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
/// Every file of `dir` by name, with its bytes.
fn files_of(dir: &Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .expect("read the directory")
        .map(|entry| {
            let entry = entry.expect("an entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            (name, std::fs::read(entry.path()).expect("read a file"))
        })
        .collect()
}

#[cfg(any(feature = "tkrzw", feature = "bdb"))]
/// Trains the top sentence of `nihao` once on a new instance.
fn train_once(context: *mut PinyinContext) {
    let instance = alloc(context);
    assert_eq!(parse(instance, "nihao"), 5);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { crate::candidates::pinyin_train(instance, 0) });
    unsafe { pinyin_free_instance(instance) };
}

/// Copies the regular files of `user` to `before`, next to it.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
fn keep_before(user: &Path) {
    let before = user.parent().expect("the child's scratch").join("before");
    std::fs::create_dir(&before).expect("the before directory");
    for (name, bytes) in files_of(user) {
        std::fs::write(before.join(name), bytes).expect("keep a file");
    }
}

/// Two whole user-bigram rows: two predecessors, each with an empty gram.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
fn two_bigram_rows() -> Vec<(Vec<u8>, Vec<u8>)> {
    vec![
        (0x0100_0001_u32.to_le_bytes().to_vec(), vec![0; 4]),
        (0x0100_0002_u32.to_le_bytes().to_vec(), vec![0; 4]),
    ]
}

/// The pin's copies assert on a failed destination write
/// (`tkrzwdb_utils.h:65`, `bdb_utils.h:61`): the save stops in
/// `_write_files` after one record, before `_rename_files` and
/// `mark_version` (`pinyin.cpp:1141-1143`), so no saved file is replaced.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
#[test]
fn save_aborts_where_a_copy_fails_a_destination_write() {
    let run = crate::test_support::assert_aborts_in_dir(
        "context::tests::save_aborts_where_a_copy_fails_a_destination_write",
        |user| {
            plant_bigram(user, two_bigram_rows());
            let context = init_at(&system_dir(), user);
            assert!(!context.is_null());
            // `pinyin_save` saves a modified context only (`pinyin.cpp:1136`).
            train_once(context);
            assert!(unsafe { pinyin_save(context) });
            keep_before(user);
            train_once(context);
            // The first copy writes one record and fails the next.
            oxpinyin_store::copy_faults::fail_destination_write_after(1);
            unsafe { pinyin_save(context) };
        },
    );
    let before = files_of(&run.user_dir.parent().expect("scratch").join("before"));
    let after = files_of(&run.user_dir);
    for (name, bytes) in &before {
        assert_eq!(after.get(name), Some(bytes), "{name} was replaced");
    }
    assert!(
        after.keys().any(|name| name.ends_with(".tmp")),
        "the copy left no staged file: {:?}",
        after.keys()
    );
}

/// The session could not be built once the library loop had handed it
/// the system libraries (`OpenError::UserSession`): the pin dies at each
/// counterpart. No input makes those steps fail, so the open's answer
/// is handed to the init's refusal directly.
#[test]
fn a_user_session_fault_aborts_the_init() {
    let stderr = crate::test_support::assert_aborts(
        "context::tests::a_user_session_fault_aborts_the_init",
        || {
            let error =
                oxpinyin_facade::OpenFailure::Runtime(oxpinyin_runtime::OpenError::UserSession(
                    oxpinyin_user::UserStoreError::SessionSeed("injected".into()),
                ));
            super::refuse_init(&error, std::path::Path::new("system"));
        },
    );
    // No Rust panic text or other line: the pin writes none here but
    // glibc's assert line, the register's policy-accepted missing line.
    assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
}

/// Whether a directory reads as a chunk on this host's temporary file
/// system (register row 96); where it does not, a directory library is row
/// 76's unmappable one (class (b)).
#[cfg(unix)]
fn directory_reads_as_a_chunk_here() -> bool {
    let probe = tempfile::tempdir().expect("probe dir");
    std::fs::write(probe.path().join("e".repeat(25)), b"x").expect("fill the probe");
    oxpinyin_data::phrase_library::directory_reads_as_a_chunk(probe.path())
}

/// A context over a private system directory, with its user dir.
#[cfg(unix)]
fn init_private(
    scratch: &std::path::Path,
) -> (std::path::PathBuf, *mut crate::types::PinyinContext) {
    let system = crate::test_support::private_system(scratch);
    let user = scratch.join("user");
    std::fs::create_dir_all(&user).expect("user dir");
    let system_c = cstr(system.to_str().expect("UTF-8 path"));
    let user_c = cstr(user.to_str().expect("UTF-8 path"));
    (system, unsafe {
        pinyin_init(system_c.as_ptr(), user_c.as_ptr())
    })
}

/// One call that maps a system library again over a private system
/// directory whose library is a directory (register row 96): the save and
/// the mask-out map `merged.bin` (`pinyin.cpp:955`, `:1264`), the reload of
/// the unloaded GBK library maps `gbk_char.bin` (`:256`).
#[cfg(unix)]
fn remap_a_directory_library(site: &str) -> bool {
    let scratch = tempfile::tempdir().expect("scratch");
    let (system, context) = init_private(scratch.path());
    assert!(!context.is_null());
    match site {
        "save" => {
            // SAFETY: `context` is a live `pinyin_init` handle, borrowed
            // uniquely for this statement.
            let ctx = unsafe { crate::state::context_mut(context) };
            ctx.core.user_store_mut().mark_modified();
            crate::test_support::directory_library(&system, "merged.bin");
            unsafe { pinyin_save(context) }
        }
        "mask" => {
            crate::test_support::directory_library(&system, "merged.bin");
            unsafe { pinyin_mask_out(context, 0xff00_0000, 0x0400_0000) }
        }
        "addon" => {
            crate::test_support::directory_library(&system, "art.bin");
            unsafe { crate::config::pinyin_load_addon_phrase_library(context, 4) }
        }
        _ => {
            assert!(unsafe { crate::config::pinyin_unload_phrase_library(context, 2) });
            crate::test_support::directory_library(&system, "gbk_char.bin");
            unsafe { crate::config::pinyin_load_phrase_library(context, 2) }
        }
    }
}

/// Register row 96 at save; row 76's completed save where a directory does
/// not read as a chunk.
// Unix only: a directory opens as a file, so the pin's `MemoryChunk::mmap` judges it by its
// size (`directory_reads_as_a_chunk`). Windows refuses the open, and oxpinyin skips the row
// without a word there (`oxpinyin-data`, `phrase_libraries.rs`), which no pin observation
// covers.
#[cfg(unix)]
#[test]
fn save_aborts_on_a_directory_library() {
    if directory_reads_as_a_chunk_here() {
        crate::test_support::assert_aborts(
            "context::tests::save_aborts_on_a_directory_library",
            || {
                let _ = remap_a_directory_library("save");
            },
        );
    } else {
        assert!(remap_a_directory_library("save"));
    }
}

/// Snapshot of phrase candidates as `(token, text)`.
fn phrase_snapshot(instance: *mut PinyinInstance) -> Vec<(u32, String)> {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let mut inst = unsafe { instance_mut(instance) };
    // The facade defers the parse's list build to its first reader.
    inst.core
        .session
        .refresh_candidates()
        .expect("deferred candidate build");
    inst.core
        .session
        .candidates()
        .iter()
        .filter_map(|candidate| {
            Some((
                candidate.token()?.value(),
                candidate.text().as_str().unwrap_or_default().to_owned(),
            ))
        })
        .collect()
}

#[test]
fn save_gates_on_dirty_and_roundtrips_through_the_abi() {
    let user_dir = TempUserDir::new("save");
    let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
    // `pinyin_init` writes the profile marker (`check_format`'s
    // user.conf, counter ratchet and all); the durable store is the
    // pin's file set, written only at `pinyin_save`.
    let marker = user_dir.path.join("user.conf");
    assert!(marker.exists(), "init writes the user.conf marker");

    // A clean context: pinyin_save is the §4 unmodified no-op (upstream
    // returns false, pinyin.cpp:1136) and leaves every file untouched.
    let before = std::fs::metadata(&marker)
        .expect("marker exists")
        .modified()
        .expect("mtime");
    assert!(!unsafe { pinyin_save(context) });
    let after = std::fs::metadata(&marker)
        .expect("marker exists")
        .modified()
        .expect("mtime");
    assert_eq!(before, after, "a clean save must not touch the profile");
    assert!(!unsafe { pinyin_save(context) });

    // Train once: the save gate arms, a dirty save returns true, clears,
    // and leaves the pin's own files behind — the bigram hash among them.
    let first = candidate(instance, "nihao", 0);
    let t1 = token_of(instance, first);
    assert!(unsafe { pinyin_choose_candidate(instance, 0, first) } > 0);
    assert!(unsafe { pinyin_guess_sentence(instance) });
    assert!(unsafe { pinyin_train(instance, 0) });
    assert!(unsafe { pinyin_save(context) });
    assert!(
        !unsafe { pinyin_save(context) },
        "the save cleared m_modified"
    );
    assert!(
        user_dir
            .path
            .join(oxpinyin_data::user_files::UserDbm::Bigram.file_name())
            .exists(),
        "the dirty save wrote the user bigram"
    );

    // Decode against the populated store (T4's merge) — the state the
    // reopened store must reproduce.
    assert!(unsafe { pinyin_reset(instance) });
    let _ = candidate(instance, "nihao", 0);
    let decode_before = phrase_snapshot(instance);
    let counts = {
        (
            with_store(instance, |store| store.bigram_count(SENTENCE_START, t1)).unwrap(),
            with_store(instance, |store| store.bigram_total(SENTENCE_START)).unwrap(),
            with_store(instance, |store| store.system_unigram(t1)),
            with_store(instance, |store| store.unigram_total()),
        )
    };

    // Teardown does NOT save (the §6 shutdown decision: upstream has no
    // flush) — the counts survive because the dirty save above already
    // wrote the profile, exactly the pin's own durability shape.
    unsafe { crate::instance::pinyin_free_instance(instance) };
    unsafe { crate::context::pinyin_fini(context) };

    // Reopen: counts, allocation state, and decode all resume.
    let (context2, instance2) = open(user_dir.path.to_str().expect("UTF-8 path"));
    let _ = candidate(instance2, "nihao", 0);
    assert_eq!(
        phrase_snapshot(instance2),
        decode_before,
        "decode-after-reopen must match the pre-save populated-store decode"
    );
    {
        assert!(
            !with_store(instance2, |store| store.is_modified()),
            "a reopen starts clean"
        );
        assert_eq!(
            with_store(instance2, |store| store.bigram_count(SENTENCE_START, t1)).unwrap(),
            counts.0
        );
        assert_eq!(
            with_store(instance2, |store| store.bigram_total(SENTENCE_START)).unwrap(),
            counts.1
        );
        assert_eq!(
            with_store(instance2, |store| store.system_unigram(t1)),
            counts.2
        );
        assert_eq!(
            with_store(instance2, |store| store.unigram_total()),
            counts.3
        );
    }
    assert!(
        !unsafe { pinyin_save(context2) },
        "the reopened store saves as a no-op"
    );

    unsafe { crate::instance::pinyin_free_instance(instance2) };
    unsafe { crate::context::pinyin_fini(context2) };
}
