//! Reader and writer for the `pinyin-capture-v1` line format.
//!
//! Format frozen in `docs/testing/capture-fixtures.md`. This module turns a
//! frozen fixture line back into an [`OracleObservation`], which is what lets
//! the W2-T3 comparison run in portable CI with no oracle installed: the oracle
//! side is real output the pinned program already produced, not a mock.
//!
//! The escaping rules and the notation of the individual fields are generic and
//! live in [`libpinyin_abi::capture`]; they are re-exported here under their
//! old names. What stays is the part that is this harness's policy: which
//! fields a record must carry, that an off-pin record is *not* rejected at the
//! door, and the [`OracleError`] each failure maps to.
//!
//! Pure and always compiled. The escaping helpers are shared with the
//! differential logs, so fixtures and logs cannot drift apart.

use libpinyin_abi::capture::{RawRecord, WireError};

pub use libpinyin_abi::capture::{
    CAPTURE_SCHEMA, escape, segment_to_wire, segments_to_wire, unescape,
};

use crate::observation::{OracleObservation, OracleSegment};
use crate::{OracleError, OracleFlags};

/// One `pinyin-capture-v1` record, as raw fields.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CaptureRecord {
    raw: RawRecord,
}

impl CaptureRecord {
    /// Splits one line into TAB-separated `key=value` fields.
    #[must_use]
    pub fn parse_line(line: &str) -> Self {
        Self {
            raw: RawRecord::parse_line(line),
        }
    }

    /// Returns the raw, still-escaped value of `field`.
    #[must_use]
    pub fn field(&self, field: &str) -> Option<&str> {
        self.raw.field(field)
    }

    fn require(&self, field: &'static str) -> Result<&str, OracleError> {
        self.field(field)
            .ok_or(OracleError::CaptureFieldMissing { field })
    }

    fn require_number(&self, field: &'static str) -> Result<usize, OracleError> {
        let raw = self.require(field)?;
        raw.parse::<usize>()
            .map_err(|_| OracleError::CaptureFieldMalformed {
                field,
                value: raw.to_owned(),
            })
    }

    /// Converts the record into an observation.
    ///
    /// # Errors
    ///
    /// Returns an error when the schema tag is unknown, a required field is
    /// absent, or a field cannot be decoded. The pin ref is *not* validated
    /// here: an off-pin record must reach the comparison so it can be logged as
    /// a divergence rather than rejected at the door.
    pub fn to_observation(&self) -> Result<OracleObservation, OracleError> {
        let schema = self.require("schema")?;
        if schema != CAPTURE_SCHEMA {
            return Err(OracleError::CaptureFieldMalformed {
                field: "schema",
                value: schema.to_owned(),
            });
        }

        let flags_raw = self.require("flags")?;
        let flags_bits = flags_raw
            .strip_prefix("0x")
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .ok_or_else(|| OracleError::CaptureFieldMalformed {
                field: "flags",
                value: flags_raw.to_owned(),
            })?;

        let input = unescape(self.require("input")?);
        let parsed_input_length = self.require_number("parsed_input_length")?;

        Ok(OracleObservation {
            pin_ref: String::from_utf8_lossy(&unescape(self.require("pin_ref")?)).into_owned(),
            family: self
                .field("family")
                .map(|raw| String::from_utf8_lossy(&unescape(raw)).into_owned()),
            case: self
                .field("case")
                .map(|raw| String::from_utf8_lossy(&unescape(raw)).into_owned()),
            input,
            flags: OracleFlags::new(flags_bits)?,
            parse_return: self.require_number("parse_return")?,
            parsed_input_length,
            segments: parse_segments(self.require("segments")?)?,
            candidate_total: self
                .require("candidate_total")?
                .parse::<u32>()
                .map_err(|_| OracleError::CaptureFieldMalformed {
                    field: "candidate_total",
                    value: self.field("candidate_total").unwrap_or("").to_owned(),
                })?,
            candidates: parse_candidates(self.require("candidates_hex")?)?,
            remainder: unescape(self.require("remainder")?),
        })
    }
}

/// Parses the `segments` field (notation owned by [`libpinyin_abi::capture`]).
fn parse_segments(raw: &str) -> Result<Vec<OracleSegment>, OracleError> {
    libpinyin_abi::capture::parse_segments(raw).map_err(wire_error)
}

/// Parses the `candidates_hex` field.
fn parse_candidates(raw: &str) -> Result<Vec<String>, OracleError> {
    libpinyin_abi::capture::parse_candidates(raw).map_err(wire_error)
}

/// Maps a generic field failure to the harness's error, field and value intact.
fn wire_error(error: WireError) -> OracleError {
    OracleError::CaptureFieldMalformed {
        field: error.field,
        value: error.value,
    }
}

/// Reads every record from a capture fixture's contents.
///
/// Blank lines are skipped. A malformed record is reported with its 1-based line
/// number rather than silently ignored.
///
/// # Errors
///
/// Returns the first decoding failure encountered.
pub fn read_fixture(contents: &str) -> Result<Vec<OracleObservation>, OracleError> {
    let mut observations = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let record = CaptureRecord::parse_line(line);
        let observation =
            record
                .to_observation()
                .map_err(|error| OracleError::CaptureRecordInvalid {
                    line: index + 1,
                    source: Box::new(error),
                })?;
        observations.push(observation);
    }
    Ok(observations)
}

