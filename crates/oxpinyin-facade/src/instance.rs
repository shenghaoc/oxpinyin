//! One instance's orchestration state: the composition and live options,
//! the parse-mode state machine, and the snapshot-adjacent fields both
//! facades' C layers borrow against.

use std::sync::atomic::Ordering;

use oxpinyin_core::{
    Dictionary, DoublePinyinParse, FullPinyinIndexParse, OptionBits, PhraseText, PhraseToken,
    ZhuyinParse,
};
use oxpinyin_engine::{
    CandidateList, EngineError, check_lookup_offset_range, normalize_lookup_offset,
};
use oxpinyin_runtime::{Runtime, RuntimeDict, RuntimeLm};
use oxpinyin_user::UserStore;

use crate::context::LiveOptions;

/// The [`InstanceState::anchored_window`] anchor for a **before-cursor**
/// window — the buffer start, not the lookup offset.
///
/// The two window builders measure a candidate's `consumed_bytes` in
/// different coordinates, and [`oxpinyin_engine::Session::select_anchored`]
/// reads the chosen span as `[anchor, anchor + consumed_bytes)`:
///
/// - **After-cursor** (`Session::candidates_at(offset)`) rebases its graph
///   onto `raw[offset..]`, so `consumed_bytes` is a LENGTH measured from
///   the lookup offset. Its anchor is that offset.
/// - **Before-cursor** (`Session::candidates_ending_at(offset)`) reads
///   the retained matrix, whose coordinates are absolute from
///   the buffer start, so every row's `consumed_bytes` is the span's
///   ABSOLUTE END — the lookup offset itself, shared by every row in the
///   window, whatever byte each span starts on. The anchor that reproduces
///   that end (and so the consumed advance) is therefore the coordinate
///   those ends are measured from: 0.
///
/// Reusing the after-cursor anchor here would read the span as
/// `[offset, 2 * offset)` and walk the composition off the end of the
/// buffer; leaving the window unanchored resolves the row's index against
/// the composition-anchored cached list instead, which is a different list
/// — the caller would commit a row it never displayed.
pub const BEFORE_CURSOR_ANCHOR: usize = 0;

/// State behind a facade's instance handle, minus the C parts: everything
/// the two C-ABI crates' `CapiInstance`s held in identical shape.
///
/// The C
/// layers hold one of these plus their ABI-only fields (the context
/// back-pointer, the `#[repr(C)]` key slots, the `CString` candidate
/// snapshot).
pub struct InstanceCore {
    /// Retained composition, with no dictionary or model owner.
    pub session: oxpinyin_engine::SessionState,
    state: InstanceState,
}

/// Cursor, parse and presentation state retained between calls.
pub struct InstanceState {
    /// The context's live option/scheme state, by handle.
    pub live: LiveOptions,
    /// The phrase-segment span DP's output, written by `phrase_segment`
    /// and read by the `get_n_phrase` / `get_phrase_token` pair. Cleared
    /// by the full reset and by nothing else.
    pub phrase_result: Vec<PhraseToken>,
    /// Bytes of raw input consumed by the most recent parse call —
    /// upstream `m_parsed_len`, in the active parse mode's own
    /// coordinates. Allocation and reset both store 0.
    pub parsed_len: usize,
    /// The candidate window from the most recent guess, as `(anchor, window)`.
    /// Retained across parsing so a later choose resolves its index against
    /// the same window the caller saw, including a guess at offset zero.
    /// A new guess replaces it and full reset drops it. Pinyin drops it on
    /// an accepted choose. Zhuyin retains it for further choices.
    pub anchored_window: Option<(usize, CandidateList)>,
    /// Most recent double-pinyin parse, when the last parse call was the
    /// double-pinyin entry point. Used for aux text and
    /// candidate-offset mapping back to the original input bytes.
    pub double_parse: Option<DoublePinyinParse>,
    /// Original double-pinyin input for sentence/preedit fallback
    /// display.
    pub double_input: String,
    /// Most recent Zhuyin parse, when the last parse call was the chewing
    /// entry point.
    pub zhuyin_parse: Option<ZhuyinParse>,
    /// Original Zhuyin input for sentence/preedit fallback display.
    pub zhuyin_input: String,
    /// Most recent full-pinyin index parse (LUOMA / `SECONDARY_ZHUYIN`),
    /// when the last parse call was the full-pinyin entry point under
    /// such a scheme. Used for aux-text rendering over the raw input.
    pub full_parse: Option<FullPinyinIndexParse>,
    /// Original full-pinyin input for aux-text cursor mapping.
    pub full_input: String,
    /// The pin's parse-time matrix for the index-parsed full pinyin, double
    /// pinyin and chewing entry points: the parse's keys plus the steps the
    /// pin runs after `fill_matrix` under the options in force at that parse
    /// (fuzzy alternatives, and for full pinyin the resplit and divided
    /// passes). Empty for plain full pinyin, whose scan matrix the session
    /// keeps, and while no key was placed.
    pub parse_matrix: Vec<oxpinyin_engine::MatrixKey>,
}

