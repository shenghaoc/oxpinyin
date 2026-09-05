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
//! Nothing here may panic on any input: every library crate in the
//! workspace denies `clippy::unwrap_used`/`expect_used`/`panic`/
//! `panic_in_result_fn` outside tests, so the entry-point bodies are
//! panic-free by construction. Rust (since 1.81) aborts the process when
//! a panic reaches an `extern "C"` boundary, so if a bug ever produced a
//! panic the failure would be a loud abort, not undefined behaviour.
//! There is deliberately no panic-catching wrapper: with the lints green
//! one is operationally inert, and abort-at-ABI makes the outcome the
//! same under either panic strategy (the release profile is `unwind`;
//! `abort` was tried and reverted for its keystroke-cycle cost, see
//! docs/perf/perf-baseline-kc-2026-09.md).
//!
//! Opaque handles cross as `*mut T` via `Box::into_raw` / `Box::from_raw`;
//! every incoming pointer is null-checked. `// SAFETY:` documents each
//! `unsafe` block.
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
    use std::ffi::CString;
    use std::path::PathBuf;
    use std::ptr;

    use super::candidates::{
        zhuyin_choose_candidate, zhuyin_get_candidate, zhuyin_get_n_candidate,
    };
    use super::context::{zhuyin_fini, zhuyin_init};
    use super::instance::{zhuyin_alloc_instance, zhuyin_free_instance};
    use super::parse::zhuyin_parse_more_chewings;
    use super::sentence::zhuyin_guess_candidates_before_cursor;
    use super::state::instance_mut;
    use super::types::{LookupCandidate, ZhuyinContext, ZhuyinInstance, lookup_candidate_type_t};

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

    /// The committed mini fixture (`fixtures/w3/<backend ext>`), the same
    /// data directory the pinyin crate's e2e tests open.
    fn system_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("w3")
            .join(oxpinyin_data::DEFAULT_STORE_EXT)
    }

    fn cstr(value: &str) -> CString {
        CString::new(value).expect("no interior NUL")
    }

    /// Opens the fixture context with no user directory (the corpus
    /// driver's shape) and one instance on it.
    fn open() -> (*mut ZhuyinContext, *mut ZhuyinInstance) {
        let system = cstr(system_dir().to_str().expect("UTF-8 path"));
        let user = cstr("");
        let context = zhuyin_init(system.as_ptr(), user.as_ptr());
        assert!(!context.is_null(), "the mini fixture must open");
        let instance = zhuyin_alloc_instance(context);
        assert!(!instance.is_null());
        (context, instance)
    }

    /// The text a snapshot row carries, by row index.
    fn candidate_text(instance: *mut ZhuyinInstance, index: usize) -> String {
        // SAFETY: `instance` is live and was produced by
        // `zhuyin_alloc_instance`; the borrow ends with this function.
        let inst = unsafe { instance_mut(instance) };
        inst.candidates[index]
            .text
            .to_str()
            .expect("candidate text is UTF-8")
            .to_owned()
    }

    /// The zhuyin twin of the pinyin crate's
    /// `choosing_from_a_reanchored_window_uses_the_anchored_span`
    /// (`oxpinyin-capi/src/e2e_tests.rs`): a
    /// `zhuyin_guess_candidates_before_cursor` window is re-anchored, so
    /// the following `zhuyin_choose_candidate` must resolve the row's
    /// index against THAT window.
    ///
    /// `su3cl3` is `ni3'hao3` in the session's `'`-joined buffer, and the
    /// two lists provably differ at row 1: the composition-anchored cached
    /// list offers 你 (the first key's span), the `before(6)` window offers
    /// 好 (the second key's span, ending at the cursor). Resolving row 1
    /// through the cached list committed 你 and answered cursor 3 — a row
    /// the caller never displayed.
    #[test]
    fn choosing_from_a_before_cursor_window_uses_that_window() {
        let (context, instance) = open();
        let input = cstr("su3cl3");
        assert_eq!(
            zhuyin_parse_more_chewings(instance, input.as_ptr()),
            "su3cl3".len(),
            "the whole keystroke run parses"
        );

        assert!(zhuyin_guess_candidates_before_cursor(instance, 6));
        let mut count = 0;
        assert!(zhuyin_get_n_candidate(instance, &raw mut count));
        assert!(count > 1, "the before-cursor window carries several rows");

        // Row 0 is the prepended BEST_MATCH sentence row (你好); row 1 is
        // the first phrase row of the spans ending at the cursor.
        let displayed = candidate_text(instance, 1);
        assert_eq!(displayed, "好", "the fixture's before(6) row 1");
        let mut cand: *mut LookupCandidate = ptr::null_mut();
        assert!(zhuyin_get_candidate(instance, 1, &raw mut cand));
        assert!(!cand.is_null());

        // The chosen span ENDS at the cursor, so the composition advances
        // to it (6), not to the first key's end (3) the cached list's row 1
        // would have given.
        assert_eq!(zhuyin_choose_candidate(instance, 6, cand), 6);

        // commit() resets the session, so it is read last: the committed
        // text is the row the caller was shown.
        let committed = {
            // SAFETY: the instance is live; commit takes &mut and resets.
            unsafe {
                instance_mut(instance)
                    .core
                    .session
                    .commit()
                    .expect("commit")
            }
        };
        assert_eq!(
            committed, displayed,
            "the committed text is the displayed row, not a cached-list row"
        );
        zhuyin_free_instance(instance);
        zhuyin_fini(context);
    }
}
