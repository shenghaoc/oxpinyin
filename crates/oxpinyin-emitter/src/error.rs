//! Errors the emitter reports. Nothing here panics on caller input.

use std::fmt;
use std::io;
use std::path::PathBuf;

use oxpinyin_counter::CounterError;
use oxpinyin_segment::SegmentError;

/// Why writing `interpolation2.text` failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum EmitterError {
    /// Opening the phrase index (the phrase-text source) failed.
    Segment(SegmentError),
    /// Counting the ngseg stream failed.
    Count(CounterError),
    /// A path could not be written or read.
    Io {
        /// Path that was opened.
        path: PathBuf,
        /// The underlying I/O error.
        source: io::Error,
    },
}

impl fmt::Display for EmitterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Segment(error) => write!(formatter, "phrase index error: {error}"),
            Self::Count(error) => write!(formatter, "count error: {error}"),
            Self::Io { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for EmitterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Segment(error) => Some(error),
            Self::Count(error) => Some(error),
            Self::Io { source, .. } => Some(source),
        }
    }
}

impl From<SegmentError> for EmitterError {
    fn from(error: SegmentError) -> Self {
        Self::Segment(error)
    }
}

impl From<CounterError> for EmitterError {
    fn from(error: CounterError) -> Self {
        Self::Count(error)
    }
}
