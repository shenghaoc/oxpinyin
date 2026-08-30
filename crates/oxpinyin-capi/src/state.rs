//! Real backing state behind the opaque C handles.
//!
//! `CapiContext` lives behind `pinyin_context_t *` and `CapiInstance`
//! behind `pinyin_instance_t *`. The opaque `#[repr(C)]` types in
//! [`crate::types`] exist only for the generated C header.
#![allow(dead_code)]

use std::ffi::CString;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering};

use oxpinyin_core::{
    DoublePinyinParse, DoublePinyinScheme, FullPinyinIndexParse, FullPinyinScheme, OptionBits,
    PINYIN_INCOMPLETE, PhraseToken, ZhuyinParse, ZhuyinScheme,
};
use oxpinyin_engine::{
    CandidateKind, CandidateList, Config, EngineError, check_lookup_offset_range,
    normalize_lookup_offset,
};
pub(crate) use oxpinyin_runtime::USER_STORE_FILE;

/// Upstream's phrase-index library count (`novel_types.h:43`, `1<<4`).
///
/// The pin asserts an index below this in the addon load/unload path; the
/// compatibility policy's availability class turns that abort into a
/// `false`.
const PHRASE_INDEX_LIBRARY_COUNT: u8 = 16;
use oxpinyin_runtime::{Runtime, RuntimeSession};
pub(crate) use oxpinyin_runtime::{RuntimeDict as SharedDict, RuntimeLm as SharedLm};
use oxpinyin_user::{
    ExportedPhrase, NETWORK_DICTIONARY, SENTENCE_START, USER_DICTIONARY, UserStore,
    is_user_file_token,
};

use crate::types::{ChewingKey, ChewingKeyRest, LookupCandidate, PinyinContext, PinyinInstance};

// ── Context ─────────────────────────────────────────────────────────────
//
// The concrete assembly lives in `oxpinyin-runtime` (`SharedDict` /
// `SharedLm` above are aliases into it), so the C ABI and the Python binding
// construct engines through one reviewed code path. This file keeps only the
// C-facing state: live option words, storage/config plumbing around the C
// contracts, and the instance snapshot machinery.

/// The session type every C handle wraps: the shared runtime's concrete
/// session.
pub(crate) type CapiSession = RuntimeSession;

/// State behind `pinyin_context_t *`.
///
/// Owns the shared [`Runtime`] (when this context has system tables).
/// Instances receive cheap handle clones from it — `dict()`, `lm()`,
/// `user_store()` — so they never borrow the context and stay alive past
/// `pinyin_fini`.
pub(crate) struct CapiContext {
    pub(crate) config: Config,
    /// The shared concrete assembly; `None` under a user-store-only context.
    runtime: Option<Runtime>,
    /// The user-learning store, shared by value-clone with every instance.
    ///
    /// `None` when the caller passed an empty user directory — the
    /// libpinyin situation where `pinyin_train` refuses — or when the store
    /// file cannot be opened (a missing/inaccessible user dir must not make
    /// `pinyin_init` fail; training then degrades to `false`, upstream-style).
    user: Option<UserStore>,
    /// Live `PINYIN_INCOMPLETE` bit. Shared with every instance so
    /// `pinyin_set_options` remasks already-allocated sessions.
    pub(crate) incomplete: Arc<AtomicBool>,
    /// Live double-pinyin scheme. Shared with every instance so
    /// `pinyin_set_double_pinyin_scheme` remasks already-allocated sessions.
    pub(crate) double_scheme: Arc<AtomicI32>,
    /// Live Zhuyin scheme. Shared with every instance so
    /// `pinyin_set_zhuyin_scheme` remasks already-allocated sessions.
    pub(crate) zhuyin_scheme: Arc<AtomicI32>,
    /// Live full-pinyin scheme. Shared with every instance so
    /// `pinyin_set_full_pinyin_scheme` remasks already-allocated
    /// sessions.
    pub(crate) full_scheme: Arc<AtomicI32>,
    /// Live `USE_TONE` bit for the bopomofo context.
    pub(crate) use_tone: Arc<AtomicBool>,
    /// Live option word. Shared with every instance so `pinyin_set_options`
    /// remasks already-allocated sessions.
    pub(crate) options: Arc<AtomicU32>,
}

