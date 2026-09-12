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

Build: `tools/oracle/build-oracle.sh`; `tools/bisection/Dockerfile.perf-matrix`
carries a prebuilt oracle at `/opt/libpinyin-tkrzw` for the perf work.

## How work proceeds

1. **Freeze behaviour** into `docs/findings/` SPECs and golden fixtures
   (characterisation of the pin first; see `docs/findings/spec-derivation.md`).
2. **Implement** under the Source policy in `AGENTS.md` (copy upstream,
   rewrite in Rust, then oxidize), with frozen SPECs/fixtures as the gate.
3. **Prove** with fixture tests everywhere; live oracle diff on Linux is the
   verification tier for Stage 1 gates.

Detailed task cards live under `.kiro/specs/` as they are derived. Until a
SPEC is frozen, do not implement that slice.

## Phase 0 (recorded; closed)

Recorded — see `.kiro/specs/foundation/tasks.md` and findings. The last
Phase 0 item, the consolidated F-E cross-lane evidence register
(foundation task 4), is `docs/findings/robustness-evidence.md`: all 14
cases (F-E-01..13 from the spec, F-E-14 registered 2026-09-05), each
with its lane and evidence entry, on main since 2026-08-30. Closed
2026-09-08. The W8 drop-in user-file read/write path, the last Stage 1
item listed below, remains open — rescoped 2026-09-09 to the seamless
same-backend requirement (maintainer ruling;
`docs/findings/compatibility-policy.md`, goal amendment).

| Need | Output |
|---|---|
| Pin + recipe | `docs/testing/oracle-environment.md` (recorded) |
| ABI surface | `docs/findings/abi-subset.md` (recorded: the consumer union, then the full 79/79 export set — §6) |
| Upstream schema | `docs/findings/upstream-schema.md` (recorded) |
| Parser / path-set / scoring SPECs | frozen 2026-08-09: `docs/findings/parser-spec.md`, `parser-path-set.md`, `scoring-spec.md` |
| Data load route (D3) | decided: oxpinyin-data loads libpinyin-format tables (`.kiro/steering/structure.md`); native production of those tables is W15 |
| Capture harness + F-A fixtures | built: `tools/capture/`, `docs/testing/capture-fixtures.md`, `fixtures/foundation/f-a.txt`, `f-c.txt`; F-E register: `docs/findings/robustness-evidence.md` (14 cases) |

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
`.kiro/specs/python-binding/`, all items closed 2026-09-08), and
`oxpinyin-testsupport` (dev-only).

**Stage 1 status (2026-09-12).** *Implementation:* complete — every
workstream above has landed or closed (W9, W10, W11, W13, W14 and W15
carry LANDED notes below, W12 closed 2026-08-22), and W8 drop-in task 9
landed 2026-09-09: user files are read and written in libpinyin's own
formats, seamless in both directions with a same-backend libpinyin per
the 2026-09-09 maintainer ruling (`docs/findings/compatibility-policy.md`,
goal amendment), measured by `tools/oracle/user-dir-round-trip.sh`
against Kyoto Cabinet and tkrzw oracles (`docs/findings/user-store.md`
§11). *Verification:* one gap open — the differential suite does not yet
drive all 58 consumer-union symbols, so the uncovered ones are
unverified rather than compliant; closing it is work
(`docs/findings/compatibility-policy.md`, §(e) consequence 3), tracked
here rather than only in the policy. One defect open under the policy:
row 30, the pinyin facade's chewing batch `FORCE_TONE` seam. `README.md`
states Stage 1 the same way.

