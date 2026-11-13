//! The C ABI of libpinyin's public API — the full live surface, all 79
//! `pinyin_*` symbols `libpinyin.ver` exports, built as `libpinyin.so.15`.
//!
//! Every `#[unsafe(no_mangle)] pub extern "C" fn` matches the signature in
//! upstream's `src/pinyin.h` at the pin (`074a2219`, 2.11.92)
//! symbol-for-symbol; that header is byte-identical to tag 2.11.91's, and
//! the checked-in `pinyin.h` is that file. History: the W8 bootstrap set
//! was the 50 pinned ibus-libpinyin 1.16.5 symbols plus
//! `pinyin_get_parsed_input_length` and `pinyin_clear_constraint`; the
//! surface closed on the full 79 on 2026-08-30 (`docs/findings/abi-reference.md`
//! §6).
//!
//! ## Panic discipline
//!
//! Panics follow constitution §4's pin-matched failure policy; class (b)
//! rulings remain authoritative, and where the pin asserts or aborts this
//! crate calls `std::process::abort()` (class (c) retired 2026-10-31 UTC). The library crates on this
//! facade's path (`oxpinyin-core`, `-data`, `-store`, `-engine`, `-facade`,
//! `-runtime`, `-user` and this crate) deny `clippy::unwrap_used`/
//! `expect_used`/`panic`/`panic_in_result_fn`/`unreachable` outside tests —
//! the macro-only `oxpinyin-capi-marshal` carries the same lints. Permitted
//! sites require narrowly scoped, justified
//! `#[expect]` attributes under `rust-conventions.md`; these prove Clippy
//! saw a construct, not that its failure condition matches the pin.
//! Other panic sources also need to respect §4. A panic that reaches an
//! `extern "C"` boundary aborts
//! the process. There is deliberately no panic-catching wrapper.
//! Abort at the ABI boundary applies under either panic strategy (the
//! release profile is `unwind`; `panic = "abort"` was tried and reverted
//! for its keystroke-cycle cost).
//!
//! Opaque handles cross the boundary as `*mut T` via `Box::into_raw` /
//! `Box::from_raw`. Every incoming pointer is null-checked before deref.
//! `// SAFETY:` documents each `unsafe` block.
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
#![expect(
    unsafe_code,
    reason = "the C ABI crate; every block carries a SAFETY comment (constitution §5)"
)]
// The entire crate is a pointer-taking C ABI: soundness of these entry
// points rests on the documented pinyin.h contract (opaque handles,
// out-params, ownership), not on Rust-side unsafe marking. Exposing the
// fuzz_api facade makes that pointer-by-contract style lint-visible, so
// the deviation is recorded here once instead of per function.
#![allow(
    clippy::not_unsafe_ptr_arg_deref,
    reason = "pointer-by-contract C ABI; fires only when the fuzz-api facade is compiled in, so an expectation would fail the default build"
)]
#![warn(missing_docs)]

mod ffi;

/// Rust-visible re-exports for the in-tree fuzz harness
/// (`fuzz/fuzz_targets/capi_commands.rs`).
/// NOT a stable Rust API: the only supported external interface is the C ABI
/// (pinyin.h); anything here can change or vanish.
#[cfg(feature = "fuzz-api")]
#[allow(missing_docs)]
pub mod fuzz_api {
    #[cfg(not(feature = "shipped"))]
    pub use crate::candidates::oxpinyin_train_aborts;
    pub use crate::candidates::{
        pinyin_choose_candidate, pinyin_clear_constraint, pinyin_get_candidate,
        pinyin_get_candidate_string, pinyin_get_candidate_type, pinyin_get_n_candidate,
        pinyin_train,
    };
    pub use crate::config::pinyin_set_double_pinyin_scheme;
    pub use crate::context::{pinyin_fini, pinyin_init};
    pub use crate::instance::{pinyin_alloc_instance, pinyin_free_instance, pinyin_reset};
    #[cfg(not(feature = "shipped"))]
    pub use crate::iterators::oxpinyin_import_aborts;
    pub use crate::iterators::{
        pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
        pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_get_next_phrase,
        pinyin_iterator_has_next_phrase,
    };
    pub use crate::parse::{pinyin_get_parsed_input_length, pinyin_parse_more_full_pinyins};
    pub use crate::sentence::{pinyin_get_sentence, pinyin_guess_sentence};
    pub use crate::types::{
        ExportIterator, GChar, ImportIterator, LookupCandidate, PinyinContext, PinyinInstance,
        lookup_candidate_type_t,
    };
}
mod dict;
mod keys;
mod phrase;
mod state;
mod types;

