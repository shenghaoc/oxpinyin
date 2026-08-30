//! Loader for custom content files (addon/system dictionaries).
//!
//! Parses the binary format documented in `docs/findings/data-formats.md`.
//! All multi-byte fields are little-endian.

use std::fmt;

/// A single token-frequency pair within a phrase record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenPair {
    /// Encoded `phrase_token_t`: `(library_index << 16) | phrase_index`.
    pub token: u32,
    /// Token-level frequency (u32, even for compact-format records).
    pub frequency: u32,
}

/// A decoded phrase record from a content file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Number of tokens in the phrase (`n_gram`).
    pub n_gram: u8,
    /// Record layout flags (0–4).
    pub flags: u8,
    /// Phrase-level frequency.
    pub phrase_frequency: u32,
    /// Token pairs for this phrase.
    pub tokens: Vec<TokenPair>,
}

/// Errors that can occur when loading a content file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// Input is too short to contain a valid header (28 bytes minimum).
    TooShort {
        /// Number of bytes provided.
        len: usize,
    },
    /// The `data_size` field does not match the file length minus 8.
    DataSizeMismatch {
        /// Value from the header's `data_size` field.
        expected: u32,
        /// Computed from the actual input length.
        actual: u32,
    },
    /// Format version is not 17.
    UnsupportedVersion {
        /// The version found in the header.
        version: u32,
    },
    /// Record parsing failed at the given byte offset.
    BadRecord {
        /// Byte offset where parsing failed.
        offset: usize,
        /// Human-readable description of the problem.
        detail: String,
    },
    /// Parsed fewer records than the header's `nitems` promised.
    IncompleteRead {
        /// `nitems` from the header.
        expected: u32,
        /// Number of records successfully parsed.
        parsed: u32,
    },
    /// A record's `n_gram` or flags value is out of valid range.
    InvalidRecordHeader {
        /// Byte offset of the record header.
        offset: usize,
        /// The record's `n_gram` field.
        n_gram: u8,
        /// The record's `flags` field.
        flags: u8,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort { len } => write!(f, "input too short for header: {len} bytes"),
            Self::DataSizeMismatch { expected, actual } => {
                write!(
                    f,
                    "data_size mismatch: header says {expected}, computed {actual}"
                )
            }
            Self::UnsupportedVersion { version } => {
                write!(f, "unsupported format version {version}, expected 17")
            }
            Self::BadRecord { offset, detail } => {
                write!(f, "bad record at offset {offset}: {detail}")
            }
            Self::IncompleteRead { expected, parsed } => {
                write!(
                    f,
                    "incomplete read: expected {expected} records, parsed {parsed}"
                )
            }
            Self::InvalidRecordHeader {
                offset,
                n_gram,
                flags,
            } => {
                write!(
                    f,
                    "invalid record header at {offset}: n_gram={n_gram}, flags={flags}"
                )
            }
        }
    }
}

impl std::error::Error for LoadError {}

// ── header ─────────────────────────────────────────────────────────

const HEADER_SIZE: usize = 28;
const PRIMARY_IDX_SIZE: usize = 35 * 4; // 140 bytes
const NGOUPS: u32 = 35;
const EXPECTED_VERSION: u32 = 17;

#[derive(Debug)]
struct Header {
    // data_size: u32,   // offset 0
    magic: u32,   // offset 4
    nitems: u32,  // offset 8
    version: u32, // offset 12
    capacity: u32, // offset 16
                  // data_size2: u32,  // offset 20 (duplicate)
                  // ngroups: u32,     // offset 24 (always 35)
}

