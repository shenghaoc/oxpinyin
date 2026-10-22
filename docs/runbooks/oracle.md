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

The oracle libraries build reproducibly: inside the pinned image
(`tools/oracle/Dockerfile.oracle`: `debian:testing` by digest, apt from
a dated snapshot) `build-oracle.sh --libs-only` produces byte-identical
`libpinyin.so` and `libzhuyin.so` wherever it runs, so their hashes are
recorded once in `tools/oracle/oracle-pin.txt`
(`libpinyin_sha256_<dbm>`, `libzhuyin_sha256_<dbm>`) rather than per
machine of the same architecture: the recorded hashes are for
**linux/amd64** only (`platform=` in `oracle-pin.txt`; the Dockerfile,
the workflow and the checks pin `--platform=linux/amd64`). CI
(`oracle-image.yml`) builds the bdb, kc and tkrzw cells and publishes them
as

    ghcr.io/shenghaoc/oxpinyin-oracle:<tag>

where `<tag>` is printed by `tools/oracle/image-tag.sh`: the libpinyin
commit, the base image digest, the apt snapshot timestamp (all from
`oracle-pin.txt`) and a hash of `Dockerfile.oracle` and `build-oracle.sh`,
so it changes whenever the library hashes can. A published tag is
immutable (the workflow never overwrites one), and `tools/cloud/setup.sh`
and the commands below derive it the same way. The hashes are also in the
image labels. The
image holds `/opt/pinyin-oracle/{bdb,kc,tkrzw}` with `lib/`, `include/`,
`lib/pkgconfig/` and `oracle-pin.txt`. It holds **no model-derived data**
(the model is not redistributable), so a pulled prefix needs the data
step below before any differential can use it.

```sh
img=ghcr.io/shenghaoc/oxpinyin-oracle:$(tools/oracle/image-tag.sh)
docker pull --platform linux/amd64 "$img"    # podman, or your runtime's equivalent
```

Copy the cells out of the image (a `FROM scratch` image has no command, so
`create` needs a dummy one) and point every cell's `.pc` files at the new
location. The destination is created first and the *contents* of
`/opt/pinyin-oracle` are copied into it (`/.` suffix), so a first run and a
rerun give the same tree:

```sh
dest=$HOME/.local/opt/pinyin-oracle        # any directory
mkdir -p "$dest"
cid=$(docker create --platform linux/amd64 "$img" /none)
docker cp "$cid:/opt/pinyin-oracle/." "$dest"
docker rm "$cid"
for cell in bdb kc tkrzw; do
    sed -i "s|^prefix=.*|prefix=$dest/$cell|" "$dest/$cell"/lib/pkgconfig/*.pc
done
```

Check the libraries against the recorded recipe hashes (a mismatch means
the image is not this repository's recipe — stop and report it):

```sh
for lib in libpinyin libzhuyin; do
    want=$(sed -n "s/^${lib}_sha256_bdb=//p" tools/oracle/oracle-pin.txt)
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
tools/oracle`). Use `--dbm kc` or `--dbm tkrzw` with the matching
subdirectory for the other cells.

Then use the prefix as if it had been built locally:

```sh
export PINYIN_ORACLE_PREFIX="$dest/bdb"
export PINYIN_ORACLE_DBM=bdb
```

The six files in `oracle-data-unstable.sha256` are not reproducible
upstream (`docs/findings/oracle-data-reproducibility.md`) and are not part
of the check.

### Reproducing the libraries

`tools/oracle/check-reproducible.sh` builds each cell twice in the pinned
image, from different work directories and prefixes, and fails unless the
two builds agree with each other and with `oracle-pin.txt`. Run it when
the recipe, the base image or the apt snapshot changes, and update the
recorded hashes in the same commit. The recipe is
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
tools/oracle/run-differentials.sh --libpinyin <built libpinyin tree> \
    --data <built system data dir> --export "$PINYIN_EXPORT_DIR" \
    --model "$PINYIN_MODEL_DIR"
```

The script takes explicit paths (`--libpinyin`, `--data` and `--export`
are required, `--model` optional) and exits 2 without them. The shell
expands the `PINYIN_EXPORT_DIR` and `PINYIN_MODEL_DIR` exports from steps
1 and 3 into those arguments before the script starts; the script itself
discovers nothing, and with `PINYIN_EXPORT_DIR` unset `--export` is empty
and the run fails.

Wires the `PINYIN_*` variables and runs the `#[ignore]`d differential
tests with `--include-ignored` (KMM, segment, lambda, counter, eval when
its inputs exist). Each suite's status is accumulated; the script exits
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
uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -c "import runpy; c = runpy.run_path('tools/bisection/contract-diff.py')['CASES']; print(len(c), sum(not n.startswith('axis-c-') for n in c))"
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