/// Where [`CapiContext::new`] takes unigram counts from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UnigramSource {
    /// Require a parsable `interpolation2.text` next to the redb tables.
    RealOnly,
    /// Test fixtures and the W3 mini tables use export-ABI flat counts.
    FlatExportForFixtures,
}

impl CapiContext {
    pub(crate) fn new(system_dir: &str, user_dir: &str) -> Option<Self> {
        Self::new_with_unigrams(system_dir, user_dir, UnigramSource::RealOnly)
    }

    /// Fixture/test constructor: the W3 mini system dir deliberately has no
    /// model file, so it opts into the old flat-export behaviour explicitly.
    pub(crate) fn new_for_fixtures(system_dir: &str, user_dir: &str) -> Option<Self> {
        Self::new_with_unigrams(system_dir, user_dir, UnigramSource::FlatExportForFixtures)
    }

    fn new_with_unigrams(
        system_dir: &str,
        user_dir: &str,
        unigram_source: UnigramSource,
    ) -> Option<Self> {
        if system_dir.is_empty() {
            return None;
        }

        // W8 fork-bootstrap wiring and the fixture split both live in the
        // shared assembly now: the constructor opens the tables, installs λ
        // from table.conf when present, fails init on a missing model file
        // (RealOnly) or derives flat fixture counts, degrades an unusable
        // user dir to "no learning", and wires addons + punctuation.
        let sys = Path::new(system_dir);
        let runtime = match unigram_source {
            UnigramSource::RealOnly => Runtime::open(sys, Some(Path::new(user_dir))),
            UnigramSource::FlatExportForFixtures => {
                Runtime::open_fixtures(sys, Some(Path::new(user_dir)))
            }
        }
        .ok()?;
        let user = runtime.user_store();
        Some(Self {
            config: Config::default(),
            runtime: Some(runtime),
            user,
            incomplete: Arc::new(AtomicBool::new(true)),
            double_scheme: Arc::new(AtomicI32::new(DoublePinyinScheme::Ms as i32)),
            zhuyin_scheme: Arc::new(AtomicI32::new(ZhuyinScheme::Standard as i32)),
            full_scheme: Arc::new(AtomicI32::new(FullPinyinScheme::Hanyu as i32)),
            use_tone: Arc::new(AtomicBool::new(false)),
            options: Arc::new(AtomicU32::new(PINYIN_INCOMPLETE)),
        })
    }

    /// User-store-only context for standalone migration tools
    /// (`oxpinyin-dictool import`). The C ABI `pinyin_init` still requires
    /// system tables — its contract is a decoder context — while this
    /// Rust-only constructor lets a tool drive the import/export/save trio
    /// without carrying a system dictionary. `pinyin_alloc_instance` reports
    /// `None` for such a context because there is nothing to decode with.
    pub(crate) fn new_user_only(user_dir: &str) -> Option<Self> {
        if user_dir.is_empty() {
            return None;
        }
        let user = UserStore::open(&Path::new(user_dir).join(USER_STORE_FILE)).ok()?;
        Some(Self {
            config: Config::default(),
            runtime: None,
            user: Some(user),
            incomplete: Arc::new(AtomicBool::new(true)),
            double_scheme: Arc::new(AtomicI32::new(DoublePinyinScheme::Ms as i32)),
            zhuyin_scheme: Arc::new(AtomicI32::new(ZhuyinScheme::Standard as i32)),
            full_scheme: Arc::new(AtomicI32::new(FullPinyinScheme::Hanyu as i32)),
            use_tone: Arc::new(AtomicBool::new(false)),
            options: Arc::new(AtomicU32::new(PINYIN_INCOMPLETE)),
        })
    }

