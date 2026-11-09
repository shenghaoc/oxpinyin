//! Port-vs-pin sentence-surface measurement for the `sentence_surface_parity`
//! test, which prints the report it asserts.
//!
//! Read-only, no oracle FFI: `fixtures/w4/oracle-sentence-surface.txt` holds
//! the pinned oracle's answer for every sampled input. The port's surface is
//! reproduced through the same ibus call order the fixture captured
//! (`guess_sentence` → `guess_candidates` → `get_sentence`) over the pinned
//! libpinyin-data package. `docs/findings/sentence-surface.md` §12 is
//! the write-up; this module is its reproduction and its gate.

use std::path::Path;

use oxpinyin_engine::{Candidate, CandidateKind, Session, SessionSettings};
use oxpinyin_runtime::{Runtime, RuntimeDict, RuntimeLm};

use crate::sentence_fixture::{SENTENCE_FIXTURE, load_fixture, load_residuals};

/// The concrete port session the measurement drives.
pub struct PortSession {
    runtime: Runtime,
    state: oxpinyin_engine::SessionState,
}

/// One operation over the retained composition and the runtime's tables.
pub type PortRead<'a> =
    Session<RuntimeDict<'a>, RuntimeLm<'a>, &'a mut oxpinyin_engine::SessionState>;

impl PortSession {
    /// Open the unchanged data directory and retain only composition.
    ///
    /// # Errors
    /// Returns the existing runtime-open or session-construction error.
    pub fn open(dir: &Path) -> Result<Self, String> {
        let runtime = Runtime::open(dir, None)
            .map_err(|error| format!("cannot open the runtime: {error}"))?;
        let state = runtime.new_session(SessionSettings::default());
        Ok(Self { runtime, state })
    }

    /// Re-borrow the current tables for an operation on retained composition.
    pub fn session(&mut self) -> PortRead<'_> {
        self.state.bind(self.runtime.dict(), self.runtime.lm())
    }
}

/// `max_index` the fixture generator read to (`observe_sentence_surface`'s
/// caller passes 2 — the n-best cap minus one).
const MAX_INDEX: u8 = 2;

/// First-N candidate rows the fixture captured per input.
const ROW_DEPTH: usize = 6;

/// The port's counterpart of one oracle line, observed through the same call
/// order.
struct PortSurface {
    guessed: bool,
    sentences: Vec<String>,
    rows: Vec<String>,
}

fn observe_port(owner: &mut PortSession, input: &str) -> Result<PortSurface, String> {
    let mut session = owner.session();
    session.reset();
    session
        .replace_raw(input)
        .map_err(|error| format!("replace_raw {input:?}: {error:?}"))?;
    let guessed = session
        .guess_sentence()
        .map_err(|error| format!("guess_sentence {input:?}: {error:?}"))?;

    // `proven` mirrors `observe_sentence_surface`: read only the indices a
    // surviving NBEST row proves are in range, clamped to `MAX_INDEX`.
    let proven = session
        .candidates()
        .iter()
        .filter(|cand| cand.kind() == CandidateKind::Sentence)
        .filter_map(Candidate::nbest_row)
        .max()
        .unwrap_or(0)
        .min(MAX_INDEX);
    let sentences: Vec<String> = (0..=proven)
        .map(|index| {
            session
                .sentence_text(index)
                .and_then(|text| text.as_str())
                .map_or_else(|| "-".to_owned(), str::to_owned)
        })
        .collect();

    let rows: Vec<String> = session
        .candidates()
        .iter()
        .take(ROW_DEPTH)
        .map(render_row)
        .collect();

    Ok(PortSurface {
        guessed,
        sentences,
        rows,
    })
}

/// The fixture's `type-letter/nbest/text` row rendering, port side.
fn render_row(cand: &Candidate) -> String {
    let letter = match cand.kind() {
        CandidateKind::Sentence => "n",
        CandidateKind::Phrase => "N",
        CandidateKind::Addon => "a",
        CandidateKind::Fallback => "f",
    };
    let nbest = match cand.kind() {
        CandidateKind::Sentence => cand
            .nbest_row()
            .map_or_else(|| "-".to_owned(), |rank| rank.to_string()),
        CandidateKind::Phrase | CandidateKind::Addon | CandidateKind::Fallback => "-".to_owned(),
    };
    format!("{letter}/{nbest}/{}", cand.text())
}

