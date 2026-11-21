//! The DBM row layouts of a libpinyin system data directory, written
//! once for their two halves.
//!
//! Four of the six DBM files a data directory carries hold rows whose
//! `(key, value)` bytes are defined by upstream's own writers and read
//! back by upstream's own readers:
//!
//! | file | key | value |
//! |---|---|---|
//! | `pinyin_index.bin` | packed `ChewingKey[L]` | `PinyinIndexItem2<L>[]` ([`pinyin_index`]) |
//! | `phrase_index.bin` | UCS-4 phrase text | `phrase_token_t[]` ([`phrase_index`]) |
//! | `bigram.db` | `phrase_token_t` | `total` + `{next, count}[]` ([`crate::single_gram`]) |
//! | `punct.bin` | `phrase_token_t` | zero-terminated UCS-4 runs ([`punct`]) |
//!
//! oxpinyin has two halves that must agree on every one of those bytes:
//! [`crate`]'s lazy readers, which a runtime opens against an unmodified
//! libpinyin install, and `oxpinyin-utils`' storage utilities, which
//! compile the same files from model20 text. This module holds the encoders,
//! decoders, and strides both halves use, so a layout change is made
//! once — the same reason [`crate::data::chunk_format`] exists for the
//! `MemoryChunk` half, and the same consequence if it drifted: a wrong
//! byte here makes a file the other half rejects.
//!
//! Everything is little-endian on the supported targets (the fields are
//! host-endian upstream; every target oxpinyin and the pin share is
//! little-endian).
//!
//! Format notes: `docs/findings/data-formats.md` §4-§7.

#[cfg(not(target_endian = "little"))]
compile_error!(
    "row_format: the DBM row layouts use host-endian fields; this module \
     encodes and decodes them as little-endian. Big-endian targets are \
     not supported."
);

/// The DBM key of a `phrase_token_t`-keyed row — `bigram.db` and
/// `punct.bin` both address rows by the raw 4-byte little-endian token
/// (`ngram_tkrzwdb.cpp`, `punct_table.cpp`).
#[must_use]
pub const fn encode_token_key(token: u32) -> [u8; 4] {
    token.to_le_bytes()
}

/// Appends raw little-endian `u32` values.
fn append_u32_le(buf: &mut Vec<u8>, words: impl Iterator<Item = u32>) {
    for word in words {
        buf.extend_from_slice(&word.to_le_bytes());
    }
}

// ── pinyin_index.bin ─────────────────────────────────────────────

/// `pinyin_index.bin` / `addon_pinyin_index.bin` — `ChewingLargeTable2`.
///
/// Two key spaces share one file: the **complete** index (every tone
/// zeroed) and the **incomplete** index (each syllable reduced to its
/// initial). A key's value is the packed `PinyinIndexItem2<L>` array of
/// every phrase with that syllable sequence, and every proper prefix of
/// a stored key exists as an empty-value `SEARCH_CONTINUED` marker.
///
/// `docs/findings/data-formats.md` §4,
/// `chewing_large_table2_tkrzwdb.cpp:133-296`.
pub mod pinyin_index {
    use crate::parser::ChewingKey;

    use crate::data::DictError;
    use crate::data::phrase_library::CHEWING_KEY_SIZE;

    /// The C++ `sizeof(PinyinIndexItem2<L>)` with tail padding to 4-byte
    /// alignment. Field sum is `4 + 2*L`; C++ rounds up to the next
    /// multiple of 4 (the `u32 token` field's alignment).
    ///
    /// ```text
    /// L=1: 4+2 = 6 → 8
    /// L=2: 4+4 = 8 → 8
    /// L=3: 4+6 = 10 → 12
    /// L=4: 4+8 = 12 → 12
    /// ```
    #[must_use]
    pub(crate) const fn item2_stride(phrase_length: usize) -> usize {
        let raw = 4 + CHEWING_KEY_SIZE * phrase_length;
        (raw + 3) & !3
    }

