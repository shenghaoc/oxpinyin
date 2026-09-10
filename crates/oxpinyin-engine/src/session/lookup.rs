//! Candidate lookup: the expanding-window scan at the composition offset or any lookup offset, the backward-anchored before-cursor window, the n-best prepend, and the DP that pools phrases into sentences.
//!
//! One of the `impl Session` slices `session/mod.rs` declares; every
//! method here was moved verbatim from the single 5,800-line
//! `session.rs` (2026-09-08) and keeps its doc comment and upstream
//! citations. Shared state, constants and free functions stay in the
//! parent module.

use super::*;

impl<D, L> Session<D, L>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: Display,
{
    /// Recomputes the candidate list for the current state.
    ///
    /// Parse into a graph and run libpinyin's expanding-window scan over it:
    /// every key-path through the parser-shaped key set, searched against the
    /// phrase table window by window. Cross-segmentation pooling falls out of
    /// the scan — `xian` offers `西安` (`xi` + `an`) alongside the single key
    /// `xian`, and `fangan` mixes `方案` (`fang` + `an`) with `反感`
    /// (`fan` + `gan`) — without a separate pooling step.
    ///
    /// Under the pinned observation surface the pin emits no sentence-level
    /// candidates at all — its `nihaoshi` list never contains `你好是`, the
    /// best sentence the DP over the segment lattice would produce — so the
    /// pooled phrase candidates are ranked by the three-key order and
    /// deduplicated directly, with no sentence prepend.
    pub(super) fn refresh(&mut self) -> Result<(), EngineError> {
        // The cached list is anchored at the composition offset the session
        // owns. Reuse its buffer so the scan keeps its capacity across
        // keystrokes.
        let anchor = self.record.consumed();
        let mut items = Vec::new();
        self.lookup.candidates.swap_items(&mut items);
        self.lookup.parsed_prefix = self.scan_window(anchor, &mut items)?;
        self.lookup.candidates.swap_items(&mut items);
        Ok(())
    }

    /// Builds the candidate window anchored at byte `anchor` in the raw
    /// buffer into `out`, returning the filtered parse length of the
    /// remaining slice from `anchor`.
    ///
    /// The window is a pure function of `(raw, anchor, constraint-derived
    /// state)`: the scan reads `&self.input.as_str()[anchor..]` and the
    /// stored n-best rows prepend the same way regardless of the anchor. It
    /// mutates only the scan scratch and `out` — never the composition
    /// offset, the constraint store, or the history — so a caller may build
    /// a window at a lookup offset without disturbing the cached list
    /// ([`Session::candidates_at`]). With `anchor == self.record.consumed()`
    /// it reproduces `Session::refresh`'s cached list exactly.
    /// Builds the working graph for `remaining` (the raw slice from
    /// `anchor`): the exact-mode chain when the session carries
    /// pre-parsed scheme segments, the parsed graph otherwise. A segment
    /// that straddles `anchor` is dropped, not truncated — a mid-key
    /// anchor is the caller's boundary question, and exact keys do not
    /// re-syllabify around it.
    pub(super) fn build_graph_at(
        &self,
        anchor: usize,
        remaining: &[u8],
    ) -> Result<SegmentGraph, EngineError> {
        if self.input.exact().is_empty() {
            return SegmentGraph::build_with_options(remaining, self.settings.options)
                .map_err(EngineError::Graph);
        }
        // An anchor strictly inside an exact segment must not decode the
        // tail segments across the skipped bytes — that would consume
        // input the anchor excluded (exact `xian'hao` anchored at 2
        // decoding `hao` over `an'hao`). Refuse instead: an empty exact
        // graph answers no candidates and a zero parse for this anchor.
        if self
            .input
            .exact()
            .iter()
            .any(|segment| segment.start() < anchor && anchor < segment.end())
        {
            return SegmentGraph::build_exact(remaining, &[]).map_err(EngineError::Graph);
        }
        let rebased: Vec<ExactSegment> = self
            .input
            .exact()
            .iter()
            .copied()
            .filter(|segment| segment.start() >= anchor)
            .map(|segment| {
                ExactSegment::new(
                    segment.start() - anchor,
                    segment.end() - anchor,
                    segment.key(),
                    segment.tone(),
                )
            })
            .collect();
        SegmentGraph::build_exact(remaining, &rebased).map_err(EngineError::Graph)
    }