impl std::ops::Deref for InstanceCore {
    type Target = InstanceState;
    fn deref(&self) -> &Self::Target {
        &self.state
    }
}
impl std::ops::DerefMut for InstanceCore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

/// One operation's composition and storage borrows.
pub struct InstanceView<'a, S, E> {
    /// Composition borrowed with current dictionary and model readers.
    pub session: oxpinyin_engine::Session<RuntimeDict<'a>, RuntimeLm<'a>, E>,
    /// Learning store borrowed for this operation.
    pub user: &'a UserStore,
    /// Current dictionary reader.
    pub dict: RuntimeDict<'a>,
    /// Current language-model reader.
    pub lm: RuntimeLm<'a>,
    /// Borrowed cursor, parse and presentation state.
    pub state: S,
}

/// An operation that can change composition but only read phrase storage.
pub type InstanceMut<'a> =
    InstanceView<'a, &'a mut InstanceState, &'a mut oxpinyin_engine::SessionState>;
/// A shared operation over retained composition and current phrase storage.
pub type InstanceRead<'a> = InstanceView<'a, &'a InstanceState, &'a oxpinyin_engine::SessionState>;

impl<S: std::borrow::Borrow<InstanceState>, E> std::ops::Deref for InstanceView<'_, S, E> {
    type Target = InstanceState;
    fn deref(&self) -> &Self::Target {
        self.state.borrow()
    }
}
impl<S: std::borrow::Borrow<InstanceState>, E: std::borrow::Borrow<oxpinyin_engine::SessionState>>
    InstanceView<'_, S, E>
{
    /// Re-borrow a composition operation without taking any storage ownership.
    pub fn read(&self) -> InstanceRead<'_> {
        InstanceView {
            session: self.session.read(self.dict, self.lm),
            user: self.user,
            dict: self.dict,
            lm: self.lm,
            state: self.state.borrow(),
        }
    }
}

impl<S: std::borrow::BorrowMut<InstanceState>, E> std::ops::DerefMut for InstanceView<'_, S, E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state.borrow_mut()
    }
}

impl<
    S: std::borrow::BorrowMut<InstanceState>,
    E: std::borrow::BorrowMut<oxpinyin_engine::SessionState>,
> InstanceView<'_, S, E>
{
    /// Retains a successful guess's `window` as the anchored window, and
    /// hands the window it replaces back to the session for its storage.
    /// Every other path that ends an anchored window simply drops it.
    pub fn replace_anchored_window(&mut self, anchor: usize, window: CandidateList) {
        if let Some((_, replaced)) = self
            .state
            .borrow_mut()
            .anchored_window
            .replace((anchor, window))
        {
            self.session.recycle_candidates(replaced);
        }
    }
}

impl InstanceCore {
    /// Assemble retained state; operation readers are supplied by the context.
    pub(crate) fn new(session: oxpinyin_engine::SessionState, live: LiveOptions) -> Self {
        Self {
            session,
            state: InstanceState {
                live,
                phrase_result: Vec::new(),
                parsed_len: 0,
                anchored_window: None,
                double_parse: None,
                double_input: String::new(),
                zhuyin_parse: None,
                zhuyin_input: String::new(),
                full_parse: None,
                full_input: String::new(),
                parse_matrix: Vec::new(),
            },
        }
    }

