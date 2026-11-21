//! One instance's orchestration state: the composition and live options,
//! the parse-mode state machine, and the snapshot-adjacent fields both
//! facades' C layers borrow against.

use crate::engine::{
    Candidate, CandidateList, EngineError, Selection, SelectionTarget, SentencePart,
    check_lookup_offset_range, normalize_lookup_offset,
};
use crate::runtime::{Runtime, RuntimeDict, RuntimeLm};
use crate::user::UserStore;
use crate::{
    DoublePinyinParse, FullPinyinIndexParse, OptionBits, PhraseText, PhraseToken, ZhuyinParse,
    model::Dictionary,
};

use crate::facade::context::LiveOptions;

/// A window row's span in the session's store coordinates, as a guess
/// places it. The pinyin facade maps its end back to the original
/// coordinates to publish the pin's `m_end` (`lookup_candidate_t`,
/// 074a2219 `pinyin.cpp:97-105`); a choose reads the published bounds
/// ([`PublishedRow`]), not this span.
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
    /// position bounds ([`crate::graph::MAX_GRAPH_POSITION`]), so
    /// the `u32` narrowing never saturates.
    #[must_use]
    pub fn of_candidate(candidate: &Candidate, anchor: usize) -> Self {
        let target = SelectionTarget::of_candidate(candidate, anchor, None);
        Self {
            begin: u32::try_from(target.span_start).unwrap_or(u32::MAX),
            end: u32::try_from(target.span_end).unwrap_or(u32::MAX),
        }
    }

    /// Where the span ends.
    #[must_use]
    pub const fn end(self) -> usize {
        self.end as usize
    }
}

/// What a choose reads off one published row: the pin's `m_begin`,
/// `m_end` and `m_token` (074a2219 `pinyin.cpp:97-105`, `:2577-2590`;
/// `zhuyin.cpp:78-85`, `:1646-1666`). The bounds are the pin's `guint16`
/// fields in the active parse mode's original coordinates, read against
/// the choosing instance's current parse. The row's display text takes no
/// part.
#[derive(Clone, Copy, Debug)]
pub struct PublishedRow {
    /// `m_begin`.
    pub begin: u16,
    /// `m_end`.
    pub end: u16,
    /// The row's token as it stands, after any in-place promotion
    /// (`pinyin.cpp:2559-2561`).
    pub token: crate::PhraseToken,
    /// Where the recorded text comes from.
    pub text: crate::engine::RowText,
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
    pub session: crate::engine::SessionState,
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
    /// (fuzzy alternatives, and for libpinyin full pinyin the resplit and divided
    /// passes). Empty for plain full pinyin, whose scan matrix the session
    /// keeps, and while no key was placed.
    pub parse_matrix: Vec<crate::engine::MatrixKey>,
    /// The layout of the pin's constraint array as the last validation
    /// sized it. Allocation and the full reset store the empty layout.
    validated: ValidatedLayout,
}