    pub(crate) fn alloc_instance(&self, context: *mut PinyinContext) -> Option<CapiInstance> {
        let runtime = self.runtime.as_ref()?;
        let session = runtime.new_session(&self.config).ok()?;
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
            options: Arc::clone(&self.options),
            double_parse: None,
            double_input: String::new(),
            zhuyin_parse: None,
            zhuyin_input: String::new(),
            full_parse: None,
            full_input: String::new(),
        })
    }

    /// Clone of the context's user store, if this context has one.
    ///
    /// The import iterator owns this clone; because the store's §4 dirty flag
    /// is shared by every clone, `pinyin_end_add_phrases` can arm
    /// `m_modified` through it without retaining a context pointer.
    pub(crate) fn user_store(&self) -> Option<UserStore> {
        self.user.clone()
    }

    /// `pinyin_save`'s body (§4): `false` without a user dir (upstream
    /// `pinyin.cpp:1133`), otherwise the store's gated save — `false` when
    /// unmodified (`:1136`), `true` after a dirty save.
    pub(crate) fn save_user(&mut self) -> bool {
        match self.user.as_mut() {
            None => false,
            Some(store) => store.save().unwrap_or(false),
        }
    }

    /// `pinyin_mask_out`'s body: the store-level deletion, or `false`
    /// without a user store.
    pub(crate) fn mask_out(&mut self, mask: u32, value: u32) -> bool {
        match self.user.as_mut() {
            None => false,
            Some(store) => store.mask_out(mask, value).is_ok(),
        }
    }

    /// Load addon library `index` from the runtime's first system data dir.
    ///
    /// The pin's addon phrase index asserts `index < PHRASE_INDEX_LIBRARY_COUNT`
    /// (`novel_types.h:43`, 1<<4) on the load path as it does on unload; per
    /// the availability class of `docs/findings/compatibility-policy.md` this
    /// answers `false` instead — the same bound [`CapiContext::unload_addon`]
    /// applies. Without it an out-of-range index would silently load a stray
    /// `addon_{index}_*.redb` on disk rather than being refused. A
    /// user-store-only context has no runtime, so it loads nothing.
    pub(crate) fn load_addon(&self, index: u8) -> bool {
        if index >= PHRASE_INDEX_LIBRARY_COUNT {
            return false;
        }
        match self.runtime.as_ref() {
            Some(runtime) => runtime.load_system_addon(index),
            None => false,
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
            return false;
        }
        match self.runtime.as_ref() {
            Some(runtime) => runtime.unload_system_addon(index),
            None => false,
        }
    }

    /// §9 phrase-export materialization. [`USER_DICTIONARY`] and
    /// [`NETWORK_DICTIONARY`] export their stored rows; any other index
    /// exports an empty list.
    pub(crate) fn export_phrases(&self, index: u32) -> Option<Vec<ExportedPhrase>> {
        let index = u8::try_from(index).ok()?;
        if index != USER_DICTIONARY && index != NETWORK_DICTIONARY {
            return Some(Vec::new());
        }
        self.user.as_ref()?.export_phrases_in(index).ok()
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
        if self.runtime.is_some() {
            return true;
        }
        let Some(store) = self.user.as_ref() else {
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
        let store = self.user.as_ref()?;
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
            let store = self.user.as_ref()?;
            let phrase = store.phrase(token).ok().flatten()?;
            // Render each reading through the shared `render_pinyin` helper,
            // skipping any unrenderable one — the same rule `export_phrases`
            // applies, so the phrase and bigram exports stay consistent.
            let pinyins: Vec<String> = phrase
                .pronunciations()
                .iter()
                .filter_map(|pronunciation| pronunciation.render_pinyin())
                .collect();
            if pinyins.is_empty() {
                return None;
            }
            Some((phrase.text().to_owned(), pinyins))
        } else {
            let dict = self.runtime.as_ref()?.dict();
            let text = dict.system().phrase_text(token).ok().flatten()?;
            let pinyins: Vec<String> = dict
                .system()
                .pronunciations(token)
                .ok()?
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
pub(crate) struct CapiCandidate {
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
pub(crate) struct CapiInstance {
    /// The owning context's C handle, returned by `pinyin_get_context`
    /// (upstream `pinyin_get_context`, `pinyin.cpp:1358-1360`). A raw
    /// pointer like every C handle here: no ownership, and using it
    /// after `pinyin_fini` is the caller's UAF, exactly upstream's.
    pub(crate) context: *mut PinyinContext,
    pub(crate) session: CapiSession,
    /// `m_phrase_result` (`pinyin.cpp:90`): the phrase-segment span DP's
    /// output, written by `pinyin_phrase_segment` and read by
    /// `pinyin_get_n_phrase` / `pinyin_get_phrase_token`. Cleared by
    /// `pinyin_reset` (`pinyin.cpp:2699`) and by nothing else.
    pub(crate) phrase_result: Vec<PhraseToken>,
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
    /// The re-anchored candidate window from the most recent
    /// `pinyin_guess_candidates` when it ran at an offset other than the
    /// composition's own, as `(anchor, window)` — retained so a later
    /// `pinyin_choose_candidate` resolves its index against the SAME window
    /// the caller saw (rather than the composition-anchored cached list,
    /// which would select a different row whenever the two differ) and
    /// measures the chosen span from that anchor (the candidate's
    /// `consumed_bytes` is anchor-relative). `None` when the last guess ran
    /// at the composition offset or under a transformed scheme, where the
    /// cached list already answers.
    pub(crate) anchored_window: Option<(usize, CandidateList)>,
    /// Bytes of raw input consumed by the most recent parse call — upstream
    /// `m_parsed_len` (`pinyin.cpp:84`), returned by
    /// `pinyin_get_parsed_input_length` (`pinyin.cpp:1611-1613`).
    /// Allocation and reset both store 0 (`pinyin.cpp:1318,2692`).
    pub(crate) parsed_len: usize,
    /// Clone of the context's user store. `None` under an empty user dir.
    pub(crate) user: Option<UserStore>,
    /// Shared dictionary (system + user-file + addon set) for prediction.
    pub(crate) dict: SharedDict,
    /// Clone of the context's language model, for the predicted-candidate
    /// frequency key: the amplified-law total must be read live per call
    /// (training changes it), like the pin's
    /// `get_phrase_index_total_freq()` read (`pinyin.cpp:1813-1814`).
    pub(crate) lm: SharedLm,
    /// Shared live `PINYIN_INCOMPLETE` flag from the owning context.
    pub(crate) incomplete: Arc<AtomicBool>,
    /// Shared live double-pinyin scheme from the owning context.
    pub(crate) double_scheme: Arc<AtomicI32>,
    /// Most recent double-pinyin parse, when the last parse call was the
    /// double-pinyin entry point. Used for aux text and candidate-offset
    /// mapping back to the original double-pinyin input bytes.
    pub(crate) double_parse: Option<DoublePinyinParse>,
    /// Original double-pinyin input for sentence/preedit fallback display.
    pub(crate) double_input: String,
    /// Shared live Zhuyin scheme from the owning context.
    pub(crate) zhuyin_scheme: Arc<AtomicI32>,
    /// Most recent Zhuyin parse, when the last parse call was the chewing
    /// entry point.
    pub(crate) zhuyin_parse: Option<ZhuyinParse>,
    /// Original Zhuyin input for sentence/preedit fallback display.
    pub(crate) zhuyin_input: String,
    /// Shared live full-pinyin scheme from the owning context.
    pub(crate) full_scheme: Arc<AtomicI32>,
    /// Most recent full-pinyin index parse, when the scheme is LUOMA or
    /// SECONDARY_ZHUYIN and the last parse call was the full-pinyin
    /// entry point. Used for aux-text rendering over the raw input.
    pub(crate) full_parse: Option<FullPinyinIndexParse>,
    /// Original full-pinyin input for aux-text cursor mapping.
    pub(crate) full_input: String,
    /// Shared live `USE_TONE` flag from the owning context.
    pub(crate) use_tone: Arc<AtomicBool>,
    /// Shared live option word from the owning context.
    pub(crate) options: Arc<AtomicU32>,
}

impl CapiInstance {
    /// The parse-path reset: the composition's parse state goes, the
    /// selection record and the §3 constraint store stay — upstream's
    /// `pinyin_parse_more_full_pinyins` never touches instance-level
    /// constraints (`pinyin.cpp:1497-1533`), and the frontend re-sends
    /// the whole buffer every keystroke, so the chosen cursor must
    /// survive the re-parse (`Session::reset_composition`, the L2
    /// lifetime rule in `docs/findings/live-typing.md`).
    pub(crate) fn reset_parse_state(&mut self) {
        self.session.reset_composition();
        self.candidates.clear();
        self.parsed_len = 0;
        self.double_parse = None;
        self.double_input.clear();
        self.zhuyin_parse = None;
        self.zhuyin_input.clear();
        self.full_parse = None;
        self.full_input.clear();
    }

    /// Begin a parse of `original` (the caller's input, in the active
    /// mode's own coordinates): continue the current composition when the
    /// buffer evolved from the stored one — extension, backspace, or
    /// re-send — whether the composition is open (upstream's
    /// parse-never-touches-constraints rule) or a selection consumed it
    /// (the R5 revert, register #8: upstream's store survives every
    /// re-parse and only `pinyin_reset` clears it, `pinyin.cpp:2693-2704`).
    /// `validate_constraint` drops what stops spelling at the next guess.
    /// Only a divergent buffer starts fresh: a different string is a
    /// different composition, and a stale selection-derived cursor must
    /// not mis-anchor its window before validate could drop the
    /// mismatched forcings.
    pub(crate) fn begin_parse(&mut self, original: &[u8]) {
        let stored: &[u8] = if self.zhuyin_parse.is_some() {
            self.zhuyin_input.as_bytes()
        } else if self.double_parse.is_some() {
            self.double_input.as_bytes()
        } else if self.full_parse.is_some() {
            self.full_input.as_bytes()
        } else {
            self.session.raw_input().as_bytes()
        };
        let continues = self.session.parse_continues(stored, original);
        let committed_continues =
            !continues && self.session.committed_parse_continues(stored, original);
        // The committed-continues shape needs exactly `reset_parse_state`:
        // its `reset_composition` keeps the store and the selection
        // record, so the full reset below must not run there.
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
    /// coordinates — the space the caller's guess/choose offsets live in.
    ///
    /// - Plain full pinyin: the full law over the session's raw buffer,
    ///   whose `'` bytes are the matrix's zero-key columns
    ///   ([`Session::normalized_lookup_offset`]).
    /// - LUOMA / SECONDARY_ZHUYIN: the full law over the consumed prefix
    ///   of the stored original input — the pinned index parse consumes
    ///   `'` as the same separator, and bytes past its consumed length
    ///   never entered the composition, so they bound the offset range
    ///   exactly like the other transformed seams' parsed lengths.
    /// - Double pinyin: no zero-key column can exist — `'` is not a scheme
    ///   key, the parse stops there, and upstream asserts the input
    ///   carries none at all (`pinyin_parser2.cpp:629`) — so only the
    ///   range refusal against the parsed original length applies.
    /// - Zhuyin: `'` is either outside the keyboard (the parse stops
    ///   there) or a *content* symbol (Gin-Yieh ㄥ, Eten ㄘ), never a zero
    ///   key, so the separator walk would mis-read content; only the
    ///   range refusal against the parsed original length applies.
    pub(crate) fn validate_lookup_offset(&self, offset: usize) -> Result<usize, EngineError> {
        if let Some(parse) = self.zhuyin_parse.as_ref() {
            check_lookup_offset_range(parse.consumed(), offset)
        } else if let Some(parse) = self.double_parse.as_ref() {
            check_lookup_offset_range(parse.consumed(), offset)
        } else if let Some(parse) = self.full_parse.as_ref() {
            // The min is defensive only: `full_input` and the parse are set
            // together and cleared together, so consumed never exceeds the
            // buffer — but a desync must refuse, not slice-panic.
            let consumed = parse.consumed().min(self.full_input.len());
            normalize_lookup_offset(&self.full_input.as_bytes()[..consumed], offset)
        } else {
            self.session.normalized_lookup_offset(offset)
        }
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
