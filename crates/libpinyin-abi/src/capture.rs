//! Field notation of the `pinyin-capture-v1` line format and its escaping.
//!
//! The format is frozen in `docs/testing/capture-fixtures.md`. This module owns
//! the parts that do not depend on what a record *means*: the byte escaping, the
//! TAB-separated `key=value` splitting, and the notation of the `segments` and
//! `candidates_hex` fields. Turning a record into an observation, and deciding
//! which fields it must carry, stays with the consumer (`pinyin-oracle` builds
//! its `OracleObservation` from these pieces), because that decision is part of
//! its verification policy.
//!
//! Pure and always compiled; the escaping helpers are shared with the
//! differential logs, so fixtures and logs cannot drift apart.

use core::fmt;
use std::fmt::Write as _;

use crate::observation::{Completeness, Segment};

/// Schema tag of the capture format.
pub const CAPTURE_SCHEMA: &str = "pinyin-capture-v1";

/// A field whose text could not be decoded.
///
/// Carries exactly the field name and the offending raw text so a consumer can
/// report it in its own error type without losing either.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WireError {
    /// Field name that failed to decode.
    pub field: &'static str,
    /// Raw value as it appeared in the record.
    pub value: String,
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "capture field {} is malformed: {:?}",
            self.field, self.value
        )
    }
}

impl std::error::Error for WireError {}

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
            0x00..=0x1f | 0x7f..=0xff => {
                let _ = write!(out, "\\x{byte:02x}");
            }
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

/// One `pinyin-capture-v1` record, as raw, still-escaped fields.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RawRecord {
    fields: Vec<(String, String)>,
}

impl RawRecord {
    /// Splits one line into TAB-separated `key=value` fields.
    ///
    /// A field without `=` is dropped, so a record can only lose information
    /// the reader would not have understood; absence is reported by the
    /// consumer when it requires the field.
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
}

/// Parses the `segments` field.
///
/// `-` means no segment. Otherwise entries are comma-separated
/// `text@begin:end:complete|partial`, or one of the sentinel spellings the
/// capture harness emits.
///
/// # Errors
///
/// Returns a [`WireError`] naming `segments` and the first entry that does not
/// decode.
pub fn parse_segments(raw: &str) -> Result<Vec<Segment>, WireError> {
    if raw == "-" {
        return Ok(Vec::new());
    }

    let mut segments = Vec::new();
    for entry in raw.split(',') {
        segments.push(parse_segment(entry)?);
    }
    Ok(segments)
}

