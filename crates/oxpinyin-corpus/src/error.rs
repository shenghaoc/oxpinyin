//! Error type for the corpus front-end.
//!
//! Every fallible path returns [`CorpusError`]; nothing panics on input.

use std::fmt;

/// A corpus-cleaning failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum CorpusError {
    /// An I/O read or write failed.
    Io(std::io::Error),
    /// The dump XML is malformed (truncated tag, missing text element).
    MalformedXml {
        /// Human-readable detail.
        detail: String,
    },
    /// The dump is not valid UTF-8 where text content was expected.
    InvalidUtf8 {
        /// Byte offset of the offending position.
        offset: usize,
    },
    /// The trad→simp converter (opencc) could not run or exited nonzero.
    Convert {
        /// Human-readable detail; the converter's own stderr is inherited,
        /// not captured.
        detail: String,
    },
}

impl fmt::Display for CorpusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::MalformedXml { detail } => write!(f, "malformed dump xml: {detail}"),
            Self::InvalidUtf8 { offset } => {
                write!(f, "invalid utf-8 in dump content at byte {offset}")
            }
            Self::Convert { detail } => write!(f, "trad-to-simp converter failed: {detail}"),
        }
    }
}

impl std::error::Error for CorpusError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for CorpusError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
