//! Real backing state behind the opaque C handles.
//!
//! `CapiContext` lives behind `pinyin_context_t *` and `CapiInstance`
//! behind `pinyin_instance_t *`. The opaque `#[repr(C)]` types in
//! [`crate::types`] exist only for the generated C header.
//!
//! The orchestration half of both structs — the runtime assembly, the
//! user store, the live option/scheme word, the parse-mode state machine,
//! the re-anchored window — lives in [`oxpinyin_facade`]'s
//! `ContextCore`/`InstanceCore`, shared with the zhuyin facade; this file
//! keeps only the C-facing shell: the context back-pointer, the ABI key
//! slots, the `CString` candidate snapshot, and this facade's §9
//! user-data export machinery.

use std::path::Path;
use std::{
    cell::{Cell, UnsafeCell},
    rc::Rc,
};

use oxpinyin_core::{OptionBits, PhraseToken};

/// Upstream's phrase-index library count (`novel_types.h:43`, `1<<4`).
///
/// The pin asserts an index below this in the addon load/unload path; the
/// compatibility policy's availability class turns that abort into a
/// `false`.
pub(crate) const PHRASE_INDEX_LIBRARY_COUNT: u8 = 16;
pub use oxpinyin_facade::ExportedBigramRow;
pub use oxpinyin_facade::InstanceCore;
use oxpinyin_facade::{CandidateSnapshot, SnapshotRow, TextSpan};
use oxpinyin_facade::{ContextCore, OpenFailure};
pub use oxpinyin_runtime::{RuntimeDict as SharedDict, RuntimeLm as SharedLm};

use crate::types::{LookupCandidate, PinyinContext, PinyinInstance};

// ── Context ─────────────────────────────────────────────────────────────

/// State behind `pinyin_context_t *`.
///
/// Owns the runtime and its storage. Context, instances and iterators share
/// this allocation; every operation borrows it under the C serialization contract.
pub struct CapiContext {
    /// The shared orchestration half: assembly, user store, layered
    /// configuration, and the live option/scheme word.
    pub(crate) core: ContextCore,
    /// Prediction reads the same table.conf lambda at context construction
    /// (`074a2219 table_info.cpp:220`), retained independently of the decoder.
    predicted_lambda: f32,
}

impl CapiContext {
    /// Opens a context the way `pinyin_init` does: the system data
    /// directory (a libpinyin install's own on Kyoto Cabinet, tkrzw and
    /// Berkeley DB) plus the optional user dir, seeded with
    /// `PINYIN_INCOMPLETE` (the pinyin facade's option word).
    /// Opens a context; the failure is kept for `pinyin_init`'s log line.
    pub(crate) fn try_new(system_dir: &Path, user_dir: Option<&Path>) -> Result<Self, OpenFailure> {
        // W8 fork-bootstrap wiring lives in the shared assembly: the
        // constructor opens the DBM handles and chunk mappings, installs λ
        // from table.conf when present, opens the user dir's session (an
        // unreadable profile loads per file, as the pin's does), and wires
        // addons + punctuation.
        let core = ContextCore::try_open(
            system_dir,
            user_dir,
            oxpinyin_facade::PINYIN_DEFAULT_OPTION_WORD,
            oxpinyin_user::UserConfLaw::Pinyin,
        )?;
        // Use the `gfloat` scanned by `table_info.cpp:220` directly.
        let predicted_lambda = core.runtime.table_conf().lambda_f32();
        Ok(Self {
            predicted_lambda,
            core,
        })
    }

    pub(crate) fn alloc_instance(
        &self,
        context: *mut PinyinContext,
        owner: ContextOwner,
    ) -> CapiInstance {
        CapiInstance {
            context,
            owner,
            core: self.core.alloc_instance(),
            state: CapiInstanceState {
                predicted_lambda: self.predicted_lambda,
                full_aux_options: OptionBits::default(),
                candidates: CandidateSnapshot::new(),
            },
        }
    }

