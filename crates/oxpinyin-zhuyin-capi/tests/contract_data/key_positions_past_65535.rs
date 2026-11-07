//! Key positions past byte 65535 (#1501). Generated from the frozen pin observations.

use crate::bodies::key_positions_past_65535::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// key-position-past-65535/chewing-zhuyin-crossing-key: MATCH.
#[test]
fn chewing_zhuyin_crossing_key() {
    run(
        "key_positions_past_65535::chewing_zhuyin_crossing_key",
        Expect::RESULT,
        b::crossing,
        Crossing {
            parsed: (65541, 65541),
            rest_65535: RestRow {
                ret: true,
                detail: Some((Some("2b00".into()), true, 65535, 1, true, 2)),
            },
        },
        Some(NO_LOGS),
    );
}

// key-position-past-65535/chewing-zhuyin-past-key: POLICY-ACCEPTED.
// Policy: docs/findings/1501-guint16-key-positions.md. The pin's guint16 key rest wraps: at 65537 it
// answers false (phonetic_key_matrix.cpp:51-55); oxpinyin answers the real key.
#[test]
fn chewing_zhuyin_past_key() {
    run(
        "key_positions_past_65535::chewing_zhuyin_past_key",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::past,
        Past {
            rest_65537: RestRow {
                ret: true,
                detail: Some((Some("2b00".into()), true, 1, 3, true, 2)),
            },
        },
        Some(NO_LOGS),
    );
}
