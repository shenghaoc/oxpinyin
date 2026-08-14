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

use pinyin_core::graph::{Edge, EdgeKind, SegmentGraph};
use pinyin_core::kbest::{DecodedPath, k_best};
use pinyin_core::scoring::{Scorer, ScoringConfig, ScoringError, expand_keys, key_cost_table};
use pinyin_core::{
    Completeness, Cost, Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey,
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

/// Largest number of candidates a refresh keeps.
///
/// Comfortably above the capture protocol's depth of ten, so the differential
/// can measure a prefix-10 overlap without the list being the thing that
/// truncates it.
const MAX_CANDIDATES: usize = 64;

/// Longest phrase, in keys, the sentence builder will look back for.
const MAX_PHRASE_KEYS: usize = 8;

/// Longest key sequence the window scan searches: the pin's phrase-length
/// cap. Paths beyond it are not searched.
const MAX_PHRASE_LENGTH: usize = 16;

/// The window scan's own expansion bound, separate from
/// [`pinyin_core::scoring::ScoringConfig::expansion_limit`] which the
/// pre-frequency fallback shares. Measured over the W2 corpus, the largest
/// expansion that hits real phrases is a three-initial `q|q|q` span
/// (14^3 = 2_744 — the pin's `qqq…` offers `请求权`); 4_096 covers it with
/// headroom. Larger products yield nothing: no stored phrase matches a
/// longer all-initial span.
const SCAN_EXPANSION_LIMIT: usize = 4_096;

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
    incomplete: bool,
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
        // nothing gets the parity behaviour.
        let incomplete = config.get_bool(KEY_INCOMPLETE).unwrap_or(true);
        Self {
            page_size,
            incomplete,
        }
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
    candidates: CandidateList,
    history: Vec<PhraseToken>,
    scoring: ScoringConfig,
    key_costs: Vec<Cost>,
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
            candidates: CandidateList::default(),
            history: Vec::new(),
            scoring: ScoringConfig::default(),
            key_costs,
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
        let Some(candidate) = self.candidates.get(index) else {
            return Err(EngineError::CandidateIndexOutOfRange {
                index,
                len: self.candidates.len(),
            });
        };

        let text = candidate.text().to_owned();
        let advance = candidate.consumed_bytes();
        let token = candidate.token();
        self.selected.push_str(&text);
        if let Some(token) = token {
            self.history.push(token);
        }
        self.consumed = self.next_boundary(self.consumed.saturating_add(advance));
        self.refresh()?;

        if self.consumed >= self.raw.len() {
            Ok(Selection::Completed)
        } else {
            Ok(Selection::Continued)
        }
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
        self.candidates = CandidateList::default();
        self.history.clear();
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
    #[must_use]
    pub const fn candidates(&self) -> &CandidateList {
        &self.candidates
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
        let remaining = self.raw[self.consumed..].to_owned();
        if remaining.is_empty() {
            self.candidates = CandidateList::default();
            return Ok(());
        }

        let graph = SegmentGraph::build(remaining.as_bytes()).map_err(EngineError::Graph)?;
        let scorer = Scorer::with_key_costs(
            self.scoring,
            &self.dictionary,
            &self.model,
            self.key_costs.clone(),
        );
        let paths = k_best(&graph, &scorer, SEGMENTATION_K).map_err(EngineError::Decode)?;

        // When the model carries the phrase index's real unigram
        // frequencies, the pinned construction runs — the expanding-window
        // scan, the three-key order (text length, pinyin span, frequency),
        // keep-first dedup, and only the raw-input fallback after an empty
        // result. Without real frequencies the session reproduces its
        // pre-frequency behaviour exactly: k-best prefixes, sentence
        // candidates, cost order, adjacent dedup.
        let mut collected: Vec<Candidate> = Vec::new();
        if self.model.has_real_unigrams() {
            self.collect_window_scan(&graph, remaining.as_bytes(), &mut collected)?;

            // The scan's result stands even when it found nothing. Tokens the
            // table lacks rank as zero rather than falling back.
            let frequencies = self
                .candidate_frequencies(&collected)?
                .unwrap_or_else(|| vec![0; collected.len()]);
            let mut keyed: Vec<(RankKey, Candidate)> = collected
                .into_iter()
                .zip(frequencies)
                .map(|(candidate, frequency)| {
                    let key = RankKey {
                        phrase_length: candidate.text().chars().count(),
                        pinyin_span: candidate.consumed_bytes(),
                        frequency,
                    };
                    (key, candidate)
                })
                .collect();

            // Stable sort, all three keys descending: an all-equal tie keeps
            // the collection order, which is the deterministic stand-in for
            // the pin's internal collection order.
            keyed.sort_by_key(|(key, _)| core::cmp::Reverse(*key));
            collected = keyed.into_iter().map(|(_, candidate)| candidate).collect();

            dedup_by_text_keep_first(&mut collected);
        } else {
            for path in &paths {
                self.collect_prefix_phrases(&graph, &scorer, path, &mut collected)?;
                self.collect_sentence(&graph, &scorer, path, &mut collected)?;
            }
            collected.sort_by_key(Candidate::cost);
            collected.dedup_by(|left, right| left.text() == right.text());
        }

        collected.truncate(MAX_CANDIDATES);

        if collected.is_empty() {
            collected.push(Candidate::new(
                remaining.clone(),
                CandidateKind::Fallback,
                0,
                remaining.len(),
                0,
                None,
            ));
        }

        self.candidates = CandidateList::from_vec(collected);
        Ok(())
    }

    /// Per-candidate real unigram frequencies, or `None` when the model
    /// carries no real frequency table at all.
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
        for (index, candidate) in collected.iter().enumerate() {
            let Some(token) = candidate.token() else {
                continue;
            };
            let count = self.model.unigram_freq(&token).map_err(|error| {
                EngineError::Scoring(ScoringError::LanguageModel(error.to_string()))
            })?;
            if let Some(count) = count {
                let table = frequencies.get_or_insert_with(|| vec![0; collected.len()]);
                table[index] = count;
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
                into.push(Candidate::new(
                    entry.text().to_owned(),
                    CandidateKind::Phrase,
                    length,
                    ends[length - 1],
                    cost,
                    Some(entry.token()),
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
    /// pooled phrase candidates only — the pin surfaces no sentence-level
    /// candidates under the pinned observation (`nihaoshi` never lists
    /// `你好是`).
    fn collect_sentence(
        &self,
        graph: &SegmentGraph,
        scorer: &Scorer<'_, D, L>,
        path: &DecodedPath,
        into: &mut Vec<Candidate>,
    ) -> Result<(), EngineError> {
        let (keys, kinds, ends) = self.walk(graph, path);
        if keys.is_empty() {
            return Ok(());
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

        if let Some((cost, text, _)) = best[keys.len()].clone()
            && !text.is_empty()
        {
            into.push(Candidate::new(
                text,
                CandidateKind::Sentence,
                keys.len(),
                ends[keys.len() - 1],
                cost,
                None,
            ));
        }
        Ok(())
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
            if !self.settings.incomplete && edge.kind() == EdgeKind::Incomplete {
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
        into: &mut Vec<Candidate>,
    ) -> Result<(), EngineError> {
        let matrix = build_scan_matrix(graph, self.settings.incomplete);
        let bound = graph.consumed();
        let mut end = 1usize;
        while end <= bound {
            // An end position no key starts at is an empty column: widen.
            let mut continued = matrix.get(end).is_none_or(|column| column.is_empty());
            let mut path: Vec<SyllableKey> = Vec::with_capacity(MAX_PHRASE_LENGTH);
            self.scan_paths(&matrix, 0, end, &mut path, into, &mut continued)?;
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
        path: &mut Vec<SyllableKey>,
        into: &mut Vec<Candidate>,
        continued: &mut bool,
    ) -> Result<(), EngineError> {
        let Some(column) = matrix.get(node) else {
            return Ok(());
        };
        for scan_key in column.iter().copied() {
            self.visit_scan_key(matrix, scan_key, end, path, into, continued)?;
        }
        Ok(())
    }

    /// One matrix key during the scan.
    fn visit_scan_key(
        &self,
        matrix: &[Vec<ScanKey>],
        scan_key: ScanKey,
        end: usize,
        path: &mut Vec<SyllableKey>,
        into: &mut Vec<Candidate>,
        continued: &mut bool,
    ) -> Result<(), EngineError> {
        let to = scan_key.to;
        if to > end {
            // A key overhanging the window: the phrase could continue, which is
            // upstream's `longest > end` CONTINUED.
            *continued = true;
            return Ok(());
        }
        path.push(scan_key.key);
        if to == end {
            self.search_scan_path(path, end, into, continued)?;
        } else if path.len() < MAX_PHRASE_LENGTH {
            self.scan_paths(matrix, to, end, path, into, continued)?;
        }
        path.pop();
        Ok(())
    }

    /// The table search on one complete key-path, and the prefix probe that
    /// decides whether the window keeps widening.
    fn search_scan_path(
        &self,
        path: &[SyllableKey],
        end: usize,
        into: &mut Vec<Candidate>,
        continued: &mut bool,
    ) -> Result<(), EngineError> {
        let has_incomplete = path
            .iter()
            .any(|key| key.completeness() == Completeness::Partial);

        let mut sequences: Vec<Vec<SyllableKey>> = Vec::new();
        if has_incomplete {
            sequences = expand_keys(path, SCAN_EXPANSION_LIMIT);
        } else {
            sequences.push(path.to_vec());
        }
        for sequence in &sequences {
            let entries = self.dictionary.lookup(sequence).map_err(|error| {
                EngineError::Scoring(ScoringError::Dictionary(error.to_string()))
            })?;
            append_scan_entries(entries, path.len(), end, into);
        }

        let can_extend = self
            .dictionary
            .phrase_prefix_exists(path)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        *continued |= can_extend;
        Ok(())
    }
}

/// Pushes the phrases one key-path search returned.
fn append_scan_entries(
    entries: Vec<PhraseEntry>,
    keys: usize,
    end: usize,
    into: &mut Vec<Candidate>,
) {
    for entry in entries {
        into.push(Candidate::new(
            entry.text().to_owned(),
            CandidateKind::Phrase,
            keys,
            end,
            0,
            Some(entry.token()),
        ));
    }
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

/// One key of the scan matrix at its byte position, with the byte position
/// it ends at and where its own text starts — the two differ from
/// `from + len` exactly when the key rides over an apostrophe separator.
#[derive(Clone, Copy)]
struct ScanKey {
    key: SyllableKey,
    from: usize,
    to: usize,
    syllable_start: usize,
    crosses_separator: bool,
}

impl ScanKey {
    fn from_edge(edge: &Edge) -> Self {
        Self {
            key: edge.key(),
            from: edge.from(),
            to: edge.to(),
            syllable_start: edge.syllable_start(),
            crosses_separator: edge.crosses_separator(),
        }
    }
}

/// The keys the pin's matrix holds per byte position: the selected parse's
/// keys, plus the resplit and divided additions. See
/// `docs/findings/matrix-split-tables.md` for the frozen pair lists and the
/// construction they reproduce.
fn build_scan_matrix(graph: &SegmentGraph, allow_incomplete: bool) -> Vec<Vec<ScanKey>> {
    let bound = graph.consumed();
    let mut columns: Vec<Vec<ScanKey>> = vec![Vec::new(); bound + 1];

    // 1. The selected parse.
    let selected_edges = selected_path(graph, allow_incomplete);
    let selected: Vec<ScanKey> = selected_edges.iter().map(ScanKey::from_edge).collect();
    for scan_key in &selected {
        columns[scan_key.from].push(*scan_key);
    }

    // 2. Resplit pairs along the selected path. A pair only resplits when
    // the two keys share a boundary with no apostrophe between them: the
    // pin fills a zero key at a separator, so its pairs never span one.
    let mut additions: Vec<ScanKey> = Vec::new();
    for pair in selected.windows(2) {
        if pair[1].from != pair[0].to || pair[0].crosses_separator || pair[1].crosses_separator {
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
        });
        additions.push(ScanKey {
            key: right_key,
            from: split,
            to: pair[1].to,
            syllable_start: split,
            crosses_separator: false,
        });
    }
    for addition in &additions {
        columns[addition.from].push(*addition);
    }

    // 3. Divided syllables over every key collected so far. The split parts
    // are measured from the syllable text itself, so a key that rides over
    // an apostrophe still divides (`bu'tian` offers `补体` from the divided
    // `ti`, whose span covers the apostrophe plus `t` + `i`).
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
        })
        .collect();
    let mut additions: Vec<ScanKey> = Vec::new();
    for scan_key in &snapshot {
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
        });
        additions.push(ScanKey {
            key: right_key,
            from: split,
            to: scan_key.to,
            syllable_start: split,
            crosses_separator: false,
        });
    }
    for addition in &additions {
        columns[addition.from].push(*addition);
    }

    // Keep the first occurrence of each key in a column.
    for column in &mut columns {
        let mut kept = 0_usize;
        for index in 0..column.len() {
            if !column[..kept]
                .iter()
                .any(|earlier| earlier.key == column[index].key)
            {
                column.swap(kept, index);
                kept += 1;
            }
        }
        column.truncate(kept);
    }

    columns
}

/// The parse the scan matrix is built from: the fewest-keys segmentation of
/// the reachable input, ties broken by first-found in left-to-right,
/// shortest-key-first order — the selection `candidate-construction.md` §8.1
/// freezes (prefer the longest parsed length, then fewest keys; every
/// admitted key carries distance zero under the pinned options, so the first
/// candidate wins ties).
fn selected_path(graph: &SegmentGraph, allow_incomplete: bool) -> Vec<Edge> {
    let bound = graph.consumed();
    // steps[node] = (key count, incoming edge, previous node); node 0 is the
    // root with count zero.
    let mut steps: Vec<Option<(usize, Edge, usize)>> = vec![None; bound + 1];

    for node in 0..bound {
        let count = if node == 0 {
            0
        } else {
            match steps[node] {
                Some((count, _, _)) => count,
                None => continue,
            }
        };
        let mut edges: Vec<Edge> = graph
            .outgoing(node)
            .iter()
            .filter(|edge| allow_incomplete || edge.kind() != EdgeKind::Incomplete)
            .copied()
            .collect();
        // Upstream tries key lengths ascending at each position.
        edges.sort_by_key(|edge| (edge.to() - edge.from(), edge.key().index()));
        for edge in edges {
            let to = edge.to();
            if to > bound {
                continue;
            }
            let candidate = count + 1;
            let replace = steps[to]
                .as_ref()
                .is_none_or(|(seen, _, _)| candidate < *seen);
            if replace {
                steps[to] = Some((candidate, edge, node));
            }
        }
    }

    let mut path = Vec::new();
    let mut node = bound;
    while node > 0 {
        let Some((_, edge, previous)) = steps[node] else {
            break;
        };
        path.push(edge);
        node = previous;
    }
    path.reverse();
    path
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
/// three-key sort two copies need not be adjacent. One heap allocation per
/// kept text; the candidate count per refresh is a few hundred at most.
fn dedup_by_text_keep_first(candidates: &mut Vec<Candidate>) {
    let mut seen: HashSet<String> = HashSet::with_capacity(candidates.len());
    candidates.retain(|candidate| {
        let text = candidate.text();
        if seen.contains(text) {
            false
        } else {
            seen.insert(text.to_owned());
            true
        }
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
    use pinyin_core::{Cost, Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};

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
}