    /// The packed two-byte little-endian form of one key.
    const fn pack(key: ChewingKey) -> [u8; CHEWING_KEY_SIZE] {
        key.to_packed().to_le_bytes()
    }

    /// Packs a `ChewingKey` slice into the DBM key for the **complete**
    /// index: every tone zeroed, each key as 2 LE bytes.
    ///
    /// `compute_chewing_index` (`pinyin_phrase3.h:160`);
    /// `chewing_large_table2_tkrzwdb.cpp:221-232` (`search`) zeroes every
    /// key's tone, then encodes the array as the lookup key.
    #[must_use]
    pub fn encode_complete_key(keys: &[ChewingKey]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(keys.len() * CHEWING_KEY_SIZE);
        for key in keys {
            buf.extend_from_slice(&pack(ChewingKey::new(
                key.initial,
                key.middle,
                key.final_,
                0,
            )));
        }
        buf
    }

    /// Packs a `ChewingKey` slice into the DBM key for the **incomplete**
    /// (initial-only) index: each key reduced to its `m_initial` only
    /// (middle, final, tone all zero), as 2 LE bytes.
    ///
    /// `compute_incomplete_chewing_index` (`pinyin_phrase3.h:171`): the
    /// two key spaces coexist in one DBM; the incomplete key space uses
    /// only the initial bits.
    #[must_use]
    pub fn encode_incomplete_key(keys: &[ChewingKey]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(keys.len() * CHEWING_KEY_SIZE);
        for key in keys {
            buf.extend_from_slice(&pack(ChewingKey::new(key.initial, 0, 0, 0)));
        }
        buf
    }

    /// One `PinyinIndexItem2<L>`: a phrase token and its stored
    /// pronunciation keys (original tones preserved).
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct PinyinIndexItem {
        /// The row's `phrase_token_t`.
        pub token: u32,
        /// The stored pronunciation keys, with their tones.
        pub keys: Vec<ChewingKey>,
    }

