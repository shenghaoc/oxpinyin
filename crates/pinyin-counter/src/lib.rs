//! Training-only n-gram counter reproducing libpinyin `gen_ngram`.
//!
//! Consumes the `ngseg` segmented-token stream (`{token} {phrase}` lines,
//! `0 {raw}` unknowns, `0 ` sentence separators — T1's
//! [`Emitted::to_ngseg_line`](pinyin_segment::Emitted::to_ngseg_line)) and
//! produces integer unigram (`token → count`) and bigram
//! (`(prev, cur) → count`) counts. Never ships with the engine. See
//! `docs/findings/counter-port.md`.
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod counter;
mod counts;
mod error;
mod parse;

pub use counter::Counter;
pub use counts::Counts;
pub use error::CounterError;
pub use parse::{parse_interpolation_dump, parse_ngseg};

use pinyin_segment::PhraseLexicon;

impl Counter {
    /// Builds a counter with the freq-1 floor seeded from `lexicon` (the
    /// tokens `gen_unigram` raises by one).
    #[must_use]
    pub fn from_lexicon(lexicon: &PhraseLexicon, train_pi_gram: bool) -> Self {
        Self::new(train_pi_gram, lexicon.tokens())
    }
}

/// Counts an ngseg stream with the floor seeded from `lexicon`.
///
/// # Errors
///
/// Returns [`CounterError::MalformedLine`] when a line is not
/// `token phrase`.
pub fn count_ngseg(
    lexicon: &PhraseLexicon,
    text: &str,
    train_pi_gram: bool,
) -> Result<Counts, CounterError> {
    let tokens = parse_ngseg(text)?;
    Ok(Counter::from_lexicon(lexicon, train_pi_gram).count(&tokens))
}