    pub(super) fn scan_window(
        &mut self,
        anchor: usize,
        out: &mut Vec<Candidate>,
    ) -> Result<usize, EngineError> {
        out.clear();
        if anchor >= self.input.len() {
            // A fully-consumed (or past-end) anchor still carries its
            // sentence rows — upstream's window prepends `m_nbest_results`
            // whether or not any phrase candidate remains at the cursor (the
            // L1 terminal-choose surface).
            self.prepend_nbest_rows(out);
            return Ok(0);
        }

        // Lift the scratch out before borrowing `raw`, so graph/scan can
        // use `&self.input.as_str()[anchor..]` without cloning into a
        // CompactString. Destructured into owned locals so the scan body
        // below reads exactly as before; reassembled and handed back at the
        // end.
        let Scratch {
            mut collected,
            mut ranked,
            mut entries,
            mut path,
            mut window_phrase,
            mut window_addon,
        } = core::mem::take(&mut self.scratch);
        collected.clear();

        let remaining = &self.input.as_str()[anchor..];
        let graph = self.build_graph_at(anchor, remaining.as_bytes())?;
        // The trailing-run extension of `full_parsed_len`, applied to the
        // remaining slice (the pin's propagation runs on every parse).
        let parsed_prefix = apostrophe_extended(
            remaining.as_bytes(),
            graph
                .fewest_keys(self.settings.incomplete())
                .last()
                .map_or(0, Edge::to),
        );

        // When the model carries the phrase index's real unigram
        // frequencies, the pinned construction runs — the expanding-window
        // scan, the three-key order (text length, pinyin span, frequency),
        // keep-first dedup, and only the raw-input fallback after an empty
        // result. Without real frequencies the session reproduces its
        // pre-frequency behaviour exactly: k-best prefixes, sentence
        // candidates, cost order, adjacent dedup.
        if self.model.has_real_unigrams() {
            {
                let mut scratch = ScanScratch {
                    path: &mut path,
                    entries: &mut entries,
                    window_phrase: &mut window_phrase,
                    window_addon: &mut window_addon,
                };
                self.collect_window_scan(
                    &graph,
                    remaining.as_bytes(),
                    self.settings.options,
                    &mut collected,
                    &mut scratch,
                )?;
            }

            // Upstream's Gates 1 and 2 (`pinyin.cpp:2200-2214`), hoisted
            // out of the candidate loop exactly as the pin hoists them: the
            // previous token is resolved once, and the system and user grams
            // are merged once, for the whole guess. Indexing that row per
            // candidate is Gate 3.
            let gram = self.dynamic_adjust_gram(anchor)?;
            // The scan's result stands even when it found nothing. Tokens the
            // table lacks rank as zero rather than falling back.
            let frequencies = self
                .candidate_frequencies(&collected, gram.as_ref())?
                .unwrap_or_else(|| vec![0; collected.len()]);
            ranked.clear();
            ranked.extend(
                collected
                    .drain(..)
                    .zip(frequencies)
                    .map(|(candidate, frequency)| {
                        let key = RankKey {
                            phrase_length: candidate.text().chars().count(),
                            pinyin_span: candidate.consumed_bytes(),
                            frequency,
                        };
                        (key, candidate)
                    }),
            );

            // Stable sort, all three keys descending: an all-equal tie keeps
            // the collection order, which the scan now lays down in the
            // pin's array order (per window, token-ascending).
            ranked.sort_by_key(|(key, _)| core::cmp::Reverse(*key));
            collected.extend(ranked.drain(..).map(|(_, candidate)| candidate));

            dedup_by_text_keep_first(&mut collected);
        } else {
            let scorer = Scorer::with_key_costs(
                self.scoring,
                &self.dictionary,
                &self.model,
                self.key_costs.clone(),
            );
            let paths = k_best(&graph, &scorer, SEGMENTATION_K)?;
            for path in &paths {
                self.collect_prefix_phrases(&graph, &scorer, path, &mut collected)?;
                self.collect_sentence(&graph, &scorer, path, &mut collected)?;
            }
            collected.sort_by_key(Candidate::cost);
            collected.dedup_by(|left, right| left.text() == right.text());
        }

        if collected.is_empty() {
            collected.push(Candidate::new(
                compact_str::CompactString::from(remaining),
                CandidateKind::Fallback,
                0,
                remaining.len(),
                0,
                None,
                None,
            ));
        }

        // W14: prepend the stored n-best rows, head first, then drop every
        // later candidate with the same text — upstream prepends after the
        // sort and its phrase-string dedup keeps the NBEST row (and the
        // lower n-best index) over any phrase candidate with the same
        // string (`pinyin.cpp:2290-2298`, `2058-2126`).
        self.prepend_nbest_rows(&mut collected);

        core::mem::swap(out, &mut collected);
        collected.clear();
        self.scratch = Scratch {
            collected,
            ranked,
            entries,
            path,
            window_phrase,
            window_addon,
        };
        Ok(parsed_prefix)
    }

