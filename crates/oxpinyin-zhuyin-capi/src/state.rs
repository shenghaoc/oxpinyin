//! Real backing state behind the opaque C handles.
//!
//! `CapiContext` lives behind `zhuyin_context_t *` and `CapiInstance`
//! behind `zhuyin_instance_t *`. The opaque `#[repr(C)]` types in
//! [`crate::types`] exist only for the generated C header.
#![allow(dead_code)]

use std::ffi::CString;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use oxpinyin_core::{
    DoublePinyinScheme, FORCE_TONE, FullPinyinScheme, OptionBits, PhraseToken, USE_TONE,
    ZhuyinParse, ZhuyinScheme,
};
use oxpinyin_engine::{
    CandidateKind, CandidateList, Config, EngineError, check_lookup_offset_range,
    normalize_lookup_offset,
};
use oxpinyin_runtime::{Runtime, RuntimeSession};
pub(crate) use oxpinyin_runtime::{RuntimeDict as SharedDict, RuntimeLm as SharedLm};
use oxpinyin_user::UserStore;

use crate::types::{ChewingKey, ChewingKeyRest, LookupCandidate, ZhuyinContext, ZhuyinInstance};

/// `USE_TONE | FORCE_TONE` — the option word `zhuyin_init` seeds
/// (`zhuyin.cpp:272` at the pin 0c5e80e1). This is the zhuyin facade's
/// distinguishing default: `pinyin_init` seeds only `PINYIN_INCOMPLETE`.
pub(crate) const ZHUYIN_DEFAULT_OPTIONS: u32 = USE_TONE | FORCE_TONE;

/// The session type every C handle wraps: the shared runtime's concrete
/// session.
pub(crate) type CapiSession = RuntimeSession;

/// State behind `zhuyin_context_t *`.
pub(crate) struct CapiContext {
    pub(crate) config: Config,
    /// The shared concrete assembly; `None` under a user-store-only context.
    pub(crate) runtime: Option<Runtime>,
    /// The user-learning store, shared by value-clone with every instance.
    user: Option<UserStore>,
    /// Live `PINYIN_INCOMPLETE` bit.
    pub(crate) incomplete: Arc<AtomicBool>,
    /// Live double-pinyin scheme.
    pub(crate) double_scheme: Arc<AtomicI32>,
    /// Live Zhuyin scheme.
    pub(crate) zhuyin_scheme: Arc<AtomicI32>,
    /// Live full-pinyin scheme.
    pub(crate) full_scheme: Arc<AtomicI32>,
    /// Live `USE_TONE` bit.
    pub(crate) use_tone: Arc<AtomicBool>,
    /// Live `FORCE_TONE` bit (nested under `USE_TONE` by the zhuyin parser).
    pub(crate) force_tone: Arc<AtomicBool>,
    /// Live option word. Shared with every instance so `zhuyin_set_options`
    /// remasks already-allocated sessions.
    pub(crate) options: Arc<AtomicU32>,
}

impl CapiContext {
    /// Opens a context the way `zhuyin_init` does: system tables plus the
    /// optional user dir, health-checked, with `USE_TONE | FORCE_TONE` as
    /// the seeding option word.
    pub(crate) fn open(system_dir: &str, user_dir: &str) -> Option<Self> {
        if system_dir.is_empty() {
            return None;
        }
        let sys = Path::new(system_dir);
        let runtime = Runtime::open(sys, Some(Path::new(user_dir))).ok()?;
        let user = runtime.user_store();
        Some(Self {
            config: Config::default(),
            runtime: Some(runtime),
            user,
            // The pin's `zhuyin_init` seeds `m_options = USE_TONE | FORCE_TONE`
            // with NO `ZHUYIN_INCOMPLETE` (`zhuyin.cpp:273`), so the default
            // is incomplete OFF — unlike `pinyin_init` (which seeds
            // `PINYIN_INCOMPLETE`). Mirrored here; a caller remasks it with
            // `zhuyin_set_options`.
            incomplete: Arc::new(AtomicBool::new(false)),
            double_scheme: Arc::new(AtomicI32::new(DoublePinyinScheme::Ms as i32)),
            zhuyin_scheme: Arc::new(AtomicI32::new(ZhuyinScheme::Standard as i32)),
            full_scheme: Arc::new(AtomicI32::new(FullPinyinScheme::Hanyu as i32)),
            use_tone: Arc::new(AtomicBool::new(true)),
            force_tone: Arc::new(AtomicBool::new(true)),
            options: Arc::new(AtomicU32::new(ZHUYIN_DEFAULT_OPTIONS)),
        })
    }

