//! Errors the KMM tools report. Nothing here panics on caller input.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Why a KMM operation failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum KmmError {
    /// A path could not be read or written.
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// A segmented-corpus or KMM-text line was malformed.
    Malformed {
        /// What was wrong.
        detail: String,
    },
    /// A KMM model failed a validation invariant
    /// (`validate_k_mixture_model`).
    Invalid {
        /// Which invariant was violated.
        detail: String,
    },
    /// A pruning computation produced an out-of-range probability
    /// (`prune_k_mixture_model` `EDOM`).
    Domain {
        /// Diagnostic context.
        detail: String,
    },
    /// A phrase-text table could not be opened (export/import).
    Lexicon {
        /// Diagnostic context.
        detail: String,
    },
}

impl fmt::Display for KmmError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "cannot access {path:?}: {source}"),
            Self::Malformed { detail } => write!(formatter, "malformed input: {detail}"),
            Self::Invalid { detail } => write!(formatter, "invalid k mixture model: {detail}"),
            Self::Domain { detail } => write!(formatter, "prune domain error: {detail}"),
            Self::Lexicon { detail } => write!(formatter, "phrase table: {detail}"),
        }
    }
}

impl std::error::Error for KmmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
