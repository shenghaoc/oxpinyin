//! Candidate lookups at every byte offset (#696, register row 65). Generated from the frozen pin observations.

use crate::bodies::candidate_lookup_offsets as b;
use crate::support::{Expect, NO_LOGS, run};

// guess-candidates-lookup-offset-zhuyin: MATCH.
// The zhuyin facade prepends no LONGER row (zhuyin.cpp:1460-1539).
#[test]
fn guess_candidates_lookup_offset_zhuyin() {
    run(
        "candidate_lookup_offsets::guess_candidates_lookup_offset_zhuyin",
        Expect::RESULT,
        b::lookup_offset_zhuyin,
        include!("fixtures/candidate_lookup_offsets/4935c466b81cea89.rs"),
        Some(NO_LOGS),
    );
}
