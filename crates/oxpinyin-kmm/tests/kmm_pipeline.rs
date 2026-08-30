//! End-to-end KMM pipeline over a small, hand-verifiable fixture, exercising
//! every transformation the trainer's main pipeline chains:
//! generate → estimate → merge → validate → prune → export → import →
//! k_mixture_model_to_interpolation.

use oxpinyin_kmm::{
    DEFAULT_CDF, DEFAULT_PRUNE_K, GenerateParams, KMixtureModel, estimate, export, import,
    kmm_text_to_interpolation, merge_into, prune, validate,
};

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
    let doc = "10 甲\n20 乙\n";

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
         \\item 10 甲 count 2\n\
         \\item 20 乙 count 2\n\
         \\2-gram\n\
         \\item 1 <start> 10 甲 count 2\n\
         \\item 10 甲 20 乙 count 2\n\
         \\end\n"
    );
}

/// Merging per-document candidates equals counting both documents in one
/// model — the invariant that makes the candidate-merge stage sound.
#[test]
fn merge_equals_combined_run() {
    let doc_a = "10 甲\n20 乙\n30 丙\n";
    let doc_b = "30 丙\n10 甲\n20 乙\n";

    let mut merged = generated(&[doc_a]);
    merge_into(&mut merged, &generated(&[doc_b])).expect("merge");

    let combined = generated(&[doc_a, doc_b]);

    assert_eq!(export(&merged), export(&combined));
    validate(&merged).expect("merged validates");
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
