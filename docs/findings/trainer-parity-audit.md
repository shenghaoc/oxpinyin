# Trainer-workflow parity audit (W9 full-scope re-audit)

Date: 2026-08-30 · Status: **SHOWN-verified against the pinned upstream
sources below** · Supersedes the W9 scope decision in
`training-algorithm.md` §7 (KMM was declared out of scope; it is now
**in scope** — see `ROADMAP.md` W9 and §2 here).

This finding traces the *actual call graph* of the currently-used
libpinyin trainer workflow from source (not inferred from filenames),
maps every reachable capability to its OXpinyin status, classifies every
helper as required / optional / obsolete, and characterises the file
formats and algorithms the native Rust re-expression must reproduce.

Every claim is tagged **SHOWN** (read directly from the cited source line)
or **INFERRED** (cross-file orchestration or absence).

---

## 0. Source pins

| Component | Repo | Commit / tag | Role |
|---|---|---|---|
| libpinyin | `github.com/libpinyin/libpinyin` | `0c5e80e1200f84fab185d1c5bde458b770a0636c` (tag `2.11.91`) | backend + `utils/` tools — matches `docs/testing/oracle-environment.md` |
| trainer | `github.com/libpinyin/trainer` | `b1927376735c0c1042d72af2eca0e25c53595724` (2024-09-02, `main`) | Python orchestration |

The libpinyin pin is the repository's existing oracle version, unchanged.
The trainer repo carries no release tags; `main` at the freeze date is
recorded above. Paths below are relative to each repo root.

The trainer is Python glue that shells out to compiled `utils/` binaries;
the load-bearing algorithms live in C++. Python is **orchestration only**,
not the implementation target (task §2).

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

> **Audit correction to the prior W9 finding.** `training-algorithm.md`
> characterised the *legacy* counting path (`gen_unigram` + `gen_ngram` +
> `gen_deleted_ngram` + `export_interpolation`) as "the load-bearing
> algorithm W9 must reproduce". The call-graph trace shows that path is
> **not invoked by the trainer at all**. The load-bearing corpus counter
> is `gen_k_mixture_model`; the shipped `interpolation2.text` is produced
> by `k_mixture_model_to_interpolation` off a merged+pruned KMM. The
> legacy path remains a valid (manually-invokable) libpinyin utility and
> its λ EM (`estimate_interpolation`) *is* on the real path inside
> `evaluate.py`, but the counting/export halves are off-path. The
> existing `oxpinyin-counter` / `oxpinyin-emitter` reproduce the off-path
> legacy tools; see §4 for their reclassification.

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

## 2. New W9 scope (supersedes `training-algorithm.md` §7)

W9 is now **complete native-Rust parity with the currently-used trainer
workflow**: segmentation (`ngseg`/`spseg`/`mergeseq`), KMM generation and
optimisation (generate → estimate → merge → validate → prune → export →
convert), evaluation (`estimate_interpolation` λ + `eval_correction_rate`),
word recognition (prepare → populate → partialword → newword → markpinyin),
punctuation generation, and the corpus/index/status orchestration that
drives them.

Compatibility ground rules (task §2):

- Python is orchestration only; the Rust re-expression is the
  implementation target.
- SQLite (word-recog) and the Tkrzw/DBM stores (KMM) are upstream
  *implementation details*, replaced by Rust in-memory/`oxpinyin-store`
  representations; observable outputs — not on-disk byte layouts — are the
  compatibility target.
- Shelling out to libpinyin binaries is forbidden for the shipped
  implementation. `make` is not part of the OXpinyin training runtime; the
  `eval_correction_rate` runtime model is assembled natively from the
  candidate `interpolation2.text` + system tables (§7).
- The compatibility target is behaviour/values, not upstream architecture.

---

## 3. Capability matrix (trainer → OXpinyin) — **final (2026-08-31)**

Status key: **done** implemented & tested · **off-path** valid libpinyin
util the trainer never invokes · **excluded** deliberate non-port (§11).
Every capability the main / word-recognition / punctuation pipelines use is
**done**. Test evidence is in §15's tables; the arithmetic is verified
line-by-line in `docs/findings/kmm-arithmetic-audit.md`.

