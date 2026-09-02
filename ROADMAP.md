# Roadmap

Portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin). Constitution and agent
rules: `AGENTS.md`. Crate roles: `.kiro/steering/structure.md`.

> **Project identity:** **oxpinyin** (repo, crate, docs). The prior
> project name is retained in git history only.
> Shipped artifact naming for the compatibility bootstrap is a separate
> concern from project identity.

## Stages

| Stage | Goal |
|---|---|
| **0** | Scaffold, pin, SPECs/fixtures (here) |
| **1** | Exact-output parity with the pin-built libpinyin oracle (differential testing) |
| **2** | Measured upgrades (trigram/KN, typo edges, own data) — every divergence vs Stage 1 baseline |

Stage 1 uses installed/libpinyin-format tables (no redistribution required).
Stage 2 is optional and measurement-gated.

## Reference pin

Authoritative freeze: `docs/testing/oracle-environment.md`  
(libpinyin `2.11.91` / ibus-libpinyin `1.16.5` / model archive SHA-256s).

Build: `tools/oracle/build-oracle.sh` (optional container recipe alongside).

## How work proceeds

1. **Freeze behaviour** into `docs/findings/` SPECs and golden fixtures
   (characterisation of the pin first; see `docs/findings/spec-derivation.md`).
2. **Implement** under the Source policy in `AGENTS.md` (copy upstream,
   rewrite in Rust, then oxidize), with frozen SPECs/fixtures as the gate.
3. **Prove** with fixture tests everywhere; live oracle diff on Linux is the
   verification tier for Stage 1 gates.

Detailed task cards live under `.kiro/specs/` as they are derived. Until a
SPEC is frozen, do not implement that slice.

## Phase 0 (blocks feature work)

Still open or partial — see `.kiro/specs/foundation/tasks.md` and findings:

| Need | Output |
|---|---|
| Pin + recipe | `docs/testing/oracle-environment.md` (recorded) |
| Frontend ABI subset | `docs/findings/abi-subset.md` (recorded) |
| Upstream schema | `docs/findings/upstream-schema.md` (recorded) |
| Parser / path-set / scoring SPECs | not yet frozen |
| Data load route (D3) | not yet decided |
| Capture harness + F-A fixtures | not yet built |

## Stage 1 workstreams (names only)

| ID | Focus | Crate(s) |
|---|---|---|
| W1 | Types, parser, correction flags | oxpinyin-core |
| W2 | Oracle FFI + differential runner | pinyin-oracle |
| W3 | Table loading | oxpinyin-data |
| W4 | SegmentGraph, k-best, engine session | oxpinyin-core, oxpinyin-engine |
| W5 | C ABI subset | oxpinyin-capi |
| W6 | User store (redb) | oxpinyin-user |
| W7 | Classic text-format interop via oxpinyin-dictool (import + export) | oxpinyin-dictool, oxpinyin-capi |
| W8 | oxpinyin library release + compatibility bootstrap for the ibus-libpinyin fork | oxpinyin-capi |
| W9 | Training toolchain — full trainer-workflow parity (KMM in scope; see `docs/findings/trainer-parity-audit.md`) | oxpinyin-segment, oxpinyin-kmm, oxpinyin-eval, oxpinyin-word, oxpinyin-punct, oxpinyin-lambda, oxpinyin-corpus (legacy: oxpinyin-counter, oxpinyin-emitter) |
| W10 | Option bits: correction, fuzzy/ambiguity, dynamic-adjust gating | oxpinyin-core, oxpinyin-engine |
| W11 | Phrase-index union at lookup (user, network, addon) | oxpinyin-engine, oxpinyin-data, oxpinyin-user |
| W12 | Corpus tail (parity gaps; candidate residual closed 2026-08-22) | oxpinyin-core, oxpinyin-engine, oxpinyin-capi |
| W13 | Double-pinyin and bopomofo input schemes (feature implementation) | oxpinyin-core, oxpinyin-engine |
| W14 | Sentence surface (n-best emission, NBEST_MATCH typing, get_sentence) | oxpinyin-capi, oxpinyin-engine |
| W15 | model20-native runtime-data production, every backend | oxpinyin-datagen, oxpinyin-store |

### Workstream notes (recorded as decisions settle)

