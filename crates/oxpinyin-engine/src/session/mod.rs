//! The session state machine.
//!
//! One session per input context. Nothing here is `Send` or `Sync` by
//! requirement, because the TSF, IMK and `ArkTS` models all want a
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
/// The pin's parser keeps its progress in `gint16`s (`parsed_len`,
/// `m_last_step`, `pinyin_parser2.cpp:333`), so no parse consumes more than
/// `i16::MAX` bytes: `ni` repeated 65537 times consumes 32767. The session
/// accepts exactly that much. Typing past it is reported as
/// [`KeyOutcome::Ignored`]: refusing more input is a state, not a failure.
///
/// The fuzz targets keep their own, shorter input length; the cap is not what
/// they exercise.
pub const MAX_INPUT_BYTES: usize = 32_767;

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
pub const MAX_PHRASE_LENGTH: usize = 16;

/// The window scan's own expansion bound, separate from
/// [`oxpinyin_core::scoring::ScoringConfig::expansion_limit`] which the
/// pre-frequency fallback shares. Measured over the W2 corpus, the largest
/// expansion that hits real phrases is a three-initial `q|q|q` span
/// (14^3 = `2_744` — the pin's `qqq…` offers `请求权`); `4_096` covers it with
/// headroom. Larger products yield nothing: no stored phrase matches a
/// longer all-initial span.
pub const SCAN_EXPANSION_LIMIT: usize = 4_096;

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
        // The captured parity profile is `PINYIN_INCOMPLETE |
        // USE_DIVIDED_TABLE | USE_RESPLIT_TABLE`, and every reference
        // consumer (ibus-libpinyin `PYLibPinyin.cc:195-196`) ORs the table
        // bits unconditionally. A source that says nothing gets the parity
        // behaviour. Other option bits arrive through
        // [`Session::set_options`] from the C ABI's raw option word.
        let incomplete = config.get_bool(KEY_INCOMPLETE).unwrap_or(true);
        let options = OptionBits::default()
            .with(oxpinyin_core::PINYIN_INCOMPLETE, incomplete)
            .with(oxpinyin_core::USE_DIVIDED_TABLE, true)
            .with(oxpinyin_core::USE_RESPLIT_TABLE, true);
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
/// "deliberately absent" list — no keysyms, no `GSettings`, no path discovery,
/// no `cfg(target_os)`, no threading or clock contract — is the freeze this
/// type implements, and later findings add cross-references to it
/// (`config-layering.md` for where configuration actually comes from,
/// `session-replay.md` for what consumes the seam). A change here that admits
/// one of those must amend the SPEC's list in the same commit, or the list
/// silently stops describing the code.
#[derive(Clone, Debug)]
pub struct Session<D, L> {
    // Backends and configuration. Read at construction (and, for the
    // options, remasked live); never composition state.
    dictionary: D,
    model: L,
    paths: StoragePaths,
    settings: Settings,
    scoring: ScoringConfig,
    /// Per-key costs for the pre-frequency fallback scorer, and **empty
    /// whenever [`LanguageModel::has_real_unigrams`] holds**.
    ///
    /// The only readers are the two [`Scorer::with_key_costs`] sites, both
    /// on the `else` of a `has_real_unigrams()` gate, so a model carrying
    /// real frequencies can never consult this table — the pinned
    /// construction prices candidates inline instead. Filling it for such a
    /// model would walk the whole frozen key inventory to produce a value
    /// nothing reads, which is what made the first `new_session` cost
    /// 42–57 ms (`docs/findings/perf-backend-matrix-2026-09.md`). Upstream
    /// has no analogue in any configuration: `pinyin_alloc_instance`
    /// allocates instance state and performs no dictionary or model read
    /// (`src/pinyin.cpp:1310-1333` at the pin `0c5e80e1`).
    key_costs: Vec<Cost>,
    /// Whether the prepended sentence rows collapse onto the 1-best row —
    /// libzhuyin's display law, set per surface via
    /// [`Session::set_collapse_sentence_rows_to_best`]; off (the pinyin
    /// surface's one-row-per-sentence law) by default. Surface config, set
    /// once and surviving every reset — not part of [`Self::sentence`].
    collapse_sentence_rows_to_best: bool,
    /// The n-best trellis's `<nstore, nbest>` — libpinyin's `<2, 3>` by
    /// default, libzhuyin's `<1, 1>` when a zhuyin facade sets it
    /// ([`Session::set_nbest_shape`]). Surface config, like
    /// [`Self::collapse_sentence_rows_to_best`].
    nbest_shape: crate::nbest::NbestShape,

