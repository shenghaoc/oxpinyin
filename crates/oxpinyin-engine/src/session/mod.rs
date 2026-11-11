//! The session state machine.
//!
//! One session per input context, with parse, candidate, selection and
//! training operations shared by the in-tree consumers.

use core::fmt::Display;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use smallvec::SmallVec;

use oxpinyin_core::graph::{Edge, EdgeKind, ExactSegment, SegmentGraph};
use oxpinyin_core::scoring::{ScoringError, expand_keys};
use oxpinyin_core::{
    Completeness, Dictionary, LanguageModel, MergedGram, OptionBits, PhraseEntry, PhraseText,
    PhraseTextConcat, PhraseToken, SyllableKey, UserModel,
};

use crate::candidate::{Candidate, CandidateKind, CandidateList};
use crate::error::EngineError;

/// Longest key sequence the window scan searches: the pin's phrase-length
/// cap. Paths beyond it are not searched.
pub const MAX_PHRASE_LENGTH: usize = 16;

/// The window scan's expansion bound. Measured over the W2 corpus, the largest
/// expansion that hits real phrases is a three-initial `q|q|q` span
/// (14^3 = `2_744` — the pin's `qqq…` offers `请求权`); `4_096` covers it with
/// headroom. Larger products yield nothing: no stored phrase matches a
/// longer all-initial span.
pub const SCAN_EXPANSION_LIMIT: usize = 4_096;

/// What choosing a candidate left behind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Selection {
    /// The retained span overruns the current constraint array. No selection
    /// was recorded; the C facade supplies its own refusal cursor.
    Refused,
    /// Input remains; more candidates are offered.
    Continued,
    /// The whole composition is chosen and can be committed.
    Completed,
}

/// What a session is opened with. Other option bits arrive later through
/// [`Session::set_options`] from the C ABI's raw option word.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SessionSettings {
    /// Whether initial-only keys are admitted (`PINYIN_INCOMPLETE`).
    /// On by default.
    pub incomplete_pinyin: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            incomplete_pinyin: true,
        }
    }
}

/// Settings a session reads once, at construction.
#[derive(Clone, Copy, Debug)]
struct Settings {
    options: OptionBits,
}

impl Settings {
    fn read(settings: SessionSettings) -> Self {
        // A session opens under `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
        // USE_RESPLIT_TABLE` unless told otherwise: the table bits are
        // always on, and incomplete pinyin follows the settings.
        let incomplete = settings.incomplete_pinyin;
        let options = OptionBits::default()
            .with(oxpinyin_core::PINYIN_INCOMPLETE, incomplete)
            .with(oxpinyin_core::USE_DIVIDED_TABLE, true)
            .with(oxpinyin_core::USE_RESPLIT_TABLE, true);
        Self { options }
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
#[derive(Clone, Debug)]
pub struct Session<D, L, S = SessionState> {
    // Backends and configuration. Read at construction (and, for the
    // options, remasked live); never composition state.
    dictionary: D,
    model: L,
    state: S,
}

/// Composition and query bookkeeping, without dictionary or model ownership.
/// A facade retains this state and borrows its storage anew for each operation.
#[derive(Clone, Debug)]
pub struct SessionState {
    settings: Settings,
    /// Whether the prepended sentence rows collapse onto the 1-best row —
    /// libzhuyin's display law, set per surface via
    /// [`Session::set_collapse_sentence_rows_to_best`]; off (the pinyin
    /// surface's one-row-per-sentence law) by default. Surface config, set
    /// once and surviving every reset — not part of [`Self::sentence`].
    collapse_sentence_rows_to_best: bool,
    /// Which comparator orders the candidate lists — the pinyin facade's sort
    /// word by default, libzhuyin's fixed comparator when a zhuyin facade
    /// sets it ([`Session::set_candidate_ranking`]). Surface config, like
    /// [`Self::collapse_sentence_rows_to_best`].
    candidate_ranking: CandidateRanking,
    /// The n-best trellis's `<nstore, nbest>` — libpinyin's `<2, 3>` by
    /// default, libzhuyin's `<1, 1>` when a zhuyin facade sets it
    /// ([`Session::set_nbest_shape`]). Surface config, like
    /// [`Self::collapse_sentence_rows_to_best`].
    nbest_shape: crate::nbest::NbestShape,
    /// Whether the parse and sentence-guess paths leave the candidate list
    /// to its next reader instead of building it — the C facades' mode,
    /// set via [`Session::set_defer_list_builds`]; off (the session API's
    /// eager list) by default. Upstream builds candidates only in
    /// `pinyin_guess_candidates` (`pinyin.cpp:2184-2308` at 074a2219), whose
    /// C-ABI port re-anchors its own window. Surface config, like
    /// [`Self::collapse_sentence_rows_to_best`].
    defer_list_builds: bool,

    // Composition state, decomposed into types that own their invariants.
    // Each groups the fields that move together, so an operation cannot
    // reach state that is not meaningful for it — the input buffer without
    // the cursor, the scan scratch without the selection record.
    /// The raw input and its pre-parsed scheme chain — the exact-mode
    /// invariants. The input has no length limit: the pin's 32767-byte
    /// `gint16` parse bound is a language artefact
    /// (`docs/findings/1501-gint16-input-cap.md`).
    input: InputBuffer,
    /// What the user has chosen so far: text, cursor, token history, and
    /// the commit-branch flag, kept mutually consistent.
    record: SelectionRecord,
    /// The §3 constraint store — one cell per raw-buffer byte position,
    /// the coordinate space the scan matrix and the choose cursor share.
    /// Survives `reset_composition` (the parse path) exactly as
    /// upstream's instance-level `m_constraints` survive
    /// `pinyin_parse_more_full_pinyins`; cleared only by the full
    /// [`Session::reset`] (`pinyin_reset`'s rule).
    constraints: crate::constraint::ConstraintStore,
    /// The candidate list and the parse length behind it — the output of
    /// one [`Session::refresh`].
    lookup: Lookup,
    /// The last sentence lookup's decoded rows and what a chosen row needs
    /// — the `m_nbest_results` surface.
    sentence: SentenceState,
    /// The pin's prefix tokens, excluding the implicit sentence_start.
    /// Parsing preserves them. Sentence and prediction guesses replace them,
    /// and a full reset clears them.
    candidate_prefixes: Vec<PhraseToken>,
    /// Reused scan buffers, threaded through one window scan and handed
    /// straight back — allocation reuse, never observable state.
    scratch: Scratch,
    /// Whether the scan in progress (or the last one) met a token of a
    /// loaded library whose item cannot be read; set by the lookups of that
    /// scan alone and read back into the [`CandidateList`] it built.
    scan_unreadable: bool,
    /// Whether the scan in progress (or the last one) searched a phrase row
    /// the pin's `_compute_frequency_of_items` scores against the facade
    /// total, before any prepend or dedup; read back into the
    /// [`CandidateList`] it built.
    scan_ranked_phrase: bool,
}

impl<D, L, S: std::borrow::Borrow<SessionState>> std::ops::Deref for Session<D, L, S> {
    type Target = SessionState;

    fn deref(&self) -> &Self::Target {
        self.state.borrow()
    }
}

impl<D, L, S: std::borrow::BorrowMut<SessionState>> std::ops::DerefMut for Session<D, L, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.state.borrow_mut()
    }
}

