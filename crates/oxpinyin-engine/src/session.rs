//! The session state machine.
//!
//! One session per input context. Nothing here is `Send` or `Sync` by
//! requirement, because the TSF, IMK and ArkTS models all want a
//! main-thread-friendly, instance-per-context object.
//!
//! The decoder behind it is parse -> graph -> k-best -> lookup, wired in at
//! W4-T4 **behind the signatures** `docs/findings/session-api.md` froze at
//! W4-T0. Not one of them changed.

use core::fmt::Display;
use std::collections::HashSet;

use smallvec::SmallVec;

use oxpinyin_core::graph::{Edge, EdgeKind, SegmentGraph};
use oxpinyin_core::kbest::{DecodedPath, k_best};
use oxpinyin_core::scoring::{Scorer, ScoringConfig, ScoringError, expand_keys, key_cost_table};
use oxpinyin_core::{
    Completeness, Cost, Dictionary, LanguageModel, OptionBits, PhraseEntry, PhraseToken,
    SyllableKey, UserModel,
};

use crate::candidate::{Candidate, CandidateKind, CandidateList};
use crate::config::ConfigSource;
use crate::error::EngineError;
use crate::key::{KeyInput, LogicalKey};
use crate::preedit::{Preedit, PreeditSpan, SpanStyle};
use crate::storage::StoragePaths;

/// Largest raw input a session accepts, in bytes.
///
/// Matches the largest input the frozen F-A fixtures and the parity corpus
/// carry. Typing past it is reported as [`KeyOutcome::Ignored`]: refusing more
/// input is a state, not a failure.
pub const MAX_INPUT_BYTES: usize = 4_096;

/// Configuration key for the candidate page size.
const KEY_PAGE_SIZE: &str = "lookup-table-page-size";

/// Page size used when the configuration source does not carry the key.
const DEFAULT_PAGE_SIZE: usize = 5;

/// Configuration key for whether initial-only keys are admitted.
const KEY_INCOMPLETE: &str = "incomplete-pinyin";

/// How many segmentations the decoder keeps.
///
/// The pin's own candidate lists mix segmentations — `xian` opens with
/// `西安` (`xi` + `an`) while its selected path is the single key `xian` — so
/// one segmentation is not enough to reproduce a candidate list.
const SEGMENTATION_K: usize = 8;

/// Longest phrase, in keys, the sentence builder will look back for.
const MAX_PHRASE_KEYS: usize = 8;

/// Longest key sequence the window scan searches: the pin's phrase-length
/// cap. Paths beyond it are not searched.
pub(crate) const MAX_PHRASE_LENGTH: usize = 16;

/// The window scan's own expansion bound, separate from
/// [`oxpinyin_core::scoring::ScoringConfig::expansion_limit`] which the
/// pre-frequency fallback shares. Measured over the W2 corpus, the largest
/// expansion that hits real phrases is a three-initial `q|q|q` span
/// (14^3 = 2_744 — the pin's `qqq…` offers `请求权`); 4_096 covers it with
/// headroom. Larger products yield nothing: no stored phrase matches a
/// longer all-initial span.
pub(crate) const SCAN_EXPANSION_LIMIT: usize = 4_096;

/// What a session did with a key.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum KeyOutcome {
    /// The session did not use the key and is unchanged.
    Ignored,
    /// The session used the key.
    Consumed,
    /// The session used the key and finished a composition.
    Commit(String),
}

/// What choosing a candidate left behind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum Selection {
    /// Input remains; more candidates are offered.
    Continued,
    /// The whole composition is chosen and can be committed.
    Completed,
}

/// Settings a session reads once, at construction.
#[derive(Clone, Copy, Debug)]
struct Settings {
    page_size: usize,
    options: OptionBits,
}

impl Settings {
    fn read(config: &dyn ConfigSource) -> Self {
        let page_size = config
            .get_int(KEY_PAGE_SIZE)
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_PAGE_SIZE);
        // The captured parity profile has PINYIN_INCOMPLETE set, and the
        // upstream default this engine carries is true; a source that says
        // nothing gets the parity behaviour. Other option bits arrive through
        // [`Session::set_options`] from the C ABI's raw option word.
        let incomplete = config.get_bool(KEY_INCOMPLETE).unwrap_or(true);
        let options = OptionBits::default().with(oxpinyin_core::PINYIN_INCOMPLETE, incomplete);
        Self { page_size, options }
    }

    /// Whether the `PINYIN_INCOMPLETE` bit is set.
    const fn incomplete(self) -> bool {
        self.options.has_incomplete()
    }
}

/// One input context.
///
/// Swapping fixture adapters for table-backed loaders is a change of `D` and
/// `L` and nothing else.
///
/// **Keep in sync with `docs/findings/session-api.md`.** That SPEC's
/// "deliberately absent" list — no keysyms, no GSettings, no path discovery,
/// no `cfg(target_os)`, no threading or clock contract — is the freeze this
/// type implements, and later findings add cross-references to it
/// (`config-layering.md` for where configuration actually comes from,
/// `session-replay.md` for what consumes the seam). A change here that admits
/// one of those must amend the SPEC's list in the same commit, or the list
/// silently stops describing the code.
#[derive(Clone, Debug)]
pub struct Session<D, L> {
    dictionary: D,
    model: L,
    paths: StoragePaths,
    settings: Settings,
    raw: String,
    selected: String,
    consumed: usize,
    /// Filtered parse length of the remaining input, from the last refresh.
    parsed_prefix: usize,
    candidates: CandidateList,
    history: Vec<PhraseToken>,
    scoring: ScoringConfig,
    key_costs: Vec<Cost>,
    /// Decoded n-best sentence rows, filled by [`Session::guess_sentence`]
    /// and cleared by [`Session::reset`] — the `m_nbest_results` gate
    /// (`docs/findings/sentence-surface.md` §1). Empty means no sentence
    /// has been guessed for the current composition.
    nbest_rows: Vec<crate::nbest::NbestRow>,
    /// History snapshot taken beside [`Self::nbest_rows`] when the lookup
    /// decoded them — the seed context the rows were decoded against.
    /// Selecting an n-best row restores it before the row's tokens extend
    /// the record, so a normal selection made between the lookup and the
    /// row choice leaves no stale token behind — the record-side half of
    /// the text assign (`docs/findings/sentence-surface.md` §10). Cleared
    /// wherever `nbest_rows` is.
    nbest_history: Vec<PhraseToken>,
    /// Whether a sentence lookup has run for the current composition —
    /// the half of the `m_nbest_results` gate an empty-but-active lookup
    /// still satisfies: upstream's `pinyin_guess_sentence` clears the
    /// results and attempts the search even on an empty key matrix, so a
    /// later `pinyin_get_sentence` must answer false rather than fall
    /// back to the pre-lookup raw form.
    sentence_lookup_active: bool,
    /// Reused across keystrokes: the scan's candidate buffer.
    scratch_collected: Vec<Candidate>,
    /// Reused Schwartzian buffer for the three-key order.
    scratch_ranked: Vec<(RankKey, Candidate)>,
    /// Reused dictionary-hit buffer for one window-scan lookup.
    scratch_entries: Vec<PhraseEntry>,
    /// Reused scan path (phrase length ≤ 16).
    scratch_path: SmallVec<[SyllableKey; 16]>,
    /// Reused per-window scan batch, default facade.
    scratch_window_phrase: Vec<Candidate>,
    /// Reused per-window scan batch, addon facade.
    scratch_window_addon: Vec<Candidate>,
}

