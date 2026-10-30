# Benches — criterion, the perf baseline, the RSS diagnosis, and profiles

Four tiers, none run by CI (`verify-nightly` has no perf lane): the
criterion benches for a component, the C-ABI perf baseline against the
oracle, the RSS diagnosis that decomposes a resident set rather than
sizing it, and a sampled profile of the keystroke cycle. Numbers are only
comparable within one container on one host; every published snapshot
(`docs/perf/`, `docs/findings/perf-*.md`) names both.

## Verify function ordering before shipping measurements

Packaging links the original layout on a linker without
`--section-ordering-file`, on an explicit opt-out, or for a backend without a
registered checked-in policy; a selected policy that does not
resolve fails the relink (`docs/packaging.md`, "Policy selection by
backend"). Every shipping arm below runs the shared
`tools/packaging/function_order.py` check before measurements. It reads the
linked ELF's SONAME, `.text` start and address-sorted function symbols. Every
policy path must resolve to one clone family, its bodies must be contiguous,
and the groups must form the policy prefix at the start of `.text`. Missing,
ambiguous, out-of-order or stripped symbols refuse the arm. Check the
unstripped linked object before stripping a private copy for size reporting.

Each arm/facade prints a JSON result: `ordered`, `refused`,
`intentional-unordered`, or `reference` for the pinned upstream library. A
refusal names the arm, policy and current compiler driver's linker probe.
That probe is diagnostic; it does not identify a prebuilt object's linker or
substitute for inspecting the object. GNU BFD ld at or above its minimum
(`just doctor` checks it on Linux) and an applicable policy are needed to
build the ordered shipping path. A successful install
and its stderr do not establish that ordering applied.

The measurement inventory (arm names are command-line names):

| Tool | Shipping expectations and build route |
| --- | --- |
| `just measure startup` / `just initbench` | Both facades of native `base`, `pr`, and byte-identical `control`; supplied oxpinyin installed prefixes. Every Linux arm is ordered; pin rows are refused. |
| `tools/measure-function-order.py` | Both facades of `base`, `pr`, `control`; supplied installed prefixes. `pin` is the reference. |
| `tools/bisection/contract-diff.py --teardown` / `teardown.py` | Selected facades of `ox` and its byte-identical `control`; supplied libraries, copied then checked before building a worker. `pin` is the reference. This includes the provision stage. |
| `tools/bisection/contract-diff.py --per-call` | Selected facades of `subject` and optional `base`; supplied libraries. `pin` is the reference. Ordinary behavioral differentials have no shipping-layout requirement. |
| `just measure snapshot` | `shipped/pinyin`; calls `install.sh`, hence `relink-versioned.sh`, then checks before strip. The separate `alloc-count` artifact is a fixture. This is the only measurement caller of those packaging scripts in this tree. |
| `just measure rss-diag` | `S/pinyin`; supplied shipping library. `L` is the reference. |
| `just measure system-import-profile` | `ox/pinyin`; supplied library for the import/save and cold-open profiles. `pin` is the reference. Ordering options follow the optional workload argument. |
| `just measure perf-baseline` | `ox/pinyin`; reusable installed stage or bare `cargo cinstall` stage. `pin` is the reference. |
| `just measure perf-matrix` | `C/pinyin`, `D/pinyin`; installed container libraries. `A`, `B` are references. |
| `just measure perf-same-data` | `C/pinyin`, `D/pinyin`; supplied libraries or legacy `cargo build` fixtures. `A`, `B` are references. |
| `just measure shipped-check` | `Z/pinyin`, `S/pinyin`; legacy bare `cargo cinstall` comparison, whose `S` description claims the product. `L` is the reference. |
| `just measure tree-recipe-control` | `X/pinyin`, `Y/pinyin`, `Z/pinyin`, `S/pinyin`; historical cargo-build/cinstall recipe comparison. `L` is the reference. |

These measurement and profile tools were shell runners under `tools/bisection/`,
`tools/perf-gate/`, `tools/env-probe/` and `tools/profile/`; each is now a
recipe of `python -m tools.dev measure RECIPE [ARGS]` (`just measure RECIPE`; the
measurement images have no `just` and run the `uv run` form). Their environment
variables, options, output files and exit statuses are the shell runners' (the
recorded differences are in `docs/plans/tooling-python.md`, unit U2), and a
diagnostic still carries the old script name:

| Recipe | Formerly |
| --- | --- |
| `perf-baseline` | `tools/bisection/run-perf-baseline.sh` |
| `perf-matrix` | `tools/bisection/run-perf-matrix.sh` |
| `perf-same-data` | `tools/bisection/run-perf-same-data.sh` |
| `shipped-check` | `tools/bisection/run-shipped-check.sh` |
| `tree-recipe-control` | `tools/bisection/run-tree-recipe-control.sh` |
| `rss-diag` | `tools/bisection/run-rss-diag.sh` |
| `callgrind-differential` | `tools/bisection/run-callgrind-differential.sh` |
| `system-import-profile` | `tools/bisection/run-system-import-profile.sh` |
| `snapshot` | `tools/perf-gate/snapshot.sh` |
| `probe-debuginfo-neutrality` | `tools/env-probe/probe-debuginfo-neutrality.sh` |
| `probe-egress` | `tools/env-probe/probe-egress.sh` |
| `just profile-w8` | `tools/profile/run-w8-cycle.sh` |

The sampling loops are serial and interleave the arms one process at a time,
because a timed or resident-set sample belongs to its arm only if nothing runs
beside it. The builds are not samples: `shipped-check` and `tree-recipe-control`
build their independent artifacts as units of one admitted pool.

### Base, PR and pin steady lookup comparison

`steady-comparison` uses the existing `bisect --perf` workload and worker.
It compares supplied ordered shipping libraries in one job on one Linux host,
rotating base, PR, pin and byte-identical base/PR controls serially. It also
rotates separate base/PR `alloc-count` fixtures. Build those fixtures with
`cargo build --locked --release -p oxpinyin-capi --no-default-features
--features <backend>,alloc-count`, through `just heavy`; they are never
installed or reported as shipping artifacts.

```bash
just heavy -- just measure steady-comparison --cell bdb \
  --base "$BASE_SO" --pr "$PR_SO" --pin "$PIN_SO" \
  --base-alloc "$BASE_ALLOC_SO" --pr-alloc "$PR_ALLOC_SO" \
  --system-dir "$DATA" --out "$WORK/steady"
```

All libraries, data, work and `TMPDIR` must be on ext4. Each process performs
eight cycles by default; the first is warmup and is excluded from every
reported metric. Twenty native rounds report medians and ranges of time per
steady cycle. Three allocation rounds report the requested Rust allocation
count and bytes per cycle, excluding DBM/libc allocations; pin allocation
counts are unavailable. Two Callgrind rounds report total instructions per
cycle under the existing `bisect_perf_steady_unit` anchor, including external
library work. Ir ranges are retained, never relabelled exact when controls
vary. Compare the base/PR effect with the same-bytes control spread; overlapping
ranges do not establish an instruction reduction.

The worker's return/count checksum must be stable and agree between base,
PR, controls and allocation fixtures. The pin checksum comparison is reported
separately; full observable equivalence belongs to the contract differentials.
This is a whole lookup workload, not an isolated measurement of a particular
internal search. In particular, a workload without carried user entries
does not establish a speedup for that rare edit path.

Legacy recipes do not gain the shipping relink merely by enabling `shipped`.
They now refuse unordered artifacts by default. To reproduce an intentional
original-layout experiment, pass `--unordered-arm ARM` for each such arm,
for example `--unordered-arm C --unordered-arm D` for the same-data runner's
cargo-build fixtures. Those rows are explicitly non-shipping experiments.
Do not report their numbers as shipping figures. Environment variables and
swallowed build warnings are not opt-outs.

All these tools accept repeated `--function-order ARM/FACADE=POLICY` overrides
for matched historical or experimental policies, for example
`--function-order base/pinyin=/path/to/base/libpinyin.order`. Defaults are the
current checked-in policies in `function-order/<backend>/`, with the backend
derived from each object's `DT_NEEDED`, independently of build environment
overrides. A named policy in another backend's directory is refused, including
through a symlink. Name `base` explicitly with `--unordered-arm base` when the
function-order campaign compares an intentionally unordered baseline. Its
same-bytes control is still checked as its own arm; specify that arm's policy
or unordered experiment too. Unknown/repeated names or conflicting
declarations fail. A check can run alone without starting a campaign:

```sh
uv run --no-project --managed-python \
  python -X lazy_imports=normal tools/packaging/function_order.py \
  --arm subject --library "pinyin=$SUBJECT_PREFIX/lib/libpinyin.so.15.0.0"
```

`tools/generate-function-order.py` intentionally profiles unordered inputs to
derive policies; its output is a policy, not shipping performance figures.
`just profile-w8` uses the thin-LTO `profiling` recipe and makes
no shipping-performance claim. `tools/workers/startup.c` is the native worker
for `tools.dev.measure.startup`. Packaging's
`release-stage.sh` and `check-pc-metadata.sh` call `install.sh` for release or
behavioral smoke checks, not measurements. Their ordinary fallback stays
unchanged.

## Callgrind debug-info control

Both campaign arms use the unchanged shipping Cargo profile. The profiled arm
adds `RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"`; Cargo appends these after its
`-C strip=debuginfo`, so rustc's final strip setting retains DWARF without changing
the profile/metadata hash. On Rust 1.97.1 its `.text` is byte-identical to plain
shipping, including `Step::relax`, `order_window_tail` and both engine
`reserve_rehash` instances. Debuginfo alone is stripped back out.

`callgrind-control` verifies DWARF compilation units in the profiled library and
runs the unchanged neutrality check against the separately built plain shipping
library. The campaign checks neutrality again before collection and uses the
plain shipping arm for wall-clock timing.

To check preparation natively without running the pin or collecting profiles:

```bash
just heavy -- env RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none" CARGO_TARGET_DIR=target/tkrzw \
  cargo cinstall -v --locked --release -p oxpinyin-capi \
  --no-default-features --features tkrzw \
  --destdir=target/callgrind-tkrzw/profile --prefix=/usr
just heavy -- env CARGO_TARGET_DIR=target/tkrzw \
  cargo cinstall -v --locked --release -p oxpinyin-capi \
  --no-default-features --features tkrzw \
  --destdir=target/callgrind-tkrzw/control --prefix=/usr
just measure callgrind-control target/callgrind-tkrzw/profile target/callgrind-tkrzw/control
```

Run from a shell with no Cargo profile or RUSTFLAGS overrides already exported.
For BDB, replace `tkrzw` with `bdb` in the feature and output paths. Both
comparisons must pass; these builds should be case (a), identical `.text`.
No runtime data or oracle build is needed for this check. The verbose build
output records the order of the compiler flags; `readelf --debug-dump=info`
on the profiled library displays its DWARF units.

### Sentence and typing-sequence Callgrind campaign

