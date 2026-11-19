//! Implements the libpinyin C API and ABI.
//!
//! Linux packages provide `libpinyin.so.15`.
//! Native Rust interfaces are unsupported internal details.
//! Each build selects exactly one database backend. The default is `tkrzw`.
//!
//! ## Further reading
//!
//! - [C header](https://github.com/shenghaoc/oxpinyin/blob/main/capi/pinyin.h)
//! - [Exported symbols](https://github.com/shenghaoc/oxpinyin/blob/main/capi/libpinyin.ver)
//! - [C library packaging](https://github.com/shenghaoc/oxpinyin/blob/main/docs/packaging.md)
//! - [Backend selection](https://github.com/shenghaoc/oxpinyin/blob/main/docs/runbooks/backends.md#selecting-a-backend)
//! - [Supported external interfaces](https://github.com/shenghaoc/oxpinyin/blob/main/docs/constitution.md#external-interfaces)
//! - [Upstream pin](https://github.com/shenghaoc/oxpinyin/blob/main/tools/oracle/oracle-pin.txt)
//! - [Failure behaviour](https://github.com/shenghaoc/oxpinyin/blob/main/docs/findings/compatibility-policy.md#failure-behaviour)
//!
//! ## Aborts
//!
//! As in libpinyin, malformed or corrupt profile, library or database data can abort the
//! process. A failed database operation or an asserted read or write of stored data can
//! also abort. Individual function descriptions omit these cases.
//!
//! ## Safety
//!
//! The caller must uphold the C header contract. Each function's `# Safety`
//! section identifies the rules below that apply and gives any additional
//! requirements.
//!
//! - **Handles.** A context pointer must be null or point to a live context
//!   returned by [`pinyin_init`]. [`pinyin_fini`] ends that context's lifetime.
//!   The caller must not use the handle again, even if a later context gets
//!   the same address. An instance pointer is null or a live pointer returned
//!   by [`pinyin_alloc_instance`]. An iterator pointer is
//!   null or a pointer that its `begin` function returned and its `end`
//!   function has yet to free.
//! - **Strings.** A `const char *` argument is null or points to a
//!   NUL-terminated string that stays valid and unchanged for the call.
//! - **Out-parameters.** A pointer the function writes through is null or is
//!   valid and aligned for a write of its pointee type.
//! - **Serial calls.** Serialize calls that use the same context or its
//!   instances or iterators.
//!
//! ### Borrowed objects
//!
//! A candidate pointer is null or a pointer borrowed from a live instance by
//! [`pinyin_get_candidate`]. The pointer becomes invalid when the instance's
//! candidate list is rebuilt or cleared, including by [`pinyin_reset`], or
//! when [`pinyin_free_instance`] frees the instance.
//!
//! A key pointer is null or points to a [`ChewingKey`] valid for reads. A
//! key-rest pointer is null or points to a [`ChewingKeyRest`] valid for reads.
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
#![warn(missing_docs)]

mod ffi;

/// Rust-visible re-exports for the in-tree fuzz harness
/// (`fuzz/fuzz_targets/capi_commands.rs`).
/// NOT a stable Rust API: the only supported external interface is the C ABI
/// (pinyin.h); anything here can change or vanish.
#[cfg(feature = "fuzz-api")]
#[allow(missing_docs)]
pub mod fuzz_api {
    pub use crate::candidates::{oxpinyin_choose_aborts, oxpinyin_train_aborts};
    pub use crate::candidates::{
        pinyin_choose_candidate, pinyin_clear_constraint, pinyin_get_candidate,
        pinyin_get_candidate_string, pinyin_get_candidate_type, pinyin_get_n_candidate,
        pinyin_train,
    };
    pub use crate::config::pinyin_set_double_pinyin_scheme;
    pub use crate::context::{pinyin_fini, pinyin_init};
    pub use crate::instance::{pinyin_alloc_instance, pinyin_free_instance, pinyin_reset};
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
pub use oxpinyin::user::DEFAULT_PHRASE_COUNT;
pub use oxpinyin::{NETWORK_DICTIONARY, USER_DICTIONARY};
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
pub use candidates::pinyin_choose_predicted_candidate;
pub use candidates::pinyin_clear_constraint;
pub use candidates::pinyin_get_candidate_nbest_index;
pub use candidates::pinyin_is_user_candidate;
pub use candidates::pinyin_remove_user_candidate;
pub use config::pinyin_load_addon_phrase_library;
pub use cursor::pinyin_get_left_pinyin_offset;
pub use cursor::pinyin_get_pinyin_key;
pub use cursor::pinyin_get_pinyin_key_rest;
pub use cursor::pinyin_get_pinyin_key_rest_length;
pub use cursor::pinyin_get_pinyin_key_rest_positions;
pub use cursor::pinyin_get_pinyin_offset;
pub use cursor::pinyin_get_right_pinyin_offset;
pub use iterators::pinyin_begin_get_bigram_phrases;
pub use iterators::pinyin_begin_get_phrases;
pub use iterators::pinyin_bigram_iterator_get_next_phrase;
pub use iterators::pinyin_bigram_iterator_has_next_phrase;
pub use iterators::pinyin_end_get_bigram_phrases;
pub use iterators::pinyin_end_get_phrases;
pub use iterators::pinyin_iterator_get_next_phrase;
pub use iterators::pinyin_iterator_has_next_phrase;
pub use parse::pinyin_get_parsed_input_length;
pub use parse::pinyin_in_chewing_keyboard;
pub use sentence::pinyin_get_character_offset;
pub use text::pinyin_get_chewing_auxiliary_text;
pub use text::pinyin_get_double_pinyin_auxiliary_text;
pub use text::pinyin_get_full_pinyin_auxiliary_text;
pub use types::BigramExportIterator;
pub use types::ExportIterator;
pub use types::GUint;
pub use types::PinyinOptionT;
pub use user_data::pinyin_remember_user_input;

// C publication and conversion tests live in sibling tests.rs files and tests/it.
// The shared test helpers name the exported functions through the library's
// own name, as the verification tests do.
#[cfg(test)]
extern crate self as oxpinyin_capi;

#[cfg(test)]
mod test_support;
