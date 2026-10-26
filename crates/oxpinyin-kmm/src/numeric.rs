//! Complete decimal fields and fixed-precision tool output.

use crate::KmmError;

/// Parses one complete decimal token or count into its domain type.
///
/// # Errors
///
/// A non-UTF-8, malformed, or out-of-range field is a line error.
pub fn decimal_field<T: std::str::FromStr>(field: &[u8]) -> Result<T, KmmError> {
    std::str::from_utf8(field)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| KmmError::Malformed {
            detail: format!("invalid decimal field {:?}", String::from_utf8_lossy(field)),
        })
}

/// Six decimal places for finite values; Rust's native NaN/infinity spelling.
#[must_use]
pub fn format_six_decimals(value: f64) -> String {
    format!("{value:.6}")
}

#[cfg(test)]
mod tests {
    use super::{decimal_field, format_six_decimals};

    #[test]
    fn decimal_fields_are_complete_and_in_range() {
        assert_eq!(decimal_field::<u32>(b"+0010").unwrap(), 10);
        assert_eq!(decimal_field::<i64>(b"-5").unwrap(), -5);
        for field in [
            b"12abc".as_slice(),
            b"abc",
            b"",
            b" 5",
            b"5\0",
            b"4294967296",
            b"-1",
        ] {
            assert!(decimal_field::<u32>(field).is_err(), "{field:?}");
        }
        assert!(decimal_field::<i64>(b"9223372036854775808").is_err());
    }

    #[test]
    fn fixed_precision_uses_native_special_values() {
        assert_eq!(format_six_decimals(0.5), "0.500000");
        assert_eq!(format_six_decimals(-0.0), "-0.000000");
        assert_eq!(format_six_decimals(f64::NAN), "NaN");
        assert_eq!(format_six_decimals(-f64::NAN), "NaN");
        assert_eq!(format_six_decimals(f64::INFINITY), "inf");
    }
}
