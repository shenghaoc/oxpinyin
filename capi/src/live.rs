//! The contexts `pinyin_init` handed out and `pinyin_fini` has not yet taken
//! back.
//!
//! `pinyin_alloc_instance` on a context that was already finalised reads the
//! freed context at the pin (`context->m_phrase_index`, `pinyin.cpp:1322`):
//! a use after free that happens to survive. A Rust handle cannot read freed
//! memory, and the address says nothing about whether it is still live, so
//! the entry points that a finalised context can reach consult this set
//! instead (register row 60, class (b)).

use oxpinyin::facade::LiveContexts;

use crate::types::PinyinContext;

static LIVE: LiveContexts = LiveContexts::new();

/// Records a context `pinyin_init` just boxed.
pub(crate) fn register(context: *mut PinyinContext) {
    LIVE.register(context.addr());
}

/// Forgets a context `pinyin_fini` is about to free. Answers whether the
/// registry held it: the test and the removal are one step under the lock,
/// so of two calls on one context exactly one sees `true`.
pub(crate) fn unregister(context: *mut PinyinContext) -> bool {
    LIVE.unregister(context.addr())
}

/// Whether `context` was handed out and not finalised since.
pub(crate) fn is_live(context: *mut PinyinContext) -> bool {
    LIVE.contains(context.addr())
}

#[cfg(test)]
mod tests;
