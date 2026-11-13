//! Shared import-iterator planning: the phrase-table search and the pin's
//! assertions before a phrase is added, for both C-ABI facades.

use oxpinyin_core::PhraseText;
use oxpinyin_runtime::RuntimeDict;
use oxpinyin_user::{PinyinKey, UserStore, is_user_file_library};

/// What an import add does with one phrase/reading pair before it writes.
pub enum ImportPlan {
    /// The pin's ordinary `false`.
    Refuse,
    /// A phrase-table lookup failed outright: no pin counterpart, one
    /// warning and `false`.
    LookupFailed,
    /// One of the pin's asserts: `reduce_tokens`' count
    /// (`phrase_large_table3.h:95`), two tokens in the target library, or
    /// the target library's item with another length or text.
    Abort,
    /// The add goes ahead, with the same-library token the phrase table
    /// already holds for the phrase, if any.
    Add {
        /// The imported phrase text.
        phrase: PhraseText,
        /// Its parsed keys, in the active facade's reading.
        keys: Vec<PinyinKey>,
        /// The same-library token the phrase table already holds.
        original: Option<u32>,
    },
}

/// Plans one add against the loaded phrase tables: the shared half of
/// `_add_phrase`, after the facade has parsed the reading and checked its
/// length.
#[must_use]
pub fn plan_add(
    index: u8,
    dict: &RuntimeDict<'_>,
    user: &UserStore,
    phrase: PhraseText,
    keys: Vec<PinyinKey>,
) -> ImportPlan {
    let length = phrase.phrase_length();
    let Ok(tokens) = phrase_table_tokens(user, dict, &phrase) else {
        return ImportPlan::LookupFailed;
    };
    if tokens.len() > 4 {
        return ImportPlan::Abort;
    }
    // The pin walks the phrase table's tokens for the phrase. Two tokens in
    // the same sub-index library trip
    // `assert(PHRASE_INDEX_LIBRARY_INDEX(token) != index)`; a lone one whose
    // phrase-index item has another length trips
    // `assert(phrase_length == item.get_phrase_length())`, and one of the
    // same length with other text the `memcmp` assert. The phrase table is
    // the `user_phrase_index` membership plus the system phrase index; the
    // item text is read without its pronunciation view.
    let same_library: Vec<u32> = tokens
        .into_iter()
        .filter(|token| token >> 24 == u32::from(index))
        .collect();
    if same_library.len() > 1 {
        return ImportPlan::Abort;
    }
    if let Some(&token) = same_library.first() {
        let item_text = dict.token_text(token);
        // A missing item leaves the pin's PhraseItem at length zero, which
        // asserts before any import mutation. The stored length first, then
        // every stored word.
        let same_length = item_text
            .as_ref()
            .is_some_and(|text| text.phrase_length() == length);
        let same_words = item_text.as_ref().is_some_and(|text| {
            text.stored_words().map_or_else(
                || text.outward_words().eq(phrase.outward_words()),
                |words| words.iter().copied().eq(phrase.outward_words()),
            )
        });
        if !(same_length && same_words) {
            return ImportPlan::Abort;
        }
    }
    // The pin walks the tokens and asserts for any library index; only then
    // does `get_range` reject a library that is not loaded.
    if !is_user_file_library(index) && !user.is_system_library(index) {
        return ImportPlan::Refuse;
    }
    ImportPlan::Add {
        phrase,
        keys,
        original: same_library.first().copied(),
    }
}

/// Search the system table followed by the user table, then concatenate the
/// per-library arrays as `reduce_tokens` does. Duplicates remain
/// observable.
fn phrase_table_tokens(
    user: &UserStore,
    dict: &RuntimeDict<'_>,
    phrase: &PhraseText,
) -> Result<Vec<u32>, ()> {
    let words: Vec<u32> = phrase.outward_words().collect();
    let mut tokens = dict.system().tokens_for_ucs4(&words).map_err(|_| ())?;
    tokens.extend(user.phrase_table_tokens_for_ucs4(&words).map_err(|_| ())?);
    tokens.retain(|token| dict.phrase_table_token_enabled(*token));
    oxpinyin_core::sort_tokens_by_library(&mut tokens);
    Ok(tokens)
}
