//! Key expansion and the scoring error.
//!
//! [`expand_keys`] enumerates the complete-key sequences an incomplete-key
//! sequence stands for; [`ScoringError`] carries a backend failure as text.

use smallvec::SmallVec;

use crate::{SyllableKey, full_pinyin_syllables};

/// One expanded key sequence. Phrase length is capped at 16, so this stays
/// on the stack.
pub type ExpandedKeys = SmallVec<[SyllableKey; 16]>;

/// Completions of one (possibly incomplete) key. The densest initial is 26
/// syllables (`l`).
type KeyCompletions = SmallVec<[SyllableKey; 32]>;

/// Why scoring failed.
///
/// Backend failures arrive as text because the frozen `Dictionary` and
/// `LanguageModel` seams leave their `Error` types unbounded.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoringError {
    /// The dictionary backend failed.
    Dictionary(String),
    /// The language model backend failed.
    LanguageModel(String),
}

impl core::fmt::Display for ScoringError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Dictionary(message) => write!(formatter, "dictionary error: {message}"),
            Self::LanguageModel(message) => write!(formatter, "language model error: {message}"),
        }
    }
}

impl std::error::Error for ScoringError {}

/// Every complete-key sequence `keys` can stand for.
///
/// A complete key stands for itself. An incomplete key stands for each
/// complete syllable with the same phonetic initial, in frozen inventory
/// order.
///
/// # Known gap: long incomplete-key sequences yield nothing
///
/// The expansion is a Cartesian product, and each initial-only key multiplies
/// it by the number of syllables with that phonetic initial — 9 (`w`) to 26
/// (`l`) over the frozen inventory. When the product would exceed `limit`,
/// this returns
/// an **empty vector** — not a truncated one — so a caller cannot mistake a
/// subset for the whole answer.
///
/// The consequence is worth stating plainly rather than hiding behind
/// "bounded": a key sequence with two or more initials gets **no candidates
/// from the dictionary at all**, not a shorter list. The smallest possible
/// pair is 9 × 9 = 81, already past the default `limit` of 64, so in practice
/// the ceiling is one initial per sequence. `zzzzzzzz` decodes to eight `z`
/// keys, expands to 17⁸, and offers nothing; the session falls back to
/// handing the raw input back.
///
/// That is deliberate for W4 — a wrong candidate list is worse than none, and
/// the pin's own behaviour here is not something the captures pin down — but
/// it is a gap, not a design endpoint. Closing it wants a dictionary that can
/// answer a prefix query directly instead of being asked one exact key
/// sequence at a time, which is a W3 loader concern.
///
/// Each sequence is a stack [`ExpandedKeys`] (phrase length ≤ 16).
#[must_use]
pub fn expand_keys(keys: &[SyllableKey], limit: usize) -> Vec<ExpandedKeys> {
    // Expand key by key and bail at the first key that empties the
    // product or pushes it past `limit`: every exit returns the same empty
    // result, and expanding the remaining keys first made a long run of
    // initial-only keys cost one full inventory scan per key per call.
    let mut alternatives: SmallVec<[KeyCompletions; 16]> = SmallVec::new();
    let mut product = 1_usize;
    if product > limit {
        return Vec::new();
    }
    for key in keys {
        let choices = completions(*key);
        if choices.is_empty() {
            return Vec::new();
        }
        product = match product.checked_mul(choices.len()) {
            Some(product) if product <= limit => product,
            _ => return Vec::new(),
        };
        alternatives.push(choices);
    }

    let mut sequences: Vec<ExpandedKeys> = vec![ExpandedKeys::new()];
    for choices in alternatives {
        let mut next = Vec::with_capacity(sequences.len() * choices.len());
        for prefix in &sequences {
            for choice in &choices {
                // bounded by expansion_limit (default 64)
                let mut extended = prefix.clone();
                extended.push(*choice);
                next.push(extended);
            }
        }
        sequences = next;
    }
    sequences
}

