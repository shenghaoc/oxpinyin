//! The raw-corpus acceptance test (`oxpinyin-train`'s authoritative result)
//! and the generate-stage rollover/filter units.
//!
//! `raw_corpus_to_final_model_and_correction_rate` starts from a **raw** Han
//! corpus on disk and drives the whole native workflow through the persistent
//! [`Trainer`] — segment → generate candidates → estimate + sort → merge →
//! prune → convert → estimate λ → apply → correction rate — with no Python,
//! `make`, SQLite, or libpinyin. It verifies every material stage's on-disk
//! product and the final (interpolation model, λ, correction rate), and that a
//! second run resumes to the identical result.
//!
//! The vocabulary pins the decode: `中`(10) and `钟`(20) both spell `zhong`,
//! `国`(30) spells `guo`. `钟` never appears in the corpus, so the trained
//! model gives it no unigram and `zhong` always decodes to `中` — for any λ.
//! So `中国` decodes back to `中国` (pass) and `钟` decodes to `中` (fail):
//! correction rate 1/2 = 0.5.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use oxpinyin_core::{PhraseToken, SyllableKey};
use oxpinyin_kmm::{GenerateParams, KMixtureModel};
use oxpinyin_lambda::count_deleted;
use oxpinyin_segment::{PhraseLexicon, SegmentModel, Segmenter};
use oxpinyin_testsupport::FixtureDictionary;
use oxpinyin_train::{
    CorpusIndex, EvalInputs, PhraseSource, SegmentMethod, SegmentedDoc, Stage, Status, TrainConfig,
    Trainer, TrainerPaths, generate_candidates,
};

// ---- fixture vocabulary ----------------------------------------------------

const VOCAB: &str = "\
token=10\tkeys=zhong\ttext=中\tunigram=1
token=20\tkeys=zhong\ttext=钟\tunigram=1
token=30\tkeys=guo\ttext=国\tunigram=1
";

fn lexicon() -> PhraseLexicon {
    PhraseLexicon::from_pairs(vec![
        (10, "中".to_owned()),
        (20, "钟".to_owned()),
        (30, "国".to_owned()),
    ])
}

/// A minimal ngseg segmenter: single-char phrases segment each Han character
/// to its token deterministically, so the segmented output is exact.
fn segmenter() -> Segmenter {
    let lexicon = lexicon();
    let mut unigrams = HashMap::new();
    unigrams.insert(10, 1);
    unigrams.insert(20, 1);
    unigrams.insert(30, 1);
    let model = SegmentModel::memory(&unigrams, HashMap::new(), &lexicon);
    Segmenter::from_parts(lexicon, model, oxpinyin_segment::PINNED_LAMBDA)
}

/// The eval `PhraseSource`: token → best keys + text (mirrors the vocab).
struct FixtureSource {
    best: BTreeMap<u32, Vec<SyllableKey>>,
    text: BTreeMap<u32, String>,
}

impl FixtureSource {
    fn new() -> Self {
        let key = |k: &str| vec![SyllableKey::from_text(k).expect("key")];
        let mut best = BTreeMap::new();
        best.insert(10, key("zhong"));
        best.insert(20, key("zhong"));
        best.insert(30, key("guo"));
        let mut text = BTreeMap::new();
        text.insert(10, "中".to_owned());
        text.insert(20, "钟".to_owned());
        text.insert(30, "国".to_owned());
        Self { best, text }
    }
}

impl PhraseSource for FixtureSource {
    fn best_keys(&self, token: PhraseToken) -> Option<Vec<SyllableKey>> {
        self.best.get(&token.value()).cloned()
    }
    fn text(&self, token: PhraseToken) -> Option<String> {
        self.text.get(&token.value()).cloned()
    }
    fn lexicon_tokens(&self) -> Vec<PhraseToken> {
        self.text
            .keys()
            .map(|&token| PhraseToken::new(token))
            .collect()
    }
}

// A segmented held-out corpus: 中→国 twice, matching the corpus's contexts so
// λ estimation has scorable held-out bigrams.
const HELD_OUT: &str = "10 中\n30 国\n0 \n10 中\n30 国\n0 \n";

