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

/// The public `lookup_candidate_type_t` a candidate was reported under.
///
/// These are the eight values the pinned header
/// (`include/libpinyin-2.11.91/pinyin.h`) declares for
/// `_lookup_candidate_type_t`, in its declared order (discriminants 1..=8). The
/// enum exists so the W2-CAND capture records the frozen public **name** rather
/// than a bare integer, and so an out-of-range value from the oracle is a
/// reported error ([`crate::OracleError::UnknownCandidateType`]) instead of a
/// silent transmute. See `docs/findings/candidate-construction.md` §1.6.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum OracleCandidateType {
    /// `NBEST_MATCH_CANDIDATE` — a sentence-decode (n-best) candidate.
    NbestMatch,
    /// `NORMAL_CANDIDATE` — a phrase-lookup candidate.
    Normal,
    /// `ZOMBIE_CANDIDATE`.
    Zombie,
    /// `PREDICTED_BIGRAM_CANDIDATE`.
    PredictedBigram,
    /// `PREDICTED_PREFIX_CANDIDATE`.
    PredictedPrefix,
    /// `ADDON_CANDIDATE`.
    Addon,
    /// `LONGER_CANDIDATE`.
    Longer,
    /// `PREDICTED_PUNCTUATION_CANDIDATE`.
    PredictedPunctuation,
}

impl OracleCandidateType {
    /// Maps the C enum discriminant to a variant, or `None` if it is not one of
    /// the eight the pinned header declares.
    #[must_use]
    pub const fn from_raw(value: i32) -> Option<Self> {
        match value {
            1 => Some(Self::NbestMatch),
            2 => Some(Self::Normal),
            3 => Some(Self::Zombie),
            4 => Some(Self::PredictedBigram),
            5 => Some(Self::PredictedPrefix),
            6 => Some(Self::Addon),
            7 => Some(Self::Longer),
            8 => Some(Self::PredictedPunctuation),
            _ => None,
        }
    }

    /// The frozen public enum **name**, as written to the fixture's `type`
    /// column.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::NbestMatch => "NBEST_MATCH_CANDIDATE",
            Self::Normal => "NORMAL_CANDIDATE",
            Self::Zombie => "ZOMBIE_CANDIDATE",
            Self::PredictedBigram => "PREDICTED_BIGRAM_CANDIDATE",
            Self::PredictedPrefix => "PREDICTED_PREFIX_CANDIDATE",
            Self::Addon => "ADDON_CANDIDATE",
            Self::Longer => "LONGER_CANDIDATE",
            Self::PredictedPunctuation => "PREDICTED_PUNCTUATION_CANDIDATE",
        }
    }

    /// Parses a frozen public enum name back to a variant, for fixture readers.
    #[must_use]
    pub fn from_wire(text: &str) -> Option<Self> {
        Some(match text {
            "NBEST_MATCH_CANDIDATE" => Self::NbestMatch,
            "NORMAL_CANDIDATE" => Self::Normal,
            "ZOMBIE_CANDIDATE" => Self::Zombie,
            "PREDICTED_BIGRAM_CANDIDATE" => Self::PredictedBigram,
            "PREDICTED_PREFIX_CANDIDATE" => Self::PredictedPrefix,
            "ADDON_CANDIDATE" => Self::Addon,
            "LONGER_CANDIDATE" => Self::Longer,
            "PREDICTED_PUNCTUATION_CANDIDATE" => Self::PredictedPunctuation,
            _ => return None,
        })
    }
}

/// One candidate as W2-CAND captures it: the string plus the two fields the
/// pinned public API exports.
///
/// `nbest_index` is `Some` only for [`OracleCandidateType::NbestMatch`]
/// candidates: `pinyin_get_candidate_nbest_index` asserts that type internally
/// and aborts on any other, so it is called only for n-best candidates and is
/// `None` (written `-`) for every other type. It is never synthesised. There is
/// deliberately no `begin`/`end`/`tokens` field: `lookup_candidate_t` is opaque
/// and the public header exposes no accessor for them
/// (`docs/findings/candidate-construction.md` §1.6).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateInfo {
    /// The candidate's UTF-8 string, identical to the sister fixture's `text`.
    pub text: String,
    /// The candidate's public type.
    pub candidate_type: OracleCandidateType,
    /// The candidate's n-best index, or `None` when the accessor reports none.
    pub nbest_index: Option<u8>,
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
    /// The oracle reported a non-empty parsed prefix that cannot hold a key, so
    /// the key walk was skipped to avoid a known abort.
    ///
    /// See `docs/testing/oracle-apostrophe-abort.md`. On the pinned oracle an
    /// apostrophe-only input reports `parsed_input_length > 0` while its key
    /// matrix has no columns, and any key query then trips an `assert()` that
    /// reaches `abort()`. Recording this sentinel keeps the input visible as a
    /// divergence instead of silently smoothing the defect away.
    NoKeyColumns {
        /// Parsed prefix length the oracle reported.
        parsed_len: usize,
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
        // `_lookup_candidate_type_t` in include/libpinyin-2.11.91/pinyin.h,
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
