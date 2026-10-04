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
use std::sync::{Mutex, PoisonError};

use crate::types::ZhuyinContext;

/// Addresses of the live contexts.
static LIVE: Mutex<BTreeSet<usize>> = Mutex::new(BTreeSet::new());

fn set() -> std::sync::MutexGuard<'static, BTreeSet<usize>> {
    LIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Records a context `zhuyin_init` just boxed.
pub(crate) fn register(context: *mut ZhuyinContext) {
    set().insert(context as usize);
}

/// Forgets a context `zhuyin_fini` is about to free.
pub(crate) fn unregister(context: *mut ZhuyinContext) {
    set().remove(&(context as usize));
}

/// Whether `context` was handed out and not finalised since.
pub(crate) fn is_live(context: *mut ZhuyinContext) -> bool {
    set().contains(&(context as usize))
}

#[cfg(test)]
mod tests {
    use super::{is_live, register, unregister};
    use crate::test_support::{TempUserDir, open_with_user};

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
        // Registering and forgetting a stray address leaves the rest alone.
        let stray = std::ptr::NonNull::<crate::types::ZhuyinContext>::dangling().as_ptr();
        register(stray);
        assert!(is_live(stray));
        unregister(stray);
        assert!(!is_live(stray));
    }
}
