//! Phrase library unload and reload: dropped session changes, saved journals and reloaded totals (inventory row 55, #805). Generated from the frozen pin observations.

use crate::bodies::library_reload as b;
use crate::support::{Expect, run_j};

// mask-out-after-compact-keeps-reload-offset: MATCH.
#[test]
fn mask_out_after_compact_keeps_reload_offset() {
    run_j(
        "library_reload::mask_out_after_compact_keeps_reload_offset",
        Expect::RESULT,
        |k| b::case(k, "mask-out-after-compact-keeps-reload-offset"),
        raw!(include!("fixtures/library_reload/a1681da18aa0ce2d.rs")),
        None,
    );
}

// mask-out-replaces-reload-offset: MATCH.
#[test]
fn mask_out_replaces_reload_offset() {
    run_j(
        "library_reload::mask_out_replaces_reload_offset",
        Expect::RESULT,
        |k| b::case(k, "mask-out-replaces-reload-offset"),
        raw!(include!("fixtures/library_reload/71d1c2bf6734437f.rs")),
        None,
    );
}

// reload-dangling-token-candidates: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn reload_dangling_token_candidates() {
    run_j(
        "library_reload::reload_dangling_token_candidates",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "reload-dangling-token-candidates"),
        raw!(include!("fixtures/library_reload/fe7fea593e105de4.rs")),
        Some(&["candidates", "readings", "tokens", "unigram"]),
    );
}

// reload-drops-session-pinyin-new-item: MATCH.
#[test]
fn reload_drops_session_pinyin_new_item() {
    run_j(
        "library_reload::reload_drops_session_pinyin_new_item",
        Expect::RESULT,
        |k| b::case(k, "reload-drops-session-pinyin-new-item"),
        raw!(include!("fixtures/library_reload/cf09b08497065324.rs")),
        None,
    );
}

// reload-drops-session-pinyin-reading: MATCH.
#[test]
fn reload_drops_session_pinyin_reading() {
    run_j(
        "library_reload::reload_drops_session_pinyin_reading",
        Expect::RESULT,
        |k| b::case(k, "reload-drops-session-pinyin-reading"),
        raw!(include!("fixtures/library_reload/f72664818e282810.rs")),
        None,
    );
}

// reload-drops-session-pinyin-train: MATCH.
#[test]
fn reload_drops_session_pinyin_train() {
    run_j(
        "library_reload::reload_drops_session_pinyin_train",
        Expect::RESULT,
        |k| b::case(k, "reload-drops-session-pinyin-train"),
        raw!(include!("fixtures/library_reload/1b7b435aca4062f6.rs")),
        None,
    );
}

// reload-keeps-saved-pinyin-new-item: MATCH.
#[test]
fn reload_keeps_saved_pinyin_new_item() {
    run_j(
        "library_reload::reload_keeps_saved_pinyin_new_item",
        Expect::RESULT,
        |k| b::case(k, "reload-keeps-saved-pinyin-new-item"),
        raw!(include!("fixtures/library_reload/d7b1b8c30839a08c.rs")),
        None,
    );
}

// reload-keeps-saved-pinyin-reading: MATCH.
#[test]
fn reload_keeps_saved_pinyin_reading() {
    run_j(
        "library_reload::reload_keeps_saved_pinyin_reading",
        Expect::RESULT,
        |k| b::case(k, "reload-keeps-saved-pinyin-reading"),
        raw!(include!("fixtures/library_reload/4a88a23e5980c875.rs")),
        None,
    );
}

// reload-keeps-saved-pinyin-train: MATCH.
#[test]
fn reload_keeps_saved_pinyin_train() {
    run_j(
        "library_reload::reload_keeps_saved_pinyin_train",
        Expect::RESULT,
        |k| b::case(k, "reload-keeps-saved-pinyin-train"),
        raw!(include!("fixtures/library_reload/7326e439105e46bb.rs")),
        None,
    );
}

// reload-persisted-empty-add-plain-pinyin: MATCH.
#[test]
fn reload_persisted_empty_add_plain_pinyin() {
    run_j(
        "library_reload::reload_persisted_empty_add_plain_pinyin",
        Expect::RESULT,
        |k| b::case(k, "reload-persisted-empty-add-plain-pinyin"),
        raw!(include!("fixtures/library_reload/c9de5e7e0b3872b9.rs")),
        None,
    );
}

// reload-persisted-empty-add-pronunciations-pinyin: MATCH.
#[test]
fn reload_persisted_empty_add_pronunciations_pinyin() {
    run_j(
        "library_reload::reload_persisted_empty_add_pronunciations_pinyin",
        Expect::RESULT,
        |k| b::case(k, "reload-persisted-empty-add-pronunciations-pinyin"),
        raw!(include!("fixtures/library_reload/f1c095584c00e549.rs")),
        None,
    );
}

// reload-persisted-empty-user-file-pinyin: MATCH.
#[test]
fn reload_persisted_empty_user_file_pinyin() {
    run_j(
        "library_reload::reload_persisted_empty_user_file_pinyin",
        Expect::RESULT,
        |k| b::case(k, "reload-persisted-empty-user-file-pinyin"),
        raw!(include!("fixtures/library_reload/7a5962e58458b7b7.rs")),
        None,
    );
}

// reload-refused-pinyin-train-library-1: MATCH.
#[test]
fn reload_refused_pinyin_train_library_1() {
    run_j(
        "library_reload::reload_refused_pinyin_train_library_1",
        Expect::RESULT,
        |k| b::case(k, "reload-refused-pinyin-train-library-1"),
        raw!(include!("fixtures/library_reload/0c5ec95ceecaf05d.rs")),
        None,
    );
}

// reload-refused-user-pinyin-item: MATCH.
#[test]
fn reload_refused_user_pinyin_item() {
    run_j(
        "library_reload::reload_refused_user_pinyin_item",
        Expect::RESULT,
        |k| b::case(k, "reload-refused-user-pinyin-item"),
        raw!(include!("fixtures/library_reload/84616d99d3429acb.rs")),
        None,
    );
}

// reload-total-while-unloaded: MATCH.
#[test]
fn reload_total_while_unloaded() {
    run_j(
        "library_reload::reload_total_while_unloaded",
        Expect::RESULT,
        |k| b::case(k, "reload-total-while-unloaded"),
        raw!(include!("fixtures/library_reload/4ac6c181ffc71a5d.rs")),
        None,
    );
}

// reload-unreadable-log-missing-pinyin: MATCH.
#[test]
fn reload_unreadable_log_missing_pinyin() {
    run_j(
        "library_reload::reload_unreadable_log_missing_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "reload-unreadable-log-missing-pinyin"),
        raw!(include!("fixtures/library_reload/7edbec5d82af99e4.rs")),
        None,
    );
}

// reload-unreadable-log-short-pinyin: MATCH.
#[test]
fn reload_unreadable_log_short_pinyin() {
    run_j(
        "library_reload::reload_unreadable_log_short_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "reload-unreadable-log-short-pinyin"),
        raw!(include!("fixtures/library_reload/7edbec5d82af99e4.rs")),
        None,
    );
}

// unload-save-skips-library-pinyin: MATCH.
#[test]
fn unload_save_skips_library_pinyin() {
    run_j(
        "library_reload::unload_save_skips_library_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "unload-save-skips-library-pinyin"),
        raw!(include!("fixtures/library_reload/718bc7920459cdd6.rs")),
        None,
    );
}
