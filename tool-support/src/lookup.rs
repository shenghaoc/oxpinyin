//! The full-pinyin parser result before scan-matrix transformations.

use crate::parser::{Key, Parse};
use oxpinyin::{
    OptionBits,
    graph::{Edge, SegmentGraph},
    vocab::SyllableKey,
};

/// Converts a selected path into the consumed byte prefix and key spans.
#[must_use]
pub fn selected_parse(input: &[u8], path: &[Edge]) -> Parse {
    Parse {
        consumed: oxpinyin::__internal::apostrophe_extended(input, path.last().map_or(0, Edge::to)),
        keys: path
            .iter()
            .map(|edge| {
                Key::from_syllable(edge.key(), edge.tone(), edge.syllable_start()..edge.to())
            })
            .collect(),
    }
}

/// One native n-best result, with non-null MatchResult positions.
pub struct LookupRow {
    /// Phrase-start column and token, in result-vector order.
    pub tokens: Vec<(usize, u32)>,
    /// Concatenated phrase text, retaining raw UCS-4 words.
    pub text: oxpinyin::vocab::PhraseText,
    /// Bytes the row spans: the bound of the decode, the pin's
    /// `MatchResult` length less its terminal column.
    pub span: usize,
}

/// The parsed scan matrix used by the nonshipping phonetic lookup mode.
pub struct PreparedLookup {
    /// Consumed input-prefix length and keys captured when the lookup was prepared. Editing
    /// this field does not rebuild the stored lookup matrix.
    pub parsed: Parse,
    matrix: oxpinyin::__internal::ParsedMatrix,
}

impl PreparedLookup {
    /// Physical columns for the dump, from the same keys and topology
    /// lookup uses. Separator and terminal zero keys are emitted in place
    /// (074a2219 phonetic_key_matrix.cpp:52-78), without a second matrix.
    pub fn matrix_keys(&self) -> impl Iterator<Item = (usize, Key)> + '_ {
        let columns = if self.parsed.keys.is_empty() {
            0
        } else {
            self.matrix.column_count()
        };
        (0..columns).flat_map(|position| {
            let zero = self.matrix.has_zero(position).then(|| Key {
                key: oxpinyin::parser::ChewingKey::default(),
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

/// Parses the input and prepares the lookup matrix from the selected edges in one pass.
///
/// # Errors
///
/// Returns the graph input error when the byte positions cannot fit the graph.
pub fn prepare_lookup(
    input: &[u8],
    options: OptionBits,
) -> Result<PreparedLookup, oxpinyin::engine::EngineError> {
    let (input, graph) =
        full_graph(input, options).ok_or(oxpinyin::engine::EngineError::Failure)?;
    let path = graph.fewest_keys(options);
    let parsed = selected_parse(input.as_ref(), &path);
    let scan = oxpinyin::__internal::ScanMatrix::from_columns(
        oxpinyin::__internal::build_scan_matrix_over(&graph, &path, options, false),
    );
    let matrix = oxpinyin::__internal::ParsedMatrix::from_scan(&scan, parsed.consumed, true);
    Ok(PreparedLookup { parsed, matrix })
}

/// Run the existing PhoneticLookup<2,3> over an already-built matrix.
///
/// # Errors
/// Returns model errors from the existing decoder.
pub fn lookup<D, L>(
    prepared: &PreparedLookup,
    dictionary: &D,
    model: &L,
) -> Result<Vec<LookupRow>, oxpinyin::engine::EngineError>
where
    D: oxpinyin::model::Dictionary<Syllable = SyllableKey>,
    L: oxpinyin::model::LanguageModel,
{
    oxpinyin::__internal::nbest_sentences_generation(
        &prepared.matrix,
        dictionary,
        model,
        &[1],
        None,
        oxpinyin::engine::NbestShape::PINYIN,
    )
    .map(|rows| {
        rows.into_iter()
            .filter_map(|row| {
                Some(LookupRow {
                    text: row.text?,
                    tokens: row
                        .spans
                        .iter()
                        .map(|span| (span.start, span.token))
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
) -> Result<Vec<oxpinyin::vocab::PhraseToken>, oxpinyin::engine::EngineError>
where
    D: oxpinyin::model::Dictionary<Syllable = SyllableKey>,
    L: oxpinyin::model::LanguageModel,
{
    oxpinyin::__internal::phrase_segment_ucs4(dictionary, model, words).map(|(_, tokens)| tokens)
}

/// Builds the full-pinyin graph over the explicit-length bytes.
#[must_use]
pub fn full_graph(
    input: &[u8],
    options: OptionBits,
) -> Option<(std::borrow::Cow<'_, [u8]>, SegmentGraph)> {
    let input = full_input(input);
    let graph = SegmentGraph::build_with_options(input.as_ref(), options)?;
    Some((input, graph))
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
