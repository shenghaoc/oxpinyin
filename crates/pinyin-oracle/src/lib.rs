//! Dev-only. Links the pin-built libpinyin at the P0-0 pin; meaningless
//! off-pin. Fresh state + learning off is the parity protocol.
//!
//! The oracle is **only** the prefix produced by
//! `tools/oracle/build-oracle.sh`. A distribution-provided libpinyin never
//! qualifies: [`pin`] refuses any prefix whose `oracle-pin.txt` does not carry
//! [`EXPECTED_PIN_REF`], and `build.rs` applies the same check before emitting
//! link flags.
//!
//! # Layering
//!
//! Everything except [`live`] is pure and always compiled, so the protocol rules
//! stay under test on every platform with no oracle present:
//!
//! - [`pin`] decides whether a prefix is the frozen oracle;
//! - [`flags`] holds the option words and rejects `DYNAMIC_ADJUST`;
//! - [`observation`] is what the oracle reported for one input;
//! - [`live`] is the FFI producer, behind the non-default `oracle-ffi` feature.
//!
//! Because a replayed capture fixture and a live FFI run both yield an
//! [`OracleObservation`], the comparison logic can be exercised in portable CI
//! against `fixtures/foundation/f-a.txt` and then run unchanged against the real
//! oracle on Linux.
//!
//! # Building against the oracle
//!
//! ```bash
//! bash tools/oracle/build-oracle.sh --prefix "$HOME/.local/opt/pinyin-oracle" --jobs "$(nproc)"
//!
//! PKG_CONFIG_PATH=$HOME/.local/opt/pinyin-oracle/lib/pkgconfig \
//! LD_LIBRARY_PATH=$HOME/.local/opt/pinyin-oracle/lib \
//! cargo test -p pinyin-oracle --features oracle-ffi
//! ```
#![warn(missing_docs)]

pub mod capture;
pub mod corpus;
pub mod differential;
mod error;
pub mod flags;
pub mod observation;
pub mod pin;
mod pin_ref;
pub mod taxonomy;

#[cfg(feature = "oracle-ffi")]
mod ffi;
#[cfg(feature = "oracle-ffi")]
pub mod live;

pub use error::OracleError;
pub use flags::OracleFlags;
pub use observation::{
    CandidateInfo, MAX_CAPTURED_CANDIDATES, OracleCandidateType, OracleCompleteness,
    OracleObservation, OracleSegment,
};
pub use pin::{EXPECTED_PIN_REF, OraclePrefix, PinManifest, VerifiedPin};
pub use taxonomy::{BudgetVerdict, DivergenceClass, Taxonomy};

#[cfg(feature = "oracle-ffi")]
pub use live::{ExportedPhrase, LiveSource, Oracle, Session};

/// Whether this build can drive a live oracle.
///
/// `false` means the crate was built without the `oracle-ffi` feature, so only
/// the pure layers are available.
#[must_use]
pub const fn ffi_available() -> bool {
    cfg!(feature = "oracle-ffi")
}

/// Default oracle prefix used when no environment override is present.
///
/// Matches the prefix in the crate-level build instructions.
pub const DEFAULT_PREFIX_SUFFIX: &str = ".local/opt/pinyin-oracle";

/// Environment variable naming the oracle prefix explicitly.
pub const PREFIX_ENV: &str = "PINYIN_ORACLE_PREFIX";
