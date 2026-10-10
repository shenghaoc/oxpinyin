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
//! key slots, the `CString` candidate snapshot (with the zhuyin-local
//! 4-value candidate-type enum), and this facade's distinguishing
//! seeds and sentence-row display law.

pub use oxpinyin_facade::InstanceCore;
use std::path::Path;

use oxpinyin_facade::{ContextCore, OpenFailure};

use crate::types::{LookupCandidate, ZhuyinContext, ZhuyinInstance};

/// `USE_TONE | FORCE_TONE` — the option word `zhuyin_init` seeds
/// (`zhuyin.cpp:273` at 0c5e80e1 and at the 074a2219 pin). This is the zhuyin facade's
/// distinguishing default: `FORCE_TONE` is on, unlike `pinyin_init`'s `USE_TONE`.
///
/// Superseded by [`oxpinyin_facade::ZHUYIN_DEFAULT_OPTION_WORD`]; kept as
/// the crate-local name the tests cite.
#[cfg(test)]
pub const ZHUYIN_DEFAULT_OPTIONS: u32 = oxpinyin_facade::ZHUYIN_DEFAULT_OPTION_WORD;

/// State behind `zhuyin_context_t *`.
pub struct CapiContext {
    /// The shared orchestration half: assembly, user store, layered
    /// configuration, and the live option/scheme word.
    pub(crate) core: ContextCore,
}

impl CapiContext {
    /// Opens a context the way `zhuyin_init` does: system tables plus the
    /// optional user dir, health-checked, with `USE_TONE | FORCE_TONE` as
    /// the seeding option word.
    /// Opens a context; the failure is kept for `zhuyin_init`'s log line.
    pub(crate) fn try_open(
        system_dir: &Path,
        user_dir: Option<&Path>,
    ) -> Result<Self, OpenFailure> {
        Ok(Self {
            core: ContextCore::try_open(
                system_dir,
                user_dir,
                oxpinyin_facade::ZHUYIN_DEFAULT_OPTION_WORD,
                oxpinyin_user::UserConfLaw::Zhuyin,
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
        // libzhuyin's trellis is `PhoneticLookup<1, 1>` (`zhuyin.cpp:50`),
        // not libpinyin's `<2, 3>`: one value per node, one sentence tail.
        core.session
            .set_nbest_shape(oxpinyin_engine::NbestShape::ZHUYIN);
        Some(CapiInstance {
            context,
            core,
            candidates: Vec::new(),
        })
    }

    pub(crate) fn load_phrase_library(
        &self,
        index: u32,
    ) -> Result<bool, oxpinyin_facade::LibraryRowAssert> {
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
    /// store's gated save. A chunk header write the pin dies on comes back
    /// as [`SaveOutcome::ChunkWriteFailed`] so the caller can log it.
    pub(crate) fn save_user(&mut self) -> oxpinyin_facade::SaveOutcome {
        self.core.save_user()
    }

    /// `zhuyin_mask_out`'s body: the store-level deletion, or
    /// [`MaskOutOutcome::Done(false)`](oxpinyin_facade::MaskOutOutcome::Done)
    /// without a user store. An over-long user pinyin index key comes back
    /// as [`MaskOutOutcome::OverlongIndexKey`](oxpinyin_facade::MaskOutOutcome::OverlongIndexKey)
    /// so the C entry can log it (class (c),
    /// `chewing_large_table2_bdb.cpp:529`).
    pub(crate) fn mask_out(&mut self, mask: u32, value: u32) -> oxpinyin_facade::MaskOutOutcome {
        self.core.mask_out(mask, value)
    }
}

// ── Instance ────────────────────────────────────────────────────────────

/// One snapshotted candidate, stored inside `CapiInstance` so that
/// `lookup_candidate_t *` can borrow into it across C calls.
pub struct CapiCandidate {
    pub(crate) text: std::ffi::CString,
    /// The four fields below are snapshotted exactly as the pinyin facade
    /// snapshots them, but this facade's display law reads only `text`,
    /// `candidate_type` and `source_index` today.
    #[expect(
        dead_code,
        reason = "snapshotted in step with oxpinyin-capi; no reader on the zhuyin display law yet"
    )]
    pub(crate) kind: oxpinyin_engine::CandidateKind,
    pub(crate) candidate_type: crate::types::lookup_candidate_type_t,
    #[expect(
        dead_code,
        reason = "snapshotted in step with oxpinyin-capi; no reader on the zhuyin display law yet"
    )]
    pub(crate) nbest_index: u8,
    /// Bytes of raw input this candidate consumed, snapshotted at guess time.
    #[expect(
        dead_code,
        reason = "snapshotted in step with oxpinyin-capi; no reader on the zhuyin display law yet"
    )]
    pub(crate) consumed_bytes: usize,
    /// The candidate's scoring token, snapshotted for training.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "snapshotted in step with oxpinyin-capi; read by the in-crate tests only"
        )
    )]
    pub(crate) token: Option<oxpinyin_core::PhraseToken>,
    /// The index this candidate held in the window it was snapshotted from.
    pub(crate) source_index: usize,
    /// Where the candidate's span starts, in original input coordinates —
    /// upstream's `m_begin`, which `zhuyin_choose_candidate` answers as the
    /// new cursor for a before-cursor row (`zhuyin.cpp:1660` at the pin).
    pub(crate) span_begin: usize,
}

/// State behind `zhuyin_instance_t *`.
pub struct CapiInstance {
    /// The owning context's C handle.
    pub(crate) context: *mut ZhuyinContext,
    /// The orchestration half — session, shared handles, live option
    /// word, parse-mode state machine, re-anchored window — shared with
    /// the pinyin facade.
    pub(crate) core: InstanceCore,
    /// Snapshotted candidates, rebuilt by `zhuyin_guess_candidates_*`.
    pub(crate) candidates: Vec<CapiCandidate>,
}

// ── Pointer casts ───────────────────────────────────────────────────────
//
// The opaque `ZhuyinContext` / `ZhuyinInstance` / `LookupCandidate` types in
// the C header are zero-sized sentinels; the pointers actually address a
// heap-allocated `CapiContext` / `CapiInstance` / `CapiCandidate`. The eight
// cast/box helpers (context_ref/mut, instance_ref/mut, box_context/instance,
// candidate_ref/ptr) are stamped by the shared marshalling macro so this
// facade and the pinyin one cannot drift; the generated `unsafe` blocks land
// here in the C-ABI crate, where the constitution's allowlist permits them.
oxpinyin_capi_marshal::opaque_handle_casts! {
    vis: pub(crate),
    context: ZhuyinContext => CapiContext,
    instance: ZhuyinInstance => CapiInstance,
    candidate: LookupCandidate => CapiCandidate,
}
