//! Deterministic full-pinyin parsing.

use core::fmt;

use crate::options::{FORCE_TONE, USE_TONE};
use crate::{CHEWING_ZERO_TONE, ChewingKey};
use crate::{FULL_PINYIN_SYLLABLES, InputParser, MAX_SYLLABLE_LEN, OptionBits, SyllableKey};

const OVER_LIMIT: usize = MAX_PARSE_RESULTS + 1;

/// Maximum number of parse alternatives materialized by one call.
pub const MAX_PARSE_RESULTS: usize = 4_096;

/// Whether a parsed syllable is complete or a terminal partial prefix.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Completeness {
    /// A member of the frozen complete-syllable table.
    Complete,
    /// A non-complete proper prefix of a frozen complete syllable.
    Partial,
}

/// One syllable segment and its byte range in the original input.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ParsedSyllable {
    /// Lowercase ASCII syllable text.
    pub syllable: String,
    /// Inclusive byte offset where this segment begins.
    pub start: usize,
    /// Exclusive byte offset where this segment ends.
    pub end: usize,
    /// Whether this segment is complete or partial.
    pub completeness: Completeness,
}

/// One ordered parse alternative.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseResult {
    /// Parsed syllables in input order.
    pub syllables: Vec<ParsedSyllable>,
    /// Untouched input suffix beginning at the first unconsumed byte.
    pub remainder: Vec<u8>,
}

/// Stateless parser for untuned lowercase full pinyin.
#[derive(Clone, Copy, Debug, Default)]
pub struct FullPinyinParser;

/// Failure to represent the complete parser path set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ParseError {
    /// The exhaustive path set exceeds the frozen materialization limit.
    TooManyAlternatives {
        /// Maximum number of alternatives accepted by the parser.
        limit: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyAlternatives { limit } => {
                write!(formatter, "parse has more than {limit} alternatives")
            }
        }
    }
}

impl std::error::Error for ParseError {}

impl InputParser for FullPinyinParser {
    type Parse = ParseResult;
    type Error = ParseError;

    fn parse(&self, input: &[u8]) -> Result<Vec<Self::Parse>, Self::Error> {
        parse_input(input, OptionBits::default())
    }
}

impl FullPinyinParser {
    /// Parses `input` under parser option bits.
    ///
    /// The [`InputParser::parse`] implementation is this method with
    /// [`OptionBits::default`], so the frozen all-off path is unchanged.
    /// Correction aliases and the parser-table ambiguity aliases become
    /// complete syllables only when their bit is set; matrix-level fuzzy
    /// alternates are not parser paths.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when the input violates the scheme's parse rules.
    pub fn parse_with_options(
        &self,
        input: &[u8],
        options: OptionBits,
    ) -> Result<Vec<ParseResult>, ParseError> {
        parse_input(input, options)
    }

    /// Parses one full-pinyin key from the whole of `input`, mirroring
    /// `FullPinyinParser2::parse_one_key`
    /// (`src/storage/pinyin_parser2.cpp:164-214`).
    ///
    /// `options` is the caller's option word. Apostrophes are refused
    /// (upstream asserts them away); under `USE_TONE` a trailing
    /// `1..=5` is the tone — stripped before the spelling probe and
    /// written onto the key on a match — and `FORCE_TONE`, nested
    /// inside `USE_TONE` exactly like the pin, rejects a toneless
    /// input. The spelling probe covers the whole stripped input, so a
    /// multi-syllable string such as `nihao` is refused, not matched at
    /// its first syllable; alias resolution through
    /// [`SyllableKey::from_option_text`] is the option gate the batch
    /// path's edges use.
    ///
    /// [`None`] is the upstream `false`, including the assert shapes the
    /// no-abort policy renders as refusal (apostrophes, empty input).
    #[must_use]
    pub fn parse_one_key(&self, options: u32, input: &[u8]) -> Option<ChewingKey> {
        if input.contains(&b'\'') {
            return None;
        }
        let bits = OptionBits::from_bits(options);
        let mut tone = CHEWING_ZERO_TONE;
        let mut core = input;
        if bits.contains(USE_TONE) {
            if let Some((&last, rest)) = input.split_last()
                && (b'1'..=b'5').contains(&last)
            {
                tone = last - b'0';
                core = rest;
            }
            if bits.contains(FORCE_TONE) && tone == CHEWING_ZERO_TONE {
                return None;
            }
        }
        let text = core::str::from_utf8(core).ok()?;
        let syllable = SyllableKey::from_option_text(text, bits)?;
        let mut key = ChewingKey::from_pinyin(syllable.text())?;
        key.tone = tone;
        Some(key)
    }
}

