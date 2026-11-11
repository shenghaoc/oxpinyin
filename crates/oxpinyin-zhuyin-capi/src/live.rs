//! The contexts `zhuyin_init` handed out and `zhuyin_fini` has not yet taken
//! back.
//!
//! `zhuyin_alloc_instance` on a context that was already finalised reads the
//! freed context at the pin (`context->m_phrase_index`, `zhuyin.cpp:845-857`):
//! a use after free that happens to survive. A Rust handle cannot read freed
//! memory, and the address says nothing about whether it is still live, so
//! the entry points that a finalised context can reach consult this set
//! instead (register row 60, class (b); the zhuyin twin, PR 14).

use std::collections::BTreeSet;
use std::sync::Mutex;

use crate::types::ZhuyinContext;

/// Addresses of the live contexts. `None` while no context is live: the set
/// owns a heap node once it has held an entry, and a static is never dropped,
/// so the registry gives that node back when the last context goes.
static LIVE: Mutex<Option<BTreeSet<usize>>> = Mutex::new(None);

fn lock_result_or_abort<T>(result: std::sync::LockResult<T>) -> T {
    match result {
        Ok(value) => value,
        Err(_) => std::process::abort(),
    }
}

fn set() -> std::sync::MutexGuard<'static, Option<BTreeSet<usize>>> {
    lock_result_or_abort(LIVE.lock())
}

fn insert(live: &mut Option<BTreeSet<usize>>, address: usize) {
    live.get_or_insert_with(BTreeSet::new).insert(address);
}

fn remove(live: &mut Option<BTreeSet<usize>>, address: usize) -> bool {
    let Some(addresses) = live else {
        return false;
    };
    let held = addresses.remove(&address);
    if addresses.is_empty() {
        *live = None;
    }
    held
}

/// Records a context `zhuyin_init` just boxed.
pub(crate) fn register(context: *mut ZhuyinContext) {
    insert(&mut set(), context as usize);
}

/// Forgets a context `zhuyin_fini` is about to free. Answers whether the
/// registry held it: the test and the removal are one step under the lock,
/// so of two calls on one context exactly one sees `true`.
pub(crate) fn unregister(context: *mut ZhuyinContext) -> bool {
    remove(&mut set(), context as usize)
}

/// Whether `context` was handed out and not finalised since.
pub(crate) fn is_live(context: *mut ZhuyinContext) -> bool {
    set()
        .as_ref()
        .is_some_and(|addresses| addresses.contains(&(context as usize)))
}

#[cfg(test)]
mod tests {
    use super::{insert, is_live, register, remove, unregister};
    use crate::test_support::{TempUserDir, open_with_user};

    #[test]
    fn poisoned_mutex_aborts() {
        crate::test_support::assert_aborts("live::tests::poisoned_mutex_aborts", || {
            let mutex = std::sync::Mutex::new(());
            let panic = std::panic::catch_unwind(|| {
                let _guard = mutex.lock().expect("unpoisoned mutex");
                panic!("poison the mutex");
            });
            assert!(panic.is_err());
            let result = mutex.lock();
            assert!(result.is_err(), "the mutex must be poisoned");
            drop(super::lock_result_or_abort(result));
        });
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
            crate::parse::zhuyin_parse_more_chewings(instance, input.as_ptr()),
            6
        );
        let marker = user_dir.path.join("user.conf");
        let before = std::fs::read(&marker).ok();
        crate::context::zhuyin_fini(context);
        assert!(!is_live(context));
        let at_fini = std::fs::read(&marker).ok();
        assert_eq!(before, at_fini);
        assert!(
            weak.upgrade().is_some(),
            "the instance retains the same allocation"
        );
        assert!(crate::sentence::zhuyin_guess_sentence(instance));
        assert!(crate::candidates::zhuyin_train(instance));
        assert!(crate::instance::zhuyin_reset(instance));
        crate::instance::zhuyin_free_instance(instance);
        assert!(
            weak.upgrade().is_none(),
            "the last handle releases the allocation"
        );
        assert_eq!(std::fs::read(marker).ok(), at_fini);
    }

    #[test]
    fn import_iterator_outlives_context() {
        let user_dir = TempUserDir::new("orphan-import");
        let (context, instance) = open_with_user(&user_dir.path);
        let import = crate::iterators::zhuyin_begin_add_phrases(context, 5);
        assert!(!import.is_null());
        crate::instance::zhuyin_free_instance(instance);
        crate::context::zhuyin_fini(context);
        let phrase = crate::test_support::cstr("你好");
        let reading = crate::test_support::cstr("ㄋㄧˇ ㄏㄠˇ");
        assert!(crate::iterators::zhuyin_iterator_add_phrase(
            import,
            phrase.as_ptr(),
            reading.as_ptr(),
            7
        ));
        crate::iterators::zhuyin_end_add_phrases(import);
    }

    #[test]
    fn a_context_is_live_from_init_to_fini() {
        let user_dir = TempUserDir::new("live-context");
        let (context, instance) = open_with_user(&user_dir.path);
        assert!(is_live(context));
        crate::instance::zhuyin_free_instance(instance);
        crate::context::zhuyin_fini(context);
        assert!(!is_live(context));
        assert!(
            crate::instance::zhuyin_alloc_instance(context).is_null(),
            "a finalised context allocates nothing"
        );
        // A second finalise is a no-op, where the pin double-frees (row 105).
        crate::context::zhuyin_fini(context);
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
        let mut live = None;
        insert(&mut live, 8);
        insert(&mut live, 16);
        assert!(live.is_some());
        remove(&mut live, 8);
        assert!(live.is_some(), "an entry is still live");
        remove(&mut live, 16);
        assert!(live.is_none(), "the last entry takes the set with it");
        remove(&mut live, 16);
        assert!(
            live.is_none(),
            "forgetting an unknown address allocates nothing"
        );
        insert(&mut live, 8);
        assert!(live.is_some(), "a later context registers again");
    }
}
