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
//! See `docs/findings/data-formats.md` §5
//! and `phrase_large_table3_tkrzwdb.cpp`.

use crate::data::DictError;
use crate::data::chewing_table::{ChewingDbm, DefaultDbm, prefix_upper_bound};
// The key and value layouts this reader shares with
// `gen_binary_files`'s writer: one written copy (`crate::data::row_format`).
#[cfg(test)]
use crate::data::row_format::phrase_index::test_encode_tokens as encode_tokens;
use crate::data::row_format::phrase_index::{
    decode_tokens, encode_ucs4_key, encode_ucs4_key_scalars,
};

// ── PhraseTable ──────────────────────────────────────────────────

/// The Rust equivalent of `PhraseLargeTable3`: a lazy, read-only view
/// over a phrase-index DBM.
///
/// Does not materialize the entire index at open time. Lookups are
/// point reads against the DBM backend.
pub struct PhraseTable<D = DefaultDbm> {
    dbm: D,
}

impl<D: ChewingDbm> PhraseTable<D> {
    /// Constructs the phrase index over its row store.
    pub const fn new(dbm: D) -> Self {
        Self { dbm }
    }

    pub(crate) fn attachment_failed(&self) -> bool {
        self.dbm.attachment_failed()
    }

    pub(crate) fn finish_attachment(&mut self) {
        self.dbm.finish_attachment();
    }

    #[cfg(test)]
    pub(crate) fn attachment_finished(&self) -> bool {
        self.dbm.attachment_finished()
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
        self.search_key(&encode_ucs4_key(text))
    }

    /// [`Self::search`] over raw UCS-4 words, including surrogate or
    /// above-Unicode values. The key is compared as stored, without
    /// building a `char` or `str` from a non-scalar.
    pub(crate) fn search_ucs4(&self, words: &[u32]) -> Result<Vec<u32>, DictError> {
        if words.is_empty() {
            return Ok(Vec::new());
        }
        self.search_key(&encode_ucs4_key_scalars(words))
    }

    fn search_key(&self, key: &[u8]) -> Result<Vec<u32>, DictError> {
        self.present_key(key)?
            .map_or_else(|| Ok(Vec::new()), |value| decode_tokens(&value))
    }

    /// `PhraseLargeTable3::search`'s answer without decoding: `None` when
    /// the key is absent, `Some(value)` when it is present, a prefix marker
    /// with an empty value included.
    pub fn present_key(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DictError> {
        let mut value = Vec::new();
        if !self.dbm.get_phrase_row(key, &mut value)? {
            return Ok(None);
        }
        Ok(Some(value))
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
    /// `prefix` is raw UCS-4 words, as the pin's `m_prefix_ucs4`, so a
    /// surrogate or above-Unicode word is compared as stored.
    pub(crate) fn search_suggestion(&self, prefix: &[u32]) -> Result<Vec<u32>, DictError> {
        if prefix.is_empty() {
            return Ok(Vec::new());
        }
        let key = encode_ucs4_key_scalars(prefix);
        if !self.dbm.contains(&key)? {
            return Ok(Vec::new());
        }
        let upper = prefix_upper_bound(&key);
        let mut tokens = Vec::new();
        self.dbm.walk(&key, upper.as_deref(), |row_key, value| {
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
}

#[cfg(test)]
mod tests;
