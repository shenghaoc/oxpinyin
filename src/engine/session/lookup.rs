//! Candidate lookup: the expanding-window scan at the composition offset or any lookup offset, the backward-anchored before-cursor window, the n-best prepend, and the DP that pools phrases into sentences.

use crate::vocab::{MAX_PHRASE_LENGTH, phrase_index_library_index};

use super::*;

/// An unreadable item contributes no Rust row, but the pin still enters
/// its scorer. Any short merged-gram header asserts before its zero total.
fn abort_unreadable_ranking(gram: Option<&MergedGram>) -> Result<(), EngineError> {
    if let Some(row) = gram {
        row.total().map_err(EngineError::lookup_gram)?;
    }
    std::process::abort();
}

/// The LONGER winner law (`pinyin.cpp:1904-1917`): a strictly greater
/// unigram replaces the winner, and an equal one only from an earlier
/// library, because `reduce_tokens` concatenates the libraries first.
fn beats_longer_winner(
    token: PhraseToken,
    unigram: u64,
    best_token: PhraseToken,
    best: u64,
) -> bool {
    unigram > best
        || (unigram == best
            && usize::from(phrase_index_library_index(token))
                < usize::from(phrase_index_library_index(best_token)))
}

impl<D, L, S: std::borrow::Borrow<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    /// Offset-zero predecessor for candidate ranking and predicted bigram
    /// training (074a2219, pinyin.cpp:1711-1740 and zhuyin.cpp:1146-1175).
    /// The longest readable phrase wins, with the first token kept on a tie.
    /// If none can be read, use sentence_start.
    #[must_use]
    pub fn prefix_predecessor(&self) -> PhraseToken {
        let mut previous = crate::vocab::SENTENCE_START;
        let mut longest = 0;
        for &token in &self.candidate_prefixes {
            if token == crate::vocab::SENTENCE_START {
                continue;
            }
            if let Some(length) = self.dictionary.phrase_length_for_token(token)
                && length > longest
            {
                previous = token;
                longest = length;
            }
        }
        previous
    }

    /// Prepends the LONGER candidate row when the sort word leaves
    /// `SORT_WITHOUT_LONGER_CANDIDATE` clear — the pin's
    /// `_prepend_longer_candidates` call (`pinyin.cpp:2292-2293`).
    ///
    /// The pin runs this after the span loop, for every valid lookup
    /// offset: the span loop's result is empty at an offset no span
    /// starts at (a mid-key byte, the reserved slot), and the prepend
    /// still fires. An in-span apostrophe is a live separator
    /// column, not an empty one: the span loop steps over the zero key
    /// and continues to the next key (`search_matrix_recur`,
    /// `phonetic_key_matrix.cpp:388-394` — measured: `ni'hao` at the
    /// apostrophe byte answers the `hao` window), and the prepend still
    /// runs. Callers put this before [`Session::prepend_nbest_rows`] so
    /// the n-best rows rotate above the LONGER one, exactly the pin's
    /// two prepends.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the whole-composition suggestion walk
    /// or a model read fails — the surfaces [`Session::longer_candidate`]
    /// reads.
    ///
    /// # Aborts
    ///
    /// Where the pin's suggestion walk dies (see
    /// [`Session::longer_candidate`]).
    fn prepend_longer_row(&self, out: &mut Vec<Candidate>) -> Result<(), EngineError> {
        if self.sort_word & SORT_WITHOUT_LONGER_CANDIDATE == 0
            && let Some((_, candidate)) = self.longer_candidate()?
        {
            out.insert(0, candidate);
        }
        Ok(())
    }

    /// The LONGER candidate row — `_prepend_longer_candidates`
    /// (`pinyin.cpp:1870-1933` at the pin): a suggestion search over the
    /// whole composition's matrix, its extensions capped at twice the
    /// parse's key count, the maximum-unigram token of the (char-length
    /// capped) survivors as the row, with ties in library order then
    /// path order within each library, typed `LONGER_CANDIDATE` with the
    /// unigram-only amplified frequency and a zero span.
    ///
    /// `None` when the pin would prepend no row: the parse is empty, the
    /// cap admits no phrase, or no surviving token owns a phrase item.
    /// The frequency law is `((1−λ) · unigram / total) · 2²⁴` truncated —
    /// [`amplified_frequency`], the bigram term at exactly `0.0` exactly
    /// as the pin's LONGER branch computes it (no merged gram there).
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the dictionary walk or a model read
    /// fails — the same surfaces the window scan reads.
    ///
    /// # Aborts
    ///
    /// On the toned incomplete key and the over-long user index key the
    /// pin's suggestion walk dies on.
    pub(super) fn longer_candidate(&self) -> Result<Option<(RankKey, Candidate)>, EngineError> {
        // `prefix_len` is `m_parsed_key_len` — the parser's key count for
        // the WHOLE composition (`pinyin.cpp:1512`), not the remaining
        // slice's. The suggestion matrix walk starts at column 0 of the
        // same matrix regardless of the guess offset
        // (`search_suggestion_with_matrix`, `phonetic_key_matrix.cpp:504-529`).
        let selected = &self.selected_path().edges;
        let prefix_len = selected.len();
        if prefix_len == 0 || prefix_len >= MAX_PHRASE_LENGTH {
            return Ok(None);
        }
        let matrix = &self.retained_parse().scan;
        // `search_suggestion_with_matrix` walks to `matrix->size() - 1`
        // (`phonetic_key_matrix.cpp:512`), the parse's reserved slot: the
        // last byte the SELECTED parse covers. That is not
        // `graph.consumed()`, which also counts incomplete edges — a
        // trailing byte the parser left unparsed (as in `nih`) would
        // otherwise put `end` past the last key's column, the walk would
        // find no path, and the LONGER row would vanish.
        let end = selected.last().map_or(0, |edge| edge.to());
        if matrix.get(0).is_none_or(<[ScanKey]>::is_empty) {
            return Ok(None);
        }

        // The suggestion walk: every key-path from column 0 to the whole
        // parse's end, within the cap. The pin's `search_matrix`-shaped
        // recursion over every column entry (all parses plus the
        // resplit/divided/fuzzy additions), zero-key separator columns
        // stepped over without pushing.
        let mut path = SuggestionPath::default();
        let mut winner = None;
        self.search_longer_paths(0, end, prefix_len, &mut path, &mut winner)?;
        let Some((token, unigram)) = winner else {
            return Ok(None);
        };
        // The winner's text, built once: the length the cap read came from
        // the same lookup, so the text is present.
        let Some(text) = self.dictionary.phrase_text_for_token(token) else {
            return Ok(None);
        };
        let total = self
            .model
            .unigram_total()
            .map_err(|_| EngineError::Failure)?
            .unwrap_or(0);
        // LONGER has no bigram term (`pinyin.cpp:1916-1927`). Its zero
        // span marks the C-facing row as LONGER. The final dedup reads
        // its frequency even when the sort word suppresses sentence rows.
        let phrase_length = text.phrase_length();
        let key = RankKey::for_sort_word(
            phrase_length,
            0,
            amplified_frequency(unigram, total, self.model.amplification_lambda()),
            self.sort_word,
        );
        Ok(Some((
            key,
            Candidate::new(text, CandidateKind::Phrase, 0, Some(token), None),
        )))
    }

    /// Updates the LONGER winner for one DFS leaf. Dictionary and model
    /// reads retain path order; ties follow [`beats_longer_winner`].
    ///
    /// # Errors
    ///
    /// The dictionary walk's or a model read's failure.
    ///
    /// # Aborts
    ///
    /// Where the pin's suggestion walk dies: a toned
    /// first initial-only key on a searched path
    /// (`abort_toned_incomplete`), or a user index key past
    /// `MAX_PHRASE_LENGTH` syllables that extends a path
    /// (`chewing_large_table2_bdb.cpp:282` at 074a2219).
    fn update_longer_winner(
        &self,
        path: &SuggestionPath,
        prefix_len: usize,
        winner: &mut Option<(PhraseToken, u64)>,
    ) -> Result<(), EngineError> {
        let SuggestionPath { keys, tones } = path;
        // `search_suggestion` asserts on a toned first initial-only key
        // before its walk (`chewing_large_table2_bdb.cpp:560`, kc `:534`,
        // tkrzw `:504`; `pinyin_phrase3.h:152`).
        crate::engine::abort_toned_incomplete(keys, tones);
        // The pin's `search_suggestion` gate: the walk reaches a user
        // index key past `MAX_PHRASE_LENGTH` syllables that extends this
        // path and `switch`'s `default: abort()` dies
        // (`chewing_large_table2_bdb.cpp:282`; kc `:269`, tkrzw `:252`).
        // The pin reaches it only inside `_prepend_longer_candidates`
        // (`pinyin.cpp:2292-2293`), called from
        // `pinyin_guess_candidates`, as here.
        let tokens = self
            .dictionary
            .checked_extension_tokens(keys.as_slice())
            .map_err(|_| EngineError::Failure)?;
        let Some(tokens) = tokens else {
            std::process::abort();
        };
        for token in tokens {
            // `get_phrase_item` + the char-length cap: a phrase longer
            // than `prefix_len * 2 + 1` characters is skipped
            // (`pinyin.cpp:1899-1902`).
            // The length alone decides the cap, so no text is built here.
            let cap = prefix_len * 2 + 1;
            let Some((phrase_length, item_unigram)) =
                self.dictionary.phrase_length_and_item_unigram(token, cap)
            else {
                continue;
            };
            if phrase_length > cap {
                continue;
            }
            // The winner law: the first survivor initializes; a
            // strictly greater unigram replaces (ties keep the earlier
            // token — `:1904-1917`). `reduce_tokens` concatenates
            // libraries first (`phrase_large_table3.h:77-97`), so an
            // equal unigram from an earlier library wins even when
            // reached through a later path. Within a library the
            // path walk order stays unchanged. The unigram carries the
            // user/training overlay (`add_unigram_frequency` writes the
            // facade item). The pin reads it from the item it fetched
            // (`:1904-1911`); the dictionary answers that field where it
            // equals the model's `unigram_freq`, and the model answers
            // every other token.
            let unigram = match item_unigram {
                Some(unigram) => u64::from(unigram),
                None => self
                    .model
                    .unigram_freq(&token)
                    .map_err(|_| EngineError::Failure)?
                    .unwrap_or(0),
            };
            if winner.as_ref().is_none_or(|(best_token, best)| {
                beats_longer_winner(token, unigram, *best_token, *best)
            }) {
                *winner = Some((token, unigram));
            }
        }
        Ok(())
    }

    /// Visits suggestion paths in DFS order, evaluating each leaf before
    /// continuing. Only the current path and winner are retained. The matrix
    /// walk itself reads immutable session state and performs no model I/O.
    fn search_longer_paths(
        &self,
        node: usize,
        end: usize,
        prefix_len: usize,
        path: &mut SuggestionPath,
        winner: &mut Option<(PhraseToken, u64)>,
    ) -> Result<(), EngineError> {
        let Some(column) = self.retained_parse().scan.get(node) else {
            return Ok(());
        };
        if node == end {
            if !path.keys.is_empty() {
                self.update_longer_winner(path, prefix_len, winner)?;
            }
            return Ok(());
        }
        // The pin's two caps (phonetic_key_matrix.cpp:452-459 at 074a2219).
        let max_keys = (prefix_len * 2).min(MAX_PHRASE_LENGTH);
        for scan_key in column.iter().copied() {
            let to = scan_key.to;
            if to > end || path.keys.len() >= max_keys {
                continue;
            }
            path.keys.push(scan_key.key);
            path.tones.push(scan_key.tone);
            self.search_longer_paths(to, end, prefix_len, path, winner)?;
            path.keys.pop();
            path.tones.pop();
        }
        Ok(())
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
    /// (the matrix has `parsed_len + 1` columns, so `ni,'hao` offset 3 is an
    /// ordinary lookup of the empty window, and the C adapters abort from
    /// offset 4). The scan's own key set
    /// ([`super::build_scan_matrix_over`]) models the key columns, so a byte some key's
    /// syllable starts on, or an in-span apostrophe byte, answers; any other
    /// byte is an empty column. Exact mode has no pin counterpart, so its
    /// columns stay the exact segments, and its inputs' apostrophes are all
    /// in-span by construction (`build_exact` rejects any other gap).
    pub(super) fn spans_a_matrix_key(&self, offset: usize) -> bool {
        if !self.input.full_pinyin() {
            return self.input.as_bytes().get(offset) == Some(&b'\'')
                || self
                    .input
                    .exact()
                    .iter()
                    .any(|segment| segment.start() == offset);
        }
        // The retained scan carries the split alternates exactly when the
        // window scan does (`scan_divided`), and exact keys never gain them.
        let retained = self.retained_parse();
        let matrix = &retained.scan;
        if matrix
            .iter()
            .flatten()
            .any(|key| key.syllable_start == offset)
        {
            return true;
        }
        let first = matrix.iter().flatten().map(|key| key.syllable_start).min();
        offset < retained.matrix.bound
            && first.is_some_and(|start| offset >= start)
            && self.input.as_bytes().get(offset) == Some(&b'\'')
    }

    /// The previous token at `offset`, as upstream's `_get_previous_token`
    /// resolves it (`pinyin.cpp:1711-1767`).
    ///
    /// At offset zero this uses [`Session::prefix_predecessor`].
    ///
    /// Above 0 upstream reads the first sentence row's spans and
    /// carries a guard worth reproducing: it inspects `result[offset]` FIRST
    /// and only walks backwards when that position holds a token. A guess at
    /// an offset no phrase starts at contributes no bigram term at all.
    pub(super) fn previous_token(&self, offset: usize) -> Option<PhraseToken> {
        if offset == 0 {
            return Some(self.prefix_predecessor());
        }
        let spans = &self.sentence.rows.first()?.spans;
        let index = spans.partition_point(|span| span.start < offset);
        // `result[offset] != null_token`: a phrase must begin here.
        if spans.get(index)?.start != offset {
            return None;
        }
        spans.get(index.checked_sub(1)?).map(|span| span.token)
    }

    /// Upstream's Gates 1 and 2 as one call: resolve the previous token and
    /// merge its system and user grams, ONCE per candidate guess.
    ///
    /// `None` whenever upstream would skip the merge — the bit is clear, no
    /// previous token, or the model carries no row for it — and the caller
    /// then contributes no bigram term. Otherwise the previous token's
    /// value and its merged row.
    ///
    /// # Errors
    ///
    /// The model's load failure, or a lookup refusal where the pin's `merge_single_gram` asserts or reads past the user
    /// gram (`ngram.cpp:304`, `:318-349`).
    pub(super) fn dynamic_adjust_gram(
        &self,
        offset: usize,
    ) -> Result<Option<MergedGram>, EngineError> {
        if !self.options.contains(OptionBits::DYNAMIC_ADJUST) {
            return Ok(None);
        }
        let Some(prev) = self.previous_token(offset) else {
            return Ok(None);
        };
        let gram = self
            .model
            .merged_successors(&prev)
            .map_err(|error| EngineError::row_read(&self.model, &error))?;
        if let Some(fault) = gram.as_ref().and_then(MergedGram::fault) {
            return Err(EngineError::lookup_gram(fault));
        }
        Ok(gram)
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
    /// law. Equal amplified frequencies retain collection order. `amplified_frequency`
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
        abort_on_rank: bool,
        frequencies: &mut Vec<u32>,
    ) -> Result<(), EngineError> {
        // Tokens the table lacks rank as zero.
        frequencies.clear();
        frequencies.resize(collected.len(), 0);
        // The facade total, `get_phrase_index_total_freq()`: the sum of
        // every item's stored unigram (`gen_unigram`'s +1 included).
        let lambda = self.model.amplification_lambda();
        let default_total = self
            .model
            .unigram_total()
            .map_err(|_| EngineError::Failure)?
            .unwrap_or(0);
        let addon_total = self
            .model
            .addon_unigram_total()
            .map_err(|_| EngineError::Failure)?
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
                    .map_err(|_| EngineError::Failure)?
                    .unwrap_or(0);
                Some(amplified_frequency(raw, addon_total, lambda))
            } else {
                // Upstream's Gate 3: the bigram possibility joins the unigram
                // term INSIDE the pin's expression, before its single
                // truncation. The addon and predicted branches above return
                // early in the pin too — they carry no bigram term at all.
                let bigram = match dynamic_adjust_bigram_possibility(self.options, gram, token) {
                    Ok(bigram) => bigram,
                    Err(fault) => {
                        return Err(EngineError::lookup_gram(fault));
                    }
                };
                let count = self
                    .model
                    .unigram_freq(&token)
                    .map_err(|_| EngineError::Failure)?;
                // The zero total asserts at this row's scorer, after its
                // dynamic adjustment and before any later row or span.
                if abort_on_rank {
                    std::process::abort();
                }
                count.map(|count| {
                    if self.law == Law::Zhuyin {
                        // zhuyin.cpp:1264-1268 has no libpinyin 0.1
                        // discount (pinyin.cpp:33,1862 at 074a2219).
                        let possibility =
                            lambda * bigram + (1.0 - lambda) * count as f32 / default_total as f32;
                        (possibility * AMPLIFY_SCALE_F32) as u32
                    } else {
                        amplified_frequency_with_bigram(count, default_total, bigram, lambda)
                    }
                })
            };
            if let Some(count) = count {
                // Unigram term of candidate frequency: always on. Upstream
                // reads FacadePhraseIndex unigrams (including trained user
                // counts) with no DYNAMIC_ADJUST check. The overlay applies in both cases.
                frequencies[index] = count;
            }
        }
        Ok(())
    }

    /// The expanding-window scan of the pinned candidate collection.
    ///
    /// Start is fixed at the requested offset in the retained matrix;
    /// `end` walks outward over the whole parse's remaining columns. At
    /// each `[start, end)` window every key-path through the scan
    /// matrix — the selected parse plus the resplit/divided additions,
    /// `docs/findings/matrix-split-tables.md` — is enumerated and the phrase
    /// table is searched on the accumulated sequence; a key path with any
    /// initial-only key is looked up once through the DBM's
    /// incomplete-projected index (see [`Self::search_scan_path`]). Every
    /// phrase found is appended with its `[start, end)` span.
    ///
    /// Widening is prefix-driven: a window whose sequences cannot extend to
    /// any stored phrase stops the scan (the pin's continued-search probe,
    /// the `SEARCH_CONTINUED` bit of
    /// [`crate::model::Dictionary::candidate_search_into`]). Consecutive apostrophe
    /// bytes after a searched window are skipped so the next window does not
    /// repeat the same key sequence.
    pub(super) fn collect_window_scan(
        &self,
        input: &[u8],
        into: &mut Vec<Candidate>,
        scratch: &mut ScanScratch<'_>,
    ) -> Result<(), EngineError> {
        // pinyin.cpp:2229-2262 searches from the offset in the existing
        // whole-parse matrix. Re-parsing the suffix loses resplit/divided
        // keys whose spans were created by the earlier whole-parse passes.
        let retained = &self.retained_parse().matrix;
        let anchor = self.input.len().saturating_sub(input.len());
        let bound = retained.bound.saturating_sub(anchor);
        let mut end = 1usize;
        while end <= bound {
            // An end position no key starts at is an empty column: widen.
            let mut continued = retained.get(anchor + end).is_none_or(<[ScanKey]>::is_empty);
            scratch.path.clear();
            scratch.window_addon.clear();
            let start = into.len();
            {
                let mut buf = ScanBuf {
                    span_base: anchor,
                    path: scratch.path,
                    tones: SmallVec::new(),
                    system: &mut *into,
                    addon: scratch.window_addon,
                    continued: &mut continued,
                    unreadable: &mut *scratch.unreadable,
                    entries: scratch.entries,
                    row: scratch.row,
                };
                self.scan_parsed_paths(retained, anchor, anchor + end, &mut buf)?;
            }
            // Order the window the pin's array way: the default facade's
            // tokens ascending (in place over the rows this window
            // appended), then the addon facade's — the order
            // `_append_items` lays down and the stable sort keeps for
            // comparator ties.
            order_window_tail(into, start);
            flush_window_batch(scratch.window_addon, into);
            // SEARCH_NONE widens even without SEARCH_CONTINUED; the pin
            // checks continuation only after SEARCH_OK (pinyin.cpp:2240-
            // 2242,2260; zhuyin.cpp:1505-1512 at 074a2219).
            if !continued && into.len() != start {
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

    /// search_matrix over the retained parse, including physical zero hops.
    /// No key can be manufactured by cutting the caller's end inside it.
    fn scan_parsed_paths(
        &self,
        matrix: &matrix::ParsedMatrix,
        node: usize,
        end: usize,
        buf: &mut ScanBuf<'_>,
    ) -> Result<(), EngineError> {
        if matrix.column_is_empty(end) {
            return Ok(());
        }
        if node < end && matrix.has_zero(node) {
            return self.scan_parsed_paths(matrix, node + 1, end, buf);
        }
        let Some(column) = matrix.get(node) else {
            return Ok(());
        };
        for key in column {
            if key.to > end {
                *buf.continued = true;
                continue;
            }
            buf.path.push(key.key);
            buf.tones.push(key.tone);
            if key.to == end {
                self.search_scan_path(buf, end)?;
            } else if buf.path.len() < MAX_PHRASE_LENGTH {
                self.scan_parsed_paths(matrix, key.to, end, buf)?;
            }
            buf.path.pop();
            buf.tones.pop();
        }
        Ok(())
    }

    /// The table search on one key-path, and the prefix probe that
    /// decides whether the window keeps widening.
    ///
    /// A path with an initial-only key is looked up once, as upstream's
    /// `ChewingLargeTable2::search` (`chewing_large_table2.cpp:161-172` at
    /// pin `074a2219`) selects the incomplete or complete index by
    /// `contains_incomplete_pinyin` and issues one `search_internal` either
    /// way. The dictionary answers it from its incomplete-projected index:
    /// `ChewingTable::search` for the system path, `UserLookup::lookup` for
    /// the user overlay.
    pub(super) fn search_scan_path(
        &self,
        buf: &mut ScanBuf<'_>,
        end: usize,
    ) -> Result<(), EngineError> {
        let ScanBuf {
            span_base,
            path,
            tones,
            system,
            addon,
            continued,
            unreadable,
            entries,
            row,
        } = buf;
        let end = end.saturating_sub(*span_base);
        // The table search asserts on a toned first initial-only key before
        // it reads anything (`chewing_large_table2.cpp:166-167`,
        // `pinyin_phrase3.h:152`); only the explicit guess is the pin's.
        crate::engine::abort_toned_incomplete(path, tones);
        // One search per table answers the rows and `SEARCH_CONTINUED`
        // (`search_matrix`, `pinyin.cpp:2231-2256` at 074a2219). A typed
        // tone already filters the rows there (`pinyin_phrase3.h:97-105`).
        let (failed_item, extends) = self
            .dictionary
            .candidate_search_into(path, tones, entries, row)
            .map_err(|_| EngineError::Failure)?;
        **unreadable += failed_item.count;
        **continued |= extends;
        append_scan_entries(entries.drain(..), end, CandidateKind::Phrase, system);
        **continued |= self
            .dictionary
            .search_addon_into(path, entries, row)
            .map_err(|_| EngineError::Failure)?;
        append_scan_entries(entries.drain(..), end, CandidateKind::Addon, addon);
        Ok(())
    }
}

impl<D, L, S: std::borrow::BorrowMut<SessionState>> Session<D, L, S>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    /// Builds the candidate window anchored at byte `anchor` in the raw
    /// buffer into `out`: libpinyin's expanding-window scan over the
    /// retained parse, every key-path through the parser-shaped key set
    /// searched against the phrase table window by window.
    /// Cross-segmentation pooling falls out of the scan — `xian` offers
    /// `西安` (`xi` + `an`) alongside the single key `xian`, and `fangan`
    /// mixes `方案` (`fang` + `an`) with `反感` (`fan` + `gan`) — without a
    /// separate pooling step.
    ///
    /// The window is a pure function of `(raw, anchor, constraint-derived
    /// state)`: the scan reads `&self.input.as_str()[anchor..]` and the
    /// stored n-best rows prepend the same way regardless of the anchor. It
    /// mutates only the scan scratch and `out` — never the composition
    /// offset, the constraint store, or the history.
    pub(super) fn scan_window(
        &mut self,
        anchor: usize,
        out: &mut Vec<Candidate>,
        abort_on_rank: bool,
    ) -> Result<(), EngineError> {
        out.clear();
        if anchor >= self.input.len() {
            // A fully-consumed (or past-end) anchor still carries its
            // sentence rows — upstream's window prepends `m_nbest_results`
            // whether or not any phrase candidate remains at the cursor (the
            // L1 terminal-choose surface) — and its LONGER row, which the
            // pin prepends at every valid offset (`pinyin.cpp:2292-2293`).
            // The reserved slot's column holds no key, so the span loop
            // finds nothing but the prepends still run.
            self.prepend_longer_row(out)?;
            self.prepend_nbest_rows(out);
            return Ok(());
        }

        self.with_scan_scratch(|session, scratch| {
            session.scan_window_with_scratch(anchor, out, abort_on_rank, scratch)
        })
    }

    // Keep scratch separate from the session so the scan can borrow the
    // retained input and topology while filling the caller's output.
    fn with_scan_scratch<T>(
        &mut self,
        scan: impl FnOnce(&mut Self, &mut Scratch) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        let mut scratch = core::mem::take(&mut self.scratch);
        let result = scan(self, &mut scratch);
        if result.is_err() {
            // Discard partial scan state while retaining its allocations.
            scratch.entries.clear();
            scratch.path.clear();
            scratch.window_addon.clear();
            scratch.frequencies.clear();
            scratch.rank_keys.clear();
        }
        self.scratch = scratch;
        result
    }

    fn scan_window_with_scratch(
        &mut self,
        anchor: usize,
        out: &mut Vec<Candidate>,
        abort_on_rank: bool,
        scratch: &mut Scratch,
    ) -> Result<(), EngineError> {
        let Scratch {
            entries,
            row,
            path,
            window_addon,
            frequencies,
            rank_keys,
            ..
        } = scratch;
        let collected = out;

        let remaining = &self.input.as_str()[anchor..];

        // The pinned construction: the expanding-window scan, the
        // three-key order (text length, pinyin span, frequency),
        // frequency-aware dedup, and a raw-input fallback for an empty
        // scan.
        // Upstream's Gates 1 and 2 (`pinyin.cpp:2200-2216`,
        // `zhuyin.cpp:1474-1489`), before the search exactly as the pin
        // orders them and hoisted out of the candidate loop as it hoists
        // them: the previous token is resolved once, and the system and
        // user grams are merged once, for the whole guess. Indexing that
        // row per candidate is Gate 3.
        let gram = self.dynamic_adjust_gram(anchor)?;
        {
            let mut unreadable = 0;
            let mut scratch = ScanScratch {
                unreadable: &mut unreadable,
                path,
                entries,
                row,
                window_addon,
            };
            self.collect_window_scan(remaining.as_bytes(), collected, &mut scratch)?;
            if abort_on_rank && unreadable != 0 {
                return abort_unreadable_ranking(gram.as_ref());
            }
        }
        // The scan's result stands even when it found nothing. Tokens the
        // table lacks rank as zero rather than falling back.
        self.candidate_frequencies(collected, gram.as_ref(), abort_on_rank, frequencies)?;
        // `Session::rank_word`: the sort word, or libzhuyin's fixed
        // comparator (`zhuyin.cpp:1129-1144`) on a zhuyin facade.
        let rank_word = self.rank_word();
        // One key row per candidate, sorted and applied in place: the
        // candidates never leave `collected`, and the key rows, kept in
        // the session scratch, are the only buffer of this stage.
        rank_keys.clear();
        rank_keys.extend(
            collected
                .iter()
                .zip(frequencies.iter().copied())
                .enumerate()
                .map(|(index, (candidate, frequency))| {
                    // `compare_item_with_sort_option` compares a key
                    // only while its bit is set and returns 0 — a tie —
                    // once it is clear (`pinyin.cpp:1678-1709`): zeroing
                    // a disabled field makes every candidate equal on
                    // it, which is that comparator exactly.
                    let key = RankKey::for_sort_word(
                        candidate.phrase_length(),
                        candidate.consumed_bytes(),
                        frequency,
                        rank_word,
                    );
                    (key, index as u32)
                }),
        );

        // Stable sort, all three keys descending: an all-equal tie keeps
        // the collection order, which the scan now lays down in the
        // pin's array order (per window, token-ascending).
        rank_candidates(collected, rank_keys);

        // The pin prepends LONGER before the frequency dedup
        // (`pinyin.cpp:2292-2300`). Keep its unigram-only `m_freq`:
        // a same-text NORMAL row can have a larger bigram term.
        let empty_phrase_scan = collected.is_empty();
        if self.sort_word & SORT_WITHOUT_LONGER_CANDIDATE == 0
            && let Some((key, row)) = self.longer_candidate()?
        {
            collected.insert(0, row);
            // The index half is spent once the rows are in order.
            rank_keys.insert(0, (key, 0));
        }
        dedup_ranked_by_text(collected, rank_keys);

        if collected.is_empty() || empty_phrase_scan {
            let remaining = &self.input.as_str()[anchor..];
            collected.push(Candidate::new(
                compact_str::CompactString::from(remaining),
                CandidateKind::Fallback,
                remaining.len(),
                None,
                None,
            ));
        }

        // Prepend the stored n-best rows, head first, then drop every
        // later candidate with the same text — upstream prepends after the
        // sort and its phrase-string dedup keeps the NBEST row (and the
        // lower n-best index) over any phrase candidate with the same
        // string (`pinyin.cpp:2290-2298`, `2058-2126`).
        self.prepend_nbest_rows(collected);

        Ok(())
    }

    // Rebuilds the candidate window anchored at a caller lookup `offset`,
    // mirroring the pin's per-offset span search — `pinyin_guess_candidates`
    // re-runs `search_matrix` from `start = offset` (`pinyin.cpp:2224-2262`),
    // its candidates all beginning at `offset` — and returns it without
    // disturbing any composition state (constraints, consumed, history).
    //
    // A byte no matrix key starts on — a mid-syllable position of the
    // composition's own parse — is one of the pin's empty columns:
    // `search_matrix` matches nothing from it for every end, so the
    // window there is the raw-suffix fallback under the prepended n-best
    // rows, never a re-parse of the suffix (the suffix's own keys are not
    // the matrix's). A zero-key (apostrophe) column inside the parse is
    // not empty: the span search steps over it to the next key, so that
    // apostrophe byte answers the following key's window. An apostrophe
    // past a stop byte sits outside the matrix — the pin aborts there —
    // and takes the empty-column window like any other unreachable byte.
    //
    // The one-past-end offset is valid: `scan_window` answers the terminal
    // sentence rows for it (the pin's reserved slot). A later offset is
    // refused, because the pin reads its matrix out of bounds there.
    /// Builds the candidate window starting at a byte offset in the current input. The facade
    /// supplies `abort_on_rank` when the facade unigram total is zero at this offset.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the offset exceeds the input length, lies inside a UTF-8
    /// character, or a candidate or sentence lookup fails.
    ///
    /// # Aborts
    ///
    /// When `abort_on_rank` is `true`, ranking any searched phrase row aborts,
    /// including a row with an unreadable item.
    pub fn candidates_at_for_abi(
        &mut self,
        offset: usize,
        abort_on_rank: bool,
    ) -> Result<CandidateList, EngineError> {
        check_lookup_offset_range(self.input.len(), offset)?;
        if !self.input.is_char_boundary(offset) {
            return Err(EngineError::Failure);
        }
        self.build_window(|session, items| {
            if offset < session.input.len() && !session.spans_a_matrix_key(offset) {
                items.push(Candidate::new(
                    compact_str::CompactString::from(&session.input.as_str()[offset..]),
                    CandidateKind::Fallback,
                    session.input.len() - offset,
                    None,
                    None,
                ));
                // An empty column still has the LONGER and sentence
                // prepends (`pinyin.cpp:2292–2296`).
                session.prepend_longer_row(items)?;
                session.prepend_nbest_rows(items);
                return Ok(());
            }
            session.scan_window(offset, items, abort_on_rank)?;
            Ok(())
        })
    }

    fn build_window(
        &mut self,
        build: impl FnOnce(&mut Self, &mut Vec<Candidate>) -> Result<(), EngineError>,
    ) -> Result<CandidateList, EngineError> {
        let mut items = core::mem::take(&mut self.scratch.window_items);
        if let Err(error) = build(self, &mut items) {
            items.clear();
            self.scratch.window_items = items;
            return Err(error);
        }
        Ok(CandidateList::from_vec(items))
    }

    // Builds the candidate window over spans ENDING at byte `offset` in the
    // raw buffer — the pin's before-cursor walk
    // (`zhuyin_guess_candidates_before_cursor`, `zhuyin.cpp:1542-1629` at
    // the pin 0c5e80e1): every key-path from each start `0..offset` to the
    // fixed end, enumerated longest-span-first (start ascending — the pin's
    // `len` loop), each span's slice ranked by the three-key order exactly
    // like the after-cursor windows, the stored n-best rows prepended over
    // the whole set, then text dedup by frequency.
    //
    // Measured decomposition on the pin (su3u3, `before(5)`, the
    // instrumented oracle at 0c5e80e1): span (0,5) yields 3 phrase
    // candidates, span (3,5) yields 597, the mid-syllable starts (1, 2, 4)
    // answer no match (empty columns — `search_matrix` returns
    // `SEARCH_NONE` before recursing), 600 phrases total, +1 sentence row,
    // −1 string-duplicate phrase → 600. The builder reproduces that walk: a
    // start with no matrix column (a mid-syllable byte) contributes nothing
    // naturally (no key starts there), and a span ending on an apostrophe
    // separator byte answers nothing (upstream's zero-key end column:
    // `search_matrix` returns `SEARCH_CONTINUED` with no items,
    // `phonetic_key_matrix.cpp:419-423`).
    //
    // Does not disturb any composition state. At offset 0 nothing precedes
    // the first key, so the window is the prepended sentence rows alone.
    /// Builds the candidate window ending at a byte offset in the current input. The facade
    /// supplies `abort_on_rank` when the facade unigram total is zero at this offset.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] if the offset exceeds the input length, lies inside a UTF-8
    /// character, or a candidate or sentence lookup fails.
    ///
    /// # Aborts
    ///
    /// When `abort_on_rank` is `true`, ranking any searched phrase row aborts,
    /// including a row with an unreadable item.
    pub fn candidates_ending_at_for_abi(
        &mut self,
        offset: usize,
        abort_on_rank: bool,
    ) -> Result<CandidateList, EngineError> {
        check_lookup_offset_range(self.input.len(), offset)?;
        if !self.input.is_char_boundary(offset) {
            return Err(EngineError::Failure);
        }
        self.build_window(|session, items| {
            // The start and apostrophe separators have no ending span.
            if offset == 0 || session.input.as_bytes().get(offset - 1) == Some(&b'\'') {
                session.prepend_nbest_rows(items);
                return Ok(());
            }
            session.scan_window_ending(offset, items, abort_on_rank)
        })
    }

    /// The backward-anchored scan proper: the complete retained parse, every
    /// live start enumerated ascending (the pin's longest span first), each
    /// span's batch flushed and ranked with its own previous-token gram (the
    /// pin resolves `_get_previous_token` per `len` slice), groups appended
    /// in order, then one text dedup — the pin dedups once, after the
    /// prepend, and [`Session::prepend_nbest_rows`] re-runs it over the
    /// joined list.
    pub(super) fn scan_window_ending(
        &mut self,
        offset: usize,
        out: &mut Vec<Candidate>,
        abort_on_rank: bool,
    ) -> Result<(), EngineError> {
        out.clear();
        self.with_scan_scratch(|session, scratch| {
            session.scan_window_ending_with_scratch(offset, out, abort_on_rank, scratch)
        })
    }

    fn scan_window_ending_with_scratch(
        &mut self,
        offset: usize,
        out: &mut Vec<Candidate>,
        abort_on_rank: bool,
        scratch: &mut Scratch,
    ) -> Result<(), EngineError> {
        let rank_word = self.rank_word();
        let Scratch {
            entries,
            row,
            path,
            window_addon,
            frequencies,
            rank_keys,
            ..
        } = scratch;
        // The retained topology also serves the before-cursor window.
        let matrix = &self.retained_parse().matrix;
        let bound = matrix.bound.min(offset);

        let mut group: Vec<Candidate> = Vec::new();
        // `rank_keys[i].0` is `out[i]`'s key: each slice appends its rows'
        // keys as it appends the rows.
        rank_keys.clear();
        let mut unreadable = 0;

        for start in 0..bound {
            // The pin ranks each `len` slice on its own, with the previous
            // token resolved and its grams merged at that slice's start,
            // before the search (`zhuyin.cpp:1566-1582`).
            let gram = self.dynamic_adjust_gram(start)?;
            // An empty column — no key starts here — is the pin's
            // `SEARCH_NONE` start (`search_matrix`,
            // `phonetic_key_matrix.cpp:416-418`): the span contributes
            // nothing and the walk skips it.
            if matrix.column_is_empty(start) {
                continue;
            }
            let mut continued = false;
            let prior_unreadable = unreadable;
            group.clear();
            {
                let mut buf = ScanBuf {
                    span_base: 0,
                    path,
                    tones: SmallVec::new(),
                    system: &mut group,
                    addon: window_addon,
                    continued: &mut continued,
                    unreadable: &mut unreadable,
                    entries,
                    row,
                };
                self.scan_parsed_paths(matrix, start, offset, &mut buf)?;
            }
            if abort_on_rank && unreadable != prior_unreadable {
                abort_unreadable_ranking(gram.as_ref())?;
            }
            order_window_tail(&mut group, 0);
            flush_window_batch(window_addon, &mut group);
            if group.is_empty() {
                continue;
            }
            // Every row of this slice spans `[start, offset)` — the pin's
            // `template_item.m_begin = start; m_end = offset`
            // (`zhuyin.cpp:1595`). The retained matrix's coordinates are
            // absolute, so the start is recorded as such; the end is the
            // row's `consumed_bytes` already.
            for candidate in &mut group {
                candidate.set_span_start(start);
            }
            self.candidate_frequencies(&group, gram.as_ref(), abort_on_rank, frequencies)?;
            let slice_start = rank_keys.len();
            rank_keys.extend(
                group
                    .iter()
                    .zip(frequencies.iter().copied())
                    .enumerate()
                    .map(|(index, (candidate, frequency))| {
                        // `Session::rank_word`: on the zhuyin facade, phrase
                        // length then frequency (`zhuyin.cpp:1129-1144`).
                        let key = RankKey::for_sort_word(
                            candidate.phrase_length(),
                            candidate.consumed_bytes(),
                            frequency,
                            rank_word,
                        );
                        (key, index as u32)
                    }),
            );
            rank_candidates(&mut group, &mut rank_keys[slice_start..]);
            out.append(&mut group);
        }

        // Each slice keeps its own sort order. The final dedup compares
        // `m_freq` across all slices (`zhuyin.cpp:1612,1626-1630`).
        dedup_ranked_by_text(out, rank_keys);

        self.prepend_nbest_rows(out);
        Ok(())
    }

    /// Prepends the stored n-best rows onto `collected`, head first, then
    /// drops every later candidate with the same text — upstream prepends
    /// after the sort and its phrase-string dedup keeps the NBEST row
    /// (and the lower n-best index) over any phrase candidate with the
    /// same string (`pinyin.cpp:2290-2298`, `2058-2126`). Extend-then-
    /// rotate keeps `collected`'s allocation — the session scratch on the
    /// scan path, a fresh small vec on the fully-consumed path.
    ///
    /// Under [`Law::Zhuyin`] only the 1-best
    /// row is prepended — libzhuyin's display law.
    ///
    /// A sort word with `SORT_WITHOUT_SENTENCE_CANDIDATE` set prepends
    /// nothing and so dedups nothing against the sentences: the pin skips
    /// `_prepend_sentence_candidates` under that bit (`pinyin.cpp:2295-2296`)
    /// before `_remove_duplicated_items_by_phrase_string` runs (`:2300`), so
    /// a phrase row whose string repeats an n-best sentence survives there
    /// (issue #582). Filtering the sentence rows out after the dedup would
    /// drop both copies of that string.
    pub(super) fn prepend_nbest_rows(&mut self, collected: &mut Vec<Candidate>) {
        if self.sentence.rows.is_empty() || self.sort_word & SORT_WITHOUT_SENTENCE_CANDIDATE != 0 {
            return;
        }
        let rows = if self.law == Law::Zhuyin {
            &self.sentence.rows[..1]
        } else {
            &self.sentence.rows[..]
        };
        let before = collected.len();
        collected.extend(rows.iter().enumerate().filter_map(|(index, row)| {
            // Rows 80 and 106 omit NULL text on the dedup path. Keep the
            // original n-best index of every displayed row.
            let text = row.text.as_ref()?;
            Some(Candidate::new(
                text.clone(),
                CandidateKind::Sentence,
                row.span,
                None,
                Some(u8::try_from(index).unwrap_or(u8::MAX)),
            ))
        }));
        let prepended = collected.len() - before;
        collected.rotate_right(prepended);
        dedup_by_text_keep_first(collected);
    }
}

/// One key path of the LONGER suggestion walk, with the tone typed on each
/// key: the table's suggestion search reads both
/// (`contains_incomplete_pinyin`, `pinyin_phrase3.h:146-156`).
#[derive(Default)]
struct SuggestionPath {
    keys: SmallVec<[SyllableKey; 16]>,
    tones: SmallVec<[u8; 16]>,
}

#[cfg(test)]
mod tests;
