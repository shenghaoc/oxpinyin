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
        // libzhuyin sorts its candidate lists with
        // `compare_item_with_length_and_frequency` (`zhuyin.cpp:1129-1144`),
        // not the pinyin sort word.
        core.session
            .set_candidate_ranking(oxpinyin_engine::CandidateRanking::LengthThenFrequency);
        // libzhuyin's trellis is `PhoneticLookup<1, 1>` (`zhuyin.cpp:50`),
        // not libpinyin's `<2, 3>`: one value per node, one sentence tail.
        core.session
            .set_nbest_shape(oxpinyin_engine::NbestShape::ZHUYIN);
        Some(CapiInstance {
            context,
            core,
            candidates: oxpinyin_facade::CandidateSnapshot::new(),
        })
    }

    pub(crate) fn load_phrase_library(
        &self,
        index: u32,
    ) -> Result<bool, oxpinyin_facade::LibraryLoadFault> {
        self.core.load_phrase_library(index)
    }

    pub(crate) fn unload_phrase_library(
        &self,
        index: u8,
    ) -> Result<bool, oxpinyin_facade::StoreWriteFailed> {
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
///
/// Sized to need, as the pinyin facade's row is: the text lives in the
/// instance's [`CandidateSnapshot`](oxpinyin_facade::CandidateSnapshot)
/// buffer and the counts are `u32`. It carries only what this facade's
/// getters and its choose read; everything else about the row is in the
/// window it was snapshotted from, at `source_index`.
pub struct CapiCandidate {
    /// The row's string in the snapshot's packed text buffer.
    pub(crate) text: oxpinyin_facade::TextSpan,
    /// The pointer `zhuyin_get_candidate_string` answers — from the row
    /// alone, as the pin reads only the candidate there — kept current by
    /// the snapshot.
    pub(crate) text_ptr: *const std::ffi::c_char,
    pub(crate) candidate_type: crate::types::lookup_candidate_type_t,
    /// The index this candidate held in the window it was snapshotted from.
    pub(crate) source_index: u32,
    /// Where the candidate's span starts, in original input coordinates —
    /// upstream's `m_begin`, which `zhuyin_choose_candidate` answers as the
    /// new cursor for a before-cursor row (`zhuyin.cpp:1660` at the pin).
    pub(crate) span_begin: u32,
}

// The row width the doc above claims; a field that grows it shows up here.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<CapiCandidate>() == 32);

#[cfg(test)]
impl CapiCandidate {
    /// The row's string, for the in-crate tests.
    pub(crate) fn text_str(&self) -> Result<&str, std::str::Utf8Error> {
        if self.text_ptr.is_null() {
            return Ok("");
        }
        // SAFETY: a pushed row's pointer addresses its NUL-terminated string
        // inside the owning snapshot's buffer, which outlives the row and
        // is re-pointed whenever that buffer moves.
        unsafe { std::ffi::CStr::from_ptr(self.text_ptr) }.to_str()
    }
}

impl oxpinyin_facade::SnapshotRow for CapiCandidate {
    fn text_span(&self) -> oxpinyin_facade::TextSpan {
        self.text
    }

    fn set_text(&mut self, span: oxpinyin_facade::TextSpan, ptr: *const std::ffi::c_char) {
        self.text = span;
        self.text_ptr = ptr;
    }
}

/// State behind `zhuyin_instance_t *`.
pub struct CapiInstance {
    /// The owning context's C handle.
    pub(crate) context: *mut ZhuyinContext,
    /// The orchestration half — session, shared handles, live option
    /// word, parse-mode state machine, re-anchored window — shared with
    /// the pinyin facade.
    pub(crate) core: InstanceCore,
    /// Snapshotted candidates, rebuilt by `zhuyin_guess_candidates_*`:
    /// the rows `lookup_candidate_t *` pointers address and the packed
    /// strings they read.
    pub(crate) candidates: oxpinyin_facade::CandidateSnapshot<CapiCandidate>,
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
