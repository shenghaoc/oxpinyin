//! Shared context, instance, candidate, and cursor operations for the pinyin and Zhuyin C
//! facades. Both use [`crate::runtime`] and the same libpinyin pin. Each facade chooses
//! its own option forwarding, cursor behavior, and profile-marker policy.
//!
//! Starting a parse clears the stored parse mode and original keys. Constraints and
//! sentence rows have their own lifetimes. The C crates own the ABI handle types and
//! pointer marshalling.

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
    ContextCore, FALLBACK_CLEARED_BIT, InitRefusal, LibraryLoadFault, LibraryRowAssert,
    LiveOptions, LiveOptionsInner, MaskOutOutcome, OpenFailure, SaveOutcome, null_chunk_asserts,
};
pub use cursor::{CursorFault, CursorLaw, KeyAt, ModeKeys, SpanSource};
pub use export_rows::{
    BigramExportWalk, BigramProbeFault, BigramStep, ExportAssert, ExportRow, ExportStep,
    ExportedBigramRow, GramLoadFault, PhraseExportCursor,
};
pub use import::ImportPlan;
pub use instance::{
    InstanceCore, InstanceMut, InstanceRead, InstanceState, InstanceView, PublishedRow,
    SelectionSpan, TrainingRefusal, guess_result, refuse_lookup_gram, training_answer,
};
pub use live::LiveContexts;
pub use offsets::{
    double_offset_is_mid_key, double_session_offset, zhuyin_lookup_session_offset,
    zhuyin_offset_is_mid_key, zhuyin_original_offset, zhuyin_session_offset,
};
pub use parse::{ChewingCorrections, PinyinCorrections, double_scheme, full_scheme, zhuyin_scheme};
pub use predict::{compute_prefixes, merged_suggestions};
pub use snapshot::{CandidateSnapshot, SnapshotRow, TextSpan};

// pinyin_init, pinyin.cpp:329.
/// Initial pinyin options. Tone input is enabled and incomplete input is disabled.
pub const PINYIN_DEFAULT_OPTION_WORD: crate::OptionBits = crate::OptionBits::USE_TONE;

// zhuyin_init, zhuyin.cpp:273.
/// Initial Zhuyin options. Tone input and forced tone are enabled and incomplete input is
/// disabled.
pub const ZHUYIN_DEFAULT_OPTION_WORD: crate::OptionBits =
    crate::OptionBits::USE_TONE.union(crate::OptionBits::FORCE_TONE);