    /// Re-borrow storage for a shared operation.
    pub fn read<'a>(&'a self, runtime: &'a Runtime) -> InstanceRead<'a> {
        InstanceView {
            session: self.session.read(runtime.dict(), runtime.lm()),
            user: runtime.user_store(),
            dict: runtime.dict(),
            lm: runtime.lm(),
            state: &self.state,
        }
    }

    /// Re-borrow storage while changing only this instance's composition state.
    pub fn bind<'a>(&'a mut self, runtime: &'a Runtime) -> InstanceMut<'a> {
        InstanceView {
            session: runtime.session(&mut self.session),
            user: runtime.user_store(),
            dict: runtime.dict(),
            lm: runtime.lm(),
            state: &mut self.state,
        }
    }
}

impl<S: std::borrow::Borrow<InstanceState>, E: std::borrow::Borrow<oxpinyin_engine::SessionState>>
    InstanceView<'_, S, E>
{
    /// The current live option word.
    #[must_use]
    pub fn options(&self) -> OptionBits {
        OptionBits::from_bits(self.live.options.load(Ordering::Relaxed))
    }

    /// The generalized lookup-offset law in the active parse mode's own
    /// coordinates — the space the caller's guess/choose offsets live in.
    ///
    /// - Plain full pinyin: the full law over the session's raw buffer,
    ///   whose `'` bytes are the matrix's zero-key columns.
    /// - LUOMA / `SECONDARY_ZHUYIN`: the full law over the consumed
    ///   prefix of the stored original input — the pinned index parse
    ///   consumes `'` as the same separator, and bytes past its consumed
    ///   length never entered the composition.
    /// - Double pinyin and the zhuyin keyboards: no zero-key column can
    ///   exist (`'` is out of scheme or a content symbol), so only the
    ///   range refusal against the parsed original length applies.
    ///
    /// The chain order is the union of the two facades'; a facade that
    /// never populates one of the parse states simply never takes that
    /// branch, which preserves its law exactly.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when `offset` exceeds the active parse's
    /// consumed input, per the range law above.
    pub fn validate_lookup_offset(&self, offset: usize) -> Result<usize, EngineError> {
        self.validate_abi_lookup_offset(offset, false)
    }

    pub(crate) fn validate_lookup_offset_impl(&self, offset: usize) -> Result<usize, EngineError> {
        if let Some(parse) = self.zhuyin_parse.as_ref() {
            return check_lookup_offset_range(parse.consumed(), offset);
        }
        if let Some(parse) = self.double_parse.as_ref() {
            return check_lookup_offset_range(parse.consumed(), offset);
        }
        if let Some(parse) = self.full_parse.as_ref() {
            // The min is defensive only: `full_input` and the parse are
            // set together and cleared together, so consumed never
            // exceeds the buffer — but a desync must refuse, not
            // slice-panic.
            let consumed = parse.consumed().min(self.full_input.len());
            return normalize_lookup_offset(&self.full_input.as_bytes()[..consumed], offset);
        }
        self.session.normalized_lookup_offset(offset)
    }

    /// The phrase-table tokens stored for `phrase`, in library order.
    #[must_use]
    pub fn phrase_tokens(&self, phrase: &PhraseText) -> Vec<u32> {
        self.dict
            .tokens_for_ucs4(&phrase.outward_words().collect::<Vec<_>>())
            .iter()
            .map(|token| token.value())
            .collect()
    }

    /// The phrase text of `token`, when its library is loaded and owns it.
    #[must_use]
    pub fn token_text(&self, token: u32) -> Option<PhraseText> {
        self.dict.token_text(token)
    }

    /// The stored pronunciation count of `token`.
    #[must_use]
    pub fn token_pronunciation_count(&self, token: u32) -> Option<u32> {
        self.dict.token_pronunciation_count(token)
    }

    /// The active parse's consumed input length, in its own coordinates.
    #[must_use]
    pub fn parsed_input_length(&self) -> usize {
        self.parsed_len
    }

    /// Whether a sentence lookup has run since the last full reset.
    #[must_use]
    pub fn sentence_lookup_active(&self) -> bool {
        self.session.sentence_lookup_active()
    }

    /// The decoded phrases of n-best row `index`, while a lookup is active.
    pub fn sentence_phrases(&self, index: u8) -> Option<impl Iterator<Item = &PhraseText>> {
        self.session.sentence_phrases(index)
    }

    /// The phrase-segment result's token count.
    #[must_use]
    pub fn phrase_count(&self) -> usize {
        self.phrase_result.len()
    }

    /// The phrase-segment result's token at `index`, if the result has one.
    #[must_use]
    pub fn phrase_token(&self, index: usize) -> Option<u32> {
        self.phrase_result.get(index).map(|token| token.value())
    }

    /// Whether training row `index` reaches one of the pin's own
    /// training aborts — the index past nonempty results, a stale forced
    /// phrase or a span outside the matrix
    /// ([`oxpinyin_engine::Session::train_nbest_preflight`]) — after the
    /// same directory gate. Reads only.
    #[must_use]
    pub fn train_aborts(&self, index: u8) -> bool {
        if !self.user.has_user_directory() {
            return false;
        }
        matches!(
            self.session.train_nbest_preflight(index),
            Err(EngineError::CandidateIndexOutOfRange { .. }
                | EngineError::StaleTrainingConstraint { .. }
                | EngineError::StaleTrainingSpan { .. })
        )
    }

    /// The default facade item's live unigram. Missing sub-indices and slots
    /// answer `None` (074a2219 `phrase_index.h:646-652`, `pinyin.cpp:2821-2834`).
    #[must_use]
    pub fn token_unigram_frequency(&self, token: u32) -> Option<u64> {
        let token = token & (oxpinyin_user::PHRASE_INDEX_LIBRARY_MASK | oxpinyin_user::PHRASE_MASK);
        let nibble = token >> 24;
        match nibble {
            _ if self.dict.is_system_library(nibble as u8) => {
                self.dict.live_items().effective_unigram(token)
            }
            5..=7 if self.dict.library_visible(nibble) => {
                self.user.user_unigram_frequency(token).ok().flatten()
            }
            _ => None,
        }
    }
}

