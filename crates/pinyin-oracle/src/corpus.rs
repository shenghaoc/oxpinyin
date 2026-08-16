//! Deterministic parity-corpus generation.
//!
//! Methodology, byte domain and reproducibility contract are frozen in
//! `docs/findings/parity-corpus.md`. This module is the implementation of that
//! finding and is pure: output is a function of the frozen 405-syllable
//! inventory and [`SEED`] alone. No clock, no entropy, no map iteration order,
//! no filesystem order.
//!
//! The corpus holds inputs only. W2-T3 produces the oracle side by observation,
//! so a committed expectation would be a second, unverified oracle.

use std::collections::HashSet;

use oxpinyin_core::{FULL_PINYIN_SYLLABLES, FullPinyinParser, InputParser};

/// Fixed generator seed, `"Parity01"` read as big-endian ASCII.
pub const SEED: u64 = 0x5061_7269_7479_3031;

/// Repository-relative directory holding the committed corpus.
pub const CORPUS_DIR: &str = "tests/parity/corpus/inputs";

/// Largest number of apostrophe-separated groups in one generated utterance.
///
/// Structural half of the result-bound exclusion: with at most 8 groups the
/// Cartesian product cannot approach `MAX_PARSE_RESULTS`.
pub const MAX_APOSTROPHE_GROUPS: usize = 8;

/// Lowest byte in the corpus domain. Space is excluded; see [`generate`].
pub const DOMAIN_LOW: u8 = 0x21;

/// Highest byte in the corpus domain.
pub const DOMAIN_HIGH: u8 = 0x7e;

/// One corpus file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stratum {
    /// File name within [`CORPUS_DIR`].
    pub file_name: &'static str,
    /// What this stratum is for.
    pub description: &'static str,
    /// Inputs, in generation order, de-duplicated within the stratum.
    pub inputs: Vec<String>,
}

impl Stratum {
    /// Renders the stratum as the exact bytes of its committed file.
    ///
    /// One input per LF-terminated line, so a stratum containing the empty input
    /// begins with an empty line.
    #[must_use]
    pub fn to_file_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for input in &self.inputs {
            bytes.extend_from_slice(input.as_bytes());
            bytes.push(b'\n');
        }
        bytes
    }

    /// Parses committed file bytes back into inputs.
    ///
    /// Preserves empty lines, because the empty input is a corpus member. A
    /// missing final newline yields a trailing input rather than silently
    /// dropping data.
    #[must_use]
    pub fn parse_file_bytes(bytes: &[u8]) -> Vec<String> {
        let mut inputs = Vec::new();
        let mut current = Vec::new();
        for byte in bytes {
            if *byte == b'\n' {
                inputs.push(String::from_utf8_lossy(&current).into_owned());
                current.clear();
            } else {
                current.push(*byte);
            }
        }
        if !current.is_empty() {
            inputs.push(String::from_utf8_lossy(&current).into_owned());
        }
        inputs
    }
}

/// SplitMix64.
///
/// Chosen for portability and brevity: a dozen lines, no state-initialisation
/// subtleties, identical output on every supported host.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform value in `0..bound`, without modulo bias.
    ///
    /// Rejects draws at or above the largest multiple of `bound` representable
    /// in a `u64`, so every residue is equally likely.
    fn below(&mut self, bound: usize) -> usize {
        if bound <= 1 {
            return 0;
        }
        let bound_u64 = bound as u64;
        let zone = (u64::MAX / bound_u64) * bound_u64;
        loop {
            let draw = self.next_u64();
            if draw < zone {
                return (draw % bound_u64) as usize;
            }
        }
    }

    /// Uniform value in `low..=high`.
    fn between(&mut self, low: usize, high: usize) -> usize {
        if high <= low {
            return low;
        }
        low + self.below(high - low + 1)
    }

    fn pick<'item, T>(&mut self, items: &'item [T]) -> &'item T {
        &items[self.below(items.len())]
    }
}

/// Printable ASCII that has no parser syntax.
///
/// `0x21..=0x7e` minus lowercase letters minus apostrophe.
fn junk_alphabet() -> Vec<u8> {
    (DOMAIN_LOW..=DOMAIN_HIGH)
        .filter(|byte| !byte.is_ascii_lowercase() && *byte != b'\'')
        .collect()
}

