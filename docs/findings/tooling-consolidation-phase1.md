# Developer-tooling consolidation — Phase 1 (research and architecture)

> **Historical inventory snapshot — not active policy.** This record reflects
> the named tree. The `Assisted-by` commit-trailer policy and its linter, hook,
> test harness, and CI jobs were later retired; their references here are
> archival and do not describe current requirements.

> **Architecture amended 2026-10-18 UTC — read this first.** The maintainer
> adopted Python 3 for developer-tool orchestration and `just` as the
> cross-platform task interface. The architecture in section C, decisions
> D1–D11 in section E and the migration stages in section D are superseded by
> [`tooling-consolidation-python-just.md`](tooling-consolidation-python-just.md)
> (Amendment 1). The unmerged Rust ABI foundation and the standalone Rust
> initbench are **not** required components. The inventory, findings and
> historical measurements in this report remain valid; its Rust destinations
> and Stage 1–13 plan are not. Amendment 1's new Stage 1 retains its separate
> authorisation. New Stage 2 and later stages require the review and explicit
> maintainer approval that Amendment 1 describes.


Dates (captured with `date -u` on the executing host, never hand-written):
tree inspected and inventory recorded 2026-10-16 UTC (reading passes ran
between roughly 15:40 and 17:00 UTC); maintainer ruling received and the original
Phase 1 revision written 2026-10-16 UTC. Amendment 1 is dated 2026-10-18 UTC.

Status at the time of Phase 1 (historical): **Stage 0 of the roadmap (this
documentation) is authorised and landed by the maintainer's ruling of
2026-10-16 UTC; Stage 1 (the shared Rust ABI foundation) is authorised only
after Stage 0 is accepted and only within the corrections recorded in
section 0; Stages 2–13 are not authorised; no legacy tool is authorised for
deletion.** The ruling and the three architecture corrections it requires are
recorded in section 0 and applied throughout; where an earlier draft of this
document said otherwise, the ruling wins. **Amendment 1 superseded the old
Stage 1 authorisation and Rust architecture on 2026-10-18 UTC. New Stage 1 retains its separate authorisation. The review
gate starts at new Stage 2.**

Tree inspected: `origin/main` at `e67f19d1` (a worktree detached from the
shared checkout, which stayed on `main` untouched). The branch carrying this
document bases on the later `d1e151d7`, whose one additional commit touches
no tool. Upstream trees: libpinyin `074a2219` (the pin), libchewing
`3c4a93aa` (2026-03-28), mozc `0136c357` (2026-10-16), anthy-unicode
`fc2260cf` (2026-09-10).

Companion files (same directory):

| File | Contents |
| --- | --- |
| `tooling-consolidation-phase1-inventory.md` | Appendix A: every inventory record (the ten-field table per tool), 290 records — the migration ledger |
| `tooling-consolidation-phase1-summary-table.md` | Appendix A1: one row per record — ID, path, language, platform, purpose, destination |
| `tooling-consolidation-phase1-upstream-survey.md` | Appendix F: the four upstream projects, observation/interpretation labelled |
| `tooling-consolidation-python-just.md` | Amendment 1: Python 3 orchestration and `just`; the current architecture, decisions and stages |
| `superseded-tooling-prs.md` | Retirement record for #836, #837 and #842: exact revisions, recovered requirements and findings |
| `startup-initialization-audit.md` | Recovered initialization-path audit from #842 |
| `../perf/startup-baseline-2026-10.md` | Recovered startup baseline from #842 |

Method. Every first-party tool file was read in full (not inferred from
names) by nine reading passes over the detached tree, each producing records
in one fixed format; the orchestration layer, the FFI crate, the capture
harness, the registry, the policy documents and the PR dispositions were
read a second time by the author to verify the parts the architecture rests
on. Counts that matter were re-measured directly (for example the
contract-diff case registry was loaded and counted: 3,449 cases, 2,561 in a
default run). Where a reading pass and a direct check disagreed, the direct
check wins and is noted.

---

## 0. Maintainer ruling (2026-10-16 UTC) and the corrections it requires

> **Historical decision record.** D1–D11 and the three corrections below were
> written for the Rust architecture. Amendment 1 replaces the decisions and
> the stages; the rulings are kept as the record of what was asked.

The ruling approves Stage 0 now, authorises Stage 1 after Stage 0 is
accepted, leaves Stages 2–13 unauthorised, and approves no deletion. It is
an approval of the specific design choices below, not permission to resolve
further architectural questions independently; a genuinely new ABI,
dependency or policy decision remains a STOP under contributor guidance.

