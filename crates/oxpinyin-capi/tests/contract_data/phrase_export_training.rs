//! Phrase export and training: the export cursor across imports, the live item a training changes, appended system readings and the user library's saved bytes (#759, #790, #1031). Generated from the frozen pin observations.

use crate::bodies::phrase_export_training as b;
use crate::support::{Expect, run_j};

// appended-system-second-reading-pinyin: MATCH.
#[test]
fn appended_system_second_reading_pinyin() {
    run_j(
        "phrase_export_training::appended_system_second_reading_pinyin",
        Expect::RESULT,
        |k| b::case(k, "appended-system-second-reading-pinyin"),
        raw!(include!(
            "fixtures/phrase_export_training/36a215a342bb695b.rs"
        )),
        None,
    );
}

// issue759-export-open-across-import-system: MATCH.
#[test]
fn issue759_export_open_across_import_system() {
    run_j(
        "phrase_export_training::issue759_export_open_across_import_system",
        Expect::RESULT,
        |k| b::case(k, "issue759-export-open-across-import-system"),
        raw!(include!(
            "fixtures/phrase_export_training/eec5a9cac3dadf4e.rs"
        )),
        None,
    );
}

// issue759-export-open-across-import-user: MATCH.
#[test]
fn issue759_export_open_across_import_user() {
    run_j(
        "phrase_export_training::issue759_export_open_across_import_user",
        Expect::RESULT,
        |k| b::case(k, "issue759-export-open-across-import-user"),
        raw!(include!(
            "fixtures/phrase_export_training/29ac4d7701f68053.rs"
        )),
        None,
    );
}

// issue790-character-offset-after-train-system: MATCH.
#[test]
fn issue790_character_offset_after_train_system() {
    run_j(
        "phrase_export_training::issue790_character_offset_after_train_system",
        Expect::RESULT,
        |k| b::case(k, "issue790-character-offset-after-train-system"),
        raw!(include!(
            "fixtures/phrase_export_training/bbd51e49bfc19ffb.rs"
        )),
        None,
    );
}

// issue790-export-after-train-system: MATCH.
#[test]
fn issue790_export_after_train_system() {
    run_j(
        "phrase_export_training::issue790_export_after_train_system",
        Expect::RESULT,
        |k| b::case(k, "issue790-export-after-train-system"),
        raw!(include!(
            "fixtures/phrase_export_training/837781d51e558254.rs"
        )),
        None,
    );
}
// issue790-export-open-before-system: MATCH.
#[test]
fn issue790_export_open_before_system() {
    run_j(
        "phrase_export_training::issue790_export_open_before_system",
        Expect::RESULT,
        |k| b::case(k, "issue790-export-open-before-system"),
        raw!(include!(
            "fixtures/phrase_export_training/837781d51e558254.rs"
        )),
        None,
    );
}
// issue790-export-prefix-overflow-system: MATCH.
#[test]
fn issue790_export_prefix_overflow_system() {
    run_j(
        "phrase_export_training::issue790_export_prefix_overflow_system",
        Expect::RESULT,
        |k| b::case(k, "issue790-export-prefix-overflow-system"),
        raw!(include!(
            "fixtures/phrase_export_training/317a047656405368.rs"
        )),
        None,
    );
}
// issue790-export-zero-and-signed-system: MATCH.
#[test]
fn issue790_export_zero_and_signed_system() {
    run_j(
        "phrase_export_training::issue790_export_zero_and_signed_system",
        Expect::RESULT,
        |k| b::case(k, "issue790-export-zero-and-signed-system"),
        raw!(include!(
            "fixtures/phrase_export_training/65a80ad5e0249a2d.rs"
        )),
        None,
    );
}

// issue790-guess-after-train-system: MATCH.
#[test]
fn issue790_guess_after_train_system() {
    run_j(
        "phrase_export_training::issue790_guess_after_train_system",
        Expect::RESULT,
        |k| b::case(k, "issue790-guess-after-train-system"),
        raw!(include!(
            "fixtures/phrase_export_training/62e215f887f48e2e.rs"
        )),
        None,
    );
}

// user-library-training-bytes-pinyin: MATCH.
#[test]
fn user_library_training_bytes_pinyin() {
    run_j(
        "phrase_export_training::user_library_training_bytes_pinyin",
        Expect::RESULT,
        |k| b::case(k, "user-library-training-bytes-pinyin"),
        raw!(include!(
            "fixtures/phrase_export_training/2661f3c04c99f242.rs"
        )),
        None,
    );
}
