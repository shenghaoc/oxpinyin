# libpinyin training pipeline — algorithm characterization (W9-T0)

Date: 2026-08-14 · Status: **SHOWN-verified against libpinyin 2.11.91 source, the pin at freeze**

The trainer's main pipeline runs the K-mixture-model (KMM) path: it
invokes `gen_k_mixture_model` (not `gen_ngram`) and produces the shipped
`interpolation2.text` with `k_mixture_model_to_interpolation` from a merged
and pruned KMM (§1, §7). [`trainer-parity-audit.md`](trainer-parity-audit.md)
is the record for the KMM data model, formats and per-document counting.
The legacy utilities this document characterises (`gen_unigram`,
`gen_ngram`, `gen_deleted_ngram`, `export_interpolation`) are valid
libpinyin utilities that the trainer does not invoke. §2–§5 (the
segmenters, the shared boundary logic and the `estimate_interpolation` λ
EM) apply to both paths; the λ EM runs inside `evaluate.py`.

This finding records, with source file and line citations, the algorithm
oxpinyin reproduces to train a model that is *algorithmically identical* to
libpinyin's on identical input. `oxpinyin-utils` provides the utilities
under their libpinyin names; `oxpinyin-training` and `oxpinyin-kmm` hold
the training-only code.

Every algorithmic claim below is tagged **SHOWN** (read directly from the cited
source) or **INFERRED** (not directly verifiable from the cited source, e.g.
cross-file behaviour, orchestration inferred from shell/Python glue, or the
shipped model's provenance, which upstream does not document).

## Source set

- libpinyin source: tag `2.11.91`, the pin at freeze, on disk at `/tmp/libpinyin-2.11.91`.
  Paths and line numbers below are relative to that tree.
- Trainer orchestration: `/tmp/tr/trainer-main/` (the `libpinyin/trainer`
  repository) — Python glue and docs only.
- The current pin is `074a2219` (`tools/oracle/oracle-pin.txt`).
  Some cited line numbers differ at `074a2219`; the citations are recorded evidence from the 2.11.91 tree.

Constants (all **SHOWN**, `src/include/novel_types.h`):

| Symbol | Value | Line |
|---|---|---|
| `ERROR_OK` | `0` | `novel_types.h:78` |
| `MAX_PHRASE_LENGTH` | `16` | `novel_types.h:119` |
| `null_token` | `0` | `novel_types.h:121` |
| `sentence_start` | `1` | `novel_types.h:122` |
| `parameter_t` | `double` | `novel_types.h:130` |

---

## 1. Pipeline overview

The trainer is a multi-epoch, status-file-driven pipeline. The authoritative
description is the trainer repo's `docs/fileformat` (5 stages), and the
orchestration is the Python scripts in the trainer repo (**SHOWN**, trainer
`docs/fileformat`; **INFERRED** where the Python glue binds the C++ tools
below).

Five stages:

1. **Segment** — `segment.py` runs `ngseg`/`spseg` over raw text to produce
   tokenized lines separated by `null_token` (§2).
2. **Generate** — `generate.py` runs `gen_k_mixture_model` per text to count
   the corpus into a K-mixture-model store (§7).
3. **Estimate** — `estimate.py` runs `estimate_k_mixture_model` and sorts
   candidate models by score (§7).
4. **Prune** — `tryprune.py` merges top-N candidates, prunes, and converts to
   `interpolation2.text` via `k_mixture_model_to_interpolation` (§7, §8).
5. **Evaluate** — `evaluate.py` builds the legacy `SYSTEM_BIGRAM` + phrase
   index from the converted model, runs `estimate_interpolation` to compute
   the interpolation weight λ (§5), writes λ into `table.conf`, then runs
   `eval_correction_rate` (**INFERRED** from `evaluate.py` flow).

Two disjoint model representations share one corpus-counting algorithm:

- **Legacy interpolation path** — `gen_unigram` (§3) + `gen_ngram` (§4) +
  `gen_deleted_ngram` (§4.3) + `estimate_interpolation` (§5). The trainer
  never invokes the first three. Only `estimate_interpolation` (§5) is on
  the trainer's path (inside `evaluate.py`).
- **K-mixture-model (KMM) path** — `gen_k_mixture_model` +
  `estimate_k_mixture_model` + prune + `k_mixture_model_to_interpolation`.
  This path produces the shipped `interpolation2.text`. Its corpus counting
  is algorithmically the same as `gen_ngram` (§7), and its λ math is the
  same deleted-interpolation EM (§5).

The segmenter needs a bigram to score Viterbi, but it produces the
corpus that trains that bigram. Seeding the segmenter with an existing
model resolves this bootstrap circularity: the shipped `SYSTEM_BIGRAM`, or
the model of a prior epoch (**INFERRED**: the trainer carries no
first-model generator; `segment.py`/`ngseg.cpp` consume an
already-populated bigram).

---

## 2. Segmenter (`ngseg` / `spseg`)

Two segmenters live in `utils/segment/`. They differ only in the *scoring* of a
segmentation; both consume raw UTF-8 on stdin and emit the tokenized
sentence-separated format the counters consume (§4).

### 2.1 `ngseg` — bigram-scored Viterbi (**SHOWN**, `utils/segment/ngseg.cpp`)

- Reads stdin line-by-line via `getline`; strips a trailing `\n`
  (`ngseg.cpp:184-186`).
- Validates the line as UCS-4 (`ngseg.cpp:189-196`); an empty line prints
  `null_token` alone (`ngseg.cpp:199-202`).
- Classifies each character via `phrase_table.search(1, char, tokens)` as
  *segmentable* (present in the dictionary) or *unknown*, using a three-state
  machine `CONTEXT_INIT / SEGMENTABLE / UNKNOWN` (`ngseg.cpp:204-210`).
- Groups contiguous same-state characters into `current_ucs4`; on a state
  boundary it calls `deal_with_segmentable` or `deal_with_unknown`
  (`ngseg.cpp:212-235`).
- `deal_with_segmentable` (`ngseg.cpp:67-91`) calls
  `phrase_lookup->get_best_match` (the bigram-scored Viterbi over the
  dictionary, `src/lookup/phrase_lookup.cpp:119-157`), converts the result to
  UTF-8 and prints it. On failure it prints `"Un-segmentable sentence"` to
  stderr and returns `false` — but the caller **ignores** the return value
  (`ngseg.cpp:248-249` ignore-path) (**SHOWN**).
- `deal_with_unknown` (`ngseg.cpp:93-100`) prints `null_token` followed by the
  raw (un-segmentable) text verbatim.
- With `--generate-extra-enter` it emits an extra `null_token` at end of line
  (`ngseg.cpp:248-249`), and always emits a final `null_token` at file tail
  (`ngseg.cpp:256`).
- The `PhraseLookup` is constructed with `lambda = system_table_info.get_lambda()`
  (`ngseg.cpp:167`), i.e. the bigram weight read from `table.conf` (§5).

### 2.2 `spseg` — fewest-words shortest path (**SHOWN**, `utils/segment/spseg.cpp`)

- Replaces the `PhraseLookup` Viterbi with a local "graph shortest path" DP that
  minimises the **number of words** (`m_nword`), *not* a bigram/unigram score
  (`spseg.cpp:48-138`).
- `segment()` (`spseg.cpp:83-138`) is O(n²): for each position `i` it tries
  every substring `i..k` via `phrase_table.search`; a 1-char phrase not found in
  the dictionary still gets `token = null_token` (`spseg.cpp:116-120`); it keeps
  the path with minimal `m_nword`. Backtrace reconstructs the path
  (`spseg.cpp:140-165`).
- Consequence: `spseg` = "fewest words" segmentation (no bigram required);
  `ngseg` = "best bigram-scored Viterbi" segmentation.

**W9 dependency:** the Rust trainer's segmentation stage must reproduce at
least one of these. `spseg` is the simpler parity target (no bigram needed);
`ngseg` needs the bigram Viterbi, which is the `get_best_match` DP already in
scope of the reimplementation stack.

---

## 3. Seeding (`gen_unigram`) — the freq-1 floor (**SHOWN**, `utils/training/gen_unigram.cpp`)

- Loops over `PHRASE_INDEX_LIBRARY_COUNT`; only seeds tables whose
  `m_file_type` is `SYSTEM_FILE` or `DICTIONARY` (`gen_unigram.cpp:45-47`).
- For each token in range, `add_unigram_frequency(token, 1)` (`gen_unigram.cpp:65-68`).
- Saves both `phrase_index` and the dictionary (`gen_unigram.cpp:72-76`).
- Called for default tables (`gen_unigram.cpp:106`) and addon tables
  (`gen_unigram.cpp:110`).
- The header comment states the purpose: "increase the value when corpus size
  becomes larger, to avoid zero value when computing unigram frequency in float
  format" (`gen_unigram.cpp:38-40`).

Every vocabulary token starts at frequency **1** before corpus counting, so no
token's unigram frequency is ever zero. The Rust seeder must reproduce this
freq-1 floor exactly, over the same `SYSTEM_FILE`/`DICTIONARY` table set.

---

## 4. Counting (`gen_ngram`) — the load-bearing algorithm (**SHOWN**, `utils/training/gen_ngram.cpp:78-125`)

`gen_ngram` is the core corpus counter. It reads segmented lines from stdin,
each line one token string, and maintains the previous and current token across
lines to accumulate bigram counts. The loop body:

```text
while (getline) {
  TAGLIB_PARSE_SEGMENTED_LINE(&phrase_index, token, linebuf);   // :86-87
  last_token = cur_token; cur_token = token;                    // :89-90
  if (null_token == cur_token) continue;                        // :92-94
  phrase_index.add_unigram_frequency(cur_token, 1);             // :96-97
  if (null_token == last_token) {                               // :100
      if (!train_pi_gram) continue;                             // :101-102
      last_token = sentence_start;                              // :104
  }
  SingleGram * single_gram = bigram.load(last_token);           // :108-112 (new if absent)
  freq = single_gram->get_freq(cur_token)
       ? set_freq(cur_token, freq+1) : insert_freq(cur_token, 1); // :115-118
  single_gram->set_total_freq(get_total_freq() + 1);            // :120-121
  bigram.store(last_token, single_gram);                        // :123
}
```

Rules, in order (**SHOWN**):

1. **Token extraction** — `TAGLIB_PARSE_SEGMENTED_LINE` (`utils/utils_helper.h:51-74`)
   splits at the first space/tab; on an empty line the token stays `null_token`.
2. **Unigram** — every non-`null_token` token increments its unigram frequency
   by 1 (`gen_ngram.cpp:96-97`), *on top of* the §3 freq-1 floor.
3. **Sentence boundary** — when `last_token == null_token` (previous line was a
   sentence separator), the bigram is trained against `sentence_start` instead,
   *unless* `--skip-pi-gram-training` is set, in which case the pair is skipped
   entirely (`gen_ngram.cpp:100-104`).
4. **Bigram** — `bigram.load(last_token)` fetches (or creates) the
   `SingleGram` for the *previous* token; the `(prev → cur)` count is
   incremented (`set_freq` on hit, `insert_freq` on miss) and the
   `SingleGram`'s total is incremented (`gen_ngram.cpp:108-123`).

### 4.1 The stored value is the raw count, not a probability (**SHOWN**, `src/storage/ngram.cpp`)

The on-disk `SingleGramItem` field is named `m_freq` but stores the **raw
count** (`ngram.cpp:30-33`). `insert_freq` stores the given value verbatim
(`ngram.cpp:178-189`, esp. `insert_item.m_freq = freq`); `set_freq` overwrites
with the raw count (`ngram.cpp:252-266`). The *normalised* probability is
computed only at retrieval: `retrieve_all` emits
`m_count = m_freq` (raw) and `m_freq = m_freq / total_freq` (normalised)
(`ngram.cpp:133-146`). The `total_freq` is the sum of raw counts
(`ngram.cpp:48-53`).

Consequence: the value-level parity target is the **raw bigram count**
`prev → (cur → count)`, and the raw unigram count, not a floating-point
probability. This is exactly what the textual export dumps (§8).

### 4.2 Flags and defaults (**SHOWN**, `utils/training/gen_ngram.cpp`)

- `--skip-pi-gram-training` disables the `sentence_start` substitution (§4.3).
- `--bigram-file` defaults to `SYSTEM_BIGRAM` (`gen_ngram.cpp`, main).

### 4.3 `gen_deleted_ngram` — held-out counting (**SHOWN**, `utils/training/gen_deleted_ngram.cpp`)

Identical to `gen_ngram` except: it does **not** increment the unigram
frequency (no `add_unigram_frequency`), and writes to `DELETED_BIGRAM` (default)
rather than `SYSTEM_BIGRAM`, and does not save the phrase index at the end. It
counts a *held-out* corpus slice whose bigram counts drive the λ estimation in
§5, while leaving unigram totals untouched.

---

## 5. λ estimation (`estimate_interpolation`) — deleted interpolation EM (**SHOWN**, `utils/training/estimate_interpolation.cpp`)

`compute_interpolation(deleted_bigram, unigram, bigram)`
(`estimate_interpolation.cpp:30-88`) estimates the interpolation weight λ that
blends the bigram (context) model with the unigram model, using the classic
deleted-interpolation expectation-maximisation over the held-out `DELETED_BIGRAM`.

Pseudo-code (**SHOWN**, exact lines cited):

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

Rules (**SHOWN**):

- The per-context bigram probability is `freq / total_freq` of the *system*
  bigram's `SingleGram` (`estimate_interpolation.cpp:56-60`), with
  `assert(0 != total_freq)` guarding division (`:59`).
- The unigram term is `unigram_freq / get_phrase_index_total_freq()`
  (`:69-71`). `get_phrase_index_total_freq()` is the **sum of all unigram
  frequencies in the phrase index** (freq-1 floor included), not the count of
  tokens (**SHOWN** via the caller; see §4.1's total semantics).
- **`get_phrase_item` return-code idiom** (`:68`): the code reads
  `if (!unigram->get_phrase_item(token, item))`. `get_phrase_item` returns
  `ERROR_OK == 0` on success and a non-zero error code otherwise
  (`src/storage/phrase_index.cpp:179-197`). Therefore `!result` is `true` on
  **success**, and the block runs when the token exists. This looks inverted
  but is correct under the `ERROR_OK == 0` convention; a faithful port must
  not "fix" it to `if (result)`.
- `main` (`estimate_interpolation.cpp:119-139`) calls `compute_interpolation`
  **per deleted-bigram key** (i.e. per distinct `prev` token) and then computes
  a **simple arithmetic mean** of the per-key λ values. The shipped λ is that
  mean, **not** a single global EM over all held-out tokens.

Consequence for parity (**SHOWN**): the Rust λ estimator must (a) reproduce the
per-context EM with `epsilon = 0.001` and seed `next_lambda = 0.6`, then
(b) average the per-context λ values arithmetically. The averaging step is what
makes the shipped λ a single scalar; the per-context λ values themselves are
computed but discarded except for the mean.

The λ is written to `table.conf` by `evaluate.py` (`make modify
LAMBDA_PARAMETER=…`), and read back by the engine via
`system_table_info.get_lambda()` (§2.1) (**INFERRED** — cross-file
orchestration, not a single C++ unit). It is **not** stored in
`interpolation2.text` (§8).

---

## 6. Unigram-total semantics (**SHOWN**)

`gen_ngram`/`gen_unigram` maintain the unigram frequency per token in the
phrase index; `get_phrase_index_total_freq()` (§5) is the sum of those
per-token frequencies over the whole phrase index. Because §3 seeds every
`SYSTEM_FILE`/`DICTIONARY` token at frequency 1, the unigram denominator in §5
is non-zero even for tokens absent from the corpus. A port that omits the
freq-1 floor changes the λ estimate, because the unigram denominator would
shrink. The floor and the total are coupled; both must be reproduced.

---

## 7. K-mixture-model (KMM) path (**SHOWN**, KMM sources)

oxpinyin reproduces the full KMM pipeline
([`trainer-parity-audit.md`](trainer-parity-audit.md)).

The KMM path is a *different storage* for the same counts, plus a pruning and
conversion stage:

- `k_mixture_model.h` (`utils/training/k_mixture_model.h:45-118`) defines the
  three-parameter K-mixture (α, γ, B) via `compute_alpha/gamma/B` and
  `compute_Pr_G_3`.
- `gen_k_mixture_model.cpp` counts the corpus with the **same** boundary logic
  as `gen_ngram` — the `null_token`/`sentence_start` substitution is byte-for-byte
  the same pattern (`gen_k_mixture_model.cpp:70-135`), but it stores
  `(prev, cur)` counts as `m_WC` (word count) in a
  document→second-word hash, with `m_T` (K-mixture total) kept equal to `m_WC`
  during counting (`gen_k_mixture_model.cpp:175-176, 203-204`).
- `estimate_k_mixture_model.cpp` runs the **same** deleted-interpolation EM
  structure as §5 (per-key λ then arithmetic mean), only against KMM storage.
- `k_mixture_model_to_interpolation.cpp` (`:78, :117-177`) reads a KMM text
  export and emits the `\data model interpolation` / `\1-gram` / `\2-gram`
  format — removing `sentence_start` from the unigram section (`:131-132`) and
  skipping zero-freq unigrams (`:137-138`).

The shipped file's count values were produced through KMM pruning (§1).

---

## 8. Output format

### 8.1 The textual interpolation format (**SHOWN**)

Two independent tools emit the identical textual format:

- `utils/storage/export_interpolation.cpp` (legacy path export): prints
  `\data model interpolation\n` (`:33-35`), then `\1-gram\n` (`:76`) with
  `\item <token> <phrase> count <freq>\n` per non-zero unigram (`:98`), then
  `\2-gram\n` (`:107`) with
  `\item <t1> <w1> <t2> <w2> count <freq>\n` (`:131-132`), then `\end\n`
  (`:38-40`). The bigram `freq` is `item->m_count`, the **raw count**
  (`:128`), consistent with §4.1.
- `utils/training/k_mixture_model_to_interpolation.cpp` (KMM path export) emits
  the same sections.

So `export_interpolation` is the **textual dump tool** the differential test
should compare against (§9).

### 8.2 Consumers

`oxpinyin-utils`' `import_interpolation` reads this format through the
pin's tagger ([`interpolation2-grammar.md`](interpolation2-grammar.md)).
It applies the `\1-gram` counts to the phrase-library chunks and the
`\2-gram` rows to `bigram.db`.

---

## 9. Differential-parity plan

Because libpinyin's training corpus is undocumented (**SHOWN** — the trainer
ships no corpus manifest), parity is **algorithmic**: identical output on
identical input, not reproduction of the shipped `interpolation2.text` bytes.

**Comparison surface (SHOWN):** `utils/storage/export_interpolation` emits the
value-level dump to compare against — `token → count` pairs (unigram) and
`prev → (cur → count)` triples (bigram) as plain text. The exporter
iterates in token order, so a sorted dump is the stable comparison form.

**Comparison:**

1. Pin one segmentation model (e.g. `spseg`, §2.2) so both sides consume the
   *same* tokenized input; feed identical raw text.
2. Run the same utilities on libpinyin and on `oxpinyin-utils`.
3. Dump both through `export_interpolation` and compare values:
   - unigram `token → count` (freq-1 floor + corpus counts, §3–§4),
   - bigram `prev → (cur → count)` raw counts (§4, §4.1),
   - λ = arithmetic mean of per-context EM results (§5).
4. Both sides write the backend's own `MemoryChunk` and DBM files, so the
   binary files are a second comparison surface.

The shipped `interpolation2.text` was produced through the KMM prune path
(§7). Without the original corpus, matching the shipped file's exact counts
is out of scope.

---

## 10. Dependencies and non-goals

**Dependencies:**

- A **training corpus** — none is pinned or redistributable
  ([`model-provenance.md`](model-provenance.md)); a training run needs a
  documented, terms-cleared corpus.
- A **segmenter** (§2): `oxpinyin-utils` provides `ngseg` and `spseg`.

**Non-goals:**

- No corpus vendoring.
- No byte-identical reproduction of the shipped `interpolation2.text` (§9).
- No redistribution of the shipped model or tables
  ([`model-provenance.md`](model-provenance.md)).

---

## Source index

| Claim | Source | Lines | Tag |
|---|---|---|---|
| Constants `ERROR_OK`, `null_token`, `sentence_start`, `MAX_PHRASE_LENGTH`, `parameter_t` | `src/include/novel_types.h` | 78, 119, 121, 122, 130 | SHOWN |
| `TAGLIB_PARSE_SEGMENTED_LINE` token extraction | `utils/utils_helper.h` | 51-74 | SHOWN |
| `ngseg` main loop / Viterbi wrapper | `utils/segment/ngseg.cpp` | 154-252, 67-91 | SHOWN |
| `spseg` fewest-words DP | `utils/segment/spseg.cpp` | 83-138, 140-165 | SHOWN |
| `PhraseLookup::get_best_match` Viterbi | `src/lookup/phrase_lookup.cpp` | 119-157 | SHOWN |
| `gen_unigram` freq-1 floor | `utils/training/gen_unigram.cpp` | 38-40, 45-47, 65-68, 72-76 | SHOWN |
| `gen_ngram` counting loop | `utils/training/gen_ngram.cpp` | 78-125 | SHOWN |
| `gen_deleted_ngram` held-out counter | `utils/training/gen_deleted_ngram.cpp` | (whole file) | SHOWN |
| `SingleGramItem.m_freq` = raw count; `retrieve_all` split | `src/storage/ngram.cpp` | 30-33, 133-146, 178-189, 252-266 | SHOWN |
| `estimate_interpolation` EM + averaging | `utils/training/estimate_interpolation.cpp` | 30-88, 119-139 | SHOWN |
| `get_phrase_item` `ERROR_OK == 0` convention | `src/storage/phrase_index.cpp` | 179-197 | SHOWN |
| KMM math + counting + conversion | `utils/training/k_mixture_model.h`, `gen_k_mixture_model.cpp`, `k_mixture_model_to_interpolation.cpp` | 45-118, 70-135, 78/117-177 | SHOWN |
| Textual interpolation export | `utils/storage/export_interpolation.cpp` | 33-40, 76-104, 106-143 | SHOWN |
| Multi-epoch orchestration / bootstrap | trainer `docs/fileformat`, `*.py` | (whole files) | INFERRED |
| Shipped corpus provenance | — | — | INFERRED (absent) |