impl<
    S: std::borrow::BorrowMut<InstanceState>,
    E: std::borrow::BorrowMut<oxpinyin_engine::SessionState>,
> InstanceView<'_, S, E>
{
    /// The list a lookup answers from: the window the last
    /// `pinyin_guess_candidates` re-anchored, else the session's own list,
    /// built first if a deferred parse or sentence guess left it stale
    /// ([`oxpinyin_engine::Session::set_defer_list_builds`]).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the deferred build hits a backend
    /// failure.
    pub fn lookup_window(&mut self) -> Result<&CandidateList, EngineError> {
        if let Some((_, window)) = self.state.borrow().anchored_window.as_ref() {
            return Ok(window);
        }
        self.session.refresh_candidates()?;
        Ok(self.session.candidates())
    }

    /// The parse-path reset: the composition's parse state goes, the
    /// selection record and the constraint store stay — upstream's
    /// parse-never-touches-constraints rule; the frontend re-sends the
    /// whole buffer every keystroke, so the chosen cursor must survive
    /// the re-parse.
    ///
    /// Candidate snapshots survive parsing in both ABIs. Keep their anchored
    /// window too, so a later choose resolves the retained row's token and
    /// span rather than an index into the new parse's candidate list
    /// (074a2219, pinyin.cpp:2580-2587, zhuyin.cpp:1646-1666).
    pub fn reset_parse_state(&mut self) {
        self.session.reset_composition();
        self.parsed_len = 0;
        self.double_parse = None;
        self.double_input.clear();
        self.zhuyin_parse = None;
        self.zhuyin_input.clear();
        self.full_parse = None;
        self.full_input.clear();
        self.parse_matrix.clear();
    }

    /// Segment `text` and retain the result on the instance.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] from the engine's phrase span DP.
    pub fn segment_phrases(&mut self, text: &str) -> Result<bool, EngineError> {
        let (matched, tokens) = self.session.phrase_segment(text)?;
        self.phrase_result = tokens;
        Ok(matched)
    }

    /// Segment raw UCS-4 words and retain the result on the instance.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] from the engine's phrase span DP.
    pub fn segment_phrases_ucs4(&mut self, words: &[u32]) -> Result<bool, EngineError> {
        let (matched, tokens) = self.session.phrase_segment_ucs4(words)?;
        self.phrase_result = tokens;
        Ok(matched)
    }

    /// Selects the candidate at `source_index` in the window the caller saw,
    /// or its promoted token when the facade promoted an addon row first.
    /// Returns the selection outcome and the chosen row's absolute end when
    /// anchored. The caller controls the window's lifetime.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] from the engine's selection.
    pub fn select_candidate(
        &mut self,
        source_index: usize,
        promoted: Option<PhraseToken>,
    ) -> Result<(oxpinyin_engine::Selection, Option<usize>), EngineError> {
        let (window_end, selection) = {
            let state = self.state.borrow();
            let window = state.anchored_window.as_ref();
            let window_end = window.and_then(|(anchor, window)| {
                window
                    .get(source_index)
                    .map(|row| anchor + row.span_start() + row.consumed_bytes())
            });
            let selection = match (promoted, window) {
                (Some(promoted), Some((anchor, window))) => {
                    self.session
                        .select_anchored_promoted(source_index, window, *anchor, promoted)
                }
                (Some(promoted), None) => self.session.select_promoted(source_index, promoted),
                (None, Some((anchor, window))) => {
                    self.session.select_anchored(source_index, window, *anchor)
                }
                (None, None) => self.session.select(source_index),
            };
            (window_end, selection)
        };
        Ok((selection?, window_end))
    }

    /// Clears the constraint at `offset`, mapping the caller's original-input
    /// offset to the active parse mode's session coordinates first. The
    /// caller selects the transformed seams its facade can populate.
    pub fn clear_abi_constraint(
        &mut self,
        offset: usize,
        map_double_pinyin: bool,
        map_full_pinyin: bool,
    ) -> bool {
        let session_offset = if let Some(parse) = self.zhuyin_parse.as_ref() {
            crate::zhuyin_session_offset(parse, offset)
        } else if map_double_pinyin && let Some(parse) = self.double_parse.as_ref() {
            crate::double_session_offset(parse, offset)
        } else if map_full_pinyin && let Some(parse) = self.full_parse.as_ref() {
            crate::full_session_offset(parse, offset)
        } else {
            offset
        };
        self.session.clear_constraint(session_offset)
    }

    /// Runs the n-best sentence lookup after the caller's matrix check. A
    /// parse without keys validates against the empty matrix and keeps the
    /// previous sentence rows.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] from the lookup.
    pub fn guess_sentence_with_matrix(
        &mut self,
        matrix_has_keys: bool,
    ) -> Result<bool, EngineError> {
        if !matrix_has_keys {
            self.session.set_candidate_prefixes(core::iter::empty());
            self.session.discard_composition();
            return Ok(false);
        }
        self.session.guess_sentence()
    }

    /// Runs the prefix-seeded sentence lookup after the caller's matrix
    /// check.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] from the lookup.
    pub fn guess_sentence_with_prefix_and_matrix(
        &mut self,
        prefixes: &[u32],
        matrix_has_keys: bool,
    ) -> Result<bool, EngineError> {
        if !matrix_has_keys {
            self.session
                .set_candidate_prefixes(prefixes.iter().copied().map(PhraseToken::new));
            self.session.discard_composition();
            return Ok(false);
        }
        let prefix_tokens: Vec<PhraseToken> = prefixes
            .iter()
            .map(|&token| PhraseToken::new(token))
            .collect();
        self.session.guess_sentence_with_prefix(&prefix_tokens)
    }

    /// The full reset — `pinyin_reset`/`zhuyin_reset`'s law: the parse
    /// path, the phrase result, and the whole session (input, selection
    /// record, n-best rows, constraint store) all go.
    pub fn full_reset(&mut self) {
        self.reset_parse_state();
        self.anchored_window = None;
        self.phrase_result.clear();
        self.session.reset();
    }

    /// Begin a parse without discarding instance-level constraints or
    /// sentence rows. Every input replaces the matrix, including changed
    /// and empty text; the next guess validates the stored constraints
    /// against it (074a2219, pinyin.cpp:1497-1525, zhuyin.cpp:1017-1043,
    /// lookup/phonetic_lookup.cpp:120-160). The engine separately keeps
    /// its selection record safe for the replaced buffer.
    pub fn begin_parse(&mut self, _original: &[u8]) {
        self.reset_parse_state();
    }
}