impl SessionState {
    /// Borrow the composition and storage without allowing composition changes.
    pub fn read<D, L>(&self, dictionary: D, model: L) -> Session<D, L, &Self> {
        Session {
            dictionary,
            model,
            state: self,
        }
    }

    /// Borrow this composition for an operation over the supplied readers.
    pub fn bind<D, L>(&mut self, dictionary: D, model: L) -> Session<D, L, &mut Self> {
        Session {
            dictionary,
            model,
            state: self,
        }
    }

    /// Takes back a window list from [`Session::candidates_at`] or
    /// [`Session::candidates_ending_at`] that its holder has replaced, so
    /// the next window is built in its storage. Its rows are dropped here;
    /// only the capacity is kept.
    pub fn recycle_candidates(&mut self, list: CandidateList) {
        let mut items = list.into_items();
        items.clear();
        self.scratch.window_items = items;
    }
}

impl<D, L> Session<D, L> {
    /// Retain only composition state after an operation's readers are dropped.
    #[must_use]
    pub fn into_state(self) -> SessionState {
        self.state
    }
}

/// The candidate list and the parse length behind it — the output of one
/// [`Session::refresh`], anchored at the composition offset.
#[derive(Clone, Debug)]
struct Lookup {
    /// The current candidates, in rank order; sentence rows are prepended
    /// once [`Session::guess_sentence`] has run for the composition.
    candidates: CandidateList,
    /// Filtered parse length of the remaining input from the last refresh
    /// — the last byte of [`SegmentGraph::fewest_keys`] under the session's
    /// `incomplete-pinyin` setting, not the unfiltered
    /// [`SegmentGraph::consumed`].
    parsed_prefix: usize,
    /// Whether a deferred parse or sentence guess changed the input behind
    /// [`Self::candidates`] and [`Self::parsed_prefix`] without rebuilding
    /// them; the next [`Session::refresh_candidates`] does.
    stale: bool,
    /// The last sort-option word applied — upstream's `m_sort_option`
    /// (`pinyin.cpp:2203`), which `pinyin_guess_candidates` re-stores on
    /// every call and `pinyin_choose_candidate` later reads. `0x1e` (all
    /// three sort keys, longer candidates suppressed) is the word the
    /// pre-§9 surface always behaved as.
    sort_word: u32,
}

impl Lookup {
    /// The pre-§9 parity word: all three sort keys, longer candidates
    /// suppressed — what every surface behaved as before the sort word
    /// became an input, so a session that never stores one keeps the
    /// recorded behavior.
    const PARITY_SORT_WORD: u32 = 0x1e;

    /// Empties the candidate list and the cached parse length — the
    /// parse-path reset (`reset_composition`). The sort word survives:
    /// upstream's `m_sort_option` is instance state that lives until the
    /// instance is freed (`pinyin_reset` does not clear it), and the next
    /// guess re-stores its own word anyway.
    fn reset(&mut self) {
        self.candidates = CandidateList::default();
        self.parsed_prefix = 0;
        self.stale = false;
    }
}

impl Default for Lookup {
    fn default() -> Self {
        Self {
            candidates: CandidateList::default(),
            parsed_prefix: 0,
            stale: false,
            sort_word: Self::PARITY_SORT_WORD,
        }
    }
}