| Capability | Trainer entry | libpinyin source | Status | Home (implementation) |
|---|---|---|---|---|
| ngseg (bigram Viterbi) | `segment.py` | `utils/segment/ngseg.cpp` | **done** | `oxpinyin-segment::segment_bytes` |
| spseg (fewest-words DP) | `segment.py --fast` | `utils/segment/spseg.cpp` | **done** | `oxpinyin-segment::spseg` |
| mergeseq (phrase merge) | `mergeseq.py` | `utils/segment/mergeseq.cpp` | **done** | `oxpinyin-segment::mergeseq` |
| KMM data model | — | `utils/training/k_mixture_model.h` | **done** | `oxpinyin-kmm::model` |
| KMM generate (per-doc count) | `generate.py` | `gen_k_mixture_model.cpp` | **done** | `oxpinyin-kmm::generate` |
| KMM estimate (λ score) | `estimate.py` | `estimate_k_mixture_model.cpp` | **done** | `oxpinyin-kmm::estimate` |
| KMM merge | `tryprune.py` | `merge_k_mixture_model.cpp` | **done** | `oxpinyin-kmm::merge` |
| KMM validate | `tryprune.py` | `validate_k_mixture_model.cpp` | **done** | `oxpinyin-kmm::validate` |
| KMM prune | `tryprune.py` | `prune_k_mixture_model.cpp` | **done** | `oxpinyin-kmm::prune` |
| KMM export/import (text) | `tryprune.py` | `export_/import_k_mixture_model.cpp` | **done** | `oxpinyin-kmm::text` |
| KMM → interpolation | `tryprune.py` | `k_mixture_model_to_interpolation.cpp` | **done** | `oxpinyin-kmm::text::kmm_text_to_interpolation` |
| candidate gather/sort/top-N | `estimate.py`, `tryprune.py` | *(Python)* | **done** | `oxpinyin-train::candidate` |
| λ estimate (deleted-interp EM) | `evaluate.py` | `estimate_interpolation.cpp` | **done** | `oxpinyin-lambda` (reused by `oxpinyin-eval`) |
| correction rate | `evaluate.py` | `eval_correction_rate.cpp` | **done** | `oxpinyin-eval::decode` |
| eval runtime model build | `evaluate.py` `make` | data Makefile | **done** | `oxpinyin-eval::model` (native, no `make`) |
| word: populate/partial/new/mark | `populate/partialword/newword/markpinyin.py` | *(Python+SQLite)* | **done** | `oxpinyin-word` |
| punctuation | `genpunct.py` | *(Python)* | **done** | `oxpinyin-punct` |
| corpus index / status / epoch / resume | all scripts | `lib/*.py` | **done** | `oxpinyin-train::{corpus,status,config}` |
| full-workflow orchestration | the five drivers | *(Python)* | **done** | `oxpinyin-train::{pipeline,workspace}` + `oxpinyin-train` CLI |
| category reduce | `reduce.py` | *(Python)* | **excluded** | corpus-layout flattening, not on any path (§11.1) |
| legacy gen_ngram/gen_unigram | *(off-path)* | `gen_ngram.cpp` etc. | **off-path** | `oxpinyin-counter` (kept, §4) |
| legacy export_interpolation | *(off-path)* | `export_interpolation.cpp` | **off-path** | `oxpinyin-emitter` (kept, §4) |
| OpenGram dict tools | `tools/*.py` | `gen_pinyin_table.cpp` | **excluded** | external data-source prep (§11) |

`word: prepare` (`prepare.py`) builds the SQLite scaffolding upstream needs
for its per-order tables; `oxpinyin-word` replaces those tables with ordered
Rust maps, so the prepare step has no native analogue to port — its work is
absorbed into the crate's in-memory state (§8, §11).

---

## 4. Reclassification of existing training crates

`oxpinyin-counter` (`gen_ngram`), `oxpinyin-lambda` held-out counter
(`gen_deleted_ngram`), and `oxpinyin-emitter` (`export_interpolation`)
faithfully reproduce libpinyin utilities that exist in `utils/` but are
**not invoked by the trainer** (§1.1). They are correct and tested and
should be **kept** — they are legitimate manual libpinyin tools and their
counting/EM logic is shared machinery — but they are re-labelled from
"the W9 training path" to "the legacy interpolation utilities". The
load-bearing corpus counter for the shipped model is `gen_k_mixture_model`
(`oxpinyin-kmm`, §6.2). `oxpinyin-lambda`'s `interpolation` EM module *is*
on the real path (`evaluate.py`) and is reused by the evaluator (§7).

