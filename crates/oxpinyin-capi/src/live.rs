//! The contexts `pinyin_init` handed out and `pinyin_fini` has not yet taken
//! back.
//!
//! `pinyin_alloc_instance` on a context that was already finalised reads the
//! freed context at the pin (`context->m_phrase_index`, `pinyin.cpp:1322`):
//! a use after free that happens to survive. A Rust handle cannot read freed
//! memory, and the address says nothing about whether it is still live, so
//! the entry points that a finalised context can reach consult this set
//! instead (register row 60, class (b)).

use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};

use crate::types::PinyinContext;

/// Addresses of the live contexts. `None` while no context is live: the set
/// owns a heap node once it has held an entry, and a static is never dropped,
/// so the registry gives that node back when the last context goes.
static LIVE: Mutex<Option<BTreeSet<usize>>> = Mutex::new(None);

fn set() -> std::sync::MutexGuard<'static, Option<BTreeSet<usize>>> {
    LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

fn insert(live: &mut Option<BTreeSet<usize>>, address: usize) {
    live.get_or_insert_with(BTreeSet::new).insert(address);
}

fn remove(live: &mut Option<BTreeSet<usize>>, address: usize) {
    if let Some(addresses) = live {
        addresses.remove(&address);
        if addresses.is_empty() {
            *live = None;
        }
    }
}

/// Records a context `pinyin_init` just boxed.
pub(crate) fn register(context: *mut PinyinContext) {
    insert(&mut set(), context as usize);
}

/// Forgets a context `pinyin_fini` is about to free.
pub(crate) fn unregister(context: *mut PinyinContext) {
    remove(&mut set(), context as usize);
}

/// Whether `context` was handed out and not finalised since.
pub(crate) fn is_live(context: *mut PinyinContext) -> bool {
    set()
        .as_ref()
        .is_some_and(|addresses| addresses.contains(&(context as usize)))
}

#[cfg(test)]
mod tests {
    use super::{insert, is_live, register, remove, unregister};
    use crate::test_support::{TempUserDir, open};

    #[test]
    fn a_context_is_live_from_init_to_fini() {
        let user_dir = TempUserDir::new("live-context");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert!(is_live(context));
        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
        assert!(!is_live(context));
        assert!(
            crate::instance::pinyin_alloc_instance(context).is_null(),
            "a finalised context allocates nothing"
        );
        // Registering and forgetting a stray address leaves the rest alone.
        let stray = std::ptr::NonNull::<crate::types::PinyinContext>::dangling().as_ptr();
        register(stray);
        assert!(is_live(stray));
        unregister(stray);
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
