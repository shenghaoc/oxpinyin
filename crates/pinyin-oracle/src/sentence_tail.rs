//! Port-vs-pin sentence-surface measurement, shared by the `sentence-tail`
//! binary and the `sentence_surface_parity` integration test so the reported
//! numbers and the asserted numbers cannot drift apart.
//!
//! Read-only, no oracle FFI: `fixtures/w4/oracle-sentence-surface.txt` holds
//! the pinned oracle's answer for every sampled input. The port's surface is
//! reproduced through the same ibus call order the fixture captured
//! (`guess_sentence` → `guess_candidates` → `get_sentence`) over the exported
//! model20 tables. `docs/findings/sentence-surface.md` §12 is the write-up;
//! this module is its reproduction and its gate.

use std::path::{Path, PathBuf};

use oxpinyin_data::{BigramLanguageModel, SystemDictionary};
use oxpinyin_engine::{Candidate, CandidateKind, EmptyConfigSource, Session, StoragePaths};

/// The concrete port session the measurement drives.
pub type PortSession = Session<SystemDictionary, BigramLanguageModel>;

/// Fixture path, relative to the repository root.
pub const SENTENCE_FIXTURE: &str = "fixtures/w4/oracle-sentence-surface.txt";

/// `max_index` the fixture generator read to (`observe_sentence_surface`'s
/// caller passes 2 — the n-best cap minus one).
const MAX_INDEX: u8 = 2;

/// First-N candidate rows the fixture captured per input.
const ROW_DEPTH: usize = 6;

/// Repository root, from this crate's manifest directory.
#[must_use]
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

/// The exported-table directory, from `PINYIN_EXPORT_DIR` or the default, when
/// it holds all three redb tables; `None` otherwise (so callers can skip).
#[must_use]
pub fn export_dir() -> Option<PathBuf> {
    let dir = std::env::var_os("PINYIN_EXPORT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new("/tmp/oxpinyin-export").to_path_buf());
    ["pinyin_index.redb", "phrase_index.redb", "bigram.redb"]
        .iter()
        .all(|name| dir.join(name).exists())
        .then_some(dir)
}

/// Opens the port session over the exported tables with real unigrams, or
/// `Ok(None)` when the exported tables or the model cache are simply absent —
/// the self-skip the whole real-tables tier uses.
///
/// The model directory must be a **complete** extracted model20 (all 18
/// files: the 17 phrase tables plus `interpolation2.text`); `locate_model_dir`
/// rejects a partial directory such as the 4-file `~/.cache/oxpinyin-data`.
/// Point `PINYIN_MODEL_DIR` at a full extract.
///
/// # Errors
///
/// Returns a message when a `PINYIN_MODEL_DIR` is **set but unusable** (a
/// partial or non-directory model dir — surfaced rather than skipped, so a
/// misconfiguration is not mistaken for "no tables present"), or when a
/// located table cannot be opened or parsed.
pub fn open_session_from_env() -> Result<Option<PortSession>, String> {
    let Some(dir) = export_dir() else {
        return Ok(None);
    };
    let model_dir = match crate::model_cache::locate_model_dir() {
        Ok(Some(model_dir)) => model_dir,
        // No model directory configured or discoverable: skip, like absent tables.
        Ok(None) => return Ok(None),
        // PINYIN_MODEL_DIR set but unusable: a misconfiguration, not an absence.
        Err(error) => return Err(format!("model directory lookup failed: {error}")),
    };
    let dict = SystemDictionary::open(
        &dir.join("pinyin_index.redb"),
        &dir.join("phrase_index.redb"),
    )
    .map_err(|error| format!("cannot open SystemDictionary: {error}"))?;
    let mut lm = BigramLanguageModel::open(&dir.join("bigram.redb"))
        .map_err(|error| format!("cannot open BigramLanguageModel: {error}"))?;
    lm.set_unigrams_from_interpolation2(&model_dir.join("interpolation2.text"))
        .map_err(|error| format!("cannot parse interpolation2: {error}"))?;
    let session = Session::new(&EmptyConfigSource, StoragePaths::new("user"), dict, lm)
        .map_err(|error| format!("cannot create Session: {error}"))?;
    Ok(Some(session))
}

/// One frozen oracle line: the ibus surface for one sampled input.
struct OracleLine {
    input: String,
    guessed: bool,
    /// `pinyin_get_sentence(0..=proven)`; `-` marks a `None` read.
    sentences: Vec<String>,
    /// First-6 candidates as `type-letter/nbest/text`.
    rows: Vec<String>,
}

fn load_fixture(path: &Path) -> Result<Vec<OracleLine>, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read fixture {}: {error}", path.display()))?;
    let mut lines = Vec::new();
    for line in raw.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let mut parts = line.split('\t');
        let input = parts.next().unwrap_or("").to_owned();
        let guessed = parts.next().unwrap_or("") == "true";
        let sentences = split_field(parts.next().unwrap_or(""));
        let rows = split_field(parts.next().unwrap_or(""));
        lines.push(OracleLine {
            input,
            guessed,
            sentences,
            rows,
        });
    }
    Ok(lines)
}

/// A `\x01`-joined fixture field; empty string means an empty list.
fn split_field(field: &str) -> Vec<String> {
    if field.is_empty() {
        Vec::new()
    } else {
        field.split('\u{1}').map(str::to_owned).collect()
    }
}

/// The port's counterpart of one oracle line, observed through the same call
/// order.
struct PortSurface {
    guessed: bool,
    sentences: Vec<String>,
    rows: Vec<String>,
}

fn observe_port(session: &mut PortSession, input: &str) -> Result<PortSurface, String> {
    session.reset();
    session
        .type_pinyin(input)
        .map_err(|error| format!("type_pinyin {input:?}: {error:?}"))?;
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
        _ => "?",
    };
    let nbest = match cand.kind() {
        CandidateKind::Sentence => cand
            .nbest_row()
            .map_or_else(|| "-".to_owned(), |rank| rank.to_string()),
        _ => "-".to_owned(),
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
    let mut report = SentenceTailReport::default();

    for line in &fixture {
        // The junk-leading inputs the oracle cannot parse have no sentence
        // surface on either side — excluded, exactly as §5 excludes them.
        if line.sentences.is_empty() {
            continue;
        }
        report.comparable += 1;

        let port = observe_port(session, &line.input)?;
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
