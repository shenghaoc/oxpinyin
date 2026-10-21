//! Full-pinyin scheme indexes: the pinned `luoma_pinyin_index` and
//! `secondary_zhuyin_index`.
//!
//! Extracted from libpinyin `2.11.91`
//! (`0c5e80e1200f84fab185d1c5bde458b770a0636c`)
//! `src/storage/pinyin_parser_table.h:674/1083`, joined to canonical
//! spellings through `content_table`. Both indexes are plain
//! `IS_PINYIN` spelling→content maps — no correction aliases, no
//! incomplete rows, no ambiguities — and both match their
//! `content_table` spelling columns exactly. Source-embedded tables,
//! copyable under the project source policy; not model-archive data.

use oxpinyin_chewing::StringTable;

/// Number of entries in [`luoma_pinyin_index`].
pub const LUOMA_PINYIN_COUNT: usize = 406;
/// Number of entries in [`secondary_zhuyin_index`].
pub const SECONDARY_ZHUYIN_COUNT: usize = 406;

/// An indexed full-pinyin spelling table, decoded on demand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FullPinyinIndex(StringTable<2>);

impl FullPinyinIndex {
    /// Number of spelling rows.
    #[must_use]
    pub const fn len(self) -> usize {
        self.0.len()
    }

    /// Whether the table has no rows.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0.is_empty()
    }

    /// The input spelling and canonical pinyin at a row.
    #[must_use]
    pub fn get(self, index: usize) -> Option<(&'static str, &'static str)> {
        self.0
            .get(index)
            .map(|[input, canonical]| (input, canonical))
    }

    /// Canonical pinyin for an exact input spelling.
    #[must_use]
    pub fn canonical(self, spelling: &str) -> Option<&'static str> {
        self.0.field(self.0.position(spelling)?, 1)
    }

    /// All spelling rows in pinned order.
    #[must_use]
    pub fn entries(
        self,
    ) -> impl ExactSizeIterator<Item = (&'static str, &'static str)> + DoubleEndedIterator + Clone
    {
        self.0
            .entries()
            .map(|[input, canonical]| (input, canonical))
    }
}

/// Luoma input spellings and canonical pinyin, in pinned byte order.
#[must_use]
pub const fn luoma_pinyin_index() -> FullPinyinIndex {
    FullPinyinIndex(crate::string_tables::luoma_pinyin_index())
}

/// Secondary-zhuyin input spellings and canonical pinyin, in pinned byte order.
#[must_use]
pub const fn secondary_zhuyin_index() -> FullPinyinIndex {
    FullPinyinIndex(crate::string_tables::secondary_zhuyin_index())
}

/// Longest spelling in either index, in bytes.
///
/// The pinned parser's lookahead is `max_full_pinyin_length = 7`
/// including the tone digit (`pinyin_parser2.cpp:82`); the longest
/// index spelling is 6, so one tone digit fits inside the lookahead.
pub const MAX_INDEX_SPELLING_LEN: usize = 6;

/// Lookahead bound of the pinned full-pinyin DP, tone digit included
/// (`max_full_pinyin_length`, `pinyin_parser2.cpp:82`).
const MAX_LOOKAHEAD: usize = 7;

/// One parsed key: the canonical spelling the input segment maps to,
/// its raw byte span (tone digit included), and the parsed tone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FullPinyinIndexKey {
    canonical: &'static str,
    start: usize,
    end: usize,
    tone: u8,
}

impl FullPinyinIndexKey {
    /// The canonical full-pinyin spelling this segment resolves to.
    #[must_use]
    pub const fn canonical(&self) -> &'static str {
        self.canonical
    }

    /// Inclusive byte offset of the segment's first input byte.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// Exclusive byte offset one past the segment's last input byte
    /// (tone digit included).
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// Tone number (1..5), or 0 for zero tone.
    #[must_use]
    pub const fn tone(&self) -> u8 {
        self.tone
    }
}

/// Result of one full-pinyin index parse.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FullPinyinIndexParse {
    keys: Vec<FullPinyinIndexKey>,
    consumed: usize,
}

impl FullPinyinIndexParse {
    /// The parsed keys in input order.
    #[must_use]
    pub fn keys(&self) -> &[FullPinyinIndexKey] {
        &self.keys
    }

    /// Number of original input bytes consumed.
    #[must_use]
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// The keys as a `'`-joined canonical full-pinyin string.
    #[must_use]
    pub fn full_pinyin(&self) -> String {
        self.keys
            .iter()
            .map(|key| key.canonical)
            .collect::<Vec<_>>()
            .join("'")
    }
}

