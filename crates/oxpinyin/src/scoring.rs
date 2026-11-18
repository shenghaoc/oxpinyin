//! The scoring error.
//!
//! [`ScoringError`] carries a backend failure as text.

/// Why scoring failed.
///
/// Backend failures arrive as text because the frozen `Dictionary` and
/// `LanguageModel` seams leave their `Error` types unbounded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoringError {
    /// The dictionary backend failed.
    Dictionary(String),
    /// The language model backend failed.
    LanguageModel(String),
}

impl core::fmt::Display for ScoringError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Dictionary(message) => write!(formatter, "dictionary error: {message}"),
            Self::LanguageModel(message) => write!(formatter, "language model error: {message}"),
        }
    }
}

impl std::error::Error for ScoringError {}