/// How the C facade reports a shared training fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrainingRefusal {
    /// The pin asserts or aborts during the training walk.
    Abort,
    /// A user bigram has a partial item tail. The C facade warns and refuses.
    PartialBigram,
}

/// Classifies the shared result of `train_result3`. The ABI keeps any
/// facade-specific index check and performs the terminal action.
///
/// # Errors
///
/// Returns the pin's abort sites or the partial-item refusal. Earlier
/// training steps have already made the same changes when this runs.
pub fn training_answer(result: Result<bool, EngineError>) -> Result<bool, TrainingRefusal> {
    match result {
        Ok(trained) => Ok(trained),
        // 074a2219: phonetic_lookup.h:868 (the forced token), and
        // phonetic_key_matrix.cpp:661, :663, :664 (the current matrix).
        // The predecessor's total and set_freq/store checks are at
        // phonetic_lookup.h:884, :903-904 (register rows 85 and 113).
        Err(
            EngineError::StaleTrainingConstraint { .. }
            | EngineError::StaleTrainingSpan { .. }
            | EngineError::ShortTrainingPredecessor { .. }
            | EngineError::TrainingSetFreqMiss { .. }
            | EngineError::FailedUserBigramStore { .. },
        ) => Err(TrainingRefusal::Abort),
        // ngram.cpp:190, :241, :261 walks a partial item past its end.
        Err(EngineError::UnsafeTrainingPredecessor { .. }) => Err(TrainingRefusal::PartialBigram),
        Err(
            EngineError::TonedIncompleteKey
            | EngineError::ShortLookupPredecessor { .. }
            | EngineError::ReadAfterCheck { .. }
            | EngineError::UnsafeLookupPredecessor { .. }
            | EngineError::CandidateIndexOutOfRange { .. }
            | EngineError::LookupOffsetPastSeparator { .. }
            | EngineError::LookupOffsetOutOfRange { .. }
            | EngineError::LookupOffsetInsideCharacter { .. }
            | EngineError::SelectionAnchorBeforeComposition { .. }
            | EngineError::ZeroKeyOffsetCheck { .. }
            | EngineError::MatrixColumnAssert { .. }
            | EngineError::OverlongUserIndexKey { .. }
            | EngineError::ReduceTokensCount { .. }
            | EngineError::UserModel(_)
            | EngineError::Graph(_)
            | EngineError::Scoring(_),
        ) => Ok(false),
    }
}