/// The window scan's reusable buffers. Taken out of the session for the
/// duration of one scan so the scan can borrow `&self.input` while it
/// fills them, then handed straight back. Always cleared before use, so
/// their contents never carry state between scans.
///
/// The default facade's rows go straight into the caller's output
/// vector — each window is ordered in place over the tail it appended
/// ([`order_window_tail`]) — so the only candidate buffer here is the
/// addon batch, empty whenever no addon library is loaded.
#[derive(Clone, Debug, Default)]
struct Scratch {
    /// Dictionary-hit buffer for one window-scan lookup.
    entries: Vec<PhraseEntry>,
    /// One scan path (phrase length ≤ 16).
    path: SmallVec<[SyllableKey; 16]>,
    /// Per-window scan batch, addon facade.
    window_addon: Vec<Candidate>,
    /// Candidate frequencies of the rows being ranked, one per row.
    frequencies: Vec<u32>,
    /// The rows' rank keys: sorted with them, then read by the text dedup.
    rank_keys: Vec<(RankKey, u32)>,
    /// A window list's storage the facade handed back
    /// ([`SessionState::recycle_candidates`]); the next window builds in it.
    window_items: Vec<Candidate>,
}

// The composition state is decomposed into types that own their own
// invariants; each lives in its own module and the session composes them.
mod buffer;
pub(crate) mod matrix;
mod record;
mod sentence;

use buffer::{InputBuffer, SelectedPath};
use record::SelectionRecord;
use sentence::SentenceState;

// The `impl Session` is split by concern into the child modules below;
// each holds one `impl<D, L> Session<D, L>` block over the session's
// private fields (children see the parent's private items) and nothing
// else. The public surface is unchanged: every method keeps its path
// `Session::name`. Shared constants, free functions, the state types
// above and the scan-matrix helpers stay here.
mod guess;
mod lookup;
mod offsets;
mod selection;
pub use selection::TrainingPlan;
pub(crate) use selection::{column_is_empty, training_readings};
mod state;

#[cfg(test)]
mod tests;

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
        let live_unigram = entry.live_unigram();
        into.push(
            Candidate::new(entry.into_text(), kind, keys, end, 0, Some(token), None)
                .with_live_unigram(live_unigram),
        );
    }
}

/// Flushes one window's facade batch in the pin's array order.
///
/// The oracle's `search_matrix` walks the window's key-paths depth-first
/// (`phonetic_key_matrix.cpp:350-408` at 074a2219) and each path's table
/// hit files its matching records into per-library range arrays in the
/// order the index chunk stores them (`ChewingTableEntry::convert`,
/// `chewing_large_table2.h:91-133`); `_append_items` then lays the
/// window down library by library, each library's ranges in that
/// encounter order (`pinyin.cpp:1769-1791`), and the stable
/// `g_array_sort_with_data` keeps exactly that order for candidates whose
/// three keys tie — the amplified-frequency collapses of
/// `docs/findings/candidate-construction.md` §8.4 Class A. A chunk is ordered by the
/// stored key (`phrase_exact_less_than2`: initials, then middles and
/// finals, then tones) and only then by token (`add_index`,
/// `chewing_large_table2.h:208-230`), so a lookup that matches several
/// stored keys — an incomplete syllable, or a tone-less query over a
/// toned index — is **not** token-ascending across the block. This flush
/// used to sort the batch by token, which reordered every such tie
/// (issue #536: `n` under ibus listed 竜/黏 the other way round from the
/// pin). The scan appends each path's records in chunk order already, so
/// the pin's order is a stable sort by library nibble over the batch,
/// keeping the first occurrence of a token reached through several
/// key-paths — the duplicate the pin's string dedup drops later.
fn flush_window_batch(batch: &mut Vec<Candidate>, into: &mut Vec<Candidate>) {
    let start = into.len();
    into.append(batch);
    order_window_tail(into, start);
}

/// Puts one window's rows, appended at `into[start..]`, into the pin's
/// array order in place: tokens ascending by library (a stable sort of
/// the tail), then the first row of each token kept — the same
/// membership rule as [`flush_window_batch`], without a batch buffer
/// between the lookups and the output.
fn order_window_tail(into: &mut Vec<Candidate>, start: usize) {
    // Strict token order implies library order and uniqueness. No row can
    // move or be removed by the fallback, so this O(n), O(1)-space check
    // avoids its sort and membership-set allocation on ordered windows.
    if window_tokens_are_strictly_increasing(&into[start..]) {
        return;
    }
    sort_dedup_window_tail(into, start);
}

fn window_tokens_are_strictly_increasing(window: &[Candidate]) -> bool {
    let mut previous = None;
    for candidate in window {
        let Some(token) = candidate.token() else {
            return false;
        };
        if previous.is_some_and(|previous| previous >= token) {
            return false;
        }
        previous = Some(token);
    }
    true
}

fn sort_dedup_window_tail(into: &mut Vec<Candidate>, start: usize) {
    into[start..].sort_by_key(|candidate| {
        candidate
            .token()
            .map_or(u32::MAX, |token| token.value() >> 24)
    });
    // Membership only: the retained candidates keep the batch's order, so
    // the set's own (unspecified) iteration order is never observed.
    let mut seen: std::collections::HashSet<u32> =
        std::collections::HashSet::with_capacity(into.len() - start);
    let mut write = start;
    for read in start..into.len() {
        if let Some(token) = into[read].token().map(oxpinyin_core::PhraseToken::value)
            && !seen.insert(token)
        {
            continue;
        }
        into.swap(write, read);
        write += 1;
    }
    into.truncate(write);
}

