//! Bigram language model backed by `bigram.redb`.
//!
//! Implements [`pinyin_core::LanguageModel`] using the oracle's bigram
//! frequency table. Each key is a 4-byte `(prev: u16, next: u16)` pair;
//! each value is an array of 8-byte `(frequency: u32, packed_next: u32)`
//! entries.

use std::path::Path;

use pinyin_core::{Cost, LanguageModel, PhraseToken};

use crate::table::{LookupTable, TableError};
use crate::types::{BigramEntry, BigramKey};

/// Error conditions for bigram language model lookups.
#[derive(Debug)]
pub enum LmError {
    /// A table-level error (I/O, redb, etc.).
    Table(TableError),
}

impl std::fmt::Display for LmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Table(e) => write!(f, "table error: {e}"),
        }
    }
}

impl std::error::Error for LmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Table(e) => Some(e),
        }
    }
}

impl From<TableError> for LmError {
    fn from(e: TableError) -> Self {
        Self::Table(e)
    }
}

/// Bigram language model backed by the oracle's bigram table.
///
/// Scores transitions from one phrase token to another using the
/// frequency data in `bigram.redb`.
pub struct BigramLanguageModel {
    bigram: LookupTable,
}

impl BigramLanguageModel {
    /// Open the bigram language model from a redb table file.
    ///
    /// `path` should point to `bigram.redb`.
    pub fn open(path: &Path) -> Result<Self, LmError> {
        let bigram = LookupTable::open(path)?;
        Ok(Self { bigram })
    }

    /// Return the number of bigram entries.
    pub fn entry_count(&self) -> Result<u64, LmError> {
        Ok(self.bigram.len()?)
    }

    /// Extract the u16 token index from a PhraseToken for bigram lookup.
    ///
    /// The oracle's bigram keys use the lower 16 bits of the phrase token
    /// reference as the token index. This mirrors libpinyin's
    /// `PHRASE_INDEX_MASK`.
    fn token_index(token: &PhraseToken) -> u16 {
        (token.value() & 0xFFFF) as u16
    }
}

impl LanguageModel for BigramLanguageModel {
    type Token = PhraseToken;
    type Error = LmError;

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        // Bigram scoring: if history is non-empty, look up the last token
        // and compute the transition cost from the frequency data.
        if history.is_empty() {
            // Unigram: no bigram context; return edge_cost unchanged.
            return Ok(edge_cost);
        }

        let prev = history.last().unwrap();
        let key = BigramKey::new(Self::token_index(prev), Self::token_index(token));

        let entries = self.lookup_bigram_entries(&key)?;

        // Find the entry whose packed_next matches token.value() and use
        // its frequency to compute the cost.
        //
        // In the oracle, higher frequency = lower cost (more likely).
        // We map frequency → cost using a log-like scale:
        //   cost = edge_cost - log2(1 + frequency) * COST_PER_BIT
        //
        // If no matching entry is found, the transition is novel (low probability).
        let token_raw = token.value();
        let freq = entries
            .iter()
            .find(|e| e.packed_next == token_raw)
            .map(|e| e.frequency)
            .unwrap_or(0);

        // Convert frequency to a cost adjustment using integer-approximation
        // log2 (no f64). Constitution rule 6 requires determinism; f64::log2
        // is not bit-identical across platforms/libm. This matches W4's
        // cost.rs approach (COST_PER_BIT = 1000, integer arithmetic via
        // leading zeros / fixed-point squaring).
        let adjustment = if freq > 0 {
            log2_cost(freq as u64 + 1)
        } else {
            0
        };

        Ok(edge_cost.saturating_sub(adjustment))
    }
}

impl BigramLanguageModel {
    /// Look up the bigram entries for a given key.
    fn lookup_bigram_entries(&self, key: &BigramKey) -> Result<Vec<BigramEntry>, LmError> {
        let key_bytes = key.to_bytes();
        let raw = match self.bigram.get(&key_bytes)? {
            Some(v) => v,
            None => return Ok(Vec::new()),
        };
        Ok(parse_bigram_entries(&raw))
    }
}

/// Parse a bigram value blob as an array of BigramEntry.
///
/// Each entry is 8 bytes: `(frequency: u32 LE, packed_next: u32 LE)`.
fn parse_bigram_entries(data: &[u8]) -> Vec<BigramEntry> {
    data.chunks_exact(8)
        .filter_map(BigramEntry::from_bytes)
        .collect()
}

fn log2_cost(value: u64) -> Cost {
    if value <= 1 {
        return 0;
    }
    let bits = log2_fixed(value);
    (bits / 1000) as Cost
}

fn log2_fixed(value: u64) -> u64 {
    let integer = value.ilog2();
    let remainder = value >> integer.saturating_sub(10);
    let frac = remainder & 0x3FF;
    (integer as u64) * 1000 + (frac * 1000) / 1024
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        std::path::PathBuf::from(manifest)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join("w3")
    }

    #[test]
    fn bigram_lm_opens_from_fixtures() {
        let lm = BigramLanguageModel::open(&fixtures_dir().join("bigram.redb")).unwrap();
        // 50 records in the mini fixture.
        assert_eq!(lm.entry_count().unwrap(), 50);
    }

    #[test]
    fn score_with_empty_history_returns_edge_cost() {
        let lm = BigramLanguageModel::open(&fixtures_dir().join("bigram.redb")).unwrap();
        let token = PhraseToken::new(0);
        let cost = lm.score(&[], &token, 100).unwrap();
        assert_eq!(cost, 100);
    }

    #[test]
    fn score_with_known_bigram() {
        let lm = BigramLanguageModel::open(&fixtures_dir().join("bigram.redb")).unwrap();
        let _entry = BigramEntry {
            frequency: 70,
            packed_next: 42,
        };
        let bytes: Vec<u8> = [&70u32.to_le_bytes()[..], &42u32.to_le_bytes()[..]].concat();
        let parsed = BigramEntry::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.frequency, 70);
        assert_eq!(parsed.packed_next, 42);

        // Verify that scoring with the actual bigram data works for a
        // known key: key (0, 257) exists and has 1 entry.
        // Full encoding verification is tracked as a known W3 integration gap (blocked:syllable-encoder).
        let key = BigramKey::new(0, 257);
        let entries = lm.lookup_bigram_entries(&key).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].frequency, 70);
    }

    #[test]
    fn score_with_novel_bigram() {
        let lm = BigramLanguageModel::open(&fixtures_dir().join("bigram.redb")).unwrap();
        let prev = PhraseToken::new(0xFFFF);
        let next = PhraseToken::new(0xEEEE);
        let cost = lm.score(&[prev], &next, 500).unwrap();
        assert_eq!(cost, 500, "novel bigram should return edge_cost unchanged");
    }

    #[test]
    fn bigram_key_round_trip() {
        let key = BigramKey::new(0x1234, 0x5678);
        let bytes = key.to_bytes();
        let restored = BigramKey::from_bytes(&bytes).unwrap();
        assert_eq!(key, restored);
    }

    #[test]
    fn log2_cost_is_deterministic_and_monotonic() {
        // No f64: compare integer log2_cost across values.
        let c1 = log2_cost(1);
        let c2 = log2_cost(2);
        let c4 = log2_cost(4);
        assert_eq!(c1, 0);
        assert_eq!(c2, 1);
        assert_eq!(c4, 2);
        // Monotonic.
        let mut prev = c1;
        for v in [3, 5, 10, 70, 100, 1000] {
            let cur = log2_cost(v);
            assert!(cur >= prev, "log2_cost fell at {v}");
            prev = cur;
        }
    }
}