#[derive(Debug)]
struct GroupPaths {
    paths: Vec<Vec<ParsedSyllable>>,
    consumed: usize,
    complete: bool,
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    cursor: usize,
    next_length: usize,
}

impl Frame {
    fn new(cursor: usize, end: usize) -> Self {
        Self {
            cursor,
            next_length: MAX_SYLLABLE_LEN.min(end.saturating_sub(cursor)),
        }
    }
}

fn parse_input(input: &[u8], options: OptionBits) -> Result<Vec<ParseResult>, ParseError> {
    let mut accumulated = vec![Vec::new()];
    let mut cursor = 0;
    let mut pending_separator = None;

    // Leading apostrophe run: upstream's DP (`pinyin_parser2.cpp:237-250`)
    // propagates across every `'` at position zero, so `'ni` consumes all
    // three bytes. Skip the run when a lowercase byte follows; otherwise
    // the whole input is remainder (matching the oracle-sentinel abort for
    // pure apostrophe strings).
    if cursor < input.len() && input[cursor] == b'\'' {
        let sep = cursor;
        while cursor < input.len() && input[cursor] == b'\'' {
            cursor += 1;
        }
        if cursor == input.len() || !input[cursor].is_ascii_lowercase() {
            return Ok(finish(accumulated, input, 0));
        }
        pending_separator = Some(sep);
    }

    loop {
        let group_start = cursor;
        let group_end = input[group_start..]
            .iter()
            .position(|byte| !byte.is_ascii_lowercase())
            .map_or(input.len(), |offset| group_start + offset);

        if group_start == group_end {
            let remainder_start = pending_separator.unwrap_or(group_start);
            return Ok(finish(accumulated, input, remainder_start));
        }

        let group = parse_group(&input[group_start..group_end], group_start, options)?;
        if group.consumed == 0 {
            let remainder_start = pending_separator.unwrap_or(group_start);
            return Ok(finish(accumulated, input, remainder_start));
        }

        accumulated = combine(accumulated, &group.paths)?;

        if !group.complete {
            return Ok(finish(accumulated, input, group_start + group.consumed));
        }

        cursor = group_end;
        if cursor == input.len() {
            return Ok(finish(accumulated, input, cursor));
        }
        if input[cursor] != b'\'' {
            return Ok(finish(accumulated, input, cursor));
        }

        let separator = cursor;
        // Upstream's `FullPinyinParser2::parse` (`pinyin_parser2.cpp:237-250`)
        // treats every apostrophe as a zero-width step-propagation, so any
        // run of consecutive apostrophes acts as a single separator once
        // the right group consumes a byte. Match that here: skip the whole
        // run before deciding whether the next group is admissible.
        while cursor < input.len() && input[cursor] == b'\'' {
            cursor += 1;
        }
        if cursor == input.len() || !input[cursor].is_ascii_lowercase() {
            return Ok(finish(accumulated, input, separator));
        }
        pending_separator = Some(separator);
    }
}

fn parse_group(
    group: &[u8],
    absolute_start: usize,
    options: OptionBits,
) -> Result<GroupPaths, ParseError> {
    let complete_count = count_complete_paths(group, options);
    if complete_count > MAX_PARSE_RESULTS {
        return Err(too_many_alternatives());
    }
    if complete_count != 0 {
        return Ok(GroupPaths {
            paths: enumerate_complete_paths(group, absolute_start, complete_count, options),
            consumed: group.len(),
            complete: true,
        });
    }

    let target = maximal_fallback_end(group, options);
    let fallback_count = count_fallback_paths(group, target, options);
    if fallback_count > MAX_PARSE_RESULTS {
        return Err(too_many_alternatives());
    }

    Ok(GroupPaths {
        paths: enumerate_fallback_paths(group, absolute_start, target, fallback_count, options),
        consumed: target,
        complete: false,
    })
}

fn count_complete_paths(group: &[u8], options: OptionBits) -> usize {
    let mut counts = vec![0_usize; group.len() + 1];
    counts[group.len()] = 1;

    for cursor in (0..group.len()).rev() {
        for length in (1..=MAX_SYLLABLE_LEN.min(group.len() - cursor)).rev() {
            if complete_syllable(&group[cursor..cursor + length], options).is_some() {
                counts[cursor] = bounded_add(counts[cursor], counts[cursor + length]);
            }
        }
    }

    counts[0]
}

