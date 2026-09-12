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
//! | `bigram.db` | `phrase_token_t` | `total` + `{next, count}[]` ([`bigram`]) |
//! | `punct.bin` | `phrase_token_t` | zero-terminated UCS-4 runs ([`punct`]) |
//!
//! oxpinyin has two halves that must agree on every one of those bytes:
//! [`crate`]'s lazy readers, which a runtime opens against an unmodified
//! libpinyin install, and `oxpinyin-datagen`'s producers, which compile
//! the same files from model20 text. This module holds the encoders,
//! decoders, and strides both halves use, so a layout change is made
//! once — the same reason [`crate::chunk_format`] exists for the
//! `MemoryChunk` half, and the same consequence if it drifted: a wrong
//! byte here makes a file the other half rejects.
//!
//! Everything is little-endian on the supported targets (the fields are
//! host-endian upstream; every target oxpinyin and the pin share is
//! little-endian).
//!
//! Format notes: `docs/findings/pinyin-dbm-format-2026-09-01.md`,
//! `docs/findings/phrase-dbm-format-2026-09-01.md`,
//! `docs/findings/bigram-punct-format-2026-09-01.md`.

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

/// Appends UCS-4 code points as raw little-endian `u32`s — the byte form
/// `g_utf8_to_ucs4`'s `gunichar[]` has on disk, shared by the phrase
/// index's keys and the punctuation table's values.
fn push_ucs4(buf: &mut Vec<u8>, codes: impl Iterator<Item = u32>) {
    for code in codes {
        buf.extend_from_slice(&code.to_le_bytes());
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
/// `docs/findings/libpinyin-system-data-formats-2026-09-01.md` §1.3,
/// `chewing_large_table2_tkrzwdb.cpp:133-296`.
pub mod pinyin_index {
    use oxpinyin_core::ChewingKey;

    use crate::dict::DictError;

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
    pub const fn item2_stride(phrase_length: usize) -> usize {
        let raw = 4 + 2 * phrase_length;
        (raw + 3) & !3
    }

    /// The packed two-byte little-endian form of one key.
    const fn pack(key: ChewingKey) -> [u8; 2] {
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
        let mut buf = Vec::with_capacity(keys.len() * 2);
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
        let mut buf = Vec::with_capacity(keys.len() * 2);
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

    /// Serialises one `PinyinIndexItem2<L>` record: token, then the
    /// stored keys (their original tones), then zero padding to
    /// [`item2_stride`].
    ///
    /// `keys.len()` **is** the record's `L`, and a DBM value is a packed
    /// array of records that all share one `L` — the phrase length
    /// [`decode_items`] is later handed, which upstream derives from the
    /// key (`key.len() / 2` syllables). So a caller assembling a value
    /// must pass the same non-empty `keys` length for every record in it;
    /// `L = 0` and mixed lengths do not name any value this format can
    /// express.
    ///
    /// Both are unreachable from the compiler that writes these files —
    /// `parse_pinyin_keys` refuses an empty syllable, and rows are grouped
    /// by their encoded key, so one value's records share a syllable count
    /// by construction. Neither is guarded here, because the guard that
    /// matters is on the read side: [`decode_items`] rejects `L = 0` and
    /// any value whose length is not a whole multiple of its stride, so a
    /// record stream written out of contract is refused rather than
    /// misparsed.
    #[must_use]
    pub fn encode_item(token: u32, keys: &[ChewingKey]) -> Vec<u8> {
        let mut buf = vec![0_u8; item2_stride(keys.len())];
        buf[..4].copy_from_slice(&token.to_le_bytes());
        for (index, key) in keys.iter().enumerate() {
            buf[4 + 2 * index..6 + 2 * index].copy_from_slice(&pack(*key));
        }
        buf
    }

    /// Serialises a whole DBM value: the records back to back, each at
    /// [`item2_stride`].
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

    /// Decodes a DBM value into `PinyinIndexItem2<L>` records.
    ///
    /// The value is a packed array of C++ structs with stride
    /// [`item2_stride`]`(phrase_length)`. Each record contains:
    /// - `u32 token` (LE, offset 0)
    /// - `ChewingKey keys[phrase_length]` (2 bytes each, offset 4)
    /// - padding to the stride
    ///
    /// Returns an empty Vec for an empty value (a prefix marker).
    ///
    /// # Errors
    ///
    /// Returns `DictError::Parse` if the value length is not a multiple
    /// of the stride, or if any record is too short to contain its
    /// fields.
    pub fn decode_items(
        value: &[u8],
        phrase_length: usize,
    ) -> Result<Vec<PinyinIndexItem>, DictError> {
        if value.is_empty() {
            return Ok(Vec::new());
        }
        if phrase_length == 0 {
            return Err(DictError::Parse(
                "phrase_length must be at least 1".to_owned(),
            ));
        }
        let stride = item2_stride(phrase_length);
        if !value.len().is_multiple_of(stride) {
            return Err(DictError::Parse(format!(
                "pinyin index value length {} is not a multiple of stride {} (L={})",
                value.len(),
                stride,
                phrase_length,
            )));
        }
        let count = value.len() / stride;
        let mut items = Vec::with_capacity(count);
        for i in 0..count {
            let base = i * stride;
            let token = u32::from_le_bytes([
                value[base],
                value[base + 1],
                value[base + 2],
                value[base + 3],
            ]);
            let mut keys = Vec::with_capacity(phrase_length);
            for j in 0..phrase_length {
                let key_offset = base + 4 + j * 2;
                if key_offset + 2 > value.len() {
                    return Err(DictError::Parse(format!(
                        "record {i} truncated at key {j} (offset {key_offset}, value len {})",
                        value.len(),
                    )));
                }
                let packed = u16::from_le_bytes([value[key_offset], value[key_offset + 1]]);
                keys.push(ChewingKey::from_packed(packed));
            }
            items.push(PinyinIndexItem { token, keys });
        }
        Ok(items)
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
/// `docs/findings/libpinyin-system-data-formats-2026-09-01.md` §1.1,
/// `phrase_large_table3_tkrzwdb.cpp`.
pub mod phrase_index {
    use crate::dict::DictError;

    use super::push_ucs4;

    /// Encodes a UTF-8 string into a UCS-4 DBM key (each char as `u32`
    /// LE) — how libpinyin encodes phrase text for the phrase index.
    ///
    /// `g_utf8_to_ucs4` produces a `gunichar[]` (= `guint32[]`), stored
    /// as raw bytes in native (LE) byte order.
    #[must_use]
    pub fn encode_ucs4_key(text: &str) -> Vec<u8> {
        let mut buf = Vec::with_capacity(text.len() * 4);
        push_ucs4(&mut buf, text.chars().map(u32::from));
        buf
    }

    /// [`encode_ucs4_key`] over code points already decoded — the form
    /// the compiler carries phrase text in.
    #[must_use]
    pub fn encode_ucs4_key_scalars(codes: &[u32]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(codes.len() * 4);
        push_ucs4(&mut buf, codes.iter().copied());
        buf
    }

    /// Decodes a UCS-4 DBM key back to a UTF-8 string.
    ///
    /// Returns `None` if the key length is not a multiple of 4 or if any
    /// 4-byte group is not a valid Unicode scalar value.
    #[must_use]
    pub fn decode_ucs4_key(key: &[u8]) -> Option<String> {
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

    /// Encodes a slice of tokens into a DBM value: a flat array of `u32`
    /// LE, 4 bytes each.
    #[must_use]
    pub fn encode_tokens(tokens: &[u32]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(tokens.len() * 4);
        for token in tokens {
            buf.extend_from_slice(&token.to_le_bytes());
        }
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
}

// ── bigram.db ────────────────────────────────────────────────────

/// `bigram.db` — the `Bigram` hash container's `SingleGram` rows.
///
/// Key: the previous `phrase_token_t` ([`crate::row_format::encode_token_key`]).
/// Value: `total: u32` then `{next_token: u32, count: u32}` records,
/// token-ascending (`SingleGram::insert_freq`).
///
/// `docs/findings/bigram-punct-format-2026-09-01.md`, `ngram_tkrzwdb.cpp`.
pub mod bigram {
    use crate::dict::DictError;

    /// One previous-token row of the system bigram.
    ///
    /// `total` is the stored row total and equals `Σ count` over
    /// [`BigramRow::records`].
    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct BigramRow {
        /// Sum of the successor counts.
        pub total: u32,
        /// `(next_token, count)` records, stored order.
        pub records: Vec<(u32, u32)>,
    }

    /// Encodes a bigram row into a DBM value: 4 bytes `total` then
    /// 8-byte records.
    #[must_use]
    pub fn encode_value(row: &BigramRow) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + row.records.len() * 8);
        buf.extend_from_slice(&row.total.to_le_bytes());
        for &(next, count) in &row.records {
            buf.extend_from_slice(&next.to_le_bytes());
            buf.extend_from_slice(&count.to_le_bytes());
        }
        buf
    }

    /// Decodes a bigram value as `(total, [{next_token, count}])`.
    ///
    /// # Errors
    ///
    /// Returns `DictError::Parse` when the value length is not `4 + 8n`.
    pub fn parse_value(data: &[u8]) -> Result<BigramRow, DictError> {
        if data.len() < 4 || !(data.len() - 4).is_multiple_of(8) {
            return Err(DictError::Parse(format!(
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
        Ok(BigramRow { total, records })
    }
}

// ── punct.bin ────────────────────────────────────────────────────

/// `punct.bin` — the predicted-punctuation table.
///
/// Key: a `phrase_token_t` ([`crate::row_format::encode_token_key`]). Value:
/// `PunctTableEntry::escape`'s layout (`punct_table.cpp:40-54`) — each
/// punctuation's UCS-4 code points followed by a `u32` zero terminator,
/// successive punctuations concatenated.
///
/// `docs/findings/prediction-punct.md`.
pub mod punct {
    use crate::dict::DictError;

    use super::push_ucs4;

    /// Encodes punctuation strings as a UCS-4 stream with `u32` zero
    /// terminators — `PunctTableEntry::escape`'s layout.
    #[must_use]
    pub fn encode_puncts<S: AsRef<str>>(puncts: &[S]) -> Vec<u8> {
        let mut buf = Vec::new();
        for punct in puncts {
            push_ucs4(&mut buf, punct.as_ref().chars().map(u32::from));
            buf.extend_from_slice(&0_u32.to_le_bytes());
        }
        buf
    }

    /// Decodes a UCS-4 punctuation stream — `PunctTableEntry::unescape`
    /// + `get_all_punctuations` (`punct_table.cpp:56-94`).
    ///
    /// # Errors
    ///
    /// Returns `DictError::Parse` when the value is not u32-aligned, ends
    /// without a terminator, or holds an undecodable scalar. Upstream
    /// reads past the buffer on such input (memory-safety class); the
    /// Rust reader refuses it instead
    /// (`docs/findings/upstream-divergences.md`).
    pub fn decode_puncts(value: &[u8]) -> Result<Vec<String>, DictError> {
        if !value.len().is_multiple_of(4) {
            return Err(DictError::Parse(
                "punct value is not u32-aligned".to_owned(),
            ));
        }
        let mut out = Vec::new();
        let mut current = String::new();
        let mut terminated = false;
        for chunk in value.chunks_exact(4) {
            let code = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            if code == 0 {
                if current.is_empty() {
                    return Err(DictError::Parse(
                        "punct value holds an empty punctuation".to_owned(),
                    ));
                }
                out.push(std::mem::take(&mut current));
                terminated = true;
                continue;
            }
            terminated = false;
            let Some(scalar) = char::from_u32(code) else {
                return Err(DictError::Parse(
                    "punct value holds an invalid UCS-4 scalar".to_owned(),
                ));
            };
            current.push(scalar);
        }
        if !terminated {
            return Err(DictError::Parse(
                "punct value is not zero-terminated".to_owned(),
            ));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use oxpinyin_core::ChewingKey;

    // Pinned vectors: a change to the shared implementation breaks both
    // halves' tests at once, the same guarantee `chunk_format` carries.

    #[test]
    fn token_key_is_the_little_endian_word() {
        assert_eq!(encode_token_key(0x0100_0295), [0x95, 0x02, 0x00, 0x01]);
    }

    #[test]
    fn item2_stride_matches_upstream_sizeof() {
        use pinyin_index::item2_stride;
        assert_eq!(item2_stride(1), 8, "L=1: 4+2=6 padded to 8");
        assert_eq!(item2_stride(2), 8, "L=2: 4+4=8 no padding");
        assert_eq!(item2_stride(3), 12, "L=3: 4+6=10 padded to 12");
        assert_eq!(item2_stride(4), 12, "L=4: 4+8=12 no padding");
        assert_eq!(item2_stride(5), 16, "L=5: 4+10=14 padded to 16");
        assert_eq!(item2_stride(6), 16, "L=6: 4+12=16 no padding");
        assert_eq!(item2_stride(16), 36, "L=16: max libpinyin phrase length");
    }

    #[test]
    fn encode_complete_key_zeroes_tone() {
        let keys = [ChewingKey::new(1, 0, 2, 3)];
        let encoded = pinyin_index::encode_complete_key(&keys);
        assert_eq!(encoded.len(), 2);
        let expected = ChewingKey::new(1, 0, 2, 0).to_packed().to_le_bytes();
        assert_eq!(encoded, expected);
    }

    #[test]
    fn encode_incomplete_key_keeps_only_initial() {
        let keys = [ChewingKey::new(5, 1, 3, 2)];
        let encoded = pinyin_index::encode_incomplete_key(&keys);
        assert_eq!(encoded.len(), 2);
        let expected = ChewingKey::new(5, 0, 0, 0).to_packed().to_le_bytes();
        assert_eq!(encoded, expected);
    }

    #[test]
    fn encode_multi_syllable_key() {
        let keys = [ChewingKey::new(1, 0, 2, 0), ChewingKey::new(3, 1, 4, 0)];
        let encoded = pinyin_index::encode_complete_key(&keys);
        assert_eq!(encoded.len(), 4);
        let k0 = ChewingKey::new(1, 0, 2, 0).to_packed().to_le_bytes();
        let k1 = ChewingKey::new(3, 1, 4, 0).to_packed().to_le_bytes();
        assert_eq!(&encoded[0..2], &k0);
        assert_eq!(&encoded[2..4], &k1);
    }

    #[test]
    fn round_trip_multi_key_item_and_multi_record_value() {
        use pinyin_index::{PinyinIndexItem, decode_items, encode_items, item2_stride};
        let ni_hao = PinyinIndexItem {
            token: 0x0100_0099,
            keys: vec![
                ChewingKey::new(14, 0, 7, 3), // ni3
                ChewingKey::new(8, 0, 2, 3),  // hao3
            ],
        };
        let encoded = encode_items(std::slice::from_ref(&ni_hao));
        assert_eq!(encoded.len(), item2_stride(2));
        assert_eq!(decode_items(&encoded, 2).unwrap(), vec![ni_hao]);

        // Two L=1 records in one value: each lands at its own stride.
        let items = vec![
            PinyinIndexItem {
                token: 0x0100_0001,
                keys: vec![ChewingKey::new(1, 0, 2, 1)],
            },
            PinyinIndexItem {
                token: 0x0100_0002,
                keys: vec![ChewingKey::new(1, 0, 2, 3)],
            },
        ];
        let encoded = encode_items(&items);
        assert_eq!(encoded.len(), item2_stride(1) * 2);
        assert_eq!(decode_items(&encoded, 1).unwrap(), items);
    }

    #[test]
    fn round_trip_item_encoding() {
        use pinyin_index::{PinyinIndexItem, decode_items, encode_item, item2_stride};
        let item = PinyinIndexItem {
            token: 0x0100_0042,
            keys: vec![ChewingKey::new(2, 0, 3, 1)],
        };
        let encoded = encode_item(item.token, &item.keys);
        assert_eq!(encoded.len(), item2_stride(1));
        // The padding byte the stride adds is zero, not struct garbage.
        assert_eq!(&encoded[6..8], &[0, 0]);
        let decoded = decode_items(&encoded, 1).unwrap();
        assert_eq!(decoded, vec![item]);
    }

    #[test]
    fn decode_items_rejects_misaligned_and_zero_length() {
        use pinyin_index::decode_items;
        assert!(decode_items(&[0; 7], 1).is_err());
        assert!(decode_items(&[0; 8], 0).is_err());
        assert!(decode_items(&[], 1).unwrap().is_empty());
    }

    /// `encode_item`'s contract, enforced where it matters: a record
    /// stream written outside it is refused by the reader, not
    /// misparsed. Both shapes are unreachable from the compiler; this
    /// pins that neither could slip past as a plausible value if one
    /// ever were.
    #[test]
    fn a_record_stream_written_out_of_contract_is_refused_on_read() {
        use pinyin_index::{decode_items, encode_item, item2_stride};
        // L = 0: four bytes of bare token, which names no phrase length.
        let degenerate = encode_item(0x0100_0001, &[]);
        assert_eq!(degenerate.len(), 4);
        assert!(decode_items(&degenerate, 0).is_err(), "L = 0 is refused");
        assert!(
            decode_items(&degenerate, 1).is_err(),
            "and it is not a whole L=1 record either"
        );
        // Mixed L in one value: an L=1 record followed by an L=3 record
        // is 8 + 12 bytes, which no single stride divides evenly.
        let mut mixed = encode_item(0x0100_0001, &[ChewingKey::new(1, 0, 2, 0)]);
        mixed.extend_from_slice(&encode_item(
            0x0100_0002,
            &[
                ChewingKey::new(1, 0, 2, 0),
                ChewingKey::new(3, 0, 4, 0),
                ChewingKey::new(5, 0, 6, 0),
            ],
        ));
        assert_eq!(mixed.len(), item2_stride(1) + item2_stride(3));
        assert!(
            decode_items(&mixed, 1).is_err(),
            "20 is not a multiple of 8"
        );
        assert!(
            decode_items(&mixed, 3).is_err(),
            "nor of 12 — neither length claims the stream"
        );
    }

    #[test]
    fn ucs4_key_round_trip_and_rejections() {
        use phrase_index::{decode_ucs4_key, encode_ucs4_key, encode_ucs4_key_scalars};
        let key = encode_ucs4_key("你好");
        assert_eq!(key.len(), 8);
        assert_eq!(
            u32::from_le_bytes(key[0..4].try_into().unwrap()),
            0x4F60,
            "你 = U+4F60"
        );
        // The two entry points agree byte for byte.
        assert_eq!(key, encode_ucs4_key_scalars(&[0x4F60, 0x597D]));
        assert_eq!(decode_ucs4_key(&key).as_deref(), Some("你好"));
        assert!(decode_ucs4_key(&[0, 0, 0]).is_none());
        let mut bad = encode_ucs4_key("x");
        bad[0..4].copy_from_slice(&0xD800_u32.to_le_bytes());
        assert!(decode_ucs4_key(&bad).is_none(), "surrogate is not a scalar");
    }

    #[test]
    fn phrase_tokens_round_trip() {
        use phrase_index::{decode_tokens, encode_tokens};
        let tokens = vec![0x0100_0001, 0x0200_0042];
        let encoded = encode_tokens(&tokens);
        assert_eq!(encoded.len(), 8);
        assert_eq!(decode_tokens(&encoded).unwrap(), tokens);
        assert!(decode_tokens(&[]).unwrap().is_empty(), "prefix marker");
        assert!(decode_tokens(&[0, 0, 0]).is_err());
    }

    #[test]
    fn bigram_value_round_trip() {
        use bigram::{BigramRow, encode_value, parse_value};
        let row = BigramRow {
            total: 100,
            records: vec![(0x0100_0010, 60), (0x0100_0020, 40)],
        };
        let encoded = encode_value(&row);
        assert_eq!(encoded.len(), 4 + 2 * 8);
        assert_eq!(parse_value(&encoded).unwrap(), row);
        assert!(parse_value(&[0, 0, 0]).is_err(), "shorter than the header");
        assert!(parse_value(&[0; 7]).is_err(), "not 4 + 8n");
    }

    #[test]
    fn punct_stream_round_trip() {
        use punct::{decode_puncts, encode_puncts};
        let encoded = encode_puncts(&["，", "。"]);
        // ， = U+FF0C, 。 = U+3002, each zero-terminated as u32.
        let mut want = Vec::new();
        want.extend_from_slice(&0xFF0C_u32.to_le_bytes());
        want.extend_from_slice(&0_u32.to_le_bytes());
        want.extend_from_slice(&0x3002_u32.to_le_bytes());
        want.extend_from_slice(&0_u32.to_le_bytes());
        assert_eq!(encoded, want);
        assert_eq!(decode_puncts(&encoded).unwrap(), vec!["，", "。"]);
        // Owned strings encode identically to borrowed ones.
        assert_eq!(encoded, encode_puncts(&["，".to_owned(), "。".to_owned()]));
        assert!(decode_puncts("，".as_bytes()).is_err(), "not u32-aligned");
        assert!(decode_puncts(b"\x00").is_err(), "not u32-aligned");
        assert!(
            decode_puncts(&0xFF0C_u32.to_le_bytes()).is_err(),
            "unterminated"
        );
        assert!(decode_puncts(b"").is_err(), "no terminator at all");
    }

    /// A terminator with nothing before it is corruption, not an empty
    /// punctuation: upstream never stores one, and admitting it would
    /// hand the prediction path a zero-length candidate.
    #[test]
    fn punct_stream_rejects_an_empty_field() {
        use punct::decode_puncts;
        assert!(decode_puncts(&0_u32.to_le_bytes()).is_err(), "leading");
        let mut trailing = Vec::new();
        trailing.extend_from_slice(&0xFF0C_u32.to_le_bytes());
        trailing.extend_from_slice(&0_u32.to_le_bytes());
        trailing.extend_from_slice(&0_u32.to_le_bytes());
        assert!(decode_puncts(&trailing).is_err(), "trailing");
    }
}
