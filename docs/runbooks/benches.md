# Benches — Callgrind and Massif recipes

The recipes here are attribution tools, none a per-change gate:
`callgrind-control`, `callgrind-differential` (with its sentence and
typing-sequence workloads) and `system-import-profile`. A plain timing is one
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
Run each recipe as `just measure-RECIPE [ARGS]`. In the measurement image,
which has no `just`, run the corresponding `uv run` command from the
`justfile` with the arguments in place of `"$@"`.
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
the profile/metadata hash. Its `.text` is expected to be byte-identical to plain
shipping; `callgrind-control` checks this. Debuginfo alone is stripped back out.

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
just measure-callgrind-control target/callgrind-tkrzw/profile target/callgrind-tkrzw/control
```

Run from a shell with no Cargo profile or RUSTFLAGS overrides already exported.
For BDB, replace `tkrzw` with `bdb` in the feature and output paths. Both
comparisons must pass; these builds should be case (a), identical `.text`.
No runtime data or oracle build is needed for this check. The verbose build
output records the order of the compiler flags; `readelf --debug-dump=info`
on the profiled library displays its DWARF units.

### Sentence and typing-sequence Callgrind campaign

`just measure-callgrind-differential --workload sentence|typing-sequence`
extends the `bisect` driver. Its no-argument campaign and `bisect --perf`
use the 20-input keystroke-cycle workload. The frozen
`system-import-profile --sentence-fixture` mode still requires its 500-row
fixture; it is not a general prefix-sequence driver.

Both workloads use the pinyin facade, options `0x18a`, an empty temporary
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
just measure-callgrind-differential --workload sentence --cell tkrzw \
  --main "$MAIN_PROFILE_SO" --pr "$PR_PROFILE_SO" --pin "$PIN_SO" \
  --main-shipping "$MAIN_SHIPPING_SO" --pr-shipping "$PR_SHIPPING_SO" \
  --system-dir "$DATA" --out "$WORK/sentence"
just measure-callgrind-differential --workload typing-sequence --cell tkrzw \
  --main "$MAIN_PROFILE_SO" --pr "$PR_PROFILE_SO" --pin "$PIN_SO" \
  --main-shipping "$MAIN_SHIPPING_SO" --pr-shipping "$PR_SHIPPING_SO" \
  --system-dir "$DATA" --out "$WORK/typing-sequence"
```

For BDB use `--cell bdb` and BDB libraries/data. For the tooling smoke check,
set main and PR to the same profiled file and both shipping arguments to
the same plain file. `--input` accepts lowercase ASCII pinyin/apostrophes;
`--iterations` defaults to 1, `--rounds` to 3 fresh processes per arm (minimum
2), and `--cpu` to the first allowed CPU. No warmup is excluded: these are
first-call/sequence scopes, not the `bisect --perf` steady cycle.

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

## BDB guess, training, and import profiles

`tools/bisection/system-import-profile.c` extends the import driver with
three full-process workloads. Build the driver in the measurement
environment with:

```sh
cc -std=gnu11 -Wall -Wextra -Werror -O2 tools/bisection/system-import-profile.c -o "$HARNESS" -ldl
```

Use the same BDB system data, fresh empty user directory, CPU, pin SO, and
subject SO throughout a comparison. Identify the subject build and use
matched pin, main and subject conditions for each comparison.

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

Remove the disposable phrase input after the runs. The expected stdout
SHA-256 values are `50dfc9c87e8eb2ce9121ac4f138584ab226a69308a9becc727cfca1c68c3100d`
for 500 guesses, `e5855249fc5c7cc2c4de81aa1e792c2f41152a84c8a3e10f04c52a824bda698d`
for 200 successful guessed training steps, and
`34e98cd8bf5a191b27e0b8fc50ffdefca9e76a2dab7dd01106cf6b6d3c9eec1d`
for 10,000 imports and 95,698 exported rows.

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
stack walk. Follow two rules:

* **Key on the callee's `(object, file, name)` triple, not the demangled
  name.** `BasicDB::get(char const*, unsigned long, char*, unsigned
  long)` is a substring of that function's own
  `::VisitorImpl::visit_full`, so a name match sums both.
* **An inlined callee has no call edge.** `BasicDB::check` and most
  `BasicDB::get` sites inline into their caller and under-report.
  Where inlining is possible, count something that cannot be inlined — a
  virtually-dispatched visitor, a heap allocation — and corroborate
  against a second such marker.

A profiling build may stand in for the shipped artifact here: block
counts are identical across the shipped recipe, `debug = 2` with LTO
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

### Measure compared arms in one run

Follow the [measurement rules](../../CONTRIBUTING.md#measurements).
Interleave compared arms run by run on the same CPU and data.
Callgrind `Ir` and wall time depend on the CPU, including runtime libc dispatch.

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
installs — and the two do **not** run at the same speed. A record that
does not say which one it built cannot be read as a statement about the
product. The artifact filename does not substitute for the command line.

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
needs no disclaimer or replacement manifest.

If a reproduction command is incomplete or the result contradicts the claim
beyond the method's expected variability, investigate and fix the software,
tooling or documented assumptions and correct or withdraw the claim. State
an actual unresolved limitation beside the affected result; discarded raw
output is not such a limitation. Keep existing measurement controls and
rejection rules, including exact checks where the method requires them.
