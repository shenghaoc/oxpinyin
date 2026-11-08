//! Phrase export and training: the export cursor across imports, the live item a training changes, appended system readings and the user library's saved bytes (#759, #790, #1031). Generated from the frozen pin observations.

use crate::bodies::phrase_export_training as b;
use crate::support::{Expect, run_j};

// appended-system-second-reading-zhuyin: MATCH.
#[test]
fn user_library_training_bytes_zhuyin() {
    run_j(
        "phrase_export_training::user_library_training_bytes_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "user-library-training-bytes-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/phrase_export_training/2661f3c04c99f242.rs"
        )),
        None,
    );
}
