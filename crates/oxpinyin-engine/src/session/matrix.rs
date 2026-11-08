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

/// The one scan-matrix build of a parse and what is read from it: the raw
/// scan columns the sentence, constraint and key-list laws index, and the
/// placed topology the window scan walks. Which facade's topology it is
/// follows [`Session::scan_divided`].
#[derive(Clone, Debug, Default)]
pub(crate) struct RetainedParse {
    pub(crate) scan: Vec<Vec<ScanKey>>,
    pub(crate) matrix: ParsedMatrix,
}

impl ParsedMatrix {
    pub(crate) fn from_scan(scan: &[Vec<ScanKey>], bound: usize, physical: bool) -> Self {
        let mut columns = vec![Vec::new(); bound + 1];
        let mut zeros = vec![false; bound + 1];
        let first = scan.iter().flatten().map(|key| key.syllable_start).min();
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

    /// The whole-input graph's selected path and parse length, computed
    /// once per input and option word.
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the buffer cannot be built into a graph.
    pub(super) fn selected_path(&self) -> Result<&SelectedPath, EngineError> {
        if let Some(selected) = self.input.selected.get() {
            return Ok(selected);
        }
        let graph = self.build_graph_at(0, self.input.as_bytes())?;
        let edges = graph.fewest_keys(self.settings.incomplete());
        let parsed_len =
            apostrophe_extended(self.input.as_bytes(), edges.last().map_or(0, Edge::to));
        Ok(self
            .input
            .selected
            .get_or_init(|| SelectedPath { edges, parsed_len }))
    }

    /// The retained scan matrix, built on first use after an input, option
    /// or collapse-mode change. This is the only scan-matrix build of a
    /// parse: the key list, span check, window scan, sentence and
    /// constraint laws all read it.
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the buffer cannot be built into a graph.
    pub(super) fn retained_parse(&self) -> Result<&RetainedParse, EngineError> {
        if let Some(retained) = self.input.parse.get() {
            return Ok(retained);
        }
        let built = self.build_retained_parse()?;
        Ok(self.input.parse.get_or_init(|| built))
    }

    /// [`Self::retained_parse`]'s build, before it is stored.
    pub(super) fn build_retained_parse(&self) -> Result<RetainedParse, EngineError> {
        let graph = self.build_graph_at(0, self.input.as_bytes())?;
        // The two facades have different full-pinyin parse orders:
        // pinyin.cpp:1514-1522 includes both split passes; zhuyin.cpp:
        // 1038-1040 runs fill then fuzzy only (pin 074a2219).
        let scan = build_scan_matrix_over(
            &graph,
            &self.selected_path()?.edges,
            self.settings.options,
            self.scan_divided(),
        );
        let matrix =
            ParsedMatrix::from_scan(&scan, graph.consumed(), self.input.physical_separators());
        Ok(RetainedParse { scan, matrix })
    }

    /// Builds the retained scan matrix if the parse has none yet.
    ///
    /// # Errors
    ///
    /// As [`Self::retained_parse`].
    pub(super) fn ensure_matrix(&self) -> Result<(), EngineError> {
        self.retained_parse().map(drop)
    }
}
