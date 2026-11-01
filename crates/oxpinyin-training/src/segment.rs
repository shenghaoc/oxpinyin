//! Training-only Han phrase data shared by the counter, emitter and λ
//! tools: the phrase lexicon over the system chunk files, the model
//! constants and `table.conf` λ. The `ngseg`, `spseg` and `mergeseq`
//! binaries own their pin interfaces in `oxpinyin-utils`; this module keeps
//! the loaders those training stages reuse. Never ships with the engine.
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![warn(missing_docs)]

mod error;
mod lexicon;
mod model;
mod paths;
pub mod tool_cli;

pub use error::SegmentError;
// Re-exported so the training-tool binaries name export tables in the
// compiled-in backend's format without a direct oxpinyin-data edge.
pub use lexicon::{MAX_PHRASE_LENGTH, PhraseLexicon};
pub use model::{NULL_TOKEN, PINNED_LAMBDA, SENTENCE_START, load_table_conf_lambda};
pub use oxpinyin_data::SystemDbm;
pub use paths::{
    DEFAULT_EXPORT_DIR, EXPORT_DBMS, EXPORT_DIR_ENV, MODEL_CACHE_ENV, MODEL_DIR_ENV, MODEL_FILES,
    SegmenterPaths, load_lambda, locate_export_dir, locate_model_dir,
};