| Decision | Ruling | Conditions recorded |
| --- | --- | --- |
| D1 dependencies | approved with changes | `libloading` approved. Hand-written JSON parsing as a general policy rejected: `serde`/`serde_json` permitted for structured data, with the exact legacy serialisers retained where byte identity matters (`pinyin-capture-v1`, contract-diff's sorted-key rows, the diff/divergence logs). `clap` acceptable when the CLI is implemented, not required in Stage 1. No other dependency authorised. |
| D2 crates | approved with changes | In-workspace `libpinyin-abi` and `oxpinyin-devtool` as internal package names. The generic CLI must not require `pinyin-oracle` or any oxpinyin internal crate; project-specific integration is optional (a feature). Public executable name deferred. No repository split or publication now. |
| D3 goldens | approved: option (a) | Retain every existing golden byte; make the harness cell-parametric; record the actual backend provenance **per fixture** (section A.4), including the Berkeley DB tie-order golden. |
| D4 instruments | approved: option (a) | Retain the measured C instruments, including PR #767's workloads; Rust consolidates orchestration and reporting; rewriting an instrument needs a separate future decision. |
| D5 ledgers | partial | Historical ledgers are revision-pinned records; their generators are **not** authorised for retirement; #793's reproducibility and re-review questions come first. |
| D6 registry | approved | `runners.json` stays the single authoritative catalog, extended compatibly; no Rust-hard-coded replacement, no second maintained catalog, and its path is not moved automatically later. |
| D7 verdicts | approved | 0 success / 77 skipped / other failure at the gate boundary; the structured result distinguishes execution errors from comparison differences; an explicitly requested missing input never becomes a successful skip. |
| D8 CI | plan only | The general order is approved; every workflow edit needs its own authorisation and old-versus-new verification. |
| D9 platforms | approved with changes | Native controller, reporting and portable scenario logic on Linux, macOS and Windows; native library execution wherever a compatible library exists, including a future Windows DLL; Linux-only tools may run in containers; no Windows libpinyin build project now. |
| D10 pacing | approved: option (b) | Family-by-family migration without freezing development; a synchronised ownership handoff and renewed equivalence verification whenever a Python family changes during migration; no Stage 7 work yet. |
| D11 retirements | deferred | No deletion approved; each candidate needs evidence of non-use or replacement, consumer reconciliation and a specific ruling. |

Three architecture corrections, applied in this revision:

1. **Generic ABI code is independent of the pinned oracle.** Dependency
   direction is `libpinyin-abi` ← generic CLI (independent libpinyin use)
   and `libpinyin-abi` ← `pinyin-oracle` (pinned oracle and oxpinyin
   policy). The generic CLI may carry an optional oxpinyin integration, but
   a standalone source build pulls in none of the oxpinyin engine, store or
   datagen crates. Section C.1 was contradictory on this and is corrected.
2. **Extract shared behaviour, not whole modules by file name.**
   `pinyin-oracle`'s `error.rs`, `observation.rs` and the other named files
   mix generic and pin-specific concepts; Stage 1 extracts only genuinely
   generic items, and the existing error variants, fixture formats, FFI
   ownership rules and public re-exports keep working. The new `Subject`
   preserves process isolation for differential runs: opening two
   libraries in one process does not become the default because
   `libloading` makes it possible.
3. **Stage 1 is not a 131-symbol rewrite.** The initial `Subject` exposes a
   checked subset of operations and reports missing or unsupported symbols
   accurately; full ABI coverage arrives through the verified migration
   stages; an arbitrary libpinyin revision is never rejected merely for
   lacking a symbol of oxpinyin's pinned version.

Stage 1 acceptance standard, as ruled: the generic library becomes a
reusable foundation without destabilising the oracle crate — compiling is
not enough; pre-existing consumers, capture readers and tests retain their
behaviour; both facades are verified where supported; the portable pure
modules are verified on all three OSes; the existing Rust fixture tests
pass unchanged.

---

## 1. Findings that decide the design

1. **Three orchestration layers in three languages re-state the same five
   rules** (cell/backend mapping, pin identity, data-directory resolution,
   subject-artifact resolution, verdict semantics). Bash: `oracle-cell.sh`,
   `system-dir.sh`, `cell-artifacts.sh`. Python: `run-all.py`,
   `contract-diff.py`, `gcov_measure.py`. Rust: `pinyin-oracle` (`pin.rs`,
   `pin_ref.rs`, `model_cache.rs`). None of the three is the source of truth
   for all five.
2. **Pin identity is spelled in at least nine places, with two different
   files called `oracle-pin.txt`.** The committed record
   (`tools/oracle/oracle-pin.txt`, schema `oracle-provisioning-pin-v4`:
   commits, base-image digest, apt snapshot, per-cell library and data
   hashes) and the per-prefix manifest `build-oracle.sh` writes (schema
   `pinyin-oracle-v1`: `pin_ref`, `dbm`, header/so/data hashes) use
   different keys for the same facts (`model_archive_sha256` vs
   `model_sha256`; `+dbm-bdb` vs `dbm=BerkeleyDB`). Readers: build-oracle.sh
   constants (declared canonical), oracle-cell.sh and
   nightly-fixture-differentials.sh (sed over build-oracle.sh),
   run-capture.sh, pinyin-oracle `pin_ref.rs`, capi bench support,
   `zhuyin_parse_parity.rs`, run-w8-cycle.sh (hard-coded tkrzw refs),
   contract-diff.py and train-index-diff.py (hard-coded commit substring),
   gcov_measure.py and generate_w3_fixtures.py (recompose or scrape),
   image-tag.sh, cloud/setup.sh, verify-oracle-image.sh, check-reproducible.sh,
   pc-version.sh (record fields), and eight `run-*-diff.sh` with a literal
   `^pin_ref=libpinyin-2.11.92-074a2219…` grep that is not cell-aware.
   Nothing mechanically checks that the record and the build-oracle.sh
   constants agree (ORC notes; #811 audit comment names the analogous gap for
   the datagen manifest).
3. **The reference cell moved to Berkeley DB on 2026-09-27, but the Rust
   harness, the capture goldens and three scripts still pin tkrzw**
   (`EXPECTED_PIN_REF … +dbm-tkrzw` in pinyin-oracle, `EXPECTED_DBM =
   "Tkrzw"`, `PINYIN_BENCH_DBM` relaxation forbidden in CI; run-capture.sh,
   run-w8-cycle.sh, run-differentials.sh). run-all.py's three-cell logic and
   pinyin-oracle's one-cell pin are two different pin policies today.
4. **The backend triple has five spellings** (cell `bdb/kc/tkrzw`, feature
   `bdb/kyotocabinet/tkrzw`, extension `db/kct/tkt`, table.conf
   `BerkeleyDB/KyotoCabinet/Tkrzw`, datagen-manifest `backend=db|kct|tkt`)
   mapped by hand in at least twelve files; several perf and release scripts
   know only two of the three peers (no bdb arm in run-perf-matrix,
   run-perf-same-data, run-shipped-check, run-tree-recipe-control,
   snapshot.sh, libpinyin-drop-in-differential.sh, release-arch.sh).
5. **Verdict semantics differ by layer.** run-all.py: exit 0 PASS / 77 SKIPPED
   / other FAIL, `SKIP:` line promotion, `--allow-skipped` never turns FAIL
   into success. Wrappers: SKIP is 77 in most, 0 in run-live-typing-diff.sh
   (so it can be a CI step), 1 in run-w11-diff.sh for off-pin; headers of
   six wrappers describe an exit code the code no longer uses.
   contract-diff.py and train-index-diff.py assert instead of skipping and
   emit MATCH/DIFFER/SKIP JSON rows with exit 1 on any unexpected row. Rust
   ignored tests panic on a missing input by policy. system-dir.sh and
   oracle-cell.sh exit 3 on fatal.
6. **Every C driver is the same program shape**: dlopen the shared object,
   resolve `pinyin_*`/`zhuyin_*` by name (four spellings of "dlsym or
   exit"), fresh user dir under a hard-coded `/tmp` (11 drivers ignore
   TMPDIR; 4 never remove it), drive a fixed call sequence, print a text
   log; the wrapper runs it twice and diffs. Only run-bisect.sh (the
   right-tail SIGABRT contract), cpp-smoke.cc (static asserts), the probe
   presence greps, union-diff (cursor 9) and the open-counter abort table
   carry hand-written expectations. The helper boilerplate (`rm_rf`,
   `resolve_g_free`, `find_by_text`, candidate-type names, window dumpers,
   the learn/train/export block shared by open-counter, session-fork and
   two-context) is copied, not shared.
7. **contract-diff.py is the deepest asset and the fastest-moving one**:
   3,449 registered cases (2,276 pinyin, 1,173 zhuyin; 1,538 control; 291
   abort-class; 85 crash-class; 1,061 stderr-compared; 37 user-dir
   compared; 14 cell-narrowed; 888 axis-C), defined four ways (decorator,
   shared probe functions, table-driven loops, the axis-C JSON), each case a
   Python function over a ctypes `Kit`; two fresh worker processes per case
   per side; sorted-key JSON equality; `--observations` retains both
   complete observations as JSONL. 105 commits since 2026-10-05. The runbook
   figure "151 of 151" predates the table-driven families and is stale.
8. **pinyin-oracle's live layer is the wrong shape for a two-subject tool
   and the right shape for nothing else**: it links one libpinyin at link
   time (`build.rs` rpath, pin verified before linking), declares 29 of 79
   symbols, serialises contexts process-wide (the pin aborts on two live
   contexts), and `differential::compare` compares an oracle observation
   against oxpinyin-core's `FullPinyinParser`. Its generic parts are the
   observation model, the `pinyin-capture-v1` wire format, the manifest
   parser, the log schemas and the fresh-user-dir discipline.
9. **The measurement instruments are whole-process**: Callgrind `Ir` and
   Massif `mem_heap_B` for `system-import-profile.c` and `bisect --perf`
   include the driver's own work, and the runbook records the driver's source
   and executable SHA-256 next to the figures (benches.md, 2026-10-14
   refresh). A Rust driver cannot reproduce those figures; it can only start
   a new same-run series. contributor guidance "Measure in one run" and benches.md
   "Measure base, PR and pin in one run" (added by PR #819) make
   cross-run comparison invalid anyway, which is what makes a re-baseline
   possible without loss, but only by maintainer decision (E, D4).
10. **Dependencies**: the workspace declares no `clap`, `serde`, `serde_json`,
    `libloading` or `libc` as direct dependencies (all five are in
    `Cargo.lock` transitively through `bindgen` and `criterion`). Rust std
    has no `dlopen`. A dlopen-based generic subject is impossible without a
    direct dependency, which contributor guidance makes an ask (E, D1). The Python layer
    is stdlib-only by stated policy; the shell layer needs gcc, pkg-config,
    glib, nm, diff, docker, valgrind, taskset, and the three backend dump
    utilities (`db5.3_dump`, `kchashmgr`, `tkrzw_dbm_util`; #826).
11. **Nothing in the Python layer is a CI input.** CI runs run-cpp-smoke.sh,
    check-exports.sh, check-alloc-pairing.sh, run-live-typing-diff.sh,
    check-pc-version.sh (ci.yml); backend-matrix.sh and the bdb sanitizer
    script (store-backends.yml); nightly-fixture-differentials.sh and
    perf-gate (verify-nightly.yml); the packaging scripts; the oracle-image
    scripts. run-all.py, contract-diff.py, the ledgers and every perf recipe
    are maintainer-driven local tools. `tools/kc/run-sanitizers.sh` is run by
    nothing.
12. **Upstream precedent** (Appendix F): none of the four projects has a
    scenario-file format with expectations that oxpinyin lacks; mozc's one
    `SessionHandlerInterpreter` behind a gtest, a `--test` CLI and a REPL,
    and its committed status golden checked by a diff test, are the closest
    model for the target; libchewing shows a Rust rewrite with a C ABI that
    kept its legacy C test suite as the compatibility contract and a single
    clap CLI; anthy's three-state verdict table is a borrowable report shape.
    Their self-rewriting goldens, always-zero exits and cwd-relative data are
    the things not to copy.

---

## A. Complete tooling inventory

**Count: 290 records, 287 distinct units** (three units were recorded by two
reading passes: `.github/scripts/lint-commits.sh` TLS-36 ≡ CI-27,
`lint-crate-cfg-placement.sh` TLS-37 ≡ CI-28, `oracle-image.yml` ORC-18 ≡
CI-11; the Cargo/clippy/deny configuration is one bundle record CI-30). The
per-record tables are Appendix A; Appendix A1 is the one-line summary with
the destination column of section D.

| Prefix | Scope | Records | Reading pass covered |
| --- | --- | ---: | --- |
| BIS-A | `tools/bisection/` C/C++ drivers, wrappers, Dockerfiles, data: names a–l | 39 | every file in the letter range, incl. the two orphans key-parse-diff.c and fullpin-tables.h |
| BIS-B | `tools/bisection/` drivers, wrappers, patches, perf/rss/shipped/tree recipes: names m–z | 57 | incl. orphans tie-order-diff.c, system-log-golden.cpp, union-edge-diff.c plant mode |
| BIS-P | `tools/bisection/` Python and the orchestration trio, runners.json, generate_w3_fixtures.py | 29 | run-all.py behaviour spec; contract-diff.py anatomy; full 55-row registry table |
| ORC | `tools/oracle/`, `tools/cloud/`, `tools/model/`, `tools/datagen/`, oracle-image workflow | 18 | PR #819 / #816 dispositions |
| TLS | `tools/abi`, `perf-gate`, `profile`, `capture`, `env-probe`, `store`, `tkrzw`, `kc`, `bdb`, `packaging`, repo lint harnesses, every `[[bench]]` | 49 | PR #767 disposition |
| CI | every job of the seven workflows, the bootstrap action, scripts, hook, gate configuration, dependabot | 31 | policy sentences; platform matrix |
| RS | `crates/pinyin-oracle` (14 modules, 9 bins, 13 tests, 3 benches, build.rs, Cargo.toml), `crates/oxpinyin-testsupport` (4 modules), `fixtures/{foundation,w3,w4,w9,datagen-toned}`, `tests/parity`, `fuzz/` (10 targets, corpus) | 67 | generic/pinned/oxpinyin classification of every public item; F-D grammar; wire formats |

Raw file census of `tools/` (origin/main): 188 files — 86 `.sh`, 42 `.c`, 38
`.py`, 6 `.cc`, 4 `.patch`, 3 `.json`, 3 `.txt`, 1 `.cpp`, 1 `.h`, 2
Dockerfiles, 1 `.md`, 1 pin record; 52,546 lines. `crates/pinyin-oracle` +
`crates/oxpinyin-testsupport`: 13,490 lines. Registry: 55 entries (47
`gate`, 8 `not-a-gate`), all three cells on every entry.

Cross-cutting catalogues (observed, from the records):

- **Environment variables read by `tools/`**: about 107 distinct names (counted
  by grep over `tools/`). Sweep-wide: `PINYIN_ORACLE_PREFIX`,
  `PINYIN_ORACLE_DBM`, `OXPINYIN_SYSTEM_DIR`, `OXPINYIN_CAPI_SO`,
  `OXPINYIN_ZHUYIN_SO`, `OXPINYIN_DICTOOL`, `OXPINYIN_CAPI_BACKEND_EXT`,
  `CARGO_TARGET_DIR`/`OXPINYIN_TARGET_ROOT`, `OXPINYIN_ALLOW_MINI_FIXTURE`,
  `PINYIN_MODEL_DIR`/`PINYIN_MODEL_CACHE`, `PINYIN_EXPORT_DIR`, `TMPDIR`.
  Per-runner aliases of the same three things (system dir, oracle prefix,
  subject .so): 11 system-dir names (`ABI_PROBE_SYSTEM`, `DICT_SYSTEM`,
  `KEY_SYSTEM`, `DYNAMIC_ADJUST_SYSTEM`, `IMPORT_SYSTEM`, `LIVETYPING_SYSTEM`,
  `CAPI_W11_SYSTEM_DIR`, `W13_CAPI_SYSTEM`, `NBEST_CAPI_SYSTEM`,
  `OPTION_SWEEP_CAPI_DATA`, `PHRASE_SYSTEM`, `UNCOVERED_SYSTEM`,
  `RESIDUE_A_SYSTEM`, `RESIDUE_MECH_SYSTEM`, `TRAINDIFF_SYSTEM`,
  `UNION_SYSTEM`, `ZHUYIN_RUST_DATA`), 7 prefix names
  (`LOCALE_DIFF_ORACLE_PREFIX`, `OPEN_COUNTER_ORACLE_PREFIX`,
  `SESSION_FORK_ORACLE_PREFIX`, `TWO_CONTEXT_ORACLE_PREFIX`,
  `MODES_ORACLE_PREFIX`, `ZHUYIN_ORACLE_PREFIX`,
  `PINYIN_PATCHED_ORACLE_PREFIX`), 9 subject-.so names. The Python layer
  uses `CONTRACT_DIFF_CELL` / `J_MEASUREMENT_CELL` for the cell and never
  reads `PINYIN_ORACLE_DBM`.
- **Pin and manifest parsers**: listed in finding 2; the per-file detail is
  in the BIS-A, BIS-B, BIS-P, ORC, TLS and CI architecture notes.
- **Cargo spawn points**: `oracle_cell_artifact` (12 wrappers),
  `cell-artifacts.sh` (three packages + one test executable),
  `system-dir.sh` (`cargo tree --offline` when the backend extension is
  unset), direct `cargo build|cinstall|test|run` in run-perf-baseline,
  run-perf-same-data, run-shipped-check, run-tree-recipe-control,
  run-residue-a-tail-diff, backend-matrix.sh, the two sanitizer scripts,
  nightly-fixture-differentials.sh, run-differentials.sh,
  user-dir-round-trip.sh, libpinyin-drop-in-differential.sh, snapshot.sh,
  install.sh, run-w8-cycle.sh, check-pc-metadata.sh, release-stage.sh.
- **Process control**: process-group kill with TERM→5 s→KILL, `flock` on
  `target/runner-locks/<resource>`, a thread pool of `--jobs` and per-runner
  `TMPDIR` exist only in run-all.py. contract-diff.py has no per-case
  timeout standalone (60 s only under gcov_measure); run-session-fork-diff
  uses `timeout -k 5`; the perf recipes use `taskset -c`; nothing else.
- **Source-tree pollution**: 12 wrappers compile their driver into
  `tools/bisection/` (hence the `bisection-source-output` lock and the
  .gitignore list); the newer wrappers build into mktemp.
  `debuginfo-neutrality-lib.sh` writes fixed-name `/tmp/neut-*` files.
- **Orphans and registry anomalies** (observed): key-parse-diff.c and
  tie-order-diff.c have no wrapper and no registry entry (only
  run-same-data-dir-diff.sh builds them); system-log-golden.cpp is built by
  nothing and needs the pin's internal headers; union-edge-diff.c's `plant`
  mode is dead; the patch sets `nbest-tails-dump` and
  `residue-a-counterfactual` have no registry or oracle-cell entry;
  predict-diff is registered twice (`predict-diff` and `w11-diff
  predict-diff`); the registry passes `SESSION_FORK_ZHUYIN_SO`, which the
  script never reads; `rss-diag` and `system-import-profile` registry
  commands carry no arguments and would hit usage errors if executed;
  pred-order-diff and uncovered-surface-diff are `kind: gate` while their
  headers call exit 2 the expected state; `docker-user-rt.sh` text-patches
  build-oracle.sh with regexes that #818's restructuring likely no longer
  matches; `libpinyin-drop-in-differential.sh` has no bdb arm;
  `tools/kc/run-sanitizers.sh` is run by nothing and diverges from its bdb
  sibling. None of these is changed here; they are inputs to D and E.

### A.3 Completed work, as established functionality (preserve or improve)

**#818 / PR #819 (merged 2026-10-16T08:35Z).** Reproducible libraries:
`SOURCE_DATE_EPOCH` = pin commit time (1788427579), `-ffile-prefix-map` and
`-fdebug-prefix-map` to `/usr/src/libpinyin`, `-Wl,--build-id=none`, no
strip, `debian:testing` by digest with apt snapshot `20260831T000000Z`,
`linux/amd64` only. Identity: the record's six library hashes and three
data-manifest hashes; the image tag = commit[0:12] + base digest[0:12] +
snapshot + sha256(Dockerfile.oracle ‖ build-oracle.sh)[0:12], immutable once
published; `check-reproducible.sh` builds each cell twice in different
work dirs and prefixes and `cmp`s the hash listings and the record;
`verify-oracle-image.sh` checks image contents and the absence of model
data; `cloud/setup.sh` pulls or builds, discards any cell whose hashes
differ, regenerates data per cell with a staged swap only on manifest
match, writes `~/.oxpinyin-oracle.env`, exits 3 on MISMATCH. Rules added to
contributor guidance: "Measure in one run" and "Push, don't hoard" (quoted in Appendix
A, ORC §(c)). **Consequence for consolidation:** any byte change to
`build-oracle.sh` or `Dockerfile.oracle` retags the image; a consolidation
that moves identity logic out of those scripts must re-run
`check-reproducible.sh` and re-record hashes in the same commit, and must
keep the record-vs-manifest key sets.

**#811 / PR #816 (merged 2026-10-16T10:26Z).** Four runners
(train-diff-dynamic-off, union-diff, import-diff, addon-candidate-diff) fed
oxpinyin the mini `fixtures/w3` tables while the pin read full data. The
fix routes them through `resolve_system_dir` after the oracle gate (exit 77
preserved), with the per-runner override and `OXPINYIN_SYSTEM_DIR` as the
only redirections; under run-all.py `OXPINYIN_SYSTEM_DIR` is the pin's own
data, so they become same-directory. **Rule to preserve:** both sides of a
pin differential read one full data directory resolved by one service that
refuses the mini fixture unless asked, and the oracle gate runs first.

**PR #767 (merged 2026-10-14T04:45Z).** `system-import-profile.c` gained
`--sentence-fixture` (500 guesses, options 0x18a, sort 0x1e, call sequence
per row recorded in Appendix A TLS §767), `--train-fixture` (200 rows,
full-consumption check, guess, `pinyin_train(inst, 0)`, one save) and
`--phrase-file` (10,000 imports from the unmeasured `--export-system`, one
save, export of library 1 = 95,698 rows); whole-process Callgrind and Massif
under `taskset -c 0`, three runs, medians, spread, suspend-overlap discard,
no wall clock; stdout hashes `50dfc9c8…`, `e5855249…`, `34e98cd8…` identical
on pin and subject; driver source SHA-256 `565be2c8…` recorded.
**Consequence:** the C driver is the measured instrument; its source is part
of the provenance line of every registered figure.

---

### A.4 Fixture provenance, per fixture (D3)

Recorded from each file's own header or manifest at `e67f19d1`; "authored"
means no oracle produced the bytes.

| Fixture | Schema / shape | Backend provenance | Source |
| --- | --- | --- | --- |
| `fixtures/foundation/f-a.txt`, `f-c.txt` | `pinyin-capture-v1` | **tkrzw** (`+dbm-tkrzw` on every record; 15 and 46 records) | tools/capture/capture.c on the tkrzw prefix; SHA-256s frozen in `docs/testing/capture-fixtures.md` |
| `fixtures/w3/pin-ref.txt` | one `pin_ref=` line | **tkrzw** | the pin prefix the w3 tables were derived from |
| `fixtures/w3/{db,kct,tkt}/` + `datagen-manifest.txt` | `oxpinyin-datagen-manifest-v1`, `pin_ref=model20-…`, `backend=db\|kct\|tkt` | **one directory per backend** (bdb, Kyoto Cabinet, tkrzw) | oxpinyin-datagen over the mini model; `fixtures.sha256` |
| `fixtures/w4/oracle-candidates.txt`, `oracle-candidate-structure.txt`, `oracle-paths.txt`, `oracle-sentence-surface.txt` | `pinyin-oracle-candidates-v1`, `…-candidate-structure-v1`, `…-path-v1`, sentence-surface (`# pin_ref`, `# sample`) | **tkrzw** (`# pin_ref=…+dbm-tkrzw`) | pinyin-oracle bins against the tkrzw prefix |
| `fixtures/w4/oracle-zhuyin-parse.txt` | per-record `pin_ref` | **tkrzw** (26 records) | oracle zhuyin parse capture |
| `fixtures/w4/oracle-sentence-tie-order.txt` | `sentence-tie-order-v1` | **Berkeley DB** (`+dbm-bdb`); the one golden on the reference cell | contract-diff.py `--regenerate-sentence-ties` (bdb-only by design) |
| `fixtures/w4/f-d-session.txt` | `pinyin-session-replay-v1` | authored (no oracle; expectations drawn from f-a.txt candidates) | `docs/findings/session-replay.md`; parsed only by `oxpinyin-engine/tests/session_replay.rs` |
| `fixtures/w4/config-*.txt`, `mini-vocab.txt`, `mini-bigram.txt` | fixture vocab/bigram v1, config layers | authored/derived (vocabulary from f-a/f-c ranks; weights authored); `mini-vocab.txt` header cites the older pin `0c5e80e` | `docs/testing/fixture-adapters.md` |
| `fixtures/w9/*` | segmenter/KMM goldens and FNV manifests | model20 token space, produced by the pin's trainer binaries (no DBM cell; training tools are backend-independent except the bigram-backed gates) | `docs/testing/corpus-pipeline.md` |
| `fixtures/datagen-toned/*` | model `.table` files + `interpolation2.text` | not oracle output (toned mini model) | `fixtures/datagen-toned/README.md` |
| `tests/parity/corpus/*` | parity corpus strata | generated inputs (no oracle output) | `docs/testing/parity-corpus.md` |

The harness therefore becomes cell-parametric (stage 5) and every golden
keeps its bytes and its recorded cell; no golden is re-captured.

## B. Capability and duplication map

Legend: **A** absorb into shared Rust, **I** integrate (specialised
implementation behind the unified interface), **R** retain (instrument or
frozen data), **X** retire after verified replacement. Counts are files.

| Capability | Today (language: files) | Duplication observed | Target home |
| --- | --- | --- | --- |
| Subject loading (dlopen + symbol resolution + ownership) | C: 44 drivers; Python: 3 ctypes loaders (contract-diff `Kit`, compare-nbest `read_unigrams`, train-index `worker`); Rust: `ffi.rs` (link-time, 29 symbols) | 48 independent copies of "load libpinyin and resolve names"; `g_free` pairing re-solved per file | `libpinyin-abi::Subject` (A); C++ compile probes stay (I) |
| Observation model and wire formats | C: text logs (one format per driver); Python: sorted-key JSON, JSONL; Rust: `pinyin-capture-v1`, `pinyin-diff-v1`, `pinyin-divergence-v1` | ≥3 serialisations of candidate windows; `type_name` table ×4 | `libpinyin-abi::Observation` with both existing wire formats kept byte-for-byte (A) |
| Two-subject comparison and verdict | Bash `diff -u` (≈40 wrappers), in-memory capture (w11, same-data-dir), Python `matches()` (plain/abort/crash/either/control), Rust `compare()` (parser-level) | verdict rules in 4 shapes; exit codes 0/77/1/2/3 inconsistent | `libpinyin-abi::Comparator` with the contract rules; run-all's 0/77/other kept at the gate boundary (A) |
| Pin identity and verification | Bash ×7 scripts, Python ×4, Rust `pin.rs`/`build.rs` | nine readers, two schemas, one hard-coded tkrzw ref ×6 | `pinyin-oracle::PinProfile` reading both schemas (A) |
| Backend/cell mapping | oracle-cell.sh, system-dir.sh, gcov_measure.CELLS, run-all.py, contract-diff.py, pin_ref.rs, perf scripts, packaging, snapshot.sh | 12+ hand mappings; several know two of three peers | `pinyin-oracle::Cell` (A) |
| System-data-directory resolution and provenance | system-dir.sh (canonical since 2026-08-28), 13 wrappers bypass it, contract-diff `d18_bigram_db` reads table.conf itself | the #811 class of error | `pinyin-oracle::SystemDir` (A) |
| Subject-artifact build/resolution | oracle-cell.sh `oracle_cell_artifact`, cell-artifacts.sh, 20+ direct cargo calls | retained-target rules in one place, direct calls elsewhere | `pinyin-oracle::Artifacts` (A) |
| Registry and scheduling | runners.json + run-all.py | single, two weeks old; aliases register one driver twice | registry file kept as the one catalog; runner core in Rust (A) |
| Process isolation, timeouts, locks | run-all.py (full), contract-diff.py (workers, no timeout), session-fork (timeout -k) | only the aggregate bounds time | runner core (A) |
| Scenario batteries | capture.c tables; F-D steps (`pinyin-session-replay-v1`); contract cases (Python functions); option sweep (470×11); corpus (10,465 rows); sentence fixture (500 rows) | 3 grammars + imperative cases | keep the three grammars; port cases as Rust functions over `Subject`, one family per PR (A) |
| Oracle provisioning and reproducibility | build-oracle.sh, Dockerfile.oracle, check-reproducible.sh, image-tag.sh, verify-oracle-image.sh, cloud/setup.sh, fetch-model.sh | six-hash compare ×3, docker extraction ×3, proxy flags ×2, data recipe ×5, model identity ×3 | recipe scripts retained (I); identity and verification logic absorbed (A) |
| Rust-test differential tier | run-differentials.sh, nightly-fixture-differentials.sh, testsupport locators, 20+ `#[ignore]` tests | model-cache verification ×3 (bash, bench support, testsupport) | devtool `rust-diff` over testsupport (A); nightly script I until D8 |
| Profiling and measurement | bisect.c `--perf`/rss-diag, system-import-profile.c, 5 perf recipes, 5 Python aggregators, perf-gate (2 parsers + snapshot), profile (w8 cycle + hot stacks), Dockerfiles | loaders duplicated across perf-ci/ratio/decomp and baseline/matrix; `run_one` taskset block ×5; `find_so` ×2; `build_cinstall` ×2 | instruments R; recipes/parsers A; valgrind/perf/taskset external |
| ABI gates | check-exports.sh, check-alloc-pairing.sh + 2 C++ probes, cpp-smoke.cc, frontend-import.cc, `.ver` files | `.ver` parser ×3; fixture staging copied from run-cpp-smoke.sh | exports logic A (nm external); probes I |
| Fuzz, coverage, sanitizers, lint | fuzz/ (10 targets), coverage.yml, 2 sanitizer scripts, Lizard, deny, 2 lint scripts | kc sanitizer orphaned; two nightly dates | I (one entry point, no new framework) |
| Axis-J coverage ledgers | 11 Python files + rules package | records pinned to a measurement tree; `--check` cannot pass on later main (#793) | R temporarily; decision D5 |
| Packaging and release | 9 scripts, release-packages.yml | companion-header check ×2; backend mapping ×3 | I (share `Cell`) |

**Functionality implemented in several languages today**: subject loading
(C, Python, Rust), pin verification (Bash, Python, Rust), backend mapping
(Bash, Python, Rust), model-cache verification (Bash, Rust ×2), user-dir
isolation (C, Python, Rust `FreshUserDir`), candidate-window dumping (C ×6,
Python, Rust), MemoryChunk/logger decoding (Python ×2, Rust data crate).

**Obsolete or dead** (observed; retirement is a decision, not done here):
system-log-golden.cpp, union-edge-diff.c `plant` mode, docker-user-rt.sh
(likely broken), `tools/kc/run-sanitizers.sh` (run by nothing),
fullpin-tables.h and chewing-diff.c's tables (copied at pin 0c5e80e1, one
pin behind), the `/tmp/oxpinyin-export` legacy path, the `pinyin-oracle`
`PINYIN_BENCH_DBM` relaxation once cells are first-class.

---

## C. Proposed unified architecture

> **Superseded architecture.** The units below (`libpinyin-abi`,
> `oxpinyin-devtool`) are not adopted. Amendment 1 places the same
> responsibilities in a Python 3 package driven by `just`, with native C
> instruments/workers retained where measurement fidelity requires them.
> Sections C.1–C.6 remain the historical design record.

### C.1 Units and dependency direction (corrected per ruling)

> **Superseded:** there is no `libpinyin-abi` crate and no
> `oxpinyin-devtool` binary in the amended architecture. See Amendment 1
> sections 3 and 9.

| Unit | Kind | Depends on | Ships | Platform |
| --- | --- | --- | --- | --- |
| `libpinyin-abi` | library crate | std + `libloading` (D1); no oxpinyin crate | internal name now; publication deferred (D2) | parse/compare/report: Linux, macOS, Windows; library execution wherever a compatible libpinyin exists (Linux `.so`, a macOS dylib if built, a future Windows DLL) |
| `oxpinyin-devtool` (generic CLI) | one binary, subcommands | `libpinyin-abi` only in its default build; `serde`/`serde_json` for structured data; `clap` when the CLI lands | never | as `libpinyin-abi` |
| `oxpinyin-devtool`, feature `oxpinyin` (optional integration) | the same binary with the oxpinyin profile enabled | adds `pinyin-oracle` (and through it the oxpinyin crates it already uses) | never | Linux gates; container profile on macOS/Windows |
| `pinyin-oracle` (kept) | library crate | `libpinyin-abi`; oxpinyin-core/data/engine/testsupport as today | never | as today |
| `tools/bisection/runners.json` (D6) | data | read by the devtool; by run-all.py until that stage is separately authorised | — | — |
| external instruments | processes | — | — | Linux |

Dependencies point one way and the standalone edge is enforced
mechanically: `libpinyin-abi` and the devtool's default build know the
libpinyin and libzhuyin C ABIs and nothing about oxpinyin, the pin, cells or
the divergence register (a `cargo tree`-based test in CI, once CI edits are
authorised, and `cargo deny`'s graph check until then). `pinyin-oracle` adds
the pin policy and the oxpinyin-specific comparisons on top of
`libpinyin-abi`. The devtool's oxpinyin integration is the only place that
depends on `pinyin-oracle`, and it is a feature that a standalone source
build never enables.

### C.2 The ten responsibilities

| # | Responsibility | Unit | Design (what it consolidates) |
| --- | --- | --- | --- |
| 1 | Discovery | devtool | the registry file as the single catalog; `list --issue/--row/--cell` as today; `kind` ∈ gate, measurement, provisioning, probe; `implementation: native \| legacy:<path>` during migration so one file answers "what exists" and "what runs it" |
| 2 | Library and ABI selection | libpinyin-abi | `Subject` = dlopen'd object + facade (`pinyin_*` or `zhuyin_*`) + resolved table over all 79/52 names with per-symbol presence (bisect.c's `resolve_all` generalised); identity = path, sha256, SONAME, NEEDED; no pin claim in this layer |
| 3 | Backend, system data, oracle identity | pinyin-oracle | `Cell` owns the five-vocabulary mapping once; `SystemDir` reproduces system-dir.sh's rules verbatim (resolution order, completeness, provenance from datagen-manifest/table.conf, mini-fixture refusal with the same banner, exit 3 texts); `PinProfile` reads both `oracle-pin.txt` schemas, derives `pin_ref` per cell and per patch set, verifies manifests, library hashes and data manifests; `Artifacts` = cell-artifacts.sh (retained per-cell targets, dev opt-level 1, prebuilt overrides never rebuilt, the test-executable symlink) |
| 4 | Scenarios, execution, replay | libpinyin-abi | a scenario is an ordered sequence of ABI steps over a `Subject` with a fresh user dir; the three existing grammars are kept as-is (capture tables, `pinyin-session-replay-v1` steps, contract cases); replay runs a capture fixture's inputs through a `Subject` |
| 5 | Differential observation and comparison | libpinyin-abi + pinyin-oracle | one `Observation` tree (returns, out-params with the UNTOUCHED sentinel, consumed bytes, GLib log records, user-dir listing, stderr bytes, exit/signal); serialised as sorted-key JSON byte-identical to contract-diff rows and as `pinyin-capture-v1` lines; `Comparator` carries the plain/abort/crash/either/control rules (they describe libpinyin behaviour classes), the per-case expected class tables stay in pinyin-oracle |
| 6 | Failure reporting, reproducibility | devtool | run-all's table and `--json` rows unchanged; `--observations` JSONL; per-runner work dir under TMPDIR removed at exit; each row carries subject identities and argv (benches.md already requires this of reports) |
| 7 | Profiling and measurement | devtool `measure` + instruments | Rust owns recipe orchestration, base/PR/pin interleaving, and the parsers (callgrind IR, Massif peak, smaps, perf series thresholds); Callgrind/Massif/perf/taskset external; the two C instruments stay the measured executables (D4) |
| 8 | Fuzz, coverage, correctness | devtool | cargo-fuzz, cargo-llvm-cov, sanitizers external behind one entry point; no new framework |
| 9 | CI and local | devtool | the same subcommands CI will run; each CI edit is a separate ask (D8) |
| 10 | Cross-platform, external tools | devtool `host` | detection of docker/podman, valgrind, nm, the backend dump utilities; a Linux container profile that mounts the checkout and runs the same subcommand inside, for macOS and Windows hosts; never a remote machine |

### C.3 What happens to each named asset

- **`pinyin-oracle`**: split by behaviour, not by file name (correction 2).
  Down to `libpinyin-abi` go only the genuinely generic items: the
  observation types and candidate-type vocabulary of `observation.rs`, the
  `pinyin-capture-v1` reader and writer of `capture.rs` (escape/unescape,
  `CaptureRecord`, `read_fixture`, `segments_to_wire`), the generic error
  variants of `error.rs` (the pin-specific ones stay), the `key=value`
  manifest line parser (not the expected values), the fresh-user-dir
  discipline of `live.rs`, and the FFI ownership rules behind the 29
  declarations, which seed a checked subset of the `Subject` operations
  (correction 3). `pinyin-oracle` keeps re-exporting every public item it
  exports today, so existing consumers, capture readers and tests compile
  and behave unchanged; the extraction is accepted only when the crate's
  own test suite and the fixture readers pass without edits. Stays: `pin_ref.rs`/`pin.rs` expected values (made per-cell),
  `flags.rs`, `taxonomy.rs`, `differential.rs` (the parser-level compare),
  `corpus.rs`, `sentence_tail.rs`, `model_cache.rs`, the nine bins and the
  tests. `live.rs` is re-implemented over `Subject` in a later, separately
  authorised stage; the differential runs keep their process isolation (one
  library per child process, as contract-diff.py and the C wrappers do
  today) — holding two libraries in one process is an explicit opt-in for
  non-differential uses, never the default (correction 2). `build.rs`'s
  link-time check becomes the run-time `PinProfile` check with the same
  strings when that stage is authorised. Pin verification
  remains a precondition of opening the oracle (`VerifiedPin` is still the
  proof obligation). The four cut lines the classification found (Appendix
  A, RS "generic vs pinned vs oxpinyin-specific") are exactly these:
  `Oracle::open*` taking `OraclePrefix` (pin policy as the only argument);
  `OurParse`/`Comparison`/`compare`/`render_our_path` importing
  oxpinyin-core's parser (re-homed behind a subject-parser trait with a
  locally defined comparable-path type); `decide` step 1 and
  `taxonomy::classify_off_pin` reading `EXPECTED_PIN_REF`; and
  `corpus::generate` built on `FULL_PINYIN_SYLLABLES`. Everything that opens
  oxpinyin tables (`sentence_tail.rs`, the real-table bins and benches, the
  decode/graph/sentence-surface tests, the testsupport fixture doubles)
  stays in pinyin-oracle and testsupport by nature.
- **`runners.json`**: the single catalog. Schema kept (`name`, `kind`,
  `issues`, `cells`, `required_inputs`, `timeout`, `exclusive_resources`,
  `command`, `env`, `input_paths`, `required_env`, `optional_skips`,
  `oracle`, `oracle_variant`, `reason`, `timeout_reason`; substitution
  tokens `{root} {prefix} {data} {cell} {capi} {zhuyin} {work} {env[…]}`)
  and extended with `implementation`. run-all.py reads it unchanged until
  retired, so there is never a second catalog. The measurement recipes get
  real argument lists instead of the empty ones that make `rss-diag` and
  `system-import-profile` unrunnable today.
- **`run-all.py`**: absorbed. The Rust runner reproduces the behaviour
  specification in Appendix A BIS-P-02 verbatim: selection and `--issue`
  normalisation, cell config, `--oracle/--data/--ibus-build/--cell-env`
  rules and the protected-env set, the manifest/patch decision table, the
  single serial build per cell, `required_env`/`input_paths` ordering, flock
  on sorted resources, `SKIP:` promotion with `optional_skips`, status and
  `reason_for`, TERM→5 s→KILL on the process group, table and JSON shape,
  exit code with `--allow-skipped`. Equivalence test: both runners over the
  same registry, prefixes and `--no-build` artifacts produce identical
  `--json` rows (`runner, cell, status, pin_ref, reason`; `seconds`
  excluded).
- **`contract-diff.py`**: absorbed family by family (the prefix families in
  Appendix A BIS-P-07 §2). `matches()` is ported once with its five rules.
  Each ported family is accepted only when the Rust port's pin-side and
  subject-side observations equal the Python `--observations` JSONL
  byte-for-byte on all three cells for every case in the family, including
  `~`-keys, `userfiles` listings and normalised stderr. The axis-C battery
  (888 cases, `axis-c-replacement.json` with its source-fixture digests, the
  fork-snapshot protocol and the four-worker stability check) and
  `--regenerate-sentence-ties` are their own steps. The gcov tier (`J-ROOT`,
  `J-GCOV-ABORT-*`) follows D5.
- **Native C drivers**: three classes. (i) Observation drivers (33 `.c` files marked "Absorb (observation
  driver)" in Appendix A1, plus `bisect.c`'s surface mode, `chewing-diff.c`
  and `union-edge-diff.c`) are absorbed as Rust
  scenario functions over `Subject` that print the same text log
  byte-for-byte; the four-way check (C and Rust driver × pin and subject)
  on all three cells is the acceptance test, and the wrapper is retired in
  the same PR. (ii) Instruments (`system-import-profile.c`, `bisect.c`
  `--perf`/rss-diag modes, `plant-oracle-bigram.cc`, `identity-probe.cc`,
  `btree-order.c`, `hash-walk.c`) stay C/C++: whole-process figures and
  backend-native APIs; the devtool compiles and orchestrates them. (iii)
  Compile/linkage probes (`cpp-smoke.cc`, `frontend-import.cc`,
  `alloc-pairing-*.cc`) stay C++ by nature and are integrated.
- **Provisioning**: `build-oracle.sh`, `Dockerfile.oracle`,
  `check-reproducible.sh`, `image-tag.sh`, `verify-oracle-image.sh`,
  `cloud/setup.sh`, `fetch-model.sh` are retained as the recipe (their
  bytes are hashed into the image tag). The devtool adds `oracle
  build|pull|regen-data|verify` that call them and absorbs only the identity
  logic (both schemas, six-hash compare, data-manifest compare, record vs
  constants agreement, which nothing checks today). `nightly-fixture-
  differentials.sh` and `run-differentials.sh` become the `rust-diff`
  subcommand over `oxpinyin-testsupport`'s locators once CI may call it.

### C.4 Data flow of one gate run

registry → select runners × cells → `PinProfile::verify(prefix, variant)`
(FAIL/SKIPPED rows before any build, same decision table) →
`Artifacts::ensure(cell)` or `--no-build` → per runner: `SystemDir`
resolution, `Subject`s (pin, subject; the zhuyin pair for zhuyin
facades), flock on exclusive resources, the scenario in two isolated child
processes (fresh user dir, `RLIMIT_CORE` 0, `PR_SET_DUMPABLE` 0 for
deliberate faults, `TMPDIR` = work dir) under the registry timeout with
process-group kill → `Observation`s → `Comparator` → row → table/JSON.

### C.5 Standalone profile for libpinyin developers

`libpinyin-abi` plus the devtool's default build (`diff`, `capture`,
`replay` and `scenario` subcommands) with no pin and no oxpinyin crate in
the build graph: subjects are any two shared objects, or one shared object
against a committed expectation file. Success is "observations equal" or
"observations match the expectation file"; there is no oracle-pin check, no
cell check and no divergence register in this profile, and a subject that
lacks a symbol of oxpinyin's pinned revision is reported as such, not
rejected (correction 3). libzhuyin is the same machinery with the `zhuyin_*`
facade table. oxpinyin's pin, cells and register are a verification profile
layered on in `pinyin-oracle` and enabled by the devtool's optional feature;
the standalone tool never embeds them in the meaning of success. There is no
cross-IME abstraction: the subject type is libpinyin's ABI, and the
comparator's behaviour classes (abort, crash) are libpinyin's.

