//! Shared fixtures for crate tests.

#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use crate::dict::pinyin_token_add_unigram_frequency;

use crate::state::instance_ref;

#[path = "../../tests/support/native_fixture.rs"]
mod native_fixture;
pub(crate) use native_fixture::*;

mod abi;
pub(crate) use abi::*;

#[path = "../../tests/support/c_harness.rs"]
mod c_harness;
pub(crate) use c_harness::*;

use crate::candidates::pinyin_get_candidate;

use crate::types::{LookupCandidate, PinyinContext, PinyinInstance};

// Oxpinyin retains the owner for a surviving instance. This is a safety
// contract, not a pin oracle: the returned public handle keeps its identity
// but is no longer live and must not be dereferenced.
pub(crate) fn finalize_public_context(context: *mut PinyinContext, instance: *mut PinyinInstance) {
    assert_eq!(
        unsafe { crate::instance::pinyin_get_context(instance) },
        context
    );
    unsafe { crate::context::pinyin_fini(context) };
    assert!(!crate::live::is_live(context));
    assert_eq!(
        unsafe { crate::instance::pinyin_get_context(instance) },
        context
    );
}

mod tests;

/// The token snapshotted on the candidate pointer.
pub fn token_of(instance: *mut PinyinInstance, cand: *mut LookupCandidate) -> u32 {
    // SAFETY: `cand` was produced by `pinyin_get_candidate` on `instance`
    // and no later guess invalidated the snapshot.
    let inst = unsafe { instance_ref(instance) };
    inst.state
        .candidates
        .iter()
        .find(|c| std::ptr::eq(*c, cand.cast::<crate::state::CapiCandidate>()))
        .and_then(|c| c.token.get())
        .expect("a phrase candidate carries its token")
        .value()
}

/// Adds the negation of the loaded facade total to token 0x01000001, so
/// the total the model divides by wraps to zero (register row 61).
pub fn wrap_facade_total_to_zero(instance: *mut PinyinInstance) {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let total = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    let total = u32::try_from(total).expect("a guint32 total");
    assert!(unsafe {
        pinyin_token_add_unigram_frequency(instance, 0x0100_0001, total.wrapping_neg())
    });
    // SAFETY: as above.
    let after = unsafe { crate::state::instance_ref(instance) }
        .core
        .lm
        .amplified_total();
    assert_eq!(after, 0, "the facade total wraps to zero");
}

pub(crate) fn ucs4(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| u32::from(ch).to_le_bytes())
        .collect()
}

#[cfg(feature = "tkrzw")]
#[path = "../../tests/common/contract_data.rs"]
pub(crate) mod contract_data;

use crate::types::lookup_candidate_type_t;
use oxpinyin::user::UserStore;
use std::ptr;

/// Read the live store for one assertion; no storage reference escapes.
pub(crate) fn with_store<R>(
    instance: *mut PinyinInstance,
    read: impl FnOnce(&UserStore) -> R,
) -> R {
    // SAFETY: tests call this only with a live instance, between serialized
    // C operations. The closure cannot return a borrow into the store.
    let inst = unsafe { instance_ref(instance) };
    read(inst.core.user)
}

/// The first snapshot row of `kind`, with its token.
pub(crate) fn predicted_row(
    instance: *mut PinyinInstance,
    kind: lookup_candidate_type_t,
) -> (*mut LookupCandidate, u32) {
    // SAFETY: `instance` is a live `pinyin_alloc_instance` handle.
    let inst = unsafe { instance_ref(instance) };
    let index = inst
        .candidates
        .iter()
        .position(|c| c.candidate_type() == kind && c.token.get().is_some())
        .expect("the prediction holds a row of this kind");
    let mut cand: *mut LookupCandidate = ptr::null_mut();
    assert!(unsafe {
        pinyin_get_candidate(
            instance,
            u32::try_from(index).expect("small candidate index"),
            &raw mut cand,
        )
    });
    (cand, token_of(instance, cand))
}
