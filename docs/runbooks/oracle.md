# Oracle — building the pin and running the differentials

For contributor compatibility verification, use a
[remote Linux host](cloud.md) provisioned for every gate.
Ordinary work uses [native setup](setup.md), without an oracle or container.
The direct build/runner operations below expose the existing machinery;
extracting libraries alone is not passing parity evidence.

The oracle is libpinyin built from the pinned commit, with the pinned
model20 data, on the tkrzw backend by default (the reference cell,
maintainer decision 2026-10-30 UTC: tkrzw is maintained upstream); Berkeley DB and Kyoto Cabinet prefixes are
built with `--dbm bdb` / `--dbm kc`, and a differential picks its cell
with `PINYIN_ORACLE_DBM`. Every parity claim in this repository is a
differential against it. Its identity is `tools/oracle/oracle-pin.txt`
(mirrored by constants in `tools/oracle/build-oracle.sh`); the history of
the pin is `docs/testing/oracle-environment.md`.

Linux only. The model20 archive is not redistributable
(`docs/findings/model-provenance.md`): no CI job fetches it, and nothing
below that needs it runs on GitHub-hosted runners. No workflow in this
repository builds the reference image. It is built on a host with
`uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image build-image`; the image holds the libraries only and
ships no model-derived data (section 2a).

## 1. Fetch the model (once per machine)

```sh
just fetch-model
export PINYIN_MODEL_DIR="$PWD/target/model20/extracted"
```

SHA-256-verified into `target/model20/`; the last stdout line is the
extracted directory.

## 2. Build the oracle

