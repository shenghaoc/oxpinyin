//! The bigram interpolation weight λ of the model's `table.conf`.
//!
//! [`crate::table_info::TableConf::load`] scans `lambda parameter:` with the
//! pin's `fscanf("%f")` (`docs/findings/data-formats.md` §3 records the
//! pinned `0.312699`). The scanned `gfloat` is λ; [`Lambda`] is the exact
//! rational the decoder's integer path uses where that `gfloat` has one
//! ([`Lambda::from_f32`]).
//!
//! Invariants:
//!
//! - A [`Lambda`] is in `[0, 1]`; a value outside it has no rational and
//!   the decoder runs the pin's float arithmetic on the `gfloat`.
//! - λ is held reduced to lowest terms (`0.312699` → `312699 / 1_000_000`).

/// λ recorded for the pinned model in `data-formats.md` §3
/// (`lambda parameter:0.312699`), as an `f32`. Convenience for logging and
/// cross-checks; the decoder uses the exact rational [`Lambda::PINNED`].
pub const PINNED_LAMBDA: f32 = 0.312_699;

/// The bigram interpolation weight λ, an exact rational `numerator /
/// denominator` in lowest terms with `numerator ≤ denominator` (λ ∈ `[0, 1]`).
///
/// `P(w_n | w_{n-1}) = λ · P_bigram + (1 − λ) · P_unigram`. Held as a rational
/// so the LM interpolation stays exact-integer (constitution §6). Constructed
/// only from [`Lambda::PINNED`] or [`Lambda::from_decimal`], both of which
/// keep the `numerator ≤ denominator` invariant the LM relies on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lambda {
    numerator: u128,
    denominator: u128,
}

impl Lambda {
    /// The pinned model's value, `0.312699 = 312699 / 1_000_000`
    /// (`data-formats.md` §3). The decoder's default when no `table.conf` is
    /// available to read. Already in lowest terms.
    pub const PINNED: Self = Self {
        numerator: 312_699,
        denominator: 1_000_000,
    };

    /// λ numerator (the bigram-term weight).
    #[must_use]
    pub const fn numerator(self) -> u128 {
        self.numerator
    }

    /// λ denominator.
    #[must_use]
    pub const fn denominator(self) -> u128 {
        self.denominator
    }

    /// λ as an `f64`, for logging and cross-checks (never used in the hot
    /// path, which reads [`Self::numerator`] / [`Self::denominator`]).
    #[must_use]
    pub fn as_f64(self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }

    /// The rational routing value of a scanned `gfloat` (maintainer ruling
    /// 2026-10-28 UTC, D1): the exact rational of its shortest
    /// round-tripping decimal, when that is a plain decimal in `[0, 1]`
    /// whose denominator fits a `u128`. `None` sends the decoder to the
    /// pin's float arithmetic on the `gfloat` itself.
    ///
    /// The scanned value is the only source of λ: the `table.conf` text is
    /// never re-read. This bridge between the exact-rational decoder path
    /// and the pin's `gfloat` is temporary; #1378 removes it.
    #[must_use]
    pub fn from_f32(value: f32) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        // `Display` writes the shortest decimal that reads back as `value`,
        // without an exponent; a negative value or zero keeps its sign and
        // is refused below.
        Self::from_decimal(&value.to_string())
    }

    /// Parses a decimal literal (`0.312699`, `0.5`, `1`) into the exact
    /// rational it denotes, reduced to lowest terms, or `None` when it is not
    /// a plain decimal in `[0, 1]`.
    ///
    /// `denominator` starts at `10^(fractional digits)` (so `0.312699` is
    /// `312699 / 1_000_000` with no float rounding) and is then reduced by the
    /// gcd, so trailing zeros cannot inflate it (`0.50` → `1 / 2`, not
    /// `50 / 100`). A value `> 1` is rejected — λ is a weight in `[0, 1]`, and
    /// admitting `> 1` would break the LM's `denominator − numerator` term.
    #[must_use]
    pub fn from_decimal(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        let (int_part, frac_part) = match text.split_once('.') {
            Some((int_part, frac_part)) => (int_part, frac_part),
            None => (text, ""),
        };
        // Reject signs, exponents, and any non-digit: a rational conversion is
        // only exact for a plain decimal.
        if int_part.is_empty() && frac_part.is_empty() {
            return None;
        }
        if !int_part.bytes().all(|b| b.is_ascii_digit())
            || !frac_part.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let denominator = 10_u128.checked_pow(u32::try_from(frac_part.len()).ok()?)?;
        let int_value: u128 = if int_part.is_empty() {
            0
        } else {
            int_part.parse().ok()?
        };
        let frac_value: u128 = if frac_part.is_empty() {
            0
        } else {
            frac_part.parse().ok()?
        };
        let numerator = int_value
            .checked_mul(denominator)?
            .checked_add(frac_value)?;
        // λ ∈ [0, 1]: a config value above 1 is treated like an absent line.
        if numerator > denominator {
            return None;
        }
        let divisor = gcd(numerator, denominator);
        Some(Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        })
    }
}

