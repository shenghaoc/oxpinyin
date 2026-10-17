//! What the oracle reported for one input.
//!
//! These types mirror the `pinyin-capture-v1` record fields frozen in
//! `docs/testing/capture-fixtures.md`, so a live FFI run and a replayed
//! fixture line produce the same value. That equivalence is what lets W2-T3 be
//! exercised in portable CI against `fixtures/foundation/f-a.txt` while the
//! same code path drives the real oracle on Linux.
//!
//! The module is pure and always compiled; only the producer behind
//! [`crate::live`] is feature-gated.

use crate::OracleFlags;

// The segment, completeness and candidate vocabulary is generic over any
// libpinyin and lives in `libpinyin-abi`; the old names are kept here so the
// capture format, the fixtures and every consumer are untouched. The pin
// reference, the fixture metadata and the validated flag word make
// `OracleObservation` below the pinned oracle's own record, so it stays.
pub use libpinyin_abi::observation::{
    CandidateInfo, CandidateType as OracleCandidateType, Completeness as OracleCompleteness,
    Segment as OracleSegment,
};

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
    pub const fn check_self_consistent(&self) -> Result<(), crate::OracleError> {
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
    use super::{OracleCandidateType, OracleCompleteness, OracleObservation, OracleSegment};
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

    #[test]
    fn candidate_type_discriminants_match_the_pinned_header() {
        // `_lookup_candidate_type_t` in include/libpinyin-2.11.92/pinyin.h,
        // discriminants 1..=8 in declared order.
        let ordered = [
            (1, OracleCandidateType::NbestMatch, "NBEST_MATCH_CANDIDATE"),
            (2, OracleCandidateType::Normal, "NORMAL_CANDIDATE"),
            (3, OracleCandidateType::Zombie, "ZOMBIE_CANDIDATE"),
            (
                4,
                OracleCandidateType::PredictedBigram,
                "PREDICTED_BIGRAM_CANDIDATE",
            ),
            (
                5,
                OracleCandidateType::PredictedPrefix,
                "PREDICTED_PREFIX_CANDIDATE",
            ),
            (6, OracleCandidateType::Addon, "ADDON_CANDIDATE"),
            (7, OracleCandidateType::Longer, "LONGER_CANDIDATE"),
            (
                8,
                OracleCandidateType::PredictedPunctuation,
                "PREDICTED_PUNCTUATION_CANDIDATE",
            ),
        ];
        for (raw, variant, wire) in ordered {
            assert_eq!(OracleCandidateType::from_raw(raw), Some(variant));
            assert_eq!(variant.as_wire(), wire);
            assert_eq!(OracleCandidateType::from_wire(wire), Some(variant));
        }
        // Values outside the declared set are rejected, not transmuted.
        assert_eq!(OracleCandidateType::from_raw(0), None);
        assert_eq!(OracleCandidateType::from_raw(9), None);
        assert_eq!(OracleCandidateType::from_raw(-1), None);
        assert_eq!(OracleCandidateType::from_wire("SOMETHING_ELSE"), None);
    }
}