Debian build dependencies by mode and backend are listed in
[`tools/oracle/dependencies.json`](../../tools/oracle/dependencies.json). The reference image is libraries-only; a full build needs the `full` dependency group. The parity oracle uses the [reference cell](backends.md)
(maintainer decision 2026-10-30 UTC: tkrzw is maintained upstream; it supersedes the 2026-09-27 UTC ruling that
made Berkeley DB the default); differentials pick the cell with
`PINYIN_ORACLE_DBM`. The capture goldens use the tkrzw cell too.
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
`oracle-pin.txt` before trusting the prefix. The local `performance` stage of `tools/oracle/Dockerfile.oracle` carries
the prebuilt libraries. Enter it with `just performance` and prepare data
using the [measurement procedure](benches.md#where-measurements-run).
The oracle never runs on GitHub Actions.

## 2a. Or pull the prebuilt libraries (any machine)

Planning figures for the pooled builds (`tools/dev/pool.py`, `BUILD_BASE_MIB`
and `BUILD_JOB_MIB`; `tools/cloud/setup.sh` uses them): the
largest cgroup peaks measured for `build-oracle.sh` on linux/amd64, Docker,
four CPUs. `--libs-only` peaked at 361 MiB (bdb), 528 MiB (kc) and 367 MiB
(tkrzw) at `-j1` and at 432, 1,312 and 691 MiB at `-j4`; bdb `--regen-data` at
`-j4` peaked at 606 MiB. Each build is counted at 300 MiB plus 275 MiB per make
job, which covers every point. Three builds at `-j1` on four CPUs ran slower
than one at a time at `-j4`, so builds overlap only while each keeps two make
jobs.

The oracle uses unpinned Debian testing (`tools/oracle/Dockerfile.oracle`).
Debian is the only major distribution whose libpinyin uses tkrzw, oxpinyin's
default backend; Fedora and Arch still link Kyoto Cabinet (the
[release lanes table](../packaging.md#release-artifacts-release-packagesyml)). Debian testing's package
is therefore the reference, and its updates trigger pin bumps. Both arms of a
performance comparison run in the same container on the same base.

Frozen contract captures remain valid until the next pin bump; this migration
never rewrites them. Only pinned inputs stay frozen: upstream commits, model
archive and libpinyin-data package hashes. Generated libraries and data depend
on the rolling toolchain, so their hashes are recorded per prefix instead.
Each prefix's `oracle-pin.txt` records the source commits, `recipe_sha256`,
actual library hashes, installed-version record hash and generated data hashes.
`versions.txt` records the installed versions. Reuse requires current pin and
recipe identity and matching recorded bytes. The image holds bdb, kc and tkrzw
cells and builds natively on amd64 or arm64.
The amd64 tags are the historical

    ghcr.io/shenghaoc/oxpinyin-oracle:<tag>

and arm64 publishes the siblings `<tag>-arm64` and `<tag>-deps-arm64`.
`<tag>` is printed by `uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image image-tag`: the libpinyin
commit and the base image tag (both from `oracle-pin.txt`) and a hash
of `Dockerfile.oracle`, `build-oracle.sh`, the Bash floor, the version-recording inputs and
`oracle-pin.txt` itself, so it changes whenever the library hashes or their
record can. The arm64 contract gate
excludes the cases diagnosed in #1137; amd64 still
runs them. See [the arm64 finding](../findings/oracle-arm64-1132.md). A published tag is
immutable (a published tag is never overwritten), and `tools/cloud/setup.sh`
and the commands below derive it the same way. The hashes are also in the
image labels. The
image holds `/opt/pinyin-oracle/{bdb,kc,tkrzw}` with `lib/`, `include/`,
`lib/pkgconfig/` and `oracle-pin.txt`. It holds **no model-derived data**
(the model is not redistributable), so a pulled prefix needs the data
step below before any differential can use it.

```sh
tag=$(uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image image-tag)
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

Check the image's current pin/recipe stamp and its own recorded library hashes
with `uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image verify-image --platform "$platform" "$image"`.
No built digest is compared with a frozen repository value.

### Regenerate the data (after every pull)

The data directory is built from the model on your own machine, with
this pin's data build, and then checked against the recorded manifest:

```sh
just fetch-model
export PINYIN_MODEL_DIR="$PWD/target/model20/extracted"
tools/oracle/build-oracle.sh --regen-data --dbm bdb --prefix "$dest/bdb" \
    --model-dir "$PINYIN_MODEL_DIR" --jobs "$(nproc)"
```

`--regen-data` fetches the pinned libpinyin commit, builds it in a work
directory with the same configure arguments, runs the pin's data build
(`data/Makefile.am`: `gen_binary_files`, `import_interpolation`,
`gen_unigram`) over the model into a staging area beside the prefix, and
computes `oracle-data.sha256`, `oracle-data-unstable.sha256` and the data
fields of `oracle-pin.txt` there. It publishes the generated data, both
manifests and their provenance together; pulled libraries stay unchanged.
Both manifest digests and all listed files are checked against this prefix's
record when reused, not against another toolchain's output.
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
local manifest before any run that reads the oracle prefix's data.

### Reproducing the libraries

Inside `just performance`, run:

```sh
uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image check-reproducible --cells bdb kc tkrzw --work-dir /performance/repro-check
```

This builds each cell twice in distinct work directories, using the same install
prefix in one environment, and compares only those two sets of library hashes. It never updates frozen input pins.
Image verification separately checks current pin/recipe provenance, recorded
library bytes and absence of model-derived data.

For oracle-only provisioning in the same native container:

```sh
PINYIN_MODEL_CACHE=/performance/model20 uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev provision oracle --cache-dir /performance/provision --cells 'bdb kc tkrzw' --jobs 4
```

The actual provisioner rebuilds missing or stale pin/recipe prefixes, regenerates
missing or damaged data, and validates all recorded outputs before returning.

## 3. Produce the export

The Rust side reads a system data directory built by libpinyin's own
sequence; `oxpinyin-utils` provides the three programs under their pin
names. `table.conf` comes from the pin's configure (`data/table.conf.in`),
so start from an installed `data/table.conf` for the same backend:

```sh
mkdir -p /tmp/oxpinyin-export && cd /tmp/oxpinyin-export
cp "$PINYIN_DATA_DIR/table.conf" .
cargo run -p oxpinyin-utils \
    --bin gen_binary_files -- --gen-punct-table --table-dir "$PINYIN_MODEL_DIR"
cargo run -p oxpinyin-utils \
    --bin import_interpolation -- --table-dir "$PINYIN_MODEL_DIR" < "$PINYIN_MODEL_DIR/interpolation2.text"
cargo run -p oxpinyin-utils \
    --bin gen_unigram -- --table-dir "$PINYIN_MODEL_DIR"
export PINYIN_EXPORT_DIR=/tmp/oxpinyin-export
```

`/tmp/oxpinyin-export` is the default every harness reads
(`PINYIN_EXPORT_DIR` overrides it).

## 4. Run the differentials

The pin-versus-oxpinyin comparison of the C ABI is the Rust contract suite
(below). The per-call C drivers it replaced retired with their frozen Rust
tests; see [pin captures](pin-captures.md).

### Pre-release fixture check

Before each release, run the three-cell sweep required by [verification rule 3](verification.md#3-sweep-and-release-coverage).
This pre-release procedure owns `just w3-fixtures check --cells bdb,kc,tkrzw` with
model20, which regenerates `fixtures/w3` through the pin's program sequence built
for each backend and compares records and chunk bytes with the committed tree.
It needs model20; the committed fixtures do not replace that input.
Report each result or outstanding input limitation before release.

The input identity authority is `tools/oracle/oracle-pin.txt`. Its
`model_archive_sha256` (`59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155`)
identifies the archive; use its verified extraction for `PINYIN_MODEL_DIR`. Record
the model20 digest, host and cell with the results. The fixture check consumes model20 and
produces its own backend files; it does not read the oracle's data directory or
link libpinyin. From the repository root:

```sh
export PINYIN_MODEL_DIR="$PWD/target/model20/extracted"
just w3-fixtures check --cells bdb,kc,tkrzw
```

The tkrzw `libpinyin_data_dir` and `sentence_surface_parity` tests, and
`package_libraries` in `oxpinyin-data`, read the pinned libpinyin-data package
(`just fetch-contract-data`) in every ordinary test run.

### The Rust contract suite

The frozen contract cases are ordinary Rust tests in `crates/oxpinyin-capi/tests`
and `crates/oxpinyin-zhuyin-capi/tests`; they never run the pin. Run the tkrzw
set with `cargo test -p oxpinyin-capi --test contract` and
`cargo test -p oxpinyin-zhuyin-capi --test contract` after
`just fetch-contract-data`. The local-only cross-open check is in
[contract references](contract-references.md).

### Training through the C API

`pinyin_train(instance, index)` trains the n-best result `index` and
returns false unless `pinyin_guess_sentence` filled the n-best results
first (`pinyin.cpp:2676` at the pin) — it does not consume the candidate
list from `pinyin_guess_candidates`. Harnesses and benches therefore reset,
parse, check full consumption and guess the sentence before training row 0
(see [verification](verification.md#procedure-specific-rules)).

### User-profile interchange

The two-way user-profile interchange property is no longer measured live. The
property moves to frozen captures, to be added in the next pin capture. One
capture certifies both directions:

- **libpinyin to oxpinyin.** The capture holds a user directory written by the
  pin. oxpinyin reads it and renders the same results the pin recorded for it.
- **oxpinyin to libpinyin.** From the same scripted input, oxpinyin writes a
  user directory, and that directory is compared with the pin's:
  - the files oxpinyin writes in its own format (`user.conf`, the `.dbin`
    journals and the phrase-library chunks) must be byte-identical to the
    pin's;
  - the database files are written through the same tkrzw, Berkeley DB and
    Kyoto Cabinet libraries the pin uses, so their bytes are not compared.
    They must hold identical records, key for key and value for value.
    libpinyin reads those files through the same libraries, so identical
    records mean it can read them.

Until those captures exist, nothing checks this property against the pin.

## 5. What to do with a divergence

Classify it against `docs/findings/compatibility-policy.md` before
touching code: reproducing the pin is the default, and only classes
(a)–(b) may be recorded (class (c) was retired 2026-10-31 UTC; a pin
abort is reproduced, not recorded) in `docs/findings/upstream-divergences.md`.
Anything else is a defect to fix. A frozen pin that moves is a STOP
(`goldens-and-pins.md`).