/// A sorted multiset key, so "same set, different order" is detectable.
fn as_multiset(items: &[String]) -> Vec<String> {
    let mut sorted = items.to_vec();
    sorted.sort();
    sorted
}

/// The distinct decoded sentences, order- and duplicate-insensitive, with the
/// `-` (no-read) marker dropped. Two lists that name the same set of sentences
/// agree here even when a duplicate second path gives them different ranks.
fn distinct_set(items: &[String]) -> Vec<String> {
    let mut set: Vec<String> = items
        .iter()
        .filter(|item| item.as_str() != "-")
        .cloned()
        .collect();
    set.sort();
    set.dedup();
    set
}

/// Rank of the first position at which two sentence lists diverge. A differing
/// element returns its index; if the overlapping entries all match but one list
/// is longer, the shared length is that first missing rank (so a shorter list
/// missing its rank-1 tail is a rank-1 divergence, not rank-2); `None` only
/// when the lists are identical (equal length, every element equal).
fn first_divergent_rank(a: &[String], b: &[String]) -> Option<usize> {
    if let Some(position) = a.iter().zip(b.iter()).position(|(x, y)| x != y) {
        return Some(position);
    }
    (a.len() != b.len()).then(|| a.len().min(b.len()))
}

fn sentence_rows(rows: &[String]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.starts_with("n/"))
        .cloned()
        .collect()
}

fn phrase_rows(rows: &[String]) -> Vec<String> {
    rows.iter()
        .filter(|row| !row.starts_with("n/"))
        .cloned()
        .collect()
}

/// The measured agreement of the port's sentence surface against the pinned
/// oracle fixture, at the three named strictnesses plus the residual
/// breakdown. Every count is over [`Self::comparable`] inputs.
#[derive(Clone, Debug, Default)]
pub struct SentenceTailReport {
    /// Exact residual surfaces in the fixture; every other input must match pin.
    pub declared_residuals: usize,
    /// Complete surfaces that moved from their pin or recorded residual.
    /// Independent of aggregate counts, so a closure cannot cancel a regression.
    pub unexpected_surfaces: Vec<String>,
    /// Inputs with a sentence surface on both sides (junk-leading excluded).
    pub comparable: usize,
    /// `guess_sentence` retval disagreements (expected 0).
    pub guessed_disagree: usize,
    /// 1-best agreement: `get_sentence(0)` equal.
    pub row0_match: usize,
    /// Ordered n-best list agreement: the full `get_sentence` vec equal.
    pub list_ordered_match: usize,
    /// Ordered misses that are one list reordered (same multiset).
    pub list_order_only: usize,
    /// Ordered misses with a differing multiset.
    pub list_set_diff: usize,
    /// Ordered misses whose *distinct* sentences still agree — the
    /// distinct-set minus ordered gap (the duplicate-path ranks).
    pub list_distinct_extra: usize,
    /// Ordered misses first divergent at rank 0 (the 1-best moved).
    pub list_diff_at_row0: usize,
    /// Ordered misses first divergent at rank 1.
    pub list_diff_at_row1: usize,
    /// Ordered misses first divergent at rank 2 (or a length-only tail).
    pub list_diff_at_row2: usize,
    /// First-6 candidate-row agreement (exact, ordered).
    pub rows_match: usize,
    /// Row misses that are the same six rows reordered.
    pub rows_order_only: usize,
    /// Row misses with a differing row multiset.
    pub rows_set_diff: usize,
    /// Row misses where only the `n/*` rows differ; the phrase order agrees.
    pub rows_sentence_only: usize,
    /// Row misses where the phrase slice differs — a windowing shift as the
    /// surviving-NBEST-row count differs, not a candidate reorder.
    pub rows_phrase_window: usize,
    /// Per-input 1-best miss lines, for the binary's dump.
    pub row0_misses: Vec<String>,
    /// Per-input ordered-list diff lines, for the binary's dump.
    pub list_diffs: Vec<String>,
    /// Per-input first-6-row diff lines, for the binary's dump.
    pub rows_diffs: Vec<String>,
}