    /// Rebuilds the candidate window anchored at a caller lookup `offset`,
    /// mirroring the pin's per-offset span search — `pinyin_guess_candidates`
    /// re-runs `search_matrix` from `start = offset` (`pinyin.cpp:2224-2262`),
    /// its candidates all beginning at `offset` — and returns it without
    /// disturbing the cached list or any composition state (constraints,
    /// consumed, history). The C ABI uses this only when the caller's
    /// normalized lookup offset differs from [`Session::composition_offset`]:
    /// a mid-composition cursor with no prior choose. At an equal offset the
    /// cached [`Session::candidates`] already answers, so offset-0 and every
    /// post-choose lookup stay bit-identical.
    ///
    /// A byte no matrix key starts on — a mid-syllable position of the
    /// composition's own parse — is one of the pin's empty columns:
    /// `search_matrix` matches nothing from it for every end, so the
    /// window there is the raw-suffix fallback under the prepended n-best
    /// rows, never a re-parse of the suffix (the suffix's own keys are not
    /// the matrix's). A zero-key (apostrophe) column inside the parse is
    /// not empty: the span search steps over it to the next key, so that
    /// apostrophe byte answers the following key's window. An apostrophe
    /// past a stop byte sits outside the matrix — the pin aborts there —
    /// and takes the empty-column window like any other unreachable byte.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when a backend fails during the scan, exactly
    /// as the anchored `Session::refresh` does;
    /// [`EngineError::LookupOffsetOutOfRange`] when `offset` exceeds the raw
    /// buffer's one-past-end position — the pin reads its matrix out of
    /// bounds there, so no pinned behaviour exists and the offset is
    /// refused — and [`EngineError::LookupOffsetInsideCharacter`] when
    /// `offset` falls inside a multi-byte character of the raw buffer (no
    /// window exists under a mid-character slice). An offset equal to
    /// one-past-end is valid: `scan_window` answers the terminal sentence
    /// rows for it (the pin's reserved slot).
    pub fn candidates_at(&mut self, offset: usize) -> Result<CandidateList, EngineError> {
        if offset > self.input.len() {
            return Err(EngineError::LookupOffsetOutOfRange {
                offset,
                len: self.input.len(),
            });
        }
        if !self.input.is_char_boundary(offset) {
            return Err(EngineError::LookupOffsetInsideCharacter {
                offset,
                len: self.input.len(),
            });
        }
        let mut items = Vec::new();
        if offset < self.input.len() && !self.spans_a_matrix_key(offset)? {
            items.push(Candidate::new(
                compact_str::CompactString::from(&self.input.as_str()[offset..]),
                CandidateKind::Fallback,
                0,
                self.input.len() - offset,
                0,
                None,
                None,
            ));
            self.prepend_nbest_rows(&mut items);
            return Ok(CandidateList::from_vec(items));
        }
        self.scan_window(offset, &mut items)?;
        Ok(CandidateList::from_vec(items))
    }

    /// Builds the candidate window over spans ENDING at byte `offset` in the
    /// raw buffer — the pin's before-cursor walk
    /// (`zhuyin_guess_candidates_before_cursor`, `zhuyin.cpp:1542-1629` at
    /// the pin 0c5e80e1): every key-path from each start `0..offset` to the
    /// fixed end, enumerated longest-span-first (start ascending — the pin's
    /// `len` loop), each span's slice ranked by the three-key order exactly
    /// like the after-cursor windows, the stored n-best rows prepended over
    /// the whole set, then text dedup keep-first.
    ///
    /// Measured decomposition on the pin (su3u3, `before(5)`, the
    /// instrumented oracle at 0c5e80e1): span (0,5) yields 3 phrase
    /// candidates, span (3,5) yields 597, the mid-syllable starts (1, 2, 4)
    /// answer no match (empty columns — `search_matrix` returns
    /// `SEARCH_NONE` before recursing), 600 phrases total, +1 sentence row,
    /// −1 string-duplicate phrase → 600. The builder reproduces that walk: a
    /// start with no matrix column (a mid-syllable byte) contributes nothing
    /// naturally (no key starts there), and a span ending on an apostrophe
    /// separator byte answers nothing (upstream's zero-key end column:
    /// `search_matrix` returns `SEARCH_CONTINUED` with no items,
    /// `phonetic_key_matrix.cpp:419-423`).
    ///
    /// Does not disturb the cached list or any composition state (the
    /// [`Session::candidates_at`] contract). At offset 0 nothing precedes
    /// the first key, so the window is the prepended sentence rows alone.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError::LookupOffsetOutOfRange`] when `offset` exceeds
    /// the raw buffer's one-past-end position, and
    /// [`EngineError::LookupOffsetInsideCharacter`] when it falls inside a
    /// multi-byte character — the same refusals as
    /// [`Session::candidates_at`] — plus backend failures during the scan.
    pub fn candidates_ending_at(&mut self, offset: usize) -> Result<CandidateList, EngineError> {
        if offset > self.input.len() {
            return Err(EngineError::LookupOffsetOutOfRange {
                offset,
                len: self.input.len(),
            });
        }
        if !self.input.is_char_boundary(offset) {
            return Err(EngineError::LookupOffsetInsideCharacter {
                offset,
                len: self.input.len(),
            });
        }
        let mut items = Vec::new();
        // A span cannot end at the composition start, and an end on an
        // apostrophe separator byte is upstream's empty end column: the
        // window is the prepended sentence rows alone.
        if offset == 0 || self.input.as_bytes().get(offset - 1) == Some(&b'\'') {
            self.prepend_nbest_rows(&mut items);
            return Ok(CandidateList::from_vec(items));
        }
        self.scan_window_ending(offset, &mut items)?;
        Ok(CandidateList::from_vec(items))
    }

