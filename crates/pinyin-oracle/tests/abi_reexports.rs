//! The moved items are the same types under their old names.
//!
//! `libpinyin-abi` now defines the segment, candidate and option vocabulary and
//! the capture notation; `pinyin-oracle` re-exports each item under the name it
//! has always had. These checks fail to compile, not merely to pass, if a
//! re-export ever becomes a distinct type, which is what would break a
//! downstream consumer that mixes the two crates.

use libpinyin_abi::capture as abi_capture;
use libpinyin_abi::observation as abi;
use pinyin_oracle::capture;
use pinyin_oracle::flags;
use pinyin_oracle::{OracleCandidateType, OracleCompleteness, OracleSegment};

#[test]
fn vocabulary_is_one_type_with_two_names() {
    let segment: OracleSegment = abi::Segment::MissingKeyRest { offset: 3 };
    let back: abi::Segment = segment.clone();
    assert_eq!(segment, back);

    let kind: OracleCandidateType = abi::CandidateType::Normal;
    assert_eq!(kind.as_wire(), "NORMAL_CANDIDATE");

    let done: OracleCompleteness = abi::Completeness::Complete;
    assert_eq!(done.as_wire(), "complete");

    let info: pinyin_oracle::observation::CandidateInfo = abi::CandidateInfo {
        text: "你".to_owned(),
        candidate_type: kind,
        nbest_index: None,
    };
    assert_eq!(info.text, "你");
}

#[test]
fn option_bits_are_the_same_constants() {
    assert_eq!(flags::IS_PINYIN, libpinyin_abi::option::IS_PINYIN);
    assert_eq!(flags::DYNAMIC_ADJUST, libpinyin_abi::option::DYNAMIC_ADJUST);
    assert_eq!(
        flags::SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY,
        libpinyin_abi::option::SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY
    );
    // The capture profiles are this crate's policy and stay defined here.
    assert_eq!(flags::CAPTURE_DEFAULT, 0x18a);
}

#[test]
fn capture_notation_is_shared_and_the_errors_keep_their_shape() {
    assert_eq!(capture::CAPTURE_SCHEMA, abi_capture::CAPTURE_SCHEMA);
    assert_eq!(capture::escape(b"a\tb"), abi_capture::escape(b"a\tb"));

    // A record's raw fields read through either crate agree.
    let line = "schema=pinyin-capture-v1\tinput=ni";
    let record = capture::CaptureRecord::parse_line(line);
    let raw = abi_capture::RawRecord::parse_line(line);
    assert_eq!(record.field("input"), raw.field("input"));

    // A malformed field still surfaces as the harness's own error variant with
    // the field name and raw value intact.
    let bad = concat!(
        "schema=pinyin-capture-v1\tpin_ref=x\tinput=ni\tflags=0x0000018a\t",
        "parse_return=2\tparsed_input_length=2\tsegments=ni@0:2:weird\t",
        "candidate_total=0\tcandidates_hex=-\tremainder="
    );
    match capture::read_fixture(bad) {
        Err(pinyin_oracle::OracleError::CaptureRecordInvalid { line, source }) => {
            assert_eq!(line, 1);
            match *source {
                pinyin_oracle::OracleError::CaptureFieldMalformed { field, value } => {
                    assert_eq!(field, "segments");
                    assert_eq!(value, "ni@0:2:weird");
                }
                other => panic!("unexpected inner error {other:?}"),
            }
        }
        other => panic!("unexpected result {other:?}"),
    }
}
