#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use super::{insert, is_live, register, remove, unregister};

use crate::test_support::{TempUserDir, open_with_user};

/// The descriptors this process holds on files under `dir`.
// Linux only: /proc/self/fd.
#[cfg(target_os = "linux")]
fn open_descriptors(dir: &std::path::Path) -> usize {
    let dir = dir.canonicalize().expect("system dir");
    std::fs::read_dir("/proc/self/fd")
        .expect("descriptor table")
        .filter_map(Result::ok)
        .filter(|entry| {
            std::fs::read_link(entry.path()).is_ok_and(|target| target.starts_with(&dir))
        })
        .count()
}

#[test]
fn poisoned_mutex_aborts() {
    crate::test_support::assert_aborts_after_setup(
        "live::tests::poisoned_mutex_aborts",
        // The panic that poisons the mutex is setup, before the strict
        // check is armed.
        || {
            let mutex = std::sync::Mutex::new(());
            let panic = std::panic::catch_unwind(|| {
                let _guard = mutex.lock().expect("unpoisoned mutex");
                panic!("poison the mutex");
            });
            assert!(panic.is_err());
            mutex
        },
        |mutex| {
            let result = mutex.lock();
            assert!(result.is_err(), "the mutex must be poisoned");
            drop(super::lock_result_or_abort(result));
        },
    );
}

#[test]
fn instance_outlives_context() {
    let user_dir = TempUserDir::new("orphan-instance");
    let (context, instance) = open_with_user(&user_dir.path);
    // SAFETY: the context is live and this test serializes all handle calls.
    let owner = unsafe { crate::state::context_owner(context) };
    let weak = std::rc::Rc::downgrade(&owner);
    drop(owner);
    let input = crate::test_support::cstr("su3cl3");
    assert_eq!(
        unsafe { crate::parse::zhuyin_parse_more_chewings(instance, input.as_ptr()) },
        6
    );
    let marker = user_dir.path.join("user.conf");
    let before = std::fs::read(&marker).ok();
    unsafe { crate::context::zhuyin_fini(context) };
    assert!(!is_live(context));
    let at_fini = std::fs::read(&marker).ok();
    assert_eq!(before, at_fini);
    assert!(
        weak.upgrade().is_some(),
        "the instance retains the same allocation"
    );
    // Row 42: the system tables are closed, so every read is a miss. The
    // guess completes over the retained in-memory state and decodes no
    // sentence, and training has no sentence to train.
    assert!(unsafe { crate::sentence::zhuyin_guess_sentence(instance) });
    assert!(!unsafe { crate::candidates::zhuyin_train(instance) });
    assert!(unsafe { crate::instance::zhuyin_reset(instance) });
    unsafe { crate::instance::zhuyin_free_instance(instance) };
    assert!(
        weak.upgrade().is_none(),
        "the last handle releases the allocation"
    );
    assert_eq!(std::fs::read(marker).ok(), at_fini);
}

/// `zhuyin_fini` deletes its tables, which closes their DBM files and
/// releases their locks, whether or not an instance is still allocated
/// (`zhuyin.cpp:744-748`, `ngram_tkrzwdb.cpp:40-46` at 074a2219). A
/// closed descriptor has released its `fcntl` lock.
// Linux only: /proc/self/fd.
#[cfg(target_os = "linux")]
#[test]
fn fini_closes_the_system_files_while_an_instance_lives() {
    // A private copy, so no other context holds these files.
    let system = TempUserDir::new("fini-closes-system");
    for entry in std::fs::read_dir(crate::test_support::system_dir()).expect("fixture dir") {
        let entry = entry.expect("fixture entry");
        if entry.file_type().expect("file type").is_file() {
            std::fs::copy(entry.path(), system.path.join(entry.file_name())).expect("copy fixture");
        }
    }
    let user_dir = TempUserDir::new("fini-closes");
    let system_c = crate::test_support::cstr(system.path.to_str().expect("UTF-8 path"));
    let user_c = crate::test_support::cstr(user_dir.path.to_str().expect("UTF-8 path"));
    let context = unsafe { crate::context::zhuyin_init(system_c.as_ptr(), user_c.as_ptr()) };
    assert!(!context.is_null());
    let instance = unsafe { crate::instance::zhuyin_alloc_instance(context) };
    assert!(!instance.is_null());
    assert!(
        open_descriptors(&system.path) > 0,
        "the attached tables hold their files"
    );
    unsafe { crate::context::zhuyin_fini(context) };
    assert_eq!(open_descriptors(&system.path), 0);
    unsafe { crate::instance::zhuyin_free_instance(instance) };
}

#[test]
fn import_iterator_outlives_context() {
    let user_dir = TempUserDir::new("orphan-import");
    let (context, instance) = open_with_user(&user_dir.path);
    let import = unsafe { crate::iterators::zhuyin_begin_add_phrases(context, 5) };
    assert!(!import.is_null());
    unsafe { crate::instance::zhuyin_free_instance(instance) };
    unsafe { crate::context::zhuyin_fini(context) };
    let phrase = crate::test_support::cstr("你好");
    let reading = crate::test_support::cstr("ㄋㄧˇ ㄏㄠˇ");
    assert!(unsafe {
        crate::iterators::zhuyin_iterator_add_phrase(import, phrase.as_ptr(), reading.as_ptr(), 7)
    });
    unsafe { crate::iterators::zhuyin_end_add_phrases(import) };
}

#[test]
fn a_context_is_live_from_init_to_fini() {
    let user_dir = TempUserDir::new("live-context");
    let (context, instance) = open_with_user(&user_dir.path);
    assert!(is_live(context));
    unsafe { crate::instance::zhuyin_free_instance(instance) };
    unsafe { crate::context::zhuyin_fini(context) };
    assert!(!is_live(context));
    assert!(
        unsafe { crate::instance::zhuyin_alloc_instance(context) }.is_null(),
        "a finalised context allocates nothing"
    );
    // A second finalise is a no-op, where the pin double-frees (row 105).
    unsafe { crate::context::zhuyin_fini(context) };
    assert!(!is_live(context));
    // Registering and forgetting a stray address leaves the rest alone.
    let stray = std::ptr::NonNull::<crate::types::ZhuyinContext>::dangling().as_ptr();
    register(stray);
    assert!(is_live(stray));
    assert!(unregister(stray));
    assert!(!unregister(stray), "a second removal finds nothing");
    assert!(!is_live(stray));
}

#[test]
fn the_registry_holds_no_allocation_once_it_empties() {
    // The shared static is also used by every other test, so exercise the
    // same transitions on a private value.
    let mut live = Vec::new();
    insert(&mut live, 8);
    insert(&mut live, 16);
    insert(&mut live, 8);
    assert_eq!(live.len(), 2, "an address is held once");
    assert!(remove(&mut live, 8));
    assert_eq!(live, [16], "an entry is still live");
    assert!(remove(&mut live, 16));
    assert_eq!(
        live.capacity(),
        0,
        "the last entry takes the buffer with it"
    );
    assert!(!remove(&mut live, 16));
    assert_eq!(
        live.capacity(),
        0,
        "forgetting an unknown address allocates nothing"
    );
    insert(&mut live, 8);
    assert_eq!(live, [8], "a later context registers again");
}
