//! The full-pinyin parser result before scan-matrix transformations.

use oxpinyin_core::{
    OptionBits, SyllableKey,
    graph::{Edge, SegmentGraph},
    tool::{Key, Parse},
};

/// Reuse the C API's graph, fewest-key selection and trailing-apostrophe
/// computation (`Session::full_parsed_len`). The standalone parser does not
/// perform the later scan-matrix resplit/divide steps: the pin disables its
/// parser post-processing at `pinyin_parser2.cpp:377-379` (074a2219).
#[must_use]
pub fn parse_full(input: &[u8], options: u32) -> Parse {
    let input = full_input(input);
    let input = input.as_ref();
    let options = OptionBits::from_bits(options);
    let Ok(graph) = SegmentGraph::build_with_options(input, options) else {
        return Parse::default();
    };
    let path = graph.fewest_keys(options.has_incomplete());
    selected_parse(input, &path)
}

fn selected_parse(input: &[u8], path: &[Edge]) -> Parse {
    Parse {
        consumed: super::apostrophe_extended(input, path.last().map_or(0, Edge::to)),
        keys: path
            .iter()
            .map(|edge| {
                Key::from_syllable(edge.key(), edge.tone(), edge.syllable_start()..edge.to())
            })
            .collect(),
    }
}

/// The physical matrix printed by the pin, including separator and terminal
/// zero keys. These programs set neither resplit nor divided-table option.
#[must_use]
pub fn physical_matrix(parsed: &Parse, options: u32) -> Vec<Vec<Key>> {
    use oxpinyin_core::{ChewingKey, SyllableKey};
    if parsed.keys.is_empty() {
        return Vec::new();
    }
    let mut columns = vec![Vec::new(); parsed.consumed + 1];
    for key in &parsed.keys {
        columns[key.span.start].push(key.clone());
    }
    columns[parsed.consumed].push(Key {
        key: ChewingKey::default(),
        span: parsed.consumed..parsed.consumed + 1,
    });
    for (i, key) in parsed.keys.iter().enumerate() {
        let next = parsed
            .keys
            .get(i + 1)
            .map_or(parsed.consumed, |key| key.span.start);
        for (fill, column) in columns.iter_mut().enumerate().take(next).skip(key.span.end) {
            column.push(Key {
                key: ChewingKey::default(),
                span: fill..fill + 1,
            });
        }
    }
    for column in &mut columns {
        let Some(origin) = column.first().cloned() else {
            continue;
        };
        let spelling = origin.key.with_tone(0).pinyin_string();
        if let Some(key) = SyllableKey::from_canonical_text(&spelling) {
            for alternate in key.fuzzy_alternatives(OptionBits::from_bits(options)) {
                column.push(Key {
                    key: alternate.chewing_key().with_tone(origin.key.tone),
                    span: origin.span.clone(),
                });
            }
        }
    }
    columns
}

/// One native n-best result, with non-null MatchResult positions.
pub struct LookupRow {
    /// Phrase-start column and token, in result-vector order.
    pub tokens: Vec<(usize, u32)>,
    /// Concatenated phrase text, retaining raw UCS-4 words.
    pub text: oxpinyin_core::PhraseText,
}

/// The one parsed scan matrix reused by test_pinyin_lookup's timed calls.
pub struct PreparedLookup {
    /// Returned parser length and original keys, for the physical dump.
    pub parsed: Parse,
    scan: Vec<Vec<super::ScanKey>>,
}

/// Parse once and build one scan matrix from the same selected edges.
///
/// # Errors
/// Returns the existing graph's input error.
pub fn prepare_lookup(input: &[u8], options: u32) -> Result<PreparedLookup, crate::EngineError> {
    let input = full_input(input);
    let options = OptionBits::from_bits(options);
    let graph = SegmentGraph::build_with_options(input.as_ref(), options)?;
    let path = graph.fewest_keys(options.has_incomplete());
    let parsed = selected_parse(input.as_ref(), &path);
    let scan = super::build_scan_matrix_over(&graph, &path, options, false);
    Ok(PreparedLookup { parsed, scan })
}