/// One dynamic-programming step of the pinned full-pinyin parse.
#[derive(Clone, Copy, Debug)]
struct Step {
    /// Whether any path reaches this step.
    reached: bool,
    /// Previous step of the best path (the DP's `m_last_step`).
    last: usize,
    /// Keys on the best path (apostrophes excluded, like upstream).
    num_keys: usize,
    /// Input bytes covered by the best path.
    parsed_len: usize,
    /// The last key the best path appended, `None` for apostrophe hops.
    key: Option<FullPinyinIndexKey>,
}

/// `FullPinyinParser2::parse_one_key` over an index
/// (`pinyin_parser2.cpp:155-205`): under `use_tone` a trailing digit
/// 1..5 is the tone and is consumed with the match; the whole substring
/// must resolve through the index (plain rows only — these indexes have
/// no option-gated rows).
fn parse_one_index_key(
    input: &[u8],
    options: crate::OptionBits,
    index: FullPinyinIndex,
) -> Option<FullPinyinIndexKey> {
    let use_tone = options.contains(crate::USE_TONE);
    let mut parsed_len = input.len();
    let mut tone = 0;
    if use_tone && parsed_len > 0 && matches!(input[parsed_len - 1], b'1'..=b'5') {
        tone = input[parsed_len - 1] - b'0';
        parsed_len -= 1;
    }
    if parsed_len == 0 || (use_tone && options.contains(crate::FORCE_TONE) && tone == 0) {
        return None;
    }

    let spelling = core::str::from_utf8(input.get(..parsed_len)?).ok()?;
    let canonical = index.canonical(spelling)?;

    let mut end = parsed_len;
    if use_tone && tone != 0 {
        end += 1;
    }
    Some(FullPinyinIndexKey {
        canonical,
        start: 0,
        end,
        tone,
    })
}

/// `FullPinyinParser2::parse` + `final_step` over an index
/// (`pinyin_parser2.cpp:222-381`): the pinned dynamic program.
///
/// Apostrophes hop between segments consuming one byte and no key; every
/// other position extends up to `max_full_pinyin_length = 7` bytes ahead
/// (stopping at the next apostrophe); steps relax by longest covered
/// input first, then fewest keys, then least distance (always zero
/// here), with the first equal value kept — which makes the earliest
/// start, i.e. the longest spelling, win ties. The result backtracks
/// the longest covered prefix from the beginning of the input.
#[must_use]
pub fn parse_full_pinyin_index(
    input: &[u8],
    use_tone: bool,
    index: FullPinyinIndex,
) -> FullPinyinIndexParse {
    let options = crate::OptionBits::from_bits(if use_tone { crate::USE_TONE } else { 0 });
    parse_index(input, options, index)
}

/// Parses an indexed full-pinyin scheme under the complete option word.
/// `FORCE_TONE` requires a tone only when `USE_TONE` is also set, matching
/// pin 074a2219 `pinyin_parser2.cpp:176-190`. Other option bits do not
/// gate these plain index rows. The boolean entry retains its behavior.
///
/// # Errors
/// The parser currently has no error cases: a rejected key ends the parsed
/// prefix. The error type is [`core::convert::Infallible`].
#[must_use]
pub fn parse_full_pinyin_index_with_options(
    input: &[u8],
    options: crate::OptionBits,
    index: FullPinyinIndex,
) -> Result<FullPinyinIndexParse, core::convert::Infallible> {
    Ok(parse_index(input, options, index))
}