### C.6 Platform boundaries

Portable (Linux, macOS, Windows): the controller, registry and discovery,
scenario parsing, capture/JSON parsing and comparison, report rendering,
perf-series parsing, replay against committed fixtures, the `host`
detection, and native library execution wherever a compatible library
exists (a Linux `.so` today; a macOS dylib or a Windows DLL when one is
built — no Windows libpinyin build project is added now, D9). Linux-only:
glib-backed subjects as built today, the three backend C libraries,
valgrind, `/proc`, `taskset`, docker-built oracles, the backend dump
utilities. macOS/Windows hosts run
the Linux parts inside the pinned container through the devtool's container
profile; `check-exports` already handles macOS `nm` for local runs.

---

## D. Migration roadmap

> **Superseded roadmap.** Amendment 1 section 10 replaces Stages 1–13 with a
> Python + `just` sequence (Stages 1–14). No stage below is authorised.

Authorisation state (ruling of 2026-10-16 UTC): Stage 0 authorised and
landed by this document; Stage 1 authorised after Stage 0 is accepted,
within section 0's corrections; Stages 2–13 not authorised and each needs
its own review; no retirement authorised (D11). The stage table is the plan,
not a licence.

Rules the whole sequence obeys (from contributor guidance and the runbooks, quoted in
Appendix A CI "Policy sentences"): verification cells (reference cell bdb,
three cells only when the path check says backend code changed, plus
start-of-sweep and pre-release); runner protocol 0/77/other and "Allowances
never turn a failure into success"; tests never self-skip; differentials
build debug at `CARGO_PROFILE_DEV_OPT_LEVEL=1` with one retained target per
cell; "Measure in one run"; goldens, pins and CI policy change only after an
ask; each PR is green standing alone.

