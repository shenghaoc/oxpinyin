//! C ABI of libpinyin's zhuyin facade — the 52-symbol `libzhuyin.so.15`
//! surface, the `--enable-libzhuyin` counterpart to `libpinyin.so.15`.
//!
//! Upstream builds this as a SEPARATE shared object from `$(pinyin_SOURCES)
//! zhuyin.cpp` with its own version script (`src/Makefile.am:108-125`,
//! `configure.ac:138-144` at 0c5e80e1 — 2.11.91 — unchanged at the 074a2219 pin): `libzhuyin.so.15`, not
//! additional symbols in `libpinyin.so.15`. This crate mirrors that cut for
//! the Rust world: a new workspace member producing `libzhuyin.so.15`, with
//! no change to `oxpinyin-capi` (which keeps building `libpinyin.so.15`).
//!
//! ## Export boundary
//!
//! The authoritative export list is the checked-in `libzhuyin.ver` (copied
//! verbatim from upstream): 52 `zhuyin_*` symbols. `zhuyin_get_raw_user_input`
//! appears in `zhuyin.h` inside `#if 0` and is NOT in the `.ver` — it is not
//! exported.
//!
//! A Rust `cdylib` cannot apply a named version script at link time (rustc
//! merges its own anonymous script, and GNU ld rejects the pair — the same
//! reason `oxpinyin-capi`'s `build.rs` enforces scope in source, not by a
//! linker script). So the boundary is enforced by source construction: every
//! symbol in the `.ver` is `#[unsafe(no_mangle)] pub extern "C"`, and nothing
//! else is. The built `libzhuyin.so.15` is verified to export exactly the 52.
//! The `.ver` ships verbatim as the record and for the packaging step.
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
//! ## Safety
//!
//! Every entry point is an `unsafe extern "C" fn`: its caller upholds the
//! `zhuyin.h` contract. Each function's `# Safety` section names the rules
//! below that bind its arguments, and adds its own requirements.
//!
//! - **Handles.** A context pointer is NULL or a pointer that `zhuyin_init`
//!   returned and `zhuyin_fini` has not taken. `zhuyin_fini` and
//!   `zhuyin_alloc_instance` also accept a pointer that `zhuyin_fini` took:
//!   they look it up in the registry of live contexts and refuse it. An
//!   instance pointer is NULL or a pointer that `zhuyin_alloc_instance`
//!   returned and `zhuyin_free_instance` has not freed. An iterator pointer
//!   is NULL or a pointer that `zhuyin_begin_add_phrases` returned and
//!   `zhuyin_end_add_phrases` has not freed.
//! - **Strings.** A `const char *` argument is NULL or points to a
//!   NUL-terminated string that stays valid and unchanged for the call.
//! - **Out-parameters.** A pointer the function writes through is NULL or is
//!   valid and aligned for a write of its pointee type.
//! - **Borrowed objects.** A candidate pointer is NULL or a row that
//!   `zhuyin_get_candidate` answered, from a candidate list that is still
//!   the current list of a live instance: the next call that rebuilds or
//!   clears that instance's list, and its free, end the row. A key or
//!   key-rest pointer is NULL or points to a `ChewingKey` or
//!   `ChewingKeyRest` that is valid for reads.
//! - **Serial calls.** Calls that use one context, its instances or its
//!   iterators do not overlap: the caller serializes them.
//!
//! Opaque handles cross as `*mut T` via `Box::into_raw` / `Box::from_raw`;
//! every incoming pointer is null-checked. `// SAFETY:` documents each
//! `unsafe` block.
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
#[cfg(test)]
mod test_support;