fn parse_index(
    input: &[u8],
    options: crate::OptionBits,
    index: FullPinyinIndex,
) -> FullPinyinIndexParse {
    let mut steps = vec![
        Step {
            reached: false,
            last: 0,
            num_keys: 0,
            parsed_len: 0,
            key: None,
        };
        input.len() + 1
    ];
    steps[0].reached = true;

    for position in 0..input.len() {
        if input[position] == b'\'' {
            // Propagate the current step past the separator.
            let previous = steps[position];
            if previous.reached {
                let next = &mut steps[position + 1];
                let value = Step {
                    reached: true,
                    last: position,
                    num_keys: previous.num_keys,
                    parsed_len: previous.parsed_len + 1,
                    key: None,
                };
                if !next.reached
                    || value.parsed_len > next.parsed_len
                    || (value.parsed_len == next.parsed_len && value.num_keys < next.num_keys)
                {
                    *next = value;
                }
            }
            continue;
        }

        let next_sep = position
            + input[position..]
                .iter()
                .position(|&byte| byte == b'\'')
                .unwrap_or(input.len() - position);
        let try_end = (position + MAX_LOOKAHEAD).min(next_sep);
        for end in (position + 1)..=try_end {
            let Some(key) = parse_one_index_key(&input[position..end], options, index) else {
                continue;
            };
            let previous = steps[position];
            if !previous.reached {
                continue;
            }
            let value = Step {
                reached: true,
                last: position,
                num_keys: previous.num_keys + 1,
                parsed_len: previous.parsed_len + (end - position),
                key: Some(FullPinyinIndexKey {
                    canonical: key.canonical,
                    start: position,
                    end,
                    tone: key.tone,
                }),
            };
            let next = &mut steps[end];
            if !next.reached
                || value.parsed_len > next.parsed_len
                || (value.parsed_len == next.parsed_len && value.num_keys < next.num_keys)
            {
                *next = value;
            }
        }
    }

    // final_step: the longest covered prefix from the input's start.
    let mut best = 0;
    for position in (0..=input.len()).rev() {
        if steps[position].reached && steps[position].parsed_len == position {
            best = position;
            break;
        }
    }

    let mut keys = Vec::new();
    let mut cursor = best;
    while cursor != 0 {
        let step = steps[cursor];
        if let Some(key) = step.key {
            keys.push(key);
        }
        cursor = step.last;
    }
    keys.reverse();

    FullPinyinIndexParse {
        keys,
        consumed: best,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LUOMA_PINYIN_COUNT, SECONDARY_ZHUYIN_COUNT, luoma_pinyin_index, parse_full_pinyin_index,
        secondary_zhuyin_index,
    };

    #[test]
    fn indexes_match_the_pinned_shapes() {
        for (index, count) in [
            (luoma_pinyin_index(), LUOMA_PINYIN_COUNT),
            (secondary_zhuyin_index(), SECONDARY_ZHUYIN_COUNT),
        ] {
            assert_eq!(index.len(), count);
            assert_eq!(count, 406);
            for pair in index.entries().collect::<Vec<_>>().windows(2) {
                assert!(pair[0].0 < pair[1].0, "index not strictly sorted");
            }
            // Every target is a canonical key: the 440-id inventory
            // covers both indexes with nothing to spare or add.
            for (spelling, canonical) in index.entries() {
                assert!(
                    crate::SyllableKey::from_canonical_text(canonical).is_some(),
                    "{spelling} -> {canonical} leaves the inventory"
                );
            }
        }

        // The pinned remaps: shared spellings mean different syllables
        // per scheme.
        let luoma = |spelling: &str| {
            luoma_pinyin_index()
                .entries()
                .find(|row| row.0 == spelling)
                .map(|row| row.1)
        };
        let secondary = |spelling: &str| {
            secondary_zhuyin_index()
                .entries()
                .find(|row| row.0 == spelling)
                .map(|row| row.1)
        };
        assert_eq!(luoma("chi"), Some("qi"));
        assert_eq!(luoma("si"), Some("xi"));
        assert_eq!(secondary("chi"), Some("qi"));
        assert_eq!(secondary("ju"), Some("zhu"));
        assert_eq!(secondary("liu"), Some("lv"));
        // And scheme-specific spellings absent from pinyin_index.
        assert_eq!(luoma("chyueh"), Some("que"));
        assert_eq!(secondary("au"), Some("ao"));
    }

    #[test]
    fn luoma_parses_through_the_dp() {
        let parse =
            |input: &str| parse_full_pinyin_index(input.as_bytes(), false, luoma_pinyin_index());

        // Longest coverage wins; the canonical spelling is what the
        // segment maps to, spans stay raw.
        let nihao = parse("nihao");
        assert_eq!(nihao.consumed(), 5);
        assert_eq!(nihao.full_pinyin(), "ni'hao");

        let chiang = parse("chiang");
        assert_eq!(chiang.consumed(), 6);
        assert_eq!(chiang.full_pinyin(), "qiang");
        assert_eq!(chiang.keys()[0].start(), 0);
        assert_eq!(chiang.keys()[0].end(), 6);

        // Apostrophes separate keys and are consumed.
        let split = parse("ni'hao");
        assert_eq!(split.consumed(), 6);
        assert_eq!(split.full_pinyin(), "ni'hao");

        // Junk stops the parse: parsed length covers only the prefix
        // that resolves.
        let junk = parse("chiangQ");
        assert_eq!(junk.consumed(), 6);

        // A spelling only pinyin_index knows parses nothing here; the
        // luoma spelling of the ㄓ series targets the initial-only row
        // ("jhih" -> zh), and the full zhi syllable has no spelling at
        // all under this scheme.
        assert_eq!(parse("zhi").consumed(), 0);
        assert_eq!(parse("jhih").full_pinyin(), "zh");
    }

    #[test]
    fn index_parse_handles_tone_digits() {
        let parse = |input: &str, use_tone| {
            parse_full_pinyin_index(input.as_bytes(), use_tone, secondary_zhuyin_index())
        };

        // Under USE_TONE the trailing digit rides the key (secondary
        // spells zhu as "ju").
        let toned = parse("ju4", true);
        assert_eq!(toned.consumed(), 3);
        assert_eq!(toned.full_pinyin(), "zhu");
        assert_eq!(toned.keys()[0].tone(), 4);
        assert_eq!(toned.keys()[0].end(), 3);

        // Without USE_TONE the digit is junk: only the "ju" spelling
        // consumes.
        let untoned = parse("ju4", false);
        assert_eq!(untoned.consumed(), 2);
        assert_eq!(untoned.full_pinyin(), "zhu");

        // A tone digit alone parses nothing.
        assert_eq!(parse("3", true).consumed(), 0);
    }

    #[test]
    fn dp_prefers_longest_coverage_then_fewest_keys() {
        // "xian" style ambiguity through the secondary index: "shian"
        // covers 5 bytes as one key; "shi"->xi + "an" also covers 5 as
        // two keys — fewest keys wins, so the single long key parses.
        let parse = |input: &str| {
            parse_full_pinyin_index(input.as_bytes(), false, secondary_zhuyin_index())
        };
        assert_eq!(parse("shian").full_pinyin(), "xian");

        // An initial-only target parses with no option bit: these
        // indexes have no gated rows (secondary spells ㄘ-only "tsz").
        assert_eq!(parse("tsz").full_pinyin(), "c");
    }
}

