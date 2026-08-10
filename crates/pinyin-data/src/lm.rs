//! Bigram language model over the verbatim-copied system bigram.
//!
//! Implements [`pinyin_core::LanguageModel`] on the byte format frozen in
//! `docs/findings/data-layer-export.md`: each key is the previous
//! `phrase_token_t` as 4 bytes little-endian; each value is a `total: u32`
//! followed by 8-byte `{next_token: u32, count: u32}` records, with
//! `total == Σ count`. The transition cost is the W4 surprisal of the
//! observed count within the entry's total, on the frozen
//! [`pinyin_core::cost`] scale.

use std::fmt;
use std::path::Path;

use pinyin_core::cost::{UNKNOWN_COST, surprisal};
use pinyin_core::{Cost, LanguageModel, PhraseToken};

use crate::table::{LookupTable, TableError};

/// Error conditions for bigram lookups.
#[derive(Debug)]
pub enum LmError {
    /// A table-level error (I/O, redb, etc.).
    Table(TableError),
    /// Value bytes did not parse under the frozen bigram schema.
    Parse(String),
}

impl fmt::Display for LmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table(e) => write!(f, "table error: {e}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for LmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Table(e) => Some(e),
            Self::Parse(_) => None,
        }
    }
}

impl From<TableError> for LmError {
    fn from(e: TableError) -> Self {
        Self::Table(e)
    }
}

/// Bigram language model backed by `bigram.redb`.
pub struct BigramLanguageModel {
    bigram: LookupTable,
}

impl BigramLanguageModel {
    /// Opens the bigram model from a redb table file.
    pub fn open(path: &Path) -> Result<Self, LmError> {
        Ok(Self {
            bigram: LookupTable::open(path).map_err(LmError::Table)?,
        })
    }

    /// Number of previous-token entries.
    pub fn entry_count(&self) -> Result<u64, LmError> {
        self.bigram.len().map_err(LmError::Table)
    }

    /// Returns `(count, total)` for the `prev → next` transition, or `None`
    /// when `prev` has no bigram entry.
    fn transition(&self, prev: u32, next: u32) -> Result<Option<(u32, u32)>, LmError> {
        let Some(raw) = self
            .bigram
            .get(&prev.to_le_bytes())
            .map_err(LmError::Table)?
        else {
            return Ok(None);
        };
        let (total, records) = parse_bigram_value(&raw)?;
        let count = records
            .iter()
            .find(|(next_token, _)| *next_token == next)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        Ok(Some((count, total)))
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
        let Some(prev) = history.last() else {
            // No context: the dictionary's frequency order carries unigram
            // ranking; the model adds nothing.
            return Ok(edge_cost);
        };

        let transition_cost = match self.transition(prev.value(), token.value())? {
            // surprisal(0, total) is the finite UNKNOWN_COST floor, so a
            // novel transition after a known token costs the floor, not ∞.
            Some((count, total)) => surprisal(u64::from(count), u64::from(total)),
            // Previous token entirely absent from the bigram: same floor.
            None => UNKNOWN_COST,
        };

        Ok(edge_cost.saturating_add(transition_cost))
    }
}

/// Parses a bigram value as `(total, [{next_token, count}])`.
fn parse_bigram_value(data: &[u8]) -> Result<(u32, Vec<(u32, u32)>), LmError> {
    if data.len() < 4 || !(data.len() - 4).is_multiple_of(8) {
        return Err(LmError::Parse(format!(
            "bigram value length {} is not 4 + 8n",
            data.len()
        )));
    }
    let total = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let records = data[4..]
        .chunks_exact(8)
        .map(|chunk| {
            (
                u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
            )
        })
        .collect();
    Ok((total, records))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 你's gb_char token in the pinned model.
    const NI: u32 = 0x0100_1225;
    /// 的's gb_char token in the pinned model.
    const DE: u32 = 0x0100_05db;

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

    fn model() -> BigramLanguageModel {
        BigramLanguageModel::open(&fixtures_dir().join("bigram.redb")).unwrap()
    }

    #[test]
    fn mini_fixture_opens() {
        assert!(model().entry_count().unwrap() > 0);
    }

    #[test]
    fn observed_transition_is_cheaper_than_novel() {
        let model = model();
        let history = [PhraseToken::new(NI)];
        let observed = model
            .score(&history, &PhraseToken::new(DE), 0)
            .expect("你 → 的 scores");
        let novel = model
            .score(&history, &PhraseToken::new(0x0100_0001), 0)
            .expect("你 → rare scores");
        assert!(
            observed < novel,
            "你 → 的 ({observed}) must undercut a novel transition ({novel})"
        );
    }

    #[test]
    fn empty_history_returns_edge_cost() {
        let cost = model().score(&[], &PhraseToken::new(DE), 1234).unwrap();
        assert_eq!(cost, 1234);
    }

    #[test]
    fn invariant_holds_for_every_fixture_entry() {
        let model = model();
        for (key, value) in model.bigram.iter().unwrap() {
            assert_eq!(key.len(), 4, "bigram keys are 4-byte prev tokens");
            let (total, records) = parse_bigram_value(&value).expect("schema parses");
            let sum: u64 = records.iter().map(|(_, count)| u64::from(*count)).sum();
            assert_eq!(u64::from(total), sum, "total == Σ count for {key:02x?}");
        }
    }
}
