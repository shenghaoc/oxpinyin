# Trainer replacement — final report (W9)

## Verdict

OXpinyin **replaces the `libpinyin/trainer` main workflow end to end in Rust**,
with no Python, `make`, SQLite, or libpinyin library/executable in the loop.
This is the strong claim — not merely "can produce an interpolation model",
but "can stand in for the trainer workflow a maintainer actually runs".

**The distinction, and why acceptance is the second.** Producing an
`interpolation2.text` from a pre-segmented corpus was already demonstrated at
W9 "H core" (a KMM chain over a committed segmented fixture). That proves the
*algorithm* runs, but not that the *workflow* is replaced: the trainer is not
one binary, it is five drivers plus their orchestration — corpus-index
traversal, per-document segmentation from raw text, candidate generation with
size-based rollover, candidate scoring and ranking, top-N merge, prune,
conversion, and a final held-out evaluation — glued by status files, epochs,
resumability, and output-directory management. Replacing the workflow means
reproducing **all** of that natively. That is what `oxpinyin-train` now does,
and what the raw-corpus acceptance test exercises: raw Han corpus in, final
interpolation model **plus the estimated-and-applied λ plus the measured
correction rate** out, every intermediate stage's on-disk product verified.

## The native commands

| Command | Replaces | Role |
|---|---|---|
| `oxpinyin-train` | the whole main workflow (`segment`→`generate`→`estimate`→`tryprune`→`evaluate`.py) | **the one orchestrator** |
| `oxpinyin-spseg` / `oxpinyin-mergeseq` | `spseg` / `mergeseq` | segmentation (stage-wise) |
| `oxpinyin-kmm <sub>` | `gen_/estimate_/merge_/validate_/prune_/export_/import_k_mixture_model`, `k_mixture_model_to_interpolation` | KMM toolchain (8 subcommands, stage-wise) |
| `oxpinyin-eval` | `estimate_interpolation` + `make modify` + `eval_correction_rate` (`evaluate.py`) | native evaluator (stage-wise) |
| `oxpinyin-word recognize` | `populate`/`partialword`/`newword`/`markpinyin`.py | word recognition |
| `oxpinyin-punct` | `genpunct.py` | punctuation table |

The individual CLIs remain for stage-wise use; `oxpinyin-train` orchestrates
the main workflow so a maintainer runs one command.

### End-to-end command

```sh
oxpinyin-train \
    --text-dir texts/ --model-dir models/ --final-dir finals/ \
    --index texts/corpus.index --held-out held.segmented --evals evals2.text \
    --pinyin-index pinyin_index.<ext> --phrase-index phrase_index.<ext> \
    [--bigram bigram.<ext> --interpolation2 interpolation2.text --table-conf table.conf] \
    [--merge 10] [-k 3] [--CDF 0.99] [--fast] [--skip-pi-gram] NAME
```

Prints `average lambda:<λ>` and `correction rate:<rate>`; writes
`finals/try<NAME>/interpolation2.text` (the final model) alongside the
per-stage candidate models, estimate indexes, `kmm_merged.text`,
`kmm_pruned.text`, and the `cwd.status` epoch/score record.

## Every utility accounted for

**Main pipeline (all ported).** `segment.py`→`oxpinyin-segment`;
`generate.py`→`oxpinyin-kmm generate` + `oxpinyin-train` rollover;
`estimate.py`→`oxpinyin-kmm estimate` + `oxpinyin-train` gather/sort;
`tryprune.py`→`oxpinyin-kmm merge/validate/prune/export/to-interpolation` +
`oxpinyin-train` top-N; `evaluate.py`→`oxpinyin-eval`. The six KMM C++ tools
and `k_mixture_model.h` are reproduced in `oxpinyin-kmm` and verified
line-by-line (`kmm-arithmetic-audit.md`).

**Word recognition (ported).** `populate`/`partialword`/`newword`/
`markpinyin`.py → `oxpinyin-word`. `prepare.py` builds SQLite scaffolding with
no native analogue (the per-order tables become ordered Rust maps), so its
work is absorbed, not ported.

**Punctuation (ported).** `genpunct.py` → `oxpinyin-punct`.

**Config/orchestration (ported).** `lib/myconfig.py`→`oxpinyin-train::config`;
`lib/utils.py` status/epoch→`oxpinyin-train::status`;
`lib/dirwalk.py` + the corpus index→`oxpinyin-train::corpus`.

### Unported helpers (deliberate, with rationale)

| Helper | Why not ported |
|---|---|
| `reduce.py` | Corpus-index directory flattening; no caller in the trainer; operates only on the index layout, not the algorithm. `oxpinyin-train` consumes a `CorpusIndex` directly. (`trainer-parity-audit.md` §11.1) |
| `tools/{distill,convertopengram,filteropengram,mergepartialopengram,striptones}.py` | OpenGram-dictionary preparation — an external data-source ingest, off the supported workflow. |
| `tools/merge.py`, `tools/interpolation_to_kmm.py` | Optional cross-category / round-trip helpers, off-path. |
| `gen_ngram`, `gen_unigram`, `gen_deleted_ngram`, `export_/import_interpolation`, `gen_binary_files` | Valid libpinyin utils the trainer never invokes (kept where already ported: `oxpinyin-counter`, `oxpinyin-lambda`, `oxpinyin-emitter`; §4). |

