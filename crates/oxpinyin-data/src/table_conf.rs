//! Reader for the bigram interpolation weight λ in the model's `table.conf`.
//!
//! The decoder reads λ from the model config rather than hardcoding it. The
//! `table.conf` format and the pinned value are recorded in
//! `docs/findings/data-formats.md` §3 (`lambda parameter:0.312699`, verified
//! against the oracle's installed copy). `table.conf` is a config artifact,
//! not part of the fetched `model20.text.tar.gz` (which carries only
//! `interpolation2.text` + the `.table` files — see
//! `docs/findings/model-provenance.md`); the fetched cache has none, so a
//! caller reads one only when an install / data dir provides it (e.g. a C-ABI
//! `system_dir`) and otherwise falls back to [`Lambda::PINNED`].
//!
//! Invariants:
//!
//! - λ ∈ `[0, 1]`. A `lambda parameter:` value `> 1` (or a non-decimal) is
//!   rejected as if the line were absent, so the fallback stands.
//! - λ is held as the **exact decimal rational** the file denotes, reduced to
//!   lowest terms (`0.312699` → `312699 / 1_000_000`), so the decode
//!   interpolation stays in the deterministic integer path (no float in the
//!   hot path — constitution §6).

use std::fs;
use std::path::Path;

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

/// Parses the `lambda parameter:` line from a `table.conf` body into an exact
/// rational (`data-formats.md` §3). Returns `None` when the line is absent or
/// its value is not a plain decimal in `[0, 1]`.
#[must_use]
pub fn parse_table_conf_lambda(text: &str) -> Option<Lambda> {
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("lambda parameter:") {
            return Lambda::from_decimal(rest);
        }
    }
    None
}

/// Reads λ from a `table.conf` at `path`, returning `None` when the file is
/// absent, unreadable, or carries no parsable `lambda parameter:` line.
///
/// The decoder falls back to [`Lambda::PINNED`] on `None`, so a missing
/// `table.conf` (the normal fetched-cache case) is not an error.
#[must_use]
pub fn read_table_conf_lambda(path: &Path) -> Option<Lambda> {
    let text = fs::read_to_string(path).ok()?;
    parse_table_conf_lambda(&text)
}

#[cfg(test)]
mod tests {
    use super::{Lambda, PINNED_LAMBDA, parse_table_conf_lambda};

    #[test]
    fn pinned_table_conf_line_parses_to_the_pinned_rational() {
        let lambda =
            parse_table_conf_lambda("binary format version:7\nlambda parameter:0.312699\n")
                .expect("parses");
        assert_eq!(lambda, Lambda::PINNED);
        assert_eq!(lambda.numerator(), 312_699);
        assert_eq!(lambda.denominator(), 1_000_000);
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

    #[test]
    fn absent_line_is_none() {
        assert_eq!(parse_table_conf_lambda("binary format version:7\n"), None);
    }
}
