//! Errors the word-recognizer reports. Nothing panics on caller input.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Why a word-recognition operation failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum WordError {
    /// A path could not be read or written.
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// A segmented-corpus or list line was malformed.
    Malformed {
        /// What was wrong.
        detail: String,
    },
    /// A recognized word referenced a phrase with no known pinyin.
    MissingPinyin {
        /// The phrase that could not be marked.
        phrase: String,
    },
}

impl fmt::Display for WordError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "cannot access {path:?}: {source}"),
            Self::Malformed { detail } => write!(formatter, "malformed input: {detail}"),
            Self::MissingPinyin { phrase } => {
                write!(formatter, "no pinyin for phrase {phrase:?}")
            }
        }
    }
}

impl std::error::Error for WordError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
