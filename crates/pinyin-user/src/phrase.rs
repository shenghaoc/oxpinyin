//! User phrase-index types and the token-layout constants pinned in
//! `docs/findings/user-store.md` §3.
//!
//! New user phrases live in the `USER_DICTIONARY` sub-index. Token allocation
//! is "max token in the sub-index + 1" (`range.m_range_end`, bumped past the
//! reserved zero id). This module names those constants and the value types
//! the store records; it does not talk to redb.

use crate::store::Token;

/// Sub-index user phrases live in (`novel_types.h:161`, §3).
pub const USER_DICTIONARY: u8 = 7;

/// Low 24 bits of a `phrase_token_t` — the phrase id inside a sub-index
/// (`novel_types.h:41`).
pub const PHRASE_MASK: Token = 0x00FF_FFFF;

/// Bits 24–27 of a `phrase_token_t` — the sub-index nibble
/// (`novel_types.h:42`).
pub const PHRASE_INDEX_LIBRARY_MASK: Token = 0x0F00_0000;

/// Exclusive upper bound on phrase length in Unicode scalar values
/// (`novel_types.h:119`). libpinyin rejects `len == 0` and `len >=` this
/// (`pinyin.cpp:3678`, `pinyin.cpp:643`).
pub const MAX_PHRASE_LENGTH: usize = 16;

/// `count` used when the caller passes "default" (`pinyin.cpp:521`; §3.2).
pub const DEFAULT_PHRASE_COUNT: u64 = 5;

/// Unigram seed factor on a *new* user phrase (`pinyin.cpp:522`; §3.2).
/// Distinct from [`crate::seed::UNIGRAM_FACTOR`] (7), which is the *training*
/// path.
pub const ADD_PHRASE_UNIGRAM_FACTOR: u64 = 3;

/// One pinyin key in a stored pronunciation.
///
/// Opaque 16-bit id. In pinyin-rs this is a [`pinyin_core::SyllableKey`]
/// index; T2 stores the sequence as values, not libpinyin's `ChewingKey`
/// bitfields.
pub type PinyinKey = u16;

/// `PHRASE_INDEX_LIBRARY_INDEX(token)` (`novel_types.h:44`).
#[must_use]
pub const fn phrase_index_library_index(token: Token) -> u8 {
    ((token & PHRASE_INDEX_LIBRARY_MASK) >> 24) as u8
}

/// `PHRASE_INDEX_MAKE_TOKEN(phrase_index, token)` (`novel_types.h:45-46`).
#[must_use]
pub const fn phrase_index_make_token(phrase_index: u8, token: Token) -> Token {
    (((phrase_index as Token) << 24) & PHRASE_INDEX_LIBRARY_MASK) | (token & PHRASE_MASK)
}

/// Whether `token` lives in [`USER_DICTIONARY`] — the same nibble test as
/// `pinyin_is_user_candidate` (`pinyin.cpp:3716-3719`, §3.2).
#[must_use]
pub const fn is_user_token(token: Token) -> bool {
    phrase_index_library_index(token) == USER_DICTIONARY
}

/// First token allocated in an empty user sub-index.
///
/// Empty `get_range` reports `m_range_end = 1` (`phrase_index.cpp:640-644`);
/// `FacadePhraseIndex` then wraps it with `MAKE_TOKEN(7, 1)`. The reserved-id
/// bump in `_add_phrase` (`pinyin.cpp:592-594`) is a no-op on that value
/// because the low 24 bits are already non-zero. Starting `range_end` at 0
/// would hit the bump and land on the same token.
pub const FIRST_USER_TOKEN: Token = phrase_index_make_token(USER_DICTIONARY, 1);

/// Apply the reserved-zero skip and reject a token that has left
/// [`USER_DICTIONARY`].
#[must_use]
pub(crate) fn canonicalize_user_token(token: Token) -> Option<Token> {
    let token = if token & PHRASE_MASK == 0 {
        token.checked_add(1)?
    } else {
        token
    };
    is_user_token(token).then_some(token)
}

/// Next token after `token` under "max + 1" inside [`USER_DICTIONARY`].
#[must_use]
pub(crate) fn next_user_token_after(token: Token) -> Option<Token> {
    canonicalize_user_token(token.checked_add(1)?)
}

/// Little-endian `u16` packing of a key sequence. redb stores this as `&[u8]`.
#[must_use]
pub(crate) fn encode_keys(keys: &[PinyinKey]) -> Vec<u8> {
    let mut out = Vec::with_capacity(keys.len().saturating_mul(2));
    for key in keys {
        out.extend_from_slice(&key.to_le_bytes());
    }
    out
}

/// Inverse of [`encode_keys`]. An odd trailing byte is dropped (`chunks_exact`).
#[must_use]
pub(crate) fn decode_keys(bytes: &[u8]) -> Vec<PinyinKey> {
    bytes
        .chunks_exact(2)
        .map(|chunk| PinyinKey::from_le_bytes([chunk[0], chunk[1]]))
        .collect()
}

/// A user-phrase pronunciation: the pinyin key sequence and its stored count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserPronunciation {
    keys: Vec<PinyinKey>,
    count: u64,
}

impl UserPronunciation {
    /// Key sequence for this reading.
    #[must_use]
    pub fn keys(&self) -> &[PinyinKey] {
        &self.keys
    }

    /// Pronunciation count (`add_pronunciation`'s `delta`, §3.2).
    #[must_use]
    pub const fn count(&self) -> u64 {
        self.count
    }

    pub(crate) fn new(keys: Vec<PinyinKey>, count: u64) -> Self {
        Self { keys, count }
    }
}

