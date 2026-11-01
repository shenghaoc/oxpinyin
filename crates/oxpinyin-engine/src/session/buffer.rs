//! The raw input buffer and its pre-parsed scheme chain.
//!
//! Owns the input-side invariant, so no caller can leave the session with
//! an input the rest of the pipeline would slice out of bounds: the
//! exact-segment chain, whose spans are absolute over `raw` and never
//! reach past its end (`end() <= raw.len()` for every segment).
//!
//! The buffer has no length limit. The pin's full-pinyin parser stops at
//! byte 32767 only because it keeps its steps in `gint16`s, a language
//! artefact oxpinyin does not reproduce
//! (`docs/findings/1501-gint16-input-cap.md`).
//!
//! Interactive typing exits exact mode — every accepted character
//! mutation clears the chain — while the two replace seams manage it
//! explicitly: the plain seam clears it before refilling, the scheme seam
//! sets it afterwards through [`InputBuffer::set_exact`].

use std::sync::{Arc, OnceLock};

use oxpinyin_core::graph::{ExactSegment, SegmentGraph};

/// The typed input and the scheme-parse chain over it.
#[derive(Clone, Debug, Default)]
pub(super) struct InputBuffer {
    /// The raw characters typed so far.
    raw: String,
    /// Whole-input parser graph, shared by the matrix, window and length queries.
    /// Input/provenance mutations and option changes invalidate it.
    pub(super) graph: OnceLock<Arc<SegmentGraph>>,
    /// Derived topology only; every input/provenance mutation invalidates it.
    pub(super) matrix: Option<super::matrix::ParsedMatrix>,
    pub(super) ending_matrix: Option<super::matrix::ParsedMatrix>,
    /// Whether apostrophes came from canonical formatting rather than typing.
    /// Used only by trellis generation, never by the ordinary scan or graph.
    formatting_separators: bool,
    /// Pre-parsed exact syllables over `raw` — the scheme-parse seam
    /// (zhuyin, double pinyin). Empty is the full-pinyin mode, where the
    /// scan parses `raw` itself; non-empty pins the graph to exactly
    /// these keys. Spans are absolute over `raw`, and `end() <= raw.len()`
    /// holds for every segment.
    exact: Vec<ExactSegment>,
    /// Pre-parsed full pinyin retains the full-pinyin matrix transforms.
    parsed_full_pinyin: bool,
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

    pub(super) fn full_pinyin(&self) -> bool {
        self.exact.is_empty() || self.parsed_full_pinyin
    }

    pub(super) fn set_full_pinyin(&mut self, segments: &[ExactSegment]) {
        self.graph.take();
        self.matrix = None;
        self.ending_matrix = None;
        self.set_exact(segments);
        self.parsed_full_pinyin = true;
        self.formatting_separators = true;
    }

    /// Appends `character` and exits exact mode.
    pub(super) fn push(&mut self, character: char) {
        self.graph.take();
        self.matrix = None;
        self.ending_matrix = None;
        self.exact.clear();
        self.parsed_full_pinyin = false;
        self.formatting_separators = false;
        self.raw.push(character);
    }

    /// Drops the last character and exits exact mode — the erase path,
    /// which shrinks the buffer under a live composition.
    pub(super) fn pop(&mut self) {
        self.graph.take();
        self.matrix = None;
        self.ending_matrix = None;
        self.exact.clear();
        self.parsed_full_pinyin = false;
        self.formatting_separators = false;
        self.raw.pop();
    }

    /// Empties the buffer and the exact chain — `pinyin_reset`'s input
    /// half.
    pub(super) fn clear(&mut self) {
        self.graph.take();
        self.matrix = None;
        self.ending_matrix = None;
        self.raw.clear();
        self.exact.clear();
        self.parsed_full_pinyin = false;
        self.formatting_separators = false;
    }

    /// Exits exact mode without touching `raw` — the plain replace seam,
    /// which clears the chain before refilling.
    pub(super) fn clear_exact(&mut self) {
        self.graph.take();
        self.matrix = None;
        self.ending_matrix = None;
        self.exact.clear();
        self.parsed_full_pinyin = false;
        self.formatting_separators = false;
    }

    /// Separator provenance is observable only in trellis generation.
    pub(super) fn physical_separators(&self) -> bool {
        !self.formatting_separators && self.exact.is_empty()
    }

    /// Stores separator provenance for the private trellis generation view.
    pub(super) fn set_physical_separators(&mut self, physical: bool) {
        self.graph.take();
        self.matrix = None;
        self.ending_matrix = None;
        self.formatting_separators = !physical;
    }

    /// Replaces `raw` with `text`. Does not touch the exact chain: the
    /// plain seam has already cleared it and the scheme seam sets it
    /// afterwards through [`InputBuffer::set_exact`].
    pub(super) fn refill(&mut self, text: &str) {
        self.graph.take();
        self.matrix = None;
        self.ending_matrix = None;
        text.clone_into(&mut self.raw);
    }

    /// Installs the scheme chain, dropping any segment that reaches past
    /// the buffer end so `end() <= raw.len()` stays an
    /// invariant of the stored segments.
    pub(super) fn set_exact(&mut self, segments: &[ExactSegment]) {
        self.graph.take();
        self.matrix = None;
        self.ending_matrix = None;
        self.formatting_separators = false;
        self.parsed_full_pinyin = false;
        let raw_len = self.raw.len();
        self.exact = segments
            .iter()
            .copied()
            .filter(|segment| segment.end() <= raw_len)
            .collect();
    }
}