/// Parses one `segments` entry.
///
/// # Errors
///
/// Returns a [`WireError`] naming `segments` and `entry` when it does not
/// decode.
pub fn parse_segment(entry: &str) -> Result<Segment, WireError> {
    let malformed = || WireError {
        field: "segments",
        value: entry.to_owned(),
    };

    for (prefix, build) in [
        (
            "<missing-rest>@",
            (|offset| Segment::MissingKeyRest { offset }) as fn(usize) -> Segment,
        ),
        ("<missing-position>@", |offset| Segment::MissingPosition {
            offset,
        }),
        ("<missing-pinyin>@", |offset| Segment::MissingPinyinString {
            offset,
        }),
        ("<no-key-columns>@", |parsed_len| Segment::NoKeyColumns {
            parsed_len,
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
        Some("complete") => Completeness::Complete,
        Some("partial") => Completeness::Partial,
        _ => return Err(malformed()),
    };
    if parts.next().is_some() {
        return Err(malformed());
    }

    Ok(Segment::Syllable {
        syllable: String::from_utf8_lossy(&unescape(text)).into_owned(),
        begin,
        end,
        completeness,
    })
}

/// Parses the `candidates_hex` field: `-`, or comma-separated lowercase hex.
///
/// # Errors
///
/// Returns a [`WireError`] naming `candidates_hex` and the first entry that is
/// not valid hex-encoded UTF-8.
pub fn parse_candidates(raw: &str) -> Result<Vec<String>, WireError> {
    if raw == "-" {
        return Ok(Vec::new());
    }

    let mut candidates = Vec::new();
    for entry in raw.split(',') {
        let malformed = || WireError {
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

/// Renders one segment in capture notation.
#[must_use]
pub fn segment_to_wire(segment: &Segment) -> String {
    match segment {
        Segment::Syllable {
            syllable,
            begin,
            end,
            completeness,
        } => format!(
            "{}@{begin}:{end}:{}",
            escape(syllable.as_bytes()),
            completeness.as_wire()
        ),
        Segment::MissingKeyRest { offset } => format!("<missing-rest>@{offset}"),
        Segment::MissingPosition { offset } => format!("<missing-position>@{offset}"),
        Segment::MissingPinyinString { offset } => format!("<missing-pinyin>@{offset}"),
        Segment::NoKeyColumns { parsed_len } => format!("<no-key-columns>@{parsed_len}"),
    }
}

/// Renders a segment list in capture notation, using `-` when empty.
#[must_use]
pub fn segments_to_wire(segments: &[Segment]) -> String {
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
    use super::{
        RawRecord, WireError, escape, parse_candidates, parse_segment, parse_segments,
        segments_to_wire, unescape,
    };
    use crate::observation::{Completeness, Segment};

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
        // A raw high byte from the C harness is tolerated as-is.
        assert_eq!(unescape("é"), "é".as_bytes());
    }

    #[test]
    fn raw_record_splits_tab_separated_fields() {
        let record = RawRecord::parse_line("a=1\tb=two=2\tnoequals\tc=\r\n");
        assert_eq!(record.field("a"), Some("1"));
        // Only the first '=' separates key from value.
        assert_eq!(record.field("b"), Some("two=2"));
        assert_eq!(record.field("noequals"), None);
        assert_eq!(record.field("c"), Some(""));
        assert_eq!(record.field("missing"), None);
        assert_eq!(RawRecord::parse_line(""), RawRecord::default());
    }

    #[test]
    fn segment_wire_round_trips() {
        let segments = vec![
            Segment::Syllable {
                syllable: "zhong".to_owned(),
                begin: 0,
                end: 5,
                completeness: Completeness::Complete,
            },
            Segment::Syllable {
                syllable: "g".to_owned(),
                begin: 5,
                end: 6,
                completeness: Completeness::Partial,
            },
            Segment::MissingKeyRest { offset: 6 },
            Segment::MissingPosition { offset: 7 },
            Segment::MissingPinyinString { offset: 8 },
            Segment::NoKeyColumns { parsed_len: 9 },
        ];
        let wire = segments_to_wire(&segments);
        assert_eq!(
            wire,
            concat!(
                "zhong@0:5:complete,g@5:6:partial,<missing-rest>@6,",
                "<missing-position>@7,<missing-pinyin>@8,<no-key-columns>@9"
            )
        );
        assert_eq!(parse_segments(&wire), Ok(segments));
        assert_eq!(segments_to_wire(&[]), "-");
        assert_eq!(parse_segments("-"), Ok(Vec::new()));
    }

    #[test]
    fn malformed_segments_name_the_entry() {
        for entry in [
            "ni",
            "ni@0:2",
            "ni@0:2:weird",
            "ni@0:2:complete:x",
            "ni@a:2:complete",
            "ni@0:70000:complete",
            "<missing-rest>@x",
        ] {
            assert_eq!(
                parse_segment(entry),
                Err(WireError {
                    field: "segments",
                    value: entry.to_owned()
                }),
                "{entry}"
            );
        }
    }

    #[test]
    fn candidates_decode_hex_utf8() {
        assert_eq!(parse_candidates("-"), Ok(Vec::new()));
        assert_eq!(
            parse_candidates("e4bda0e5a5bd,e4bda0"),
            Ok(vec!["你好".to_owned(), "你".to_owned()])
        );
        for bad in ["zz", "abc", "ff"] {
            assert_eq!(
                parse_candidates(bad),
                Err(WireError {
                    field: "candidates_hex",
                    value: bad.to_owned()
                })
            );
        }
    }

    #[test]
    fn arbitrary_text_never_panics() {
        for text in [
            "",
            "=",
            "@@@",
            ",",
            ",,",
            "-,",
            "\\",
            "ni@0:2:",
            "<no-key-columns>@",
        ] {
            let _ = parse_segments(text);
            let _ = parse_candidates(text);
            let _ = RawRecord::parse_line(text);
            let _ = unescape(text);
        }
    }
}