    pub(crate) fn alloc_instance(&self, context: *mut ZhuyinContext) -> Option<CapiInstance> {
        let runtime = self.runtime.as_ref()?;
        let mut session = runtime.new_session(&self.config).ok()?;
        // The zhuyin surface's sentence-row display law: upstream fills every
        // BEST_MATCH row from `zhuyin_get_sentence` (always the 1-best), so
        // the observable list carries exactly one sentence row — see
        // `Session::set_collapse_sentence_rows_to_best`.
        session.set_collapse_sentence_rows_to_best(true);
        Some(CapiInstance {
            context,
            session,
            phrase_result: Vec::new(),
            key_slot: ChewingKey::ZERO,
            key_rest_slot: ChewingKeyRest { begin: 0, end: 0 },
            candidates: Vec::new(),
            anchored_window: None,
            parsed_len: 0,
            user: self.user.clone(),
            dict: runtime.dict(),
            lm: runtime.lm(),
            incomplete: Arc::clone(&self.incomplete),
            double_scheme: Arc::clone(&self.double_scheme),
            zhuyin_scheme: Arc::clone(&self.zhuyin_scheme),
            full_scheme: Arc::clone(&self.full_scheme),
            use_tone: Arc::clone(&self.use_tone),
            force_tone: Arc::clone(&self.force_tone),
            options: Arc::clone(&self.options),
            zhuyin_parse: None,
            zhuyin_input: String::new(),
            full_parse: None,
            full_input: String::new(),
        })
    }

    pub(crate) fn load_phrase_library(&self, index: u32) -> bool {
        match self.runtime.as_ref() {
            Some(runtime) => runtime.load_library(index),
            None => false,
        }
    }

    pub(crate) fn unload_phrase_library(&self, index: u8) -> bool {
        match self.runtime.as_ref() {
            Some(runtime) => runtime.unload_library(index as u32),
            None => false,
        }
    }

    /// Cloned user store, for the import iterator.
    pub(crate) fn user_store(&self) -> Option<UserStore> {
        self.user.clone()
    }

    /// `zhuyin_save`'s body: `false` without a user dir, otherwise the
    /// store's gated save.
    pub(crate) fn save_user(&mut self) -> bool {
        match self.user.as_mut() {
            None => false,
            Some(store) => store.save().unwrap_or(false),
        }
    }

    /// `zhuyin_mask_out`'s body: the store-level deletion, or `false`
    /// without a user store.
    pub(crate) fn mask_out(&mut self, mask: u32, value: u32) -> bool {
        match self.user.as_mut() {
            None => false,
            Some(store) => store.mask_out(mask, value).is_ok(),
        }
    }
}

// ── Instance ────────────────────────────────────────────────────────────

/// One snapshotted candidate, stored inside `CapiInstance` so that
/// `lookup_candidate_t *` can borrow into it across C calls.
pub(crate) struct CapiCandidate {
    pub(crate) text: CString,
    pub(crate) kind: CandidateKind,
    pub(crate) candidate_type: crate::types::lookup_candidate_type_t,
    pub(crate) nbest_index: u8,
    /// Bytes of raw input this candidate consumed, snapshotted at guess time.
    pub(crate) consumed_bytes: usize,
    /// The candidate's scoring token, snapshotted for training.
    pub(crate) token: Option<PhraseToken>,
    /// The index this candidate held in the window it was snapshotted from.
    pub(crate) source_index: usize,
}

