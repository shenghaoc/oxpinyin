//! [`UserModel`] wiring for [`UserStore`].
//!
//! This is the first implementor of the `pinyin-core` seam — T0
//! (`docs/findings/user-store.md` §7) found the trait had zero implementors.
//! T1 wired `observe`. T4 exposes the stored counts as
//! [`pinyin_core::UserCountDelta`] for the decode-time additive merge;
//! [`UserModel::score`] cannot return that merge as a [`Cost`] (adding a
//! user cost after the λ blend would be a new weighting scheme), so it
//! stays at `0` and decode reads the counts through the language model.
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

    /// Always `0`. The §5 merge is count addition *before* the probability
    /// is taken, which lives in the language-model overlay via
    /// [`UserStore::count_delta`]. A non-zero [`Cost`] here would be a
    /// post-probability term — a weighting scheme §5 forbids.
    fn score(&self, history: &[Self::Token], token: &Self::Token) -> Result<Cost, Self::Error> {
        // Surface store-read failures rather than pretending the overlay
        // is fine when the file is unreadable.
        let _ = self.count_delta(history.last().map(|t| t.value()), token.value())?;
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
        let mut store = UserStore::open(&path).unwrap();
        assert_eq!(
            UserModel::score(&store, &[token(1), token(2)], &token(3)).unwrap(),
            0
        );
        // A populated store still returns 0 from this method: the merge is
        // in the count domain, not a post-hoc cost.
        UserModel::observe(&mut store, &[], &token(3)).unwrap();
        assert_eq!(UserModel::score(&store, &[], &token(3)).unwrap(), 0);
        assert_eq!(store.count_delta(None, 3).unwrap().unigram_delta, 483);
        let _ = std::fs::remove_file(&path);
    }
}
