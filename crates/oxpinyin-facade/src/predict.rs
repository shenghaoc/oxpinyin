//! Phrase prediction: the prefix → token resolution seeding
//! `guess_sentence_with_prefix`.

use oxpinyin_runtime::RuntimeDict;
use oxpinyin_user::UserStore;

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
    let user_lookup = user.and_then(|store| oxpinyin_user::UserLookup::from_store(store).ok());
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
