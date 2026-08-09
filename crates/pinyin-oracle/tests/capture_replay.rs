//! The capture reader against the real frozen fixtures.
//!
//! Runs on the portable tier with no oracle present. If this passes, the W2-T3
//! replay producer is reading genuine pinned-oracle output rather than anything
//! synthesised for the test.

use std::path::PathBuf;

use pinyin_oracle::capture::{self, segments_to_wire};
use pinyin_oracle::{EXPECTED_PIN_REF, OracleFlags};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/foundation")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

#[test]
fn reads_every_frozen_f_a_record() {
    let observations = capture::read_fixture(&fixture("f-a.txt")).expect("F-A decodes");

    // capture-fixtures.md freezes F-A at 15 records.
    assert_eq!(observations.len(), 15);

    for observed in &observations {
        assert_eq!(observed.pin_ref, EXPECTED_PIN_REF);
        assert_eq!(observed.family.as_deref(), Some("F-A"));
        assert_eq!(observed.flags, OracleFlags::DEFAULT);
        assert!(observed.case.is_some());
        observed
            .check_self_consistent()
            .expect("frozen record is self-consistent");
        assert_eq!(
            observed.remainder.len(),
            observed.input.len() - observed.parsed_input_length,
            "remainder must start at the parsed length"
        );
    }
}

#[test]
fn reads_every_frozen_f_c_record() {
    let observations = capture::read_fixture(&fixture("f-c.txt")).expect("F-C decodes");

    // capture-fixtures.md freezes F-C at 46 records.
    assert_eq!(observations.len(), 46);

    for observed in &observations {
        assert_eq!(observed.pin_ref, EXPECTED_PIN_REF);
        assert_eq!(observed.family.as_deref(), Some("F-C"));
        // Every F-C record builds on the IS_PINYIN baseline.
        assert!(observed.flags.contains(pinyin_oracle::flags::IS_PINYIN));
        // DYNAMIC_ADJUST is excluded by the capture protocol, and OracleFlags
        // could not have been constructed if a record carried it.
        assert!(
            !observed
                .flags
                .contains(pinyin_oracle::flags::DYNAMIC_ADJUST)
        );
        observed
            .check_self_consistent()
            .expect("frozen record is self-consistent");
    }
}

#[test]
fn decoded_records_match_their_frozen_field_values() {
    let observations = capture::read_fixture(&fixture("f-a.txt")).expect("F-A decodes");
    let by_case = |name: &str| {
        observations
            .iter()
            .find(|observed| observed.case.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("case {name} is present"))
    };

    let multiple = by_case("valid-multiple");
    assert_eq!(multiple.input, b"nihao");
    assert_eq!(multiple.parse_return, 5);
    assert_eq!(multiple.parsed_input_length, 5);
    assert_eq!(
        segments_to_wire(&multiple.segments),
        "ni@0:2:complete,hao@2:5:complete"
    );
    assert_eq!(multiple.candidate_total, 126);
    assert_eq!(
        multiple.candidates.first().map(String::as_str),
        Some("你好")
    );

    let phrase = by_case("valid-phrase");
    assert_eq!(
        segments_to_wire(&phrase.segments),
        "zhong@0:5:complete,guo@5:8:complete,ren@8:11:complete"
    );
    assert_eq!(
        phrase.candidates.first().map(String::as_str),
        Some("中国人")
    );

    // The oracle's non-greedy choice, and the tie-swap example for W2-T5.
    let fangan = by_case("ambiguous-fangan");
    assert_eq!(
        segments_to_wire(&fangan.segments),
        "fan@0:3:complete,gan@3:6:complete"
    );

    // A partial tail decodes as partial, not complete.
    let nih = by_case("incomplete-nih");
    assert_eq!(
        segments_to_wire(&nih.segments),
        "ni@0:2:complete,h@2:3:partial"
    );

    // Apostrophe leaves a one-byte gap between segment ranges.
    let chang_an = by_case("apostrophe-chang-an");
    assert_eq!(
        segments_to_wire(&chang_an.segments),
        "chang@0:5:complete,an@6:8:complete"
    );

    // No segment is spelled `-`, and junk becomes the remainder.
    let junk = by_case("junk-prefix");
    assert_eq!(segments_to_wire(&junk.segments), "-");
    assert_eq!(junk.parsed_input_length, 0);
    assert_eq!(junk.remainder, b"!ni");
    assert_eq!(junk.candidate_total, 0);
    assert!(junk.candidates.is_empty());

    let empty = by_case("empty");
    assert!(empty.input.is_empty());
    assert!(empty.segments.is_empty());

    let long = by_case("very-long-junk-4096");
    assert_eq!(long.input.len(), 4_096);
    assert_eq!(long.remainder.len(), 4_096);
    assert_eq!(long.parsed_input_length, 0);
}

#[test]
fn no_frozen_record_carries_a_sentinel_segment() {
    // F-E-01's NULL key-rest shape is representable and would be reported, but
    // the pin does not exhibit it on the frozen F-A inputs. Pinning that fact
    // means a future pin bump that starts emitting sentinels shows up here.
    for family in ["f-a.txt", "f-c.txt"] {
        for observed in capture::read_fixture(&fixture(family)).expect("decodes") {
            assert!(
                !observed.has_sentinel_segment(),
                "{family} case {:?} unexpectedly carries a sentinel",
                observed.case
            );
        }
    }
}
