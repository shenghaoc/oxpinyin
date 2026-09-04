//! C ABI of libpinyin's zhuyin facade — the 52-symbol `libzhuyin.so.15`
//! surface, the `--enable-libzhuyin` counterpart to `libpinyin.so.15`.
//!
//! Upstream builds this as a SEPARATE shared object from `$(pinyin_SOURCES)
//! zhuyin.cpp` with its own version script (`src/Makefile.am:108-125`,
//! `configure.ac:138-144` at the pin 0c5e80e1): `libzhuyin.so.15`, not
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
//! Every entry point wraps its body in [`ffi::ffi_catch`] (the
//! [`std::panic::catch_unwind`] boundary), returning the sentinel value
//! (false / null / 0) on panic. Opaque handles cross as `*mut T` via
//! `Box::into_raw` / `Box::from_raw`; every incoming pointer is null-checked.
//! `// SAFETY:` documents each `unsafe` block.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![allow(unsafe_code)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]
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
mod parse;
mod phrase;
mod sentence;
mod state;
mod types;

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
pub use sentence::{
    zhuyin_get_sentence, zhuyin_guess_candidates_after_cursor,
    zhuyin_guess_candidates_before_cursor, zhuyin_guess_sentence,
    zhuyin_guess_sentence_with_prefix,
};

#[cfg(test)]
mod tests {
    use super::types::lookup_candidate_type_t;

    /// The Phase-1 correction, pinned: the zhuyin 4-value enum's exact
    /// discriminants. The zhuyin header (`zhuyin.h:41-45`) defines four
    /// enumerators, and they collide with the pinyin eight at 3 and 4 — so
    /// the enum must never be aliased to the pinyin one.
    #[test]
    fn zhuyin_candidate_type_discriminants_match_zhuyin_h() {
        assert_eq!(lookup_candidate_type_t::BEST_MATCH_CANDIDATE as i32, 1);
        assert_eq!(
            lookup_candidate_type_t::NORMAL_CANDIDATE_AFTER_CURSOR as i32,
            2
        );
        assert_eq!(
            lookup_candidate_type_t::NORMAL_CANDIDATE_BEFORE_CURSOR as i32,
            3
        );
        assert_eq!(lookup_candidate_type_t::ZOMBIE_CANDIDATE as i32, 4);
    }

    /// The zhuyin init seed is `USE_TONE | FORCE_TONE` (the pin's
    /// `zhuyin.cpp:273`), unlike `pinyin_init`'s `PINYIN_INCOMPLETE`. The
    /// constant is the wire value the context stores.
    #[test]
    fn zhuyin_default_options_is_use_tone_or_force_tone() {
        assert_ne!(
            super::state::ZHUYIN_DEFAULT_OPTIONS & oxpinyin_core::USE_TONE,
            0
        );
        assert_ne!(
            super::state::ZHUYIN_DEFAULT_OPTIONS & oxpinyin_core::FORCE_TONE,
            0
        );
        // No ZHUYIN_INCOMPLETE is seeded.
        assert_eq!(
            super::state::ZHUYIN_DEFAULT_OPTIONS & oxpinyin_core::PINYIN_INCOMPLETE,
            0
        );
    }

    /// The packed `ChewingKey` word is 2 bytes and the `ChewingKeyRest`
    /// span is 4 bytes, matching upstream's `_ChewingKey`/`_ChewingKeyRest`
    /// (`chewing_key.h`).
    #[test]
    fn opaque_handles_layout() {
        assert_eq!(std::mem::size_of::<super::types::ChewingKey>(), 2);
        assert_eq!(std::mem::size_of::<super::types::ChewingKeyRest>(), 4);
    }
}
