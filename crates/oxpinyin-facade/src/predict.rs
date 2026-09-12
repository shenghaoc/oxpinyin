//! Phrase prediction: the prefix → token resolution seeding
//! `guess_sentence_with_prefix`, and the merged suggestion rows
//! `_compute_predicted_prefix_candidates` ranks.

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
/// store's own phrase inventory.
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
        tokens.extend(
            dict.system()
                .tokens_for_text(&suffix)
                .unwrap_or_default()
                .into_iter()
                .filter(|token| dict.library_visible_token(*token)),
        );
        if let Some(lookup) = user_lookup.as_ref() {
            tokens.extend(lookup.tokens_for_text(&suffix).iter().copied());
        }
    }
    tokens
}

/// The system and user `suggest_after` rows for `prefix`, merged in the
/// order `_compute_predicted_prefix_candidates` receives them.
///
/// System rows come from the loaded phrase table filtered by the
/// loaded-library mask — the same gate [`compute_prefixes`] applies on
/// the prefix path; user rows from the user store's phrase inventory.
/// The ordering law itself is
/// [`oxpinyin_runtime::merge_suggestion_rows`], held with this crate's
/// other shared orchestration so the C-ABI facades and the Python
/// binding rank one suggestion order rather than each assembling an
/// equivalent.
#[must_use]
pub fn merged_suggestions(
    dict: &RuntimeDict,
    user: Option<&UserStore>,
    prefix: &str,
) -> Vec<(u32, String)> {
    let system: Vec<(u32, String)> = dict
        .system()
        .suggest_after(prefix)
        .unwrap_or_default()
        .into_iter()
        .filter(|(token, _)| dict.library_visible_token(*token))
        .collect();
    let user_rows = if let Some(store) = user
        && let Ok(lookup) = UserLookup::from_store(store)
    {
        lookup.suggest_after(prefix)
    } else {
        Vec::new()
    };
    oxpinyin_runtime::merge_suggestion_rows(&system, &user_rows)
}
