//! Shared fixtures for crate tests — the zhuyin twin of
//! `oxpinyin-capi/src/test_support.rs`.

use std::path::PathBuf;

use crate::state::instance_mut;

use crate::types::ZhuyinInstance;

#[path = "../../tests/support/native_fixture.rs"]
mod native_fixture;
pub(crate) use native_fixture::*;

mod abi;
pub(crate) use abi::*;

#[path = "../../tests/support/c_harness.rs"]
mod c_harness;
pub(crate) use c_harness::*;

/// The text a snapshot row carries, by row index.
///
/// # Safety
///
/// `instance` is a live instance, not NULL: `zhuyin_alloc_instance` returned it and
/// `zhuyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule).
pub unsafe fn candidate_text(instance: *mut ZhuyinInstance, index: usize) -> String {
    // SAFETY: `instance` is live and was produced by
    // `zhuyin_alloc_instance`; the borrow ends with this function.
    let inst = unsafe { instance_mut(instance) };
    inst.state.candidates[index]
        .text_str()
        .expect("candidate text is UTF-8")
        .to_owned()
}

/// Runs `f` against the instance's user store handle (the same
/// connection the entry points write through; every update commits
/// before returning). The borrow is scoped to the callback, so it cannot
/// outlive the instance or its close.
///
/// # Safety
///
/// `instance` is a live instance, not NULL: `zhuyin_alloc_instance` returned it and
/// `zhuyin_free_instance` has not freed it.
///
/// Calls are serial across the context, its sibling instances and its iterators: no other call
/// through any of them, and no borrow of their state, overlaps this call (the crate's Serial calls
/// rule). `f` runs while this helper holds the store reference, so it must not call through the
/// context, its instances or its iterators.
pub unsafe fn with_store<R>(
    instance: *mut ZhuyinInstance,
    f: impl FnOnce(&oxpinyin::user::UserStore) -> R,
) -> R {
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`; the store is a value field, and the shared
    // reference lives only for the duration of `f`.
    let inst = unsafe { instance_mut(instance) };
    f(inst.core.user)
}

/// Hand-built `PhraseIndexLogger` records for the journal-replay tests
#[path = "../../tests/support/c_harness_tests.rs"]
mod tests;