/// Run the existing PhoneticLookup<2,3> over an already-built matrix.
///
/// # Errors
/// Returns model errors from the existing decoder.
pub fn lookup<D, L>(
    prepared: &PreparedLookup,
    dictionary: &D,
    model: &L,
) -> Result<Vec<LookupRow>, crate::EngineError>
where
    D: oxpinyin_core::Dictionary<Syllable = SyllableKey>,
    D::Error: core::fmt::Display,
    L: oxpinyin_core::LanguageModel<Token = oxpinyin_core::PhraseToken>,
    L::Error: core::fmt::Display,
{
    crate::nbest::nbest_sentences_generation(
        crate::nbest::GenerationInput {
            matrix: &prepared.scan,
            bound: prepared.parsed.consumed,
            physical_separators: true,
            topology: None,
        },
        dictionary,
        model,
        &[oxpinyin_core::PhraseToken::new(1)],
        None,
        crate::NbestShape::PINYIN,
    )
    .map(|rows| {
        rows.into_iter()
            .map(|row| LookupRow {
                tokens: row
                    .spans
                    .iter()
                    .map(|span| (span.start, span.token.value()))
                    .collect(),
                text: row.text,
            })
            .collect()
    })
}

/// The existing PhraseLookup span DP over raw UCS-4 input.
///
/// # Errors
/// Returns decoder/model errors.
pub fn phrase_lookup<D, L>(
    words: &[u32],
    dictionary: &D,
    model: &L,
) -> Result<Vec<oxpinyin_core::PhraseToken>, crate::EngineError>
where
    D: oxpinyin_core::Dictionary<Syllable = SyllableKey>,
    D::Error: core::fmt::Display,
    L: oxpinyin_core::LanguageModel<Token = oxpinyin_core::PhraseToken>,
    L::Error: core::fmt::Display,
{
    crate::phrase::phrase_segment_ucs4(dictionary, model, words).map(|(_, tokens)| tokens)
}

// FullPinyinParser2 duplicates the whole explicit-length input with
// g_strndup. Its documented contract pads the n+1 buffer with NULs
// when strlen(str) < n (GLib gstrfuncs.c:397-407 at 2.86.1).
fn full_input(input: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    let Some(nul) = input.iter().position(|&byte| byte == 0) else {
        return std::borrow::Cow::Borrowed(input);
    };
    let mut padded = input.to_vec();
    padded[nul..].fill(0);
    std::borrow::Cow::Owned(padded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxpinyin_core::{PINYIN_INCOMPLETE, USE_TONE};
    /// 074a2219 tests/lookup/test_pinyin_lookup.cpp:94-111 carries parsed_len
    /// through fill_matrix to get_nbest_match, including the terminal column.
    #[test]
    fn prepared_lookup_keeps_the_consumed_bound() {
        use crate::fixture_doubles::FixtureDictionary;
        use oxpinyin_core::{LanguageModel, NbestStepCosts, PhraseToken};
        struct Model;
        impl LanguageModel for Model {
            type Error = core::convert::Infallible;
            type Token = PhraseToken;
            fn nbest_step_costs(
                &self,
                _: &PhraseToken,
                _: &PhraseToken,
            ) -> Result<NbestStepCosts, Self::Error> {
                Ok(NbestStepCosts {
                    blended: Some(0.5),
                    unigram: Some(0.5),
                    unigram_lambda: 1.0,
                })
            }
        }
        let vocab = "token=2\tkeys=ni\ttext=你\tunigram=1000\n";
        let dictionary = FixtureDictionary::parse(vocab).unwrap();
        let model = Model;
        let prepared = prepare_lookup(b"ni", 0).unwrap();
        assert_eq!(prepared.parsed.consumed, 2);
        let rows = lookup(&prepared, &dictionary, &model).unwrap();
        assert_eq!(rows[0].tokens, [(0, 2)]);
        assert_eq!(rows[0].text.outward_words().collect::<Vec<_>>(), [0x4f60]);
    }

    #[test]
    fn explicit_nul_spans_keep_the_pin_length_and_zero_pad_the_tail() {
        for options in [PINYIN_INCOMPLETE, USE_TONE | PINYIN_INCOMPLETE] {
            let parsed = parse_full(b"ni\0'hao", options);
            assert_eq!(parsed.consumed, 7);
            assert_eq!(parsed.keys.len(), 1);
            assert_eq!(parsed.keys[0].key.pinyin_string(), "ni");
            assert_eq!(parsed.keys[0].span, 0..7);
        }
    }
}
