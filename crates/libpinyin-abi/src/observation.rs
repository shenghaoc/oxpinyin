//! The vocabulary one parse of the public libpinyin ABI reports: segments,
//! candidates and their types.
//!
//! These are the types `pinyin-oracle` historically named `OracleSegment`,
//! `OracleCompleteness`, `OracleCandidateType` and `CandidateInfo`; it
//! re-exports them under those names, so the capture format, the fixtures and
//! every existing consumer keep working. Nothing here refers to a pin: the
//! wire tokens are the `pinyin-capture-v1` spellings and the candidate names
//! are the public header's.

/// Whether a segment was reported as a complete syllable or a partial prefix.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Completeness {
    /// Reported as a complete table syllable.
    Complete,
    /// Reported as an incomplete (initial-only) prefix.
    Partial,
}

impl Completeness {
    /// The token used in the `pinyin-capture-v1` wire format.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
        }
    }
}

/// The `lookup_candidate_type_t` of **libpinyin's** `pinyin.h`.
///
/// Eight values, discriminants 1..=8 in declared order. libzhuyin declares a
/// different enumeration of the same name (four values, with different
/// meanings for the same integers), so this type must not be used to read a
/// `zhuyin_get_candidate_type` result; `from_raw` returning `None` is how a
/// revision whose enumeration differs stays visible instead of being
/// transmuted. See `docs/findings/candidate-construction.md` §1.6.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CandidateType {
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

impl CandidateType {
    /// Maps the C enum discriminant to a variant, or `None` if it is not one of
    /// the eight libpinyin declares.
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

    /// The public enum **name**, as written to a fixture's `type` column.
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

    /// Parses a public enum name back to a variant, for fixture readers.
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

/// One candidate: the string plus the two fields the public API exports.
///
/// `nbest_index` is `Some` only for [`CandidateType::NbestMatch`] candidates:
/// `pinyin_get_candidate_nbest_index` asserts that type internally and aborts
/// on any other, so it is read only for n-best candidates and is `None`
/// (written `-`) for every other type. It is never synthesised. There is
/// deliberately no `begin`/`end`/`tokens` field: `lookup_candidate_t` is opaque
/// and the public header exposes no accessor for them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateInfo {
    /// The candidate's UTF-8 string.
    pub text: String,
    /// The candidate's public type.
    pub candidate_type: CandidateType,
    /// The candidate's n-best index, or `None` when the accessor reports none.
    pub nbest_index: Option<u8>,
}

/// One entry of a parse's `segments` field.
///
/// The sentinel variants are not defensive padding: the capture harness emits
/// them when a library yields a key without a usable key-rest, position pair,
/// or pinyin string, or reports a parsed prefix that cannot hold a key. A
/// consumer must be able to record those shapes rather than crash or silently
/// skip them.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Segment {
    /// A syllable with a byte range and a completeness verdict.
    Syllable {
        /// Canonical syllable text as the library spelled it.
        syllable: String,
        /// Inclusive begin offset into the input.
        begin: u16,
        /// Exclusive end offset into the input.
        end: u16,
        /// Complete or partial.
        completeness: Completeness,
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
    /// The library reported a non-empty parsed prefix that cannot hold a key,
    /// so the key walk was skipped to avoid a known abort in some revisions
    /// (an apostrophe-only input on 2.11.92).
    NoKeyColumns {
        /// Parsed prefix length the library reported.
        parsed_len: usize,
    },
}

impl Segment {
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

/// The `lookup_candidate_type_t` of **libzhuyin's** `zhuyin.h`.
///
/// Four values, discriminants 1..=4 in declared order. It shares a name with
/// libpinyin's enumeration but not its meaning (libpinyin's `2` is
/// `NORMAL_CANDIDATE`, libzhuyin's is `NORMAL_CANDIDATE_AFTER_CURSOR`), which is
/// why the two are separate types.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ZhuyinCandidateType {
    /// `BEST_MATCH_CANDIDATE`.
    BestMatch,
    /// `NORMAL_CANDIDATE_AFTER_CURSOR`.
    NormalAfterCursor,
    /// `NORMAL_CANDIDATE_BEFORE_CURSOR`.
    NormalBeforeCursor,
    /// `ZOMBIE_CANDIDATE`.
    Zombie,
}

impl ZhuyinCandidateType {
    /// Maps the C enum discriminant to a variant, or `None` if it is not one of
    /// the four libzhuyin declares.
    #[must_use]
    pub const fn from_raw(value: i32) -> Option<Self> {
        match value {
            1 => Some(Self::BestMatch),
            2 => Some(Self::NormalAfterCursor),
            3 => Some(Self::NormalBeforeCursor),
            4 => Some(Self::Zombie),
            _ => None,
        }
    }

