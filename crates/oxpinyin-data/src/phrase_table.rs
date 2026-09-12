//! Direct reader for libpinyin's `phrase_index.bin` DBM — the Rust
//! equivalent of `PhraseLargeTable3`.
//!
//! libpinyin stores the phrase table as a backend DBM (KC `TreeDB` or Tkrzw
//! `TreeDBM`) mapping **UCS-4 phrase text** → **`u32 token[]`**.
//!
//! - **Key:** each character of the phrase encoded as a `guint32` (4 bytes
//!   LE on LE platforms), concatenated. A 2-character phrase like 你好 has
//!   an 8-byte key: `[0x4f60_u32.to_le_bytes(), 0x597d_u32.to_le_bytes()]`.
//! - **Value:** one or more `phrase_token_t` values (each `u32` LE),
//!   concatenated. Multiple tokens mean the same text maps to multiple
//!   phrase items (e.g. different library origins).
//!
//! The phrase DBM also uses prefix markers (empty-value entries) for
//! `SEARCH_CONTINUED`, the same mechanism the pinyin index uses: every
//! shorter prefix of a stored key has an entry (empty or real) so that
//! `search_suggestion` can walk prefixes to find continuation candidates.
//!
//! See `docs/findings/libpinyin-system-data-formats-2026-09-01.md` §1.1
//! and `phrase_large_table3_tkrzwdb.cpp`.

use crate::chewing_table::{ChewingDbm, prefix_upper_bound};
use crate::dict::DictError;
// The key and value layouts this reader shares with
// `oxpinyin-datagen`'s writer: one written copy (`crate::row_format`).
#[cfg(test)]
use crate::row_format::phrase_index::encode_tokens;
use crate::row_format::phrase_index::{decode_tokens, encode_ucs4_key};

// ── PhraseTable ──────────────────────────────────────────────────

/// The Rust equivalent of `PhraseLargeTable3`: a lazy, read-only view
/// over a phrase-index DBM.
///
/// Does not materialize the entire index at open time. Lookups are
/// point reads against the DBM backend.
pub struct PhraseTable {
    dbm: Box<dyn ChewingDbm + Send + Sync>,
}

impl PhraseTable {
    pub(crate) fn new(dbm: Box<dyn ChewingDbm + Send + Sync>) -> Self {
        Self { dbm }
    }

    /// Looks up tokens for an exact phrase text.
    ///
    /// Port of `PhraseLargeTable3::search`
    /// (`phrase_large_table3_tkrzwdb.cpp:28-52`):
    ///
    /// - Key not found → empty
    /// - Key found, empty value → empty (prefix marker)
    /// - Key found, non-empty → decoded tokens
    pub(crate) fn search(&self, text: &str) -> Result<Vec<u32>, DictError> {
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let key = encode_ucs4_key(text);
        self.dbm
            .get(&key)?
            .map_or_else(|| Ok(Vec::new()), |value| decode_tokens(&value))
    }

    /// Tokens of every stored phrase that starts with `prefix` and is
    /// longer — `PhraseLargeTable3::search_suggestion`
    /// (`phrase_large_table3_kyotodb.cpp` / `_tkrzwdb.cpp`), the
    /// prediction path's source of longer phrases:
    ///
    /// * the prefix itself must be a key (`m_db->check` — empty marker or
    ///   phrase; absent → `SEARCH_NONE`, nothing);
    /// * the cursor jumps to the prefix and steps past it, then keeps
    ///   reading while `phrase_continue_search` holds: the row's key is
    ///   longer than the prefix and its first `prefix` characters compare
    ///   equal (`compare_phrase` over the shorter length) — exactly the
    ///   keys whose bytes extend the prefix's, a contiguous run under the
    ///   tree's byte order;
    /// * each row's tokens are appended (`PhraseTableEntry::search`; an
    ///   empty marker contributes none).
    ///
    /// Tokens come back in the DBM's key order — upstream's physical
    /// bucket walk, which the caller sorts into oxpinyin's defined
    /// prediction order. Upstream also drops tokens whose library array is
    /// `NULL` (not loaded); the dictionary applies that when it resolves
    /// text.
    pub(crate) fn search_suggestion(&self, prefix: &str) -> Result<Vec<u32>, DictError> {
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        let key = encode_ucs4_key(prefix);
        if self.dbm.get(&key)?.is_none() {
            return Ok(Vec::new());
        }
        let upper = prefix_upper_bound(&key);
        let mut tokens = Vec::new();
        self.dbm
            .walk(&key, upper.as_deref(), &mut |row_key, value| {
                // The jump lands on the prefix's own row; upstream steps past
                // it before reading.
                if row_key.len() <= key.len() || !row_key.starts_with(&key) {
                    return Ok(false);
                }
                tokens.extend(decode_tokens(value)?);
                Ok(false)
            })?;
        Ok(tokens)
    }