/// The complete keys one initial-only key stands for.
///
/// An incomplete key `K` stands for every complete syllable whose
/// [`phonetic_initial`] is `K`. That is the pinned `m_initial` index:
/// `n` does not reach `ng`, and `z`/`c`/`s` do not reach `zh`/`ch`/`sh`.
fn completions(key: SyllableKey) -> KeyCompletions {
    if key.completeness() == crate::Completeness::Complete {
        let mut out = KeyCompletions::new();
        out.push(key);
        return out;
    }

    let initial = key.text();
    // The prefix test is implied by the initial test and rejects most of
    // the inventory for one comparison, before the longest-initial search.
    full_pinyin_syllables()
        .filter(|syllable| syllable.starts_with(initial))
        .filter(|syllable| crate::phonetic_initial(syllable) == Some(initial))
        .filter_map(SyllableKey::from_text)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::expand_keys;
    use crate::SyllableKey;

    fn keys(text: &str) -> Vec<SyllableKey> {
        text.split(',')
            .map(|key| SyllableKey::from_text(key).expect("frozen key"))
            .collect()
    }

    #[test]
    fn an_incomplete_key_expands_by_phonetic_initial() {
        // `n` reaches every N-initial syllable and never the zero-initial
        // spelling `ng`; `z`/`c`/`s` stop at their initial and exclude the
        // retroflex `zh`/`ch`/`sh` initials.
        for (initial, included, excluded) in [
            ("n", &["na", "nei", "ni", "nv"][..], &["ng"][..]),
            ("z", &["za", "zeng", "zuo"][..], &["zha", "zhong"][..]),
            ("c", &["ca", "ceng", "cuo"][..], &["cha", "cheng"][..]),
            ("s", &["sa", "seng", "suo"][..], &["sha", "sheng"][..]),
            ("zh", &["zha", "zhong"][..], &["za", "zeng"][..]),
            ("ch", &["cha", "cheng"][..], &["ca", "ceng"][..]),
            ("sh", &["sha", "sheng"][..], &["sa", "seng"][..]),
        ] {
            let expanded = expand_keys(&keys(initial), 4096);
            for syllable in included {
                assert!(
                    expanded
                        .iter()
                        .any(|sequence| sequence.as_slice() == keys(syllable).as_slice()),
                    "{initial} must include {syllable}"
                );
            }
            for syllable in excluded {
                assert!(
                    !expanded
                        .iter()
                        .any(|sequence| sequence.as_slice() == keys(syllable).as_slice()),
                    "{initial} must exclude {syllable}"
                );
            }
        }
    }

    #[test]
    fn an_incomplete_key_stands_for_what_it_prefixes() {
        let expanded = expand_keys(&keys("ni,h"), 64);
        assert!(expanded.len() > 10, "h prefixes many syllables");
        assert!(
            expanded
                .iter()
                .all(|sequence| sequence[0] == keys("ni")[0] && sequence.len() == 2)
        );
        assert!(
            expanded
                .iter()
                .any(|sequence| sequence.as_slice() == keys("ni,hao").as_slice())
        );
        assert!(
            expanded
                .iter()
                .any(|sequence| sequence.as_slice() == keys("ni,hong").as_slice())
        );

        let complete = expand_keys(&keys("ni,hao"), 64);
        assert_eq!(complete.len(), 1);
        assert_eq!(complete[0].as_slice(), keys("ni,hao").as_slice());
        let empty = expand_keys(&[], 64);
        assert_eq!(empty.len(), 1);
        assert!(empty[0].is_empty());
    }

    #[test]
    fn the_expansion_limit_includes_the_empty_product() {
        assert!(expand_keys(&[], 0).is_empty());
        for limit in [1, 64] {
            let empty = expand_keys(&[], limit);
            assert_eq!(empty.len(), 1);
            assert!(empty[0].is_empty());
        }
        assert!(expand_keys(&keys("ni"), 0).is_empty());
    }

    #[test]
    fn expansion_beyond_the_limit_yields_nothing_rather_than_a_subset() {
        assert!(expand_keys(&keys("h,h,h,h"), 64).is_empty());
        assert!(expand_keys(&keys("ni,h"), 2).is_empty());
    }
}
