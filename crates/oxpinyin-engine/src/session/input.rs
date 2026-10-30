//! Raw-input appending and erasing, including selection rollback.

use super::*;

impl<D, L> Session<D, L>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Types a run of characters and refreshes candidates once.
    ///
    /// Accepts every printable ASCII
    /// character (`0x21..=0x7E`). Non-`a-z`/`'` bytes stay in the raw buffer so
    /// the decoder sees the same junk-bearing strings the oracle fixture
    /// carries; the segment graph stops at those bytes as hard boundaries.
    /// Space and non-ASCII are skipped. Typing past [`MAX_INPUT_BYTES`] stops
    /// accepting further characters.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when refreshing candidates hits a backend
    /// failure.
    pub fn type_pinyin(&mut self, text: &str) -> Result<KeyOutcome, EngineError> {
        let before = self.input.len();
        for character in text.chars() {
            if !is_batch_input_character(character) {
                continue;
            }
            // `try_push` exits exact mode on each accepted character and
            // stops at the cap; ignored input never reaches it, so a batch
            // that changes nothing leaves an exact composition alone.
            if !self.input.try_push(character) {
                break;
            }
        }
        if self.input.len() == before {
            return Ok(KeyOutcome::Ignored);
        }
        self.refresh()?;
        Ok(KeyOutcome::Consumed)
    }

    /// Removes the last unselected input character, or clears a completed
    /// selection and its constraints when no unselected input remains.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when refreshing candidates fails.
    pub fn erase(&mut self) -> Result<KeyOutcome, EngineError> {
        if self.record.consumed() < self.input.len() {
            // The buffer is about to shrink: `pop` drops the last byte and
            // exits exact mode, whose absolute spans would dangle past the
            // new end.
            self.input.pop();
            self.refresh()?;
            return Ok(KeyOutcome::Consumed);
        }
        if !self.record.selected().is_empty() {
            // The all-or-nothing un-select: the store goes with the
            // record, or a forcing would outlive its own selection.
            self.record.clear();
            self.constraints.clear();
            self.refresh()?;
            return Ok(KeyOutcome::Consumed);
        }
        Ok(KeyOutcome::Ignored)
    }
}