    /// Load addon library `index` from the runtime's first system data dir.
    ///
    /// The pin answers `false` for `index >= PHRASE_INDEX_LIBRARY_COUNT`
    /// (`novel_types.h:43`, 1<<4) on the load path (`pinyin.cpp:479-480`),
    /// and so does this. Without it an out-of-range index would silently load a
    /// stray `addon_{index}_*` table on disk (whichever backend's
    /// extension `default_store_file` names) rather than being refused.
    pub(crate) fn load_addon(&mut self, index: u8) -> Result<bool, &'static str> {
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            return Ok(false);
        }
        match self.core.load_addon_phrase_library(index) {
            Ok(loaded) => Ok(loaded),
            // Register row 79, `pinyin.cpp:491`: the pin asserts a loaded
            // addon row is a `DICTIONARY`, and the process aborts.
            Err(oxpinyin_facade::LibraryRowAssert::NotDictionary) => std::process::abort(),
            // Register row 79, `memory_chunk.h:493`: a row naming no chunk
            // maps the system directory, and where that directory reads as
            // a chunk the pin asserts on its length word. Where it does not,
            // the facade has written the pin's `mmap %s failed!` line and
            // the pin dies of SIGSEGV on the empty chunk (class (b)), so
            // the call fails with one warning.
            Err(oxpinyin_facade::LibraryRowAssert::NullChunkName)
                if self
                    .core
                    .runtime
                    .system_dir()
                    .is_none_or(oxpinyin_facade::null_chunk_asserts) =>
            {
                std::process::abort()
            }
            Err(oxpinyin_facade::LibraryRowAssert::NullChunkName) => {
                Err(oxpinyin_data::table_info::TableConfSite::NullSystemFile.warning())
            }
            // Register row 96: `pinyin.cpp:290` maps the addon through the same
            // `MemoryChunk::mmap`, and a directory of header size or more fails
            // the read at `memory_chunk.h:493`.
            Err(oxpinyin_facade::LibraryRowAssert::ChunkHeaderRead) => std::process::abort(),
            // The addon load answers no `NotLoadable`.
            Err(oxpinyin_facade::LibraryRowAssert::NotLoadable) => Ok(false),
        }
    }

    /// Unload addon library `index`.
    ///
    /// The pin asserts `index < PHRASE_INDEX_LIBRARY_COUNT`
    /// (`novel_types.h:43`, 1<<4), and the process aborts here too
    /// (register row 21, `pinyin.cpp:499`). In range, it mirrors the pin's
    /// unconditional `true`.
    pub(crate) fn unload_addon(&mut self, index: u8) -> bool {
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            std::process::abort();
        }
        self.core.runtime.unload_system_addon(index)
    }

    /// The pin's bigram export iterator over this context, shared:
    /// [`ContextCore::bigram_export_walk`].
    pub(crate) fn bigram_export_walk(&self) -> Option<oxpinyin_facade::BigramExportWalk> {
        self.core.bigram_export_walk()
    }

    /// Whether `user_bigram.db` carried a key that is not a four-byte
    /// `phrase_token_t`, shared: [`ContextCore::has_non_token_bigram_key`].
    /// The pin's `get_all_items` assert dies on it (`ngram_bdb.cpp:199`),
    /// reached by `pinyin_begin_get_bigram_phrases`.
    pub(crate) fn has_non_token_bigram_key(&self) -> bool {
        self.core.has_non_token_bigram_key()
    }
}

// ── Instance ────────────────────────────────────────────────────────────

/// One snapshotted candidate, stored inside `CapiInstance` so that
/// `lookup_candidate_t *` can borrow into it across C calls.
///
/// The row is sized to need (40 bytes): its text lives in the instance's
/// [`CandidateSnapshot`] buffer and the row keeps the span, the type is
/// one byte ([`CapiCandidate::candidate_type`]), and both ends of the
/// selection span are `u32` because a graph position bounds the raw
/// buffer.
pub struct CapiCandidate {
    /// The row's string in the snapshot's packed text buffer.
    pub(crate) text: TextSpan,
    /// The pointer `pinyin_get_candidate_string` answers: the row's string
    /// inside the snapshot's buffer, kept current by the snapshot.
    pub(crate) text_ptr: *const std::ffi::c_char,
    /// [`Self::candidate_type`]'s tag.
    pub(crate) type_tag: Cell<u8>,
    pub(crate) nbest_index: u8,
    /// The candidate's scoring token, snapshotted so the training entry
    /// points (`pinyin_train`'s observation, predicted-candidate training,
    /// `pinyin_is_user_candidate`) can resolve it without re-decoding.
    /// `None` for sentence-level candidates and predicted punctuation rows,
    /// which carry no token (the pin's `null_token`) and are not trained
    /// (§2: only pinned phrases train).
    pub(crate) token: Cell<Option<PhraseToken>>,
    /// The row's own span, the pin's `m_begin` and `m_end` in session
    /// coordinates, which a choose selects (`pinyin.cpp:2577-2590` at
    /// 074a2219). Zero for a predicted row, as the pin's default
    /// `lookup_candidate_t` (`:114`).
    pub(crate) selection: oxpinyin_facade::SelectionSpan,
    /// The pin's `m_end - m_begin` in the caller's coordinates, fixed at the
    /// guess: `m_begin` is the guess's own offset and `m_end` the row's span
    /// end there (`pinyin.cpp:2228-2252`). An accepted choose answers the
    /// choose's offset plus this length (`:2582-2590`). Zero for a
    /// predicted row.
    pub(crate) span_len: u32,
}