mod aux_matrix;
mod candidates;
mod config;
mod context;
mod cursor;
mod instance;
mod iterators;
mod live;
mod parse;
mod predict;
mod sentence;
mod text;
mod user_data;

#[cfg(feature = "tools")]
pub mod tool;

pub use context::{pinyin_fini, pinyin_init, pinyin_save};
pub use iterators::{pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase};
pub use oxpinyin_user::{DEFAULT_PHRASE_COUNT, NETWORK_DICTIONARY, USER_DICTIONARY};
pub use state::ExportedBigramRow;
pub use types::{ChewingKey, ChewingKeyRest, GChar, LookupCandidate, PinyinInstance};
pub use types::{GArray, ImportIterator, PhraseTokenT, PinyinContext, lookup_candidate_type_t};

// ── candidates ─────────────────────────────────────────────
pub use candidates::pinyin_choose_candidate;
pub use candidates::pinyin_get_candidate;
pub use candidates::pinyin_get_candidate_string;
pub use candidates::pinyin_get_candidate_type;
pub use candidates::pinyin_get_n_candidate;
pub use candidates::pinyin_train;

// ── config ─────────────────────────────────────────────
pub use config::pinyin_mask_out;
pub use config::pinyin_set_double_pinyin_scheme;
pub use config::pinyin_set_full_pinyin_scheme;
pub use config::pinyin_set_options;
pub use config::pinyin_set_zhuyin_scheme;
pub use config::pinyin_unload_addon_phrase_library;

// ── instance ─────────────────────────────────────────────
pub use instance::pinyin_alloc_instance;
pub use instance::pinyin_free_instance;
pub use instance::pinyin_get_context;
pub use instance::pinyin_reset;

// ── parse ─────────────────────────────────────────────
pub use parse::pinyin_parse_more_chewings;
pub use parse::pinyin_parse_more_double_pinyins;
pub use parse::pinyin_parse_more_full_pinyins;

pub use cursor::pinyin_get_pinyin_string;
pub use cursor::pinyin_get_pinyin_strings;
pub use cursor::pinyin_get_zhuyin_string;
// ── dict ─────────────────────────────────────────────
pub use config::pinyin_load_phrase_library;
pub use config::pinyin_unload_phrase_library;
pub use dict::pinyin_lookup_tokens;
pub use dict::pinyin_token_add_unigram_frequency;
pub use dict::pinyin_token_get_n_pronunciation;
pub use dict::pinyin_token_get_nth_pronunciation;
pub use dict::pinyin_token_get_phrase;
pub use dict::pinyin_token_get_unigram_frequency;

pub use keys::pinyin_get_luoma_pinyin_string;
pub use keys::pinyin_get_pinyin_is_incomplete;
pub use keys::pinyin_get_secondary_zhuyin_string;
pub use keys::pinyin_parse_chewing;
pub use keys::pinyin_parse_double_pinyin;
pub use keys::pinyin_parse_full_pinyin;

// ── sentence ─────────────────────────────────────────────
pub use predict::pinyin_guess_predicted_candidates;
pub use sentence::pinyin_get_sentence;
pub use sentence::pinyin_guess_candidates;
pub use sentence::pinyin_guess_predicted_candidates_with_punctuations;
pub use sentence::pinyin_guess_sentence;
pub use sentence::pinyin_guess_sentence_with_prefix;

// ── phrase ─────────────────────────────────────────────
pub use phrase::pinyin_get_n_phrase;
pub use phrase::pinyin_get_phrase_token;
pub use phrase::pinyin_phrase_segment;

// ── text ─────────────────────────────────────────────
pub use text::pinyin_get_chewing_auxiliary_text;
pub use text::pinyin_get_full_pinyin_auxiliary_text;

// The ABI-driven black-box suites live in `tests/abi.rs`. These three mods
// are white-box unit tests of the ABI layer: they drive the C symbols for
// setup and act, then assert directly on `CapiInstance` internals through
// `instance_ref`. Moving them under `tests/` would require exporting
// internals purely for test inspection or weakening the assertions, so
// they stay at the unit layer (docs/testing/upstream-test-coverage.md).
#[cfg(test)]
mod boundary_tests;
#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod guess_offset_tests;
#[cfg(test)]
mod matrix_bound_tests;
#[cfg(test)]
mod parse_matrix_tests;
#[cfg(all(test, feature = "tkrzw"))]
mod pinned_data_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod union_e2e_tests;