impl<D, L> Session<D, L>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Opens a session over the supplied backends.
    ///
    /// Configuration and storage locations arrive as data; the session reads
    /// no environment and discovers no path.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend rejects the settings it is
    /// opened with. No such rejection exists yet, so this currently always
    /// succeeds.
    pub fn new(
        config: &dyn ConfigSource,
        paths: StoragePaths,
        dictionary: D,
        model: L,
    ) -> Result<Self, EngineError> {
        let key_costs = key_cost_table(&dictionary, &model).map_err(EngineError::Scoring)?;
        Ok(Self {
            dictionary,
            model,
            paths,
            settings: Settings::read(config),
            raw: String::new(),
            selected: String::new(),
            consumed: 0,
            parsed_prefix: 0,
            candidates: CandidateList::default(),
            history: Vec::new(),
            scoring: ScoringConfig::default(),
            key_costs,
            nbest_rows: Vec::new(),
            nbest_history: Vec::new(),
            sentence_lookup_active: false,
            scratch_collected: Vec::new(),
            scratch_ranked: Vec::new(),
            scratch_entries: Vec::new(),
            scratch_path: SmallVec::new(),
            scratch_window_phrase: Vec::new(),
            scratch_window_addon: Vec::new(),
        })
    }

    /// Feeds one key press to the session.
    ///
    /// Characters the parser has syntax for — ASCII lowercase and the
    /// apostrophe — extend the composition. `Backspace` removes the last one,
    /// or undoes a selection when nothing else remains. `Escape` clears the
    /// composition. `Enter` commits it. `Space` chooses the first candidate,
    /// or commits when there is none. Every other key, and any key held with a
    /// command modifier, is [`KeyOutcome::Ignored`] and changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when refreshing candidates hits a backend
    /// failure.
    pub fn process_key(&mut self, input: &KeyInput) -> Result<KeyOutcome, EngineError> {
        if input.modifiers().has_command_modifier() {
            return Ok(KeyOutcome::Ignored);
        }

        match input.key() {
            LogicalKey::Character(character) => self.type_character(character),
            LogicalKey::Backspace => self.erase(),
            LogicalKey::Escape => {
                if self.is_composing() {
                    self.reset();
                    Ok(KeyOutcome::Consumed)
                } else {
                    Ok(KeyOutcome::Ignored)
                }
            }
            LogicalKey::Enter => {
                if self.is_composing() {
                    Ok(KeyOutcome::Commit(self.commit()?))
                } else {
                    Ok(KeyOutcome::Ignored)
                }
            }
            LogicalKey::Space => self.accept_first(),
            _ => Ok(KeyOutcome::Ignored),
        }
    }

    /// Replaces the scoring weights used by subsequent refreshes.
    ///
    /// Does not recompute the per-key cost table (that depends only on the
    /// dictionary and language model). Intended for constant sweeps and
    /// measurements; interactive shells normally keep [`ScoringConfig::default`].
    pub fn set_scoring_config(&mut self, config: ScoringConfig) {
        self.scoring = config;
    }

    /// The scoring weights currently in force.
    #[must_use]
    pub const fn scoring_config(&self) -> &ScoringConfig {
        &self.scoring
    }

    /// Types a run of characters and refreshes candidates once.
    ///
    /// For the final composition state this is equivalent to calling
    /// [`Session::process_key`] once per character when no selection
    /// intervenes **and** every character is parser syntax (`a`–`z` / `'`),
    /// but without recomputing candidates after every keystroke. Batch
    /// differential runs use this; interactive shells should keep calling
    /// [`Session::process_key`] so intermediate candidate lists update.
    ///
    /// Unlike [`Session::process_key`], this accepts every printable ASCII
    /// character (`0x21..=0x7E`). Non-`a-z`/`'` bytes stay in the raw buffer so
    /// the decoder sees the same junk-bearing strings the oracle fixture
    /// carries; the segment graph stops at those bytes as hard boundaries.
    /// Space and non-ASCII are skipped. Typing past [`MAX_INPUT_BYTES`] stops
    /// accepting further characters.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when refreshing candidates hits a backend
    /// failure.
    pub fn type_pinyin(&mut self, text: &str) -> Result<KeyOutcome, EngineError> {
        let before = self.raw.len();
        for character in text.chars() {
            if !is_batch_input_character(character) {
                continue;
            }
            if self.raw.len() + character.len_utf8() > MAX_INPUT_BYTES {
                break;
            }
            self.raw.push(character);
        }
        if self.raw.len() == before {
            return Ok(KeyOutcome::Ignored);
        }
        self.refresh()?;
        Ok(KeyOutcome::Consumed)
    }

    /// Chooses the candidate at `index`.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::CandidateIndexOutOfRange`] for an index the
    /// current list does not hold — including a stale index left over from an
    /// earlier list — and leaves the session usable.
    pub fn select(&mut self, index: usize) -> Result<Selection, EngineError> {
        self.select_inner(index, None)
    }

    /// Chooses the candidate at `index`, recording `promoted_token` in the
    /// sentence history in place of the candidate's own token.
    ///
    /// The addon-promotion path (`pinyin.cpp:2532-2561`,
    /// `docs/findings/addon-choose-promotion.md`): a chosen `ADDON_CANDIDATE`
    /// becomes a `NORMAL_CANDIDATE` at a freshly allocated default-facade
    /// nibble-5 token, and it is that promoted token the constraint — and a
    /// later `pinyin_train` — records, not the addon-facade token.
    ///
    /// # Errors
    ///
    /// Same as [`Session::select`].
    pub fn select_promoted(
        &mut self,
        index: usize,
        promoted_token: PhraseToken,
    ) -> Result<Selection, EngineError> {
        self.select_inner(index, Some(promoted_token))
    }

    fn select_inner(
        &mut self,
        index: usize,
        token_override: Option<PhraseToken>,
    ) -> Result<Selection, EngineError> {
        let Some(candidate) = self.candidates.get(index) else {
            return Err(EngineError::CandidateIndexOutOfRange {
                index,
                len: self.candidates.len(),
            });
        };

        let text = candidate.text().to_owned();
        let advance = candidate.consumed_bytes();
        let token = token_override.or_else(|| candidate.token());
        if candidate.nbest_row().is_some() {
            self.selected = text;
        } else {
            self.selected.push_str(&text);
        }
        if let Some(token) = token {
            self.history.push(token);
        } else if let Some(rank) = candidate.nbest_row() {
            // A prepended n-best row records its whole token path —
            // upstream's `pinyin_choose_candidate` keeps the chosen
            // `MatchResult` on the instance and `pinyin_train` walks it;
            // the engine's record is the token history
            // (`docs/findings/user-store.md` §2.1). The row is looked up
            // by its own tail rank, never by list position: the NBEST-wins
            // dedup can drop an earlier duplicate row, shifting a
            // surviving row off the position its rank would give it, and a
            // positional lookup then trains the wrong path. A fallback
            // sentence candidate carries no rank and no tokens; it records
            // nothing, exactly as before.
            if let Some(row) = self.nbest_rows.get(usize::from(rank)) {
                // The row replaces everything decoded since the lookup ran
                // — the text side of that replace is the assign above. A
                // normal selection made in between must leave no token in
                // the record either, so restore the snapshot the rows were
                // decoded against before extending with this row's path.
                self.history.clone_from(&self.nbest_history);
                self.history.extend(row.tokens.iter().copied());
            }
        }
        self.consumed = self.next_boundary(self.consumed.saturating_add(advance));
        self.refresh()?;

        if self.consumed >= self.raw.len() {
            Ok(Selection::Completed)
        } else {
            Ok(Selection::Continued)
        }
    }

    /// Trains the recorded selection through the user-model seam.
    ///
    /// Walks the sentence recorded so far — the tokens of every phrase the
    /// user pinned through [`Session::select`], in order — and calls
    /// [`UserModel::observe`] once per token with the preceding tokens as
    /// history. The first token therefore observes against an empty history,
    /// which the pinned store maps to `sentence_start`
    /// (`docs/findings/user-store.md` §2.1). The C ABI's `pinyin_train` is
    /// this call: per-candidate selection ([`Session::select`]) only records
    /// the constraint, and the bigram update is deferred to here (§2.2).
    /// Learning-off callers omit it entirely.
    ///
    /// Re-calling without new selections re-observes the same sentence, which
    /// is the upstream behaviour (a second `pinyin_train` doubles the counts —
    /// there is no guard upstream either).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::UserModel`] when the user model rejects an
    /// observation. Tokens observed before the failure stay observed: the
    /// sentence is trained prefix-wise, like the upstream loop.
    pub fn train<U>(&self, user: &mut U) -> Result<(), EngineError>
    where
        U: UserModel<Token = PhraseToken>,
        U::Error: Display,
    {
        for (index, token) in self.history.iter().enumerate() {
            user.observe(&self.history[..index], token)
                .map_err(|error| EngineError::UserModel(error.to_string()))?;
        }
        Ok(())
    }

    /// The sentence recorded so far: the token of every phrase the user
    /// pinned in this composition, in selection order.
    ///
    /// Sentence-level candidates carry no token and are not part of the
    /// record — exactly the phrases a `pinyin_train` call would train
    /// (`docs/findings/user-store.md` §2.1). The C ABI uses the tail of this
    /// slice as the predecessor for predicted-candidate training (§2.3).
    #[must_use]
    pub fn selected_tokens(&self) -> &[PhraseToken] {
        &self.history
    }

    /// The current composition's syllable keys, in the engine's selected
    /// parse order.
    ///
    /// This is the fewest-keys segmentation the scan matrix is built from
    /// ([`SegmentGraph::fewest_keys`], `docs/findings/candidate-construction.md`
    /// §8.1) over the whole raw buffer — the standing-in for libpinyin's saved
    /// keys, which `pinyin_remember_user_input` walks to store a phrase with
    /// its pinyin (`docs/findings/user-store.md` §3.1).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::Graph`] when the raw buffer cannot be built
    /// into a segment graph (an over-long input; the buffer is capped by
    /// [`MAX_INPUT_BYTES`]).
    pub fn composition_keys(&self) -> Result<Vec<SyllableKey>, EngineError> {
        let graph = SegmentGraph::build_with_options(self.raw.as_bytes(), self.settings.options)
            .map_err(EngineError::Graph)?;
        Ok(graph
            .fewest_keys(self.settings.incomplete())
            .into_iter()
            .map(|edge| edge.key())
            .collect())
    }

    /// Finishes the composition and returns its text.
    ///
    /// Never fails on an empty composition: the text is then empty too.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails while the session resets.
    pub fn commit(&mut self) -> Result<String, EngineError> {
        let mut text = core::mem::take(&mut self.selected);
        text.push_str(&self.raw[self.consumed..]);
        self.reset();
        Ok(text)
    }

    /// Discards the composition.
    pub fn reset(&mut self) {
        self.raw.clear();
        self.selected.clear();
        self.consumed = 0;
        self.parsed_prefix = 0;
        self.candidates = CandidateList::default();
        self.history.clear();
        self.nbest_rows.clear();
        self.nbest_history.clear();
        self.sentence_lookup_active = false;
    }

    /// Filtered parse length of the remaining input after the last refresh.
    ///
    /// This is the last byte of [`SegmentGraph::fewest_keys`] under the
    /// session's `incomplete-pinyin` setting — not the unfiltered
    /// [`SegmentGraph::consumed`].
    #[must_use]
    pub const fn parsed_prefix_len(&self) -> usize {
        self.parsed_prefix
    }

    /// Apply a live `incomplete-pinyin` change and refresh if composing.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a composing session fails to refresh
    /// under the new setting.
    pub fn set_incomplete_pinyin(&mut self, enabled: bool) -> Result<(), EngineError> {
        let options = self
            .settings
            .options
            .with(oxpinyin_core::PINYIN_INCOMPLETE, enabled);
        self.set_options(options)
    }

    /// Apply a live option-word change and refresh if composing.
    ///
    /// This is the engine half of `pinyin_set_options`: correction and
    /// ambiguity bits remask already-allocated sessions on the next parse or
    /// guess. The C ABI stores the raw word and calls this before parse/guess.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a composing session fails to refresh
    /// under the new options.
    pub fn set_options(&mut self, options: OptionBits) -> Result<(), EngineError> {
        if self.settings.options == options {
            return Ok(());
        }
        self.settings.options = options;
        if self.raw.is_empty() {
            return Ok(());
        }
        self.refresh()
    }

    /// What the shell should display.
    #[must_use]
    pub fn preedit(&self) -> Preedit {
        let remaining = &self.raw[self.consumed..];
        if self.selected.is_empty() && remaining.is_empty() {
            return Preedit::default();
        }

        let mut text = self.selected.clone();
        text.push_str(remaining);

        let mut spans = Vec::with_capacity(2);
        if !self.selected.is_empty() {
            spans.push(PreeditSpan::new(
                0,
                self.selected.len(),
                SpanStyle::Selected,
            ));
        }
        if !remaining.is_empty() {
            spans.push(PreeditSpan::new(
                self.selected.len(),
                text.len(),
                SpanStyle::Raw,
            ));
        }

        let cursor = text.len();
        Preedit::new(text, spans, cursor)
    }

    /// The current candidates, in rank order.
    ///
    /// Sentence rows appear at the head of this list only after
    /// [`Session::guess_sentence`] has run for the current composition:
    /// upstream's candidate list prepends its n-best rows exactly when
    /// `m_nbest_results` is non-empty (`pinyin.cpp:2292-2293`), and the
    /// corpus pins were captured without a sentence guess.
    #[must_use]
    pub const fn candidates(&self) -> &CandidateList {
        &self.candidates
    }

    /// Runs the n-best sentence lookup and stores its rows
    /// (`pinyin_guess_sentence`, `pinyin.cpp:1373-1385`).
    ///
    /// With real unigrams this is the trellis port of upstream's
    /// `PhoneticLookup<2, 3>` ([`crate::nbest`]); without them the
    /// pre-frequency per-path DP supplies up to three rows so the surface
    /// exists for every model. Rows survive further typing and selections
    /// until the next [`Session::guess_sentence`] or [`Session::reset`] —
    /// upstream's `m_nbest_results` is cleared nowhere else — and
    /// [`Session::candidates`] prepends them while they live.
    ///
    /// Returns whether a lookup ran at all (upstream returns the lookup's
    /// `false` only for an empty key matrix; zero rows is still `true`).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails during the lookup.
    pub fn guess_sentence(&mut self) -> Result<bool, EngineError> {
        let remaining = &self.raw[self.consumed..];
        self.nbest_rows.clear();
        self.nbest_history.clear();
        self.sentence_lookup_active = true;
        if remaining.is_empty() {
            return Ok(false);
        }

        let graph = SegmentGraph::build_with_options(remaining.as_bytes(), self.settings.options)
            .map_err(EngineError::Graph)?;
        let bound = graph.consumed();
        if bound == 0 {
            return Ok(false);
        }

        self.nbest_rows = if self.model.has_real_unigrams() {
            let matrix = build_scan_matrix(&graph, self.settings.options);
            crate::nbest::nbest_sentences(
                &matrix,
                bound,
                &self.dictionary,
                &self.model,
                &self.history,
            )?
        } else {
            let scorer = Scorer::with_key_costs(
                self.scoring,
                &self.dictionary,
                &self.model,
                self.key_costs.clone(),
            );
            let paths = k_best(&graph, &scorer, SEGMENTATION_K).map_err(EngineError::Decode)?;
            let mut sentences: Vec<(Candidate, Vec<PhraseToken>)> = Vec::new();
            for path in &paths {
                sentences.extend(self.collect_sentences_with_tokens(&graph, &scorer, path)?);
            }
            sentences.sort_by_key(|(candidate, _)| candidate.cost());
            let mut seen: HashSet<compact_str::CompactString> = HashSet::new();
            sentences
                .into_iter()
                .filter(|(candidate, _)| seen.insert(candidate.text().into()))
                .take(crate::nbest::NBEST_ROWS)
                .map(|(candidate, tokens)| crate::nbest::NbestRow {
                    text: candidate.text().into(),
                    tokens,
                    keys: candidate.consumed_keys(),
                    span: candidate.consumed_bytes(),
                    cost: candidate.cost(),
                })
                .collect()
        };
        // The rows above were seeded with the history as it stands right
        // here; a later row selection restores this snapshot before extending
        // the record with the row's own tokens.
        self.nbest_history.clone_from(&self.history);

        if !self.selected.is_empty() {
            for row in &mut self.nbest_rows {
                let mut full = compact_str::CompactString::from(&self.selected);
                full.push_str(&row.text);
                row.text = full;
            }
        }

        self.refresh()?;
        Ok(true)
    }

    /// Whether a sentence lookup has run since the last reset.
    ///
    /// The lookup-active half of the gate: while this is true,
    /// `pinyin_get_sentence` answers decoded-or-nothing (upstream's
    /// `0 == results.size()` false) and never the pre-lookup raw form,
    /// even when the lookup produced no rows.
    #[must_use]
    pub const fn sentence_lookup_active(&self) -> bool {
        self.sentence_lookup_active
    }

    /// The decoded text of n-best row `index`, best-first
    /// (`pinyin_get_sentence`'s payload). `None` when fewer rows exist.
    #[must_use]
    pub fn sentence_text(&self, index: u8) -> Option<&str> {
        self.nbest_rows
            .get(usize::from(index))
            .map(|row| row.text.as_str())
            .filter(|text| !text.is_empty())
    }

    /// The raw input typed so far.
    #[must_use]
    pub fn raw_input(&self) -> &str {
        &self.raw
    }

    /// Whether a composition is in progress.
    #[must_use]
    pub fn is_composing(&self) -> bool {
        !self.raw.is_empty()
    }

    /// Bytes of the raw input consumed by selections so far — the
    /// composition offset the candidate lookup is anchored at.
    ///
    /// After a successful [`Session::select`] this is the chosen
    /// candidate's absolute end position: the previous anchor plus the
    /// candidate's span (separator run included), never past the raw
    /// input. The C ABI answers it as the new lookup cursor — the caller
    /// offset may sit past a separator run the span also covers, so
    /// caller-offset-plus-span would count that run twice
    /// (libpinyin@412f88e3 instead anchors `m_begin` at the caller offset,
    /// reaching the same end).
    #[must_use]
    pub const fn composition_offset(&self) -> usize {
        self.consumed
    }

    /// Candidates per page, from the configuration the session was opened
    /// with.
    #[must_use]
    pub const fn page_size(&self) -> usize {
        self.settings.page_size
    }

    /// The storage locations the session was opened with.
    #[must_use]
    pub const fn paths(&self) -> &StoragePaths {
        &self.paths
    }

    /// The dictionary backend.
    #[must_use]
    pub const fn dictionary(&self) -> &D {
        &self.dictionary
    }

    /// The language model backend.
    #[must_use]
    pub const fn language_model(&self) -> &L {
        &self.model
    }

    fn type_character(&mut self, character: char) -> Result<KeyOutcome, EngineError> {
        if !is_input_character(character) {
            return Ok(KeyOutcome::Ignored);
        }
        if self.raw.len() + character.len_utf8() > MAX_INPUT_BYTES {
            return Ok(KeyOutcome::Ignored);
        }

        self.raw.push(character);
        self.refresh()?;
        Ok(KeyOutcome::Consumed)
    }

    fn erase(&mut self) -> Result<KeyOutcome, EngineError> {
        if self.consumed < self.raw.len() {
            self.raw.pop();
            self.refresh()?;
            return Ok(KeyOutcome::Consumed);
        }
        if !self.selected.is_empty() {
            self.selected.clear();
            self.consumed = 0;
            self.history.clear();
            self.refresh()?;
            return Ok(KeyOutcome::Consumed);
        }
        Ok(KeyOutcome::Ignored)
    }

    fn accept_first(&mut self) -> Result<KeyOutcome, EngineError> {
        if self.candidates.is_empty() {
            if self.is_composing() {
                return Ok(KeyOutcome::Commit(self.commit()?));
            }
            return Ok(KeyOutcome::Ignored);
        }

        match self.select(0)? {
            Selection::Completed => Ok(KeyOutcome::Commit(self.commit()?)),
            Selection::Continued => Ok(KeyOutcome::Consumed),
        }
    }

    /// Normalizes a caller lookup offset back to the first byte of the
    /// apostrophe separator run before it, then validates it — the
    /// `_compute_zero_start` + `_check_offset` pair `pinyin_guess_candidates`
    /// runs at libpinyin@dbff264 (`pinyin.cpp:2182-2228`).
    ///
    /// ibus-libpinyin ≥ 1.16.1 passes the raw begin of the next key rest,
    /// which can sit one position past the zero-`ChewingKey` `'` run
    /// (ibus-libpinyin issue #570). The pin walks the matrix from
    /// `offset - 1` downward while the index stays positive and the column
    /// is a lone zero key; in the raw buffer those columns are exactly the
    /// `'` bytes, so the byte walk is the same law.
    ///
    /// The candidate lookup stays anchored at the composition offset the
    /// session owns, so a choose at the caller offset keeps round-tripping;
    /// the normalized offset is the one a materialized `DYNAMIC_ADJUST`
    /// previous-token lookup must use (#99 folds that bigram term to zero
    /// today, `dynamic_adjust_bigram_term`).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::LookupOffsetOutOfRange`] for an offset beyond
    /// the raw input's one-past-end position (upstream reads its matrix out
    /// of bounds there — no pinned behaviour exists to reproduce), and
    /// [`EngineError::LookupOffsetPastSeparator`] where upstream's
    /// `_check_offset` aborts: the normalized offset still sits one past a
    /// separator, which only a leading apostrophe run can cause (the walk
    /// never crosses byte 0).
    pub fn normalized_lookup_offset(&self, offset: usize) -> Result<usize, EngineError> {
        let raw = self.raw.as_bytes();
        if offset > raw.len() {
            return Err(EngineError::LookupOffsetOutOfRange {
                offset,
                len: raw.len(),
            });
        }
        let mut normalized = offset;
        let mut index = offset.saturating_sub(1);
        while index > 0 && raw.get(index) == Some(&b'\'') {
            normalized = index;
            index -= 1;
        }
        if normalized > 0 && raw.get(normalized - 1) == Some(&b'\'') {
            return Err(EngineError::LookupOffsetPastSeparator { offset, normalized });
        }
        Ok(normalized)
    }

    /// Rounds `offset` up to the next character boundary of the raw input.
    ///
    /// The raw buffer only ever holds ASCII, so this is the identity in
    /// practice; it exists so a future input character class cannot turn a
    /// byte count into a slicing panic.
    fn next_boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.raw.len());
        while !self.raw.is_char_boundary(offset) {
            offset += 1;
        }
        offset
    }

    /// Recomputes the candidate list for the current state.
    ///
    /// Parse into a graph and run libpinyin's expanding-window scan over it:
    /// every key-path through the parser-shaped key set, searched against the
    /// phrase table window by window. Cross-segmentation pooling falls out of
    /// the scan — `xian` offers `西安` (`xi` + `an`) alongside the single key
    /// `xian`, and `fangan` mixes `方案` (`fang` + `an`) with `反感`
    /// (`fan` + `gan`) — without a separate pooling step.
    ///
    /// Under the pinned observation surface the pin emits no sentence-level
    /// candidates at all — its `nihaoshi` list never contains `你好是`, the
    /// best sentence the DP over the segment lattice would produce — so the
    /// pooled phrase candidates are ranked by the three-key order and
    /// deduplicated directly, with no sentence prepend.
    fn refresh(&mut self) -> Result<(), EngineError> {
        if self.consumed >= self.raw.len() {
            self.candidates = CandidateList::default();
            self.parsed_prefix = 0;
            return Ok(());
        }

        // Lift scratches before borrowing `raw`, so graph/scan can use
        // `&self.raw[consumed..]` without cloning into CompactString.
        let mut collected = core::mem::take(&mut self.scratch_collected);
        collected.clear();
        let mut path = core::mem::take(&mut self.scratch_path);
        let mut entries = core::mem::take(&mut self.scratch_entries);
        let mut ranked = core::mem::take(&mut self.scratch_ranked);
        let mut window_phrase = core::mem::take(&mut self.scratch_window_phrase);
        let mut window_addon = core::mem::take(&mut self.scratch_window_addon);

        let remaining = &self.raw[self.consumed..];
        let graph = SegmentGraph::build_with_options(remaining.as_bytes(), self.settings.options)
            .map_err(EngineError::Graph)?;
        self.parsed_prefix = graph
            .fewest_keys(self.settings.incomplete())
            .last()
            .map_or(0, Edge::to);

        // When the model carries the phrase index's real unigram
        // frequencies, the pinned construction runs — the expanding-window
        // scan, the three-key order (text length, pinyin span, frequency),
        // keep-first dedup, and only the raw-input fallback after an empty
        // result. Without real frequencies the session reproduces its
        // pre-frequency behaviour exactly: k-best prefixes, sentence
        // candidates, cost order, adjacent dedup.
        if self.model.has_real_unigrams() {
            {
                let mut scratch = ScanScratch {
                    path: &mut path,
                    entries: &mut entries,
                    window_phrase: &mut window_phrase,
                    window_addon: &mut window_addon,
                };
                self.collect_window_scan(
                    &graph,
                    remaining.as_bytes(),
                    self.settings.options,
                    &mut collected,
                    &mut scratch,
                )?;
            }

            // The scan's result stands even when it found nothing. Tokens the
            // table lacks rank as zero rather than falling back.
            let frequencies = self
                .candidate_frequencies(&collected)?
                .unwrap_or_else(|| vec![0; collected.len()]);
            ranked.clear();
            ranked.extend(
                collected
                    .drain(..)
                    .zip(frequencies)
                    .map(|(candidate, frequency)| {
                        let key = RankKey {
                            phrase_length: candidate.text().chars().count(),
                            pinyin_span: candidate.consumed_bytes(),
                            frequency,
                        };
                        (key, candidate)
                    }),
            );

            // Stable sort, all three keys descending: an all-equal tie keeps
            // the collection order, which the scan now lays down in the
            // pin's array order (per window, token-ascending).
            ranked.sort_by_key(|(key, _)| core::cmp::Reverse(*key));
            collected.extend(ranked.drain(..).map(|(_, candidate)| candidate));

            dedup_by_text_keep_first(&mut collected);
        } else {
            let scorer = Scorer::with_key_costs(
                self.scoring,
                &self.dictionary,
                &self.model,
                self.key_costs.clone(),
            );
            let paths = k_best(&graph, &scorer, SEGMENTATION_K).map_err(EngineError::Decode)?;
            for path in &paths {
                self.collect_prefix_phrases(&graph, &scorer, path, &mut collected)?;
                self.collect_sentence(&graph, &scorer, path, &mut collected)?;
            }
            collected.sort_by_key(Candidate::cost);
            collected.dedup_by(|left, right| left.text() == right.text());
        }

        if collected.is_empty() {
            collected.push(Candidate::new(
                compact_str::CompactString::from(remaining),
                CandidateKind::Fallback,
                0,
                remaining.len(),
                0,
                None,
                None,
            ));
        }

        // W14: prepend the stored n-best rows, head first, then drop every
        // later candidate with the same text — upstream prepends after the
        // sort and its phrase-string dedup keeps the NBEST row (and the
        // lower n-best index) over any phrase candidate with the same
        // string (`pinyin.cpp:2290-2298`, `2058-2126`).
        if !self.nbest_rows.is_empty() {
            // Extend-then-rotate keeps `collected`'s allocation (the session
            // scratch). Assigning a fresh `merged` vec would drop that
            // capacity on every refresh that has n-best rows.
            let nbest_n = self.nbest_rows.len();
            collected.extend(self.nbest_rows.iter().enumerate().map(|(index, row)| {
                Candidate::new(
                    row.text.clone(),
                    CandidateKind::Sentence,
                    row.keys,
                    row.span,
                    row.cost,
                    None,
                    Some(u8::try_from(index).unwrap_or(u8::MAX)),
                )
            }));
            collected.rotate_right(nbest_n);
            dedup_by_text_keep_first(&mut collected);
        }

        self.candidates.swap_items(&mut collected);
        collected.clear();
        self.scratch_collected = collected;
        self.scratch_path = path;
        self.scratch_entries = entries;
        self.scratch_ranked = ranked;
        self.scratch_window_phrase = window_phrase;
        self.scratch_window_addon = window_addon;
        Ok(())
    }

    /// Per-candidate sort frequencies on the pin's amplified scale, or
    /// `None` when the model carries no real frequency table at all.
    ///
    /// The pinned oracle does not compare raw unigram counts: it truncates
    /// the f32 possibility `(1−λ)·unigram/total` amplified by 2²⁴ into a
    /// `guint32` (`_compute_frequency_of_items`, `pinyin.cpp:1855-1866`;
    /// `DYNAMIC_ADJUST` clear ⇒ the bigram term is zero). Near-ties collapse
    /// to equal keys under that truncation — the tie class
    /// `docs/findings/corpus-tail.md` calls Class A — and equal keys fall to
    /// the collection order the stable sort keeps. `amplified_frequency`
    /// reproduces the arithmetic bit-for-bit; the `+1` is the shipped
    /// model20 data identity (every phrase-index item's baked unigram is
    /// its interpolation2 count + 1; items absent from interpolation2 are
    /// 1), and the denominator is the index total that follows from it:
    /// interpolation2 sum + item count.
    ///
    /// `Some(0)` marks a phrase the n-gram corpus never saw: it still sorts,
    /// last among its equal-length, equal-span peers. Only the first `Some`
    /// switches the construction on, so a model that mixes per-token answers
    /// degrades deterministically (missing tokens rank as zero).
    fn candidate_frequencies(
        &self,
        collected: &[Candidate],
    ) -> Result<Option<Vec<u64>>, EngineError> {
        let mut frequencies: Option<Vec<u64>> = None;
        let default_total = self
            .model
            .unigram_total()
            .map_err(|error| EngineError::Scoring(ScoringError::LanguageModel(error.to_string())))?
            .unwrap_or(0)
            .saturating_add(self.dictionary.phrase_index_item_count().map_err(|error| {
                EngineError::Scoring(ScoringError::Dictionary(error.to_string()))
            })?);
        let addon_total = self
            .model
            .addon_unigram_total()
            .map_err(|error| EngineError::Scoring(ScoringError::LanguageModel(error.to_string())))?
            .unwrap_or(0);
        for (index, candidate) in collected.iter().enumerate() {
            let Some(token) = candidate.token() else {
                continue;
            };
            let count = if candidate.kind() == CandidateKind::Addon {
                // The addon facade's own amplified scale (`pinyin.cpp:1829-
                // 1843`): no `+1`, the addon index's items carry their own
                // unigrams. An empty facade has no items and no candidates.
                let raw = self
                    .model
                    .addon_unigram_freq(&token)
                    .map_err(|error| {
                        EngineError::Scoring(ScoringError::LanguageModel(error.to_string()))
                    })?
                    .unwrap_or(0);
                Some(amplified_frequency(raw, addon_total))
            } else {
                self.model
                    .unigram_freq(&token)
                    .map_err(|error| {
                        EngineError::Scoring(ScoringError::LanguageModel(error.to_string()))
                    })?
                    .map(|count| amplified_frequency(count.saturating_add(1), default_total))
            };
            if let Some(count) = count {
                let table = frequencies.get_or_insert_with(|| vec![0; collected.len()]);
                // Unigram term of candidate frequency: always on. Upstream
                // reads FacadePhraseIndex unigrams (including trained user
                // counts) with no DYNAMIC_ADJUST check. W6-T4's overlay is
                // that unigram term and stays for both bit states.
                table[index] = candidate_frequency_sort_key(self.settings.options, count);
            }
        }
        Ok(frequencies)
    }

    /// Offers every phrase spelling a prefix of `path`.
    ///
    /// Only the pre-frequency fallback uses this: the pinned construction
    /// collects through the window scan instead. Kept verbatim so a missing
    /// model cache reproduces the prior behaviour exactly.
    fn collect_prefix_phrases(
        &self,
        graph: &SegmentGraph,
        scorer: &Scorer<'_, D, L>,
        path: &DecodedPath,
        into: &mut Vec<Candidate>,
    ) -> Result<(), EngineError> {
        let (keys, kinds, ends) = self.walk(graph, path);

        // A dictionary phrase never spans more than MAX_PHRASE_KEYS keys, so
        // looking further is both pointless and quadratic in the input.
        for length in 1..=keys.len().min(MAX_PHRASE_KEYS) {
            let ranked = scorer
                .rank_phrases(&self.history, &keys[..length], &kinds[..length])
                .map_err(EngineError::Scoring)?;
            for (entry, cost) in ranked {
                let token = entry.token();
                into.push(Candidate::new(
                    entry.into_text(),
                    CandidateKind::Phrase,
                    length,
                    ends[length - 1],
                    cost,
                    Some(token),
                    None,
                ));
            }
        }
        Ok(())
    }

    /// Offers the cheapest sequence of phrases covering the whole of `path`.
    ///
    /// Only the pre-frequency fallback uses this: when the model carries no
    /// real unigram table the session reproduces its prior behaviour exactly,
    /// sentence candidates included. The real-frequency construction emits
    /// pooled phrase candidates only until [`Session::guess_sentence`] runs.
    fn collect_sentence(
        &self,
        graph: &SegmentGraph,
        scorer: &Scorer<'_, D, L>,
        path: &DecodedPath,
        into: &mut Vec<Candidate>,
    ) -> Result<(), EngineError> {
        for (candidate, _) in self.collect_sentences_with_tokens(graph, scorer, path)? {
            into.push(candidate);
        }
        Ok(())
    }

    /// [`Self::collect_sentence`] with each sentence's token path, which a
    /// chosen fallback row records like a trellis row does.
    fn collect_sentences_with_tokens(
        &self,
        graph: &SegmentGraph,
        scorer: &Scorer<'_, D, L>,
        path: &DecodedPath,
    ) -> Result<Vec<(Candidate, Vec<PhraseToken>)>, EngineError> {
        let (keys, kinds, ends) = self.walk(graph, path);
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        // best[i] is the cheapest way to spell keys[..i].
        let mut best: Vec<Option<(Cost, String, Vec<PhraseToken>)>> = vec![None; keys.len() + 1];
        best[0] = Some((0, String::new(), self.history.clone()));

        for end in 1..=keys.len() {
            let first = end.saturating_sub(MAX_PHRASE_KEYS);
            for start in first..end {
                // TODO: avoid cloning — bounded by MAX_PHRASE_KEYS × input.
                // The clone is here because the loop writes back into `best`
                // while reading this entry; splitting the read and the write,
                // or holding an index instead of the text, would remove it.
                let Some((prefix_cost, prefix_text, prefix_history)) = best[start].clone() else {
                    continue;
                };
                let ranked = scorer
                    .rank_phrases(&prefix_history, &keys[start..end], &kinds[start..end])
                    .map_err(EngineError::Scoring)?;
                let Some((entry, cost)) = ranked.first() else {
                    continue;
                };

                let total = prefix_cost.saturating_add(*cost);
                if best[end].as_ref().is_none_or(|(seen, ..)| total < *seen) {
                    let mut text = prefix_text.clone();
                    text.push_str(entry.text());
                    let mut history = prefix_history.clone();
                    history.push(entry.token());
                    best[end] = Some((total, text, history));
                }
            }
        }

        if let Some((cost, text, tokens)) = best[keys.len()].clone()
            && !text.is_empty()
        {
            let tokens = tokens[self.history.len()..].to_vec();
            return Ok(vec![(
                Candidate::new(
                    text,
                    CandidateKind::Sentence,
                    keys.len(),
                    ends[keys.len() - 1],
                    cost,
                    None,
                    None,
                ),
                tokens,
            )]);
        }
        Ok(Vec::new())
    }

    /// The keys, edge kinds and end offsets along one decoded path.
    ///
    /// An `Incomplete` edge is dropped when the configuration turns
    /// initial-only keys off, which is the `PINYIN_INCOMPLETE` bit the parity
    /// profile sets.
    fn walk(
        &self,
        graph: &SegmentGraph,
        path: &DecodedPath,
    ) -> (Vec<SyllableKey>, Vec<EdgeKind>, Vec<usize>) {
        let mut keys = Vec::with_capacity(path.len());
        let mut kinds = Vec::with_capacity(path.len());
        let mut ends = Vec::with_capacity(path.len());

        for id in path.edges() {
            let Some(edge) = graph.edge(*id) else {
                continue;
            };
            if !self.settings.incomplete() && edge.kind() == EdgeKind::Incomplete {
                break;
            }
            keys.push(edge.key());
            kinds.push(edge.kind());
            ends.push(edge.to());
        }
        (keys, kinds, ends)
    }

    /// The expanding-window scan of the pinned candidate collection.
    ///
    /// Start is fixed at the composition offset (byte 0 of the remaining
    /// input); `end` walks outward over every byte position the graph
    /// reaches. At each `[start, end)` window every key-path through the scan
    /// matrix — the selected parse plus the resplit/divided additions,
    /// `docs/findings/matrix-split-tables.md` — is enumerated and the phrase
    /// table is searched on the accumulated sequence; initial-only keys expand
    /// through [`expand_keys`]. Every phrase found is appended with its
    /// `[start, end)` span.
    ///
    /// Widening is prefix-driven: a window whose sequences cannot extend to
    /// any stored phrase stops the scan (the pin's continued-search probe,
    /// [`crate::Dictionary::phrase_prefix_exists`]). Consecutive apostrophe
    /// bytes after a searched window are skipped so the next window does not
    /// repeat the same key sequence.
    fn collect_window_scan(
        &self,
        graph: &SegmentGraph,
        input: &[u8],
        options: OptionBits,
        into: &mut Vec<Candidate>,
        scratch: &mut ScanScratch<'_>,
    ) -> Result<(), EngineError> {
        let matrix = build_scan_matrix(graph, options);
        let bound = graph.consumed();
        let mut end = 1usize;
        while end <= bound {
            // An end position no key starts at is an empty column: widen.
            let mut continued = matrix.get(end).is_none_or(|column| column.is_empty());
            scratch.path.clear();
            scratch.window_phrase.clear();
            scratch.window_addon.clear();
            {
                let mut buf = ScanBuf {
                    path: scratch.path,
                    system: scratch.window_phrase,
                    addon: scratch.window_addon,
                    continued: &mut continued,
                    entries: scratch.entries,
                };
                self.scan_paths(&matrix, 0, end, &mut buf)?;
            }
            // Flush the window in the pin's array order: the default
            // facade's tokens ascending, then the addon facade's — the
            // order `_append_items` lays down and the stable sort keeps
            // for comparator ties.
            flush_window_batch(scratch.window_phrase, into);
            flush_window_batch(scratch.window_addon, into);
            if !continued {
                break;
            }
            end += 1;
            // Skip windows that would only cross an apostrophe separator: they
            // repeat the previous key sequence.
            while end <= bound && input.get(end - 1) == Some(&b'\'') {
                end += 1;
            }
        }
        Ok(())
    }

    /// Enumerates every key-path from `node` to `end` and searches the table on
    /// each complete path.
    fn scan_paths(
        &self,
        matrix: &[Vec<ScanKey>],
        node: usize,
        end: usize,
        buf: &mut ScanBuf<'_>,
    ) -> Result<(), EngineError> {
        let Some(column) = matrix.get(node) else {
            return Ok(());
        };
        for scan_key in column.iter().copied() {
            self.visit_scan_key(matrix, scan_key, end, buf)?;
        }
        Ok(())
    }

    /// One matrix key during the scan.
    fn visit_scan_key(
        &self,
        matrix: &[Vec<ScanKey>],
        scan_key: ScanKey,
        end: usize,
        buf: &mut ScanBuf<'_>,
    ) -> Result<(), EngineError> {
        let to = scan_key.to;
        if to > end {
            // A key overhanging the window: the phrase could continue, which is
            // upstream's `longest > end` CONTINUED.
            *buf.continued = true;
            return Ok(());
        }
        buf.path.push(scan_key.key);
        if to == end {
            self.search_scan_path(buf, end)?;
        } else if buf.path.len() < MAX_PHRASE_LENGTH {
            self.scan_paths(matrix, to, end, buf)?;
        }
        buf.path.pop();
        Ok(())
    }

    /// The table search on one complete key-path, and the prefix probe that
    /// decides whether the window keeps widening.
    fn search_scan_path(&self, buf: &mut ScanBuf<'_>, end: usize) -> Result<(), EngineError> {
        let ScanBuf {
            path,
            system,
            addon,
            continued,
            entries,
        } = buf;
        let has_incomplete = path
            .iter()
            .any(|key| key.completeness() == Completeness::Partial);

        if has_incomplete {
            for sequence in expand_keys(path, SCAN_EXPANSION_LIMIT) {
                self.lookup_and_append(
                    sequence.as_slice(),
                    path.len(),
                    end,
                    system,
                    addon,
                    entries,
                )?;
            }
        } else {
            self.lookup_and_append(path, path.len(), end, system, addon, entries)?;
        }

        let can_extend = self
            .dictionary
            .phrase_prefix_exists(path)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        let addon_extend = self
            .dictionary
            .phrase_prefix_exists_addon(path)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        **continued |= can_extend || addon_extend;
        Ok(())
    }

    fn lookup_and_append(
        &self,
        sequence: &[SyllableKey],
        keys: usize,
        end: usize,
        system: &mut Vec<Candidate>,
        addon: &mut Vec<Candidate>,
        entries: &mut Vec<PhraseEntry>,
    ) -> Result<(), EngineError> {
        self.dictionary
            .lookup_into(sequence, entries)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        append_scan_entries(entries.drain(..), keys, end, CandidateKind::Phrase, system);
        self.dictionary
            .lookup_addon_into(sequence, entries)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        append_scan_entries(entries.drain(..), keys, end, CandidateKind::Addon, addon);
        Ok(())
    }
}

