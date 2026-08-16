//! Engine failures.

use core::fmt;

use oxpinyin_core::graph::GraphError;
use oxpinyin_core::kbest::DecodeError;
use oxpinyin_core::scoring::ScoringError;

/// Anything a session can fail at.
///
/// `#[non_exhaustive]`, so later tasks add variants without breaking callers.
/// Backend failures arrive as text because the frozen `Dictionary` and
/// `LanguageModel` seams leave their `Error` types unbounded: the engine
/// renders them at the boundary rather than leaking an associated type into
/// its public surface.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum EngineError {
    /// A candidate index was out of range, most likely a stale one.
    CandidateIndexOutOfRange {
        /// The index the caller asked for.
        index: usize,
        /// How many candidates the list actually holds.
        len: usize,
    },
    /// The dictionary backend failed.
    Dictionary(String),
    /// The language model backend failed.
    LanguageModel(String),
    /// The user-model backend failed (the learning/observation seam).
    UserModel(String),
    /// The input could not be represented as a segment graph.
    Graph(GraphError),
    /// The k-best search refused its parameters.
    Decode(DecodeError),
    /// Scoring failed.
    Scoring(ScoringError),
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CandidateIndexOutOfRange { index, len } => {
                write!(
                    formatter,
                    "candidate index {index} is out of range 0..{len}"
                )
            }
            Self::Dictionary(message) => write!(formatter, "dictionary error: {message}"),
            Self::LanguageModel(message) => write!(formatter, "language model error: {message}"),
            Self::UserModel(message) => write!(formatter, "user model error: {message}"),
            Self::Graph(error) => write!(formatter, "graph error: {error}"),
            Self::Decode(error) => write!(formatter, "decode error: {error}"),
            Self::Scoring(error) => write!(formatter, "scoring error: {error}"),
        }
    }
}

impl std::error::Error for EngineError {}

#[cfg(test)]
mod tests {
    use super::EngineError;

    #[test]
    fn errors_render_the_offending_values() {
        let error = EngineError::CandidateIndexOutOfRange { index: 9, len: 3 };
        assert_eq!(error.to_string(), "candidate index 9 is out of range 0..3");
        assert_eq!(
            EngineError::Dictionary("closed".to_owned()).to_string(),
            "dictionary error: closed"
        );
        assert_eq!(
            EngineError::LanguageModel("closed".to_owned()).to_string(),
            "language model error: closed"
        );
    }
}
