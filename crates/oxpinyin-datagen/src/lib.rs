//! model20 → oxpinyin runtime tables, compiled natively from the canonical
//! linguistic source for every storage backend.
//!
//! # Architecture
//!
//! The canonical source of truth is the pinned `model20.text.tar.gz`
//! archive (`docs/findings/model-provenance.md`; fetched and verified by
//! `tools/model/fetch-model.sh`). Every runtime-data producer consumes that
//! archive **directly** — no producer may take libpinyin-generated runtime
//! data as its input:
//!
//! ```text
//! pinned model20 ──► libpinyin's own build ──► libpinyin tables ─┐
//!        │                                                        ├─► differential
//!        ├──► oxpinyin-datagen (redb)  ──► redb tables ──────────┤
//!        ├──► oxpinyin-datagen (lmdb)  ──► LMDB tables ──────────┤
//!        └──► oxpinyin-datagen (tkrzw) ──► Tkrzw tables ─────────┘
//! ```
//!
//! This replaces the retired `oxpinyin-migrate` route, which exported the
//! dictionary through the pin-built oracle's C ABI and copied the bigram
//! verbatim from the oracle's `bigram.db`
//! (`docs/findings/data-layer-export.md` — that route tested migration
//! compatibility, not implementation parity). The native derivation here is
//! the same arithmetic libpinyin's own `data/Makefile.am` performs:
//!
//! * `pinyin_index` + `phrase_index` — the four system `.table` files
//!   (`gb_char`, `gbk_char`, `opengram`, `merged`), rows
//!   `pinyin phrase token count`, exactly as
//!   `FacadePhraseIndex::load_text` reads them and the public-ABI export
//!   iterator prints them.
//! * `bigram` — the `\2-gram` section of `interpolation2.text`, grouped by
//!   first token with `total == Σ count`, exactly as
//!   `import_interpolation` stores it.
//! * addon tables — the twelve topic `.table` files; `punct` —
//!   `punct.table`.
//!
//! Byte-exact equivalence of this native compilation with the frozen
//! oracle-derived export was measured on the pinned model (all 138,096
//! phrase rows, 93,349 pinyin keys, and 56,359 bigram entries identical);
//! see `docs/findings/datagen-model20.md` and the crate's tests.
//!
//! # Determinism
//!
//! Entries are emitted in ascending key-byte order with frozen value
//! layouts, so repeated runs over the same archive produce identical tables
//! (byte-identical for redb; key/value-stream identical across backends).
//!
//! This crate is data-prep tooling for packagers, CI, and differentials; it
//! never ships in an oxpinyin installation.
//!
//! # Placement
//!
//! This is the counterpart of libpinyin's build-time data tools
//! (`utils/storage/gen_binary_files`, `utils/storage/import_interpolation`,
//! `utils/training/gen_unigram`) — compiled from the source tree but not
//! part of the shipped library. The runtime loader is `oxpinyin-data`
//! (equivalent of libpinyin's library-side DB readers); new-model training
//! is the W9 crate chain (equivalent of the separate trainer repo). The
//! three backends are instantiations of `oxpinyin-store`'s [`WriteStore`]:
//! one linguistic model, three storage containers, no algorithm
//! duplication. See `docs/findings/datagen-model20.md` for the full
//! capability map and the equivalence evidence.

// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![forbid(unsafe_code)]
use std::fmt;

pub mod addon;
pub mod manifest;
pub mod punct;
pub mod system;
pub mod table;
pub mod write;

/// One compiled table: `(key, value)` pairs in the frozen writer order —
/// the exact insertion sequence of the retired `oxpinyin-migrate` writers,
/// which redb file byte-identity depends on (string-keyed tables and the
/// bigram in ascending key-byte order; token-keyed dictionary, addon, and
/// punctuation tables in integer token order). Reading a table back through
/// any store always yields ascending key-byte order regardless.
pub type Entries = Vec<(Vec<u8>, Vec<u8>)>;

/// Errors from compiling model20 text or writing a runtime table.
#[derive(Debug)]
#[non_exhaustive]
pub enum DatagenError {
    /// An I/O failure reading the model or writing the output.
    Io(std::io::Error),
    /// A storage-backend failure while writing or verifying a table.
    Store(oxpinyin_store::StoreError),
    /// The model directory is missing required files.
    MissingModel {
        /// The model directory that was inspected.
        dir: std::path::PathBuf,
        /// The file that was expected.
        file: &'static str,
    },
    /// A model text line does not parse.
    Parse {
        /// The file being parsed.
        path: std::path::PathBuf,
        /// 1-based line number.
        line: usize,
        /// What was wrong.
        message: String,
    },
    /// The model text contradicts the tables (token/word mismatch,
    /// duplicate 2-gram pair, u32 overflow, unsorted rows).
    Consistency(String),
}

impl fmt::Display for DatagenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "datagen I/O error: {e}"),
            Self::Store(e) => write!(f, "datagen store error: {e}"),
            Self::MissingModel { dir, file } => {
                write!(f, "model cache {} is missing {file}", dir.display())
            }
            Self::Parse {
                path,
                line,
                message,
            } => {
                write!(f, "{}:{line}: {message}", path.display())
            }
            Self::Consistency(message) => write!(f, "model consistency error: {message}"),
        }
    }
}

impl std::error::Error for DatagenError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Store(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for DatagenError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<oxpinyin_store::StoreError> for DatagenError {
    fn from(e: oxpinyin_store::StoreError) -> Self {
        Self::Store(e)
    }
}

/// FNV-1a 64-bit, dependency-free and deterministic across platforms.
///
/// Same construction as the W9 training manifests
/// (`fixtures/w9/*.manifest`): a change-detection fingerprint, not a
/// cryptographic digest.
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::fnv1a64;

    #[test]
    fn fnv_matches_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
