//! Errors the punctuation tool reports. Nothing panics on caller input.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Why a punctuation operation failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum PunctError {
    /// A path could not be read or written.
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// A segmented-corpus or punctuation-table line was malformed.
    Malformed {
        /// What was wrong.
        detail: String,
    },
}

impl fmt::Display for PunctError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "cannot access {path:?}: {source}"),
            Self::Malformed { detail } => write!(formatter, "malformed input: {detail}"),
        }
    }
}

impl std::error::Error for PunctError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Malformed { .. } => None,
        }
    }
}