/// Greatest common divisor (Euclid). `gcd(0, d) = d`, so `0` reduces to
/// `0 / 1` and any denominator stays `≥ 1`.
const fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    if a == 0 { 1 } else { a }
}

#[cfg(test)]
mod tests {
    use super::{Lambda, PINNED_LAMBDA};

    #[test]
    fn the_pinned_gfloat_routes_to_the_pinned_rational() {
        assert_eq!(Lambda::from_f32(PINNED_LAMBDA), Some(Lambda::PINNED));
    }

    #[test]
    fn a_gfloat_routes_by_its_shortest_decimal() {
        assert_eq!(Lambda::from_f32(0.5), Lambda::from_decimal("0.5"));
        assert_eq!(Lambda::from_f32(1.0), Lambda::from_decimal("1"));
        assert_eq!(Lambda::from_f32(0.0), Lambda::from_decimal("0"));
        assert_eq!(
            Lambda::from_f32(1e-30),
            Lambda::from_decimal(&format!("0.{}1", "0".repeat(29)))
        );
        for value in [
            -0.0_f32,
            -0.5,
            1.5,
            f32::from_bits(1),
            f32::NAN,
            f32::INFINITY,
        ] {
            assert_eq!(Lambda::from_f32(value), None, "{value}");
        }
    }

    #[test]
    fn pinned_rational_agrees_with_pinned_lambda_f32() {
        // The exact rational and the f32 convenience constant agree closely.
        let exact = Lambda::PINNED.as_f64();
        let approx = f64::from(PINNED_LAMBDA);
        assert!(
            (exact - approx).abs() < f64::from(f32::EPSILON) * 0.312_699,
            "exact {exact} vs f32 {approx}"
        );
    }

    #[test]
    fn from_decimal_reduces_and_bounds_to_unit_interval() {
        // Reduced to lowest terms — trailing zeros do not inflate the denom.
        assert_eq!(
            Lambda::from_decimal("0.5"),
            Some(Lambda::from_decimal("0.50").unwrap())
        );
        let half = Lambda::from_decimal("0.5").unwrap();
        assert_eq!((half.numerator(), half.denominator()), (1, 2));
        // Boundaries: 0 and 1 are valid weights.
        assert_eq!(Lambda::from_decimal("1").map(Lambda::as_f64), Some(1.0));
        let zero = Lambda::from_decimal("0").unwrap();
        assert_eq!((zero.numerator(), zero.denominator()), (0, 1));
        // λ > 1 is rejected like an absent line (guards the LM's den − num).
        assert_eq!(Lambda::from_decimal("1.5"), None);
        assert_eq!(Lambda::from_decimal("2"), None);
        // Non-decimals rejected.
        assert_eq!(Lambda::from_decimal(""), None);
        assert_eq!(Lambda::from_decimal("-0.3"), None);
        assert_eq!(Lambda::from_decimal("1e-3"), None);
        assert_eq!(Lambda::from_decimal("abc"), None);
    }
}
