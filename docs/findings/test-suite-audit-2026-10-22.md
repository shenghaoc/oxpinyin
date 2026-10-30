# Test-suite audit, Phase 1

> **W2 retirement update.** `pinyin-oracle`'s `differential_replay`,
> `taxonomy_replay` and `differential_live` suites and their comparator have
> been retired. The measurements below describe the audited revision. The
> unique live sentinel assertions now live in `tests/graph_paths.rs`.

Date: 2026-10-22 (UTC) · Issue: #1005 · Status: **audit, read-only. No code,
test, workflow or `.config/nextest.toml` change. Each implementation PR is
its own decision (§9).**

Goal (from #1005): lower verification cost with equal or stronger defect
detection. Fewer tests is not a goal. This document has no reduction
target.

## 1. Baseline and host

- Baseline: `origin/main` at `5decad775be9f76e070351e2380b0ca2b8879eb5`.
- Host: Linux 7.3.0-9-generic, Ubuntu 26.10, x86_64, 12 logical CPUs,
  15 GiB RAM. `rust-toolchain.toml` toolchain (rustc 1.97.1).
  cargo-nextest 0.9.143, the version and checksum `ci.yml` installs.
- Feature set: the CI Linux `test` job's selection, `--workspace` with
  default features (Berkeley DB). Other cells were not measured (§6).
- One run, one host (contributor guidance "Measure in one run"). All figures below
  come from the commands in §2, run on 2026-10-22 between 05:24Z and
  06:19Z. The host was shared with other agent sessions doing cell builds
  and differentials. Absolute seconds are therefore noisy. The comparisons
  this audit relies on are ratios taken minutes apart in the same job.
- Admission: the clean build ran under `just heavy`, which chose
  `CARGO_BUILD_JOBS=3` from available memory. A second `just heavy`
  admission expired after 1,800 s (`resource-blocked: 600 MiB needed,
  0 MiB available`). At the maintainer's instruction, the remaining steps
  ran directly with `CARGO_BUILD_JOBS=3` set explicitly, so build figures
  stay comparable with the clean build.
- CPU-time re-measurement (C12–C14), after review of the first revision:
  `origin/main` at `52753e48a63badb6a3bf9b6e15d758b6d1273fb1`, same
  host, 2026-10-22 between 08:44Z and 08:56Z, in two back-to-back
  `just heavy` admissions with `CARGO_BUILD_JOBS=3` explicit. Every CPU figure and
  every cargo test vs nextest work comparison in this document comes
  from that run. Its workspace listing has 1,561 tests, three more than
  C3. The inventory in §3 stays at the first baseline.

## 2. Commands

Raw evidence was not retained. The nextest JSON listings, the `$R` and
`$R2` per-process logs, the GNU time output, the strace traces and the
cargo-timings data were written to an ephemeral scratch directory and
are not attached anywhere. Each figure in this document can be
reproduced with the command recorded for it below.

Run from the repository root of a worktree at the baseline. `$R` is a
target runner that wraps each test-binary process in GNU time:
`exec /usr/bin/time -a -o "$AUDIT_RUNLOG" -f "%e\t%M\t%x\t%C" "$@"`. It is
passed as `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=$R` and gives
wall seconds, peak RSS and exit status per process. Both cargo test and
nextest honour it. `$T` is `/usr/bin/time -f 'WALL=%e MAXRSS_KB=%M'`.
`$R2` is the same runner with `-f "%e\t%U\t%S\t%M\t%x\t%C"`, and `$T2`
is `/usr/bin/time -f 'WALL=%e USER=%U SYS=%S MAXRSS_KB=%M'`. CPU time
is user plus system seconds from the rusage of the wrapped process and
the children it waits for. Summed per-process wall time is not used as
a work measure: concurrent processes overlap and wait on one another.

| ID | Command | Gives |
|---|---|---|
| C1 | `just heavy -- $T cargo test --locked --workspace --no-run --timings` (empty `target/`) | clean build wall; per-unit durations from the `UNIT_DATA` table in `target/cargo-timings/cargo-timing-*.html` |
| C2 | `cargo metadata --locked --no-deps --format-version 1` | targets, kinds, `required-features` |
| C3 | `cargo nextest list --locked --workspace --message-format json` and the same with `--run-ignored all` | binaries, test names, ignored flags, binary paths |
| C4 | `$T cargo test --locked --workspace --lib --bins --tests --no-fail-fast -- --test-threads 4`, with `$R` | cargo test wall; per-binary process wall and peak RSS |
| C5 | `$T cargo nextest run --locked --profile ci --status-level skip --workspace --no-fail-fast`, with `$R` | nextest wall (4 threads, CI profile); per-test process wall and peak RSS |
| C6 | C4 and C5 again without `$R` | wall without wrapper overhead |
| C7 | `$T cargo test --locked --workspace --doc` | doctest wall |
| C8 | `strace -f -qq -e trace=execve -e signal=none -o <file>` around C4 and C5; count lines matching `execve(` that end `= 0` | processes started |
| C9 | `$T cargo test --locked --workspace --no-run` after: nothing; `touch crates/oxpinyin-core/src/lib.rs` (with `--timings`); `touch crates/oxpinyin-core/tests/scheme_parsers.rs`; `touch crates/*/tests/*.rs` (with `--timings`) | incremental build wall and per-unit durations |
| C10 | §5 grouping experiment, base and grouped, each in a fresh `CARGO_TARGET_DIR`: `$T cargo test --locked -p oxpinyin-core --no-run --timings`; then the two touches of C9 restricted to `-p oxpinyin-core`; then three repetitions each of `cargo test --locked -p oxpinyin-core --tests --no-fail-fast -- --test-threads 4` and `cargo nextest run --locked --profile ci -p oxpinyin-core --no-fail-fast`; C8 around one nextest run | grouping cost and benefit |
| C11 | `PINYIN_ORACLE_PREFIX=$HOME/.cache/oxpinyin/oracle/tkrzw`, `PKG_CONFIG_PATH`/`LIBRARY_PATH` at `$HOME/.cache/oxpinyin/tkrzw/lib`, `LD_LIBRARY_PATH=$HOME/.cache/oxpinyin/tkrzw/lib:$PINYIN_ORACLE_PREFIX/lib`; `F='--no-default-features --features oracle-ffi,tkrzw'`; `cargo test --locked -p pinyin-oracle $F --no-run`; then C4 and C5 restricted to `-p pinyin-oracle $F --test differential_live --test live_smoke --test export_seam`, with `$R` | oracle-ffi costs under both harnesses |
| C12 | All four with `$T2` and `$R2`, after `cargo test --locked --workspace --no-run`: `cargo test --locked --workspace --lib --bins --tests --no-fail-fast -- --test-threads 4`; the same with `--test-threads 1`; `cargo nextest run --locked --profile ci --status-level skip --workspace --no-fail-fast`; the same with `--test-threads 1` | CPU per test process and for the whole command, at four threads and serially |
| C13 | C11's environment and `$F`, with `$T2` and `$R2`: `cargo test --locked -p pinyin-oracle $F --test differential_live --test live_smoke --test export_seam --no-fail-fast -- --test-threads 4`; the same with `--test-threads 1`; `cargo nextest run --locked --profile ci -p pinyin-oracle $F` with the same three `--test` and `--no-fail-fast`; the same with `--test-threads 1` | oracle-ffi CPU under both harnesses |
| C14 | C11's environment without `$F`: `$T2 cargo test --locked -p pinyin-oracle --features oracle-ffi` | the documented `oracle-ffi` invocation with the default (bdb) Rust features |

C12–C14 ran under `CARGO_BUILD_JOBS=3 just heavy -- bash <script>`, in
two back-to-back admissions. The two `--test-threads 1` cargo test runs
were in the second.

C11 notes. The `oracle-ffi` build refuses the Berkeley DB oracle prefix:
its `build.rs` accepts only the `dbm-tkrzw` `pin_ref`. The Rust backend
feature is a separate choice. C14 built `pinyin-oracle` with its default
(bdb) features plus `oracle-ffi` against the tkrzw oracle prefix and
passed: 128 passed, 0 failed, 6 ignored. `--no-default-features
--features oracle-ffi,tkrzw` is only the choice C11 and C13 made for
their cell. The tkrzw library is
the one `tools/cloud/setup.sh` builds into `~/.cache/oxpinyin/tkrzw` and
probes before writing its `REV`. In C11, C13 and C14, `ldd` showed
`differential_live`, `live_smoke` and `export_seam` resolving `libtkrzw.so.1` and
`libpinyin.so.15` from those prefixes, not from `/usr/lib` (contributor guidance
Verification cells, rule 9). C11 is a cost measurement. It is not a
tkrzw cell result, and no DIFFER is reported from it. All 21 tests
passed under both harnesses.

Static facts (OnceLock users, fixtures, subprocesses, ignore reasons,
invokers) come from reading the files named in each row, and from
`grep` over `crates/*/tests`, `crates/*/src`, `tools/`, `.github/`,
`justfile` and `docs/`.

## 3. Inventory (task 1)

### 3.1 Headline counts (C2, C3)

| Item | Count |
|---|---|
| Workspace crates | 20 |
| Test binaries nextest builds (C3) | 109: 19 lib unit-test, 66 integration, 24 bin |
| Binaries that list zero tests in this cell | 28: 21 bins, the `oxpinyin-capi-marshal` lib, and 6 cfg-gated integration binaries (§3.3) |
| Tests listed (C3) | 1,558: 1,176 lib unit, 365 integration, 17 bin |
| Of which `#[ignore]` | 32 listed (37 in source; §7) |
| Tests run by C5 | 1,526 passed, 32 skipped |
| Doctests (C7) | 4 run, 2 ignored, across 19 doc-test crates |
| `fuzz/` targets | 10, in a separate workspace, `test = false`. Nothing replays `fuzz/corpus` as a regular test |

### 3.2 Per crate (C1, C3, C4, C5, C12)

"Build" is the sum of cargo-timings unit durations for the crate's test
units in C1 (compile and link, measured at `-j3`). "Incr" is the same for
C9's `touch crates/*/tests/*.rs` step. "CPU s" is the summed CPU time of
the crate's test-binary processes in C12 at four threads (cargo test
`--test-threads 4`, nextest `--profile ci`). The serial figures are in
§4.1. Process counts and peak RSS are from C4 and C5. Peak RSS is the
largest single test process. Rows are rounded independently.

