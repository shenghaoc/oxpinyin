//! Parser records for the pin's nonshipping `test_parser2` program.
//!
//! [`Key`] pairs a phonetic key with its input
//! byte span. [`Parse`] retains those keys and the consumed prefix.
//! [`parse_direct`] parses direct full-pinyin or Zhuyin input for the REPL.

use core::ops::Range;

use oxpinyin::__internal::parser::FullPinyinParser;
use oxpinyin::facade::{ChewingKey, SyllableKey};

/// A key and its `ChewingKeyRest`, retained together so their counts agree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Key {
    /// Parsed key, including tone.
    pub key: ChewingKey,
    /// Byte offsets before the pin's `guint16` outward conversion.
    pub span: Range<usize>,
}

impl Key {
    /// Resolves the syllable's display spelling to a `ChewingKey`, applies
    /// `tone`, and keeps `span`. A matrix key without a spelling row becomes
    /// the zero key before the tone is applied.
    #[must_use]
    #[expect(
        clippy::expect_used,
        reason = "every_key_decodes_from_its_chewing_fields verifies the shared inventory"
    )]
    pub fn from_syllable(key: SyllableKey, tone: u8, span: Range<usize>) -> Self {
        Self {
            // Every SyllableKey names a row in the same chewing inventory.
            key: ChewingKey::from_pinyin(key.text())
                .expect("SyllableKey names a chewing content row")
                .with_tone(tone),
            span,
        }
    }
}

/// The pin's returned length and parallel key/key-rest arrays.
#[derive(Default, Debug, Eq, PartialEq)]
pub struct Parse {
    /// `parse`'s consumed byte count, including skipped separators.
    pub consumed: usize,
    /// Keys and raw spans, in parser order.
    pub keys: Vec<Key>,
}

// PinyinDirectParser2::parse and ZhuyinDirectParser2::parse, pinyin_parser2.cpp:623–715
// and zhuyin_parser2.cpp:858–960.
/// Parses space- or apostrophe-separated full pinyin, or Zhuyin text under
/// [`Law::Zhuyin`](oxpinyin::facade::Law::Zhuyin). Each segment must spell one key. After a match, separator runs count toward the
/// consumed byte prefix. A leading separator or a rejected segment stops the parse.
#[must_use]
pub fn parse_direct(
    input: &[u8],
    options: oxpinyin::OptionBits,
    law: oxpinyin::facade::Law,
) -> Parse {
    let mut keys = Vec::new();
    let consumed = oxpinyin::__internal::scan_direct(
        input,
        |span| {
            let segment = &input[span];
            if law == oxpinyin::facade::Law::Zhuyin {
                let (key, tone) = oxpinyin::__internal::parse_zhuyin_direct_key(
                    core::str::from_utf8(segment).ok()?,
                    options,
                )?;
                ChewingKey::from_pinyin(key.text()).map(|key| key.with_tone(tone))
            } else {
                FullPinyinParser::parse_one_key(options, segment)
            }
        },
        |key, span| keys.push(Key { key, span }),
    );
    Parse { consumed, keys }
}

#[cfg(test)]
mod tests;

/// Matrix-level fuzzy alternates, upstream append order.
///
/// `fuzzy_syllable_step` applies initials, re-fetches the column, then
/// finals (`phonetic_key_matrix.cpp:238-306`), so an initial bit and a
/// final bit compose (`can` → `chang` under `AMB_C_CH` + `AMB_AN_ANG`).
#[must_use]
pub fn fuzzy_alternatives(key: SyllableKey, options: oxpinyin::OptionBits) -> Vec<SyllableKey> {
    fuzzy_chewing_alternatives(key.chewing_key(), options)
        .into_iter()
        .map(SyllableKey::from_matrix_key)
        .collect()
}

// fuzzy_syllable_step, phonetic_key_matrix.cpp:238–306.
/// Returns the enabled initial and final substitutions in matrix order, preserving tone.
/// The original key is omitted. Final substitutions apply to the original and to each
/// admitted initial alternative, even when the resulting final has no spelling row.
#[must_use]
fn fuzzy_chewing_alternatives(key: ChewingKey, options: oxpinyin::OptionBits) -> Vec<ChewingKey> {
    let initials = oxpinyin::__internal::fuzzy_chewing_initials(key, options);
    let mut out = initials.clone();
    for source in std::iter::once(key).chain(initials) {
        out.extend(oxpinyin::__internal::fuzzy_chewing_finals(source, options));
    }
    out
}
