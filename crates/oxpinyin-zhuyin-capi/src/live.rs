//! The contexts `zhuyin_init` handed out and `zhuyin_fini` has not yet taken
//! back.
//!
//! `zhuyin_alloc_instance` on a context that was already finalised reads the
//! freed context at the pin (`context->m_phrase_index`, `zhuyin.cpp:845-857`):
//! a use after free that happens to survive. A Rust handle cannot read freed
//! memory, and the address says nothing about whether it is still live, so
//! the entry points that a finalised context can reach consult this set
//! instead (register row 60, class (b); the zhuyin twin, PR 14).

use std::sync::Mutex;

use crate::types::ZhuyinContext;

/// Addresses of the live contexts, each once, in no order: a process holds
/// few contexts, so a membership test walks them. The list owns a buffer
/// once it has held an entry, and a static is never dropped, so the registry
/// gives the buffer back when the last context goes.
static LIVE: Mutex<Vec<usize>> = Mutex::new(Vec::new());

fn lock_result_or_abort<T>(result: std::sync::LockResult<T>) -> T {
    match result {
        Ok(value) => value,
        Err(_) => std::process::abort(),
    }
}

fn set() -> std::sync::MutexGuard<'static, Vec<usize>> {
    lock_result_or_abort(LIVE.lock())
}

fn insert(live: &mut Vec<usize>, address: usize) {
    if !live.contains(&address) {
        live.push(address);
    }
}

fn remove(live: &mut Vec<usize>, address: usize) -> bool {
    let before = live.len();
    live.retain(|held| *held != address);
    if live.is_empty() {
        *live = Vec::new();
    }
    live.len() != before
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
    set().contains(&(context as usize))
}

#[cfg(test)]
mod tests;
