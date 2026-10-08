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
        Self {
            columns,
            zeros,
            bound,
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
        if self.input.matrix.is_none()
            || (self.collapse_sentence_rows_to_best && self.input.ending_matrix.is_none())
        {
            let graph = self.build_graph_at(0, self.input.as_bytes())?;
            // zhuyin.cpp:1017-1040 omits the full-pinyin divided/resplit
            // transforms applied by pinyin.cpp:1521-1523.
            if self.collapse_sentence_rows_to_best {
                let ending = build_scan_matrix(&graph, self.settings.options, false);
                let mut retained = ParsedMatrix::from_scan(
                    &ending,
                    graph.consumed(),
                    self.input.physical_separators(),
                );
                // Even formatted/exact keys have a live reserved end column
                // for search_matrix (phonetic_key_matrix.cpp:60-66).
                if ending.iter().any(|column| !column.is_empty()) {
                    retained.zeros[graph.consumed()] = true;
                }
                self.input.ending_matrix = Some(retained);
            }
            let scan = build_scan_matrix(&graph, self.settings.options, self.input.full_pinyin());
            self.input.matrix = Some(ParsedMatrix::from_scan(
                &scan,
                graph.consumed(),
                self.input.physical_separators(),
            ));
        }
        Ok(())
    }
}
