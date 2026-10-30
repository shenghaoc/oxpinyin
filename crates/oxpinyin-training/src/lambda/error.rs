//! Errors the λ estimator reports. Existing typed refusals are preserved
//! under constitution §4's pin-matched failure policy.

use std::fmt;

use crate::counter::CounterError;

/// Why λ estimation failed.
#[derive(Debug)]
pub enum LambdaError {
    /// Parsing the held-out ngseg stream failed.
    Count(CounterError),
    /// The held-out (`DELETED_BIGRAM`) counts are empty, so the outer
    /// average `lambda_sum / lambda_count` would divide by zero
    /// (`estimate_interpolation.cpp:139`). The tool uses Rust's NaN spelling;
    /// estimation reports this error to the pipeline.
    EmptyDeleted,
}

impl fmt::Display for LambdaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Count(error) => write!(formatter, "held-out counting failed: {error}"),
            Self::EmptyDeleted => write!(
                formatter,
                "no held-out bigram contexts: λ is undefined (0 contexts to average)"
            ),
        }
    }
}

impl std::error::Error for LambdaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Count(error) => Some(error),
            Self::EmptyDeleted => None,
        }
    }
}

impl From<CounterError> for LambdaError {
    fn from(error: CounterError) -> Self {
        Self::Count(error)
    }
}