Parked, not open: the W12 live-typing behaviours the parity sequence does
not exercise (`docs/findings/live-typing.md`, no pin gates). The
BerkeleyDB backend (drop-in task 10) landed 2026-09-12 and is no longer
parked.

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
  dir is libpinyin's own file set under the same naming rule (`user.conf`
  names the backend family, and a non-conforming profile is wiped on
  open as upstream's `check_format` does). Switching
  backends is a storage-format transition — the runtime does not
  transparently open one backend's files with another, and old
  backend-specific user data is not carried across the switch. (This
  matches the model distributions use for libpinyin's own backend
  transitions, and is policy since 2026-09-09:
  `docs/findings/compatibility-policy.md`, goal amendment.)

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
  registered divergences — the predicted-candidate tie order and the
  single `union-diff` bigram-prediction row (policy row 20). Init fell from ~100× the pin to within ~1.3×
  (`docs/findings/runtime-direct-libpinyin-data-2026-09-02.md`, whose
  oxpinyin cells were built by `cargo build`, not the shipping
  `cargo cinstall`; the two ends of that ~100× were built differently, so it
  is not a like-for-like quotient — see
  `docs/findings/perf-build-recipe-audit-2026-09-10.md`). Measured since on
  the shipping path: init at 1.16× (Tkrzw) / 1.12× (KC)
  (`docs/findings/perf-backend-matrix-2026-09.md`) and 4.9× after the
  key-cost deferral moved work out of init
  (`docs/perf/perf-baseline-kc-2026-09.md`).

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
  readers it uses for its own output (W15 note). Task 9, learned user
  data read and written in libpinyin's own user-file format, landed
  2026-09-09 (`docs/findings/user-store.md` §11); task 10, the
  BerkeleyDB backend, landed 2026-09-12 on the consumer ask as the fifth
  store peer — non-default, libdb 5.3 only, verified against a
  `--with-dbm=BerkeleyDB` oracle in Debian and Fedora containers
  (`docs/findings/berkeleydb-backend.md`, which also records the
  `user_driver.c` regression it found in the round-trip harness). The
  spec's design and requirements were brought to the P6 architecture and
  the 79-symbol surface on 2026-09-12.

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
  reads libpinyin's own file formats directly (W15 note above). Same data
  directory as the pin: init within ~1.1× (KC) / ~1.3× (tkrzw)
  of the pin, from ~100× before
  (`docs/findings/runtime-direct-libpinyin-data-2026-09-02.md` — a
  `cargo build` artifact, not the shipping `cargo cinstall` one, so it reads
  high, and it states no host architecture;
  `docs/findings/perf-build-recipe-audit-2026-09-10.md`). The "~100× before"
  is **cross-recipe and not like-for-like**: its baseline is the `cinstall`
  #260 matrix (`docs/findings/perf-backend-matrix-2026-08-31.md`) and its
  endpoint the `cargo build` figure above. At that magnitude the recipe is
  noise and the improvement is real, but the two ends were not built the
  same way.
  ARM64/KC re-baseline: init 102 → 21 ms, RSS 72,652 → 28,388 KiB,
  runtime data 101.80 → 36.88 MiB (`docs/perf/perf-baseline-kc-2026-09.md`).
- **Release profile**: fat LTO + one codegen unit
  (`docs/perf/perf-so-size-2026-09.md`); `panic = "abort"` was tried and
  reverted (1b0c84a0, +5.7% keystroke cycle for −64 KiB — the revert's
  commit subject says 5.5%; the records give +5.5–5.7% steady, the
  baseline `docs/perf/perf-baseline-kc-2026-09.md` +5.7%).
- **Store/user-crate optimisation** (2026-09-05 → 2026-09-06): redb,
  LMDB and user-store hot paths, measured in
  `docs/findings/perf-store-opt-2026-09.md` (S5, F4).

The two targets the P6 finding named have both moved on, and its figures for
them should not be quoted as current. The per-instance `pinyin_alloc_instance`
key-cost table (P6: ~16.5 ms) was deferred to first `new_session` on 2026-09-04
and then **eliminated** on 2026-09-07 (`6886dc1f`,
`docs/findings/perf-keycost-first-alloc-2026-09-07.md`). The steady-state
candidate lookup (P6: ~1.5× the pin) has been re-measured three times since —
at parity on amd64, ~1.16× on arm64, and below 1 on the current tree
(`docs/findings/perf-backend-matrix-2026-09.md`,
`docs/findings/perf-keycost-first-alloc-2026-09-07.md`,
`docs/findings/perf-cycle-ir-differential-2026-09-08.md`); P6's own figure
additionally reads high because its artifact was a `cargo build` fixture rather
than the shipping `cargo cinstall` library
(`docs/findings/perf-build-recipe-audit-2026-09-10.md`). The current Stage-2
target is allocation churn on the steady cycle, per the IR differential. Model
upgrades (trigram/KN, typo edges, own data) have no landed work and remain
candidates behind the same gate.
