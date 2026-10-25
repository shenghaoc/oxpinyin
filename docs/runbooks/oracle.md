# Oracle — building the pin and running the differentials

For contributor compatibility verification, use a supported
[cloud session](cloud.md) provisioned for every gate, or the second supported
[canonical Debian compatibility container](compatibility-container.md).
The latter executes subject, reference and harness together on Linux storage.
Ordinary work uses [native setup](setup.md), without an oracle or container.
The direct build/runner operations below expose the existing machinery used
by both routes; extracting libraries alone is not passing parity evidence.

The oracle is libpinyin built from the pinned commit, with the pinned
model20 data, on the Berkeley DB backend by default (the reference
build is a bare `./configure`); Kyoto Cabinet and tkrzw prefixes are
built with `--dbm kc` / `--dbm tkrzw`, and a differential picks its cell
with `PINYIN_ORACLE_DBM`. Every parity claim in this repository is a
differential against it. Its identity is `tools/oracle/oracle-pin.txt`
(mirrored by constants in `tools/oracle/build-oracle.sh`); the history of
the pin is `docs/testing/oracle-environment.md`.

Linux only. The model20 archive is not redistributable
(`docs/findings/model-provenance.md`): no CI job fetches it, and nothing
below that needs it runs on GitHub-hosted runners. The one hosted job
that touches the oracle, `.github/workflows/oracle-image.yml`, builds the
libraries only and ships no model-derived data (section 2a).
On a pull request, a push whose image tag and verification files already
passed on that pull request and architecture skips the rebuild and says so in
the job log; any change to an input rebuilds and verifies.

## 1. Fetch the model (once per machine)

```sh
tools/model/fetch-model.sh
export PINYIN_MODEL_DIR="$PWD/target/model20/extracted"
```

SHA-256-verified into `target/model20/`; the last stdout line is the
extracted directory.

## 2. Build the oracle

Build dependencies: autotools, a C/C++ toolchain, pkg-config, gettext,
gnome-common, curl, python3, and the dev headers for GLib 2.0, IBus 1.0,
SQLite 3 and the DBM (`libdb-dev` for the default Berkeley DB cell;
`--dbm kc` needs `libkyotocabinet-dev`, `--dbm tkrzw` needs
`libtkrzw-dev`). The parity oracle defaults to Berkeley DB (ruling
2026-09-27 UTC, following Q1); differentials pick the cell with
`PINYIN_ORACLE_DBM`. The capture goldens and the `pinyin-oracle`
crate's tests still need the tkrzw cell: build that prefix with
`--dbm tkrzw`.
Add `--enable-libzhuyin` to also build libzhuyin, which the zhuyin, candidate-assembly, two-context and open-counter differentials need.

```sh
tools/oracle/build-oracle.sh --prefix ~/.local/opt/pinyin-oracle   # fetches libpinyin by commit SHA, verifies, builds
```

The `--prefix` above is the default the differential and perf runners
read from `PINYIN_ORACLE_PREFIX`, so the build and the runners meet
without further settings. Without `--prefix` the script installs to
`${TMPDIR:-/tmp}/oxpinyin-oracle/prefix` (`/tmp/oxpinyin-oracle/prefix`
when `TMPDIR` is unset), and the runners find it only if
`PINYIN_ORACLE_PREFIX` points there.
The script writes a manifest the differential runners compare against
`oracle-pin.txt` before trusting the prefix. The container recipes in
`tools/bisection/Dockerfile.perf-matrix` carry a prebuilt oracle at
`/opt/libpinyin-tkrzw` for the perf work.

## 2a. Or pull the prebuilt libraries (any machine)

The oracle libraries build reproducibly: inside the pin-reference image
(`tools/oracle/Dockerfile.oracle`: `debian:trixie`, the current Debian stable
codename, from the default mirrors with normal security updates)
`build-oracle.sh --libs-only` produces byte-identical
`libpinyin.so` and `libzhuyin.so` wherever it runs on the same platform, so
their hashes are recorded once per platform in
`tools/oracle/oracle-pin.txt` (`libpinyin_sha256_<dbm>`,
`libzhuyin_sha256_<dbm>`). The historical unqualified fields are
**linux/amd64**; linux/arm64 adds the same fields with a `_linux_arm64`
suffix. `tools/oracle/pin.sh` selects them; `tools/cloud/setup.sh` derives
the platform from `uname -m` and refuses an architecture with no frozen
hashes. The Dockerfile and the checks take `--platform`. CI
(`oracle-image.yml`) builds the bdb, kc and tkrzw cells on native amd64 and
arm64 runners. The amd64 tags are the historical

    ghcr.io/shenghaoc/oxpinyin-oracle:<tag>

