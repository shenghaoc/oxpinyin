//! [`UserModel`] wiring for [`UserStore`].
//!
//! This is the first implementor of the `pinyin-core` seam — T0
//! (`docs/findings/user-store.md` §7) found the trait had zero implementors.
//! T1 wires only the `observe` (write) side. `score` is a neutral placeholder:
//! the decode-time additive merge of user counts with the system model is
//! W6-T4, and until then nothing consults `score`, so it must not influence
//! decode.
//!
//! T3 types the seam with the engine's vocabulary token
//! ([`PhraseToken`], `docs/findings/session-api.md`): [`Session::train`]
//! passes the recorded sentence to `observe`, and the engine never interprets
//! the numeric value. The store itself stays keyed on the raw `u32`
//! `phrase_token_t` layout; the impl converts at the seam.

use pinyin_core::{Cost, PhraseToken, UserModel};

use crate::store::{SENTENCE_START, UserStore, UserStoreError};

impl UserModel for UserStore {
    type Token = PhraseToken;
    type Error = UserStoreError;

    /// Neutral placeholder — always `0` (no cost adjustment).
    ///
    /// The additive user/system merge is W6-T4; T1 expresses no preference so
    /// that adding the store cannot move any decode result.
    fn score(&self, _history: &[Self::Token], _token: &Self::Token) -> Result<Cost, Self::Error> {
        Ok(0)
    }

    /// Record an accepted `token` after `history` via the training-selection
    /// path (§2). The predecessor is the last token of `history`, or
    /// [`SENTENCE_START`] when `history` is empty.
    fn observe(&mut self, history: &[Self::Token], token: &Self::Token) -> Result<(), Self::Error> {
        let last = history.last().map_or(SENTENCE_START, |t| t.value());
        self.observe_selection(last, token.value())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use pinyin_core::PhraseToken;

    use super::*;

    fn token(value: u32) -> PhraseToken {
        PhraseToken::new(value)
    }

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pinyin-user-model-{tag}-{}.redb",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn observe_uses_last_of_history_as_predecessor() {
        let path = temp_path("history");
        let mut store = UserStore::open(&path).unwrap();

        // history [5, 7] -> predecessor is 7.
        UserModel::observe(&mut store, &[token(5), token(7)], &token(42)).unwrap();
        assert_eq!(store.bigram_count(7, 42).unwrap(), 69);
        assert_eq!(store.bigram_count(5, 42).unwrap(), 0);

        // empty history -> sentence_start.
        UserModel::observe(&mut store, &[], &token(9)).unwrap();
        assert_eq!(store.bigram_count(SENTENCE_START, 9).unwrap(), 69);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn score_is_neutral_zero() {
        let path = temp_path("score");
        let store = UserStore::open(&path).unwrap();
        assert_eq!(
            UserModel::score(&store, &[token(1), token(2)], &token(3)).unwrap(),
            0
        );
        let _ = std::fs::remove_file(&path);
    }
}
