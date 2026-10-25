//! The decimal conversions the pin's training tools apply to record fields.
//!
//! `TAGLIB_PARSE_SEGMENTED_LINE` and `TAGLIB_GET_TOKEN` (`utils_helper.h:23-35,
//! :57-69`) read a token with `atoi`; `TAGLIB_GET_TAGVALUE(glong, ..., atol)`
//! reads a count. Both are `strtol(nptr, NULL, 10)` (glibc 2.41
//! `stdlib/atoi.c:27`, `stdlib/atol.c:27`, and the inline forms at
//! `stdlib/stdlib.h:481-489`), so this is `__strtol_l_internal`
//! (`stdlib/strtol_l.c:295-524`) at base 10 on an LP64 target:
//!
//! - leading C white space is skipped (`:304`): TAB, LF, VT, FF, CR and
//!   SPACE. The tools call `setlocale(LC_ALL, "")`, so the table is the
//!   locale's; the C locale's is these six bytes (`locale/C-ctype.c:366-369`)
//!   and no byte above 0x7f is white space, as in a UTF-8 locale;
//! - one optional `+` or `-` follows (`:311-317`); a sign alone, no digit, or
//!   an empty field converts nothing and gives 0 (`:306`, `:495`, `:526-545`);
//! - decimal digits are consumed up to the first other byte, which is not an
//!   error (`:449-492`); there is no grouping (`group` is 0, `:108`);
//! - a value beyond the `long` range gives `LONG_MAX` or `LONG_MIN`
//!   (`:503-521`), and the magnitude `2^63` is `LONG_MIN` when negated
//!   (`:507-509`, `:524`).
//!
//! `atoi` then narrows the `long` to `int` (`atoi.c:27`) and the macros store
//! that in a `phrase_token_t` (`guint32`): the low 32 bits.

/// The bytes `isspace` accepts in the C locale (`locale/C-ctype.c:366-369`).
const fn is_c_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b' ')
}

/// `strtol(field, NULL, 10)` on the bytes of one field: the part of `field`
/// before its first NUL is the C string.
#[must_use]
pub fn atol(field: &[u8]) -> i64 {
    let text = field.split(|byte| *byte == 0).next().unwrap_or_default();
    let start = text
        .iter()
        .position(|byte| !is_c_space(*byte))
        .unwrap_or(text.len());
    let text = &text[start..];
    let (negative, text) = match text.first() {
        Some(b'-') => (true, &text[1..]),
        Some(b'+') => (false, &text[1..]),
        _ => (false, text),
    };
    let mut magnitude = Some(0_u64);
    for digit in text.iter().take_while(|byte| byte.is_ascii_digit()) {
        magnitude = magnitude.and_then(|value| {
            value
                .checked_mul(10)
                .and_then(|value| value.checked_add(u64::from(digit - b'0')))
        });
    }
    let limit = if negative {
        i64::MIN.unsigned_abs()
    } else {
        i64::MAX.unsigned_abs()
    };
    match magnitude {
        Some(value) if value <= limit => {
            // For the negative limit this is `2^63 as i64`, which negates to
            // itself: `LONG_MIN`.
            let value = value as i64;
            if negative {
                value.wrapping_neg()
            } else {
                value
            }
        }
        _ if negative => i64::MIN,
        _ => i64::MAX,
    }
}

/// `atoi(field)` stored in a `phrase_token_t`: the low 32 bits of
/// [`atol`], as the `int` cast in `atoi` and the `guint32` store keep them.
#[must_use]
pub fn atoi(field: &[u8]) -> u32 {
    atol(field) as u32
}

/// C `printf("%f")` rendering: six fixed decimals for a finite value, and
/// glibc's `nan`/`-nan` (sign bit honoured) and `inf`/`-inf`. The pin's
/// tools print a NaN when they divide zero by zero (`estimate_*` with no
/// scorable context, `eval_correction_rate` with no test); Rust's own `{:.6}`
/// would print `NaN`.
#[must_use]
pub fn printf_f(value: f64) -> String {
    if value.is_nan() {
        if value.is_sign_negative() {
            "-nan"
        } else {
            "nan"
        }
        .to_owned()
    } else if value.is_infinite() {
        if value.is_sign_negative() {
            "-inf"
        } else {
            "inf"
        }
        .to_owned()
    } else {
        format!("{value:.6}")
    }
}

