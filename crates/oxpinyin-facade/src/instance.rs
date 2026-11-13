//! One instance's orchestration state: the composition and live options,
//! the parse-mode state machine, and the snapshot-adjacent fields both
//! facades' C layers borrow against.

use oxpinyin_core::{
    Dictionary, DoublePinyinParse, FullPinyinIndexParse, OptionBits, PhraseText, PhraseToken,
    ZhuyinParse,
};
use oxpinyin_engine::{
    Candidate, CandidateList, EngineError, Selection, SelectionTarget, SentencePart,
    check_lookup_offset_range, normalize_lookup_offset,
};
use oxpinyin_runtime::{Runtime, RuntimeDict, RuntimeLm};
use oxpinyin_user::UserStore;

use crate::context::LiveOptions;

/// The anchor of a **before-cursor** window — the buffer start, not the
/// lookup offset — that [`SelectionSpan::of_candidate`] places its rows'
/// spans from.
///
/// The two window builders measure a candidate's `consumed_bytes` in
/// different coordinates, and [`SelectionTarget::of_candidate`] reads the
/// chosen span as `[anchor, anchor + consumed_bytes)`:
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
/// buffer; resolving the row's index against the composition-anchored
/// cached list instead reads a different list — the caller would commit a
/// row it never displayed.
pub const BEFORE_CURSOR_ANCHOR: usize = 0;

/// A published row's chosen span in the session's store coordinates — the
/// pin's `m_begin` and `m_end` (`lookup_candidate_t`, 074a2219
/// `pinyin.cpp:97-105`), which its choose reads off the row alone
/// (`:2577-2590`, `zhuyin.cpp:1646-1666`). Each snapshot row carries its
/// own, so a choose needs no retained window.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SelectionSpan {
    begin: u32,
    end: u32,
}

impl SelectionSpan {
    /// The span of `candidate` in a window anchored at `anchor`, as
    /// [`SelectionTarget::of_candidate`] places it.
    ///
    /// Both ends are positions in the raw buffer, which a segment-graph
    /// position bounds ([`oxpinyin_core::graph::MAX_GRAPH_POSITION`]), so
    /// the `u32` narrowing never saturates.
    #[must_use]
    pub fn of_candidate(candidate: &Candidate, anchor: usize) -> Self {
        let target = SelectionTarget::of_candidate(candidate, anchor, None);
        Self {
            begin: u32::try_from(target.span_start).unwrap_or(u32::MAX),
            end: u32::try_from(target.span_end).unwrap_or(u32::MAX),
        }
    }

    /// Where the span starts.
    #[must_use]
    pub const fn begin(self) -> usize {
        self.begin as usize
    }

    /// Where the span ends.
    #[must_use]
    pub const fn end(self) -> usize {
        self.end as usize
    }
}

/// What a choose reads off one published row.
#[derive(Clone, Copy, Debug)]
pub struct PublishedRow<'a> {
    /// The row's own span ([`SelectionSpan`]).
    pub span: SelectionSpan,
    /// The row's token as it stands, after any in-place promotion
    /// (`pinyin.cpp:2559-2561`).
    pub token: Option<oxpinyin_core::PhraseToken>,
    /// The n-best tail rank, for a sentence row only.
    pub nbest_row: Option<u8>,
    /// The row's text, stored words included.
    pub text: &'a PhraseText,
}

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
    /// Ends a guess whose rows were published from `window`, each with its
    /// own span: the window goes back to the session for its storage.
    pub fn publish_window(&mut self, window: CandidateList) {
        self.session.recycle_candidates(window);
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
        OptionBits::from_bits(self.live.options.get())
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

    /// The decoded phrases of n-best row `index`, while a lookup is active,
    /// each token resolved through the live phrase index now, as the pin's
    /// `convert_to_utf8` does when the caller reads the sentence
    /// (`src/lookup/lookup.cpp:37-48` at 074a2219).
    ///
    /// The pin ignores the result of `get_phrase_item` (`:43`), and a failed
    /// read leaves the item as it was (`SubPhraseIndex::get_phrase_item`,
    /// `storage/phrase_index.cpp:180-206`). A token that does not resolve
    /// thus repeats the last phrase that resolved in the same read, or adds
    /// "" when none has: the default item has length 0
    /// (`storage/phrase_index.h:81-84`).
    pub fn sentence_phrases(&self, index: u8) -> Option<impl Iterator<Item = PhraseText> + '_> {
        let mut last = PhraseText::default();
        Some(
            self.session
                .sentence_phrases(index)?
                .map(move |part| match part {
                    SentencePart::Token(token) => {
                        if let Some(text) = self.dict.token_text(token.value()) {
                            last.clone_from(&text);
                            text
                        } else {
                            last.clone()
                        }
                    }
                    SentencePart::Raw(text) => text,
                }),
        )
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
    /// The parse-path reset: the composition's parse state goes, the
    /// selection record and the constraint store stay — upstream's
    /// parse-never-touches-constraints rule; the frontend re-sends the
    /// whole buffer every keystroke, so the chosen cursor must survive
    /// the re-parse.
    ///
    /// Candidate snapshots survive parsing in both ABIs, each row with its
    /// own span and token, so a later choose uses the retained row's token
    /// and span rather than an index into the new parse's candidate list
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

    /// Selects the published `row` by its own span, token, rank and text —
    /// the pin's choose reads the row alone (`pinyin.cpp:2577-2590`,
    /// `zhuyin.cpp:1646-1666`), so a repeat choose of the same row, after a
    /// reparse or an accepted choose, forces the same span again. The
    /// caller's choose offset takes no part in it.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] from the engine's selection.
    pub fn select_published(&mut self, row: &PublishedRow<'_>) -> Result<Selection, EngineError> {
        let target = SelectionTarget {
            span_start: row.span.begin(),
            span_end: row.span.end(),
            token: row.token,
            nbest_row: row.nbest_row,
            text: row.text,
        };
        self.session.select_target(&target)
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