#[cfg(test)]
mod tests {
    use super::{escape, read_fixture, segments_to_wire, unescape};
    use crate::observation::{OracleCompleteness, OracleSegment};

    #[test]
    fn escaping_round_trips_every_byte() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(unescape(&escape(&all)), all);
    }

    #[test]
    fn escaping_uses_the_frozen_spellings() {
        assert_eq!(escape(b"\\"), "\\\\");
        assert_eq!(escape(b"\t"), "\\t");
        assert_eq!(escape(b"\n"), "\\n");
        assert_eq!(escape(b"\r"), "\\r");
        assert_eq!(escape(&[0x00]), "\\x00");
        assert_eq!(escape(&[0x7f]), "\\x7f");
        assert_eq!(escape(b"ni'hao!"), "ni'hao!");
    }

    #[test]
    fn malformed_escapes_are_preserved_not_fatal() {
        assert_eq!(unescape("\\"), b"\\");
        assert_eq!(unescape("\\xZZ"), b"\\xZZ");
        assert_eq!(unescape("\\q"), b"\\q");
    }

    #[test]
    fn reads_a_frozen_style_record() {
        let line = concat!(
            "schema=pinyin-capture-v1\tpin_ref=libpinyin-x+model20-y+dbm-tkrzw\t",
            "family=F-A\tcase=valid-multiple\tapi_sequence=pinyin_init\t",
            "input=nihao\tflags=0x0000018a\tparse_return=5\tparsed_input_length=5\t",
            "segments=ni@0:2:complete,hao@2:5:complete\tcandidate_total=126\t",
            "candidates_hex=e4bda0e5a5bd,e4bda0\tremainder="
        );
        let observations = read_fixture(line).expect("record decodes");
        assert_eq!(observations.len(), 1);

        let observed = &observations[0];
        assert_eq!(observed.input, b"nihao");
        assert_eq!(observed.flags.bits(), 0x0000_018a);
        assert_eq!(observed.parsed_input_length, 5);
        assert_eq!(observed.candidate_total, 126);
        assert_eq!(observed.candidates, ["你好", "你"]);
        assert_eq!(observed.family.as_deref(), Some("F-A"));
        assert_eq!(observed.case.as_deref(), Some("valid-multiple"));
        assert!(observed.remainder.is_empty());
        assert_eq!(
            observed.segments,
            vec![
                OracleSegment::Syllable {
                    syllable: "ni".to_owned(),
                    begin: 0,
                    end: 2,
                    completeness: OracleCompleteness::Complete,
                },
                OracleSegment::Syllable {
                    syllable: "hao".to_owned(),
                    begin: 2,
                    end: 5,
                    completeness: OracleCompleteness::Complete,
                },
            ]
        );
    }

    #[test]
    fn no_segment_marker_and_sentinels_decode() {
        assert_eq!(segments_to_wire(&[]), "-");

        let sentinels = vec![
            OracleSegment::MissingKeyRest { offset: 2 },
            OracleSegment::MissingPosition { offset: 3 },
            OracleSegment::MissingPinyinString { offset: 4 },
        ];
        let wire = segments_to_wire(&sentinels);
        assert_eq!(
            wire,
            "<missing-rest>@2,<missing-position>@3,<missing-pinyin>@4"
        );
    }

    #[test]
    fn segment_wire_round_trips() {
        let segments = vec![
            OracleSegment::Syllable {
                syllable: "zhong".to_owned(),
                begin: 0,
                end: 5,
                completeness: OracleCompleteness::Complete,
            },
            OracleSegment::Syllable {
                syllable: "g".to_owned(),
                begin: 5,
                end: 6,
                completeness: OracleCompleteness::Partial,
            },
            OracleSegment::MissingKeyRest { offset: 6 },
        ];
        let wire = segments_to_wire(&segments);
        assert_eq!(wire, "zhong@0:5:complete,g@5:6:partial,<missing-rest>@6");
        assert_eq!(super::parse_segments(&wire).expect("round trips"), segments);
    }

    #[test]
    fn unknown_schema_is_rejected() {
        let line = "schema=pinyin-capture-v2\tinput=ni";
        assert!(read_fixture(line).is_err());
    }

    #[test]
    fn malformed_records_report_their_line_number() {
        let contents = "schema=pinyin-capture-v1\tinput=ni\n";
        let error = read_fixture(contents).expect_err("missing fields");
        assert!(
            format!("{error}").contains("line 1"),
            "error should name the line: {error}"
        );
    }

    #[test]
    fn arbitrary_text_never_panics() {
        for text in [
            "",
            "\n\n",
            "=",
            "schema=",
            "\t\t\t",
            "segments=@@@",
            "segments=ni@0:2:weird",
            "candidates_hex=zz",
            "flags=nothex",
        ] {
            let _ = read_fixture(text);
        }
    }
}
