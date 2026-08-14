//! Real backing state behind the opaque C handles.
//!
//! `CapiContext` lives behind `pinyin_context_t *` and `CapiInstance`
//! behind `pinyin_instance_t *`. The opaque `#[repr(C)]` types in
//! [`crate::types`] exist only for the generated C header.
//!
//! The cast helpers are consumed incrementally across T2–T4; unused ones
//! are intentional (the full set exists so each task adds calls, not casts).
#![allow(dead_code)]

use std::convert::Infallible;

use pinyin_core::{Cost, Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};
use pinyin_engine::{EmptyConfigSource, Session, StoragePaths};

use crate::types::{PinyinContext, PinyinInstance};

// ── Stub backends (T2) ─────────────────────────────────────────────────
//
// Zero-sized types that satisfy the Session's trait bounds. Real backends
// (pinyin-data tables) arrive at T4; until then every lookup returns an
// empty result and every score passes through.

#[derive(Clone, Copy)]
pub(crate) struct StubDict;

impl Dictionary for StubDict {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = Infallible;

    fn lookup(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, Infallible> {
        Ok(Vec::new())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct StubLm;

impl LanguageModel for StubLm {
    type Token = PhraseToken;
    type Error = Infallible;

    fn score(
        &self,
        _history: &[PhraseToken],
        _token: &PhraseToken,
        edge_cost: Cost,
    ) -> Result<Cost, Infallible> {
        Ok(edge_cost)
    }
}

// ── Context ─────────────────────────────────────────────────────────────

pub(crate) type CapiSession = Session<StubDict, StubLm>;

/// State behind `pinyin_context_t *`.
pub(crate) struct CapiContext {
    pub(crate) paths: StoragePaths,
}

impl CapiContext {
    pub(crate) fn new(system_dir: &str, user_dir: &str) -> Self {
        let paths = if system_dir.is_empty() {
            StoragePaths::new(user_dir)
        } else {
            StoragePaths::new(user_dir).with_system_dirs([system_dir])
        };
        Self { paths }
    }

    pub(crate) fn alloc_instance(&self) -> Option<CapiInstance> {
        let session =
            Session::new(&EmptyConfigSource, self.paths.clone(), StubDict, StubLm).ok()?;
        Some(CapiInstance { session })
    }
}

// ── Instance ────────────────────────────────────────────────────────────

/// State behind `pinyin_instance_t *`.
pub(crate) struct CapiInstance {
    pub(crate) session: CapiSession,
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
