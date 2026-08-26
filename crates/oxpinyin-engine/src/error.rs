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
    /// A lookup offset sat one position past a leading separator run,
    /// which the zero-start walk cannot normalize away — `_check_offset`'s
    /// refusal at libpinyin@dbff264, answered as an error instead of the
    /// upstream abort.
    LookupOffsetPastSeparator {
        /// The offset the caller asked to look up at.
        offset: usize,
        /// Where the zero-start walk stopped.
        normalized: usize,
    },
    /// A lookup offset lay beyond the raw input's one-past-end position —
    /// upstream reads its matrix out of bounds there, so the range has no
    /// pinned behaviour and the engine refuses it.
    LookupOffsetOutOfRange {
        /// The offset the caller asked to look up at.
        offset: usize,
        /// The raw input length the offset may at most equal.
        len: usize,
    },
    /// A selection was requested from a window anchored before the
    /// composition offset — a stale cursor behind the selection, whose span
    /// would regress the consumed boundary. Rejected rather than
    /// reconciled; no frontend drives a backward selection.
    SelectionAnchorBeforeComposition {
        /// The window anchor the caller supplied.
        anchor: usize,
        /// The composition offset (the selected boundary) it precedes.
        composition: usize,
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
            Self::LookupOffsetPastSeparator { offset, normalized } => {
                write!(
                    formatter,
                    "lookup offset {offset} sits one past a leading separator \
                     run (normalized to {normalized})"
                )
            }
            Self::LookupOffsetOutOfRange { offset, len } => {
                write!(
                    formatter,
                    "lookup offset {offset} is out of range 0..={len}"
                )
            }
            Self::SelectionAnchorBeforeComposition {
                anchor,
                composition,
            } => {
                write!(
                    formatter,
                    "selection anchor {anchor} precedes the composition offset {composition}"
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
        assert_eq!(
            EngineError::LookupOffsetPastSeparator {
                offset: 1,
                normalized: 1
            }
            .to_string(),
            "lookup offset 1 sits one past a leading separator run (normalized to 1)"
        );
        assert_eq!(
            EngineError::LookupOffsetOutOfRange { offset: 9, len: 3 }.to_string(),
            "lookup offset 9 is out of range 0..=3"
        );
    }
}
