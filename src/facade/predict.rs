//! Phrase prediction: the prefix → token resolution seeding
//! `guess_sentence_with_prefix`, and the merged suggestion rows
//! `_compute_predicted_prefix_candidates` ranks.

use crate::model::Dictionary;
use crate::runtime::RuntimeDict;
use crate::user::UserStore;
use crate::vocab::PhraseText;

// _compute_prefixes, pinyin.cpp:1389–1422. reduce_tokens checks each suffix separately,
// phrase_large_table3.h:95.
/// Resolves suffixes of the prefix's outward UCS-4 words to enabled phrase tokens.
/// Suffixes are visited shortest first up to [`crate::vocab::MAX_PHRASE_LENGTH`]. A failed
/// system read contributes no system tokens for that suffix. The user table is still
/// consulted.
///
/// # Errors
///
/// Returns [`crate::engine::EngineError::PinAssert`] when more than four enabled
/// tokens match a single suffix.
pub fn compute_prefixes(
    dict: &RuntimeDict,
    prefix: Option<&PhraseText>,
) -> Result<Vec<u32>, crate::engine::EngineError> {
    let Some(prefix) = prefix else {
        return Ok(Vec::new());
    };
    let prefix: Vec<u32> = prefix.outward_words().collect();
    let max = prefix.len().min(crate::vocab::MAX_PHRASE_LENGTH);
    let mut tokens = Vec::new();
    for length in 1..=max {
        // The system and user tables' enabled tokens, sorted by library.
        let matches = dict.tokens_for_ucs4(&prefix[prefix.len() - length..]);
        // 074a2219 pinyin.cpp:1414-1420 reduces each suffix separately.
        if matches.len() > 4 {
            return Err(crate::engine::EngineError::PinAssert);
        }
        tokens.extend(matches);
    }
    Ok(tokens)
}

/// The system and user phrase-table suggestion tokens for `prefix`,
/// merged in the order `_compute_predicted_prefix_candidates` receives
/// them, keeping only suggestions with a readable phrase item.
///
/// The loaded-library mask applies to both tables. The ordering law is
/// `runtime::merge_suggestion_rows`.
#[must_use]
pub fn merged_suggestions(
    dict: &RuntimeDict,
    user: &UserStore,
    prefix: &PhraseText,
) -> Vec<(u32, PhraseText)> {
    let prefix: Vec<u32> = prefix.outward_words().collect();
    let system: Vec<u32> = dict
        .system()
        .suggestion_tokens(&prefix)
        .unwrap_or_default()
        .into_iter()
        .filter(|token| dict.phrase_table_token_enabled(*token))
        .collect();
    let mut user_rows = user.lookup().suggest_after(&prefix);
    // PhraseTableEntry::search skips disabled library arrays
    // (074a2219 phrase_large_table3.cpp:40-42) in both tables.
    for (_, tokens) in &mut user_rows {
        tokens.retain(|token| dict.phrase_table_token_enabled(*token));
    }
    // Search visits raw keys; the candidate's text comes from its live item
    // (074a2219 pinyin.cpp:2391-2395, 2018-2022), including keys containing
    // UCS-4 values that cannot be represented by a Rust String.
    crate::runtime::merge_suggestion_rows(&system, &user_rows)
        .into_iter()
        .filter_map(|token| dict.token_text(token).map(|text| (token, text)))
        .collect()
}

#[cfg(test)]
mod tests;
