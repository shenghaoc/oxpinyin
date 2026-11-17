//! The utilities' shared pin punctuation append operation.
use super::dbm::Tree;
use oxpinyin_core::input::tool;
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
        .map(|word| u32::from_le_bytes(*word))
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

#[cfg(test)]
mod tests;
