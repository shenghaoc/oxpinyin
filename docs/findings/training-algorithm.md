# libpinyin training pipeline

Status: **reference**

This finding records the trainer workflow's call graph, the file formats
and the algorithms of the libpinyin utilities that the trainer runs.
oxpinyin reproduces those utilities under their libpinyin names, options,
messages, inputs and outputs (`oxpinyin-utils`). The utilities read and
write the backend's own files, as a libpinyin build of each utility does.
The trainer's own Python scripts are not libpinyin code. They stay
external and drive the staged utilities unchanged
([packaging: trainer utility layout](../packaging.md#trainer-utility-layout)).
The per-utility compatibility cases are in
[the utility runbook](../runbooks/tool-diff.md).

## Source set

| Component | Repo | Commit | Role |
|---|---|---|---|
| libpinyin | `github.com/libpinyin/libpinyin` | `074a2219c90feaf962d0d24f034514033ece5f99` (2.11.92, the pin) | `utils/` tools and storage |
| trainer | `github.com/libpinyin/trainer` | `b1927376735c0c1042d72af2eca0e25c53595724` (`main`; the repo has no tags) | Python orchestration |

The libpinyin line numbers below are at the pin, `074a2219`
(`tools/oracle/oracle-pin.txt`). Paths are relative to each repository
root.

Constants (`src/include/novel_types.h`):

| Symbol | Value | Line |
|---|---|---|
| `ERROR_OK` | `0` | `novel_types.h:78` |
| `MAX_PHRASE_LENGTH` | `16` | `novel_types.h:119` |
| `null_token` | `0` | `novel_types.h:121` |
| `sentence_start` | `1` | `novel_types.h:122` |
| `parameter_t` | `double` | `novel_types.h:130` |

---

## 1. Pipeline and call graph

`lib/myconfig.py` fixes every parameter, `lib/utils.py` is the JSON
status layer and `lib/dirwalk.py` walks the `*.index` corpus files. Each
stage is epoch-gated by a `.status` file (`utils.check_epoch`). The main
pipeline has five stages and uses the K-mixture model (KMM) throughout
(the five `__main__` scripts):

```text
segment.py   → utils/segment/ngseg   (default)              segment
             → utils/segment/spseg   (--fast)
generate.py  → utils/training/gen_k_mixture_model            KMM count (per document)
                 --maximum-occurs-allowed 20
                 --maximum-increase-rates-allowed 3.0
estimate.py  → utils/training/estimate_k_mixture_model       per-candidate λ score
                 --deleted-bigram-file estimates.db          → estimate.sorted.index (score desc)
tryprune.py  → merge_k_mixture_model  --result-file merged.db  (top --merge=10 candidates)
             → validate_k_mixture_model merged.db
             → export_k_mixture_model  → kmm_merged.text
             → prune_k_mixture_model  -k 3 --CDF 0.99 pruned.db
             → validate_k_mixture_model pruned.db
             → export_k_mixture_model  → kmm_pruned.text
             → k_mixture_model_to_interpolation < kmm_pruned.text → interpolation2.text
evaluate.py  → /usr/bin/make            (rebuild eval runtime model from interpolation2.text)
             → estimate_interpolation   → average λ
             → /usr/bin/make modify LAMBDA_PARAMETER=λ
             → eval_correction_rate      → correction rate
```

`evaluate.py`'s `make` runs `gen_binary_files`, `import_interpolation` and
`gen_unigram` through the data `Makefile` (`data/Makefile.am:60-62`).

| Utility | Invoked by |
|---|---|
| `segment/ngseg` | `segment.py` (default) |
| `segment/spseg` | `segment.py --fast` |
| `segment/mergeseq` | `mergeseq.py` (word-recognition corpus preparation) |
| `training/gen_k_mixture_model` | `generate.py` |
| `training/estimate_k_mixture_model` | `estimate.py` |
| `training/merge_k_mixture_model`, `validate_k_mixture_model`, `prune_k_mixture_model`, `export_k_mixture_model`, `k_mixture_model_to_interpolation` | `tryprune.py` |
| `training/estimate_interpolation`, `training/eval_correction_rate` | `evaluate.py` |
| `storage/gen_pinyin_table` | `tools/convertopengram.py`, `tools/striptones.py` (OpenGram dictionary preparation, optional) |

No trainer script invokes `training/gen_ngram`, `training/gen_deleted_ngram`,
`storage/export_interpolation`, `training/import_k_mixture_model` or
`storage/gen_zhuyin_table`. They are valid libpinyin
utilities, and `oxpinyin-utils` provides them because they are part of
the pin's utility set.

`generate.py` refuses to run if the `MergeSequence` epoch is signed
(`generate.py:26-27,112-113`, "Please skip mergeseq"). Thus `mergeseq` and
the KMM `generate` stage are mutually exclusive on one corpus tree.

The word-recognition pipeline (`prepare.py`, `populate.py`,
`partialword.py`, `newword.py`, `markpinyin.py`), the punctuation pipeline
(`genpunct.py`, which writes the `puncts.table` that
`gen_binary_files --gen-punct-table` compiles), `reduce.py` and the other
`tools/*.py` helpers are pure Python and SQLite. They call no libpinyin
binary and stay external.

| Capability | Trainer entry | libpinyin source | oxpinyin |
|---|---|---|---|
| ngseg, spseg, mergeseq | `segment.py`, `mergeseq.py` | `utils/segment/*.cpp` | `oxpinyin-utils` binaries; they share the `segment` module's line framing and lexicon |
| KMM generate, estimate, merge, validate, prune, export, import | `generate.py`, `estimate.py`, `tryprune.py` | `utils/training/*_k_mixture_model.cpp` | `oxpinyin-utils` binaries and its `k_mixture_model` module |
| KMM → interpolation | `tryprune.py` | `k_mixture_model_to_interpolation.cpp` | `oxpinyin-utils` `k_mixture_model_to_interpolation` |
| eval runtime model build | `evaluate.py` `make` | `data/Makefile.am:60-62` | `oxpinyin-utils` `gen_binary_files`, `import_interpolation`, `gen_unigram` |
| λ estimate | `evaluate.py` | `estimate_interpolation.cpp` | `oxpinyin-utils` `estimate_interpolation` |
| correction rate | `evaluate.py` | `eval_correction_rate.cpp` | `oxpinyin-utils` `eval_correction_rate` over its `training::eval` module |
| legacy gen_ngram, gen_deleted_ngram, export_interpolation; gen_zhuyin_table | none | `utils/training`, `utils/storage` | `oxpinyin-utils` binaries |
| OpenGram dictionary tools | `tools/*.py` | `gen_pinyin_table.cpp` | `oxpinyin-utils` `gen_pinyin_table`; the Python helpers stay external |
| orchestration, status, word recognition, punctuation | the Python scripts | none | external |

Two model representations share one corpus-counting algorithm:

- **Legacy interpolation path**: `gen_unigram` (§3), `gen_ngram` (§4),
  `gen_deleted_ngram` (§4.3) and `estimate_interpolation` (§5).
- **KMM path**: `gen_k_mixture_model`, `estimate_k_mixture_model`, the
  prune stage and `k_mixture_model_to_interpolation` (§7). This path
  produces the shipped `interpolation2.text`. Its counting is the same as
  `gen_ngram`'s, and its λ math is the same deleted-interpolation EM (§5).

The segmenter needs a bigram to score its Viterbi, but it produces the
corpus that trains that bigram. An existing model seeds the segmenter: the
shipped `SYSTEM_BIGRAM` or the model of a prior epoch. The trainer has no
first-model generator, and `segment.py` and `ngseg.cpp` consume a
populated bigram.

---

## 2. Segmenters

`ngseg` and `spseg` consume raw UTF-8 on stdin and emit the tokenized,
sentence-separated format that the counters read (§4). They share the
line framing, the state machine and the unknown-run handling. Only the
scorer of a segmentable run differs. `mergeseq` reads that format.

### 2.1 `ngseg`: bigram-scored Viterbi (`utils/segment/ngseg.cpp`)

- Reads stdin (or the file argument) line by line with `std::getline`
  into a `LineString`, which drops the `\n` (`ngseg.cpp:187-188`).
- Validates the line as UCS-4 (`ngseg.cpp:190-198`). An empty line prints
  `null_token` alone (`ngseg.cpp:201-204`).
- Classifies each character with `phrase_table.search(1, char, tokens)` as
  *segmentable* (in the dictionary) or *unknown*, through the states
  `CONTEXT_INIT / SEGMENTABLE / UNKNOWN` (`ngseg.cpp:206-212`).
- Groups contiguous same-state characters into `current_ucs4`. At a state
  boundary it calls `deal_with_segmentable` or `deal_with_unknown`
  (`ngseg.cpp:214-237`).
- `deal_with_segmentable` (`ngseg.cpp:70-94`) calls
  `phrase_lookup->get_best_match` (the bigram-scored Viterbi over the
  dictionary, `src/lookup/phrase_lookup.cpp:119-157`), converts the result
  to UTF-8 and prints it. On failure it prints `"Un-segmentable sentence"`
  to stderr and returns `false`. The caller ignores the return value.
- `deal_with_unknown` (`ngseg.cpp:96-103`) prints `null_token` followed by
  the raw text.
- With `--generate-extra-enter` it emits an extra `null_token` at the end of
  each line (`ngseg.cpp:250-251`). It always emits a final `null_token` at
  the end of the file (`ngseg.cpp:258`).
- The `PhraseLookup` takes `lambda = system_table_info.get_lambda()`
  (`ngseg.cpp:172`), the bigram weight from `table.conf` (§5).

### 2.2 `spseg`: fewest-words shortest path (`spseg.cpp:86-168, 261-341`)

A segmentable run is split by an O(n²) DP that minimises the **word
count** `m_nword`, not a bigram score. No bigram is needed.

- `steps[0].m_nword = 0`. For each start `i` and each end `k = i+1..n`,
  search the span `[i,k)`. If it is not `SEARCH_OK` and `len ≠ 1`, skip it.
  A length-1 span that misses still gets `token = null_token` and counts
  as a word (`spseg.cpp:119-123`). `nword = steps[i].m_nword + 1`. Relax
  `steps[k]` only on a **strict** improvement (`nword < steps[k].m_nword`),
  so the first path wins ties. Break the inner loop when the span is not
  `SEARCH_CONTINUED`.
- Backtrace from `n` through `m_backward_nstep`, reverse, and emit
  `"{token} {utf8}\n"` per segment (`spseg.cpp:143-168`).
- Line handling (`spseg.cpp:268-341`): `std::getline` drops the `\n`. A non-UCS-4
  or empty line gives `"0 \n"`. `--generate-extra-enter` appends `"0 \n"`
  per line. A final `"0 \n"` is always emitted at the end of the file. An
  unknown run prints `"0 {raw}\n"`.

### 2.3 `mergeseq` (`mergeseq.cpp:78-197, 263-282`)

Reads a segmented stream (`{token} {phrase}` lines with `0 …` separators),
greedily merges maximal adjacent runs that form a known dictionary phrase
(at most `MAX_PHRASE_LENGTH` = 16 characters) and emits the merged stream.

- It keeps a queue of `TokenInfo{token, char_len}` and the concatenated
  UCS-4 buffer. `feed_line` parses the token. On `null_token` it flushes
  the queue (`merge_sequence`, then `pop_first_token`, repeated) and echoes
  the separator line. Otherwise it appends the token (length from
  `get_phrase_item(token).get_phrase_length()`). While the queued character
  length is `≥ MAX_PHRASE_LENGTH`, it merges and pops from the front.
- `merge_sequence` (`:78-126`) tries the whole queued span, then shrinks
  it from the **end** until `search(seq_len chars)` is `SEARCH_OK`. On a
  hit it replaces that front prefix with the single merged token
  (`g_array_remove_range(0, index)` and a prepend). `get_first_token`
  takes the lowest token id.
- `pop_first_token` emits `"{token} {utf8(first token_len chars)}\n"` and
  drops the token from both queues.
- At EOF it feeds a synthetic `"0 "` line to flush, so a trailing separator
  is always emitted.

---

## 3. Seeding (`gen_unigram`): the frequency-1 floor (`utils/training/gen_unigram.cpp`)

- Loops over `PHRASE_INDEX_LIBRARY_COUNT` and seeds only the tables whose
  `m_file_type` is `SYSTEM_FILE` or `DICTIONARY` (`gen_unigram.cpp:45-47`).
- For each token in range, calls `add_unigram_frequency(token, 1)`
  (`gen_unigram.cpp:65-68`).
- Saves the phrase index and the dictionary (`gen_unigram.cpp:72-76`).
- Runs for the default tables (`gen_unigram.cpp:106`) and the addon tables
  (`gen_unigram.cpp:110`).
- The header comment gives the purpose: to avoid a zero value when the
  unigram frequency is computed in float format (`gen_unigram.cpp:38-40`).

Every vocabulary token starts at frequency **1** before corpus counting,
so no token's unigram frequency is zero. The seeder must reproduce this
floor over the same `SYSTEM_FILE`/`DICTIONARY` table set.

---

## 4. Counting (`gen_ngram`) (`utils/training/gen_ngram.cpp:78-118`)

`gen_ngram` reads segmented lines from stdin, one token string per line,
and keeps the previous and current token across lines:

```text
while (std::getline(std::cin, linebuf)) {                     // :79
  TAGLIB_PARSE_SEGMENTED_LINE(&phrase_index, token, linebuf);   // :80
  last_token = cur_token; cur_token = token;                    // :82-83
  if (null_token == cur_token) continue;                        // :85-87
  phrase_index.add_unigram_frequency(cur_token, 1);             // :89-90
  if (null_token == last_token) {                               // :93
      if (!train_pi_gram) continue;                             // :94-95
      last_token = sentence_start;                              // :97
  }
  SingleGram * single_gram = bigram.load(last_token);           // :101-105 (new if absent)
  freq = single_gram->get_freq(cur_token)
       ? set_freq(cur_token, freq+1) : insert_freq(cur_token, 1); // :108-111
  single_gram->set_total_freq(get_total_freq() + 1);            // :113-114
  bigram.store(last_token, single_gram);                        // :116
}
```

Rules, in order:

1. **Token extraction**: `TAGLIB_PARSE_SEGMENTED_LINE`
   (`utils/utils_helper.h:51-74`) splits at the first space or tab. On an
   empty line the token stays `null_token`.
2. **Unigram**: every non-`null_token` token adds 1 to its unigram
   frequency (`gen_ngram.cpp:89-90`), on top of the §3 floor.
3. **Sentence boundary**: when `last_token == null_token`, the bigram is
   trained against `sentence_start`, unless `--skip-pi-gram-training` is
   set, which skips the pair (`gen_ngram.cpp:93-97`).
4. **Bigram**: `bigram.load(last_token)` fetches or creates the
   `SingleGram` of the previous token. The `(prev → cur)` count goes up by
   one (`set_freq` on a hit, `insert_freq` on a miss), and so does the
   `SingleGram`'s total (`gen_ngram.cpp:101-116`).

`--bigram-file` defaults to `SYSTEM_BIGRAM`.

### 4.1 The stored value is the raw count (`src/storage/ngram.cpp`)

The on-disk `SingleGramItem` field is named `m_freq` but stores the **raw
count** (`ngram.cpp:30-33`). `insert_freq` stores the given value
(`ngram.cpp:178-189`), and `set_freq` overwrites it with the raw count
(`ngram.cpp:252-266`). The normalised probability exists only at
retrieval: `retrieve_all` emits `m_count = m_freq` and
`m_freq = m_freq / total_freq` (`ngram.cpp:133-146`). `total_freq` is the
sum of the raw counts (`ngram.cpp:48-53`).

The value-level comparison target is therefore the raw bigram count
`prev → (cur → count)` and the raw unigram count, which the textual export
dumps (§8).

### 4.2 `gen_deleted_ngram`: held-out counting (`utils/training/gen_deleted_ngram.cpp`)

The same as `gen_ngram`, except that it does not add unigram frequency,
writes to `DELETED_BIGRAM` (the default) instead of `SYSTEM_BIGRAM`, and
does not save the phrase index. It counts the held-out slice whose bigram
counts drive the λ estimate (§5).

---

## 5. λ estimation (`estimate_interpolation`): deleted-interpolation EM (`utils/training/estimate_interpolation.cpp`)

`compute_interpolation(deleted_bigram, unigram, bigram)`
(`estimate_interpolation.cpp:30-88`) estimates the weight λ that blends
the bigram model with the unigram model, by expectation-maximisation over
the held-out `DELETED_BIGRAM`:

```text
lambda = 0; next_lambda = 0.6; epsilon = 0.001              // :34-35
while |lambda - next_lambda| > epsilon:                     // :37
    lambda = next_lambda; next_lambda = 0; table_num = 0    // :38-40
    for each (token, deleted_count) in deleted_bigram:      // :44-51
        # bigram term  P(token | context) = freq/total_freq
        elem_poss = bigram.get_freq(token) / bigram.total_freq   // :56-60
        numerator = lambda * elem_poss                            // :62
        # unigram term  P(token) = unigram_freq / total
        elem_poss = unigram_freq / get_phrase_index_total_freq()  // :69-71
        part_of_denominator = (1 - lambda) * elem_poss            // :73
        if numerator + denominator == 0: continue                 // :76-77
        next_lambda += deleted_count * numerator/(numerator + part_of_denominator)  // :79
    next_lambda /= deleted_bigram.total_freq (table_num)          // :81-82
return next_lambda                                                 // :86-87
```

Rules:

- The per-context bigram probability is `freq / total_freq` of the system
  bigram's `SingleGram` (`:56-60`); `assert(0 != total_freq)` guards the
  division (`:59`).
- The unigram term is `unigram_freq / get_phrase_index_total_freq()`
  (`:69-71`). The denominator is the sum of all unigram frequencies in the
  phrase index, floor included (§6), not the token count.
- `get_phrase_item` return code (`:68`): the code reads
  `if (!unigram->get_phrase_item(token, item))`. `get_phrase_item` returns
  `ERROR_OK == 0` on success (`src/storage/phrase_index.cpp:180-198`), so
  the block runs when the token exists. A port must not "fix" it to
  `if (result)`.
- `main` (`:119-139`) calls `compute_interpolation` **per deleted-bigram
  key** (per distinct `prev` token) and prints the **arithmetic mean** of
  the per-key λ values. The shipped λ is that mean, not one global EM over
  all held-out tokens.

The port reproduces the per-context EM with `epsilon = 0.001` and the seed
`next_lambda = 0.6`, then the arithmetic mean.

`evaluate.py` writes λ into `table.conf` (`make modify
LAMBDA_PARAMETER=…`), and the engine reads it back through
`system_table_info.get_lambda()`.
λ is not stored in `interpolation2.text` (§8).

---

## 6. Unigram-total semantics

`gen_ngram` and `gen_unigram` keep the unigram frequency per token in the
phrase index. `get_phrase_index_total_freq()` (§5) is the sum of those
frequencies over the whole phrase index. Because §3 seeds every
`SYSTEM_FILE`/`DICTIONARY` token at 1, the §5 unigram denominator is
non-zero for tokens absent from the corpus. Omitting the floor shrinks the
denominator and changes λ, so the floor and the total must both be
reproduced.

---

## 7. K-mixture model (`utils/training/k_mixture_model.h` and the eight `*_k_mixture_model*.cpp`)

The KMM path is a different store for the same counts, plus pruning and
conversion. `parameter_t` is `double`; counts are `guint32`.

### 7.1 Data model (`k_mixture_model.h:115-159`)

The magic header `KMMP` holds `m_WC` (Σ instances of all words), `m_N`
(document count) and `m_total_freq` (Σ unigram frequency). Each W1 array
header holds `m_WC` (Σ instances of W1) and `m_freq` (W1's unigram
frequency). Each (W1, W2) item holds `m_WC` (pair instance count;
`m_T ≡ m_WC`), `m_N_n_0` (documents containing the pair;
`n_0 = m_N − m_N_n_0`), `m_n_1` (documents with exactly one occurrence)
and `m_Mr` (the maximum instances of the pair in one document).

The model math (`k_mixture_model.h:45-118`, used only for pruning), with
`N` documents, pair total `T = m_WC`, `n_0` and `n_1`:
`α = 1 − n_0/N`; `γ = 1 − n_1/(N−n_0)`;
`B = (T−n_1)/(N−n_0−n_1)` (`2` when `T−n_1 == 0 && N−n_0−n_1 == 0`);
`Pr_G_3(k) = 1−α (k=0); α(1−γ) (k=1);
(αγ/(B−1))·(1−1/(B−1))^{k−2} (k>1)`.

### 7.2 `gen_k_mixture_model`: per-document counting (`:65-403`)

**Each input file is one document.** `read_document` builds two
per-document hashes, `unigram[token] → freq` and
`document[token1] → {token2 → count}`, with the boundary logic of
`gen_ngram` (`:72-130`): `std::getline`, `TAGLIB_PARSE_SEGMENTED_LINE`, skip
`cur == null_token`, add to the unigram, and if `prev == null_token` either
skip (`--skip-pi-gram-training`) or set `prev = sentence_start`; then add
to `document[prev][cur]`.

Then, per token1, `train_second_word` → `train_word_pair` folds the
document counts into the persistent KMM bigram:

- **Maximum-occurs filter** (defaults `g_maximum_occurs = 20`,
  `g_maximum_increase_rates = 3.0`): if the item exists, the cap is
  `max(20, ceil(m_Mr · 3.0))`; if `count > cap`, subtract `count` from the
  unigram hash (remove at zero, `abort` if it would go negative) and
  **skip** the pair. For a new item the cap is `20`.
- Accumulate: an existing item gets `m_WC += count; m_N_n_0++; if count==1
  m_n_1++; m_Mr = max(m_Mr, count)`. A new item gets `m_WC = count;
  m_N_n_0 = 1; m_n_1 = (count == 1); m_Mr = count`. Always
  `array_header.m_WC += count`. `m_T` stays equal to `m_WC`
  (`:168-169, 196-197`).
- `magic.m_WC += delta` (overflow-guarded), `magic.m_N++` per document;
  then `post_processing_unigram` adds the surviving unigram frequencies
  into each `array_header.m_freq` and `magic.m_total_freq`.

Within one document the result does not depend on the GLib hash order:
each accumulation is commutative, and each pair subtracts its own over-cap
count once. Across documents, order matters through `m_Mr` and
`ceil(m_Mr · rate)` when a pair recurs (§10).

### 7.3 `estimate_k_mixture_model` (`:36-155`)

Scores a candidate against the held-out `estimates.db`. For each token1 in
the deleted model it runs the §5 EM (`next_λ = 0.6`, `ε = 0.001`): per
held-out item, the bigram term is `item.m_WC / bigram_array_header.m_WC`,
the unigram term is `unigram_array_header.m_freq / magic.m_total_freq`,
`next_λ += deleted_count · λ·bigram / (λ·bigram + (1−λ)·unigram)`, then
`next_λ /= deleted_array_header.m_WC`. λ is computed per token1 with a
non-zero deleted WC. The printed score is `average lambda:%f` (Σλ / count).
`estimate.py` writes `estimate.index` (`subdir#model#score`) and sorts it
by score, **descending**, into `estimate.sorted.index`.

### 7.4 `merge_k_mixture_model` (`:38-238`)

A token-ordered merge-join of two candidates. A matching (W1, W2) gets
`m_WC +=; m_N_n_0 +=; m_n_1 +=; m_Mr = max`. Array headers and the magic
header sum field by field (`m_N` too). Items are stored token-sorted
(`retrieve_all` yields ascending `m_token`; the merge asserts it).

### 7.5 `validate_k_mixture_model` (`:28-139`)

Checks `magic.m_WC == magic.m_total_freq` (both non-zero); Σ array-header
`m_WC == magic.m_WC`; Σ array-header `m_freq == magic.m_total_freq`; per
single gram Σ item `m_WC == array_header.m_WC`; a zero-WC header has no
items (frequency-only headers are allowed). It exits with `ENODATA` on a
failure.

### 7.6 `prune_k_mixture_model -k 3 --CDF 0.99` (`:45-191`)

For each (W1, W2) item it computes `remained = 1 − Σ_{k=0}^{K−1}
Pr_G_3(k, N, T, n_0, n_1)`, the probability of at least K occurrences. It
clamps `|remained| < DBL_EPSILON` to 0 and aborts with `EDOM` out of range.
If `remained < CDF`, it **removes** the item and subtracts its `m_WC` from
`array_header.m_WC`, `magic.m_WC` and `magic.m_total_freq`. A post-pass
subtracts the removed `m_WC` from `array_header.m_freq`. Last, it drops
every array whose header is all zero.

### 7.7 KMM text format: `export` and `import` (`export_k_mixture_model.cpp:35-110`)

```text
\data model "k mixture model" count <m_WC> N <m_N> total_freq <m_total_freq>
\1-gram
\item <token> <phrase> count <arrayhdr.m_WC> freq <arrayhdr.m_freq>
…
\2-gram
\item <t1> <w1> <t2> <w2> count <m_WC> T <m_WC> N_n_0 <m_N_n_0> n_1 <m_n_1> Mr <m_Mr>
…
\end
```

Both sections are in `get_all_items` order (token-ascending). Per W1 the
2-gram items are in `retrieve_all` order (token2-ascending). `import` is
the exact reverse (taglib-parsed) and re-signs the magic and array headers.

### 7.8 `k_mixture_model_to_interpolation` (stdin → stdout, `:55-213`)

Parses the KMM text and emits the interpolation text (§8.1). The
`\1-gram` count is the KMM `freq` field; it drops `sentence_start`
(`:127-128`) and zero frequencies (`:133-134`). The `\2-gram` count is the
KMM `count` (`m_WC`). This is the shipped `interpolation2.text` format
([`interpolation2-grammar.md`](interpolation2-grammar.md)).

---

## 8. Output format

### 8.1 The textual interpolation format

`utils/storage/export_interpolation.cpp` (the legacy export) prints
`\data model interpolation\n` (`:33-35`), then `\1-gram\n` (`:76`) with
`\item <token> <phrase> count <freq>\n` per non-zero unigram (`:98`), then
`\2-gram\n` (`:107`) with `\item <t1> <w1> <t2> <w2> count <freq>\n`
(`:131-132`), then `\end\n` (`:38-40`). The bigram `freq` is
`item->m_count`, the raw count (`:128`). `k_mixture_model_to_interpolation`
emits the same sections.

### 8.2 Consumers

`oxpinyin-utils`' `import_interpolation` reads this format through the
pin's tagger ([`interpolation2-grammar.md`](interpolation2-grammar.md)). It
applies the `\1-gram` counts to the phrase-library chunks and the
`\2-gram` rows to `bigram.db`.

---

## 9. Evaluation: `estimate_interpolation` and `eval_correction_rate`

`evaluate.py` (`:53-116`) copies the candidate `interpolation2.text` into
the evaluation data directory, rebuilds the runtime `SYSTEM_BIGRAM` and
phrase index from it with `make`, runs `estimate_interpolation` (§5, over
`deleted_bigram.db`), writes λ into `table.conf` with `make modify`, then
runs `eval_correction_rate`.

`eval_correction_rate.cpp:36-208`:

- Loads `SYSTEM_PINYIN_INDEX`, the phrase index, `SYSTEM_BIGRAM` and the
  `table.conf` λ, and reads `evals2.text` (segmented token lines separated
  by `null_token`) as the test sentences.
- Per sentence, `get_possible_pinyin` takes each token's **highest-frequency**
  pronunciation to build the key sequence and a 1:1 matrix, then runs
  `PhoneticLookup<1,1>::get_nbest_match` (prefix `sentence_start`, single
  best) and compares the guessed UTF-8 with the original.
- `correction rate:%f` is passed / tested. A result is correct when the
  decoded sentence equals the source. Exactly one n-best result is
  asserted; a malformed line aborts.

The evaluator is a decode round trip. The `eval_correction_rate` of
`oxpinyin-utils`:

- reads the binary chunks and `bigram.db` that the data `Makefile` built;
- decodes with a term-for-term port of
  `PhoneticLookup<1,1>::get_nbest_match` (its `training::eval` module: a
  beam of 32, bigram before unigram expansion,
  `log((λ·P_bi + (1 − λ)·P_uni) · P_pron)` at the pin's float widths);
- reports the same rate.

It does **not** use the engine's typing scorer, which gives a different
rate.

---

## 10. Determinism

- **Hash iteration order**: upstream KMM export and merge walk
  `get_all_items` in the backend's native order. The `k_mixture_model`
  module walks the pin's own `FlexibleBigram` container in that order.
- **Document order**: KMM `m_Mr` and the maximum-occurs filter depend on
  the order of documents. The utilities take documents in the order the
  trainer passes them.
- **Floating point**: `parameter_t = double` throughout the KMM math and the
  λ EM. The ports use `f64` with the same operation order.
- **Locale**: upstream calls `setlocale(LC_ALL, "")`. The utilities make
  the pin's locale calls through the tool boundary
  ([external I/O boundary](external-io-boundary.md)).
- **Last line**: `gen_ngram`, `gen_k_mixture_model` and `eval_correction_rate`
  read with `std::getline`, so a last line without a newline is processed
  (`gen_ngram.cpp:79`, `gen_k_mixture_model.cpp:74`, `eval_correction_rate.cpp:177`).

---

## 11. Comparison surface

libpinyin's training corpus is undocumented (the trainer ships no corpus
manifest), so parity is **algorithmic**: identical output on identical
input. Reproducing the shipped `interpolation2.text` counts is out of scope
without the original corpus.

| Stage | Comparison |
|---|---|
| spseg, ngseg, mergeseq | the pin's token stream, byte-exact |
| gen_unigram, gen_ngram, gen_deleted_ngram | the `export_interpolation` dump: unigram `token → count` and bigram `prev → (cur → count)` (§3, §4, §4.1), in token order; and the backend's own `MemoryChunk` and DBM files |
| KMM generate, merge, prune | the KMM export text |
| KMM estimate, `estimate_interpolation` | `average lambda:%f` |
| KMM → interpolation | `interpolation2.text`, textual equality |
| correction rate | `correction rate:%f` |

The utility compatibility cases freeze the pin's answers as Rust values
([utility runbook](../runbooks/tool-diff.md)).

---

## 12. Dependencies and non-goals

- A training run needs a documented, terms-cleared corpus. None is pinned
  or redistributable ([`model-provenance.md`](model-provenance.md)).
- No corpus vendoring, no byte-identical reproduction of the shipped
  `interpolation2.text` (§11) and no redistribution of the shipped model or
  tables ([`model-provenance.md`](model-provenance.md)).

---

## Source index

| Claim | Source | Lines |
|---|---|---|
| Constants | `src/include/novel_types.h` | 78, 119, 121, 122, 130 |
| Five-stage invocations | trainer `segment/generate/estimate/tryprune/evaluate.py` | whole |
| `mergeseq` alternative-path guard | trainer `generate.py` | 26-27, 112-113 |
| Configuration, status, epochs | trainer `lib/myconfig.py`, `lib/utils.py`, `docs/fileformat` | whole |
| `TAGLIB_PARSE_SEGMENTED_LINE` | `utils/utils_helper.h` | 51-74 |
| `ngseg` main loop and Viterbi wrapper | `utils/segment/ngseg.cpp` | 159-258, 70-103 |
| `PhraseLookup::get_best_match` | `src/lookup/phrase_lookup.cpp` | 119-157 |
| `spseg` DP and framing | `utils/segment/spseg.cpp` | 86-168, 261-341 |
| `mergeseq` merge | `utils/segment/mergeseq.cpp` | 78-197, 263-282 |
| `gen_unigram` floor | `utils/training/gen_unigram.cpp` | 38-40, 45-47, 65-68, 72-76, 106, 110 |
| `gen_ngram` counting loop | `utils/training/gen_ngram.cpp` | 78-118 |
| `gen_deleted_ngram` | `utils/training/gen_deleted_ngram.cpp` | whole |
| Raw count; `retrieve_all` split | `src/storage/ngram.cpp` | 30-33, 48-53, 133-146, 178-189, 252-266 |
| `estimate_interpolation` EM and mean | `utils/training/estimate_interpolation.cpp` | 30-88, 119-139 |
| `get_phrase_item` return convention | `src/storage/phrase_index.cpp` | 180-198 |
| KMM data model and math | `utils/training/k_mixture_model.h` | 45-159 |
| KMM per-document counting | `utils/training/gen_k_mixture_model.cpp` | 65-403 |
| KMM estimate | `utils/training/estimate_k_mixture_model.cpp` | 36-155 |
| KMM merge | `utils/training/merge_k_mixture_model.cpp` | 38-238 |
| KMM validate | `utils/training/validate_k_mixture_model.cpp` | 28-139 |
| KMM prune | `utils/training/prune_k_mixture_model.cpp` | 45-191 |
| KMM text export and import | `utils/training/export_k_mixture_model.cpp`, `import_k_mixture_model.cpp` | whole |
| KMM → interpolation | `utils/training/k_mixture_model_to_interpolation.cpp` | 55-213 |
| Interpolation text export | `utils/storage/export_interpolation.cpp` | 33-40, 76-143 |
| Correction-rate decode | `utils/training/eval_correction_rate.cpp` | 36-208 |
| Bootstrap of the segmenter's bigram | trainer `docs/fileformat`, `*.py` | whole |