| Crate | Bins | Integ. | Empty | Tests | Ign. | Size MiB | Build s | Incr s | cargo test CPU s | nextest CPU s | cargo procs | nextest procs | Peak RSS MiB (cargo / nextest) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| oxpinyin-capi | 5 | 4 | 0 | 124 | 0 | 272 | 10.9 | 2.0 | 1.6 | 1.1 | 5 | 134 | 16 / 13 |
| oxpinyin-capi-marshal | 2 | 1 | 1 | 14 | 0 | 13 | 0.5 | 0.2 | 0.0 | 0.0 | 2 | 18 | 3 / 3 |
| oxpinyin-chewing | 2 | 1 | 0 | 10 | 0 | 25 | 1.5 | 0.3 | 0.1 | 0.1 | 2 | 14 | 3 / 3 |
| oxpinyin-core | 7 | 6 | 0 | 195 | 0 | 106 | 11.9 | 1.4 | 29.8 | 41.5 | 7 | 209 | 12 / 11 |
| oxpinyin-corpus | 5 | 3 | 1 | 51 | 3 | 72 | 5.0 | 0.7 | 4.3 | 5.7 | 5 | 58 | 17 / 10 |
| oxpinyin-data | 5 | 4 | 0 | 206 | 1 | 96 | 7.5 | 1.1 | 2.6 | 4.1 | 5 | 215 | 10 / 10 |
| oxpinyin-datagen | 6 | 4 | 1 | 27 | 2 | 124 | 5.6 | 1.0 | 0.0 | 0.0 | 6 | 37 | 4 / 4 |
| oxpinyin-dictool | 2 | 1 | 0 | 16 | 0 | 65 | 3.4 | 0.2 | 0.0 | 0.0 | 2 | 20 | 7 / 8 |
| oxpinyin-engine | 5 | 4 | 0 | 214 | 0 | 109 | 17.7 | 1.3 | 8.2 | 11.2 | 5 | 224 | 9 / 8 |
| oxpinyin-facade | 2 | 1 | 0 | 37 | 0 | 106 | 4.6 | 0.5 | 0.3 | 0.3 | 2 | 41 | 11 / 9 |
| oxpinyin-kmm | 4 | 2 | 0 | 46 | 5 | 41 | 3.2 | 0.6 | 0.0 | 0.0 | 4 | 49 | 4 / 4 |
| oxpinyin-punct | 3 | 1 | 1 | 13 | 0 | 23 | 1.2 | 0.1 | 0.0 | 0.0 | 3 | 19 | 3 / 3 |
| oxpinyin-runtime | 5 | 4 | 1 | 28 | 2 | 206 | 11.6 | 1.8 | 0.5 | 0.7 | 5 | 36 | 11 / 10 |
| oxpinyin-store | 3 | 2 | 0 | 55 | 4 | 32 | 4.6 | 0.5 | 0.1 | 0.2 | 3 | 57 | 7 / 6 |
| oxpinyin-testsupport | 1 | 0 | 0 | 18 | 0 | 14 | 1.5 | 0.0 | 0.0 | 0.0 | 1 | 20 | 10 / 10 |
| oxpinyin-training | 18 | 9 | 8 | 120 | 12 | 315 | 10.4 | 3.1 | 0.0 | 0.0 | 18 | 144 | 5 / 6 |
| oxpinyin-user | 3 | 2 | 0 | 226 | 0 | 140 | 23.3 | 0.8 | 0.8 | 0.6 | 3 | 232 | 12 / 11 |
| oxpinyin-word | 3 | 1 | 1 | 25 | 0 | 29 | 1.9 | 0.2 | 0.0 | 0.0 | 3 | 31 | 4 / 4 |
| oxpinyin-zhuyin-capi | 4 | 3 | 0 | 23 | 0 | 187 | 6.2 | 1.1 | 0.2 | 0.3 | 4 | 31 | 13 / 11 |
| pinyin-oracle | 24 | 13 | 14 | 110 | 3 | 384 | 10.9 | 3.9 | 31.8 | 81.7 | 24 | 155 | 24 / 18 |
| **Total** | **109** | **66** | **28** | **1,558** | **32** | **2,358** | **143.2** | **21.1** | **80.4** | **147.4** | **109** | **1,744** | **24 / 18** |