---

## 5. Segmentation — `spseg` and `mergeseq` (SHOWN)

Both depend only on the phrase table (dictionary), already loaded as
`oxpinyin_segment::PhraseLexicon` (`search(span) → (ok, continued,
tokens)`, `text(token)`, char length). Neither needs the bigram, unlike
`ngseg`.

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
(word-count DP vs bigram Viterbi). The Rust `spseg` shares
`oxpinyin-segment`'s line framing and lexicon; only a new DP is needed.

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
so the Rust port must process documents in the trainer's file order
(index order) and, within a document, is order-free. See §12.

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
(`oxpinyin-data::parse_interpolation2` already reads its `\1-gram`).

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

So the evaluator is the *decode round-trip* OXpinyin already implements in
`oxpinyin-engine`. The native evaluator (task §7): build an isolated model
from the candidate `interpolation2.text` + system tables (no `make`, no
libpinyin install), reuse the engine's Viterbi for the best match, and
report the same rate. λ is computed by reusing `oxpinyin-lambda`'s EM over
the KMM-storage-derived counts (the deleted model), not by the off-path
legacy counter.

---

## 8. Word recognition (SHOWN, `prepare/populate/partialword/newword/markpinyin.py`)

Upstream uses one SQLite DB per n-gram order (N=7) plus FTS3 for sequence
matching. OXpinyin replaces SQLite with ordered Rust maps; the observable
outputs (`partialword.txt`, `newword.txt`, `recognized.txt`) are the
compatibility target.

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
tables; OXpinyin can derive them from `oxpinyin-data` (§10).

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

The `repr()`/`eval()` intermediate files are a Python implementation
detail; OXpinyin uses typed structures and reproduces only the final
`puncts.table` (task §10 — typed status, not `repr`).

Consumer of `puncts.table`: it feeds the engine's punctuation candidate
table; trace and document the exact consumer before freezing the format
(task §10) — deferred to the word/punct implementation PR.

---

## 10. Corpus / index / status (SHOWN, `lib/*.py`, `docs/fileformat`)

- **Index files** `*.index`: lines `title#/rel/text/path`; content files
  `<n>.text`. `dirwalk.walkIndex` recurses, dispatching `*.index`.
- **Status files** `*.status`: JSON `{'<Pass>Epoch': n, …}`; `check_epoch`
  compares against `myconfig` epochs (equal ⇒ done, smaller ⇒ redo,
  larger ⇒ error). Resumability is real for generate (`GenerateTextEnd`/
  `GenerateModelEnd` checkpoints). OXpinyin uses a **typed** status record
  (task §8), not Python `repr`/`eval`, except where an externally-consumed
  file demands byte-compat (none identified).
- **Minimum file size** filter: `getMinimumFileSize() = 1200·3 + 1200/2 =
  4200` bytes; smaller segmented texts are skipped (generate, populate,
  genpunct).
- **reduce.py**: flattens the category tree to N levels by concatenating
  sub-index files; optional — port only if the supported corpus layout
  needs it.

---

## 11. Helper classification (task §1, §16.8)

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

Historical helpers are **not** ported merely because they exist (task §1).

### 11.1 `reduce.py` classification (task §7) — **optional, not on any path**

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
Not required (no stage depends on it), not historical/unreachable (it is
reachable and functional). Its native equivalent is a trivial directory
concatenation; it is deliberately **not** part of `oxpinyin-train`, which
consumes a `CorpusIndex` directly (the flattened index files the pipeline
actually reads). Porting it would add a corpus-layout convenience outside the
trainer-workflow parity boundary — excluded by the same rule that excludes the
OpenGram `tools/` helpers.

---

## 12. Determinism (task §13)

Sources of nondeterminism in the upstream tools and the Rust policy:

