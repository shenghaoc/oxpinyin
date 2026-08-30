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

use oxpinyin_core::graph::{Edge, EdgeKind, ExactSegment, SegmentGraph};
use oxpinyin_core::kbest::{DecodedPath, k_best};
use oxpinyin_core::scoring::{Scorer, ScoringConfig, ScoringError, expand_keys, key_cost_table};
use oxpinyin_core::{
    Completeness, Cost, Dictionary, LanguageModel, MergedGram, OptionBits, PhraseEntry,
    PhraseToken, SyllableKey, UserModel,
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
    /// Pre-parsed exact syllables over `raw` — the scheme-parse seam
    /// (zhuyin, double pinyin). Empty is the full-pinyin mode, where the
    /// scan parses `raw` itself; non-empty pins the graph to exactly
    /// these keys, mirroring upstream's decoder receiving the scheme
    /// parser's `ChewingKey`s (`docs/findings/bopomofo-spec.md` — the
    /// joined text must never be re-segmented by the pinyin inventory).
    /// Spans are absolute over the whole `raw` buffer; any input
    /// replacement re-establishes them wholesale.
    exact_segments: Vec<ExactSegment>,
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
    /// Whether a selection consumed the whole buffer — the commit-branch
    /// shape. A composition completed by choosing re-parses fresh (the
    /// frontend's reset-between-compositions contract, the #141 cursor
    /// flows' pinned rule); a composition the buffer shrank INTO (the
    /// cursor never moved, the backspace ate the tail) stays open, so a
    /// re-extension continues it with the surviving forcings.
    selection_committed: bool,
    /// The §3 constraint store — one cell per raw-buffer byte position,
    /// the coordinate space the scan matrix and the choose cursor share.
    /// Survives `reset_composition` (the parse path) exactly as
    /// upstream's instance-level `m_constraints` survive
    /// `pinyin_parse_more_full_pinyins`; cleared only by the full
    /// [`Session::reset`] (`pinyin_reset`'s rule).
    constraints: crate::constraint::ConstraintStore,
    /// The last sentence lookup's 1-best phrases at their absolute
    /// positions — upstream's `m_nbest_results[0]`, the result
    /// `pinyin_train` walks against the constraint store
    /// (`train_result3`). Cleared wherever the rows are.
    last_result: Vec<crate::constraint::PhraseSpan>,
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
        let key_costs = key_cost_table(&dictionary, &model)?;
        Ok(Self {
            dictionary,
            model,
            paths,
            settings: Settings::read(config),
            raw: String::new(),
            selected: String::new(),
            consumed: 0,
            parsed_prefix: 0,
            exact_segments: Vec::new(),
            candidates: CandidateList::default(),
            history: Vec::new(),
            scoring: ScoringConfig::default(),
            key_costs,
            nbest_rows: Vec::new(),
            nbest_history: Vec::new(),
            sentence_lookup_active: false,
            selection_committed: false,
            constraints: crate::constraint::ConstraintStore::default(),
            last_result: Vec::new(),
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
        // Ignored input leaves an exact (scheme-parsed) composition alone;
        // only a real buffer change exits exact mode.
        self.exact_segments.clear();
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
        // Clone the candidate out of the cached list first: `select_inner`
        // borrows `self` mutably, so it cannot also borrow `self.candidates`.
        let candidate =
            self.candidates
                .get(index)
                .cloned()
                .ok_or(EngineError::CandidateIndexOutOfRange {
                    index,
                    len: self.candidates.len(),
                })?;
        self.select_inner(self.consumed, &candidate, None)
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
        let candidate =
            self.candidates
                .get(index)
                .cloned()
                .ok_or(EngineError::CandidateIndexOutOfRange {
                    index,
                    len: self.candidates.len(),
                })?;
        self.select_inner(self.consumed, &candidate, Some(promoted_token))
    }

    /// Chooses the candidate at `index` from an explicit candidate window
    /// (a re-anchored `candidates_at` list), not the session's cached list.
    ///
    /// `pinyin_guess_candidates` at an offset other than the composition's
    /// own rebuilds the window the caller sees at that offset; a subsequent
    /// `pinyin_choose_candidate` must resolve its index against that SAME
    /// window, or it would select a different row from the composition-
    /// anchored cached list whenever the two differ. The selection record,
    /// constraint span, and consumed advance are otherwise identical to
    /// [`Session::select`] — they come from the candidate and the session
    /// state, not from which list holds the candidate.
    ///
    /// # Errors
    ///
    /// Same as [`Session::select`]: [`EngineError::CandidateIndexOutOfRange`]
    /// for an index the given window does not hold, and
    /// [`EngineError::LookupOffsetOutOfRange`] for an anchor past the raw
    /// input's end.
    pub fn select_anchored(
        &mut self,
        index: usize,
        window: &CandidateList,
        anchor: usize,
    ) -> Result<Selection, EngineError> {
        if anchor > self.raw.len() {
            return Err(EngineError::LookupOffsetOutOfRange {
                offset: anchor,
                len: self.raw.len(),
            });
        }
        let candidate = window
            .get(index)
            .ok_or(EngineError::CandidateIndexOutOfRange {
                index,
                len: window.len(),
            })?;
        self.select_inner(anchor, candidate, None)
    }

    /// [`Session::select_anchored`] with an addon-promotion token override,
    /// mirroring [`Session::select_promoted`].
    ///
    /// # Errors
    ///
    /// Same as [`Session::select_anchored`].
    pub fn select_anchored_promoted(
        &mut self,
        index: usize,
        window: &CandidateList,
        anchor: usize,
        promoted_token: PhraseToken,
    ) -> Result<Selection, EngineError> {
        if anchor > self.raw.len() {
            return Err(EngineError::LookupOffsetOutOfRange {
                offset: anchor,
                len: self.raw.len(),
            });
        }
        let candidate = window
            .get(index)
            .ok_or(EngineError::CandidateIndexOutOfRange {
                index,
                len: window.len(),
            })?;
        self.select_inner(anchor, candidate, Some(promoted_token))
    }

    /// Selects the candidate at `index`, which must not alias `self` —
    /// either an owned clone of a cached candidate (the composition case)
    /// or a reference into an external window.
    fn select_inner(
        &mut self,
        anchor: usize,
        candidate: &Candidate,
        token_override: Option<PhraseToken>,
    ) -> Result<Selection, EngineError> {
        let text = candidate.text().to_owned();
        let advance = candidate.consumed_bytes();
        let token = token_override.or_else(|| candidate.token());
        // The chosen span in the store's coordinates. For the composition-
        // anchored cached list, `anchor` is the composition offset and the
        // candidate's `consumed_bytes` is measured from it; for a re-anchored
        // window, `anchor` is that window's caller offset and the candidate's
        // `consumed_bytes` is measured from IT (the pin's `m_begin = start`,
        // `pinyin.cpp:2227`). Both drifts are the same expression: the span
        // starts at `anchor` and advances by the candidate's own byte span.
        let constraint_start = anchor;
        let constraint_end = self.next_boundary(anchor.saturating_add(advance));
        // Reject a window anchor before the composition offset: the
        // candidate's span would regress `self.consumed`, a backward
        // selection no frontend drives (a stale cursor behind the
        // selection). Rejected, not reconciled — the gap handling below
        // covers only the anchor == / > composition-offset shapes.
        if anchor < self.consumed {
            return Err(EngineError::SelectionAnchorBeforeComposition {
                anchor,
                composition: self.consumed,
            });
        }
        // The raw bytes between the composition offset and the window anchor
        // were typed without being selected. For a re-anchored selection
        // (anchor > composition offset) they would otherwise be dropped from
        // the committed/preedit text — the same gap the constraint rebuild
        // preserves (`rebuild_selection_from_constraints`). The composition-
        // anchored path (anchor == composition offset) has an empty gap.
        let gap = if anchor > self.consumed {
            self.raw.get(self.consumed..anchor).unwrap_or("")
        } else {
            ""
        };
        if candidate.nbest_row().is_some() {
            // An n-best row is a whole-composition hypothesis: its text
            // already covers the full input and its span (consumed_bytes)
            // is the whole composition, so a re-anchored selection must not
            // prepend the typed-but-unselected gap — that would duplicate
            // the raw prefix in the committed text (upstream commits the
            // row's sentence text, pinyin_choose_candidate's NBEST branch
            // returning matrix.size() - 1). The composition-anchored path
            // has an empty gap either way. The clone keeps `text`
            // alive for the constraint write below.
            self.selected = text.clone();
        } else {
            self.selected.push_str(gap);
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
        // The §3 constraint writes (`pinyin_choose_candidate`,
        // `pinyin.cpp:2576-2584`): a token-bearing candidate forces its
        // span; an n-best row constrains only the phrases where it
        // differs from the 1-best (`diff_result`) — a row-0 choose
        // constrains nothing, exactly upstream.
        if let Some(rank) = candidate.nbest_row() {
            let best = self.nbest_rows.first().map(|row| row.spans.as_slice());
            let Some(chosen) = self.nbest_rows.get(usize::from(rank)) else {
                self.consumed = constraint_end;
                self.refresh()?;
                return self.selection_outcome();
            };
            if let Some(best) = best {
                // Upstream validates the store at choose time
                // (`pinyin.cpp:2576-2580`); the engine sizes it here so a
                // fresh composition's row choose — whose store was never
                // resized, `add` refusing every span past an empty cell
                // count — cannot silently write nothing.
                self.constraints.resize(self.raw.len() + 1);
                self.constraints
                    .diff_result(best, &chosen.spans, self.raw.len());
            }
        } else if let Some(token) = token {
            self.constraints.resize(self.raw.len() + 1);
            self.constraints.add(
                constraint_start,
                constraint_end,
                token,
                compact_str::CompactString::from(text.as_str()),
            );
        }
        self.consumed = constraint_end;
        self.selection_committed = constraint_end >= self.raw.len();
        self.refresh()?;

        self.selection_outcome()
    }

    /// The common tail of [`Session::select_inner`].
    fn selection_outcome(&self) -> Result<Selection, EngineError> {
        if self.consumed >= self.raw.len() {
            Ok(Selection::Completed)
        } else {
            Ok(Selection::Continued)
        }
    }

    /// Trains the recorded sentence through the user-model seam.
    ///
    /// The §3 constraint-aware walk (`train_result3`,
    /// `phonetic_lookup.h:841-935`): the last sentence lookup's 1-best
    /// result is walked phrase by phrase against the constraint store — a
    /// phrase trains when it is user-forced (`OneStep`) or when
    /// `train_next` is set (the first decoded phrase after each forced
    /// run, where propagation stops), and the bigram predecessor advances
    /// over **every** phrase, trained or not. A user who forces 你 for
    /// "ni" and commits the decoded 好 therefore trains 你→好, not just
    /// sentence_start→你 (the L3 surface, `docs/findings/live-typing.md`).
    ///
    /// Without a decoded result — the fixture fallback models, or no
    /// lookup since the last reset — the selection history stands in: one
    /// [`UserModel::observe`] per pinned token with the preceding tokens
    /// as context (`docs/findings/user-store.md` §2.1). The C ABI's
    /// `pinyin_train` is this call; per-candidate selection only records
    /// the constraint, and the bigram update is deferred to here (§2.2).
    /// Learning-off callers omit it entirely.
    ///
    /// Re-calling without new selections re-observes the same sentence,
    /// which is the upstream behaviour (a second `pinyin_train` doubles
    /// the counts — there is no guard upstream either).
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
        // The constrained walk applies only when the result actually
        // sits on user forcings — some phrase of the last lookup's 1-best
        // lands on a OneStep cell. Everything else falls to the selection
        // record: a row-0 choose constrains nothing (exactly upstream,
        // `diff_result` adds cells only for the differing phrases) yet the
        // engine's row chooses still record tokens, and a model that
        // cannot run the constrained walk (the pre-frequency fallback)
        // leaves the result without spans at all. The record is the
        // stand-in for exactly those shapes — it cannot mask a missed
        // cell: a forcing that failed to record changes the row and
        // window surfaces the differential probes, not the train output.
        let constrained = self
            .last_result
            .iter()
            .any(|span| self.constraints.is_one_step_at(span.start));
        if constrained {
            let mut context: Vec<PhraseToken> = Vec::with_capacity(self.last_result.len());
            let mut train_next = false;
            for span in &self.last_result {
                let forced = self.constraints.is_one_step_at(span.start);
                if train_next || forced {
                    train_next = forced;
                    user.observe(&context, &span.token)
                        .map_err(|error| EngineError::UserModel(error.to_string()))?;
                }
                context.push(span.token);
            }
            return Ok(());
        }
        for (index, token) in self.history.iter().enumerate() {
            user.observe(&self.history[..index], token)
                .map_err(|error| EngineError::UserModel(error.to_string()))?;
        }
        Ok(())
    }

    /// Clears the constraint run at `offset` — `pinyin_clear_constraint`
    /// (`pinyin.cpp:2641-2647`). The offset indexes the store's
    /// coordinate space (raw-buffer byte positions, #141's law); a hit
    /// anywhere inside a forced run un-forces the whole run. The
    /// selection record follows the surviving forcings, so the cleared
    /// phrase's text leaves the preedit and its token leaves the record.
    ///
    /// Returns `false` for a free cell or an out-of-range offset —
    /// upstream's own defined return, never an abort.
    #[must_use]
    pub fn clear_constraint(&mut self, offset: usize) -> bool {
        if !self.constraints.clear_by_offset(offset) {
            return false;
        }
        self.rebuild_selection_from_constraints();
        true
    }

    /// Rebuilds the selection record (`selected`, `consumed`, `history`)
    /// from the surviving forcings. Upstream keeps no such record — the
    /// frontend tracks its own cursor — so the store is the engine's
    /// single source once forcings exist.
    fn rebuild_selection_from_constraints(&mut self) {
        let runs = self.constraints.runs();
        if runs.is_empty() {
            self.selected.clear();
            self.consumed = 0;
            self.history.clear();
            self.selection_committed = false;
            return;
        }
        // Gaps between forced runs are free spans (diff_result forces only
        // the differing phrases); their text is the current buffer's bytes,
        // so the rebuilt record never drops raw input the forcings skip
        // over — the preedit would otherwise lose exactly that gap.
        let mut selected = String::new();
        let mut cursor = 0_usize;
        let mut history = Vec::with_capacity(runs.len());
        for (start, end, token, text) in &runs {
            if *start > cursor
                && let Some(gap) = self.raw.get(cursor..*start)
            {
                selected.push_str(gap);
            }
            selected.push_str(text);
            history.push(*token);
            cursor = *end;
        }
        self.selected = selected;
        self.consumed = cursor.min(self.raw.len());
        self.history = history;
        // A rebuild means the record changed under the selection — a
        // cleared run or a validate drop — so the commit-branch shape no
        // longer holds even when the surviving forcings still reach the
        // buffer end. Leaving the flag set would make the next compatible
        // re-parse start fresh and silently drop the survivors.
        self.selection_committed = false;
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
        let graph = self.build_graph_at(0, self.raw.as_bytes())?;
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
    ///
    /// The full reset — upstream's `pinyin_reset`: the input, the
    /// selection record, the n-best rows, and the constraint store all
    /// go (`pinyin.cpp:2697` clears `m_constraints`).
    pub fn reset(&mut self) {
        self.reset_composition();
        self.exact_segments.clear();
        self.raw.clear();
        self.selected.clear();
        self.consumed = 0;
        self.selection_committed = false;
        self.history.clear();
        self.constraints.clear();
    }

    /// The parse-path reset: the composition's PARSE state goes; the
    /// raw input, the selection record, and the constraint store stay.
    ///
    /// `pinyin_parse_more_full_pinyins` replaces the input buffer — the
    /// frontend re-sends the whole buffer every keystroke — and never
    /// touches upstream's instance-level `m_constraints` or the chosen
    /// cursor (`pinyin.cpp:1497-1533`); the next `guess_sentence`
    /// re-validates the surviving forcings against the new matrix. This
    /// is that split's engine half: the L2 lifetime rule
    /// (`docs/findings/live-typing.md`).
    ///
    /// The raw buffer is NOT cleared here: a cleared raw with a surviving
    /// cursor would leave `consumed > raw.len()` observable, and every
    /// `raw[consumed..]` slice (`preedit`, `commit`) would panic — the
    /// constitution forbids that window. The input replacement is atomic
    /// in [`Session::replace_raw`], which clears, refills, clamps, and
    /// refreshes in one call, so this reset alone always leaves a
    /// consistent session.
    pub fn reset_composition(&mut self) {
        self.parsed_prefix = 0;
        self.candidates = CandidateList::default();
        self.nbest_rows.clear();
        self.nbest_history.clear();
        self.last_result.clear();
        self.sentence_lookup_active = false;
    }

    /// Replaces the raw input with `text` in one step — the capi parse
    /// path's `parse_more` contract (the frontend re-sends the whole
    /// buffer every keystroke). The selection record and the constraint
    /// store survive (whether they should is the caller's
    /// [`Session::parse_continues`] decision); the cursor is clamped into
    /// the new buffer and the candidates refresh, so the session is never
    /// observable with a cursor past its input. A replacement that does
    /// not extend the covered selection span — a clamp below it, or a
    /// byte divergence inside it — reconciles the store and record
    /// ([`Session::reconcile_replaced_selection`]), so
    /// [`Session::commit`] answers only text valid for the current
    /// input.
    ///
    /// Keeps every character: the pin's parser accepts any input string
    /// and simply stops consuming at the first byte no key matches
    /// (`pinyin_parser2.cpp:237-328` — there is no explicit stop, the
    /// termination is the DP's reachability), so space, control, and
    /// non-ASCII bytes must REACH the decoder for it to stop there
    /// (class B2 of `uncovered-surface-differentials.md`). The decoder
    /// hard-stops on them; this seam must not pre-filter them away.
    ///
    /// The batch [`Session::type_pinyin`] keeps its printable-ASCII
    /// accept set (the frozen F1 design, `f1-junk-aware-parse.md`): the
    /// two seams are deliberately different. The corpus and sentence pins
    /// feed through `type_pinyin` only — no path reaches this seam — so
    /// the loosened filter here cannot move them.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the refresh under the new input hits
    /// a backend failure.
    pub fn replace_raw(&mut self, text: &str) -> Result<(), EngineError> {
        self.exact_segments.clear();
        let continuous = self.replacement_extends_selection(text);
        self.refill_raw(text);
        if !continuous {
            self.reconcile_replaced_selection()?;
        }
        self.refresh()
    }

    /// Replaces the raw input with `text` parsed into exactly
    /// `segments` — the scheme-parse seam (zhuyin, double pinyin).
    ///
    /// The scan and the training record use these keys verbatim: the
    /// graph is one [`EdgeKind::Exact`] chain, so the pinyin inventory
    /// never re-segments the joined spelling (upstream's decoder receives
    /// the scheme parser's `ChewingKey`s the same way). Segments are
    /// absolute over `text`; spans outside the accepted prefix of
    /// `text` (the [`MAX_INPUT_BYTES`] clamp) are dropped, keeping
    /// `end <= raw.len()` an invariant of the stored segments.
    ///
    /// The selection record and the constraint store survive while the
    /// replacement extends the covered span, and a discontinuous one
    /// reconciles, exactly as in [`Session::replace_raw`].
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the refresh under the new input hits
    /// a backend failure.
    pub fn replace_raw_exact(
        &mut self,
        text: &str,
        segments: &[ExactSegment],
    ) -> Result<(), EngineError> {
        let continuous = self.replacement_extends_selection(text);
        self.refill_raw(text);
        let raw_len = self.raw.len();
        self.exact_segments = segments
            .iter()
            .copied()
            .filter(|segment| segment.end() <= raw_len)
            .collect();
        if !continuous {
            self.reconcile_replaced_selection()?;
        }
        self.refresh()
    }

    /// Whether `text` extends the bytes the selection record was built
    /// over (`raw[..consumed]`) — the continuity retaining the record
    /// requires. The parse seams' own prefix checks run in the caller's
    /// coordinates (the scheme parses' original input), while the record
    /// lives in these canonical bytes: a replacement that passes those
    /// checks but does not extend the covered span — a scheme switch
    /// that decodes the same codes to a different spelling, any
    /// transform divergence — must reconcile, or [`Session::commit`]
    /// combines the stale selection with the new raw suffix.
    fn replacement_extends_selection(&self, text: &str) -> bool {
        text.as_bytes()
            .starts_with(&self.raw.as_bytes()[..self.consumed])
    }

    /// Reconciles the selection to a replacement it does not extend:
    /// the full validate the next guess would run — bounds and spelling
    /// over the new input's matrix, so a forcing that no longer spells
    /// under the divergent replacement drops here instead of surviving
    /// under a stale record — then the record re-derived from the
    /// surviving runs, whatever the validate dropped. A backward clamp
    /// is the runs-empty extreme: the whole record goes and the
    /// composition re-opens at 0. The empty-record parse path pays only
    /// the continuity check.
    fn reconcile_replaced_selection(&mut self) -> Result<(), EngineError> {
        if self.consumed == 0 {
            return Ok(());
        }
        let graph = self.build_graph_at(0, self.raw.as_bytes())?;
        let bound = graph.consumed();
        if bound > 0 {
            let matrix = build_scan_matrix(
                &graph,
                self.settings.options,
                self.exact_segments.is_empty(),
            );
            self.constraints.validate(bound + 1, |start, end, token| {
                crate::nbest::span_finds_token(&matrix, start, end, token, &self.dictionary)
            })?;
        } else {
            // Nothing spells over the replaced buffer: every forcing is
            // dead and the record with it.
            self.constraints.clear();
        }
        self.rebuild_selection_from_constraints();
        Ok(())
    }

    /// The shared body of the two replace seams: refill the raw buffer
    /// and clamp the cursor onto the new input. No refresh — the callers
    /// refresh under their own parse mode (the exact seam must set its
    /// segments first).
    fn refill_raw(&mut self, text: &str) {
        self.raw.clear();
        for character in text.chars() {
            if self.raw.len() + character.len_utf8() > MAX_INPUT_BYTES {
                break;
            }
            self.raw.push(character);
        }
        // A stale consumed from the replaced composition may now sit inside
        // a multi-byte character of the new raw (`a` selected to consumed 1,
        // then `，` replaces it). `refresh`/`scan_window` slice
        // `raw[consumed..]`, so the clamp must land on a char boundary —
        // the composition restarts from the boundary before it.
        self.consumed = self.consumed.min(self.raw.len());
        while !self.raw.is_char_boundary(self.consumed) {
            self.consumed -= 1;
        }
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
    /// Which matrix the walk covers: an unconstrained decode with input
    /// remaining is today's remaining-input walk (the W6 re-seed,
    /// bit-identical under the frozen pins — the store is empty there).
    /// Anything else with a non-empty raw buffer walks the **full**
    /// matrix: a constrained composition (the §3 gates, the chosen
    /// prefix forced) or a fully-consumed one (upstream's walk still
    /// answers a terminal choose — the L1 surface,
    /// `docs/findings/live-typing.md`), which the remaining-input model
    /// structurally cannot.
    ///
    /// Returns whether a lookup ran at all (upstream returns the lookup's
    /// `false` only for an empty key matrix; zero rows is still `true`).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails during the lookup.
    pub fn guess_sentence(&mut self) -> Result<bool, EngineError> {
        self.nbest_rows.clear();
        self.nbest_history.clear();
        self.last_result.clear();
        self.sentence_lookup_active = true;
        if self.raw.is_empty() {
            return Ok(false);
        }
        let remaining_empty = self.consumed >= self.raw.len();
        if !remaining_empty && (!self.constraints.is_active() || !self.model.has_real_unigrams()) {
            return self.guess_over_remaining();
        }
        if !self.model.has_real_unigrams() {
            // The pre-frequency fallback has no constrained form; with the
            // input consumed there is nothing to fall back to either.
            return Ok(false);
        }

        let graph = self.build_graph_at(0, self.raw.as_bytes())?;
        let bound = graph.consumed();
        if bound == 0 {
            return Ok(false);
        }
        let matrix = build_scan_matrix(
            &graph,
            self.settings.options,
            self.exact_segments.is_empty(),
        );

        // `pinyin_update_constraints`: re-sync the store to the matrix —
        // grow with free cells (forcings survive typing), shrink by
        // truncation, drop forcings that overrun or no longer spell. If a
        // forcing dropped (the buffer changed under it), the selection
        // record follows the surviving forcings.
        let mut store = core::mem::take(&mut self.constraints);
        let dropped = store.validate(bound + 1, |start, end, token| {
            crate::nbest::span_finds_token(&matrix, start, end, token, &self.dictionary)
        });
        self.constraints = store;
        if dropped? {
            self.rebuild_selection_from_constraints();
        }

        self.nbest_rows = crate::nbest::nbest_sentences(
            &matrix,
            bound,
            &self.dictionary,
            &self.model,
            &[],
            Some(&self.constraints),
        )?;
        // The full-matrix rows already carry the chosen prefix: a chosen
        // row's record is its own whole path, so no lookup-time history
        // snapshot stands behind it (the remaining-input walk's §10
        // snapshot-restore pair does not apply).
        self.nbest_history.clear();
        self.last_result = self
            .nbest_rows
            .first()
            .map_or_else(Vec::new, |row| row.spans.clone());

        self.refresh()?;
        Ok(true)
    }

    /// Segments an arbitrary already-typed sentence string into its
    /// best dictionary phrase path — upstream `pinyin_phrase_segment`
    /// (`pinyin.cpp:1443-1460`), the phrase-lookup span DP over the
    /// sentence's characters, independent of the live composition.
    /// Returns `(matched, tokens)` in `m_phrase_result`'s shape:
    /// character-length, each phrase's token at its span's start
    /// position, `null_token` between phrases — and, on a failed
    /// match, the fully sized all-null array (`PhraseLookup::final_step`
    /// sizes and null-fills before its empty-last-step `false`).
    ///
    /// # Errors
    ///
    /// Propagates the model's step-cost failures.
    pub fn phrase_segment(&self, sentence: &str) -> Result<(bool, Vec<PhraseToken>), EngineError> {
        crate::phrase::phrase_segment(&self.dictionary, &self.model, sentence)
    }

    /// Guesses a sentence seeded with prefix tokens — upstream
    /// `pinyin_guess_sentence_with_prefix` (`pinyin.cpp:1426-1441`):
    /// the prefix tokens join the virtual start as zero-cost initial
    /// trellis nodes (`fill_prefixes`, `phonetic_lookup.h:244-276`),
    /// the constraint store validates against the matrix, and the
    /// ordinary full-matrix decode runs — no remaining-input shortcut.
    /// The caller supplies the tail-substring tokens (`_compute_prefixes`
    /// over the prefix text).
    ///
    /// # Errors
    ///
    /// Propagates engine failures from the decode.
    pub fn guess_sentence_with_prefix(
        &mut self,
        prefix_tokens: &[PhraseToken],
    ) -> Result<bool, EngineError> {
        self.nbest_rows.clear();
        self.nbest_history.clear();
        self.last_result.clear();
        self.sentence_lookup_active = true;
        if self.raw.is_empty() {
            return Ok(false);
        }
        let graph = self.build_graph_at(0, self.raw.as_bytes())?;
        let bound = graph.consumed();
        if bound == 0 {
            return Ok(false);
        }
        let matrix = build_scan_matrix(
            &graph,
            self.settings.options,
            self.exact_segments.is_empty(),
        );

        let mut store = core::mem::take(&mut self.constraints);
        let dropped = store.validate(bound + 1, |start, end, token| {
            crate::nbest::span_finds_token(&matrix, start, end, token, &self.dictionary)
        });
        self.constraints = store;
        if dropped? {
            self.rebuild_selection_from_constraints();
        }

        // `m_prefixes = [sentence_start] + _compute_prefixes(prefix)`:
        // every entry seeds a zero-cost initial node.
        let mut seeds = Vec::with_capacity(prefix_tokens.len() + 1);
        seeds.push(PhraseToken::new(crate::nbest::SENTENCE_START));
        seeds.extend_from_slice(prefix_tokens);
        self.nbest_rows = crate::nbest::nbest_sentences_with_seeds(
            &matrix,
            bound,
            &self.dictionary,
            &self.model,
            &seeds,
            Some(&self.constraints),
        )?;
        self.last_result = self
            .nbest_rows
            .first()
            .map_or_else(Vec::new, |row| row.spans.clone());

        self.refresh()?;
        Ok(true)
    }

    /// Today's remaining-input walk — the W6 re-seed surface, verbatim:
    /// the trellis over `raw[consumed..]` seeded from the selection
    /// history, the §10 text prefix, and the lookup-time history
    /// snapshot a later row choice restores.
    fn guess_over_remaining(&mut self) -> Result<bool, EngineError> {
        let remaining = &self.raw[self.consumed..];
        if remaining.is_empty() {
            return Ok(false);
        }

        let graph = self.build_graph_at(self.consumed, remaining.as_bytes())?;
        let bound = graph.consumed();
        if bound == 0 {
            return Ok(false);
        }

        let offset = self.consumed;
        self.nbest_rows = if self.model.has_real_unigrams() {
            let matrix = build_scan_matrix(
                &graph,
                self.settings.options,
                self.exact_segments.is_empty(),
            );
            crate::nbest::nbest_sentences(
                &matrix,
                bound,
                &self.dictionary,
                &self.model,
                &self.history,
                None,
            )?
        } else {
            let scorer = Scorer::with_key_costs(
                self.scoring,
                &self.dictionary,
                &self.model,
                self.key_costs.clone(),
            );
            let paths = k_best(&graph, &scorer, SEGMENTATION_K)?;
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
                    spans: Vec::new(),
                    keys: candidate.consumed_keys(),
                    span: candidate.consumed_bytes(),
                    cost: candidate.cost(),
                })
                .collect()
        };
        // The rows were seeded with the history as it stands right here;
        // a later row selection restores this snapshot before extending
        // the record with the row's own tokens.
        self.nbest_history.clone_from(&self.history);
        // The walk's positions are remaining-relative; the store and the
        // train result are absolute.
        for row in &mut self.nbest_rows {
            for span in &mut row.spans {
                span.start += offset;
            }
        }
        self.last_result = self
            .nbest_rows
            .first()
            .map_or_else(Vec::new, |row| row.spans.clone());

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
        // The character is accepted: the buffer is about to change, so the
        // exact chain's absolute spans go now — not before validation, or a
        // rejected character would silently exit exact mode.
        self.exact_segments.clear();
        self.raw.push(character);
        self.refresh()?;
        Ok(KeyOutcome::Consumed)
    }

    fn erase(&mut self) -> Result<KeyOutcome, EngineError> {
        if self.consumed < self.raw.len() {
            // The buffer is about to shrink: the exact chain's absolute
            // spans would dangle past the new end. Operations that do not
            // modify raw leave exact mode alone.
            self.exact_segments.clear();
            self.raw.pop();
            self.refresh()?;
            return Ok(KeyOutcome::Consumed);
        }
        if !self.selected.is_empty() {
            // The all-or-nothing un-select: the store goes with the
            // record, or a forcing would outlive its own selection.
            self.selected.clear();
            self.consumed = 0;
            self.selection_committed = false;
            self.history.clear();
            self.constraints.clear();
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
    /// session owns, so a choose at the caller offset keeps round-tripping.
    ///
    /// Previous-token context: upstream resolves the bigram predecessor by
    /// indexing per-position match results at the lookup offset, and the
    /// raw one-past-separator offset hits a null slot — the system+user
    /// bigram merge is silently skipped and ranking quietly degrades
    /// (C++ libpinyin 2.11.92 still does; libpinyin@412f88e3 feeds the
    /// normalized offset instead). oxpinyin's counterpart is the selection
    /// history — [`Session::selected_tokens`]' tail seeds `rank_phrases`
    /// and the n-best trellis — which no lookup offset ever indexes, so
    /// that degradation cannot occur here; any future offset-indexed
    /// context lookup must take this method's normalized offset (#99
    /// folds the ranking bigram term to zero today,
    /// `dynamic_adjust_bigram_term`).
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
        normalize_lookup_offset(self.raw.as_bytes(), offset)
    }

    /// Normalizes a user cursor position to a lookup offset over the
    /// session's own buffer and options — the `pinyin_get_pinyin_offset`
    /// law ([`crate::lookup_offset_for_cursor`]).
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the buffer cannot be represented as a
    /// segment graph, and [`EngineError::ZeroKeyOffsetCheck`] where the
    /// pin's `_check_offset` aborts. A cursor past one-past-end is NOT an
    /// error: like the pin, the cursor is clamped to the parsed length, so
    /// there is no out-of-range shape here.
    pub fn lookup_offset_for_cursor(&self, cursor: usize) -> Result<usize, EngineError> {
        crate::cursor::lookup_offset_for_cursor(self.raw.as_bytes(), self.settings.options, cursor)
    }

    /// The word-level left move over the session's own buffer and options
    /// — the `pinyin_get_left_pinyin_offset` law
    /// ([`crate::left_word_offset`]).
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the buffer cannot be represented as a
    /// segment graph; [`EngineError::ZeroKeyOffsetCheck`] where the pin's
    /// `_check_offset` aborts (an input offset one past a lone zero-key
    /// column, or the second check on the computed result); and
    /// [`EngineError::LookupOffsetOutOfRange`] when the offset exceeds the
    /// buffer's one-past-end position (upstream reads its matrix out of
    /// bounds there).
    pub fn left_word_offset(&self, offset: usize) -> Result<usize, EngineError> {
        crate::cursor::left_word_offset(self.raw.as_bytes(), self.settings.options, offset)
    }

    /// The word-level right move over the session's own buffer and options
    /// — the `pinyin_get_right_pinyin_offset` law
    /// ([`crate::right_word_offset`]). `Ok(None)` is the pin's graceful
    /// false: no key starts at the position.
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the buffer cannot be represented as a
    /// segment graph; [`EngineError::ZeroKeyOffsetCheck`] where the pin's
    /// `_check_offset` aborts (an input offset one past a lone zero-key
    /// column, or the second check on the computed result); and
    /// [`EngineError::LookupOffsetOutOfRange`] when the offset exceeds the
    /// buffer's one-past-end position (upstream reads its matrix out of
    /// bounds there).
    pub fn right_word_offset(&self, offset: usize) -> Result<Option<usize>, EngineError> {
        crate::cursor::right_word_offset(self.raw.as_bytes(), self.settings.options, offset)
    }

    /// The composition's scan-matrix keys with their raw byte spans.
    ///
    /// The same walk the cursor laws above run — `matrix_spans` is a
    /// projection of this — so a key answered here and an offset answered
    /// by [`Session::right_word_offset`] agree by construction. The C ABI's
    /// `pinyin_get_pinyin_key` family reads this.
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the raw buffer cannot be built into a
    /// segment graph.
    pub fn matrix_keys(&self) -> Result<(Vec<crate::cursor::MatrixKey>, usize), EngineError> {
        crate::cursor::matrix_keys(self.raw.as_bytes(), self.settings.options)
    }

    /// Whether a selection consumed the whole buffer and no rebuild has
    /// since changed the record — the commit-branch shape the R5 revert
    /// keeps composing through ([`Session::committed_parse_continues`]).
    /// A pure query over valid state.
    #[must_use]
    pub const fn selection_committed(&self) -> bool {
        self.selection_committed
    }

    /// Whether a re-parse of `original` continues the current composition
    /// (`CapiInstance::begin_parse`'s rule): the composition is open —
    /// not completed by a selection — and the buffer evolved from
    /// itself (one input is a prefix of the other: forward typing or
    /// backspace). Upstream's constraints survive every re-parse with
    /// `validate_constraint` dropping whatever stops spelling at the
    /// next guess, so an open composition's extension, shrink, or
    /// re-send continues it — the cursor may sit mid-buffer, or the
    /// buffer may have shrunk TO the cursor (a backspace that ate the
    /// tail — still open). A selection-consumed composition continues
    /// through [`Session::committed_parse_continues`] (the R5 revert,
    /// register #8); only a divergent buffer starts fresh — a different
    /// string is a different composition, and a stale selection-derived
    /// cursor must not mis-anchor its window before validate could drop
    /// the mismatched forcings.
    ///
    /// A pure query, not a fallible operation — it reads already-valid
    /// state and cannot fail, so the constitution's `Result` rule for
    /// fallible public APIs does not reach it. The state-changing halves
    /// of the parse pipeline are the fallible [`Session::replace_raw`]
    /// and the infallible [`Session::reset_composition`]/[`reset`].
    #[must_use]
    pub fn parse_continues(&self, stored: &[u8], original: &[u8]) -> bool {
        !self.selection_committed
            && !stored.is_empty()
            && (original.starts_with(stored) || stored.starts_with(original))
    }

    /// The R5 half of the parse rule (register #8): a SELECTION-committed
    /// composition whose buffer evolved from the stored one still
    /// continues — the constraint store and the selection record survive
    /// into the next guess, where validate drops whatever stops spelling.
    /// Upstream's parse path never touches `m_constraints`
    /// (`pinyin.cpp:1497-1517`) and only `pinyin_reset` clears the store
    /// (`pinyin.cpp:2693-2704`), so a commit no longer ends the
    /// composition engine-side: the pre-revert rule re-parsed this shape
    /// fresh — an emulation of the frontend's reset-on-commit contract
    /// the #141 cursor flows pinned — which dropped forcings upstream
    /// keeps. The divergence boundary that stays: a DIVERGENT buffer
    /// answers `false` here and in [`Session::parse_continues`], so it
    /// alone re-parses fresh.
    ///
    /// A pure query, not a fallible operation — it reads already-valid
    /// state and cannot fail, so the constitution's `Result` rule for
    /// fallible public APIs does not reach it. The state-changing halves
    /// of the parse pipeline are the fallible [`Session::replace_raw`]
    /// and the infallible [`Session::reset_composition`]/[`reset`].
    #[must_use]
    pub fn committed_parse_continues(&self, stored: &[u8], original: &[u8]) -> bool {
        self.selection_committed
            && !stored.is_empty()
            && (original.starts_with(stored) || stored.starts_with(original))
    }

    /// The filtered fewest-keys parse length of the WHOLE raw buffer —
    /// the `pinyin_parse_more_*` return and `pinyin_get_parsed_input_length`
    /// value, which are defined over the passed input, never the
    /// remaining slice a mid-composition re-parse decodes from.
    ///
    /// Extends the filtered key path over any trailing apostrophe run:
    /// the pin's DP propagates `'` byte-for-byte from any reachable
    /// position (`pinyin_parser2.cpp:237-251`) and `final_step` answers
    /// the consistent-chain length, so a trailing or standalone run is
    /// consumed even though no key covers it.
    #[must_use]
    pub fn full_parsed_len(&self) -> usize {
        if self.raw.is_empty() {
            return 0;
        }
        // The exact chain drives the length when a scheme parse owns the
        // buffer: re-segmenting the joined text through the pinyin
        // inventory would under-report zhuyin-only spellings ("den" → 2).
        match self.build_graph_at(0, self.raw.as_bytes()) {
            Ok(graph) => apostrophe_extended(
                self.raw.as_bytes(),
                graph
                    .fewest_keys(self.settings.incomplete())
                    .last()
                    .map_or(0, Edge::to),
            ),
            Err(_) => 0,
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
        // The cached list is anchored at the composition offset the session
        // owns. Reuse its buffer so the scan keeps its capacity across
        // keystrokes.
        let anchor = self.consumed;
        let mut items = Vec::new();
        self.candidates.swap_items(&mut items);
        self.parsed_prefix = self.scan_window(anchor, &mut items)?;
        self.candidates.swap_items(&mut items);
        Ok(())
    }

    /// Builds the candidate window anchored at byte `anchor` in the raw
    /// buffer into `out`, returning the filtered parse length of the
    /// remaining slice from `anchor`.
    ///
    /// The window is a pure function of `(raw, anchor, constraint-derived
    /// state)`: the scan reads `&self.raw[anchor..]` and the stored n-best
    /// rows prepend the same way regardless of the anchor. It mutates only
    /// the scan scratch and `out` — never the composition offset, the
    /// constraint store, or the history — so a caller may build a window at a
    /// lookup offset without disturbing the cached list
    /// ([`Session::candidates_at`]). With `anchor == self.consumed` it
    /// reproduces [`Session::refresh`]'s cached list exactly.
    /// Builds the working graph for `remaining` (the raw slice from
    /// `anchor`): the exact-mode chain when the session carries
    /// pre-parsed scheme segments, the parsed graph otherwise. A segment
    /// that straddles `anchor` is dropped, not truncated — a mid-key
    /// anchor is the caller's boundary question, and exact keys do not
    /// re-syllabify around it.
    fn build_graph_at(&self, anchor: usize, remaining: &[u8]) -> Result<SegmentGraph, EngineError> {
        if self.exact_segments.is_empty() {
            return SegmentGraph::build_with_options(remaining, self.settings.options)
                .map_err(EngineError::Graph);
        }
        // An anchor strictly inside an exact segment must not decode the
        // tail segments across the skipped bytes — that would consume
        // input the anchor excluded (exact `xian'hao` anchored at 2
        // decoding `hao` over `an'hao`). Refuse instead: an empty exact
        // graph answers no candidates and a zero parse for this anchor.
        if self
            .exact_segments
            .iter()
            .any(|segment| segment.start() < anchor && anchor < segment.end())
        {
            return SegmentGraph::build_exact(remaining, &[]).map_err(EngineError::Graph);
        }
        let rebased: Vec<ExactSegment> = self
            .exact_segments
            .iter()
            .copied()
            .filter(|segment| segment.start() >= anchor)
            .map(|segment| {
                ExactSegment::new(
                    segment.start() - anchor,
                    segment.end() - anchor,
                    segment.key(),
                    segment.tone(),
                )
            })
            .collect();
        SegmentGraph::build_exact(remaining, &rebased).map_err(EngineError::Graph)
    }

    fn scan_window(
        &mut self,
        anchor: usize,
        out: &mut Vec<Candidate>,
    ) -> Result<usize, EngineError> {
        out.clear();
        if anchor >= self.raw.len() {
            // A fully-consumed (or past-end) anchor still carries its
            // sentence rows — upstream's window prepends `m_nbest_results`
            // whether or not any phrase candidate remains at the cursor (the
            // L1 terminal-choose surface).
            self.prepend_nbest_rows(out);
            return Ok(0);
        }

        // Lift scratches before borrowing `raw`, so graph/scan can use
        // `&self.raw[anchor..]` without cloning into CompactString.
        let mut collected = core::mem::take(&mut self.scratch_collected);
        collected.clear();
        let mut path = core::mem::take(&mut self.scratch_path);
        let mut entries = core::mem::take(&mut self.scratch_entries);
        let mut ranked = core::mem::take(&mut self.scratch_ranked);
        let mut window_phrase = core::mem::take(&mut self.scratch_window_phrase);
        let mut window_addon = core::mem::take(&mut self.scratch_window_addon);

        let remaining = &self.raw[anchor..];
        let graph = self.build_graph_at(anchor, remaining.as_bytes())?;
        // The trailing-run extension of `full_parsed_len`, applied to the
        // remaining slice (the pin's propagation runs on every parse).
        let parsed_prefix = apostrophe_extended(
            remaining.as_bytes(),
            graph
                .fewest_keys(self.settings.incomplete())
                .last()
                .map_or(0, Edge::to),
        );

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

            // Upstream's Gates 1 and 2 (`pinyin.cpp:2200-2214`), hoisted
            // out of the candidate loop exactly as the pin hoists them: the
            // previous token is resolved once, and the system and user grams
            // are merged once, for the whole guess. Indexing that row per
            // candidate is Gate 3.
            let gram = self.dynamic_adjust_gram(anchor)?;
            // The scan's result stands even when it found nothing. Tokens the
            // table lacks rank as zero rather than falling back.
            let frequencies = self
                .candidate_frequencies(&collected, gram.as_ref())?
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
            let paths = k_best(&graph, &scorer, SEGMENTATION_K)?;
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
        self.prepend_nbest_rows(&mut collected);

        core::mem::swap(out, &mut collected);
        collected.clear();
        self.scratch_collected = collected;
        self.scratch_path = path;
        self.scratch_entries = entries;
        self.scratch_ranked = ranked;
        self.scratch_window_phrase = window_phrase;
        self.scratch_window_addon = window_addon;
        Ok(parsed_prefix)
    }

    /// Rebuilds the candidate window anchored at a caller lookup `offset`,
    /// mirroring the pin's per-offset span search — `pinyin_guess_candidates`
    /// re-runs `search_matrix` from `start = offset` (`pinyin.cpp:2224-2262`),
    /// its candidates all beginning at `offset` — and returns it without
    /// disturbing the cached list or any composition state (constraints,
    /// consumed, history). The C ABI uses this only when the caller's
    /// normalized lookup offset differs from [`Session::composition_offset`]:
    /// a mid-composition cursor with no prior choose. At an equal offset the
    /// cached [`Session::candidates`] already answers, so offset-0 and every
    /// post-choose lookup stay bit-identical.
    ///
    /// A byte no matrix key starts on — a mid-syllable position of the
    /// composition's own parse — is one of the pin's empty columns:
    /// `search_matrix` matches nothing from it for every end, so the
    /// window there is the raw-suffix fallback under the prepended n-best
    /// rows, never a re-parse of the suffix (the suffix's own keys are not
    /// the matrix's). A zero-key (apostrophe) column inside the parse is
    /// not empty: the span search steps over it to the next key, so that
    /// apostrophe byte answers the following key's window. An apostrophe
    /// past a stop byte sits outside the matrix — the pin aborts there —
    /// and takes the empty-column window like any other unreachable byte.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails during the scan, exactly
    /// as the anchored [`Session::refresh`] does;
    /// [`EngineError::LookupOffsetOutOfRange`] when `offset` exceeds the raw
    /// buffer's one-past-end position — the pin reads its matrix out of
    /// bounds there, so no pinned behaviour exists and the offset is
    /// refused — and [`EngineError::LookupOffsetInsideCharacter`] when
    /// `offset` falls inside a multi-byte character of the raw buffer (no
    /// window exists under a mid-character slice). An offset equal to
    /// one-past-end is valid: `scan_window` answers the terminal sentence
    /// rows for it (the pin's reserved slot).
    pub fn candidates_at(&mut self, offset: usize) -> Result<CandidateList, EngineError> {
        if offset > self.raw.len() {
            return Err(EngineError::LookupOffsetOutOfRange {
                offset,
                len: self.raw.len(),
            });
        }
        if !self.raw.is_char_boundary(offset) {
            return Err(EngineError::LookupOffsetInsideCharacter {
                offset,
                len: self.raw.len(),
            });
        }
        let mut items = Vec::new();
        if offset < self.raw.len() && !self.spans_a_matrix_key(offset)? {
            items.push(Candidate::new(
                compact_str::CompactString::from(&self.raw[offset..]),
                CandidateKind::Fallback,
                0,
                self.raw.len() - offset,
                0,
                None,
                None,
            ));
            self.prepend_nbest_rows(&mut items);
            return Ok(CandidateList::from_vec(items));
        }
        self.scan_window(offset, &mut items)?;
        Ok(CandidateList::from_vec(items))
    }

    /// Whether `offset` names a column the pin's span search can answer.
    ///
    /// The pin's matrix holds the chosen parse's keys at their raw begins
    /// (`fill_matrix`), plus the split keys `resplit_step` and
    /// `inner_split_step` append (`docs/findings/matrix-split-tables.md`) —
    /// `jie` in `nihaoshijie` also carries `ji` + `e`, so byte 10 is a live
    /// column — and a zero key at every apostrophe the parse reached, which
    /// the span search steps over to the following key. The matrix ends at
    /// the parse: an apostrophe past a stop byte sits outside it entirely
    /// (the pin aborts there — `ni,'hao@3`, measured SIGABRT — and the
    /// empty-column window is the no-abort answer). The scan's own key set
    /// ([`build_scan_matrix`]) models the key columns, so a byte some key's
    /// syllable starts on, or an in-span apostrophe byte, answers; any other
    /// byte is an empty column. Exact mode has no pin counterpart, so its
    /// columns stay the exact segments, and its inputs' apostrophes are all
    /// in-span by construction (`build_exact` rejects any other gap).
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the composition cannot be represented
    /// as a segment graph.
    fn spans_a_matrix_key(&self, offset: usize) -> Result<bool, EngineError> {
        if !self.exact_segments.is_empty() {
            return Ok(self.raw.as_bytes().get(offset) == Some(&b'\'')
                || self
                    .exact_segments
                    .iter()
                    .any(|segment| segment.start() == offset));
        }
        let graph = SegmentGraph::build_with_options(self.raw.as_bytes(), self.settings.options)
            .map_err(EngineError::Graph)?;
        // The split alternates are a full-pinyin-parse artifact — the same
        // law the scan applies (`build_scan_matrix`'s `divided` argument at
        // the anchored call site) — and exact keys never gain them.
        let matrix = build_scan_matrix(&graph, self.settings.options, true);
        if matrix
            .iter()
            .flatten()
            .any(|key| key.syllable_start == offset)
        {
            return Ok(true);
        }
        Ok(offset < graph.consumed() && self.raw.as_bytes().get(offset) == Some(&b'\''))
    }

    /// Prepends the stored n-best rows onto `collected`, head first, then
    /// drops every later candidate with the same text — upstream prepends
    /// after the sort and its phrase-string dedup keeps the NBEST row
    /// (and the lower n-best index) over any phrase candidate with the
    /// same string (`pinyin.cpp:2290-2298`, `2058-2126`). Extend-then-
    /// rotate keeps `collected`'s allocation — the session scratch on the
    /// scan path, a fresh small vec on the fully-consumed path.
    fn prepend_nbest_rows(&mut self, collected: &mut Vec<Candidate>) {
        if self.nbest_rows.is_empty() {
            return;
        }
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
        dedup_by_text_keep_first(collected);
    }

    /// The previous token at `offset`, as upstream's `_get_previous_token`
    /// resolves it (`pinyin.cpp:1711-1767`).
    ///
    /// At offset 0 upstream answers `sentence_start` and then prefers the
    /// longest token in `m_prefixes`. `m_prefixes` is populated only by
    /// `pinyin_guess_sentence_with_prefix`, which neither reference consumer
    /// calls, so the drop-in surface always takes the `sentence_start`
    /// answer there.
    ///
    /// Above 0 upstream reads the 1-best result — `last_result` here — and
    /// carries a guard worth reproducing: it inspects `result[offset]` FIRST
    /// and only walks backwards when that position holds a token. A guess at
    /// an offset no phrase starts at contributes no bigram term at all.
    fn previous_token(&self, offset: usize) -> Option<PhraseToken> {
        if offset == 0 {
            return Some(PhraseToken::new(crate::nbest::SENTENCE_START));
        }
        if self.last_result.is_empty() {
            return None;
        }
        // `result[offset] != null_token`: a phrase must begin here.
        self.last_result
            .iter()
            .any(|span| span.start == offset)
            .then(|| {
                self.last_result
                    .iter()
                    .filter(|span| span.start < offset)
                    .max_by_key(|span| span.start)
                    .map(|span| span.token)
            })
            .flatten()
    }

    /// Upstream's Gates 1 and 2 as one call: resolve the previous token and
    /// merge its system and user grams, ONCE per candidate guess.
    ///
    /// `None` whenever upstream would skip the merge — the bit is clear, no
    /// previous token, or the model carries no row for it — and the caller
    /// then contributes no bigram term.
    fn dynamic_adjust_gram(&self, offset: usize) -> Result<Option<MergedGram>, EngineError> {
        if !self.settings.options.has_dynamic_adjust() {
            return Ok(None);
        }
        let Some(prev) = self.previous_token(offset) else {
            return Ok(None);
        };
        self.model
            .merged_successors(&prev)
            .map_err(|error| EngineError::Scoring(ScoringError::LanguageModel(error.to_string())))
    }

    /// Per-candidate sort frequencies on the pin's amplified scale, or
    /// `None` when the model carries no real frequency table at all.
    ///
    /// The pinned oracle does not compare raw unigram counts: it truncates
    /// the f32 possibility `(1−λ)·unigram/total` amplified by 2²⁴ into a
    /// `guint32` (`_compute_frequency_of_items`, `pinyin.cpp:1855-1866`).
    /// `gram` is the row merged once for this guess: `None`, or a row that
    /// misses the token, contributes a bigram possibility of exactly `0.0`
    /// and leaves the amplified value bit-identical to the unigram-only
    /// law. Near-ties collapse
    /// to equal keys under that truncation — the tie class
    /// `docs/testing/corpus-tail.md` calls Class A — and equal keys fall to
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
        gram: Option<&MergedGram>,
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
                    .map(|count| {
                        // Upstream's Gate 3: the bigram possibility joins the
                        // unigram term INSIDE the pin's expression, before
                        // its single truncation. The addon and predicted
                        // branches above return early in the pin too — they
                        // carry no bigram term at all.
                        let bigram = dynamic_adjust_bigram_possibility(
                            self.settings.options,
                            gram,
                            token.value(),
                        );
                        amplified_frequency_with_bigram(
                            count.saturating_add(1),
                            default_total,
                            bigram,
                        )
                    })
            };
            if let Some(count) = count {
                let table = frequencies.get_or_insert_with(|| vec![0; collected.len()]);
                // Unigram term of candidate frequency: always on. Upstream
                // reads FacadePhraseIndex unigrams (including trained user
                // counts) with no DYNAMIC_ADJUST check. W6-T4's overlay is
                // that unigram term and stays for both bit states.
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
            let ranked = scorer.rank_phrases(&self.history, &keys[..length], &kinds[..length])?;
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
                let ranked =
                    scorer.rank_phrases(&prefix_history, &keys[start..end], &kinds[start..end])?;
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
        let matrix = build_scan_matrix(graph, options, self.exact_segments.is_empty());
        let bound = graph.consumed();
        let mut end = 1usize;
        while end <= bound {
            // An end position no key starts at is an empty column: widen.
            let mut continued = matrix.get(end).is_none_or(std::vec::Vec::is_empty);
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
/// `docs/testing/corpus-tail.md` Class A. The scan reaches the same
/// tokens through several key-paths; sorting the batch by token and
/// keeping the first of each reproduces the one-row-per-token array the
/// pin sorts.
fn flush_window_batch(batch: &mut Vec<Candidate>, into: &mut Vec<Candidate>) {
    batch.sort_by_key(|candidate| {
        candidate
            .token()
            .map_or(u32::MAX, oxpinyin_core::PhraseToken::value)
    });
    let mut last: Option<u32> = None;
    for candidate in batch.drain(..) {
        let token = candidate.token().map(oxpinyin_core::PhraseToken::value);
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
    amplified_frequency_with_bigram(unigram, total, 0.0)
}

/// The pin's `BIGRAM_FREQUENCY_DISCOUNT` (`pinyin.cpp:33`).
const BIGRAM_FREQUENCY_DISCOUNT_F32: f32 = 0.1;

/// [`amplified_frequency`] with the DYNAMIC_ADJUST bigram term folded in,
/// reproducing the pin's whole expression (`pinyin.cpp:1862-1866`):
///
/// ```c
/// freq = (lambda * bigram_poss * BIGRAM_FREQUENCY_DISCOUNT +
///         (1 - lambda) * unigram / (gfloat) total_freq) * 256 * 256 * 256;
/// ```
///
/// The two terms are summed **before** the single truncation, which is the
/// whole reason this is one function rather than an additive term bolted
/// onto [`amplified_frequency`]'s result: the pin truncates the sum once,
/// and `trunc(a) + trunc(b)` differs from `trunc(a + b)` by up to one unit.
/// That unit is not a rounding detail here — the truncation collapses
/// near-ties into equal comparator keys, so an off-by-one moves candidates
/// between tie classes and reorders the list.
///
/// With `bigram_poss` at `0.0` the first term is exactly `0.0` and
/// `0.0 + x == x` in IEEE-754, so the DYNAMIC_ADJUST-clear path is
/// bit-identical to the pre-existing unigram-only law by construction —
/// not merely by the frozen words happening to leave the bit clear.
fn amplified_frequency_with_bigram(unigram: u64, total: u64, bigram_poss: f32) -> u64 {
    if total == 0 {
        return 0;
    }
    let possibility = PIN_LAMBDA_F32 * bigram_poss * BIGRAM_FREQUENCY_DISCOUNT_F32
        + (1.0_f32 - PIN_LAMBDA_F32) * unigram as f32 / total as f32;
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
pub(crate) fn build_scan_matrix(
    graph: &SegmentGraph,
    options: OptionBits,
    divided: bool,
) -> Vec<Vec<ScanKey>> {
    let bound = graph.consumed();
    let mut columns: Vec<Vec<ScanKey>> = vec![Vec::new(); bound + 1];

    // 1. The selected parse.
    let selected_edges = graph.fewest_keys(options.has_incomplete());
    let selected: Vec<ScanKey> = selected_edges.iter().map(ScanKey::from_edge).collect();
    for scan_key in &selected {
        columns[scan_key.from].push(*scan_key);
    }

    // The divided/resplit alternates are a full-pinyin-parse artifact:
    // upstream generates them inside its pinyin parser's matrix fill, so
    // keys that arrive pre-parsed (the scheme seam — zhuyin, double
    // pinyin — exact keys) never gain them. The oracle's candidate list
    // for ㄅㄧㄝ is the bie rows alone, with no bi+e divided pair.
    if !divided {
        return columns;
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

/// The lookup-offset law over one coordinate buffer whose `'` bytes are
/// zero-key separator columns — plain full pinyin's raw buffer, or the
/// original input of an index-parsed scheme (Luoma, secondary zhuyin),
/// whose pinned parse consumes `'` as the same separator.
///
/// Range first ([`check_lookup_offset_range`]), then the
/// `_compute_zero_start` walk and the `_check_offset` validation of
/// `pinyin_guess_candidates` at libpinyin@dbff264: from `offset - 1`
/// downward while the index stays positive and the byte is `'`, then
/// refuse a normalized offset still one past a separator (only a leading
/// run can cause it — the walk never crosses byte 0).
///
/// Do **not** call this for a buffer where `'` is not a separator: double
/// pinyin never admits one into a composition, and the Gin-Yieh/Eten
/// zhuyin keyboards bind `'` to the content symbols ㄥ/ㄘ — there only
/// [`check_lookup_offset_range`] applies.
///
/// # Errors
///
/// [`EngineError::LookupOffsetOutOfRange`] past one-past-end;
/// [`EngineError::LookupOffsetPastSeparator`] for the leading-run shape
/// upstream aborts on.
pub fn normalize_lookup_offset(input: &[u8], offset: usize) -> Result<usize, EngineError> {
    check_lookup_offset_range(input.len(), offset)?;
    let mut normalized = offset;
    let mut index = offset.saturating_sub(1);
    while index > 0 && input.get(index) == Some(&b'\'') {
        normalized = index;
        index -= 1;
    }
    if normalized > 0 && input.get(normalized - 1) == Some(&b'\'') {
        return Err(EngineError::LookupOffsetPastSeparator { offset, normalized });
    }
    Ok(normalized)
}

/// The range half of the lookup-offset law: an offset may at most equal
/// the coordinate buffer's one-past-end position (upstream's reserved
/// matrix slot). This is the whole law for parse modes whose compositions
/// hold no zero-key columns (double pinyin, the zhuyin keyboards).
///
/// # Errors
///
/// [`EngineError::LookupOffsetOutOfRange`] when `offset > len` — upstream
/// reads its matrix out of bounds there, so no pinned behaviour exists
/// and the offset is refused.
pub fn check_lookup_offset_range(len: usize, offset: usize) -> Result<usize, EngineError> {
    if offset > len {
        return Err(EngineError::LookupOffsetOutOfRange { offset, len });
    }
    Ok(offset)
}

/// The bigram possibility the DYNAMIC_ADJUST term is built from — the pin's
/// Gate 3 (`pinyin.cpp:1854-1860`).
///
/// Zero on any of the three ways upstream skips the term: the bit is clear,
/// there is no previous token (so no gram was merged), or the merged row's
/// total is zero. Otherwise `bigram_freq / total` from the row merged once
/// at guess time.
fn dynamic_adjust_bigram_possibility(
    options: OptionBits,
    gram: Option<&MergedGram>,
    token: u32,
) -> f32 {
    if !options.has_dynamic_adjust() {
        return 0.0;
    }
    gram.map_or(0.0, |row| row.possibility(token))
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

/// Whether the engine batch path ([`Session::type_pinyin`]) accepts
/// `character`.
///
/// Printable ASCII (`0x21..=0x7E`), including junk the parity corpus embeds
/// in inputs. The decoder (`SegmentGraph`) treats non-`a-z`/`'` bytes as
/// hard boundaries; see `docs/testing/f1-junk-aware-parse.md`. Space and
/// controls are excluded so they cannot bypass `LogicalKey::Space` / `Tab`
/// / `Enter`.
///
/// This filter belongs to `type_pinyin` ONLY. The capi parse seam
/// ([`Session::replace_raw`]) keeps every character so the decoder sees —
/// and stops at — the bytes the pin stops at; the corpus and sentence
/// pins never reach that seam.
const fn is_batch_input_character(character: char) -> bool {
    character.is_ascii_graphic()
}

/// Extends a filtered key-path end over the apostrophe run following it.
///
/// The pin's DP propagates `'` byte-for-byte from any reachable position
/// (`pinyin_parser2.cpp:237-251`) and `final_step` answers the
/// consistent-chain length, so bytes of a trailing or standalone run are
/// consumed even though no key covers them (`ni'` parses to 3, `'''` to
/// 3, `nihao'` to 6).
fn apostrophe_extended(input: &[u8], mut end: usize) -> usize {
    while input.get(end) == Some(&b'\'') {
        end += 1;
    }
    end
}

#[cfg(test)]
mod tests {
    #[test]
    fn ignored_type_input_preserves_exact_mode() {
        let mut session = session();
        let segments: Vec<oxpinyin_core::graph::ExactSegment> = {
            use oxpinyin_core::graph::ExactSegment;
            let ni = oxpinyin_core::SyllableKey::from_text("ni").expect("ni");
            let hao = oxpinyin_core::SyllableKey::from_text("hao").expect("hao");
            vec![
                ExactSegment::new(0, 2, ni, 0),
                ExactSegment::new(3, 6, hao, 0),
            ]
        };
        session
            .replace_raw_exact("ni'hao", &segments)
            .expect("replace");
        // A batch input whose every character is filtered (the space) is
        // Ignored and must not exit exact mode.
        assert_eq!(
            session.type_pinyin("  ").expect("ignored input"),
            KeyOutcome::Ignored
        );
        assert!(session.exact_segments.len() == 2);
        // An accepted character does exit exact mode.
        assert_eq!(
            session.type_pinyin("h").expect("typed"),
            KeyOutcome::Consumed
        );
        assert!(session.exact_segments.is_empty());
    }

    #[test]
    fn a_rejected_character_preserves_exact_mode_and_backspace_clears_it() {
        let mut session = session();
        let segments: Vec<oxpinyin_core::graph::ExactSegment> = {
            use oxpinyin_core::graph::ExactSegment;
            let ni = oxpinyin_core::SyllableKey::from_text("ni").expect("ni");
            vec![ExactSegment::new(0, 2, ni, 0)]
        };
        session.replace_raw_exact("ni", &segments).expect("replace");
        // An over-capacity or non-input character is Ignored: exact mode
        // survives because raw never changed.
        assert_eq!(
            session
                .process_key(&KeyInput::plain(LogicalKey::Backspace))
                .expect("erase"),
            KeyOutcome::Consumed
        );
        assert!(
            session.exact_segments.is_empty(),
            "erase must drop the exact chain"
        );
    }

    #[test]
    fn full_parsed_len_reflects_the_exact_chain() {
        let mut session = session();
        let segments: Vec<oxpinyin_core::graph::ExactSegment> = {
            use oxpinyin_core::graph::ExactSegment;
            let ni = oxpinyin_core::SyllableKey::from_text("ni").expect("ni");
            let hao = oxpinyin_core::SyllableKey::from_text("hao").expect("hao");
            vec![
                ExactSegment::new(0, 2, ni, 0),
                ExactSegment::new(3, 6, hao, 0),
            ]
        };
        session
            .replace_raw_exact("ni'hao", &segments)
            .expect("replace");
        assert_eq!(session.full_parsed_len(), 6);
        assert_eq!(session.parsed_prefix_len(), 6);
    }

    #[test]
    fn an_anchor_inside_an_exact_segment_decodes_nothing() {
        let mut session = session();
        use oxpinyin_core::graph::ExactSegment;
        let ni_hao: Vec<ExactSegment> = {
            let ni = oxpinyin_core::SyllableKey::from_text("ni").expect("ni");
            let hao = oxpinyin_core::SyllableKey::from_text("hao").expect("hao");
            vec![
                ExactSegment::new(0, 2, ni, 0),
                ExactSegment::new(3, 6, hao, 0),
            ]
        };
        session
            .replace_raw_exact("ni'hao", &ni_hao)
            .expect("replace");
        // Anchor 1 sits inside the `ni` segment: the tail `hao` must not
        // decode across the skipped `i'` bytes.
        let raw = session.raw.clone();
        let graph = session
            .build_graph_at(1, &raw.as_bytes()[1..])
            .expect("anchor inside a segment answers an empty graph");
        assert!(graph.edges().is_empty());
        assert_eq!(graph.consumed(), 0);
        // A boundary anchor (2, the end of the first segment) still decodes.
        let graph = session
            .build_graph_at(2, &raw.as_bytes()[2..])
            .expect("boundary anchor builds");
        assert_eq!(graph.edges().len(), 1);
    }

    use oxpinyin_core::{
        Cost, Dictionary, LanguageModel, NbestStepCosts, PhraseEntry, PhraseToken, SyllableKey,
        UserModel,
    };
    use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

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
    fn type_pinyin_still_filters_stop_bytes() {
        // The frozen half of the parse-termination split: `type_pinyin`
        // keeps its printable-ASCII accept set (F1), so the corpus and
        // sentence pins that feed through it cannot reach the loosened
        // `replace_raw` seam — a space never enters `raw` here.
        let mut session = session();
        session
            .type_pinyin("ni hao")
            .expect("batch typing cannot fail");
        assert_eq!(session.raw_input(), "nihao");
    }

    #[test]
    fn replace_raw_keeps_the_bytes_the_pin_stops_at() {
        // Class B2: the pin accepts any input string and stops consuming
        // at the first byte no key matches (pinyin_parser2.cpp:237-328);
        // the capi parse seam must let those bytes reach the decoder.
        // Measured on the rebuilt pin: `ni hao` parses 2, `，nihao`
        // parses 0 (uncovered-surface differential, phase B).
        let mut session = session();
        session.replace_raw("ni hao").expect("cannot fail");
        assert_eq!(session.raw_input(), "ni hao");
        assert_eq!(session.full_parsed_len(), 2, "the space stops the parse");

        session.replace_raw("\u{ff0c}nihao").expect("cannot fail");
        assert_eq!(
            session.full_parsed_len(),
            0,
            "the full-width comma stops at byte 0"
        );

        session.replace_raw("nihao").expect("cannot fail");
        assert_eq!(session.full_parsed_len(), 5, "clean input unaffected");
    }

    #[test]
    fn replace_raw_consumes_trailing_and_standalone_apostrophe_runs() {
        // The inherited apostrophe class (ledgered on
        // fix/cursor-offset-normalization, folded into the termination
        // law): the pin's DP propagation consumes `'` bytes no key
        // covers — `ni'` parses 3, `nihao'` parses 6, `'''` parses 3
        // (the F-E-14 table).
        let mut session = session();
        session.replace_raw("ni'").expect("cannot fail");
        assert_eq!(session.full_parsed_len(), 3);

        session.replace_raw("nihao'").expect("cannot fail");
        assert_eq!(session.full_parsed_len(), 6);

        session.replace_raw("'''").expect("cannot fail");
        assert_eq!(session.full_parsed_len(), 3);

        session.replace_raw("ni'hao").expect("cannot fail");
        assert_eq!(
            session.full_parsed_len(),
            6,
            "internal runs stay covered by edges"
        );
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
    /// phrases, no model bytes (`docs/testing/fixture-adapters.md`).
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
            spans: Vec::new(),
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
            spans: Vec::new(),
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
                spans: Vec::new(),
                keys: 1,
                span: 3,
                cost: 10,
            },
            NbestRow {
                text: "\u{597d}".into(),
                tokens: vec![PhraseToken::new(0x101)],
                spans: Vec::new(),
                keys: 1,
                span: 3,
                cost: 20,
            },
            NbestRow {
                text: "\u{6d69}".into(),
                tokens: vec![PhraseToken::new(0x102)],
                spans: Vec::new(),
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
    fn an_nbest_row_chosen_from_a_reanchored_window_commits_only_its_text() {
        use crate::nbest::NbestRow;

        let mut session = train_session();
        session.type_pinyin("nihao").expect("typing cannot fail");

        // A single whole-composition sentence hypothesis: its span is the
        // full input, so selecting it from a re-anchored window must commit
        // the row's text alone — never the typed-but-unselected gap (which
        // would duplicate the raw prefix).
        session.nbest_rows = vec![NbestRow {
            text: "你好".into(),
            tokens: vec![PhraseToken::new(0x100), PhraseToken::new(0x101)],
            spans: Vec::new(),
            keys: 2,
            span: 5,
            cost: 10,
        }];
        session.refresh().expect("refresh cannot fail");

        // Re-anchor the window at offset 2 (mid-composition, before any
        // choose). The n-best row rides the prepend into the window.
        let window = session.candidates_at(2).expect("offset 2 is in range");
        let nbest = window
            .iter()
            .position(|candidate| candidate.nbest_row() == Some(0))
            .expect("the sentence row is prepended at a re-anchored offset");

        session
            .select_anchored(nbest, &window, 2)
            .expect("selection cannot fail");
        // The row's span is the whole composition, so the selection consumes
        // everything; commit() returns the row text with no raw prefix.
        assert_eq!(
            session.commit().expect("commit cannot fail"),
            "你好",
            "an n-best row from a re-anchored window commits its own text,\n\
             not the gap-prefixed duplicate"
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
    fn dynamic_adjust_folds_the_bigram_term_only_with_the_bit_and_a_gram() {
        use oxpinyin_core::DYNAMIC_ADJUST;
        use oxpinyin_core::MergedGram;
        use oxpinyin_core::OptionBits;

        let clear = OptionBits::default();
        let set = OptionBits::default().with(DYNAMIC_ADJUST, true);
        // 500 of a 1000-count row: the pin's bigram_freq / total.
        let gram = MergedGram::new(1_000, vec![(42, 500), (7, 100)]);

        assert_eq!(
            super::dynamic_adjust_bigram_possibility(clear, Some(&gram), 42),
            0.0,
            "bit clear omits the term however populated the row is"
        );
        assert_eq!(
            super::dynamic_adjust_bigram_possibility(set, None, 42),
            0.0,
            "no previous token means no gram was merged, so no term"
        );
        assert_eq!(
            super::dynamic_adjust_bigram_possibility(set, Some(&gram), 43),
            0.0,
            "a token the row misses contributes nothing"
        );
        assert_eq!(
            super::dynamic_adjust_bigram_possibility(set, Some(&gram), 42),
            0.5,
            "bit set with a merged row is bigram_freq / total"
        );

        // The term must actually move the frequency, not merely exist. This
        // is what a stub cannot satisfy: returning a constant zero
        // possibility leaves `adjusted` equal to `base`.
        let (unigram, total) = (1_234_u64, 51_051_831_u64);
        let base = super::amplified_frequency_with_bigram(unigram, total, 0.0);
        let adjusted = super::amplified_frequency_with_bigram(unigram, total, 0.5);
        assert!(
            adjusted > base,
            "a non-zero possibility must raise the amplified frequency ({adjusted} vs {base})"
        );
        assert_eq!(
            base,
            super::amplified_frequency(unigram, total),
            "the bit-clear path is the pre-existing unigram law exactly"
        );
    }

    /// The frozen candidate pins were measured with DYNAMIC_ADJUST clear on
    /// both sides, which is the whole reason implementing it cannot move
    /// them. That safety argument is **not** "the term is zero at offset 0"
    /// — upstream's `_get_previous_token` answers `sentence_start` (1) there,
    /// not `null_token`, so Gate 2 fires and a real gram is merged. The
    /// argument is only that the bit is clear in every frozen word.
    ///
    /// So this reads the harness's own option words rather than a copy of
    /// them: adding the bit to a frozen profile fails here instead of
    /// silently moving pins.
    #[test]
    fn no_frozen_option_word_sets_dynamic_adjust() {
        use std::path::Path;

        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/bisection");
        let mut checked = 0_usize;
        for entry in std::fs::read_dir(&dir).expect("the bisection harness is in-tree") {
            let path = entry.expect("readable dir entry").path();
            if path.extension().and_then(|e| e.to_str()) != Some("c") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("readable source");
            for line in source.lines() {
                let Some(rest) = line.strip_prefix("#define PARITY_OPTIONS") else {
                    continue;
                };
                let hex = rest
                    .split("0x")
                    .nth(1)
                    .expect("PARITY_OPTIONS is written in hex")
                    .trim_end_matches(|c: char| !c.is_ascii_hexdigit());
                let word = u32::from_str_radix(hex, 16).expect("parsable hex option word");
                assert_eq!(
                    word & 0x200,
                    0,
                    "{}: frozen option word 0x{word:x} sets DYNAMIC_ADJUST (1<<9). \
                     The frozen candidate pins were measured with it clear; setting it \
                     changes candidate ranking at every offset, including 0.",
                    path.display()
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "found no PARITY_OPTIONS definitions to check — the guard would be vacuous"
        );
    }

    /// A system-only facade with a caller-chosen entry count, so a guess's
    /// merge count can be measured against its candidate count.
    struct SystemPhrases(Vec<PhraseEntry>);

    impl Dictionary for SystemPhrases {
        type Entry = PhraseEntry;
        type Error = EngineError;
        type Syllable = SyllableKey;

        fn lookup(&self, _syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, EngineError> {
            Ok(self.0.clone())
        }
    }

    /// Fixed unigrams plus one merged bigram row, counting how often the
    /// engine asks for that row and recording which token it asked about.
    ///
    /// The counter is the point: upstream merges ONE gram per
    /// `pinyin_guess_candidates` and indexes it per candidate
    /// (`pinyin.cpp:2200-2224`). A count that tracks the candidate list is
    /// the complexity regression the source policy forbids, and it is
    /// invisible to any output-only assertion.
    struct CountingBigrams {
        unigrams: FixedUnigrams,
        row: Option<oxpinyin_core::MergedGram>,
        merges: std::rc::Rc<std::cell::Cell<usize>>,
        asked_about: std::rc::Rc<std::cell::Cell<u32>>,
    }

    impl LanguageModel for CountingBigrams {
        type Error = EngineError;
        type Token = PhraseToken;

        fn score(
            &self,
            history: &[PhraseToken],
            token: &PhraseToken,
            edge_cost: Cost,
        ) -> Result<Cost, EngineError> {
            self.unigrams.score(history, token, edge_cost)
        }

        fn has_real_unigrams(&self) -> bool {
            true
        }

        fn unigram_freq(&self, token: &PhraseToken) -> Result<Option<u64>, EngineError> {
            self.unigrams.unigram_freq(token)
        }

        fn unigram_total(&self) -> Result<Option<u64>, EngineError> {
            self.unigrams.unigram_total()
        }

        fn addon_unigram_freq(&self, token: &PhraseToken) -> Result<Option<u64>, EngineError> {
            self.unigrams.addon_unigram_freq(token)
        }

        fn addon_unigram_total(&self) -> Result<Option<u64>, EngineError> {
            self.unigrams.addon_unigram_total()
        }

        fn merged_successors(
            &self,
            prev: &PhraseToken,
        ) -> Result<Option<oxpinyin_core::MergedGram>, EngineError> {
            self.merges.set(self.merges.get() + 1);
            self.asked_about.set(prev.value());
            Ok(self.row.clone())
        }
    }

    /// The three gates end to end through `Session`, which the C-level
    /// differential cannot demonstrate here: the in-tree `fixtures/w3` mini
    /// tables answer `no-first-candidate` for nearly every input, so that
    /// differential self-skips without the oracle and proves nothing on its
    /// own. This test needs no oracle and no data set.
    ///
    /// Gate 1 — the previous token: at offset 0 upstream answers
    /// `sentence_start` (1), not `null_token`. Gate 2 — one merge per
    /// guess, never one per candidate. Gate 3 — the possibility joins the
    /// unigram term inside the pin's single truncation, and only for the
    /// token the row credits.
    #[test]
    fn dynamic_adjust_merges_one_row_per_guess_and_lifts_only_the_credited_token() {
        use oxpinyin_core::{DYNAMIC_ADJUST, MergedGram, OptionBits, PINYIN_INCOMPLETE};
        use std::cell::Cell;
        use std::rc::Rc;

        const TEXTS: [&str; 6] = ["系", "统", "习", "题", "集", "锦"];
        const FIRST: u32 = 0x0100_0001;
        const SECOND: u32 = 0x0100_0002;

        struct Run {
            texts: Vec<String>,
            merges: usize,
            asked_about: u32,
        }

        fn run(entries: usize, dynamic: bool, row: Option<MergedGram>) -> Run {
            let merges = Rc::new(Cell::new(0_usize));
            let asked_about = Rc::new(Cell::new(u32::MAX));
            let phrases = (0..entries)
                .map(|index| {
                    PhraseEntry::new(
                        PhraseToken::new(FIRST + u32::try_from(index).expect("small index")),
                        TEXTS[index].to_owned(),
                    )
                })
                .collect();
            let mut session = Session::new(
                &EmptyConfigSource,
                StoragePaths::new("user"),
                SystemPhrases(phrases),
                CountingBigrams {
                    unigrams: FixedUnigrams {
                        system: 13,
                        addon: 0,
                        total: 51_051_831,
                        addon_total: 1,
                    },
                    row,
                    merges: Rc::clone(&merges),
                    asked_about: Rc::clone(&asked_about),
                },
            )
            .expect("Session::new");
            // Both arms set the same word apart from the one bit, so nothing
            // else about the parse can differ between them.
            session
                .set_options(
                    OptionBits::default()
                        .with(PINYIN_INCOMPLETE, true)
                        .with(DYNAMIC_ADJUST, dynamic),
                )
                .expect("set_options");
            session.type_pinyin("a").expect("typing cannot fail");
            Run {
                texts: session
                    .candidates()
                    .iter()
                    .map(|candidate| candidate.text().to_owned())
                    .collect(),
                merges: merges.get(),
                asked_about: asked_about.get(),
            }
        }

        // A row that credits the SECOND phrase with half its mass. The
        // unigram answer is a constant across tokens, so with the bit clear
        // the two candidates tie on all three RankKeys and hold collection
        // order; only the bigram term can separate them.
        let credits_second = || MergedGram::new(1_000, vec![(SECOND, 500)]);

        // Gate 1, bit clear: the model is never consulted at all, and the
        // order is the pre-existing unigram law's.
        let clear = run(2, false, Some(credits_second()));
        assert_eq!(
            clear.merges, 0,
            "with the bit clear upstream never reaches the merge, so neither may this"
        );
        assert_eq!(
            clear.texts,
            ["系", "统"],
            "the bit-clear order is the frozen unigram-only order"
        );

        // Gate 1, bit set: offset 0 resolves to `sentence_start`, not a null
        // token — the premise that offset 0 is safe by construction is false,
        // and this is the assertion that says so.
        let no_row = run(2, true, None);
        assert_eq!(
            no_row.asked_about,
            crate::nbest::SENTENCE_START,
            "offset 0 asks about sentence_start, exactly as `_get_previous_token` answers"
        );
        assert_eq!(
            no_row.texts,
            ["系", "统"],
            "a model with no row for the previous token contributes no term"
        );

        // Gate 3: the credited token overtakes a candidate it ties with on
        // every other key.
        let lifted = run(2, true, Some(credits_second()));
        assert_eq!(
            lifted.texts,
            ["统", "系"],
            "the bigram term must actually move the credited candidate above its tie peer"
        );

        // Gate 2: the merge count is a property of the guess, not of the
        // candidate list. Tripling the candidates must not change it.
        assert!(
            lifted.merges > 0,
            "the bit is set and a previous token exists, so a merge must happen"
        );
        let wider = run(6, true, Some(credits_second()));
        assert_eq!(
            wider.merges,
            lifted.merges,
            "merging is once per guess ({} candidates merged {} times, {} candidates merged {} \
             times): a count that tracks the candidate list is the O(candidates) regression the \
             source policy forbids",
            lifted.texts.len(),
            lifted.merges,
            wider.texts.len(),
            wider.merges
        );
        assert_eq!(
            wider.texts.first().map(String::as_str),
            Some("统"),
            "the credited token leads a wider list too"
        );
    }

    #[test]
    fn amplified_frequency_pins_the_class_a_probe_values() {
        // The denominator is the pin's phrase-index total over model20:
        // interpolation2 sum 50_913_735 + 138_096 items, each item's baked
        // unigram being its interpolation2 count + 1 (probe-verified over
        // the whole index; `docs/testing/corpus-tail.md` Class A). The
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
            .filter_map(|c| c.token().map(oxpinyin_core::PhraseToken::value))
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
            .candidate_frequencies(&collected, None)
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
        let texts: Vec<&str> = session
            .candidates()
            .iter()
            .map(super::super::candidate::Candidate::text)
            .collect();
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
            true,
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
        let columns = super::build_scan_matrix(&toneless, incomplete, true);
        assert!(
            columns[0]
                .iter()
                .any(|key| key.key.text() == "an" && key.to == 2)
        );

        let toned_pair = SegmentGraph::build_with_options(b"a4nan", toned).expect("valid");
        let columns = super::build_scan_matrix(&toned_pair, toned, true);
        assert!(!columns[0].iter().any(|key| key.key.text() == "an"));

        // Divided: "bian" divides toneless; "bian4" carries its tone instead.
        let toneless = SegmentGraph::build_with_options(b"bian", incomplete).expect("valid");
        let columns = super::build_scan_matrix(&toneless, incomplete, true);
        assert!(
            columns[0]
                .iter()
                .any(|key| key.key.text() == "bi" && key.to == 2)
        );

        let toned_key = SegmentGraph::build_with_options(b"bian4", toned).expect("valid");
        let columns = super::build_scan_matrix(&toned_key, toned, true);
        assert!(
            columns[0]
                .iter()
                .any(|key| key.key.text() == "bian" && key.tone == 4)
        );
        assert!(!columns[0].iter().any(|key| key.key.text() == "bi"));
    }

    /// A model whose n-best step costs exist, so the trellis runs: both
    /// branches at a fixed cost, `score` passes the edge through.
    struct TrellisModel {
        blended: Cost,
        unigram: Cost,
    }

    impl LanguageModel for TrellisModel {
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

        fn nbest_step_costs(
            &self,
            _prev: &PhraseToken,
            _token: &PhraseToken,
        ) -> Result<NbestStepCosts, EngineError> {
            Ok(NbestStepCosts {
                blended: Some(self.blended),
                unigram: Some(self.unigram),
            })
        }
    }

    fn trellis_session() -> Session<FixtureDictionary, TrellisModel> {
        Session::new(
            &EmptyConfigSource,
            StoragePaths::new("user"),
            FixtureDictionary::parse(TRAIN_VOCAB).expect("authored fixture"),
            TrellisModel {
                blended: 100,
                unigram: 200,
            },
        )
        .expect("the fixtures open")
    }

    /// Types `text` and selects the candidate carrying `token`.
    fn type_and_select_over<M>(session: &mut Session<FixtureDictionary, M>, text: &str, token: u32)
    where
        M: LanguageModel<Token = PhraseToken>,
        M::Error: std::fmt::Display,
    {
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

    /// §3: a chosen candidate forces its span — the constrained walk pins
    /// the chosen 你 and decodes the continuation, so the row carries the
    /// full sentence.
    #[test]
    fn a_selection_forces_its_span_in_the_sentence_walk() {
        let mut session = trellis_session();
        type_and_select_over(&mut session, "nihao", 1);
        assert!(
            session.guess_sentence().expect("guess cannot fail"),
            "the constrained walk runs"
        );
        assert_eq!(
            session.sentence_text(0).expect("row 0 exists"),
            "\u{4f60}\u{597d}",
            "the forced 你 leads the decoded continuation"
        );
    }

    /// L1: a terminal selection still answers — the walk covers the full
    /// matrix, so a fully-consumed composition has rows.
    #[test]
    fn a_terminal_selection_still_answers_the_full_matrix() {
        let mut session = trellis_session();
        type_and_select_over(&mut session, "ni", 1);
        assert_eq!(session.raw_input(), "ni");
        assert!(
            session.guess_sentence().expect("guess cannot fail"),
            "the fully-consumed composition walks the full matrix"
        );
        let rows: Vec<&str> = (0..3)
            .filter_map(|index| session.sentence_text(index))
            .collect();
        assert_eq!(
            rows,
            ["\u{4f60}", "\u{4f60}"],
            "the forced phrase is the row — twice: the bigram and unigram branch \
             lineages of the same token, the shape the oracle's terminal-choose \
             rows show (the candidate window dedups them)"
        );
    }

    /// L2: the forcing survives further typing and is released only by
    /// the full reset.
    #[test]
    fn the_forcing_survives_typing_and_releases_only_on_reset() {
        let mut session = trellis_session();
        type_and_select_over(&mut session, "nihao", 1);
        for character in "s".chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
        assert!(
            session.guess_sentence().expect("guess cannot fail"),
            "the walk runs over the extended buffer"
        );
        assert!(
            session
                .sentence_text(0)
                .expect("row 0 exists")
                .starts_with('\u{4f60}'),
            "the forcing survived the keystroke"
        );

        session.reset();
        assert!(
            !session.clear_constraint(0),
            "the reset released the forcing — the store is empty"
        );
        for character in "nihaos".chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
        assert!(
            session.guess_sentence().expect("guess cannot fail"),
            "the post-reset walk runs"
        );
    }

    /// `pinyin_clear_constraint`'s engine half: a hit inside a run
    /// un-forces the whole run, the selection record follows the
    /// survivors, and a free or out-of-range offset answers false.
    #[test]
    fn clear_constraint_unforces_the_run_and_rebuilds_the_record() {
        let mut session = train_session();
        type_and_select(&mut session, "nihao", 1);
        type_and_select(&mut session, "hao", 2);
        assert_eq!(
            session.selected_tokens(),
            [PhraseToken::new(1), PhraseToken::new(2)]
        );

        // Out of range answers false, never panic.
        assert!(!session.clear_constraint(999));

        // A hit anywhere inside the head run — its NoSearch interior
        // included — un-forces the whole run; the record follows the
        // survivor.
        assert!(session.clear_constraint(1));
        assert_eq!(session.selected_tokens(), [PhraseToken::new(2)]);
        assert!(!session.clear_constraint(0), "the head run is already free");

        assert!(session.clear_constraint(2));
        assert!(session.selected_tokens().is_empty());
        assert!(!session.clear_constraint(0));
    }

    /// Review regression: clearing a tail forcing re-opens a committed
    /// composition. The rebuild once left `selection_committed` set, so
    /// the next compatible re-parse started fresh and silently dropped
    /// the surviving head forcing.
    #[test]
    fn clearing_a_tail_forcing_reopens_a_committed_composition() {
        let mut session = trellis_session();
        for character in "nihaoshijie".chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
        let select_token = |session: &mut Session<FixtureDictionary, TrellisModel>, token| {
            let index = session
                .candidates()
                .iter()
                .position(|candidate| candidate.token() == Some(PhraseToken::new(token)))
                .expect("the fixture candidate is offered");
            session.select(index).expect("selection cannot fail");
        };
        select_token(&mut session, 1); // 你 over [0,2)
        select_token(&mut session, 2); // 好 over [2,5)
        // The remainder has no window candidates: select the raw-text
        // fallback to consume the buffer — the commit-branch shape.
        let index = session
            .candidates()
            .iter()
            .position(|candidate| candidate.kind() == crate::CandidateKind::Fallback)
            .expect("the fallback covers the remainder");
        session.select(index).expect("selection cannot fail");
        assert!(
            session.selection_committed(),
            "the last selection consumed the buffer"
        );

        // Un-force the tail run; the record rebuilds over the survivor
        // and the composition is open again.
        assert!(session.clear_constraint(2));
        assert!(
            !session.selection_committed(),
            "the rebuild re-opened the composition"
        );
        assert_eq!(session.selected_tokens(), [PhraseToken::new(1)]);

        // Further typing keeps the surviving 你 forcing alive.
        for character in "s".chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
        assert!(
            session.guess_sentence().expect("guess cannot fail"),
            "the walk runs"
        );
        assert!(
            session
                .sentence_text(0)
                .expect("row 0 exists")
                .starts_with('\u{4f60}'),
            "the surviving head forcing outlived the clearing and the typing"
        );
    }

    /// Review regression: the clamp reconcile must not depend on a
    /// forcing being dropped. A row-0 n-best choose writes the RECORD
    /// (the whole row's text, tokens, and span-end cursor) while
    /// `diff_result` writes no run — the store stays empty. A shrinking
    /// re-parse then clamps the cursor backward with nothing to drop,
    /// and the rebuild must run anyway: the record's coverage exceeds
    /// the new input, and Enter would commit the stale row text for an
    /// input that can no longer produce it.
    #[test]
    fn a_shrinking_reparse_reconciles_a_constraint_free_row_selection() {
        let mut session = trellis_session();
        for character in "nihao".chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
        assert!(session.guess_sentence().expect("guess cannot fail"));
        let row0 = session
            .candidates()
            .iter()
            .position(|candidate| {
                candidate.kind() == crate::CandidateKind::Sentence
                    && candidate.nbest_row() == Some(0)
            })
            .expect("the row-0 sentence is offered");
        assert_eq!(
            session.select(row0).expect("selection cannot fail"),
            Selection::Completed
        );
        assert!(session.selection_committed());
        assert!(
            !session.constraints.is_active(),
            "the row-0 choose wrote no forcing"
        );

        // The shrinking re-parse: the clamp moves the cursor backward
        // with an empty store — the rebuild runs regardless.
        session.replace_raw("ni").expect("replace cannot fail");
        assert!(
            session.selected_tokens().is_empty(),
            "the constraint-free row record reconciled away"
        );
        assert_eq!(
            session.composition_offset(),
            0,
            "the composition re-opened at 0"
        );

        let outcome = session
            .process_key(&KeyInput::plain(LogicalKey::Enter))
            .expect("enter on a composing session cannot fail");
        assert_eq!(
            outcome,
            KeyOutcome::Commit("ni".to_owned()),
            "the commit answers the current input, never the stale row text"
        );
    }

    /// Review regression: the reconcile must fire on canonical
    /// divergence, not only on a backward clamp. A committed selection
    /// covers `raw[..consumed]`; a replacement that does not extend
    /// those bytes — here the same-length "mihao" over "nihao" — leaves
    /// the cursor unclamped but the selection stale, and the forcing no
    /// longer spells over the new bytes. The spell-probe validate drops
    /// it at the replacement and the record follows; Enter commits the
    /// current input, never the stale \u{4f60}\u{597d}. (Through the
    /// transformed seams this is the shape a scheme-coordinate
    /// continuation passing while the canonical spelling diverges — a
    /// live scheme switch — produces.)
    #[test]
    fn a_divergent_replacement_reconciles_the_committed_selection() {
        let mut session = trellis_session();
        for character in "nihao".chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
        for token in [1_u32, 2] {
            let index = session
                .candidates()
                .iter()
                .position(|candidate| candidate.token() == Some(PhraseToken::new(token)))
                .expect("the fixture candidate is offered");
            session.select(index).expect("selection cannot fail");
        }
        assert!(session.selection_committed());

        // Same length, divergent inside the covered span: no clamp, so
        // only the continuity check can reach the reconcile. The 你 run
        // no longer spells over "mi" (the spell probe drops it) and the
        // 好 run overruns the spellable bound (the bounds drop).
        session.replace_raw("mixxo").expect("replace cannot fail");
        assert!(
            session.selected_tokens().is_empty(),
            "the diverged selection reconciled away"
        );
        assert_eq!(
            session.composition_offset(),
            0,
            "the composition re-opened at 0"
        );

        let outcome = session
            .process_key(&KeyInput::plain(LogicalKey::Enter))
            .expect("enter on a composing session cannot fail");
        assert_eq!(
            outcome,
            KeyOutcome::Commit("mixxo".to_owned()),
            "the commit answers the current input, never the stale selection"
        );
    }

    /// The engine-internal backspace keeps the forcing: erase shrinks
    /// the raw buffer one keystroke at a time (the engine's own
    /// backspace path — the capi's shrink is the same rule through
    /// `begin_parse`), the store survives, and the next guess
    /// re-validates: down to the forcing's own floor the row is the
    /// forced phrase alone, and re-typing continues the composition
    /// with the forcing intact.
    #[test]
    fn the_forcing_survives_the_engine_backspace_and_retype() {
        let mut session = trellis_session();
        for character in "nihaoshijie".chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
        let index = session
            .candidates()
            .iter()
            .position(|candidate| candidate.token() == Some(PhraseToken::new(1)))
            .expect("the fixture candidate is offered");
        session.select(index).expect("selection cannot fail");

        // Backspace the buffer down to the forcing's floor.
        for _ in 0..9 {
            assert_eq!(
                session.process_key(&KeyInput::plain(LogicalKey::Backspace)),
                Ok(KeyOutcome::Consumed),
                "erase pops while input remains"
            );
        }
        assert_eq!(session.raw_input(), "ni");
        assert!(
            session.guess_sentence().expect("guess cannot fail"),
            "the floor still walks"
        );
        assert_eq!(
            session.sentence_text(0).expect("row 0 exists"),
            "\u{4f60}",
            "the forced phrase is the floor's row"
        );

        // The re-type continues the open composition with the forcing.
        for character in "haoshijie".chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
        assert!(
            session.guess_sentence().expect("guess cannot fail"),
            "the retype walks"
        );
        assert!(
            session
                .sentence_text(0)
                .expect("row 0 exists")
                .starts_with('\u{4f60}'),
            "the forcing survived the backspace and the retype"
        );

        // The all-or-nothing erase (nothing left to pop) un-selects
        // everything, store included.
        for _ in 0..11 {
            let _ = session.process_key(&KeyInput::plain(LogicalKey::Backspace));
        }
        assert!(
            !session.clear_constraint(0),
            "the un-select cleared the store"
        );
    }

    /// The train fallback's boundary: a row-0 choose constrains nothing
    /// (upstream-faithful), so the record — not the result — carries the
    /// training; a result that sits on a forcing takes the constrained
    /// walk instead (the test above). This pins the trigger, not just the
    /// outcome: taking the constrained path here would observe nothing.
    /// Review regression: a rank-greater-than-zero row choose on a FRESH
    /// composition records its forcing. The row branch once called
    /// diff_result on a store that was never sized — `add` refused every
    /// span past an empty cell count, and the forcing silently never
    /// landed.
    #[test]
    fn a_fresh_composition_row_choose_records_its_forcing() {
        let mut session = train_session();
        session.type_pinyin("nihao").expect("typing cannot fail");
        // Hand-crafted rows whose rank-1 phrase differs from row 0's —
        // the shifted-row shape (`sentence-surface.md` §8).
        session.nbest_rows = vec![
            crate::nbest::NbestRow {
                text: "\u{597d}".into(),
                tokens: vec![PhraseToken::new(0x100)],
                spans: vec![crate::constraint::PhraseSpan {
                    start: 0,
                    token: PhraseToken::new(0x100),
                    text: "\u{597d}".into(),
                }],
                keys: 1,
                span: 3,
                cost: 10,
            },
            crate::nbest::NbestRow {
                text: "\u{6d69}".into(),
                tokens: vec![PhraseToken::new(0x102)],
                spans: vec![crate::constraint::PhraseSpan {
                    start: 0,
                    token: PhraseToken::new(0x102),
                    text: "\u{6d69}".into(),
                }],
                keys: 1,
                span: 3,
                cost: 30,
            },
        ];
        session.refresh().expect("refresh cannot fail");
        let index = session
            .candidates()
            .iter()
            .position(|candidate| candidate.nbest_row() == Some(1))
            .expect("the rank-1 row is offered");
        session.select(index).expect("the row is choosable");
        assert!(
            session.clear_constraint(0),
            "the differing phrase's forcing landed on the fresh composition"
        );
    }

    /// Review regression: the record rebuild keeps the raw text of the
    /// gaps between forcings — `diff_result` leaves unchanged phrases
    /// free, and the preedit must not drop exactly those bytes.
    #[test]
    fn a_record_rebuild_keeps_the_gap_text_between_forcings() {
        let mut session = train_session();
        session
            .type_pinyin("nihaoshijie")
            .expect("typing cannot fail");
        // A gapped store straight from diff_result's shape: 你 over
        // [0,2), a free gap "ha" over [2,5), a forcing over [5,10).
        session.constraints.resize(12);
        session
            .constraints
            .add(0, 2, PhraseToken::new(1), "\u{4f60}".into());
        session
            .constraints
            .add(5, 10, PhraseToken::new(2), "\u{4e16}\u{754c}".into());
        session.rebuild_selection_from_constraints();
        let preedit = session.preedit();
        assert!(
            preedit.text().starts_with("\u{4f60}hao\u{4e16}\u{754c}"),
            "the gap's raw bytes survived the rebuild: got {:?}",
            preedit.text()
        );
    }

    #[test]
    fn a_row_zero_choose_trains_through_the_record() {
        let mut session = trellis_session();
        for character in "nihao".chars() {
            session
                .process_key(&KeyInput::character(character))
                .expect("typing cannot fail");
        }
        assert!(
            session.guess_sentence().expect("guess cannot fail"),
            "rows exist before the choose"
        );
        let row = session
            .candidates()
            .iter()
            .position(|candidate| candidate.nbest_row() == Some(0))
            .expect("the rank-0 row is offered at the head");
        session.select(row).expect("the row is choosable");
        assert!(
            !session.clear_constraint(0),
            "a row-0 choose recorded no forcing"
        );
        let row_tokens: Vec<PhraseToken> = session.selected_tokens().to_vec();
        assert_eq!(
            row_tokens,
            vec![PhraseToken::new(1), PhraseToken::new(2)],
            "the row's whole path is the record"
        );

        let mut recorder = Recorder {
            observed: Vec::new(),
        };
        session.train(&mut recorder).expect("train cannot fail");
        assert_eq!(
            recorder.observed,
            vec![
                (Vec::new(), PhraseToken::new(1)),
                (vec![PhraseToken::new(1)], PhraseToken::new(2)),
            ],
            "the record walked — the forcing-less result trained nothing by itself"
        );
    }

    /// L3: the constraint-aware train walk — the forced phrase and the
    /// first decoded phrase after it train, with the predecessor threading
    /// over every phrase.
    #[test]
    fn a_decoded_continuation_trains_through_the_constraint_walk() {
        let mut session = trellis_session();
        type_and_select_over(&mut session, "nihao", 1);
        assert!(
            session.guess_sentence().expect("guess cannot fail"),
            "the constrained decode ran"
        );
        let mut recorder = Recorder {
            observed: Vec::new(),
        };
        session.train(&mut recorder).expect("train cannot fail");
        assert_eq!(
            recorder.observed,
            vec![
                (Vec::new(), PhraseToken::new(1)),
                (vec![PhraseToken::new(1)], PhraseToken::new(2)),
            ],
            "你 (forced) then 好 (first decoded after the run) train, 你→好 included"
        );
    }

    #[test]
    fn replace_raw_walks_consumed_back_to_a_char_boundary() {
        // A one-byte composition selected to consumed 1, then replaced by
        // `，` (three bytes): the stale consumed sits inside the character
        // and `refresh` slices `raw[consumed..]`. The clamp must walk back
        // to the boundary before it — nothing panics on any input.
        let mut session = session();
        session.replace_raw("a").expect("cannot fail");
        session.select(0).expect("the fallback row selects");
        session.replace_raw("\u{ff0c}").expect("cannot fail");
        assert_eq!(session.composition_offset(), 0);
    }

    #[test]
    fn candidates_at_rejects_mid_character_offsets() {
        // The full-width comma occupies bytes 0..3, so offsets 1 and 2 sit
        // inside it: no window exists under a mid-character slice, and the
        // offset is refused with the inside-character error — not rounded
        // to a neighbour and not the out-of-range error, whose contract is
        // past-one-past-end only.
        let mut session = session();
        session.replace_raw("\u{ff0c}nihao").expect("cannot fail");
        for offset in [1, 2] {
            assert!(
                matches!(
                    session.candidates_at(offset),
                    Err(EngineError::LookupOffsetInsideCharacter { .. })
                ),
                "offset {offset} is inside the character and must be refused"
            );
        }
        assert!(session.candidates_at(3).is_ok(), "offset 3 is a boundary");
    }

    #[test]
    fn candidates_at_mid_syllable_offsets_answer_the_empty_column() {
        use super::CandidateKind;

        // "nihaoshijie" parses ni|hao|shi|jie: the matrix keys start at
        // 0/2/5/8, so bytes 1/3/4/6/7/9 are the pin's empty columns —
        // `search_matrix` matches nothing from them (`pinyin.cpp:2224-2262`),
        // and the window is the raw-suffix fallback alone, never the suffix
        // re-parse (offset 3 must not answer the `ao…` window, 6 not the
        // `h…` window). The syllable starts keep their windows.
        let mut session = train_session();
        session
            .type_pinyin("nihaoshijie")
            .expect("typing cannot fail");
        for offset in [1usize, 3, 4, 6, 7, 9] {
            let window = session
                .candidates_at(offset)
                .expect("a mid-syllable offset is in range");
            assert!(
                window
                    .iter()
                    .all(|cand| cand.kind() == CandidateKind::Fallback),
                "offset {offset}: only the fallback row, no suffix re-parse"
            );
            assert_eq!(
                window.iter().count(),
                1,
                "offset {offset}: the fallback alone"
            );
        }
        assert!(
            session
                .candidates_at(2)
                .expect("offset 2 is a syllable start")
                .iter()
                .any(|cand| cand.kind() != CandidateKind::Fallback),
            "offset 2 keeps the hao window"
        );
    }

    #[test]
    fn candidates_at_mid_syllable_keeps_the_prepended_nbest_rows() {
        use super::CandidateKind;
        use crate::nbest::NbestRow;

        // The pin prepends `m_nbest_results` whether or not the span search
        // finds anything, so a post-sentence mid-syllable window is the
        // n-best rows (measured: `nihaoshijie@3` after `guess_sentence`
        // answers n=3 on the pin) over the fallback — not an empty list.
        let mut session = train_session();
        session
            .type_pinyin("nihaoshijie")
            .expect("typing cannot fail");
        session.nbest_rows = vec![NbestRow {
            text: "你好世界".into(),
            tokens: vec![
                PhraseToken::new(0x100),
                PhraseToken::new(0x101),
                PhraseToken::new(0x102),
            ],
            spans: Vec::new(),
            keys: 3,
            span: 11,
            cost: 10,
        }];
        session.refresh().expect("refresh cannot fail");

        let window = session.candidates_at(3).expect("offset 3 is in range");
        assert!(
            window
                .iter()
                .any(|cand| cand.kind() == CandidateKind::Sentence && cand.text() == "你好世界"),
            "the n-best row rides the prepend at the empty column"
        );
        assert!(
            window
                .iter()
                .filter(|cand| cand.kind() != CandidateKind::Fallback)
                .count()
                == 1,
            "no phrase rows join the n-best row at the empty column"
        );
    }

    #[test]
    fn candidates_at_the_apostrophe_column_is_transparent() {
        // `ni'hao` parses ni|hao with the zero-key column at 2; the pin's
        // span search steps over it (measured: `ni'hao@2` answers the hao
        // window, n=93), so the apostrophe byte answers the next key's
        // window instead of collapsing to the empty-column law.
        let mut session = train_session();
        session.type_pinyin("ni'hao").expect("typing cannot fail");
        let at_separator = session
            .candidates_at(2)
            .expect("the separator byte is in range");
        let at_start = session.candidates_at(3).expect("the hao start is in range");
        assert_eq!(
            at_separator
                .iter()
                .map(super::super::candidate::Candidate::text)
                .collect::<Vec<_>>(),
            at_start
                .iter()
                .map(super::super::candidate::Candidate::text)
                .collect::<Vec<_>>(),
            "the zero-key column answers the following key's window"
        );
        assert!(
            !at_separator.is_empty(),
            "the stepped-over window is the hao window, not the empty column"
        );
    }

    #[test]
    fn candidates_at_an_incomplete_tail_column_stays_empty() {
        use super::CandidateKind;

        // "nihaozh" under INCOMPLETE keeps `zh` as one matrix key at
        // 5..7 (measured: `nihaozh@6` answers n-best rows only on the
        // pin), so byte 6 is an empty column even though the lone suffix
        // `h` could start a parse of its own.
        let mut session = train_session();
        session.type_pinyin("nihaozh").expect("typing cannot fail");
        let window = session.candidates_at(6).expect("byte 6 is in range");
        assert!(
            window
                .iter()
                .all(|cand| cand.kind() == CandidateKind::Fallback),
            "byte 6 inside the `zh` key is an empty column, not an h window"
        );
    }

    #[test]
    fn candidates_at_an_apostrophe_beyond_the_parse_span_stays_empty() {
        use super::CandidateKind;

        // "ni,'hao" parses only `ni` — the comma stops the parse — so the
        // apostrophe at byte 3 sits outside the matrix: the pin aborts
        // there (measured SIGABRT, the same out-of-matrix landmine as one
        // past a lone zero-key run), and the empty-column window is the
        // no-abort answer, not a re-parse of the `'hao` suffix. The
        // in-span apostrophe of `ni'hao` stays transparent.
        let mut session = train_session();
        session.replace_raw("ni,'hao").expect("cannot fail");
        let window = session.candidates_at(3).expect("byte 3 is in range");
        assert!(
            window
                .iter()
                .all(|cand| cand.kind() == CandidateKind::Fallback),
            "byte 3 is outside the parse span — no suffix re-parse window"
        );

        let mut session = train_session();
        session.replace_raw("ni'hao").expect("cannot fail");
        let at_separator = session
            .candidates_at(2)
            .expect("the in-span separator byte is transparent");
        assert!(
            at_separator
                .iter()
                .any(|cand| cand.kind() != CandidateKind::Fallback),
            "the in-span apostrophe keeps the hao window"
        );
    }

    #[test]
    fn candidates_at_a_divided_split_column_answers_the_split_window() {
        use super::CandidateKind;

        // The pin's divided table splits `jie` into `ji` + `e`
        // (`special_table.h:16`, `inner_split_step` under
        // `USE_DIVIDED_TABLE`), so byte 10 of `nihaoshijie` — the `e`
        // half's start — is a live matrix column, and the pin answers the
        // e-family window there (measured: fresh n=190, 阿 first). The
        // mid-chunk bytes 3/4/6 stay empty: `hao`/`shi` are not divided
        // entries. `fangan` resplits `fan`+`gan` into `fang`+`an`
        // (`resplit_step`), making byte 4 live the same way.
        //
        // The plain fixture carries no `e`-key token, which would make the
        // live column indistinguishable from an empty one through
        // `candidates_at` (both answer the fallback alone), so this test's
        // session adds one.
        const SPLIT_VOCAB: &str = "token=1\tkeys=ni\ttext=你\tunigram=1000\n\
                                   token=2\tkeys=hao\ttext=好\tunigram=900\n\
                                   token=3\tkeys=e\ttext=恶\tunigram=800\n";
        let split_session = || {
            Session::new(
                &EmptyConfigSource,
                StoragePaths::new("user"),
                FixtureDictionary::parse(SPLIT_VOCAB).expect("authored fixture"),
                FixtureLanguageModel::parse(SPLIT_VOCAB, "").expect("authored fixture"),
            )
            .expect("the fixtures open")
        };

        let mut session = split_session();
        session
            .type_pinyin("nihaoshijie")
            .expect("typing cannot fail");
        assert!(
            session.spans_a_matrix_key(10).expect("byte 10 is in range"),
            "byte 10 is the ji|e divided-split column"
        );
        for offset in [3usize, 4, 6] {
            assert!(
                !session.spans_a_matrix_key(offset).expect("in range"),
                "byte {offset} is an empty column"
            );
        }

        // Through the public surface: byte 10 answers the e-family window
        // (the fixture's 恶 row), the mid-chunk offsets stay fallback-only.
        let e_window = session.candidates_at(10).expect("byte 10 is in range");
        assert!(
            e_window
                .iter()
                .any(|cand| cand.kind() != CandidateKind::Fallback && cand.text() == "恶"),
            "byte 10 answers the divided e window"
        );
        for offset in [3usize, 4, 6] {
            let window = session.candidates_at(offset).expect("in range");
            assert!(
                window
                    .iter()
                    .all(|cand| cand.kind() == CandidateKind::Fallback),
                "offset {offset} is an empty column — fallback only"
            );
        }

        let mut session = split_session();
        session.type_pinyin("fangan").expect("typing cannot fail");
        assert!(
            session.spans_a_matrix_key(4).expect("byte 4 is in range"),
            "byte 4 is the fang|an resplit column"
        );
        assert!(
            !session.spans_a_matrix_key(5).expect("in range"),
            "byte 5 is an empty column"
        );
        let window = session.candidates_at(5).expect("byte 5 is in range");
        assert!(
            window
                .iter()
                .all(|cand| cand.kind() == CandidateKind::Fallback),
            "byte 5 is an empty column — fallback only"
        );
    }
}