- **Kyoto Cabinet is the default selected backend** (2026-08-29). The four
  supported oxpinyin store backends — Kyoto Cabinet, redb, LMDB, tkrzw —
  are peer implementations behind one `ReadStore`/`WriteStore` trait
  surface, and any single build picks one at compile time
  (`DefaultStore`; chain kyotocabinet > tkrzw > lmdb > redb is a
  tie-break for cargo's additive feature unification, not a hierarchy).
  Kyoto Cabinet is the feature enabled in the workspace's default set;
  the other three are selected explicitly with `--no-default-features
  --features {redb|lmdb|tkrzw}`. Native table files carry the peer's
  extension (`.kct`/`.tkt`/`.lmdb`/`.redb`). Switching backends is a
  storage-format transition — the runtime does not transparently open
  one backend's files with another, and old backend-specific user data
  is not carried across the switch. (This matches the model
  distributions use for libpinyin's own backend transitions.)

- **W15 LANDED.** The data pipeline inversion is complete: runtime tables
  are compiled natively from the canonical pinned `model20` archive for every
  storage backend (Kyoto Cabinet, redb, LMDB, Tkrzw) — no producer consumes
  libpinyin-generated runtime data. Implemented in `crates/oxpinyin-datagen`
  (`system.rs`, `table.rs`, `punct.rs`, `addon.rs`, `manifest.rs`, `write.rs`);
  all four backend producers are feature-gated in `Cargo.toml`. The retired
  `oxpinyin-migrate` route (oracle ABI export + verbatim Tkrzw conversion) is
  proven unnecessary: the native compilation reproduces its frozen full export
  entry-for-entry, which unparked the five differentials that needed a full
  system dir. Architecture and equivalence evidence:
  `docs/findings/datagen-model20.md` (Status: recorded / implemented).

- **W7 is flat, not a task stack.** One deliverable: classic text-format
  interop via oxpinyin-dictool (import + export). The line-oriented
  `phrase (SP|TAB) pinyin [count]` format has been libpinyin's public
  interchange since 1.1.0; ibus-libpinyin's Import/Export buttons drive
  `LibPinyinBackEnd::importPinyinDictionary` /
  `exportPinyinDictionary` (`PYLibPinyin.cc:230-277`, `:280-353`).
  Historically neither libpinyin nor ibus-libpinyin migrated user data
  from their predecessors (pinyin engine, novel-pinyin, ibus-pinyin) —
  the pattern is fresh start plus the text-format interchange for users
  who care. Binary legacy-DB migration was investigated
  (`feat/w7-t2-legacy-migrate`, shelved with findings at
  `docs/findings/legacy-migration.md`) and cancelled per that precedent.
  GSettings key-for-key mapping was cancelled too: a Rust-language rewrite
  would have its own component/schema ids per the W8 decision. No
  T-numbering here — one deliverable delivered in one PR, flat like the
  decoder-λ fix (PR #55).

- **W8 is the oxpinyin library release, with a compatibility bootstrap for
  the maintainer's ibus-libpinyin fork.** oxpinyin-capi exposes the
  51-symbol call surface of the fork (`feat/oxpinyin-backend` Phase-0 doc
  `docs/oxpinyin-switch.md` in that ibus-libpinyin repo, tip `0d71866`):
  the 50 symbols pinned from ibus-libpinyin 1.16.5 plus
  `pinyin_get_parsed_input_length` (fork commit `2c5baa9`). For W8, the
  fork surface supersedes the upstream tag freeze.
  The first oxpinyin release ships this as a binary the fork links against
  with minimal changes — enough to switch the fork off the C++ libpinyin
  backend and onto oxpinyin.

  After that first release, the surface is **free to evolve**. The
  51-symbol fork call surface is a bootstrap contract for the initial
  switch, not a permanent ABI freeze. Long-term soname or header
  compatibility with upstream libpinyin is explicitly a non-goal: the fork
  and oxpinyin evolve together, and upstream compatibility is not
  maintained.

  Two precedents, cited for what they inform:

  - **libchewing** (Kan-Ru Chen): a library-only rewrite; frontend packages
    were left alone. oxpinyin follows this pattern.
  - **pinyin → libpinyin** (Peng Huang → Peng Wu): historically a new library
    name with a new frontend, no drop-in. oxpinyin's bootstrap is a
    transitional inversion of that — the first release IS a working swap for
    the fork — but the long-term shape returns to the pinyin → libpinyin
    pattern: own library, own frontend fork, no upstream compatibility
    promise.

  Acceptance for the initial release: the maintainer's ibus-libpinyin fork
  builds against oxpinyin's compatibility surface with minimal changes, and
  the resulting engine produces the same wire-level output on a scripted
  input sequence as the pinned upstream configuration.

  Earlier language in this repo variously described W8 as
  "capi + forked frontend", then "ibus-pinyin-rs zbus rewrite", then
  "drop-in libpinyin.so.15" — all superseded by the above. The maintainer
  being independent of Red Hat / Fedora / upstream libpinyin is what enables
  this scope; a maintainer bound to those distros would be forced into the
  drop-in shape.

  W8 closes the bootstrap milestone, not Stage 1: the fork switched and
  running against oxpinyin, wire-level parity on the defined bootstrap
  surface, cargo-c packaging, and a compatibility + performance report
  that establishes the Stage-2 measurement baseline. Stage 1 parity
  continues through W10–W12 and closes when the parity bar is met.
  Measuring Stage-2 baselines while Stage-1 work continues is deliberate —
  those numbers are prerequisites for improving against them. Remaining
  work is not W8 — it is W10–W14 below.

- **W9 is the training toolchain — full-scope re-audit (2026-08-30).**
  W9 now targets **complete native-Rust parity with the currently-used
  libpinyin/trainer workflow**: segmentation (`ngseg`, `spseg`,
  `mergeseq`), K-mixture-model generation and optimisation (generate →
  estimate → merge → validate → prune → export → KMM→interpolation),
  evaluation (`estimate_interpolation` λ + `eval_correction_rate`), word
  recognition (prepare → populate → partialword → newword → markpinyin),
  punctuation generation, and the corpus/index/status orchestration that
  drives them — implemented natively, with no dependency on libpinyin
  binaries/libraries, the Python trainer scripts, SQLite, or `make` at
  runtime.

  This **supersedes the earlier deliberate scope cut** that skipped the
  KMM path. A source-level call-graph re-audit
  (`docs/findings/trainer-parity-audit.md`, pinned to libpinyin `2.11.91`
  and trainer `b192737`) shows the trainer's five-stage main pipeline is
  KMM throughout: `gen_k_mixture_model` is the load-bearing corpus
  counter, and the shipped `interpolation2.text` is produced by
  `k_mixture_model_to_interpolation` off a merged-and-pruned KMM — **not**
  by the legacy `gen_ngram`/`export_interpolation` path.

  First increment shipped five stages — segmenter (`ngseg`), counter
  (`gen_ngram`), held-out/λ estimator (`gen_deleted_ngram` +
  `estimate_interpolation`), emitter (`export_interpolation`), corpus
  front-end (zhwiki cleaner). `ngseg` remains the active default
  segmenter and the corpus front-end remains active; the re-audit
  reclassifies only the n-gram counting and export utilities —
  `gen_ngram`/`gen_unigram`/`gen_deleted_ngram`/`export_interpolation` —
  as **legacy libpinyin utilities that the trainer does not invoke**
  (kept, correct, retitled); `estimate_interpolation`'s λ EM stays on the
  real path inside `evaluate.py`. Remaining scope is
  decomposed as Parts B–H in the audit: `spseg`/`mergeseq`
  (`oxpinyin-segment`); the KMM pipeline (`oxpinyin-kmm`); the evaluator
  (`oxpinyin-eval`, reusing the engine decoder); word recognition
  (`oxpinyin-word`); punctuation (`oxpinyin-punct`); native end-to-end
  orchestration.

- **W10–W12 are three parity workstreams, not one.** They have different shapes —
  bounded/mechanical (W10), architectural (W11), open-ended (W12) — and
  bundling them would make completion hostage to the least predictable
  member. Same reasoning that flattened W7.

- **W10 LANDED (7fca228, ccb52d4, dade719).** Correction
  (`PINYIN_CORRECT_*`), fuzzy/ambiguity (`PINYIN_AMB_*`), and
  `DYNAMIC_ADJUST` option bits are implemented. Correction and fuzzy bits
  feed parser-table selection and are verified against the pinned oracle via
  the parse differential. `DYNAMIC_ADJUST` gates the bigram term of
  candidate frequency at guess time; the full three-gate implementation
  (matching the pin's three call sites) landed in PR #204 (commit
  217d0c4).

- **W11 LANDED (ffb2a22, 9ff0c61, 41227a0, 75d709a).** User, network, and
  addon phrase union at lookup is implemented: `pinyin_load_addon_phrase_library`
  is live, and user-dictionary phrases surface as candidates. The prediction
  gap is also closed — `pinyin_guess_predicted_candidates_with_punctuations`
  and `pinyin_choose_predicted_candidate` are implemented (PR #111,
  75d709a). Architecture ground: `docs/findings/phrase-union.md`.

- **W12 is the corpus tail.** Closed 2026-08-22: the candidate surface
  agrees with the pinned oracle bit-identically on every W2 corpus input
  at depth 10 (10,190 / 10,190 / 98,930 of 98,930 / absent 0 /
  tie-swaps 0). Class B (`ni''hao`) closed 2026-08-21 by the
  doubled-apostrophe alignment; Class A — the 12 top-two comparator
  tie-swaps and the 1,036 order-only / 4,058 prefix-10 residuals, all one
  species — closed 2026-08-22 by porting the pin's tie law (the amplified
  f32 frequency key and the array order its stable sort keeps,
  `docs/testing/corpus-tail.md`, `pin-refreeze-2026-08.md` third
  amendment). The completion criterion was diagnosis-driven, and every
  diagnosed class is now zero. Also parked
  here: the live-typing behaviors the parity sequence doesn't yet
  exercise (deep paging, mid-composition edits, punctuation modes).

- **W13 LANDED (20e6b3a).** Double-pinyin (ZRM/MS/Ziguang/ABC/PYJJ/Xiaohe)
  and standard bopomofo (Zhuyin) schemes are implemented and verified against
  the pinned oracle on their scheme-specific differential surfaces. The
  double-pinyin SPEC is **frozen** (2026-09-02, maintainer freeze of the
  Phase 0 draft after landed W13; freeze record at the bottom of
  `docs/findings/double-pinyin-spec.md`). The freeze fixed the FORCE_TONE
  law for both parse seams, and the batch seam
  (`pinyin_parse_more_double_pinyins`) now implements it (5ec782ea): the
  caller's option word crosses the seam and drives the
  `pinyin_parser2.cpp:412` length-3 gate plus the `USE_TONE` tone carriage,
  measured byte-identical against the pinned oracle over full model20 KC
  tables for double schemes 1, 2, 4, 5, 6 — including a new FORCE_TONE /
  USE_TONE `tonelaw` probe section in the scheme differential — and for all
  eight bopomofo keyboards, with `run-key-surface-diff.sh` still IDENTICAL
  (2,131 probe lines). No W13 items remain; the divergence register's
  FORCE_TONE entry carries the closure.

- **W14 LANDED (489e94d, PR #113).** Three parts, all delivered: (a) sentence
  candidates emit with real unigrams loaded — up to N n-best rows prepended
  and merged/retyped as NBEST_MATCH per `pinyin.cpp:2290-2298`; (b)
  `pinyin_get_sentence` returns the decoded n-best text (index 0 = 1-best),
  not raw input; (c) `SORT_WITHOUT_SENTENCE_CANDIDATE` gating. Corpus
  candidate pins bit-identical; sentence-surface fixture green (488/496 1-best,
  385/496 n-best distinct-set). The §12 measured 117-position ordered/first-6
  residual (hypothesis selection, trellis-side) is **FROZEN as a permanent
  Stage-1 divergence** (maintainer ruling 2026-09-02,
  `docs/findings/sentence-surface.md` §12): the pin's `gfloat` trellis
  accumulation is not bit-reproducible under the constitution's determinism
  rule, and the gate `sentence_surface_matches_the_declared_residual` holds
  the frozen 488/385/379 as a defined residual, not a parity target. The
  predicted-candidate ordering divergence is recorded as accepted
  (`docs/findings/sentence-surface.md`) and is not an open implementation task.
  Nothing in W14 remains open.
