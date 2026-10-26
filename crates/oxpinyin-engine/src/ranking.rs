//! Candidate scores shared by ordinary windows and prediction.

use std::cmp::Ordering;

/// An interpolated frequency score with a deterministic total order.
/// No integer scale, truncation or unsigned subtraction participates.
#[derive(Clone, Copy, Debug, Default)]
pub struct RankScore(f32);

impl PartialEq for RankScore {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for RankScore {}

impl PartialOrd for RankScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for RankScore {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// The candidate interpolation, including the product's 0.1 bigram
/// discount (074a2219 pinyin.cpp:33,1811-1866). Prefix, addon and LONGER
/// candidates pass a zero bigram possibility. Counts retain the widths
/// prescribed by persistent data at their producers.
#[must_use]
pub fn interpolated_score(unigram: u64, total: u64, bigram: f32, lambda: f32) -> RankScore {
    if total == 0 {
        return RankScore::default();
    }
    RankScore(lambda * bigram * 0.1 + (1.0 - lambda) * unigram as f32 / total as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_unequal_scores_remain_distinct_and_the_bigram_joins_before_ranking() {
        assert_eq!(interpolated_score(0, 10, 0.5, 0.5), RankScore(0.025));
        let a = interpolated_score(1, 51_051_831, 0.0, 0.312_699);
        let b = interpolated_score(3, 51_051_831, 0.0, 0.312_699);
        assert!(b > a);
        assert!(interpolated_score(1, 51_051_831, 0.5, 0.312_699) > b);
        assert_eq!(interpolated_score(99, 0, 0.5, 0.3), RankScore::default());
    }

    #[test]
    fn ordering_is_total_even_for_special_scores() {
        let values = [
            f32::NEG_INFINITY,
            -1.0,
            -0.0,
            0.0,
            1.0,
            f32::INFINITY,
            f32::NAN,
        ];
        for a in values {
            for b in values {
                assert_eq!(RankScore(a).cmp(&RankScore(b)), a.total_cmp(&b));
                assert_eq!(RankScore(a) == RankScore(b), a.to_bits() == b.to_bits());
            }
        }
    }
}