// The row width the doc above claims; a field that grows it shows up here.
#[cfg(target_pointer_width = "64")]
const _: () = {
    assert!(core::mem::size_of::<CapiCandidate>() == 40);
    assert!(core::mem::align_of::<CapiCandidate>() == 8);
};

impl CapiCandidate {
    /// The row's `lookup_candidate_type_t`.
    pub(crate) fn candidate_type(&self) -> crate::types::lookup_candidate_type_t {
        crate::types::lookup_candidate_type_t::from_tag(self.type_tag.get())
    }

    /// Sets the row's `lookup_candidate_type_t`.
    pub(crate) fn set_candidate_type(&self, candidate_type: crate::types::lookup_candidate_type_t) {
        self.type_tag.set(candidate_type.tag());
    }
}

#[cfg(test)]
impl CapiCandidate {
    /// The row's string bytes, for the in-crate tests.
    pub(crate) fn text_bytes(&self) -> &[u8] {
        self.text_cstr().to_bytes()
    }

    /// The row's string, for the in-crate tests.
    pub(crate) fn text_str(&self) -> Result<&str, std::str::Utf8Error> {
        self.text_cstr().to_str()
    }

    fn text_cstr(&self) -> &std::ffi::CStr {
        if self.text_ptr.is_null() {
            return c"";
        }
        // SAFETY: a pushed row's pointer addresses its NUL-terminated string
        // inside the owning snapshot's buffer, which outlives the row and
        // is re-pointed whenever that buffer moves.
        unsafe { std::ffi::CStr::from_ptr(self.text_ptr) }
    }
}

impl SnapshotRow for CapiCandidate {
    fn text_span(&self) -> TextSpan {
        self.text
    }

    fn set_text(&mut self, span: TextSpan, ptr: *const std::ffi::c_char) {
        self.text = span;
        self.text_ptr = ptr;
    }
}

/// State behind `pinyin_instance_t *`.
pub struct CapiInstance<C = InstanceCore, S = CapiInstanceState, O = ContextOwner> {
    /// Retains the sole runtime allocation after the public context is finalised.
    pub(crate) owner: O,
    /// The owning context's C handle, returned by `pinyin_get_context`
    /// (upstream `pinyin_get_context`, `pinyin.cpp:1358-1360`). A raw
    /// pointer like every C handle here: no ownership, and using it
    /// after `pinyin_fini` is the caller's UAF, exactly upstream's.
    pub(crate) context: *mut PinyinContext,
    /// The orchestration half — composition, live option
    /// word, parse-mode state machine, re-anchored window — shared with
    /// the zhuyin facade.
    pub(crate) core: C,
    pub(crate) state: S,
}

/// ABI presentation state retained independently of phrase storage.
pub struct CapiInstanceState {
    /// The context's initial gfloat interpolation weight for prediction.
    pub(crate) predicted_lambda: f32,
    /// Options used to parse the keys that plain full-pinyin aux text displays.
    pub(crate) full_aux_options: OptionBits,
    /// Snapshotted candidates, rebuilt by `pinyin_guess_candidates`.
    /// `lookup_candidate_t *` pointers borrow into its rows, and the
    /// strings they read into its text buffer.
    pub(crate) candidates: CandidateSnapshot<CapiCandidate>,
}

// ── Pointer casts ───────────────────────────────────────────────────────
//
// The opaque `PinyinContext` / `PinyinInstance` / `LookupCandidate` types in
// the C header are zero-sized sentinels; the pointers actually address a
// heap-allocated `CapiContext` / `CapiInstance` / `CapiCandidate`. The eight
// cast/box helpers (context_ref/mut, instance_ref/mut, box_context/instance,
// candidate_ref/ptr) are stamped by the shared marshalling macro so this
// facade and the zhuyin one cannot drift; the generated `unsafe` blocks land
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
            context: self.context,
            owner: self.owner.borrow(),
            core: self.core.read(),
            state: self.state.borrow(),
        }
    }
}