/// Proper prefixes of table syllables that are not themselves table syllables.
fn partial_prefixes() -> Vec<String> {
    let table: HashSet<&str> = FULL_PINYIN_SYLLABLES.iter().copied().collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut prefixes: Vec<String> = Vec::new();

    for syllable in FULL_PINYIN_SYLLABLES {
        for length in 1..syllable.len() {
            let prefix = &syllable[..length];
            if table.contains(prefix) {
                continue;
            }
            if seen.insert(prefix) {
                prefixes.push(prefix.to_owned());
            }
        }
    }
    prefixes
}

/// Syllables that admit more than one complete segmentation on their own.
///
/// `docs/findings/parser-path-set.md` records 164 such entries. Deriving the set
/// from the parser rather than restating the number keeps the two in step.
fn ambiguous_syllables() -> Vec<&'static str> {
    let parser = FullPinyinParser;
    FULL_PINYIN_SYLLABLES
        .iter()
        .copied()
        .filter(|syllable| {
            parser
                .parse(syllable.as_bytes())
                .is_ok_and(|paths| paths.len() > 1)
        })
        .collect()
}

/// Whether the frozen parser can represent this input's full path set.
///
/// Assertion half of the result-bound exclusion.
fn within_result_bound(input: &str) -> bool {
    FullPinyinParser.parse(input.as_bytes()).is_ok()
}

/// In-domain, in-bound, not already present.
fn accept(seen: &mut HashSet<String>, candidate: &str) -> bool {
    let in_domain = candidate
        .bytes()
        .all(|byte| (DOMAIN_LOW..=DOMAIN_HIGH).contains(&byte));
    if !in_domain && !candidate.is_empty() {
        return false;
    }
    if !within_result_bound(candidate) {
        return false;
    }
    seen.insert(candidate.to_owned())
}

/// Draws `count` inputs by repeatedly calling `make`, skipping rejects.
///
/// Bounded so a stratum whose generator cannot reach `count` distinct inputs
/// terminates instead of spinning.
fn fill(
    rng: &mut SplitMix64,
    count: usize,
    mut make: impl FnMut(&mut SplitMix64) -> String,
) -> Vec<String> {
    let mut inputs = Vec::with_capacity(count);
    let mut seen = HashSet::with_capacity(count * 2);
    let mut attempts = 0_usize;
    let budget = count.saturating_mul(64).saturating_add(1024);

    while inputs.len() < count && attempts < budget {
        attempts += 1;
        let candidate = make(rng);
        if accept(&mut seen, &candidate) {
            inputs.push(candidate);
        }
    }
    inputs
}

fn join_syllables(rng: &mut SplitMix64, count: usize) -> String {
    let mut text = String::new();
    for _ in 0..count {
        text.push_str(rng.pick(&FULL_PINYIN_SYLLABLES));
    }
    text
}

