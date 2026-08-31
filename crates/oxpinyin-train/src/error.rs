//! Errors the trainer orchestrator reports. Nothing panics on caller input;
//! every stage surfaces a typed failure (requirement: failure reporting).

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Why a training stage failed.
#[derive(Debug)]
#[non_exhaustive]
pub enum TrainError {
    /// A path could not be read or written.
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying I/O error.
        source: io::Error,
    },
    /// A corpus index, status file, or model text was malformed.
    Malformed {
        /// What was wrong.
        detail: String,
    },
    /// A status file carried an epoch newer than this build understands
    /// (upstream's "un-excepted larger epoch"): the workflow was produced by
    /// a later trainer and must not be silently resumed.
    EpochTooNew {
        /// The stage whose epoch is too new.
        stage: &'static str,
        /// Epoch found in the status file.
        found: u32,
        /// Epoch this build signs.
        known: u32,
    },
    /// The segment stage failed.
    Segment {
        /// Diagnostic context.
        detail: String,
    },
    /// A KMM stage (generate/estimate/merge/validate/prune/export) failed.
    Kmm {
        /// Which stage.
        stage: &'static str,
        /// Diagnostic context.
        detail: String,
    },
    /// The evaluation stage failed (λ estimation, application, or decode).
    Eval {
        /// Diagnostic context.
        detail: String,
    },
    /// Not enough candidates to merge the requested top-N.
    NotEnoughCandidates {
        /// Candidates requested to merge.
        requested: usize,
        /// Candidates actually available.
        available: usize,
    },
    /// The sorted candidate index was not in descending score order — a
    /// corruption upstream's `mergeSomeModels` also refuses.
    ScoresNotDescending {
        /// The offending score.
        score: f64,
        /// The previous (smaller) score it followed.
        previous: f64,
    },
}

impl fmt::Display for TrainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(formatter, "cannot access {path:?}: {source}"),
            Self::Malformed { detail } => write!(formatter, "malformed input: {detail}"),
            Self::EpochTooNew {
                stage,
                found,
                known,
            } => write!(
                formatter,
                "{stage} epoch {found} is newer than this build's {known}; \
                 the workflow was produced by a later trainer"
            ),
            Self::Segment { detail } => write!(formatter, "segment stage: {detail}"),
            Self::Kmm { stage, detail } => write!(formatter, "kmm {stage}: {detail}"),
            Self::Eval { detail } => write!(formatter, "evaluate stage: {detail}"),
            Self::NotEnoughCandidates {
                requested,
                available,
            } => write!(
                formatter,
                "cannot merge top {requested}: only {available} candidate(s) available"
            ),
            Self::ScoresNotDescending { score, previous } => write!(
                formatter,
                "sorted candidate scores must be descending: {score} follows {previous}"
            ),
        }
    }
}

impl std::error::Error for TrainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl TrainError {
    /// An I/O failure against `path`.
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
