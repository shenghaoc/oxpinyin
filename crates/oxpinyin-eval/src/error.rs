//! Errors the evaluator reports. Nothing panics on caller input.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Why evaluation failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum EvalError {
    /// A path could not be read.
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// A segmented or model line was malformed.
    Malformed {
        /// What was wrong.
        detail: String,
    },
    /// λ could not be estimated (no scorable held-out context).
    Lambda {
        /// Diagnostic context.
        detail: String,
    },
    /// A token in an evaluation sentence has no pronunciation, so it cannot
    /// be spelled into keys (`eval_correction_rate` would `abort`).
    NoPronunciation {
        /// The token that could not be spelled.
        token: u32,
    },
    /// A token in an evaluation sentence has no phrase text, so the expected
    /// sentence cannot be formed (`convert_to_utf8` would read an unset
    /// phrase item).
    NoText {
        /// The token without text.
        token: u32,
    },
    /// No phrase cover spells the sentence's key chain, so there is no best
    /// match to compare (`eval_correction_rate` asserts exactly one result).
    Undecodable {
        /// The number of keys in the chain.
        keys: usize,
    },
    /// A decode backend (dictionary or model) failed.
    Backend {
        /// Diagnostic context.
        detail: String,
    },
}

impl fmt::Display for EvalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "cannot access {path:?}: {source}"),
            Self::Malformed { detail } => write!(formatter, "malformed input: {detail}"),
            Self::Lambda { detail } => write!(formatter, "lambda estimation: {detail}"),
            Self::NoPronunciation { token } => {
                write!(formatter, "token {token} has no pronunciation")
            }
            Self::NoText { token } => write!(formatter, "token {token} has no phrase text"),
            Self::Undecodable { keys } => {
                write!(formatter, "no phrase cover spells the {keys}-key chain")
            }
            Self::Backend { detail } => write!(formatter, "decode backend: {detail}"),
        }
    }
}

impl std::error::Error for EvalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
