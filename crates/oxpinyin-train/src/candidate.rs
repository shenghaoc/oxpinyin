//! Candidate collection, scoring index, sort, and top-N selection
//! (`estimate.py`'s `gatherModels`/`sortModels` and `tryprune.py`'s
//! `mergeSomeModels`).
//!
//! After each candidate is scored (its `EstimateScore`, the average λ from
//! `estimate_k_mixture_model`), the estimate stage gathers `subdir#model#score`
//! records into `estimate.index`, sorts them by score descending into
//! `estimate.sorted.index`, and the prune stage merges the top N. Reproduced
//! here as a typed [`Candidate`] with a stable descending sort and a
//! descending-order guard, so the merge order is a pure function of the
//! scores.

use crate::error::TrainError;

/// One scored candidate model: where it lives (`subdir`/`model_name`) and its
/// ranking score (average λ). The `estimate.index` record `subdir#model#score`.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    /// The candidate's directory relative to the model root (`subdir`).
    pub subdir: String,
    /// The candidate model filename (`getCandidateModelName`).
    pub model_name: String,
    /// The candidate's average-λ score (`EstimateScore`).
    pub score: f64,
}

impl Candidate {
    /// The `subdir#model_name#score` index line (`gatherModels`).
    #[must_use]
    pub fn index_line(&self) -> String {
        format!(
            "{}#{}#{}",
            self.subdir,
            self.model_name,
            format_score(&self.score)
        )
    }

    /// Parses one `subdir#model#score` line (`sortModels`' `split('#', 2)`).
    ///
    /// # Errors
    ///
    /// Returns [`TrainError::Malformed`] when the line lacks two `#` or the
    /// score is not a number.
    pub fn parse(line: &str) -> Result<Self, TrainError> {
        let mut parts = line.splitn(3, '#');
        let (Some(subdir), Some(model_name), Some(score)) =
            (parts.next(), parts.next(), parts.next())
        else {
            return Err(TrainError::Malformed {
                detail: format!("candidate line needs subdir#model#score: {line:?}"),
            });
        };
        let score = score
            .trim()
            .parse::<f64>()
            .map_err(|_| TrainError::Malformed {
                detail: format!("candidate score is not a number: {score:?}"),
            })?;
        Ok(Self {
            subdir: subdir.to_owned(),
            model_name: model_name.to_owned(),
            score,
        })
    }
}

/// A gathered, then sorted, set of candidates — the `estimate.index` and
/// `estimate.sorted.index` contents as typed records.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CandidateIndex {
    candidates: Vec<Candidate>,
}

impl CandidateIndex {
    /// An empty index.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Gathers scored candidates (`gatherModels`), keeping insertion order.
    #[must_use]
    pub fn from_candidates(candidates: Vec<Candidate>) -> Self {
        Self { candidates }
    }

    /// Parses an `estimate.index` / `estimate.sorted.index` body.
    ///
    /// # Errors
    ///
    /// Returns [`TrainError`] when a line is malformed.
    pub fn parse(text: &str) -> Result<Self, TrainError> {
        let mut candidates = Vec::new();
        for line in text.lines() {
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                continue;
            }
            candidates.push(Candidate::parse(line)?);
        }
        Ok(Self { candidates })
    }

    /// Adds one scored candidate.
    pub fn push(&mut self, candidate: Candidate) {
        self.candidates.push(candidate);
    }

    /// The candidates, in current order.
    #[must_use]
    pub fn candidates(&self) -> &[Candidate] {
        &self.candidates
    }

    /// The number of candidates.
    #[must_use]
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    /// Whether there are no candidates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// The gather-index text (`subdir#model#score` lines, one per candidate).
    #[must_use]
    pub fn to_index_text(&self) -> String {
        let mut text = String::new();
        for candidate in &self.candidates {
            text.push_str(&candidate.index_line());
            text.push('\n');
        }
        text
    }

    /// Sorts by score descending, returning the sorted index
    /// (`records.sort(key=itemgetter(2), reverse=True)`). Python's sort is
    /// stable, so ties keep gather order; `sort_by` here is stable too.
    #[must_use]
    pub fn sorted_by_score_desc(&self) -> Self {
        let mut candidates = self.candidates.clone();
        // Stable, descending. `total_cmp` gives a total order over the f64
        // scores (they are finite λ averages) without a partial-cmp unwrap.
        candidates.sort_by(|a, b| b.score.total_cmp(&a.score));
        Self { candidates }
    }

    /// The top `n` candidates for the merge stage, validating that the scores
    /// are in descending order and none exceeds `1.0` — the invariant
    /// `mergeSomeModels` enforces (`last_score` starts at `1.` and each score
    /// must not exceed the previous).
    ///
    /// # Errors
    ///
    /// Returns [`TrainError::NotEnoughCandidates`] when fewer than `n` exist,
    /// or [`TrainError::ScoresNotDescending`] when the order is wrong.
    pub fn top_n(&self, n: usize) -> Result<Vec<Candidate>, TrainError> {
        if self.candidates.len() < n {
            return Err(TrainError::NotEnoughCandidates {
                requested: n,
                available: self.candidates.len(),
            });
        }
        let mut last_score = 1.0_f64;
        let mut selected = Vec::with_capacity(n);
        for candidate in self.candidates.iter().take(n) {
            if candidate.score > last_score {
                return Err(TrainError::ScoresNotDescending {
                    score: candidate.score,
                    previous: last_score,
                });
            }
            last_score = candidate.score;
            selected.push(candidate.clone());
        }
        Ok(selected)
    }
}

