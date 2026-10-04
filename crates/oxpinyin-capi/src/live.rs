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

/// Addresses of the live contexts.
static LIVE: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());

fn set() -> std::sync::MutexGuard<'static, BTreeSet<usize>> {
    LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Records a context `pinyin_init` just boxed.
pub(crate) fn register(context: *mut PinyinContext) {
    set().insert(context as usize);
}

/// Forgets a context `pinyin_fini` is about to free.
pub(crate) fn unregister(context: *mut PinyinContext) {
    set().remove(&(context as usize));
}

/// Whether `context` was handed out and not finalised since.
pub(crate) fn is_live(context: *mut PinyinContext) -> bool {
    set().contains(&(context as usize))
}

#[cfg(test)]
mod tests {
    use super::{is_live, register, unregister};
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
}
