# Trainer-workflow parity audit

Date: 2026-08-30 · Status: **SHOWN-verified against the pinned upstream
sources below** · Current state: §2–§4 and §14

This finding traces the call graph of the libpinyin trainer workflow from
source, classifies every helper as required, optional or off-path, and
characterises the file formats and algorithms the trainer's utilities use.

oxpinyin supports the trainer by providing libpinyin's utilities under
their own names (`oxpinyin-utils`). The trainer's own Python scripts stay
external and drive those utilities, as they drive libpinyin's
([packaging: trainer utility layout](../packaging.md#trainer-utility-layout)).
The per-utility compatibility cases are in
[the utility runbook](../runbooks/tool-diff.md).

Every claim is tagged **SHOWN** (read directly from the cited source line)
or **INFERRED** (cross-file orchestration or absence).

---

## 0. Source pins

| Component | Repo | Commit / tag | Role |
|---|---|---|---|
| libpinyin | `github.com/libpinyin/libpinyin` | `0c5e80e1200f84fab185d1c5bde458b770a0636c` (tag `2.11.91`) at the audit; the current pin is `074a2219` (`tools/oracle/oracle-pin.txt`) | backend + `utils/` tools |
| trainer | `github.com/libpinyin/trainer` | `b1927376735c0c1042d72af2eca0e25c53595724` (2024-09-02, `main`) | Python orchestration |

The trainer repo carries no release tags; `main` at the audit date is
recorded above. Paths below are relative to each repo root.

The trainer is Python glue that shells out to compiled `utils/` binaries;
the load-bearing algorithms live in C++.

---

## 1. The actual call graph (SHOWN)

`lib/myconfig.py` fixes every parameter; `lib/utils.py` is the JSON status
layer; `lib/dirwalk.py` walks `*.index` corpus files. Three disjoint
pipelines share the segmented-corpus input.

### 1.1 Main model pipeline — five stages, KMM throughout

Traced from the five `__main__` scripts. Each stage is epoch-gated by a
`.status` JSON file (`utils.check_epoch`).

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

**SHOWN** invocation inventory (`grep -oE 'utils/(segment|training|storage)/[a-z_]+'`
over `*.py`, excluding `tools/`):

| Utility | Invoked by | Reachable? |
|---|---|---|
| `segment/ngseg` | `segment.py` | **yes** (default) |
| `segment/spseg` | `segment.py --fast` | **yes** |
| `segment/mergeseq` | `mergeseq.py` | **yes** (word-recog corpus prep; §1.2) |
| `training/gen_k_mixture_model` | `generate.py` | **yes** |
| `training/estimate_k_mixture_model` | `estimate.py` | **yes** |
| `training/merge_k_mixture_model` | `tryprune.py` | **yes** |
| `training/validate_k_mixture_model` | `tryprune.py` | **yes** |
| `training/prune_k_mixture_model` | `tryprune.py` | **yes** |
| `training/export_k_mixture_model` | `tryprune.py` | **yes** |
| `training/k_mixture_model_to_interpolation` | `tryprune.py` | **yes** |
| `training/estimate_interpolation` | `evaluate.py` | **yes** |
| `training/eval_correction_rate` | `evaluate.py` | **yes** |
| `storage/gen_pinyin_table` | `tools/convertopengram.py`, `tools/striptones.py` | optional (dict prep; §1.4) |

**Not reachable from any trainer script** (verified by grep — no
invocation exists): `training/gen_ngram`, `training/gen_unigram`,
`training/gen_deleted_ngram`, `storage/export_interpolation`,
`storage/import_interpolation`, `training/import_k_mixture_model`,
`storage/gen_binary_files`, `storage/gen_zhuyin_table`.

The legacy counting path (`gen_unigram` + `gen_ngram` +
`gen_deleted_ngram` + `export_interpolation`) is **not invoked by the
trainer**. The corpus counter for the shipped model is
`gen_k_mixture_model`; `k_mixture_model_to_interpolation` produces the
shipped `interpolation2.text` from a merged and pruned KMM. The legacy
utilities remain valid libpinyin tools, and `estimate_interpolation` is on
the trainer's path inside `evaluate.py`. `evaluate.py`'s `make` also runs
`gen_binary_files`, `import_interpolation` and `gen_unigram` through the
data `Makefile` (pin `data/Makefile.am:60-62`).

### 1.2 Word-recognition pipeline (SHOWN, `docs/wordrecogimpl`)

Separate entry points, pure Python + SQLite (no libpinyin binary):

```text
prepare.py     → create 1..N-gram SQLite DBs (N=7)                     Prepare
populate.py    → count word-history n-grams from segmented corpus,     Populate
                 prune freq ≤ 1 per pass
partialword.py → threshold from dict word freqs (last 50%);            PartialWord
                 iterate: pull 2-gram items freq>threshold as partial
                 words; merge sequences down through the n-gram tables
                 (FTS3); write partialword.txt
newword.py     → build bigram table; prefix/postfix information        NewWord
                 entropy thresholds (last 60%); filter partial words →
                 newword.txt
markpinyin.py  → recursively assign pinyin+freq to recognised words    MarkPinyin
                 from atomic (oldwords.txt) and merged (partialword.txt)
                 → recognized.txt
```

`mergeseq.py` (→ `segment/mergeseq`) is an **alternative** corpus-prep
step: `generate.py` refuses to run if the `MergeSequence` epoch is signed
(`generate.py:26-27,112-113`, "Please skip mergeseq"). So mergeseq and the
KMM `generate` stage are mutually exclusive on a given corpus tree;
mergeseq feeds a merged-sequence corpus, not the KMM candidate models.

### 1.3 Punctuation pipeline (SHOWN, `genpunct.py`)

Single entry point, pure Python (no libpinyin binary): scans the segmented
corpus for (word, following-punctuation) pairs over a fixed 11-punctuation
search list, counts, prunes per-index (threshold 500) then globally
(threshold 10000), sorts each word's puncts by frequency desc, emits
`puncts.table` as `token word punct freq` lines.

### 1.4 `tools/` helpers — OpenGram dictionary integration (SHOWN)

None are on the main/word-recog/punct critical path. `distill.py`,
`convertopengram.py`, `filteropengram.py`, `mergepartialopengram.py`,
`striptones.py` convert/filter the external OpenGram dictionary into
libpinyin's `gen_pinyin_table` input format. `merge.py` merges
`recognized.txt` outputs across categories. `interpolation_to_kmm.py` is a
Python inverse of `k_mixture_model_to_interpolation` (round-trip helper,
pairs with the off-path `import_k_mixture_model`).

---

## 2. Scope

oxpinyin reproduces every libpinyin utility the trainer invokes (§1.1), and
the off-path ones (§11), as `oxpinyin-utils` binaries under the pin's names,
options, messages, inputs and outputs. The utilities read and write the
backend's own files, the same files a libpinyin build of the utility
reads and writes.

The trainer's Python scripts — orchestration, status files, the
word-recognition pipeline (§1.2, §8) and the punctuation pipeline (§1.3,
§9) — are not libpinyin code. They stay external and run unchanged against
the staged utilities ([packaging](../packaging.md#trainer-utility-layout)).

---

## 3. Capability matrix (trainer → oxpinyin)

| Capability | Trainer entry | libpinyin source | oxpinyin |
|---|---|---|---|
| ngseg (bigram Viterbi) | `segment.py` | `utils/segment/ngseg.cpp` | `oxpinyin-utils` `ngseg` |
| spseg (fewest-words DP) | `segment.py --fast` | `utils/segment/spseg.cpp` | `oxpinyin-utils` `spseg` |
| mergeseq (phrase merge) | `mergeseq.py` | `utils/segment/mergeseq.cpp` | `oxpinyin-utils` `mergeseq` |
| KMM generate, estimate, merge, validate, prune, export, import | `generate.py`, `estimate.py`, `tryprune.py` | `utils/training/*_k_mixture_model.cpp` | `oxpinyin-utils` binaries over `oxpinyin-kmm` |
| KMM → interpolation | `tryprune.py` | `k_mixture_model_to_interpolation.cpp` | `oxpinyin-utils` `k_mixture_model_to_interpolation` |
| eval runtime model build | `evaluate.py` `make` | `data/Makefile.am:60-62` | `oxpinyin-utils` `gen_binary_files`, `import_interpolation`, `gen_unigram` |
| λ estimate (deleted-interp EM) | `evaluate.py` | `estimate_interpolation.cpp` | `oxpinyin-utils` `estimate_interpolation` |
| correction rate | `evaluate.py` | `eval_correction_rate.cpp` | `oxpinyin-utils` `eval_correction_rate` over `oxpinyin-training::eval` |
| orchestration, status, word recognition, punctuation | the Python scripts | *(Python)* | external (the trainer's own scripts) |
| legacy gen_ngram, gen_unigram, gen_deleted_ngram, export_interpolation | *(off-path)* | `utils/training`, `utils/storage` | `oxpinyin-utils` binaries |
| OpenGram dict tools | `tools/*.py` | `gen_pinyin_table.cpp` | `oxpinyin-utils` `gen_pinyin_table`; the Python helpers stay external (§11) |

---

## 4. The legacy interpolation utilities

`gen_ngram`, `gen_deleted_ngram` and `export_interpolation` are libpinyin
utilities that exist in `utils/` but are **not invoked by the trainer**
(§1.1). `oxpinyin-utils` provides them because they are part of the pin's
utility set. `gen_unigram` and `estimate_interpolation` are on the
trainer's path (§1.1, §7).

---

## 5. Segmentation — `spseg` and `mergeseq` (SHOWN)

Both depend only on the phrase table (dictionary): `search(span) → (ok,
continued, tokens)`, `text(token)` and char length. Neither needs the
bigram, unlike `ngseg`.

### 5.1 `spseg` (`spseg.cpp:83-165, 256-339`)

Fewest-words shortest-path segmentation. Same three-state grouping as
`ngseg` (`CONTEXT_INIT/SEGMENTABLE/UNKNOWN`, per-character
`search(1,·)&SEARCH_OK`), but a *segmentable* run is split by an O(n²) DP
minimising the **word count** `m_nword`, not a bigram score:

- `steps[0].m_nword = 0`; for each start `i`, for each end `k=i+1..n`:
  search span `[i,k)`. If not `SEARCH_OK` and `len≠1`, skip; a length-1
  span that misses still gets `token=null_token` and counts as a word.
  `nword = steps[i].m_nword + 1`. Relax `steps[k]` only on **strict**
  improvement (`nword < steps[k].m_nword`), so the first (leftmost,
  longest-first via k ascending) path wins ties. Break the inner loop when
  the span is not `SEARCH_CONTINUED` (no longer phrase can extend it).
- Backtrace from `n` via `m_backward_nstep`, reverse, emit
  `"{token} {utf8}\n"` per segment.
- **Line handling** (`spseg.cpp:264-339`): strip trailing `\n`; non-UCS4
  or empty line → `"0 \n"`; `--generate-extra-enter` appends `"0 \n"` per
  line; **always** a final `"0 \n"` at file tail. Unknown run →
  `deal_with_unknown` prints `"0 {raw}\n"`.

Contrast with `ngseg`: identical framing (states, unknown handling, tail
enter, error behaviour); only the segmentable-run scorer differs
(word-count DP vs bigram Viterbi). In `oxpinyin-utils` the two share the
`segment` module's line framing and lexicon.

### 5.2 `mergeseq` (`mergeseq.cpp:75-194, 259-283`)

Reads a segmented stream (`{token} {phrase}` lines, `0 …` separators),
greedily merges maximal adjacent runs that form a known dictionary phrase
(≤ `MAX_PHRASE_LENGTH = 16` chars), emits the merged stream:

- Maintain a queue of `TokenInfo{token, char_len}` and the concatenated
  UCS4 buffer. `feed_line`: parse token; on `null_token`, flush the queue
  (`merge_sequence` then `pop_first_token`, repeat) and echo the
  separator line verbatim; else append the token (length from
  `get_phrase_item(token).get_phrase_length()`), and while the queued
  char-length `≥ MAX_PHRASE_LENGTH` merge+pop from the front.
- `merge_sequence` (`:75-123`): try the whole queued span; shrink from the
  **end** (drop last token, reduce `seq_len`) until `search(seq_len chars)`
  is `SEARCH_OK`; on hit, replace that front prefix with the single merged
  token (`g_array_remove_range(0, index)` + prepend). `get_first_token`
  picks the first (lowest) token id — matches the lexicon's sorted tokens.
- `pop_first_token`: emit `"{token} {utf8(first token_len chars)}\n"`,
  drop it from both queues.
- EOF: feed a synthetic `"0 "` line to flush, so a trailing separator is
  always emitted.

---

## 6. K-mixture model — data model, formats, algorithms (SHOWN)

`utils/training/k_mixture_model.h` + the eight `*_k_mixture_model*.cpp`.
`parameter_t = double` (`novel_types.h:130`); counts are `guint32`.

### 6.1 Data model (`k_mixture_model.h:115-159`)

Magic header `KMMP`: `m_WC` (Σ instances of all words), `m_N` (document
count), `m_total_freq` (Σ unigram freq). Per-W1 array header:
`m_WC` (Σ instances of W1), `m_freq` (unigram freq of W1). Per-(W1,W2)
item: `m_WC` (pair instance count; `m_T ≡ m_WC`), `m_N_n_0` (docs
containing the pair; `n_0 = m_N − m_N_n_0`), `m_n_1` (docs with exactly
1 occurrence), `m_Mr` (max instances of the pair in any single seen doc).

Model math (used only for pruning): with `N` docs, pair total `T=m_WC`,
`n_0`, `n_1`:
`α = 1 − n_0/N`; `γ = 1 − n_1/(N−n_0)`;
`B = (T−n_1)/(N−n_0−n_1)` (special-case `2` when `T−n_1==0 &&
N−n_0−n_1==0`); `Pr_G_3(k) = 1−α (k=0); α(1−γ) (k=1);
(αγ/(B−1))·(1−1/(B−1))^{k−2} (k>1)`.

### 6.2 `gen_k_mixture_model` — per-document counting (`:63-412`)

**Per input file = one document.** `read_document` builds two per-document
hashes: `unigram[token]→freq` and `document[token1]→{token2→count}`,
using the same boundary logic as `gen_ngram`: strip `\n`,
`TAGLIB_PARSE_SEGMENTED_LINE`, skip `cur==null_token`, bump unigram, if
`prev==null_token` then (skip if `--skip-pi-gram-training` else
`prev=sentence_start`), bump `document[prev][cur]`.

Then per token1, `train_second_word`→`train_word_pair` folds the document
counts into the persistent KMM bigram:

- **maximum-occurs filter** (defaults `g_maximum_occurs=20`,
  `g_maximum_increase_rates=3.0`): if the item exists, cap =
  `max(20, ceil(m_Mr·3.0))`; if `count > cap`, subtract `count` from the
  unigram hash (steal on zero, `abort` if it would go negative) and
  **skip** the pair. If the item is new, the cap is the bare `20`.
- accumulate: existing → `m_WC += count; m_N_n_0++; if count==1 m_n_1++;
  m_Mr = max(m_Mr, count)`. New → `m_WC=count; m_N_n_0=1;
  m_n_1=(count==1); m_Mr=count`. Always `array_header.m_WC += count`.
- `magic.m_WC += delta` (Σ array-header WC growth; overflow-guarded),
  `magic.m_N++` per document, then `post_processing_unigram` adds the
  surviving unigram freqs into each `array_header.m_freq` and
  `magic.m_total_freq`.

Determinism note: upstream iterates GLib hashes in unspecified order, but
the accumulation is commutative per pair (`+=`, `max`) **except** the
maximum-occurs unigram subtraction, which is order-independent too (each
pair subtracts its own over-cap count once). Document *order* matters only
through `m_Mr`/`ceil(m_Mr·rate)` when the same pair recurs across docs —
so documents are processed in the trainer's file order (index order);
within a document the result is order-free. See §12.

### 6.3 `estimate_k_mixture_model` (`:36-155`)

Scores a candidate against the held-out `estimates.db` (a fixed KMM built
once from a held-out slice). For each token1 present in the deleted model,
run the deleted-interpolation EM `compute_interpolation` (seed
`next_λ=0.6`, `ε=0.001`): per held-out item, bigram term
`elem = item.m_WC / bigram_array_header.m_WC`, unigram term
`elem = unigram_array_header.m_freq / magic.m_total_freq`,
`next_λ += deleted_count · λ·bigram / (λ·bigram + (1−λ)·unigram)`, then
`next_λ /= deleted_array_header.m_WC`. λ per token1 with non-zero deleted
WC; the score printed is `average lambda:%f` = Σλ / count. `estimate.py`
parses that line, writes `estimate.index` (`subdir#model#score`), sorts by
score **descending** into `estimate.sorted.index`.

### 6.4 `merge_k_mixture_model` (`:38-238`)

Token-ordered merge-join of two candidates. Matching (W1,W2):
`m_WC +=; m_N_n_0 +=; m_n_1 +=; m_Mr = max`. Array headers and magic
header sum field-wise (`m_N` sums too). Confirms items are stored
token-sorted (`retrieve_all` yields ascending m_token; the merge asserts
it).

### 6.5 `validate_k_mixture_model` (`:28-139`)

Consistency: `magic.m_WC == magic.m_total_freq` (both non-zero); Σ
array-header `m_WC == magic.m_WC`; Σ array-header `m_freq ==
magic.m_total_freq`; per single-gram Σ item `m_WC == array_header.m_WC`;
a zero-WC header must have zero items (freq-only headers allowed). Exit
`ENODATA` on failure.

### 6.6 `prune_k_mixture_model -k 3 --CDF 0.99` (`:45-191`)

Per (W1,W2) item compute `remained = 1 − Σ_{k=0}^{K−1} Pr_G_3(k, N, T,
n_0, n_1)` (= P(occurrences ≥ K)); clamp `|remained|<DBL_EPSILON`→0;
`EDOM`-abort on out-of-range. If `remained < CDF (0.99)`, **remove** the
item and decrement `array_header.m_WC`, `magic.m_WC`, `magic.m_total_freq`
by its `m_WC`; post-pass subtract removed `m_WC` from `array_header.m_freq`;
finally drop any array whose header is fully zero. (Keeps only bursty
pairs whose P(≥K occurrences per doc) ≥ 0.99.)

### 6.7 KMM text format — `export`/`import` (`export…:35-110`, `import…`)

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

Order: `get_all_items` (token-ascending) for both sections; per W1 the
2-gram items are `retrieve_all` (token2-ascending). `import` is the exact
reverse (taglib-parsed) and re-signs the magic/array headers.

### 6.8 `k_mixture_model_to_interpolation` (stdin→stdout, `:59-217`)

Parses the KMM text (from `export`), emits the interpolation text:

```text
\data model interpolation
\1-gram
\item <token> <phrase> count <freq>      # from the KMM \1-gram `freq` field;
                                         # drops sentence_start(=1); drops freq==0
\2-gram
\item <t1> <w1> <t2> <w2> count <count>  # from the KMM \2-gram `count`(=m_WC)
\end
```

This is the shipped `interpolation2.text` format
([`interpolation2-grammar.md`](interpolation2-grammar.md)).

---

## 7. Evaluation — `estimate_interpolation` + `eval_correction_rate`

`evaluate.py` (`:53-116`) copies the candidate `interpolation2.text` into
the eval data dir, `make`-rebuilds the runtime `SYSTEM_BIGRAM`+phrase
index from it, runs `estimate_interpolation` (deleted-interpolation EM
over `deleted_bigram.db`, average λ), writes λ into `table.conf` via
`make modify`, then `eval_correction_rate`.

`eval_correction_rate.cpp:34-215` (**SHOWN**) — the reusable insight:

- Loads `SYSTEM_PINYIN_INDEX`, phrase index, `SYSTEM_BIGRAM`,
  `table.conf` λ; reads `evals2.text` (segmented token lines, `null_token`
  separated) as **test sentences**.
- Per sentence: `get_possible_pinyin` picks each token's **highest-freq**
  pronunciation → key sequence; builds a trivial 1:1 matrix; runs
  `PhoneticLookup<1,1>::get_nbest_match` (prefix `sentence_start`, single
  best) → guessed tokens; compares guessed UTF-8 to the original.
- `correction rate:%f` = passed / tested (a **correct** result = decoded
  sentence equals the source; **incorrect** = differs; exactly one nbest
  result asserted; malformed lines `abort`).

The evaluator is therefore a *decode round-trip*. The
`eval_correction_rate` of `oxpinyin-utils`:

- reads the binary chunks and `bigram.db` that the data `Makefile` built;
- decodes with a term-for-term port of
  `PhoneticLookup<1,1>::get_nbest_match` (`oxpinyin-training::eval`: beam
  of 32, bigram before unigram expansion,
  `log((λ·P_bi + (1 − λ)·P_uni) · P_pron)` at the pin's float widths);
- reports the same rate.

It does **not** use the engine's typing scorer.
Recorded evidence: an engine-scorer evaluator measured `0.52` against the
pin's `0.88`, and the port measured `0.880000`, equal to the pin, over 25
sentences.

---

## 8. Word recognition (SHOWN, `prepare/populate/partialword/newword/markpinyin.py`)

Upstream uses one SQLite DB per n-gram order (N=7) plus FTS3 for sequence
matching. The pipeline is the trainer's Python (§2); its outputs are
`partialword.txt`, `newword.txt` and `recognized.txt`.

- **populate**: multi-pass (length 1..N). Per document, slide a window of
  `length` words (reset on `token==0`), key `" w1 w2 … "` (space-fenced),
  `UPDATE…+1 OR INSERT…1`; after each pass **prune** rows with
  `freq ≤ getPruneMinimumOccurrence()=1`.
- **partialword** (`myconfig`: `WordMinimumOccurrence=3`,
  `PartialWordThreshold=0.50`, `NgramMinimumOccurrence=9`,
  `MaximumIteration=20`, N=7): threshold = the freq of the word at position
  `−int(len·0.50)` in the ascending-freq list of dictionary words
  (`words.txt`) with freq ≥ 3. Then iterate ≤ 20 times: pull 2-gram items
  with `freq > threshold` as `(merged, prefix, postfix, freq)`; skip
  already-known/merged; append survivors to `partialword.txt`; for
  `i=N..2`, clone the high n-gram to an FTS table (rows with `freq ≥ 9`),
  and for each partial word merge every matched high-gram sequence into the
  next-lower gram (`UPDATE+matched_freq OR INSERT`, delete origin on
  insert); remember merged pairs; stop when a pass adds nothing.
- **newword** (`NewWordThreshold=0.60`, `MinimumEntropy=0.01`): build a
  bigram table from the 2-gram DB; prefix/postfix **information entropy**
  `H = −Σ p log p` over the bigram neighbours; threshold = entropy at
  position `−int(len·0.60)` in the ascending list of dict words with
  entropy ≥ 0.01 (computed separately for prefix and postfix). Keep a
  partial word iff both its prefix and postfix entropy ≥ the respective
  threshold → `newword.txt`.
- **markpinyin** (`DefaultPinyinTotalFrequency=100`,
  `MinimumPinyinFrequency=3`): atomic words from `oldwords.txt`
  (`phrase pinyin freq`); merged words from `partialword.txt`. For a merged
  word, recurse into prefix/postfix pinyin lists, combine
  `pinyin = pre "'" post`, `freq = 100·mfreq·pfreq·qfreq /
  msum/psum/qsum`; `mergePinyin` sums same-pinyin freqs, rescales to total
  100, `int()`-truncates, drops freq < 3 → `recognized.txt`
  (`word pinyin freq`).

Word-recog needs two dictionary-derived inputs the trainer supplies
out-of-band: `words.txt` (dictionary phrases, ≥2 chars) and `oldwords.txt`
(phrase + toneless pinyin + freq). Both come from the libpinyin phrase
tables.

---

## 9. Punctuation (SHOWN, `genpunct.py`)

Fixed search order (order-significant, `:19`):
`['……','…','，','。','；','？','！','：','“','”','、']`. Per document, track
`(prev_token, prev_str)`; when the current line is a `null_token`
separator whose raw text **starts with** one of the puncts (first match
wins), record `(prev_token,prev_str) → punct` and count. Per index: prune
puncts with `freq < 500`, write `repr(dict)` to
`punctuation-index.text`. Globally: merge per-index dicts (sum same-punct
freqs), prune `< 10000`, then per word sort puncts by freq **desc**, emit
`token word punct freq` lines to `puncts.table`.

The `repr()`/`eval()` intermediate files are Python implementation
details. `puncts.table` is the input `gen_binary_files --gen-punct-table`
compiles into `punct.bin`.

---

## 10. Corpus / index / status (SHOWN, `lib/*.py`, `docs/fileformat`)

- **Index files** `*.index`: lines `title#/rel/text/path`; content files
  `<n>.text`. `dirwalk.walkIndex` recurses, dispatching `*.index`.
- **Status files** `*.status`: JSON `{'<Pass>Epoch': n, …}`; `check_epoch`
  compares against `myconfig` epochs (equal ⇒ done, smaller ⇒ redo,
  larger ⇒ error). Resumability is real for generate (`GenerateTextEnd`/
  `GenerateModelEnd` checkpoints).
- **Minimum file size** filter: `getMinimumFileSize() = 1200·3 + 1200/2 =
  4200` bytes; smaller segmented texts are skipped (generate, populate,
  genpunct).
- **reduce.py**: flattens the category tree to N levels by concatenating
  sub-index files; optional — port only if the supported corpus layout
  needs it.

---

## 11. Helper classification

| Script/tool | Class | Rationale |
|---|---|---|
| segment/generate/estimate/tryprune/evaluate.py | **required** | the five-stage main pipeline |
| mergeseq.py | **required** | word-recog corpus prep (alt path) |
| prepare/populate/partialword/newword/markpinyin.py | **required** | word-recognition pipeline |
| genpunct.py | **required** | punctuation pipeline |
| reduce.py | **optional** | category flattening; corpus-layout dependent — see §11.1 |
| lib/{myconfig,utils,dirwalk}.py | **required** | config + status + walk (absorbed into orchestration) |
| tools/distill,convertopengram,filteropengram,mergepartialopengram,striptones.py | **obsolete/optional** | OpenGram-dictionary prep, external data source, off the supported workflow |
| tools/merge.py | **optional** | cross-category `recognized.txt` merge |
| tools/interpolation_to_kmm.py | **optional** | round-trip helper (pairs with off-path import) |
| gen_ngram/gen_unigram/gen_deleted_ngram, export/import_interpolation, gen_binary_files, gen_zhuyin_table | **off-path** | valid libpinyin utils, not invoked by the trainer |

### 11.1 `reduce.py` classification — **optional, not on any path**

Call-graph evidence (pin `trainer/reduce.py` read in full):

- **No caller.** `grep -rn` across `trainer/*.py` finds no `import reduce`
  and no reference to its functions (`iterateSubDirectory`, `mergeSubIndex`)
  anywhere but `reduce.py` itself. None of the five main-pipeline drivers,
  the word-recognition drivers, or `genpunct.py` invoke it. It is a
  standalone, manually-run CLI.
- **What it does.** `iterateSubDirectory(origdir, destdir, level)` recurses
  `level` directory levels into `origdir`; at level ≤ 0 it concatenates every
  `*.index` file under that subtree (`mergeSubIndex`, a plain `read_file` +
  `writelines`) into one `<newroot>.index` under `destdir`. It touches only
  the corpus **index directory layout** — never a model, a segmented file, a
  KMM, or an evaluation. It is a `find … -name '*.index' | xargs cat`-shaped
  reshaping of a deep per-category corpus hierarchy into a shallower one.
- **When it runs.** Before segmentation, once, by a corpus maintainer whose
  raw corpus is organised in more index levels than they want the pipeline to
  walk. `segment.py`'s `walkThroughIndex` already recurses the whole index
  tree, so a corpus already at the desired granularity never needs it.

**Class: optional / corpus-preparation, not part of the training algorithm.**
No stage depends on it. It is trainer Python and stays external (§2).

---

## 12. Determinism

Sources of nondeterminism in the upstream tools, and how the utilities
answer them:

- **Hash iteration order** — upstream KMM export and merge walk
  `get_all_items` in the backend's native order. `oxpinyin-kmm` walks the
  pin's own `FlexibleBigram` container in that same native order.
- **Document order** — KMM `m_Mr`/maximum-occurs is order-sensitive
  across documents. The utilities take documents in the order the trainer
  passes them.
- **Floating point** — `parameter_t = double` throughout the KMM math and
  the λ EM; the ports use `f64` with the same operation order.
- **Locale** — upstream calls `setlocale(LC_ALL,"")`; the utilities make
  the pin's locale calls through the tool boundary
  ([external I/O boundary](external-io-boundary.md)).

---

## 13. Comparison surface

| Stage | Canonical comparison |
|---|---|
| spseg, ngseg, mergeseq | the pin's token stream, byte-exact |
| KMM generate, merge, prune | the KMM export text |
| KMM estimate, `estimate_interpolation` | `average lambda:%f` |
| KMM → interpolation | `interpolation2.text`, textual equality |
| correction rate | `correction rate:%f` |

The utility compatibility cases freeze the pin's answers as Rust values
([utility runbook](../runbooks/tool-diff.md)).

---

## Source index

| Claim | Source | Lines |
|---|---|---|
| 5-stage invocations | trainer `segment/generate/estimate/tryprune/evaluate.py` | whole |
| word-recog pipeline | trainer `prepare/populate/partialword/newword/markpinyin.py`, `docs/wordrecogimpl` | whole |
| punctuation | trainer `genpunct.py` | 19-259 |
| mergeseq alt-path guard | trainer `generate.py` | 26-27, 112-113 |
| config parameters | trainer `lib/myconfig.py` | whole |
| status/epoch | trainer `lib/utils.py`, `docs/fileformat` | whole |
| spseg DP + framing | `utils/segment/spseg.cpp` | 83-165, 256-339 |
| mergeseq merge | `utils/segment/mergeseq.cpp` | 75-194, 259-283 |
| KMM data model + math | `utils/training/k_mixture_model.h` | 40-159 |
| KMM per-doc counting | `utils/training/gen_k_mixture_model.cpp` | 63-412 |
| KMM estimate EM | `utils/training/estimate_k_mixture_model.cpp` | 36-155 |
| KMM merge join | `utils/training/merge_k_mixture_model.cpp` | 38-238 |
| KMM validate | `utils/training/validate_k_mixture_model.cpp` | 28-139 |
| KMM prune CDF | `utils/training/prune_k_mixture_model.cpp` | 45-191 |
| KMM export/import text | `utils/training/export_k_mixture_model.cpp`, `import_k_mixture_model.cpp` | whole |
| KMM → interpolation | `utils/training/k_mixture_model_to_interpolation.cpp` | 59-217 |
| correction rate decode | `utils/training/eval_correction_rate.cpp` | 34-215 |
| taglib line parse | `utils/utils_helper.h` | 27-71 |

---

## 14. Implementation status

Every libpinyin utility the trainer invokes is an `oxpinyin-utils` binary
under the pin's name; `just trainer-layout` stages them for the trainer
([packaging](../packaging.md#trainer-utility-layout)). The trainer's
Python scripts run unchanged against that layout.
