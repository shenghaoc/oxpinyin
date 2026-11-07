//! System and user directories whose names are not UTF-8 (#587). Generated from the frozen pin observations.

use crate::bodies::non_utf8_directories as b;
use crate::support::{Expect, NO_LOGS, run};

// non-utf8-directory-zhuyin-both: MATCH.
// macOS and Windows cannot name such a directory; the pin runs only on Linux, so that arm
// is the file system's refusal, not a pin observation.
#[test]
fn non_utf8_directory_zhuyin_both() {
    run(
        "non_utf8_directories::non_utf8_directory_zhuyin_both",
        Expect::RESULT,
        |k| b::non_utf8_dirs(k, true, true),
        if cfg!(any(target_os = "macos", windows)) {
            None
        } else {
            Some(include!(
                "fixtures/non_utf8_directories/d172c050e709a1ae.rs"
            ))
        },
        Some(NO_LOGS),
    );
}
