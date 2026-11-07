//! Candidate lookups at every byte offset (#696, register row 65). Generated from the frozen pin observations.

use crate::bodies::candidate_lookup_offsets as b;
use crate::support::{Expect, NO_LOGS, run};

// guess-candidates-lookup-offset: MATCH.
// Offsets 0 through the reserved slot (pinyin.cpp:2230) and one past it, where the LONGER
// row is still prepended (:2292-2293).
#[test]
fn guess_candidates_lookup_offset() {
    run(
        "candidate_lookup_offsets::guess_candidates_lookup_offset",
        Expect::RESULT,
        b::lookup_offset,
        include!("fixtures/candidate_lookup_offsets/802e28e28c821e54.rs"),
        Some(NO_LOGS),
    );
}
