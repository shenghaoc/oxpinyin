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
//! slots, the CString candidate snapshot, and this facade's §9
//! user-data export machinery.
#![allow(dead_code)]

use std::ffi::CString;

use oxpinyin_core::PhraseToken;
use oxpinyin_engine::CandidateKind;

/// Upstream's phrase-index library count (`novel_types.h:43`, `1<<4`).
///
/// The pin asserts an index below this in the addon load/unload path; the
/// compatibility policy's availability class turns that abort into a
/// `false`.
const PHRASE_INDEX_LIBRARY_COUNT: u8 = 16;
use oxpinyin_facade::ContextCore;
pub use oxpinyin_facade::InstanceCore;
pub use oxpinyin_runtime::{RuntimeDict as SharedDict, RuntimeLm as SharedLm};
use oxpinyin_user::{
    ExportedPhrase, NETWORK_DICTIONARY, SENTENCE_START, USER_DICTIONARY, is_user_file_token,
};

use crate::types::{ChewingKey, ChewingKeyRest, LookupCandidate, PinyinContext, PinyinInstance};

// ── Context ─────────────────────────────────────────────────────────────

/// The session type every C handle wraps: the shared runtime's concrete
/// session.
pub type CapiSession = oxpinyin_runtime::RuntimeSession;

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
}

impl CapiContext {
    /// Opens a context the way `pinyin_init` does: the system data
    /// directory (a libpinyin install's own on Kyoto Cabinet and tkrzw)
    /// plus the optional user dir, seeded with `PINYIN_INCOMPLETE` (the
    /// pinyin facade's option word).
    pub(crate) fn new(system_dir: &str, user_dir: &str) -> Option<Self> {
        // W8 fork-bootstrap wiring lives in the shared assembly: the
        // constructor opens the DBM handles and chunk mappings, installs λ
        // from table.conf when present, degrades an unusable user dir to
        // "no learning", and wires addons + punctuation.
        Some(Self {
            core: ContextCore::open(
                system_dir,
                user_dir,
                oxpinyin_facade::PINYIN_DEFAULT_OPTION_WORD,
            )?,
        })
    }

    /// User-store-only context for standalone migration tools
    /// (`oxpinyin-dictool import`). The C ABI `pinyin_init` still requires
    /// system tables — its contract is a decoder context — while this
    /// Rust-only constructor lets a tool drive the import/export/save trio
    /// without carrying a system dictionary. `pinyin_alloc_instance` reports
    /// `None` for such a context because there is nothing to decode with.
    pub(crate) fn new_user_only(user_dir: &str) -> Option<Self> {
        Some(Self {
            core: ContextCore::new_user_only(
                user_dir,
                oxpinyin_facade::PINYIN_DEFAULT_OPTION_WORD,
            )?,
        })
    }

    pub(crate) fn alloc_instance(&self, context: *mut PinyinContext) -> Option<CapiInstance> {
        Some(CapiInstance {
            context,
            key_slot: ChewingKey::ZERO,
            key_rest_slot: ChewingKeyRest { begin: 0, end: 0 },
            candidates: Vec::new(),
            core: self.core.alloc_instance()?,
        })
    }

    /// `pinyin_load_phrase_library`'s read side: the runtime's
    /// library-load (mask-clear) rule; `false` without a runtime.
    pub(crate) fn load_phrase_library(&self, index: u32) -> bool {
        self.core.load_phrase_library(index)
    }

