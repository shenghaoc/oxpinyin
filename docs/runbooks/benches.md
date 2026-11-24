# Benches — comparison and profiling recipes

This page gives the measurement recipes and how to report their results.
The recipes are attribution tools, and none of them is a per-change gate:

- `callgrind-control`;
- `callgrind-differential`, with its sentence and typing-sequence workloads;
- `system-import-profile`.

A plain timing is one ordinary command around the `bisect` driver. Numbers are comparable only within
one session on one host.
`just measure compare` reports different metrics on macOS, Windows and Linux.
Numbers from different platforms are not comparable: the measured loop, the
counters and the memory figures differ.

The [controlling rule](../source-policy.md#controlling-rule) decides when to measure and which measurement to use.
The recipes below are existing tools, not required counts. A number that a recipe states
for samples, rounds, repetitions, arms or passes is the default of that recipe.
A section that uses Callgrind or Massif is an attribution procedure under that rule.

## Where measurements run

Pin comparisons run locally and natively on a Linux host, with no container.
They cover `callgrind-differential` and `system-import-profile`.
Valgrind and Callgrind exist only on Linux, so these recipes run only on Linux hosts.
macOS and Windows keep the native `just measure` paths.
The oracle is never built or run on GitHub Actions; the recipes and the preparation refuse to run there.

Prepare the host as the [setup runbook](setup.md) states: `just doctor` checks Valgrind and cargo-c,
and `just provision` checks Kyoto Cabinet's development package.
Linux measurements run on the local cell, Kyoto Cabinet from the distribution
([Linux setup](setup.md#linux)): it is reproducible, which the exact-count controls need.
The pin's build needs the host packages that [`tools/oracle/dependencies.json`](../../tools/oracle/dependencies.json)
lists as `common` and `kc`. kc is the one Linux measurement cell. Then prepare the inputs:

```sh
just measure-prepare callgrind
```

A direct `uv run` command needs `OXPINYIN_CACHE` ([cache directory](setup.md#cache-directory)).

These rules apply:

- Builds match the host operating system and architecture.
  The [procedure-specific rules](verification.md#procedure-specific-rules) state the reference platform, the arm64 capture and the emulation ban.
- Every prepared input lives under the `measure` entry of the cache root (`just cache-root`):
  the pin prefixes in `pinyin-oracle/<cell>`, the subject stages in `oxpinyin-kc` and `oxpinyin-nodebug`,
  their Cargo target directories in `target`, and the `bisect` driver in `tmp`.
- Recipe outputs default to `results/<recipe>/` under that entry. Explicit
  output overrides stay supported.
- Each `callgrind-differential` capture and each `system-import-profile` run needs a new
  output directory. Both refuse a directory that exists. A second differential capture
  must set `CG_OUT` to a new path.
- `tools/oracle/build.py` builds the pin natively: `--libs-only`, then `--regen-data` from the pinned model.
  Preparation reuses a prefix only when its `versions.txt` equals this host's inventory, so that the pin
  and the subject share one base, and rebuilds changed or damaged inputs.
  The pin is linked as its recipe links it, with no RPATH or RUNPATH. `build.py` replaces `LD_LIBRARY_PATH`
  with the prefix's own library directories while it generates the data; the distribution's
  libkyotocabinet is on the default loader path, so the prefix holds no library of its own.
- `--cells` is valid only with `oracle` preparation.

Preparation lives in `tools/dev/measure/prepare.py`.
`just measure-prepare callgrind` selects the profiled and shipping Kyoto Cabinet arms, and
`just measure-prepare oracle --cells kc` selects pin data only. Model data stays local.
The pin, the subject and the recipes use the distribution's libkyotocabinet; no arm loads a libtkrzw,
and the [runtime check](verification.md#9-sound-tkrzw-library) still refuses one that does.
Additional base or PR revisions are built on the same host as well,
with separate Cargo target directories. The existing recipe options are set to
the resulting paths.

Pin-versus-subject numbers are comparable only within one session on one host.
The host's compiler, C library and libkyotocabinet build both arms, and the recipes record them in `environment.txt`.

## Shipping measurement inputs

Measurements use ordinary shipping builds. See the [function-layout ruling](../packaging.md#function-layout).
Each recipe keeps the backend, artifact identity, input-digest and runtime checks.
The recipes are `just measure-callgrind-control`, `just measure-callgrind-differential` and
`just measure-system-import-profile`. Their direct `uv run` invocations are in the justfile.
The sampling loops are serial and interleave the arms one process at a time.
A timed or resident-set sample belongs to its arm only if nothing runs
beside it.

The drivers are one nonshipping Rust workspace package.
They load the selected C library with `libloading`. Linux builds require the
Valgrind development headers for `valgrind-requests`; other targets do not
compile that dependency. `perf-event` supplies the Linux hardware counters.
Native memory counters use libc on macOS and windows-sys on Windows.
The macOS driver also links CoreFoundation. The subject reaches it through
GLib's gettext, and dyld loads that closure of about 400 images at launch for a
linked driver. `dlopen` loads it after launch and adds about 4 MB of resident
file-backed pages and 0.3 MB of footprint, which would show in `peak_rss_bytes`
and `lifetime_max_phys_footprint_bytes`.

A plain timing needs no recipe. It builds the `bisect` driver and runs its `--perf`
mode once for each library. Each run prints one JSON line:

```sh
cargo build --locked --release -p oxpinyin-measurement --bin bisect
cp target/release/bisect "$WORK/bisect"
PERF_MODE=speed "$WORK/bisect" --perf "$SO" "$DATA"
```

## Callgrind debug-info control

Both campaign arms use the unchanged shipping Cargo profile. The profiled arm
adds `RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"`. Cargo appends these flags after its
`-C strip=debuginfo`, so the final strip setting of rustc keeps DWARF without a change to
the profile/metadata hash. Debuginfo alone is stripped back out.

`callgrind-control` verifies the DWARF compilation units in the profiled library
and requires its `.text` bytes to match the separately built plain shipping library.
Any `.text` difference stops the measurement. To investigate, run `objdump -d` on each
library and compare the disassemblies by hand.
The campaign checks neutrality again before collection and uses the
plain shipping arm for wall-clock timing.

To check preparation natively, without a pin run or profile collection:

```bash
env RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none" CARGO_TARGET_DIR=target/kc \
  cargo cinstall -v --locked --release -p oxpinyin-capi --no-default-features --features kyotocabinet \
  --destdir=target/callgrind-kc/profile --prefix=/usr
env CARGO_TARGET_DIR=target/kc \
  cargo cinstall -v --locked --release -p oxpinyin-capi --no-default-features --features kyotocabinet \
  --destdir=target/callgrind-kc/control --prefix=/usr
just measure-callgrind-control target/callgrind-kc/profile target/callgrind-kc/control
```

These commands run from a shell that has no Cargo profile or RUSTFLAGS overrides already exported.
For BDB, `bdb` replaces `kyotocabinet` in the feature and `kc` in the output paths. Both
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

The campaign takes these inputs:

- actual library files;
- a complete backend-matched system directory;
- separately built plain shipping files for the two debug-info neutrality
  checks.

The profiled files use the shipping Cargo profile and only
`RUSTFLAGS="-Cdebuginfo=1 -Cstrip=none"`. The shipping artifact is installed through
`just packaging install`. Before collection, the campaign
checks `DT_NEEDED` against `--cell`, DWARF, code neutrality,
and the sound-tkrzw runtime. It runs the harness build and
all collections serially; no other work overlaps them.

```bash
just measure-callgrind-differential --workload sentence --cell kc \
  --main "$MAIN_PROFILE_SO" --pr "$PR_PROFILE_SO" --pin "$PIN_SO" \
  --main-shipping "$MAIN_SHIPPING_SO" --pr-shipping "$PR_SHIPPING_SO" \
  --system-dir "$DATA" --out "$WORK/sentence"
just measure-callgrind-differential --workload typing-sequence --cell kc \
  --main "$MAIN_PROFILE_SO" --pr "$PR_PROFILE_SO" --pin "$PIN_SO" \
  --main-shipping "$MAIN_SHIPPING_SO" --pr-shipping "$PR_SHIPPING_SO" \
  --system-dir "$DATA" --out "$WORK/typing-sequence"
```

For BDB, the campaign takes `--cell bdb` and BDB libraries and data. For the tooling smoke check,
main and PR are the same profiled file, and both shipping arguments are
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

The output directory must be new. `result.json` reports raw counts, control
spread, and Ir per sentence or per complete typing sequence. `environment.json`
records the host, inputs and library identities. Exact commands, worker
output and profiles stay local for attribution with
`callgrind_annotate --inclusive=yes`. They are disposable verification data,
not PR attachments. Harness `elapsed_ns` is collected under Valgrind and is
not a native timing result. These commands make no wall-time or heap claim.

A same-run native time comparison adds `--native-rounds 40
--native-iterations 20`. This optional phase runs before Callgrind. It uses
separate equal-length copies of the supplied **plain shipping** libraries,
the same CPU pin and worker anchors. It rotates base, PR, pin and both
same-bytes controls. It validates every complete observation and reports
native `elapsed_ns` per invocation in `native.json`, independently of the
counters of the profiled arm. Each native process also reports:

- `hardware`: user-space `PERF_COUNT_HW_INSTRUCTIONS` per invocation, counted around
  the same anchor, with the same pairing, PR/base and PR/pin ratios and same-bytes
  controls as the time. A host that refuses the counter gives
  `available: false` with its errno; the Callgrind figure then stands alone.
- `memory`: `/proc/self/smaps_rollup` after the last invocation, in KiB. The headline
  is `anonymous` (Anonymous); `pss` sits beside it. `uss` (Private_Clean plus
  Private_Dirty) and `rss` are secondary: clean file-backed dictionary pages count in
  them and move between sessions. It has PR/pin ratios and the residency control of
  `measure compare`, applied to `anonymous`.

The options are:

- `--native-rounds` requires at least 20. Its default is zero (off).
- `--native-iterations` defaults to 20 and does not change
  the `--iterations` of Callgrind.

No warmup is discarded; all invocations of the
native process are included. The shared paired-bootstrap statistics give
PR/base median ratios and 95% intervals, and the existing same-bytes time
control stability check. Unstable controls are DIAGNOSTIC. Their tolerance
is not permission for a product regression. A report states the interval and the control
limits. An interval that contains one is not proof of exact
time equality. This phase measures residency, not heap usage.

## BDB guess, training, and import profiles

`tools/measurement` provides the `system-import-profile` binary with
three full-process workloads. The driver is built in the measurement
environment:

```sh
cargo build --locked --release -p oxpinyin-measurement --bin system-import-profile
cp target/release/system-import-profile "$HARNESS"
```

The default import workload prints four Callgrind dumps. `open` runs from just before the library
load through `pinyin_init`, so it holds `dlopen` relocation, symbol lookup and context
initialization, and not the process or driver start-up; `import`, `save` and `close` follow.
With `--cold-open` the recipe collects one Callgrind dump from just before the library load
through the first instance and the teardown, the Massif heap and page peaks, and a native
`hardware` line with the user-space instruction count over the same span, teardown included
(the driver's teardown is a collected phase in every mode). A host that refuses the counter prints why instead.

Through a comparison, the BDB system data, a fresh empty user directory, the CPU, pin SO and
subject SO stay the same. Each comparison identifies the subject build and uses
matched pin, main and subject conditions.

The frozen sentence fixture is `fixtures/w4/oracle-sentence-surface.txt`
(SHA-256 `d119f3a7f665333ef5717b1925171799c29e710d4ad1575748e0a9d3ae2c9e21`).

- `--sentence-fixture` replays its 500 rows with a nonempty sentence field.
  It guesses a sentence and candidates for each row.
- `--train-fixture` takes the
  first 200 such rows. For each row, it checks a full parse and a successful guessed train at
  index 0. It then saves once.

Both set options `0x18a`. Candidate
sorting in the guess workload uses `0x1e`.

For `--phrase-file`, the input comes from the same BDB pin SO and data,
through the **unmeasured** `--export-system` of the driver:

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
uv run --managed-python python -X utf8 - "$HARNESS" "$PIN_SO" "$DATA" "$PHRASE_FILE" <<'PY'
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
touched library (library 1: 95,698 rows). **Every** command uses a fresh empty
`USER_DIR`. Each pin/subject command is repeated as often as the decision
requires. For an instruction or heap attribution:

- the figures are the median of
  whole-process Callgrind `Ir` and of the maximum Massif
  `mem_heap_B`;
- the spread is reported as `(max − min) / median`. The Massif value
  excludes stacks, allocator overhead and file mappings;
- output
  equivalence and operation counts are checked before costs are compared;
- profiler
  output stays in streams or memory, not in archives, logs, captures or profile
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
`kcdbget`.** On the oxpinyin cell it produces wrong chains that the symbol table makes look
plausible; on the libpinyin cell it is correct. `--num-callers=24`, resolving at `addr - 1`,
`debug = 2` with `lto = false` and `-C force-frame-pointers=yes` do not change the stacks.
Kyoto Cabinet debug symbols (`libkyotocabinet16v5-dbgsym`) fix only the KC half.

**Callgrind caller→callee edges are used instead.** They give exact call counts and need no
stack walk. Two rules apply:

* **The key is the `(object, file, name)` triple of the callee, not the demangled
  name.** `BasicDB::get(char const*, unsigned long, char*, unsigned
  long)` is a substring of the
  `::VisitorImpl::visit_full` of that function, so a name match sums both.
* **An inlined callee has no call edge.** `BasicDB::check` and most
  `BasicDB::get` sites inline into their caller and under-report.
  Where inlining is possible, the count is of something that cannot be inlined, such as a
  virtually-dispatched visitor or a heap allocation. A second marker of that kind
  confirms the count.

A profiling build may stand in for the shipped artifact here: block
counts are identical across the shipped recipe, `debug = 2` with LTO
off, and forced frame pointers.

## Writing it up

A write-up follows the [verification reporting rules](verification.md#7-proportionate-verification) and the
[controlling rule](../source-policy.md#controlling-rule).
For a measurement, the details needed to interpret the comparison are
the revisions, workload, backend, build recipe, host/container and controls.
The [complexity argument](../source-policy.md#complexity-evidence) applies to changes that alter
an algorithm, a data structure, allocation behaviour or an input bound.
The [time/space trade-off rule](../source-policy.md#performance-standard) applies to every change.

### Measure compared arms in one run

When a measurement compares arms, they run in one job on one machine, and the report gives only that run's numbers and ratios.
An absolute figure from another run, host or CPU is not used as a baseline.
Compared arms are interleaved run by run on the same CPU and data.
Callgrind `Ir` and wall time depend on the CPU, including runtime libc dispatch.

### State the Valgrind version when used

A report that uses Callgrind or Massif states the Valgrind version that it used, as
`valgrind --version` prints it, next to the figures. Callgrind `Ir` and Massif heap
peaks change with the Valgrind release as well as with the CPU. A report
without the version therefore cannot be compared with another run. Normal cloud
provisioning (`tools.dev.provision`) installs Valgrind idempotently and prints the
version. Another measurement host uses the distribution package (`apt-get install valgrind`
on Debian/Ubuntu, `dnf install valgrind` on Fedora/RHEL). Ordinary contributions install no
measurement tools.

### State the build recipe, per artifact

A report identifies each timed subject by revision, backend and build recipe, including
the profile and relevant flags. A maintained recipe and its arguments can
supply the build command; it is not repeated for every figure.

The hashes and loaded-library checks that the tools need stay tool-internal
checks. A report needs the results and the reproduction recipe. It does not need a checklist
of their verdicts, a checksum manifest or a full `NEEDED` inventory. It includes a
specific hash or dependency only when that explains a concrete build-identity
or loader discrepancy.

`cargo cinstall` produces the shipped library; `cargo build` produces a bisection fixture.
Their performance differs, so a report identifies the build command, not just the artifact filename.

## Native non-Linux revision comparisons

The measured arms of a native non-Linux revision comparison are serialised; normal
background activity on the host is permitted. A same-binary control runs in the
same session, and the report gives its variability beside the paired estimates. The
control is diagnostic: it invalidates no runs and sets no automatic verdict or
acceptance threshold. Every completed valid run, including outliers, is kept
within the predefined work count, schedule and budget. Only incomplete work, an
operation fingerprint mismatch, a wrong loaded image or a broken required
capture invalidates a run. Runs are not repeated for significance. Linux and
pin-relative work keep the same-bytes control rules of
[Native Linux comparisons](#native-linux-comparisons).

## Native macOS source revisions

These comparisons use full Xcode on an Apple silicon Mac. The runner probes the installed tools;
Command Line Tools alone do not provide xctrace. Native revision comparisons
follow [Native non-Linux revision comparisons](#native-non-linux-revision-comparisons).
They do not compare against the pin.

```sh
just measure compare --base origin/main --head origin/main
just measure profile --revision origin/main --template 'CPU Profiler'
just measure profile --revision origin/main --template Allocations
```

`just measure profile --template 'CPU Counters'` records the default CPU Bottlenecks mode and prints the Useful, Instruction Delivery, Instruction Processing and Discarded shares of CPU bandwidth over the process lifetime, with the measured loop's share of that span. Pass enough `--cycles` for the printed loop share to be at least 0.9; 200 cycles gave 0.88 on a quiet Apple M5.

`just measure` builds the source of each revision with the tooling of the
checkout, so a comparison needs only the interpreter of the checkout.
`just packaging sizes` is different: it runs each revision's own `packaging`
recipe. Each revision’s `.python-version` and `uv.toml` determine its interpreter and uv requirement. See
[comparing against another revision](setup.md#comparing-against-another-revision).

These commands use the same typing corpus in `tools/measurement/src/workload.rs`. Schedule defaults
live in `tools/dev/measure/recipes.py`; `just measure --help` lists overrides.
The budget bounds the measured schedule, excluding preparation. An exhausted
budget fails the campaign and preserves completed runs. Keystrokes are work
items, not independent statistical observations. Estimates use process pairs.

Each distinct source SHA builds once, into a fresh directory for each invocation, through the
packaging install procedure: locked release and Cargo's default backend,
with `RUSTFLAGS='-Cdebuginfo=1 -Cstrip=none'` and `OXPINYIN_DSYM=1` for symbols. The dylib and
dSYM must have matching arm64 UUIDs. Product code uses no profiling profile or instrumentation.
macOS measurement builds use the `tkrzw.pc` selected by
[macOS setup](setup.md#macos). They do not require `VCPKG_ROOT`.

Before this build recipe is accepted, its one-time neutrality validation runs:

```sh
just measure neutrality --revision origin/main
```

This compares the symbol-bearing packaged build with a plain release build.
Executable Mach-O section bytes, addresses and arm64 disassembly must be
identical. A difference fails, and nothing is measured. Routine comparisons
reuse the symbol-bearing build and do not build a plain artifact.

Builds and disposable sessions live under `just cache-root`. The same-binary control runs arm A
against A′, a byte-identical copy of A's product and driver staged in its own prefix exactly as
B is; the report labels A′ as `B`. A′ passes the same staging, loaded-image and UUID checks as
any arm. Before measuring, the runner verifies that the resolved C function belongs to the staged
dylib.

The driver brackets all typing cycles, excluding initialization and teardown:

- `wall_seconds` and `cpu_seconds` cover that loop;
- `operations` counts completed prefix steps; the return/count fingerprint must agree across runs;
- `loop_instructions` and `loop_cpu_cycles` are the `proc_pid_rusage` instruction and cycle deltas;
- `loop_runnable_seconds` is time on a core or waiting for one. The kernel advances it only at
  scheduling events, so a loop shorter than a scheduling quantum can read 0.

A macOS report leads with the `loop_instructions` B/A ratio and wall time.
Runnable time explains host contention: each arm reports it over its CPU time,
and an excess is time spent waiting for a core. It has no B/A ratio.
Instruments remains the tool for attribution.
A Windows report leads with the `loop_cpu_cycles` B/A ratio and wall time; its
memory figure is the `peak_private_commit_bytes` B minus A difference.

`lifetime_max_phys_footprint_bytes`, read after teardown, is the memory headline. It is a
process-lifetime peak, not live memory at a checkpoint, and teardown can set it.
`loop_end_phys_footprint_bytes` is a single `ri_phys_footprint` reading taken at loop exit,
before teardown; it is not a peak over the loop. `peak_rss_bytes` is the direct child's own
`wait4` diagnostic; a profile launch reports none. Neither field is a Linux residency category.
No loop-scoped peak footprint is reported: the macOS SDK declares neither
`proc_reset_footprint_interval` nor `proc_rlimit_control`, the calls that restart the interval peak.

Profiles cover the launched process lifecycle. They are not scoped-loop timing estimates.
Allocations rows are allocations still outstanding when the process exited, not live memory at a
lifecycle checkpoint or allocation traffic. Its `Statistics` detail gives total, transient and
persistent allocation counts and bytes. Missing data, wrong-process captures and unresolved product
frames fail visibly.

Installed xctrace exposes neither CPU Profiler deferred recording nor an
Allocations backtrace setting. Their defaults apply; there is no options file or
workaround. Allocation backtraces must still exist in the exported capture.
`xctrace export` exposes no time-range switch. For an explicitly selected
range, `xctrace toc` inspects the trace, then `xctrace show` takes the
reported track/detail and `--time-start` plus `--duration`. A
whole-trace export is not described as a scoped-loop export.

Other installed instruments use the same profile mode:

```sh
just measure profile --template 'System Trace'
just measure profile --template 'Processor Trace'
just measure profile --template Allocations --instrument 'VM Tracker'
```

VM Tracker is an instrument added to a named template, not a standalone
template. Availability of a template does not prove host recording permission;
a failed required capture fails the command. Traces, exports and JSON reports
are disposable local output. They are not attached or committed. The PR publishes only
reproduction commands and a concise result table. The initial
main-against-main validation is not a performance claim.

### Native Windows comparisons

`just measure compare` also runs on Windows x64 with MSVC and the manifest
dependencies installed as in setup.md. The recipe needs no `VCPKG_ROOT`.
Both arms use the pkg-config metadata and GLib DLL in the repository's
`vcpkg_installed/x64-windows`, and the repository's static tkrzw
(`just tkrzw`) from the cache root, first on `PKG_CONFIG_PATH`.
Each distinct SHA builds once with
`cargo build --locked --release -p oxpinyin-capi`, default features and no
added linker or debug flags. The artifact is `oxpinyin_capi.dll`, not a packaged
libpinyin DLL; tkrzw is inside it. A′ is staged as on macOS. A run fails unless the imported ABI function's owning
module is the arm's DLL and both arms load the same dependency files: every module outside the
Windows system directory, which is GLib, plus the C runtime.

QPC elapsed seconds, the QueryProcessCycleTime delta and GetProcessTimes
user+kernel seconds bracket the same typing loop as macOS, excluding
initialization and teardown. Each CPU figure covers all threads of the process.
`loop_cpu_cycles` is a count of CPU cycles. It is never converted to seconds:
Microsoft documents against that conversion. A zero cycle delta is a broken
capture and fails. `cpu_seconds` is secondary; GetProcessTimes advances in
1/64-second ticks. A sample whose `cpu_seconds` is zero is below that
resolution: the session reports no `cpu_seconds` ratio and says why, and every
other metric is reported as usual.
`loop_cpu_cycles` is fine-grained CPU time counted at the fixed reference (TSC) rate.
It moves with clock speed and contention. It is not a
frequency-independent work count like the macOS instruction count, so a small
difference needs more pairs and longer runs on Windows.
`peak_private_commit_bytes` (PeakPagefileUsage) is a process-lifetime peak,
compared as paired B minus A in bytes. `peak_working_set_bytes` is a
diagnostic lifetime peak. The report
states expected commit from copy-on-write views once, as the total page-rounded
size of default SYSTEM_FILE chunks named by the inputs. It is not a measured
allocation counter or subtracted from peak commit. Required unread metrics fail.

Windows refuses profile and neutrality modes; attribution is done on macOS.
The Windows `bisect` driver runs `--measure`, `--perf-sentence` and `--perf-typing-sequence`.
It exits with status 2 on `--perf`, which has no per-cycle CPU time finer than the
1/64-second process tick and no memory readings there, and on any unknown mode.

A short same-head smoke, under normal host load:

```powershell
just measure compare --base HEAD --head HEAD --cycles 20 --pairs 2 --control-pairs 2 --budget 180
```

This schedules eight fresh processes, including a balanced diagnostic control
of A against A′.
The 180-second budget excludes preparation. Local JSON reports remain after the run.

### Native Linux comparisons

`just measure compare` runs on Linux x86_64 and arm64 with the existing Linux
tooling: the dlopen `bisect --perf` driver, its `PERF_HW` counter, the
Callgrind anchor and the `smaps_rollup` reader. It adds no second measurement
system. Every arm is the Linux local cell, Kyoto Cabinet from the distribution
([Linux setup](setup.md#linux)); `tools/tkrzw/runtime.py` still checks each launch.
Each distinct SHA builds once through the packaging install procedure: locked
release, `--no-default-features --features kyotocabinet` and no other flags.
Without `--data`, the arms read the Kyoto Cabinet pin data that `just measure-prepare oracle` wrote
under the cache root (`host_cell` in `tools/dev/cell.py`); Debian's `libpinyin-data`
package, the macOS and Windows default, is tkrzw data.
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

The same-bytes control of A against A′ follows the Linux rules, not the rule of
[Native non-Linux revision comparisons](#native-non-linux-revision-comparisons). Wall time, CPU time and the
hardware count use the 0.95 to 1.05 band. Residency uses ±max(64 KiB, 1% of the
A median). A control outside its limits is `DIAGNOSTIC`: the estimate stays in
the report beside the control limits. Callgrind `Ir` must be equal in every run
of A and A′. If any count differs, the command records `INVALID`, withholds
the `Ir` comparison and returns nonzero after it writes its report.

One budget covers both passes. Callgrind is far slower than a native run, so a
comparison that uses it needs an explicit `--budget`. The Valgrind
version is stated beside its figures.

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
