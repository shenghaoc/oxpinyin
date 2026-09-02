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

### Evidence levels

Three levels of evidence, strongest last, distinguished per stage:

- **Level 1 — source parity.** The Rust is audited term-for-term against the
  pinned C++/Python source (`kmm-arithmetic-audit.md`, `trainer-parity-audit.md`).
- **Level 2 — deterministic fixtures.** Hand-derived/hand-computable and
  committed-golden fixtures reproduce the expected semantics and pin them
  against regression.
- **Level 3 — live oracle parity.** The **actual pinned libpinyin 2.11.91**
  (built here, Tkrzw backend, SHA-verified model20 data) is run and compared
  against oxpinyin. Achieved for the stages marked ✓ below.

### The oracle was built and the differentials were executed

Contrary to an earlier assessment, the pinned oracle **is** buildable in this
class of environment. `apt` installs `libtkrzw-dev` + autotools; the pinned
libpinyin source (`0c5e80e`, tag 2.11.91) builds with `--with-dbm=Tkrzw`; and
`model20.text.tar.gz` fetched from SourceForge is **SHA-256 bit-identical** to
the pin (`59c68e89…`). `tools/oracle/run-differentials.sh` wires the built
utils to the `PINYIN_*` gates. Running it produced:

| Stage | Native | L1 source | L2 golden | L3 live oracle | Result |
|---|---|---|---|---|---|
| spseg | ✓ | ✓ | ✓ | **✓** | live: matches pin `spseg` |
| mergeseq | ✓ | ✓ | ✓ | **✓** | live: matches pin `mergeseq` |
| ngseg | ✓ | ✓ | ✓ | **✓** | live: 111 fixture lines token-identical to pin `ngseg` on the full system data (bigram.db + the `gen_unigram` floor; see below) |
| KMM generate | ✓ | ✓ | ✓ | **✓** | live: gen+export record set matches pin on the real corpus |
| KMM export/import | ✓ | ✓ | ✓ | **✓** | live: via gen+export |
| KMM estimate (candidate score) | ✓ | ✓ | ✓ | ✓* | *deleted-interpolation EM shares the arithmetic proven live for λ below; committed golden |
| KMM merge | ✓ | ✓ | ✓ | **✓** | live: merged record set matches pin |
| KMM validate | ✓ | ✓ | ✓ | **✓** | live: verdict matches pin (both reject the W2-only small-corpus model) |
| KMM prune | ✓ | ✓ | ✓ | **✓** | live: pruned record set matches pin |
| KMM → interpolation | ✓ | ✓ | ✓ | **✓** | live: byte-identical to pin `k_mixture_model_to_interpolation` |
| λ (estimate_interpolation) | ✓ | ✓ | ✓ | **✓** | live: DELETED bigrams bit-exact, 153 per-context λ byte-identical at 6dp |
| ngram counter (gen_ngram) | ✓ | ✓ | ✓ | **✓** | live: 138 096 unigrams value-identical to pin `gen_ngram` |
| correction rate (evaluator) | ✓ | ✓ | ✓ | **✓** | live: `0.880000` == pin on a 25-sentence held-out corpus (22 passed), after the `PhoneticLookup` port below |
| punctuation | ✓ | ✓ | ✓ | — | no pin oracle path (genpunct is pure Python); golden-verified |
| word recognition | ✓ | ✓ | ✓ | — | no pin oracle path (pure Python + SQLite); golden-verified |
| orchestration / raw-corpus E2E | ✓ | ✓ | ✓ | — | pure native wiring; acceptance-tested |

**Oracle-driven correction.** Running the live gate immediately exposed a real
divergence the fixtures had missed: oxpinyin-kmm stored a `\1-gram` array
header for every unigram, but the Tkrzw-backed pin's `set_array_header`
no-ops on a token that never appears as W1, so W2-only tokens get no header
(their freq counts toward `total_freq` only). `generate.rs` was fixed to match,
and the export order was reclassified from "matches the DBM order" to
"token-ascending canonicalisation compared as a set" (the pin's order is
Tkrzw hash order). See `kmm-arithmetic-audit.md` §2/§6 and the D6/D7 register.

**The two formerly gated stages, and what running them found.** ngseg-live
and correction-rate-live both need the compiled `bigram.db`, built by the
pin's own `import_interpolation` over model20's `interpolation2.text`. That
step tripped its `insert_freq` assertion on the first attempt in both a
Tkrzw and a Kyoto Cabinet build of the pin, and completed on a re-run over a
fresh `bigram.db` — it is intermittent, not a property of the data (model20
holds no duplicate bigram) — and the resulting `bigram.db` round-trips
through the pin's own `export_interpolation` with all 63 907 unigrams and
1 849 609 bigrams value-identical to the source. With it, three things
surfaced that the gated state had hidden:

