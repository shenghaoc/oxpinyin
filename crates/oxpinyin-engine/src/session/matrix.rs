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
    collapse_sentence_rows_to_best: bool,
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
            collapse_sentence_rows_to_best: false,
        }
    }
}

impl<D, L> Session<D, L>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    pub(super) fn ensure_matrix(&mut self) -> Result<(), EngineError> {
        let collapse = self.collapse_sentence_rows_to_best;
        let matches_mode =
            |matrix: &ParsedMatrix| matrix.collapse_sentence_rows_to_best == collapse;
        if !self.input.matrix.as_ref().is_some_and(matches_mode)
            || (collapse && !self.input.ending_matrix.as_ref().is_some_and(matches_mode))
        {
            // The public const setter can change the facade mode without
            // dropping cached allocations. Reject both old topologies at
            // their next use, including the zhuyin -> pinyin transition.
            self.input.matrix = None;
            self.input.ending_matrix = None;
            let graph = self.build_graph_at(0, self.input.as_bytes())?;
            // zhuyin.cpp:1017-1040 omits the full-pinyin divided/resplit
            // transforms applied by pinyin.cpp:1521-1523.
            if collapse {
                let ending = build_scan_matrix(&graph, self.settings.options, false);
                let mut retained = ParsedMatrix::from_scan(
                    &ending,
                    graph.consumed(),
                    self.input.physical_separators(),
                );
                retained.collapse_sentence_rows_to_best = collapse;
                self.input.ending_matrix = Some(retained);
            }
            // The two facades have different full-pinyin parse orders:
            // pinyin.cpp:1514-1522 includes both split passes; zhuyin.cpp:
            // 1038-1040 runs fill then fuzzy only (pin 074a2219).
            let scan = build_scan_matrix(
                &graph,
                self.settings.options,
                self.input.full_pinyin() && !collapse,
            );
            let mut retained =
                ParsedMatrix::from_scan(&scan, graph.consumed(), self.input.physical_separators());
            retained.collapse_sentence_rows_to_best = collapse;
            self.input.matrix = Some(retained);
        }
        Ok(())
    }
}
