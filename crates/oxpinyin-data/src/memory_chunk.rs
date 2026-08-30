//! libpinyin `MemoryChunk` file reader — the 8-byte length+checksum wrapper
//! around every content-table `.bin` file a libpinyin install ships.
//!
//! Format (`src/include/memory_chunk.h`, byte-identical from 2.8.1 through
//! the 2.11.91 pin):
//!
//! ```text
//! bytes 0..4   u32 length    — byte count of the data section
//! bytes 4..8   u32 checksum  — XOR checksum over the data section
//! bytes 8..    data
//! ```
//!
//! The header words are written by a plain native-endian `write`; every
//! target libpinyin ships on is little-endian, and the checksum words are
//! explicitly little-endian in the pin (`get_check_sum`), so this reader
//! reads both as LE.
//!
//! Checksum (`memory_chunk.h::get_check_sum`, mirrored exactly): XOR of the
//! little-endian `u32` words over the 4-byte-aligned prefix, then each
//! remaining byte XORed in at a shift of `8 × (index mod 4)` bits.
//!
//! `load` enforces the pin's own acceptance rule: `length` must equal
//! `file size − 8` and the checksum must match. Any mismatch is an error
//! naming the file — never a silent fallback.

use std::fmt;
use std::path::{Path, PathBuf};

/// Why a `MemoryChunk` file was rejected.
#[derive(Debug)]
pub enum MemoryChunkError {
    /// The file could not be read.
    Io(PathBuf, std::io::Error),
    /// The file is shorter than the 8-byte header.
    TooShort(PathBuf, u64),
    /// The header length does not equal `file size − 8`.
    LengthMismatch {
        /// The offending file.
        path: PathBuf,
        /// The header's length word.
        header: u32,
        /// The actual data byte count (`file size − 8`).
        actual: u64,
    },
    /// The checksum over the data does not match the header.
    ChecksumMismatch {
        /// The offending file.
        path: PathBuf,
        /// The header's checksum word.
        header: u32,
        /// The checksum computed over the data.
        computed: u32,
    },
}

impl fmt::Display for MemoryChunkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(path, e) => write!(f, "{}: {e}", path.display()),
            Self::TooShort(path, size) => write!(
                f,
                "{}: {size} bytes is shorter than the 8-byte MemoryChunk header",
                path.display()
            ),
            Self::LengthMismatch {
                path,
                header,
                actual,
            } => write!(
                f,
                "{}: MemoryChunk header claims {header} data bytes but the file \
                 holds {actual}",
                path.display()
            ),
            Self::ChecksumMismatch {
                path,
                header,
                computed,
            } => write!(
                f,
                "{}: MemoryChunk checksum mismatch (header {header:#010x}, \
                 computed {computed:#010x})",
                path.display()
            ),
        }
    }
}

impl std::error::Error for MemoryChunkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(_, e) => Some(e),
            _ => None,
        }
    }
}

/// The pin's XOR checksum (`memory_chunk.h::get_check_sum`), exactly.
#[must_use]
pub fn check_sum(data: &[u8]) -> u32 {
    let mut checksum = 0u32;
    let aligned = data.len() & !0x3;
    for word in data[..aligned].chunks_exact(4) {
        checksum ^= u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
    }
    let mut shift = 0u32;
    for &byte in &data[aligned..] {
        checksum ^= u32::from(byte) << shift;
        shift += 8;
    }
    checksum
}

/// Loads and verifies a `MemoryChunk` file, returning its data section.
///
/// # Errors
///
/// [`MemoryChunkError`] on I/O failure, a short file, a length mismatch, or
/// a checksum mismatch — each naming the file.
pub fn load(path: &Path) -> Result<Vec<u8>, MemoryChunkError> {
    let bytes = std::fs::read(path).map_err(|e| MemoryChunkError::Io(path.to_path_buf(), e))?;
    if bytes.len() < 8 {
        return Err(MemoryChunkError::TooShort(
            path.to_path_buf(),
            bytes.len() as u64,
        ));
    }
    let header_len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let header_sum = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let data = &bytes[8..];
    if u64::from(header_len) != data.len() as u64 {
        return Err(MemoryChunkError::LengthMismatch {
            path: path.to_path_buf(),
            header: header_len,
            actual: data.len() as u64,
        });
    }
    let computed = check_sum(data);
    if computed != header_sum {
        return Err(MemoryChunkError::ChecksumMismatch {
            path: path.to_path_buf(),
            header: header_sum,
            computed,
        });
    }
    Ok(data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::{MemoryChunkError, check_sum, load};

    fn write_chunk(dir: &std::path::Path, name: &str, data: &[u8]) -> std::path::PathBuf {
        let mut bytes = Vec::with_capacity(8 + data.len());
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&check_sum(data).to_le_bytes());
        bytes.extend_from_slice(data);
        let path = dir.join(name);
        std::fs::write(&path, bytes).expect("write chunk");
        path
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-memory-chunk-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn a_good_file_round_trips() {
        let dir = temp_dir("good");
        // 4-byte-aligned body plus a 3-byte tail: both checksum arms run.
        let data: Vec<u8> = (0u8..23).collect();
        let path = write_chunk(&dir, "good.bin", &data);
        assert_eq!(load(&path).expect("verified load"), data);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_single_bit_flip_in_the_data_is_rejected() {
        let dir = temp_dir("flip");
        let data: Vec<u8> = (0u8..23).collect();
        let path = write_chunk(&dir, "flip.bin", &data);
        let mut bytes = std::fs::read(&path).expect("read back");
        bytes[8 + 5] ^= 0x10; // one bit, inside the data section
        std::fs::write(&path, bytes).expect("rewrite");
        match load(&path) {
            Err(MemoryChunkError::ChecksumMismatch { path: p, .. }) => {
                assert!(p.ends_with("flip.bin"), "error names the file");
            }
            other => panic!("bit flip must be a checksum error, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_truncated_file_is_a_length_error() {
        let dir = temp_dir("short");
        let data: Vec<u8> = (0u8..16).collect();
        let path = write_chunk(&dir, "short.bin", &data);
        let bytes = std::fs::read(&path).expect("read back");
        std::fs::write(&path, &bytes[..bytes.len() - 1]).expect("truncate");
        assert!(matches!(
            load(&path),
            Err(MemoryChunkError::LengthMismatch { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_checksum_matches_the_pin_for_a_known_vector() {
        // Hand-computed against memory_chunk.h's algorithm:
        // words 0x04030201 ^ 0x08070605 = 0x0c040404; tail bytes 9,10 at
        // shifts 0,8 XOR in 0x0a09.
        assert_eq!(
            check_sum(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10]),
            0x0c04_0404 ^ 0x0a09
        );
    }
}
