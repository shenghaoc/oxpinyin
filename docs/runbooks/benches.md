# Benches — criterion, the perf baseline, and profiles

Three tiers, none run by CI (`verify-nightly` has no perf lane): the
criterion benches for a component, the C-ABI perf baseline against the
oracle, and a sampled profile of the keystroke cycle. Numbers are only
comparable within one container on one host; every published snapshot
(`docs/perf/`, `docs/findings/perf-*.md`) names both.

## Criterion benches

| crate | bench | needs |
| --- | --- | --- |
| oxpinyin-capi | `stage2` | the export (`PINYIN_EXPORT_DIR`), the C-ABI surface |
| oxpinyin-store | `backend_matrix_{tkrzw,kyotocabinet,lmdb,redb}` | `--features <backend>` (each is `required-features`-gated) |
| oxpinyin-store | `lmdb_bulk_load`, `redb_is_empty` | the named backend |
| oxpinyin-user | `export_phrases`, `phrase_read` | the export |
| pinyin-oracle | `scan_perf`, `dbm_bench`, `alloc_profile` | the export, model20 (`dbm_bench` also the bench oracles) |

Run one bench by name; without `--bench <name>` cargo also runs the lib
under libtest, which rejects criterion flags:

```sh
cargo bench -p oxpinyin-store --no-default-features --features redb --bench backend_matrix_redb
cargo bench -p oxpinyin-capi --bench stage2 -- --profile-time 10
```

A backend-specific criterion bench — one that names a peer's optional
dependency, such as heed for `lmdb` — must carry
`required-features = ["<backend>"]` in its `[[bench]]` entry. CI runs
`cargo clippy --workspace --all-targets` on the default backend, and
without it the target fails to resolve the dependency instead of being
skipped. Run one bench with `--bench <name>`; without it cargo also runs
the lib under libtest, which rejects criterion flags such as
`--profile-time`. To run a bench in debug mode: `cargo test --bench
<name>` (AGENTS.md points here).

## The perf baseline (oracle vs installed oxpinyin)

`tools/bisection/run-perf-baseline.sh` measures speed, installed size
and RSS with one dlopen harness (`bisect --perf`), alternating oracle
and oxpinyin processes. It needs the pin-built oracle, `cargo-c` and
the model cache. The reproducible form is the perf-matrix container:

```sh
docker build --platform linux/arm64 -f tools/bisection/Dockerfile.perf-matrix -t oxpinyin-matrix .
docker run --rm -v /tmp/perf-out:/out -e PINYIN_ORACLE_PREFIX=/opt/libpinyin-tkrzw \
    -e OXPINYIN_PERF_WORK=/out oxpinyin-matrix bash tools/bisection/run-perf-baseline.sh
```

Knobs: `PERF_RUNS`, `PERF_CYCLES`, `PERF_RAM_RUNS`, `PERF_CPU` (pin a
CPU; do it). `tools/bisection/run-perf-matrix.sh` is the four-backend
form. For a parent-vs-HEAD comparison use one `CARGO_TARGET_DIR` per
tree: cargo's freshness check is fooled by `git archive` mtimes and will
reuse the wrong artifacts otherwise.

## Profiles

```sh
tools/profile/run-w8-cycle.sh
```

Stages oxpinyin-capi through `cargo cinstall --profile profiling` (the
`profiling` profile keeps line tables and thin LTO) and samples the
keystroke cycle with `samply`, falling back to `cargo flamegraph` or
`perf`. Artifacts land in `target/profile/`;
`tools/profile/extract-hot-stacks.py` summarises them.
`PROFILE_REUSE_STAGE=1` skips the restage.

## Writing it up

A snapshot records the container image, the host, the pin, the exact
commands, and the controls (`docs/perf/perf-stage2-harness-2026-08.md`
is the template; `docs/findings/perf-keycost-first-alloc-2026-09-07.md`
a recent full example with confidence intervals and an acceptance
gate). A change that trades time for space or the reverse must say so
and justify it (AGENTS.md, "Source policy").
