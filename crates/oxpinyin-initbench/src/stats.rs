//! Robust summaries of repeated measurements.
//!
//! Samples come from independent processes on a shared host, so the
//! distributions are skewed and have outliers: the report uses medians, the
//! interquartile range, the median absolute deviation, and a percentile
//! bootstrap for the uncertainty of a median or of a ratio of medians. The
//! bootstrap uses a fixed-seed generator so the same samples always give the
//! same interval (determinism, constitution §6).

/// Resamples drawn by the bootstrap.
pub const BOOTSTRAP_RESAMPLES: usize = 2_000;

/// A fixed seed, so a report is a pure function of its samples.
const SEED: u64 = 0x9e37_79b9_7f4a_7c15;

/// A two-sided 95% percentile interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    /// 2.5th percentile.
    pub low: f64,
    /// 97.5th percentile.
    pub high: f64,
}

/// Order statistics of one cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    /// Number of samples.
    pub n: usize,
    /// Smallest sample.
    pub min: f64,
    /// Largest sample.
    pub max: f64,
    /// Median.
    pub median: f64,
    /// First quartile (linear interpolation).
    pub q1: f64,
    /// Third quartile (linear interpolation).
    pub q3: f64,
    /// Median absolute deviation from the median.
    pub mad: f64,
}

impl Summary {
    /// Spread as `(max - min) / median`, the figure `benches.md` reports;
    /// `None` for a zero median.
    #[must_use]
    pub fn range_over_median(&self) -> Option<f64> {
        (self.median != 0.0).then(|| (self.max - self.min) / self.median)
    }
}

fn sorted(values: &[f64]) -> Option<Vec<f64>> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let mut copy = values.to_vec();
    copy.sort_by(f64::total_cmp);
    Some(copy)
}

/// The `p`-quantile (0..=1) of an already sorted slice, by linear
/// interpolation between order statistics.
fn quantile_sorted(sorted: &[f64], p: f64) -> f64 {
    let last = sorted.len() - 1;
    let position = p.clamp(0.0, 1.0) * last as f64;
    let below = position.floor() as usize;
    let above = position.ceil() as usize;
    let fraction = position - below as f64;
    sorted[below] + (sorted[above.min(last)] - sorted[below]) * fraction
}

/// The median; `None` for no samples or a non-finite sample.
#[must_use]
pub fn median(values: &[f64]) -> Option<f64> {
    sorted(values).map(|s| quantile_sorted(&s, 0.5))
}

/// Order statistics; `None` for no samples or a non-finite sample.
#[must_use]
pub fn summarize(values: &[f64]) -> Option<Summary> {
    let s = sorted(values)?;
    let median = quantile_sorted(&s, 0.5);
    let deviations: Vec<f64> = s.iter().map(|value| (value - median).abs()).collect();
    Some(Summary {
        n: s.len(),
        min: s[0],
        max: s[s.len() - 1],
        median,
        q1: quantile_sorted(&s, 0.25),
        q3: quantile_sorted(&s, 0.75),
        mad: self::median(&deviations)?,
    })
}

/// A xorshift64* generator: tiny, seedable, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, bound: usize) -> usize {
        // Modulo bias is irrelevant at these sizes against a 2^64 range.
        (self.next() % bound as u64) as usize
    }
}

fn resampled_median(values: &[f64], rng: &mut Rng, scratch: &mut Vec<f64>) -> f64 {
    scratch.clear();
    for _ in 0..values.len() {
        scratch.push(values[rng.below(values.len())]);
    }
    scratch.sort_by(f64::total_cmp);
    quantile_sorted(scratch, 0.5)
}

fn interval_of(mut estimates: Vec<f64>) -> Option<Interval> {
    estimates.retain(|value| value.is_finite());
    if estimates.len() < 2 {
        return None;
    }
    estimates.sort_by(f64::total_cmp);
    Some(Interval {
        low: quantile_sorted(&estimates, 0.025),
        high: quantile_sorted(&estimates, 0.975),
    })
}