### 3.3 Binaries with no tests in this cell

These still compile and link, and nextest starts two `--list` processes
for each:

- cfg-gated integration binaries: `pinyin-oracle::{differential_live,
  live_smoke, export_seam}` (`#![cfg(feature = "oracle-ffi")]`), and
  `pinyin-oracle::real_tables_integration` (every item behind
  `cfg(feature = "oracle-ffi")`). Also `oxpinyin-datagen::libpinyin_parity`
  and `oxpinyin-runtime::libpinyin_data_dir`
  (`cfg(any(feature = "kyotocabinet", feature = "tkrzw"))`).
  `oxpinyin-capi::abi`, `oxpinyin-zhuyin-capi::companion_headers` and
  `oxpinyin-runtime::user_dir_round_trip` are empty off Linux only.
- 21 bin targets with no `#[test]`: `oxpinyin-corpus`, `oxpinyin-punct`,
  `oxpinyin-word`, eight `oxpinyin-training` bins and ten `pinyin-oracle`
  bins. Each test harness is 7 MiB and 0.1–0.2 s of build.
- The `oxpinyin-capi-marshal` lib has no unit tests.

## 4. Cost (tasks 2 and 3)

### 4.1 Workspace totals

| Measure | cargo test | nextest (`--profile ci`) | Ratio n/c |
|---|---|---|---|
| Run wall, with `$R` (C4, C5) | 74.2 s | 41.5 s | 0.56 |
| Run wall, without `$R` (C6) | 51.2 s | 36.7 s | 0.72 |
| Test-binary processes (`$R` log) | 109 | 1,744 (1,526 tests + 218 `--list`) | 16.0 |
| All successful `execve` (C8) | 132 | 3,232 | 24.5 |
| Largest test-process peak RSS | 24 MiB | 18 MiB | — |
| Test-process CPU, serial (C12, `--test-threads 1`) | 72.1 s | 102.0 s | 1.41 |
| Test-process CPU, four threads (C12) | 80.4 s | 147.4 s | 1.83 |
| Whole-command CPU incl. cargo and nextest, serial (C12, `$T2`) | 86.8 s | 124.8 s | 1.44 |
| Whole-command CPU, four threads (C12, `$T2`) | 81.6 s | 175.9 s | 2.16 |
| Run wall, serial / four threads (C12) | 78.3 / 54.1 s | 122.2 / 45.2 s | — |

nextest finishes sooner because it runs tests from different binaries in
parallel, while cargo test runs binaries one after another. Measured
serially, nextest's test processes use 1.41× the CPU of cargo test's:
29.9 s more, of which 29.4 s is the `pinyin-oracle` lib binary's corpus
regeneration (§4.3). Every other binary differs by 0.6 s or less. At four
threads the ratio is 1.83×, but the same nextest selection then takes
45.4 s more CPU than it does serially, so the four-thread figure
includes contention and overstates the work. The first revision of this
document summed per-process wall time (73.0 s vs 137.2 s, 1.88×). That
is not work under concurrency, and those figures are withdrawn. Its wall
time has a floor: the longest single test,
`oxpinyin-core::parser_acceptance over_limit_ambiguity_is_total_and_deterministic`,
took 26.4 s in its own process. Doctests (C7) add 2.1 s to either
harness.