    /// Whether `text` exists as a key (empty or non-empty value).
    #[cfg(test)]
    pub(crate) fn has_key(&self, text: &str) -> Result<bool, DictError> {
        if text.is_empty() {
            return Ok(false);
        }
        let key = encode_ucs4_key(text);
        Ok(self.dbm.get(&key)?.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::sync::Mutex;

    struct MemoryDbm {
        data: Mutex<BTreeMap<Vec<u8>, Vec<u8>>>,
    }

    impl MemoryDbm {
        fn new() -> Self {
            Self {
                data: Mutex::new(BTreeMap::new()),
            }
        }

        fn put(&self, key: Vec<u8>, value: Vec<u8>) {
            self.data.lock().unwrap().insert(key, value);
        }
    }

    impl ChewingDbm for MemoryDbm {
        fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DictError> {
            Ok(self.data.lock().unwrap().get(key).cloned())
        }

        fn walk(
            &self,
            lo: &[u8],
            hi: Option<&[u8]>,
            visit: &mut crate::chewing_table::RowVisitor<'_>,
        ) -> Result<(), DictError> {
            let data = self.data.lock().unwrap();
            for (key, value) in data.range(lo.to_vec()..) {
                if hi.is_some_and(|hi| key.as_slice() >= hi) {
                    break;
                }
                if visit(key, value)? {
                    break;
                }
            }
            Ok(())
        }
    }

    #[test]
    fn search_suggestion_collects_every_longer_phrase_under_the_prefix() {
        let dbm = MemoryDbm::new();
        // 你 is a marker, 你好 / 你们 / 你好吗 are phrases, 好 is a neighbour.
        dbm.put(encode_ucs4_key("你"), Vec::new());
        dbm.put(
            encode_ucs4_key("你好"),
            encode_tokens(&[0x0100_0099, 0x0200_0001]),
        );
        dbm.put(encode_ucs4_key("你好吗"), encode_tokens(&[0x0300_0005]));
        dbm.put(encode_ucs4_key("你们"), encode_tokens(&[0x0100_0098]));
        dbm.put(encode_ucs4_key("好"), encode_tokens(&[0x0100_0011]));
        let table = PhraseTable::new(Box::new(dbm));
        let mut tokens = table.search_suggestion("你").unwrap();
        tokens.sort_unstable();
        assert_eq!(
            tokens,
            vec![0x0100_0098, 0x0100_0099, 0x0200_0001, 0x0300_0005]
        );
        // The prefix's own tokens are not suggestions.
        let tokens = table.search_suggestion("你好").unwrap();
        assert_eq!(tokens, vec![0x0300_0005]);
        // A prefix that is no key at all answers nothing, even though
        // longer phrases would extend it.
        let dbm = MemoryDbm::new();
        dbm.put(encode_ucs4_key("你好"), encode_tokens(&[0x0100_0099]));
        let table = PhraseTable::new(Box::new(dbm));
        assert!(table.search_suggestion("你").unwrap().is_empty());
        assert!(table.search_suggestion("").unwrap().is_empty());
    }

    #[test]
    fn search_finds_exact_phrase() {
        let dbm = MemoryDbm::new();
        let key = encode_ucs4_key("你好");
        let value = encode_tokens(&[0x0100_0099]);
        dbm.put(key, value);

        let table = PhraseTable::new(Box::new(dbm));
        let tokens = table.search("你好").unwrap();
        assert_eq!(tokens, vec![0x0100_0099]);
    }

    #[test]
    fn search_returns_multiple_tokens() {
        let dbm = MemoryDbm::new();
        let key = encode_ucs4_key("中");
        let value = encode_tokens(&[0x0100_0020, 0x0200_0020]);
        dbm.put(key, value);

        let table = PhraseTable::new(Box::new(dbm));
        let tokens = table.search("中").unwrap();
        assert_eq!(tokens.len(), 2);
    }

    #[test]
    fn search_miss_returns_empty() {
        let dbm = MemoryDbm::new();
        let table = PhraseTable::new(Box::new(dbm));
        let tokens = table.search("不存在").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn search_empty_input_returns_empty() {
        let dbm = MemoryDbm::new();
        let table = PhraseTable::new(Box::new(dbm));
        let tokens = table.search("").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn search_prefix_marker_returns_empty() {
        let dbm = MemoryDbm::new();
        let key = encode_ucs4_key("你");
        dbm.put(key, Vec::new());

        let table = PhraseTable::new(Box::new(dbm));
        let tokens = table.search("你").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn has_key_finds_existing() {
        let dbm = MemoryDbm::new();
        dbm.put(encode_ucs4_key("好"), encode_tokens(&[0x0100_0011]));

        let table = PhraseTable::new(Box::new(dbm));
        assert!(table.has_key("好").unwrap());
        assert!(!table.has_key("坏").unwrap());
    }

    #[test]
    fn has_key_finds_prefix_marker() {
        let dbm = MemoryDbm::new();
        dbm.put(encode_ucs4_key("你"), Vec::new());

        let table = PhraseTable::new(Box::new(dbm));
        assert!(table.has_key("你").unwrap());
    }

    #[test]
    fn has_key_empty_returns_false() {
        let dbm = MemoryDbm::new();
        let table = PhraseTable::new(Box::new(dbm));
        assert!(!table.has_key("").unwrap());
    }

    #[test]
    fn malformed_value_does_not_panic() {
        let dbm = MemoryDbm::new();
        dbm.put(encode_ucs4_key("坏"), vec![0xFF; 5]);

        let table = PhraseTable::new(Box::new(dbm));
        assert!(table.search("坏").is_err());
    }
}