fn enumerate_complete_paths(
    group: &[u8],
    absolute_start: usize,
    expected: usize,
    options: OptionBits,
) -> Vec<Vec<ParsedSyllable>> {
    let mut results = Vec::with_capacity(expected);
    let mut path = Vec::new();
    let mut stack = vec![Frame::new(0, group.len())];

    while let Some(frame) = stack.last_mut() {
        if frame.cursor == group.len() {
            results.push(path.clone());
            stack.pop();
            if !stack.is_empty() {
                path.pop();
            }
            continue;
        }

        let cursor = frame.cursor;
        let mut next = None;
        while frame.next_length != 0 {
            let length = frame.next_length;
            frame.next_length -= 1;
            if let Some(syllable) = complete_syllable(&group[cursor..cursor + length], options) {
                next = Some((syllable, length));
                break;
            }
        }

        if let Some((syllable, length)) = next {
            path.push(segment(
                syllable,
                absolute_start + cursor,
                absolute_start + cursor + length,
                Completeness::Complete,
            ));
            stack.push(Frame::new(cursor + length, group.len()));
        } else {
            stack.pop();
            if !stack.is_empty() {
                path.pop();
            }
        }
    }

    // Count/enumerate mismatch is an internal bug, not caller input; panicking
    // here preserves R3.7 (never silently drop valid paths) in release builds.
    assert_eq!(
        results.len(),
        expected,
        "complete path enumeration under-counted or over-counted"
    );
    results
}

fn maximal_fallback_end(group: &[u8], options: OptionBits) -> usize {
    let mut reachable = vec![false; group.len() + 1];
    reachable[0] = true;
    let mut target = 0;

    for cursor in 0..group.len() {
        if !reachable[cursor] {
            continue;
        }

        target = target.max(cursor + longest_partial_prefix(&group[cursor..]));
        for length in 1..=MAX_SYLLABLE_LEN.min(group.len() - cursor) {
            if complete_syllable(&group[cursor..cursor + length], options).is_some() {
                reachable[cursor + length] = true;
                target = target.max(cursor + length);
            }
        }
    }

    target
}

fn count_fallback_paths(group: &[u8], target: usize, options: OptionBits) -> usize {
    let mut ways = vec![0_usize; target + 1];
    ways[0] = 1;

    for cursor in 0..target {
        if ways[cursor] == 0 {
            continue;
        }

        for length in 1..=MAX_SYLLABLE_LEN.min(target - cursor) {
            if complete_syllable(&group[cursor..cursor + length], options).is_some() {
                ways[cursor + length] = bounded_add(ways[cursor + length], ways[cursor]);
            }
        }
    }

    let mut count = ways[target];
    for cursor in 0..target {
        if is_partial_only(&group[cursor..target]) {
            count = bounded_add(count, ways[cursor]);
        }
    }
    count
}

fn enumerate_fallback_paths(
    group: &[u8],
    absolute_start: usize,
    target: usize,
    expected: usize,
    options: OptionBits,
) -> Vec<Vec<ParsedSyllable>> {
    let mut results = Vec::with_capacity(expected);
    let mut path = Vec::new();
    let mut stack = vec![Frame::new(0, target)];

    while let Some(frame) = stack.last_mut() {
        if frame.cursor == target {
            results.push(path.clone());
            stack.pop();
            if !stack.is_empty() {
                path.pop();
            }
            continue;
        }

        let cursor = frame.cursor;
        let mut descended = false;
        while frame.next_length != 0 {
            let length = frame.next_length;
            frame.next_length -= 1;
            let end = cursor + length;
            let bytes = &group[cursor..end];

            if let Some(syllable) = complete_syllable(bytes, options) {
                path.push(segment(
                    syllable,
                    absolute_start + cursor,
                    absolute_start + end,
                    Completeness::Complete,
                ));
                stack.push(Frame::new(end, target));
                descended = true;
                break;
            }

            if end == target && is_partial_only(bytes) {
                let mut result = path.clone();
                result.push(segment(
                    ascii_text(bytes),
                    absolute_start + cursor,
                    absolute_start + end,
                    Completeness::Partial,
                ));
                results.push(result);
            }
        }

        if !descended && stack.last().is_some_and(|last| last.cursor == cursor) {
            stack.pop();
            if !stack.is_empty() {
                path.pop();
            }
        }
    }

    // Count/enumerate mismatch is an internal bug, not caller input; panicking
    // here preserves R3.7 (never silently drop valid paths) in release builds.
    assert_eq!(
        results.len(),
        expected,
        "fallback path enumeration under-counted or over-counted"
    );
    results
}

