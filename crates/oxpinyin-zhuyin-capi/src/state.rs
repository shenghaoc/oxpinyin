//! Real backing state behind the opaque C handles.
//!
//! `CapiContext` lives behind `zhuyin_context_t *` and `CapiInstance`
//! behind `zhuyin_instance_t *`. The opaque `#[repr(C)]` types in
//! [`crate::types`] exist only for the generated C header.
//!
//! The orchestration half of both structs — the runtime assembly, the
//! user store, the live option/scheme word, the parse-mode state machine,
//! the re-anchored window — lives in [`oxpinyin_facade`]'s
//! `ContextCore`/`InstanceCore`, shared with the pinyin facade; this file
//! keeps only the zhuyin-facing shell: the context back-pointer, the ABI
//! key slots, the CString candidate snapshot (with the zhuyin-local
//! 4-value candidate-type enum), and this facade's distinguishing
//! seeds and sentence-row display law.
#![allow(dead_code)]

use oxpinyin_facade::ContextCore;
pub use oxpinyin_facade::InstanceCore;

use crate::types::{ChewingKey, ChewingKeyRest, LookupCandidate, ZhuyinContext, ZhuyinInstance};

/// `USE_TONE | FORCE_TONE` — the option word `zhuyin_init` seeds
/// (`zhuyin.cpp:272` at the pin 0c5e80e1). This is the zhuyin facade's
/// distinguishing default: `pinyin_init` seeds only `PINYIN_INCOMPLETE`.
///
/// Superseded by [`oxpinyin_facade::ZHUYIN_DEFAULT_OPTION_WORD`]; kept as
/// the crate-local name the tests and docs cite.
pub(crate) const ZHUYIN_DEFAULT_OPTIONS: u32 = oxpinyin_facade::ZHUYIN_DEFAULT_OPTION_WORD;

/// The session type every C handle wraps: the shared runtime's concrete
/// session.
pub(crate) type CapiSession = oxpinyin_runtime::RuntimeSession;

/// State behind `zhuyin_context_t *`.
pub(crate) struct CapiContext {
    /// The shared orchestration half: assembly, user store, layered
    /// configuration, and the live option/scheme word.
    pub(crate) core: ContextCore,
}

impl CapiContext {
    /// Opens a context the way `zhuyin_init` does: system tables plus the
    /// optional user dir, health-checked, with `USE_TONE | FORCE_TONE` as
    /// the seeding option word.
    pub(crate) fn open(system_dir: &str, user_dir: &str) -> Option<Self> {
        Some(Self {
            core: ContextCore::open(
                system_dir,
                user_dir,
                oxpinyin_facade::ZHUYIN_DEFAULT_OPTION_WORD,
            )?,
        })
    }

    pub(crate) fn alloc_instance(&self, context: *mut ZhuyinContext) -> Option<CapiInstance> {
        let mut core = self.core.alloc_instance()?;
        // The zhuyin surface's sentence-row display law: upstream fills every
        // BEST_MATCH row from `zhuyin_get_sentence` (always the 1-best), so
        // the observable list carries exactly one sentence row — see
        // `Session::set_collapse_sentence_rows_to_best`.
        core.session.set_collapse_sentence_rows_to_best(true);
        Some(CapiInstance {
            context,
            core,
            key_slot: ChewingKey::ZERO,
            key_rest_slot: ChewingKeyRest { begin: 0, end: 0 },
            candidates: Vec::new(),
        })
    }

    pub(crate) fn load_phrase_library(&self, index: u32) -> bool {
        self.core.load_phrase_library(index)
    }

    pub(crate) fn unload_phrase_library(&self, index: u8) -> bool {
        self.core.unload_phrase_library(index)
    }

    /// Cloned user store, for the import iterator.
    pub(crate) fn user_store(&self) -> Option<oxpinyin_user::UserStore> {
        self.core.user_store()
    }

    /// `zhuyin_save`'s body: `false` without a user dir, otherwise the
    /// store's gated save.
    pub(crate) fn save_user(&mut self) -> bool {
        self.core.save_user()
    }

    /// `zhuyin_mask_out`'s body: the store-level deletion, or `false`
    /// without a user store.
    pub(crate) fn mask_out(&mut self, mask: u32, value: u32) -> bool {
        self.core.mask_out(mask, value)
    }
}