`just measure callgrind-differential --workload sentence|typing-sequence`
extends the existing `bisect` driver (#1348). Its no-argument campaign and
`bisect --perf` retain their original 20-input workload. The frozen
`system-import-profile --sentence-fixture` mode still requires its 500-row
fixture; it is not a general prefix-sequence driver.

Both new workloads use the pinyin facade, options `0x18a`, an empty temporary
user directory per process, and the same instance throughout each iteration.
They never train or save. The default input is `nihaozhongguo`.

| Workload | Setup outside collection | Collected anchor |
| --- | --- | --- |
| `sentence` | Reset and parse the complete input | `bisect_sentence_unit`: one `pinyin_guess_sentence` |
| `typing-sequence` | One reset before the sequence | `bisect_typing_unit`: parse, `pinyin_guess_sentence`, then `pinyin_guess_candidates` at offset 0, sort `0x1e`, for each accumulated ASCII prefix |

The sentence input/configuration are those of stage2's
`guess_sentence_get_sentence_0/full_nbest_post_116`. Its collection measures
sentence guessing alone: rank-0 retrieval and release happen afterwards.
Typing collects all 13 prefixes, from `n` through `nihaozhongguo`, in order.
Output records each operation's returns, rank-0 sentence, and every typing
candidate's text/type/N-best index in physical order. Observation, printing,
init, instance allocation and teardown are outside collection.

Supply actual library files, a complete backend-matched system directory,
and separately built plain shipping files for the two debug-info neutrality
checks. Use the shipping Cargo profile and only
`RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"` for the profiled files. Install through
`just packaging install` to apply the shipping function order; the bare
`cargo cinstall` commands above check codegen neutrality only. The campaign
checks `DT_NEEDED` against `--cell`, function order, DWARF, code neutrality,
and the sound-tkrzw runtime before collecting. It acquires the heavy slot
for the harness build and all serial collections; do not overlap other work.

```bash
just measure callgrind-differential --workload sentence --cell tkrzw \
  --main "$MAIN_PROFILE_SO" --pr "$PR_PROFILE_SO" --pin "$PIN_SO" \
  --main-shipping "$MAIN_SHIPPING_SO" --pr-shipping "$PR_SHIPPING_SO" \
  --system-dir "$DATA" --out "$WORK/sentence"
just measure callgrind-differential --workload typing-sequence --cell tkrzw \
  --main "$MAIN_PROFILE_SO" --pr "$PR_PROFILE_SO" --pin "$PIN_SO" \
  --main-shipping "$MAIN_SHIPPING_SO" --pr-shipping "$PR_SHIPPING_SO" \
  --system-dir "$DATA" --out "$WORK/typing-sequence"
```

For BDB use `--cell bdb` and BDB libraries/data. For the tooling smoke check,
set main and PR to the same profiled file and both shipping arguments to
the same plain file. `--input` accepts lowercase ASCII pinyin/apostrophes;
`--iterations` defaults to 1, `--rounds` to 3 fresh processes per arm (minimum
2), and `--cpu` to the first allowed CPU. No warmup is excluded: these are
first-call/sequence scopes, not the legacy steady cycle.

Every round rotates main, PR, pin and byte-identical copies of main and PR
serially, using equal-length private library paths. A control passes only
when all instruction totals in its identical pair are equal, with zero
tolerance, and full observations agree. Main/PR observations must also agree.
The pin observations are compared and reported separately; this campaign
does not classify known pin divergences or replace the ranking/recount gates.
Profiler creator/PID/command/Ir-only events and reconciled exclusive totals
are required. Invalid controls, observations or collections return nonzero
and suppress the per-invocation comparison. The startup RandomState range
exception is not extended to the sentence workload. The typing sequence
alone reports a varying control as `DIAGNOSTIC` (rule `randomstate`, ranges,
no tolerance) when every varying function is `dedup_by_text_keep_first`,
`dedup_ranked_by_text`, `order_window_tail` (std `HashSet`/`HashMap` with
the per-process `RandomState` SipHash-1-3 keys) or `memcmp`, their equality
callee; any other varying function is `INVALID`.

The output directory must be new. `result.json` reports raw counts, control
spread and Ir per sentence/complete typing sequence; `environment.json`
records the host, inputs and library identities. Exact commands, worker
output and profiles remain local for attribution with
`callgrind_annotate --inclusive=yes`. They are disposable verification data,
not PR attachments. Harness `elapsed_ns` is collected under Valgrind and is
not a native timing result. These commands make no wall-time or heap claim.

As in other measurement recipes, explicit `--function-order ARM/pinyin=POLICY`
and `--unordered-arm ARM` options label layout experiments. Arm names are
`main`, `pr__`, `mctl`, `pctl`, and the exempt reference `pin_`. An unordered
smoke run validates tooling only and cannot establish shipping performance.

The separate `probe-debuginfo-neutrality` diagnostic still compares independently
compiled profiles and may fail. Changing Cargo's debug profile changes crate
metadata, which can affect fat-LTO specialization as well as layout (#1337).
That diagnostic does not create the campaign's control artifact.

## Native startup measurement

The 2026-10-22 UTC ruling (§7.8 in the tooling consolidation findings) places
oxpinyin-only startup in `just measure startup`, also available as
`just initbench`. Supply installed unstripped oxpinyin base and PR prefixes,
one system directory and pin-provisioned profiles from
[Regeneration](../packaging.md#regeneration). Native measurement only verifies
these inputs; populated profiles need nonempty `witness.txt` files.

```sh
TMPDIR="$HOME/owned-startup-tmp" just initbench \
  --base "$BASE" --pr "$PR" --system-dir "$DATA" --profiles-dir "$PROFILES"
```

Create the owned temporary parent first. Linux requires ext4 for every
library, system file, profile and work path; macOS requires one APFS volume.
The controller acquires `tools.dev.slot` for its worker build and whole run.
It uses private, equal-length base/PR/control library paths and fresh user
copies, checks each Linux arm with the shared function-order verifier, and
removes temporary inputs even after failure. It writes no profiler capture.
On macOS it reports `function-order` as `unavailable` and measures the
supplied dylibs as they are; the ordered layout is applied and verified when
the prefix is installed ([Mach-O order file](../packaging.md#mach-o-order-file-macos)),
so a macOS base built with `OXPINYIN_FUNCTION_ORDER_POLICY=` is cargo-c's
unordered object and a PR built without it is the ordered relink.
Do not overlap measurement with builds; use the existing host-wide slot.
The PR control is always a byte-for-byte copy. Explicit policy overrides
use `--function-order ARM/FACADE=POLICY`; there is no unordered bypass.

Native verification compares U1–U6, the full script, witness answers,
save/reopen observations, stderr, the user filename set and raw non-database
file bytes. It never reads database rows. Python hashes the worker's exact
canonical U1–U6 record including the final newline. The parent reads stopped
child memory at every checkpoint. Linux reads `/proc`; macOS reads the shared
Darwin libproc binding and reports Darwin-named fields: resident size and its
peak, physical footprint and its interval/lifetime maxima, faults, page-ins,
disk bytes read and written, and the host instruction and cycle counters.
Memory sizes are KiB and counts stay counts; Linux-named fields stay
unavailable on macOS, and Linux output is unchanged. The counters are
`instructions` and `cycles`, each with
`instructions_available` and `cycles_available`; they are raw XNU
`rusage_info_v4` counts, not Valgrind Ir, and are never written to a
Linux-named field. XNU fills them only when `CONFIG_PERVASIVE_CPI` is built
and the host backs them; otherwise they are zero with no separate marker.
Before trusting either one, the adapter probes the host by requiring the
counter to advance across real work and a forced context switch. A counter
that does not advance is reported unavailable, never as zero. The worker
reports its own resident peak at each handshake; a controller-side task-port
read is only a fallback. A value neither source provides is unavailable, never
zero. Memory-run timings are excluded.

`--stage verify` stops after verification and separate hostile probes.
`--stage instrument` also checks Linux Callgrind Ir. Init uses two fresh
processes per arm. The [2026-10-23 11:04 UTC ruling](../findings/init-gap-1090-2026-10-23.md#rulings)
identifies BDB init Ir as nondeterministic because libdb seeds `srandom_r`
from PID, time and address. When every arm's checked `DT_NEEDED` backend is
BDB, init reports each base/PR/control arm's process min–max as `bounds`, all
original `counts` and raw profiles, with verdict `DIAGNOSTIC`, then continues.
Non-BDB and unknown init backends retain the exact rule: any same-bytes
variation returns 3 immediately. No tolerance is introduced.
Under the 2026-10-24 UTC maintainer ruling (§7.5), first-candidate and steady
Ir use five fresh processes per base/PR/control arm. Steady repeats the same
candidate request on the initialized instance, in a separate profiler process.
The control and PR min–max ranges must overlap for range qualification. The further 2026-10-24 UTC
ruling removes the span limit entirely. Claims are bounds: base min minus
PR max through base max minus PR min, never a point or exact difference. Every PR/control exclusive function count is checked
against the single `tools.dev.measure.instructions::RANDOMSTATE_ALLOWLIST`
definition. Under the additional 2026-10-24 UTC attribution ruling, init
does not require complete function naming. Its exact and BDB diagnostic
paths both validate creator, PID, command, Ir-only events and reconciled totals.
For range metrics, unresolved sites use full object identity plus an offset
from the object's load base, never raw ASLR addresses. The same-process worker
reports loader PT_LOAD segments and load biases via `dl_iterate_phdr` after
the units. The lowest runtime PT_LOAD address is the base; each unresolved
object-relative instruction position plus its recorded load bias must map uniquely into an executable segment.
Constant unresolved entries present in every PR/control process are allowed
and reported by count and summed Ir. Varying or unmatched unresolved entries, missing
or ambiguous loader mappings, other named variation, profiler identity
failure, or unreconciled exclusive totals are INVALID (status 3), including
for diagnostics. The RandomState exception applies only to first-candidate
and steady scopes with variation confined to that allowlist: a differing
same-bytes control reports each arm's min–max `bounds`, every original count,
and verdict `DIAGNOSTIC`, then continues even when the ranges do not overlap.
This is diagnostic output, not a qualified comparison. It does not relax
deterministic scopes or the profiler creator/PID/command/events checks.
Raw loader
records and profiler instruction positions remain in the output. The
[Callgrind format](https://valgrind.org/docs/manual/cl-format.html#cl-format.reference.header)
specifies object-relative instruction positions for identified objects; the
loader bias is added before checking the executable segment. Callgrind's
anonymous `???` object has zero bias (`fn.c::new_obj_node`), and
`dump.c::get_debug_pos` emits runtime address minus that bias. Its instruction
position is therefore checked directly against same-process executable
segments. In both cases the uniquely proven object's load base is subtracted
to form the key. Unknown object names alone do not cause rejection; missing
or ambiguous mappings still do. Failed parsing retains the sample PID, raw
stdout/profile bytes and loader records in the INVALID report, even when no
parsed function table exists. No earlier failure is retroactively attributed.
Qualified comparisons report RANGE-QUALIFIED and their bounds, never exact;
the diagnostic cases above report DIAGNOSTIC.
Every original timing sample, scoped total, per-function count and raw
profile is reported; no normalization, hash reseeding or retrospective
attribution of earlier phase-total-only differences is allowed.
Re-evaluate retained controller JSONL before repeating covered scopes:

```sh
just measure startup --recheck-instructions "$RETAINED_JSONL"
# Then use the original initbench inputs and selection, adding:
just initbench --base "$BASE" --pr "$PR" --system-dir "$DATA" \
  --profiles-dir "$PROFILES" --instructions-from "$RETAINED_JSONL"
```

Replay requires one original start record, complete raw init profiles and
complete range profiles with raw data, PIDs and loader records. It reparses
originals and reports all counts and the new bounds; duplicate scopes/range
PIDs or incomplete provenance refuse. Continuation additionally matches input
digests, library hashes, worker source, binding and cache mode. Each qualified
or validated diagnostic retained scope is skipped, while uncovered scopes
(including zhuyin) still run. BDB init diagnostics require the recorded backend
and the current checked backends to identify BDB; unknown backends stay exact.
Fresh behavioral verification is a preflight; subsequent timing/memory are a
separate measurement segment linked by the retained file SHA-256. Retained
and new samples are never pooled or claimed as one original run.

The default `--stage run` checks these counts before timing. Native macOS
instruction counts are unavailable. Valgrind output stays in memory.
The default run has 60 timing rounds plus 3 warm-ups and 30 memory rounds
plus 2 warm-ups (`--bind lazy` defaults to 30 timing rounds). Subjects rotate
within each round. `--rounds`, `--warmups`, `--memory-rounds` and
`--memory-warmups` permit an explicitly reported smoke selection.

Timing and residency use the 2026-10-21 UTC control rules and shared
`tools.dev.measure.stats` helpers. An unstable metric is INCONCLUSIVE and
gets no PR/base ratio. Fault counts are diagnostic, not residency ratios.
The statuses are 0 completed (including INCONCLUSIVE), 1 failure/refusal,
2 usage error, 3 NOT COMPARABLE or INVALID exact control, 130 cancelled.
Pin rows require the Layer 4 container path; Windows stays deferred (#950).
`--cache cold` runs on native macOS (APFS) and native Linux (ext4); any other
host, and on Linux any input or work file off ext4, refuses with status 1.
Maintainer ruling (2026-10-25
UTC, recorded 2026-10-25 17:53 UTC): "the shared cold-mode rejection limit is 2
rejected samples per arm and metric. A metric with more than 2 rejected samples
is INCONCLUSIVE. The limit is a fixed constant and is never derived from or
tuned against campaign output."

On macOS the controller prepares every timing and memory sample through
`tools.dev.measure.cold` ([APFS cold preparation](#apfs-cold-preparation))
after the user copy is made and before the worker is spawned. A set still
resident after the second preparation is rejected. Its `sample` record carries
the rejection (`cold.verdict` `REJECTED`) and no worker output. The sample is
excluded, never pooled and never rerun, so `--rounds` and `--warmups` still
count attempted rounds. Every metric report in a cold run carries `cold`: the
limit (`REJECTION_LIMIT` in `tools/dev/measure/cold.py`, the one constant
both hosts read), the accepted and rejected counts of the measured samples
per arm, and the rejected warm-ups, which never count toward the limit. A
metric with more than 2 rejected samples in any arm, or with no round in which
every arm was accepted, is INCONCLUSIVE with no summaries or ratio. Any other
metric follows the usual rules over the rounds in which every arm was accepted,
so paired ratios and controls compare like with like. Status stays 0. The
Darwin metric is `darwin_resident_pages`; it appears in `cold` and
`page-residency` records, never in a Linux-named field.

On Linux the controller prepares every timing and memory sample through
`tools.dev.measure.cold_linux` ([Linux cold preparation](#linux-cold-preparation))
at the same point, and rejects, counts and pools exactly as above under the
same limit of 2 per arm and metric. The Linux metric is
`linux_resident_pages`; it is never written to the Darwin field.

Linux KC smoke acceptance completed on 2026-10-24 UTC at 08:23 with controller
`1cea405cf297e68da8ca3caf97594de45f467ef8` and retained shipping build
`ebc9d4f5bf3edc6d08f254de39a6db46bb75c9b3`, not moving main. This was a
retained-profile recheck followed by a separate continuation, both exit 0.
`$INPUT` is the directory that held the retained shipping build's installed
prefix (`stage`) and the provisioned profiles (`provision/profiles`).
`$RETAINED` is the retained controller output (`measurements.jsonl`) of the
earlier profiler segment. `$DATA` is the `lib/libpinyin/data` directory of the
Kyoto Cabinet oracle prefix. `$TMPDIR` is an owned temporary parent directory on
ext4 for the continuation's work files:

```sh
just measure startup --recheck-instructions "$RETAINED"
TMPDIR="$TMPDIR" \
  just initbench --base "$INPUT/stage" --pr "$INPUT/stage" \
  --system-dir "$DATA" \
  --profiles-dir "$INPUT/provision/profiles" --profiles empty \
  --rounds 3 --warmups 1 --memory-rounds 2 --memory-warmups 1 \
  --instructions-from "$RETAINED"
```

The continuation skipped all three covered pinyin profiler scopes. Init was
exact across base/PR/control (two processes each): pinyin 6,922,434 Ir;
zhuyin 3,472,152 Ir. All four first/steady metrics were RANGE-QUALIFIED:

| Facade | Scope | Base minus PR bounds (Ir) |
| --- | --- | --- |
| pinyin | first candidate | −187 to 95 |
| pinyin | steady | −468 to 341 |
| zhuyin | first candidate | −166 to 132 |
| zhuyin | steady | −375 to 149 |

Every range overlapped its control; no named variation outside the allowlist
or invalid unresolved entry was found. Each facade's first-candidate scope
had 58 constant unresolved sites totalling 3,006 Ir; steady had 39 / 1,874 Ir.
Both facade observations and digests were COMPARABLE, all 72 hostile probes
returned 0, and all six arms passed ordering (326 pinyin / 306 zhuyin paths).
The continuation reported 42 original timing/memory samples including warmups.
All 20 timing metrics were INCONCLUSIVE with ratios withheld: no timing gain
is established. Memory: pinyin 51 STABLE, five INCONCLUSIVE and 14 diagnostic;
zhuyin 56 STABLE and 14 diagnostic. Pinyin's inconclusive metrics were
lib_load RSS, RSS delta, peak RSS, file-backed and end file-backed.
Child RSS and virtual size were positive at all seven handshakes.
Raw records remain local and are not published. These segments do not prove
one uninterrupted session or retrospectively attribute earlier failed captures.
BDB instruction counts are diagnostic only
(`libdb` seeds `srandom_r`); they are not reproducible exact controls.
The maintainer reports bounded native macOS checks at `d4e9f54d`: initbench
returned 0 with ELF ordering unavailable, four refusals returned 1, and
parent memory reads were nonzero at all seven child handshakes. The macOS
reader uses `ps` for RSS and virtual size (§7.7).
[CI run 39892909515](https://github.com/shenghaoc/oxpinyin/actions/runs/39892909515)
is green at that head. Unit tests and verification are not performance results.

### APFS cold preparation

`tools.dev.measure.cold` prepares the native controller's macOS sample inputs
cold. For the copied facade, every system-data file and every regular file in
the sample's private user copy, it makes a read-only `MAP_SHARED` mapping,
touches one byte per host page, and calls `msync(MS_SYNC | MS_INVALIDATE)`
before the worker is spawned. It then checks the whole set with `mincore` over
`PROT_NONE` mappings. A resident page gets one more full preparation and
check; a set still resident is rejected. The Darwin metric is
`darwin_resident_pages`; zero qualifies. It is never written to a Linux
cold-page field.

Qualification on 2026-10-25 UTC on macOS 27.0.1 (build 26A434), arm64, APFS,
16 KiB pages, with a 6.3 MB database file, a 2.9 MB Mach-O dylib and a 2 MiB
user file:

| file | bytes | pages | resident before | after `MS_SYNC` alone | after `MS_SYNC + MS_INVALIDATE` |
| --- | ---: | ---: | ---: | ---: | ---: |
| `data.db` | 6,302,040 | 385 | 385 | 385 | 0 |
| `libpinyin.dylib` | 2,968,416 | 182 | 182 | 182 | 0 |
| `user.db` | 2,097,152 | 128 | 128 | 128 | 0 |
| whole set | 11,397,608 | 695 | — | — | 0 (one preparation, no retry) |

"Resident before" is after every page was touched and before `msync`.
`msync(MS_SYNC)` alone left every page resident; adding `MS_INVALIDATE`
dropped all of them, and `mincore` reported the drop. The whole-set
preparation then qualified on its first attempt.

```sh
just cold-qualify /tmp/cold-qualify
```

`cold-qualify` is read-only: it maps and touches the named files and runs no
worker. `--cache cold` on macOS prepares each sample's set the same way and
applies the rejection limit above; the native controller's sample records carry
the preparation record and metric. Files the sample does not own (libc, backend
dylibs) stay out of the set, matching #1162's Linux scope.

### Linux cold preparation

`tools.dev.measure.cold_linux` prepares the native controller's Linux sample
inputs cold, with the record shape of the APFS preparation. It first
`fsync`s every regular file in the sample's private user copy, which was
written just before the sample and whose dirty pages `POSIX_FADV_DONTNEED`
would leave. It then evicts the copied facade, every system-data file and
every user-copy file with `posix_fadvise(POSIX_FADV_DONTNEED)` and, after the
whole set is evicted, checks it with `mincore` over `PROT_NONE` mappings
through `tools.dev.mman`, the one `ctypes` binding of `mmap`, `munmap` and
`mincore` that the APFS preparation also uses (Python has no `mincore`). A resident page
gets one more eviction and check; a set still resident is rejected. Every
file must be on ext4. Since Linux 5.2, `mincore` reports a file the caller
cannot write as wholly resident, so a permission gap rejects samples instead
of qualifying them. Files the sample does not own (libc, libstdc++, backend
libraries) stay warm. Cold campaigns evict only named files; run them when
no other measurement lane shares the host's page cache.

### Processor Trace check (macOS)

On an Apple M4 or newer Mac with Xcode 16.3 or newer, the Processor Trace
instrument records every function call of one process from the CPU's branch
trace. `just measure processor-trace` compares one startup's calls inside a
facade dylib with the Mach-O order file the install derived
([Mach-O order file](../packaging.md#mach-o-order-file-macos)): the
functions executed and ordered, by policy tier; the executed functions the
policy does not name, could not resolve, or names under another
instantiating crate; and the ordered init-tier functions the trace did not
execute. It changes no policy; its lists are proposals. Record the startup
worker (`tools/workers/startup.c`, built as the controller builds it) in its
`stop` mode at `init` or `first_candidate`, the policy's startup scope, then
read the recording:

```sh
cc -std=gnu11 -Wall -Wextra -Werror -O2 -g -o "$WORK/startup" tools/workers/startup.c
EXPORTS=$(grep -o 'pinyin_[A-Za-z0-9_]*[[:space:]]*;' crates/oxpinyin-capi/libpinyin.ver | sed 's/[[:space:]]*;//' | paste -sd, -)
xcrun xctrace record --template 'Processor Trace' --output "$WORK/init.trace" --no-prompt \
  --env "OXPINYIN_STARTUP_EXPORTS=$EXPORTS" --launch -- "$WORK/startup" stop pinyin \
  "$PREFIX/lib/libpinyin.15.0.0.dylib" "$DATA" "$WORK/user" now first_candidate
just measure processor-trace --trace "$WORK/init.trace" \
  --library "$PREFIX/lib/libpinyin.15.0.0.dylib" --policy tools/packaging/function-order/bdb/libpinyin.order
```

`$WORK/user` is a fresh copy of the empty profile's `user` directory. The
recipe exports the intervals table with `xctrace export` into a temporary
directory (`--export-dir` keeps it, `--intervals` reads an existing export),
attributes each call's Mach-O vm offset to the dylib's `__TEXT,__text`
symbol with llvm-nm, and prints one JSON record. A Processor Trace holds a
whole process, so the record also counts the binaries seen; only the named
library is compared. Nothing is archived.

## Native teardown workload

`contract-diff.py --teardown` adds one workload to the existing driver. It
builds `bisect.c` with its `teardown.h` mode; no external initbench is used.
Python provisions private profiles, interleaves subjects and reads the paused
native child's memory. ABI calls are bracketed in C. Other bisect and contract
modes keep their existing output.

All stages (`provision`, `verify`, `pilot`, `confirm`, `repeat`, `multi`,
`attribute`, `init-attribute`) support the selected cell, BDB, KC or tkrzw. Provisioning writes
startup profiles through that cell's own pin. Comparison stages retain
backend-aware final-file and reopen equivalence before any ratios. KC user
indexes and user bigram are snapshot streams; tkrzw indexes are TreeDBM
and its user bigram is HashDBM. The existing contract worker reads every
logical row (empty values included), sorts by key and applies only the
registered index-padding normalization. Other files remain byte comparisons
with the existing registered chunk-header timestamp normalization. BDB's
container detection and db_dump comparison are unchanged.

Peer container reads use a short-lived mode of the existing contract worker,
not a backend CDLL retained in the measurement controller. Every native
teardown launch, fixture-population launch and peer comparison worker checks
the actual copied library through `tools/tkrzw/runtime.py`. The tkrzw reader
loads the exact certified resolution. A load, iterator, read, close or runtime
failure refuses the comparison; an unreadable DB is never an empty DB.

Run inside the selected pinned Linux amd64 cell, after installing both
shipping libraries with
`just packaging install`. Use the existing container reservation and heavy
admission paths. Keep one campaign active. The work directory and measured
library copies must be on ext4; tmpfs gives misleading private-dirty
attribution for clean library pages.

```bash
teardown_work=$(mktemp -d "$PWD/target/teardown.XXXXXX")
trap 'rm -rf "$teardown_work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
just heavy -- uv run --no-project --managed-python \
  python -B -X lazy_imports=normal tools/bisection/contract-diff.py \
  bdb "$pin_prefix" "$shipping_prefix/lib/libpinyin.so" \
  --zhuyin-so "$shipping_prefix/lib/libzhuyin.so" \
  --teardown --teardown-stage verify --teardown-work-dir "$teardown_work"
```

The host's `python -m tools.dev slot run --` is the same admission entry used
by `just heavy`. After verification, select `pilot`, then `confirm` explicitly.
The latter uses 60 timing rounds plus three warm-ups and 30 memory rounds
plus two warm-ups. `--teardown-cases` selects named scenarios; the defaults are
`empty`, `modest`, `phrases`, `journals`, `modest-unsaved`, `modest-saved`.
Every sample receives a private starting profile. A transcript, diagnostic,
final-file or reopen mismatch is `NOT COMPARABLE` and stops before ratios.
Final containers use the existing contract driver's logical row and known
padding rules; random file IDs/LSNs and backend allocation layout are not
profile contents. Every file and empty row remains compared. Raw starting/final
profile identities are recorded
separately. Dirty imports use romanized readings for pinyin and direct bopomofo
for zhuyin, as their pinned importer contracts require.

`--teardown-cache warm` is the default. On native Linux,
`--teardown-cache cold` uses the same `pilot`/`confirm` rounds and rotating
subject/control/pin order, including the discarded initial rounds. Before
each native timing or memory sample, the controller enumerates the facade's
`DT_NEEDED` closure with `readelf` and `ldd`, then evicts the facade, all
non-C-runtime dependencies, every system data file and the private restored
user files. glibc, its loader and `libgcc_s` stay warm, for every side:
the controller's own interpreter maps `libgcc_s`, so its pages cannot be
evicted (#1091). libstdc++, libdb, GLib and their non-C-runtime
dependencies are included when needed. Each file must
be on ext4. Only these files are flushed and advised with
`posix_fadvise(fd, 0, 0, POSIX_FADV_DONTNEED)`; no root or kernel changes are
used. The source profile is never evicted.

Each `cache-eviction` record names every file and its filesystem, with
`mincore` total/resident page counts before and after advice and a final
whole-set check immediately before launching the worker. Any resident page
refuses the sample and names the file, including libraries mapped by another
process. Linux may also conservatively report residency when mincore access
is restricted; that refuses the sample too. Do not treat a refusal as cold
measurement evidence. Cold memory samples also read every cold file's
resident pages with the same `mincore` at each checkpoint, and a `residency`
record gives their per-file summaries at M0 (after load, before init) through
M3; this reads no file bytes. The same readings keep each file's `mincore`
vector, and a `page-residency` record per pair of neighbouring checkpoints
names the pages that became resident between them (see "Per-page residency
attribution"). Equivalence/provisioning observations stay warm;
`verify` performs no cold samples. Cold mode refuses profiling and repeated
in-process lifecycles. Run cold campaigns only when other measurement lanes
are idle and the heavy slot is available: dependency and system-data cache
pages can be shared across lanes.

Save, each instance free, all frees, fini and dlclose have separate wall/CPU
times. `teardown_total_ns` sums the free-call and fini durations. The full
lifecycle guardrail sums native load, init-through-fini and dlclose intervals;
it excludes result formatting and process launch. Memory samples are never
aggregated as timing samples. M0 is after load, M1 after workload/optional
save, M2 after every instance is freed, M3 after fini returns. The process
waits alive for parent observation. Report signed M1−M2, M2−M3 and M3−M0;
never use VmHWM differences or label retained library pages as context leaks.
Totals and mapping groups come from one smaps read; Pss is the sum of its
per-mapping rounded KiB values. A later smaps_rollup read and its signed gap are
diagnostics: shared-page classification may change between sequential reads,
even while the native child waits, so equality is not a sample gate.

The same-bytes control is mandatory (§7.5 of
`docs/findings/tooling-consolidation-python-just.md`). The subject, control
and pin library directory names (`subject`, `control`, `pinlibs`) have equal
byte lengths. For a timing metric, its paired median
ratio and entire 95% paired-bootstrap interval must lie within 0.95–1.05. For a
memory checkpoint level, the paired difference control − ox in KiB must have
its median and entire interval within ±max(64 KiB, 1% of |median(ox)|).
Otherwise the metric is inconclusive and pin ratios are suppressed. Callgrind
scoped `Ir`, Massif checkpoint `mem_heap_B` and Memcheck live bytes and blocks
of the control must equal ox exactly; otherwise that comparison is INVALID, its
ox and pin rows are not printed, and the stage exits nonzero. Do not change this rule after
looking at results. Keep profiler data in memory and emit plain-text reports;
commit or upload no captures. `repeat`, `multi` and `attribute` select the
separate lifecycle and attribution diagnostics; their timings are not primary
performance samples.
`attribute` uses separate optional Massif/Memcheck builds of this same native
worker. Shipping libraries stay unchanged. Massif takes M0–M3 boundary
snapshots; its reported interval maximum is a sampled peak, not a promise to
catch every transient allocation. Memcheck counts live blocks at M0 and the
first lifecycle's boundaries, then after cycles 10/50/100 on empty and the
selected populated profile. Compare those live counts with native glibc arena
statistics before calling growth a leak or an allocator plateau. Callgrind
collects only the free/fini wrappers, with exclusive costs and overlapping
inclusive edges reported separately. All profiler documents are parsed in RAM.

`init-attribute` (#1090) attributes the init interval only, after the same
equivalence check. Its samples run the worker's `init` mode: `time` with
pauses after load (M0) and after init (I1), where the controller reads smaps
and the `mincore` residency of every measured file. Every phase record also
carries `getrusage` user/sys time, minor/major faults and context switches;
under tick CPU accounting the user/sys split of a 2 ms interval is quantized.
One probe runs per sample, in the rotating order:

For first-candidate attribution (#1031), select
`--teardown-attribute-window first-candidate` with the warm `init-attribute`
stage and `--teardown-cases phrases,journals`. The window starts after init,
configuration and instance allocation and includes parse, guess and first
candidate observation. `--teardown-init-probes count` selects the retained
resource probe; three fresh Callgrind processes per arm still run. Ir is a
diagnostic min–max range under production RandomState, without a tolerance.
The default window remains `init` and the default probe selection remains all.
`--teardown-init-probes none` selects Callgrind alone. For local retention,
pass `--teardown-summary-dir` an empty directory inside the worktree. Each
fresh process writes its full parsed counters, exclusive function costs,
inclusive function costs and call edges before the next process starts;
each facade/profile also gets all three rounds together. Exclusive totals
are reconciled before writing. Inclusive costs overlap and may recurse,
so they must never be summed as a partition. Files are created exclusively,
flushed and synced, and existing summaries are never overwritten. The raw
profile is retained beside its summary before parsing. These are local
profiler files; do not commit or publish them as routine evidence.
The `verify`, `confirm` and `init-attribute` stages accept
`--base-pinyin-so` and `--base-zhuyin-so`; the four arms rotate serially with
equal-length library-directory names. `confirm` reports paired PR/base ratios
under the existing same-bytes timing rule, including the per-sample sum of
init and first candidate. Its `--teardown-startup-heap` option uses the
existing Massif worker and reports sampled startup peaks through M1, with
exact ABI-boundary heap controls. Those peaks include candidate enumeration
through M1 and are sampled maxima, not exact transient allocation peaks.

- `count`: perf counters around init, minus an empty calibration bracket
  that the record reports too. An event the kernel refuses (for example a
  hardware event on a host without a PMU) is NOT RUN with its errno, never 0.
  With `--teardown-cache cold`, its rounds also give each side's
  `page-residency` record for M0 to I1.
- `sample` and `sample-coarse`: `cpu-clock` samples every 20 µs and every
  200 µs, with kernel call chains. Kernel samples are named by their
  innermost syscall or page-fault frame from `/proc/kallsyms`; user samples
  by the object mapped at I1 and its function (`nm`, symbol table first). The kernel charges each sample's handler to
  the task, so the fine period inflates CPU time; compare shares, not
  sampled durations, and check them against the coarse period.
- `faults`: every minor and major fault's IP and address, with every mmap,
  in one ring in order; each fault is assigned to the mapping that held its
  address at the time.
- `trace`: `strace -f -y -T` between two marker writes the worker makes
  immediately around the init call. Counts are exact; per-call times include
  ptrace stops and only bound kernel time. Requires `strace` on PATH.
- `pages`: one Lackey run (`--trace-mem=yes`), streamed from a pipe and
  kept only between the same markers: each distinct instruction address and
  64-byte data line init touches in the facade. The `init-pages` record maps
  them, and the facade pages that the `count` samples' M0 and I1 `mincore`
  bitmaps show init reading into the page cache (cold), onto the object's
  sections, sized symbols (`readelf`, `nm`) and the ordered `.text` prefix
  that the packaging check verifies.

Then, warm only, three rounds of Callgrind collected on the facade's init
export, with simulated caches (`--I1`/`--D1`/`--LL` fixed) and branches.
Init Ir must be equal for ox and control on KC and tkrzw; an INVALID control
withholds ox and pin and fails the stage. On BDB, init Ir is diagnostic only,
because libdb seeds `srandom_r` from the process. With `--teardown-cache cold`,
each sample evicts as above and Callgrind is NOT RUN. None of these samples is
a primary timing sample; report primary timings from `confirm`.

### Populated USER_FILE baseline

The shared startup/teardown profiles also include `user`, `user8` and
`usersparse`. The first two import the same 10,000 distinct two-or-more-character
phrases from the pin's system export into library 7, at count 5, with one and
eight readings per phrase. The original reading comes first; additional
readings vary only tones, in lexical tone order. `usersparse` imports `user`
and uses the pin's mask API to retain slots congruent to 1 modulo 16: 625 live
items across nearly 10,000 slots. Existing profile recipes are unchanged.
Provisioning validates existing manifests and fills only missing profiles.

The dedicated stages below extend `contract-diff.py --teardown`, its existing
`bisect.c` worker and its private-profile restoration. No shipping code or
private Rust API is instrumented. Use the ordered shipping prefixes built
above, an ext4 `TMPDIR`, and an owned ext4 work directory:

```bash
just heavy -- uv run --no-project --managed-python python -B -X lazy_imports=normal \
  tools/bisection/contract-diff.py bdb "$PIN_PREFIX" "$PR_PREFIX/lib/libpinyin.so" \
  --base-pinyin-so "$BASE_PREFIX/lib/libpinyin.so" --teardown \
  --teardown-stage user-verify --teardown-facades pinyin \
  --teardown-cases empty,user,user8,usersparse --teardown-work-dir "$WORK"
# After the bounded preflight passes, change user-verify to user-phrase.
```

`user-verify` checks the census, getter/enumeration/export hashes and saved
`user.bin` bytes against the pin. The broader `verify` stage still checks its
full candidate transcripts and reopen behavior; a mismatch there stays
NOT COMPARABLE and must be reported separately. A passing bounded preflight
does not establish end-to-end candidate equivalence.
Pass the existing `--teardown-summary-dir` option with an empty local
directory to retain complete preflight observations and their hashes for
diagnosis, including failures. This never changes the preflight verdict.

`user-phrase` rotates base, PR, the byte-identical PR control and pin. It uses
60 native timing rounds after three discarded rounds and 30 separate smaps
rounds after two discarded rounds, then separate fresh-process
Memcheck, Massif and three Callgrind rounds per operation. Its boundaries are:

| Window | Work |
| --- | --- |
| M0–I1, open | The complete `pinyin_init`, after dlopen and export resolution. |
| R0–R1, read | 10,000 reads of slot 1: text, count, unigram and every nth reading. The instance and a 16-key GArray are allocated before R0. |
| R1–N1, enumerate | Probe every slot, skipping holes, and read each existing item's fields and readings. |
| N1–E1, export | Library-7 export iterator to exhaustion, including rendering and freeing returned text/readings; hash results without stdout formatting. |
| W0–W1, write | Eight cycles removing the first phrase's current token and reimporting its eight readings, then save. Each cycle calls mask-out and import-end; save is included. |

Pss at M0 and I1 comes from each mapping in one `/proc/<pid>/smaps` read,
including separate anonymous, shipping-library, dependency and data-file
groups. The later smaps_rollup remains diagnostic. Memcheck's full
`xtmemory` `totBk` and `totB` differences count all heap allocation events and
bytes, including transient buffers that are freed before return. `curBk`
and `curB` differences separately report retained blocks and bytes. These
are whole public-call windows, including both implementations' C-boundary
copies and all heaps; the open count includes system/configuration work and
the user indexes, not just `user.bin`. Compare the empty open separately;
do not label its subtraction as an isolated item-arena allocation count.

Massif reports boundary live heap and sampled peaks in those windows. The
existing exact control rule applies to boundary heaps and allocation
counters; sampled peaks are diagnostic and do not have an exact control
requirement. Ir retains fresh-process ranges rather than claiming exact
counts under BDB/RandomState. The write profile also counts the pin's
`PhraseIndex::compact` calls. The row representation has no item arena:
its arena length and the proposed arena's garbage bounds are inapplicable
until that representation exists. Neither file length nor total heap is
relabeled as a measured arena peak.

For an init-only comparison, `just measure startup --instruction-scopes init`
captures only the requested instruction window. The default still captures
init, first candidate and steady. Record selected windows with the results;
selecting init does not qualify a failed first-candidate or steady scope.
`--instructions-from` can reuse already qualified windows from a retained
segment after checking its input, worker and shipping-library hashes.

For the steady cycle, pass `--user-dir "$WORK/profiles/user/user"` (and then
`user8/user`) to `just measure steady-comparison`. Every native, allocation
and Callgrind launch gets a fresh private copy, removed by the controller;
the source profile is unchanged. Without this option the existing empty
profile workload remains unchanged. The native startup controller accepts
these same profile names through its existing `--profiles` selection.

## Per-page residency attribution

`tools/dev/measure/pages.py` (#1163) names the file pages that become resident
during a cold sample. It compares one file's `mincore` vectors from two
checkpoints and lists each newly resident object page with every section and
symbol it overlaps: ELF section headers with `.symtab` and `.dynsym`
(`elf_sections`, `elf_symbols`), or thin arm64 and x86_64 Mach-O segments,
sections and `LC_SYMTAB` symbols (`macho_segments`, `macho_sections`,
`macho_symbols`). Data and user files are reported as byte ranges
(`offset_range`). Universal Mach-O selects and identifies the host architecture
slice; unsupported formats fall back to byte ranges with a reason.

Two controllers report through it. Nothing is archived or uploaded.

The native startup controller (`just measure startup --cache cold`, native
Linux on ext4 and native macOS on APFS) reads each cold file of a cold memory
sample at the worker's existing checkpoint pauses (`start`, `lib_load`,
`init`, `config`, `alloc`, `first_candidate`, `end`), with
`tools.dev.mman.vector`, the read that `tools.dev.measure.worker` makes while
the child waits. The cold files are those its preparation evicted: the copied
facade (`object`), each system-data file (`data/<path>`) and each private user
file (`user/<path>`); libc and backend libraries stay warm and are not
reported. Timing samples, warm samples and rejected samples collect no
vectors. Once all memory rounds for one facade and profile finish, it prints
one `page-residency` record per arm, accepted round and pair of neighbouring
checkpoints. Rejected rounds have no vectors and are skipped. Each record has
`facade`, `profile`, `arm`, `round`, `checkpoints`, `page_size`,
`metric`, `limits` and `files`. The metric names the host:
`linux_resident_pages` on Linux, `darwin_resident_pages` on macOS. Each file
with a newly resident page carries that metric's page count and its `rows`; a
file that appeared, disappeared or was replaced between the checkpoints
carries a `reason` instead. `lib_load` to `init` is the teardown campaign's M0
to I1. The facade is parsed after the rounds, from a descriptor whose device,
inode, size and modification time match the checkpoint; otherwise its rows are
byte ranges and `layout_error` names why. A universal Mach-O also names its
`macho_slice`.

```sh
# The records of a native startup run whose standard output is in $startup_run:
grep '"startup": "page-residency"' "$startup_run"
```

For memory attribution without timing or profiler sampling, select the native
controller's `memory` stage. It keeps verification/reopen comparisons and the
usual memory rounds and warmups, and emits no timing or instruction results.
On Linux, each memory sample also retains the complete `/proc/<pid>/smaps`
text in `checkpoints.lib_load.smaps_raw` and `checkpoints.init.smaps_raw`, read
while the existing worker handshake pauses at M0 and I1. Shipping objects and
the worker's behavior are unchanged.

```sh
just initbench --stage memory --cache cold --profiles modest --facades pinyin \
  --base "$BASE" --pr "$PR" --system-dir "$DATA" --profiles-dir "$PROFILES"
```

Sum per-mapping `Pss` from those snapshots, distinguishing `[heap]`, other
anonymous VMAs, the subject's access classes, data files and other mappings.
Private anonymous/COW pages within file-named mappings remain in those mapping
classes. The existing `pss` field comes from a separate `smaps_rollup` read;
report its discrepancy from the sum explicitly. Kernel per-mapping rounding
and the sequential reads need not give identical totals. Mincore page-cache
records are not per-process RSS/PSS attribution.

The teardown campaign's cold mode (`--teardown-cache cold`, native Linux)
reports the cold memory samples of `pilot` and `confirm`, and the cold init
samples of the `init-attribute` stage's `count` probe. The details below
describe the teardown report:

- **Capture.** At each paused checkpoint the controller already reads every
  cold file's residency with `tools.dev.mman.vector`, the one `mincore` reader
  in the tree; the page counts in the `residency` and `init-residency` records
  are derived from the same vector. A cold sample now keeps that vector, with
  the file's size and inode and its kind: an object (the facade and its
  non-C-runtime dependencies, which the report parses) or data (system data,
  the private user files and the dirty scenario's rows file). There is no
  second read and no file byte is read.
- **Pairs.** Only neighbouring checkpoints pair. An `init` sample (the
  `init-attribute` stage's `count` probe) gives M0 to I1. A `memory` sample
  (`pilot`, `confirm`) gives M0 to M1, M1 to M2 and M2 to M3. M0 is after load,
  I1 after init, M1 after the workload and optional save, M2 after the frees
  and M3 after fini.
- **Record.** After the rounds of one facade, case and side, one
  `page-residency` record per pair holds `facade`, `case`, `side`, `cache`,
  `checkpoints` (the pair), `rounds`, `page_size`, the `limits` below, `files`
  and `unchanged`. `files` maps each cold file that had a newly resident page in
  any round to its `kind`, `pages`, `new_pages` (the per-round count, as a
  summary) and `rows`. An object row is one page: `page_offset` with
  `elf_sections` and `elf_symbols`. A data row is a half-open `offset_range`,
  its last page clipped to the file size. Both carry `in_rounds`, the number of
  rounds in which that page became resident; pages join a range only when
  their counts are equal. `unchanged` lists the files with no newly resident
  page in any round. File labels are `facade`, `data/<path>`, `user/<path>` and
  the dependency's name for memory samples, and `SUBJECT`, `DATA:<path>`,
  `USER:<path>` and `LIB:<name>` for init samples.
- **Replaced and unparsed files.** A file that is another inode at the later
  checkpoint (a saved user database that was recreated) has nothing to compare:
  its entry carries `replaced_in_rounds` and those rounds are left out of the
  rest. An object that cannot be parsed (unsupported, malformed or
  unreadable) is reported as byte ranges with a `layout_error` naming why; the
  campaign goes on. Before parsing, the opened descriptor's device, inode,
  size and recorded modification time must match the checkpoint metadata;
  the mapper parses bytes from that same descriptor. A replacement or
  modification after capture, or disagreement across retained rounds, also
  falls back to byte ranges with the reason.
- **Outside the timed scopes.** Objects are parsed after the rounds, when the
  record is printed, never at a checkpoint and never inside a sample: parsing
  reads the object into the page cache, and the next cold sample evicts it
  again. The worker's timed intervals are bracketed by its own clocks, and it
  waits at each checkpoint while the controller reads, as it did before.

```sh
# The records of a cold run whose standard output is in $cold_run:
grep '"teardown": "page-residency"' "$cold_run"
```

Three limits apply to every result of either controller, and every record
repeats them in `limits`:

- A `mincore` difference includes readahead. A page that became resident
  between two checkpoints was not necessarily faulted; the kernel may have
  read it ahead of a fault on a nearby page.
- Pausing at a checkpoint can let outstanding readahead finish. Pages
  requested before one checkpoint can arrive during the pause and show as
  newly resident at the next.
- A page the process wrote is resident too. A user file that grew or was
  rewritten shows its written pages as newly resident, whether or not anything
  read them.

In a teardown record, a page with `in_rounds` below `rounds` was newly
resident in only some of the rounds. A startup record holds one round.

## Criterion benches

| crate | bench | needs |
| --- | --- | --- |
| oxpinyin-capi | `stage2` | the export (`PINYIN_EXPORT_DIR`), the C-ABI surface |
| oxpinyin-store | `backend_matrix_{tkrzw,kyotocabinet,bdb}` | `--features <backend>` (each is `required-features`-gated) |
| oxpinyin-store | `kyotocabinet_bnum` | `--features kyotocabinet,bench-internal`; **a distro-installed Kyoto Cabinet data directory via `OXPINYIN_BNUM_DATA_DIR`** for meaningful numbers (see below) |
| oxpinyin-user | `export_phrases`, `phrase_read` | the export |
| pinyin-oracle | `scan_perf`, `dbm_bench`, `alloc_profile` | the export, model20 (`dbm_bench` also the bench oracles) |

Run one bench by name; without `--bench <name>` cargo also runs the lib
under libtest, which rejects criterion flags:

```sh
cargo bench -p oxpinyin-store --no-default-features --features tkrzw --bench backend_matrix_tkrzw
cargo bench -p oxpinyin-capi --bench stage2 -- --profile-time 10
```

A backend-specific criterion bench — one that needs a peer's own
build inputs, such as a system C library — must carry
`required-features = ["<backend>"]` in its `[[bench]]` entry. CI runs
`cargo clippy --workspace --all-targets` on the default backend, and
without it the target fails to resolve the dependency instead of being
skipped. Run one bench with `--bench <name>`; without it cargo also runs
the lib under libtest, which rejects criterion flags such as
`--profile-time`. To run a bench in debug mode: `cargo test --bench
<name>` (AGENTS.md points here).

### `just bench` and `just bench-smoke`

Maintainer ruling 2026-10-22 UTC. Both recipes run under the heavy slot
and memory admission:

- `just bench [args ...]` runs
  `cargo bench --workspace --benches <args>`.
- `just bench-smoke` runs `just bench --no-run`. It builds every
  selected harness and runs none. It needs no model20 cache.

`--benches` selects every target with `bench = true`. That is each
`[[bench]]` target plus each library and binary target, which keep the
default `bench = true`. Library and binary harnesses run their unit
tests under libtest. Criterion flags such as `--profile-time` are
rejected there, so pass them to one bench through `just heavy`, as
above.

`--benches` skips a target whose `required-features` are not enabled.
With the default features it skips these four targets:

| package | bench | run it with |
| --- | --- | --- |
| oxpinyin-store | `backend_matrix_kyotocabinet` | `just heavy -- cargo bench -p oxpinyin-store --no-default-features --features kyotocabinet --bench backend_matrix_kyotocabinet` |
| oxpinyin-store | `backend_matrix_tkrzw` | `just heavy -- cargo bench -p oxpinyin-store --no-default-features --features tkrzw --bench backend_matrix_tkrzw` |
| oxpinyin-store | `kyotocabinet_bnum` | `just heavy -- cargo bench -p oxpinyin-store --no-default-features --features kyotocabinet,bench-internal --bench kyotocabinet_bnum` |
| pinyin-oracle | `dbm_bench` | `just heavy -- cargo bench -p pinyin-oracle --features oracle-ffi --bench dbm_bench` (also needs the bench oracles) |

`backend_matrix_bdb` needs `bdb`, which is a default feature, so it is
selected.

These default-feature benches load the model20 cache and panic when it is
missing: `stage2` (oxpinyin-capi, `benches/support/mod.rs`), and
`scan_perf` and `alloc_profile` (pinyin-oracle, `benches/support/mod.rs`).
That is the repository's fail-on-missing-input rule
([docs/testing/README.md](../testing/README.md)), not a defect, and no
bench is excluded from the selection. Fetch the cache with
`just fetch-model`.

On a host with model20, `just bench -- --test` is the full start-up
check. It runs every selected harness once: each Criterion bench runs
once without measurement, and each library and binary harness runs its
unit tests.

### `alloc_profile` — heap attribution is opt-in

`alloc_profile` is a custom harness, not a Criterion group. Its timing and
probe-count modes run on the system allocator. Heap attribution
(`--dhat`) needs the non-default `dhat-heap` feature, which installs
`dhat`'s global allocator for the whole executable:

```sh
cargo bench -p pinyin-oracle --features dhat-heap --bench alloc_profile -- --dhat [--keystroke-only]
cargo bench -p pinyin-oracle --bench alloc_profile -- --parity-timed
```

Each build rejects the other's modes: without the feature `--dhat` exits
with this command, and with it every mode except `--dhat` exits rather than
print timings the installed allocator would inflate. With the feature,
`--dhat` combined with `--parity-timed` or `--repeats` also exits 2, because
the profile would run a different workload from the one requested;
`--dhat` alone and `--dhat --keystroke-only` are unchanged. `dhat` is an optional
dependency of `pinyin-oracle` (a non-shipping crate), so ordinary test and
bench builds do not compile it. Dormant, the wrapper took the allocator
lock on every allocation: on the keystroke cycle it measured about 18% over
no wrapper in one run on one host, which is a property of that run, not a
factor to apply to older numbers. Timings that earlier snapshots took from
this bench's non-`--dhat` modes came from the always-installed wrapper and
stand as recorded.

DHAT site counts and stack grouping are build-specific; compare matched
workloads using allocation totals and reconciled source-level attribution,
rather than treating raw site-count changes alone as allocation
regressions.

### `kyotocabinet_bnum` — the #402 time-and-space measurement

Motivation: shenghaoc/oxpinyin#402 quantifies the space side of lowering
`#bnum` on the five read-only system TreeDBs deterministically (~1,057
KiB → ~80 KiB per TreeDB, ~4.77 MiB across the five). This bench
produces the matching time-side number.

**Not a local-change proposal.** oxpinyin passes the same open
parameters libpinyin does (no `#bnum=`, `#msiz=`, `#pccap=`, `#apow=`,
`#fpow=`, `#psiz=` or `#opts=`), and that is the standing policy —
external-library handling matches upstream. Tuning `#bnum` locally
would be a case-3 configuration divergence outside
[`compatibility-policy.md`](../findings/compatibility-policy.md)'s
three exception classes. The measurement's audience is an upstream
report (Kyoto Cabinet, or libpinyin itself), not this repo's shipping
open path.

**Data.** The bench needs a Kyoto Cabinet-format libpinyin `data/`
directory containing all five system TreeDBs — `pinyin_index.bin`,
`addon_pinyin_index.bin`, `phrase_index.bin`, `addon_phrase_index.bin`,
`punct.bin` — with production-scale record counts (~10^4–10^5 per
TreeDB). Distro coverage:

* **Fedora arm64** ships `libpinyin` linked against
  `libkyotocabinet.so.16` and installs `libpinyin-data` (KC-format) at
  `/usr/lib64/libpinyin/data`. **Use this for the measurement.**
* **Debian testing** ships `libpinyin-data` at
  `/usr/lib/aarch64-linux-gnu/libpinyin/data` (multi-arch, arch varies),
  built against tkrzw — not readable as `TreeDB` and not usable for this
  bench. See memory: `distro-libpinyin-data-matrix`.
* The checked-in `fixtures/w3/kct/` has ~10^2 records per TreeDB, so a
  run against it is a harness smoke test only, not measurement of the
  `#bnum` axis.

**Container recipe** (Fedora arm64):

```sh
tar -cf - --exclude=target --exclude=.git . | docker run --rm -i \
    --platform linux/arm64 \
    -v "$PWD/target-linux":/src/target \
    fedora:latest bash -c '
    set -eu
    dnf install -y --setopt=install_weak_deps=False -q \
        gcc gcc-c++ make pkgconf-pkg-config git \
        clang-devel llvm-devel \
        kyotocabinet kyotocabinet-devel \
        libpinyin libpinyin-data \
        curl ca-certificates
    # Exact on purpose (#1074): output depends on the Rust version. Keep it equal to the channel in rust-toolchain.toml.
    curl -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.97.1 --profile minimal
    . "$HOME/.cargo/env"
    mkdir -p /src && tar -xf - -C /src && cd /src
    OXPINYIN_BNUM_DATA_DIR=/usr/lib64/libpinyin/data \
        cargo bench \
            --no-default-features --features kyotocabinet,bench-internal \
            --bench kyotocabinet_bnum
    '
```

Do **not** pass `-- --bench` here. `cargo bench` already appends
`--bench` to the criterion binary, and criterion 0.8's clap rejects the
flag when it is specified twice ("an argument cannot be used with one or
more of the other specified arguments"). Add `-- --sample-size N
--measurement-time Ns` if you want to shorten runs; a bare `cargo bench`
uses the criterion defaults (100 samples / 5 s warmup / 5 s measurement).

Reads: for each `#bnum` candidate (default 65,536; 16,384; 4,096; 1,024)
a resident line on stderr (VmHWM in a fresh child process per group)
and a criterion timing group on stdout — five TreeDB benches per group,
each timing 512 (or the file's own record count if smaller) `get_raw`
calls after a warmup pass. Space and time columns sit alongside each
other in one output.

**Reading the result.** VmHWM at the ~13 MiB total footprint has ~1
MiB per-process jitter, so the header arithmetic
(`kcplantdb.h:2399-2409` / `:2822-2830`) is the authoritative source for
per-configuration bytes; treat the bench's resident column as a
sanity check. The timing table is the load-bearing output: it says
what lookup cost falling from `#bnum=65,536` to lower values on the
read-only system TreeDBs actually is, in µs per 512-key sweep. That
number belongs on shenghaoc/oxpinyin#402 (and, if it is stable and
favourable, on an upstream report drafted from
[`upstream-report-drafts.md`](../findings/upstream-report-drafts.md)) —
not in this repo's shipping open path.

**Report the result and recipe.** The recipe above regenerates the figures.
An issue or upstream-report draft needs the conclusion and relevant numbers,
not the raw run output; follow [results and reproduction](../../CONTRIBUTING.md#results-and-reproduction).

## BDB guess, training, and import profiles (#757–#759)

`tools/bisection/system-import-profile.c` extends the existing import
driver with three full-process workloads. The historical #757–#760
measurement driver source has SHA-256
`eb2cbdf2b573ccbcbf39b90b549b6d618fe7a4c3de7ee417051e510b108bfa0b`;
its measured executable has SHA-256
`c4b09cb0b15a25deb7e418a66d8fc63256cce4fe40caf3a27700a2fd2a05fd51`.
The reviewed source, with strict phrase-number parsing and the full-consumption
check before training's guess, has SHA-256
`565be2c8bfb07682c1d342ffcd0d7e91795aae5f6115dd3dd004d9267070b5ea`.
Build the driver in the measurement environment with:

```sh
cc -std=gnu11 -Wall -Wextra -Werror -O2 tools/bisection/system-import-profile.c -o "$HARNESS" -ldl
```

Use the same BDB system data, fresh empty user directory, CPU, pin SO, and
subject SO throughout a comparison. The registered measurements in
[#757](https://github.com/shenghaoc/oxpinyin/issues/757),
[#758](https://github.com/shenghaoc/oxpinyin/issues/758), and
[#759](https://github.com/shenghaoc/oxpinyin/issues/759) used a Fedora 44
container on a Fedora 45 host, Valgrind 3.27.1, and CPU 0. The pin SO there
has SHA-256 `d535d077eb55736f742a270a4128763ca4f9b0471819778b06b5708b47b4badc`.
The subject SO for those registered findings has SHA-256
`8d74995f5f4cab5013da28ef083259e588821b2cf28249548dba705db643436b`.
Its recorded build invoked
`just packaging install libpinyin --prefix=/lane/stage-main -- --locked --release --jobs 1 --no-default-features --features bdb,shipped`
with `CARGO_BUILD_JOBS=1` and `CARGO_TARGET_DIR=/lane/target-bdb`.
From `crates/oxpinyin-capi`, that wrapper ran this exact Cargo command,
then relinked the installed SO under the version script:

```sh
cargo cinstall --prefix=/lane/stage-main --libdir=/lane/stage-main/lib --includedir=/lane/stage-main/include --target-dir=/lane/target-bdb --locked --release --jobs 1 --no-default-features --features bdb,shipped
```

The timed, relinked `libpinyin.so.15.0.0` is 5,177,408 bytes.
`readelf -d` reports `NEEDED`: `libgobject-2.0.so.0`,
`libglib-2.0.so.0`, `libdb-5.3.so`, `libm.so.6`,
`libgcc_s.so.1`, `libc.so.6`, and `ld-linux-x86-64.so.2`.
Later PR comparisons must identify their own subject build and use matched
pin/main/subject conditions; these registered figures are not a substitute.

The frozen sentence fixture is `fixtures/w4/oracle-sentence-surface.txt`
(SHA-256 `d119f3a7f665333ef5717b1925171799c29e710d4ad1575748e0a9d3ae2c9e21`).
`--sentence-fixture` replays its 500 rows with a nonempty sentence field,
guessing a sentence and candidates for each. `--train-fixture` takes the
first 200 such rows, checks full parse and a successful guessed train at
index 0 for each, then saves once. Both set options `0x18a`; candidate
sorting in the guess workload uses `0x1e`.

For `--phrase-file`, generate the input from the same BDB pin SO and data
with the driver's **unmeasured** `--export-system`. It walks libraries
1–4 in token/pronunciation order. Select the first 10,000 distinct phrase
texts, keep the first pronunciation of each, and set every import count to 5.
This exact byte recipe writes tab-separated
`library<TAB>phrase<TAB>pinyin<TAB>5<LF>` rows; the first is
`1<TAB>锕<TAB>a<TAB>5` and the last is
`1<TAB>爱河<TAB>ai'he<TAB>5`. The input is 121,316 bytes with SHA-256
`22637aee560c86761bb6d9d187d9af8b9fea970a6690bc21e21e5b6fc1cbb117`.
It is a disposable workload input, not a committed fixture or profile
capture:

```sh
uv run --no-project --managed-python python -X lazy_imports=normal - "$HARNESS" "$PIN_SO" "$DATA" "$PHRASE_FILE" <<'PY'
import hashlib
import subprocess
import sys
import tempfile

with tempfile.TemporaryDirectory() as user_dir:
    exported = subprocess.run(
        [sys.argv[1], sys.argv[2], sys.argv[3], user_dir, "--export-system"],
        check=True, stdout=subprocess.PIPE,
    ).stdout
seen = set()
selected = []
for line in exported.splitlines():
    library, phrase, pinyin, _frequency = line.split(b"\t")
    if phrase in seen:
        continue
    seen.add(phrase)
    selected.append(b"\t".join((library, phrase, pinyin, b"5")) + b"\n")
    if len(selected) == 10_000:
        break
payload = b"".join(selected)
assert len(selected) == 10_000 and len(payload) == 121_316
assert hashlib.sha256(payload).hexdigest() == (
    "22637aee560c86761bb6d9d187d9af8b9fea970a6690bc21e21e5b6fc1cbb117"
)
with open(sys.argv[4], "wb") as output:
    output.write(payload)
PY
```

The phrase workload imports all 10,000 rows, saves once, then exports the
touched library (library 1: 95,698 rows). Use a fresh empty `USER_DIR` for
**every** command and repeat each pin/subject command three times. Take the
median of whole-process Callgrind `Ir` and of Massif's maximum
`mem_heap_B`; report spread as `(max − min) / median`. The Massif value
excludes stacks, allocator overhead, and file mappings. Check output
equivalence and operation counts before comparing costs. Keep profiler
output in streams or memory, not in archives, logs, captures, or profile
files. The commands below write Valgrind output to stderr:

```sh
taskset -c 0 valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --sentence-fixture fixtures/w4/oracle-sentence-surface.txt
taskset -c 0 valgrind --tool=massif --time-unit=i --heap=yes --stacks=no --peak-inaccuracy=0.0 --detailed-freq=1 --max-snapshots=1000 --massif-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --sentence-fixture fixtures/w4/oracle-sentence-surface.txt

taskset -c 0 valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --train-fixture fixtures/w4/oracle-sentence-surface.txt
taskset -c 0 valgrind --tool=massif --time-unit=i --heap=yes --stacks=no --peak-inaccuracy=0.0 --detailed-freq=1 --max-snapshots=1000 --massif-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --train-fixture fixtures/w4/oracle-sentence-surface.txt

taskset -c 0 valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --phrase-file "$PHRASE_FILE"
taskset -c 0 valgrind --tool=massif --time-unit=i --heap=yes --stacks=no --peak-inaccuracy=0.0 --detailed-freq=1 --max-snapshots=1000 --massif-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --phrase-file "$PHRASE_FILE"
```

Remove the disposable phrase input after the runs. The registered stdout
SHA-256 values were `50dfc9c87e8eb2ce9121ac4f138584ab226a69308a9becc727cfca1c68c3100d`
for 500 guesses, `e5855249fc5c7cc2c4de81aa1e792c2f41152a84c8a3e10f04c52a824bda698d`
for 200 successful guessed training steps, and
`34e98cd8bf5a191b27e0b8fc50ffdefca9e76a2dab7dd01106cf6b6d3c9eec1d`
for 10,000 imports and 95,698 exported rows.

### Native Fedora baseline refresh (2026-10-14 UTC)

This refresh uses Fedora 45 on Linux `7.2.9-300.fc45.x86_64`, an
Intel Core i7-9750H, CPU 0, GCC 16.2.1, Valgrind 3.27.1, cargo/rustc
1.97.1, and cargo-c 0.10.24. GitHub `main` was still
`d2a5d20776612b438aff183632ef4de6fa33857f` when checked for this
run. Its BDB subject was built from that exact commit with:

```sh
CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=/home/sheng/Documents/Codex/2026-10-14/task-4/target-main-bdb \
  just packaging install libpinyin \
    --prefix=/home/sheng/Documents/Codex/2026-10-14/task-4/stage-main-shipped \
    --libdir=/home/sheng/Documents/Codex/2026-10-14/task-4/stage-main-shipped/lib \
    -- --locked --release --jobs 1 --no-default-features --features bdb,shipped
```

The resulting relinked `libpinyin.so.15.0.0` is 5,226,232 bytes, SHA-256
`bbd860357e17af1797a72e9dabb8cc0bbea05851122e68a26e59e6c1a70bef75`.
Its `readelf -d` `NEEDED` list is `libgobject-2.0.so.0`,
`libglib-2.0.so.0`, `libdb-5.3.so`, `libm.so.6`,
`libgcc_s.so.1`, `libc.so.6`, and `ld-linux-x86-64.so.2`.
The pinned oracle remains `074a2219c90feaf962d0d24f034514033ece5f99`,
with SO SHA-256 `d535d077eb55736f742a270a4128763ca4f9b0471819778b06b5708b47b4badc`.

For the six commands above, `HARNESS=/tmp/pr767-system-import-profile-final`
was the final strict-compiled C driver (executable SHA-256
`d1eef411aacc080690d3295c4b7058c5d01c3ca08a762a8b8819d94799e838a9`),
`DATA=/home/sheng/work/oracle/bdb/lib/libpinyin/data`, and `PHRASE_FILE`
was the verified 121,316-byte input with SHA-256
`22637aee560c86761bb6d9d187d9af8b9fea970a6690bc21e21e5b6fc1cbb117`.
Set `SO` to `/home/sheng/work/oracle/bdb/lib/libpinyin.so.15.0.0` for the
pin rows, or to the staged main library above for the main rows. Each command
used a fresh empty `USER_DIR` under `/tmp` and ran three accepted times,
serialized with `taskset -c 0`. Output was read from pipes; no profiler
capture was retained. A run overlapping suspend is discarded by checking
the change in `CLOCK_BOOTTIME - CLOCK_MONOTONIC` and is retried.

| Workload | Library | Median Callgrind `Ir` | Ir spread | Median Massif peak `mem_heap_B` | Heap spread |
| --- | --- | ---: | ---: | ---: | ---: |
| 500 sentence guesses | pin | 2,245,511,454 | 0% | 2,147,301 | 0% |
| 500 sentence guesses | `d2a5d207` main | 6,641,501,950 | 0.025904% | 2,620,088 | 0% |
| 200 training steps and save | pin | 454,876,602 | 0% | 7,124,656 | 0% |
| 200 training steps and save | `d2a5d207` main | 1,186,203,677 | 0.001216% | 1,546,948 | 0% |
| 10,000 imports, save, and export | pin | 758,108,369 | 0% | 9,354,935 | 0% |
| 10,000 imports, save, and export | `d2a5d207` main | 1,725,238,006 | 0% | 7,423,931 | 0% |

All 36 accepted profile runs produced the corresponding stdout hashes above;
none overlapped a suspend. The final driver and historical executable also
produced byte-identical stdout on the pin and refreshed main SO for all three
workloads. The initial measurement attempts stopped before this campaign were
discarded after the driver changed and after a typo in the in-memory training
hash check; neither contributes to these medians.
The final driver rejected 16 malformed 10,000-row inputs covering missing
digits, suffixes, library and count range errors, bad line endings, and an
embedded NUL. Valid `+5`, `-5`, CRLF, and an EOF-terminated final row retained
byte-identical stdout against the historical executable.

## The perf baseline (oracle vs installed oxpinyin)

`just measure perf-baseline` measures speed, installed size
and RSS with one dlopen harness (`bisect --perf`), alternating oracle
and oxpinyin processes. It needs the pin-built oracle, `cargo-c` and
the model cache. The reproducible form is the perf-matrix container, which
carries uv and the managed interpreter (it has no `just`):

```sh
docker build --platform linux/arm64 -f tools/bisection/Dockerfile.perf-matrix -t oxpinyin-matrix .
docker run --rm -v /tmp/perf-out:/out -e PINYIN_ORACLE_PREFIX=/opt/libpinyin-tkrzw \
    -e OXPINYIN_PERF_WORK=/out oxpinyin-matrix \
    uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure perf-baseline
```

Knobs: `PERF_RUNS`, `PERF_CYCLES`, `PERF_RAM_RUNS`, `PERF_CPU` (pin a
CPU; do it). `measure perf-matrix` is the four-backend
form, and the image's default command. For a parent-vs-HEAD comparison use one `CARGO_TARGET_DIR` per
tree: cargo's freshness check is fooled by `git archive` mtimes and will
reuse the wrong artifacts otherwise.

## RSS diagnosis (where the resident set actually sits)

`bisect --perf` in `PERF_MODE=rss-diag` answers a different question from
the RAM modes: not how large the resident set is, but what it is made of.
At init and again after the keystroke cycles it reads
`/proc/self/smaps_rollup` (anonymous vs file-backed, the clean/dirty
split) and `mallinfo2()` (heap high-water, retained free chunks, arena
count), then calls `malloc_trim(0)` and reads both again — the one call
that separates allocator retention from memory genuinely held.

`just measure rss-diag` drives the two cells round-robin over
**one** data directory (the drop-in configuration: both engines open
libpinyin's own installed `data/`) and writes `rounds.jsonl` plus an
`identity.txt` naming every artifact by SHA-256:

```sh
just measure rss-diag \
    --lp /opt/libpinyin-kc/lib/libpinyin.so.15.0.0 \
    --ox <shipped oxpinyin .so> \
    --data /opt/libpinyin-kc/lib/libpinyin/data \
    --out /tmp/rss-diag --rounds 12 --cycles 8
```

Two rules the harness enforces and a reader must not undo:

* **`RSS_DIAG_DIR` belongs in its own round.** Setting it makes `bisect`
  write the `/proc/self/maps`, `/proc/self/smaps` and `malloc_info`
  dumps, and writing them allocates — which moves the very RSS the
  `malloc_trim` delta measures. The runner takes the measurement rounds
  with it unset and one mapping round per cell with it set.
* **RSS is backend-sensitive.** Two cells on one backend give a valid
  ratio; the absolute figures describe that backend's configuration and
  do not stand in for another's.

`tools/bisection/rss-smaps.py` turns a dump into a per-mapping `Rss`
table, and `--diff A B` into the per-mapping delta that names which
mappings carry a gap. `docs/findings/rss-attribution-2026-09-09.md` is
the worked example.

For live and peak **live** bytes on the Rust side, build the artifact
with the non-default `alloc-count` feature; `bisect` resolves the
`oxpinyin_alloc_*` readers with `dlsym` and reports `-1` when they are
absent, so the JSON shape does not change between cells. The feature is
never in a shipped artifact — `nm -D` is the check.

### Attributing a heap gap to callers

DHAT names the allocation *site*; naming its *caller* across the C-ABI
boundary is where this goes wrong, and the failure looks like a result.

**Valgrind's unwind of the dlopened Rust cdylib is not trustworthy above
`kcdbget`.** It resolves the libpinyin cell correctly, and on the
oxpinyin cell it produces symbol-table-plausible garbage — a chain with
`Arc<T>::drop_slow` calling `dict::ucs4_walk_key`, and
`pinyin_iterator_add_phrase` directly beneath `main`. Each address really
does sit inside the function it names, so nothing looks broken. None of
`--num-callers=24`, resolving at `addr - 1`, `debug = 2` with `lto =
false`, or `-C force-frame-pointers=yes` changes it; all produce
byte-identical stacks. Kyoto Cabinet debug symbols
(`libkyotocabinet16v5-dbgsym`) do fix the KC half and nothing else. The
worked case is
`docs/findings/rss-attribution-2026-09-09.md`, "DHAT's cross-FFI stacks
were wrong in a way that looked plausible".

**Use callgrind caller→callee edges instead** — exact call counts, no
stack walk. Two rules that produced wrong numbers before they were
caught:

* **Key on the callee's `(object, file, name)` triple, not the demangled
  name.** `BasicDB::get(char const*, unsigned long, char*, unsigned
  long)` is a substring of that function's own
  `::VisitorImpl::visit_full`, and summing both gave 575 where the answer
  was 152.
* **An inlined callee has no call edge.** `BasicDB::check` and most
  `BasicDB::get` sites inline into their caller, under-reporting by ~3×.
  Where inlining is possible, count something that cannot be inlined — a
  virtually-dispatched visitor, a heap allocation — and corroborate
  against a second such marker.

A profiling build may stand in for the shipped artifact here: block
counts were identical across the shipped recipe, `debug = 2` with LTO
off, and forced frame pointers.

## Profiles

```sh
PINYIN_EXPORT_DIR="$EXPORT" PINYIN_MODEL_DIR="$MODEL" just profile-w8
```

The launcher stages oxpinyin-capi from the tkrzw cell through
`cargo cinstall --profile profiling` (the `profiling` profile keeps line
tables and thin LTO) and samples the keystroke cycle. It needs the exported
tables, the pinned model20 cache, cargo-c, and a profiler. `PROFILE_REUSE_STAGE=1`
skips the restage. Artifacts land in `target/profile/`;
`tools/profile/extract-hot-stacks.py` summarises them.

**Sample count.** A hot-stack report ranks stacks by how many samples each
holds, so its order is only as good as the profile is long. samply takes one
sample per CPU millisecond, so the total follows the work in a cycle. On the
measuring host (Apple M5, macOS 27 arm64, samply 0.13.1) a steady cycle over
the full model20 tkrzw export takes about 8 ms and yields about 8 samples; the
small fixture exports take about 1 ms and yield about 1. Unset, samply runs
`PERF_CYCLES=2048`: about 16,000 samples in about 17 s there, three times the
level below, so a host up to three times faster per cycle still reaches it. A
set `PERF_CYCLES` applies to every profiler. The other profilers keep their
32-cycle default: Callgrind counts instructions exactly, and `perf record`
stores a stack dump with every sample. The W8 cycle and `bisect --perf` are
unchanged; the timing JSON it prints and the launcher saves has one entry per
cycle, so it is longer at 2048.

After a samply record the launcher prints `samples: N` from the profile and
warns on stderr when N is below 5,000. `extract-hot-stacks.py` opens its report
with the same line and warning; `--total` prints the line alone and
`--min-samples` changes the level. The flamegraph and perf fallbacks write no
samply profile and print no total. A faster host or a smaller export takes
fewer samples per cycle: when warned, raise `PERF_CYCLES` in proportion to the
shortfall.

The level comes from repeated runs. Each row is six runs at that
`PERF_CYCLES` on the measuring host, ranked against the runs pooled at 4096:

| `PERF_CYCLES` | samples (median) | top stack | top two, in order | top three, in order |
| ---: | ---: | ---: | ---: | ---: |
| 32 | 309 | 1/6 | 0/6 | 0/6 |
| 64 | 563 | 6/6 | 2/6 | 0/6 |
| 128 | 1,069 | 6/6 | 5/6 | 4/6 |
| 256 | 2,106 | 6/6 | 6/6 | 4/6 |
| 512 | 4,259 | 6/6 | 6/6 | 4/6 |
| 1024 | 8,281 | 6/6 | 6/6 | 4/6 |
| 2048 | 16,435 | 6/6 | 6/6 | 5/6 |
| 4096 | 33,728 | 6/6 | 6/6 | 4/6 |

The top two stacks, `core::slice::sort::stable::drift::sort` (6.1%) and
`oxpinyin_data::dict::resolve_items` (4.0%), kept their places in every run
from 256 cycles (about 2,100 samples) up. The level is 5,000, about twice that
size. At the old 32-cycle default a tkrzw leaf-node read (`_platform_memmove`,
a fixed cost of about 25 samples) led in five of six runs. Third place never
settles. Its stack (`order_window_tail`, 2.7%) is within 0.2 points of the
fourth, and in 5 of the 42 runs from 64 cycles up a `libsystem_malloc` frame
calling `mach_absolute_time` took a top-three slot, which more samples do not
prevent. Compare the counts as well as the ranks.
To repeat the table, run `PERF_CYCLES=n PROFILE_REUSE_STAGE=1 just profile-w8`
for each size and compare the first stacks of each report.

The save-only profile is unsymbolicated. When the installed samply supports
`--unstable-presymbolicate` (0.13.0 or later), the launcher passes it and
samply writes `target/profile/w8-cycle.profile.json.syms.json` beside the
profile. The extractor maps each frame through the profile's `libs` and
`resourceTable` to a library-relative address, then looks that address up in
the sidecar's per-library `known_addresses`; the names are samply's demangled
symbols. No separate symbolizer or debug-file step is required. A frame the
sidecar does not cover prints as `unresolved`, never as its raw `0x...` offset.
`samply load` uses the same sidecar. An older samply still records, with a
warning, and the report is all unresolved. The flag is unstable upstream, so
the launcher probes the installed help instead of requiring a version. Raw
profiles and sidecars are not committed.

On Linux the profiler chain is unchanged: samply, then the flamegraph wrapper,
then perf record, then Valgrind Callgrind. When Linux's samply supports the
flag, it writes the same sidecar and the extractor reports the same way. On
macOS samply is required and the recipe supplies Homebrew's GLib, Berkeley DB,
tkrzw and codec paths. The
launcher stages the tkrzw C ABI and uses `tools/tkrzw/runtime.py`'s `otool`
resolution for the staged dylib; set `OXPINYIN_TKRZW_PREFIX` (the recipe does)
when running the script directly. There is no `setarch`, `taskset` or
`DYLD_LIBRARY_PATH`: disabling ASLR is unnecessary, Mach affinity is a cache
hint rather than pinning, and the dylib's dependencies are absolute.

`PERF_CPU` is refused on macOS for that affinity reason. `perf record` and
its `perf_event_paranoid` hint are Linux-only and are skipped with a stated
reason; the flamegraph and Valgrind fallbacks are likewise not used. A missing
samply, cargo-c, export or model is a refusal, not a timing-only run. This
macOS path has been exercised on arm64 only. The Linux profiler chain is
unchanged.

## Writing it up

Follow [results and reproduction](../../CONTRIBUTING.md#results-and-reproduction):
state the conclusion, final measurements and reproduction commands. Include
the revisions, workload, backend, build recipe, host/container and controls
needed to interpret the comparison. One invocation may reproduce a whole
table. Link existing setup procedures; a separate findings file is optional.
A change that trades time for space or the reverse must say so and justify
it (AGENTS.md, "Source policy"). Historical reports are examples of results,
not templates requiring their full provenance sections to be copied.

### Measure base, PR and pin in one run

**A measurement is the ratio of artifacts timed in the same job on the same
machine, never a number carried over from another run.** A measuring PR
builds the base tree, the PR tree and the pin-built oracle, runs all three
in one job on one machine (same CPU, same data), **interleaving** the base,
PR and pin artifacts run by run, and reports only those figures and the
ratios between them. Interleaving all three is a requirement of the
measurement, not a property of `just measure perf-same-data`,
which interleaves two pin-built backends and two artifacts of the current
tree and does not run a base artifact. If the needed base/PR/pin selection is
missing, extend the maintained in-tree tooling and its `just` entry point;
do not supply an out-of-tree measurement controller.
No absolute figure from an earlier run, another host or another CPU is a
baseline: callgrind `Ir` and wall time both move with the CPU (glibc picks
`memcpy` and friends at run time), so a baseline taken elsewhere cannot be
compared with a PR measured here. Earlier snapshots in this repository
remain records of what was measured where; they are not baselines to
subtract from. The pin comes from the reproducible oracle
(`docs/runbooks/oracle.md`, section 2a), so the pin's hash is the same on
every machine.

### State the Valgrind version

Every measurement report states the Valgrind version it used, as printed by
`valgrind --version`, next to the figures. Callgrind `Ir` and Massif heap
peaks move with the Valgrind release as well as the CPU, so a report
without the version cannot be compared with another run. Normal cloud
provisioning (`tools/cloud/setup.sh`) installs it idempotently and prints the
version. Ordinary native setup and `--in-container` behavioural checks do not
require it. On other measurement hosts, install it explicitly.
On Debian/Ubuntu, `sudo` installs the distribution package:

```bash
sudo apt-get update
sudo apt-get install -y valgrind
valgrind --version
```

In a root development container omit `sudo`; Fedora/RHEL use the available
`valgrind` package via `dnf`. Do not install measurement tools for ordinary
contributions.

### State the build recipe, per artifact

Identify each timed subject by revision, backend and build recipe, including
the profile and relevant flags. A maintained recipe and its arguments can
provide the build command; do not duplicate it for every figure. Report
shipping-library sizes when applicable under AGENTS.md Constitution 2.
Hashes and loaded-library checks needed by the tools remain tool-internal
checks; a report needs the results and reproduction recipe, not a checklist
of their verdicts, checksum manifest or full `NEEDED` inventory. Include a
specific hash or dependency only when it explains a concrete build-identity
or loader discrepancy.

This is not bookkeeping. `cargo cinstall` is the shipping path
(`.github/workflows/release-packages.yml` → `just packaging install`;
`libpinyin.so.15.0.0`, plus `--features shipped` for the drop-in), while
`cargo build` produces `libpinyin_capi.so`, a bisection fixture nothing
installs — and the two do **not** run at the same speed. On the steady
keystroke-cycle ratio the shipping artifact measures **0.064 to 0.087 ratio
units lower on amd64 and 0.032 to 0.038 lower on arm64**
(`docs/findings/perf-cycle-ir-differential-2026-09-08.md`, which publishes the
signed form `Z÷L − Y÷L` — `Z` the cinstall cell, `Y` the cargo-build one — so
its own tables read negative), with no mechanism established. A record that
does not say which one it built cannot be read as a statement about the
product.

Two shortcuts do not substitute for the command line, and both have already
misled a reader:

- **The artifact filename discriminates only after `d32557e3`
  (2026-08-29T16:33:26Z)**, which introduced the `libpinyin.so.15` SONAME.
  Before it, cargo-c installed under the same `libpinyin_capi.so` name the
  cargo build produces.
- **Naming `perf-same-data` is not naming a recipe.** Its
  `OXPINYIN_KC_SO` / `OXPINYIN_TKRZW_SO` variables take a prebuilt `.so` and
  skip `build_capi` entirely.

`just profile-w8` is a third case worth naming explicitly: it
stages through `cargo cinstall` but with `--profile profiling` (thin LTO, line
tables, `panic = "unwind"`), so it is neither the fixture nor the product.

The audit of which existing records sit where is
`docs/findings/perf-build-recipe-audit-2026-09-10.md`.

### Evidence: what is committed and what is not

Publish the final result and a working reproduction procedure. Use one
command for figures produced together; identify different inputs, options or
runs where they affect the comparison. Keep shared readers and calculations
in the repository's Python tooling so another contributor can regenerate
and interpret the results without an author's scripts or old captures.

Raw logs, DHAT JSON, `smaps` dumps, Callgrind/perf output and evidence archives
are local working data, governed by
[the reporting policy](../../CONTRIBUTING.md#results-and-reproduction).
Do not publish or retain them as a submission requirement. Their absence
needs no disclaimer or replacement manifest. Existing historical attachments
do not create a requirement for new work.

If a reproduction command is incomplete or the result contradicts the claim
beyond the method's expected variability, investigate and fix the software,
tooling or documented assumptions and correct or withdraw the claim. State
an actual unresolved limitation beside the affected result; discarded raw
output is not such a limitation. Keep existing measurement controls and
rejection rules, including exact checks where the method requires them.


## Per-call C-API costs

Use the contract driver for #882, #883 and #884. Build base and subject
with `just packaging install`. Use one target directory per worktree.
Run this command under the shared host admission wrapper:

```sh
tools/bisection/run-all.py --heavy-slot -- \
  uv run --no-project --managed-python \
  python -X lazy_imports=normal tools/bisection/contract-diff.py \
  bdb "$PINYIN_ORACLE_PREFIX" "$SUBJECT_PREFIX/lib/libpinyin.so" \
  --zhuyin-so "$SUBJECT_PREFIX/lib/libzhuyin.so" \
  --base-pinyin-so "$BASE_PREFIX/lib/libpinyin.so" \
  --base-zhuyin-so "$BASE_PREFIX/lib/libzhuyin.so" \
  --per-call wrap --per-call-cpu 2
```

The host needs an x86-64 Linux CPU, a C compiler, binutils, Valgrind and
its client headers. Select an allowed CPU. The driver uses the lowest
allowed CPU when `--per-call-cpu` is absent. `--per-call-facades pinyin`
or `zhuyin` restricts a smoke test. Omit both base paths for a two-side run.
Use `--per-call lookup-only` for 10,000 native lookup calls with no adds.
That loop reuses one GArray and looks up `你好` in a fresh context.
It measures a stable store. The wrap workload measures lookup after each add.

The wrap workload runs the existing `wrap-256th-reading` cases on both
facades. It selects only calls inside `step`, including merges and the
256th and 257th attempts. It excludes reading preparation and item reads.
The fixture, frequencies and observation fields stay the same. Every
sample must pass the existing strict verdict predicate. Attempt counts
must agree. A crash, missing result or changed observation fails the run.

Wall time is the median of three interleaved rounds per side and facade.
Each round has a fresh process and user directory. The side order rotates.
Native timestamps bracket the function-pointer call. Python work, fixture
setup and teardown are outside that interval. Results use ns/call and
include the native clock boundary cost. Wall time has no subtraction.
This boundary differs from #880's Python wall boundary; do not compare
absolute times between those campaigns.

Callgrind client requests bracket only the selected C call. Collection
stays off elsewhere. A separate pass measures each operation and side.
The helper calibrates each wrapper with 1,000 calls to a three-instruction
callee. It checks the callee's disassembly. It subtracts those three
instructions from gross wrapper Ir, then subtracts the resulting wrapper
cost from each real call. Calibration runs in the same campaign; it does
not reuse #880's constants. Results use Ir/call. Ratios use this run's pin.
The output also gives library hashes, native round values and the digest
of the compared observations.

The driver compiles a temporary helper from source embedded in the driver.
This helper is not a second driver. It writes profiler output to
`/dev/stderr`, parses it in memory and saves no capture. Temporary helper
and fixture files are removed at exit. Measurement mode rejects capture,
parent and fixture-regeneration modes. The ordinary contract runner keeps
its existing selection and verdict rules.


### Internal call attribution

Add `--per-call-attribution` to emit function and call-edge Ir from the
same instruction pass. `--per-call-operations add` selects only add calls.
It leaves the fixture and three native wall rounds unchanged. The default
still measures add, count and lookup. Use `--per-call-attribution-limit 0`
to emit every function and edge; the default shows the largest 25 of each.

The parser reads compressed object, file and function names. It reads
call associations and compressed position columns. Exclusive function
costs must sum to the raw collected Ir. The output keeps raw costs apart
from the calibrated aggregate and reports the wrapper cost separately.
It saves no profiler file. The [Callgrind format specification](https://valgrind.org/docs/manual/cl-format.html)
defines these records.

`functions` gives exclusive Ir/call. These costs are additive.
`call_edges` gives inclusive Ir/call and Callgrind calls per selected call.
Inclusive edges can overlap; do not sum them as a phase partition.
Uncollected ancestors can carry the selected costs on inclusive edges.
Their presence adds no Python cost. Callgrind call counts can include
calls outside collection; they are not selected-call attempt counts.
`named_phase_candidates` reports roots for phrase-table lookup, item
introspection used for text validation, and store add/merge. These are
named call boundaries, not a fitted split. LTO and inlining can hide a
boundary. A missing boundary reports `unresolved`, not zero. Inspect the
function and edge identities before assigning a semantic phase. The
mode reports `phase_partition: not_established`; it does not claim that
these roots cover every instruction or form disjoint phases. Aggregate
Ir calibration and native wall timing keep their existing definitions.