    // Composition state, decomposed into types that own their invariants.
    // Each groups the fields that move together, so an operation cannot
    // reach state that is not meaningful for it — the input buffer without
    // the cursor, the scan scratch without the selection record.
    /// The raw input and its pre-parsed scheme chain — the [`MAX_INPUT_BYTES`]
    /// cap and the exact-mode invariants.
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
    /// Reused scan buffers, threaded through one window scan and handed
    /// straight back — allocation reuse, never observable state.
    scratch: Scratch,
    /// Whether the scan in progress (or the last one) met a token of a
    /// loaded library whose item cannot be read; set by the lookups of that
    /// scan alone and read back into the [`CandidateList`] it built.
    scan_unreadable: bool,
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
    }
}

impl Default for Lookup {
    fn default() -> Self {
        Self {
            candidates: CandidateList::default(),
            parsed_prefix: 0,
            sort_word: Self::PARITY_SORT_WORD,
        }
    }
}

/// The window scan's reusable buffers. Taken out of the session for the
/// duration of one scan so the scan can borrow `&self.input` while it
/// fills them, then handed straight back. Always cleared before use, so
/// their contents never carry state between scans.
#[derive(Clone, Debug, Default)]
struct Scratch {
    /// The scan's candidate buffer.
    collected: Vec<Candidate>,
    /// Schwartzian buffer for the three-key order.
    ranked: Vec<(RankKey, Candidate)>,
    /// Dictionary-hit buffer for one window-scan lookup.
    entries: Vec<PhraseEntry>,
    /// One scan path (phrase length ≤ 16).
    path: SmallVec<[SyllableKey; 16]>,
    /// Per-window scan batch, default facade.
    window_phrase: Vec<Candidate>,
    /// Per-window scan batch, addon facade.
    window_addon: Vec<Candidate>,
}

// The composition state is decomposed into types that own their own
// invariants; each lives in its own module and the session composes them.
mod buffer;
pub(crate) mod matrix;
mod record;
mod sentence;

use buffer::InputBuffer;
use record::SelectionRecord;
use sentence::SentenceState;