/// State behind `zhuyin_instance_t *`.
pub(crate) struct CapiInstance {
    /// The owning context's C handle.
    pub(crate) context: *mut ZhuyinContext,
    pub(crate) session: CapiSession,
    /// The phrase-segment result array.
    pub(crate) phrase_result: Vec<PhraseToken>,
    /// Per-instance slots the `zhuyin_get_zhuyin_key` family hands out.
    pub(crate) key_slot: ChewingKey,
    pub(crate) key_rest_slot: ChewingKeyRest,
    /// Snapshotted candidates, rebuilt by `zhuyin_guess_candidates_*`.
    pub(crate) candidates: Vec<CapiCandidate>,
    /// The re-anchored candidate window from a guess at a non-composition
    /// offset, as `(anchor, window)`.
    pub(crate) anchored_window: Option<(usize, CandidateList)>,
    /// Bytes of raw input consumed by the most recent parse call.
    pub(crate) parsed_len: usize,
    /// Clone of the context's user store.
    pub(crate) user: Option<UserStore>,
    /// Shared dictionary for prediction.
    pub(crate) dict: SharedDict,
    /// Clone of the context's language model.
    pub(crate) lm: SharedLm,
    /// Shared live `PINYIN_INCOMPLETE` flag.
    pub(crate) incomplete: Arc<AtomicBool>,
    /// Shared live double-pinyin scheme.
    pub(crate) double_scheme: Arc<AtomicI32>,
    /// Shared live Zhuyin scheme.
    pub(crate) zhuyin_scheme: Arc<AtomicI32>,
    /// Shared live full-pinyin scheme.
    pub(crate) full_scheme: Arc<AtomicI32>,
    /// Shared live `USE_TONE` flag.
    pub(crate) use_tone: Arc<AtomicBool>,
    /// Shared live `FORCE_TONE` flag.
    pub(crate) force_tone: Arc<AtomicBool>,
    /// Shared live option word.
    pub(crate) options: Arc<AtomicU32>,
    /// Most recent Zhuyin parse, when the last parse call was the chewing
    /// entry point.
    pub(crate) zhuyin_parse: Option<ZhuyinParse>,
    /// Original Zhuyin input.
    pub(crate) zhuyin_input: String,
    /// Most recent full-pinyin index parse, for LUOMA / SECONDARY_ZHUYIN.
    pub(crate) full_parse: Option<oxpinyin_core::FullPinyinIndexParse>,
    /// Original full-pinyin input.
    pub(crate) full_input: String,
}

impl CapiInstance {
    /// The parse-path reset: the composition's parse state goes, the
    /// selection record and the constraint store stay.
    pub(crate) fn reset_parse_state(&mut self) {
        self.session.reset_composition();
        self.candidates.clear();
        // Drop a stale re-anchored window so a later candidate selection
        // cannot re-read it against a different composition.
        self.anchored_window = None;
        self.parsed_len = 0;
        self.zhuyin_parse = None;
        self.zhuyin_input.clear();
        self.full_parse = None;
        self.full_input.clear();
    }

    /// Continue a parse when the buffer evolved from the stored one; a
    /// divergent buffer starts fresh.
    pub(crate) fn begin_parse(&mut self, original: &[u8]) {
        let stored: &[u8] = if self.zhuyin_parse.is_some() {
            self.zhuyin_input.as_bytes()
        } else if self.full_parse.is_some() {
            self.full_input.as_bytes()
        } else {
            self.session.raw_input().as_bytes()
        };
        let continues = self.session.parse_continues(stored, original);
        let committed_continues =
            !continues && self.session.committed_parse_continues(stored, original);
        self.reset_parse_state();
        if !continues && !committed_continues {
            self.session.reset();
        }
    }

    /// The current live option word.
    pub(crate) fn options(&self) -> OptionBits {
        OptionBits::from_bits(self.options.load(Ordering::Relaxed))
    }

    /// The generalized lookup-offset law in the active parse mode's own
    /// coordinates. Zhuyin keyboards hold no zero-key columns, so only the
    /// range refusal against the consumed length applies.
    pub(crate) fn validate_lookup_offset(&self, offset: usize) -> Result<usize, EngineError> {
        if let Some(parse) = self.zhuyin_parse.as_ref() {
            check_lookup_offset_range(parse.consumed(), offset)
        } else if let Some(parse) = self.full_parse.as_ref() {
            let consumed = parse.consumed().min(self.full_input.len());
            normalize_lookup_offset(&self.full_input.as_bytes()[..consumed], offset)
        } else {
            self.session.normalized_lookup_offset(offset)
        }
    }

    /// Whether the live option word carries `USE_TONE`.
    pub(crate) fn use_tone_enabled(&self) -> bool {
        self.options().contains(USE_TONE)
    }
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
