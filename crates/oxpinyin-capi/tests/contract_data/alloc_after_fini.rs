//! `pinyin_alloc_instance` on a finalised context (#528). Generated from the frozen pin observations.

use crate::bodies::alloc_after_fini::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// alloc-instance-after-fini: MATCH.
// The pin reads the freed context (pinyin.cpp:1321-1322); its second answer is indeterminate.
#[test]
fn alloc_instance_after_fini() {
    run(
        "alloc_after_fini::alloc_instance_after_fini",
        Expect::RESULT,
        b::alloc_after_fini,
        AfterFini {
            live_instance: true,
        },
        Some(NO_LOGS),
    );
}
