//! Errors the counter reports. Nothing here panics on caller input.

use std::fmt;
use std::io;
use std::path::PathBuf;

use oxpinyin_segment::SegmentError;

/// Why counting failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum CounterError {
    /// A segmented line did not split into exactly two `token phrase`
    /// fields (`TAGLIB_PARSE_SEGMENTED_LINE` aborts on this shape).
    MalformedLine {
        /// 1-based line number in the ngseg stream.
        line: usize,
    },
    /// Opening the phrase index (the seed-token source) failed.
    Segment(SegmentError),
    /// A path could not be read.
    Io {
        /// Path that was opened.
        path: PathBuf,
        /// The underlying I/O error.
        source: io::Error,
    },
}

impl fmt::Display for CounterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MalformedLine { line } => {
                write!(formatter, "segmented line {line} is not `token phrase`")
            }
            Self::Segment(error) => write!(formatter, "phrase index error: {error}"),
            Self::Io { path, source } => write!(formatter, "cannot read {path:?}: {source}"),
        }
    }
}

impl std::error::Error for CounterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Segment(error) => Some(error),
            Self::Io { source, .. } => Some(source),
            Self::MalformedLine { .. } => None,
        }
    }
}

impl From<SegmentError> for CounterError {
    fn from(error: SegmentError) -> Self {
        Self::Segment(error)
    }
}