    /// The prefix graph the backward-anchored scan walks: the raw input cut
    /// to `offset`, with only the exact segments fully inside the cut. A
    /// key crossing the lookup boundary cannot end within the prefix —
    /// upstream's matrix keeps such a key but `search_matrix` can only
    /// report it as overhang (`SEARCH_CONTINUED`), never as a match for
    /// this end — so dropping it changes no candidate, and keeping it
    /// (beyond the cut) would poison the graph's own bound. Exact-segment
    /// coordinates are absolute from the buffer start, so no rebasing is
    /// needed at anchor 0.
    pub(super) fn build_prefix_graph(&self, offset: usize) -> Result<SegmentGraph, EngineError> {
        let remaining = &self.input.as_bytes()[..offset];
        if self.input.exact().is_empty() {
            return SegmentGraph::build_with_options(remaining, self.settings.options)
                .map_err(EngineError::Graph);
        }
        let rebased: Vec<ExactSegment> = self
            .input
            .exact()
            .iter()
            .copied()
            .filter(|segment| segment.end() <= offset)
            .collect();
        SegmentGraph::build_exact(remaining, &rebased).map_err(EngineError::Graph)
    }

    /// The backward-anchored scan proper: the prefix graph's matrix, every
    /// live start enumerated ascending (the pin's longest span first), each
    /// span's batch flushed and ranked with its own previous-token gram (the
    /// pin resolves `_get_previous_token` per `len` slice), groups appended
    /// in order, then one text dedup — the pin dedups once, after the
    /// prepend, and [`Session::prepend_nbest_rows`] re-runs it over the
    /// joined list.
    ///
    /// Returns the filtered parse length of the prefix slice, mirroring
    /// [`Session::scan_window`]'s return.
    pub(super) fn scan_window_ending(
        &mut self,
        offset: usize,
        out: &mut Vec<Candidate>,
    ) -> Result<usize, EngineError> {
        out.clear();
        let graph = self.build_prefix_graph(offset)?;
        let matrix =
            build_scan_matrix(&graph, self.settings.options, self.input.exact().is_empty());
        let bound = graph.consumed().min(offset);

        let Scratch {
            mut collected,
            mut ranked,
            mut entries,
            mut path,
            mut window_phrase,
            mut window_addon,
        } = core::mem::take(&mut self.scratch);
        collected.clear();
        let mut group: Vec<Candidate> = Vec::new();

        for start in 0..bound {
            // An empty column — no key starts here — is the pin's
            // `SEARCH_NONE` start (`search_matrix`,
            // `phonetic_key_matrix.cpp:416-418`): the span contributes
            // nothing and the walk skips it.
            if matrix.get(start).is_none_or(|column| column.is_empty()) {
                continue;
            }
            let mut continued = false;
            {
                let mut buf = ScanBuf {
                    path: &mut path,
                    system: &mut window_phrase,
                    addon: &mut window_addon,
                    continued: &mut continued,
                    entries: &mut entries,
                };
                self.scan_paths(&matrix, start, offset, &mut buf)?;
            }
            group.clear();
            flush_window_batch(&mut window_phrase, &mut group);
            flush_window_batch(&mut window_addon, &mut group);
            if group.is_empty() {
                continue;
            }
            // Every row of this slice spans `[start, offset)` — the pin's
            // `template_item.m_begin = start; m_end = offset`
            // (`zhuyin.cpp:1595`). The prefix graph's coordinates are
            // absolute, so the start is recorded as such; the end is the
            // row's `consumed_bytes` already.
            for candidate in &mut group {
                candidate.set_span_start(start);
            }
            // The pin ranks each `len` slice on its own, with the previous
            // token resolved at that slice's start.
            let gram = self.dynamic_adjust_gram(start)?;
            let frequencies = self
                .candidate_frequencies(&group, gram.as_ref())?
                .unwrap_or_else(|| vec![0; group.len()]);
            ranked.clear();
            ranked.extend(
                group
                    .drain(..)
                    .zip(frequencies)
                    .map(|(candidate, frequency)| {
                        let key = RankKey {
                            phrase_length: candidate.text().chars().count(),
                            pinyin_span: candidate.consumed_bytes(),
                            frequency,
                        };
                        (key, candidate)
                    }),
            );
            ranked.sort_by_key(|(key, _)| core::cmp::Reverse(*key));
            group.extend(ranked.drain(..).map(|(_, candidate)| candidate));
            collected.append(&mut group);
        }

        out.append(&mut collected);

        self.scratch = Scratch {
            collected,
            ranked,
            entries,
            path,
            window_phrase,
            window_addon,
        };

        dedup_by_text_keep_first(out);
        self.prepend_nbest_rows(out);
        Ok(bound)
    }

