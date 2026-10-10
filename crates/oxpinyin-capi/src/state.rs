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

use std::ffi::CString;
use std::path::Path;

use oxpinyin_core::PhraseToken;
use oxpinyin_engine::CandidateKind;

/// Upstream's phrase-index library count (`novel_types.h:43`, `1<<4`).
///
/// The pin asserts an index below this in the addon load/unload path; the
/// compatibility policy's availability class turns that abort into a
/// `false`.
pub(crate) const PHRASE_INDEX_LIBRARY_COUNT: u8 = 16;
pub use oxpinyin_facade::ExportedBigramRow;
pub use oxpinyin_facade::InstanceCore;
use oxpinyin_facade::{ContextCore, OpenFailure};
pub use oxpinyin_runtime::{RuntimeDict as SharedDict, RuntimeLm as SharedLm};

use crate::types::{LookupCandidate, PinyinContext, PinyinInstance};

// ── Context ─────────────────────────────────────────────────────────────

/// State behind `pinyin_context_t *`.
///
/// Owns the shared [`Runtime`] (when this context has system tables).
/// Instances receive cheap handle clones from it — `dict()`, `lm()`,
/// `user_store()` — so they never borrow the context and stay alive past
/// `pinyin_fini`.
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
        // from table.conf when present, degrades an unusable user dir to
        // "no learning", and wires addons + punctuation.
        let core = ContextCore::try_open(
            system_dir,
            user_dir,
            oxpinyin_facade::PINYIN_DEFAULT_OPTION_WORD,
            oxpinyin_user::UserConfLaw::Pinyin,
        )?;
        // `table_info.cpp:220` scans λ straight into a `gfloat`; the exact
        // rational the decoder may hold is not that value.
        let predicted_lambda = core
            .runtime
            .as_ref()
            .map_or(oxpinyin_data::PINNED_LAMBDA, |runtime| {
                runtime.table_conf().lambda_f32()
            });
        Ok(Self {
            predicted_lambda,
            core,
        })
    }

    pub(crate) fn alloc_instance(&self, context: *mut PinyinContext) -> Option<CapiInstance> {
        Some(CapiInstance {
            context,
            predicted_lambda: self.predicted_lambda,
            prefixes: Vec::new(),
            candidates: Vec::new(),
            core: self.core.alloc_instance()?,
        })
    }

    /// `pinyin_unload_phrase_library`'s read side: GBK-only, `true` on
    /// every unload of GBK; `false` without a runtime (a user-store-only context
    /// never loaded GBK — upstream's sub-index is NULL there too).
    pub(crate) fn unload_phrase_library(&self, index: u8) -> bool {
        self.core.unload_phrase_library(index)
    }

    /// Clone of the context's user store, if this context has one.
    ///
    /// The import iterator owns this clone; because the store's §4 dirty flag
    /// is shared by every clone, `pinyin_end_add_phrases` can arm
    /// `m_modified` through it without retaining a context pointer.
    pub(crate) fn user_store(&self) -> Option<oxpinyin_user::UserStore> {
        self.core.user_store()
    }

    /// `pinyin_save`'s body (§4): `false` without a user dir (upstream
    /// `pinyin.cpp:1133`), otherwise the store's gated save — `false` when
    /// unmodified (`:1136`), `true` after a dirty save. A chunk header
    /// write the pin dies on comes back as [`SaveOutcome::ChunkWriteFailed`]
    /// so the caller can log it.
    pub(crate) fn save_user(&mut self) -> oxpinyin_facade::SaveOutcome {
        self.core.save_user()
    }

    /// `pinyin_mask_out`'s body: the store-level deletion, or `false`
    /// without a user store.
    pub(crate) fn mask_out(&mut self, mask: u32, value: u32) -> bool {
        self.core.mask_out(mask, value)
    }

    /// Load addon library `index` from the runtime's first system data dir.
    ///
    /// The pin's addon phrase index asserts `index < PHRASE_INDEX_LIBRARY_COUNT`
    /// (`novel_types.h:43`, 1<<4) on the load path as it does on unload; per
    /// the availability class of `docs/findings/compatibility-policy.md` this
    /// answers `false` instead — the same bound [`CapiContext::unload_addon`]
    /// applies. Without it an out-of-range index would silently load a
    /// stray `addon_{index}_*` table on disk (whichever backend's
    /// extension `default_store_file` names) rather than being refused. A
    /// user-store-only context has no runtime, so it loads nothing.
    pub(crate) fn load_addon(&self, index: u8) -> bool {
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            return false;
        }
        // Class (c): `pinyin.cpp:491` asserts a loaded addon row is a
        // `DICTIONARY`; a row naming no chunk asserts inside `MemoryChunk`
        // (`memory_chunk.h:493`).
        match self.core.load_addon_phrase_library(index) {
            Ok(loaded) => loaded,
            Err(assert) => {
                crate::ffi::log_warning(match assert {
                    oxpinyin_facade::LibraryRowAssert::NullChunkName => {
                        oxpinyin_data::table_info::TableConfSite::NullSystemFile.warning()
                    }
                    _ => {
                        "pinyin_load_addon_phrase_library: assertion \
                         'DICTIONARY == table_info->m_file_type' failed"
                    }
                });
                false
            }
        }
    }

    /// Unload addon library `index`.
    ///
    /// The pin asserts `index < PHRASE_INDEX_LIBRARY_COUNT`
    /// (`novel_types.h:43`, 1<<4) and aborts otherwise; per the
    /// compatibility policy's availability class this answers `false`
    /// instead. In range, it mirrors the pin's unconditional `true`.
    pub(crate) fn unload_addon(&self, index: u8) -> bool {
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            // Class (c), `pinyin.cpp:499`: **`assert`**.
            crate::ffi::log_warning(
                "pinyin_unload_addon_phrase_library: assertion \
                 'index < PHRASE_INDEX_LIBRARY_COUNT' failed",
            );
            return false;
        }
        self.core
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.unload_system_addon(index))
    }

    /// §9 bigram-export renderability, shared: [`ContextCore::
    /// can_render_export_bigrams`].
    pub(crate) fn can_render_export_bigrams(&self) -> bool {
        self.core.can_render_export_bigrams()
    }

    /// The pin's bigram export iterator over this context, shared:
    /// [`ContextCore::bigram_export_walk`].
    pub(crate) fn bigram_export_walk(&self) -> Option<oxpinyin_facade::BigramExportWalk> {
        self.core.bigram_export_walk()
    }

    /// §9 bigram-export rows, shared: [`ContextCore::export_bigram_rows`].
    #[cfg(test)]
    pub(crate) fn export_bigram_rows(&self) -> Option<Vec<ExportedBigramRow>> {
        self.core.export_bigram_rows()
    }
}