/// λ as the pin parses it out of `table.conf` (`fscanf "%f"`,
/// `table_info.cpp:220,242`) — the same `f32` bits `oxpinyin_data`'s
/// `PINNED_LAMBDA` names. Duplicated here because the engine depends on
/// the core traits, not the data crate.
#[cfg(test)]
const PIN_LAMBDA_F32: f32 = 0.312_699;

/// The pin's candidate `m_freq` under the default profile: the unigram
/// possibility `(1−λ)·unigram/total` computed and amplified by 2²⁴ in C
/// `float` arithmetic, then truncated into the `guint32` field
/// (`pinyin.cpp:1862-1866`; `DYNAMIC_ADJUST` clear ⇒ bigram term zero).
///
/// The truncation is load-bearing, not a rounding detail: it collapses
/// near-ties into equal comparator keys — the Class A tie class — which
/// the stable sort then resolves by collection order. Evaluation order
/// mirrors the C expression left-to-right (`f32` throughout, the three
/// `* 256` factors kept as written); any `f64` intermediate or a
/// pre-combined `* 2²⁴` risks drifting off the tie boundary.
///
/// The predicted-candidate path (`pinyin.cpp:1811-1824`) stores the same
/// expression, so `oxpinyin-capi` calls this function for its rows.
#[must_use]
pub fn amplified_frequency(unigram: u64, total: u64, lambda: f32) -> u32 {
    amplified_frequency_with_bigram(unigram, total, 0.0, lambda)
}

/// The pin's `BIGRAM_FREQUENCY_DISCOUNT` (`pinyin.cpp:33`).
const BIGRAM_FREQUENCY_DISCOUNT_F32: f32 = 0.1;

/// The pin's amplification of a `[0, 1]` possibility into the `guint32`
/// candidate frequency: `* 256 * 256 * 256` (`pinyin.cpp:1821`), i.e. 2²⁴,
/// written as the same three-factor product so the `float` rounding
/// sequence is the pin's.
const AMPLIFY_SCALE_F32: f32 = 256.0 * 256.0 * 256.0;

/// [`amplified_frequency`] with the `DYNAMIC_ADJUST` bigram term folded in,
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
///
/// A zero total is the pin's early return (compatibility policy row 61).
/// The final store is Rust's `as u32`: in range it truncates like the
/// pin's; a negative, NaN or over-range score, which C leaves undefined
/// and the pin's x86-64 and AArch64 builds already store differently,
/// saturates (class (b), `docs/findings/upstream-divergences.md`).
#[must_use]
pub fn amplified_frequency_with_bigram(
    unigram: u64,
    total: u64,
    bigram_poss: f32,
    lambda: f32,
) -> u32 {
    if total == 0 {
        return 0;
    }
    let possibility = lambda * bigram_poss * BIGRAM_FREQUENCY_DISCOUNT_F32
        + (1.0_f32 - lambda) * unigram as f32 / total as f32;
    (possibility * AMPLIFY_SCALE_F32) as u32
}

include!("split_tables.rs");

/// Scratch the window scan threads through the recursive walk.
/// The window scan's borrowed scratch: the session-owned buffers one scan
/// reuses, grouped so the scan takes a single argument.
struct ScanScratch<'a> {
    /// Set when a lookup of this scan met a token of a loaded library whose
    /// item cannot be read.
    unreadable: &'a mut bool,
    path: &'a mut SmallVec<[SyllableKey; 16]>,
    entries: &'a mut Vec<PhraseEntry>,
    window_addon: &'a mut Vec<Candidate>,
    /// An explicit guess: the scan is the pin's `pinyin_guess_candidates`
    /// search, so its table searches assert where the pin's do
    /// ([`EngineError::TonedIncompleteKey`]); an eager refresh, which the
    /// pin never runs, does not.
    explicit: bool,
}

struct ScanBuf<'a> {
    span_base: usize,
    path: &'a mut SmallVec<[SyllableKey; 16]>,
    /// The typed tone of each key on `path`, in step with it.
    tones: SmallVec<[u8; 16]>,
    system: &'a mut Vec<Candidate>,
    addon: &'a mut Vec<Candidate>,
    continued: &'a mut bool,
    /// As [`ScanScratch::unreadable`].
    unreadable: &'a mut bool,
    entries: &'a mut Vec<PhraseEntry>,
    /// As [`ScanScratch::explicit`].
    explicit: bool,
}

