//! [`UserModel`] wiring for [`UserStore`].
//!
//! This implements the `oxpinyin-core` seam: `observe` records training,
//! and the stored counts are exposed as
//! [`oxpinyin_core::UserCountDelta`] for the decode-time additive merge.
//! The seam carries counts only: decode reads them through the language
//! model, because a `Cost` added after the λ blend would be a new weighting
//! scheme.
//!
//! T3 types the seam with the engine's vocabulary token
//! ([`PhraseToken`], `docs/findings/session-api.md`): the engine's
//! `prepare_training`/`observe` path passes the recorded sentence to
//! `observe`, and the engine never interprets
//! the numeric value. The store itself stays keyed on the raw `u32`
//! `phrase_token_t` layout; the impl converts at the seam.

use oxpinyin_core::{PhraseToken, UserModel};

use crate::store::{SENTENCE_START, UserStore, UserStoreError};

impl UserModel for UserStore {
    type Token = PhraseToken;
    type Error = UserStoreError;

    /// One training step's store checks before any step is observed
    /// ([`UserStore::check_training_step`]): the predecessor is the last
    /// token of `history`, or [`SENTENCE_START`] when `history` is empty.
    fn check_step(
        &mut self,
        history: &[Self::Token],
        token: &Self::Token,
    ) -> Result<(), Self::Error> {
        let last = history.last().map_or(SENTENCE_START, |t| t.value());
        self.check_training_step(last, token.value())
    }

    /// Record an accepted `token` after `history` via the training-selection
    /// path (§2). The predecessor is the last token of `history`, or
    /// [`SENTENCE_START`] when `history` is empty.
    fn observe(&mut self, history: &[Self::Token], token: &Self::Token) -> Result<(), Self::Error> {
        let last = history.last().map_or(SENTENCE_START, |t| t.value());
        self.observe_selection(last, token.value())?;
        Ok(())
    }

    fn observe_with_keys(
        &mut self,
        history: &[Self::Token],
        token: &Self::Token,
        readings: impl Iterator<Item = Vec<oxpinyin_core::ChewingKey>>,
    ) -> Result<(), Self::Error> {
        let last = history.last().map_or(SENTENCE_START, |t| t.value());
        let seed = self.observe_selection(last, token.value())?;
        self.train_pronunciations(token.value(), readings, seed)
    }
}

#[cfg(test)]
mod tests {
    use oxpinyin_core::PhraseToken;

    use super::*;

    fn token(value: u32) -> PhraseToken {
        PhraseToken::new(value)
    }

    /// The pin's NULL-user-directory session over the stock layout.
    fn transient() -> UserStore {
        UserStore::open_transient(
            crate::persistence::SystemOriginals::new(
                Default::default(),
                oxpinyin_data::user_files::UserFileLayout::stock(),
            ),
            oxpinyin_data::user_files::SystemVersions::for_this_build(7, 14),
            crate::persistence::UserConfLaw::Pinyin,
        )
        .unwrap()
    }

    #[test]
    fn observe_uses_last_of_history_as_predecessor() {
        let mut store = transient();

        // history [5, 7] -> predecessor is 7.
        UserModel::observe(&mut store, &[token(5), token(7)], &token(42)).unwrap();
        assert_eq!(store.bigram_count(7, 42).unwrap(), 69);
        assert_eq!(store.bigram_count(5, 42).unwrap(), 0);

        // empty history -> sentence_start.
        UserModel::observe(&mut store, &[], &token(9)).unwrap();
        assert_eq!(store.bigram_count(SENTENCE_START, 9).unwrap(), 69);
    }
}