fn combine(
    left: Vec<Vec<ParsedSyllable>>,
    right: &[Vec<ParsedSyllable>],
) -> Result<Vec<Vec<ParsedSyllable>>, ParseError> {
    let Some(count) = left.len().checked_mul(right.len()) else {
        return Err(too_many_alternatives());
    };
    if count > MAX_PARSE_RESULTS {
        return Err(too_many_alternatives());
    }

    let mut combined = Vec::with_capacity(count);
    for left_path in left {
        for right_path in right {
            let mut path = Vec::with_capacity(left_path.len() + right_path.len());
            path.extend(left_path.iter().cloned());
            path.extend(right_path.iter().cloned());
            combined.push(path);
        }
    }
    Ok(combined)
}

fn finish(
    paths: Vec<Vec<ParsedSyllable>>,
    input: &[u8],
    remainder_start: usize,
) -> Vec<ParseResult> {
    paths
        .into_iter()
        .map(|syllables| ParseResult {
            syllables,
            remainder: input[remainder_start..].to_vec(),
        })
        .collect()
}

fn complete_syllable(bytes: &[u8], options: OptionBits) -> Option<&'static str> {
    let spelling = core::str::from_utf8(bytes).ok()?;
    let key = SyllableKey::from_option_text(spelling, options)?;
    if key.completeness() == crate::Completeness::Complete {
        Some(key.text())
    } else {
        None
    }
}

fn longest_partial_prefix(bytes: &[u8]) -> usize {
    (1..=MAX_SYLLABLE_LEN.min(bytes.len()))
        .rev()
        .find(|length| is_partial_only(&bytes[..*length]))
        .unwrap_or(0)
}

fn is_partial_only(bytes: &[u8]) -> bool {
    complete_syllable(bytes, OptionBits::default()).is_none()
        && FULL_PINYIN_SYLLABLES
            .iter()
            .any(|syllable| bytes.len() < syllable.len() && syllable.as_bytes().starts_with(bytes))
}

fn segment(
    syllable: impl Into<String>,
    start: usize,
    end: usize,
    completeness: Completeness,
) -> ParsedSyllable {
    ParsedSyllable {
        syllable: syllable.into(),
        start,
        end,
        completeness,
    }
}