// evals2.text: 中国 (should pass) then 钟 (decodes to 中, fails).
const EVALS: &str = "10 中\n30 国\n0 \n20 钟\n0 \n";

// ---- temp workspace --------------------------------------------------------

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-train-{}-{tag}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

// ---- the acceptance test ---------------------------------------------------

/// Config with test-sized thresholds: no minimum-size filter, and a 1-byte
/// candidate size so every document rolls over into its own candidate — which
/// exercises numbering, gather/sort, and top-N. CDF 0 keeps every pair so the
/// 中→国 bigram survives into the final model.
fn test_config() -> TrainConfig {
    TrainConfig {
        minimum_file_size: 0,
        candidate_model_size: 1,
        merge_number: 2,
        prune_cdf: 0.0,
        ..TrainConfig::default()
    }
}

#[test]
fn raw_corpus_to_final_model_and_correction_rate() {
    let temp = TempDir::new("e2e");
    let text_dir = temp.path.join("texts");
    let model_dir = temp.path.join("models");
    let final_dir = temp.path.join("finals");

    // Raw Han corpus: three documents, each containing 中国.
    write(&text_dir.join("a.text"), "中国中国\n中国\n");
    write(&text_dir.join("b.text"), "中国中\n国中国\n");
    write(&text_dir.join("c.text"), "中国\n中国中国\n");
    let index = CorpusIndex::parse("doc a#/a.text\ndoc b#/b.text\ndoc c#/c.text\n").expect("index");

    // The held-out corpus seeds both the scoring deleted model and the eval
    // deleted counts.
    let mut scoring_deleted = KMixtureModel::new();
    scoring_deleted
        .add_document(HELD_OUT, GenerateParams::default())
        .expect("scoring deleted");
    let deleted_counts = count_deleted(HELD_OUT, true).expect("deleted counts");

    let dictionary = FixtureDictionary::parse(VOCAB).expect("vocab");
    let source = FixtureSource::new();
    let eval = EvalInputs {
        dictionary: &dictionary,
        source: &source,
        evals_text: EVALS,
        deleted: &deleted_counts,
    };

    let paths = TrainerPaths {
        text_dir: text_dir.clone(),
        model_dir: model_dir.clone(),
        final_dir: final_dir.clone(),
    };
    let trainer = Trainer::new(test_config(), paths, SegmentMethod::Ngseg);
    let outcome = trainer
        .run(&segmenter(), &index, &scoring_deleted, &eval, "1")
        .expect("train run");

    // --- segment stage: each raw document produced a signed .segmented file.
    for name in ["a.text", "b.text", "c.text"] {
        let seg = text_dir.join(format!("{name}.segmented"));
        assert!(seg.is_file(), "segmented {name} exists");
        let segmented = std::fs::read_to_string(&seg).expect("read segmented");
        // Single-char lexicon: 中→10, 国→30, and a trailing null token.
        assert!(segmented.contains("10 中\n"), "{name}: {segmented:?}");
        assert!(segmented.contains("30 国\n"), "{name}: {segmented:?}");
        assert!(segmented.trim_end().ends_with("0"), "{name} ends in null");
        let status = Status::load(&Status::path_for(&seg)).expect("seg status");
        assert!(status.is_done(Stage::Segment).expect("done"));
    }

    // --- generate stage: three numbered candidates, each with a signed status.
    for number in 0..3 {
        let candidate = model_dir.join(format!("model-candidates-{number}.db"));
        assert!(candidate.is_file(), "candidate {number} exists");
        let status = Status::load(&Status::path_for(&candidate)).expect("cand status");
        assert!(status.is_done(Stage::Generate).expect("gen done"));
        assert!(status.generate_start.is_some(), "GenerateStart recorded");
        assert!(status.generate_end.is_some(), "GenerateEnd recorded");
        assert!(status.estimate_score.is_some(), "EstimateScore recorded");
    }
    assert_eq!(outcome.candidate_count, 3, "one candidate per document");

    // --- estimate stage: gather + sorted index written, sorted descending.
    let gathered = std::fs::read_to_string(model_dir.join("estimate.index")).expect("gather");
    assert_eq!(gathered.lines().count(), 3, "three gathered candidates");
    let sorted_text =
        std::fs::read_to_string(model_dir.join("estimate.sorted.index")).expect("sorted");
    let scores: Vec<f64> = sorted_text
        .lines()
        .map(|line| line.rsplit('#').next().unwrap().parse().unwrap())
        .collect();
    assert_eq!(scores.len(), 3);
    for pair in scores.windows(2) {
        assert!(pair[0] >= pair[1], "sorted descending: {scores:?}");
    }

    // --- prune stage: the try workspace holds the final model + intermediates.
    let trydir = final_dir.join("try1");
    let interp_path = trydir.join("interpolation2.text");
    assert!(interp_path.is_file(), "final model written");
    let interp = std::fs::read_to_string(&interp_path).expect("interp");
    assert!(
        interp.starts_with("\\data model interpolation\n"),
        "{interp}"
    );
    assert!(interp.contains("\\1-gram\n") && interp.contains("\\2-gram\n"));
    assert!(interp.trim_end().ends_with("\\end"));
    assert!(trydir.join("kmm_merged.text").is_file());
    assert!(trydir.join("kmm_pruned.text").is_file());

    // The final model equals what the run returned.
    assert_eq!(interp, outcome.interpolation2);

    // --- evaluate stage: cwd.status carries the epochs and results.
    let cwd = Status::load(&trydir.join("cwd.status")).expect("cwd status");
    assert!(cwd.is_done(Stage::Prune).expect("prune done"));
    assert!(cwd.is_done(Stage::Evaluate).expect("eval done"));
    assert_eq!(cwd.prune_merge_number, Some(2));
    assert_eq!(cwd.prune_model_size, Some(interp.len() as u64));
    assert_eq!(cwd.evaluate_correction_rate, Some(outcome.correction_rate));

    // --- authoritative result: λ ∈ [0,1], correction rate = 0.5.
    let lambda = outcome.average_lambda.as_f64();
    assert!((0.0..=1.0).contains(&lambda), "λ {lambda} in range");
    assert!(
        (outcome.correction_rate - 0.5).abs() < 1e-12,
        "correction rate {} (中国 passes, 钟 fails)",
        outcome.correction_rate
    );
    assert_eq!(outcome.report.tested, 2);
    assert_eq!(outcome.report.passed, 1);
    assert_eq!(
        outcome.report.mismatches,
        vec![("钟".to_owned(), "中".to_owned())]
    );

    // --- resumability: a second run reuses artifacts and yields the same
    // authoritative result, bit for bit.
    let rerun = trainer
        .run(&segmenter(), &index, &scoring_deleted, &eval, "1")
        .expect("resumed run");
    assert_eq!(rerun.interpolation2, outcome.interpolation2);
    assert_eq!(rerun.correction_rate, outcome.correction_rate);
    assert_eq!(
        rerun.average_lambda.as_f64(),
        outcome.average_lambda.as_f64()
    );
}

