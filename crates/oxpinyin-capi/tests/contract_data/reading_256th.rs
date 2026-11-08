//! A 256th pronunciation on one item (#804). Generated from the frozen pin observations.

use crate::bodies::reading_256th as b;
use crate::support::{Expect, NO_LOGS, run};

// wrap-256th-reading: MATCH.
// The count wraps through a guint8 (phrase_index.cpp:28, :88); reads size the item from it
// (:203-204) and the next add copies only that view (:228). Fields the pin leaves indeterminate
// (an unread stack buffer, the addon libraries' unigram) are not compared.
#[test]
fn wrap_256th_reading() {
    run(
        "reading_256th::wrap_256th_reading",
        Expect::RESULT,
        b::wrap_256th,
        include!("fixtures/reading_256th/4315ddbe26c74809.rs"),
        Some(NO_LOGS),
    );
}
