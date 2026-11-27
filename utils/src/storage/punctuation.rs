//! The utilities' shared pin punctuation append operation.
use super::dbm::Tree;
use oxpinyin_tool_support::input as tool;
use std::ffi::CStr;

/// `PunctTable::append_punctuation` / `PunctTableEntry::append_punctuation`
/// (`src/storage/punct_table.cpp:97-115`, `:158-167`).
pub(crate) fn append_punctuation(table: &Tree, token: u32, punctuation: &CStr) {
    let key = token.to_le_bytes();
    let (ucs4, _written) = tool::utf8_to_ucs4(punctuation);
    let ucs4 = ucs4.unwrap_or_default();
    let existing = table.get(&key).unwrap_or_default();
    // `get_all_punctuations`: zero-terminated UCS-4 runs. The piece after
    // the last terminator is no run.
    let words: Vec<u32> = existing
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect();
    let mut runs: Vec<&[u32]> = words.split(|&word| word == 0).collect();
    runs.pop();
    if runs.contains(&ucs4.as_slice()) {
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
