//! End-to-end KMM pipeline over a small, hand-verifiable fixture, exercising
//! every transformation the trainer's main pipeline chains:
//! generate → estimate → merge → validate → prune → export → import →
//! k_mixture_model_to_interpolation.

use std::path::PathBuf;

use oxpinyin_kmm::{
    DEFAULT_CDF, DEFAULT_PRUNE_K, GenerateParams, KMixtureModel, estimate, export, import,
    kmm_text_to_interpolation, merge_into, prune, validate,
};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn generated(docs: &[&str]) -> KMixtureModel {
    let mut model = KMixtureModel::new();
    for doc in docs {
        model
            .add_document(doc, GenerateParams::default())
            .expect("generate");
    }
    model
}

/// Merge two identical single-document candidates, prune with CDF 0 (keep
/// all), export, and convert to interpolation — a fully hand-verifiable
/// path with a byte golden.
#[test]
fn full_pipeline_golden() {
    // The document cycles 甲→乙→甲 so both tokens appear as a W1 and the model
    // is complete (every unigram freq is stored in an array header) — required
    // for `validate` to pass. A W2-only token would get no array header under
    // the pin's Tkrzw backend and the model would fail `validate`, exactly as
    // the pin's own validate rejects such a small-corpus model (oracle-verified).
    let doc = "10 甲\n20 乙\n10 甲\n";

    // generate two candidates.
    let candidate_a = generated(&[doc]);
    let candidate_b = generated(&[doc]);

    // estimate: both score in the unit interval.
    let deleted = generated(&["10 甲\n20 乙\n"]);
    let score_a = estimate(&candidate_a, &deleted)
        .expect("estimate a")
        .average;
    let score_b = estimate(&candidate_b, &deleted)
        .expect("estimate b")
        .average;
    assert!((0.0..=1.0).contains(&score_a));
    assert_eq!(score_a, score_b, "identical candidates score identically");

    // merge the sorted candidates (identical scores).
    let mut merged = candidate_a.clone();
    merge_into(&mut merged, &candidate_b).expect("merge");
    validate(&merged).expect("merged validates");

    // export round-trips through import.
    let kmm_text = export(&merged);
    assert_eq!(import(&kmm_text).expect("import"), merged);

    // prune with CDF 0 keeps everything.
    let mut pruned = merged.clone();
    prune(&mut pruned, DEFAULT_PRUNE_K, 0.0).expect("prune");
    assert_eq!(pruned, merged, "CDF 0 prunes nothing");
    validate(&pruned).expect("pruned validates");

    // convert to interpolation.
    let interp = kmm_text_to_interpolation(&export(&pruned)).expect("to interpolation");
    assert_eq!(
        interp,
        "\\data model interpolation\n\
         \\1-gram\n\
         \\item 10 甲 count 4\n\
         \\item 20 乙 count 2\n\
         \\2-gram\n\
         \\item 1 <start> 10 甲 count 2\n\
         \\item 10 甲 20 乙 count 2\n\
         \\item 20 乙 10 甲 count 2\n\
         \\end\n"
    );
}

/// Merging per-document candidates equals counting both documents in one
/// model — the invariant that makes the candidate-merge stage sound.
#[test]
fn merge_equals_combined_run() {
    // The invariant holds only when every token appears as a W1 in each
    // document it occurs in (see `merge::tests::merge_equals_single_run…`): a
    // W2-only-in-one-candidate token stores its unigram freq in the combined
    // run but not in the per-candidate merge, matching the pin's Tkrzw gen.
    // These cyclic documents keep every token a W1.
    let doc_a = "10 甲\n20 乙\n30 丙\n10 甲\n";
    let doc_b = "20 乙\n30 丙\n10 甲\n20 乙\n";

    let mut merged = generated(&[doc_a]);
    merge_into(&mut merged, &generated(&[doc_b])).expect("merge");

    let combined = generated(&[doc_a, doc_b]);

    assert_eq!(export(&merged), export(&combined));
    validate(&merged).expect("merged validates");
}

