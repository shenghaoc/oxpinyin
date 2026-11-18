//! [`UserModel`] wiring for [`UserStore`].
//!
//! This implements the `oxpinyin` seam: `observe` records training,
//! and the stored counts are exposed as
//! [`crate::model::UserCountDelta`] for the decode-time additive merge.
//! The seam carries counts only: decode reads them through the language
//! model, because a score added after the λ blend would be a new weighting
//! scheme.
//!
//! T3 types the seam with the engine's vocabulary token
//! ([`PhraseToken`], `docs/findings/session-api.md`): the engine's
//! `prepare_training`/`observe` path passes the recorded sentence to
//! `observe`, and the engine never interprets
//! the numeric value. The store itself stays keyed on the raw `u32`
//! `phrase_token_t` layout; the impl converts at the seam.

use crate::{PhraseToken, model::UserModel};

use crate::SENTENCE_START;
use crate::user::store::{UserStore, UserStoreError};

impl UserModel for UserStore {
    type Error = UserStoreError;

    /// Record an accepted `token` after `history` via the training-selection
    /// path (§2). The predecessor is the last token of `history`, or
    /// [`SENTENCE_START`] when `history` is empty.
    fn observe(&mut self, history: &[PhraseToken], token: &PhraseToken) -> Result<(), Self::Error> {
        let last = history.last().map_or(SENTENCE_START, |t| *t);
        self.observe_selection(last, *token)?;
        Ok(())
    }

    fn observe_with_keys(
        &mut self,
        history: &[PhraseToken],
        token: &PhraseToken,
        readings: impl Iterator<Item = Vec<crate::ChewingKey>>,
    ) -> Result<(), Self::Error> {
        let last = history.last().map_or(SENTENCE_START, |t| *t);
        let seed = self.observe_selection(last, *token)?;
        self.train_pronunciations(*token, readings, seed)
    }
}

#[cfg(test)]
mod tests;