and arm64 publishes the siblings `<tag>-arm64` and `<tag>-deps-arm64`.
`<tag>` is printed by `tools/oracle/image-tag.sh`: the libpinyin
commit and the base image codename (both from `oracle-pin.txt`) and a hash
of `Dockerfile.oracle`, `build-oracle.sh`, the version-check inputs and
`oracle-pin.txt` itself, so it changes whenever the library hashes or their
record can. The arm64 contract gate
excludes the 13 cases diagnosed in #1137 and reports 0 DIFFER; amd64 still
runs them. See [the arm64 finding](../findings/oracle-arm64-1132.md). A published tag is
immutable (the workflow never overwrites one), and `tools/cloud/setup.sh`
and the commands below derive it the same way. The hashes are also in the
image labels. The
image holds `/opt/pinyin-oracle/{bdb,kc,tkrzw}` with `lib/`, `include/`,
`lib/pkgconfig/` and `oracle-pin.txt`. It holds **no model-derived data**
(the model is not redistributable), so a pulled prefix needs the data
step below before any differential can use it.

```sh
tag=$(tools/oracle/image-tag.sh)
platform=linux/amd64                       # or linux/arm64
suffix=
[[ $platform == linux/amd64 ]] || suffix=-arm64
img=ghcr.io/shenghaoc/oxpinyin-oracle:$tag$suffix
docker pull --platform "$platform" "$img"   # podman, or your runtime's equivalent
```

Copy the cells out of the image (a `FROM scratch` image has no command, so
`create` needs a dummy one) and point every cell's `.pc` files at the new
location. The destination is created first and the *contents* of
`/opt/pinyin-oracle` are copied into it (`/.` suffix), so a first run and a
rerun give the same tree:

```sh
dest=$HOME/.local/opt/pinyin-oracle        # any directory
mkdir -p "$dest"
cid=$(docker create --platform "$platform" "$img" /none)
docker cp "$cid:/opt/pinyin-oracle/." "$dest"
docker rm "$cid"
for cell in bdb kc tkrzw; do
    sed -i "s|^prefix=.*|prefix=$dest/$cell|" "$dest/$cell"/lib/pkgconfig/*.pc
done
```