    /// Whether `offset` names a column the pin's span search can answer.
    ///
    /// The pin's matrix holds the chosen parse's keys at their raw begins
    /// (`fill_matrix`), plus the split keys `resplit_step` and
    /// `inner_split_step` append (`docs/findings/matrix-split-tables.md`) —
    /// `jie` in `nihaoshijie` also carries `ji` + `e`, so byte 10 is a live
    /// column — and a zero key at every apostrophe the parse reached, which
    /// the span search steps over to the following key. The matrix ends at
    /// the parse: an apostrophe past a stop byte sits outside it entirely
    /// (the pin aborts there — `ni,'hao@3`, measured SIGABRT — and the
    /// empty-column window is the no-abort answer). The scan's own key set
    /// ([`build_scan_matrix`]) models the key columns, so a byte some key's
    /// syllable starts on, or an in-span apostrophe byte, answers; any other
    /// byte is an empty column. Exact mode has no pin counterpart, so its
    /// columns stay the exact segments, and its inputs' apostrophes are all
    /// in-span by construction (`build_exact` rejects any other gap).
    ///
    /// # Errors
    ///
    /// [`EngineError::Graph`] when the composition cannot be represented
    /// as a segment graph.
    pub(super) fn spans_a_matrix_key(&self, offset: usize) -> Result<bool, EngineError> {
        if !self.input.exact().is_empty() {
            return Ok(self.input.as_bytes().get(offset) == Some(&b'\'')
                || self
                    .input
                    .exact()
                    .iter()
                    .any(|segment| segment.start() == offset));
        }
        let graph = SegmentGraph::build_with_options(self.input.as_bytes(), self.settings.options)
            .map_err(EngineError::Graph)?;
        // The split alternates are a full-pinyin-parse artifact — the same
        // law the scan applies (`build_scan_matrix`'s `divided` argument at
        // the anchored call site) — and exact keys never gain them.
        let matrix = build_scan_matrix(&graph, self.settings.options, true);
        if matrix
            .iter()
            .flatten()
            .any(|key| key.syllable_start == offset)
        {
            return Ok(true);
        }
        Ok(offset < graph.consumed() && self.input.as_bytes().get(offset) == Some(&b'\''))
    }

    /// Prepends the stored n-best rows onto `collected`, head first, then
    /// drops every later candidate with the same text — upstream prepends
    /// after the sort and its phrase-string dedup keeps the NBEST row
    /// (and the lower n-best index) over any phrase candidate with the
    /// same string (`pinyin.cpp:2290-2298`, `2058-2126`). Extend-then-
    /// rotate keeps `collected`'s allocation — the session scratch on the
    /// scan path, a fresh small vec on the fully-consumed path.
    ///
    /// Under [`Session::set_collapse_sentence_rows_to_best`] only the 1-best
    /// row is prepended — libzhuyin's display law.
    pub(super) fn prepend_nbest_rows(&mut self, collected: &mut Vec<Candidate>) {
        if self.sentence.rows.is_empty() {
            return;
        }
        let rows = if self.collapse_sentence_rows_to_best {
            &self.sentence.rows[..1]
        } else {
            &self.sentence.rows[..]
        };
        let nbest_n = rows.len();
        collected.extend(rows.iter().enumerate().map(|(index, row)| {
            Candidate::new(
                row.text.clone(),
                CandidateKind::Sentence,
                row.keys,
                row.span,
                row.cost,
                None,
                Some(u8::try_from(index).unwrap_or(u8::MAX)),
            )
        }));
        collected.rotate_right(nbest_n);
        dedup_by_text_keep_first(collected);
    }

    /// The previous token at `offset`, as upstream's `_get_previous_token`
    /// resolves it (`pinyin.cpp:1711-1767`).
    ///
    /// At offset 0 upstream answers `sentence_start` and then prefers the
    /// longest token in `m_prefixes`. `m_prefixes` is populated only by
    /// `pinyin_guess_sentence_with_prefix`, which neither reference consumer
    /// calls, so the drop-in surface always takes the `sentence_start`
    /// answer there.
    ///
    /// Above 0 upstream reads the 1-best result — `last_result` here — and
    /// carries a guard worth reproducing: it inspects `result[offset]` FIRST
    /// and only walks backwards when that position holds a token. A guess at
    /// an offset no phrase starts at contributes no bigram term at all.
    pub(super) fn previous_token(&self, offset: usize) -> Option<PhraseToken> {
        if offset == 0 {
            return Some(PhraseToken::new(crate::nbest::SENTENCE_START));
        }
        if self.sentence.last_result.is_empty() {
            return None;
        }
        // `result[offset] != null_token`: a phrase must begin here.
        self.sentence
            .last_result
            .iter()
            .any(|span| span.start == offset)
            .then(|| {
                self.sentence
                    .last_result
                    .iter()
                    .filter(|span| span.start < offset)
                    .max_by_key(|span| span.start)
                    .map(|span| span.token)
            })
            .flatten()
    }

    /// Upstream's Gates 1 and 2 as one call: resolve the previous token and
    /// merge its system and user grams, ONCE per candidate guess.
    ///
    /// `None` whenever upstream would skip the merge — the bit is clear, no
    /// previous token, or the model carries no row for it — and the caller
    /// then contributes no bigram term.
    pub(super) fn dynamic_adjust_gram(
        &self,
        offset: usize,
    ) -> Result<Option<MergedGram>, EngineError> {
        if !self.settings.options.has_dynamic_adjust() {
            return Ok(None);
        }
        let Some(prev) = self.previous_token(offset) else {
            return Ok(None);
        };
        self.model
            .merged_successors(&prev)
            .map_err(|error| EngineError::Scoring(ScoringError::LanguageModel(error.to_string())))
    }

