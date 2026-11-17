//! The full-pinyin parser result before scan-matrix transformations.

use crate::{
    OptionBits, SyllableKey,
    graph::{Edge, SegmentGraph},
    tool::{Key, Parse},
};

/// Reuse the C API's graph, fewest-key selection and trailing-apostrophe
/// computation (`Session::full_parsed_len`). The standalone parser does not
/// perform the later scan-matrix resplit/divide steps: the pin disables its
/// parser post-processing at `pinyin_parser2.cpp:377-379` (074a2219).
#[must_use]
pub fn parse_full(input: &[u8], options: OptionBits) -> Parse {
    let input = full_input(input);
    let input = input.as_ref();
    let Ok(graph) = SegmentGraph::build_with_options(input, options) else {
        return Parse::default();
    };
    let path = graph.fewest_keys(options.contains(OptionBits::PINYIN_INCOMPLETE));
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
pub fn physical_matrix(parsed: &Parse, options: OptionBits) -> Vec<Vec<Key>> {
    use crate::{ChewingKey, SyllableKey};
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
            for alternate in key.fuzzy_alternatives(options) {
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
    pub text: crate::PhraseText,
    /// Bytes the row spans: the bound of the decode, the pin's
    /// `MatchResult` length less its terminal column.
    pub span: usize,
}

/// The parsed scan matrix used by the nonshipping phonetic lookup mode.
pub struct PreparedLookup {
    /// Returned parser length and original keys.
    pub parsed: Parse,
    scan: super::matrix::ScanMatrix,
    matrix: super::matrix::ParsedMatrix,
}

impl PreparedLookup {
    /// Physical columns for the dump, from the same keys and topology
    /// lookup uses. Separator and terminal zero keys are emitted in place
    /// (074a2219 phonetic_key_matrix.cpp:52-78), without a second matrix.
    pub fn matrix_keys(&self) -> impl Iterator<Item = (usize, Key)> + '_ {
        let columns = if self.parsed.keys.is_empty() {
            0
        } else {
            self.matrix.len()
        };
        (0..columns).flat_map(|position| {
            let zero = self.matrix.zeros[position].then(|| Key {
                key: crate::ChewingKey::default(),
                span: position..position + 1,
            });
            let keys = self
                .matrix
                .get(position)
                .unwrap_or_default()
                .iter()
                .map(|key| Key {
                    key: key.key.chewing_key().with_tone(key.tone),
                    span: key.syllable_start..key.to,
                });
            zero.into_iter().chain(keys).map(move |key| (position, key))
        })
    }
}

/// Parse once and build one scan matrix from the same selected edges.
///
/// # Errors
/// Returns the existing graph's input error.
pub fn prepare_lookup(
    input: &[u8],
    options: OptionBits,
) -> Result<PreparedLookup, crate::engine::EngineError> {
    let input = full_input(input);
    let graph = SegmentGraph::build_with_options(input.as_ref(), options)?;
    let path = graph.fewest_keys(options.contains(OptionBits::PINYIN_INCOMPLETE));
    let parsed = selected_parse(input.as_ref(), &path);
    let scan = super::matrix::ScanMatrix::from_columns(super::build_scan_matrix_over(
        &graph, &path, options, false,
    ));
    let matrix = super::matrix::ParsedMatrix::from_scan(&scan, parsed.consumed, true);
    Ok(PreparedLookup {
        parsed,
        scan,
        matrix,
    })
}

/// Run the existing PhoneticLookup<2,3> over an already-built matrix.
///
/// # Errors
/// Returns model errors from the existing decoder.
pub fn lookup<D, L>(
    prepared: &PreparedLookup,
    dictionary: &D,
    model: &L,
) -> Result<Vec<LookupRow>, crate::engine::EngineError>
where
    D: crate::Dictionary<Syllable = SyllableKey>,
    D::Error: core::fmt::Display,
    L: crate::LanguageModel<Token = crate::PhraseToken>,
    L::Error: core::fmt::Display,
{
    crate::engine::nbest::nbest_sentences_generation(
        crate::engine::nbest::GenerationInput {
            matrix: &prepared.scan,
            bound: prepared.parsed.consumed,
            physical_separators: true,
            topology: Some(&prepared.matrix),
        },
        dictionary,
        model,
        &[crate::PhraseToken::new(1)],
        None,
        crate::engine::NbestShape::PINYIN,
    )
    .map(|rows| {
        rows.into_iter()
            .filter_map(|row| {
                Some(LookupRow {
                    text: row.text?,
                    tokens: row
                        .spans
                        .iter()
                        .map(|span| (span.start, span.token.value()))
                        .collect(),
                    span: row.span,
                })
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
) -> Result<Vec<crate::PhraseToken>, crate::engine::EngineError>
where
    D: crate::Dictionary<Syllable = SyllableKey>,
    D::Error: core::fmt::Display,
    L: crate::LanguageModel<Token = crate::PhraseToken>,
    L::Error: core::fmt::Display,
{
    crate::engine::phrase::phrase_segment_ucs4(dictionary, model, words).map(|(_, tokens)| tokens)
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
mod tests;