    /// `pinyin_unload_phrase_library`'s read side: GBK-only, first-unload
    /// `true`; `false` without a runtime (a user-store-only context
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
    /// unmodified (`:1136`), `true` after a dirty save.
    pub(crate) fn save_user(&mut self) -> bool {
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
        self.core
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.load_system_addon(index))
    }

    /// Unload addon library `index`.
    ///
    /// The pin asserts `index < PHRASE_INDEX_LIBRARY_COUNT`
    /// (`novel_types.h:43`, 1<<4) and aborts otherwise; per the
    /// compatibility policy's availability class this answers `false`
    /// instead. In range, it mirrors the pin's unconditional `true`.
    pub(crate) fn unload_addon(&self, index: u8) -> bool {
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            return false;
        }
        self.core
            .runtime
            .as_ref()
            .is_some_and(|runtime| runtime.unload_system_addon(index))
    }

    /// §9 phrase-export materialization. [`USER_DICTIONARY`] and
    /// [`NETWORK_DICTIONARY`] export their stored rows; any other index
    /// exports an empty list.
    pub(crate) fn export_phrases(&self, index: u32) -> Option<Vec<ExportedPhrase>> {
        let index = u8::try_from(index).ok()?;
        if index != USER_DICTIONARY && index != NETWORK_DICTIONARY {
            return Some(Vec::new());
        }
        self.core.user.as_ref()?.export_phrases_in(index).ok()
    }

    /// §9 bigram-export materialization with upstream's filters and
    /// rendering (`pinyin_begin_get_bigram_phrases` in `pinyin.cpp`):
    /// skip `sentence_start` predecessors and counts at or below the
    /// first-seed threshold (`initial_seed − 1` = 68); phrase = prev text +
    /// next text; pinyin = prev pinyin + `'` + next pinyin (one row per
    /// pronunciation combination); count = stored × 2 (upstream's local
    /// `unigram_factor`).
    /// False when this context cannot render every exportable bigram row
    /// (user-store-only, and at least one stored pair needs the system
    /// phrase index). Callers must fail the snapshot rather than skip those
    /// rows into an incomplete file.
    pub(crate) fn can_render_export_bigrams(&self) -> bool {
        const INITIAL_SEED: u64 = 23 * 3;
        if self.core.runtime.is_some() {
            return true;
        }
        let Some(store) = self.core.user.as_ref() else {
            return true;
        };
        let Ok(raw) = store.export_bigrams() else {
            return false;
        };
        !raw.iter().any(|(prev, cur, count)| {
            *prev != SENTENCE_START
                && *count >= INITIAL_SEED
                && (!is_user_file_token(*prev) || !is_user_file_token(*cur))
        })
    }

    pub(crate) fn export_bigram_rows(&self) -> Option<Vec<ExportedBigramRow>> {
        const INITIAL_SEED: u64 = 23 * 3;
        let store = self.core.user.as_ref()?;
        let raw = store.export_bigrams().ok()?;
        let mut rows = Vec::new();
        // Memoize the (text, pinyins) rendering: a system token recurs across
        // many bigram rows and `render_token` is an O(pinyin-index) scan, so
        // resolving it once per distinct token keeps the export off the
        // rows×index quadratic.
        let mut rendered: std::collections::HashMap<u32, Option<(String, Vec<String>)>> =
            std::collections::HashMap::new();
        for (prev, cur, count) in raw {
            if prev == SENTENCE_START {
                continue;
            }
            // Upstream's threshold is `initial_seed - 1` = 68.
            if count < INITIAL_SEED {
                continue;
            }
            let Some((prev_text, prev_pinyins)) = rendered
                .entry(prev)
                .or_insert_with(|| self.render_token(prev))
                .clone()
            else {
                continue;
            };
            let Some((cur_text, cur_pinyins)) = rendered
                .entry(cur)
                .or_insert_with(|| self.render_token(cur))
                .clone()
            else {
                continue;
            };
            let phrase = format!("{prev_text}{cur_text}");
            for first in &prev_pinyins {
                for second in &cur_pinyins {
                    rows.push(ExportedBigramRow {
                        phrase: phrase.clone(),
                        pinyin: format!("{first}'{second}"),
                        count: i64::try_from(count.saturating_mul(2)).unwrap_or(i64::MAX),
                    });
                }
            }
        }
        Some(rows)
    }

    /// `(text, pinyin spellings)` for a token: user tokens render from the
    /// user store's phrase/pronunciation tables, system tokens from the
    /// system phrase index and the pinyin index (reverse-scanned).
    fn render_token(&self, token: u32) -> Option<(String, Vec<String>)> {
        if is_user_file_token(token) {
            let store = self.core.user.as_ref()?;
            let phrase = store.phrase(token).ok().flatten()?;
            // Render each reading through the shared `render_pinyin` helper,
            // skipping any unrenderable one — the same rule `export_phrases`
            // applies, so the phrase and bigram exports stay consistent.
            let pinyins: Vec<String> = phrase
                .pronunciations()
                .iter()
                .filter_map(oxpinyin_user::UserPronunciation::render_pinyin)
                .collect();
            if pinyins.is_empty() {
                return None;
            }
            Some((phrase.text().to_owned(), pinyins))
        } else {
            let dict = self.core.runtime.as_ref()?.dict();
            let text = dict.system().phrase_text(token)?;
            let pinyins: Vec<String> = dict
                .system()
                .pronunciations(token)
                .into_iter()
                .map(|(pinyin, _freq)| pinyin)
                .collect();
            if pinyins.is_empty() {
                return None;
            }
            Some((text, pinyins))
        }
    }
}

/// One rendered §9 bigram-export row: concatenated phrase text, the
/// `'`-joined pronunciation of the pair, and the scaled count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportedBigramRow {
    /// Concatenated predecessor + successor phrase text.
    pub phrase: String,
    /// The `'`-joined pronunciation of the pair.
    pub pinyin: String,
    /// The rendered bigram count (`stored × 2`).
    pub count: i64,
}