// ── Instance ────────────────────────────────────────────────────────────

/// One snapshotted candidate, stored inside `CapiInstance` so that
/// `lookup_candidate_t *` can borrow into it across C calls.
pub(crate) struct CapiCandidate {
    pub(crate) text: std::ffi::CString,
    pub(crate) kind: oxpinyin_engine::CandidateKind,
    pub(crate) candidate_type: crate::types::lookup_candidate_type_t,
    pub(crate) nbest_index: u8,
    /// Bytes of raw input this candidate consumed, snapshotted at guess time.
    pub(crate) consumed_bytes: usize,
    /// The candidate's scoring token, snapshotted for training.
    pub(crate) token: Option<oxpinyin_core::PhraseToken>,
    /// The index this candidate held in the window it was snapshotted from.
    pub(crate) source_index: usize,
}

/// State behind `zhuyin_instance_t *`.
pub(crate) struct CapiInstance {
    /// The owning context's C handle.
    pub(crate) context: *mut ZhuyinContext,
    /// The orchestration half — session, shared handles, live option
    /// word, parse-mode state machine, re-anchored window — shared with
    /// the pinyin facade.
    pub(crate) core: InstanceCore,
    /// Per-instance slots the `zhuyin_get_zhuyin_key` family hands out.
    pub(crate) key_slot: ChewingKey,
    pub(crate) key_rest_slot: ChewingKeyRest,
    /// Snapshotted candidates, rebuilt by `zhuyin_guess_candidates_*`.
    pub(crate) candidates: Vec<CapiCandidate>,
}

// ── Pointer casts ───────────────────────────────────────────────────────

/// Casts a `*mut ZhuyinContext` to `&CapiContext`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(
/// CapiContext { .. }))`.
pub(crate) unsafe fn context_ref<'a>(ptr: *mut ZhuyinContext) -> &'a CapiContext {
    // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
    unsafe { &*(ptr.cast::<CapiContext>()) }
}

/// Casts a `*mut ZhuyinContext` to `&mut CapiContext`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(
/// CapiContext { .. }))`. No other reference to the same context may exist.
pub(crate) unsafe fn context_mut<'a>(ptr: *mut ZhuyinContext) -> &'a mut CapiContext {
    // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
    unsafe { &mut *(ptr.cast::<CapiContext>()) }
}

/// Casts a `*mut ZhuyinInstance` to `&CapiInstance`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(
/// CapiInstance { .. }))`.
pub(crate) unsafe fn instance_ref<'a>(ptr: *mut ZhuyinInstance) -> &'a CapiInstance {
    // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
    unsafe { &*(ptr.cast::<CapiInstance>()) }
}

/// Casts a `*mut ZhuyinInstance` to `&mut CapiInstance`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(
/// CapiInstance { .. }))`. No other reference to the same instance may exist.
pub(crate) unsafe fn instance_mut<'a>(ptr: *mut ZhuyinInstance) -> &'a mut CapiInstance {
    // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
    unsafe { &mut *(ptr.cast::<CapiInstance>()) }
}

/// Converts a `CapiContext` into a `*mut ZhuyinContext` for return to C.
pub(crate) fn box_context(ctx: CapiContext) -> *mut ZhuyinContext {
    Box::into_raw(Box::new(ctx)).cast()
}

/// Converts a `CapiInstance` into a `*mut ZhuyinInstance` for return to C.
pub(crate) fn box_instance(inst: CapiInstance) -> *mut ZhuyinInstance {
    Box::into_raw(Box::new(inst)).cast()
}

/// Casts a `*mut LookupCandidate` back to `&CapiCandidate`.
///
/// # Safety
///
/// `ptr` must be non-null and point into an active `CapiInstance::candidates`
/// vec (produced by [`candidate_ptr`]).
pub(crate) unsafe fn candidate_ref<'a>(ptr: *mut LookupCandidate) -> &'a CapiCandidate {
    // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
    unsafe { &*(ptr.cast::<CapiCandidate>()) }
}

/// Returns a `*mut LookupCandidate` pointing to a `CapiCandidate`.
pub(crate) fn candidate_ptr(cand: &CapiCandidate) -> *mut LookupCandidate {
    (cand as *const CapiCandidate as *mut CapiCandidate).cast()
}