    /// One validated record, borrowing its packed pronunciation from the row.
    pub(crate) struct PinyinIndexItemView<'a> {
        pub token: u32,
        keys: &'a [u8],
    }

    impl PinyinIndexItemView<'_> {
        pub(crate) fn keys(&self) -> impl ExactSizeIterator<Item = ChewingKey> + '_ {
            self.keys
                .as_chunks::<2>()
                .0
                .iter()
                .map(|bytes| ChewingKey::from_packed(u16::from_le_bytes(*bytes)))
        }
    }

    /// Yields a row's whole records without owning their keys.
    ///
    /// A trailing partial record is dropped. The pin reads the row as
    /// `IndexItem` pointers from `m_chunk.begin()` to `m_chunk.end()`
    /// (`chewing_large_table2.h:145-150`, `:197-202` at 074a2219), and the
    /// pointer difference counts whole records. For a stride of 8, 16 or 32
    /// bytes (1, 2, 5, 6, 13 or 14 keys) that count is exact; for any other
    /// stride it is undefined, and oxpinyin takes the same whole-record count.
    pub(crate) fn item_views(
        value: &[u8],
        phrase_length: usize,
    ) -> Result<impl ExactSizeIterator<Item = PinyinIndexItemView<'_>>, DictError> {
        let (stride, key_bytes) = if value.is_empty() {
            (1, 0)
        } else {
            if phrase_length == 0 {
                return Err(DictError::Parse);
            }
            let key_bytes = CHEWING_KEY_SIZE * phrase_length;
            (item2_stride(phrase_length), key_bytes)
        };
        Ok(value
            .chunks_exact(stride)
            .map(move |record| PinyinIndexItemView {
                token: u32::from_le_bytes(record.as_chunks::<4>().0[0]),
                keys: &record[4..4 + key_bytes],
            }))
    }

    // PinyinIndexItem2<L>, chewing_large_table2.h:91–133. The row key determines L.
    /// Encodes one token and its packed keys, including the original tones and zero padding.
    /// Supply at least one key. Use the same key count for every record in a DBM value and
    /// pass that count to [`decode_items`].
    #[must_use]
    pub fn encode_item(token: u32, keys: &[ChewingKey]) -> Vec<u8> {
        let mut buf = vec![0_u8; item2_stride(keys.len())];
        buf[..4].copy_from_slice(&token.to_le_bytes());
        let (words, _) = buf[4..].as_chunks_mut::<CHEWING_KEY_SIZE>();
        for (word, key) in words.iter_mut().zip(keys) {
            *word = pack(*key);
        }
        buf
    }

    /// Serialises a whole DBM value: the records back to back, each at
    /// `item2_stride`.
    ///
    /// Fixture helper, not part of the schema's surface: the compiler
    /// writes values record by record through [`encode_item`], and
    /// nothing outside this crate's tests assembles one from
    /// [`PinyinIndexItem`]s. Kept crate- and test-scoped so the
    /// mixed-`L` value it would happily build for a caller who passes
    /// items of differing key counts is not reachable from the public
    /// API — see [`encode_item`] for that contract.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn encode_items(items: &[PinyinIndexItem]) -> Vec<u8> {
        let mut buf = Vec::new();
        for item in items {
            buf.extend_from_slice(&encode_item(item.token, &item.keys));
        }
        buf
    }

    // PinyinIndexItem2<L>, chewing_large_table2.h:91–133. item_views uses chunks_exact and
    // drops a trailing partial record.
    /// Decodes complete records from one pinyin-index value. Each record has a little-endian
    /// token, `phrase_length` packed two-byte keys and padding to the record stride. An empty
    /// value marks a prefix. Supply a whole number of records for the selected stride.
    ///
    /// # Errors
    ///
    /// Returns [`DictError::Parse`] for a nonempty value with zero phrase length.
    pub fn decode_items(
        value: &[u8],
        phrase_length: usize,
    ) -> Result<Vec<PinyinIndexItem>, DictError> {
        Ok(item_views(value, phrase_length)?
            .map(|item| PinyinIndexItem {
                token: item.token,
                keys: item.keys().collect(),
            })
            .collect())
    }
}

// ── phrase_index.bin ─────────────────────────────────────────────

/// `phrase_index.bin` / `addon_phrase_index.bin` — `PhraseLargeTable3`.
///
/// Key: the phrase text as raw UCS-4 (`g_utf8_to_ucs4`), 4 LE bytes per
/// character. Value: the phrase's `phrase_token_t`s as ascending `u32`
/// values. Every proper UCS-4 prefix of every phrase exists as an
/// empty-value continuation marker.
///
/// `docs/findings/data-formats.md` §5,
/// `phrase_large_table3_tkrzwdb.cpp`.
pub mod phrase_index {
    use crate::data::DictError;

    use super::append_u32_le;

    /// Encodes a UTF-8 string into a UCS-4 DBM key (each char as `u32`
    /// LE) — how libpinyin encodes phrase text for the phrase index.
    ///
    /// `g_utf8_to_ucs4` produces a `gunichar[]` (= `guint32[]`), stored
    /// as raw bytes in native (LE) byte order.
    #[must_use]
    pub fn encode_ucs4_key(text: &str) -> Vec<u8> {
        let mut buf = Vec::with_capacity(text.len() * 4);
        append_u32_le(&mut buf, text.chars().map(u32::from));
        buf
    }

    /// [`encode_ucs4_key`] over code points already decoded — the form
    /// the compiler carries phrase text in.
    #[must_use]
    pub fn encode_ucs4_key_scalars(codes: &[u32]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(codes.len() * 4);
        append_u32_le(&mut buf, codes.iter().copied());
        buf
    }

    /// Encodes a slice of tokens into a DBM value: a flat array of `u32`
    /// LE, 4 bytes each.
    #[must_use]
    pub fn encode_tokens(tokens: &[u32]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(tokens.len() * 4);
        append_u32_le(&mut buf, tokens.iter().copied());
        buf
    }