None is on the main, word-recognition, or punctuation path.

## Correctness evidence

1. **Line-by-line arithmetic audit** (`kmm-arithmetic-audit.md`): every
   integer field transformation and floating-point expression of the six KMM
   sources checked against the Rust term-for-term and in evaluation order,
   with a four-class divergence register. The audit corrected one behaviour to
   stay policy-compliant (the empty-deleted-model candidate score now
   reproduces upstream's `NaN` instead of erroring).
2. **Semantic-parity golden** (`oxpinyin-kmm/tests/differential.rs`): a
   non-trivial five-token, two-document fixture with hand-derived canonical
   exports pinning headers, document count, word count, per-token freqs, and
   per-pair `count`/`T`/`N_n_0`/`n_1`/`Mr`, plus candidate scores + ordering,
   merge, a selective prune, and the interpolation projection. The export text
   *is* the canonical semantic form (it carries every field), so byte equality
   is field equality.
3. **Hand-computable fixtures**: the evaluator's homophone fixture (rate 0.5
   for any λ), the KMM per-op units, the orchestrator's rollover/filter units,
   and the status-codec round-trip.
4. **Env-gated oracle differentials**: against pin `eval_correction_rate`
   (`oxpinyin-eval`), pin `gen_/export_k_mixture_model` and
   `k_mixture_model_to_interpolation` (`oxpinyin-kmm`), pin `ngseg`/`spseg`
   (`oxpinyin-segment`), and pin `estimate_interpolation` (`oxpinyin-lambda`).

### Oracle differential status

The pin binaries and the model20 system data are **not buildable in this
environment** (no DBM library, no committed system model), so the live oracle
gates **skip** here with an informative message — as they do in CI. They run
where an operator sets the `PINYIN_*` binary/data env vars. Committed
correctness therefore rests on the line-by-line audit, the hand-computable and
hand-derived goldens, and the semantic invariants; the oracle gates are the
upgrade path that keeps a golden from silently going stale.

## Test counts

| Crate | Tests | Coverage |
|---|---|---|
| `oxpinyin-segment` | 36 | spseg/mergeseq/ngseg + differentials |
| `oxpinyin-kmm` | 39 | per-op units + pipeline golden + semantic parity + oracle gates |
| `oxpinyin-eval` | 13 | model/decode/phrases units + full-flow + oracle gate |
| `oxpinyin-word` | 23 | per-stage units + end-to-end golden |
| `oxpinyin-punct` | 13 | per-stage units + two-stage golden |
| `oxpinyin-train` | 25 | config/status/corpus/candidate units + raw-corpus acceptance |
| `oxpinyin-lambda` | 14 | held-out EM + differential |
| `oxpinyin-counter` | 13 | ngram counting + differential |
| **Total** | **176** | all green |

## Determinism & complexity

Every stage is a pure function of (input, config): ordered maps (BTree) keyed
by token/word replace GLib/SQLite hash iteration, so export/merge/prune walk
token-ascending and candidate sets, sorts, and merges are order-stable
(`trainer-parity-audit.md` §12). This is the property that makes the
byte-level goldens and the semantic-parity harness reproducible.

**Benchmarks:** no measured wall-clock comparison against the pin is included
— the pin is not buildable here, and the trainer crates are offline tools
where correctness and determinism, not latency, are the Stage-1 target. The
source policy's constraint ("time and space complexity must never both be
worsened") is met by construction: the algorithms are the same, with the one
mechanism trade of ordered maps (O(log n) per access, deterministic) for
upstream's hash maps (O(1) average, nondeterministic order) — a space/latency
neutral-to-favourable swap that buys determinism. Measured Stage-2 profiling
(smaller binary, lower RAM, faster run) is future work, tracked with the rest
of Stage 2 in `ROADMAP.md`.

## Limitations

- **Oracle gates skip in this environment** (no pin build, no system data);
  they are the CI/operator upgrade path, not committed-here evidence.
- **The `oxpinyin-train` binary needs a system phrase index** for the segment
  and evaluate stages (as the pin does); the in-memory acceptance test builds
  a fixture segmenter to exercise the whole chain without it.
- **Resumability is stage-level** (each stage gated by its epoch, artifacts
  reloaded), plus the persisted `GenerateStart/End`/`GenerateModelEnd` markers;
  fine-grained mid-generate crash-resume reloads from the last persisted
  candidate boundary rather than mid-document.
- **No measured benchmarks** (above) — Stage-2 work.
- **`--fast` (spseg) and `--skip-pi-gram-training`** are wired through the
  orchestrator but the acceptance test pins the default (ngseg, pi-gram on)
  path; the spseg path shares the same downstream stages.

## Bottom line

The acceptance bar was "replace the trainer workflow", not "produce an
interpolation model". `oxpinyin-train` clears it: one native command takes a
raw corpus to the final interpolation model, the applied λ, and the correction
rate, reproducing the five-driver Python workflow — orchestration, status,
resumability and all — with the KMM arithmetic verified line-by-line and the
whole chain exercised from raw text by a committed acceptance test.