/// Pushes the phrases one key-path search returned.
fn append_scan_entries(
    entries: impl IntoIterator<Item = PhraseEntry>,
    keys: usize,
    end: usize,
    kind: CandidateKind,
    into: &mut Vec<Candidate>,
) {
    for entry in entries {
        let token = entry.token();
        into.push(Candidate::new(
            entry.into_text(),
            kind,
            keys,
            end,
            0,
            Some(token),
            None,
        ));
    }
}

/// Flushes one window's facade batch in the pin's array order.
///
/// The oracle appends each window's search hits library by library, token
/// by token (`_append_items`, `pinyin.cpp:1769-1791`), and its stable
/// `g_array_sort_with_data` keeps exactly that order for candidates whose
/// three keys tie — the amplified-frequency collapses of
/// `docs/findings/corpus-tail.md` Class A. The scan reaches the same
/// tokens through several key-paths; sorting the batch by token and
/// keeping the first of each reproduces the one-row-per-token array the
/// pin sorts.
fn flush_window_batch(batch: &mut Vec<Candidate>, into: &mut Vec<Candidate>) {
    batch.sort_by_key(|candidate| candidate.token().map_or(u32::MAX, |t| t.value()));
    let mut last: Option<u32> = None;
    for candidate in batch.drain(..) {
        let token = candidate.token().map(|token| token.value());
        if token == last {
            continue;
        }
        last = token;
        into.push(candidate);
    }
}