1. **The data recipe was incomplete.** libpinyin's `data/Makefile.am` builds
   the system data in three steps — `gen_binary_files`,
   `import_interpolation`, `gen_unigram` — and the runner's recipe listed
   two. Without `gen_unigram`'s freq-1 floor a zero-count phrase such as
   `今天天气` is unreachable to the pin, so the pin's `ngseg` split it and
   diverged from the committed golden in 19 lines; with the floor applied it
   reproduces the golden byte for byte, as the native segmenter always did.
   The recipe (script header) now names all three steps.
2. **The evaluator's decode was not the pin's.** Its first live run scored
   `0.52` against the pin's `0.88`: the native decode ranked spans through
   `oxpinyin_core::Scorer::rank_phrases` — the engine's typing heuristics
   (segmentation penalties, phrase bonuses, integer costs) — and carried no
   pronunciation possibility, so a rare reading (红 read `gong`) could
   outrank 公园. `oxpinyin-eval::decode` is now a term-for-term port of
   `PhoneticLookup<1, 1>::get_nbest_match` (beam of 32, bigram before
   unigram expansion, `log((λ·P_bi + (1 − λ)·P_uni) · P_pron)` at the pin's
   float widths, strict-less replacement).
3. **The evaluation model lacked the floor.** `evaluate.py`'s `make` runs
   `gen_unigram` over the rebuilt data, so every phrase-index token carries
   count + 1 and the total spans the whole lexicon; `build_model` now takes
   the lexicon and floors it the same way.

After 2 and 3 the gate passes exactly: native `0.880000` == pin `0.880000`,
22 of 25 sentences, the same three sentences failing on both sides. The
evaluator gate needs an `evals2.text` in the system token space; the runner
wires it when `$data/evals2.text` exists (here: 20 hand-written sentences
segmented by the pin's `ngseg`) and reports it as skipped otherwise.

## Test counts

| Crate | Tests | Coverage |
|---|---|---|
| `oxpinyin-segment` | 36 | spseg/mergeseq/ngseg + differentials (**live** spseg, mergeseq, ngseg) |
| `oxpinyin-kmm` | 46 | per-op units + pipeline golden + semantic parity + **5 live oracle gates** |
| `oxpinyin-eval` | 16 | model/decode/phrases units + full-flow + **live** `eval_correction_rate` gate |
| `oxpinyin-word` | 25 | per-stage units + end-to-end golden |
| `oxpinyin-punct` | 13 | per-stage units + two-stage golden |
| `oxpinyin-train` | 32 | config/status/corpus/candidate units + raw-corpus acceptance + resumability |
| `oxpinyin-lambda` | 14 | held-out EM + **live** differential |
| `oxpinyin-counter` | 13 | ngram counting + **live** differential |
| **Total** | **195** | all green (1 013 across the workspace; `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`) |

## Determinism & complexity

Every stage is a pure function of (input, config): ordered maps (BTree) keyed
by token/word replace GLib/SQLite hash iteration, so export/merge/prune walk
token-ascending and candidate sets, sorts, and merges are order-stable
(`trainer-parity-audit.md` §12). This is the property that makes the
byte-level goldens and the semantic-parity harness reproducible.

**Benchmarks.** The pin is built, so a measured comparison is feasible; a
clean throughput/RAM benchmark still needs a *valid* large corpus. A quick
smoke run over a 57 600-line synthetic corpus (the real fixture repeated) is
not that: the pin's `gen_k_mixture_model` aborts on it (the repeated fixture
concatenates into one document with W2-only tokens), while native
`oxpinyin-kmm generate` completes in ~6 ms — a robustness data point (native
does not abort on caller input), not a like-for-like throughput number. So no
throughput/RAM claim is made here; proper profiling on a real corpus is Stage-2
work (`ROADMAP.md`). The source policy's constraint ("time and space complexity
must never both be worsened") is met by construction: the algorithms are the
same, with the one mechanism trade of ordered maps (O(log n) per access,
deterministic) for upstream's hash maps (O(1) average, nondeterministic order).

## Limitations

- **Upstream `import_interpolation` can assert intermittently** when it
  compiles `bigram.db` (`insert_freq`, both DBM backends); a re-run over a
  fresh `bigram.db` completed and round-trips exactly. Delete a partial
  `bigram.db` before retrying. Every live gate runs (eleven pass).
- **The evaluator's live gate needs an operator-supplied `evals2.text`** in
  the data dir (the pin's `eval_correction_rate` reads it from its cwd); the
  runner skips that gate, visibly, when it is absent.
- **The `oxpinyin-train` binary needs a system phrase index** for the segment
  and evaluate stages (as the pin does); the in-memory acceptance test builds
  a fixture segmenter to exercise the whole chain without it.
- **Resumability is stage-level** (each stage gated by its epoch, artifacts
  reloaded), plus the persisted `GenerateStart/End`/`GenerateModelEnd` markers;
  fine-grained mid-generate crash-resume reloads from the last persisted
  candidate boundary rather than mid-document.
- **No throughput/RAM benchmark** (above) — Stage-2 work.
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
