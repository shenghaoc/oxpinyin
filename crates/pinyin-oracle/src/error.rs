//! Failure modes of the oracle harness.

use core::fmt;
use std::path::PathBuf;

/// Anything that can go wrong while verifying or driving the pinned oracle.
///
/// Every variant is a reported failure rather than a panic: the harness must
/// survive a missing prefix, an off-pin prefix, and a hostile oracle response.
#[derive(Debug)]
#[non_exhaustive]
pub enum OracleError {
    /// The pin manifest could not be read from the prefix.
    ManifestUnreadable {
        /// Path the harness attempted to read.
        path: PathBuf,
        /// Underlying I/O failure.
        source: std::io::Error,
    },
    /// The pin manifest was not valid UTF-8.
    ManifestNotUtf8 {
        /// Path that produced the invalid bytes.
        path: PathBuf,
    },
    /// The pin manifest omitted a field the harness requires.
    ManifestFieldMissing {
        /// Field name that was absent.
        field: &'static str,
    },
    /// The pin manifest carried a field the harness requires to differ.
    ManifestFieldMismatch {
        /// Field name that disagreed.
        field: &'static str,
        /// Value the frozen pin requires.
        expected: String,
        /// Value found in the manifest.
        found: String,
    },
    /// The pin manifest repeated a field, so its value is ambiguous.
    ManifestFieldRepeated {
        /// Field name that appeared more than once.
        field: String,
    },
    /// A prefix-relative artefact required by the pin was absent.
    PrefixArtefactMissing {
        /// Path the harness expected to exist.
        path: PathBuf,
    },
    /// No pin-verified oracle prefix could be located.
    PrefixNotFound {
        /// Candidate roots that were examined, in order.
        tried: Vec<PathBuf>,
    },
    /// A path or input string contained an interior NUL byte.
    InteriorNul {
        /// Which value carried the NUL.
        what: &'static str,
    },
    /// A path could not be represented as a C string for the FFI boundary.
    PathNotRepresentable {
        /// Offending path.
        path: PathBuf,
    },
    /// `pinyin_init` returned NULL.
    ContextInitFailed {
        /// System data directory passed to the oracle.
        system_dir: PathBuf,
        /// Fresh user directory passed to the oracle.
        user_dir: PathBuf,
    },
    /// `pinyin_alloc_instance` returned NULL.
    InstanceAllocFailed,
    /// The parity protocol rejects `DYNAMIC_ADJUST`; it is never silently masked.
    DynamicAdjustRejected {
        /// Flag word that carried the forbidden bit.
        flags: u32,
    },
    /// A libpinyin entry point reported failure.
    Call {
        /// Name of the C function that returned false.
        function: &'static str,
    },
    /// The oracle produced a string that was not valid UTF-8.
    NonUtf8 {
        /// Name of the C function that produced the bytes.
        function: &'static str,
    },
    /// The oracle reported a parsed prefix longer than the input it was given.
    ParsedLengthOutOfRange {
        /// Length the oracle reported.
        parsed: usize,
        /// Length of the input actually supplied.
        input_len: usize,
    },
    /// The harness was built without the `oracle-ffi` feature.
    FfiNotCompiled,
    /// A capture record omitted a field the reader requires.
    CaptureFieldMissing {
        /// Field name that was absent.
        field: &'static str,
    },
    /// A capture record carried a field the reader could not decode.
    CaptureFieldMalformed {
        /// Field name that failed to decode.
        field: &'static str,
        /// Raw value as it appeared in the record.
        value: String,
    },
    /// A capture record could not be decoded, with its position in the file.
    CaptureRecordInvalid {
        /// One-based line number within the fixture.
        line: usize,
        /// Underlying decoding failure.
        source: Box<OracleError>,
    },
}

impl fmt::Display for OracleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ManifestUnreadable { path, source } => {
                write!(
                    formatter,
                    "cannot read oracle pin manifest {path:?}: {source}"
                )
            }
            Self::ManifestNotUtf8 { path } => {
                write!(formatter, "oracle pin manifest {path:?} is not valid UTF-8")
            }
            Self::ManifestFieldMissing { field } => {
                write!(formatter, "oracle pin manifest omits field {field}")
            }
            Self::ManifestFieldMismatch {
                field,
                expected,
                found,
            } => write!(
                formatter,
                "oracle pin manifest field {field} is {found:?}, expected {expected:?}"
            ),
            Self::ManifestFieldRepeated { field } => {
                write!(formatter, "oracle pin manifest repeats field {field}")
            }
            Self::PrefixArtefactMissing { path } => {
                write!(formatter, "oracle prefix is missing {path:?}")
            }
            Self::PrefixNotFound { tried } => write!(
                formatter,
                "no pin-verified oracle prefix found (tried {tried:?}); \
                 build one with tools/oracle/build-oracle.sh"
            ),
            Self::InteriorNul { what } => {
                write!(formatter, "{what} contains an interior NUL byte")
            }
            Self::PathNotRepresentable { path } => {
                write!(formatter, "path {path:?} cannot cross the C boundary")
            }
            Self::ContextInitFailed {
                system_dir,
                user_dir,
            } => write!(
                formatter,
                "pinyin_init failed for system {system_dir:?} and user {user_dir:?}"
            ),
            Self::InstanceAllocFailed => formatter.write_str("pinyin_alloc_instance failed"),
            Self::DynamicAdjustRejected { flags } => write!(
                formatter,
                "flag word {flags:#010x} sets DYNAMIC_ADJUST, which the parity protocol rejects"
            ),
            Self::Call { function } => write!(formatter, "{function} reported failure"),
            Self::NonUtf8 { function } => {
                write!(formatter, "{function} returned a non-UTF-8 string")
            }
            Self::ParsedLengthOutOfRange { parsed, input_len } => write!(
                formatter,
                "oracle reported parsed length {parsed} for a {input_len}-byte input"
            ),
            Self::FfiNotCompiled => formatter
                .write_str("pinyin-oracle was built without the `oracle-ffi` cargo feature"),
            Self::CaptureFieldMissing { field } => {
                write!(formatter, "capture record omits field {field}")
            }
            Self::CaptureFieldMalformed { field, value } => {
                write!(formatter, "capture field {field} is malformed: {value:?}")
            }
            Self::CaptureRecordInvalid { line, source } => {
                write!(formatter, "capture record at line {line}: {source}")
            }
        }
    }
}

impl std::error::Error for OracleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ManifestUnreadable { source, .. } => Some(source),
            _ => None,
        }
    }
}