/// The pin's `m_constraints` layout: `validate_constraint` sizes it to the
/// matrix columns (`phonetic_lookup.cpp:120-138`) in a guess and a
/// validating choose, and a parse leaves it as it was. A caller's offset
/// indexes it in those columns (`phonetic_lookup.cpp:88-90`), so a clear
/// maps the offset through the keys of the parse that was validated, not
/// the current one.
#[derive(Default)]
struct ValidatedLayout {
    /// The array length, the matrix columns at the last validation.
    columns: usize,
    /// That parse's key images, or `None` when session coordinates were
    /// the original ones.
    images: Option<Vec<KeyImage>>,
    /// Whether the images came from a double-pinyin parse.
    double_pinyin: bool,
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
    pub session: crate::engine::Session<RuntimeDict<'a>, RuntimeLm<'a>, E>,
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
    InstanceView<'a, &'a mut InstanceState, &'a mut crate::engine::SessionState>;
/// A shared operation over retained composition and current phrase storage.
pub type InstanceRead<'a> = InstanceView<'a, &'a InstanceState, &'a crate::engine::SessionState>;

impl<S: std::borrow::Borrow<InstanceState>, E> std::ops::Deref for InstanceView<'_, S, E> {
    type Target = InstanceState;
    fn deref(&self) -> &Self::Target {
        self.state.borrow()
    }
}
impl<S: std::borrow::Borrow<InstanceState>, E: std::borrow::Borrow<crate::engine::SessionState>>
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
    E: std::borrow::BorrowMut<crate::engine::SessionState>,
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
    pub(crate) fn new(session: crate::engine::SessionState, live: LiveOptions) -> Self {
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
                validated: ValidatedLayout::default(),
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

impl<S: std::borrow::Borrow<InstanceState>, E: std::borrow::Borrow<crate::engine::SessionState>>
    InstanceView<'_, S, E>
{
    /// The current live option word.
    #[must_use]
    pub fn options(&self) -> OptionBits {
        self.live.options.get()
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
            .to_vec()
    }

    /// The stored length and text of `token`'s item, as the C token getters
    /// read them ([`crate::runtime::RuntimeDict::token_phrase`]).
    #[must_use]
    pub fn token_phrase(&self, token: u32) -> Option<crate::runtime::TokenPhrase> {
        self.dict.token_phrase(token)
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
    /// The pin ignores the result of `get_phrase_item` (`:45`), and a failed
    /// read leaves the item as it was (`SubPhraseIndex::get_phrase_item`,
    /// `storage/phrase_index.cpp:180-206`). A token that does not resolve
    /// thus repeats the last phrase that resolved in the same read, or adds
    /// "" when none has: the default item has length 0
    /// (`storage/phrase_index.h:81-84`).
    ///
    /// Each part is `None` where `g_ucs4_to_utf8` answers NULL for the
    /// item's words (`src/lookup/lookup.cpp:58`), which `convert_to_utf8` then leaves out.
    pub fn sentence_phrases(
        &self,
        index: u8,
    ) -> Option<impl Iterator<Item = Option<PhraseText>> + '_> {
        let mut last = Some(PhraseText::default());
        Some(
            self.session
                .sentence_phrases(index)?
                .map(move |part| match part {
                    SentencePart::Token(token) => {
                        if let Some(phrase) = self.dict.token_phrase(token) {
                            last.clone_from(&phrase.text);
                            phrase.text
                        } else {
                            last.clone()
                        }
                    }
                    SentencePart::Raw(text) => Some(text),
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
        self.phrase_result.get(index).copied()
    }

    /// Whether training row `index` terminates `pinyin_train` — after the
    /// same directory gate. The probe runs the one preparation production
    /// training runs (`crate::engine::Session::prepare_training`), with
    /// the same simulated store check, and classifies a store failure
    /// exactly as production does: the pin's own aborts (the index past
    /// nonempty results, a stale forced phrase, a stale span) and the
    /// store faults [`training_answer`] classifies [`TrainingRefusal::Abort`]
    /// answer `true`; a partial tail (the warn-and-refuse class (b)) and
    /// an ordinary backend failure answer `false`. The walk stops at the
    /// first failure, so the probe answers for the fault production meets
    /// first. Reads only: the probe never observes the plan and never
    /// marks the store modified.
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub fn train_aborts(&self, index: u8) -> bool {
        if !self.user.has_user_directory() {
            return false;
        }
        let user = self.user;
        let mut check = crate::user::TrainingCheck::default();
        let prepared = self
            .read()
            .session
            .prepare_training(index, &mut |history, token| {
                crate::runtime::training_step_check(user, &mut check, history, token)
            });
        match prepared {
            Err(crate::engine::TrainingPrepFault::PinAbort) => true,
            Err(crate::engine::TrainingPrepFault::Engine(EngineError::UserModel)) => {
                matches!(
                    training_answer(Err(crate::runtime::training_check_error(check.fault()))),
                    Err(TrainingRefusal::Abort)
                )
            }
            Err(crate::engine::TrainingPrepFault::Engine(_)) | Ok(_) => false,
        }
    }

    /// Whether the pin's NBEST choose of row `rank` asserts: a missing
    /// result, or a differing phrase whose constraint write answers zero
    /// (`crate::engine::Session::nbest_choose_aborts`). Reads only.
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub fn nbest_choose_aborts(&self, rank: u8) -> bool {
        self.session.nbest_choose_aborts(rank)
    }

    /// The default facade item's live unigram. Missing sub-indices and slots
    /// answer `None` (074a2219 `phrase_index.h:646-652`, `pinyin.cpp:2821-2834`).
    #[must_use]
    pub fn token_unigram_frequency(&self, token: u32) -> Option<u64> {
        let token = token & (crate::PHRASE_INDEX_LIBRARY_MASK | crate::PHRASE_MASK);
        let nibble = crate::phrase_index_library_index(token);
        match nibble {
            _ if self.dict.is_system_library(nibble) => {
                self.dict.live_items().effective_unigram(token)
            }
            // Any other sub-index the pin holds is a `USER_FILE` row's
            // (`pinyin.cpp:303-321`); one it does not hold has no item.
            _ if self.dict.library_visible(u32::from(nibble)) => {
                self.user.user_unigram_frequency(token)
            }
            _ => None,
        }
    }
}

impl<
    S: std::borrow::BorrowMut<InstanceState>,
    E: std::borrow::BorrowMut<crate::engine::SessionState>,
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

    /// The normal choose of a published `row` on this instance: the pin's
    /// `validate_constraint`, `add_constraint(m_begin, m_end, token)` and
    /// `validate_constraint` (074a2219 `pinyin.cpp:2577-2590`,
    /// `zhuyin.cpp:1646-1666`). The row may come from another instance:
    /// the pin never checks which instance published it, and reads its
    /// bounds against this instance's matrix.
    ///
    /// `add_constraint` refuses an end past the store, which validation
    /// sized to the matrix (`phonetic_lookup.cpp:64-65`): that is
    /// [`Selection::Refused`], with both validations run. Otherwise the
    /// bounds map onto this parse's store coordinates, the caller choosing
    /// the transformed seams its facade can populate. A bound inside a key
    /// names a matrix column no key fills, so the forcing does not survive
    /// the second validation (`crate::engine::Session::choose_unspellable_span`).
    /// The columns are the pin's own (`parse_matrix`), divided and resplit
    /// keys included.
    /// The caller's choose offset takes no part.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] from the engine's selection.
    pub fn select_published(
        &mut self,
        row: &PublishedRow,
        map_double_pinyin: bool,
    ) -> Result<Selection, EngineError> {
        self.note_validation();
        let (begin, end) = (usize::from(row.begin), usize::from(row.end));
        let size = if self.session.selection_capacity() == 0 {
            0
        } else {
            self.parsed_len + 1
        };
        if end > size {
            return self.session.refuse_selection();
        }
        let (start, stop, spells) = match self.abi_key_images(map_double_pinyin) {
            Some(images) => {
                // The pin's own columns (`fill_matrix`, `resplit_step`,
                // `inner_split_step`; `phonetic_key_matrix.cpp:27-200`), in
                // original coordinates, divided and resplit keys included.
                // A `'` between two keys holds a zero key (`:67-79`), which a
                // forcing may start on too.
                let starts_a_key = self
                    .parse_matrix
                    .iter()
                    .any(|key| key.syllable_start() == begin)
                    || images
                        .windows(2)
                        .any(|pair| pair[0].begin_end.1 <= begin && begin < pair[1].begin_end.0);
                let ends_a_key = self.parse_matrix.iter().any(|key| key.end() == end);
                (
                    session_bound(&images, begin, false),
                    session_bound(&images, end, true),
                    starts_a_key && ends_a_key,
                )
            }
            None => (begin, end, true),
        };
        if spells {
            self.session.select_row(start, stop, row.token, row.text)
        } else {
            self.session.choose_unspellable_span(start, stop)
        }
    }

    /// The base keys of the active transformed parse, each with its
    /// original span and its image in session coordinates, or `None` for
    /// plain and index-parsed full pinyin, whose session coordinates are
    /// the original ones.
    fn abi_key_images(&self, map_double_pinyin: bool) -> Option<Vec<KeyImage>> {
        if let Some(parse) = self.zhuyin_parse.as_ref() {
            return Some(key_images(
                parse
                    .keys()
                    .iter()
                    .map(|item| (item.start(), item.end(), item.key().text().len())),
            ));
        }
        if map_double_pinyin && let Some(parse) = self.double_parse.as_ref() {
            return Some(key_images(
                parse
                    .keys()
                    .iter()
                    .map(|item| (item.start(), item.end(), item.key().text().len())),
            ));
        }
        None
    }

    /// Builds one bound mapper for a publication pass. It owns the base
    /// key images, so every candidate reuses the same allocation.
    /// The original-coordinate bound a published row carries for the
    /// session position `offset`, the inverse of the mapping
    /// [`Self::select_published`] applies: a key's column maps to its
    /// original start, a key's text end to its original end, and a
    /// position inside a key, such as a divided or resplit column
    /// (`phonetic_key_matrix.cpp:150-200`), to the same distance into the
    /// key's original span.
    pub fn publication_bound_mapper(
        &self,
        map_double_pinyin: bool,
    ) -> impl Fn(usize) -> usize + use<S, E> {
        let images = self.abi_key_images(map_double_pinyin);
        move |offset| {
            images
                .as_ref()
                .map_or(offset, |images| published_bound(images, offset))
        }
    }

    /// Whether this instance's decoded results hold rank `rank` and row 0,
    /// which the pin's NBEST choose asserts (074a2219 `pinyin.cpp:2517-2518`).
    #[must_use]
    pub fn has_nbest_rank(&self, rank: u8) -> bool {
        self.session.has_nbest_rank(rank)
    }

    /// The pin's NBEST choose on this instance: its own results at `rank`
    /// (074a2219 `pinyin.cpp:2515-2520`). The caller checks
    /// [`Self::has_nbest_rank`] first.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] from the engine's selection.
    pub fn select_nbest_rank(&mut self, rank: u8) -> Result<Selection, EngineError> {
        self.session.select_nbest_row(rank)
    }

    /// Clears the constraint at `offset`, a column of the pin's constraint
    /// array as the last validation laid it out. An offset at or past that
    /// array's length refuses (`phonetic_lookup.cpp:88-90`). Any other
    /// offset maps through that validation's keys to session coordinates.
    /// The caller selects the transformed seams its facade can populate.
    pub fn clear_abi_constraint(&mut self, offset: usize, map_double_pinyin: bool) -> bool {
        let layout = &self.validated;
        if offset >= layout.columns {
            return false;
        }
        let session_offset = layout
            .images
            .as_deref()
            .filter(|_| map_double_pinyin || !layout.double_pinyin)
            .map_or(offset, |images| session_bound(images, offset, false));
        self.session.clear_constraint(session_offset)
    }

    /// Runs before each pin entry that calls `validate_constraint`: the
    /// guesses and the validating chooses. The pin keeps each forcing at
    /// its matrix columns across a reparse, while the session string spells
    /// each key canonically, so a reparse that respells an earlier key moves
    /// the session positions of the keys after it. Each forcing therefore
    /// maps back to its columns through the recorded layout and forward
    /// through the current parse. A span that overruns the new matrix is
    /// dropped (`phonetic_lookup.cpp:153-156`). A bound inside a key maps
    /// inside the key's text, where no column is filled, so validation drops
    /// it as the pin does (`phonetic_key_matrix.cpp:592-595`). The layout
    /// records the bounds for subsequent clears.
    fn note_validation(&mut self) {
        let images = self.abi_key_images(true);
        let columns = self.pin_matrix_columns();
        if self.validated.images != images {
            let old = self.validated.images.take();
            let new = images.as_deref();
            let original = |offset| {
                old.as_deref()
                    .map_or(offset, |old| published_bound(old, offset))
            };
            let start_at = |column| new.map_or(column, |new| session_bound(new, column, false));
            // The end slot after the last key is the session's own last
            // column.
            let end_at = |column| match new {
                Some(new) if column >= columns => new.last().map_or(0, |key| key.text_end + 1),
                Some(new) => session_end(new, column),
                None => column,
            };
            self.session.remap_constraints(
                |start, end| {
                    let (start, end) = (original(start), original(end));
                    (end < columns).then(|| (start_at(start), end_at(end)))
                },
                |span| {
                    let (start, end) = (original(span.start), original(span.end).min(columns));
                    start_at(start)..end_at(end)
                },
            );
        }
        self.validated = ValidatedLayout {
            columns,
            double_pinyin: images.is_some() && self.zhuyin_parse.is_none(),
            images,
        };
    }

    /// A choose that validates and writes no forcing: the pin's two
    /// `validate_constraint` calls around a refused or absent write.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] from constraint validation.
    pub fn refuse_selection(&mut self) -> Result<Selection, EngineError> {
        self.note_validation();
        self.session.refuse_selection()
    }

    /// Validates a tokenless choice against the receiving instance's parse.
    ///
    /// # Errors
    /// Returns a lookup error from constraint validation.
    pub fn choose_tokenless(&mut self) -> Result<(), EngineError> {
        self.note_validation();
        self.session.choose_tokenless()
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
        self.note_validation();
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
        self.note_validation();
        if !matrix_has_keys {
            self.session
                .set_candidate_prefixes(prefixes.iter().copied());
            self.session.discard_composition();
            return Ok(false);
        }
        let prefix_tokens: Vec<PhraseToken> = prefixes.to_vec();
        self.session.guess_sentence_with_prefix(&prefix_tokens)
    }

    /// The full reset — `pinyin_reset`/`zhuyin_reset`'s law: the parse
    /// path, the phrase result, and the whole session (input, selection
    /// record, n-best rows, constraint store) all go.
    pub fn full_reset(&mut self) {
        self.reset_parse_state();
        self.phrase_result.clear();
        self.validated = ValidatedLayout::default();
        self.session.reset();
    }

    // pinyin.cpp:1497–1525; zhuyin.cpp:1017–1043,1058–1081; lookup/phonetic_lookup.cpp:88–165.
    /// Clears this instance’s stored parse state before every parse, including empty or
    /// changed input. Constraints and sentence rows survive, and a clear after the parse
    /// still indexes the constraint columns of the last validation. The next validation,
    /// which a guess or a validating choose runs, checks the constraints against the
    /// replacement matrix.
    pub fn begin_parse(&mut self, _original: &[u8]) {
        self.reset_parse_state();
    }
}

/// How the C facade reports a shared training fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrainingRefusal {
    /// The pin asserts during a training step's store operations.
    Abort,
    /// A user bigram has a partial item tail. The C facade warns and refuses.
    PartialBigram,
}

/// Classifies the shared result of `train_result3`. The engine terminates
/// at the pin's own training aborts (`Runtime::train_session`); the ABI
/// performs the terminal action for the store faults classified here.
///
/// # Errors
///
/// Returns the store's abort sites or the partial-item refusal. Earlier
/// training steps have already made the same changes when this runs.
pub fn training_answer(result: Result<bool, EngineError>) -> Result<bool, TrainingRefusal> {
    match result {
        Ok(trained) => Ok(trained),
        // The predecessor's total and set_freq/store checks are at
        // phonetic_lookup.h:884, :903-904 (register rows 85 and 113).
        Err(EngineError::TrainingAbort) => Err(TrainingRefusal::Abort),
        // ngram.cpp:190, :241, :261 walks a partial item past its end.
        Err(EngineError::UnsafeTrainingPredecessor) => Err(TrainingRefusal::PartialBigram),
        Err(
            EngineError::ReadAfterCheck
            | EngineError::UnsafeLookupPredecessor
            | EngineError::LookupOffsetOutOfRange { .. }
            | EngineError::Failure
            | EngineError::ZeroKeyOffsetCheck
            | EngineError::PinAssert
            | EngineError::UserModel,
        ) => Ok(false),
    }
}

/// Returns the plain or prefixed sentence guess. On a lookup error, apply
/// the facade's diagnostic or abort, then return false if control returns.
pub fn guess_result(result: Result<bool, EngineError>, name: &str, log: impl FnOnce(&str)) -> bool {
    match result {
        Ok(found) => found,
        Err(error) => {
            refuse_lookup_gram(name, &error, log);
            false
        }
    }
}

// The engine terminates where the pin asserts an incomplete toned key
// (pinyin_phrase3.h:146–157) or a bigram value shorter than its total, system or user
// (ngram.cpp:303–304, through memory_chunk.h:387–392).
// A failed Kyoto Cabinet body read after its size check asserts at ngram_kyotodb.cpp:131–139.
/// Handles a lookup failure at the C boundary. A read past the end of a predecessor’s
/// stored bigram value, from the system or the user gram, logs one warning and returns
/// `true`. Returns `false` for other recoverable errors.
///
/// # Aborts
///
/// Terminates when called with [`EngineError::ReadAfterCheck`], a Kyoto Cabinet
/// bigram read that fails after its size check. The engine aborts at the lookup
/// site for a toned incomplete key or when the lookup needs to read a total
/// from a predecessor value too short to hold it.
pub fn refuse_lookup_gram(name: &str, error: &EngineError, log: impl FnOnce(&str)) -> bool {
    let message = match error {
        EngineError::ReadAfterCheck => {
            std::process::abort();
        }
        EngineError::UnsafeLookupPredecessor => {
            "a bigram value would be read past its end (upstream reads past it)"
        }
        EngineError::LookupOffsetOutOfRange { .. }
        | EngineError::Failure
        | EngineError::ZeroKeyOffsetCheck
        | EngineError::PinAssert
        | EngineError::UnsafeTrainingPredecessor
        | EngineError::TrainingAbort
        | EngineError::UserModel => return false,
    };
    log(&format!("{name}: {message}"));
    true
}

/// One base key of a transformed parse: its original span, the session
/// column its forcing starts at (0, or the `'` byte in front of it) and
/// its canonical text's session span.
#[derive(PartialEq, Eq)]
struct KeyImage {
    begin_end: (usize, usize),
    column: usize,
    text_start: usize,
    text_end: usize,
}

/// The images of keys given as `(original start, original end, canonical
/// text length)` in the `'`-joined session string.
fn key_images(keys: impl Iterator<Item = (usize, usize, usize)>) -> Vec<KeyImage> {
    let mut images = Vec::new();
    let mut text_start = 0;
    for (begin, end, len) in keys {
        let column = if images.is_empty() { 0 } else { text_start - 1 };
        images.push(KeyImage {
            begin_end: (begin, end),
            column,
            text_start,
            text_end: text_start + len,
        });
        text_start += len + 1; // the apostrophe between keys
    }
    images
}

/// The session position of the original bound `offset`. A key's start
/// maps to its column and a key's end to its text end. A position inside
/// a key maps the same distance into its text: the pin places a divided
/// or resplit column at the key's start plus the new key's spelling
/// length (`phonetic_key_matrix.cpp:150-200`), and the session string
/// spells the key canonically. It stays inside the key's text, so a bound
/// no column holds maps to no column either.
fn session_bound(images: &[KeyImage], offset: usize, is_end: bool) -> usize {
    let exact = if is_end {
        images
            .iter()
            .find(|image| image.begin_end.1 == offset)
            .map(|image| image.text_end)
    } else {
        images
            .iter()
            .find(|image| image.begin_end.0 == offset)
            .map(|image| image.column)
    };
    if let Some(position) = exact {
        return position;
    }
    if let Some(image) = images
        .iter()
        .find(|image| image.begin_end.0 < offset && offset < image.begin_end.1)
    {
        let inside = image.text_start + (offset - image.begin_end.0);
        return inside.clamp(
            image.text_start + 1,
            image.text_end.saturating_sub(1).max(image.text_start + 1),
        );
    }
    if offset == 0 {
        return 0;
    }
    images
        .iter()
        .find(|image| image.begin_end.0 > offset)
        .map_or_else(
            || images.last().map_or(0, |image| image.text_end),
            |image| image.column,
        )
}

/// The session position of an original span end. A key's end maps to its
/// text end. An end at the start of a key that follows a `'` maps to that
/// key's column, the `'` in front of it, so the span covers the same keys.
/// Otherwise it maps as [`session_bound`] maps an end.
fn session_end(images: &[KeyImage], offset: usize) -> usize {
    if let Some(image) = images.iter().find(|image| image.begin_end.1 == offset) {
        return image.text_end;
    }
    images
        .iter()
        .find(|image| image.begin_end.0 == offset)
        .map_or_else(|| session_bound(images, offset, true), |image| image.column)
}

/// Map one session bound through the publication pass's base key images.
fn published_bound(images: &[KeyImage], offset: usize) -> usize {
    if let Some(image) = images.iter().find(|image| image.text_end == offset) {
        return image.begin_end.1;
    }
    if let Some(image) = images.iter().find(|image| image.column == offset) {
        return image.begin_end.0;
    }
    if let Some(image) = images
        .iter()
        .find(|image| image.text_start < offset && offset < image.text_end)
    {
        let (begin, end) = image.begin_end;
        return (begin + (offset - image.text_start)).min(end.saturating_sub(1).max(begin));
    }
    images.last().map_or(offset, |image| image.begin_end.1)
}

#[cfg(test)]
mod tests;