### 4.2 Build and link (C1, C9)

| Step | Wall |
|---|---|
| Clean build of all test targets, `-j3` (C1) | 107.3 s; 352 units, 318.6 unit-seconds including dependencies |
| No-op rebuild | 0.36 s |
| `touch` `oxpinyin-core/src/lib.rs` | 19.1 s |
| `touch` one integration test file (`oxpinyin-core/tests/scheme_parsers.rs`) | 0.56 s |
| `touch` all 66 integration test files | 7.5 s |

Of the 318.6 unit-seconds in C1, workspace integration-test units take
56.8 s over 66 units (0.86 s each on average), workspace lib unit-test
units 79.8 s, and bin test harnesses 6.7 s over 24 units. The 66
integration binaries hold 1,581 MiB of the 2,358 MiB of debug test
executables. The size, not the compile time, is the larger
per-binary cost: each integration binary statically links its crate's
dependency closure with debug info, 7–68 MiB each.

### 4.3 Per-process costs (task 3)

Per-test setup that already runs once per test under cargo test costs
the same under nextest. The table lists only setups that cargo test
shares across tests and nextest repeats.

| Binary | Shared setup | Measured, CPU s, cargo test vs nextest (C12, C13; serial, then four threads) | Cells affected |
|---|---|---|---|
| `pinyin-oracle` lib, `corpus::tests` | `static CORPUS: OnceLock` holding `corpus::generate()` (10,465 inputs); 9 of 10 tests use it, `generation_is_deterministic` generates once more. 2 generations under cargo test, 10 under nextest | whole lib binary 11.3 s vs 40.7 s serial, 13.0 s vs 59.0 s at four threads. Serially, each nextest test that only reads `CORPUS` takes 3.4–4.2 s, about one generation; `generation_is_deterministic` takes 7.5 s | **Now**: CI Linux and macOS nextest jobs. Not the cargo-test cells (Windows nightly, kc/tkrzw store-features, overflow lane, coverage) |
| `pinyin-oracle::parity_corpus` | none shared. 3 of 4 tests call `generate()` | 11.3 s vs 10.9 s serial, 13.7 s vs 16.1 s at four threads: the same work. The first revision's 4.6 s vs 13.0 s was summed wall time, which counted concurrency, not work | Now, all cells; no nextest multiplier |
| `pinyin-oracle::differential_live` | `static RUN: OnceLock<RunOutput>` in `run_corpus()`, used by 7 tests; `a_live_run_is_deterministic` calls `execute_corpus()` twice. 3 corpus runs under cargo test, 9 under nextest. `static ORACLE_LOCK` in `src/live.rs` serialises oracles within one process only | C13: 26.9 s vs 69.6 s serial (2.6×), 22.8 s vs 80.1 s at four threads; 7.7–9.0 s per corpus run serially. Run wall for the three oracle binaries at four threads: 23.1 s vs 26.3 s | **Only with `oracle-ffi`**. No workflow enables it (§7). Latent for any oracle-enabled nextest cell |
| `pinyin-oracle::live_smoke`, `export_seam` | none shared. 8 of 9 `live_smoke` tests open their own oracle | 0.2 s or less in either harness | Only with `oracle-ffi` |
| `oxpinyin-engine` lib, `nbest` tests | `static DICTIONARY` holding an empty `FixtureDictionary` | negligible | — |

Other per-process costs, all paid per test under both runners:

- Fixture parsing: `graph_paths` parses `fixtures/w4/oracle-paths.txt`
  (about 950 KB) in each of 5 tests (CPU 1.7 s vs 1.8 s serial);
  `decode_differential` parses it in both tests (1.9 s vs 2.2 s);
  `differential_replay`, `taxonomy_replay` and `capture_replay` re-read
  `fixtures/foundation/f-a.txt` and `f-c.txt` (19 KB and 36 KB).
- w3 fixture opens: `oxpinyin-capi::abi` (33 tests,
  `oxpinyin_init_for_fixtures`), `oxpinyin-runtime::assembly` (20,
  `Runtime::open`), `oxpinyin-facade::instance_laws` (16,
  `ContextCore::open`). Each of these crates' test processes used 1.6 s
  CPU or less in every C12 run.
- Backend files: `oxpinyin-store::trait_laws`,
  `oxpinyin-user::{user_store_acid, user_store_semantics}`,
  `oxpinyin-runtime::{assembly, empty_user_dir}`, the oxpinyin-data
  reader tests and `oxpinyin-datagen` write tempdir stores per test.
- Subprocesses: `oxpinyin-corpus::cli` and `oxpinyin-dictool::cli` run
  their `CARGO_BIN_EXE_*`; `user_store_acid` re-runs its own executable
  for `abort_mid_batch_leaves_a_consistent_prefix`;
  `pinyin-oracle::model_fetch` runs `bash tools/model/fetch-model.sh`;
  `oxpinyin-data` `table_conf.rs` unit tests run `mkfifo`. Ignored
  differentials run pin-built tools. No test runs `cargo`. C8 counts 132
  successful `execve` for the whole cargo test run.
- Proptest: `parser_acceptance` (three properties, up to 4,096-byte
  inputs) is the slowest binary in either harness: 20.5 s CPU under cargo
  test and 20.3 s under nextest, serially (19.9 s and 29.5 s at four
  threads).

`--include-ignored` adds the ignored tests' own setups (§7). None of
them shares a static, so none has a nextest multiplier. Under nextest
they can overlap each other, except where §6 serialises them.

