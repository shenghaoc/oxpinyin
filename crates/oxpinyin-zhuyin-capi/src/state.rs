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
//! keeps only the zhuyin-facing shell: the ABI
//! key slots, the `CString` candidate snapshot (with the zhuyin-local
//! 4-value candidate-type enum), and this facade's distinguishing
//! seeds and sentence-row display law.

pub use oxpinyin_facade::InstanceCore;
use std::path::Path;
use std::{cell::UnsafeCell, rc::Rc};

use oxpinyin_facade::{ContextCore, OpenFailure};

use crate::types::{LookupCandidate, ZhuyinContext, ZhuyinInstance};

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

    pub(crate) fn alloc_instance(&self, owner: ContextOwner) -> CapiInstance {
        let mut core = self.core.alloc_instance();
        // The zhuyin surface's sentence-row display law: upstream fills every
        // BEST_MATCH row from `zhuyin_get_sentence` (always the 1-best), so
        // the observable list carries exactly one sentence row — see
        // `Session::set_collapse_sentence_rows_to_best`.
        let runtime = &self.core.runtime;
        let mut session = runtime.session(&mut core.session);
        session.set_collapse_sentence_rows_to_best(true);
        // libzhuyin sorts its candidate lists with
        // `compare_item_with_length_and_frequency` (`zhuyin.cpp:1129-1144`),
        // not the pinyin sort word.
        session.set_candidate_ranking(oxpinyin_engine::CandidateRanking::LengthThenFrequency);
        // libzhuyin's trellis is `PhoneticLookup<1, 1>` (`zhuyin.cpp:50`),
        // not libpinyin's `<2, 3>`: one value per node, one sentence tail.
        session.set_nbest_shape(oxpinyin_engine::NbestShape::ZHUYIN);
        CapiInstance {
            owner,
            core,
            state: CapiInstanceState {
                candidates: oxpinyin_facade::CandidateSnapshot::new(),
            },
        }
    }
}

// ── Instance ────────────────────────────────────────────────────────────

/// One snapshotted candidate, stored inside `CapiInstance` so that
/// `lookup_candidate_t *` can borrow into it across C calls.
///
/// Sized to need, as the pinyin facade's row is: the text lives in the
/// instance's [`CandidateSnapshot`](oxpinyin_facade::CandidateSnapshot)
/// buffer and the counts are `u32`. It carries only what this facade's
/// getters and its choose read: the pin's choose reads the row's own
/// `m_begin`, `m_end` and `m_token` (`zhuyin.cpp:1646-1666` at 074a2219),
/// on whichever instance a caller passes it to.
pub struct CapiCandidate {
    /// The row's string in the snapshot's packed text buffer.
    pub(crate) text: oxpinyin_facade::TextSpan,
    /// The pointer `zhuyin_get_candidate_string` answers — from the row
    /// alone, as the pin reads only the candidate there — kept current by
    /// the snapshot.
    pub(crate) text_ptr: *const std::ffi::c_char,
    pub(crate) candidate_type: crate::types::lookup_candidate_type_t,
    /// Where the candidate's span starts, in original input coordinates —
    /// upstream's `guint16 m_begin` (`zhuyin.cpp:83` at the pin), which
    /// `zhuyin_choose_candidate` answers as the new cursor for a
    /// before-cursor row (`:1660`).
    pub(crate) span_begin: u16,
    /// Original `guint16 m_end` (`zhuyin.cpp:84`), the after-cursor answer,
    /// also returned when reparsing makes the span refuse.
    pub(crate) span_end: u16,
    /// The row's scoring token, `m_token`, which a choose forces.
    pub(crate) token: Option<oxpinyin_core::PhraseToken>,
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
pub struct CapiInstance<C = InstanceCore, S = CapiInstanceState, O = ContextOwner> {
    /// Retains the sole runtime allocation after the public context is finalised.
    pub(crate) owner: O,
    /// The orchestration half — composition, live option
    /// word, parse-mode state machine, re-anchored window — shared with
    /// the pinyin facade.
    pub(crate) core: C,
    pub(crate) state: S,
}

/// ABI presentation state retained independently of phrase storage.
pub struct CapiInstanceState {
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
impl<C, S: std::borrow::Borrow<CapiInstanceState>, O> std::ops::Deref for CapiInstance<C, S, O> {
    type Target = CapiInstanceState;
    fn deref(&self) -> &Self::Target {
        self.state.borrow()
    }
}
impl<C, S: std::borrow::BorrowMut<CapiInstanceState>, O> std::ops::DerefMut
    for CapiInstance<C, S, O>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state.borrow_mut()
    }
}

pub type CapiInstanceRead<'a> =
    CapiInstance<oxpinyin_facade::InstanceRead<'a>, &'a CapiInstanceState, &'a ContextOwner>;
pub type CapiInstanceMut<'a> =
    CapiInstance<oxpinyin_facade::InstanceMut<'a>, &'a mut CapiInstanceState, &'a ContextOwner>;

impl<
    S: std::borrow::Borrow<oxpinyin_facade::InstanceState>,
    E: std::borrow::Borrow<oxpinyin_engine::SessionState>,
    V: std::borrow::Borrow<CapiInstanceState>,
    O: std::borrow::Borrow<ContextOwner>,
> CapiInstance<oxpinyin_facade::InstanceView<'_, S, E>, V, O>
{
    pub(crate) fn read(&self) -> CapiInstanceRead<'_> {
        CapiInstance {
            owner: self.owner.borrow(),
            core: self.core.read(),
            state: self.state.borrow(),
        }
    }
}

/// Shared ownership of one context allocation, including its sole runtime.
/// Handles retain ownership; every access still borrows only for one C call.
pub type ContextOwner = Rc<UnsafeCell<CapiContext>>;

oxpinyin_capi_marshal::state_handle_helpers! {
    context: ZhuyinContext, instance: ZhuyinInstance, instance_binding: instance, error: oxpinyin_engine::EngineError, context_field: {  }
}

mod handles {
    use super::*;
    oxpinyin_capi_marshal::opaque_handle_casts! {
        vis: pub(crate),
        context: ZhuyinContext => ContextOwner,
        instance: ZhuyinInstance => CapiInstance,
        candidate: LookupCandidate => CapiCandidate,
    }
}
pub(crate) use handles::{box_instance, candidate_ptr, candidate_ref};
