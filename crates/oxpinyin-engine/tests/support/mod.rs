//! Helpers the engine's integration tests share.

use oxpinyin_engine::Candidate;

/// A candidate read as the text it holds. Every candidate these tests read
/// comes from a fixture of Unicode scalar values, so its string is text.
pub trait CandidateText {
    fn text(&self) -> &str;
}

impl CandidateText for Candidate {
    fn text(&self) -> &str {
        self.string().as_str().expect("a candidate made of text")
    }
}
