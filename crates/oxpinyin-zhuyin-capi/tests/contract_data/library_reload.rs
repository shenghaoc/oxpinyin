//! Phrase library unload and reload: dropped session changes, saved journals and reloaded totals (inventory row 55, #805). Generated from the frozen pin observations.

use crate::bodies::library_reload as b;
use crate::support::{Expect, run_aborting, run_j};

// reload-dangling-token-candidates-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn reload_dangling_token_candidates_zhuyin() {
    run_j(
        "library_reload::reload_dangling_token_candidates_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "reload-dangling-token-candidates-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/library_reload/fe7fea593e105de4.rs"
        )),
        Some(&["candidates", "readings", "tokens", "unigram"]),
    );
}

// reload-drops-session-zhuyin-new-item: MATCH.
#[test]
fn reload_drops_session_zhuyin_new_item() {
    run_j(
        "library_reload::reload_drops_session_zhuyin_new_item",
        Expect::RESULT,
        |k| b::case(k, "reload-drops-session-zhuyin-new-item"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/library_reload/cf09b08497065324.rs"
        )),
        None,
    );
}

// reload-drops-session-zhuyin-reading: MATCH.
#[test]
fn reload_drops_session_zhuyin_reading() {
    run_j(
        "library_reload::reload_drops_session_zhuyin_reading",
        Expect::RESULT,
        |k| b::case(k, "reload-drops-session-zhuyin-reading"),
        raw!(include!("fixtures/library_reload/6c3b714c54078a9b.rs")),
        None,
    );
}

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

// reload-keeps-saved-zhuyin-new-item: MATCH.
#[test]
fn reload_keeps_saved_zhuyin_new_item() {
    run_j(
        "library_reload::reload_keeps_saved_zhuyin_new_item",
        Expect::RESULT,
        |k| b::case(k, "reload-keeps-saved-zhuyin-new-item"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/library_reload/d7b1b8c30839a08c.rs"
        )),
        None,
    );
}

// reload-keeps-saved-zhuyin-reading: MATCH.
#[test]
fn reload_keeps_saved_zhuyin_reading() {
    run_j(
        "library_reload::reload_keeps_saved_zhuyin_reading",
        Expect::RESULT,
        |k| b::case(k, "reload-keeps-saved-zhuyin-reading"),
        raw!(include!("fixtures/library_reload/a8371aa78e13dc3e.rs")),
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

// reload-persisted-empty-add-plain-zhuyin: MATCH.
#[test]
fn reload_persisted_empty_add_plain_zhuyin() {
    run_j(
        "library_reload::reload_persisted_empty_add_plain_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "reload-persisted-empty-add-plain-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/library_reload/c9de5e7e0b3872b9.rs"
        )),
        None,
    );
}

// reload-persisted-empty-add-pronunciations-zhuyin: MATCH.
#[test]
fn reload_persisted_empty_add_pronunciations_zhuyin() {
    run_j(
        "library_reload::reload_persisted_empty_add_pronunciations_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "reload-persisted-empty-add-pronunciations-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/library_reload/f1c095584c00e549.rs"
        )),
        None,
    );
}

// reload-persisted-empty-user-file-zhuyin: MATCH.
#[test]
fn reload_persisted_empty_user_file_zhuyin() {
    run_j(
        "library_reload::reload_persisted_empty_user_file_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "reload-persisted-empty-user-file-zhuyin"),
        raw!(include!("fixtures/library_reload/3b670f0ae53b023b.rs")),
        None,
    );
}

// reload-refused-zhuyin-train-library-1: MATCH.
#[test]
fn reload_refused_zhuyin_train_library_1() {
    run_j(
        "library_reload::reload_refused_zhuyin_train_library_1",
        Expect::RESULT,
        |k| b::case(k, "reload-refused-zhuyin-train-library-1"),
        raw!(include!("fixtures/library_reload/f60f828bb514fad3.rs")),
        None,
    );
}

// reload-unreadable-log-missing-zhuyin: MATCH.
#[test]
fn reload_unreadable_log_missing_zhuyin() {
    run_j(
        "library_reload::reload_unreadable_log_missing_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "reload-unreadable-log-missing-zhuyin"),
        raw!(include!("fixtures/library_reload/9db8f117f3032ce1.rs")),
        None,
    );
}

// reload-user-zhuyin-item: MATCH.
#[test]
fn reload_user_zhuyin_item() {
    run_j(
        "library_reload::reload_user_zhuyin_item",
        Expect::RESULT,
        |k| b::case(k, "reload-user-zhuyin-item"),
        raw!(include!("fixtures/library_reload/e04c3bd80416adda.rs")),
        None,
    );
}

// reload-user-zhuyin-item-saved: MATCH.
#[test]
fn reload_user_zhuyin_item_saved() {
    run_j(
        "library_reload::reload_user_zhuyin_item_saved",
        Expect::RESULT,
        |k| b::case(k, "reload-user-zhuyin-item-saved"),
        raw!(include!("fixtures/library_reload/6fa2a6f65626d4d0.rs")),
        None,
    );
}

// reload-user-zhuyin-reading: MATCH.
#[test]
fn reload_user_zhuyin_reading() {
    run_j(
        "library_reload::reload_user_zhuyin_reading",
        Expect::RESULT,
        |k| b::case(k, "reload-user-zhuyin-reading"),
        raw!(include!("fixtures/library_reload/9a48a6f06478abde.rs")),
        None,
    );
}

// reload-user-zhuyin-reading-saved: MATCH.
#[test]
fn reload_user_zhuyin_reading_saved() {
    run_j(
        "library_reload::reload_user_zhuyin_reading_saved",
        Expect::RESULT,
        |k| b::case(k, "reload-user-zhuyin-reading-saved"),
        raw!(include!("fixtures/library_reload/7498a214a83b1099.rs")),
        None,
    );
}

// reload-user-zhuyin-reuse-other: MATCH.
#[test]
fn reload_user_zhuyin_reuse_other() {
    run_j(
        "library_reload::reload_user_zhuyin_reuse_other",
        Expect::RESULT,
        |k| b::case(k, "reload-user-zhuyin-reuse-other"),
        raw!(include!("fixtures/library_reload/13a406950fe459d6.rs")),
        None,
    );
}

// reload-user-zhuyin-reuse-other-mask: MATCH.
#[test]
fn reload_user_zhuyin_reuse_other_mask() {
    run_j(
        "library_reload::reload_user_zhuyin_reuse_other_mask",
        Expect::RESULT,
        |k| b::case(k, "reload-user-zhuyin-reuse-other-mask"),
        raw!(include!("fixtures/library_reload/5d696db9191a5eb0.rs")),
        None,
    );
}

// reload-user-zhuyin-reuse-other-save-reopen: MATCH.
#[test]
fn reload_user_zhuyin_reuse_other_save_reopen() {
    run_j(
        "library_reload::reload_user_zhuyin_reuse_other_save_reopen",
        Expect::RESULT,
        |k| b::case(k, "reload-user-zhuyin-reuse-other-save-reopen"),
        raw!(include!("fixtures/library_reload/cd3ca6153bbdd47b.rs")),
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

// unload-save-skips-user-library-zhuyin: MATCH.
#[test]
fn unload_save_skips_user_library_zhuyin() {
    run_j(
        "library_reload::unload_save_skips_user_library_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "unload-save-skips-user-library-zhuyin"),
        raw!(include!("fixtures/library_reload/081a723612fccb4a.rs")),
        None,
    );
}
