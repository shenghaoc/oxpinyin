//! Direct reader for libpinyin's `phrase_index.bin` DBM — the Rust
//! equivalent of `PhraseLargeTable3`.
//!
//! libpinyin stores the phrase table as a backend DBM (KC TreeDB or Tkrzw
//! TreeDBM) mapping **UCS-4 phrase text** → **`u32 token[]`**.
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

use crate::chewing_table::ChewingDbm;
use crate::dict::DictError;

// ── Key encoding ─────────────────────────────────────────────────

/// Encodes a UTF-8 string into a UCS-4 DBM key (each char as `u32` LE).
///
/// This matches how libpinyin encodes phrase text for the `phrase_index.bin`
/// DBM: `g_utf8_to_ucs4` produces a `gunichar[]` (= `guint32[]`), stored
/// as raw bytes in native (LE) byte order.
#[must_use]
pub(crate) fn encode_ucs4_key(text: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(text.len() * 4);
    for ch in text.chars() {
        buf.extend_from_slice(&(ch as u32).to_le_bytes());
    }
    buf
}

/// Decodes a UCS-4 DBM key back to a UTF-8 string.
///
/// Returns `None` if the key length is not a multiple of 4 or if any
/// 4-byte group is not a valid Unicode scalar value.
#[must_use]
pub(crate) fn decode_ucs4_key(key: &[u8]) -> Option<String> {
    if !key.len().is_multiple_of(4) {
        return None;
    }
    let mut text = String::with_capacity(key.len() / 4 * 3);
    for chunk in key.chunks_exact(4) {
        let code = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        text.push(char::from_u32(code)?);
    }
    Some(text)
}

// ── Value decoding ───────────────────────────────────────────────

/// Decodes a DBM value into phrase tokens.
///
/// The value is a flat array of `u32` tokens (LE), 4 bytes each.
/// Returns an empty Vec for an empty value (prefix marker).
///
/// # Errors
///
/// Returns `DictError::Parse` if the value length is not a multiple of 4.
pub(crate) fn decode_tokens(value: &[u8]) -> Result<Vec<u32>, DictError> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    if !value.len().is_multiple_of(4) {
        return Err(DictError::Parse(format!(
            "phrase index value length {} is not a multiple of 4",
            value.len(),
        )));
    }
    Ok(value
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect())
}

/// Encodes a slice of tokens into a DBM value.
pub(crate) fn encode_tokens(tokens: &[u32]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(tokens.len() * 4);
    for token in tokens {
        buf.extend_from_slice(&token.to_le_bytes());
    }
    buf
}

// ── PhraseTable ──────────────────────────────────────────────────

/// The Rust equivalent of `PhraseLargeTable3`: a lazy, read-only view
/// over a phrase-index DBM.
///
/// Does not materialize the entire index at open time. Lookups are
/// point reads against the DBM backend.
pub(crate) struct PhraseTable {
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
        match self.dbm.get(&key)? {
            Some(value) => decode_tokens(&value),
            None => Ok(Vec::new()),
        }
    }

    /// Whether `text` exists as a key (empty or non-empty value).
    ///
    /// Used for the phrase-segment DP's span probe: does this character
    /// span correspond to a known phrase?
    pub(crate) fn has_key(&self, text: &str) -> Result<bool, DictError> {
        if text.is_empty() {
            return Ok(false);
        }
        let key = encode_ucs4_key(text);
        Ok(self.dbm.get(&key)?.is_some())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
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
    }

    #[test]
    fn encode_ucs4_key_single_char() {
        let key = encode_ucs4_key("你");
        assert_eq!(key.len(), 4);
        let code = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        assert_eq!(code, '你' as u32);
        assert_eq!(code, 0x4F60);
    }

    #[test]
    fn encode_ucs4_key_multi_char() {
        let key = encode_ucs4_key("你好");
        assert_eq!(key.len(), 8);
        let c0 = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        let c1 = u32::from_le_bytes([key[4], key[5], key[6], key[7]]);
        assert_eq!(c0, '你' as u32);
        assert_eq!(c1, '好' as u32);
    }

    #[test]
    fn encode_ucs4_key_ascii() {
        let key = encode_ucs4_key("ab");
        assert_eq!(key.len(), 8);
        assert_eq!(
            u32::from_le_bytes([key[0], key[1], key[2], key[3]]),
            'a' as u32
        );
        assert_eq!(
            u32::from_le_bytes([key[4], key[5], key[6], key[7]]),
            'b' as u32
        );
    }

    #[test]
    fn decode_ucs4_key_round_trip() {
        let original = "中国人民";
        let key = encode_ucs4_key(original);
        let decoded = decode_ucs4_key(&key).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn decode_ucs4_key_rejects_bad_length() {
        assert!(decode_ucs4_key(&[0, 0, 0]).is_none());
        assert!(decode_ucs4_key(&[0, 0, 0, 0, 0]).is_none());
    }

    #[test]
    fn decode_ucs4_key_rejects_invalid_scalar() {
        let mut key = encode_ucs4_key("x");
        key[0..4].copy_from_slice(&0xD800_u32.to_le_bytes());
        assert!(decode_ucs4_key(&key).is_none());
    }

    #[test]
    fn decode_tokens_round_trip() {
        let tokens = vec![0x01000001, 0x02000042];
        let encoded = encode_tokens(&tokens);
        assert_eq!(encoded.len(), 8);
        let decoded = decode_tokens(&encoded).unwrap();
        assert_eq!(decoded, tokens);
    }

    #[test]
    fn decode_tokens_empty_is_prefix_marker() {
        let decoded = decode_tokens(&[]).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn decode_tokens_rejects_bad_length() {
        assert!(decode_tokens(&[0, 0, 0]).is_err());
    }

    #[test]
    fn search_finds_exact_phrase() {
        let dbm = MemoryDbm::new();
        let key = encode_ucs4_key("你好");
        let value = encode_tokens(&[0x01000099]);
        dbm.put(key, value);

        let table = PhraseTable::new(Box::new(dbm));
        let tokens = table.search("你好").unwrap();
        assert_eq!(tokens, vec![0x01000099]);
    }

    #[test]
    fn search_returns_multiple_tokens() {
        let dbm = MemoryDbm::new();
        let key = encode_ucs4_key("中");
        let value = encode_tokens(&[0x01000020, 0x02000020]);
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
        dbm.put(encode_ucs4_key("好"), encode_tokens(&[0x01000011]));

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