/// Generates every stratum, in file order.
///
/// Deterministic: identical output for identical inputs on every host.
///
/// Space (`0x20`) is outside the domain so committed lines can never carry
/// leading or trailing whitespace, which editors and review tooling routinely
/// strip.
#[must_use]
pub fn generate() -> Vec<Stratum> {
    let junk = junk_alphabet();
    let prefixes = partial_prefixes();
    let ambiguous = ambiguous_syllables();

    let mut strata = Vec::new();

    // 01 — exhaustive over the inventory, in frozen order, no sampling.
    strata.push(Stratum {
        file_name: "01-single-syllable.txt",
        description: "every table syllable alone, in frozen inventory order",
        inputs: FULL_PINYIN_SYLLABLES
            .iter()
            .map(|syllable| (*syllable).to_owned())
            .collect(),
    });

    let mut rng = SplitMix64::new(SEED);

    strata.push(Stratum {
        file_name: "02-syllable-pairs.txt",
        description: "two concatenated syllables",
        inputs: fill(&mut rng, 2_000, |rng| join_syllables(rng, 2)),
    });

    strata.push(Stratum {
        file_name: "03-short-utterances.txt",
        description: "three to four concatenated syllables",
        inputs: fill(&mut rng, 2_500, |rng| {
            let count = rng.between(3, 4);
            join_syllables(rng, count)
        }),
    });

    strata.push(Stratum {
        file_name: "04-long-utterances.txt",
        description: "five to twelve concatenated syllables",
        inputs: fill(&mut rng, 1_500, |rng| {
            let count = rng.between(5, 12);
            join_syllables(rng, count)
        }),
    });

    strata.push(Stratum {
        file_name: "05-apostrophe.txt",
        description: "two to eight apostrophe-separated groups",
        inputs: fill(&mut rng, 1_200, |rng| {
            let groups = rng.between(2, MAX_APOSTROPHE_GROUPS);
            let mut parts = Vec::with_capacity(groups);
            for _ in 0..groups {
                let count = rng.between(1, 2);
                parts.push(join_syllables(rng, count));
            }
            parts.join("'")
        }),
    });

    strata.push(Stratum {
        file_name: "06-partial-tails.txt",
        description: "complete prefix followed by a partial syllable prefix",
        inputs: fill(&mut rng, 1_200, |rng| {
            let leading = rng.between(0, 3);
            let mut text = join_syllables(rng, leading);
            if rng.below(4) == 0 && !text.is_empty() {
                text.push('\'');
            }
            text.push_str(rng.pick(&prefixes));
            text
        }),
    });

    strata.push(Stratum {
        file_name: "07-ambiguity.txt",
        description: "built from syllables admitting multiple segmentations",
        inputs: fill(&mut rng, 800, |rng| {
            let count = rng.between(1, 4);
            let mut text = String::new();
            for _ in 0..count {
                text.push_str(rng.pick(&ambiguous));
            }
            text
        }),
    });

    strata.push(Stratum {
        file_name: "08-junk.txt",
        description: "unsupported printable bytes at prefix, middle and suffix",
        inputs: fill(&mut rng, 800, |rng| {
            let count = rng.between(1, 3);
            let base = join_syllables(rng, count);
            let noise = char::from(*rng.pick(&junk));
            match rng.below(3) {
                0 => format!("{noise}{base}"),
                1 => {
                    let target = rng.between(1, base.len().saturating_sub(1).max(1));
                    let split = base
                        .char_indices()
                        .map(|(index, _)| index)
                        .find(|index| *index >= target)
                        .unwrap_or(0);
                    let (left, right) = base.split_at(split);
                    format!("{left}{noise}{right}")
                }
                _ => format!("{base}{noise}"),
            }
        }),
    });

    strata.push(Stratum {
        file_name: "09-edge.txt",
        description: "boundary shapes mirroring the curated F-A edge cases",
        inputs: edge_cases(),
    });

    strata
}

