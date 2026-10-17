//! The error type of the benchmark.

use core::fmt;
use std::io;
use std::path::PathBuf;

/// Why a step failed. A failed step is never recorded as a (possibly empty)
/// success: every variant ends the sample or the run.
#[derive(Debug)]
#[non_exhaustive]
pub enum BenchError {
    /// A filesystem or process operation failed.
    Io {
        /// What was being done.
        context: String,
        /// The underlying error.
        source: io::Error,
    },
    /// A record could not be encoded or decoded.
    Json(serde_json::Error),
    /// An input or a result violated a rule of the benchmark.
    Invalid(String),
    /// The host cannot do what was asked (never silently substituted).
    Unsupported(String),
    /// The loader refused the shared object.
    Library {
        /// The path handed to the loader.
        path: PathBuf,
        /// The loader's message.
        reason: String,
    },
    /// The library lacks symbols the benchmark needs.
    SymbolsMissing {
        /// The missing names.
        names: Vec<String>,
    },
    /// A library call reported failure where success is required.
    Call {
        /// The function.
        function: &'static str,
    },
    /// A child process failed.
    Child {
        /// The command.
        command: String,
        /// Exit status, `None` when killed by a signal.
        code: Option<i32>,
        /// Captured standard error, truncated.
        stderr: String,
    },
}

impl BenchError {
    /// An [`BenchError::Io`] with context.
    pub fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    /// A [`BenchError::Invalid`].
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}

impl fmt::Display for BenchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::Json(error) => write!(f, "record encoding: {error}"),
            Self::Invalid(message) => write!(f, "{message}"),
            Self::Unsupported(message) => write!(f, "unsupported on this host: {message}"),
            Self::Library { path, reason } => {
                write!(f, "cannot load {}: {reason}", path.display())
            }
            Self::SymbolsMissing { names } => {
                write!(
                    f,
                    "the library lacks symbols this benchmark needs: {}",
                    names.join(", ")
                )
            }
            Self::Call { function } => write!(f, "{function} reported failure"),
            Self::Child {
                command,
                code,
                stderr,
            } => match code {
                Some(code) => write!(f, "`{command}` exited with status {code}: {stderr}"),
                None => write!(f, "`{command}` was killed by a signal: {stderr}"),
            },
        }
    }
}

impl std::error::Error for BenchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for BenchError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
