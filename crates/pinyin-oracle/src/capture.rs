//! Reader and writer for the `pinyin-capture-v1` line format.
//!
//! Format frozen in `docs/findings/capture-fixtures.md`. This module turns a
//! frozen fixture line back into an [`OracleObservation`], which is what lets
//! the W2-T3 comparison run in portable CI with no oracle installed: the oracle
//! side is real output the pinned program already produced, not a mock.
//!
//! Pure and always compiled. The escaping helpers are shared with the
//! differential logs, so fixtures and logs cannot drift apart.

use crate::observation::{OracleCompleteness, OracleObservation, OracleSegment};
use crate::{OracleError, OracleFlags};

/// Schema tag of the capture format.
pub const CAPTURE_SCHEMA: &str = "pinyin-capture-v1";

/// Escapes a byte string for a single log or fixture field.
///
/// `\\`, TAB, CR and LF get C-style escapes. Control bytes, DEL and every byte
/// at or above `0x80` become `\xNN`. Bytes `0x20..=0x7e` pass through, so ASCII
/// text stays readable.
///
/// Escaping the high half is deliberately *stricter* than
/// `tools/capture/capture.c`, which passes bytes above DEL through raw. Two
/// reasons: the output must be a Rust `String`, so a raw high byte would be
/// re-encoded as two UTF-8 bytes and break the round trip [`unescape`] depends
/// on; and a pure-ASCII log line diffs cleanly. The two rules agree on every
/// byte either format actually carries, since capture inputs and the parity
/// corpus are both ASCII. [`unescape`] still accepts a raw high byte, so this
/// reader remains tolerant of anything the C harness could emit.
#[must_use]
pub fn escape(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            0x00..=0x1f | 0x7f..=0xff => out.push_str(&format!("\\x{byte:02x}")),
            _ => out.push(char::from(*byte)),
        }
    }
    out
}

/// Reverses [`escape`].
///
/// A trailing lone backslash or a malformed `\xNN` is preserved literally rather
/// than treated as fatal: a fixture the reader cannot fully decode must still be
/// reportable, not silently dropped.
#[must_use]
pub fn unescape(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'\\' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        match bytes.get(index + 1) {
            Some(b'\\') => {
                out.push(b'\\');
                index += 2;
            }
            Some(b't') => {
                out.push(b'\t');
                index += 2;
            }
            Some(b'n') => {
                out.push(b'\n');
                index += 2;
            }
            Some(b'r') => {
                out.push(b'\r');
                index += 2;
            }
            Some(b'x') => {
                let hex = bytes.get(index + 2..index + 4).and_then(|pair| {
                    let text = core::str::from_utf8(pair).ok()?;
                    u8::from_str_radix(text, 16).ok()
                });
                if let Some(value) = hex {
                    out.push(value);
                    index += 4;
                } else {
                    out.push(b'\\');
                    index += 1;
                }
            }
            _ => {
                out.push(b'\\');
                index += 1;
            }
        }
    }
    out
}

/// One `pinyin-capture-v1` record, as raw fields.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CaptureRecord {
    fields: Vec<(String, String)>,
}

impl CaptureRecord {
    /// Splits one line into TAB-separated `key=value` fields.
    #[must_use]
    pub fn parse_line(line: &str) -> Self {
        let fields = line
            .trim_end_matches(['\r', '\n'])
            .split('\t')
            .filter_map(|field| {
                let (key, value) = field.split_once('=')?;
                Some((key.to_owned(), value.to_owned()))
            })
            .collect();
        Self { fields }
    }

