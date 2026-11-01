//! Complete decimal fields and fixed-precision tool output.
//!
//! These are the former `oxpinyin-kmm/src/numeric.rs` helpers, moved here
//! when the KMM crate moved its working model onto the pin's DBM records:
//! the training stages that need them (counter, eval, segment and lambda)
//! are the only callers, and they should not depend on the KMM container.

use std::fmt;

/// A decimal field that the domain type refuses.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NumericError {
    /// What was wrong with the field.
    pub detail: String,
}

impl fmt::Display for NumericError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for NumericError {}

/// Parses one complete decimal token or count into its domain type.
///
/// # Errors
///
/// A non-UTF-8, malformed, or out-of-range field is a line error.
pub fn decimal_field<T: std::str::FromStr>(field: &[u8]) -> Result<T, NumericError> {
    std::str::from_utf8(field)
        .ok()
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| NumericError {
            detail: format!("invalid decimal field {:?}", String::from_utf8_lossy(field)),
        })
}

/// Six decimal places for finite values; Rust's native NaN/infinity spelling.
#[must_use]
pub fn format_six_decimals(value: f64) -> String {
    format!("{value:.6}")
}
