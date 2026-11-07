# Benches — Callgrind and Massif recipes

The recipes here are attribution tools, none a per-change gate:
`callgrind-control`, `callgrind-differential` (with its sentence and
typing-sequence workloads) and `system-import-profile`, the teardown workload
of `contract-diff.py`, and the per-call C-API costs. A plain timing is one
ordinary command around the `bisect` driver. Numbers are only comparable within
one container on one host.

Performance evidence follows the [controlling rule](../source-policy.md#controlling-rule).
The recipes below are existing tools, not required counts: a number they state
for samples, rounds, repetitions, arms or passes is that recipe's default.
Callgrind and Massif are attribution tools: a section that uses them is an
attribution procedure, used only after a cheaper measurement shows a gap,
once, on the operation that shows it.

## Where measurements run

Pin comparisons run locally in the `performance` target of
`tools/oracle/Dockerfile.oracle`, inheriting the oracle's unpinned Debian testing `deps`
stage (`debian:testing`). This covers `callgrind-differential` and
`system-import-profile`. Build both the pin and subject inputs there. Never
build or run the oracle on GitHub Actions.

```sh
just performance
# Inside the shell:
uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev.measure.prepare callgrind
```

`just performance --build-only` builds without entering; `--skip-build` enters
an already built matching image. Architecture follows the host, with no
platform option. Conflicting platform environment overrides are refused;
never collect timing or memory under emulation. The oracle's amd64 default
remains for correctness captures only. The local Cargo/model cache uses a
named volume (`--volume NAME` selects another); source is mounted read-only at `/repo`.
Recipe outputs default to `/performance/results/<recipe>/`; explicit
output overrides remain supported. The harness and scratch builds live under
`/performance`. Prepared pin prefixes persist there too: preparation validates the
prefix with `tools.oracle.image.prefix_ok` (pin commits, recipe, cached libraries),
requires its `versions.txt` to equal the running image's inventory, so pin and subject
share one base, and checks every file in both recorded data manifests before reuse. Changed or damaged inputs are rebuilt. Rosetta Python is refused; use native Python.
`--cells` is valid only with `oracle` preparation.

Preparation lives in `tools/dev/measure/prepare.py`, not in image layers.
Select `callgrind` for profiled/shipping/allocation Tkrzw arms, or
`oracle --cells tkrzw` for pin data only. Model data remains local.
Both arms load Debian's packaged libtkrzw: the pin links it in the `build` stage,
the subject finds it through pkg-config, and `OXPINYIN_TKRZW_LIBRARY` points the
runtime check at it. The repository's tkrzw source build stays for CI and other hosts.
Callgrind preparation checks debug-info neutrality and collection anchors.
Additional base/PR revisions must also be built in this testing environment,
with separate Cargo target directories. Set the existing recipe options to
their resulting paths. Ordinary development remains native.

## Shipping measurement inputs

Use ordinary shipping builds. See the [function-layout ruling](../packaging.md#function-layout).
Keep the backend, artifact identity, input-digest and runtime checks for each recipe.
Each recipe runs as `python -m tools.dev measure RECIPE [ARGS]` (`just measure RECIPE`;
the measurement image has no `just` and runs the `uv run` form).
The sampling loops are serial and interleave the arms one process at a time,
because a timed or resident-set sample belongs to its arm only if nothing runs
beside it.

A plain timing needs no recipe. Build the `bisect` driver and run its `--perf`
mode once per library; each run prints one JSON line:

```sh
cc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$WORK/bisect" tools/bisection/bisect.c -ldl
PERF_MODE=speed "$WORK/bisect" --perf "$SO" "$DATA"
```


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
env RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none" CARGO_TARGET_DIR=target/tkrzw \
  cargo cinstall -v --locked --release -p oxpinyin-capi \
  --destdir=target/callgrind-tkrzw/profile --prefix=/usr
env CARGO_TARGET_DIR=target/tkrzw \
  cargo cinstall -v --locked --release -p oxpinyin-capi \
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

The sentence collection measures sentence guessing alone: rank-0 retrieval and release happen afterwards.
Typing collects all 13 prefixes, from `n` through `nihaozhongguo`, in order.
Output records each operation's returns, rank-0 sentence, and every typing
candidate's text/type/N-best index in physical order. Observation, printing,
init, instance allocation and teardown are outside collection.

Supply actual library files, a complete backend-matched system directory,
and separately built plain shipping files for the two debug-info neutrality
checks. Use the shipping Cargo profile and only
`RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"` for the profiled files. Install through
`just packaging install` for the shipping artifact. The campaign
checks `DT_NEEDED` against `--cell`, DWARF, code neutrality,
and the sound-tkrzw runtime before collecting. It runs the harness build and
all collections serially; do not overlap other work.

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
and suppress the per-invocation comparison. The typing sequence
alone reports a varying control as `DIAGNOSTIC` (rule `randomstate`, ranges,
no tolerance) when every varying function is `dedup_by_text_keep_first`,
`prepend_nbest_rows` (packaged fat-LTO builds inline
`dedup_by_text_keep_first` into it), `dedup_ranked_by_text`,
`order_window_tail` (std `HashSet`/`HashMap` with the per-process
`RandomState` SipHash-1-3 keys) or `memcmp`, their equality callee; any
other varying function is `INVALID`.

The output directory must be new. `result.json` reports raw counts, control
spread and Ir per sentence/complete typing sequence; `environment.json`
records the host, inputs and library identities. Exact commands, worker
output and profiles remain local for attribution with
`callgrind_annotate --inclusive=yes`. They are disposable verification data,
not PR attachments. Harness `elapsed_ns` is collected under Valgrind and is
not a native timing result. These commands make no wall-time or heap claim.

For a same-run native time comparison, add `--native-rounds 40
--native-iterations 20`. This optional phase runs before Callgrind, using
separate equal-length copies of the supplied **plain shipping** libraries,
the same CPU pin and worker anchors, and rotating base, PR, pin and both
same-bytes controls. It validates every complete observation and reports
native `elapsed_ns` per invocation in `native.json`, independently of the
profiled arm's counters. `--native-rounds` requires at least 20; its default
is zero (off), while `--native-iterations` defaults to 20 and does not alter
Callgrind's `--iterations`. No warmup is discarded; all invocations of the
native process are included. The shared paired-bootstrap statistics give
PR/base median ratios and 95% intervals, plus the existing same-bytes time
control stability check. Unstable controls are DIAGNOSTIC; their tolerance
is not permission for a product regression. Report the interval and control
limits, rather than treating an interval containing one as proof of exact
time equality. This phase measures no heap usage.

## Native teardown workload

`contract-diff.py --teardown` adds one workload to the existing driver. It
builds `bisect.c` with its `teardown.h` mode.
Python provisions private profiles, interleaves subjects and reads the paused
native child's memory. ABI calls are bracketed in C. Other bisect and contract
modes keep their existing output.

All stages (`provision`, `verify`, `pilot`, `confirm`, `repeat`, `multi`,
`attribute`, `init-attribute`) support the selected cell, BDB, KC or tkrzw. Provisioning writes
user profiles through that cell's own pin. Comparison stages retain
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
`just packaging install`. Keep one campaign active. The work directory and measured
library copies must be on ext4; tmpfs gives misleading private-dirty
attribution for clean library pages.

```bash
teardown_work=$(mktemp -d "$PWD/target/teardown.XXXXXX")
trap 'rm -rf "$teardown_work"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
uv run --no-project --managed-python \
  python -B -X lazy_imports=normal tools/bisection/contract-diff.py \
  bdb "$pin_prefix" "$shipping_prefix/lib/libpinyin.so" \
  --zhuyin-so "$shipping_prefix/lib/libzhuyin.so" \
  --teardown --teardown-stage verify --teardown-work-dir "$teardown_work"
```

After verification, select `pilot`, then `confirm` explicitly.
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
in-process lifecycles. Run cold campaigns only when other measurement runs
are idle: dependency and system-data cache
pages can be shared across runs.

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

The existing `count` probe also reports absolute M0/I1 Pss per mapping and
the total from that same smaps read, with the residency control above.
These values never come from the later rollup read.

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

The teardown profiles also include `user`, `user8` and
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
uv run --no-project --managed-python python -B -X lazy_imports=normal \
  tools/bisection/contract-diff.py bdb "$PIN_PREFIX" "$PR_PREFIX/lib/libpinyin.so" \
  --base-pinyin-so "$BASE_PREFIX/lib/libpinyin.so" --teardown \
  --teardown-stage user-verify --teardown-facades pinyin \
  --teardown-cases empty,user,user8,usersparse --teardown-work-dir "$WORK"
# After the bounded preflight passes, change user-verify to user-phrase.
```

To select only needed collections, `--teardown-user-stages` accepts a
comma-separated subset of `timing,memory,allocations,heap,instructions`.
Omitting it retains all stages and their existing rounds. A selection without
timing still runs one untimed equivalence pass per arm before collecting.
Explicit selections provision only the named cases, plus `user8` when the
full workload needs its write recipe; existing manifests are validated and reused.

For only `user8`'s R0–R1 instruction counts, use the existing read anchor:

```bash
uv run --no-project --managed-python python -B -X lazy_imports=normal \
  tools/bisection/contract-diff.py bdb "$PIN_PREFIX" "$PR_PREFIX/lib/libpinyin.so" \
  --base-pinyin-so "$BASE_PREFIX/lib/libpinyin.so" --teardown \
  --teardown-stage user-phrase --teardown-facades pinyin --teardown-cases user8 \
  --teardown-user-stages instructions --teardown-user-ir-window read \
  --teardown-work-dir "$WORK"
```

`read` requires the instruction-only selection. The same native worker executes
the unchanged 10,000-read unit and cleans up without enumeration, export or
writes. Init and instance/GArray allocation remain outside the collection window.
One untimed read-equivalence pass checks all arms' read hashes, diagnostics and
final profiles, followed by three fresh Callgrind rounds per arm, with the existing
rotation, parser and byte-identical PR control. No timing, smaps, allocation or
Massif collection runs. The `user-selection` record names the narrower scope;
it does not establish equivalence for omitted operations. `user-verify` accepts
the same selectors for a standalone preflight, but the instruction run already
performs it. Report `user-ir`'s counts and ranges from one run; do not retry an
inconclusive comparison.

By default, `user-verify` checks the census, getter/enumeration/export hashes and saved
`user.bin` bytes against the pin. The broader `verify` stage still checks its
full candidate transcripts and reopen behavior; a mismatch there stays
NOT COMPARABLE and must be reported separately. A passing bounded preflight
does not establish end-to-end candidate equivalence.
`confirm` and `init-attribute` also report the existing worker-workload
scope (`samples`, diagnostics and final files) separately from the broader
witness script. A script-only difference limits that end-to-end comparison;
it does not block an identical measured workload. Every candidate field in
the measured sample remains compared, and a changed sample, diagnostic or
file blocks that case. Other verified cases continue. `verify` retains its
strict broad verdict and complete-observation hashes.
Pass the existing `--teardown-summary-dir` option with an empty local
directory to retain complete preflight observations and their hashes for
diagnosis, including failures. This never changes the preflight verdict.

`user-phrase` rotates base, PR, the byte-identical PR control and pin. By default it uses
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

To measure a stack in one run, add one `--teardown-arm NAME=PINYIN_SO[:ZHUYIN_SO]`
per further revision (NAME: one to four lowercase letters or digits) to the
`confirm`, `init-attribute`, `user-verify` or `user-phrase` stage. It needs
`--base-pinyin-so`. Each arm rotates with base, PR, control and pin, passes the
same equivalence preflight, and its timing records carry `arm_ratios` against
base and pin under the PR's same-bytes control. Its profiler rows are printed
beside the others.

With `--teardown-summary-dir`, `user-phrase` also keeps each arm's first
write-window Callgrind profile (`pinyin-CASE-SIDE-user-write.callgrind`) and
its Massif profile (`pinyin-CASE-SIDE-user-heap.massif`) for local diagnosis:
`callgrind_annotate` gives instructions by function and `ms_print` the heap
peak by allocation site.

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

The teardown campaign reports through it. Nothing is archived or uploaded.

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

Three limits apply to every result, and every record
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

A page with `in_rounds` below `rounds` was newly resident in only some of
the rounds.

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
**every** command. Repeat each pin/subject command as often as the decision
requires. For an instruction or heap attribution, take the median of
whole-process Callgrind `Ir` and of Massif's maximum
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
run. Its BDB subject was built from that exact commit, with `$WORK` a scratch
directory:

```sh
CARGO_BUILD_JOBS=1 CARGO_TARGET_DIR=$WORK/target-main-bdb \
  just packaging install libpinyin \
    --prefix=$WORK/stage-main-shipped \
    --libdir=$WORK/stage-main-shipped/lib \
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

## Attributing a heap gap to callers

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
(`libkyotocabinet16v5-dbgsym`) do fix the KC half and nothing else.

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


## Writing it up

Follow [results and reproduction](../../CONTRIBUTING.md#results-and-reproduction):
state the conclusion, final measurements and reproduction commands. Include
the revisions, workload, backend, build recipe, host/container and controls
needed to interpret the comparison. One invocation may reproduce a whole
table. Link existing setup procedures; a separate findings file is optional.
A change that alters an algorithm, a data structure, allocation behaviour or
an input bound states its source-derived complexity argument
([source policy](../source-policy.md#complexity-evidence)).
A change that trades time for space or the reverse must say so and justify
it ([source policy](../source-policy.md#performance-standard)).
Historical reports are examples of results,
not templates requiring their full provenance sections to be copied.

### Measure compared arms in one run

**A measurement is the ratio of artifacts timed in the same job on the same
machine, never a number carried over from another run.** It answers the
[controlling rule](../source-policy.md#controlling-rule)'s concrete question
with the arms and inputs relevant to it. When the question compares arms,
build and run them in one job on one machine (same CPU, same data),
**interleaving** them run by run, and report only those figures and the
ratios between them.
New measurement tooling needs the maintainer's approval.
Do not supply an out-of-tree measurement controller.
No absolute figure from an earlier run, another host or another CPU is a
baseline: Callgrind `Ir` and wall time both move with the CPU (glibc picks
`memcpy` and friends at run time), so a baseline taken elsewhere cannot be
compared with a PR measured here. Earlier snapshots in this repository
remain records of what was measured where; they are not baselines to
subtract from. The pin comes from the reproducible oracle
(`docs/runbooks/oracle.md`, section 2a), so the pin's hash is the same on
every machine.

### State the Valgrind version when used

A report that uses Callgrind or Massif states the Valgrind version it used, as printed by
`valgrind --version`, next to the figures. Callgrind `Ir` and Massif heap
peaks move with the Valgrind release as well as the CPU, so a report
without the version cannot be compared with another run. Normal cloud
provisioning (`tools/cloud/setup.sh`) installs it idempotently and prints the
version. Ordinary native setup does not require it. On other measurement hosts, install it explicitly.
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
provide the build command; do not duplicate it for every figure.

Shipping-library size reporting follows [constitution §2](../constitution.md),
independently of any measurement.

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
units lower on amd64 and 0.032 to 0.038 lower on arm64**, with no mechanism
established. A record that
does not say which one it built cannot be read as a statement about the
product.

A shortcut does not substitute for the command line; it has already misled a
reader:

- **The artifact filename discriminates only after `d32557e3`
  (2026-08-29T16:33:26Z)**, which introduced the `libpinyin.so.15` SONAME.
  Before it, cargo-c installed under the same `libpinyin_capi.so` name the
  cargo build produces.

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
Run this command:

```sh
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

The recipe's default is three interleaved rounds per side and facade.
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
