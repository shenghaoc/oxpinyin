//! The Rust layer of the libpinyin and libzhuyin C libraries.
//!
//! `oxpinyin-capi` and `oxzhuyin-capi` build on these context, instance, candidate and
//! cursor operations. Both follow the same libpinyin pin. Each C crate chooses its own
//! option forwarding, cursor behaviour and profile-marker policy, and owns the ABI handle
//! types and pointer marshalling. The
//! [architecture](https://github.com/shenghaoc/oxpinyin/blob/main/docs/architecture.md#facades)
//! describes the layering.
//!
//! Starting a parse clears the stored parse mode and original keys. Constraints and
//! sentence rows have their own lifetimes.
//!
//! ## Aborts
//!
//! Malformed or corrupt stored data and failed backend operations can abort the process,
//! as in libpinyin. Individual items omit these aborts. Their `Aborts` sections describe
//! termination caused by caller arguments or call order, or by an operation that exits
//! the process. The
//! [compatibility policy](https://github.com/shenghaoc/oxpinyin/blob/main/docs/findings/compatibility-policy.md#failure-behaviour)
//! sets this failure behaviour.

// Constitution §4: keep these non-test denials. A permitted site needs the
// narrowest justified #[expect], citing the pin's abort site or an invariant
// making failure unreachable; see docs/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// Inline #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]
#![warn(missing_docs)]

mod context;
mod cursor;
mod export_rows;
mod import;
mod instance;
mod live;
mod offsets;
mod parse;
mod predict;
mod snapshot;
#[cfg(test)]
use crate::user::test_child;
#[cfg(test)]
mod test_fixture;
#[cfg(test)]
mod tests;

pub use context::{
    ContextCore, InitRefusal, LibraryLoadFault, LibraryRowAssert, LiveOptions, LiveOptionsInner,
    MaskOutOutcome, OpenFailure, SaveOutcome, null_chunk_asserts,
};
pub use cursor::{CursorFault, CursorLaw, KeyAt, ModeKeys, SpanSource};
pub use export_rows::{
    BigramExportWalk, BigramProbeFault, BigramStep, ExportAssert, ExportRow, ExportStep,
    ExportedBigramRow, GramLoadFault, PhraseExportCursor,
};
pub use import::ImportPlan;
pub use instance::{
    InstanceCore, InstanceMut, InstanceRead, InstanceState, InstanceView, PublishedRow,
    TrainingRefusal, guess_result, refuse_lookup_gram, training_answer,
};
pub use live::LiveContexts;
pub use offsets::{
    double_offset_is_mid_key, double_session_offset, zhuyin_lookup_session_offset,
    zhuyin_offset_is_mid_key, zhuyin_original_offset, zhuyin_session_offset,
};
pub use parse::{ChewingCorrections, PinyinCorrections, full_scheme, zhuyin_scheme};
pub use predict::{compute_prefixes, merged_suggestions, predicted_bigram_items};
pub use snapshot::{CandidateSnapshot, SnapshotRow, TextSpan};

// The items of the modules below that the two C crates name. The C crates import
// them from here; no other module is part of the library's interface.
pub use crate::data::{TableConf, TableConfSite, table_conf_load_failed};
pub use crate::engine::{
    BEFORE_CURSOR_ANCHOR, Candidate, CandidateKind, CandidateList, CharacterOffset, EngineError,
    Law, MatrixKey, RowText, SORT_WITHOUT_SENTENCE_CANDIDATE, Selection, SelectionTarget,
    SessionState, amplified_frequency, key_pronounced_by, rank_by_frequency, remember_walk,
};
pub use crate::graph::SegmentGraph;
pub use crate::input::filename;
pub use crate::marshal::{
    SentencePhrase, direct_zhuyin_reading, parser_prefix, phrase_text_bytes, phrase_text_len,
    prefix_text, scalar_text, utf8_to_ucs4,
};
pub use crate::model::Dictionary;
pub use crate::parser::{
    ChewingKey, DoublePinyinScheme, FullPinyinIndexParse, FullPinyinScheme, KeyText, ZhuyinParse,
    ZhuyinScheme, parse_zhuyin_direct,
};
pub use crate::runtime::RuntimeDict;
pub use crate::user::{
    INITIAL_SEED, PinyinKey, UnigramAdd, UserConfLaw, UserStore, UserStoreError, is_user_token,
    toned_key, unigram_delta,
};
pub use crate::vocab::{
    ADDON_DICTIONARY, GBK_DICTIONARY, MAX_PHRASE_LENGTH, NETWORK_DICTIONARY, NULL_TOKEN,
    PHRASE_INDEX_LIBRARY_COUNT, PHRASE_INDEX_LIBRARY_MASK, PHRASE_MASK, PhraseText, PhraseToken,
    SyllableKey, TSI_DICTIONARY, USER_DICTIONARY, phrase_index_make_token,
};

// pinyin_init, pinyin.cpp:329.
/// Initial pinyin options. Tone input is enabled and incomplete input is disabled.
pub const PINYIN_DEFAULT_OPTION_WORD: crate::OptionBits = crate::OptionBits::USE_TONE;

// zhuyin_init, zhuyin.cpp:273.
/// Initial Zhuyin options. Tone input and forced tone are enabled and incomplete input is
/// disabled.
pub const ZHUYIN_DEFAULT_OPTION_WORD: crate::OptionBits =
    crate::OptionBits::USE_TONE.union(crate::OptionBits::FORCE_TONE);
