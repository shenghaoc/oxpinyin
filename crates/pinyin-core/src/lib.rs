//! Pure pinyin algorithms: parsing, SegmentGraph, k-best search, scoring
//! traits. No I/O, no platform deps, no `unsafe`. Internal crate — the
//! supported public API is `pinyin-engine`.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cost;
pub mod fixture;
pub mod graph;
pub mod kbest;
mod parser;
pub mod scoring;
mod syllables;
mod vocab;

pub use parser::{
    Completeness, FullPinyinParser, MAX_PARSE_RESULTS, ParseError, ParseResult, ParsedSyllable,
};
pub use syllables::{
    FULL_PINYIN_SYLLABLE_COUNT, FULL_PINYIN_SYLLABLES, INCOMPLETE_PINYIN_KEY_COUNT,
    INCOMPLETE_PINYIN_KEYS, MAX_SYLLABLE_LEN,
};
pub use vocab::{PhraseEntry, PhraseToken, SYLLABLE_KEY_COUNT, SyllableKey};

/// Deterministic signed cost used by scoring seams.
///
/// The concrete scale is defined by the decoder/scoring specification.
pub type Cost = i64;

/// Read-only lookup seam for dictionaries.
///
/// Implementations return entries in stable order, use an empty vector for a
/// successful lookup with no entries, and do not panic on caller input.
pub trait Dictionary {
    /// Syllable representation accepted by this dictionary.
    type Syllable;

    /// Entry representation returned by this dictionary.
    type Entry;

    /// Lookup failure reported by this dictionary.
    type Error;

    /// Looks up entries matching `syllables`.
    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error>;

    /// Whether a stored phrase's pinyin can extend `syllables`.
    ///
    /// This is libpinyin's `SEARCH_CONTINUED` probe: the phrase index reports
    /// the bit when the key sequence is a prefix of some stored phrase's
    /// pinyin, which is what lets the candidate window scan stop widening.
    /// When `syllables` contains an initial-only key the probe runs on the
    /// initial sequence instead — the index the pin uses for incomplete
    /// spellings — where a complete key contributes its own initial and an
    /// initial-only key stands for every syllable that shares it.
    ///
    /// Defaulted to `true` so the frozen implementors keep compiling: always
    /// continuing the scan yields the same candidate set (the probe only
    /// stops windows that would return nothing), at the cost of searching
    /// every window to the end of the input
    /// (`docs/findings/core-trait-seam.md`: the seam grows by defaulted
    /// methods only).
    fn phrase_prefix_exists(&self, _syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

impl<D: Dictionary + ?Sized> Dictionary for &D {
    type Syllable = D::Syllable;
    type Entry = D::Entry;
    type Error = D::Error;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        (**self).lookup(syllables)
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        (**self).phrase_prefix_exists(syllables)
    }
}

/// Scoring and observation seam for explicit user-learning state.
///
/// Implementations are deterministic for the same explicit input and state,
/// do not consult hidden process-global state, and do not panic on caller
/// input.
pub trait UserModel {
    /// Token representation scored and observed by this model.
    type Token;

    /// Scoring or observation failure reported by this model.
    type Error;

    /// Returns the user-specific cost for `token` after `history`.
    fn score(&self, history: &[Self::Token], token: &Self::Token) -> Result<Cost, Self::Error>;

    /// Records an accepted `token` after `history`.
    ///
    /// Learning-off callers omit this operation entirely.
    fn observe(&mut self, history: &[Self::Token], token: &Self::Token) -> Result<(), Self::Error>;
}

/// Deterministic language-model scoring seam.
///
/// Implementations do not panic on caller input. They may combine the supplied
/// edge cost with their own cost and report arithmetic or backend failures
/// through [`Result`].
pub trait LanguageModel {
    /// Token representation scored by this model.
    type Token;

    /// Scoring failure reported by this model.
    type Error;

    /// Returns the model cost for `token` after `history` with `edge_cost`
    /// available for deterministic composition.
    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error>;

    /// The model's real unigram frequency of `token`, when the model carries
    /// one.
    ///
    /// `None` means the model exposes no frequency table at all; the engine
    /// then keeps its pre-frequency behaviour. `Some(0)` is a real table miss
    /// — the phrase has no frequency — and ranks below every counted phrase.
    ///
    /// Defaulted so the frozen implementors and any third-party model keep
    /// compiling unchanged (`docs/findings/core-trait-seam.md`: the seam grows
    /// by defaulted methods only).
    fn unigram_freq(&self, _token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        Ok(None)
    }

    /// Whether the model carries a real unigram frequency table.
    ///
    /// This is the construction switch: when it returns `true` the engine
    /// collects and ranks candidates by the pinned construction, and when it
    /// returns `false` the pre-frequency behaviour runs. Defaulted to `false`
    /// so the frozen implementors and any third-party model keep compiling
    /// unchanged.
    fn has_real_unigrams(&self) -> bool {
        false
    }
}

impl<L: LanguageModel + ?Sized> LanguageModel for &L {
    type Token = L::Token;
    type Error = L::Error;

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        (**self).score(history, token, edge_cost)
    }

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        (**self).unigram_freq(token)
    }

    fn has_real_unigrams(&self) -> bool {
        (**self).has_real_unigrams()
    }
}

/// Total byte-input parsing seam.
pub trait InputParser {
    /// One owned parse alternative.
    type Parse;

    /// Parser backend or resource-limit failure (for example too many alternatives).
    ///
    /// Malformed, junk, and partial caller input are represented in parse
    /// outputs rather than as this error.
    type Error;

    /// Returns every valid segmentation of `input` in frozen path-set order.
    fn parse(&self, input: &[u8]) -> Result<Vec<Self::Parse>, Self::Error>;
}
