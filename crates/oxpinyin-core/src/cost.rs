//! Deterministic cost arithmetic.
//!
//! **The scale: [`COST_PER_BIT`] is 1,000, and it is one bit of surprisal.**
//! An event the model gives half its mass to costs 1,000, a quarter 2,000, an
//! eighth 3,000; [`UNKNOWN_COST`] is 40,000, or forty bits, the finite floor
//! charged for an event with no mass at all. Lower is better and costs add
//! along a path. Stated here so a downstream crate reading a `Cost` does not
//! have to go to `docs/findings/scoring-spec.md` to find out what the number
//! means.
//!
//! Costs are integers on a fixed-point negative-log₂ scale, and every step of
//! the computation is integer arithmetic. Floating point is deliberately
//! absent: `f64::ln` is not required to be bit-identical across platforms and
//! libms, and constitution item 6 makes engine output a pure function of
//! (input, user state, config) — on every operating system, not just this one.

use crate::Cost;

/// Cost units charged for one bit of surprisal.
///
/// A phrase the model gives half its mass to costs 1,000; a quarter, 2,000.
pub const COST_PER_BIT: i64 = 1_000;

/// Cost charged for an event a model gives no mass to at all.
///
/// Finite rather than infinite so a path through an unknown token stays
/// comparable to other paths instead of poisoning the arithmetic. Forty bits
/// is far beyond any surprisal a real table produces, so a known event always
/// wins.
pub const UNKNOWN_COST: Cost = 40 * COST_PER_BIT;

/// Fractional bits in the fixed-point logarithm.
const FRAC_BITS: u32 = 32;

/// Half a fixed-point unit, for round-to-nearest.
const HALF: u64 = 1 << (FRAC_BITS - 1);

/// Scale of the mantissa used while extracting fractional log bits.
///
/// A mantissa in `[1, 2)` is held as `[2^62, 2^63)`, so squaring it stays
/// below `2^126` and cannot overflow `u128`.
const MANTISSA_SHIFT: u32 = 62;

/// Cost of an event seen `count` times out of `total`.
///
/// Returns [`UNKNOWN_COST`] when the event has no mass, and `0` when it has
/// all of it. `count` above `total` is clamped to `total` rather than
/// producing a negative cost, so a malformed table cannot make a path look
/// better than certain.
#[must_use]
pub fn surprisal(count: u64, total: u64) -> Cost {
    if count == 0 || total == 0 {
        return UNKNOWN_COST;
    }
    let count = count.min(total);
    let bits = log2_fixed(total) - log2_fixed(count);
    let scaled = (bits.saturating_mul(COST_PER_BIT.unsigned_abs()) + HALF) >> FRAC_BITS;
    Cost::try_from(scaled)
        .unwrap_or(UNKNOWN_COST)
        .min(UNKNOWN_COST)
}

/// Base-2 logarithm of `value` as a fixed-point number with [`FRAC_BITS`]
/// fractional bits.
///
/// Integer part from `ilog2`; fractional bits by repeated squaring of the
/// mantissa, which is the standard bit-at-a-time algorithm and uses nothing
/// but `u128` multiplication and shifts.
fn log2_fixed(value: u64) -> u64 {
    debug_assert!(value > 0, "log2_fixed requires a positive value");
    let integer = value.ilog2();

    // Mantissa in [1, 2), scaled by 2^MANTISSA_SHIFT.
    let mut mantissa = (u128::from(value) << MANTISSA_SHIFT) >> integer;
    let mut fraction = 0_u64;
    for bit in 0..FRAC_BITS {
        mantissa = (mantissa * mantissa) >> MANTISSA_SHIFT;
        if mantissa >= 2_u128 << MANTISSA_SHIFT {
            fraction |= 1 << (FRAC_BITS - 1 - bit);
            mantissa >>= 1;
        }
    }

    (u64::from(integer) << FRAC_BITS) | fraction
}

/// Shrinks a ratio until both halves fit in a `u64`, preserving its value as
/// closely as a right shift allows.
///
/// Interpolating probabilities multiplies counts by totals, which leaves
/// `u64` behind for realistic tables. Shifting both halves by the same amount
/// keeps the ratio and stays deterministic.
#[must_use]
pub fn reduce_ratio(numerator: u128, denominator: u128) -> (u64, u64) {
    const LIMIT: u128 = 1 << 63;

    let mut numerator = numerator;
    let mut denominator = denominator;
    while numerator >= LIMIT || denominator >= LIMIT {
        numerator >>= 1;
        denominator >>= 1;
    }
    // Both halves are below 2^63 here, so neither conversion can fail.
    (
        u64::try_from(numerator).unwrap_or(u64::MAX),
        u64::try_from(denominator).unwrap_or(u64::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::{COST_PER_BIT, FRAC_BITS, UNKNOWN_COST, log2_fixed, reduce_ratio, surprisal};

    #[test]
    fn exact_powers_of_two_have_exact_logarithms() {
        for exponent in 0..64_u32 {
            assert_eq!(
                log2_fixed(1 << exponent),
                u64::from(exponent) << FRAC_BITS,
                "2^{exponent}"
            );
        }
    }

    #[test]
    fn the_logarithm_is_monotonic() {
        let mut previous = log2_fixed(1);
        for value in 2..4_096_u64 {
            let current = log2_fixed(value);
            assert!(current > previous, "log2 fell at {value}");
            previous = current;
        }
    }

    #[test]
    fn surprisal_charges_one_unit_per_halving() {
        assert_eq!(surprisal(1, 1), 0);
        assert_eq!(surprisal(1, 2), COST_PER_BIT);
        assert_eq!(surprisal(1, 4), 2 * COST_PER_BIT);
        assert_eq!(surprisal(1, 1_024), 10 * COST_PER_BIT);
        assert_eq!(surprisal(3, 12), 2 * COST_PER_BIT);
    }

    #[test]
    fn surprisal_is_total_for_degenerate_input() {
        assert_eq!(surprisal(0, 100), UNKNOWN_COST);
        assert_eq!(surprisal(5, 0), UNKNOWN_COST);
        assert_eq!(surprisal(10, 5), 0, "count above total is clamped");
        assert_eq!(surprisal(1, u64::MAX), UNKNOWN_COST, "clamped to the floor");
    }

    #[test]
    fn a_rarer_event_never_costs_less() {
        let total = 87_000;
        let mut previous = surprisal(total, total);
        for count in (1..total).rev().step_by(97) {
            let current = surprisal(count, total);
            assert!(current >= previous, "cost fell at count {count}");
            previous = current;
        }
    }

    #[test]
    fn reducing_a_ratio_keeps_its_value() {
        assert_eq!(reduce_ratio(3, 12), (3, 12));

        let (numerator, denominator) = reduce_ratio(3 << 70, 12 << 70);
        assert!(numerator < 1 << 63 && denominator < 1 << 63);
        assert_eq!(surprisal(numerator, denominator), 2 * COST_PER_BIT);
    }
}