/// A 95% bootstrap interval for the median. `None` below two samples (one
/// sample carries no uncertainty) or for a non-finite sample.
#[must_use]
pub fn median_interval(values: &[f64]) -> Option<Interval> {
    if values.len() < 2 || values.iter().any(|value| !value.is_finite()) {
        return None;
    }
    let mut rng = Rng(SEED);
    let mut scratch = Vec::with_capacity(values.len());
    let estimates = (0..BOOTSTRAP_RESAMPLES)
        .map(|_| resampled_median(values, &mut rng, &mut scratch))
        .collect();
    interval_of(estimates)
}

/// A 95% bootstrap interval for `median(numerator) / median(denominator)`,
/// the two cells resampled independently. `None` below two samples on either
/// side, for a non-finite sample, or when the denominator median is zero in
/// too many resamples to form an interval.
#[must_use]
pub fn ratio_interval(numerator: &[f64], denominator: &[f64]) -> Option<Interval> {
    if numerator.len() < 2
        || denominator.len() < 2
        || numerator
            .iter()
            .chain(denominator)
            .any(|value| !value.is_finite())
    {
        return None;
    }
    let mut rng = Rng(SEED);
    let mut scratch_n = Vec::with_capacity(numerator.len());
    let mut scratch_d = Vec::with_capacity(denominator.len());
    let estimates = (0..BOOTSTRAP_RESAMPLES)
        .map(|_| {
            let n = resampled_median(numerator, &mut rng, &mut scratch_n);
            let d = resampled_median(denominator, &mut rng, &mut scratch_d);
            n / d
        })
        .collect();
    interval_of(estimates)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn median_of_odd_and_even() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
    }

    #[test]
    fn empty_or_non_finite_input_is_refused() {
        assert_eq!(median(&[]), None);
        assert_eq!(summarize(&[1.0, f64::NAN]), None);
        assert_eq!(median_interval(&[1.0, f64::INFINITY]), None);
    }

    #[test]
    fn summary_matches_a_hand_computation() {
        let summary = summarize(&[1.0, 2.0, 3.0, 4.0, 100.0]).unwrap();
        assert_eq!(summary.n, 5);
        assert_eq!(summary.min, 1.0);
        assert_eq!(summary.max, 100.0);
        assert_eq!(summary.median, 3.0);
        assert_eq!(summary.q1, 2.0);
        assert_eq!(summary.q3, 4.0);
        // deviations 2,1,0,1,97 -> median 1
        assert_eq!(summary.mad, 1.0);
        assert!((summary.range_over_median().unwrap() - 33.0).abs() < 1e-12);
    }

    #[test]
    fn one_sample_has_no_interval() {
        assert_eq!(median_interval(&[5.0]), None);
        assert_eq!(ratio_interval(&[5.0], &[1.0, 2.0]), None);
    }

    #[test]
    fn intervals_contain_the_median_and_are_deterministic() {
        let values: Vec<f64> = (0..40).map(|i| 100.0 + f64::from(i % 7)).collect();
        let a = median_interval(&values).unwrap();
        let b = median_interval(&values).unwrap();
        assert_eq!(a, b);
        let m = median(&values).unwrap();
        assert!(a.low <= m && m <= a.high);
    }

    #[test]
    fn ratio_interval_brackets_the_ratio_of_medians() {
        let num: Vec<f64> = (0..30).map(|i| 200.0 + f64::from(i % 5)).collect();
        let den: Vec<f64> = (0..30).map(|i| 100.0 + f64::from(i % 3)).collect();
        let interval = ratio_interval(&num, &den).unwrap();
        let ratio = median(&num).unwrap() / median(&den).unwrap();
        assert!(interval.low <= ratio && ratio <= interval.high);
        assert!(interval.low > 1.9 && interval.high < 2.1);
    }

    #[test]
    fn identical_samples_collapse_the_interval() {
        let interval = median_interval(&[7.0; 10]).unwrap();
        assert_eq!(
            interval,
            Interval {
                low: 7.0,
                high: 7.0
            }
        );
    }
}
