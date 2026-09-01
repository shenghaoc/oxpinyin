//! Converts datagen entries to libpinyin-compatible key/value encodings.
//!
//! The existing datagen produces:
//! - `pinyin_index`: apostrophe-joined UTF-8 keys, `{token:u32, freq:u32}[]` values
//! - `phrase_index`: token u32 LE keys, UTF-8 text values
//!
//! libpinyin expects:
//! - `pinyin_index.bin`: packed `ChewingKey[L]` keys (tone-zeroed), `PinyinIndexItem2<L>[]` values
//! - `phrase_index.bin`: UCS-4 text keys, `u32 token[]` values
//!
//! The bigram and punctuation formats are already byte-compatible.
//!
//! This module converts the intermediate `Entries` from the system/addon
//! compilers into the libpinyin-compatible representation, using the same
//! `ChewingKey::from_pinyin` encoding the P2 reader decodes.

use std::collections::BTreeMap;

use oxpinyin_core::ChewingKey;

use crate::{DatagenError, Entries};

// ── Pinyin index conversion ──────────────────────────────────────

/// Converts pinyin_index entries from apostrophe-joined UTF-8 keys to
/// packed ChewingKey format with PinyinIndexItem2 values and prefix
/// markers.
///
/// Input: `(apostrophe-joined pinyin, {token:u32, freq:u32}[])`
/// Output: `(packed ChewingKey[L] tone-zeroed, PinyinIndexItem2<L>[])`
///         plus empty-value prefix markers for SEARCH_CONTINUED
///         plus incomplete (initial-only) index entries.
pub fn convert_pinyin_index(entries: &Entries) -> Result<Entries, DatagenError> {
    let mut complete: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut incomplete: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut prefix_markers: BTreeMap<Vec<u8>, ()> = BTreeMap::new();

    for (key_bytes, value_bytes) in entries {
        let pinyin = std::str::from_utf8(key_bytes)
            .map_err(|_| DatagenError::Consistency("pinyin index key is not UTF-8".to_owned()))?;

        let syllables: Vec<&str> = pinyin.split('\'').collect();
        let chewing_keys: Vec<ChewingKey> = syllables
            .iter()
            .filter_map(|s| ChewingKey::from_pinyin(s))
            .collect();

        if chewing_keys.len() != syllables.len() {
            continue;
        }

        if value_bytes.len() % 8 != 0 {
            return Err(DatagenError::Consistency(format!(
                "pinyin index value length {} is not a multiple of 8 for key {pinyin:?}",
                value_bytes.len()
            )));
        }

        let phrase_length = chewing_keys.len();

        // Parse the {token, freq} records from the existing format.
        let records: Vec<(u32, u32)> = value_bytes
            .chunks_exact(8)
            .map(|chunk| {
                (
                    u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                    u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
                )
            })
            .collect();

        // Encode the complete key (tone-zeroed).
        let complete_key = encode_complete_key(&chewing_keys);

        // Encode the value as PinyinIndexItem2<L>[].
        // The existing freq is the pronunciation frequency. In the upstream
        // format, the PinyinIndexItem2 stores {token, ChewingKey[L]} — no
        // freq field. The freq is in the MemoryChunk phrase library, not
        // the pinyin index. So each (token, freq) record becomes a
        // PinyinIndexItem2 with the tone-zero ChewingKeys.
        let mut item_value = Vec::new();
        for &(token, _freq) in &records {
            let stride = item2_stride(phrase_length);
            let mut record = vec![0u8; stride];
            record[..4].copy_from_slice(&token.to_le_bytes());
            for (j, key) in chewing_keys.iter().enumerate() {
                let packed = key.to_packed().to_le_bytes();
                record[4 + j * 2] = packed[0];
                record[4 + j * 2 + 1] = packed[1];
            }
            item_value.extend_from_slice(&record);
        }

        complete
            .entry(complete_key.clone())
            .or_default()
            .extend_from_slice(&item_value);

        // Add prefix markers for every shorter prefix.
        for prefix_len in 1..phrase_length {
            let prefix_key = encode_complete_key(&chewing_keys[..prefix_len]);
            prefix_markers.entry(prefix_key).or_insert(());
        }

        // Encode the incomplete (initial-only) key.
        let incomplete_key = encode_incomplete_key(&chewing_keys);
        let mut initial_value = Vec::new();
        for &(token, _freq) in &records {
            let stride = item2_stride(phrase_length);
            let mut record = vec![0u8; stride];
            record[..4].copy_from_slice(&token.to_le_bytes());
            for (j, key) in chewing_keys.iter().enumerate() {
                let initial_only = ChewingKey::new(key.initial, 0, 0, 0);
                let packed = initial_only.to_packed().to_le_bytes();
                record[4 + j * 2] = packed[0];
                record[4 + j * 2 + 1] = packed[1];
            }
            initial_value.extend_from_slice(&record);
        }

        incomplete
            .entry(incomplete_key)
            .or_default()
            .extend_from_slice(&initial_value);

        // Add prefix markers for incomplete keys too.
        for prefix_len in 1..phrase_length {
            let prefix_key = encode_incomplete_key(&chewing_keys[..prefix_len]);
            prefix_markers.entry(prefix_key).or_insert(());
        }
    }

    // Merge prefix markers (empty values) with the complete/incomplete
    // entries. A marker is only written for keys that don't already have
    // a real entry.
    let mut result = Entries::new();
    for (key, value) in &complete {
        result.push((key.clone(), value.clone()));
    }
    for (key, value) in &incomplete {
        if !complete.contains_key(key) {
            result.push((key.clone(), value.clone()));
        }
    }
    for (key, _) in &prefix_markers {
        if !complete.contains_key(key) && !incomplete.contains_key(key) {
            result.push((key.clone(), Vec::new()));
        }
    }

    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

/// Converts phrase_index entries from token→text to UCS-4 text→tokens.
///
/// Input: `(token u32 LE, UTF-8 text)`
/// Output: `(UCS-4 text bytes, u32 token[] LE)`
pub fn convert_phrase_index(entries: &Entries) -> Result<Entries, DatagenError> {
    let mut text_to_tokens: BTreeMap<Vec<u8>, Vec<u32>> = BTreeMap::new();

    for (key_bytes, value_bytes) in entries {
        if key_bytes.len() != 4 {
            continue;
        }
        let token = u32::from_le_bytes([key_bytes[0], key_bytes[1], key_bytes[2], key_bytes[3]]);
        let text = std::str::from_utf8(value_bytes).map_err(|_| {
            DatagenError::Consistency(format!(
                "phrase index value for token {token:#010x} is not UTF-8"
            ))
        })?;
        let ucs4_key = encode_ucs4_key(text);
        text_to_tokens.entry(ucs4_key).or_default().push(token);
    }

    let mut result = Entries::new();
    for (ucs4_key, tokens) in text_to_tokens {
        let mut value = Vec::with_capacity(tokens.len() * 4);
        for token in &tokens {
            value.extend_from_slice(&token.to_le_bytes());
        }
        result.push((ucs4_key, value));
    }

    result.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(result)
}

// ── Key encoding helpers ─────────────────────────────────────────

fn encode_complete_key(keys: &[ChewingKey]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(keys.len() * 2);
    for key in keys {
        let zeroed = ChewingKey::new(key.initial, key.middle, key.final_, 0);
        buf.extend_from_slice(&zeroed.to_packed().to_le_bytes());
    }
    buf
}

fn encode_incomplete_key(keys: &[ChewingKey]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(keys.len() * 2);
    for key in keys {
        let initial_only = ChewingKey::new(key.initial, 0, 0, 0);
        buf.extend_from_slice(&initial_only.to_packed().to_le_bytes());
    }
    buf
}

fn encode_ucs4_key(text: &str) -> Vec<u8> {
    let mut buf = Vec::with_capacity(text.len() * 4);
    for ch in text.chars() {
        buf.extend_from_slice(&(ch as u32).to_le_bytes());
    }
    buf
}

const fn item2_stride(phrase_length: usize) -> usize {
    let raw = 4 + 2 * phrase_length;
    (raw + 3) & !3
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn make_pinyin_entry(pinyin: &str, records: &[(u32, u32)]) -> (Vec<u8>, Vec<u8>) {
        let key = pinyin.as_bytes().to_vec();
        let mut value = Vec::new();
        for &(token, freq) in records {
            value.extend_from_slice(&token.to_le_bytes());
            value.extend_from_slice(&freq.to_le_bytes());
        }
        (key, value)
    }

    fn make_phrase_entry(token: u32, text: &str) -> (Vec<u8>, Vec<u8>) {
        (token.to_le_bytes().to_vec(), text.as_bytes().to_vec())
    }

    #[test]
    fn convert_pinyin_single_syllable() {
        let entries = vec![make_pinyin_entry("ba", &[(0x01000001, 100)])];
        let result = convert_pinyin_index(&entries).unwrap();
        assert!(!result.is_empty());

        let ba = ChewingKey::from_pinyin("ba").unwrap();
        let expected_key = encode_complete_key(&[ba]);
        let found = result.iter().find(|(k, _)| *k == expected_key);
        assert!(found.is_some(), "ba key must exist in output");
        let (_, value) = found.unwrap();
        assert!(!value.is_empty(), "ba must have real records");
    }

    #[test]
    fn convert_pinyin_multi_syllable_adds_prefix_markers() {
        let entries = vec![make_pinyin_entry("ni'hao", &[(0x01000099, 50)])];
        let result = convert_pinyin_index(&entries).unwrap();

        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let hao = ChewingKey::from_pinyin("hao").unwrap();

        let nihao_key = encode_complete_key(&[ni, hao]);
        let ni_key = encode_complete_key(&[ni]);

        let nihao = result.iter().find(|(k, _)| *k == nihao_key);
        assert!(nihao.is_some(), "ni'hao key must exist");

        let ni_marker = result.iter().find(|(k, _)| *k == ni_key);
        assert!(ni_marker.is_some(), "ni prefix marker must exist");
        assert!(
            ni_marker.unwrap().1.is_empty(),
            "prefix marker must have empty value"
        );
    }

    #[test]
    fn convert_pinyin_includes_incomplete_keys() {
        let entries = vec![make_pinyin_entry("ba", &[(0x01000001, 100)])];
        let result = convert_pinyin_index(&entries).unwrap();

        let ba = ChewingKey::from_pinyin("ba").unwrap();
        let incomplete_key = encode_incomplete_key(&[ba]);
        let found = result.iter().find(|(k, _)| *k == incomplete_key);
        assert!(found.is_some(), "incomplete key for b must exist");
    }

    #[test]
    fn convert_phrase_reverses_direction() {
        let entries = vec![
            make_phrase_entry(0x01000010, "你"),
            make_phrase_entry(0x01000099, "你好"),
        ];
        let result = convert_phrase_index(&entries).unwrap();

        let ni_key = encode_ucs4_key("你");
        let ni_entry = result.iter().find(|(k, _)| *k == ni_key);
        assert!(ni_entry.is_some(), "你 must exist in output");
        let tokens_value = &ni_entry.unwrap().1;
        assert_eq!(tokens_value.len(), 4, "one token for 你");
        assert_eq!(
            u32::from_le_bytes([
                tokens_value[0],
                tokens_value[1],
                tokens_value[2],
                tokens_value[3]
            ]),
            0x01000010
        );
    }

    #[test]
    fn convert_phrase_multiple_tokens_for_same_text() {
        let entries = vec![
            make_phrase_entry(0x01000010, "你"),
            make_phrase_entry(0x02000010, "你"),
        ];
        let result = convert_phrase_index(&entries).unwrap();

        let ni_key = encode_ucs4_key("你");
        let ni_entry = result.iter().find(|(k, _)| *k == ni_key).unwrap();
        assert_eq!(ni_entry.1.len(), 8, "two tokens for 你");
    }

    #[test]
    fn convert_pinyin_skips_unknown_syllables() {
        let entries = vec![make_pinyin_entry("nonexistent", &[(0x01000001, 100)])];
        let result = convert_pinyin_index(&entries).unwrap();
        assert!(result.is_empty(), "unknown syllables produce no output");
    }

    #[test]
    fn stride_padding_matches_upstream() {
        assert_eq!(item2_stride(1), 8);
        assert_eq!(item2_stride(2), 8);
        assert_eq!(item2_stride(3), 12);
    }
}