/// Shared ownership of one context allocation, including its sole runtime.
/// Handles retain ownership; every access still borrows only for one C call.
pub type ContextOwner = Rc<UnsafeCell<CapiContext>>;

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
pub(crate) unsafe fn context_owner(ptr: *mut PinyinContext) -> ContextOwner {
    // SAFETY: the public handle is live for this call; clone only its owner.
    Rc::clone(unsafe { handles::context_ref(ptr) })
}

/// Box the context's reference-counted owner as the public C handle.
pub(crate) fn box_context(context: CapiContext) -> *mut PinyinContext {
    handles::box_context(Rc::new(UnsafeCell::new(context)))
}

/// Borrow a live public context for one operation.
/// # Safety
/// `ptr` is live and the caller excludes mutation during the borrow.
pub(crate) unsafe fn context_ref<'a>(ptr: *mut PinyinContext) -> &'a CapiContext {
    // SAFETY: the caller keeps the public handle live and serializes its calls.
    unsafe { owner_ref(handles::context_ref(ptr)) }
}

/// Borrow a live public context exclusively for one operation.
/// # Safety
/// `ptr` is live; no call or borrow through any handle overlaps this one.
pub(crate) unsafe fn context_mut<'a>(ptr: *mut PinyinContext) -> &'a mut CapiContext {
    // SAFETY: the caller keeps the public handle live and grants exclusive access.
    unsafe { owner_mut(handles::context_mut(ptr)) }
}

/// Borrow the runtime retained by an instance, without its public context.
/// # Safety
/// `ptr` is live; all earlier readers have ended and the C caller grants
/// exclusive access across every handle sharing this runtime for this call.
pub(crate) unsafe fn instance_context_mut<'a>(ptr: *mut PinyinInstance) -> &'a mut CapiContext {
    // SAFETY: the instance owns the allocation; serialized C calls exclude
    // access through any other instance, iterator or the public context.
    unsafe { owner_mut(&mut handles::instance_mut(ptr).owner) }
}

mod handles {
    use super::*;
    oxpinyin_capi_marshal::opaque_handle_casts! {
        vis: pub,
        context: PinyinContext => ContextOwner,
        instance: PinyinInstance => CapiInstance,
        candidate: LookupCandidate => CapiCandidate,
    }
}
pub(crate) use handles::{box_instance, candidate_ptr, candidate_ref};

/// Borrow this call's instance and its live context.
/// # Safety
/// The instance must be live and all calls sharing its runtime serialized.
/// No instance or context mutation may overlap the returned reader.
pub unsafe fn instance_ref<'a>(ptr: *mut PinyinInstance) -> CapiInstanceRead<'a> {
    // SAFETY: the caller supplies a live instance and serializes all shared-handle calls.
    let instance = unsafe { handles::instance_ref(ptr) };
    // SAFETY: the instance retains the allocation; the C caller excludes concurrent mutation.
    let context = unsafe { owner_ref(&instance.owner) };
    let runtime = &context.core.runtime;
    CapiInstance {
        context: instance.context,
        owner: &instance.owner,
        core: instance.core.read(runtime),
        state: &instance.state,
    }
}

/// Borrow composition exclusively and phrase storage for this call only.
/// # Safety
/// The instance must be live, with no overlapping access through any shared handle.
pub unsafe fn instance_mut<'a>(ptr: *mut PinyinInstance) -> CapiInstanceMut<'a> {
    // SAFETY: the caller grants exclusive access to this live instance.
    let instance = unsafe { handles::instance_mut(ptr) };
    // SAFETY: the instance retains the allocation; the C caller excludes runtime mutation.
    let context = unsafe { owner_ref(&instance.owner) };
    let runtime = &context.core.runtime;
    CapiInstance {
        context: instance.context,
        owner: &instance.owner,
        core: instance.core.bind(runtime),
        state: &mut instance.state,
    }
}

/// Train with exclusive access to the context, after all read views have ended.
/// # Safety
/// The caller supplies a live instance and exclusive access across all shared handles.
pub(crate) unsafe fn train_instance(
    ptr: *mut PinyinInstance,
    index: u8,
) -> Result<bool, oxpinyin_engine::EngineError> {
    // SAFETY: the caller grants this operation exclusive access to the live instance.
    let instance = unsafe { handles::instance_mut(ptr) };
    // SAFETY: the owner retains the allocation; serialized C calls exclude other readers.
    let context = unsafe { owner_mut(&mut instance.owner) };
    context.core.train(&instance.core, index)
}

pub(crate) use handles::instance_ref as instance_state_ref;
