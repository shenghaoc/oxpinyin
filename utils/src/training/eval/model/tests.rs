use super::super::counts::Counts;
use super::EvalLanguageModel;

fn model() -> EvalLanguageModel {
    let mut counts = Counts::default();
    counts.unigrams.insert(10, 6);
    counts.unigrams.insert(20, 2);
    // <start>(1) → 10 twice; 10 → 20 four times; 10 → 10 once.
    counts.bigrams.insert((1, 10), 2);
    counts.bigrams.insert((10, 20), 4);
    counts.bigrams.insert((10, 10), 1);
    EvalLanguageModel::from_counts(&counts, 0.312_699)
}

#[test]
fn the_float_probabilities_are_the_stored_ratios() {
    let model = model();
    assert_eq!(model.unigram_poss(10).to_bits(), 0.75_f64.to_bits());
    assert_eq!(model.bigram_poss(10, 20), Some(0.8));
    assert_eq!(model.bigram_poss(20, 10), None);
    assert!(model.has_bigram_row(10));
    assert!(!model.has_bigram_row(20));
}
