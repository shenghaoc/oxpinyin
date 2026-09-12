//! The raw input buffer and its pre-parsed scheme chain.
//!
//! Owns the two coupled input-side invariants, so no caller can leave the
//! session with an input the rest of the pipeline would slice out of
//! bounds:
//!
//! - the [`MAX_INPUT_BYTES`] cap — a push that would overrun it is
//!   refused whole, never truncated mid-character (`raw` only ever holds
//!   ASCII, but the byte check keeps a future multi-byte input class from
//!   splitting a character);
//! - the exact-segment chain, whose spans are absolute over `raw` and
//!   never reach past its end (`end() <= raw.len()` for every segment).
//!
//! Interactive typing exits exact mode — every accepted character
//! mutation clears the chain — while the two replace seams manage it
//! explicitly: the plain seam clears it before refilling, the scheme seam
//! sets it afterwards through [`InputBuffer::set_exact`].

use oxpinyin_core::graph::ExactSegment;

use super::MAX_INPUT_BYTES;

/// The typed input and the scheme-parse chain over it.
#[derive(Clone, Debug, Default)]
pub(super) struct InputBuffer {
    /// The raw characters typed so far, never longer than
    /// [`MAX_INPUT_BYTES`].
    raw: String,
    /// Pre-parsed exact syllables over `raw` — the scheme-parse seam
    /// (zhuyin, double pinyin). Empty is the full-pinyin mode, where the
    /// scan parses `raw` itself; non-empty pins the graph to exactly
    /// these keys. Spans are absolute over `raw`, and `end() <= raw.len()`
    /// holds for every segment.
    exact: Vec<ExactSegment>,
}

impl InputBuffer {
    /// The raw input as a string slice.
    pub(super) fn as_str(&self) -> &str {
        &self.raw
    }

    /// The raw input as bytes — the coordinate space every scan, matrix
    /// and offset law reads.
    pub(super) const fn as_bytes(&self) -> &[u8] {
        self.raw.as_bytes()
    }

    /// Bytes in the raw buffer.
    pub(super) const fn len(&self) -> usize {
        self.raw.len()
    }

    /// Whether no input has been typed.
    pub(super) const fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    /// Whether `index` lands on a character boundary of the raw buffer.
    pub(super) fn is_char_boundary(&self, index: usize) -> bool {
        self.raw.is_char_boundary(index)
    }

    /// The pre-parsed scheme chain; empty in full-pinyin mode.
    pub(super) fn exact(&self) -> &[ExactSegment] {
        &self.exact
    }

    /// Appends `character` and exits exact mode, unless it would push the
    /// buffer past [`MAX_INPUT_BYTES`] — then nothing changes and `false`
    /// is returned (the caller reports the key as ignored, or stops the
    /// batch). The exact chain is cleared only on an accepted push, so a
    /// character refused at the cap leaves a scheme composition intact.
    pub(super) fn try_push(&mut self, character: char) -> bool {
        if self.raw.len() + character.len_utf8() > MAX_INPUT_BYTES {
            return false;
        }
        self.exact.clear();
        self.raw.push(character);
        true
    }

    /// Drops the last character and exits exact mode — the erase path,
    /// which shrinks the buffer under a live composition.
    pub(super) fn pop(&mut self) {
        self.exact.clear();
        self.raw.pop();
    }

    /// Empties the buffer and the exact chain — `pinyin_reset`'s input
    /// half.
    pub(super) fn clear(&mut self) {
        self.raw.clear();
        self.exact.clear();
    }

    /// Exits exact mode without touching `raw` — the plain replace seam,
    /// which clears the chain before refilling.
    pub(super) fn clear_exact(&mut self) {
        self.exact.clear();
    }

    /// Replaces `raw` with `text` clamped to [`MAX_INPUT_BYTES`], keeping
    /// every character that fits and stopping at the first that would
    /// overflow. Does not touch the exact chain: the plain seam has
    /// already cleared it and the scheme seam sets it afterwards through
    /// [`InputBuffer::set_exact`].
    pub(super) fn refill(&mut self, text: &str) {
        self.raw.clear();
        for character in text.chars() {
            if self.raw.len() + character.len_utf8() > MAX_INPUT_BYTES {
                break;
            }
            self.raw.push(character);
        }
    }

    /// Installs the scheme chain, dropping any segment that reaches past
    /// the (already clamped) buffer end so `end() <= raw.len()` stays an
    /// invariant of the stored segments.
    pub(super) fn set_exact(&mut self, segments: &[ExactSegment]) {
        let raw_len = self.raw.len();
        self.exact = segments
            .iter()
            .copied()
            .filter(|segment| segment.end() <= raw_len)
            .collect();
    }
}
