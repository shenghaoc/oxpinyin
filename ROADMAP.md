# Roadmap

Portable Rust re-expression of
[libpinyin](https://github.com/libpinyin/libpinyin). Constitution and agent
rules: `AGENTS.md`. Crate roles: `.kiro/steering/structure.md`.

> **Project identity:** **oxpinyin** (repo, crate, docs). The prior
> project name is retained in git history only.
> Shipped artifact naming for the libpinyin drop-in (`libpinyin.so.15`) is a separate
> concern from project identity.

## Stages

| Stage | Goal |
|---|---|
| **0** | Scaffold, pin, SPECs/fixtures (complete) |
| **1** | Exact-output parity with the pin-built libpinyin oracle (differential testing) — complete: candidate surface bit-identical on all 10,190 corpus rows (`docs/testing/corpus-tail.md`) |
| **2** | Measured upgrades — smaller binary, faster execution, lower RAM first (in progress, `docs/perf/`); model upgrades (trigram/KN, typo edges, own data) remain candidates — every divergence vs the Stage 1 baseline |

Stage 1 uses installed/libpinyin-format tables (no redistribution required:
`docs/findings/model-provenance.md` — build-time fetch of the pinned model20
archive, compiled by `oxpinyin-datagen`; on Kyoto Cabinet and tkrzw an
unmodified libpinyin install's `data/` opens as is). Stage 2 is optional and
measurement-gated.

## Reference pin

Authoritative freeze: `docs/testing/oracle-environment.md`  
(libpinyin `2.11.92` / ibus-libpinyin `1.16.5` / model archive SHA-256s).

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

## Phase 0 (blocked feature work; now recorded)

Recorded — see `.kiro/specs/foundation/tasks.md` and findings. The one
open Phase 0 item is the consolidated F-E cross-lane evidence register
(foundation task 4): the 14 cases (F-E-01..13 enumerated in the spec,
F-E-14 registered 2026-09-05) have their evidence spread across the
findings and testing docs, not yet one artifact on main. Beside it, the
Stage-2 open-items list below carries the W8 drop-in user-file write
path.

| Need | Output |
|---|---|
| Pin + recipe | `docs/testing/oracle-environment.md` (recorded) |
| ABI surface | `docs/findings/abi-subset.md` (recorded: the consumer union, then the full 79/79 export set — §6) |
| Upstream schema | `docs/findings/upstream-schema.md` (recorded) |
| Parser / path-set / scoring SPECs | frozen 2026-08-09: `docs/findings/parser-spec.md`, `parser-path-set.md`, `scoring-spec.md` |
| Data load route (D3) | decided: oxpinyin-data loads libpinyin-format tables (`.kiro/steering/structure.md`); native production of those tables is W15 |
| Capture harness + F-A fixtures | built: `tools/capture/`, `docs/testing/capture-fixtures.md`, `fixtures/foundation/f-a.txt`, `f-c.txt`; F-E register still open |

## Stage 1 workstreams (names only)

| ID | Focus | Crate(s) |
|---|---|---|
| W1 | Types, parser, correction flags | oxpinyin-core |
| W2 | Oracle FFI + differential runner | pinyin-oracle |
| W3 | Table loading | oxpinyin-data |
| W4 | SegmentGraph, k-best, engine session | oxpinyin-core, oxpinyin-engine |
| W5 | C ABI (began as the consumer subset; the full 79-symbol surface closed under W8) | oxpinyin-capi |
| W6 | User store (ACID store over the compiled-in `DefaultStore`; began on redb) | oxpinyin-user, oxpinyin-store |
| W7 | Classic text-format interop via oxpinyin-dictool (import + export) | oxpinyin-dictool, oxpinyin-capi |
| W8 | libpinyin drop-in: full 79-symbol `.so` ABI under `libpinyin.so.15` (see `.kiro/specs/drop-in/`) | oxpinyin-capi |
| W9 | Training toolchain — full trainer-workflow parity (KMM in scope; see `docs/findings/trainer-parity-audit.md`) | oxpinyin-segment, oxpinyin-kmm, oxpinyin-eval, oxpinyin-word, oxpinyin-punct, oxpinyin-lambda, oxpinyin-corpus, oxpinyin-train (legacy: oxpinyin-counter, oxpinyin-emitter) |
| W10 | Option bits: correction, fuzzy/ambiguity, dynamic-adjust gating | oxpinyin-core, oxpinyin-engine |
| W11 | Phrase-index union at lookup (user, network, addon) | oxpinyin-engine, oxpinyin-data, oxpinyin-user |
| W12 | Corpus tail (parity gaps; candidate residual closed 2026-08-22) | oxpinyin-core, oxpinyin-engine, oxpinyin-capi |
| W13 | Double-pinyin and bopomofo input schemes (feature implementation) | oxpinyin-core, oxpinyin-engine |
| W14 | Sentence surface (n-best emission, NBEST_MATCH typing, get_sentence) | oxpinyin-capi, oxpinyin-engine |
| W15 | model20-native runtime-data production, every backend — since P5/P6 in libpinyin's own file formats, read directly by the runtime | oxpinyin-datagen, oxpinyin-store, oxpinyin-data, oxpinyin-runtime |

Crates outside the Stage 1 table (roles in `.kiro/steering/structure.md`):
`oxpinyin-facade` and `oxpinyin-runtime` (the shared orchestration and
assembly layers under both C ABIs and the Python binding),
`oxpinyin-chewing` (the excisable zhuyin layer, D6 seam:
`docs/findings/chewing-crate-seam.md`), `oxpinyin-zhuyin-capi`
(`libzhuyin.so.15`, upstream's `--enable-libzhuyin` counterpart, 52
symbols), `oxpinyin-python` (`docs/python.md`; spec
`.kiro/specs/python-binding/`, three open items), and
`oxpinyin-testsupport` (dev-only).

**Stage 1 status (2026-09-06):** every workstream above has landed or
closed — W9, W10, W11, W13, W14 and W15 carry LANDED notes below, W12
closed 2026-08-22, and `README.md` records Stage 1 as complete. Still
open or pending:

- W8 drop-in task 9 — the write path for learned user data in
  libpinyin's own user-file format (`.kiro/specs/drop-in/tasks.md`; the
  user store is `user_store.<ext>` today).
- The Phase 0 F-E register (foundation task 4).

Parked, not open: the W12 live-typing behaviours the parity sequence does
not exercise (`docs/findings/live-typing.md`, no pin gates) and the
shelved BerkeleyDB compat path (drop-in task 10).

### Workstream notes (recorded as decisions settle)

- **tkrzw is the default selected backend** (05688575, 2026-09-05;
  Kyoto Cabinet had been the default since 2026-08-29 — RHEL 10.2 ships
  `tkrzw-devel` but not `kyotocabinet-devel`, which made the KC default
  unbuildable from source on the primary development machine). The four
  supported oxpinyin store backends — tkrzw, Kyoto Cabinet, LMDB, redb —
  are peer implementations behind one `ReadStore`/`WriteStore` trait
  surface, and any single build compiles in exactly one of them
  (`DefaultStore`; `oxpinyin-store` refuses a build with zero or more
  than one backend feature at compile time). tkrzw is the feature in the
  workspace's default set; the other three are selected explicitly with
  `--no-default-features --features {kyotocabinet|lmdb|redb}`. redb is
  the pure-Rust portability fallback; KC/tkrzw/LMDB are C dependencies.
  System data files carry libpinyin's own names on Kyoto Cabinet and
  tkrzw (the drop-in set) and `<stem>.<ext>` on redb and LMDB; the user
  store is `user_store.<ext>` (`kct`/`tkt`/`lmdb`/`redb`). Switching
  backends is a storage-format transition — the runtime does not
  transparently open one backend's files with another, and old
  backend-specific user data is not carried across the switch. (This
  matches the model distributions use for libpinyin's own backend
  transitions.)

- **W15 LANDED.** The data pipeline inversion is complete: runtime tables
  are compiled natively from the canonical pinned `model20` archive for every
  storage backend (tkrzw, Kyoto Cabinet, LMDB, redb) — no producer consumes
  libpinyin-generated runtime data. Implemented in `crates/oxpinyin-datagen`;
  all four backend producers are feature-gated in its `Cargo.toml`. The
  retired `oxpinyin-migrate` route (oracle ABI export + verbatim Tkrzw
  conversion) was proven unnecessary by the native compilation.
  Architecture and the canonical-source invariant:
  `docs/findings/datagen-model20.md`.

  **P1–P6 (2026-09-01 → 2026-09-02) changed what those tables are.**
  `oxpinyin-datagen compile` now writes the data directory libpinyin's
  own build produces — the sixteen per-library chunk files (byte-exact
  against the pin), `pinyin_index.bin`, `phrase_index.bin`, `bigram.db`,
  `punct.bin`, the `addon_*` pair and `table.conf` — through the selected
  backend; on Kyoto Cabinet and tkrzw under libpinyin's names, on redb
  and LMDB as the same records in that backend's container
  (`docs/findings/datagen-compat-2026-09-01.md`; the pre-P6 native
  schema and its serializers are gone). The production runtime reads
  those files directly through lazy readers — a handle plus a mmap per
  table, nothing scanned at open — so `Runtime::open`, `pinyin_init` and
  the Python binding open a system directory the way libpinyin does
  (`docs/findings/runtime-direct-libpinyin-data-2026-09-02.md`).
  `interpolation2.text` is consumed by datagen only and is no longer
  emitted or read at runtime (3f0f0f36). The drop-in invariant is gated
  end to end by `tools/bisection/run-same-data-dir-diff.sh`: the
  pin-built `libpinyin.so` and oxpinyin's C ABI open one unchanged
  libpinyin `data/` and are byte-identical on every surface but the two
  registered divergences. Init fell from ~100× the pin to within ~1.3×
  (`docs/perf/perf-baseline-kc-2026-09.md`,
  `docs/findings/perf-backend-matrix-2026-09.md`).

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
  No T-numbering here — one deliverable delivered in one PR, flat like the
  decoder-λ fix (PR #55).

- **W8 is the libpinyin drop-in: the full `.so` ABI, not a fork
  bootstrap (supersession declared 2026-08-29, closed 2026-08-30 with all
  79 symbols live).** oxpinyin-capi maintains the whole
  live upstream export surface — 79/79 `pinyin_*` symbols from
  `libpinyin.ver` at the pin (`pinyin_get_raw_full_pinyin` excluded, dead
  in upstream itself) — under libpinyin's own binary identity: SONAME
  `libpinyin.so.15`, `LIBPINYIN` symbol versions, the header under
  `libpinyin-2.11.91/`, and `libpinyin.pc`, all produced by cargo-c
  (`docs/packaging.md`, `docs/findings/drop-in-abi-identity.md`). The
  goal is the compatibility policy's: rename the built object to
  `libpinyin.so.15`, put it on the library path, and unmodified consumers
  work against the data already on the system
  (`docs/findings/compatibility-policy.md`). The spec is
  `.kiro/specs/drop-in/`; the supersession record is
  `docs/findings/abi-subset.md` §6 (3d918866).

  This supersedes the earlier 51-symbol bootstrap contract for the
  maintainer's ibus-libpinyin fork (`feat/oxpinyin-backend`, tip
  `0d71866`), which itself had superseded "capi + forked frontend" and
  "ibus-pinyin-rs zbus rewrite". The fork surface is now a historical
  complement inside the full ABI, not the boundary; the drop-in shape
  the fork route was meant to avoid is the shape shipped. The libchewing
  precedent (library-only rewrite, frontends left alone) still applies —
  more strongly, since no frontend change is needed at all.

  Landed: binary identity and cargo-c metadata (#206, #192); the compat
  read path over installed libpinyin data (#228); measured drop-in on
  Fedora rawhide (Kyoto Cabinet), Debian testing (tkrzw) and NixOS —
  1,571/1,571 corpus rows, sorted sets byte-identical, the only
  divergence the R1 defined-order rule (`docs/findings/upstream-divergences.md`,
  2026-08-30). Since P6 (2026-09-02) there is no compat layer at all:
  the runtime reads an unmodified install's `data/` through the same
  readers it uses for its own output (W15 note). Open in the drop-in
  spec: task 9, the write path for learned user data in libpinyin's own
  user-file format; task 10, the BerkeleyDB compat path, is shelved until
  a consumer requires it. The spec's design/requirements text still
  describes the pre-P6 `compat/` modules and the 58-symbol consumer
  union; the code and `abi-subset.md` §6 are the current record.

  Stage-2 baselines were measured while Stage-1 parity work continued —
  those numbers are prerequisites for improving against them. Parity
  work was never W8 — it was W10–W14 below, all now landed.

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
  real path inside `evaluate.py`. The scope was
  decomposed as Parts B–H in the audit: `spseg`/`mergeseq`
  (`oxpinyin-segment`); the KMM pipeline (`oxpinyin-kmm`); the evaluator
  (`oxpinyin-eval`, reusing the engine decoder); word recognition
  (`oxpinyin-word`); punctuation (`oxpinyin-punct`); native end-to-end
  orchestration.

  **W9 LANDED (2026-08-31).** Parts B–H are all implemented and tested,
  including the `oxpinyin-train` orchestrator (raw corpus → segment →
  KMM → interpolation model → λ → correction rate, no Python/make/SQLite/
  libpinyin at runtime). The audit's §15 status table records each part
  and states that nothing remains for trainer-workflow parity; the
  unported helpers are its §11 deliberate exclusions.

- **W10–W12 are three parity workstreams, not one.** They have different shapes —
  bounded/mechanical (W10), architectural (W11), open-ended (W12) — and
  bundling them would make completion hostage to the least predictable
  member. Same reasoning that flattened W7.

- **W10 LANDED (7fca2283, ccb52d4a, 217d0c4e).** Correction
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
  `docs/testing/corpus-tail.md`,
  `docs/findings/pin-refreeze-2026-08.md` third amendment). The completion criterion was diagnosis-driven, and every
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
  (`pinyin_parse_more_double_pinyins`) now implements it (c5d0b088): the
  caller's option word crosses the seam and drives the
  `pinyin_parser2.cpp:412` length-3 gate plus the `USE_TONE` tone carriage.
  Measured against the pinned oracle over full model20 KC tables, evidence
  sources distinct: the batch `tonelaw` profiles (FORCE_TONE/USE_TONE) ran
  on all six double schemes — the whole differentials byte-identical on
  1, 2, 4, 5, 6, scheme 3 differing only in the pre-existing §12 row — the
  eight bopomofo keyboards stay byte-identical on the existing chewing
  corpus (no FORCE_TONE profile; regression coverage), and the one-key seam
  gate `run-key-surface-diff.sh` stays IDENTICAL (2,131 probe lines,
  including its FORCE_TONE profile sweep). No W13 implementation items
  remain; the divergence register's FORCE_TONE entry carries the closure.
  The bopomofo SPEC is frozen as well: the 2026-09-03 draft was frozen
  as drafted by the maintainer's ruling of 2026-09-06 (PR #353; freeze
  record at the bottom of `docs/findings/bopomofo-spec.md`). Its one
  open implementation item — the pinyin facade's chewing batch seam does
  not forward `FORCE_TONE` — stays open under the frozen law, carried by
  the divergence register.

- **W14 LANDED (489e94d, PR #113).** Three parts, all delivered: (a) sentence
  candidates emit with real unigrams loaded — up to N n-best rows prepended
  and merged/retyped as NBEST_MATCH per `pinyin.cpp:2290-2298`; (b)
  `pinyin_get_sentence` returns the decoded n-best text (index 0 = 1-best),
  not raw input; (c) `SORT_WITHOUT_SENTENCE_CANDIDATE` gating. Corpus
  candidate pins bit-identical. The §12 ordered/first-6 residual
  (hypothesis selection, trellis-side) is **FROZEN as a permanent
  Stage-1 divergence** (maintainer ruling 2026-09-02,
  `docs/findings/sentence-surface.md` §12): the pin's `gfloat` trellis
  accumulation is not bit-reproducible under the constitution's determinism
  rule, and the gate `sentence_surface_matches_the_declared_residual` holds
  the frozen numbers as a defined residual, not a parity target. Frozen at
  488/385/379 (1-best / n-best distinct-set / ordered, of 496); **re-frozen
  2026-09-04 at 491/396/390** (94b38948, maintainer ruling 2026-09-04) after
  P6 moved the trellis unigram onto the item field libpinyin reads — 11
  ordered-list inputs fixed, none regressed, residual 117 → 106. The
  predicted-candidate ordering divergence is recorded as accepted
  (`docs/findings/sentence-surface.md`) and is not an open implementation task.
  Nothing in W14 remains open.

## Stage 2 (in progress; measurement-gated)

Entry point: `docs/perf/README.md` (dated snapshots;
`perf-stage2-harness-2026-08.md` is the harness). What has landed so far
is the size/speed/RAM work the constitution puts first, each change
measured against the pin in the same container:

- **Measurement harness** (2026-08-19): Criterion groups on the C-ABI
  surface, the `profiling` cargo profile, `tools/profile/run-w8-cycle.sh`.
- **P1–P6 data-layer inversion** (2026-09-01 → 2026-09-02): the runtime
  reads libpinyin's own file formats directly (W15 note above). x86_64,
  same data directory as the pin: init within ~1.1× (KC) / ~1.3× (tkrzw)
  of the pin, from ~100× before (`docs/findings/perf-backend-matrix-2026-09.md`).
  ARM64/KC re-baseline: init 102 → 21 ms, RSS 72,652 → 28,388 KiB,
  runtime data 101.80 → 36.88 MiB (`docs/perf/perf-baseline-kc-2026-09.md`).
- **Release profile**: fat LTO + one codegen unit
  (`docs/perf/perf-so-size-2026-09.md`); `panic = "abort"` was tried and
  reverted (1b0c84a0, +5.5% keystroke cycle for −64 KiB).
- **Store/user-crate optimisation** (2026-09-05 → 2026-09-06): redb,
  LMDB and user-store hot paths, measured in
  `docs/findings/perf-store-opt-2026-09.md` (S5, F4).

Next targets named by the P6 finding: per-instance `pinyin_alloc_instance`
key-cost table (~16.5 ms, memoize or defer) and the steady-state candidate
lookup (~1.5× the pin). Model upgrades (trigram/KN, typo edges, own data)
have no landed work and remain candidates behind the same gate.
