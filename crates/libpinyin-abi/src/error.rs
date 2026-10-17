//! Failure modes of driving a loaded libpinyin or libzhuyin.

use core::fmt;
use std::path::PathBuf;

use crate::subject::Facade;

/// Anything that can go wrong while loading a subject or calling through it.
///
/// Every variant is a reported failure rather than a panic: a subject is a
/// program under test, so a missing symbol, a refused call or a hostile answer
/// must be an observation, not a crash of the caller.
#[derive(Debug)]
#[non_exhaustive]
pub enum AbiError {
    /// The shared object could not be loaded.
    LibraryLoad {
        /// Path handed to the loader.
        path: PathBuf,
        /// The loader's own message.
        reason: String,
    },
    /// The operation needs an exported symbol the loaded library does not have.
    ///
    /// This is a statement about the library, not a rejection of it: a revision
    /// that lacks a symbol of another revision is still a usable subject for
    /// every operation it does export.
    SymbolMissing {
        /// The symbol the operation needs.
        name: &'static str,
    },
    /// A subject is already open in this process.
    ///
    /// Differential runs load one library per process; opening a second one in
    /// the same process is an explicit choice (`Subject::open_additional`),
    /// never the default.
    SubjectAlreadyOpen,
    /// A session of the other facade was requested.
    FacadeMismatch {
        /// Facade of the context.
        context: Facade,
        /// Facade the operation belongs to.
        requested: Facade,
    },
    /// A path or input carried an interior NUL byte.
    InteriorNul {
        /// Which value carried the NUL.
        what: &'static str,
    },
    /// A path could not be passed across the C boundary as a UTF-8 string.
    PathNotRepresentable {
        /// Offending path.
        path: PathBuf,
    },
    /// `*_init` returned NULL.
    ContextInitFailed {
        /// System data directory passed to the library.
        system_dir: PathBuf,
        /// User directory passed to the library.
        user_dir: PathBuf,
    },
    /// `*_alloc_instance` returned NULL.
    InstanceAllocFailed,
    /// A library entry point reported failure.
    Call {
        /// Name of the C function that returned false (or a NULL out-value).
        function: &'static str,
    },
    /// The library produced a string that was not valid UTF-8.
    NonUtf8 {
        /// Name of the C function that produced the bytes.
        function: &'static str,
    },
}

impl fmt::Display for AbiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibraryLoad { path, reason } => {
                write!(formatter, "cannot load {}: {reason}", path.display())
            }
            Self::SymbolMissing { name } => {
                write!(formatter, "the loaded library does not export {name}")
            }
            Self::SubjectAlreadyOpen => formatter.write_str(
                "a subject is already open in this process; differential runs load one \
                 library per process (use Subject::open_additional to opt out)",
            ),
            Self::FacadeMismatch { context, requested } => write!(
                formatter,
                "a {requested:?} session was requested on a {context:?} context"
            ),
            Self::InteriorNul { what } => {
                write!(formatter, "{what} contains an interior NUL byte")
            }
            Self::PathNotRepresentable { path } => write!(
                formatter,
                "path {} cannot cross the C boundary as UTF-8",
                path.display()
            ),
            Self::ContextInitFailed {
                system_dir,
                user_dir,
            } => write!(
                formatter,
                "init failed for system {} and user {}",
                system_dir.display(),
                user_dir.display()
            ),
            Self::InstanceAllocFailed => formatter.write_str("alloc_instance failed"),
            Self::Call { function } => write!(formatter, "{function} reported failure"),
            Self::NonUtf8 { function } => {
                write!(formatter, "{function} returned a non-UTF-8 string")
            }
        }
    }
}

impl std::error::Error for AbiError {}
