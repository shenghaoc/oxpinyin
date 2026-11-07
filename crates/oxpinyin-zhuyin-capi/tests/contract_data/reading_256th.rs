//! A 256th pronunciation on one item (#804). Generated from the frozen pin observations.

use crate::bodies::reading_256th as b;
use crate::support::{Expect, NO_LOGS, run};

// wrap-256th-reading-zhuyin: MATCH.
// The count wraps through a guint8 (phrase_index.cpp:28, :88); reads size the item from it
// (:203-204) and the next add copies only that view (:228). Fields the pin leaves indeterminate
// (an unread stack buffer, the addon libraries' unigram) are not compared.
#[test]
fn wrap_256th_reading_zhuyin() {
    run(
        "reading_256th::wrap_256th_reading_zhuyin",
        Expect::RESULT,
        b::wrap_256th,
        include!("fixtures/reading_256th/8da5111dce395be8.rs"),
        Some(NO_LOGS),
    );
}