    /// The public enum **name**.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::BestMatch => "BEST_MATCH_CANDIDATE",
            Self::NormalAfterCursor => "NORMAL_CANDIDATE_AFTER_CURSOR",
            Self::NormalBeforeCursor => "NORMAL_CANDIDATE_BEFORE_CURSOR",
            Self::Zombie => "ZOMBIE_CANDIDATE",
        }
    }
}

/// What one input produced on a loaded subject, with the candidate record of
/// the facade that produced it.
///
/// The candidate list is capped by the caller; `candidate_total` is the
/// library's own uncapped count. When nothing was parsed no candidates are
/// listed (some revisions abort on a query against an empty parse).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation<C> {
    /// Exact input bytes handed to the library.
    pub input: Vec<u8>,
    /// Value returned by the parse call.
    pub parse_return: usize,
    /// Value returned by `*_get_parsed_input_length`.
    pub parsed_input_length: usize,
    /// The best sentence, when the sentence guess succeeded.
    pub sentence: Option<String>,
    /// Uncapped candidate count the library reported.
    pub candidate_total: u32,
    /// The first candidates, in the library's order.
    pub candidates: Vec<C>,
}

#[cfg(test)]
mod tests {
    use super::{CandidateType, Completeness, Segment, ZhuyinCandidateType};

    #[test]
    fn completeness_wire_tokens_match_the_capture_format() {
        assert_eq!(Completeness::Complete.as_wire(), "complete");
        assert_eq!(Completeness::Partial.as_wire(), "partial");
    }

    #[test]
    fn candidate_type_discriminants_follow_the_declared_order() {
        let ordered = [
            (1, CandidateType::NbestMatch, "NBEST_MATCH_CANDIDATE"),
            (2, CandidateType::Normal, "NORMAL_CANDIDATE"),
            (3, CandidateType::Zombie, "ZOMBIE_CANDIDATE"),
            (
                4,
                CandidateType::PredictedBigram,
                "PREDICTED_BIGRAM_CANDIDATE",
            ),
            (
                5,
                CandidateType::PredictedPrefix,
                "PREDICTED_PREFIX_CANDIDATE",
            ),
            (6, CandidateType::Addon, "ADDON_CANDIDATE"),
            (7, CandidateType::Longer, "LONGER_CANDIDATE"),
            (
                8,
                CandidateType::PredictedPunctuation,
                "PREDICTED_PUNCTUATION_CANDIDATE",
            ),
        ];
        for (raw, variant, wire) in ordered {
            assert_eq!(CandidateType::from_raw(raw), Some(variant));
            assert_eq!(variant.as_wire(), wire);
            assert_eq!(CandidateType::from_wire(wire), Some(variant));
        }
        assert_eq!(CandidateType::from_raw(0), None);
        assert_eq!(CandidateType::from_raw(9), None);
        assert_eq!(CandidateType::from_raw(-1), None);
        assert_eq!(CandidateType::from_wire("SOMETHING_ELSE"), None);
    }

    #[test]
    fn sentinels_carry_no_syllable_or_range() {
        let syllable = Segment::Syllable {
            syllable: "ni".to_owned(),
            begin: 0,
            end: 2,
            completeness: Completeness::Complete,
        };
        assert!(!syllable.is_sentinel());
        assert_eq!(syllable.syllable(), Some("ni"));
        assert_eq!(syllable.range(), Some((0, 2)));
        for sentinel in [
            Segment::MissingKeyRest { offset: 1 },
            Segment::MissingPosition { offset: 1 },
            Segment::MissingPinyinString { offset: 1 },
            Segment::NoKeyColumns { parsed_len: 1 },
        ] {
            assert!(sentinel.is_sentinel());
            assert_eq!(sentinel.syllable(), None);
            assert_eq!(sentinel.range(), None);
        }
    }

    #[test]
    fn zhuyin_candidate_types_are_not_pinyin_candidate_types() {
        let ordered = [
            (1, ZhuyinCandidateType::BestMatch, "BEST_MATCH_CANDIDATE"),
            (
                2,
                ZhuyinCandidateType::NormalAfterCursor,
                "NORMAL_CANDIDATE_AFTER_CURSOR",
            ),
            (
                3,
                ZhuyinCandidateType::NormalBeforeCursor,
                "NORMAL_CANDIDATE_BEFORE_CURSOR",
            ),
            (4, ZhuyinCandidateType::Zombie, "ZOMBIE_CANDIDATE"),
        ];
        for (raw, variant, wire) in ordered {
            assert_eq!(ZhuyinCandidateType::from_raw(raw), Some(variant));
            assert_eq!(variant.as_wire(), wire);
        }
        assert_eq!(ZhuyinCandidateType::from_raw(0), None);
        assert_eq!(ZhuyinCandidateType::from_raw(5), None);
        // The same integer means different things in the two facades.
        assert_eq!(CandidateType::from_raw(2), Some(CandidateType::Normal));
        assert_eq!(
            ZhuyinCandidateType::from_raw(2),
            Some(ZhuyinCandidateType::NormalAfterCursor)
        );
    }
}
