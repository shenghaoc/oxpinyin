//! Key input: one key press or one batch of typed pinyin into the raw buffer, with the accept set and the erase law.
//!
//! One of the `impl Session` slices `session/mod.rs` declares; every
//! method here was moved verbatim from the single 5,800-line
//! `session.rs` (2026-09-08) and keeps its doc comment and upstream
//! citations. Shared state, constants and free functions stay in the
//! parent module.

use super::*;

impl<D, L> Session<D, L>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Feeds one key press to the session.
    ///
    /// Characters the parser has syntax for — ASCII lowercase and the
    /// apostrophe — extend the composition. `Backspace` removes the last one,
    /// or undoes a selection when nothing else remains. `Escape` clears the
    /// composition. `Enter` commits it. `Space` chooses the first candidate,
    /// or commits when there is none. Every other key, and any key held with a
    /// command modifier, is [`KeyOutcome::Ignored`] and changes nothing.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when refreshing candidates hits a backend
    /// failure.
    pub fn process_key(&mut self, input: &KeyInput) -> Result<KeyOutcome, EngineError> {
        if input.modifiers().has_command_modifier() {
            return Ok(KeyOutcome::Ignored);
        }

        match input.key() {
            LogicalKey::Character(character) => self.type_character(character),
            LogicalKey::Backspace => self.erase(),
            LogicalKey::Escape => {
                if self.is_composing() {
                    self.reset();
                    Ok(KeyOutcome::Consumed)
                } else {
                    Ok(KeyOutcome::Ignored)
                }
            }
            LogicalKey::Enter => {
                if self.is_composing() {
                    Ok(KeyOutcome::Commit(self.commit()?))
                } else {
                    Ok(KeyOutcome::Ignored)
                }
            }
            LogicalKey::Space => self.accept_first(),
            _ => Ok(KeyOutcome::Ignored),
        }
    }

    /// Types a run of characters and refreshes candidates once.
    ///
    /// For the final composition state this is equivalent to calling
    /// [`Session::process_key`] once per character when no selection
    /// intervenes **and** every character is parser syntax (`a`–`z` / `'`),
    /// but without recomputing candidates after every keystroke. Batch
    /// differential runs use this; interactive shells should keep calling
    /// [`Session::process_key`] so intermediate candidate lists update.
    ///
    /// Unlike [`Session::process_key`], this accepts every printable ASCII
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
        let before = self.raw.len();
        for character in text.chars() {
            if !is_batch_input_character(character) {
                continue;
            }
            if self.raw.len() + character.len_utf8() > MAX_INPUT_BYTES {
                break;
            }
            self.raw.push(character);
        }
        if self.raw.len() == before {
            return Ok(KeyOutcome::Ignored);
        }
        // Ignored input leaves an exact (scheme-parsed) composition alone;
        // only a real buffer change exits exact mode.
        self.exact_segments.clear();
        self.refresh()?;
        Ok(KeyOutcome::Consumed)
    }

    pub(super) fn type_character(&mut self, character: char) -> Result<KeyOutcome, EngineError> {
        if !is_input_character(character) {
            return Ok(KeyOutcome::Ignored);
        }
        if self.raw.len() + character.len_utf8() > MAX_INPUT_BYTES {
            return Ok(KeyOutcome::Ignored);
        }
        // The character is accepted: the buffer is about to change, so the
        // exact chain's absolute spans go now — not before validation, or a
        // rejected character would silently exit exact mode.
        self.exact_segments.clear();
        self.raw.push(character);
        self.refresh()?;
        Ok(KeyOutcome::Consumed)
    }

    pub(super) fn erase(&mut self) -> Result<KeyOutcome, EngineError> {
        if self.consumed < self.raw.len() {
            // The buffer is about to shrink: the exact chain's absolute
            // spans would dangle past the new end. Operations that do not
            // modify raw leave exact mode alone.
            self.exact_segments.clear();
            self.raw.pop();
            self.refresh()?;
            return Ok(KeyOutcome::Consumed);
        }
        if !self.selected.is_empty() {
            // The all-or-nothing un-select: the store goes with the
            // record, or a forcing would outlive its own selection.
            self.selected.clear();
            self.consumed = 0;
            self.selection_committed = false;
            self.history.clear();
            self.constraints.clear();
            self.refresh()?;
            return Ok(KeyOutcome::Consumed);
        }
        Ok(KeyOutcome::Ignored)
    }

    pub(super) fn accept_first(&mut self) -> Result<KeyOutcome, EngineError> {
        if self.candidates.is_empty() {
            if self.is_composing() {
                return Ok(KeyOutcome::Commit(self.commit()?));
            }
            return Ok(KeyOutcome::Ignored);
        }

        match self.select(0)? {
            Selection::Completed => Ok(KeyOutcome::Commit(self.commit()?)),
            Selection::Continued => Ok(KeyOutcome::Consumed),
        }
    }
}