    /// Returns the raw, still-escaped value of `field`.
    #[must_use]
    pub fn field(&self, field: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(key, _)| key == field)
            .map(|(_, value)| value.as_str())
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

/// Parses the `segments` field.
///
/// `-` means no segment. Otherwise entries are comma-separated
/// `text@begin:end:complete|partial`, or one of the sentinel spellings the
/// capture harness emits for the F-E-01 shape.
fn parse_segments(raw: &str) -> Result<Vec<OracleSegment>, OracleError> {
    if raw == "-" {
        return Ok(Vec::new());
    }

    let mut segments = Vec::new();
    for entry in raw.split(',') {
        segments.push(parse_segment(entry)?);
    }
    Ok(segments)
}

fn parse_segment(entry: &str) -> Result<OracleSegment, OracleError> {
    let malformed = || OracleError::CaptureFieldMalformed {
        field: "segments",
        value: entry.to_owned(),
    };

    for (prefix, build) in [
        (
            "<missing-rest>@",
            (|offset| OracleSegment::MissingKeyRest { offset }) as fn(usize) -> OracleSegment,
        ),
        ("<missing-position>@", |offset| {
            OracleSegment::MissingPosition { offset }
        }),
        ("<missing-pinyin>@", |offset| {
            OracleSegment::MissingPinyinString { offset }
        }),
        ("<no-key-columns>@", |parsed_len| {
            OracleSegment::NoKeyColumns { parsed_len }
        }),
    ] {
        if let Some(rest) = entry.strip_prefix(prefix) {
            let offset = rest.parse::<usize>().map_err(|_| malformed())?;
            return Ok(build(offset));
        }
    }

    // text@begin:end:completeness — split from the right, because a syllable
    // spelling never contains ':' but this keeps the parse unambiguous.
    let (text, positions) = entry.rsplit_once('@').ok_or_else(malformed)?;
    let mut parts = positions.split(':');
    let begin = parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(malformed)?;
    let end = parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(malformed)?;
    let completeness = match parts.next() {
        Some("complete") => OracleCompleteness::Complete,
        Some("partial") => OracleCompleteness::Partial,
        _ => return Err(malformed()),
    };
    if parts.next().is_some() {
        return Err(malformed());
    }

    Ok(OracleSegment::Syllable {
        syllable: String::from_utf8_lossy(&unescape(text)).into_owned(),
        begin,
        end,
        completeness,
    })
}

/// Parses the `candidates_hex` field: `-`, or comma-separated lowercase hex.
fn parse_candidates(raw: &str) -> Result<Vec<String>, OracleError> {
    if raw == "-" {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    for entry in raw.split(',') {
        let malformed = || OracleError::CaptureFieldMalformed {
            field: "candidates_hex",
            value: entry.to_owned(),
        };
        if entry.len() % 2 != 0 {
            return Err(malformed());
        }
        let mut bytes = Vec::with_capacity(entry.len() / 2);
        for pair in entry.as_bytes().chunks(2) {
            let text = core::str::from_utf8(pair).map_err(|_| malformed())?;
            bytes.push(u8::from_str_radix(text, 16).map_err(|_| malformed())?);
        }
        candidates.push(String::from_utf8(bytes).map_err(|_| malformed())?);
    }
    Ok(candidates)
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

/// Renders one segment in capture notation.
#[must_use]
pub fn segment_to_wire(segment: &OracleSegment) -> String {
    match segment {
        OracleSegment::Syllable {
            syllable,
            begin,
            end,
            completeness,
        } => format!(
            "{}@{begin}:{end}:{}",
            escape(syllable.as_bytes()),
            completeness.as_wire()
        ),
        OracleSegment::MissingKeyRest { offset } => format!("<missing-rest>@{offset}"),
        OracleSegment::MissingPosition { offset } => format!("<missing-position>@{offset}"),
        OracleSegment::MissingPinyinString { offset } => format!("<missing-pinyin>@{offset}"),
        OracleSegment::NoKeyColumns { parsed_len } => format!("<no-key-columns>@{parsed_len}"),
    }
}

/// Renders a segment list in capture notation, using `-` when empty.
#[must_use]
pub fn segments_to_wire(segments: &[OracleSegment]) -> String {
    if segments.is_empty() {
        return "-".to_owned();
    }
    segments
        .iter()
        .map(segment_to_wire)
        .collect::<Vec<_>>()
        .join(",")
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