Check the libraries against the recorded recipe hashes (a mismatch means
the image is not this repository's recipe — stop and report it):

```sh
source tools/oracle/pin.sh
for lib in libpinyin libzhuyin; do
    want=$(pin_platform_field tools/oracle/oracle-pin.txt "$platform" "${lib}_sha256_bdb")
    echo "$want  $dest/bdb/lib/$lib.so" | sha256sum --check
done
```

### Regenerate the data (after every pull)

The data directory is built from the model on your own machine, with
this pin's data build, and then checked against the recorded manifest:

```sh
tools/model/fetch-model.sh
export PINYIN_MODEL_DIR="$PWD/target/model20/extracted"
tools/oracle/build-oracle.sh --regen-data --dbm bdb --prefix "$dest/bdb" \
    --model-dir "$PINYIN_MODEL_DIR" --jobs "$(nproc)"
```

`--regen-data` fetches the pinned libpinyin commit, builds it in a work
directory with the same configure arguments, runs the pin's data build
(`data/Makefile.am`: `gen_binary_files`, `import_interpolation`,
`gen_unigram`) over the model into a staging area beside the prefix, and
computes `oracle-data.sha256`, `oracle-data-unstable.sha256` and the data
fields of `oracle-pin.txt` there. **Only if the stable manifest's digest
equals `data_manifest_sha256_<dbm>` in `tools/oracle/oracle-pin.txt`** does
it swap `lib/libpinyin/data`, the data-build utilities and those files into
the prefix — the pulled libraries are left as they are. On a mismatch it
exits non-zero and the prefix is byte-for-byte what it was.
It needs the build dependencies of section 2 (the pinned image's `deps`
stage has them: `docker build --target deps -f tools/oracle/Dockerfile.oracle
.`). Use `--dbm kc` or `--dbm tkrzw` with the matching
subdirectory for the other cells.

Then use the prefix as if it had been built locally:

```sh
export PINYIN_ORACLE_PREFIX="$dest/bdb"
export PINYIN_ORACLE_DBM=bdb
```

The six files in `oracle-data-unstable.sha256` are not reproducible
upstream (`docs/findings/oracle-data-reproducibility.md`) and are not part
of the cross-build digest comparison. Verify them against the prefix's
local manifest before the pre-release real-installation checks below.

### Reproducing the libraries

`tools/oracle/check-reproducible.sh --platform linux/arm64` builds each
cell twice in the trixie pin-reference image, from different work
directories and prefixes, and fails unless the two builds agree with each
other and with the platform's `oracle-pin.txt` fields. The default platform
is `linux/amd64`; run it when the recipe or the base image changes, and
update the recorded hashes in the same commit. The recipe is
`SOURCE_DATE_EPOCH` = the pin's commit time, `-ffile-prefix-map` and
`-fdebug-prefix-map` on the source path, and `-Wl,--build-id=none`; the
`.la` and `.pc` files still record the install prefix and are not part
of the contract.

## 3. Produce the export

The Rust side reads a system data directory it compiled itself:

```sh
cargo run -p oxpinyin-datagen -- compile \
    --model-dir "$PINYIN_MODEL_DIR" --out-dir /tmp/oxpinyin-export
export PINYIN_EXPORT_DIR=/tmp/oxpinyin-export
```

`/tmp/oxpinyin-export` is the default every harness reads
(`oxpinyin_testsupport::model_cache::DEFAULT_EXPORT_DIR`).

## 4. Run the differentials

```sh
just heavy -- tools/oracle/run-differentials.sh --libpinyin <built libpinyin tree> \
    --data <built system data dir> --export "$PINYIN_EXPORT_DIR" \
    --model "$PINYIN_MODEL_DIR"
```

The script takes explicit paths (`--libpinyin`, `--data`, `--export`
and `--model` are required) and exits 2 without them. The shell
expands the `PINYIN_EXPORT_DIR` and `PINYIN_MODEL_DIR` exports from steps
1 and 3 into those arguments before the script starts; the script itself
discovers nothing, and with `PINYIN_EXPORT_DIR` unset `--export` is empty
and the run fails.

Wires the `PINYIN_*` variables and runs the `#[ignore]`d differential
tests with `--include-ignored` (datagen `cross_backend` with model20,
`libpinyin_data_dir` on tkrzw, KMM, segment, lambda, counter, and eval
when its inputs exist). `--data` is also exported as
`OXPINYIN_LIBPINYIN_DATA_DIR`; the runtime test opens that directory
unchanged. `--model` supplies `PINYIN_MODEL_DIR` for the datagen test.
Each suite's status is accumulated; the script exits
non-zero if any suite failed. A test that panics with `missing input:
…` names the input it lacked — that is a provisioning gap, not a
divergence.

The C-ABI drop-in gate, both libraries opened on one unchanged data
directory:

```sh
cargo build --locked -p oxpinyin-capi
tools/bisection/run-same-data-dir-diff.sh \
    "$PINYIN_ORACLE_PREFIX/lib/libpinyin.so" target/debug/libpinyin_capi.so \
    "$PINYIN_ORACLE_PREFIX/lib/libpinyin/data"
```

Exit 0 is identical on every driver; 2 prints each divergence. The
per-surface drivers (`tools/bisection/run-*-diff.sh`) run one surface
each; their headers say what they prove.

### Pre-release real-installation checks

Before each release, run the three-cell sweep required by `AGENTS.md`,
verification rule 3. The tkrzw suites above own `cross_backend` with
model20 and `libpinyin_data_dir` on tkrzw. This pre-release procedure owns
`cross_backend` with model20 on BDB and KC, `bdb_libpinyin_files` (BDB),
`reads_a_real_libpinyin_installation` (the BDB reference installation),
and `libpinyin_data_dir` on KC. They need model20 or real installation
files; the committed synthetic fixtures do not replace these inputs. Ordinary `cargo test` leaves them ignored. Explicitly selecting
one fails when a required input is missing (`docs/testing/README.md`).
Report each result or outstanding input limitation before release.

Use prefixes verified by `tools/cloud/setup.sh` ([cloud setup](cloud.md)),
and set `TMPDIR` on disk with room for scratch data. On Ubuntu, tkrzw
must use setup's sound libtkrzw, pass `tools/tkrzw/distro-probe.sh`, and
resolve both the oracle and subject to that library with `ldd`
(verification rule 9). Keep one target directory per cell. When setup is
needed in a concurrent lane, set `OXPINYIN_ORACLE_ENV` to a lane-private
file; do not overwrite another lane's environment registration.

The input identity authority is `tools/oracle/oracle-pin.txt`:

| Input | Identity |
| --- | --- |
| Pin source | `libpinyin_commit` (`074a2219c90feaf962d0d24f034514033ece5f99`) |
| Pin build, each cell | `libpinyin_sha256_<cell>` and `libzhuyin_sha256_<cell>` for the host platform (`_linux_arm64` for arm64; unqualified for amd64); the matching prefix's `oracle-pin.txt` records `shared_object_sha256` |
| Real data, each cell | `data_manifest_sha256_<cell>` for the host platform, plus the prefix's own `data_unstable_manifest_sha256`; use the shared [data verification](cloud.md#data-verification) |
| Model20 archive | `model_archive_sha256` (`59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`); use its verified extraction for `PINYIN_MODEL_DIR` |

For each selected cell, run the read-only command in cloud setup's
[data verification](cloud.md#data-verification) with its matching
environment loaded. It uses the same `data_ok` function as setup to check
both manifest digests and every listed file in that exact prefix, so the
pre-release identity check follows setup's contract.

Record the actual build and data-manifest digests, the model20 digest,
host and cell with the results. The BDB real-file tests read `bigram.db`
and `pinyin_index.bin`. The real-installation phrase-library test reads
`gb_char.bin`, `gbk_char.bin`, `opengram.bin`, `merged.bin` and the
installation's optional `art.bin`; these chunk files are backend-neutral,
so the BDB reference installation carries this check. The runtime test
opens the KC or tkrzw installation's complete data directory. The datagen
read-back test consumes model20 and produces its own backend files; it
does not read the oracle's data directory or link libpinyin.

From the repository root, choose the setup-verified prefixes below and
run the exact commands. `target/real-installation/<cell>` is private to
this checkout; reuse it when resuming. For the default setup cache:

```sh
export TMPDIR="$PWD/target/real-installation/tmp"
mkdir -p "$TMPDIR"
prefix_bdb="$HOME/.cache/oxpinyin/oracle/bdb"
prefix_kc="$HOME/.cache/oxpinyin/oracle/kc"
export PINYIN_MODEL_DIR="$HOME/.cache/oxpinyin/model20/extracted"

# BDB: compile the verified model20 extraction and read back BDB files.
CARGO_TARGET_DIR="$PWD/target/real-installation/bdb" \
just heavy -- cargo test -p oxpinyin-datagen --no-default-features --features bdb \
    --test cross_backend -- --include-ignored --nocapture

# KC: the same model20 input, compiled and read back as Kyoto Cabinet.
CARGO_TARGET_DIR="$PWD/target/real-installation/kc" \
just heavy -- cargo test -p oxpinyin-datagen --no-default-features --features kyotocabinet \
    --test cross_backend -- --include-ignored --nocapture

# BDB: four real-file storage tests.
OXPINYIN_LIBPINYIN_DATA_DIR="$prefix_bdb/lib/libpinyin/data" \
CARGO_TARGET_DIR="$PWD/target/real-installation/bdb" \
just heavy -- cargo test -p oxpinyin-store --no-default-features --features bdb \
    --test bdb_libpinyin_files -- --include-ignored --nocapture

# BDB reference installation: real per-library chunks.
OXPINYIN_LIBPINYIN_DATA="$prefix_bdb/lib/libpinyin/data" \
CARGO_TARGET_DIR="$PWD/target/real-installation/bdb" \
just heavy -- cargo test -p oxpinyin-data --no-default-features --features bdb \
    --test phrase_library -- reads_a_real_libpinyin_installation --exact --include-ignored --nocapture

# KC: an unchanged pin-built data directory.
OXPINYIN_LIBPINYIN_DATA_DIR="$prefix_kc/lib/libpinyin/data" \
CARGO_TARGET_DIR="$PWD/target/real-installation/kc" \
just heavy -- cargo test -p oxpinyin-runtime --no-default-features --features kyotocabinet \
    --test libpinyin_data_dir -- --include-ignored --nocapture
```

The two tkrzw suite commands in `run-differentials.sh` can also be run
individually with the same inputs, after loading setup's tkrzw environment:

```sh
export PINYIN_MODEL_DIR="$HOME/.cache/oxpinyin/model20/extracted"
export OXPINYIN_LIBPINYIN_DATA_DIR="$PINYIN_ORACLE_PREFIX/lib/libpinyin/data"
export CARGO_TARGET_DIR="$PWD/target/real-installation/tkrzw"
just heavy -- cargo test -p oxpinyin-datagen --no-default-features --features tkrzw \
    --test cross_backend -- --include-ignored --nocapture
just heavy -- cargo test -p oxpinyin-runtime --no-default-features --features tkrzw \
    --test libpinyin_data_dir -- --include-ignored --nocapture
```

### The full default contract selection

"The full default contract selection" is the contract driver run with no
`--cases`, `--issue` or `--family`, on one cell:

```sh
just heavy -- tools/bisection/run-contract-diff.sh bdb "$PINYIN_ORACLE_PREFIX" \
    <target>/debug/libpinyin_capi.so <target>/debug/libzhuyin_capi.so
```

It runs every registered case except the axis C battery (`axis-c-*`), in
sorted order. At `5decad77` that is 2,889 cases (pinyin 1,734, zhuyin
1,155), and one of them is a SKIP on the BDB cell. The count changes
whenever a case is registered, so report the count from the run. Add
`-- --plan-only` to print it without running a case: the first line is
`plan: <N> cases (default suite: every registered non-axis-C case)`. Two
runs that report different counts ran different registries. For example,
2,865 is the registry at `b587d599`, before `074f0b36` added cases.

Ordinary cases remain serial by default. Add `-- --jobs N` to request
at most N active cases in the existing contract driver. Each case has its
own scratch directory, its pin and subject share the same deadline, and
verdicts and `--observations` remain in selection order. Parallel calls
use the existing heavy slot, including validated slot inheritance from
`just heavy`. Admission charges 1,024 MiB per active case, or the larger
build estimate, and reduces the requested case count when memory is short.
If one case cannot fit, admission waits or fails before starting workers.
Admission limits workers by memory, not CPU, so a `--jobs` well above the
host's CPU count can make a case hit its wall-clock deadline and report
TIMEOUT; keep `--jobs` at or below the CPU count.
The deadline starts when the case starts, after admission and queueing.

The aggregate runner already schedules cells independently, with private
oracle environments and cell targets. Select `--contract-jobs N` for its
per-cell case maximum. Its `--jobs` still counts concurrent runner rows;
the host's heavy slots and memory reservations also limit concurrent cells.
An explicit three-slot example, using already built cell artifacts, is:

```sh
OXPINYIN_HEAVY_SLOTS=3 tools/bisection/run-all.py \
    --runners contract-diff --cells bdb,kc,tkrzw --no-build \
    --jobs 3 --contract-jobs 4 --runner-timeout contract-diff=7200 \
    --oracle-root "$OXPINYIN_CLOUD_CACHE/oracle"
```

Neither case concurrency nor the host's slot count changes by default.
The final timing record reports the effective per-cell case count as `jobs`.

Five `table-conf-abort-*` cases (`dir_chunk` in `contract-diff.py`, #1011)
take a different pin path depending on the file system of `TMPDIR`, where
the harness makes its scratch. On a file system whose directories answer
`lseek(SEEK_END)` with 8 or more (ext4, for example), the pin dies of
SIGABRT. On tmpfs, the default `/tmp` of many desktop hosts, it survives
the two user-directory cases and dies of SIGSEGV in the three
system-directory cases. The subject follows the file system in all five,
and the harness probes it, so all five MATCH on both.

`user-gram-unsorted-export` and `user-gram-duplicate-export`
(`strjoinv_over_read` in `contract-diff.py`, register row 1) take a
different pin path depending on heap layout: the pin's bigram export joins
an unterminated `GPtrArray`. Where the pin completes, the subject's rows are
compared exactly and an equal result MATCHes. Where the pin dies of SIGSEGV
after the same stderr as the subject, and the subject exits normally with a
result, the case MATCHes without comparing rows. Any other pin signal, or
different stderr, is INVALID. The kc cell has faulted on a host provisioned
with `tools/cloud/setup.sh`; bdb and tkrzw completed there.

### Contract differentials in a macOS-hosted Linux container

Keep the contract harness's scratch directory and generated user directories
on the container's local filesystem, outside macOS bind mounts. Create the
directory and export `TMPDIR` **inside the Linux container** before starting
`tools/bisection/run-contract-diff.sh`:

```sh
mkdir -p /var/tmp/lane-c-contract
export TMPDIR=/var/tmp/lane-c-contract
```

The harness creates its per-run scratch under `TMPDIR` and passes that path
to both workers, which create their fresh user directories beneath it. The
non-UTF-8 pathname cases need this: in the amd64/Rosetta macOS session, a
`mkdir` containing byte `0xFF` under the Mac-mounted `/lane/tmp` failed with
`EPERM`, while the same operation under container-local `/var/tmp` succeeded.
The bounded probe recorded these two results:

| Probe pathname (byte `0xFF` shown as `\\xff`) | Result |
| --- | --- |
| `/lane/tmp/lane-c-invalid-\xff-check` (Mac bind mount) | `mkdir` failed with errno 1 (`EPERM`); no directory was created. |
| `/var/tmp/lane-c-invalid-\xff-check` (container-local) | `mkdir` succeeded and the probe removed the directory. |

Using `TMPDIR=/var/tmp/lane-c-contract`, the full Lane C BDB suite passed
**151 of 151 at `dcc635576153291619b08c839b210d6d8293a03c`**. The single
full run omitted `--cases` and used the debug subject libraries against
libpinyin 2.11.92 pin `074a2219c90feaf962d0d24f034514033ece5f99`, both on
BDB. All 302 worker stdout/stderr observations are retained outside the
commits; no harness or expectation change was needed.

The earlier pre-restack baseline was **149 of 149 at
`5995db2cdbbfd1dbf1bd3f383863b728e714ad20`**. That historical registry did
not yet contain the two user-library-token unload cases included in the
151-case run.

From the repository root, print the current number of registered cases
without running the CLI or any case. The first number is every registered
case; the second is the default selection, which leaves out the axis C
battery:

```sh
uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -c "import runpy; c = runpy.run_path('tools/bisection/contract-diff.py')['CASES']; print(len(c), sum(not n.startswith('axis-c-') for n in c))"
```

The harness runs every registered non-axis-C case, sorted, when no selector
is given (the full default selection, below). Count the registry at the
tested commit rather than assuming an earlier count still applies; a
selected-case run does not establish a complete-suite result.

Retain the original failure logs when diagnosing this setup error. Evidence
logs may be written or copied back to the mounted work directory, but keep
scratch and the user directories exercised by the cases container-local.

### Training through the C API

`pinyin_train(instance, index)` trains the n-best result `index` and
returns false unless `pinyin_guess_sentence` filled the n-best results
first (`pinyin.cpp:2676` at the pin) — it does not consume the candidate
list from `pinyin_guess_candidates`. `Session::train_top` in
`pinyin-oracle` exists to make this impossible to get wrong; harness
and bench authors call it rather than `pinyin_train` directly
(AGENTS.md points here).

## 5. What to do with a divergence

Classify it against `docs/findings/compatibility-policy.md` before
touching code: reproducing the pin is the default, and only classes
(a)–(c) may be recorded in `docs/findings/upstream-divergences.md`.
Anything else is a defect to fix. A frozen pin that moves is a STOP
(`goldens-and-pins.md`).