fn ascii_text(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

const fn bounded_add(left: usize, right: usize) -> usize {
    let sum = left.saturating_add(right);
    if sum > OVER_LIMIT { OVER_LIMIT } else { sum }
}

const fn too_many_alternatives() -> ParseError {
    ParseError::TooManyAlternatives {
        limit: MAX_PARSE_RESULTS,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Completeness, FullPinyinParser, MAX_PARSE_RESULTS, ParseError, ParseResult, ParsedSyllable,
    };
    use crate::ChewingKey;
    use crate::options::{FORCE_TONE, PINYIN_CORRECT_GN_NG, USE_TONE};
    use crate::{FULL_PINYIN_SYLLABLES, InputParser};

    /// `FullPinyinParser2::parse_one_key`: the whole stripped input is
    /// one spelling probe — multi-syllable and apostrophe inputs are
    /// refused, a trailing `1..=5` is the tone under `USE_TONE`, and
    /// `FORCE_TONE` (nested inside `USE_TONE`) rejects toneless input.
    /// The crate's `content_table` port covers exactly the frozen
    /// inventory: every [`SyllableKey`] resolves to a key whose row
    /// spelling is the key's own, and the packed form round-trips.
    #[test]
    fn frozen_inventory_matches_the_chewing_content_table() {
        for index in 0..crate::SYLLABLE_KEY_COUNT {
            let syllable = crate::SyllableKey::from_index(index).expect("in range");
            let key = ChewingKey::from_pinyin(syllable.text()).expect("row exists");
            assert_eq!(key.pinyin_spelling(), syllable.text());
            assert_eq!(key.tone, 0);
            assert_eq!(ChewingKey::from_packed(key.to_packed()), key);
        }
    }

    #[test]
    fn full_pinyin_one_key_law() {
        let parser = FullPinyinParser;

        let key = parser.parse_one_key(0, b"ni").expect("ni parses");
        assert_eq!(key.pinyin_string(), "ni");

        // The probe covers the whole input: no forward maximum match.
        assert_eq!(parser.parse_one_key(0, b"nihao"), None);
        // Apostrophes are refused (the pin asserts on them).
        assert_eq!(parser.parse_one_key(0, b"ni'hao"), None);
        assert_eq!(parser.parse_one_key(0, b""), None);

        // Toneless without USE_TONE; tone carried with it.
        let toned = parser.parse_one_key(USE_TONE, b"zai4").expect("zai4");
        assert_eq!(toned.pinyin_string(), "zai4");
        assert_eq!(parser.parse_one_key(0, b"zai4"), None);

        // FORCE_TONE is dead without USE_TONE, rejects the toneless
        // `zai` with it, and accepts the toned form.
        assert!(parser.parse_one_key(FORCE_TONE, b"zai").is_some());
        assert_eq!(parser.parse_one_key(USE_TONE | FORCE_TONE, b"zai"), None);
        assert!(
            parser
                .parse_one_key(USE_TONE | FORCE_TONE, b"zai4")
                .is_some()
        );
        // `6` is not a tone: the digit stays in the core and fails it.
        assert_eq!(parser.parse_one_key(USE_TONE, b"zai6"), None);

        // Initial-only keys parse under PINYIN_INCOMPLETE-adjacent
        // exactness: they are real inventory keys.
        let b = parser.parse_one_key(0, b"b").expect("initial-only");
        assert_eq!(b.pinyin_string(), "b");
        assert_eq!(b.yunmu_string(), "");

        // Correction aliases resolve through the option gate.
        let ang = parser
            .parse_one_key(PINYIN_CORRECT_GN_NG, b"agn")
            .expect("agn corrects");
        assert_eq!(ang.pinyin_string(), "ang");
        assert_eq!(parser.parse_one_key(0, b"agn"), None);
    }

    #[test]
    fn complete_paths_follow_frozen_greedy_order() {
        assert_parse(b"", &[result(vec![], b"")]);
        assert_parse(b"ni", &[result(vec![complete("ni", 0, 2)], b"")]);
        assert_parse(
            b"nihao",
            &[
                result(vec![complete("ni", 0, 2), complete("hao", 2, 5)], b""),
                result(
                    vec![
                        complete("ni", 0, 2),
                        complete("ha", 2, 4),
                        complete("o", 4, 5),
                    ],
                    b"",
                ),
            ],
        );
        assert_parse(
            b"xian",
            &[
                result(vec![complete("xian", 0, 4)], b""),
                result(vec![complete("xi", 0, 2), complete("an", 2, 4)], b""),
            ],
        );
        assert_parse(
            b"fangan",
            &[
                result(vec![complete("fang", 0, 4), complete("an", 4, 6)], b""),
                result(vec![complete("fan", 0, 3), complete("gan", 3, 6)], b""),
                result(
                    vec![
                        complete("fa", 0, 2),
                        complete("ng", 2, 4),
                        complete("an", 4, 6),
                    ],
                    b"",
                ),
            ],
        );
    }

    #[test]
    fn apostrophes_and_partial_fallback_follow_frozen_boundaries() {
        assert_parse(
            b"xi'an",
            &[result(
                vec![complete("xi", 0, 2), complete("an", 3, 5)],
                b"",
            )],
        );
        assert_parse(
            b"chang'an",
            &[
                result(vec![complete("chang", 0, 5), complete("an", 6, 8)], b""),
                result(
                    vec![
                        complete("cha", 0, 3),
                        complete("ng", 3, 5),
                        complete("an", 6, 8),
                    ],
                    b"",
                ),
            ],
        );
        assert_parse(
            b"nih",
            &[result(vec![complete("ni", 0, 2), partial("h", 2, 3)], b"")],
        );
        assert_parse(
            b"zhongg",
            &[result(
                vec![complete("zhong", 0, 5), partial("g", 5, 6)],
                b"",
            )],
        );
        assert_parse(
            b"ni'h",
            &[result(vec![complete("ni", 0, 2), partial("h", 3, 4)], b"")],
        );
        assert_parse(
            b"nih'ao",
            &[result(
                vec![complete("ni", 0, 2), partial("h", 2, 3)],
                b"'ao",
            )],
        );
        // Consecutive apostrophes are a single separator: upstream's DP
        // propagates step-by-step across every `'`, so `ni''hao` consumes
        // the whole input and the right group's own alternates surface.
        assert_parse(
            b"ni''hao",
            &[
                result(vec![complete("ni", 0, 2), complete("hao", 4, 7)], b""),
                result(
                    vec![
                        complete("ni", 0, 2),
                        complete("ha", 4, 6),
                        complete("o", 6, 7),
                    ],
                    b"",
                ),
            ],
        );
        // Leading apostrophe run: upstream's DP propagates across every
        // `'` at position zero, so `'ni` consumes all three bytes.
        assert_parse(b"'ni", &[result(vec![complete("ni", 1, 3)], b"")]);
        assert_parse(b"''ni", &[result(vec![complete("ni", 2, 4)], b"")]);
    }

    #[test]
    fn junk_and_invalid_separators_are_untouched_remainders() {
        let cases: &[(&[u8], Vec<ParsedSyllable>, &[u8])] = &[
            (b"!ni", vec![], b"!ni"),
            (b"ni!hao", vec![complete("ni", 0, 2)], b"!hao"),
            (b"ni!", vec![complete("ni", 0, 2)], b"!"),
            (b"ni'", vec![complete("ni", 0, 2)], b"'"),
            (b"ni'i", vec![complete("ni", 0, 2)], b"'i"),
            (b"ni'!", vec![complete("ni", 0, 2)], b"'!"),
            (&[0xff, b'n', b'i'], vec![], &[0xff, b'n', b'i']),
            (
                &[b'n', b'i', 0xff, b'h', b'a', b'o'],
                vec![complete("ni", 0, 2)],
                &[0xff, b'h', b'a', b'o'],
            ),
        ];

        for (input, syllables, remainder) in cases {
            assert_parse(input, &[result(syllables.clone(), remainder)]);
        }
    }

    #[test]
    fn every_frozen_syllable_is_the_first_complete_path() {
        let parser = FullPinyinParser;
        for syllable in FULL_PINYIN_SYLLABLES {
            let parsed = parser
                .parse(syllable.as_bytes())
                .expect("one complete syllable stays below the path limit");
            assert_eq!(
                parsed[0],
                result(vec![complete(syllable, 0, syllable.len())], b"")
            );
        }
    }

    #[test]
    fn parsing_is_deterministic() {
        let parser = FullPinyinParser;
        for input in [
            b"nihao".as_slice(),
            b"xian",
            b"fangan",
            b"xi'an",
            b"nih'ao",
            b"ni!hao",
            &[0xff, b'a'],
        ] {
            assert_eq!(parser.parse(input), parser.parse(input));
        }
    }

    #[test]
    fn cartesian_path_limit_is_exact() {
        let at_limit = repeated_ambiguous_groups(12);
        let parser = FullPinyinParser;
        assert_eq!(
            parser
                .parse(&at_limit)
                .expect("twelve two-way groups are exactly at the limit")
                .len(),
            MAX_PARSE_RESULTS
        );

        let over_limit = repeated_ambiguous_groups(13);
        assert_eq!(
            parser.parse(&over_limit),
            Err(ParseError::TooManyAlternatives {
                limit: MAX_PARSE_RESULTS
            })
        );
    }

    fn assert_parse(input: &[u8], expected: &[ParseResult]) {
        let actual = FullPinyinParser
            .parse(input)
            .expect("frozen examples stay below the path limit");
        assert_eq!(actual, expected, "input: {input:?}");
    }

    fn complete(syllable: &str, start: usize, end: usize) -> ParsedSyllable {
        segment(syllable, start, end, Completeness::Complete)
    }

    fn partial(syllable: &str, start: usize, end: usize) -> ParsedSyllable {
        segment(syllable, start, end, Completeness::Partial)
    }

    fn segment(
        syllable: &str,
        start: usize,
        end: usize,
        completeness: Completeness,
    ) -> ParsedSyllable {
        ParsedSyllable {
            syllable: syllable.to_owned(),
            start,
            end,
            completeness,
        }
    }

    fn result(syllables: Vec<ParsedSyllable>, remainder: &[u8]) -> ParseResult {
        ParseResult {
            syllables,
            remainder: remainder.to_vec(),
        }
    }

    fn repeated_ambiguous_groups(count: usize) -> Vec<u8> {
        std::iter::repeat_n("xian", count)
            .collect::<Vec<_>>()
            .join("'")
            .into_bytes()
    }
}
