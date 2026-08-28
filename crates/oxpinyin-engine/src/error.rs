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
    /// A lookup offset fell inside a multi-byte character of the raw
    /// input — no window exists under a mid-character slice, so the
    /// anchor is refused rather than rounded (rounding would silently
    /// answer a neighbouring offset's window).
    LookupOffsetInsideCharacter {
        /// The offset the caller asked to look up at.
        offset: usize,
        /// The raw input length the offset was range-checked against.
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
    /// A cursor normalization or word move examined an offset one past a
    /// lone zero-key matrix column — the shape the pin's `_check_offset`
    /// aborts on (`assert(zero_key != key)`, `pinyin.cpp:2175` at the pin)
    /// and post-`95e3af7` libpinyin answers with a `false` every caller
    /// discards (the function then completes with the computed value).
    /// The engine answers with an error instead, so the C surface returns
    /// `false` — the no-abort policy, diverging from both upstream arms
    /// (`docs/findings/upstream-divergences.md`).
    ZeroKeyOffsetCheck {
        /// The examined offset whose preceding column holds the lone zero key.
        offset: usize,
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
            Self::LookupOffsetInsideCharacter { offset, len } => {
                write!(
                    formatter,
                    "lookup offset {offset} falls inside a multi-byte character \
                     of the {len}-byte raw input"
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
            Self::ZeroKeyOffsetCheck { offset } => {
                write!(
                    formatter,
                    "offset {offset} sits one past a lone zero-key column"
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

impl From<GraphError> for EngineError {
    fn from(error: GraphError) -> Self {
        Self::Graph(error)
    }
}

impl From<DecodeError> for EngineError {
    fn from(error: DecodeError) -> Self {
        Self::Decode(error)
    }
}

impl From<ScoringError> for EngineError {
    fn from(error: ScoringError) -> Self {
        Self::Scoring(error)
    }
}

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
        assert_eq!(
            EngineError::LookupOffsetInsideCharacter { offset: 1, len: 8 }.to_string(),
            "lookup offset 1 falls inside a multi-byte character of the 8-byte raw input"
        );
        assert_eq!(
            EngineError::ZeroKeyOffsetCheck { offset: 11 }.to_string(),
            "offset 11 sits one past a lone zero-key column"
        );
    }

    #[test]
    fn core_errors_convert_at_the_engine_boundary() {
        use oxpinyin_core::graph::GraphError;
        use oxpinyin_core::kbest::DecodeError;
        use oxpinyin_core::scoring::ScoringError;

        let graph: EngineError = GraphError::InputTooLong { len: 1, limit: 0 }.into();
        assert!(matches!(graph, EngineError::Graph(_)));
        let decode: EngineError = DecodeError::KTooLarge { k: 99, limit: 8 }.into();
        assert!(matches!(decode, EngineError::Decode(_)));
        let scoring: EngineError = ScoringError::Dictionary("closed".to_owned()).into();
        assert!(matches!(scoring, EngineError::Scoring(_)));
    }
}