/// The lookup faults of a predecessor's user bigram. A value too short for
/// its total is the pin's assert in `get_total_freq`; a Kyoto Cabinet read
/// that fails after its size check also aborts. A read past the value logs
/// the one warning and answers `true`; every other fault answers `false`.
pub fn refuse_lookup_gram(name: &str, error: &EngineError, log: impl FnOnce(&str)) -> bool {
    let message = match error {
        EngineError::TonedIncompleteKey => std::process::abort(),
        EngineError::ShortLookupPredecessor { .. } | EngineError::ReadAfterCheck { .. } => {
            std::process::abort();
        }
        EngineError::UnsafeLookupPredecessor { .. } => {
            "a user bigram value would be read past its end (upstream reads past it)"
        }
        EngineError::CandidateIndexOutOfRange { .. }
        | EngineError::LookupOffsetPastSeparator { .. }
        | EngineError::LookupOffsetOutOfRange { .. }
        | EngineError::LookupOffsetInsideCharacter { .. }
        | EngineError::SelectionAnchorBeforeComposition { .. }
        | EngineError::ZeroKeyOffsetCheck { .. }
        | EngineError::MatrixColumnAssert { .. }
        | EngineError::StaleTrainingConstraint { .. }
        | EngineError::UnsafeTrainingPredecessor { .. }
        | EngineError::ShortTrainingPredecessor { .. }
        | EngineError::TrainingSetFreqMiss { .. }
        | EngineError::FailedUserBigramStore { .. }
        | EngineError::StaleTrainingSpan { .. }
        | EngineError::OverlongUserIndexKey { .. }
        | EngineError::ReduceTokensCount { .. }
        | EngineError::UserModel(_)
        | EngineError::Graph(_)
        | EngineError::Scoring(_) => return false,
    };
    log(&format!("{name}: {message}"));
    true
}
