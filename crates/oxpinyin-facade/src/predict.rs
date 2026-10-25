//! Phrase prediction: the prefix → token resolution seeding
//! `guess_sentence_with_prefix`, and the merged suggestion rows
//! `_compute_predicted_prefix_candidates` ranks.

use oxpinyin_core::PhraseText;
use oxpinyin_runtime::RuntimeDict;
use oxpinyin_user::{UserLookup, UserStore};

/// Resolves a prefix string to the phrase tokens its tail substrings
/// name, in the order `guess_sentence_with_prefix` consumes them
/// (upstream's `_compute_prefixes`).
///
/// System tokens ride the loaded-library mask: an unloaded library must
/// not contribute prefix tokens (upstream's `_get_phrase_item_from_token`
/// refuses them at the item lookup; filtering here is the closest we get
/// to that gate on the prefix path). User tokens come from the user
/// store's raw phrase table.
#[must_use]
pub fn compute_prefixes(dict: &RuntimeDict, user: Option<&UserStore>, prefix: &str) -> Vec<u32> {
    let chars: Vec<char> = prefix.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }
    let user_lookup = user.and_then(|store| UserLookup::from_store(store).ok());
    let max = chars.len().min(oxpinyin_user::MAX_PHRASE_LENGTH);
    let mut tokens = Vec::new();
    for length in 1..=max {
        let suffix: String = chars[chars.len() - length..].iter().collect();
        let mut matches = dict.system().tokens_for_text(&suffix).unwrap_or_default();
        if let Some(lookup) = user_lookup.as_ref() {
            matches.extend(lookup.tokens_for_text(&suffix).iter().copied());
        }
        matches.retain(|token| dict.phrase_table_token_enabled(*token));
        // 074a2219 pinyin.cpp:1414-1420 reduces each suffix separately.
        matches.sort_by_key(|token| token >> 24);
        tokens.extend(matches);
    }
    tokens
}

/// The system and user `suggest_after` rows for `prefix`, merged in the
/// order `_compute_predicted_prefix_candidates` receives them.
///
/// System rows come from the loaded phrase table filtered by the
/// loaded-library mask — the same gate [`compute_prefixes`] applies on
/// the prefix path; user rows from its raw phrase table.
/// The ordering law itself is
/// [`oxpinyin_runtime::merge_suggestion_rows`], held with this crate's
/// other shared orchestration so the C-ABI facades rank one suggestion
/// order rather than each assembling an equivalent.
#[must_use]
pub fn merged_suggestions(
    dict: &RuntimeDict,
    user: Option<&UserStore>,
    prefix: &str,
) -> Vec<(u32, PhraseText)> {
    let system: Vec<(u32, PhraseText)> = dict
        .system()
        .suggest_after(prefix)
        .unwrap_or_default()
        .into_iter()
        .filter(|(token, _)| dict.phrase_table_token_enabled(*token))
        .collect();
    let mut user_rows = if let Some(store) = user
        && let Ok(lookup) = UserLookup::from_store(store)
    {
        lookup.suggest_after(prefix)
    } else {
        Vec::new()
    };
    // PhraseTableEntry::search skips disabled library arrays
    // (074a2219 phrase_large_table3.cpp:40-42) in both tables.
    user_rows.retain(|(token, _)| dict.phrase_table_token_enabled(*token));
    // Search visits raw keys; the candidate's text comes from its live item
    // (074a2219 pinyin.cpp:2391-2395, 2018-2022), including keys containing
    // UCS-4 values that cannot be represented by a Rust String.
    oxpinyin_runtime::merge_suggestion_rows(&system, &user_rows)
        .into_iter()
        .filter_map(|(token, fallback)| {
            dict.token_introspection(token)
                .map(|item| item.text)
                // Preserve the established valid-key fallback when the live
                // item is absent; raw-key traversal adds no new missing-item policy.
                .or(fallback)
                .map(|text| (token, text))
        })
        .collect()
}
