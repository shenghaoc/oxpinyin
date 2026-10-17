# Benches — criterion, the perf baseline, the RSS diagnosis, and profiles

Four tiers, none run by CI (`verify-nightly` has no perf lane): the
criterion benches for a component, the C-ABI perf baseline against the
oracle, the RSS diagnosis that decomposes a resident set rather than
sizing it, and a sampled profile of the keystroke cycle. Numbers are only
comparable within one container on one host; every published snapshot
(`docs/perf/`, `docs/findings/perf-*.md`) names both.

## Criterion benches

| crate | bench | needs |
| --- | --- | --- |
| oxpinyin-capi | `stage2` | the export (`PINYIN_EXPORT_DIR`), the C-ABI surface |
| oxpinyin-store | `backend_matrix_{tkrzw,kyotocabinet,bdb}` | `--features <backend>` (each is `required-features`-gated) |
| oxpinyin-store | `kyotocabinet_bnum` | `--features kyotocabinet,bench-internal`; **an installed libpinyin `data/` via `OXPINYIN_BNUM_DATA_DIR`** for meaningful numbers (see below) |
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

**No capture committed.** The recipe above is what regenerates every
figure; run output belongs on the issue or in the upstream-report
draft, not in-tree.

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
`tools/packaging/install.sh libpinyin --prefix=/lane/stage-main -- --locked --release --jobs 1 --no-default-features --features bdb,shipped`
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
python3 - "$HARNESS" "$PIN_SO" "$DATA" "$PHRASE_FILE" <<'PY'
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
  tools/packaging/install.sh libpinyin \
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

## RSS diagnosis (where the resident set actually sits)

`bisect --perf` in `PERF_MODE=rss-diag` answers a different question from
the RAM modes: not how large the resident set is, but what it is made of.
At init and again after the keystroke cycles it reads
`/proc/self/smaps_rollup` (anonymous vs file-backed, the clean/dirty
split) and `mallinfo2()` (heap high-water, retained free chunks, arena
count), then calls `malloc_trim(0)` and reads both again — the one call
that separates allocator retention from memory genuinely held.

`tools/bisection/run-rss-diag.sh` drives the two cells round-robin over
**one** data directory (the drop-in configuration: both engines open
libpinyin's own installed `data/`) and writes `rounds.jsonl` plus an
`identity.txt` naming every artifact by SHA-256:

```sh
tools/bisection/run-rss-diag.sh \
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

### Measure base, PR and pin in one run

**A measurement is the ratio of artifacts timed in the same job on the same
machine, never a number carried over from another run.** A measuring PR
builds the base tree, the PR tree and the pin-built oracle, runs all three
in one job on one machine (same CPU, same data), **interleaving** the base,
PR and pin artifacts run by run, and reports only those figures and the
ratios between them. Interleaving all three is a requirement of the
measurement, not a property of `tools/bisection/run-perf-same-data.sh`,
which interleaves two pin-built backends and two artifacts of the current
tree and does not run a base artifact; a measuring PR that needs the
base/PR/pin triple arranges that itself.
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
without the version cannot be compared with another run. In a cloud session
`tools/cloud/setup.sh` installs Valgrind from the distribution archive when
it is missing and prints the version in its summary
(`docs/runbooks/cloud.md`).

### State the build recipe, per artifact

**Which artifact was timed is part of the measurement, not a detail of it.**
Name the exact cargo command line for every timed oxpinyin artifact, and give
its `sha256`, `NEEDED` list and byte size.
`docs/findings/perf-steady-cycle-cross-host-2026-09-07.md:155` is the worked
example.

This is not bookkeeping. `cargo cinstall` is the shipping path
(`.github/workflows/release-packages.yml` → `tools/packaging/install.sh`;
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
- **Naming `run-perf-same-data.sh` is not naming a recipe.** Its
  `OXPINYIN_KC_SO` / `OXPINYIN_TKRZW_SO` variables take a prebuilt `.so` and
  skip `build_capi` entirely.

`tools/profile/run-w8-cycle.sh` is a third case worth naming explicitly: it
stages through `cargo cinstall` but with `--profile profiling` (thin LTO, line
tables, `panic = "unwind"`), so it is neither the fixture nor the product.

The audit of which existing records sit where is
`docs/findings/perf-build-recipe-audit-2026-09-10.md`.

### Evidence: what is committed and what is not

**A findings document commits no captures.** Not DHAT JSON, not `/proc`
`smaps` dumps, not callgrind out-files, not `perf` data. They are large,
machine-generated, and unreviewable in a diff — and in at least one case
the bulk of the file is a part the document itself records as unusable.

What the document carries instead:

1. **The command that produced each figure.** Not each class of figure —
   each figure. Where two numbers come from different commands, different
   inputs, or different runs, they get separate entries, because a shared
   heading is exactly how one of them ends up with no provenance at all.
   That is what makes a number regenerable once the capture is gone, and
   it is the substitute for a path on a machine.
2. **The capture bundle, attached to the pull request**, with its URL *in
   the document* and not only in the PR description — a document that says
   "evidence is attached to the PR" without a link has the same problem as
   one pointing at `/tmp`. Record the bundle's SHA-256 alongside the link.
   A release asset is an **additional** mirror if you want one, never a
   substitute for the PR attachment.
3. **An explicit note where a figure cannot be regenerated**, *at the
   point of use*, so a weaker claim reads as one. Do not point at a path
   that no longer exists.

**The one exception to (2), and it is narrow.** If the capture environment
was ephemeral and the bundle could not be retained anywhere — no
attachment, no mirror, nothing to link — the document says so plainly, in
place of the link, and (1) carries the whole reproduction burden. Keep the
SHA-256 even then, so anyone still holding a copy can verify it. This is
not a licence to skip the attachment when attaching is possible: it exists
because it happened, in the record that prompted this rule, and a rule its
own founding case violates is worse than no rule. When it applies, (1) is
not "a Provenance section" but a complete recipe — toolchain, packages,
builds, measurement commands, and the reader invocations that turn the
captures into the document's tables.

Small, human-readable, directly-cited artifacts are the exception that
proves the rule — a SHA manifest, a pin file — and
`docs/findings/oracle-pin-074a221-evidence/` is what that looks like.
The line is bulk machine output, not evidence as such.

Derived readers stay in `tools/bisection/` (`rss-smaps.py`,
`cg-calls.py`): they are tools, not evidence, and they are what makes the
pipeline reproducible for anyone holding the bundle.

Initialization and startup are measured by `initbench`, not by a criterion
bench: see [`initbench.md`](initbench.md).