/// λ as the pin parses it out of `table.conf` (`fscanf "%f"`,
/// `table_info.cpp:220,242`) — the same `f32` bits `oxpinyin_data`'s
/// `PINNED_LAMBDA` names. Duplicated here because the engine depends on
/// the core traits, not the data crate.
const PIN_LAMBDA_F32: f32 = 0.312_699;

/// The pin's candidate `m_freq` under the default profile: the unigram
/// possibility `(1−λ)·unigram/total` computed and amplified by 2²⁴ in C
/// `float` arithmetic, then truncated like the `guint32` assignment
/// (`pinyin.cpp:1862-1866`; `DYNAMIC_ADJUST` clear ⇒ bigram term zero).
///
/// The truncation is load-bearing, not a rounding detail: it collapses
/// near-ties into equal comparator keys — the Class A tie class — which
/// the stable sort then resolves by collection order. Evaluation order
/// mirrors the C expression left-to-right (`f32` throughout, the three
/// `* 256` factors kept as written); any `f64` intermediate or a
/// pre-combined `* 2²⁴` risks drifting off the tie boundary.
fn amplified_frequency(unigram: u64, total: u64) -> u64 {
    if total == 0 {
        return 0;
    }
    let possibility = (1.0_f32 - PIN_LAMBDA_F32) * unigram as f32 / total as f32;
    u64::from((possibility * 256.0 * 256.0 * 256.0) as u32)
}

