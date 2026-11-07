//! Predicted-candidate type selection (#540, #542).

use crate::facade;
use crate::readers::{candidate_at, candidate_type, n_candidate, predicted_rows};
use crate::support::{Kit, P};

#[derive(Debug, PartialEq)]
pub struct PredictedTypes {
    pub before: (Vec<(i32, String)>, u32),
    pub punctuation: Option<bool>,
    pub punctuation_again: Option<bool>,
    pub prefix: Option<bool>,
}

/// Chooses the first candidate of `kind` in the existing list
/// (`choose_predicted_of`); `None` when none remains.
fn choose_predicted_of(inst: P, kind: i32) -> Option<bool> {
    (0..n_candidate(inst))
        .filter_map(|index| candidate_at(inst, index))
        .find(|&cand| candidate_type(inst, cand) == kind)
        .map(|cand| facade::choose_predicted_candidate(inst, cand))
}

/// Predictions for 我, then a punctuation row twice and a prefix row, from the
/// cached instance's one generated list (kinds 8 and 5).
pub fn choose_predicted_types(k: &mut Kit) -> PredictedTypes {
    let inst = k.inst();
    let before = predicted_rows(inst, "我", 12);
    let punctuation = choose_predicted_of(inst, 8);
    let punctuation_again = choose_predicted_of(inst, 8);
    let prefix = choose_predicted_of(inst, 5);
    PredictedTypes {
        before,
        punctuation,
        punctuation_again,
        prefix,
    }
}