fn read_u32_le(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn parse_header(data: &[u8]) -> Result<Header, LoadError> {
    if data.len() < HEADER_SIZE {
        return Err(LoadError::TooShort { len: data.len() });
    }
    let data_size = read_u32_le(data, 0);
    let computed = (data.len() as u32).wrapping_sub(8);
    if data_size != computed {
        return Err(LoadError::DataSizeMismatch {
            expected: data_size,
            actual: computed,
        });
    }
    let version = read_u32_le(data, 12);
    if version != EXPECTED_VERSION {
        return Err(LoadError::UnsupportedVersion { version });
    }
    Ok(Header {
        magic: read_u32_le(data, 4),
        nitems: read_u32_le(data, 8),
        version,
        capacity: read_u32_le(data, 16),
    })
}

fn data_start(nitems: u32) -> usize {
    let sec_entries = nitems.saturating_sub(NGOUPS) as usize;
    let preamble = if sec_entries > 0 { 10 } else { 6 };
    HEADER_SIZE + PRIMARY_IDX_SIZE + sec_entries * 4 + preamble
}

// ── record parsing ─────────────────────────────────────────────────

/// Size of a simple (non-nested) sub-record: fl ∈ {0, 1, 2}.
fn sub_record_size(ng: u8, fl: u8) -> usize {
    match fl {
        0 => 6 + ng as usize * 8,
        1 => {
            if ng == 1 {
                14
            } else {
                6 + 8 + (ng as usize - 1) * 6 + 2
            }
        }
        2 => 6 + (ng as usize + 1) * 8,
        _ => 0,
    }
}

/// Compute the byte size of a record at `pos` in `data`.
fn record_size(ng: u8, fl: u8, data: &[u8], pos: usize) -> usize {
    if fl <= 2 {
        return sub_record_size(ng, fl);
    }

    // fl >= 3: extra token pairs + optional padding
    let extra = (fl - 1) as usize;
    let eff = ng as usize + extra;

    if ng < 10 {
        let mut base = 6 + eff * 8;
        if ng >= 3 {
            base += (ng as usize - 2) * 2;
        }
        if fl == 4 && ng >= 5 {
            base += 2;
        }
        return base;
    }

    // fl=3, ng >= 10: nested fl=1 sub-records
    let mut inner = pos + 6;
    let bound = (pos + 6 + eff * 30).min(data.len());
    while inner < bound {
        if inner + 2 > data.len() {
            break;
        }
        let s_ng = data[inner];
        let s_fl = data[inner + 1];
        if s_ng == 0 || s_ng > 30 || s_fl > 2 {
            break;
        }
        let sz = sub_record_size(s_ng, s_fl);
        if sz == 0 || inner + sz > bound {
            break;
        }
        inner += sz;
    }
    (inner - pos).max(8)
}

/// Decode a single record's token pairs.
///
/// `ng` = `n_gram`, `fl` = flags, `data` points to the record start.
fn decode_tokens(data: &[u8], ng: u8, fl: u8) -> Vec<TokenPair> {
    let mut tokens = Vec::with_capacity(ng as usize);
    let mut off = 6; // skip [n_gram, flags, freq_u32]

    if fl == 1 {
        // Compact format
        // First pair: [u32 token][u32 freq]
        if off + 8 <= data.len() {
            tokens.push(TokenPair {
                token: read_u32_le(data, off),
                frequency: read_u32_le(data, off + 4),
            });
            off += 8;
        }
        // Remaining pairs: [u32 token][u16 freq][u16 pad]
        for _ in 1..ng as usize {
            if off + 6 <= data.len() {
                let freq_lo = u16::from_le_bytes([data[off + 4], data[off + 5]]);
                tokens.push(TokenPair {
                    token: read_u32_le(data, off),
                    frequency: freq_lo as u32,
                });
                off += 6;
            }
        }
        return tokens;
    }

    // Standard format (fl=0, or fl>=2 with extra tokens)
    let num_pairs = if fl == 0 {
        ng as usize
    } else {
        ng as usize + (fl - 1) as usize
    };

    for _ in 0..num_pairs {
        if off + 8 <= data.len() {
            tokens.push(TokenPair {
                token: read_u32_le(data, off),
                frequency: read_u32_le(data, off + 4),
            });
            off += 8;
        }
    }

    // For fl>=3 with ng>=10: parse nested sub-records
    if fl >= 3 && ng >= 10 {
        tokens.clear();
        let mut inner = 6;
        let bound = data.len();
        while inner < bound {
            if inner + 2 > bound {
                break;
            }
            let s_ng = data[inner];
            let s_fl = data[inner + 1];
            if s_ng == 0 || s_ng > 30 || s_fl > 2 {
                break;
            }
            let sz = sub_record_size(s_ng, s_fl);
            if sz == 0 || inner + sz > bound {
                break;
            }
            let sub_tokens = decode_tokens(&data[inner..inner + sz], s_ng, s_fl);
            tokens.extend(sub_tokens);
            inner += sz;
        }
        return tokens;
    }

    tokens
}

fn parse_record(data: &[u8], pos: usize) -> Result<(Record, usize), LoadError> {
    if pos + 6 > data.len() {
        return Err(LoadError::BadRecord {
            offset: pos,
            detail: "truncated header".into(),
        });
    }
    let ng = data[pos];
    let fl = data[pos + 1];
    if ng == 0 || ng > 30 {
        return Err(LoadError::InvalidRecordHeader {
            offset: pos,
            n_gram: ng,
            flags: fl,
        });
    }
    let phrase_freq = read_u32_le(data, pos + 2);
    let sz = record_size(ng, fl, data, pos);
    if pos + sz > data.len() {
        return Err(LoadError::BadRecord {
            offset: pos,
            detail: format!("size {sz} exceeds data"),
        });
    }

    let record_data = &data[pos..pos + sz];
    let tokens = decode_tokens(record_data, ng, fl);

    Ok((
        Record {
            n_gram: ng,
            flags: fl,
            phrase_frequency: phrase_freq,
            tokens,
        },
        sz,
    ))
}

// ── public API ─────────────────────────────────────────────────────

/// A loaded custom content file (addon or system dictionary).
///
/// Records are indexed by their 0-based position in the file,
/// corresponding to the `phrase_index` field of a `phrase_token_t`.
#[derive(Debug, Clone)]
pub struct ContentTable {
    magic: u32,
    version: u32,
    capacity: u32,
    records: Vec<Record>,
}

impl ContentTable {
    /// Parse a content file from raw bytes.
    ///
    /// Returns an error if the header is invalid or records cannot be parsed.
    pub fn load(data: &[u8]) -> Result<Self, LoadError> {
        let hdr = parse_header(data)?;
        let ds = data_start(hdr.nitems);
        let mut pos = ds;
        // `nitems` is untrusted: a tiny hostile file claiming ~2^32 records
        // must not translate into a giant speculative allocation before the
        // length-checked loop below runs. Every record is at least 6 bytes
        // (fl=0, ng=0), so the remaining bytes bound how many can exist;
        // the exact-count check below still rejects the lie.
        let cap = (hdr.nitems as usize).min(data.len().saturating_sub(ds) / 6);
        let mut records = Vec::with_capacity(cap);

        while pos < data.len() && records.len() < hdr.nitems as usize {
            let (rec, sz) = parse_record(data, pos)?;
            pos += sz;
            records.push(rec);
        }

        if records.len() != hdr.nitems as usize {
            return Err(LoadError::IncompleteRead {
                expected: hdr.nitems,
                parsed: u32::try_from(records.len()).unwrap_or(u32::MAX),
            });
        }

        Ok(Self {
            magic: hdr.magic,
            version: hdr.version,
            capacity: hdr.capacity,
            records,
        })
    }

    /// The per-file magic/checksum value from the header.
    #[must_use]
    pub fn magic(&self) -> u32 {
        self.magic
    }

    /// The format version (always 17).
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Maximum `phrase_index` + 1.
    #[must_use]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Number of records in the table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Returns true if the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Get a record by its 0-based phrase index.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&Record> {
        self.records.get(index)
    }

    /// Iterate over all records in order.
    pub fn iter(&self) -> impl Iterator<Item = &Record> {
        self.records.iter()
    }
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        PathBuf::from(manifest)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join("w3")
    }
    #[test]
    fn header_rejects_short_input() {
        let data = [0u8; 10];
        let err = ContentTable::load(&data).unwrap_err();
        assert!(matches!(err, LoadError::TooShort { .. }));
    }

    #[test]
    fn header_rejects_wrong_version() {
        let mut data = vec![0u8; 28];
        // Write a plausible data_size
        let ds = u32::try_from(data.len() - 8).unwrap();
        data[0..4].copy_from_slice(&ds.to_le_bytes());
        data[20..24].copy_from_slice(&ds.to_le_bytes());
        // version = 99
        data[12..16].copy_from_slice(&99u32.to_le_bytes());
        let err = ContentTable::load(&data).unwrap_err();
        assert!(matches!(err, LoadError::UnsupportedVersion { version: 99 }));
    }

    #[test]
    fn culture_record_sizes() {
        // culture.bin uses compact (fl=1) for 33 records, with one fl=2 entry.
        // ng=2 fl=1 → 22, ng=3 fl=1 → 28, ng=4 fl=1 → 34, ng=1 fl=1 → 14.
        // ng=2 fl=2 → 30
        let dir = fixtures_dir();
        let data = std::fs::read(dir.join("culture.bin")).unwrap();
        let hdr = parse_header(&data).unwrap();
        let ds = data_start(hdr.nitems);
        let mut pos = ds;
        let mut sizes = Vec::new();
        let mut non_fl1 = 0;
        while pos < data.len() && sizes.len() < hdr.nitems as usize {
            let ng = data[pos];
            let fl = data[pos + 1];
            let sz = record_size(ng, fl, &data, pos);
            sizes.push((ng, fl, sz));
            if fl != 1 {
                non_fl1 += 1;
            }
            pos += sz;
        }
        // Only record 2 should have fl=2
        assert_eq!(non_fl1, 1);
        assert_eq!(sizes[2], (2, 2, 30));
        // All other records: fl=1 with known sizes
        for &(ng, fl, sz) in &sizes {
            if fl != 1 {
                continue;
            }
            let expected = if ng == 1 {
                14
            } else {
                14 + (ng as usize - 1) * 6 + 2
            };
            assert_eq!(sz, expected, "ng={ng}: expected {expected}, got {sz}");
        }
    }
}