- **GLib/SQLite hash iteration order** — upstream KMM counting, export
  ordering, and word-recog rely on undefined hash order. Rust uses
  **ordered** maps (BTree / sorted vecs) keyed by token / word, so export,
  merge, and prune walk tokens ascending (matching `get_all_items`), and
  candidate sets are deterministic.
- **Document/candidate order** — KMM `m_Mr`/maximum-occurs is
  order-sensitive across documents; the Rust port fixes document order to
  the index-file order. Candidate merge order in `tryprune` follows the
  score-descending `estimate.sorted.index`; ties there break by the
  Python `sort` (stable, by original index order) — reproduced as a
  stable sort keyed on score desc.
- **Floating point** — `parameter_t = double` throughout the KMM math and
  the λ EM; the Rust port uses `f64` with the same operation order
  (per-item accumulate, same division points) to keep the EM fixpoint and
  the prune CDF bit-comparable. `markpinyin`'s `int()` truncation and the
  entropy `log` (natural log) are reproduced exactly.
- **Locale** — upstream `setlocale(LC_ALL,"")`; segmentation is UCS4 and
  locale-independent for the covered scripts. The Rust port is
  locale-free.
- **Parallelism** — not introduced until a deterministic order is
  specified for the stage (task §13, §15).

---

## 13. Differential-testing plan (task §12)

Follow the established `oxpinyin-segment` pattern: committed golden
fixtures (byte-exact where a byte format exists) plus an env-gated live
oracle upgrade (`PINYIN_*` binaries) so a golden cannot silently go stale.
Per stage:

| Stage | Canonical comparison | Equality |
|---|---|---|
| spseg | pin `spseg` token stream (same framing grammar as ngseg) | byte-exact vs golden / live |
| mergeseq | pin `mergeseq` merged token stream | byte-exact vs golden / live |
| KMM generate | canonical KMM record set (export text, sorted) | exact map equality |
| KMM estimate | `average lambda` | exact (double `%f`) or documented tol |
| KMM merge/prune | retained record set (export text) | exact |
| KMM → interpolation | interpolation2.text | canonical textual equality |
| λ | average λ | exact / documented tol |
| correction rate | rate | exact |
| word recog | recognized.txt records | exact records |
| punctuation | puncts.table | exact freq + order |

Small deterministic hand-verifiable fixtures per transformation (task §6,
§9) accompany each implementation PR.

---

## 14. Remaining-work decomposition (task §17)

Parts, dependencies, and target crates (this audit is Part A):

- **B** segmentation: `spseg`, `mergeseq` → `oxpinyin-segment`.
- **C** KMM core: data model, generate, validate, export/import →
  `oxpinyin-kmm`.
- **D** KMM optimisation: estimate/score, merge, prune, →interpolation,
  candidate gather/sort → `oxpinyin-kmm` (+ orchestration CLI).
- **E** evaluator: `estimate_interpolation` reuse + `eval_correction_rate`
  → `oxpinyin-eval` (reuses `oxpinyin-engine` decode, `oxpinyin-lambda`).
- **F** word recognition: prepare/populate/partialword/newword/markpinyin
  → `oxpinyin-word`.
- **G** punctuation → `oxpinyin-punct`.
- **H** orchestration: typed status, index walk, native end-to-end
  pipeline + integration fixtures + reproducibility → `oxpinyin-corpus` /
  a `oxpinyin-train` driver.

B, C, E, F, G are independently developable once this audit and the shared
format specs (§5–§9) are frozen; D depends on C; E and H depend on the
interpolation output of D.

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

## 15. Implementation status (updated 2026-08-31 — **complete**)

Every capability the current `libpinyin/trainer` main workflow uses is
implemented natively in Rust. Each part is an independently-reviewable commit:

| Part | Deliverable | Crate / home | Tests |
|---|---|---|---|
| B | `spseg` (fewest-words DP), `mergeseq` (phrase merge) | `oxpinyin-segment` (`spseg`, `mergeseq`, two CLIs) | toy unit + committed-golden differential (W3 table, CI-always) + env-gated live cross-check |
| C+D | full KMM pipeline — data model, generate, estimate, merge, validate, prune, export/import, →interpolation | `oxpinyin-kmm` (self-contained, one CLI, 8 subcommands) | per-op unit + hand-verified golden + merge-equals-combined + end-to-end from the real segmented corpus + **semantic-parity golden + env-gated oracle differential** (`tests/differential.rs`) |
| C+D audit | line-by-line arithmetic verification vs the six KMM sources | `docs/findings/kmm-arithmetic-audit.md` | term-for-term tables + divergence register (four-class) |
| E | evaluator — `estimate_interpolation` λ over KMM-derived counts + `eval_correction_rate` decode round-trip, native runtime model, no `make`/libpinyin | `oxpinyin-eval` (`oxpinyin-eval` CLI) | hand-computable homophone fixture + full-flow integration + env-gated `eval_correction_rate` differential |
| G | punctuation table (`genpunct.py`) | `oxpinyin-punct` (count/merge CLI) | per-stage unit + two-stage golden (segmented → puncts.table) |
| F | word recognition — populate, partial-word discovery + cross-order merge, new-word entropy filtering, pinyin marking | `oxpinyin-word` (`recognize` CLI) | per-stage unit (incl. the `partition` merge walk) + hand-traced end-to-end golden (→ recognized word + pinyin) |
| H (core) | end-to-end main pipeline on real committed data (segment → KMM → interpolation2.text) | `oxpinyin-kmm` integration test over the committed `spseg` fixture | passes on CI |
| **H (full)** | **native trainer orchestrator + `oxpinyin-train`** — typed config/status/epoch/corpus-index/candidate structures, segment → generate (rollover + min-file-size filter) → estimate + gather + sort → merge top N → prune → convert → evaluate, with the on-disk `try<name>` workspace, status files, cleanup, and stage-level resumability | `oxpinyin-train` (`oxpinyin-train` CLI) | typed-structure units + **raw-corpus acceptance test** (raw Han corpus → final interpolation model + λ + correction rate, verifying every stage's on-disk product + resumability) |

**Reclassified** (kept, retitled): `oxpinyin-counter` (`gen_ngram`) and
`oxpinyin-emitter` (`export_interpolation`) are legacy libpinyin utilities
off the trainer path (§4); `oxpinyin-lambda`'s `estimate_interpolation` EM is
on the real path (reused by `oxpinyin-eval`).

**Nothing remains** for trainer-workflow parity. The unported helpers are the
deliberate exclusions of §11 (`reduce.py` corpus-layout flattening; the
OpenGram `tools/`; the off-path libpinyin utils) — none is invoked by the
main, word-recognition, or punctuation pipelines.

### The one native command (H full)

```sh
# raw corpus → segment → generate → estimate → merge/prune → interpolation2.text
#           → estimate λ → apply λ → correction rate, no Python/make/SQLite/libpinyin
oxpinyin-train \
    --text-dir texts/ --model-dir models/ --final-dir finals/ \
    --index texts/corpus.index --held-out held.segmented --evals evals2.text \
    --pinyin-index pinyin_index.<ext> --phrase-index phrase_index.<ext> \
    [--merge 10] [-k 3] [--CDF 0.99] [--fast] NAME
# → prints "average lambda:<λ>" and "correction rate:<rate>";
#   writes finals/try<NAME>/interpolation2.text (the final model).
```

The individual CLIs remain for stage-wise use (`oxpinyin-spseg`,
`oxpinyin-kmm <subcommand>`, `oxpinyin-eval`, `oxpinyin-word recognize`,
`oxpinyin-punct`); `oxpinyin-train` orchestrates the whole main workflow.

### Acceptance tests (task §9) — the three authoritative flows

| Flow | Test | Result |
|---|---|---|
| raw corpus → **final interpolation model + λ + correction rate** | `oxpinyin-train/tests/end_to_end.rs::raw_corpus_to_final_model_and_correction_rate` | correction rate 0.5 (中 dominates 钟), λ ∈ [0,1], every stage's on-disk product verified, second run resumes identically |
| segmented corpus → **recognized words + pinyin** | `oxpinyin-word/tests/word_pipeline.rs::recognizes_the_merged_word_with_combined_pinyin` | `甲乙\tjia'yi\t100` |
| segmented corpus → **puncts.table** | `oxpinyin-punct/tests/punct_pipeline.rs::two_stage_pipeline_golden` | `10 甲 。 5` after per-index + global prune |