    /// Decodes a DBM value into phrase tokens. Returns an empty Vec for
    /// an empty value (prefix marker).
    ///
    /// # Errors
    ///
    /// Returns `DictError::Parse` if the value length is not a multiple
    /// of 4.
    pub fn decode_tokens(value: &[u8]) -> Result<Vec<u32>, DictError> {
        if value.is_empty() {
            return Ok(Vec::new());
        }
        if !value.len().is_multiple_of(4) {
            return Err(DictError::Parse);
        }
        Ok(value
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| u32::from_le_bytes(*chunk))
            .collect())
    }
}

// ── punct.bin ────────────────────────────────────────────────────

/// `punct.bin` — the predicted-punctuation table.
///
/// Key: a `phrase_token_t` ([`crate::data::row_format::encode_token_key`]). Value:
/// `PunctTableEntry::escape`'s layout (`punct_table.cpp:40-54`) — each
/// punctuation's UCS-4 code points followed by a `u32` zero terminator,
/// successive punctuations concatenated.
pub mod punct {
    use crate::vocab::PhraseText;

    #[cfg(any(test, feature = "test-hooks"))]
    use super::append_u32_le;

    /// Encodes punctuation strings as a UCS-4 stream with `u32` zero
    /// terminators — `PunctTableEntry::escape`'s layout.
    #[cfg(any(test, feature = "test-hooks"))]
    #[must_use]
    pub fn encode_puncts<S: AsRef<str>>(puncts: &[S]) -> Vec<u8> {
        let mut buf = Vec::new();
        for punct in puncts {
            append_u32_le(&mut buf, punct.as_ref().chars().map(u32::from));
            buf.extend_from_slice(&0_u32.to_le_bytes());
        }
        buf
    }

    /// Decodes a punctuation value as the pin walks it: the stored words
    /// of each punctuation, which the C boundary encodes
    /// (`PunctTableEntry::unescape` and `get_all_punctuations`,
    /// `punct_table.cpp:57-94` at 074a2219).
    ///
    /// Each punctuation is the word at the cursor followed by the nonzero
    /// words after it, up to a zero or the value's last whole word; the
    /// cursor then steps one word past that run, over the zero or past
    /// the end. Any stored word is kept: a non-scalar is encoded with
    /// GLib's historical form at the C boundary, and a zero first word
    /// gives an empty punctuation, as `g_strdup` cuts the pin's string
    /// there. A value with no final zero ends with its unterminated run;
    /// the pin decides that by reading one word past the value, and both
    /// outcomes of the read end the same run (upstream divergences,
    /// "Unterminated punctuation value over-read"). Partial trailing bytes
    /// are ignored: only complete stored words belong to the value.
    #[must_use]
    pub fn decode_puncts(value: &[u8]) -> Vec<PhraseText> {
        let words: Vec<u32> = value
            .as_chunks::<4>()
            .0
            .iter()
            .map(|chunk| u32::from_le_bytes(*chunk))
            .collect();
        let mut out = Vec::new();
        let mut cursor = 0_usize;
        while cursor < words.len() {
            let rest = words.get(cursor..).unwrap_or_default();
            let run = match rest.split_first() {
                Some((_, tail)) => 1 + tail.iter().take_while(|&&word| word != 0).count(),
                None => 0,
            };
            out.push(punct_text(&rest[..run]));
            cursor += run + 1;
        }
        out
    }

    /// Ordinary text when every word is a nonzero scalar; otherwise the
    /// stored words.
    fn punct_text(words: &[u32]) -> PhraseText {
        let text: Option<String> = words
            .iter()
            .map(|&word| char::from_u32(word).filter(|_| word != 0))
            .collect();
        text.map_or_else(
            || PhraseText::from_stored_parts(words.to_vec(), words.len()),
            PhraseText::from,
        )
    }
}

#[cfg(test)]
mod tests;