/// One resplit pair the scan matrix admits alongside the selected parse,
/// frozen in `docs/findings/matrix-split-tables.md`.
///
/// `(first, second) -> (left, right)`; `left` occupies the start of `first`
/// and `right` runs from its end to `second`'s end.
const RESPLIT_TABLE: &[(&str, &str, &str, &str)] = &[
    ("a", "nan", "an", "an"),
    ("an", "gang", "ang", "ang"),
    ("ba", "nan", "ban", "an"),
    ("ca", "nan", "can", "an"),
    ("chan", "gan", "chang", "an"),
    ("chan", "ge", "chang", "e"),
    ("che", "nai", "chen", "ai"),
    ("chen", "gan", "cheng", "an"),
    ("chu", "nan", "chun", "an"),
    ("dan", "gan", "dang", "an"),
    ("e", "nai", "en", "ai"),
    ("e", "nen", "en", "en"),
    ("fa", "nan", "fan", "an"),
    ("fan", "gai", "fang", "ai"),
    ("fan", "gan", "fang", "an"),
    ("fan", "ge", "fang", "e"),
    ("ga", "nai", "gan", "ai"),
    ("ga", "nen", "gan", "en"),
    ("gan", "gao", "gang", "ao"),
    ("guan", "gan", "guang", "an"),
    ("hu", "nan", "hun", "an"),
    ("huan", "gan", "huang", "an"),
    ("ji", "ne", "jin", "e"),
    ("ji", "nou", "jin", "ou"),
    ("jia", "nai", "jian", "ai"),
    ("jia", "nan", "jian", "an"),
    ("jia", "nao", "jian", "ao"),
    ("jia", "ne", "jian", "e"),
    ("jia", "nou", "jian", "ou"),
    ("jian", "gan", "jiang", "an"),
    ("jin", "gai", "jing", "ai"),
    ("jin", "gan", "jing", "an"),
    ("jin", "ge", "jing", "e"),
    ("kuan", "gao", "kuang", "ao"),
    ("li", "nan", "lin", "an"),
    ("lia", "nai", "lian", "ai"),
    ("lia", "ne", "lian", "e"),
    ("lian", "gan", "liang", "an"),
    ("ma", "ne", "man", "e"),
    ("men", "gen", "meng", "en"),
    ("min", "gan", "ming", "an"),
    ("min", "ge", "ming", "e"),
    ("na", "nai", "nan", "ai"),
    ("na", "nan", "nan", "an"),
    ("na", "nao", "nan", "ao"),
    ("na", "nou", "nan", "ou"),
    ("nin", "gan", "ning", "an"),
    ("pa", "nan", "pan", "an"),
    ("pen", "gan", "peng", "an"),
    ("pin", "gan", "ping", "an"),
    ("qi", "nai", "qin", "ai"),
    ("qi", "nan", "qin", "an"),
    ("qia", "nan", "qian", "an"),
    ("qia", "ne", "qian", "e"),
    ("qin", "gai", "qing", "ai"),
    ("qin", "gan", "qing", "an"),
    ("qu", "na", "qun", "a"),
    ("re", "nai", "ren", "ai"),
    ("re", "nan", "ren", "an"),
    ("san", "gou", "sang", "ou"),
    ("shan", "gan", "shang", "an"),
    ("she", "nai", "shen", "ai"),
    ("she", "nao", "shen", "ao"),
    ("wa", "nan", "wan", "an"),
    ("wa", "ne", "wan", "e"),
    ("wa", "nou", "wan", "ou"),
    ("wen", "gan", "weng", "an"),
    ("xi", "nai", "xin", "ai"),
    ("xi", "nan", "xin", "an"),
    ("xia", "nai", "xian", "ai"),
    ("xia", "nan", "xian", "an"),
    ("xia", "ne", "xian", "e"),
    ("xian", "gai", "xiang", "ai"),
    ("xian", "gan", "xiang", "an"),
    ("xian", "ge", "xiang", "e"),
    ("xin", "gai", "xing", "ai"),
    ("xin", "gan", "xing", "an"),
    ("ya", "nan", "yan", "an"),
    ("yi", "nan", "yin", "an"),
    ("yi", "ne", "yin", "e"),
    ("zhan", "gai", "zhang", "ai"),
    ("zhe", "nai", "zhen", "ai"),
    ("zhe", "nan", "zhen", "an"),
    ("zhen", "gan", "zheng", "an"),
    ("zhua", "nan", "zhuan", "an"),
];

/// One divided syllable the scan matrix splits, frozen in
/// `docs/findings/matrix-split-tables.md`.
///
/// `syllable -> (left, right)`, where `left` ends inside the syllable.
const DIVIDED_TABLE: &[(&str, &str, &str)] = &[
    ("bian", "bi", "an"),
    ("bie", "bi", "e"),
    ("dian", "di", "an"),
    ("jian", "ji", "an"),
    ("jiang", "ji", "ang"),
    ("jie", "ji", "e"),
    ("jue", "ju", "e"),
    ("kuai", "ku", "ai"),
    ("lian", "li", "an"),
    ("liang", "li", "ang"),
    ("liao", "li", "ao"),
    ("luan", "lu", "an"),
    ("qian", "qi", "an"),
    ("qie", "qi", "e"),
    ("shuan", "shu", "an"),
    ("tian", "ti", "an"),
    ("tuan", "tu", "an"),
    ("xian", "xi", "an"),
    ("yuan", "yu", "an"),
    ("zuan", "zu", "an"),
];

/// Scratch the window scan threads through the recursive walk.
/// The window scan's borrowed scratch: the session-owned buffers one scan
/// reuses, grouped so the scan takes a single argument.
struct ScanScratch<'a> {
    path: &'a mut SmallVec<[SyllableKey; 16]>,
    entries: &'a mut Vec<PhraseEntry>,
    window_phrase: &'a mut Vec<Candidate>,
    window_addon: &'a mut Vec<Candidate>,
}

struct ScanBuf<'a> {
    path: &'a mut SmallVec<[SyllableKey; 16]>,
    system: &'a mut Vec<Candidate>,
    addon: &'a mut Vec<Candidate>,
    continued: &'a mut bool,
    entries: &'a mut Vec<PhraseEntry>,
}

/// One key of the scan matrix at its byte position, with the byte position
/// it ends at and where its own text starts — the two differ from
/// `from + len` exactly when the key rides over an apostrophe separator.
#[derive(Clone, Copy)]
pub(crate) struct ScanKey {
    pub(crate) key: SyllableKey,
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) syllable_start: usize,
    pub(crate) crosses_separator: bool,
    /// The tone consumed with this key under `USE_TONE` (`Edge::tone`).
    /// Rides the fuzzy alternates and locks the resplit/divided tables,
    /// which compare full `ChewingKey` equality against zero-tone structs
    /// (`chewing_key.h:81-91`) and therefore never match a toned key.
    pub(crate) tone: u8,
}

impl ScanKey {
    fn from_edge(edge: &Edge) -> Self {
        Self {
            key: edge.key(),
            from: edge.from(),
            to: edge.to(),
            syllable_start: edge.syllable_start(),
            crosses_separator: edge.crosses_separator(),
            tone: edge.tone(),
        }
    }
}