#[cfg(test)]
mod tests {
    use super::{atoi, atol, printf_f};

    #[test]
    fn printf_f_matches_glibc_for_nan_inf_and_finite_values() {
        assert_eq!(printf_f(0.5), "0.500000");
        assert_eq!(printf_f(-0.0), "-0.000000");
        assert_eq!(printf_f(f64::INFINITY), "inf");
        assert_eq!(printf_f(f64::NEG_INFINITY), "-inf");
        assert_eq!(printf_f(f64::NAN.copysign(1.0)), "nan");
        assert_eq!(printf_f(f64::NAN.copysign(-1.0)), "-nan");
        // The upstream `lambda_sum / lambda_count` with both zero.
        let zero: f64 = "0".parse().expect("zero");
        assert_eq!(printf_f(zero / zero).trim_start_matches('-'), "nan");
    }

    #[test]
    fn plain_and_prefixed_decimals() {
        assert_eq!(atol(b"16817937"), 16_817_937);
        assert_eq!(atol(b"0"), 0);
        assert_eq!(atol(b""), 0);
        assert_eq!(atol(b"12abc"), 12);
        assert_eq!(atol(b"abc"), 0);
        assert_eq!(atol(b"0x10"), 0);
        assert_eq!(atol(b"007"), 7);
    }

    #[test]
    fn white_space_and_sign() {
        assert_eq!(atol(b"+16817937"), 16_817_937);
        assert_eq!(atol(b"\r16817937"), 16_817_937);
        assert_eq!(atol(b" \t\n\x0b\x0c\r5"), 5);
        assert_eq!(atol(b"-5"), -5);
        assert_eq!(atol(b"-0"), 0);
        for none in [
            &b"+"[..],
            b"-",
            b"+-5",
            b"--5",
            b"- 5",
            b"+ 5",
            b" ",
            b"\xa05",
        ] {
            assert_eq!(atol(none), 0, "{none:?}");
        }
        // A sign is read once, after the white space and not before it.
        assert_eq!(atol(b"5-3"), 5);
        assert_eq!(atol(b"5 3"), 5);
    }

    #[test]
    fn the_field_ends_at_its_first_nul() {
        assert_eq!(atol(b"12\x0034"), 12);
        assert_eq!(atol(b"\x0034"), 0);
        assert_eq!(atol(b" \x0034"), 0);
    }

    #[test]
    fn saturation_at_the_long_range() {
        assert_eq!(atol(b"9223372036854775807"), i64::MAX);
        assert_eq!(atol(b"9223372036854775808"), i64::MAX);
        assert_eq!(atol(b"99999999999999999999999"), i64::MAX);
        assert_eq!(atol(b"-9223372036854775807"), -i64::MAX);
        assert_eq!(atol(b"-9223372036854775808"), i64::MIN);
        assert_eq!(atol(b"-9223372036854775809"), i64::MIN);
        assert_eq!(atol(b"-99999999999999999999999"), i64::MIN);
        // Digits after an overflow are still consumed, then ignored.
        assert_eq!(atol(b"18446744073709551616x"), i64::MAX);
        assert_eq!(atol(b"18446744073709551615"), i64::MAX);
    }

    #[test]
    fn tokens_keep_the_low_32_bits() {
        assert_eq!(atoi(b"16817937"), 16_817_937);
        assert_eq!(atoi(b"+16817937"), 16_817_937);
        assert_eq!(atoi(b"-1"), u32::MAX);
        assert_eq!(atoi(b"4294967295"), u32::MAX);
        assert_eq!(atoi(b"4294967296"), 0);
        assert_eq!(atoi(b"4311785233"), 16_817_937);
        assert_eq!(atoi(b"2147483648"), 2_147_483_648);
        assert_eq!(atoi(b"9223372036854775807"), u32::MAX);
        assert_eq!(atoi(b"9223372036854775808"), u32::MAX);
        assert_eq!(atoi(b"-9223372036854775808"), 0);
        assert_eq!(atoi(b"-9223372036854775809"), 0);
        assert_eq!(atoi(b"-4294967296"), 0);
        assert_eq!(atoi(b"-4294967295"), 1);
    }
}