## 5. Grouping integration binaries: oxpinyin-core (task 4)

Experiment (C10), in a detached scratch worktree that was never
committed. `crates/oxpinyin-core/Cargo.toml` got `autotests = false` and
one `[[test]] name = "integration", path = "tests/all/main.rs"`.
`tests/all/main.rs` declares the six existing files as modules with
`#[path = "../<file>.rs"] mod <file>;`. The test files did not change,
and their `include_str!` paths still resolve relative to each file.

| Measure | Base (6 binaries) | Grouped (1 binary) | Change |
|---|---|---|---|
| Clean `-p oxpinyin-core --no-run`, `-j3` | 35.3 s | 29.0 s | −18% |
| Integration-test unit time (cargo-timings) | 4.90 s (6 units) | 1.58 s (1 unit) | −68% |
| Integration-binary bytes on disk | 78.1 MiB | 28.7 MiB | −63% |
| Rebuild after `touch tests/scheme_parsers.rs` | 0.36 s | 0.46 s | +0.10 s |
| Rebuild after `touch src/lib.rs` | 1.64 s | 1.26 s | −0.38 s |
| cargo test run, 3 repetitions | 23.1 / 21.9 / 22.4 s | 20.3 / 21.1 / 21.2 s | about −1.6 s |
| nextest run, 3 repetitions | 21.0 / 22.2 / 21.6 s | 20.8 / 19.5 / 20.2 s | about −1.4 s |
| Successful `execve` in one nextest run | 401 | 352 | −49 |
| Tests listed | 151 lib + 44 integration | 151 lib + 44 integration | same |

Effects:

- Test identities change from `oxpinyin-core::<file> <test>` to
  `oxpinyin-core::integration <file>::<test>`. Every test name is kept
  and only gains a module prefix. Nothing in `.config/nextest.toml`,
  `ci.yml`, `verify-nightly.yml`, `store-backends.yml`, `tools/` or
  `justfile` names an oxpinyin-core integration binary (grep of each for
  the six binary names).
- Parallelism: nextest still runs one process per test, so the shape of
  its parallelism does not change. Under cargo test the 44 tests share
  one libtest thread pool instead of six sequential pools. The run time
  is dominated by the 26 s proptest either way.
- nextest groups: oxpinyin-core is in no test group and has no
  `threads-required` override.
- Process isolation: none of the six files changes process state (locale,
  working directory, environment). Each file builds its own fixture
  doubles from `include_str!` constants.
- Incremental: editing one test file now recompiles the merged crate.
  The measured cost is +0.1 s. The cost of linking five fewer 7–21 MiB
  binaries outweighs it on every other path.

The same pattern does not transfer as-is to the binaries #1005 excludes.
`oxpinyin-capi::locale` and `oxpinyin-zhuyin-capi::locale` are separate
on purpose (process locale). The `store-backend` group and
`threads-required` select binaries by `binary(...)` name. The oracle
suites are cfg-gated per binary.

## 6. CI cells and the test selection they run

| Cell (workflow, job) | Harness | Backend | Selection |
|---|---|---|---|
| `ci.yml` `test` (Linux) | nextest `--profile ci`, then `cargo test --doc` | bdb (default) | `--workspace`, all crates |
| `ci.yml` `test-macos` | nextest `--profile ci`, then `--doc` | bdb | excludes capi, zhuyin-capi, corpus, kmm, punct, word |
| `verify-nightly.yml` `test-windows` | cargo test | bdb (vcpkg 4.8) | same exclusions as macOS |
| `store-backends.yml` `store-features` | cargo test | kc, tkrzw (matrix) | excludes corpus, kmm, punct, word |
| `store-backends.yml` `tkrzw-sanitizers` | cargo test (ASan) | tkrzw | `-p oxpinyin-store` |
| `store-backends.yml` `bdb-sanitizers` | `tools/bdb/run-sanitizers.sh` | bdb | `-p oxpinyin-store` |
| `verify-nightly.yml` `overflow-lane` | cargo test `--release` with overflow checks | kc | `--workspace` |
| `coverage.yml` (nightly and PR advisory) | `cargo llvm-cov` | kc | `--workspace` |
| `verify-nightly.yml` `differentials` | `tools/oracle/nightly-fixture-differentials.sh` | tkrzw | named ignored tests (§7) |

Only the two `ci.yml` jobs use nextest, so only they pay the nextest
multipliers in §4.3. No workflow passes `--include-ignored` or
`--run-ignored`. No workflow enables `oracle-ffi`. This host measured
only the first row.

`.config/nextest.toml`: the `store-backend` group (`max-threads = 1`)
names eight binaries. `threads-required = "num-test-threads"` applies to
`bdb_libpinyin_files`, `libpinyin_data_dir` and `user_dir_round_trip`.
All tests in those three are `#[ignore]` (and `libpinyin_data_dir` is
empty under bdb), so the budget override takes effect only with
`--run-ignored`. The second override repeats `test-group =
'store-backend'`. That is harmless because the first matching override
already sets it.

## 7. Ignored tests and the invocation that selects them (task 6)

37 tests carry `#[ignore = "needs …"]` in source. 32 are listed in the
bdb Linux cell. The other five do not compile there:
`oracle_candidates_fixture_is_fresh`, `sentence_surface_fixture_is_fresh`
and `structure_fixture_is_fresh` need `oracle-ffi`.
`drop_in_output_matches_the_pin_built_data_dir` and
`an_unmodified_libpinyin_data_directory_opens_and_decodes` need kc or
tkrzw.

Invokers (grep of `.github/`, `tools/`, `justfile`, `docs/runbooks`,
`docs/testing`):

- **N** — nightly: `tools/oracle/nightly-fixture-differentials.sh`
  (`verify-nightly.yml` `differentials`, tkrzw).
- **M** — manual developer runner: `tools/oracle/run-differentials.sh`
  (`docs/runbooks/oracle.md`).