/// The keys the pin's matrix holds per byte position: the selected parse's
/// keys, plus the resplit, divided and fuzzy additions. See
/// `docs/findings/matrix-split-tables.md` for the frozen pair lists and
/// `docs/findings/option-bits.md` for the fuzzy step.
pub(crate) fn build_scan_matrix(graph: &SegmentGraph, options: OptionBits) -> Vec<Vec<ScanKey>> {
    let bound = graph.consumed();
    let mut columns: Vec<Vec<ScanKey>> = vec![Vec::new(); bound + 1];

    // 1. The selected parse.
    let selected_edges = graph.fewest_keys(options.has_incomplete());
    let selected: Vec<ScanKey> = selected_edges.iter().map(ScanKey::from_edge).collect();
    for scan_key in &selected {
        columns[scan_key.from].push(*scan_key);
    }

    // 2. Resplit pairs along the selected path. A pair only resplits when
    // the two keys share a boundary with no apostrophe between them: the
    // pin fills a zero key at a separator, so its pairs never span one.
    // A toned key never resplits: upstream matches the full ChewingKey
    // (tone included) against zero-tone table structs.
    let mut additions: Vec<ScanKey> = Vec::new();
    for pair in selected.windows(2) {
        if pair[1].from != pair[0].to || pair[0].crosses_separator || pair[1].crosses_separator {
            continue;
        }
        if pair[0].tone != 0 || pair[1].tone != 0 {
            continue;
        }
        let Some((_, _, left, right)) = RESPLIT_TABLE.iter().find(|(first, second, _, _)| {
            *first == pair[0].key.text() && *second == pair[1].key.text()
        }) else {
            continue;
        };
        let Some(left_key) = SyllableKey::from_text(left) else {
            continue;
        };
        let Some(right_key) = SyllableKey::from_text(right) else {
            continue;
        };
        let split = pair[0].from + left.len();
        additions.push(ScanKey {
            key: left_key,
            from: pair[0].from,
            to: split,
            syllable_start: pair[0].from,
            crosses_separator: false,
            tone: 0,
        });
        additions.push(ScanKey {
            key: right_key,
            from: split,
            to: pair[1].to,
            syllable_start: split,
            crosses_separator: false,
            tone: 0,
        });
    }
    for addition in &additions {
        columns[addition.from].push(*addition);
    }

    // 3. Divided syllables over every key collected so far. The split parts
    // are measured from the syllable text itself, so a key that rides over
    // an apostrophe still divides (`bu'tian` offers `补体` from the divided
    // `ti`, whose span covers the apostrophe plus `t` + `i`). A toned key
    // never divides: the divided table's structs are zero-tone and upstream
    // matches the full ChewingKey.
    let snapshot: Vec<ScanKey> = columns
        .iter()
        .enumerate()
        .flat_map(|(position, keys)| keys.iter().map(move |key| (position, *key)))
        .map(|(position, key)| ScanKey {
            key: key.key,
            from: position,
            to: key.to,
            syllable_start: key.syllable_start,
            crosses_separator: key.crosses_separator,
            tone: key.tone,
        })
        .collect();
    let mut additions: Vec<ScanKey> = Vec::new();
    for scan_key in &snapshot {
        if scan_key.tone != 0 {
            continue;
        }
        let Some((_, left, right)) = DIVIDED_TABLE
            .iter()
            .find(|(syllable, _, _)| *syllable == scan_key.key.text())
        else {
            continue;
        };
        let Some(left_key) = SyllableKey::from_text(left) else {
            continue;
        };
        let Some(right_key) = SyllableKey::from_text(right) else {
            continue;
        };
        let split = scan_key.syllable_start + left.len();
        additions.push(ScanKey {
            key: left_key,
            from: scan_key.from,
            to: split,
            syllable_start: scan_key.syllable_start,
            crosses_separator: scan_key.crosses_separator,
            tone: 0,
        });
        additions.push(ScanKey {
            key: right_key,
            from: split,
            to: scan_key.to,
            syllable_start: split,
            crosses_separator: false,
            tone: 0,
        });
    }
    for addition in &additions {
        columns[addition.from].push(*addition);
    }

    // Pre-fuzzy pin: first `SyllableKey` in a column. Fuzzy is off on the
    // parity word, so this is the all-off / 0x18a matrix.
    keep_first_in_column(&mut columns, false);

    // 4. `fuzzy_syllable_step`. Upstream `PhoneticTable::append` is a bag
    // push (`phonetic_key_matrix.h:92-99`); `ChewingKeyRest` is the span
    // (`chewing_key.h:97-104`). Same key, different `m_raw_end`, coexist.
    // After fuzzy, keep `(key, to)` so those edges survive; key-only
    // collapse here is #103. The tone rides the alternate — upstream
    // copies the whole key before swapping the initial or final
    // (`phonetic_key_matrix.cpp:250-259`).
    let snapshot: Vec<(usize, ScanKey)> = columns
        .iter()
        .enumerate()
        .flat_map(|(position, keys)| keys.iter().map(move |key| (position, *key)))
        .collect();
    let mut additions: Vec<ScanKey> = Vec::new();
    for (position, scan_key) in snapshot {
        for alternate in scan_key.key.fuzzy_alternatives(options) {
            additions.push(ScanKey {
                key: alternate,
                from: position,
                to: scan_key.to,
                syllable_start: scan_key.syllable_start,
                crosses_separator: scan_key.crosses_separator,
                tone: scan_key.tone,
            });
        }
    }
    for addition in &additions {
        columns[addition.from].push(*addition);
    }
    keep_first_in_column(&mut columns, true);

    columns
}

/// Keep the first column entry. `by_span` false is key-only (pre-fuzzy
/// pin); true is `(key, to)` (upstream Rest span).
fn keep_first_in_column(columns: &mut [Vec<ScanKey>], by_span: bool) {
    for column in columns {
        let mut kept = 0_usize;
        for index in 0..column.len() {
            let duplicate = column[..kept].iter().any(|earlier| {
                earlier.key == column[index].key && (!by_span || earlier.to == column[index].to)
            });
            if !duplicate {
                column.swap(kept, index);
                kept += 1;
            }
        }
        column.truncate(kept);
    }
}

/// Unigram sort key plus the DYNAMIC_ADJUST bigram increment (#99).
fn candidate_frequency_sort_key(options: OptionBits, unigram: u64) -> u64 {
    unigram.saturating_add(dynamic_adjust_bigram_term(options))
}

/// Bit-SET fold of `λ · bigram_poss · DISCOUNT` into RankKey: #99.
fn dynamic_adjust_bigram_term(options: OptionBits) -> u64 {
    let _ = options.has_dynamic_adjust();
    0
}

/// The three sort keys of the pinned candidate construction.
///
/// `Ord` derives the pinned precedence: phrase length first, then pinyin
/// span, then frequency. All comparisons run descending, so the stable sort
/// keeps collection order exactly when all three tie.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RankKey {
    /// Unicode scalar count of the candidate text.
    phrase_length: usize,
    /// Bytes of the raw input the candidate covers.
    pinyin_span: usize,
    /// Real unigram count from the model's frequency table.
    frequency: u64,
}

/// Keeps the first occurrence of every distinct candidate text, in order.
///
/// Full dedup rather than the adjacent-only `Vec::dedup_by`: the same text can
/// be reached through different spans or segmentations, and after the
/// three-key sort two copies need not be adjacent. Two-pass so the seen-set
/// can hold `&str` into the live candidates instead of cloning each kept
/// text.
fn dedup_by_text_keep_first(candidates: &mut Vec<Candidate>) {
    let mut keep = Vec::with_capacity(candidates.len());
    {
        let mut seen: HashSet<&str> = HashSet::with_capacity(candidates.len());
        keep.extend(
            candidates
                .iter()
                .map(|candidate| seen.insert(candidate.text())),
        );
    }
    let mut index = 0;
    candidates.retain(|_| {
        let kept = keep[index];
        index += 1;
        kept
    });
}

/// Whether the interactive key path accepts `character`.
///
/// `docs/findings/session-api.md` / `docs/findings/parser-spec.md`: only
/// lowercase ASCII `a`–`z` and the ASCII apostrophe. Everything else belongs
/// to the shell (`KeyOutcome::Ignored`).
const fn is_input_character(character: char) -> bool {
    character.is_ascii_lowercase() || character == '\''
}

/// Whether the batch path ([`Session::type_pinyin`]) accepts `character`.
///
/// Printable ASCII (`0x21..=0x7E`), including junk the parity corpus embeds in
/// inputs. The decoder (`SegmentGraph`) treats non-`a-z`/`'` bytes as hard
/// boundaries; see `docs/findings/f1-junk-aware-parse.md`. Space and controls
/// are excluded so they cannot bypass `LogicalKey::Space` / `Tab` / `Enter`.
const fn is_batch_input_character(character: char) -> bool {
    character.is_ascii_graphic()
}

#[cfg(test)]
mod tests {
    use oxpinyin_core::fixture::{FixtureDictionary, FixtureLanguageModel};
    use oxpinyin_core::{
        Cost, Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey, UserModel,
    };

    use super::{KeyOutcome, MAX_INPUT_BYTES, Selection, Session};
    use crate::config::EmptyConfigSource;
    use crate::error::EngineError;
    use crate::key::{KeyInput, LogicalKey, Modifiers};
    use crate::preedit::SpanStyle;
    use crate::storage::StoragePaths;

    /// A backend that answers nothing, so these tests measure the state
    /// machine and not a data set.
    struct Silent;

    impl Dictionary for Silent {
        type Entry = PhraseEntry;
        type Error = EngineError;
        type Syllable = SyllableKey;

        fn lookup(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
            Ok(Vec::new())
        }
    }

    impl LanguageModel for Silent {
        type Error = EngineError;
        type Token = PhraseToken;

        fn score(
            &self,
            _history: &[PhraseToken],
            _token: &PhraseToken,
            edge_cost: Cost,
        ) -> Result<Cost, EngineError> {
            Ok(edge_cost)
        }
    }

    fn session() -> Session<Silent, Silent> {
        Session::new(
            &EmptyConfigSource,
            StoragePaths::new("user"),
            Silent,
            Silent,
        )
        .expect("opening a session cannot fail yet")
    }

    fn type_text(session: &mut Session<Silent, Silent>, text: &str) {
        for character in text.chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
    }

    #[test]
    fn only_parser_syntax_extends_the_composition() {
        let mut session = session();
        type_text(&mut session, "ni'hao");
        assert_eq!(session.raw_input(), "ni'hao");
        assert!(session.is_composing());

        for ignored in ['N', '1', ' ', '!', '\u{4f60}'] {
            assert_eq!(
                session
                    .process_key(&KeyInput::character(ignored))
                    .expect("ignored keys cannot fail"),
                KeyOutcome::Ignored,
                "character: {ignored:?}"
            );
        }
        assert_eq!(session.raw_input(), "ni'hao");
    }

    #[test]
    fn type_pinyin_keeps_printable_junk_in_the_raw_buffer() {
        // Batch path (parity harness): printable ASCII including junk is kept
        // so the decoder sees the fixture string; process_key still ignores it.
        let mut session = session();
        session
            .type_pinyin("b#ing")
            .expect("batch typing cannot fail");
        assert_eq!(session.raw_input(), "b#ing");
    }

    #[test]
    fn command_modifiers_leave_the_session_alone() {
        let mut session = session();
        type_text(&mut session, "ni");

        for modifier in [Modifiers::CONTROL, Modifiers::ALT, Modifiers::SUPER] {
            let input = KeyInput::new(LogicalKey::Character('h'), modifier, "h");
            assert_eq!(
                session.process_key(&input).expect("no failure"),
                KeyOutcome::Ignored
            );
        }
        assert_eq!(session.raw_input(), "ni");

        let shifted = KeyInput::new(LogicalKey::Character('h'), Modifiers::SHIFT, "H");
        assert_eq!(
            session.process_key(&shifted).expect("no failure"),
            KeyOutcome::Consumed
        );
        assert_eq!(session.raw_input(), "nih");
    }

    #[test]
    fn backspace_erases_then_reports_nothing_to_do() {
        let mut session = session();
        type_text(&mut session, "ni");

        let backspace = KeyInput::plain(LogicalKey::Backspace);
        assert_eq!(
            session.process_key(&backspace).expect("no failure"),
            KeyOutcome::Consumed
        );
        assert_eq!(session.raw_input(), "n");
        session.process_key(&backspace).expect("no failure");
        assert_eq!(session.raw_input(), "");
        assert_eq!(
            session.process_key(&backspace).expect("no failure"),
            KeyOutcome::Ignored
        );
    }