/// Renders a score the way `str(float)` does for the index line: the shortest
/// decimal that round-trips (Rust `{}` for `f64` matches Python's `repr`).
fn format_score(score: &f64) -> String {
    format!("{score}")
}

#[cfg(test)]
mod tests {
    use super::{Candidate, CandidateIndex};

    fn candidate(name: &str, score: f64) -> Candidate {
        Candidate {
            subdir: "sub".to_owned(),
            model_name: name.to_owned(),
            score,
        }
    }

    #[test]
    fn index_line_round_trips() {
        let candidate = candidate("model-candidates-0.db", 0.312699);
        let parsed = Candidate::parse(&candidate.index_line()).expect("parse");
        assert_eq!(parsed, candidate);
    }

    #[test]
    fn sorts_by_score_descending_stably() {
        let index = CandidateIndex::from_candidates(vec![
            candidate("a.db", 0.2),
            candidate("b.db", 0.9),
            candidate("c.db", 0.5),
            candidate("d.db", 0.9), // ties b; stable keeps b before d.
        ]);
        let sorted = index.sorted_by_score_desc();
        let names: Vec<&str> = sorted
            .candidates()
            .iter()
            .map(|c| c.model_name.as_str())
            .collect();
        assert_eq!(names, ["b.db", "d.db", "c.db", "a.db"]);
    }

    #[test]
    fn top_n_takes_the_first_n_in_descending_order() {
        let index = CandidateIndex::from_candidates(vec![
            candidate("a.db", 0.9),
            candidate("b.db", 0.5),
            candidate("c.db", 0.1),
        ]);
        let top = index.top_n(2).expect("top 2");
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].model_name, "a.db");
        assert_eq!(top[1].model_name, "b.db");
    }

    #[test]
    fn top_n_needs_enough_candidates() {
        let index = CandidateIndex::from_candidates(vec![candidate("a.db", 0.9)]);
        let error = index.top_n(3).unwrap_err();
        assert!(matches!(
            error,
            crate::error::TrainError::NotEnoughCandidates {
                requested: 3,
                available: 1
            }
        ));
    }

    #[test]
    fn top_n_rejects_ascending_scores() {
        // Not sorted descending: mergeSomeModels would raise "scores must be
        // descending".
        let index =
            CandidateIndex::from_candidates(vec![candidate("a.db", 0.3), candidate("b.db", 0.9)]);
        assert!(matches!(
            index.top_n(2).unwrap_err(),
            crate::error::TrainError::ScoresNotDescending { .. }
        ));
    }

    #[test]
    fn top_n_rejects_a_score_above_one() {
        let index = CandidateIndex::from_candidates(vec![candidate("a.db", 1.5)]);
        assert!(matches!(
            index.top_n(1).unwrap_err(),
            crate::error::TrainError::ScoresNotDescending { .. }
        ));
    }
}
