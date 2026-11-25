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

use crate::model::UserModel;
use crate::vocab::PhraseToken;

use crate::user::store::{TrainingCheck, UserStore, UserStoreError};
use crate::vocab::SENTENCE_START;

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
        readings: impl Iterator<Item = Vec<crate::parser::ChewingKey>>,
    ) -> Result<(), Self::Error> {
        let last = history.last().map_or(SENTENCE_START, |t| *t);
        let seed = self.observe_selection(last, *token)?;
        self.train_pronunciations(*token, readings, seed);
        Ok(())
    }
}

/// The store observing the steps a [`TrainingCheck`] passed, in step order:
/// each step stores the gram the check trained and applies its seed
/// (`UserStore::observe_checked_selection`), so a step reads and trains its
/// predecessor's gram once (`phonetic_lookup.h:876-904` at 074a2219).
pub(crate) struct CheckedTraining<'a> {
    user: &'a mut UserStore,
    check: &'a mut TrainingCheck,
}

impl<'a> CheckedTraining<'a> {
    pub(crate) const fn new(user: &'a mut UserStore, check: &'a mut TrainingCheck) -> Self {
        Self { user, check }
    }
}

impl UserModel for CheckedTraining<'_> {
    type Error = UserStoreError;

    fn observe(&mut self, history: &[PhraseToken], token: &PhraseToken) -> Result<(), Self::Error> {
        let last = history.last().map_or(SENTENCE_START, |t| *t);
        self.user
            .observe_checked_selection(self.check, last, *token)?;
        Ok(())
    }

    fn observe_with_keys(
        &mut self,
        history: &[PhraseToken],
        token: &PhraseToken,
        readings: impl Iterator<Item = Vec<crate::parser::ChewingKey>>,
    ) -> Result<(), Self::Error> {
        let last = history.last().map_or(SENTENCE_START, |t| *t);
        let seed = self
            .user
            .observe_checked_selection(self.check, last, *token)?;
        self.user.train_pronunciations(*token, readings, seed);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
