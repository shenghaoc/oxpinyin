//! Remembered toned readings and the zhuyin facade's missing API. Generated from the frozen pin observations.

use crate::bodies::remember_tone::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// remember-api-absent-zhuyin: MATCH.
// libzhuyin.ver exports neither zhuyin_remember_user_input nor zhuyin_begin_get_phrases.
#[test]
fn remember_api_absent_zhuyin() {
    run(
        "remember_tone::remember_api_absent_zhuyin",
        Expect::RESULT,
        b::api_absent,
        Apis {
            remember_api: false,
            export_api: false,
        },
        Some(NO_LOGS),
    );
}
