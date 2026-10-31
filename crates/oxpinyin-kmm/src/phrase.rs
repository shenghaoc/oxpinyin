//! The pin's `taglib_token_to_string` and `taglib_validate_token_with_string`
//! (`src/storage/tag_utility.cpp:360-417` at `074a2219`), behind a trait so
//! the operations do not depend on the phrase-index layer.
//!
//! `taglib_token_to_string` is where the phrase text of a token comes from:
//! library zero is the special table (`<start>` for `sentence_start`), every
//! other token is resolved through `FacadePhraseIndex` and encoded with
//! `g_ucs4_to_utf8`. A token neither table answers prints
//! `error: unknown token:%d.\n` and answers NULL; callers that print skip
//! the record, and callers that `strcmp` it (import, gen) read through NULL
//! in the pin (class (b) of the compatibility policy). The trait's
//! [`PhraseIndex::validate_token_with_string`] answers `false` for that
//! NULL instead of reading through it, so the caller's `check_result`
//! abort is reproduced without the undefined read.

/// `taglib_token_to_string`'s answer, with its stderr diagnostic.
pub trait PhraseIndex {
    /// The UTF-8 phrase text of `token`, or `None` where the pin's function
    /// answers NULL. The implementation prints the pin's
    /// `error: unknown token:%d.\n` in that case.
    fn token_to_string(&self, token: u32) -> Option<Vec<u8>>;

    /// `taglib_validate_token_with_string`: whether the token's phrase text
    /// equals `text` (compared as C strings). A token the pin cannot
    /// resolve answers `false`, where the pin's `strcmp` reads through NULL.
    fn validate_token_with_string(&self, token: u32, text: &[u8]) -> bool {
        self.token_to_string(token).is_some_and(|answer| {
            let answer = answer.split(|byte| *byte == 0).next().unwrap_or(&[]);
            answer == text
        })
    }
}

/// A phrase source that knows only the pin's special table: `sentence_start`
/// is `<start>` and every other token is unknown. For an import or export
/// fixture that carries no phrase index; a real utility builds
/// [`PhraseIndex`] over the loaded `SYSTEM_FILE` chunks instead.
pub struct SpecialTokens;

impl PhraseIndex for SpecialTokens {
    fn token_to_string(&self, token: u32) -> Option<Vec<u8>> {
        if token == 1 {
            Some(b"<start>".to_vec())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PhraseIndex, SpecialTokens};

    #[test]
    fn sentence_start_is_the_special_table() {
        assert_eq!(SpecialTokens.token_to_string(1), Some(b"<start>".to_vec()));
        assert_eq!(SpecialTokens.token_to_string(2), None);
        assert!(SpecialTokens.validate_token_with_string(1, b"<start>"));
        assert!(!SpecialTokens.validate_token_with_string(2, b"x"));
    }
}
