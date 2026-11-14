//! Retained parse topology, independent of dictionary and sentence state.
//!
//! Pin 074a2219 storage/phonetic_key_matrix.cpp:52-78: real keys occupy
//! raw begins, zero keys fill only gaps between keys and the final column.
//! In particular an initial separator gap is empty, not a zero-key path.

use super::*;

#[derive(Clone, Debug, Default)]
pub(crate) struct ParsedMatrix {
    pub(crate) columns: Vec<Vec<ScanKey>>,
    pub(crate) zeros: Vec<bool>,
    pub(crate) bound: usize,
}

/// The scan matrix one parse fills, as the pin's `PhoneticKeyMatrix`
/// holds it (`phonetic_key_matrix.h` at 074a2219): every key in one buffer,
/// column by column, with the offset each column starts at. A column is
/// the byte a key's edge starts from. A key that rides over a separator
/// run starts its edge inside that run, so no key of a later column has a
/// smaller syllable start: the buffer is also the keys in the pin's column
/// order, which the key getters read as it stands.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScanMatrix {
    keys: Vec<ScanKey>,
    /// `keys[starts[column]..starts[column + 1]]` is the column; one more
    /// entry than there are columns, or none for the empty matrix.
    starts: Vec<usize>,
}

impl ScanMatrix {
    /// Lays out `columns`, keeping each column's own order.
    pub(crate) fn from_columns(columns: Vec<Vec<ScanKey>>) -> Self {
        let mut starts = Vec::with_capacity(columns.len() + 1);
        starts.push(0);
        let mut keys = Vec::with_capacity(columns.iter().map(Vec::len).sum());
        for column in columns {
            keys.extend(column);
            starts.push(keys.len());
        }
        debug_assert!(keys.is_sorted_by_key(|key| key.syllable_start));
        Self { keys, starts }
    }

    /// Column `column`, or `None` past the last one.
    pub(crate) fn get(&self, column: usize) -> Option<&[ScanKey]> {
        let start = *self.starts.get(column)?;
        let end = *self.starts.get(column + 1)?;
        Some(&self.keys[start..end])
    }

    /// The columns in order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &[ScanKey]> + '_ {
        self.starts
            .windows(2)
            .map(|bounds| &self.keys[bounds[0]..bounds[1]])
    }

    /// Every key, in column order.
    pub(crate) fn keys(&self) -> &[ScanKey] {
        &self.keys
    }
}

/// The one scan-matrix build of a parse and what is read from it: the raw
/// scan columns the sentence, constraint and key-list laws index, and the
/// placed topology the window scan walks. Which facade's topology it is
/// follows [`Session::scan_divided`].
#[derive(Clone, Debug, Default)]
pub(crate) struct RetainedParse {
    pub(crate) scan: ScanMatrix,
    pub(crate) matrix: ParsedMatrix,
}

impl ParsedMatrix {
    pub(crate) fn from_scan(scan: &ScanMatrix, bound: usize, physical: bool) -> Self {
        let mut columns = vec![Vec::new(); bound + 1];
        let mut zeros = vec![false; bound + 1];
        let first = scan.keys().iter().map(|key| key.syllable_start).min();
        let mut last_end = 0;
        for key in scan.iter().flatten().copied() {
            let mut raw = key;
            if physical && key.crosses_separator {
                // A preceding real key is required: fill_matrix never fills
                // before the first key, even for several leading separators.
                if first.is_some_and(|start| key.from >= start) {
                    for zero in zeros
                        .iter_mut()
                        .take(key.syllable_start.min(bound))
                        .skip(key.from)
                    {
                        *zero = true;
                    }
                }
                raw.from = key.syllable_start;
            }
            raw.crosses_separator = false;
            raw.syllable_start = raw.from;
            if let Some(column) = columns.get_mut(raw.from) {
                column.push(raw);
            }
            last_end = last_end.max(raw.to);
        }
        if physical {
            for zero in zeros.iter_mut().take(bound + 1).skip(last_end) {
                *zero = true;
            }
        }
        // fill_matrix always installs its reserved zero-key end slot,
        // including exact-key schemes without physical separators
        // (phonetic_key_matrix.cpp:60-66 at pin 074a2219).
        if first.is_some() {
            zeros[bound] = true;
        }
        Self {
            columns,
            zeros,
            bound,
        }
    }
}

impl<D, L, S: std::borrow::Borrow<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Whether the session's scan matrix carries the resplit and divided
    /// passes. zhuyin.cpp:1017-1040 omits the full-pinyin divided/resplit
    /// transforms pinyin.cpp:1521-1523 applies, and every libzhuyin reader
    /// then reads that one matrix; the collapse mode is the zhuyin facade's.
    pub(super) fn scan_divided(&self) -> bool {
        self.input.full_pinyin() && !self.collapse_sentence_rows_to_best
    }

    /// The selected key path and parse length of the last parse.
    pub(super) fn selected_path(&self) -> &SelectedPath {
        &self.input.parsed.selected
    }

    /// The scan matrix the last parse filled: the key list, span check,
    /// window scan, sentence and constraint laws all read it.
    pub(super) fn retained_parse(&self) -> &RetainedParse {
        &self.input.parsed.retained
    }

    /// The parse of the raw input under the current option word and
    /// collapse mode: the graph is built, read for its selected path and
    /// scan matrix, and dropped, as the pin's parser output is freed once
    /// `fill_matrix` and the matrix steps ran (`pinyin.cpp:1497-1525`).
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the buffer cannot be built into a graph.
    pub(super) fn build_parse(&self) -> Result<Parsed, EngineError> {
        let graph = self.build_graph()?;
        let edges = graph.fewest_keys(self.settings.incomplete());
        let parsed_len =
            apostrophe_extended(self.input.as_bytes(), edges.last().map_or(0, Edge::to));
        // The two facades have different full-pinyin parse orders:
        // pinyin.cpp:1514-1522 includes both split passes; zhuyin.cpp:
        // 1038-1040 runs fill then fuzzy only (pin 074a2219).
        let scan = ScanMatrix::from_columns(build_scan_matrix_over(
            &graph,
            &edges,
            self.settings.options,
            self.scan_divided(),
        ));
        let matrix =
            ParsedMatrix::from_scan(&scan, graph.consumed(), self.input.physical_separators());
        Ok(Parsed {
            selected: SelectedPath { edges, parsed_len },
            retained: RetainedParse { scan, matrix },
        })
    }

    /// The whole input's graph: the exact-mode chain when the session
    /// carries pre-parsed scheme segments, the parsed graph otherwise.
    fn build_graph(&self) -> Result<SegmentGraph, EngineError> {
        let raw = self.input.as_bytes();
        if self.input.exact().is_empty() {
            SegmentGraph::build_with_options(raw, self.settings.options)
        } else {
            SegmentGraph::build_exact(raw, self.input.exact())
        }
        .map_err(EngineError::Graph)
    }
}

impl<D, L, S: std::borrow::BorrowMut<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Parses the raw input and keeps the result: every parse seam calls
    /// this after it changes the input. A failed build leaves the empty
    /// parse.
    ///
    /// # Errors
    ///
    /// As [`Self::build_parse`].
    pub(super) fn reparse(&mut self) -> Result<(), EngineError> {
        match self.build_parse() {
            Ok(parsed) => {
                self.input.parsed = parsed;
                Ok(())
            }
            Err(error) => {
                self.input.parsed = Parsed::default();
                Err(error)
            }
        }
    }
}