// ── Instance ────────────────────────────────────────────────────────────

/// One snapshotted candidate, stored inside `CapiInstance` so that
/// `lookup_candidate_t *` can borrow into it across C calls.
pub struct CapiCandidate {
    pub(crate) text: CString,
    pub(crate) kind: CandidateKind,
    pub(crate) candidate_type: crate::types::lookup_candidate_type_t,
    pub(crate) nbest_index: u8,
    /// Bytes of raw input this candidate consumed, snapshotted at guess time.
    /// No reader today: `pinyin_choose_candidate` answers the parse end
    /// (register #9) and the anchored-window law resolves spans from the
    /// engine. Kept beside the fields the zhuyin facade snapshots identically.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "read by the in-crate tests only since the row-choose cursor moved to the parse end"
        )
    )]
    pub(crate) consumed_bytes: usize,
    /// The candidate's scoring token, snapshotted so the training entry
    /// points (`pinyin_train`'s observation, predicted-candidate training,
    /// `pinyin_is_user_candidate`) can resolve it without re-decoding.
    /// `None` for sentence-level and fallback candidates, which carry no
    /// token and are not trained (§2: only pinned phrases train).
    pub(crate) token: Option<PhraseToken>,
    /// The index this candidate held in the window it was snapshotted from.
    /// The snapshot (`Vec<CapiCandidate>`) may omit entries (the engine's
    /// `Fallback` row, a `CString` conversion failure), so a candidate's position in the snapshot is NOT its
    /// position in the window; `pinyin_choose_candidate` must select by
    /// THIS index, which is the one `Session::select[_anchored]` indexes.
    pub(crate) source_index: usize,
}

/// State behind `pinyin_instance_t *`.
pub struct CapiInstance {
    /// The owning context's C handle, returned by `pinyin_get_context`
    /// (upstream `pinyin_get_context`, `pinyin.cpp:1358-1360`). A raw
    /// pointer like every C handle here: no ownership, and using it
    /// after `pinyin_fini` is the caller's UAF, exactly upstream's.
    pub(crate) context: *mut PinyinContext,
    /// The orchestration half — session, shared handles, live option
    /// word, parse-mode state machine, re-anchored window — shared with
    /// the zhuyin facade.
    pub(crate) core: InstanceCore,
    /// The context's initial gfloat interpolation weight for prediction.
    pub(crate) predicted_lambda: f32,
    /// The pin's `instance->m_prefixes` minus its `sentence_start` entry:
    /// the tokens `pinyin_guess_predicted_candidates` and
    /// `pinyin_guess_sentence_with_prefix` computed from their prefix text,
    /// empty after `pinyin_guess_sentence` and `pinyin_reset`. The
    /// predecessor of `pinyin_choose_predicted_candidate`'s bigram is the
    /// longest of them (`_get_previous_token`, `pinyin.cpp:1711-1740`).
    pub(crate) prefixes: Vec<u32>,
    /// Snapshotted candidates, rebuilt by `pinyin_guess_candidates`.
    /// `lookup_candidate_t *` pointers borrow into this vec.
    pub(crate) candidates: Vec<CapiCandidate>,
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
oxpinyin_capi_marshal::opaque_handle_casts! {
    vis: pub,
    context: PinyinContext => CapiContext,
    instance: PinyinInstance => CapiInstance,
    candidate: LookupCandidate => CapiCandidate,
}
