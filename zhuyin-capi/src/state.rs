//! Real backing state behind the opaque C handles.
//!
//! `CapiContext` lives behind `zhuyin_context_t *` and `CapiInstance`
//! behind `zhuyin_instance_t *`. The opaque `#[repr(C)]` types in
//! [`crate::types`] represent the opaque handles declared by the C header.
//!
//! The orchestration half of both structs — the runtime assembly, the
//! user store, the live option/scheme word, the parse-mode state machine,
//! the re-anchored window — lives in [`oxpinyin::facade`]'s
//! `ContextCore`/`InstanceCore`, shared with the pinyin facade; this file
//! keeps only the zhuyin-facing shell: the ABI
//! key slots, the `CString` candidate snapshot (with the zhuyin-local
//! 4-value candidate-type enum), and this facade's distinguishing
//! seeds and sentence-row display law.

use oxpinyin::facade::InstanceCore;
use std::path::Path;
use std::{cell::UnsafeCell, rc::Rc};

use oxpinyin::facade::{ContextCore, OpenFailure};

use crate::types::{LookupCandidate, ZhuyinContext, ZhuyinInstance};

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
    /// Opens a context; the failure is kept for `zhuyin_init`'s log line.
    pub(crate) fn try_open(
        system_dir: &Path,
        user_dir: Option<&Path>,
    ) -> Result<Self, OpenFailure> {
        Ok(Self {
            core: ContextCore::try_open(
                system_dir,
                user_dir,
                oxpinyin::facade::ZHUYIN_DEFAULT_OPTION_WORD,
                oxpinyin::user::UserConfLaw::Zhuyin,
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
        session.set_candidate_ranking(oxpinyin::engine::CandidateRanking::LengthThenFrequency);
        // libzhuyin's trellis is `PhoneticLookup<1, 1>` (`zhuyin.cpp:50`),
        // not libpinyin's `<2, 3>`: one value per node, one sentence tail.
        session.set_nbest_shape(oxpinyin::engine::NbestShape::ZHUYIN);
        CapiInstance {
            owner,
            core,
            state: CapiInstanceState {
                candidates: oxpinyin::facade::CandidateSnapshot::new(),
            },
        }
    }
}

// ── Instance ────────────────────────────────────────────────────────────

/// One snapshotted candidate, stored inside `CapiInstance` so that
/// `lookup_candidate_t *` can borrow into it across C calls.
///
/// Sized to need, as the pinyin facade's row is: the text lives in the
/// instance's [`CandidateSnapshot`](oxpinyin::facade::CandidateSnapshot)
/// buffer and the counts are `u32`. It carries only what this facade's
/// getters and its choose read: the pin's choose reads the row's own
/// `m_begin`, `m_end` and `m_token` (`zhuyin.cpp:1646-1666` at 074a2219),
/// on whichever instance a caller passes it to.
pub(crate) struct CapiCandidate {
    /// The row's string in the snapshot's packed text buffer.
    pub(crate) text: oxpinyin::facade::TextSpan,
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
    pub(crate) token: Option<oxpinyin::vocab::PhraseToken>,
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

impl oxpinyin::facade::SnapshotRow for CapiCandidate {
    fn text_span(&self) -> oxpinyin::facade::TextSpan {
        self.text
    }

    fn set_text(&mut self, span: oxpinyin::facade::TextSpan, ptr: *const std::ffi::c_char) {
        self.text = span;
        self.text_ptr = ptr;
    }
}

/// State behind `zhuyin_instance_t *`.
pub(crate) struct CapiInstance<C = InstanceCore, S = CapiInstanceState, O = ContextOwner> {
    /// Retains the sole runtime allocation after the public context is finalised.
    pub(crate) owner: O,
    /// The orchestration half — composition, live option
    /// word, parse-mode state machine, re-anchored window — shared with
    /// the pinyin facade.
    pub(crate) core: C,
    pub(crate) state: S,
}

/// ABI presentation state retained independently of phrase storage.
pub(crate) struct CapiInstanceState {
    /// Snapshotted candidates, rebuilt by `zhuyin_guess_candidates_*`:
    /// the rows `lookup_candidate_t *` pointers address and the packed
    /// strings they read.
    pub(crate) candidates: oxpinyin::facade::CandidateSnapshot<CapiCandidate>,
}

pub(crate) type CapiInstanceRead<'a> =
    CapiInstance<oxpinyin::facade::InstanceRead<'a>, &'a CapiInstanceState, &'a ContextOwner>;
pub(crate) type CapiInstanceMut<'a> =
    CapiInstance<oxpinyin::facade::InstanceMut<'a>, &'a mut CapiInstanceState, &'a ContextOwner>;

impl<
    S: std::borrow::Borrow<oxpinyin::facade::InstanceState>,
    E: std::borrow::Borrow<oxpinyin::engine::SessionState>,
    V: std::borrow::Borrow<CapiInstanceState>,
    O: std::borrow::Borrow<ContextOwner>,
> CapiInstance<oxpinyin::facade::InstanceView<'_, S, E>, V, O>
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
pub(crate) type ContextOwner = Rc<UnsafeCell<CapiContext>>;

/// Borrow storage retained by a handle, including after context fini.
/// # Safety
/// The C caller must serialize calls on this context and all its handles;
/// no mutation may overlap the returned operation-scoped borrow.
pub(crate) unsafe fn owner_ref(owner: &ContextOwner) -> &CapiContext {
    // SAFETY: the owner keeps the allocation alive, and the caller's
    // one-call-at-a-time contract excludes mutation during this borrow.
    unsafe { &*owner.get() }
}

/// Borrow the shared allocation exclusively for one C call.
/// # Safety
/// No other call or retained Rust borrow may access this allocation until
/// the returned borrow ends, including through another owning handle.
pub(crate) unsafe fn owner_mut(owner: &mut ContextOwner) -> &mut CapiContext {
    // SAFETY: ownership guarantees liveness; the C caller's one-call-at-a-time
    // contract, not uniqueness of the reference count, guarantees exclusivity.
    unsafe { &mut *owner.get() }
}

/// Retain the backing allocation while the public context handle is live.
/// # Safety
/// `ptr` must be a live context; the caller serializes operations on it.
pub(crate) unsafe fn context_owner(ptr: *mut ZhuyinContext) -> ContextOwner {
    // SAFETY: the public handle is live for this call; clone only its owner.
    Rc::clone(unsafe { handles::context_ref(ptr) })
}

/// Box the context's reference-counted owner as the public C handle.
pub(crate) fn box_context(context: CapiContext) -> *mut ZhuyinContext {
    handles::box_context(Rc::new(UnsafeCell::new(context)))
}

/// Borrow a live public context for one operation.
/// # Safety
/// `ptr` is live and the caller excludes mutation during the borrow.
pub(crate) unsafe fn context_ref<'a>(ptr: *mut ZhuyinContext) -> &'a CapiContext {
    // SAFETY: the caller keeps the public handle live and serializes its calls.
    unsafe { owner_ref(handles::context_ref(ptr)) }
}

/// Borrow a live public context exclusively for one operation.
/// # Safety
/// `ptr` is live; no call or borrow through any handle overlaps this one.
pub(crate) unsafe fn context_mut<'a>(ptr: *mut ZhuyinContext) -> &'a mut CapiContext {
    // SAFETY: the caller keeps the public handle live and grants exclusive access.
    unsafe { owner_mut(handles::context_mut(ptr)) }
}

/// Borrow the runtime retained by an instance, without its public context.
/// # Safety
/// `ptr` is live; all earlier readers have ended and the C caller grants
/// exclusive access across every handle sharing this runtime for this call.
pub(crate) unsafe fn instance_context_mut<'a>(ptr: *mut ZhuyinInstance) -> &'a mut CapiContext {
    // SAFETY: the instance owns the allocation; serialized C calls exclude
    // access through any other instance, iterator or the public context.
    unsafe { owner_mut(&mut handles::instance_mut(ptr).owner) }
}

/// Borrow this call's instance and its live context.
/// # Safety
/// The instance must be live and all calls sharing its runtime serialized.
/// No instance or context mutation may overlap the returned reader.
pub(crate) unsafe fn instance_ref<'a>(ptr: *mut ZhuyinInstance) -> CapiInstanceRead<'a> {
    // SAFETY: the caller supplies a live instance and serializes all shared-handle calls.
    let instance = unsafe { handles::instance_ref(ptr) };
    // SAFETY: the instance retains the allocation; the C caller excludes concurrent mutation.
    let context = unsafe { owner_ref(&instance.owner) };
    let runtime = &context.core.runtime;
    CapiInstance {
        owner: &instance.owner,
        core: instance.core.read(runtime),
        state: &instance.state,
    }
}

/// Borrow composition exclusively and phrase storage for this call only.
/// # Safety
/// The instance must be live, with no overlapping access through any shared handle.
pub(crate) unsafe fn instance_mut<'a>(ptr: *mut ZhuyinInstance) -> CapiInstanceMut<'a> {
    // SAFETY: the caller grants exclusive access to this live instance.
    let instance = unsafe { handles::instance_mut(ptr) };
    // SAFETY: the instance retains the allocation; the C caller excludes runtime mutation.
    let context = unsafe { owner_ref(&instance.owner) };
    let runtime = &context.core.runtime;
    CapiInstance {
        owner: &instance.owner,
        core: instance.core.bind(runtime),
        state: &mut instance.state,
    }
}

/// Train with exclusive access to the context, after all read views have ended.
/// # Safety
/// The caller supplies a live instance and exclusive access across all shared handles.
pub(crate) unsafe fn train_instance(
    ptr: *mut ZhuyinInstance,
    index: u8,
) -> Result<bool, oxpinyin::engine::EngineError> {
    // SAFETY: the caller grants this operation exclusive access to the live instance.
    let instance = unsafe { handles::instance_mut(ptr) };
    // SAFETY: the owner retains the allocation; serialized C calls exclude other readers.
    let context = unsafe { owner_mut(&mut instance.owner) };
    context.core.train(&instance.core, index)
}

mod handles {
    use super::*;

    /// Grants a shared borrow of the boxed owner.
    ///
    /// # Safety
    ///
    /// `ptr` must come from `box_context` and remain live through `'a`.
    /// No mutation of the boxed owner may overlap the borrow.
    pub(crate) unsafe fn context_ref<'a>(ptr: *mut ZhuyinContext) -> &'a ContextOwner {
        // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
        unsafe { &*(ptr.cast::<ContextOwner>()) }
    }

    /// Grants exclusive access to the boxed owner.
    ///
    /// # Safety
    ///
    /// `ptr` must come from `box_context` and remain live through `'a`.
    /// No other access to the boxed owner may overlap the borrow.
    pub(crate) unsafe fn context_mut<'a>(ptr: *mut ZhuyinContext) -> &'a mut ContextOwner {
        // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
        unsafe { &mut *(ptr.cast::<ContextOwner>()) }
    }

    /// Grants a shared borrow of the boxed instance.
    ///
    /// # Safety
    ///
    /// `ptr` must come from `box_instance` and remain live through `'a`.
    /// No mutation of the boxed instance may overlap the borrow.
    pub(crate) unsafe fn instance_ref<'a>(ptr: *mut ZhuyinInstance) -> &'a CapiInstance {
        // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
        unsafe { &*(ptr.cast::<CapiInstance>()) }
    }

    /// Grants exclusive access to the boxed instance.
    ///
    /// # Safety
    ///
    /// `ptr` must come from `box_instance` and remain live through `'a`.
    /// No other access to the boxed instance may overlap the borrow.
    pub(crate) unsafe fn instance_mut<'a>(ptr: *mut ZhuyinInstance) -> &'a mut CapiInstance {
        // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
        unsafe { &mut *(ptr.cast::<CapiInstance>()) }
    }

    /// Converts a backing context into a handle pointer for return to C.
    pub(crate) fn box_context(ctx: ContextOwner) -> *mut ZhuyinContext {
        Box::into_raw(Box::new(ctx)).cast()
    }

    /// Converts a backing instance into a handle pointer for return to C.
    pub(crate) fn box_instance(inst: CapiInstance) -> *mut ZhuyinInstance {
        Box::into_raw(Box::new(inst)).cast()
    }

    /// Casts a lookup-candidate handle pointer back to its backing.
    ///
    /// # Safety
    ///
    /// `ptr` must be non-null and point into an active backing
    /// `candidates` vec (produced by `candidate_ptr`). The candidate must
    /// stay alive, and valid for shared access, for the returned
    /// lifetime `'a`.
    pub(crate) unsafe fn candidate_ref<'a>(ptr: *mut LookupCandidate) -> &'a CapiCandidate {
        // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
        unsafe { &*(ptr.cast::<CapiCandidate>()) }
    }

    /// Returns a lookup-candidate handle pointer to a backing candidate.
    pub(crate) const fn candidate_ptr(cand: &CapiCandidate) -> *mut LookupCandidate {
        (cand as *const CapiCandidate as *mut CapiCandidate).cast()
    }
}
pub(crate) use handles::{box_instance, candidate_ptr, candidate_ref};