/// Explicit boundary shapes. Fixed, not sampled.
///
/// The empty input is first, so the committed file begins with an empty line.
fn edge_cases() -> Vec<String> {
    let mut inputs: Vec<String> = Vec::new();

    // The empty input: one empty segmentation, not a failure.
    inputs.push(String::new());

    // Every single lowercase letter: some are complete syllables, some are
    // partials, some are neither.
    for letter in b'a'..=b'z' {
        inputs.push(char::from(letter).to_string());
    }

    // Apostrophe syntax, valid and invalid.
    for text in [
        "'", "''", "'''", "ni'", "'ni", "ni''hao", "ni'!", "ni'i", "xi'an", "chang'an", "a'a",
        "ni'h",
    ] {
        inputs.push(text.to_owned());
    }

    // Curated F-A shapes, so corpus and fixture agree on shape.
    for text in [
        "ni",
        "nihao",
        "zhongguoren",
        "zhuan",
        "xian",
        "fangan",
        "nih",
        "zhongg",
        "!ni",
        "ni!hao",
        "ni!",
    ] {
        inputs.push(text.to_owned());
    }

    // Longest table syllables, alone and doubled.
    for text in ["zhuang", "chuang", "shuang", "zhuangzhuang"] {
        inputs.push(text.to_owned());
    }

    // Pure junk of increasing length, including the F-A 4,096-byte case.
    for length in [1_usize, 2, 64, 4_096] {
        inputs.push("!".repeat(length));
    }

    // Long all-lowercase runs that are not valid pinyin.
    inputs.push("q".repeat(32));
    inputs.push("zzzzzzzz".to_owned());

    let mut seen: HashSet<String> = HashSet::new();
    inputs.retain(|input| seen.insert(input.clone()));
    inputs
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::{CORPUS_DIR, DOMAIN_HIGH, DOMAIN_LOW, Stratum, generate};
    use oxpinyin_core::{FULL_PINYIN_SYLLABLES, FullPinyinParser, InputParser};

    /// Generation is pure, so one run serves every assertion. Regenerating per
    /// test would dominate the suite's runtime for no extra coverage;
    /// `generation_is_deterministic` covers repeatability explicitly.
    fn corpus() -> &'static [Stratum] {
        static CORPUS: OnceLock<Vec<Stratum>> = OnceLock::new();
        CORPUS.get_or_init(generate)
    }

    #[test]
    fn generation_is_deterministic() {
        assert_eq!(corpus(), generate().as_slice());
    }

    #[test]
    fn corpus_meets_the_ten_thousand_input_floor() {
        let total: usize = corpus().iter().map(|s| s.inputs.len()).sum();
        assert!(total >= 10_000, "corpus has only {total} inputs");
    }

    #[test]
    fn every_input_is_in_the_frozen_byte_domain() {
        for stratum in corpus() {
            for input in &stratum.inputs {
                assert!(
                    input
                        .bytes()
                        .all(|byte| (DOMAIN_LOW..=DOMAIN_HIGH).contains(&byte)),
                    "{}: {input:?} leaves the corpus byte domain",
                    stratum.file_name
                );
            }
        }
    }

    #[test]
    fn every_input_is_within_the_parser_result_bound() {
        let parser = FullPinyinParser;
        for stratum in corpus() {
            for input in &stratum.inputs {
                assert!(
                    parser.parse(input.as_bytes()).is_ok(),
                    "{}: {input:?} exceeds MAX_PARSE_RESULTS",
                    stratum.file_name
                );
            }
        }
    }

    #[test]
    fn first_stratum_is_exhaustive_over_the_inventory() {
        let first = &corpus()[0];
        assert_eq!(first.file_name, "01-single-syllable.txt");
        assert_eq!(first.inputs.len(), 405);
        assert!(
            first
                .inputs
                .iter()
                .zip(FULL_PINYIN_SYLLABLES)
                .all(|(got, want)| got == want)
        );
    }

    #[test]
    fn strata_have_no_internal_duplicates() {
        for stratum in corpus() {
            let mut seen = stratum.inputs.clone();
            seen.sort_unstable();
            let before = seen.len();
            seen.dedup();
            assert_eq!(before, seen.len(), "{} repeats an input", stratum.file_name);
        }
    }

    #[test]
    fn strata_reach_their_documented_counts() {
        let counts: Vec<(&str, usize)> = corpus()
            .iter()
            .map(|s| (s.file_name, s.inputs.len()))
            .collect();
        assert_eq!(
            counts,
            vec![
                ("01-single-syllable.txt", 405),
                ("02-syllable-pairs.txt", 2_000),
                ("03-short-utterances.txt", 2_500),
                ("04-long-utterances.txt", 1_500),
                ("05-apostrophe.txt", 1_200),
                ("06-partial-tails.txt", 1_200),
                ("07-ambiguity.txt", 800),
                ("08-junk.txt", 800),
                ("09-edge.txt", 60),
            ]
        );
    }

    #[test]
    fn empty_input_is_the_first_edge_case() {
        let edge = corpus()
            .iter()
            .find(|s| s.file_name == "09-edge.txt")
            .expect("edge stratum exists");
        assert_eq!(edge.inputs.first().map(String::as_str), Some(""));
    }

    #[test]
    fn file_bytes_round_trip_including_the_empty_input() {
        for stratum in corpus() {
            let bytes = stratum.to_file_bytes();
            assert_eq!(Stratum::parse_file_bytes(&bytes), stratum.inputs);
            assert!(
                bytes.ends_with(b"\n"),
                "{} must end with LF",
                stratum.file_name
            );
        }
    }

    #[test]
    fn corpus_dir_is_the_documented_path() {
        assert_eq!(CORPUS_DIR, "tests/parity/corpus/inputs");
    }
}