// ---- resumability ----------------------------------------------------------

fn write_raw_corpus(temp: &TempDir) {
    let text_dir = temp.path.join("texts");
    write(&text_dir.join("a.text"), "中国中国\n中国\n");
    write(&text_dir.join("b.text"), "中国中\n国中国\n");
    write(&text_dir.join("c.text"), "中国\n中国中国\n");
}

const CORPUS_INDEX: &str = "doc a#/a.text\ndoc b#/b.text\ndoc c#/c.text\n";

/// Full setup + run against the workspace under `temp`, into `try<tryname>`.
/// Everything the run borrows is constructed here, so nothing escapes.
fn run_full(
    temp: &TempDir,
    tryname: &str,
) -> Result<oxpinyin_train::TrainOutcome, oxpinyin_train::TrainError> {
    run_full_with_index(temp, tryname, CORPUS_INDEX)
}

/// [`run_full`] over an explicit corpus index text.
fn run_full_with_index(
    temp: &TempDir,
    tryname: &str,
    index_text: &str,
) -> Result<oxpinyin_train::TrainOutcome, oxpinyin_train::TrainError> {
    let index = CorpusIndex::parse(index_text).expect("index");
    let mut scoring_deleted = KMixtureModel::new();
    scoring_deleted
        .add_document(HELD_OUT, GenerateParams::default())
        .expect("scoring deleted");
    let deleted_counts = count_deleted(HELD_OUT, true).expect("deleted counts");
    let dictionary = FixtureDictionary::parse(VOCAB).expect("vocab");
    let source = FixtureSource::new();
    let eval = EvalInputs {
        dictionary: &dictionary,
        source: &source,
        evals_text: EVALS,
        deleted: &deleted_counts,
    };
    let paths = TrainerPaths {
        text_dir: temp.path.join("texts"),
        model_dir: temp.path.join("models"),
        final_dir: temp.path.join("finals"),
    };
    let trainer = Trainer::new(test_config(), paths, SegmentMethod::Ngseg);
    trainer.run(&segmenter(), &index, &scoring_deleted, &eval, tryname)
}

