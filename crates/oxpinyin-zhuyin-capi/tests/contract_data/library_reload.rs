//! Phrase library unload and reload: dropped session changes, saved journals and reloaded totals (inventory row 55, #805). Generated from the frozen pin observations.

use crate::bodies::library_reload as b;
use crate::support::{Expect, run_aborting, run_j};
// reload-drops-session-zhuyin-train: MATCH.
#[test]
fn reload_drops_session_zhuyin_train() {
    run_j(
        "library_reload::reload_drops_session_zhuyin_train",
        Expect::RESULT,
        |k| b::case(k, "reload-drops-session-zhuyin-train"),
        raw!(include!("fixtures/library_reload/841b484cbf1d9efd.rs")),
        None,
    );
}
// reload-keeps-saved-zhuyin-train: MATCH.
#[test]
fn reload_keeps_saved_zhuyin_train() {
    run_j(
        "library_reload::reload_keeps_saved_zhuyin_train",
        Expect::RESULT,
        |k| b::case(k, "reload-keeps-saved-zhuyin-train"),
        raw!(include!("fixtures/library_reload/5ca863e1397f2c68.rs")),
        None,
    );
}
// reload-user-zhuyin-reuse-same: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn reload_user_zhuyin_reuse_same() {
    run_aborting(
        "library_reload::reload_user_zhuyin_reuse_same",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "reload-user-zhuyin-reuse-same");
        },
    );
}

// unload-save-skips-library-zhuyin: MATCH.
#[test]
fn unload_save_skips_library_zhuyin() {
    run_j(
        "library_reload::unload_save_skips_library_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "unload-save-skips-library-zhuyin"),
        raw!(include!("fixtures/library_reload/f3ed7aa8121e96ef.rs")),
        None,
    );
}