/// End-to-end over a real segmented corpus: consume the committed `spseg`
/// output (the segment stage's real product over the W3 phrase index) and
/// run the whole KMM chain — generate → estimate → merge → validate →
/// prune → export → to-interpolation — with no Python, SQLite, `make`, or
/// libpinyin. Proves the main training pipeline runs natively (completion
/// criteria §9, §14).
#[test]
fn end_to_end_from_real_segmented_corpus() {
    let segmented = std::fs::read_to_string(repo_root().join("fixtures/w9/segmenter-spseg-w3.txt"))
        .expect("committed spseg segmented corpus");

    // generate: one candidate from the whole segmented document.
    let mut candidate = KMixtureModel::new();
    candidate
        .add_document(&segmented, GenerateParams::default())
        .expect("generate");
    assert_eq!(candidate.n, 1, "one document");
    // This small real corpus has W2-only tokens (words that never begin a
    // pair), so — under the pin's Tkrzw backend — their unigram freq is in
    // magic total_freq but in no array header, and `validate` rejects the
    // model: `Σ header freq != total_freq`. The pin's own validate rejects
    // this exact corpus identically (exit 61, "the total freq differs from
    // sum of freqs" — oracle-verified). At real corpus scale every token is a
    // W1 and the model validates; the rejection here is pin-faithful, not a
    // pipeline failure — the chain below still runs and produces output.
    assert!(
        validate(&candidate).is_err(),
        "pin-faithful: a small-corpus model with W2-only tokens fails validate"
    );

    // estimate against a held-out slice (the first half of the lines).
    let held_lines: Vec<&str> = segmented
        .lines()
        .take(segmented.lines().count() / 2)
        .collect();
    let mut deleted = KMixtureModel::new();
    deleted
        .add_document(&held_lines.join("\n"), GenerateParams::default())
        .expect("held-out generate");
    let score = estimate(&candidate, &deleted).expect("estimate").average;
    assert!((0.0..=1.0).contains(&score), "lambda {score} out of range");

    // merge one candidate into a fresh result, export round-trip. (validate
    // is not asserted here for the same pin-faithful reason as above — the
    // merged small-corpus model still carries W2-only tokens.)
    let mut merged = KMixtureModel::new();
    merge_into(&mut merged, &candidate).expect("merge");
    assert_eq!(import(&export(&merged)).expect("import"), merged);

    // prune (CDF 0 keeps everything so the model stays non-empty), then
    // convert to interpolation2.text.
    let mut pruned = merged.clone();
    prune(&mut pruned, DEFAULT_PRUNE_K, 0.0).expect("prune");
    let interp = kmm_text_to_interpolation(&export(&pruned)).expect("to interpolation");

    // The result is well-formed interpolation2.text: header, both sections,
    // no <start> unigram, and every unigram/bigram line is `\item …`.
    assert!(interp.starts_with("\\data model interpolation\n"));
    assert!(interp.contains("\\1-gram\n"));
    assert!(interp.contains("\\2-gram\n"));
    assert!(interp.trim_end().ends_with("\\end"));
    for line in interp.lines() {
        if line.starts_with("\\item ") {
            // No sentence_start (<start>) survives the 1-gram section.
            let is_unigram_start = line.starts_with("\\item 1 <start> count");
            assert!(!is_unigram_start, "unigram <start> must be dropped: {line}");
        }
    }
    // There is at least one real interpolation record.
    assert!(
        interp.lines().filter(|l| l.starts_with("\\item ")).count() > 0,
        "the pipeline produced interpolation records"
    );
}

/// The default `--CDF 0.99` prune drops every rare pair (each occurs once),
/// and the pipeline stays deterministic across runs.
#[test]
fn default_prune_and_determinism() {
    let docs = [
        "10 甲\n20 乙\n30 丙\n",
        "20 乙\n30 丙\n10 甲\n",
        "40 丁\n50 戊\n",
    ];

    let run = || {
        let mut model = generated(&docs);
        prune(&mut model, DEFAULT_PRUNE_K, DEFAULT_CDF).expect("prune");
        export(&model)
    };
    let first = run();
    let second = run();
    assert_eq!(first, second, "pipeline is deterministic");

    // Every rare bigram pair is pruned by the default CDF.
    let mut model = generated(&docs);
    prune(&mut model, DEFAULT_PRUNE_K, DEFAULT_CDF).expect("prune");
    let bigram_pairs: usize = model.grams.values().map(|g| g.items.len()).sum();
    assert_eq!(bigram_pairs, 0);
}