    /// Per-candidate sort frequencies on the pin's amplified scale, or
    /// `None` when the model carries no real frequency table at all.
    ///
    /// The pinned oracle does not compare raw unigram counts: it truncates
    /// the f32 possibility `(1−λ)·unigram/total` amplified by 2²⁴ into a
    /// `guint32` (`_compute_frequency_of_items`, `pinyin.cpp:1855-1866`).
    /// `gram` is the row merged once for this guess: `None`, or a row that
    /// misses the token, contributes a bigram possibility of exactly `0.0`
    /// and leaves the amplified value bit-identical to the unigram-only
    /// law. Near-ties collapse
    /// to equal keys under that truncation — the tie class
    /// `docs/testing/corpus-tail.md` calls Class A — and equal keys fall to
    /// the collection order the stable sort keeps. `amplified_frequency`
    /// reproduces the arithmetic bit-for-bit over the model's own numbers:
    /// the item's stored unigram (`get_unigram_frequency`, `gen_unigram`'s
    /// +1 included — a phrase the corpus never saw is 1, never 0) over the
    /// facade total (`get_phrase_index_total_freq`, Σ item), exactly the
    /// two reads `_compute_frequency_of_items` performs.
    ///
    /// Only the first `Some` switches the construction on, so a model that
    /// mixes per-token answers degrades deterministically (missing tokens
    /// rank as zero).
    pub(super) fn candidate_frequencies(
        &self,
        collected: &[Candidate],
        gram: Option<&MergedGram>,
    ) -> Result<Option<Vec<u64>>, EngineError> {
        let mut frequencies: Option<Vec<u64>> = None;
        // The facade total, `get_phrase_index_total_freq()`: the sum of
        // every item's stored unigram (`gen_unigram`'s +1 included).
        let default_total = self
            .model
            .unigram_total()
            .map_err(|error| EngineError::Scoring(ScoringError::LanguageModel(error.to_string())))?
            .unwrap_or(0);
        let addon_total = self
            .model
            .addon_unigram_total()
            .map_err(|error| EngineError::Scoring(ScoringError::LanguageModel(error.to_string())))?
            .unwrap_or(0);
        for (index, candidate) in collected.iter().enumerate() {
            let Some(token) = candidate.token() else {
                continue;
            };
            let count = if candidate.kind() == CandidateKind::Addon {
                // The addon facade's own amplified scale (`pinyin.cpp:1829-
                // 1843`): no `+1`, the addon index's items carry their own
                // unigrams. An empty facade has no items and no candidates.
                let raw = self
                    .model
                    .addon_unigram_freq(&token)
                    .map_err(|error| {
                        EngineError::Scoring(ScoringError::LanguageModel(error.to_string()))
                    })?
                    .unwrap_or(0);
                Some(amplified_frequency(raw, addon_total))
            } else {
                self.model
                    .unigram_freq(&token)
                    .map_err(|error| {
                        EngineError::Scoring(ScoringError::LanguageModel(error.to_string()))
                    })?
                    .map(|count| {
                        // Upstream's Gate 3: the bigram possibility joins the
                        // unigram term INSIDE the pin's expression, before
                        // its single truncation. The addon and predicted
                        // branches above return early in the pin too — they
                        // carry no bigram term at all.
                        let bigram = dynamic_adjust_bigram_possibility(
                            self.settings.options,
                            gram,
                            token.value(),
                        );
                        amplified_frequency_with_bigram(count, default_total, bigram)
                    })
            };
            if let Some(count) = count {
                let table = frequencies.get_or_insert_with(|| vec![0; collected.len()]);
                // Unigram term of candidate frequency: always on. Upstream
                // reads FacadePhraseIndex unigrams (including trained user
                // counts) with no DYNAMIC_ADJUST check. W6-T4's overlay is
                // that unigram term and stays for both bit states.
                table[index] = count;
            }
        }
        Ok(frequencies)
    }

    /// Offers every phrase spelling a prefix of `path`.
    ///
    /// Only the pre-frequency fallback uses this: the pinned construction
    /// collects through the window scan instead. Kept verbatim so a missing
    /// model cache reproduces the prior behaviour exactly.
    pub(super) fn collect_prefix_phrases(
        &self,
        graph: &SegmentGraph,
        scorer: &Scorer<'_, D, L>,
        path: &DecodedPath,
        into: &mut Vec<Candidate>,
    ) -> Result<(), EngineError> {
        let (keys, kinds, ends) = self.walk(graph, path);

        // A dictionary phrase never spans more than MAX_PHRASE_KEYS keys, so
        // looking further is both pointless and quadratic in the input.
        for length in 1..=keys.len().min(MAX_PHRASE_KEYS) {
            let ranked =
                scorer.rank_phrases(self.record.history(), &keys[..length], &kinds[..length])?;
            for (entry, cost) in ranked {
                let token = entry.token();
                into.push(Candidate::new(
                    entry.into_text(),
                    CandidateKind::Phrase,
                    length,
                    ends[length - 1],
                    cost,
                    Some(token),
                    None,
                ));
            }
        }
        Ok(())
    }