Coverage inventory: every capability in B has a stage below that replaces
it and a retire condition; a capability with no stage is retained. The
`implementation` field in the registry is the running ledger: a tool is
`legacy:<path>` until its stage lands, `native` after, and the old file is
deleted only in the PR whose acceptance test proved equivalence.

| Stage | PR | Files and functionality affected | Shared implementation introduced | Interfaces / dependencies | Legacy retained | Equivalence test | Platform verification | Rollback | Delete condition |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | docs | this report and Appendix A under `docs/findings/`; registry anomalies filed as issues | none | none | all | n/a | fmt only | revert | n/a |
| 1 | `libpinyin-abi` foundation (authorised after Stage 0 is accepted) | genuinely generic behaviour extracted from `pinyin-oracle` (observation types, `pinyin-capture-v1` reader/writer, generic error variants, manifest line parser, fresh-user-dir discipline, FFI ownership rules); a narrowly scoped dynamic `Subject` over `libloading` exposing a checked subset of operations with accurate missing-symbol reporting; both facades where supported | the standalone crate; `oxpinyin-devtool` skeleton only if needed for tests | **`libloading` as a direct dependency (D1, approved)**; `serde`/`serde_json` only where structured data needs them and never for the byte-exact legacy formats; pinyin-oracle re-exports every public item it exports today (no API break); no two-library-per-process default | everything in tools/; run-all.py, contract-diff.py and every C driver untouched | pinyin-oracle's test suite passes unchanged; `read_fixture` over `fixtures/foundation/*.txt` and the w4 goldens parses identically; existing capture readers and consumers unchanged; a standalone build of the crate and the devtool default build shows no oxpinyin crate in `cargo tree` | portable pure modules tested on Linux, macOS and Windows CI lanes; `Subject` live tests on Linux (both facades); `cargo test -p` for both crates, fmt, clippy | revert the crate | none deleted |
| 2 | capture | `tools/capture/capture.c`, `run-capture.sh` | `devtool capture` (F-A/F-C tables as data, `pinyin-capture-v1` writer) | none | capture.c until proven | byte-identical output to `fixtures/foundation/f-a.txt`, `f-c.txt` (their SHA-256s are frozen) and to a fresh capture.c run on the tkrzw prefix | Linux, tkrzw cell (the goldens' cell, D3) | re-point the runbook | capture.c and run-capture.sh deleted when the two files reproduce |
| 3 | runner core | `run-all.py`, `run-all.sh`; registry gains `implementation` | `devtool run` (BIS-P-02 spec) | reads the same registry; emits the same table/JSON | run-all.py kept; run-all.sh still execs Python | both runners, same inputs, `--no-build`, full gate set, bdb; identical JSON rows minus `seconds`; once on all three cells | Linux; the parse/select logic unit-tested on all OSes | run-all.sh keeps exec'ing Python | run-all.py deleted after one three-cell sweep agrees and the runbook moves |
| 4 | cell, data-dir, artifact services | `oracle-cell.sh`, `system-dir.sh`, `cell-artifacts.sh`; the 13 wrappers that bypass oracle-cell.sh | `pinyin-oracle::{Cell, SystemDir, PinProfile, Artifacts}`; `devtool describe --cell` prints the same JSON as `cell-artifacts.sh --describe` | the Bash trio becomes a thin shim that sources an env file the devtool writes (the existing `--describe` handoff reversed) | the three scripts as shims | byte-equal `--describe` JSON and env dumps for a matrix of inputs incl. every exit-3 message; record-vs-constants check added as a new test | Linux | shims call the old bodies | shims deleted with the last wrapper (stage 6) |
| 5 | pin profile unification | `pin_ref.rs`, `build.rs`, `run-capture.sh`, the eight literal greps, `PINYIN_BENCH_DBM` | `PinProfile` per cell replaces the hard-coded tkrzw ref; link-time check → run-time check | pinyin-oracle tests select a cell explicitly | none | pinyin-oracle's live tests pass on the tkrzw prefix exactly as before; the wrappers' pin checks become cell-aware (a kc oracle on a bdb cell is now FAIL where today it passes the literal grep) | Linux, three prefixes | keep `EXPECTED_PIN_REF` constant alongside | the literal greps go with their wrappers |
| 6 | observation drivers, in families | the 36 absorbed drivers and their 35 wrappers, grouped: (a) the same-data-dir set (key-surface, key-parse, tie-order, dict-surface, phrase-surface, pred-order, predict, punct, addon-candidate, user-candidate, union, import, live-typing, nbest-train, dynamic-adjust); (b) user-store set (open-counter, session-fork, two-context, user-file-modes); (c) scheme set (chewing, fullpin, option-sweep, zhuyin, zhuyin-import); (d) export/import set (export, import-semantics, bigram-export, residue-a, residue-mechanism, train-diff, candidate-assembly, uncovered-surface, locale, abi-probe, bisect surface mode) | Rust scenario functions over `Subject` printing the identical log; shared helpers replace the copied `rm_rf`/`resolve_g_free`/`find_by_text`/window dumpers | registry rows flip to `native` one by one | C file and wrapper until the four-way check passes | for each driver: C log == Rust log on pin and on subject, three cells, diff empty; for drivers with hand-written expectations (union cursor 9, open-counter abort table, right-tail case) the expectation moves as a test and is exercised | Linux, three cells per family PR (backend code is touched only indirectly, but the drivers are backend-sensitive: run three cells, state the path check) | registry row back to `legacy` | C file, wrapper, gitignore entry deleted in the same PR |
| 7 | contract families | `contract-diff.py` families (BIS-P-07 §2), `run-contract-diff.sh` | `Comparator` (five rules) + per-family Rust cases; `--observations` JSONL writer | registry `contract-diff` row grows a `--families` selector during migration | Python kept for unported families; one `cases` list, split by implementation | per family, all cells: Rust observations == Python JSONL for pin and subject; verdict rows identical | Linux, three cells | family back to Python | contract-diff.py deleted after the last family and one full-suite three-cell run; `--expect-parent` semantics kept |
| 8 | axis-C battery and tie regeneration | `axis-c-replacement.json`, `--axis-c-*`, `--regenerate-sentence-ties`, `fixtures/w4/oracle-sentence-tie-order.txt` | `devtool battery axis-c`, `devtool regen sentence-ties` | none | Python until equal | identical `difference_signature` lines, counts and stability verdicts; regenerated fixture byte-identical (frozen) | Linux, bdb (regeneration is bdb-only by design) | Python path | after one identical run per cell |
| 9 | option sweep and Rust-test tier | `transformed-options-sweep.py` and inputs; `run-differentials.sh`; `user-dir-round-trip.sh`; `libpinyin-drop-in-differential.sh` | `devtool battery options`; `devtool rust-diff` over testsupport locators | none | scripts until equal | sweep: identical per-scheme mismatch counts and JSON; rust-diff: same `cargo test --include-ignored` set, same pass/fail; drop-in: gains the missing bdb arm | Linux | scripts | scripts deleted; the nightly workflow switch is stage 12 |
| 10 | measurement orchestration and parsers | 5 perf recipes, `run-callgrind-differential.sh`, `run-system-import-profile.sh`, `run-rss-diag.sh`, perf-gate (`callgrind-ir.py`, `series.py`, `snapshot.sh`), profile scripts, 5 Python aggregators | `devtool measure <recipe>` with interleaved base/PR/pin and the parsers; the two C instruments compiled from their unchanged sources (SHA-256 asserted against the runbook) | none | Python parsers until golden-equal | parsers: identical output on retained callgrind/massif/smaps/jsonl inputs (the `.test.sh` fixtures become Rust tests); recipes: same commands issued (dry-run transcript diff) | Linux; parsers on all OSes | scripts | parsers and recipes deleted; instruments stay |
| 11 | ABI and packaging seams | `check-exports.sh` logic, `.ver` parser ×3, cell mapping in packaging, model-cache verification ×3 | `devtool abi exports` (nm external); `Cell` used by packaging | none | wrappers | identical symbol-set verdicts on the dev and shipped builds; identical staged trees | Linux (macOS local nm path kept) | wrappers | wrappers deleted; CI switch is stage 12 |
| 12 | CI switch-over (each line its own ask, D8) | ci.yml steps (cpp-smoke, exports, alloc, live-typing, pc-version), store-backends.yml (backend-matrix, sanitizers), verify-nightly.yml (fixture differentials, perf-gate), oracle-image.yml | the devtool subcommands | workflow edits | old steps until the new ones are green | both steps run side by side for one cycle; identical conclusions | GitHub runners | revert the step | old script deleted after one green cycle |
| 13 | retirements needing a ruling | `system-log-golden.cpp`, `docker-user-rt.sh`, `tools/kc/run-sanitizers.sh`, `fullpin-tables.h`, dead `plant` mode, duplicate registry aliases, axis-J ledgers (D5) | none | none | — | evidence in A (built by nothing; run by nothing; stale pin) | — | — | on the maintainer's word |

Behaviours preserved across every stage (checked by the equivalence tests
named above): PASS/SKIPPED/FAIL semantics and exit codes at the gate
boundary; oracle pin, patch and cell checks before any build; model and data
provenance (SHA-verified model, data manifests, backend provenance of the
data directory); user-state isolation (fresh dirs per side, removed on
exit) and worker isolation (two processes per case, `RLIMIT_CORE`,
`PR_SET_DUMPABLE`); ABI outputs, candidate ordering and pointer ownership
(every `gchar*` out-param freed exactly where the Python/C code frees it;
borrowed strings never freed; `GArray`s allocated by the driver);
registry timeouts, process-group cleanup and `exclusive_resources` locks;
scenario and fixture formats byte-for-byte; the #767 workloads and the
one-run measurement policy. No golden, scoring rule or accepted divergence
changes in any stage; where a stage would change one (D3), it is listed as a
decision, not done.

---

## E. Decisions — alternatives, evidence and the recommendation put to the maintainer

> **Historical recommendations.** Amendment 1 section 9 replaces D1–D11
> (and adds D12–D13). The text below is kept as the record of what was asked.

Each item states the alternatives, the evidence, and the recommendation as
submitted. **The rulings are in section 0 and supersede the
recommendations below where they differ** (D1: `serde`/`serde_json`
permitted, hand-written JSON as a general policy rejected; D2: the generic
CLI must not depend on `pinyin-oracle`; D5 and D11: no retirement
authorised; D9: native execution wherever a compatible library exists).
The text is kept as the record of what was asked.

**D1. Direct dependencies for the Rust toolchain.** A dlopen-based
`Subject` needs `libloading` (safe wrapper; already in `Cargo.lock` via
bindgen) or `libc` (raw `dlopen`/`dlsym` behind `unsafe`; also already in
the lock). Argument parsing and JSON can stay std-only (the Python layer is
stdlib-only by policy and the existing wire formats are hand-written
key=value and TSV), or use `clap` and `serde_json` (both already in the lock
via bindgen/criterion; deny.toml's licence set covers them). Evidence:
Cargo.lock lines for `clap`, `libloading`, `serde`, `serde_json`, `libc`; no
direct declaration anywhere. Recommendation: `libloading` for the standalone
crate (it is what makes the crate usable outside oxpinyin); std-only
argument parsing and a hand-written JSON writer/reader for the two existing
wire shapes, because every format the tool must emit byte-for-byte is
already specified by hand and `serde_json` would add a second formatter to
keep identical. Revisit `clap` only if the subcommand surface outgrows what
the Python `argparse` surfaces are today.

**D2. Names and homes.** `libpinyin-abi` as a workspace member that never
depends on an oxpinyin crate (CI's existing `cargo deny` and a workspace
test can enforce the edge), published later or not; `oxpinyin-devtool` as a
non-shipping member. Alternative: a separate repository for the standalone
crate from day one. Recommendation: in-workspace until the standalone
profile has one external user; moving a crate out later is cheap, keeping
two repositories in step during migration is not.

**D3. Reference cell for the Rust harness and the goldens.** Today:
reference cell bdb for gates; `fixtures/foundation/*.txt`, `fixtures/w3`,
`fixtures/w4/*` and pinyin-oracle's expected pin are tkrzw. Options: (a)
keep tkrzw goldens and make the Rust harness cell-parametric (stage 5),
re-capturing nothing; (b) re-capture the goldens on bdb (a golden change: a
STOP). Recommendation: (a); the goldens are records of the pin on one cell
and stay valid; the harness merely stops hard-coding that cell.

**D4. Native instruments.** `system-import-profile.c` and `bisect.c`'s
perf/rss modes are the measured executables of every registered figure
(their source SHA-256s are in the runbook). Options: (a) retain them
permanently as C, orchestrated by Rust (recommended; verification rule 6 and
the PR #767 bar both point here); (b) write Rust instruments and start a
new same-run series, declaring the old figures historical. (b) is
permitted by "Measure in one run" but discards the only cross-PR instrument
continuity the project has; it should be taken, if at all, as its own
measurement PR with the equivalence evidence in Appendix A TLS §767.

**D5. Axis-J ledgers and the gcov tier** (11 Python files, 3,800 lines,
#793: `--check` cannot pass on later main; the oracle digest is host-bound).
Options: (a) freeze as records pinned to their measurement tree and retire
the scripts once the ledger documents state which check is expected to pass;
(b) port to Rust (no equivalence oracle exists, since the guard fails on
current main). Recommendation: (a), decided together with #793.

**D6. Registry.** Keep `runners.json` as the one catalog read by Rust
(recommended; zero-risk for stage 3's equivalence test), moved to
`tools/registry.json` only when the Python reader is gone; or encode the
registry in Rust and generate the JSON for Python during migration (two
sources of one truth, rejected by the brief).

**D7. Standalone verdict semantics.** The 0/77/other protocol is oxpinyin's
gate contract. For the standalone profile: keep the same codes (recommended:
one contract, documented once) or use plain 0/1. Either way the runner core
preserves the aggregate's semantics unchanged.

**D8. CI switch-over.** Every workflow change is an ask. Proposed order:
ci.yml ABI steps and cpp-smoke (stage 11 outputs), then store-backends
scripts, then verify-nightly (fixture differentials, perf-gate), then
oracle-image (only if its scripts move, which this plan avoids).

**D9. Platform scope.** Confirm that "cross-platform" means: orchestration,
replay and reporting native on Linux/macOS/Windows; driving a libpinyin
shared object on Linux (and macOS only if someone builds libpinyin there);
macOS/Windows gates through the pinned container. No Windows libpinyin
build exists in this project's recipes, and none is proposed.

**D10. Pacing against contract-diff.py's churn.** The file gained 105
commits in twelve days and is the active instrument of open lanes. Options:
(a) freeze the file during stage 7 (blocks lanes); (b) migrate family by
family with the JSONL equivalence gate per PR and a rule that new cases land
in Rust once their family is native (recommended); (c) defer stage 7 until
the audit lanes close. The brief's "no two permanent implementations" is
met by (b) because the registry's `implementation` field names which side
owns each family at every commit.

**D11. Retirement rulings** (stage 13): the items in B "Obsolete or dead"
are observations; each deletion needs the maintainer's word.

---

## F. Upstream projects — what was borrowed and what was not

Full survey with revision, directory list and observation/interpretation
labels per row: Appendix F. Summary:

- **libpinyin (074a2219)**: no scenario format, no stored goldens; tests are
  stdin REPL probes with `assert`/`check_result` (the latter a no-op under
  `NDEBUG`), cwd-relative data and `/tmp` paths. Borrowed: the
  `\data`/`\item` tag grammar and the `token phrase` + `0 ` sentinel corpus
  line as the training interchange the Rust-test tier already speaks;
  `eval_correction_rate`'s sentence-exact-match metric. Not copied: vanishing
  asserts, unpinned `wget` of model data, REPL probes as the test interface.
- **libchewing (3c4a93aa)**: a Rust rewrite with a C ABI that kept the
  legacy C test suite, a symbol-version map and a struct-size test as the
  compatibility contract (no oracle against the old C library); one clap CLI
  `chewing-cli {init-database,info,dump}` with generated man pages; a
  keystroke mini-grammar (`<E>`, `<B>`, `<C1>`) shared by tests, filter,
  recorder, reducer and the fuzzer's `GEN` output. Borrowed: the single-CLI
  shape, the fuzzer-to-reproducer idea. Not copied: the shared user-dictionary
  path that forces serial tests.
- **mozc (0136c357)**: 83 TSV scenario files run by one
  `SessionHandlerInterpreter` behind a gtest, a `--test` CLI and a REPL; a
  7-column quality TSV with a committed `evaluation.tsv` status golden
  checked by a Bazel `diff_test`. Borrowed: one interpreter with several
  front-ends (C.2 §4), the status-golden-plus-diff gate as the model for the
  F-D and contract expectation files, the "platform" column mapping to the
  DBM cell. Not copied: the hand-maintained scenario file list, regenerating
  goldens by hand-copying build output, a threshold gate that exists only as
  a library.
- **anthy-unicode (fc2260cf)**: a curated accuracy driver with a
  three-state verdict table (`OK`/`X`/`*`) and `|seg|seg|` notation with
  `~`/`!` miss marks. Borrowed: the three-state verdict for a future curated
  accuracy report, the run-environment banner. Not copied: tests that always
  exit 0 and rewrite their own expectation file.

None of the four treats two IMEs as behaviourally comparable, and neither
does this design.

---

## G. Observations recorded in passing (not acted on)

Documentation and code drift the reading passes noticed, for the
maintainer's backlog, unchanged here: `docs/runbooks/backends.md` still
calls tkrzw the default; `run-differentials.sh`'s header describes a
tarball recipe and a Tkrzw default; `cloud.md` says no image is published
yet; six wrapper headers describe skip exit codes the code no longer uses;
`ci-strategy.md` lists a removed `fuzz` job; `tooling-evaluation.md` §12
says Lizard is never a PR blocker (it is); the runbook's "151 of 151"
contract-case count is stale against the 2,561-case default run; #826's
three provisioning gaps (dump utilities, a sound `libtkrzw` on Ubuntu,
podman) sit on the standalone tool's `host` detection path. From the Rust
pass: the F-D grammar (`pinyin-session-replay-v1`) is parsed only by
`crates/oxpinyin-engine/tests/session_replay.rs`, not by pinyin-oracle, so
the "scenario" responsibility has no shared reader today; two residual
self-skips survive the no-self-skip rule (`model_fetch::
pinned_archive_extracts_and_second_run_skips`,
`real_tables_integration::load_w2_corpus`); the real-table open sequence is
copied six times across bins and benches, the candidate-fixture loader four
times, the corpus loader six times, and there are five TAB `key=value`
splitters and three segment-notation parsers; `sentence_tail.rs`'s bin doc
and `goldens-and-pins.md` disagree with the test's 500/500/500 counts;
`corpus_tail.rs` and `mini-vocab.txt` still cite pin `0c5e80e`;
`oracle-sentence-tie-order.txt` is the one golden pinned to `dbm-bdb`.

---

*Phase 1 ends here as a historical record. Its architecture, decisions and
stages are amended by [`tooling-consolidation-python-just.md`](tooling-consolidation-python-just.md);
Stage 0 of the amended plan covers documentation only. New Stage 1 retains
its separate authorisation. New Stage 2 and later stages require the review
and explicit maintainer approval described there. Every retirement still needs
its own ruling.*
