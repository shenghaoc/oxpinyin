# KC and tkrzw Pss attribution (#1098)

## Scope and release condition

Source-only preparation, recorded 2026-10-23 UTC. No provisioning, build,
profiling or measurement has run for this issue. The Ubuntu campaign waits
for the parent to release the heavy-work queue behind #1031's exact-Ir
STOP. #1100 has a separate analysis owner on this host. #1099 follows this
issue sequentially. No historical figure is a current-run baseline.

The deliverable is findings only: both shipping ordered facades on KC and
tkrzw, subject, byte-identical control and pin together on one host. No
production change is authorized. A residency/time trade requires a STOP.

## Source-derived tooling limitation

At source `747b00cd`, `tools/bisection/teardown.py::run` rejects all stages
other than `provision` for KC and tkrzw. `profile_contents` recognizes BDB
containers and otherwise compares file bytes; it does not supply the
backend-aware final-profile comparison these stages require. The
[measurement runbook](../runbooks/benches.md#native-teardown-workload)
explicitly documents the restriction. #1076 enabled profile provisioning,
not confirmation or attribution, on the two peer backends.

Consequently the requested KC/tkrzw `confirm` and `attribute` commands are
not executable through the supported controller as it stands. Do not remove
the guard, label another backend BDB, omit equivalence checks or substitute
raw DB-file equality. A supported controller extension needs resolution
before those stages. This checkpoint changes documentation only.

The existing worker and readers already provide the relevant primitives:
`teardown.py::mapping_group` and `read_memory` read mapping classes and
checkpoint totals from the same smaps observation; the teardown worker
pauses at M0 and M1. `tools/measure-function-order.py::teardown_memory`
reports total Pss from a later smaps_rollup read. Its text-Pss measurement
uses the earlier smaps read. These are different sampling definitions;
report any rollup-versus-mapping-sum difference explicitly rather than
silently substituting one for the other.

## Continuation after release

Use `/home/sheng/wt/1098` on `wip/1098-kc-tkrzw-pss` and
`TMPDIR=/home/sheng/tmp/oxpinyin-1098`. Fetch and rebase before publication.
Re-read current instructions and resolve the tooling limitation above.

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

Once supported, collect M0/M1 Rss, Pss and Private_Dirty per mapping for
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
