# Benches — comparison and profiling recipes

This page gives the measurement recipes and how to report their results.
The recipes are attribution tools, and none of them is a per-change gate:

- `callgrind-control`;
- `callgrind-differential`, with its sentence and typing-sequence workloads;
- `system-import-profile`.

A plain timing is one ordinary command around the `bisect` driver. Numbers are comparable only within
one session on one host. Native macOS source-revision commands are described
in [Native macOS source revisions](#native-macos-source-revisions).
`just measure compare` reports different metrics on macOS, Windows and Linux.
Numbers from different platforms are not comparable: the measured loop, the
counters and the memory figures differ.

The [controlling rule](../source-policy.md#controlling-rule) decides when to measure and which measurement to use.
The recipes below are existing tools, not required counts. A number that a recipe states
for samples, rounds, repetitions, arms or passes is the default of that recipe.
A section that uses Callgrind or Massif is an attribution procedure under that rule.

## Where measurements run

Pin comparisons run locally in the `performance` target of
`tools/oracle/Dockerfile.oracle`. This target inherits the unpinned Debian testing `deps`
stage of the oracle (`debian:testing`). It covers `callgrind-differential` and
`system-import-profile`. Build both the pin and the subject inputs there. Never
build or run the oracle on GitHub Actions.

```sh
just performance
# Inside the shell:
uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev.measure.prepare callgrind
```

These rules apply to the `performance` shell:

- `just performance --build-only` builds without entering. `--skip-build` enters
  an already built matching image.
- The architecture follows the host; there is no platform option. Conflicting platform environment overrides are refused.
  Never collect timing or memory under emulation. The amd64 default of the oracle
  stays for correctness captures only.
- The local Cargo/model cache uses a
  named volume (`--volume NAME` selects another). The source is mounted read-only at `/repo`.
- Recipe outputs default to `/performance/results/<recipe>/`. Explicit
  output overrides stay supported. The harness and scratch builds live under
  `/performance`.
- Each `callgrind-differential` capture and each `system-import-profile` run needs a new
  output directory. Both refuse a directory that exists. A second differential capture
  must set `CG_OUT` to a new path.
- Prepared pin prefixes also persist under `/performance`. Before reuse, preparation:
  - validates the prefix with `tools.oracle.image.prefix_ok` (pin commits, recipe, cached libraries);
  - requires its `versions.txt` to equal the inventory of the running image, so that the pin and the subject
    share one base;
  - checks every file in both recorded data manifests.

  Preparation rebuilds changed or damaged inputs.
- Rosetta Python is refused; use native Python.
- `--cells` is valid only with `oracle` preparation.

Preparation lives in `tools/dev/measure/prepare.py`, not in image layers.
Select `callgrind` for the profiled and shipping Tkrzw arms, or
`oracle --cells tkrzw` for pin data only. Model data stays local.
Both arms load the packaged libtkrzw of Debian:

- the pin links it in the `build` stage;
- the subject finds it through pkg-config;
- `OXPINYIN_TKRZW_LIBRARY` points the runtime check at it.

The tkrzw source build of the repository stays for CI and other hosts.
Callgrind preparation checks debug-info neutrality and collection anchors.
Build additional base or PR revisions in this testing environment as well,
with separate Cargo target directories. Set the existing recipe options to
the resulting paths. Ordinary development stays native.

## Shipping measurement inputs

Use ordinary shipping builds. See the [function-layout ruling](../packaging.md#function-layout).
For each recipe, keep the backend, artifact identity, input-digest and runtime checks.
Use `just measure-callgrind-control`, `just measure-callgrind-differential` or
`just measure-system-import-profile`. Their direct `uv run` invocations are in the justfile.
The measurement image has no `just`, so it runs the `uv run` form.
The sampling loops are serial and interleave the arms one process at a time.
A timed or resident-set sample belongs to its arm only if nothing runs
beside it.

A plain timing needs no recipe. Build the `bisect` driver and run its `--perf`
mode once for each library. Each run prints one JSON line:

```sh
cc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$WORK/bisect" tools/bisection/bisect.c -ldl
PERF_MODE=speed "$WORK/bisect" --perf "$SO" "$DATA"
```

## Callgrind debug-info control

Both campaign arms use the unchanged shipping Cargo profile. The profiled arm
adds `RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"`. Cargo appends these flags after its
`-C strip=debuginfo`, so the final strip setting of rustc keeps DWARF without a change to
the profile/metadata hash. The `.text` of the profiled arm is expected to be byte-identical to plain
shipping; `callgrind-control` checks this. Debuginfo alone is stripped back out.

`callgrind-control` verifies the DWARF compilation units in the profiled library. It
runs the unchanged neutrality check against the separately built plain shipping
library. The campaign checks neutrality again before collection and uses the
plain shipping arm for wall-clock timing.

To check preparation natively, without a pin run or profile collection:

```bash
env RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none" CARGO_TARGET_DIR=target/tkrzw \
  cargo cinstall -v --locked --release -p oxpinyin-capi \
  --destdir=target/callgrind-tkrzw/profile --prefix=/usr
env CARGO_TARGET_DIR=target/tkrzw \
  cargo cinstall -v --locked --release -p oxpinyin-capi \
  --destdir=target/callgrind-tkrzw/control --prefix=/usr
just measure-callgrind-control target/callgrind-tkrzw/profile target/callgrind-tkrzw/control
```

Run from a shell that has no Cargo profile or RUSTFLAGS overrides already exported.
For BDB, replace `tkrzw` with `bdb` in the feature and output paths. Both
comparisons must pass. These builds should be case (a), identical `.text`.
This check needs no runtime data and no oracle build. The verbose build
output records the order of the compiler flags. `readelf --debug-dump=info`
on the profiled library shows its DWARF units.

### Sentence and typing-sequence Callgrind campaign

`just measure-callgrind-differential --workload sentence|typing-sequence`
extends the `bisect` driver. Its no-argument campaign and `bisect --perf`
use the 20-input keystroke-cycle workload. The frozen
`system-import-profile --sentence-fixture` mode still requires its 500-row
fixture. It is not a general prefix-sequence driver.

Both workloads use the pinyin facade, options `0x18a`, an empty temporary
user directory for each process, and the same instance through each iteration.
They never train or save. The default input is `nihaozhongguo`.

| Workload | Setup outside collection | Collected anchor |
| --- | --- | --- |
| `sentence` | Reset and parse the complete input | `bisect_sentence_unit`: one `pinyin_guess_sentence` |
| `typing-sequence` | One reset before the sequence | `bisect_typing_unit`: parse, `pinyin_guess_sentence`, then `pinyin_guess_candidates` at offset 0, sort `0x1e`, for each accumulated ASCII prefix |

The sentence collection measures sentence guessing alone. Rank-0 retrieval and release happen after it.
The typing collection covers all 13 prefixes, from `n` through `nihaozhongguo`, in order.
The output records the returns of each operation, the rank-0 sentence, and the text, type and N-best index of every typing
candidate in physical order. Observation, printing,
init, instance allocation and teardown are outside collection.

Supply these inputs:

- actual library files;
- a complete backend-matched system directory;
- separately built plain shipping files for the two debug-info neutrality
  checks.

For the profiled files, use the shipping Cargo profile and only
`RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"`. For the shipping artifact, install through
`just packaging install`. Before collection, the campaign
checks `DT_NEEDED` against `--cell`, DWARF, code neutrality,
and the sound-tkrzw runtime. It runs the harness build and
all collections serially; do not overlap other work.

```bash
just measure-callgrind-differential --workload sentence --cell tkrzw \
  --main "$MAIN_PROFILE_SO" --pr "$PR_PROFILE_SO" --pin "$PIN_SO" \
  --main-shipping "$MAIN_SHIPPING_SO" --pr-shipping "$PR_SHIPPING_SO" \
  --system-dir "$DATA" --out "$WORK/sentence"
just measure-callgrind-differential --workload typing-sequence --cell tkrzw \
  --main "$MAIN_PROFILE_SO" --pr "$PR_PROFILE_SO" --pin "$PIN_SO" \
  --main-shipping "$MAIN_SHIPPING_SO" --pr-shipping "$PR_SHIPPING_SO" \
  --system-dir "$DATA" --out "$WORK/typing-sequence"
```

For BDB, use `--cell bdb` and BDB libraries and data. For the tooling smoke check,
set main and PR to the same profiled file, and set both shipping arguments to
the same plain file. The options are:

- `--input` accepts lowercase ASCII pinyin and apostrophes;
- `--iterations` defaults to 1;
- `--rounds` defaults to 3 fresh processes per arm (minimum 2);
- `--cpu` defaults to the first allowed CPU.

No warmup is excluded: these are
first-call and sequence scopes, not the steady cycle of `bisect --perf`.

Every round rotates main, PR, pin and byte-identical copies of main and PR
serially, with equal-length private library paths. These rules decide the result:

- A control passes only
  when all instruction totals in its identical pair are equal, with zero
  tolerance, and the full observations agree.
- The main and PR observations must also agree.
- The pin observations are compared and reported separately. This campaign
  does not classify known pin divergences or replace the ranking/recount gates.
- Profiler creator/PID/command/Ir-only events and reconciled exclusive totals
  are required.
- Invalid controls, observations or collections return nonzero
  and suppress the per-invocation comparison.
- The typing sequence
  alone reports a varying control as `DIAGNOSTIC` (rule `randomstate`, ranges,
  no tolerance) when every varying function is one of these:
  `dedup_by_text_keep_first`;
  `prepend_nbest_rows` (packaged fat-LTO builds inline
  `dedup_by_text_keep_first` into it); `dedup_ranked_by_text`;
  `order_window_tail` (std `HashSet`/`HashMap` with the per-process
  `RandomState` SipHash-1-3 keys); or `memcmp`, their equality callee. Any
  other varying function is `INVALID`.

The output directory must be new. `result.json` reports raw counts, control
spread, and Ir per sentence or per complete typing sequence. `environment.json`
records the host, inputs and library identities. Exact commands, worker
output and profiles stay local for attribution with
`callgrind_annotate --inclusive=yes`. They are disposable verification data,
not PR attachments. Harness `elapsed_ns` is collected under Valgrind and is
not a native timing result. These commands make no wall-time or heap claim.

For a same-run native time comparison, add `--native-rounds 40
--native-iterations 20`. This optional phase runs before Callgrind. It uses
separate equal-length copies of the supplied **plain shipping** libraries,
the same CPU pin and worker anchors. It rotates base, PR, pin and both
same-bytes controls. It validates every complete observation and reports
native `elapsed_ns` per invocation in `native.json`, independently of the
counters of the profiled arm. The options are:

- `--native-rounds` requires at least 20. Its default is zero (off).
- `--native-iterations` defaults to 20 and does not change
  the `--iterations` of Callgrind.

No warmup is discarded; all invocations of the
native process are included. The shared paired-bootstrap statistics give
PR/base median ratios and 95% intervals, and the existing same-bytes time
control stability check. Unstable controls are DIAGNOSTIC. Their tolerance
is not permission for a product regression. Report the interval and the control
limits. Do not treat an interval that contains one as proof of exact
time equality. This phase measures no heap usage.

## BDB guess, training, and import profiles

`tools/bisection/system-import-profile.c` extends the import driver with
three full-process workloads. Build the driver in the measurement
environment:

```sh
cc -std=gnu11 -Wall -Wextra -Werror -O2 tools/bisection/system-import-profile.c -o "$HARNESS" -ldl
```

Through a comparison, use the same BDB system data, a fresh empty user directory, the same CPU, pin SO, and
subject SO. Identify the subject build, and use
matched pin, main and subject conditions for each comparison.

The frozen sentence fixture is `fixtures/w4/oracle-sentence-surface.txt`
(SHA-256 `d119f3a7f665333ef5717b1925171799c29e710d4ad1575748e0a9d3ae2c9e21`).

- `--sentence-fixture` replays its 500 rows with a nonempty sentence field.
  It guesses a sentence and candidates for each row.
- `--train-fixture` takes the
  first 200 such rows. For each row, it checks a full parse and a successful guessed train at
  index 0. It then saves once.

Both set options `0x18a`. Candidate
sorting in the guess workload uses `0x1e`.

For `--phrase-file`, generate the input from the same BDB pin SO and data
with the **unmeasured** `--export-system` of the driver:

1. The export walks libraries
   1–4 in token/pronunciation order.
2. Select the first 10,000 distinct phrase
   texts.
3. Keep the first pronunciation of each.
4. Set every import count to 5.

This exact byte recipe writes tab-separated
`library<TAB>phrase<TAB>pinyin<TAB>5<LF>` rows. The first row is
`1<TAB>锕<TAB>a<TAB>5` and the last row is
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
requires. For an instruction or heap attribution:

- take the median of
  whole-process Callgrind `Ir` and of the maximum Massif
  `mem_heap_B`;
- report the spread as `(max − min) / median`. The Massif value
  excludes stacks, allocator overhead and file mappings;
- check output
  equivalence and operation counts before you compare costs;
- keep profiler
  output in streams or memory, not in archives, logs, captures or profile
  files.

The commands below write the Valgrind output to stderr:

```sh
taskset -c 0 valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --sentence-fixture fixtures/w4/oracle-sentence-surface.txt
taskset -c 0 valgrind --tool=massif --time-unit=i --heap=yes --stacks=no --peak-inaccuracy=0.0 --detailed-freq=1 --max-snapshots=1000 --massif-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --sentence-fixture fixtures/w4/oracle-sentence-surface.txt

taskset -c 0 valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --train-fixture fixtures/w4/oracle-sentence-surface.txt
taskset -c 0 valgrind --tool=massif --time-unit=i --heap=yes --stacks=no --peak-inaccuracy=0.0 --detailed-freq=1 --max-snapshots=1000 --massif-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --train-fixture fixtures/w4/oracle-sentence-surface.txt

taskset -c 0 valgrind --tool=callgrind --cache-sim=no --branch-sim=no --callgrind-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --phrase-file "$PHRASE_FILE"
taskset -c 0 valgrind --tool=massif --time-unit=i --heap=yes --stacks=no --peak-inaccuracy=0.0 --detailed-freq=1 --max-snapshots=1000 --massif-out-file=/dev/stderr "$HARNESS" "$SO" "$DATA" "$USER_DIR" --phrase-file "$PHRASE_FILE"
```

Remove the disposable phrase input after the runs. The expected stdout
SHA-256 values are:

- `50dfc9c87e8eb2ce9121ac4f138584ab226a69308a9becc727cfca1c68c3100d`
  for 500 guesses;
- `e5855249fc5c7cc2c4de81aa1e792c2f41152a84c8a3e10f04c52a824bda698d`
  for 200 successful guessed training steps;
- `34e98cd8bf5a191b27e0b8fc50ffdefca9e76a2dab7dd01106cf6b6d3c9eec1d`
  for 10,000 imports and 95,698 exported rows.

## Attributing a heap gap to callers

DHAT names the allocation *site*. An attempt to name its *caller* across the C-ABI
boundary from the Valgrind stack fails, and the failure looks like a valid result.

**The Valgrind unwind of the dlopened Rust cdylib is not trustworthy above
`kcdbget`.** It resolves the libpinyin cell correctly. On the
oxpinyin cell, it produces wrong chains that the symbol table makes look plausible. Examples are a chain with
`Arc<T>::drop_slow` calling `dict::ucs4_walk_key`, and
`pinyin_iterator_add_phrase` directly beneath `main`. Each address
is inside the function that it names, so nothing looks broken. None of these options changes the result:
`--num-callers=24`, resolving at `addr - 1`, `debug = 2` with `lto =
false`, or `-C force-frame-pointers=yes`. All of them produce
byte-identical stacks. Kyoto Cabinet debug symbols
(`libkyotocabinet16v5-dbgsym`) fix the KC half and nothing else.

**Use callgrind caller→callee edges instead.** They give exact call counts and need no
stack walk. Follow two rules:

* **Key on the `(object, file, name)` triple of the callee, not on the demangled
  name.** `BasicDB::get(char const*, unsigned long, char*, unsigned
  long)` is a substring of the
  `::VisitorImpl::visit_full` of that function, so a name match sums both.
* **An inlined callee has no call edge.** `BasicDB::check` and most
  `BasicDB::get` sites inline into their caller and under-report.
  Where inlining is possible, count something that cannot be inlined, such as a
  virtually-dispatched visitor or a heap allocation. Confirm the count
  against a second marker of that kind.

A profiling build may stand in for the shipped artifact here: block
counts are identical across the shipped recipe, `debug = 2` with LTO
off, and forced frame pointers.

## Writing it up

Follow [results and reproduction](../../CONTRIBUTING.md#results-and-reproduction) and the
[measurement rules](../../CONTRIBUTING.md#measurements).
For a measurement, the details needed to interpret the comparison are
the revisions, workload, backend, build recipe, host/container and controls.
The [complexity argument](../source-policy.md#complexity-evidence) applies to changes that alter
an algorithm, a data structure, allocation behaviour or an input bound.
The [time/space trade-off rule](../source-policy.md#performance-standard) applies to every change.

### Measure compared arms in one run

Follow the [measurement rules](../../CONTRIBUTING.md#measurements).
Interleave compared arms run by run on the same CPU and data.
Callgrind `Ir` and wall time depend on the CPU, including runtime libc dispatch.

### State the Valgrind version when used

A report that uses Callgrind or Massif states the Valgrind version that it used, as
`valgrind --version` prints it, next to the figures. Callgrind `Ir` and Massif heap
peaks change with the Valgrind release as well as with the CPU. A report
without the version therefore cannot be compared with another run. Normal cloud
provisioning (`tools/cloud/setup.sh`) installs Valgrind idempotently and prints the
version. Ordinary native setup does not require it. On other measurement hosts, install it explicitly.
On Debian/Ubuntu, install the distribution package with `sudo`:

```bash
sudo apt-get update
sudo apt-get install -y valgrind
valgrind --version
```

In a root development container, omit `sudo`. On Fedora/RHEL, install the available
`valgrind` package with `dnf`. Do not install measurement tools for ordinary
contributions.

### State the build recipe, per artifact

Identify each timed subject by revision, backend and build recipe, including
the profile and relevant flags. A maintained recipe and its arguments can
supply the build command; do not repeat it for every figure.

Shipping-library size reporting follows [constitution §2](../constitution.md),
independently of any measurement.

The hashes and loaded-library checks that the tools need stay tool-internal
checks. A report needs the results and the reproduction recipe. It does not need a checklist
of their verdicts, a checksum manifest or a full `NEEDED` inventory. Include a
specific hash or dependency only when it explains a concrete build-identity
or loader discrepancy.

`cargo cinstall` produces the shipped library; `cargo build` produces a bisection fixture.
Their performance differs, so identify the build command, not just the artifact filename.

### Evidence: what is committed and what is not

Follow the [reporting policy](../../CONTRIBUTING.md#results-and-reproduction).
Keep shared readers and calculations in repository Python tooling.
Keep all measurement controls and rejection rules, including required exact checks.


## Native macOS source revisions

Use full Xcode on an Apple silicon Mac. The runner probes the installed tools;
Command Line Tools alone do not provide xctrace. Native revision comparisons
follow the [measurement rules](../../CONTRIBUTING.md#measurements).
They do not compare against the pin.

```sh
just measure compare --base origin/main --head origin/main
just measure profile --revision origin/main --template 'CPU Profiler'
just measure profile --revision origin/main --template Allocations
```

These commands use the same typing corpus in `bisect.c`. Schedule defaults
live in `tools/dev/measure/recipes.py`; `just measure --help` lists overrides.
The budget bounds the measured schedule, excluding preparation. An exhausted
budget fails the campaign and preserves completed runs. Keystrokes are work
items, not independent statistical observations. Estimates use process pairs.

Each distinct source SHA builds once through `tools.dev.packaging`'s install
procedure: locked release, the `shipped` feature and Cargo's default backend.
No backend flag is passed. Cargo metadata supplies the default feature names
for the local report. The effective symbol overrides are
`RUSTFLAGS='-Cdebuginfo=1 -Cstrip=none'` and `OXPINYIN_DSYM=1`.
The latter retains the final export-controlled Darwin link's LTO object with
`-object_path_lto` until `dsymutil` finishes. The dylib and dSYM must have
matching arm64 UUIDs. Product code uses no profiling profile or instrumentation.
Set `VCPKG_ROOT` to the checkout installed by the [macOS setup](setup.md#macos).
Measurement builds put its `installed/arm64-osx-oxpinyin/lib/pkgconfig` first
in `PKG_CONFIG_PATH` for both arms and require that triplet's `tkrzw.pc`.
Each invocation builds into a fresh directory; no build is reused across sessions.
Within that invocation, each distinct source SHA builds once. The local session
records cold build time.

Before accepting this build recipe, run its one-time neutrality validation:

```sh
just measure neutrality --revision origin/main
```

This compares the symbol-bearing packaged build with a plain release build.
Executable Mach-O section bytes, addresses and arm64 disassembly must be
identical. A difference fails; stop without measuring it. Routine comparisons
reuse the symbol-bearing build and do not build a plain artifact.

Builds and disposable sessions live under `just cache-root`, with a separate
prefix for each arm even for the same SHA. The same-binary control runs arm A
against A′, a second byte-identical copy of A's built product and driver staged
in its own prefix exactly as B is, so the control also covers effects tied to a
newly staged directory; it reports A′ under the label `B`. The three prefixes
are named `A`, `B` and `C` on every platform, so the same bytes sit at paths of
one length. A′ passes the same
staging, loaded-image and UUID checks as any arm. One normally linked driver is
built, its dependency is rewritten to `@rpath`, then it is ad-hoc signed with
`com.apple.security.get-task-allow` and without hardened runtime, and verified
before identical copies are staged. This permits the Instruments task-port
request; the product dylib is never re-signed. Its executable-relative rpath
selects the arm. Before measuring, `dladdr` and the loaded Mach-O UUID verify
the resolved C function against the staged dylib. No `DYLD_*` selection is
required. xctrace receives workload settings through `--env`.

The driver brackets all typing cycles with monotonic wall-clock and
`getrusage(RUSAGE_SELF)` readings. `wall_seconds` and `cpu_seconds` cover that
loop, excluding initialization and teardown. `operations` counts completed
prefix steps; the return/count fingerprint must agree across runs.
The driver also reads `proc_pid_rusage` (`rusage_info_v4`) immediately before
and after that loop. `loop_instructions` and `loop_cpu_cycles` are the
`ri_instructions` and `ri_cycles` deltas, as counts. `loop_runnable_seconds`
is the `ri_runnable_time` delta converted with `mach_timebase_info`: time on a
core or waiting for one. The kernel advances it at scheduling events only, so
it is coarse and a loop shorter than a scheduling quantum can read 0.

Lead a macOS report with the `loop_instructions` B/A ratio and wall time.
Runnable time explains host contention: each arm reports it over its CPU time,
and an excess is time spent waiting for a core. It has no B/A ratio.
Instruments remains the tool for attribution.
Lead a Windows report with the `loop_cpu_cycles` B/A ratio and wall time; its
memory figure is the `peak_private_commit_bytes` B minus A difference.

`lifetime_max_phys_footprint_bytes`, read with `proc_pid_rusage` after teardown,
is the memory headline. It is a process-lifetime peak, not live memory at a
checkpoint. `peak_rss_bytes` is the direct child's own `wait4` diagnostic;
Darwin reports bytes. Neither field is a Linux residency category.
A profile launch has no direct-child RSS reading for the driver and reports
none; xctrace's own RSS is not substituted.
No loop-scoped peak footprint is reported. `ri_interval_max_phys_footprint`
restarts only through `proc_reset_footprint_interval` or `proc_rlimit_control`,
and the macOS SDK declares neither: its public headers carry the
`RLIMIT_FOOTPRINT_INTERVAL` flag names but no prototype and no manual page.

Profiles cover the launched process lifecycle. They are not scoped-loop timing
estimates. The runner exports table data, resolves XML references, and requires
the driver PID, usable time/stack rows and a known C entry point or `RuntimeDict::fill_lookup` frame from the
measured UUID. Allocations exports the TOC-listed `Allocations` → `Allocations List` table
under `tracks/track/details/detail`. It validates the single launched driver's
PID and exit status, then each row's address, size, category, timestamp and
backtrace. These rows are allocations still outstanding when the process exited, not live memory
at a lifecycle checkpoint or allocation traffic. The TOC-listed `Statistics` detail
provides total, transient and persistent allocation counts and bytes, overall and
per category; the runner validates those fields and their totals.
Product-frame addresses are resolved with `atos` and the UUID-matched dSYM;
a known product function must resolve. `xctrace show` is not used for this
legacy detail. Missing data,
wrong-process captures and unresolved product frames fail visibly.

Installed xctrace exposes neither CPU Profiler deferred recording nor an
Allocations backtrace setting. Use their defaults; there is no options file or
workaround. Allocation backtraces must still exist in the exported capture.
`xctrace export` exposes no time-range switch. For an explicitly selected
range, inspect the trace with `xctrace toc`, then use `xctrace show` with the
reported track/detail and `--time-start` plus `--duration`. Do not describe a
whole-trace export as a scoped-loop export.

Other installed instruments use the same profile mode:

```sh
just measure profile --template 'System Trace'
just measure profile --template 'Processor Trace'
just measure profile --template Allocations --instrument 'VM Tracker'
```

VM Tracker is an instrument added to a named template, not a standalone
template. Availability of a template does not prove host recording permission;
a failed required capture fails the command. Traces, exports and JSON reports
are disposable local output. Do not attach them or commit them. Publish only
reproduction commands and a concise result table in the PR. The initial
main-against-main validation is not a performance claim.

### Native Windows comparisons

`just measure compare` also runs on Windows x64 with MSVC and `VCPKG_ROOT`
set as in setup.md. Both arms use that checkout's `installed/x64-windows`
pkg-config metadata and runtime DLLs. Each distinct SHA builds once with
`cargo build --locked --release -p oxpinyin-capi`, default features and no
added linker or debug flags. The artifact is `pinyin_capi.dll`, not a packaged
libpinyin DLL. The import library links one MSVC C driver; identical copies
sit beside each arm's DLL. A′ is staged as on macOS and passes the same DLL and
dependency checks as any arm. The driver checks the imported ABI function's
owning module before work. Preflight and completed module inventories must
identify the same dependency files for both arms: every loaded module outside
the Windows system directory, such as tkrzw and GLib, plus the C runtime.
Other system modules follow the host, not the arm, and are not compared.
The driver verifies the product's imported GLib allocator; it never substitutes
its own `free` on Windows.

QPC elapsed seconds, the QueryProcessCycleTime delta and GetProcessTimes
user+kernel seconds bracket the same typing loop as macOS, excluding
initialization and teardown. Each CPU figure covers all threads of the process.
`loop_cpu_cycles` is a count of CPU cycles. It is never converted to seconds:
Microsoft documents against that conversion. A zero cycle delta is a broken
capture and fails. `cpu_seconds` is secondary; GetProcessTimes advances in
1/64-second ticks. A sample whose `cpu_seconds` is zero is below that
resolution: the session reports no `cpu_seconds` ratio and says why, and every
other metric is reported as usual.
`loop_cpu_cycles` is fine-grained CPU time: in the smoke its ratio matched the
wall-time ratio, consistent with QueryProcessCycleTime counting at the fixed
reference (TSC) rate. It moves with clock speed and contention. It is not a
frequency-independent work count like the macOS instruction count, so a small
difference needs more pairs and longer runs on Windows.
`peak_private_commit_bytes` (PeakPagefileUsage) is a process-lifetime peak,
compared as paired B minus A in bytes. `peak_working_set_bytes` is a
diagnostic lifetime peak. The report
states expected commit from copy-on-write views once, as the total page-rounded
size of default SYSTEM_FILE chunks named by the inputs. It is not a measured
allocation counter or subtracted from peak commit. Required unread metrics fail.

Windows refuses profile and neutrality modes; attribution is done on macOS.

A short same-head smoke, under normal host load:

```powershell
just measure compare --base HEAD --head HEAD --cycles 20 --pairs 2 --control-pairs 2 --budget 180
```

This schedules eight fresh processes, including a balanced diagnostic control
of A against A′.
The 180-second budget excludes preparation. Windows build trees, staged DLLs
and driver copies are removed on success or failure; local JSON reports remain.

### Native Linux comparisons

`just measure compare` runs on Linux x86_64 and arm64 with the existing Linux
tooling: the dlopen `bisect --perf` driver, its `PERF_HW` counter, the
Callgrind anchor and the `smaps_rollup` reader. It adds no second measurement
system. tkrzw is the vcpkg overlay install of [Linux setup](setup.md#linux)
for every arm; `tools/tkrzw/runtime.py` certifies each launch against it.
Each distinct SHA builds once through the packaging install procedure: locked
release, the `shipped` feature, Cargo's default backend and no added flags.
This is the plain shipping library, the arm that Linux times. A′ is staged as
on macOS. Each sample is one fresh process, pinned to the first allowed CPU,
with the staged library's directory first on the loader path.

The measured loop is the steady cycles 1 to n−1, the region that both
instruction sources bracket with `bisect_perf_steady_unit`. Cycle 0,
initialization and teardown are outside it; `--cycles` must be at least 2.
The driver does not count operations here. A run is complete when the process
succeeds, reports every cycle and matches the `PERF_WORKLOAD_CHECKSUM`
fingerprint of every other run.

- Instructions come from one source for a whole comparison. One unmeasured
  launch asks for the hardware counter. Where the host grants it, the metric is
  `loop_hw_instructions` (`perf_event_open`, user space). Where the host
  refuses it, as most cloud guests do, a second pass over the same schedule
  collects `loop_callgrind_ir` with Callgrind. The report names the source in
  `instruction_source` and in the metric name. A later refusal fails the
  comparison; the sources are never mixed. A wslc container needs
  `--cap-add PERFMON` for the hardware counter; without it the runner falls
  back to Callgrind.
- `wall_seconds` and `cpu_seconds` sum the per-cycle `CLOCK_MONOTONIC` and
  `CLOCK_PROCESS_CPUTIME_ID` readings over the same cycles.
- `anonymous_kib` and `private_dirty_kib` are the `Anonymous` and
  `Private_Dirty` lines of `/proc/self/smaps_rollup` after the loop, compared
  as paired B minus A in KiB.

The same-bytes control of A against A′ follows the Linux rules, not the dated
rule for native non-Linux comparisons in
[Measurements](../../CONTRIBUTING.md#measurements). Wall time, CPU time and the
hardware count use the 0.95 to 1.05 band. Residency uses ±max(64 KiB, 1% of the
A median). A control outside its limits is `DIAGNOSTIC`: the estimate stays in
the report beside the control limits. Callgrind `Ir` must be equal in every run
of A and A′. A varying count follows the typing-sequence rule above: it is
`DIAGNOSTIC`, with ranges and bounds and no ratio, when every varying function
is in that list. Any other variation is `INVALID`: the `Ir` comparison is
withheld and the command returns nonzero after it writes its report.

One budget covers both passes. Callgrind is far slower than a native run, so a
comparison that uses it needs an explicit `--budget`. State the Valgrind
version beside its figures.

`just measure profile --template Callgrind` and `--template Massif` run one
process of a build that adds `RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"`. Callgrind
uses the campaign's options and annotations for the steady cycles. Massif uses
the heap options of `system-import-profile` for the process lifetime. Linux
refuses the xctrace templates and the neutrality mode;
`just measure-callgrind-control` is the Linux neutrality check. Build trees and
staged copies are removed; local reports and profiles remain.

A short same-head smoke on a host that refuses the hardware counter:

```bash
just measure compare --base HEAD --head HEAD --cycles 8 --pairs 2 --control-pairs 2 --budget 900
```
