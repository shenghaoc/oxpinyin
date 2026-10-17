# Running the initialization benchmark

`initbench` (crate `oxpinyin-initbench`) measures where initialization time and
memory go for two shared objects: library load, `*_init`, option setup,
`*_alloc_instance` and startup memory. The time to the first usable candidate is
the outer startup boundary, measured to expose initialization deferred until
first use. It loads each object with `dlopen`
(`libloading`), so it measures the *artifact*, not a source tree. It does not
measure typing, steady state or training. Evidence and findings:
`../findings/startup-initialization-audit.md`; numbers:
`../perf/startup-baseline-2026-10.md`.

Linux and macOS are supported natively; Windows is not verified. Memory comes
from `/proc` on Linux and `ps` on macOS (the macOS path is implemented and unit
tested but has not been run on a Mac; treat it as unmeasured). The
`instrument` subcommand needs Valgrind and is Linux-only.

## What is compared

Acceptance rule: same backend (Berkeley DB), same system model data, equivalent
initial profile state, same API workload, **and** verified identical observable
behaviour. The workload is fixed by `workload_spec()` in
`crates/oxpinyin-initbench/src/workload.rs`; the report prints its SHA-256.

Units, in order, per process: U1 load + resolve every symbol; U2 `*_init`
(system dir, private user dir); U3 `*_set_options(0x18a)`; U4 `*_alloc_instance`;
U5 parse `nihao` and fetch the first candidate string (not a typing workload);
U6 teardown, outside the timed intervals. Pinyin additionally calls
`pinyin_guess_candidates(0, 0x1e)`.

Before any timing the tool runs an untimed **verification** phase through the
same code path: every return value, parsed length, candidate count/type/string/
order, sentence, stderr diagnostics, files left in the profile, hostile inputs
(one process each, exit class recorded), and a save/reopen persistence
scenario. A mismatch makes the cell **NOT COMPARABLE**: no ratio is printed for
it. Each timed sample also carries a digest of its observations, and a sample
that contradicts the verified transcript is rejected, not averaged.

## Run it

```sh
cargo build --release -p oxpinyin-initbench
B=target/release/initbench
# fixtures: empty; modest (80 import calls, 2 distinct phrases); phrases and
# journals (10 000 distinct phrases at frequency 20 000); built
# through the public import API by the reference library and checked to load
$B profiles --out <dir> --pin-lib <pin libpinyin> --system-dir <data>
$B run --subject pin=<pin libpinyin> --zhuyin-subject pin=<pin libzhuyin> \
       --subject ox=<ox libpinyin>   --zhuyin-subject ox=<ox libzhuyin> \
       --subject ox-control=<same bytes as ox, another path> ... \
       --system-dir <data> --profile <dir>/empty --profile <dir>/modest ... \
       --work-dir <scratch> --cpu 2 --kind timing --rounds 60 --warmup 3 \
       --out run-timing.jsonl
$B run ... --kind memory --out run-memory.jsonl
$B report --input run-timing.jsonl --reference pin --ratio ox/pin --ratio ox-control/ox
$B instrument --lib <so> --facade pinyin --system-dir <data> --profile <fixture> \
       --tool callgrind --cpu 2 --work-dir <scratch>
```

Bad arguments print the usage. Options:
`--cache cold` drops the page cache first (needs root; reported as "cold at
guest", not cold hardware), `--bind lazy` uses `RTLD_LAZY`, `--rounds 0`
verifies only. Samples alternate subjects within a round, one process per
sample.

Always include a same-bytes control subject (`ox-control`). Its ratio against
`ox` shows the noise floor; if it is not near 1, the machine is not quiet and
the run is not a result. Pin the benchmark to a CPU (`--cpu`) and record the
host line the report prints.

## Tests

`cargo test -p oxpinyin-initbench` runs the unit tests (sequencing, timing
boundaries, validation, errors, comparison, mutation detection) and the
integration tests that need no model. The live tests need the artifacts and
fail, never skip, when they are missing:

```sh
OXPINYIN_INITBENCH_PIN_PINYIN=… OXPINYIN_INITBENCH_PIN_ZHUYIN=… \
OXPINYIN_INITBENCH_OX_PINYIN=… OXPINYIN_INITBENCH_OX_ZHUYIN=… \
OXPINYIN_INITBENCH_DATA=<system data dir> \
cargo test -p oxpinyin-initbench --test initbench -- --ignored --test-threads=1
```

The oxpinyin objects must be built from the tree under test
(`cargo cinstall --release`, backend `bdb`); the benchmark records their
SHA-256 and does not build them.

## Safety

All `unsafe` is in `src/ffi.rs`, behind the `SymbolSource` seam, with a
`// SAFETY:` comment on every block. The library handle is never unloaded, a
missing symbol is an error that names every missing symbol, and the context and
instance are released in pin order. Evidence captures and profiler output are
not committed; keep them out of the PR.
