//! Phrase prediction: the prefix → token resolution seeding
//! `guess_sentence_with_prefix`, and the merged suggestion rows
//! `_compute_predicted_prefix_candidates` ranks.

use crate::model::Dictionary;
use crate::runtime::RuntimeDict;
use crate::user::UserStore;
use crate::vocab::{PhraseText, PhraseToken};

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

/// `_compute_predicted_bigram_candidates`'s gram walk (`pinyin.cpp:2321-2341`
/// at the pin `074a2219`): for the prefixes from last to first, `load` the
/// prefix's user gram and `merge_single_gram(merged, NULL, user)` — a copy
/// of it, or, when the prefix has no gram, the previous copy kept — and
/// stop at the first whose `get_length` is not zero; then `retrieve_all`.
/// Answers the items in stored order, duplicates included, or the one
/// warning a call refuses with where the pin reads past a partial item
/// tail after a whole item (`retrieve_all`, `ngram.cpp:143`). Where the pin asserts, the process aborts at the same
/// point of the walk: a value too short for its total (`get_length` reads
/// it, `memory_chunk.h:390`) and a gram with no whole item
/// and a total left over (`ngram.cpp:70` via `pinyin.cpp:2332`).
/// Only the prefixes the walk loads are read; a fault under any other token
/// is never reached.
///
/// # Errors
///
/// Returns the warning text when a gram has a partial item tail.
///
/// # Aborts
///
/// Aborts on a gram too short for its total, and on a gram with no whole item and a
/// non-zero total.
pub fn predicted_bigram_items(
    user: &UserStore,
    prefixes: &[PhraseToken],
) -> Result<Vec<(u32, u32)>, &'static str> {
    let mut merged = crate::single_gram::new_gram();
    for prev in prefixes.iter().rev() {
        if let Ok(Some(row)) = user.bigram_row(*prev) {
            // The one-sided merge copies every byte without inspecting the row
            // (`merge_single_gram`; 074a2219 ngram.cpp:286-290).
            merged = row;
        }
        if crate::single_gram::get_length(&merged) != 0 {
            break;
        }
        match crate::single_gram::get_total_freq(&merged) {
            Some(0) => {}
            None | Some(_) => std::process::abort(),
        }
    }
    if crate::single_gram::get_length(&merged) == 0 {
        return Ok(Vec::new());
    }
    if !(merged.len() - crate::single_gram::TOTAL_LEN).is_multiple_of(crate::single_gram::ITEM_LEN)
    {
        return Err(
            "pinyin_guess_predicted_candidates: a user bigram value has a partial item tail \
             (upstream reads past it, ngram.cpp:143)",
        );
    }
    Ok(crate::single_gram::retrieve_all(&merged).collect())
}

#[cfg(test)]
mod tests;