    /// Offers the cheapest sequence of phrases covering the whole of `path`.
    ///
    /// Only the pre-frequency fallback uses this: when the model carries no
    /// real unigram table the session reproduces its prior behaviour exactly,
    /// sentence candidates included. The real-frequency construction emits
    /// pooled phrase candidates only until [`Session::guess_sentence`] runs.
    pub(super) fn collect_sentence(
        &self,
        graph: &SegmentGraph,
        scorer: &Scorer<'_, D, L>,
        path: &DecodedPath,
        into: &mut Vec<Candidate>,
    ) -> Result<(), EngineError> {
        for (candidate, _) in self.collect_sentences_with_tokens(graph, scorer, path)? {
            into.push(candidate);
        }
        Ok(())
    }

    /// [`Self::collect_sentence`] with each sentence's token path, which a
    /// chosen fallback row records like a trellis row does.
    pub(super) fn collect_sentences_with_tokens(
        &self,
        graph: &SegmentGraph,
        scorer: &Scorer<'_, D, L>,
        path: &DecodedPath,
    ) -> Result<Vec<(Candidate, Vec<PhraseToken>)>, EngineError> {
        let (keys, kinds, ends) = self.walk(graph, path);
        if keys.is_empty() {
            return Ok(Vec::new());
        }

        // best[i] is the cheapest way to spell keys[..i].
        let mut best: Vec<Option<(Cost, String, Vec<PhraseToken>)>> = vec![None; keys.len() + 1];
        best[0] = Some((0, String::new(), self.record.history().to_vec()));

        for end in 1..=keys.len() {
            let first = end.saturating_sub(MAX_PHRASE_KEYS);
            // The cell being written (`best[end]`) sits strictly after every
            // cell read in this round (`best[start]`, `start < end`), so the
            // table splits into a read half and a write slot; the prefix is
            // borrowed, not cloned, and only an improving span allocates.
            let (settled, open) = best.split_at_mut(end);
            let cell = &mut open[0];
            for (start, prefix) in settled.iter().enumerate().skip(first) {
                let Some((prefix_cost, prefix_text, prefix_history)) = prefix.as_ref() else {
                    continue;
                };
                let ranked =
                    scorer.rank_phrases(prefix_history, &keys[start..end], &kinds[start..end])?;
                let Some((entry, cost)) = ranked.first() else {
                    continue;
                };

                let total = prefix_cost.saturating_add(*cost);
                if cell.as_ref().is_none_or(|(seen, ..)| total < *seen) {
                    let mut text = String::with_capacity(prefix_text.len() + entry.text().len());
                    text.push_str(prefix_text);
                    text.push_str(entry.text());
                    let mut history = Vec::with_capacity(prefix_history.len() + 1);
                    history.extend_from_slice(prefix_history);
                    history.push(entry.token());
                    *cell = Some((total, text, history));
                }
            }
        }

        if let Some((cost, text, tokens)) = best.pop().flatten()
            && !text.is_empty()
        {
            let tokens = tokens[self.record.history().len()..].to_vec();
            return Ok(vec![(
                Candidate::new(
                    text,
                    CandidateKind::Sentence,
                    keys.len(),
                    ends[keys.len() - 1],
                    cost,
                    None,
                    None,
                ),
                tokens,
            )]);
        }
        Ok(Vec::new())
    }

    /// The keys, edge kinds and end offsets along one decoded path.
    ///
    /// An `Incomplete` edge is dropped when the configuration turns
    /// initial-only keys off, which is the `PINYIN_INCOMPLETE` bit the parity
    /// profile sets.
    pub(super) fn walk(
        &self,
        graph: &SegmentGraph,
        path: &DecodedPath,
    ) -> (Vec<SyllableKey>, Vec<EdgeKind>, Vec<usize>) {
        let mut keys = Vec::with_capacity(path.len());
        let mut kinds = Vec::with_capacity(path.len());
        let mut ends = Vec::with_capacity(path.len());

        for id in path.edges() {
            let Some(edge) = graph.edge(*id) else {
                continue;
            };
            if !self.settings.incomplete() && edge.kind() == EdgeKind::Incomplete {
                break;
            }
            keys.push(edge.key());
            kinds.push(edge.kind());
            ends.push(edge.to());
        }
        (keys, kinds, ends)
    }