/// A user phrase: its token, text, and pronunciations.
///
/// This is the lookup surface T3 needs to resolve a user token. T2 does not
/// wire it into Session or the C ABI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserPhrase {
    token: Token,
    text: String,
    pronunciations: Vec<UserPronunciation>,
}

impl UserPhrase {
    /// Allocated user token (library nibble [`USER_DICTIONARY`]).
    #[must_use]
    pub const fn token(&self) -> Token {
        self.token
    }

    /// Phrase text as stored (UTF-8; Unicode scalar length is the phrase
    /// length).
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Pronunciations in key-sequence order (redb composite-key order).
    #[must_use]
    pub fn pronunciations(&self) -> &[UserPronunciation] {
        &self.pronunciations
    }

    pub(crate) fn new(token: Token, text: String, pronunciations: Vec<UserPronunciation>) -> Self {
        Self {
            token,
            text,
            pronunciations,
        }
    }
}

/// Phrase length in Unicode scalar values — libpinyin's `ucs4_t` count.
#[must_use]
pub(crate) fn phrase_len(phrase: &str) -> usize {
    phrase.chars().count()
}

/// `true` when `phrase` and `keys` are a valid `_add_phrase` input: non-empty,
/// shorter than [`MAX_PHRASE_LENGTH`], and the same length.
#[must_use]
pub(crate) fn phrase_and_keys_valid(phrase: &str, keys: &[PinyinKey]) -> bool {
    let len = phrase_len(phrase);
    len > 0 && len < MAX_PHRASE_LENGTH && len == keys.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_match_pinned_spec() {
        // docs/findings/user-store.md §3 constants table + novel_types.h.
        assert_eq!(USER_DICTIONARY, 7);
        assert_eq!(PHRASE_MASK, 0x00FF_FFFF);
        assert_eq!(PHRASE_INDEX_LIBRARY_MASK, 0x0F00_0000);
        assert_eq!(MAX_PHRASE_LENGTH, 16);
        assert_eq!(DEFAULT_PHRASE_COUNT, 5);
        assert_eq!(ADD_PHRASE_UNIGRAM_FACTOR, 3);
    }

    #[test]
    fn make_token_and_library_index_match_macros() {
        assert_eq!(phrase_index_make_token(7, 1), 0x0700_0001);
        assert_eq!(phrase_index_make_token(7, 0), 0x0700_0000);
        assert_eq!(phrase_index_library_index(0x0700_0001), 7);
        assert_eq!(phrase_index_library_index(0x0100_0001), 1);
        assert_eq!(phrase_index_library_index(1), 0);
    }

    #[test]
    fn first_user_token_is_library_7_id_1() {
        assert_eq!(FIRST_USER_TOKEN, 0x0700_0001);
        assert!(is_user_token(FIRST_USER_TOKEN));
        assert_eq!(FIRST_USER_TOKEN & PHRASE_MASK, 1);
    }

    #[test]
    fn user_tokens_are_distinguished_by_library_nibble() {
        // pinyin_is_user_candidate: PHRASE_INDEX_LIBRARY_INDEX == USER_DICTIONARY.
        assert!(is_user_token(0x0700_0001));
        assert!(is_user_token(0x0700_0002));
        assert!(!is_user_token(0x0100_0001)); // GB_DICTIONARY
        assert!(!is_user_token(0x0200_0001)); // GBK_DICTIONARY
        assert!(!is_user_token(0x0500_0001)); // ADDON_DICTIONARY
        assert!(!is_user_token(1)); // sentence_start
        assert!(!is_user_token(0));
    }

    #[test]
    fn allocation_increments_by_one_inside_user_dictionary() {
        let first = FIRST_USER_TOKEN;
        let second = next_user_token_after(first).unwrap();
        let third = next_user_token_after(second).unwrap();
        assert_eq!(second, first + 1);
        assert_eq!(third, first + 2);
        assert!(is_user_token(second));
        assert!(is_user_token(third));
    }

    #[test]
    fn reserved_zero_id_is_skipped() {
        // token = 0x07000000 → low 24 bits zero → bump to 0x07000001.
        assert_eq!(
            canonicalize_user_token(phrase_index_make_token(USER_DICTIONARY, 0)),
            Some(FIRST_USER_TOKEN)
        );
    }

    #[test]
    fn allocation_stops_at_the_user_nibble_boundary() {
        // 0x07FFFFFF + 1 leaves USER_DICTIONARY (library 8).
        let last = phrase_index_make_token(USER_DICTIONARY, PHRASE_MASK);
        assert_eq!(last, 0x07FF_FFFF);
        assert!(is_user_token(last));
        assert_eq!(next_user_token_after(last), None);
    }

    #[test]
    fn phrase_length_is_unicode_scalars() {
        assert_eq!(phrase_len("你好"), 2);
        assert_eq!(phrase_len(""), 0);
        assert!(phrase_and_keys_valid("你好", &[1, 2]));
        assert!(!phrase_and_keys_valid("", &[]));
        assert!(!phrase_and_keys_valid("你好", &[1]));
        assert!(!phrase_and_keys_valid(&"啊".repeat(16), &[0; 16]));
        assert!(phrase_and_keys_valid(&"啊".repeat(15), &[0; 15]));
    }

    #[test]
    fn key_roundtrip_is_little_endian_u16() {
        let keys = [0x0001, 0x0100, 0xFFFF];
        assert_eq!(decode_keys(&encode_keys(&keys)), keys);
        assert_eq!(encode_keys(&keys), vec![1, 0, 0, 1, 0xFF, 0xFF]);
    }
}