#[cfg(test)]
mod option_tests {
    use super::*;
    use crate::{FORCE_TONE, OptionBits, USE_TONE};

    #[test]
    fn force_tone_is_nested_under_use_tone_for_both_indexes() {
        for (index, input) in [
            (luoma_pinyin_index(), "shih"),
            (secondary_zhuyin_index(), "tsz"),
        ] {
            for bits in [0, FORCE_TONE, USE_TONE, USE_TONE | FORCE_TONE] {
                let options = OptionBits::from_bits(bits);
                let plain = parse_full_pinyin_index_with_options(input.as_bytes(), options, index)
                    .expect("infallible parse");
                assert_eq!(
                    plain.consumed(),
                    if bits == USE_TONE | FORCE_TONE {
                        0
                    } else {
                        input.len()
                    }
                );
                let toned = format!("{input}4");
                let parsed = parse_full_pinyin_index_with_options(toned.as_bytes(), options, index)
                    .expect("infallible parse");
                assert_eq!(
                    parsed.consumed(),
                    if bits & USE_TONE != 0 {
                        toned.len()
                    } else {
                        input.len()
                    }
                );
                assert_eq!(
                    parsed.keys()[0].tone(),
                    if bits & USE_TONE != 0 { 4 } else { 0 }
                );
            }
        }
    }
}

#[cfg(test)]
mod identity_tests {
    #[test]
    fn luoma_pinyin_index_matches_original_rows() {
        assert_eq!(
            super::LUOMA_PINYIN_COUNT,
            super::reference::LUOMA_PINYIN_COUNT
        );
        assert!(
            super::luoma_pinyin_index()
                .entries()
                .eq(super::reference::LUOMA_PINYIN_INDEX)
        );
    }

    #[test]
    fn secondary_zhuyin_index_matches_original_rows() {
        assert_eq!(
            super::SECONDARY_ZHUYIN_COUNT,
            super::reference::SECONDARY_ZHUYIN_COUNT
        );
        assert!(
            super::secondary_zhuyin_index()
                .entries()
                .eq(super::reference::SECONDARY_ZHUYIN_INDEX)
        );
    }
}

#[cfg(test)]
mod reference {
    include!("table_data/full_pinyin_indexes.rs");
}