/// One key of the scan matrix at its byte position, with the byte position
/// it ends at and where its own text starts — the two differ from
/// `from + len` exactly when the key rides over an apostrophe separator.
#[derive(Clone, Copy, Debug)]
pub struct ScanKey {
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
    const fn from_edge(edge: &Edge) -> Self {
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
pub fn build_scan_matrix(
    graph: &SegmentGraph,
    options: OptionBits,
    divided: bool,
) -> Vec<Vec<ScanKey>> {
    let selected_edges = graph.fewest_keys(options.has_incomplete());
    build_scan_matrix_over(graph, &selected_edges, options, divided)
}

/// [`build_scan_matrix`] over the selected parse the caller already holds:
/// `selected_edges` must be `graph.fewest_keys(options.has_incomplete())`.
pub(crate) fn build_scan_matrix_over(
    graph: &SegmentGraph,
    selected_edges: &[Edge],
    options: OptionBits,
    divided: bool,
) -> Vec<Vec<ScanKey>> {
    let bound = graph.consumed();
    let mut columns: Vec<Vec<ScanKey>> = vec![Vec::new(); bound + 1];

    // 1. The selected parse.
    for scan_key in selected_edges.iter().map(ScanKey::from_edge) {
        columns[scan_key.from].push(scan_key);
    }

    // The divided/resplit alternates are a full-pinyin-parse artifact:
    // upstream adds them after its full-pinyin parser's matrix fill, so
    // keys that arrive pre-parsed (the scheme seam — zhuyin, double
    // pinyin — exact keys) never gain them. The oracle's candidate list
    // for ㄅㄧㄝ is the bie rows alone, with no bi+e divided pair.
    // Both exact-key paths still run fill then fuzzy: double pinyin at
    // pinyin.cpp:1557-1559 and chewing at :1602-1604 (pin 074a2219).

    extend_matrix(&mut columns, options, divided);

    columns
}

/// Steps 2 to 4 of [`build_scan_matrix`] over a matrix whose selected keys
/// are already placed: the resplit and divided passes when `divided`, then
/// the fuzzy additions, then the identical-path collapse.
fn extend_matrix(columns: &mut [Vec<ScanKey>], options: OptionBits, divided: bool) {
    // 2. Resplit every key in each column, including right halves that an
    // earlier column appended during this pass. A pair only resplits when
    // the two keys share a boundary with no apostrophe between them: the
    // pin fills a zero key at a separator, so its pairs never span one.
    // A toned key never resplits: upstream matches the full ChewingKey
    // (tone included) against zero-tone table structs.
    //
    // Gated on `USE_RESPLIT_TABLE` — upstream's `resplit_step` checks
    // `options & USE_RESPLIT_TABLE` (`phonetic_key_matrix.cpp:89`) and
    // returns false without it. At `0x0` neither table bit is set.
    if divided && options.has_resplit_table() {
        resplit_step(columns);
    }

    // 3. Divided syllables over every key collected so far. The split parts
    // are measured from the syllable text itself, so a key that rides over
    // an apostrophe still divides (`bu'tian` offers `补体` from the divided
    // `ti`, whose span covers the apostrophe plus `t` + `i`). A toned key
    // never divides: the divided table's structs are zero-tone and upstream
    // matches the full ChewingKey.
    //
    // Gated on `USE_DIVIDED_TABLE` — upstream's `inner_split_step` checks
    // `options & USE_DIVIDED_TABLE` (`phonetic_key_matrix.cpp:171`) and
    // returns false without it. At `0x0` the bit is clear, so `xian`'s
    // divided pairs (`xi` + `an`) are not in the inventory.
    if divided && options.has_divided_table() {
        inner_split_step(columns);
    }

    // 4. `fuzzy_syllable_step`. Upstream `PhoneticTable::append` is a bag
    // push (`phonetic_key_matrix.h:92-99`); `ChewingKeyRest` is the span
    // (`chewing_key.h:97-104`). Same key, different `m_raw_end`, coexist.
    // After fuzzy, keep `(key, to)` so those edges survive; key-only
    // collapse here is #103. The tone rides the alternate — upstream
    // copies the whole key before swapping the initial or final
    // (`phonetic_key_matrix.cpp:250-259`).
    for addition in &fuzzy_additions(columns, options) {
        columns[addition.from].push(*addition);
    }
    keep_first_in_column(columns);
}

/// The pin's parse-time matrix for keys a parser placed outside the
/// session's own graph (the index-parsed full pinyin, double pinyin and
/// chewing seams): each key sits at its syllable start, then the same
/// steps the pin runs after `fill_matrix` for that entry point.
///
/// `divided` selects the full-pinyin entry point's resplit and divided
/// passes (`pinyin.cpp:1514-1520`); double pinyin and chewing run
/// `fill_matrix` and `fuzzy_syllable_step` only (`:1557-1559`,
/// `:1602-1604`). Every alternative keeps its source key's tone, column and
/// end. The result is in column order; no keys leave the matrix empty.
#[must_use]
pub fn parse_time_keys(
    keys: &[crate::cursor::MatrixKey],
    parsed_len: usize,
    options: OptionBits,
    divided: bool,
) -> Vec<crate::cursor::MatrixKey> {
    if keys.is_empty() {
        return Vec::new();
    }
    let mut columns = vec![Vec::new(); parsed_len + 1];
    for &key in keys {
        if let Some(column) = columns.get_mut(key.syllable_start()) {
            column.push(key);
        }
    }
    if divided && options.has_resplit_table() {
        for index in 0..parsed_len {
            for slot in 0..columns[index].len() {
                let key = columns[index][slot];
                let Some(next_len) = columns.get(key.end()).map(Vec::len) else {
                    continue;
                };
                for next_slot in 0..next_len {
                    let next = columns[key.end()][next_slot];
                    if key.tone() != 0 || next.tone() != 0 || next.syllable_start() != key.end() {
                        continue;
                    }
                    if let Some((_, _, left, right)) = RESPLIT_TABLE.iter().find(|(a, b, _, _)| {
                        oxpinyin_core::ChewingKey::from_pinyin(a) == Some(key.key())
                            && oxpinyin_core::ChewingKey::from_pinyin(b) == Some(next.key())
                    }) {
                        append_matrix_split(&mut columns, key, next.end(), left, right);
                    }
                }
            }
        }
    }
    if divided && options.has_divided_table() {
        for index in 0..columns.len() {
            for slot in 0..columns[index].len() {
                let key = columns[index][slot];
                if key.tone() != 0 {
                    continue;
                }
                if let Some((_, left, right)) = DIVIDED_TABLE
                    .iter()
                    .find(|(a, _, _)| oxpinyin_core::ChewingKey::from_pinyin(a) == Some(key.key()))
                {
                    append_matrix_split(&mut columns, key, key.end(), left, right);
                }
            }
        }
    }
    for column in &mut columns {
        for slot in 0..column.len() {
            let key = column[slot];
            for alternate in oxpinyin_core::fuzzy_chewing_alternatives(key.key(), options) {
                column.push(crate::cursor::MatrixKey::from_chewing(
                    alternate,
                    key.syllable_start(),
                    key.end(),
                ));
            }
        }
        let mut unique = Vec::new();
        for &key in column.iter() {
            if !unique.contains(&key) {
                unique.push(key);
            }
        }
        *column = unique;
    }
    columns.into_iter().flatten().collect()
}

fn append_matrix_split(
    columns: &mut [Vec<crate::cursor::MatrixKey>],
    key: crate::cursor::MatrixKey,
    end: usize,
    left: &str,
    right: &str,
) {
    let (Some(left_key), Some(right_key)) = (
        oxpinyin_core::ChewingKey::from_pinyin(left),
        oxpinyin_core::ChewingKey::from_pinyin(right),
    ) else {
        return;
    };
    let split = key.syllable_start() + left.len();
    if split >= columns.len() {
        return;
    }
    columns[key.syllable_start()].push(crate::cursor::MatrixKey::from_chewing(
        left_key,
        key.syllable_start(),
        split,
    ));
    columns[split].push(crate::cursor::MatrixKey::from_chewing(
        right_key, split, end,
    ));
}

/// The pin's forward resplit pass (074a2219,
/// `phonetic_key_matrix.cpp:87-167`). Snapshot the current column when
/// reached, and the end column for each key. Appending to the current
/// column does not extend that snapshot; right halves appended to later
/// columns are visited when those columns are reached in this same pass.
fn resplit_step(columns: &mut [Vec<ScanKey>]) {
    for index in 0..columns.len().saturating_sub(1) {
        for slot in 0..columns[index].len() {
            let key = columns[index][slot];
            let Some(next_len) = columns.get(key.to).map(Vec::len) else {
                continue;
            };
            // Resolve each key's text once, not once per table row.
            let key_text = key.key.text();
            for next_slot in 0..next_len {
                let next_key = columns[key.to][next_slot];
                if next_key.crosses_separator || key.tone != 0 || next_key.tone != 0 {
                    continue;
                }
                let next_text = next_key.key.text();
                let Some((_, _, left, right)) = RESPLIT_TABLE
                    .iter()
                    .find(|(first, second, _, _)| *first == key_text && *second == next_text)
                else {
                    continue;
                };
                append_split(columns, key, next_key.to, left, right);
            }
        }
    }
}

/// The divided pass follows resplit over the updated matrix, taking one
/// column snapshot at a time (`phonetic_key_matrix.cpp:169-228`).
fn inner_split_step(columns: &mut [Vec<ScanKey>]) {
    for index in 0..columns.len() {
        for slot in 0..columns[index].len() {
            let key = columns[index][slot];
            if key.tone != 0 {
                continue;
            }
            let text = key.key.text();
            let Some((_, left, right)) = DIVIDED_TABLE
                .iter()
                .find(|(syllable, _, _)| *syllable == text)
            else {
                continue;
            };
            append_split(columns, key, key.to, left, right);
        }
    }
}

fn append_split(columns: &mut [Vec<ScanKey>], key: ScanKey, end: usize, left: &str, right: &str) {
    let (Some(left_key), Some(right_key)) =
        (SyllableKey::from_text(left), SyllableKey::from_text(right))
    else {
        return;
    };
    let split = key.syllable_start + left.len();
    if split >= columns.len() {
        return;
    }
    columns[key.from].push(ScanKey {
        key: left_key,
        to: split,
        ..key
    });
    columns[split].push(ScanKey {
        key: right_key,
        from: split,
        to: end,
        syllable_start: split,
        crosses_separator: false,
        tone: 0,
    });
}

/// Phase 4 of [`build_scan_matrix`]: the fuzzy alternates of every key in
/// the matrix, each riding its source span with the swapped tone.
fn fuzzy_additions(columns: &[Vec<ScanKey>], options: OptionBits) -> Vec<ScanKey> {
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
    additions
}

/// Collapse only identical key/span/tone paths after all mutation passes.
/// The pin appends a bag: the same key with a different end must survive.
fn keep_first_in_column(columns: &mut [Vec<ScanKey>]) {
    for column in columns {
        let mut kept = 0_usize;
        for index in 0..column.len() {
            let duplicate = column[..kept].iter().any(|earlier| {
                earlier.key == column[index].key
                    && earlier.to == column[index].to
                    && earlier.tone == column[index].tone
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
/// zero-key separator columns.
///
/// This is plain full pinyin's raw buffer, or the original input of an
/// index-parsed scheme (Luoma, secondary zhuyin), whose pinned parse
/// consumes `'` as the same separator.
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
    // Leading separators propagate parser state but do not create zero
    // columns (074a2219 phonetic_key_matrix.cpp:68-78).
    let leading = input.iter().take_while(|byte| **byte == b'\'').count();
    if normalized <= leading && input.get(leading).is_some() {
        return Ok(offset);
    }
    if normalized > 0 && input.get(normalized - 1) == Some(&b'\'') {
        return Err(EngineError::LookupOffsetPastSeparator { offset, normalized });
    }
    Ok(normalized)
}

/// The range half of the lookup-offset law: an offset may at most equal
/// the coordinate buffer's one-past-end position (upstream's reserved
/// matrix slot).
///
/// This is the whole law for parse modes whose compositions hold no
/// zero-key columns (double pinyin, the zhuyin keyboards).
///
/// # Errors
///
/// [`EngineError::LookupOffsetOutOfRange`] when `offset > len` — upstream
/// reads its matrix out of bounds there, so no pinned behaviour exists
/// and the offset is refused.
pub const fn check_lookup_offset_range(len: usize, offset: usize) -> Result<usize, EngineError> {
    if offset > len {
        return Err(EngineError::LookupOffsetOutOfRange { offset, len });
    }
    Ok(offset)
}

/// The bigram possibility the `DYNAMIC_ADJUST` term is built from — the pin's
/// Gate 3 (`pinyin.cpp:1854-1860`).
///
/// Zero on any of the three ways upstream skips the term: the bit is clear,
/// there is no previous token (so no gram was merged), or the merged row's
/// total is zero. Otherwise `bigram_freq / total` from the row merged once
/// at guess time, read in the pin's order (`pinyin.cpp:1848-1851`,
/// `zhuyin.cpp:1250-1253`): `get_total_freq`, then `get_freq`, then the
/// term only when the total is not zero.
///
/// # Errors
///
/// The [`GramFault`](oxpinyin_core::GramFault) where either read asserts or
/// leaves the merged row.
fn dynamic_adjust_bigram_possibility(
    options: OptionBits,
    gram: Option<&MergedGram>,
    token: u32,
) -> Result<f32, oxpinyin_core::GramFault> {
    if !options.has_dynamic_adjust() {
        return Ok(0.0);
    }
    let Some(row) = gram else {
        return Ok(0.0);
    };
    let total = row.total()?;
    let freq = row.get_freq(token)?.unwrap_or(0);
    if total == 0 {
        return Ok(0.0);
    }
    Ok(freq as f32 / total as f32)
}

/// Which comparator a session's candidate lists are sorted with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CandidateRanking {
    /// libpinyin's `compare_item_with_sort_option` (`pinyin.cpp:1678-1709`)
    /// under the stored sort word.
    #[default]
    SortWord,
    /// libzhuyin's `compare_item_with_length_and_frequency`
    /// (`zhuyin.cpp:1129-1144`): phrase length, then frequency, whatever the
    /// sort word.
    LengthThenFrequency,
}

/// Enabled candidate sort fields, plus the unmodified frequency for dedup.
/// The explicit comparator ignores `m_freq`, including when frequency sorting
/// is disabled. Equal keys keep collection order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RankKey {
    /// Unicode scalar count of the candidate text.
    phrase_length: usize,
    /// Bytes of the raw input the candidate covers, as the pin's `guint8`
    /// key holds them: the low 8 bits.
    pinyin_span: u8,
    /// Scaled candidate frequency, zero when frequency sorting is disabled.
    frequency: u32,
    /// The pin's `m_freq` whatever the sort word: the string dedup reads it
    /// when `SORT_BY_FREQUENCY` is clear and `frequency` is zeroed. Not a
    /// sort key; `compare_descending` never reads it. Both are `guint32`
    /// in the pin (`lookup_candidate_t::m_freq`, `pinyin.cpp:105`), so the
    /// key keeps its 24 bytes.
    m_freq: u32,
}

/// `sort_option_t::SORT_BY_PHRASE_LENGTH` (`pinyin.h`).
const SORT_BY_PHRASE_LENGTH: u32 = 0x4;
/// `sort_option_t::SORT_BY_PINYIN_LENGTH` (`pinyin.h`).
const SORT_BY_PINYIN_LENGTH: u32 = 0x8;
/// `sort_option_t::SORT_BY_FREQUENCY` (`pinyin.h`).
const SORT_BY_FREQUENCY: u32 = 0x10;
/// `sort_option_t::SORT_WITHOUT_SENTENCE_CANDIDATE` (`pinyin.h:56`): set →
/// the n-best rows are not prepended (`pinyin.cpp:2295-2296`), so the
/// phrase-string dedup after it never sees them (`:2298-2300`).
const SORT_WITHOUT_SENTENCE_CANDIDATE: u32 = 0x1;
/// `sort_option_t::SORT_WITHOUT_LONGER_CANDIDATE` (`pinyin.h`): set →
/// the LONGER row is suppressed (`pinyin.cpp:2292-2293`).
const SORT_WITHOUT_LONGER_CANDIDATE: u32 = 0x2;

/// Stably rank candidates by enabled phrase length and pinyin span, descending,
/// then guarded wrapping frequency. The pin's stable-sort contract is preserved
/// (`pinyin.cpp:1678-1709,2286`; `zhuyin.cpp:1129-1144,1529,1610`).
///
/// `keys[i]` is `(candidates[i]`'s key, `i)`. Sort compact cached keys, then
/// permute the larger candidates while retaining the keys used by dedup.
fn rank_candidates(candidates: &mut [Candidate], keys: &mut [(RankKey, u32)]) {
    debug_assert_eq!(candidates.len(), keys.len());
    crate::rank_by_frequency(keys, |(key, _)| {
        (key.phrase_length, key.pinyin_span, key.frequency)
    });
    // Apply the sorted order with swaps, following each displaced element
    // to where an earlier swap moved it (the standard library's
    // `sort_by_cached_key` scheme): no second candidate buffer, no
    // per-call slot vector. After the loop `keys[i]` is `candidates[i]`'s
    // key.
    for i in 0..keys.len() {
        let mut index = keys[i].1 as usize;
        while index < i {
            index = keys[index].1 as usize;
        }
        keys[i].1 = index as u32;
        candidates.swap(i, index);
    }
}

impl RankKey {
    /// The pin's three enabled fields, with its own frequency comparison.
    /// List sorting guards frequency cycles before this can reach std.
    #[cfg(test)]
    fn compare_descending(&self, other: &Self) -> std::cmp::Ordering {
        (other.phrase_length, other.pinyin_span)
            .cmp(&(self.phrase_length, self.pinyin_span))
            .then_with(|| crate::candidate::frequency_compare(self.frequency, other.frequency))
    }

    /// The key `compare_item_with_sort_option` sees under `sort_word`
    /// (`pinyin.cpp:1678-1709` at the pin): a field whose bit is clear
    /// compares equal for every candidate — zeroed here — so the sort
    /// orders by the enabled keys only and their ties fall through to the
    /// collection order the stable sort keeps.
    const fn for_sort_word(
        phrase_length: usize,
        pinyin_span: usize,
        frequency: u32,
        sort_word: u32,
    ) -> Self {
        Self {
            phrase_length: if sort_word & SORT_BY_PHRASE_LENGTH != 0 {
                phrase_length
            } else {
                0
            },
            // `guint8 len_lhs = item_lhs->m_end - item_lhs->m_begin;`
            // (`pinyin.cpp:1692-1694` at 074a2219): a span of 256 bytes or
            // more wraps, an upstream defect reproduced as is (#1275).
            pinyin_span: if sort_word & SORT_BY_PINYIN_LENGTH != 0 {
                pinyin_span as u8
            } else {
                0
            },
            frequency: if sort_word & SORT_BY_FREQUENCY != 0 {
                frequency
            } else {
                0
            },
            m_freq: frequency,
        }
    }
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
        let mut seen: HashSet<&PhraseText> = HashSet::with_capacity(candidates.len());
        keep.extend(
            candidates
                .iter()
                .map(|candidate| seen.insert(candidate.text())),
        );
    }
    let mut keep = keep.into_iter();
    candidates.retain(|_| keep.next().unwrap_or(false));
}

/// The pin's phrase-string dedup over the sorted phrase rows
/// (`_remove_duplicated_items_by_phrase_string`, `pinyin.cpp:2128-2138`;
/// `zhuyin.cpp:1406-1416`): of the rows that share a text, the one with the
/// larger `m_freq` survives, and a tie keeps the earlier row.
///
/// The pin sorts its indices by string with a stable sort and walks each run
/// of equal strings left to right, so the survivor is the first maximum of
/// the run in list order. The input includes the LONGER row and every
/// before-cursor slice. The sentence prepend then keeps n-best rows over
/// all phrase rows (`pinyin.cpp:2116-2125`; `zhuyin.cpp:1394-1403`).
///
/// The map holds one index per distinct text, as the keep-first set held one
/// text: time and space stay linear in the rows.
///
/// `keys[i].0` is `ranked[i]`'s key, as [`rank_candidates`] leaves it.
fn dedup_ranked_by_text(ranked: &mut Vec<Candidate>, keys: &[(RankKey, u32)]) {
    debug_assert_eq!(ranked.len(), keys.len());
    let mut keep = vec![true; ranked.len()];
    {
        let mut survivor: HashMap<&PhraseText, usize> = HashMap::with_capacity(ranked.len());
        for (index, ((key, _), candidate)) in keys.iter().zip(ranked.iter()).enumerate() {
            match survivor.entry(candidate.text()) {
                Entry::Vacant(slot) => {
                    slot.insert(index);
                }
                Entry::Occupied(mut slot) => {
                    let held = *slot.get();
                    if key.m_freq > keys[held].0.m_freq {
                        keep[held] = false;
                        slot.insert(index);
                    } else {
                        keep[index] = false;
                    }
                }
            }
        }
    }
    let mut keep = keep.into_iter();
    ranked.retain(|_| keep.next().unwrap_or(false));
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

#[cfg(feature = "tools")]
pub mod tool;
