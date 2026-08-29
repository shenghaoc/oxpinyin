//! Errors the segmenter reports. Nothing here panics on caller input.

use std::fmt;
use std::io;
use std::path::PathBuf;

use oxpinyin_data::{InterpolationError, LmError, TableError};

/// Why constructing or running the segmenter failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum SegmentError {
    /// A system table could not be opened or read.
    Table(TableError),
    /// The system bigram could not be opened or parsed.
    LanguageModel(LmError),
    /// `interpolation2.text` could not be read or parsed.
    Interpolation(InterpolationError),
    /// A path could not be read.
    Io {
        /// Path that was opened.
        path: PathBuf,
        /// The underlying I/O error.
        source: io::Error,
    },
    /// A required input path was missing or incomplete.
    MissingPath {
        /// What the caller asked for.
        detail: String,
    },
    /// A configuration value could not be parsed.
    Config(String),
}

impl fmt::Display for SegmentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table(error) => write!(formatter, "table error: {error}"),
            Self::LanguageModel(error) => write!(formatter, "language model error: {error}"),
            Self::Interpolation(error) => write!(formatter, "interpolation2 error: {error}"),
            Self::Io { path, source } => write!(formatter, "cannot read {path:?}: {source}"),
            Self::MissingPath { detail } => write!(formatter, "{detail}"),
            Self::Config(detail) => write!(formatter, "{detail}"),
        }
    }
}

impl std::error::Error for SegmentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Table(error) => Some(error),
            Self::LanguageModel(error) => Some(error),
            Self::Interpolation(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::MissingPath { .. } | Self::Config(_) => None,
        }
    }
}

impl From<TableError> for SegmentError {
    fn from(error: TableError) -> Self {
        Self::Table(error)
    }
}

impl From<LmError> for SegmentError {
    fn from(error: LmError) -> Self {
        Self::LanguageModel(error)
    }
}

impl From<InterpolationError> for SegmentError {
    fn from(error: InterpolationError) -> Self {
        Self::Interpolation(error)
    }
}
