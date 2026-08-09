//! What the oracle reported for one input.
//!
//! These types mirror the `pinyin-capture-v1` record fields frozen in
//! `docs/findings/capture-fixtures.md`, so a live FFI run and a replayed
//! fixture line produce the same value. That equivalence is what lets W2-T3 be
//! exercised in portable CI against `fixtures/foundation/f-a.txt` while the
//! same code path drives the real oracle on Linux.
//!
//! The module is pure and always compiled; only the producer behind
//! [`crate::live`] is feature-gated.

use crate::OracleFlags;

/// Whether the oracle called a segment complete or a partial prefix.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OracleCompleteness {
    /// Reported as a complete table syllable.
    Complete,
    /// Reported as an incomplete (initial-only) prefix.
    Partial,
}

impl OracleCompleteness {
    /// The token used in the `pinyin-capture-v1` wire format.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
        }
    }
}

/// One entry of the oracle's `segments` field.
///
/// The sentinel variants are not defensive padding: `tools/capture/capture.c`
/// emits them when the oracle yields a key without a usable key-rest, position
/// pair, or pinyin string. That is the observable shape of catalogue row
/// F-E-01 (issue #566, NULL key-rest), so the harness must be able to record it
/// rather than crash or silently skip it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum OracleSegment {
    /// A syllable with a byte range and a completeness verdict.
    Syllable {
        /// Canonical syllable text as the oracle spelled it.
        syllable: String,
        /// Inclusive begin offset into the input.
        begin: u16,
        /// Exclusive end offset into the input.
        end: u16,
        /// Complete or partial.
        completeness: OracleCompleteness,
    },
    /// `pinyin_get_pinyin_key_rest` yielded no key-rest at this offset.
    MissingKeyRest {
        /// Offset at which the key-rest was absent.
        offset: usize,
    },
    /// `pinyin_get_pinyin_key_rest_positions` failed at this offset.
    MissingPosition {
        /// Offset at which positions were unavailable.
        offset: usize,
    },
    /// `pinyin_get_pinyin_string` failed at this offset.
    MissingPinyinString {
        /// Offset at which the string was unavailable.
        offset: usize,
    },
}

impl OracleSegment {
    /// Whether this entry is a sentinel rather than a usable syllable.
    #[must_use]
    pub const fn is_sentinel(&self) -> bool {
        !matches!(self, Self::Syllable { .. })
    }

    /// The syllable text, when this entry is a syllable.
    #[must_use]
    pub fn syllable(&self) -> Option<&str> {
        match self {
            Self::Syllable { syllable, .. } => Some(syllable),
            _ => None,
        }
    }

    /// The half-open byte range, when this entry is a syllable.
    #[must_use]
    pub const fn range(&self) -> Option<(u16, u16)> {
        match self {
            Self::Syllable { begin, end, .. } => Some((*begin, *end)),
            _ => None,
        }
    }
}

/// The oracle's complete response for one input under one flag word.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OracleObservation {
    /// Pin reference the observation was produced under.
    pub pin_ref: String,
    /// Fixture family, when the observation came from a capture fixture.
    pub family: Option<String>,
    /// Stable case identifier, when the observation came from a capture fixture.
    pub case: Option<String>,
    /// Exact input bytes handed to the oracle.
    pub input: Vec<u8>,
    /// Option word in force.
    pub flags: OracleFlags,
    /// Value returned by `pinyin_parse_more_full_pinyins`.
    pub parse_return: usize,
    /// Value returned by `pinyin_get_parsed_input_length`.
    pub parsed_input_length: usize,
    /// The oracle's selected segmentation, in input order.
    pub segments: Vec<OracleSegment>,
    /// Uncapped candidate count reported by `pinyin_get_n_candidate`.
    pub candidate_total: u32,
    /// The first [`MAX_CAPTURED_CANDIDATES`] candidate strings.
    pub candidates: Vec<String>,
    /// Input suffix beginning at `parsed_input_length`.
    pub remainder: Vec<u8>,
}

/// Candidate capture depth, matching the capture protocol and the W2-T1 card.
pub const MAX_CAPTURED_CANDIDATES: usize = 10;

