# KC and tkrzw Pss attribution (#1098)

## Scope and release condition

Tooling preparation, recorded 2026-10-23 UTC. Findings measurements remain
paused; #1031 retains heavy-work priority. #1099 remains parked and must not
start automatically. No provisioning, shipping build, profiling or measurement
has run for this issue. No historical figure is a current-run baseline.

The maintainer approved extending KC/tkrzw comparison stages on the new
`wip/1091-cold-cloud` parent. Its verified committed tip was `fd28e96e`, based
on main `294f14eb`; the old unfinished `wip/1091-cold-cache` was not adopted.
`gh stack` registers and rebases this layer on the cloud parent. The extension
does not change the parent's cold-file enumeration, eviction policy, major
fault instrumentation or native lifecycle worker. #1090's init instrumentation
follows this shared-tooling checkpoint and is not implemented here.

## Comparison-stage extension

At source `747b00cd`, only KC/tkrzw profile provisioning was supported.
Confirmation and attribution were refused because final-profile comparison
handled BDB containers only. This extension resolves that source limitation
before measurements: each peer backend's user indexes and user bigram are
compared by all logical records, including empty values. KC loads snapshots;
tkrzw opens TreeDBM indexes and HashDBM bigrams read-only. Unknown files
remain byte comparisons. Existing pinyin-index padding and chunk-header
timestamp rules remain the only normalization. BDB behavior is preserved.

Peer readers run as a mode of the existing isolated contract worker, so
backend native libraries do not remain mapped in the controller between
samples. Runtime checks cover each actual copied object before native,
fixture-population and comparison launches. The tkrzw reader loads the exact
certified runtime path; errors refuse a comparison rather than producing
an empty or partial database result. No new driver or dependency is added.

The source basis was read with
`git -C /home/sheng/repos/libpinyin show 074a2219:PATH`:
`src/storage/ngram_kyotodb.cpp:54-64,82-91` loads/saves the user bigram as
a snapshot; `src/storage/ngram_tkrzwdb.cpp:48-81` uses a HashDBM temporary;
`src/storage/phrase_large_table3_tkrzwdb.cpp:83-118` uses TreeDBM;
`src/storage/chewing_large_table2_tkrzwdb.cpp:92-106` loads TreeDBM records.
These are source-derived storage requirements, not performance results.

The existing worker and readers already provide the relevant primitives:
`teardown.py::mapping_group` and `read_memory` read mapping classes and
checkpoint totals from the same smaps observation; the teardown worker
pauses at M0 and M1. `tools/measure-function-order.py::teardown_memory`
reports total Pss from a later smaps_rollup read. Its text-Pss measurement
uses the earlier smaps read. These are different sampling definitions;
report any rollup-versus-mapping-sum difference explicitly rather than
silently substituting one for the other.

## Tooling verification checkpoint

Light gates passed on Ubuntu with the repository-managed CPython 3.15:

```sh
uv run --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal tools/bisection/contract-diff.test.py
# 93 tests passed, including seven peer-reader/runtime tests.
uv run --no-project --managed-python --python cpython@3.15+gil python -B -X lazy_imports=normal tools/bisection/teardown.test.py PureTests ColdTests
# 35 tests passed.
cargo fmt --all --check
git diff --check
```

AST comparison against parent `fd28e96e` confirmed unchanged
`library_dependencies`, `cold_files`, `resident_pages`, `evict_files` and
`report_times`; `git diff --exit-code wip/1091-cold-cloud --
tools/bisection/teardown.h` also passed. Native teardown/stub and real-ELF
runtime harness gates remain pending heavy admission after #1031 priority;
this is a WIP checkpoint, not a completed code-change gate. No backend
campaign or shipping measurement has run.

The three verification-rule-2 checks are empty for this layer: no store path,
no backend feature pattern in changed non-Markdown files, and no changed
backend-naming Cargo.toml line. No Rust crate is modified; crate tests and
clippy do not apply. The authorized scope is tooling plus harness tests,
with measurement separately queued. Raw test output is not retained in files.

## Measurement continuation after release

Use `/home/sheng/wt/1098` on `wip/1098-kc-tkrzw-pss` and
`TMPDIR=/home/sheng/tmp/oxpinyin-1098`. Fetch and rebase before publication.
Re-read current instructions and complete the tooling gates before release.

Run `tools/cloud/setup.sh --cells "kc tkrzw"` with task-owned
`OXPINYIN_ORACLE_ENV` and `--cache-dir` under the home disk. Only documented
Rust prerequisites may be installed; setup failure is a STOP. Setup,
compilation and profiling remain subject to serialized heavy admission.

Before measuring, record actual CPU, affinity, memory, kernel, OS, page
size, tool versions and artifact identities. Run `findmnt -T` on every
resolved subject/control/pin library, backend and shared library, data file,
profile file, work directory and TMPDIR; all measured paths must be ext4.
The source-preparation filesystem check does not certify future artifacts.
Use `tools/tkrzw/runtime.sh` for every tkrzw launch and certify the actual
copied objects as well as original installed objects.

Install both shipping facades separately for each backend with its own
`CARGO_TARGET_DIR`. Use `tools/packaging/install.sh`, release and locked
inputs, `kyotocabinet,shipped` or `tkrzw,shipped` for libpinyin and the
matching backend for libzhuyin. Inspect both facades with
`tools/packaging/function_order.py`; successful installation alone does not
prove ordering. Never use an unordered opt-out for a shipping claim.

Diagnose KC pinyin's same-bytes total-Pss control before pin comparison.
Use equal-length arm paths, fresh profile copies, interleaving and one
fresh process per sample. Investigate changing sharing of resolved native
libraries and distinguish clean shared residency from private dirty and
anonymous memory. No cause is established by source inspection alone.
Do not stop unrelated processes. A nonexact/unexplained control must be
reported as invalid under the task instruction; residency acceptance also
requires the section 7.5 paired difference median and entire bootstrap
interval inside the specified band. Do not tune the band after sampling.

After release, collect M0/M1 Rss, Pss and Private_Dirty per mapping for
both facades and `phrases`/`journals`, subject/control/pin in the same run.
Add Massif peak heap per side with exact deterministic controls; distinguish
sampled lifecycle peaks from checkpoint heap. Rank positive M1 Pss
contributors by oxpinyin mapping, backend/shared library or anonymous
owner. Explain different use of the same shared library from source and
valid attribution, and give each contributor a next step without
implementing it.

Each reported figure must carry its exact command and current-run inputs.
All raw observations and profiler streams stay in memory; no archive, log,
capture or evidence file may be created, attached or uploaded. State raw
capture unavailability at the point of use. Publish a findings-only draft
PR after fmt and the documentation gate, without review requests, mentions,
checkboxes, merging or draft-state changes. Author and committer are
Shenghao Chen <shenghaoc@outlook.com>.

## References

- [Issue #1098](https://github.com/shenghaoc/oxpinyin/issues/1098).
- [BDB attribution method](init-attribution-2026-10-22.md).
- [Backend shipping policies and historical controls](backend-function-order-1076.md).
- [Section 7.5](tooling-consolidation-python-just.md#75-metrics-and-statistics).

No current Pss, heap or performance result is available.