- **D** — `tools/datagen/libpinyin-drop-in-differential.sh` (manual, and
  also called by N).
- **U** — `tools/oracle/user-dir-round-trip.sh`, the
  `user-dir-round-trip` gate in `tools/bisection/runners.json` (three
  cells; lane runs, not a workflow).
- **G** — `tools/bisection/gcov_measure.py` (J-ledger measurement).
- **Doc** — a documented manual procedure only.
- **None** — no script, gate or runbook selects it.

| Binary | Test | Needs | Invoker | Class |
|---|---|---|---|---|
| oxpinyin-training::segment-differential | `rust_matches_live_ngseg_when_both_exist` | export, model20, pin `ngseg` | N, M | intentional |
| same | `rust_matches_committed_ngseg_golden`, `committed_golden_matches_live_ngseg` | export + model20; pin `ngseg` | M | intentional; the nightly header excludes model20-space goldens |
| oxpinyin-training::segment-spseg-mergeseq | `rust_matches_live_spseg_when_present` | pin `spseg` | N, M | intentional |
| same | `rust_matches_live_mergeseq_when_present` | pin `mergeseq` | M | intentional; the nightly header excludes it (pin token outside the w3 table) |
| oxpinyin-kmm::differential | `rust_kmm_matches_pin_{gen_and_export, to_interpolation, merge, validate}` | pin KMM tools | N, M | intentional |
| same | `rust_kmm_matches_pin_prune` | pin KMM tools | M | intentional; the nightly header excludes it (pin SIGSEGV on that input) |
| oxpinyin-training::counter-differential | `rust_matches_committed_manifest`, `rust_matches_live_gen_ngram` | export; pin `gen_ngram` | M | intentional; model20 space |
| oxpinyin-training::lambda-differential | `rust_lambda_matches_committed_manifest`, `rust_lambda_matches_live_estimate_interpolation` | export; pin tools | M | intentional; model20 space |
| oxpinyin-training::eval-differential | `native_correction_rate_matches_pin_eval_correction_rate` | pin `eval_correction_rate` + data | M (when its data exist) | intentional; the nightly header excludes it |
| oxpinyin-training::emitter-differential | `fixture_emit_roundtrips_through_parse_interpolation2`, `rust_matches_live_export_interpolation` | export; pin `export_interpolation` | Doc (`docs/runbooks/goldens-and-pins.md`, trainer manifests row; not in `run-differentials.sh`) | intentional |
| oxpinyin-datagen::libpinyin_parity (kc/tkrzw) | `drop_in_output_matches_the_pin_built_data_dir` | pin data dir + model20 | D, N (tkrzw) | intentional |
| oxpinyin-runtime::user_dir_round_trip | `a_pin_profile_loads_and_saves_back_in_place`, `oxpinyin_trains_and_saves_its_own_profile` | pin oracle | U (filters `a_pin_profile`, `oxpinyin_trains`); G | intentional, lane gate |
| pinyin-oracle::sentence_surface_parity | three `sentence_surface_*` tests | export + model20 | G | intentional, measurement |
| pinyin-oracle::candidate_structure (oracle-ffi) | `structure_fixture_is_fresh` | live oracle, full corpus | Doc (regenerate with `oracle-candidate-structure`) | intentional, freshness |
| pinyin-oracle::real_tables_integration (oracle-ffi) | `oracle_candidates_fixture_is_fresh` (~3 h), `sentence_surface_fixture_is_fresh` | live oracle | Doc (`docs/testing/f1-…`, `f2-…`, `f3-…`, `corpus-tail.md`) | intentional, freshness |
| oxpinyin-datagen::fixtures_identity | `mini_compile_reproduces_the_committed_fixture_directory` | model20 | Doc (`docs/runbooks/goldens-and-pins.md`) | intentional |
| oxpinyin-corpus::differential | `rust_chain_consumes_t4b_sample_with_zero_glue`, `end_to_end_matches_live_libpinyin_pipeline` | export + model20; pin trainers | Doc (`docs/testing/corpus-pipeline.md`) | intentional |
| oxpinyin-corpus::invariants | `converter_turns_traditional_into_simplified_when_opencc_present` | opencc | Doc (`corpus-pipeline.md`) | intentional |
| oxpinyin-store::bdb_libpinyin_files (bdb) | four tests (`the_real_system_bigram_*`, `the_real_index_tree_walks_in_byte_order`, `a_written_user_bigram_reads_back_through_the_user_seam`) | real BDB data dir | **None** (`tools/bdb/run-sanitizers.sh` reads the same variable but runs its own `hash-walk`, not these tests) | **unintentional gap**: real BDB file-format coverage |
| oxpinyin-data::phrase_library | `reads_a_real_libpinyin_installation` | real libpinyin install | **None** | **unintentional gap** |
| oxpinyin-datagen::cross_backend | `the_compiled_backend_reads_back_through_the_runtime_reader` | model20 | **None**. The module doc says "the backend matrix's local recipe does", but no script under `tools/store/` or runbook passes `--include-ignored` for it | **unintentional gap; stale comment** |
| oxpinyin-runtime::libpinyin_data_dir (kc/tkrzw) | `an_unmodified_libpinyin_data_directory_opens_and_decodes` | pin data dir | **None** (named in `docs/testing/upstream-test-coverage.md` only) | **unintentional gap**; its module doc still says local runs "skip" |

