//! Implements the libzhuyin C API and ABI.
//!
//! Linux packages provide `libzhuyin.so.15`.
//! Native Rust interfaces are unsupported internal details.
//! Each build selects exactly one database backend. The default is `tkrzw`.
//!
//! ## Further reading
//!
//! - [C header](https://github.com/shenghaoc/oxpinyin/blob/main/zhuyin-capi/zhuyin.h)
//! - [Exported symbols](https://github.com/shenghaoc/oxpinyin/blob/main/zhuyin-capi/libzhuyin.ver)
//! - [C library packaging](https://github.com/shenghaoc/oxpinyin/blob/main/docs/packaging.md)
//! - [Backend selection](https://github.com/shenghaoc/oxpinyin/blob/main/docs/runbooks/backends.md#selecting-a-backend)
//! - [Supported external interfaces](https://github.com/shenghaoc/oxpinyin/blob/main/docs/constitution.md#external-interfaces)
//! - [Upstream pin](https://github.com/shenghaoc/oxpinyin/blob/main/tools/oracle/oracle-pin.txt)
//! - [Failure behaviour](https://github.com/shenghaoc/oxpinyin/blob/main/docs/findings/compatibility-policy.md#failure-behaviour)
//!
//! ## Safety
//!
//! The caller must uphold the C header contract. Each function's `# Safety`
//! section identifies the rules below that apply and gives any additional
//! requirements.
//!
//! - **Handles.** A context pointer must be null or point to a live context
//!   returned by [`zhuyin_init`]. [`zhuyin_fini`] ends that context's lifetime.
//!   The caller must not use the handle again, even if a later context gets
//!   the same address. An instance pointer is null or a live pointer returned
//!   by [`zhuyin_alloc_instance`]. An iterator pointer is
//!   null or a pointer that [`zhuyin_begin_add_phrases`] returned and
//!   [`zhuyin_end_add_phrases`] has yet to free.
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
//! [`zhuyin_get_candidate`]. The pointer becomes invalid when the instance's
//! candidate list is rebuilt or cleared, including by [`zhuyin_reset`], or
//! when [`zhuyin_free_instance`] frees the instance.
//!
//! A key pointer is null or points to a [`ChewingKey`] valid for reads. A
//! key-rest pointer is null or points to a [`ChewingKeyRest`] valid for reads.
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

mod candidates;
mod config;
mod context;
mod cursor;
mod dict;
mod ffi;
mod instance;
mod iterators;
mod keys;
mod live;
mod parse;
mod phrase;
mod sentence;
mod state;
mod types;

#[cfg(feature = "tools")]
pub mod tool;

// The pure-MIRROR and SHARED-CHEWING symbols are the public entry points;
// the four enum-touching symbols + zhuyin_init live where the zhuyin-local
// state is. Re-export for in-tree tooling.
pub use context::{zhuyin_fini, zhuyin_init, zhuyin_save};
pub use iterators::{zhuyin_begin_add_phrases, zhuyin_end_add_phrases, zhuyin_iterator_add_phrase};

// ── candidate / sentence re-exports for the harness ───────────────
pub use candidates::{
    zhuyin_choose_candidate, zhuyin_clear_constraint, zhuyin_get_candidate,
    zhuyin_get_candidate_string, zhuyin_get_candidate_type, zhuyin_get_n_candidate, zhuyin_train,
};
pub use config::zhuyin_load_phrase_library;
pub use config::zhuyin_mask_out;
pub use config::zhuyin_set_chewing_scheme;
pub use config::zhuyin_set_full_pinyin_scheme;
pub use config::zhuyin_set_options;
pub use config::zhuyin_unload_phrase_library;
pub use cursor::zhuyin_get_left_zhuyin_offset;
pub use cursor::zhuyin_get_right_zhuyin_offset;
pub use cursor::zhuyin_get_zhuyin_key;
pub use cursor::zhuyin_get_zhuyin_key_rest;
pub use cursor::zhuyin_get_zhuyin_key_rest_length;
pub use cursor::zhuyin_get_zhuyin_key_rest_positions;
pub use cursor::zhuyin_get_zhuyin_offset;
pub use dict::zhuyin_lookup_tokens;
pub use dict::zhuyin_token_add_unigram_frequency;
pub use dict::zhuyin_token_get_n_pronunciation;
pub use dict::zhuyin_token_get_nth_pronunciation;
pub use dict::zhuyin_token_get_phrase;
pub use dict::zhuyin_token_get_unigram_frequency;
pub use instance::zhuyin_alloc_instance;
pub use instance::zhuyin_free_instance;
pub use instance::zhuyin_reset;
pub use keys::zhuyin_get_pinyin_string;
pub use keys::zhuyin_get_zhuyin_string;
pub use keys::zhuyin_parse_chewing;
pub use keys::zhuyin_parse_full_pinyin;
pub use parse::zhuyin_get_parsed_input_length;
pub use parse::zhuyin_in_chewing_keyboard;
pub use parse::zhuyin_parse_more_chewings;
pub use parse::zhuyin_parse_more_full_pinyins;
pub use phrase::zhuyin_get_n_phrase;
pub use phrase::zhuyin_get_phrase_token;
pub use phrase::zhuyin_phrase_segment;
pub use sentence::zhuyin_get_character_offset;
pub use sentence::{
    zhuyin_get_sentence, zhuyin_guess_candidates_after_cursor,
    zhuyin_guess_candidates_before_cursor, zhuyin_guess_sentence,
    zhuyin_guess_sentence_with_prefix,
};
pub use types::ChewingKey;
pub use types::ChewingKeyRest;
pub use types::GChar;
pub use types::GUint;
pub use types::ImportIterator;
pub use types::LookupCandidate;
pub use types::PhraseTokenT;
pub use types::PinyinOptionT;
pub use types::ZhuyinContext;
pub use types::ZhuyinInstance;
pub use types::lookup_candidate_type_t;

// C publication and conversion tests live in sibling tests.rs files and tests/it.
// The shared test helpers name the exported functions through the library's
// own name, as the verification tests do.
#[cfg(test)]
extern crate self as oxzhuyin_capi;

#[cfg(test)]
mod test_support;
