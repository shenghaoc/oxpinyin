//! Real backing state behind the opaque C handles.
//!
//! `CapiContext` lives behind `pinyin_context_t *` and `CapiInstance`
//! behind `pinyin_instance_t *`. The opaque `#[repr(C)]` types in
//! [`crate::types`] exist only for the generated C header.
#![allow(dead_code)]

use std::ffi::CString;
use std::path::Path;
use std::sync::Arc;

use pinyin_core::{Cost, Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};
use pinyin_data::{BigramLanguageModel, DictError, LmError, SystemDictionary};
use pinyin_engine::{CandidateKind, Config, Session, StoragePaths};

use crate::types::{LookupCandidate, PinyinContext, PinyinInstance};

// ── Shared backends ─────────────────────────────────────────────────────
//
// Context and instance both hold `Arc` clones. Instances must not borrow
// the context as `'static`: `pinyin_fini` drops the context Box while
// instances may still be alive, and a `'static` reference would then be a
// use-after-free.

/// `Arc` wrapper so instances share the context's dictionary without a
/// `'static` borrow.
#[derive(Clone)]
pub(crate) struct SharedDict(Arc<SystemDictionary>);

impl Dictionary for SharedDict {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        self.0.lookup(syllables)
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        self.0.phrase_prefix_exists(syllables)
    }
}

/// `Arc` wrapper so instances share the context's language model without
/// a `'static` borrow.
#[derive(Clone)]
pub(crate) struct SharedLm(Arc<BigramLanguageModel>);

impl LanguageModel for SharedLm {
    type Token = PhraseToken;
    type Error = LmError;

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        self.0.score(history, token, edge_cost)
    }

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        self.0.unigram_freq(token)
    }

    fn has_real_unigrams(&self) -> bool {
        self.0.has_real_unigrams()
    }
}

// ── Context ─────────────────────────────────────────────────────────────

pub(crate) type CapiSession = Session<SharedDict, SharedLm>;

/// State behind `pinyin_context_t *`.
///
/// Owns the dictionary and language model. Instances receive `Arc` clones
/// so they do not borrow the context.
pub(crate) struct CapiContext {
    pub(crate) paths: StoragePaths,
    pub(crate) config: Config,
    dict: SharedDict,
    lm: SharedLm,
}

impl CapiContext {
    pub(crate) fn new(system_dir: &str, user_dir: &str) -> Option<Self> {
        if system_dir.is_empty() {
            return None;
        }
        let paths = StoragePaths::new(user_dir).with_system_dirs([system_dir]);

        let sys = Path::new(system_dir);
        let dict = SystemDictionary::open(
            &sys.join("pinyin_index.redb"),
            &sys.join("phrase_index.redb"),
        )
        .ok()?;
        let mut lm = BigramLanguageModel::open(&sys.join("bigram.redb")).ok()?;
        // Read λ from the install's table.conf when present (data-formats.md
        // §3); a real install ships one. Absent (no table.conf in the dir),
        // the pinned 0.312699 default stands.
        lm.set_lambda_from_table_conf(&sys.join("table.conf"));
        lm.set_unigrams_from_dict(&dict);

        Some(Self {
            paths,
            config: Config::default(),
            dict: SharedDict(Arc::new(dict)),
            lm: SharedLm(Arc::new(lm)),
        })
    }

    pub(crate) fn alloc_instance(&self) -> Option<CapiInstance> {
        let session = Session::new(
            &self.config,
            self.paths.clone(),
            self.dict.clone(),
            self.lm.clone(),
        )
        .ok()?;
        Some(CapiInstance {
            session,
            candidates: Vec::new(),
        })
    }
}

// ── Instance ────────────────────────────────────────────────────────────

/// One snapshotted candidate, stored inside `CapiInstance` so that
/// `lookup_candidate_t *` can borrow into it across C calls.
pub(crate) struct CapiCandidate {
    pub(crate) text: CString,
    pub(crate) kind: CandidateKind,
    pub(crate) nbest_index: u8,
    /// Bytes of raw input this candidate consumed, snapshotted at guess time
    /// so `pinyin_choose_candidate` can report the new cursor position.
    pub(crate) consumed_bytes: usize,
}

/// State behind `pinyin_instance_t *`.
pub(crate) struct CapiInstance {
    pub(crate) session: CapiSession,
    /// Snapshotted candidates, rebuilt by `pinyin_guess_candidates`.
    /// `lookup_candidate_t *` pointers borrow into this vec.
    pub(crate) candidates: Vec<CapiCandidate>,
}

// ── Pointer casts ───────────────────────────────────────────────────────
//
// The opaque `PinyinContext` / `PinyinInstance` types in the C header are
// zero-sized sentinels. What the pointer actually addresses is a heap-
// allocated `CapiContext` / `CapiInstance`. These helpers centralise the
// cast so each call site stays readable.

/// Casts a `*mut PinyinContext` to `&CapiContext`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(CapiContext { .. }))`.
/// The returned reference must not outlive the `Box` (i.e. must not be used
/// after `pinyin_fini` reconstructs and drops it), and must not be stored in
/// a `CapiInstance` or any other longer-lived location.
pub(crate) unsafe fn context_ref<'a>(ptr: *mut PinyinContext) -> &'a CapiContext {
    // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
    unsafe { &*(ptr.cast::<CapiContext>()) }
}

/// Casts a `*mut PinyinContext` to `&mut CapiContext`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(CapiContext { .. }))`.
/// No other reference to the same context may exist, and the returned
/// reference must not outlive the `Box` (i.e. must not be used after
/// `pinyin_fini` reconstructs and drops it) or be stored in a `CapiInstance`.
pub(crate) unsafe fn context_mut<'a>(ptr: *mut PinyinContext) -> &'a mut CapiContext {
    // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
    unsafe { &mut *(ptr.cast::<CapiContext>()) }
}

/// Casts a `*mut PinyinInstance` to `&CapiInstance`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(CapiInstance { .. }))`.
/// The returned reference must not outlive the `Box` (i.e. must not be used
/// after `pinyin_free_instance` reconstructs and drops it), and must not be
/// stored in any longer-lived location.
pub(crate) unsafe fn instance_ref<'a>(ptr: *mut PinyinInstance) -> &'a CapiInstance {
    // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
    unsafe { &*(ptr.cast::<CapiInstance>()) }
}

/// Casts a `*mut PinyinInstance` to `&mut CapiInstance`.
///
/// # Safety
///
/// `ptr` must be non-null and produced by `Box::into_raw(Box::new(CapiInstance { .. }))`.
/// No other reference to the same instance may exist, and the returned
/// reference must not outlive the `Box` (i.e. must not be used after
/// `pinyin_free_instance` reconstructs and drops it) or be stored elsewhere.
pub(crate) unsafe fn instance_mut<'a>(ptr: *mut PinyinInstance) -> &'a mut CapiInstance {
    // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
    unsafe { &mut *(ptr.cast::<CapiInstance>()) }
}

/// Converts a `CapiContext` into a `*mut PinyinContext` for return to C.
pub(crate) fn box_context(ctx: CapiContext) -> *mut PinyinContext {
    Box::into_raw(Box::new(ctx)).cast()
}

/// Converts a `CapiInstance` into a `*mut PinyinInstance` for return to C.
pub(crate) fn box_instance(inst: CapiInstance) -> *mut PinyinInstance {
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
