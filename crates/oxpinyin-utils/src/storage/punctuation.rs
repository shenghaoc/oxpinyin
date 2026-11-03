//! Shared pin punctuation mutations; the tool never deletes an empty entry.
use super::dbm::Tree;
use oxpinyin_data::row_format::punct::encode_puncts;
use oxpinyin_input::tool;
use std::ffi::CStr;

/// `PunctTable::append_punctuation` / `PunctTableEntry::append_punctuation`
/// (`src/storage/punct_table.cpp:97-115`, `:158-167`).
pub(crate) fn append_punctuation(table: &Tree, token: u32, punctuation: &CStr) {
    let key = token.to_le_bytes();
    let (ucs4, _written) = tool::utf8_to_ucs4(punctuation);
    let ucs4 = ucs4.unwrap_or_default();
    let existing = table.get(&key).unwrap_or_default();
    // `get_all_punctuations`: zero-terminated UCS-4 runs.
    let mut runs: Vec<Vec<u32>> = Vec::new();
    let mut current = Vec::new();
    for word in existing
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
    {
        if word == 0 {
            runs.push(std::mem::take(&mut current));
        } else {
            current.push(word);
        }
    }
    if runs.contains(&ucs4) {
        tool::storage::report_duplicated_punctuation(punctuation);
        return;
    }
    let mut value = existing;
    for word in &ucs4 {
        value.extend_from_slice(&word.to_le_bytes());
    }
    value.extend_from_slice(&0_u32.to_le_bytes());
    let _ = table.put(&key, &value);
}

/// `PunctTable::remove_punctuation` (`punct_table.cpp:169-177`).
pub(crate) fn remove_punctuation(table: &Tree, token: u32, punct: &str) -> bool {
    let key = token.to_le_bytes();
    let mut entry = table.get(&key).unwrap_or_default();
    if !remove_entry(&mut entry, &encode_puncts(&[punct])) {
        return false;
    }
    // Even the final removal stores a zero-byte entry, never deletes its key.
    table.put(&key, &entry).is_ok()
}

/// `PunctTableEntry::remove_punctuation` (`punct_table.cpp:117-145`):
/// first whole NUL-terminated span, at a punctuation boundary.
fn remove_entry(entry: &mut Vec<u8>, punct: &[u8]) -> bool {
    let mut start = 0_usize;
    while let Some(candidate) = entry.get(start..start.saturating_add(punct.len())) {
        if candidate == punct {
            entry.drain(start..start + punct.len());
            return true;
        }
        let Some(end) = entry[start..]
            .as_chunks::<4>()
            .0
            .iter()
            .position(|word| *word == [0; 4])
        else {
            return false;
        };
        start += (end + 1) * 4;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_first_whole_span_and_keep_empty_entry() {
        let mut entry = encode_puncts(&["……", "…", "？", "…"]);
        assert!(remove_entry(&mut entry, &encode_puncts(&["…"])));
        assert_eq!(entry, encode_puncts(&["……", "？", "…"]));
        assert!(!remove_entry(&mut entry, &encode_puncts(&["。"])));
        for punct in ["……", "？", "…"] {
            assert!(remove_entry(&mut entry, &encode_puncts(&[punct])));
        }
        assert!(entry.is_empty());
        assert!(!remove_entry(&mut entry, &encode_puncts(&["…"])));
    }
}
