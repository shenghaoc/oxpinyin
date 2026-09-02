//! model20 → a libpinyin system data directory, compiled natively from the
//! canonical linguistic source for every storage backend.
//!
//! # Architecture
//!
//! The canonical source of truth is the pinned `model20.text.tar.gz`
//! archive (`docs/findings/model-provenance.md`; fetched and verified by
//! `tools/model/fetch-model.sh`). Every producer consumes that archive
//! **directly** — no producer may take libpinyin-generated runtime data as
//! its input:
//!
//! ```text
//! pinned model20 ──► libpinyin's own build ──► libpinyin's data dir ─┐
//!        │                                                            ├─► differential
//!        ├──► oxpinyin-datagen (kyotocabinet, default) ► KC data dir ─┤   (same files)
//!        ├──► oxpinyin-datagen (tkrzw) ─────────────────► Tkrzw data dir┤
//!        ├──► oxpinyin-datagen (redb) ──────────────────► redb data dir ┤   (same records,
//!        └──► oxpinyin-datagen (lmdb) ──────────────────► LMDB data dir ┘    own container)
//! ```
//!
//! One semantic read pass ([`system::read_semantic`]) and one set of
//! serializers implementing libpinyin's own formats — the byte-level
//! output of its `gen_binary_files` + `import_interpolation` +
//! `gen_unigram` chain (`data/Makefile.am`):
//!
//! * the sixteen per-library chunk files (`MemoryChunk` +
//!   `SubPhraseIndex::store`, [`chunks`]) — byte-exact against the pin;
//! * `pinyin_index.bin` / `addon_pinyin_index.bin` (`ChewingLargeTable2`,
//!   [`libpinyin::pinyin_index_entries`]) and `phrase_index.bin` /
//!   `addon_phrase_index.bin` (`PhraseLargeTable3`,
//!   [`libpinyin::phrase_index_entries`]);
//! * `bigram.db` (`Bigram`, [`system::compile`]) and `punct.bin`
//!   (`PunctTable`, [`punct::compile`]);
//! * `table.conf`.
//!
//! The four backends are instantiations of `oxpinyin-store`'s
//! [`WriteStore`]: the same rows through each container. On Kyoto Cabinet
//! and tkrzw the result is the file set a libpinyin build of that DBM
//! ships, name for name; a libpinyin runtime opens it and so does
//! oxpinyin's. There is no conversion layer between the two
//! implementations: each compiles the text.
//!
//! Verification: `tests/libpinyin_parity.rs` compares the output with a
//! pin-built data directory record by record (and the chunk files byte
//! by byte), on the pinned model20 and on the toned mini model under
//! `fixtures/datagen-toned/`
//! (`tools/datagen/libpinyin-drop-in-differential.sh`);
//! `docs/findings/datagen-compat-2026-09-01.md` carries the findings.
//!
//! # Determinism
//!
//! Entries are emitted in ascending key-byte order with the upstream value
//! layouts, so repeated runs over the same archive produce identical row
//! streams (the on-disk container byte layout depends on the writing
//! DBM's own conventions — and, for the pinyin index, on struct padding
//! upstream leaves uninitialized and this crate zeroes).
//!
//! This crate is data-prep tooling for packagers, CI, and differentials; it
//! never ships in an oxpinyin installation.
//!
//! # Placement
//!
//! This is the counterpart of libpinyin's build-time data tools
//! (`utils/storage/gen_binary_files`, `utils/storage/import_interpolation`,
//! `utils/training/gen_unigram`) — compiled from the source tree but not
//! part of the shipped library. The runtime reader is `oxpinyin-data`
//! (the library-side DB readers); new-model training is the W9 crate
//! chain (equivalent of the separate trainer repo).

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
pub mod chunks;
pub mod libpinyin;
pub mod manifest;
pub mod punct;
pub mod system;
pub mod table;
pub mod write;

/// One DBM's rows: `(key, value)` pairs in ascending key-byte order — the
/// physical order of every tree container and the order every raw walk
/// reads back.
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