#[test]
fn resume_skips_segmentation_after_the_raw_corpus_is_corrupted() {
    let temp = TempDir::new("resume-seg");
    write_raw_corpus(&temp);
    let first = run_full(&temp, "1").expect("first run");

    // Corrupt a raw document: were the segment stage to run again, the corpus
    // would change (all 钟, no 中国). The segment stage is epoch-gated, so the
    // committed `.segmented` is reused and the corrupted raw is never
    // re-segmented — the result is identical. This is the resume-after-a-stage
    // guarantee: a completed stage is not redone.
    write(&temp.path.join("texts/a.text"), "钟钟钟\n");
    let second = run_full(&temp, "2").expect("second run");

    assert_eq!(
        second.interpolation2, first.interpolation2,
        "segment reused"
    );
    assert_eq!(second.correction_rate, first.correction_rate);
    // The segmented file still holds the original 中/国 tokens, not 钟.
    let segmented =
        std::fs::read_to_string(temp.path.join("texts/a.text.segmented")).expect("segmented");
    assert!(segmented.contains("10 中\n") && !segmented.contains("20 钟\n"));
}

#[test]
fn a_status_with_a_newer_epoch_is_rejected_not_silently_resumed() {
    let temp = TempDir::new("resume-epoch");
    write_raw_corpus(&temp);
    run_full(&temp, "1").expect("first run");

    // A status file stamped by a *newer* trainer (SegmentEpoch 2 > this build's
    // 1) must not be silently resumed — the orchestrator refuses it.
    write(
        &temp.path.join("texts/a.text.segmented.status"),
        "{\"SegmentEpoch\": 2}",
    );
    let error = run_full(&temp, "2").expect_err("must reject a newer epoch");
    assert!(
        matches!(
            error,
            oxpinyin_train::TrainError::EpochTooNew {
                stage: "Segment",
                found: 2,
                known: 1
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn a_newer_epoch_in_the_index_status_is_rejected() {
    let temp = TempDir::new("resume-epoch-index");
    write_raw_corpus(&temp);
    run_full(&temp, "1").expect("first run");

    // models/corpus.index.status stamped by a newer trainer at Estimate: the
    // generate stage meets it first and must refuse it rather than rewrite it.
    write(
        &temp.path.join("models/corpus.index.status"),
        "{\"GenerateEpoch\": 1, \"EstimateEpoch\": 2}",
    );
    let error = run_full(&temp, "2").expect_err("must reject a newer index epoch");
    assert!(
        matches!(
            error,
            oxpinyin_train::TrainError::EpochTooNew {
                stage: "Estimate",
                found: 2,
                known: 1
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn a_newer_epoch_in_the_final_status_is_rejected() {
    let temp = TempDir::new("resume-epoch-final");
    write_raw_corpus(&temp);
    run_full(&temp, "1").expect("first run");

    // finals/try1/cwd.status stamped by a newer trainer at Evaluate; the same
    // try name is reused so the prune stage meets the file before it would
    // overwrite it.
    write(
        &temp.path.join("finals/try1/cwd.status"),
        "{\"PruneEpoch\": 1, \"EvaluateEpoch\": 2}",
    );
    let error = run_full(&temp, "1").expect_err("must reject a newer final epoch");
    assert!(
        matches!(
            error,
            oxpinyin_train::TrainError::EpochTooNew {
                stage: "Evaluate",
                found: 2,
                known: 1
            }
        ),
        "got {error:?}"
    );
}

#[test]
fn resume_regenerates_when_the_corpus_index_grew() {
    let temp = TempDir::new("resume-grow");
    write_raw_corpus(&temp);
    let first = run_full(&temp, "1").expect("first run");

    // A document appended to the index after a signed Generate must be
    // generated, not silently dropped by reloading the old candidates.
    write(&temp.path.join("texts/d.text"), "中国中国\n");
    let grown = format!("{CORPUS_INDEX}doc d#/d.text\n");
    let second = run_full_with_index(&temp, "2", &grown).expect("second run");
    assert_eq!(
        second.candidate_count,
        first.candidate_count + 1,
        "the appended document forms its own candidate under the test config"
    );
    assert!(temp.path.join("models/model-candidates-3.db").is_file());
}

#[test]
fn a_malformed_status_is_an_error_not_a_panic() {
    let temp = TempDir::new("resume-malformed");
    write_raw_corpus(&temp);
    run_full(&temp, "1").expect("first run");

    // A corrupted status file is a typed error, never a panic (constitution
    // item 4).
    write(
        &temp.path.join("texts/a.text.segmented.status"),
        "not json at all",
    );
    let error = run_full(&temp, "2").expect_err("must reject malformed status");
    assert!(
        matches!(error, oxpinyin_train::TrainError::Malformed { .. }),
        "got {error:?}"
    );
}

// ---- generate-stage rollover + filter units --------------------------------

fn doc(size: u64) -> SegmentedDoc {
    // A real segmented body of at least one token so add_document has content;
    // `size` is what the rollover/filter weigh, set independently of the body.
    SegmentedDoc {
        title: "t".to_owned(),
        text: "10 中\n30 国\n0 \n".to_owned(),
        size,
    }
}

#[test]
fn generate_rolls_over_when_the_aggregate_exceeds_the_candidate_size() {
    let config = TrainConfig {
        minimum_file_size: 0,
        candidate_model_size: 10,
        ..TrainConfig::default()
    };
    // Sizes 6, 6, 6: after doc0 agg=6 (≤10), after doc1 agg=12 (>10) → close
    // candidate 0 covering [0,2); doc2 agg=6 → trailing candidate 1 [2,3).
    let docs = [doc(6), doc(6), doc(6)];
    let candidates = generate_candidates(&config, &docs).expect("generate");
    assert_eq!(candidates.len(), 2);
    assert_eq!((candidates[0].text_start, candidates[0].text_end), (0, 2));
    assert_eq!((candidates[1].text_start, candidates[1].text_end), (2, 3));
    assert_eq!(candidates[0].number, 0);
    assert_eq!(candidates[1].number, 1);
}

#[test]
fn generate_skips_documents_below_the_minimum_file_size() {
    let config = TrainConfig {
        minimum_file_size: 5,
        candidate_model_size: 1_000_000,
        ..TrainConfig::default()
    };
    // doc0 size 3 < 5 → skipped; doc1 size 6 ≥ 5 → kept. The candidate holds
    // only the kept document, but the window spans every document considered
    // (`GenerateStart`/`GenerateEnd` are index positions, not kept counts), so
    // the range is [0,2), not [1,2).
    let docs = [doc(3), doc(6)];
    let candidates = generate_candidates(&config, &docs).expect("generate");
    assert_eq!(candidates.len(), 1);
    assert_eq!((candidates[0].text_start, candidates[0].text_end), (0, 2));
    // The candidate holds exactly one document.
    assert_eq!(candidates[0].model.n, 1);
}

#[test]
fn generate_emits_no_trailing_empty_candidate() {
    let config = TrainConfig {
        minimum_file_size: 0,
        candidate_model_size: 5,
        ..TrainConfig::default()
    };
    // Each doc size 6 > 5 → every doc closes a candidate; no partial remains.
    let docs = [doc(6), doc(6)];
    let candidates = generate_candidates(&config, &docs).expect("generate");
    assert_eq!(candidates.len(), 2, "no empty trailing candidate");
    assert_eq!(candidates[1].text_end, 2);
}