impl OracleObservation {
    /// Whether the oracle consumed the whole input.
    #[must_use]
    pub const fn fully_consumed(&self) -> bool {
        self.remainder.is_empty()
    }

    /// Whether any segment entry is a sentinel.
    ///
    /// A true result is the F-E-01 shape and is classified as an oracle defect
    /// rather than a disagreement about parsing.
    #[must_use]
    pub fn has_sentinel_segment(&self) -> bool {
        self.segments.iter().any(OracleSegment::is_sentinel)
    }

    /// The selected segmentation as `(syllable, begin, end, completeness)`
    /// tuples, or `None` if any entry is a sentinel.
    #[must_use]
    pub fn selected_path(&self) -> Option<Vec<(&str, u16, u16, OracleCompleteness)>> {
        self.segments
            .iter()
            .map(|segment| match segment {
                OracleSegment::Syllable {
                    syllable,
                    begin,
                    end,
                    completeness,
                } => Some((syllable.as_str(), *begin, *end, *completeness)),
                _ => None,
            })
            .collect()
    }

    /// Checks the invariants the harness relies on.
    ///
    /// The oracle is a subject under test, not a trusted component, so its
    /// self-consistency is verified rather than assumed.
    ///
    /// # Errors
    ///
    /// Returns [`crate::OracleError::ParsedLengthOutOfRange`] when the reported
    /// parsed prefix exceeds the input length.
    pub fn check_self_consistent(&self) -> Result<(), crate::OracleError> {
        if self.parsed_input_length > self.input.len() {
            return Err(crate::OracleError::ParsedLengthOutOfRange {
                parsed: self.parsed_input_length,
                input_len: self.input.len(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{OracleCompleteness, OracleObservation, OracleSegment};
    use crate::{OracleError, OracleFlags};

    fn observation(segments: Vec<OracleSegment>, parsed: usize, input: &str) -> OracleObservation {
        OracleObservation {
            pin_ref: crate::EXPECTED_PIN_REF.to_owned(),
            family: None,
            case: None,
            input: input.as_bytes().to_vec(),
            flags: OracleFlags::DEFAULT,
            parse_return: parsed,
            parsed_input_length: parsed,
            segments,
            candidate_total: 0,
            candidates: Vec::new(),
            remainder: input.as_bytes()[parsed..].to_vec(),
        }
    }

    fn syllable(text: &str, begin: u16, end: u16) -> OracleSegment {
        OracleSegment::Syllable {
            syllable: text.to_owned(),
            begin,
            end,
            completeness: OracleCompleteness::Complete,
        }
    }

    #[test]
    fn selected_path_reads_back_the_segmentation() {
        let observed = observation(
            vec![syllable("ni", 0, 2), syllable("hao", 2, 5)],
            5,
            "nihao",
        );
        let path = observed.selected_path().expect("no sentinels");
        assert_eq!(path.len(), 2);
        assert_eq!(path[0].0, "ni");
        assert_eq!((path[1].1, path[1].2), (2, 5));
        assert!(observed.fully_consumed());
    }

    #[test]
    fn sentinel_segments_suppress_the_selected_path() {
        let observed = observation(
            vec![
                syllable("ni", 0, 2),
                OracleSegment::MissingKeyRest { offset: 2 },
            ],
            3,
            "nih",
        );
        assert!(observed.has_sentinel_segment());
        assert!(observed.selected_path().is_none());
    }

    #[test]
    fn parsed_length_beyond_the_input_is_reported() {
        let mut observed = observation(vec![], 0, "ni");
        observed.parsed_input_length = 99;
        assert!(matches!(
            observed.check_self_consistent(),
            Err(OracleError::ParsedLengthOutOfRange {
                parsed: 99,
                input_len: 2
            })
        ));
    }

    #[test]
    fn completeness_wire_tokens_match_the_capture_format() {
        assert_eq!(OracleCompleteness::Complete.as_wire(), "complete");
        assert_eq!(OracleCompleteness::Partial.as_wire(), "partial");
    }
}
