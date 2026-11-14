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
//! (`docs/findings/upstream-divergences.md#the-full-pinyin-parses-16-bit-steps`).
//!
//! Interactive typing exits exact mode — every accepted character
//! mutation clears the chain — while the two replace seams manage it
//! explicitly: the plain seam clears it before refilling, the scheme seam
//! sets it afterwards through [`InputBuffer::set_exact`].

use oxpinyin_core::graph::{Edge, ExactSegment};

/// The fewest-keys path of the whole-input graph and the filtered parse
/// length it gives: the path's last byte extended over a trailing
/// apostrophe run (`pinyin_parser2.cpp:237-251` at the pin).
#[derive(Clone, Debug, Default)]
pub(super) struct SelectedPath {
    pub(super) edges: Vec<Edge>,
    pub(super) parsed_len: usize,
}

/// What a parse of the raw input leaves behind, as the pin's parse leaves
/// `m_parsed_len`, `m_parsed_key_len` and `m_matrix` on the instance
/// (`pinyin.cpp:1497-1525`, `zhuyin.cpp:1017-1076` at 074a2219). Every
/// parse seam fills it ([`super::Session::reparse`]); readers take it as it
/// stands, so an option word changed after the parse does not change it.
#[derive(Clone, Debug, Default)]
pub(super) struct Parsed {
    /// The selected key path and the parsed length read from it.
    pub(super) selected: SelectedPath,
    /// The scan matrix the parse filled.
    pub(super) retained: super::matrix::RetainedParse,
}

/// The typed input and the scheme-parse chain over it.
#[derive(Clone, Debug, Default)]
pub(super) struct InputBuffer {
    /// The raw characters typed so far.
    raw: String,
    /// The last parse of `raw`. A mutation below leaves it as it was; the
    /// parse seam that made the mutation refills it.
    pub(super) parsed: Parsed,
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
        self.set_exact(segments);
        self.parsed_full_pinyin = true;
        self.formatting_separators = true;
    }

    /// Empties the buffer and the exact chain — `pinyin_reset`'s input
    /// half.
    pub(super) fn clear(&mut self) {
        self.parsed = Parsed::default();
        self.raw.clear();
        self.exact.clear();
        self.parsed_full_pinyin = false;
        self.formatting_separators = false;
    }

    /// Exits exact mode without touching `raw` — the plain replace seam,
    /// which clears the chain before refilling.
    pub(super) fn clear_exact(&mut self) {
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
        self.formatting_separators = !physical;
    }

    /// Replaces `raw` with `text`. Does not touch the exact chain: the
    /// plain seam has already cleared it and the scheme seam sets it
    /// afterwards through [`InputBuffer::set_exact`].
    pub(super) fn refill(&mut self, text: &str) {
        text.clone_into(&mut self.raw);
    }

    /// Installs the scheme chain, dropping any segment that reaches past
    /// the buffer end so `end() <= raw.len()` stays an
    /// invariant of the stored segments.
    pub(super) fn set_exact(&mut self, segments: &[ExactSegment]) {
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