Four groups have no invoker (G1, tracked in #1021). The first revision
of this document also counted `emitter-differential` as one. The trainer
manifests row of `docs/runbooks/goldens-and-pins.md` names the ignored
emitter differential tests as the check for regenerating those
manifests, so that row is Doc.

Non-ignored tests that no workflow compiles, because they need
`oracle-ffi`: `differential_live` (11), `live_smoke` (9) and
`export_seam` (1). Their documented invocation is
`docs/testing/oracle-ffi-seam.md`: `cargo test -p pinyin-oracle
--features oracle-ffi` with `PINYIN_ORACLE_PREFIX` at the tkrzw oracle.
C14 ran it with the default (bdb) Rust features, and it passed.

The three oracle-ffi freshness tests, `structure_fixture_is_fresh`,
`oracle_candidates_fixture_is_fresh` and
`sentence_surface_fixture_is_fresh`, are `#[ignore]`. That invocation
does not select them; they are in the table above. Each needs the same
environment plus the ignored filter, for example `cargo test -p
pinyin-oracle --features oracle-ffi --test candidate_structure --
--ignored structure_fixture_is_fresh`, and `--test
real_tables_integration -- --ignored <name>` for the other two
(`oracle_candidates_fixture_is_fresh` runs about 3 h, by its ignore
reason).

Both are coverage questions, not deletion candidates.

## 8. Duplication (task 5)

Judged by protected behaviour, failure mode and input equivalence. A
REMOVE needs the same entry point, a subset of the inputs, the same
source of expected values and the same failure mode.

### 8.1 oxpinyin-core parser trio

`parser_acceptance.rs` (12), `scheme_parsers.rs` (12) and
`zhuyin_parse_parity.rs` (2) have different entry points and inputs:

- `parser_acceptance` drives `FullPinyinParser`.
- `scheme_parsers` drives `DoublePinyinParser`, `ZhuyinParser` and
  `parse_full_pinyin_index`. It ports `test_parser2.cpp`.
- `zhuyin_parse_parity` replays the pin golden
  `fixtures/w4/oracle-zhuyin-parse.txt` (26 records).

The trio does not overlap across files. The overlap is between
`parser_acceptance` and the unit tests in `src/parser.rs`:

| Test | Overlap | Verdict |
|---|---|---|
| `empty_input_returns_one_empty_segmentation` | `src/parser.rs` `complete_paths_follow_frozen_greedy_order` asserts the same `b""` result through the same `assert_parse` shape | strict duplicate |
| `ambiguous_xian_returns_both_complete_segmentations` | same unit test, identical input and expected vector | strict duplicate |
| `apostrophe_is_hard_boundary_for_xi_an` | `apostrophes_and_partial_fallback_follow_frozen_boundaries`, identical input and expected vector | strict duplicate |
| `single_valid_syllables_parse`, `maximum_length_valid_syllable_zhuang` | inputs `ma`, `zhuang`, `nv` are a subset of `every_table_syllable_round_trips_as_complete_identity` (all 405 syllables, same assertions) | strict subset |
| `every_table_syllable_round_trips_as_complete_identity` | `src/parser.rs` `every_frozen_syllable_is_the_first_complete_path` checks only `paths[0]` and is the subsumed one | keep the integration test |
| `trailing_junk_after_valid_prefix_nihx`, `pure_non_pinyin_input` | inputs not in the unit tests | KEEP |
| `frozen_f_a_selected_paths_are_portable_paths` | pin golden `f-a.txt`; only test that catches drift from the pin capture at the parser | KEEP |
| three proptests | totality, determinism, over-limit `TooManyAlternatives` for 13–32 groups (unit test covers the 12/13 boundary) | KEEP |

In `scheme_parsers.rs`, two tests overlap unit tests in `src/scheme.rs`
but differ in start state or input (`the_customized_slot_is_rejected_at_the_parser_seam`
starts from the default Ms layout; `incomplete_zhuyin_keystrokes_consume_zero_either_way`
adds `v`). They are not strict duplicates. Every other `scheme_parsers`
test has no overlap.

`scoring.rs` and `scoring_fixture.rs` share F-A inputs (`fangan`, `nih`)
but not failure modes. `scoring_fixture` scores only the pin's selected
segmentation, so it cannot catch a broken cross-segmentation penalty,
which `worked_example_the_segmentation_penalty_orders_fangan` catches.
Both stay.

### 8.2 C API and Zhuyin C API mirror pairs

- `oxpinyin-capi/tests/locale.rs` and `oxpinyin-zhuyin-capi/tests/locale.rs`:
  `diff` shows one non-comment line, `pinyin_capi::pinyin_init` vs
  `zhuyin_capi::zhuyin_init`. Same input, same expected value (the
  LC_NUMERIC reset in `table_info.cpp`, as the test's doc comment cites
  it). Each guards a
  different exported symbol in a different cdylib, and each is its own
  executable because process locale must not interleave with other
  tests. KEEP both. A shared helper would be a new generic abstraction,
  which #1005 rules out, for about 55 lines.
- `userdir_arg.rs` pair: besides the symbol swaps and pin citations
  (`pinyin.cpp` vs `zhuyin.cpp`), the expected values differ. After an
  init with `""`, pinyin expects `user.conf` in the working directory and
  zhuyin expects nothing (the test's own pin citation,
  `zhuyin.cpp:126-162`; not re-read from a pin checkout for this audit). The
  imported phrase differs too. Not duplicates. KEEP both.
- `oxpinyin-runtime/tests/empty_user_dir.rs` tests resolution at the
  Rust runtime seam. It complements the C-boundary NULL vs `""` tests.

### 8.3 Other pairs checked, no duplication

`oxpinyin-user` `user_store_acid` (crash consistency, hostile files) vs
`user_store_semantics` (algebra, upstream ports);
`oxpinyin-store::trait_laws` (raw `Store` laws per backend) vs the
oxpinyin-user suites (user state over a store); oxpinyin-data reader
tests (synthetic files) vs `oxpinyin-runtime::assembly` (committed w3
through `Runtime::open`).

## 9. Candidates and classification (task 7)

Everything not listed here is **KEEP**. That covers every pin-golden and
differential test, both C API suites, all backend file-format,
user-state persistence and store tests, and all 37 ignored tests.

| ID | Candidate | Class | Benefit (measured) | Risk | Detection unchanged because |
|---|---|---|---|---|---|
| A1 | oxpinyin-core: six integration binaries into one (§5) | CONSOLIDATE | −49 MiB of test binaries, −3.3 s of test-unit build, −6.3 s clean `-p` build, −1.4 to −1.6 s of run wall; five fewer binaries to build, link and list | Test identities gain a module prefix. One file's edit recompiles all six modules (+0.1 s) | Same files, same `#[test]` items, same assertions and fixtures; listed test count unchanged (151 + 44) |
| A2 | `pinyin-oracle` lib `corpus::tests`: 10 corpus generations per nextest run | CONSOLIDATE (fold the nine read-only checks over one generated corpus into fewer `#[test]` items) or leave as is | 10 → 2 corpus generations per nextest run if `generation_is_deterministic` joins the merged item, 10 → 3 if it stays separate: about 29 s or 26 s of CPU measured serially, at about 3.7 s per generation (the lib binary's 40.7 s toward cargo test's 11.3 s, C12). That is 29.4 s of nextest's 29.9 s serial CPU excess over cargo test, in the CI Linux and macOS nextest jobs | Coarser failure reports: one failing check hides the next unless the test collects failures. Test identities change | Same checks over the same generated corpus. `generation_is_deterministic` keeps its own second generation |
| A3 | `pinyin-oracle::differential_live`: 9 corpus runs under nextest | Latent. CONSOLIDATE option: merge the seven `run_corpus()` readers into fewer `#[test]` items. A nextest test group with `max-threads` only caps how many oracle contexts run at once. It saves no CPU, because nextest still runs each test in its own process and each process fills its own `OnceLock` | Merging alone: 9 → 3 corpus runs, about 69.6 → 27 s of CPU serially (C13), in any future oracle-enabled nextest cell. A test group alone: fewer concurrent pin contexts, no CPU saving | No current cell pays it. Changing it now changes nothing measured in CI | Same assertions on the same `RunOutput` |
| A4 | 21 test-less bin harnesses and 6 cfg-empty integration binaries | RELOCATE (`test = false` on the bins only) or KEEP | About 3.3 s of build, about 150 MiB, 42 `--list` processes per nextest run | A future `#[test]` in such a bin would silently not run. The cfg-empty binaries must stay because other cells fill them | Nothing runs today |
| A5 | `parser_acceptance` strict duplicates (§8.1, 5 tests), or the unit copies; `src/parser.rs::every_frozen_syllable_is_the_first_complete_path` | REMOVE (one copy each) | Milliseconds; maintenance only | The unit and integration copies sit at different seams (in-crate vs public API). Removing the unit copies keeps the public-seam check | Identical input, entry point and expected value remain in the other copy |
| A6 | `scheme_parsers` `the_customized_slot_is_rejected_at_the_parser_seam`, `incomplete_zhuyin_keystrokes_consume_zero_either_way` | KEEP | — | — | Not strict subsets (§8.1) |
| A7 | `capi`/`zhuyin-capi` `locale.rs` and `userdir_arg.rs` pairs | KEEP | — | — | Separate ABIs; `userdir_arg` expected values differ |
| A8 | `parser_acceptance` `over_limit_ambiguity_is_total_and_deterministic` (26.4 s, sets nextest's wall floor) | KEEP | — | Moving it out of per-PR CI would weaken per-PR detection of no-panic (constitution §4) | — |
| G1 | Ignored tests with no invoker, four groups (§7: `bdb_libpinyin_files` ×4, `reads_a_real_libpinyin_installation`, `cross_backend`, `libpinyin_data_dir`). Tracked in #1021 | KEEP; coverage gap to assign | Restores selection of real-file BDB, real-installation and cross-backend coverage | Needs inputs CI never has. Selecting them belongs in a runner such as `run-differentials.sh`, not in CI | — |
| G2 | Stale module docs: `cross_backend.rs` ("the backend matrix's local recipe does"), `libpinyin_data_dir.rs` (local runs "skip"), and the headers of `oxpinyin-training/tests/{counter,segment,lambda,eval}-differential.rs`, `oxpinyin-kmm/tests/differential.rs` and `oxpinyin-corpus/tests/differential.rs`, which say a missing prerequisite makes the test skip. An explicitly selected ignored test panics on a missing input (`docs/testing/README.md`). Tracked in #1021 | doc fix | Accuracy | None | — |

## 10. Go / no-go for the first implementation PR

**GO for A1 alone**, as its own PR from main: group the six oxpinyin-core
integration tests into one binary.

Reasons:

- It comes first in #1005's priority order (per-binary measurement, then
  safe binary consolidation, then semantic dedup), and §5 measured it on
  this baseline.
- It changes no assertion, fixture or test body. The listed count stays
  at 195 for the crate.
- No `binary(...)` filter, CI invocation, runner or process-isolated
  executable names an oxpinyin-core integration binary.
- The only cost found is +0.1 s when one test file is edited.

Conditions for that PR: keep the six files and their test names, compare
`cargo nextest list -p oxpinyin-core` before and after (195 tests, names
equal apart from the module prefix), and record binary bytes and build
time from one run against base, as §5 does.

**No-go for now** on A2 and A3 (their benefit depends on accepting
coarser test identities, and A3 has no current cell), A4 (silent-skip
risk), A5 (negligible benefit; last in priority) and any change to the
capi mirror pairs. G1 and G2 are coverage and documentation follow-ups,
not test-suite reductions. Each needs its own decision.

What the CPU-time re-measurement changed (§4.1, C12–C13): A2's measured
benefit falls from about 46 s of summed wall to about 26–29 s of CPU per
nextest run. A3's benefit now comes from merging test items alone, and
the nextest test group no longer counts as a saving. `parity_corpus` has
no nextest multiplier. No classification and no go/no-go decision
changed. A1's evidence is build time, binary bytes and run wall (§5),
which the re-measurement does not touch.
