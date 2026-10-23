# tkrzw init attribution (#1099)

## Scope and release condition

Source-only preparation, recorded 2026-10-23 UTC. This campaign follows
#1098 sequentially and also waits for the parent release behind #1031's
exact-Ir STOP. No provisioning, build, timing, Callgrind run or scaling
experiment has run for this issue. The deliverable is a separate
findings-only draft PR, followed by STOP. No production change is authorized.

Use `/home/sheng/wt/1099`, branch `wip/1099-tkrzw-init`, and
`TMPDIR=/home/sheng/tmp/oxpinyin-1099`. Fetch and rebase before publication.
After release, run `tools/cloud/setup.sh --cells tkrzw` with task-owned
home-disk cache and oracle environment. Install only documented Rust
prerequisites, and STOP on setup failure.

## Source-derived open paths

The Rust source below was read at `747b00cd`. The pinned source was read
with `git -C /home/sheng/repos/libpinyin show 074a2219:PATH`, rather than
using that checkout's unrelated HEAD. These are source conclusions, not
runtime attribution or proof of the expensive path.

| Role | oxpinyin source item | pin source at 074a2219 |
| --- | --- | --- |
| Hash file, read only | `crates/oxpinyin-store/src/tkrzw/mod.rs::open_hash`: C API `tkrzw_dbm_open`, writable false, `dbm=hash,no_create=true` | `src/storage/ngram_tkrzwdb.cpp:86-99`, `Bigram::attach`: `HashDBM::Open` with flags from `attach_options` |
| Ordered file, read only | same module, `open`: writable false, `dbm=tree,no_create=true` | `src/storage/phrase_large_table3_tkrzwdb.cpp:65-80`, `PhraseLargeTable3::attach`; `src/storage/chewing_large_table2_tkrzwdb.cpp:75-89`, `ChewingLargeTable2::attach`: `TreeDBM::Open` |
| Ordered file, writable | same `open`: writable true, `dbm=tree` | the same attach methods; `src/storage/tkrzwdb_utils.h:32-46` derives writable and `OPEN_NO_CREATE` from attachment flags |
| Private in-memory ordered table | same module, `open_in_memory`: empty path, writable true, `dbm=baby` | constructors and load paths use `BabyDBM`: `src/storage/phrase_large_table3_tkrzwdb.cpp:45-48,83-99`, `src/storage/chewing_large_table2_tkrzwdb.cpp:56-61,92-109` |
| Load user bigram into memory | distinguish from read-only system bigram before attributing | `src/storage/ngram_tkrzwdb.cpp:48-63`: `TinyDBM`, read-only `HashDBM` temporary and `copy_tkrzwdb`; `src/storage/tkrzwdb_utils.h:48-74` iterates every row |

`crates/oxpinyin-data/src/bigram_table.rs::BigramTable::open` uses the
hash-reader seam. Inventory all actual system/user file callers before
assigning init cost to an open or a full-row copy. Do not infer a whole-file
scan, recovery, validation or index rebuild merely from a DBM class name.
The exact provisioned runtime's C wrapper, PolyDBM and selected DBM
implementation must explain any hot function seen after release.

## Existing attribution primitives

`tools/generate-function-order.py::callgrind_prefix` provides a native
worker collection pattern using `--collect-atstart=no`, a single
`--toggle-collect=pinyin_init` or `zhuyin_init`, and
`--callgrind-out-file=/dev/stderr`. Its `collect_toggle` checks tkrzw before
launch. Its function-set reader is for ordering, not inclusive/exclusive
instruction accounting.

`tools/bisection/contract-diff.py::per_call_profile` already parses the
Callgrind stream in memory, retaining exclusive costs keyed by object and
function and inclusive caller/callee edges separately. It rejects an
exclusive-cost sum that differs from the collected total. Inclusive edges
overlap and cannot be summed as a phase partition. Reuse these primitives
and the existing native worker rather than creating a new driver or
saving profiler output. Confirm the command/format combination fits the
reader before a campaign; no such check has run in this preparation.

The teardown controller still rejects tkrzw `confirm`/`attribute` at
`tools/bisection/teardown.py::run`; only `provision` is supported. Its
Callgrind attribution scopes free/fini, not init. It is therefore not an
init-attribution command for this issue, and cannot currently supply the
requested warm timing through its tkrzw confirmation stage. Resolve the
supported tooling route without disabling backend/equivalence guards.

## Continuation after release and #1098

Build both shipping ordered facades from the then-current source with the
locked release recipe through `tools/packaging/install.sh`, a tkrzw target
directory, `tkrzw,shipped` for libpinyin and `tkrzw` for libzhuyin. Inspect
actual objects with `tools/packaging/function_order.py`. Run every tkrzw
launch through `tools/tkrzw/runtime.sh` with the probe-certified runtime,
including copied subject/control/pin objects and workers.

Record actual CPU/affinity, memory, kernel, OS, page size, Valgrind and
compiler versions. Check every measured resolved library, data/profile
file, work directory and TMPDIR with `findmnt -T`; all must be ext4.
No source-preparation observation certifies future artifact placement.
Use the serialized heavy slot; no overlap with #1031, #1098 or another
measurement campaign.

Measure subject and pin together, with an equal-path-length same-bytes
control, both facades, initially one profile. Expand profiles if their init
cost differs by more than 1%. Collect only the init subtree, report raw
exclusive costs and inclusive edges, reconcile exclusive totals, and
require exact same-bytes Ir. Compare totals to #1096 solely as the task's
within-1% reproduction check, never as a current-run pin baseline; STOP if
that check fails. Current source may have changed while waiting, which must
be stated rather than silently accepted as a replacement reference.

Trace the costly functions to actual opens, files, flags and tuning,
including backend defaults and clean-shutdown/recovery state. Analyze
scaling with file size and record count separately: a correlated corpus
alone does not distinguish the two. No altered file, open mode or parameter
becomes a shipping candidate in this task. Keep contracts and tkrzw-to-tkrzw
pin byte compatibility in both directions unchanged.

Warm init wall time requires an interleaved fresh-process same-bytes
control. Under section 7.5, the paired median control/subject ratio and
entire 95% bootstrap interval (4,000 resamples, seed `0x074A2219`) must lie
within [0.95, 1.05]. Otherwise report INCONCLUSIVE and suppress the pin
ratio. Do not aggregate memory-sample timings or whole-process wall time as
init wall time.

If the costly backend work is also paid by the pin, record an upstream
finding, not a divergence. Describe alternatives and compatibility effects
for a ruling; implement none. Every figure needs its exact command and
current inputs. Raw observations and profiler streams stay in RAM. No
archive, log, capture or evidence file may be created, attached or uploaded;
state unavailability at each figure's use. Complete fmt and the docs gate,
publish a separate draft PR without review requests, mentions, checkboxes,
merging or draft-state changes, and STOP. Author and committer are
Shenghao Chen <shenghaoc@outlook.com>.

## References

- [Issue #1099](https://github.com/shenghaoc/oxpinyin/issues/1099).
- [Historical backend figures](backend-function-order-1076.md).
- [BDB attribution methodology](init-attribution-2026-10-22.md).
- [Section 7.5](tooling-consolidation-python-just.md#75-metrics-and-statistics).

Current init function costs, scaling and warm timing remain unknown.