// ── Instance ────────────────────────────────────────────────────────────

/// One snapshotted candidate, stored inside `CapiInstance` so that
/// `lookup_candidate_t *` can borrow into it across C calls.
pub struct CapiCandidate {
    pub(crate) text: CString,
    pub(crate) kind: CandidateKind,
    pub(crate) candidate_type: crate::types::lookup_candidate_type_t,
    pub(crate) nbest_index: u8,
    /// Bytes of raw input this candidate consumed, snapshotted at guess time
    /// so `pinyin_choose_candidate` can report the new cursor position.
    pub(crate) consumed_bytes: usize,
    /// The candidate's scoring token, snapshotted so the training entry
    /// points (`pinyin_train`'s observation, predicted-candidate training,
    /// `pinyin_is_user_candidate`) can resolve it without re-decoding.
    /// `None` for sentence-level and fallback candidates, which carry no
    /// token and are not trained (§2: only pinned phrases train).
    pub(crate) token: Option<PhraseToken>,
    /// The index this candidate held in the window it was snapshotted from.
    /// The snapshot (`Vec<CapiCandidate>`) may omit entries (sentence rows
    /// under `SORT_WITHOUT_SENTENCE_CANDIDATE`, a `CString` conversion
    /// failure), so a candidate's position in the snapshot is NOT its
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
    /// Per-instance slots the `pinyin_get_pinyin_key` family hands out as
    /// `ChewingKey *` / `ChewingKeyRest *`.
    ///
    /// The pin returns `&`-of a function-local `static`, so its pointer is
    /// one process-wide slot every instance and thread overwrites
    /// (`pinyin.cpp`, `static ChewingKey key;`). Per-instance is observably
    /// identical for the documented use — the consumer reads the pointer
    /// before its next call, as fcitx does (`eim.cpp:419-520`) — and does
    /// not share mutable state across instances.
    pub(crate) key_slot: ChewingKey,
    pub(crate) key_rest_slot: ChewingKeyRest,
    /// Snapshotted candidates, rebuilt by `pinyin_guess_candidates`.
    /// `lookup_candidate_t *` pointers borrow into this vec.
    pub(crate) candidates: Vec<CapiCandidate>,
}

impl CapiInstance {
    /// The parse-path reset: the shared core's reset (composition parse
    /// state, re-anchored window, stored parses) plus this layer's
    /// candidate snapshot — the selection record and the §3 constraint
    /// store stay (upstream's parse-never-touches-constraints rule, the
    /// L2 lifetime rule in `docs/findings/live-typing.md`).
    pub(crate) fn reset_parse_state(&mut self) {
        self.core.reset_parse_state();
        self.candidates.clear();
    }
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
/// after `pinyin_fini` reconstructs and drops it), and must not be stored in a
/// `CapiInstance` or any other longer-lived location.
pub unsafe fn context_ref<'a>(ptr: *mut PinyinContext) -> &'a CapiContext {
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
pub unsafe fn context_mut<'a>(ptr: *mut PinyinContext) -> &'a mut CapiContext {
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
pub unsafe fn instance_ref<'a>(ptr: *mut PinyinInstance) -> &'a CapiInstance {
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
pub unsafe fn instance_mut<'a>(ptr: *mut PinyinInstance) -> &'a mut CapiInstance {
    // SAFETY: Caller guarantees the pointer is valid and unique for the chosen lifetime.
    unsafe { &mut *(ptr.cast::<CapiInstance>()) }
}

/// Converts a `CapiContext` into a `*mut PinyinContext` for return to C.
pub fn box_context(ctx: CapiContext) -> *mut PinyinContext {
    Box::into_raw(Box::new(ctx)).cast()
}

/// Converts a `CapiInstance` into a `*mut PinyinInstance` for return to C.
pub fn box_instance(inst: CapiInstance) -> *mut PinyinInstance {
    Box::into_raw(Box::new(inst)).cast()
}

/// Casts a `*mut LookupCandidate` back to `&CapiCandidate`.
///
/// # Safety
///
/// `ptr` must be non-null and point into an active `CapiInstance::candidates`
/// vec (produced by [`candidate_ptr`]).
pub unsafe fn candidate_ref<'a>(ptr: *mut LookupCandidate) -> &'a CapiCandidate {
    // SAFETY: Caller guarantees the pointer is valid for the chosen lifetime.
    unsafe { &*(ptr.cast::<CapiCandidate>()) }
}

/// Returns a `*mut LookupCandidate` pointing to a `CapiCandidate`.
pub const fn candidate_ptr(cand: &CapiCandidate) -> *mut LookupCandidate {
    (cand as *const CapiCandidate as *mut CapiCandidate).cast()
}