    #[test]
    fn enter_commits_and_escape_discards() {
        let mut session = session();
        type_text(&mut session, "nihao");
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Enter))
                .expect("no failure"),
            KeyOutcome::Commit("nihao".to_owned())
        );
        assert!(!session.is_composing());
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Enter))
                .expect("no failure"),
            KeyOutcome::Ignored
        );

        type_text(&mut session, "nihao");
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Escape))
                .expect("no failure"),
            KeyOutcome::Consumed
        );
        assert_eq!(session.raw_input(), "");
    }

    #[test]
    fn keys_the_session_does_not_use_change_nothing() {
        let mut session = session();
        type_text(&mut session, "ni");

        for key in [
            LogicalKey::Tab,
            LogicalKey::Delete,
            LogicalKey::Left,
            LogicalKey::Right,
            LogicalKey::Up,
            LogicalKey::Down,
            LogicalKey::Home,
            LogicalKey::End,
            LogicalKey::PageUp,
            LogicalKey::PageDown,
            LogicalKey::Unknown,
        ] {
            assert_eq!(
                session
                    .process_key(&KeyInput::plain(key))
                    .expect("no failure"),
                KeyOutcome::Ignored,
                "key: {key:?}"
            );
        }
        assert_eq!(session.raw_input(), "ni");
    }

    #[test]
    fn the_preedit_covers_its_text_exactly() {
        let mut session = session();
        assert!(session.preedit().is_empty());

        type_text(&mut session, "nihao");
        let preedit = session.preedit();
        assert_eq!(preedit.text(), "nihao");
        assert_eq!(preedit.cursor(), 5);
        assert_eq!(preedit.spans().len(), 1);
        assert_eq!(preedit.spans()[0].style(), SpanStyle::Raw);
        assert_eq!(preedit.spans()[0].start(), 0);
        assert_eq!(preedit.spans()[0].end(), preedit.text().len());
    }

    #[test]
    fn a_stale_candidate_index_is_an_error_not_a_panic() {
        let mut session = session();
        type_text(&mut session, "nihao");
        let len = session.candidates().len();
        assert_eq!(len, 1, "only the raw fallback exists before the decoder");

        for index in [len, len + 1, usize::MAX] {
            assert_eq!(
                session.select(index),
                Err(EngineError::CandidateIndexOutOfRange { index, len })
            );
        }
        assert_eq!(session.raw_input(), "nihao");
        assert!(session.candidates().get(usize::MAX).is_none());
    }

    #[test]
    fn choosing_the_fallback_completes_the_composition() {
        let mut session = session();
        type_text(&mut session, "nihao");
        assert_eq!(
            session.select(0).expect("the fallback exists"),
            Selection::Completed
        );

        let preedit = session.preedit();
        assert_eq!(preedit.text(), "nihao");
        assert_eq!(preedit.spans().len(), 1);
        assert_eq!(preedit.spans()[0].style(), SpanStyle::Selected);
        assert!(session.candidates().is_empty());
        assert_eq!(session.commit().expect("no failure"), "nihao");
    }

    #[test]
    fn space_accepts_the_first_candidate_and_commits() {
        let mut session = session();
        type_text(&mut session, "nihao");
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Space))
                .expect("no failure"),
            KeyOutcome::Commit("nihao".to_owned())
        );
        assert!(!session.is_composing());
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Space))
                .expect("no failure"),
            KeyOutcome::Ignored
        );
    }

    #[test]
    fn backspace_undoes_a_selection_before_reporting_nothing_to_do() {
        let mut session = session();
        type_text(&mut session, "nihao");
        session.select(0).expect("the fallback exists");

        let backspace = KeyInput::plain(LogicalKey::Backspace);
        assert_eq!(
            session.process_key(&backspace).expect("no failure"),
            KeyOutcome::Consumed
        );
        assert_eq!(session.preedit().text(), "nihao");
        assert_eq!(session.preedit().spans()[0].style(), SpanStyle::Raw);
    }

    #[test]
    fn a_full_buffer_ignores_further_input() {
        // Apostrophes on purpose: they fill the buffer without building a
        // decodable graph, so this measures the bound and not the decoder.
        let mut session = session();
        for _ in 0..MAX_INPUT_BYTES {
            session
                .process_key(&KeyInput::character('\''))
                .expect("no failure");
        }
        assert_eq!(session.raw_input().len(), MAX_INPUT_BYTES);
        assert_eq!(
            session
                .process_key(&KeyInput::character('\''))
                .expect("no failure"),
            KeyOutcome::Ignored
        );
        assert_eq!(session.raw_input().len(), MAX_INPUT_BYTES);
    }

    #[test]
    fn configuration_and_paths_are_the_injected_data() {
        let session = session();
        assert_eq!(session.page_size(), 5);
        assert_eq!(session.paths().user_data_dir().to_str(), Some("user"));
    }

    #[test]
    fn commit_on_an_empty_session_is_empty_text() {
        let mut session = session();
        assert_eq!(session.commit().expect("no failure"), "");
    }

    /// Authored mini vocabulary for the training tests: two single-key
    /// phrases, no model bytes (`docs/findings/fixture-adapters.md`).
    const TRAIN_VOCAB: &str =
        "token=1\tkeys=ni\ttext=你\tunigram=1000\ntoken=2\tkeys=hao\ttext=好\tunigram=900\n";

    /// A [`UserModel`] that records every `observe` call instead of storing.
    struct Recorder {
        observed: Vec<(Vec<PhraseToken>, PhraseToken)>,
    }

    impl UserModel for Recorder {
        type Token = PhraseToken;
        type Error = EngineError;

        fn score(
            &self,
            _history: &[Self::Token],
            _token: &Self::Token,
        ) -> Result<Cost, Self::Error> {
            Ok(0)
        }

        fn observe(
            &mut self,
            history: &[Self::Token],
            token: &Self::Token,
        ) -> Result<(), Self::Error> {
            self.observed.push((history.to_vec(), *token));
            Ok(())
        }
    }

    fn train_session() -> Session<FixtureDictionary, FixtureLanguageModel> {
        Session::new(
            &EmptyConfigSource,
            StoragePaths::new("user"),
            FixtureDictionary::parse(TRAIN_VOCAB).expect("authored fixture"),
            FixtureLanguageModel::parse(TRAIN_VOCAB, "").expect("authored fixture"),
        )
        .expect("the fixtures open")
    }

    /// Selects the candidate carrying `token` after typing `text` (the
    /// selection must exist: this is the sentence record the training path
    /// walks).
    fn type_and_select(
        session: &mut Session<FixtureDictionary, FixtureLanguageModel>,
        text: &str,
        token: u32,
    ) {
        for character in text.chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
        let index = session
            .candidates()
            .iter()
            .position(|candidate| candidate.token() == Some(PhraseToken::new(token)))
            .expect("the fixture candidate is offered");
        session.select(index).expect("selection cannot fail");
    }

    #[test]
    fn train_observes_each_recorded_token_after_its_prefix() {
        let mut session = train_session();
        type_and_select(&mut session, "ni", 1);
        type_and_select(&mut session, "hao", 2);
        assert_eq!(
            session.selected_tokens(),
            [PhraseToken::new(1), PhraseToken::new(2)]
        );

        let mut recorder = Recorder {
            observed: Vec::new(),
        };
        session.train(&mut recorder).expect("training cannot fail");
        // First token observes against an empty history (the store maps that
        // to sentence_start); the second observes after the first.
        assert_eq!(
            recorder.observed,
            vec![
                (Vec::new(), PhraseToken::new(1)),
                (vec![PhraseToken::new(1)], PhraseToken::new(2)),
            ]
        );

        // Re-training re-observes the same sentence: upstream has no guard
        // (a second pinyin_train doubles the counts), and neither does this.
        session.train(&mut recorder).expect("training cannot fail");
        assert_eq!(recorder.observed.len(), 4);
    }

    #[test]
    fn train_reports_a_failing_user_model() {
        struct Failing;
        impl UserModel for Failing {
            type Token = PhraseToken;
            type Error = EngineError;

            fn score(
                &self,
                _history: &[Self::Token],
                _token: &Self::Token,
            ) -> Result<Cost, Self::Error> {
                Ok(0)
            }

            fn observe(
                &mut self,
                _history: &[Self::Token],
                _token: &Self::Token,
            ) -> Result<(), Self::Error> {
                Err(EngineError::UserModel("closed".to_owned()))
            }
        }

        let mut session = train_session();
        type_and_select(&mut session, "ni", 1);
        // The engine renders the model's error at the boundary: the failing
        // model reports an `EngineError::UserModel`, so the wrap doubles the
        // prefix — the point is that the failure surfaces, not that the text
        // is pretty.
        let error = session.train(&mut Failing).expect_err("the model fails");
        assert_eq!(
            error.to_string(),
            "user model error: user model error: closed"
        );
    }

    #[test]
    fn composition_keys_report_the_selected_parse() {
        let mut plain = session();
        type_text(&mut plain, "nihao");
        let keys = plain.composition_keys().expect("the graph builds");
        let texts: Vec<&str> = keys.iter().map(|key| key.text()).collect();
        assert_eq!(texts, ["ni", "hao"]);

        // The apostrophe keeps xi'an from collapsing into xian.
        let mut split = session();
        type_text(&mut split, "xi'an");
        let keys = split.composition_keys().expect("the graph builds");
        let texts: Vec<&str> = keys.iter().map(|key| key.text()).collect();
        assert_eq!(texts, ["xi", "an"]);
    }

    #[test]
    fn a_fallback_sentence_never_records_row_tokens() {
        use crate::nbest::NbestRow;

        let mut session = train_session();
        session.type_pinyin("nihao").expect("typing cannot fail");
        assert!(!session.sentence_lookup_active(), "no lookup has run yet");

        // One authored row whose text differs from the DP sentence the
        // fallback list also offers, so a fallback sentence candidate sits
        // beyond the row at a known place.
        session.nbest_rows = vec![NbestRow {
            text: "\u{884}".into(),
            tokens: vec![PhraseToken::new(9)],
            keys: 1,
            span: 2,
            cost: 0,
        }];
        session.refresh().expect("refresh cannot fail");
        assert!(
            !session.sentence_lookup_active(),
            "only guess_sentence activates the lookup"
        );

        // The row itself records its whole token path.
        assert_eq!(
            session.select(0).expect("the row is live"),
            Selection::Continued
        );
        assert_eq!(session.selected_tokens(), [PhraseToken::new(9)]);

        // A fallback sentence candidate — kind Sentence beyond the rows —
        // records nothing, and in particular never defaults through to
        // row zero's tokens.
        session.reset();
        session.type_pinyin("nihao").expect("typing cannot fail");
        session.nbest_rows = vec![NbestRow {
            text: "\u{884}".into(),
            tokens: vec![PhraseToken::new(9)],
            keys: 1,
            span: 2,
            cost: 0,
        }];
        session.refresh().expect("refresh cannot fail");
        let fallback = session
            .candidates()
            .iter()
            .position(|candidate| {
                candidate.kind() == crate::CandidateKind::Sentence
                    && candidate.text() == "\u{4f60}\u{597d}"
            })
            .expect("the fallback sentence is offered beyond the row");
        assert!(fallback >= session.nbest_rows.len());
        session.select(fallback).expect("the index is live");
        assert!(
            session.selected_tokens().is_empty(),
            "a fallback sentence records no tokens"
        );
    }

    #[test]
    fn a_shifted_row_records_its_own_rank_not_its_position() {
        use crate::nbest::NbestRow;

        let mut session = train_session();
        session.type_pinyin("nihao").expect("typing cannot fail");

        // Rows [好, 好, 浩]: the NBEST-wins dedup keeps the lower-index 好
        // and drops row 1, so the surviving 浩 row sits at list position 1
        // while its rank is 2 — the shape a positional record gets wrong
        // (the 你→浩 training divergence, `sentence-surface.md` §8).
        session.nbest_rows = vec![
            NbestRow {
                text: "\u{597d}".into(),
                tokens: vec![PhraseToken::new(0x100)],
                keys: 1,
                span: 3,
                cost: 10,
            },
            NbestRow {
                text: "\u{597d}".into(),
                tokens: vec![PhraseToken::new(0x101)],
                keys: 1,
                span: 3,
                cost: 20,
            },
            NbestRow {
                text: "\u{6d69}".into(),
                tokens: vec![PhraseToken::new(0x102)],
                keys: 1,
                span: 3,
                cost: 30,
            },
        ];
        session.refresh().expect("refresh cannot fail");

        let hao = session
            .candidates()
            .iter()
            .position(|candidate| candidate.nbest_row() == Some(2))
            .expect("the 浩 row survived the dedup");
        assert_eq!(
            hao, 1,
            "the deduped 好 row shifts the 浩 row off its own rank"
        );

        session.select(hao).expect("the row is live");
        assert_eq!(
            session.selected_tokens(),
            [PhraseToken::new(0x102)],
            "the chosen row records its own token path, not the deduped row 1's"
        );
    }

    #[test]
    fn a_lookup_activates_the_sentence_gate_even_without_rows() {
        let mut session = session();
        assert!(!session.sentence_lookup_active());
        // The Silent backends answer nothing: the lookup runs, finds no
        // rows, and still counts as active — upstream clears
        // m_nbest_results before every attempt.
        session
            .type_pinyin("qqq")
            .expect("batch typing cannot fail");
        assert!(
            session.guess_sentence().expect("guess cannot fail"),
            "the lookup ran"
        );
        assert!(session.sentence_lookup_active());
        assert_eq!(session.sentence_text(0), None);
        session.reset();
        assert!(!session.sentence_lookup_active());
    }

    #[test]
    fn normalized_lookup_offset_walks_the_zero_run_and_refuses_a_leading_one() {
        let mut session = session();
        session
            .type_pinyin("ni'hao")
            .expect("batch typing cannot fail");
        assert_eq!(session.normalized_lookup_offset(0), Ok(0));
        assert_eq!(
            session.normalized_lookup_offset(3),
            Ok(2),
            "one past the separator normalizes to the zero key's own byte"
        );
        assert_eq!(session.normalized_lookup_offset(2), Ok(2));

        session.reset();
        session
            .type_pinyin("ni''hao")
            .expect("batch typing cannot fail");
        assert_eq!(
            session.normalized_lookup_offset(4),
            Ok(2),
            "the whole run collapses to its first byte"
        );

        session.reset();
        session
            .type_pinyin("'ni")
            .expect("batch typing cannot fail");
        assert_eq!(
            session.normalized_lookup_offset(1),
            Err(EngineError::LookupOffsetPastSeparator {
                offset: 1,
                normalized: 1
            }),
            "the walk never crosses byte 0, so a leading run refuses \
             (_check_offset aborts upstream)"
        );
        assert_eq!(session.normalized_lookup_offset(0), Ok(0));

        session.reset();
        session
            .type_pinyin("ni'")
            .expect("batch typing cannot fail");
        assert_eq!(
            session.normalized_lookup_offset(3),
            Ok(2),
            "a trailing run normalizes without reading past the buffer"
        );
        assert_eq!(
            session.normalized_lookup_offset(9),
            Err(EngineError::LookupOffsetOutOfRange { offset: 9, len: 3 }),
            "an offset beyond one-past-end refuses before the walk"
        );
    }

    #[test]
    fn dynamic_adjust_does_not_fold_a_bigram_term_into_the_unigram_sort_key() {
        use oxpinyin_core::DYNAMIC_ADJUST;
        use oxpinyin_core::OptionBits;

        let unigram = 1_234;
        assert_eq!(
            super::candidate_frequency_sort_key(OptionBits::default(), unigram),
            unigram,
            "bit-clear omits the bigram term and keeps the unigram sort input"
        );
        assert_eq!(
            super::candidate_frequency_sort_key(
                OptionBits::default().with(DYNAMIC_ADJUST, true),
                unigram
            ),
            unigram,
            "bit-set leaves the W6-T4 unigram merge intact and does not invent a RankKey bigram"
        );
    }

    #[test]
    fn amplified_frequency_pins_the_class_a_probe_values() {
        // The denominator is the pin's phrase-index total over model20:
        // interpolation2 sum 50_913_735 + 138_096 items, each item's baked
        // unigram being its interpolation2 count + 1 (probe-verified over
        // the whole index; `docs/findings/corpus-tail.md` Class A). The
        // values are the amplified keys the 12 top-1 tie-swaps collapse on.
        const PIN_TOTAL: u64 = 51_051_831;
        // 0: the 量比/两笔, 建仓/减仓, 拜倒/白道, 冰坝/并把, 长着/唱着 pairs.
        assert_eq!(super::amplified_frequency(1, PIN_TOTAL), 0);
        assert_eq!(super::amplified_frequency(3, PIN_TOTAL), 0);
        // 3: 写歌 16 vs 写稿 14 (`xiego`).
        assert_eq!(super::amplified_frequency(14, PIN_TOTAL), 3);
        assert_eq!(super::amplified_frequency(16, PIN_TOTAL), 3);
        // 4: 古稀 21 vs 股息 20 (`guxi`), 酸楚 20 vs 算出 18 (`suanch`).
        assert_eq!(super::amplified_frequency(18, PIN_TOTAL), 4);
        assert_eq!(super::amplified_frequency(20, PIN_TOTAL), 4);
        assert_eq!(super::amplified_frequency(21, PIN_TOTAL), 4);
        // 17: 每家 78 vs 美加 77 (`meijia…`).
        assert_eq!(super::amplified_frequency(77, PIN_TOTAL), 17);
        assert_eq!(super::amplified_frequency(78, PIN_TOTAL), 17);
        // 19: 狗狗 = 沟谷 = 87 (`goug`).
        assert_eq!(super::amplified_frequency(87, PIN_TOTAL), 19);
        assert_eq!(super::amplified_frequency(0, PIN_TOTAL), 0);
        assert_eq!(
            super::amplified_frequency(20, 0),
            0,
            "no index total ranks as zero"
        );
    }

    #[test]
    fn amplified_frequency_is_c_float_not_f64() {
        // 2_349_890 is a corpus-scale count (interpolation2 tops out at
        // 3_081_671) where the C float chain and the same chain in f64
        // truncate apart — 530_766 vs 530_765 — so this pins the f32
        // arithmetic the oracle's m_freq runs in.
        assert_eq!(super::amplified_frequency(2_349_890, 51_051_831), 530_766);
    }

    #[test]
    fn window_flush_is_token_ascending_and_one_row_per_token() {
        use super::{CandidateKind, flush_window_batch};
        use crate::candidate::Candidate;

        let mut batch = vec![
            Candidate::new(
                compact_str::CompactString::from("狗狗"),
                CandidateKind::Phrase,
                2,
                4,
                0,
                Some(PhraseToken::new(0x0300_16df)),
                None,
            ),
            Candidate::new(
                compact_str::CompactString::from("沟谷"),
                CandidateKind::Phrase,
                2,
                4,
                0,
                Some(PhraseToken::new(0x0100_4c41)),
                None,
            ),
            Candidate::new(
                compact_str::CompactString::from("沟谷"),
                CandidateKind::Phrase,
                1,
                4,
                0,
                Some(PhraseToken::new(0x0100_4c41)),
                None,
            ),
        ];
        let mut into = Vec::new();
        flush_window_batch(&mut batch, &mut into);
        let tokens: Vec<u32> = into
            .iter()
            .filter_map(|c| c.token().map(|t| t.value()))
            .collect();
        assert_eq!(tokens, [0x0100_4c41, 0x0300_16df]);
        assert_eq!(
            into[0].consumed_keys(),
            2,
            "the first collected row of a duplicated token is the one kept"
        );
    }

    /// A model with fixed facade answers, so the frequency table's
    /// per-branch inputs are visible without a real table.
    struct FixedUnigrams {
        system: u64,
        addon: u64,
        total: u64,
        addon_total: u64,
    }

    impl LanguageModel for FixedUnigrams {
        type Error = EngineError;
        type Token = PhraseToken;

        fn score(
            &self,
            _history: &[PhraseToken],
            _token: &PhraseToken,
            edge_cost: Cost,
        ) -> Result<Cost, EngineError> {
            Ok(edge_cost)
        }

        fn has_real_unigrams(&self) -> bool {
            true
        }

        fn unigram_freq(&self, _token: &PhraseToken) -> Result<Option<u64>, EngineError> {
            Ok(Some(self.system))
        }

        fn unigram_total(&self) -> Result<Option<u64>, EngineError> {
            Ok(Some(self.total))
        }

        fn addon_unigram_freq(&self, _token: &PhraseToken) -> Result<Option<u64>, EngineError> {
            Ok(Some(self.addon))
        }

        fn addon_unigram_total(&self) -> Result<Option<u64>, EngineError> {
            Ok(Some(self.addon_total))
        }
    }

    #[test]
    fn addon_candidates_rank_on_their_own_amplified_scale() {
        use super::CandidateKind;
        use crate::candidate::Candidate;

        // The pin's two amplified branches (`pinyin.cpp:1829-1843` for the
        // addon, `:1855-1866` for the system): the system branch adds the
        // model20 +1 and divides by the index total; the addon branch
        // amplifies its own raw count over the addon facade's total. The
        // same raw count 14 therefore lands on 3 over 51,051,831 but 6
        // over the half-size addon total.
        let session = Session::new(
            &EmptyConfigSource,
            StoragePaths::new("user"),
            Silent,
            FixedUnigrams {
                system: 13,
                addon: 14,
                total: 51_051_831,
                addon_total: 25_525_916,
            },
        )
        .expect("Session::new");
        let collected = vec![
            Candidate::new(
                compact_str::CompactString::from("股"),
                CandidateKind::Phrase,
                1,
                3,
                0,
                Some(PhraseToken::new(1)),
                None,
            ),
            Candidate::new(
                compact_str::CompactString::from("附"),
                CandidateKind::Addon,
                1,
                3,
                0,
                Some(PhraseToken::new(2)),
                None,
            ),
        ];
        let frequencies = session
            .candidate_frequencies(&collected)
            .expect("frequency reads cannot fail here");
        assert_eq!(
            frequencies,
            Some(vec![3, 6]),
            "system = amplified(13 + 1, 51_051_831) = 3; addon = amplified(14, 25_525_916) = 6"
        );
    }

    /// One entry per facade, so a scan's two batches are exactly one
    /// system and one addon candidate.
    struct TwoFacadeDict {
        system: PhraseEntry,
        addon: PhraseEntry,
    }

    impl Dictionary for TwoFacadeDict {
        type Entry = PhraseEntry;
        type Error = EngineError;
        type Syllable = SyllableKey;

        fn lookup(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
            Ok(vec![self.system.clone()])
        }

        fn lookup_addon(
            &self,
            _syllables: &[SyllableKey],
        ) -> Result<Vec<PhraseEntry>, EngineError> {
            Ok(vec![self.addon.clone()])
        }
    }

    #[test]
    fn window_scan_emits_system_candidates_before_addon_candidates() {
        // Two one-character candidates whose three RankKeys tie (length 1,
        // span 1, amplified 3: system 13 + 1 over 51,051,831; addon 7 over
        // the half-size addon total). The stable sort must therefore keep
        // the scan's flush order — the default facade's batch before the
        // addon facade's, the array order `_append_items`
        // (`pinyin.cpp:1769-1791`) lays down.
        let mut session = Session::new(
            &EmptyConfigSource,
            StoragePaths::new("user"),
            TwoFacadeDict {
                system: PhraseEntry::new(PhraseToken::new(0x0100_0001), "系".to_owned()),
                addon: PhraseEntry::new(PhraseToken::new(0x0500_0002), "附".to_owned()),
            },
            FixedUnigrams {
                system: 13,
                addon: 7,
                total: 51_051_831,
                addon_total: 25_525_916,
            },
        )
        .expect("Session::new");
        session.type_pinyin("a").expect("typing cannot fail");
        let texts: Vec<&str> = session.candidates().iter().map(|c| c.text()).collect();
        assert_eq!(
            texts,
            ["系", "附"],
            "a full three-key tie must keep the pin's system-before-addon array order"
        );
    }

    #[test]
    fn scan_matrix_tone_rides_fuzzy_and_locks_the_split_tables() {
        use oxpinyin_core::graph::SegmentGraph;
        use oxpinyin_core::{OptionBits, PINYIN_AMB_Z_ZH, PINYIN_INCOMPLETE, USE_TONE};

        let incomplete = OptionBits::from_bits(PINYIN_INCOMPLETE);
        let toned = OptionBits::from_bits(PINYIN_INCOMPLETE | USE_TONE);

        // Fuzzy alternates inherit the tone: upstream copies the whole
        // ChewingKey before swapping the initial
        // (`phonetic_key_matrix.cpp:250-259`).
        let graph = SegmentGraph::build_with_options(b"zai4", toned).expect("valid");
        let columns = super::build_scan_matrix(
            &graph,
            OptionBits::from_bits(PINYIN_INCOMPLETE | USE_TONE | PINYIN_AMB_Z_ZH),
        );
        let column: Vec<_> = columns[0]
            .iter()
            .map(|key| (key.key.text(), key.to, key.tone))
            .collect();
        assert!(column.contains(&("zai", 4, 4)));
        assert!(column.contains(&("zhai", 4, 4)));

        // Resplit: ("a", "nan") is a live pair on the toneless walk; a toned
        // member locks it, because the table structs are zero-tone and the
        // pin matches the full ChewingKey.
        let toneless = SegmentGraph::build_with_options(b"anan", incomplete).expect("valid");
        let columns = super::build_scan_matrix(&toneless, incomplete);
        assert!(
            columns[0]
                .iter()
                .any(|key| key.key.text() == "an" && key.to == 2)
        );

        let toned_pair = SegmentGraph::build_with_options(b"a4nan", toned).expect("valid");
        let columns = super::build_scan_matrix(&toned_pair, toned);
        assert!(!columns[0].iter().any(|key| key.key.text() == "an"));

        // Divided: "bian" divides toneless; "bian4" carries its tone instead.
        let toneless = SegmentGraph::build_with_options(b"bian", incomplete).expect("valid");
        let columns = super::build_scan_matrix(&toneless, incomplete);
        assert!(
            columns[0]
                .iter()
                .any(|key| key.key.text() == "bi" && key.to == 2)
        );

        let toned_key = SegmentGraph::build_with_options(b"bian4", toned).expect("valid");
        let columns = super::build_scan_matrix(&toned_key, toned);
        assert!(
            columns[0]
                .iter()
                .any(|key| key.key.text() == "bian" && key.tone == 4)
        );
        assert!(!columns[0].iter().any(|key| key.key.text() == "bi"));
    }
}
