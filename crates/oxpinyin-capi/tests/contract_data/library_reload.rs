//! Phrase library unload and reload: dropped session changes, saved journals and reloaded totals (inventory row 55, #805). Generated from the frozen pin observations.

use crate::bodies::library_reload as b;
use crate::support::{Expect, run_j};
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