impl SentenceTailReport {
    /// Distinct-set agreement: order- and duplicate-insensitive.
    #[must_use]
    pub const fn distinct_set_match(&self) -> usize {
        self.list_ordered_match + self.list_distinct_extra
    }
}

/// Measures the port session against the frozen oracle fixture at
/// [`SENTENCE_FIXTURE`] under `root`.
///
/// # Errors
///
/// Returns a message when the fixture is unreadable or a port call fails.
pub fn measure(session: &mut PortSession, root: &Path) -> Result<SentenceTailReport, String> {
    let fixture = load_fixture(&root.join(SENTENCE_FIXTURE))?;
    let residuals = load_residuals(root, &fixture)?;
    let mut report = SentenceTailReport {
        declared_residuals: residuals.len(),
        ..SentenceTailReport::default()
    };

    for line in &fixture {
        // The junk-leading inputs the oracle cannot parse have no sentence
        // surface on either side — excluded, exactly as §5 excludes them.
        if line.sentences.is_empty() {
            continue;
        }
        report.comparable += 1;

        let port = observe_port(session, &line.input)?;
        let expected = residuals.get(&line.input).unwrap_or(line);
        if port.guessed != expected.guessed
            || port.sentences != expected.sentences
            || port.rows != expected.rows
        {
            report.unexpected_surfaces.push(format!(
                "input={:?}\tguessed={}\tsentences={:?}\trows={:?}\texpected={expected:?}",
                line.input, port.guessed, port.sentences, port.rows,
            ));
        }
        if port.guessed != line.guessed {
            report.guessed_disagree += 1;
        }

        // Row 0 — decoded 1-best.
        if port.sentences.first() == line.sentences.first() {
            report.row0_match += 1;
        } else {
            report.row0_misses.push(format!(
                "input={:?}\toracle0={:?}\tport0={:?}",
                line.input,
                line.sentences.first(),
                port.sentences.first(),
            ));
        }

        // Full sentence list.
        if port.sentences == line.sentences {
            report.list_ordered_match += 1;
        } else {
            let order_only = as_multiset(&port.sentences) == as_multiset(&line.sentences);
            if order_only {
                report.list_order_only += 1;
            } else {
                report.list_set_diff += 1;
            }
            let distinct_same = distinct_set(&port.sentences) == distinct_set(&line.sentences);
            if distinct_same {
                report.list_distinct_extra += 1;
            }
            match first_divergent_rank(&port.sentences, &line.sentences) {
                Some(0) => report.list_diff_at_row0 += 1,
                Some(1) => report.list_diff_at_row1 += 1,
                // Rank ≥ 2 (content or a shorter list first missing a rank-2
                // tail); `None` cannot occur here — the lists already differ.
                _ => report.list_diff_at_row2 += 1,
            }
            report.list_diffs.push(format!(
                "input={:?}\t{}{}\tfirst@{:?}\toracle={:?}\tport={:?}",
                line.input,
                if order_only { "order-only" } else { "SET-DIFF" },
                if distinct_same { " DISTINCT-SAME" } else { "" },
                first_divergent_rank(&port.sentences, &line.sentences),
                line.sentences,
                port.sentences,
            ));
        }

        // First-6 candidate rows.
        if port.rows == line.rows {
            report.rows_match += 1;
        } else {
            let order_only = as_multiset(&port.rows) == as_multiset(&line.rows);
            if order_only {
                report.rows_order_only += 1;
            } else {
                report.rows_set_diff += 1;
            }
            let phrase_window = phrase_rows(&port.rows) != phrase_rows(&line.rows);
            if phrase_window {
                report.rows_phrase_window += 1;
            } else if sentence_rows(&port.rows) != sentence_rows(&line.rows) {
                report.rows_sentence_only += 1;
            }
            report.rows_diffs.push(format!(
                "input={:?}\t{}{}\toracle={:?}\tport={:?}",
                line.input,
                if order_only { "order-only" } else { "SET-DIFF" },
                if phrase_window { " phrase-window" } else { "" },
                line.rows,
                port.rows,
            ));
        }
    }

    Ok(report)
}
