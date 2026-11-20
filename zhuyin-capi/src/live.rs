//! The contexts `zhuyin_init` handed out and `zhuyin_fini` has not yet taken
//! back.
//!
//! `zhuyin_alloc_instance` on a context that was already finalised reads the
//! freed context at the pin (`context->m_phrase_index`, `zhuyin.cpp:845-857`):
//! a use after free that happens to survive. A Rust handle cannot read freed
//! memory, and the address says nothing about whether it is still live, so
//! the entry points that a finalised context can reach consult this set
//! instead (register row 60, class (b); the zhuyin twin, PR 14).

use oxpinyin::facade::LiveContexts;

use crate::types::ZhuyinContext;

static LIVE: LiveContexts = LiveContexts::new();

/// Records a context `zhuyin_init` just boxed.
pub(crate) fn register(context: *mut ZhuyinContext) {
    LIVE.register(context.addr());
}

/// Forgets a context `zhuyin_fini` is about to free. Answers whether the
/// registry held it: the test and the removal are one step under the lock,
/// so of two calls on one context exactly one sees `true`.
pub(crate) fn unregister(context: *mut ZhuyinContext) -> bool {
    LIVE.unregister(context.addr())
}

/// Whether `context` was handed out and not finalised since.
pub(crate) fn is_live(context: *mut ZhuyinContext) -> bool {
    LIVE.contains(context.addr())
}

#[cfg(test)]
mod tests;
