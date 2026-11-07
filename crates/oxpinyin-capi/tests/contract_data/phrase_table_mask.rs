//! The user phrase table's token arrays (#759, #785, #799, #1418). Generated from the frozen pin observations.

use crate::bodies::phrase_table_mask as b;
use crate::support::{Expect, NO_LOGS, run};

// phrase-table-mask-long-short: MATCH.
// A mask matching no token, the scratch entry reused unshrunk (phrase_large_table3.cpp:102-109).
#[test]
fn phrase_table_mask_long_short() {
    run(
        "phrase_table_mask::phrase_table_mask_long_short",
        Expect::RESULT,
        |k| b::mask_records(k, false, false),
        include!("fixtures/phrase_table_mask/276c6f82e9d3fb6d.rs"),
        Some(NO_LOGS),
    );
}

// phrase-table-mask-duplicate-long-short: MATCH.
// Kept: the pin stores token arrays, so a duplicate token is kept (#759, #785).
#[test]
fn phrase_table_mask_duplicate_long_short() {
    run(
        "phrase_table_mask::phrase_table_mask_duplicate_long_short",
        Expect::RESULT,
        |k| b::mask_records(k, true, false),
        include!("fixtures/phrase_table_mask/b6c4e1e475d1bf1a.rs"),
        Some(NO_LOGS),
    );
}

// phrase-table-mask-post-removal-shrink: MATCH.
// A mask matching a token shrinks the entry (phrase_large_table3.cpp:111-116).
#[test]
fn phrase_table_mask_post_removal_shrink() {
    run(
        "phrase_table_mask::phrase_table_mask_post_removal_shrink",
        Expect::RESULT,
        |k| b::mask_records(k, false, true),
        include!("fixtures/phrase_table_mask/6a4e368f5fda0083.rs"),
        Some(NO_LOGS),
    );
}

// phrase-table-add-search-remove-roundtrip: MATCH.
#[test]
fn phrase_table_add_search_remove_roundtrip() {
    run(
        "phrase_table_mask::phrase_table_add_search_remove_roundtrip",
        Expect::RESULT,
        b::roundtrip,
        include!("fixtures/phrase_table_mask/97030de6671fcfe8.rs"),
        Some(NO_LOGS),
    );
}

// phrase-table-never-loaded-library-filter: MATCH.
#[test]
fn phrase_table_never_loaded_library_filter() {
    run(
        "phrase_table_mask::phrase_table_never_loaded_library_filter",
        Expect::RESULT,
        b::never_loaded,
        include!("fixtures/phrase_table_mask/7fa1a24ecb5bf049.rs"),
        Some(NO_LOGS),
    );
}

// phrase-table-predict-nonunicode-raw-key: MATCH.
// The suggestion walk compares key bytes only; the row is the live item.
#[test]
fn phrase_table_predict_nonunicode_raw_key() {
    run(
        "phrase_table_mask::phrase_table_predict_nonunicode_raw_key",
        Expect::RESULT,
        b::raw_key_prediction,
        include!("fixtures/phrase_table_mask/894bef5d256694f1.rs"),
        Some(NO_LOGS),
    );
}

// phrase-table-saved-index-pin-format: MATCH.
#[test]
fn phrase_table_saved_index_pin_format() {
    run(
        "phrase_table_mask::phrase_table_saved_index_pin_format",
        Expect::RESULT,
        b::saved_index_format,
        include!("fixtures/phrase_table_mask/7d7438b9858531f9.rs"),
        Some(NO_LOGS),
    );
}
