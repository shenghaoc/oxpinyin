//! Phrase prediction helper: prefix → token-resolution for the
//! `zhuyin_guess_sentence_with_prefix` seed path.

use oxpinyin_user::UserStore;

use crate::state::SharedDict;

/// Resolves a prefix string to the phrase tokens its tail substrings name,
/// in the order `zhuyin_guess_sentence_with_prefix` consumes them.
///
/// This is a port of `oxpinyin-capi::predict::compute_prefixes` (which the
/// pinyin facade's `pinyin_guess_sentence_with_prefix` uses): system tokens
/// ride the loaded-library mask; user tokens come from the user store's own
/// phrase inventory.
pub(crate) fn compute_prefixes(
    dict: &SharedDict,
    user: Option<&UserStore>,
    prefix: &str,
) -> Vec<u32> {
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
