//! Training-only Han segmenter reproducing libpinyin `ngseg`.
//!
//! Takes raw UTF-8 (line-oriented) and the pinned system model, and emits
//! the `token phrase` / `null_token` stream `gen_ngram` consumes. Never
//! ships with the engine. See `docs/findings/segmenter-port.md`.
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod driver;
mod error;
mod lexicon;
mod model;
mod paths;
mod trellis;

pub use driver::{Emitted, segment_bytes, segment_line};
pub use error::SegmentError;
pub use lexicon::{MAX_PHRASE_LENGTH, PhraseLexicon};
pub use model::{NULL_TOKEN, PINNED_LAMBDA, SENTENCE_START, SegmentModel, parse_table_conf_lambda};
pub use paths::{
    DEFAULT_EXPORT_DIR, EXPORT_DIR_ENV, EXPORT_FILES, MODEL_CACHE_ENV, MODEL_DIR_ENV, MODEL_FILES,
    SegmenterPaths, load_lambda, locate_export_dir, locate_model_dir,
};

use std::path::Path;

/// A loaded segmenter: phrase table + pinned bigram/unigram + λ.
pub struct Segmenter {
    lexicon: PhraseLexicon,
    model: SegmentModel,
    lambda: f32,
}

impl Segmenter {
    /// Opens the phrase index, system bigram, and interpolation2 unigrams.
    ///
    /// `lambda` is the pin's `table.conf` value ([`PINNED_LAMBDA`]) unless
    /// the caller parsed a different file.
    ///
    /// # Errors
    ///
    /// Returns [`SegmentError`] when a table cannot be read.
    pub fn open(paths: &SegmenterPaths, lambda: f32) -> Result<Self, SegmentError> {
        let lexicon = PhraseLexicon::from_phrase_index(&paths.phrase_index)?;
        let model = SegmentModel::open(&paths.bigram, &paths.interpolation2, &lexicon)?;
        Ok(Self {
            lexicon,
            model,
            lambda,
        })
    }

    /// Discovers the system-table export and the model20 cache, then opens them
    /// with [`PINNED_LAMBDA`] (or `table.conf` when `table_conf` is set).
    ///
    /// # Errors
    ///
    /// Returns [`SegmentError`] when a path is missing or a table fails.
    pub fn discover(table_conf: Option<&Path>) -> Result<Self, SegmentError> {
        let paths = SegmenterPaths::discover()?;
        let lambda = load_lambda(table_conf).unwrap_or(PINNED_LAMBDA);
        Self::open(&paths, lambda)
    }

    /// In-memory constructor for tests.
    #[must_use]
    pub fn from_parts(lexicon: PhraseLexicon, model: SegmentModel, lambda: f32) -> Self {
        Self {
            lexicon,
            model,
            lambda,
        }
    }

    /// The interpolation weight in force.
    #[must_use]
    pub const fn lambda(&self) -> f32 {
        self.lambda
    }

    /// The phrase table.
    #[must_use]
    pub const fn lexicon(&self) -> &PhraseLexicon {
        &self.lexicon
    }

    /// Segments one UTF-8 line (no trailing newline).
    ///
    /// # Errors
    ///
    /// Returns [`SegmentError`] when a bigram table read fails.
    pub fn segment_line(&self, line: &str) -> Result<Vec<Emitted>, SegmentError> {
        segment_line(&self.lexicon, &self.model, self.lambda, line)
    }

    /// Segments a whole file, including the trailing file-tail `null_token`.
    ///
    /// # Errors
    ///
    /// Returns [`SegmentError`] when a bigram table read fails.
    pub fn segment_bytes(&self, input: &[u8], extra_enter: bool) -> Result<String, SegmentError> {
        segment_bytes(&self.lexicon, &self.model, self.lambda, input, extra_enter)
    }
}