    /// The expanding-window scan of the pinned candidate collection.
    ///
    /// Start is fixed at the composition offset (byte 0 of the remaining
    /// input); `end` walks outward over every byte position the graph
    /// reaches. At each `[start, end)` window every key-path through the scan
    /// matrix — the selected parse plus the resplit/divided additions,
    /// `docs/findings/matrix-split-tables.md` — is enumerated and the phrase
    /// table is searched on the accumulated sequence; initial-only keys expand
    /// through [`expand_keys`]. Every phrase found is appended with its
    /// `[start, end)` span.
    ///
    /// Widening is prefix-driven: a window whose sequences cannot extend to
    /// any stored phrase stops the scan (the pin's continued-search probe,
    /// [`crate::Dictionary::phrase_prefix_exists`]). Consecutive apostrophe
    /// bytes after a searched window are skipped so the next window does not
    /// repeat the same key sequence.
    pub(super) fn collect_window_scan(
        &self,
        graph: &SegmentGraph,
        input: &[u8],
        options: OptionBits,
        into: &mut Vec<Candidate>,
        scratch: &mut ScanScratch<'_>,
    ) -> Result<(), EngineError> {
        let matrix = build_scan_matrix(graph, options, self.input.exact().is_empty());
        let bound = graph.consumed();
        let mut end = 1usize;
        while end <= bound {
            // An end position no key starts at is an empty column: widen.
            let mut continued = matrix.get(end).is_none_or(std::vec::Vec::is_empty);
            scratch.path.clear();
            scratch.window_phrase.clear();
            scratch.window_addon.clear();
            {
                let mut buf = ScanBuf {
                    path: scratch.path,
                    system: scratch.window_phrase,
                    addon: scratch.window_addon,
                    continued: &mut continued,
                    entries: scratch.entries,
                };
                self.scan_paths(&matrix, 0, end, &mut buf)?;
            }
            // Flush the window in the pin's array order: the default
            // facade's tokens ascending, then the addon facade's — the
            // order `_append_items` lays down and the stable sort keeps
            // for comparator ties.
            flush_window_batch(scratch.window_phrase, into);
            flush_window_batch(scratch.window_addon, into);
            if !continued {
                break;
            }
            end += 1;
            // Skip windows that would only cross an apostrophe separator: they
            // repeat the previous key sequence.
            while end <= bound && input.get(end - 1) == Some(&b'\'') {
                end += 1;
            }
        }
        Ok(())
    }

    /// Enumerates every key-path from `node` to `end` and searches the table on
    /// each complete path.
    pub(super) fn scan_paths(
        &self,
        matrix: &[Vec<ScanKey>],
        node: usize,
        end: usize,
        buf: &mut ScanBuf<'_>,
    ) -> Result<(), EngineError> {
        let Some(column) = matrix.get(node) else {
            return Ok(());
        };
        for scan_key in column.iter().copied() {
            self.visit_scan_key(matrix, scan_key, end, buf)?;
        }
        Ok(())
    }

    /// One matrix key during the scan.
    pub(super) fn visit_scan_key(
        &self,
        matrix: &[Vec<ScanKey>],
        scan_key: ScanKey,
        end: usize,
        buf: &mut ScanBuf<'_>,
    ) -> Result<(), EngineError> {
        let to = scan_key.to;
        if to > end {
            // A key overhanging the window: the phrase could continue, which is
            // upstream's `longest > end` CONTINUED.
            *buf.continued = true;
            return Ok(());
        }
        buf.path.push(scan_key.key);
        if to == end {
            self.search_scan_path(buf, end)?;
        } else if buf.path.len() < MAX_PHRASE_LENGTH {
            self.scan_paths(matrix, to, end, buf)?;
        }
        buf.path.pop();
        Ok(())
    }

    /// The table search on one complete key-path, and the prefix probe that
    /// decides whether the window keeps widening.
    pub(super) fn search_scan_path(
        &self,
        buf: &mut ScanBuf<'_>,
        end: usize,
    ) -> Result<(), EngineError> {
        let ScanBuf {
            path,
            system,
            addon,
            continued,
            entries,
        } = buf;
        let has_incomplete = path
            .iter()
            .any(|key| key.completeness() == Completeness::Partial);

        if has_incomplete {
            for sequence in expand_keys(path, SCAN_EXPANSION_LIMIT) {
                self.lookup_and_append(
                    sequence.as_slice(),
                    path.len(),
                    end,
                    system,
                    addon,
                    entries,
                )?;
            }
        } else {
            self.lookup_and_append(path, path.len(), end, system, addon, entries)?;
        }

        let can_extend = self
            .dictionary
            .phrase_prefix_exists(path)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        let addon_extend = self
            .dictionary
            .phrase_prefix_exists_addon(path)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        **continued |= can_extend || addon_extend;
        Ok(())
    }

    pub(super) fn lookup_and_append(
        &self,
        sequence: &[SyllableKey],
        keys: usize,
        end: usize,
        system: &mut Vec<Candidate>,
        addon: &mut Vec<Candidate>,
        entries: &mut Vec<PhraseEntry>,
    ) -> Result<(), EngineError> {
        self.dictionary
            .lookup_into(sequence, entries)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        append_scan_entries(entries.drain(..), keys, end, CandidateKind::Phrase, system);
        self.dictionary
            .lookup_addon_into(sequence, entries)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        append_scan_entries(entries.drain(..), keys, end, CandidateKind::Addon, addon);
        Ok(())
    }
}
