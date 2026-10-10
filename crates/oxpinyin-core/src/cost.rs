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

/// A cost that is not a number: the pin's `m_poss` after a `log` of a
/// negative argument, or `inf - inf`. It is sticky under [`cost_add`] and
/// compares false against everything ([`cost_gt`]), as a NaN does. Reached
/// only through a `lambda parameter:` outside `[0, 1]`.
pub const NAN_COST: Cost = Cost::MIN;

/// A cost of `+inf` — a possibility of 0, `log(0) = -inf` in the pin.
pub const POS_INF_COST: Cost = Cost::MAX;

/// A cost of `-inf` — a possibility of `+inf`, which `log(inf)` gives the pin.
pub const NEG_INF_COST: Cost = Cost::MIN + 1;

/// Whether `cost` is [`NAN_COST`].
#[must_use]
pub const fn cost_is_nan(cost: Cost) -> bool {
    cost == NAN_COST
}

/// `a + b` with the IEEE rules for the special costs: NaN is sticky,
/// `+inf + -inf` is NaN, an infinity absorbs a finite cost, and finite sums
/// saturate short of the reserved values.
#[must_use]
pub const fn cost_add(a: Cost, b: Cost) -> Cost {
    if a == NAN_COST || b == NAN_COST {
        return NAN_COST;
    }
    match (a, b) {
        (POS_INF_COST, NEG_INF_COST) | (NEG_INF_COST, POS_INF_COST) => NAN_COST,
        (POS_INF_COST, _) | (_, POS_INF_COST) => POS_INF_COST,
        (NEG_INF_COST, _) | (_, NEG_INF_COST) => NEG_INF_COST,
        _ => {
            let sum = a.saturating_add(b);
            if sum == POS_INF_COST {
                POS_INF_COST - 1
            } else if sum <= NEG_INF_COST {
                NEG_INF_COST + 1
            } else {
                sum
            }
        }
    }
}

/// `a > b` as C compares two floats: false whenever either is NaN.
#[must_use]
pub const fn cost_gt(a: Cost, b: Cost) -> bool {
    a != NAN_COST && b != NAN_COST && a > b
}

/// The cost of a probability `x` that may lie outside `(0, 1]`: `-log2(x)` on
/// the fixed-point scale, computed from the bits of `x` with integer
/// arithmetic only. `x` above one costs less than nothing; `0` is `+inf`;
/// `+inf` is `-inf`; a negative `x` or NaN is NaN — what `log` gives the pin.
#[must_use]
pub fn cost_of_probability(x: f64) -> Cost {
    if x.is_nan() || x < 0.0 {
        return NAN_COST;
    }
    if x == 0.0 {
        return POS_INF_COST;
    }
    if x.is_infinite() {
        return NEG_INF_COST;
    }
    let bits = x.to_bits();
    let biased = i64::try_from((bits >> 52) & 0x7ff).unwrap_or(0);
    let fraction = bits & ((1_u64 << 52) - 1);
    // x = mantissa × 2^(exponent − 52), mantissa in [2^52, 2^53) for a
    // normal number; a subnormal is normalised first.
    let (mantissa, exponent) = if biased == 0 {
        let shift = i64::from(fraction.leading_zeros()) - 11;
        (fraction << shift, -1022 - shift)
    } else {
        (fraction | (1_u64 << 52), biased - 1023)
    };
    // log2(x) = log2(mantissa) − 52 + exponent, fixed point.
    let log2 = i128::from(log2_fixed(mantissa)) - (52_i128 << FRAC_BITS)
        + (i128::from(exponent) << FRAC_BITS);
    let scaled = (-log2 * i128::from(COST_PER_BIT) + i128::from(HALF)) >> FRAC_BITS;
    let limit = i128::from(POS_INF_COST) - 1;
    Cost::try_from(scaled.clamp(-limit, limit)).unwrap_or(POS_INF_COST - 1)
}

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
    use super::{NAN_COST, NEG_INF_COST, POS_INF_COST, cost_add, cost_gt, cost_of_probability};

    #[test]
    fn special_costs_follow_ieee_addition_and_comparison() {
        assert_eq!(cost_add(NAN_COST, 5), NAN_COST);
        assert_eq!(cost_add(5, NAN_COST), NAN_COST);
        assert_eq!(cost_add(POS_INF_COST, NEG_INF_COST), NAN_COST);
        assert_eq!(cost_add(POS_INF_COST, 7), POS_INF_COST);
        assert_eq!(cost_add(-7, NEG_INF_COST), NEG_INF_COST);
        assert_eq!(cost_add(POS_INF_COST - 1, 10), POS_INF_COST - 1);
        assert_eq!(cost_add(3, -8), -5);
        assert!(cost_gt(2, 1) && !cost_gt(1, 2) && !cost_gt(1, 1));
        assert!(!cost_gt(NAN_COST, 1) && !cost_gt(1, NAN_COST) && !cost_gt(NAN_COST, NAN_COST));
        assert!(cost_gt(POS_INF_COST, 1) && cost_gt(1, NEG_INF_COST));
    }

    #[test]
    fn probabilities_outside_the_unit_interval_have_their_ieee_costs() {
        assert_eq!(cost_of_probability(0.5), COST_PER_BIT);
        assert_eq!(cost_of_probability(1.0), 0);
        assert_eq!(cost_of_probability(2.0), -COST_PER_BIT);
        assert_eq!(cost_of_probability(8.0), -3 * COST_PER_BIT);
        assert_eq!(cost_of_probability(0.0), POS_INF_COST);
        assert_eq!(cost_of_probability(-0.0), POS_INF_COST);
        assert_eq!(cost_of_probability(f64::INFINITY), NEG_INF_COST);
        assert_eq!(cost_of_probability(-1.0), NAN_COST);
        assert_eq!(cost_of_probability(f64::NAN), NAN_COST);
        // A subnormal still has a finite cost: 2^-1074 is 1074 bits.
        assert_eq!(cost_of_probability(f64::from_bits(1)), 1074 * COST_PER_BIT);
    }
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