// The `impl Session` is split by concern into the child modules below;
// each holds one `impl<D, L> Session<D, L>` block over the session's
// private fields (children see the parent's private items) and nothing
// else. The public surface is unchanged: every method keeps its path
// `Session::name`. Shared constants, free functions, the state types
// above and the scan-matrix helpers stay here.
mod guess;
mod input;
mod lookup;
mod offsets;
mod selection;
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
/// The oracle's `search_matrix` walks the window's key-paths depth-first
/// (`phonetic_key_matrix.cpp:350-408` at 074a2219) and each path's table
/// hit files its matching records into per-library range arrays in the
/// order the index chunk stores them (`ChewingTableEntry::convert`,
/// `chewing_large_table2.h:91-133`); `_append_items` then lays the
/// window down library by library, each library's ranges in that
/// encounter order (`pinyin.cpp:1769-1791`), and the stable
/// `g_array_sort_with_data` keeps exactly that order for candidates whose
/// three keys tie — the amplified-frequency collapses of
/// `docs/testing/corpus-tail.md` Class A. A chunk is ordered by the
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
    batch.sort_by_key(|candidate| {
        candidate
            .token()
            .map_or(u32::MAX, |token| token.value() >> 24)
    });
    // Membership only: the retained candidates keep the batch's order, so
    // the set's own (unspecified) iteration order is never observed.
    let mut seen: std::collections::HashSet<u32> =
        std::collections::HashSet::with_capacity(batch.len());
    for candidate in batch.drain(..) {
        if let Some(token) = candidate.token().map(oxpinyin_core::PhraseToken::value)
            && !seen.insert(token)
        {
            continue;
        }
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
fn amplified_frequency_with_bigram(unigram: u64, total: u64, bigram_poss: f32) -> u64 {
    if total == 0 {
        return 0;
    }
    let possibility = PIN_LAMBDA_F32 * bigram_poss * BIGRAM_FREQUENCY_DISCOUNT_F32
        + (1.0_f32 - PIN_LAMBDA_F32) * unigram as f32 / total as f32;
    u64::from((possibility * AMPLIFY_SCALE_F32) as u32)
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
    /// Set when a lookup of this scan met a token of a loaded library whose
    /// item cannot be read.
    unreadable: &'a mut bool,
    path: &'a mut SmallVec<[SyllableKey; 16]>,
    entries: &'a mut Vec<PhraseEntry>,
    window_phrase: &'a mut Vec<Candidate>,
    window_addon: &'a mut Vec<Candidate>,
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
    let bound = graph.consumed();
    let mut columns: Vec<Vec<ScanKey>> = vec![Vec::new(); bound + 1];

    // 1. The selected parse.
    let selected_edges = graph.fewest_keys(options.has_incomplete());
    let selected: Vec<ScanKey> = selected_edges.iter().map(ScanKey::from_edge).collect();
    for scan_key in &selected {
        columns[scan_key.from].push(*scan_key);
    }

    // The divided/resplit alternates are a full-pinyin-parse artifact:
    // upstream adds them after its full-pinyin parser's matrix fill, so
    // keys that arrive pre-parsed (the scheme seam — zhuyin, double
    // pinyin — exact keys) never gain them. The oracle's candidate list
    // for ㄅㄧㄝ is the bie rows alone, with no bi+e divided pair.
    // Both exact-key paths still run fill then fuzzy: double pinyin at
    // pinyin.cpp:1557-1559 and chewing at :1602-1604 (pin 074a2219).

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
        resplit_step(&mut columns);
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
        inner_split_step(&mut columns);
    }

    // 4. `fuzzy_syllable_step`. Upstream `PhoneticTable::append` is a bag
    // push (`phonetic_key_matrix.h:92-99`); `ChewingKeyRest` is the span
    // (`chewing_key.h:97-104`). Same key, different `m_raw_end`, coexist.
    // After fuzzy, keep `(key, to)` so those edges survive; key-only
    // collapse here is #103. The tone rides the alternate — upstream
    // copies the whole key before swapping the initial or final
    // (`phonetic_key_matrix.cpp:250-259`).
    for addition in &fuzzy_additions(&columns, options) {
        columns[addition.from].push(*addition);
    }
    keep_first_in_column(&mut columns);

    columns
}

/// The pin's forward resplit pass (074a2219,
/// `phonetic_key_matrix.cpp:87-167`). Snapshot the current column when
/// reached, and the end column for each key. Appending to the current
/// column does not extend that snapshot; right halves appended to later
/// columns are visited when those columns are reached in this same pass.
fn resplit_step(columns: &mut [Vec<ScanKey>]) {
    for index in 0..columns.len().saturating_sub(1) {
        let keys = columns[index].clone();
        for key in keys {
            let Some(next_keys) = columns.get(key.to).cloned() else {
                continue;
            };
            for next_key in next_keys {
                if next_key.crosses_separator || key.tone != 0 || next_key.tone != 0 {
                    continue;
                }
                let Some((_, _, left, right)) =
                    RESPLIT_TABLE.iter().find(|(first, second, _, _)| {
                        *first == key.key.text() && *second == next_key.key.text()
                    })
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
        let keys = columns[index].clone();
        for key in keys {
            if key.tone != 0 {
                continue;
            }
            let Some((_, left, right)) = DIVIDED_TABLE
                .iter()
                .find(|(syllable, _, _)| *syllable == key.key.text())
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

impl RankKey {
    /// The key `compare_item_with_sort_option` sees under `sort_word`
    /// (`pinyin.cpp:1678-1709` at the pin): a field whose bit is clear
    /// compares equal for every candidate — zeroed here — so the sort
    /// orders by the enabled keys only and their ties fall through to the
    /// collection order the stable sort keeps.
    const fn for_sort_word(
        phrase_length: usize,
        pinyin_span: usize,
        frequency: u64,
        sort_word: u32,
    ) -> Self {
        Self {
            phrase_length: if sort_word & SORT_BY_PHRASE_LENGTH != 0 {
                phrase_length
            } else {
                0
            },
            pinyin_span: if sort_word & SORT_BY_PINYIN_LENGTH != 0 {
                pinyin_span
            } else {
                0
            },
            frequency: if sort_word & SORT_BY_FREQUENCY != 0 {
                frequency
            } else {
                0
            },
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
