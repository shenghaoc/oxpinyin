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
`tools/oracle/build-contract-image.sh`; the image holds the libraries only and
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
`oracle-pin.txt` before trusting the prefix. The container recipes in
`tools/bisection/Dockerfile.perf-matrix` carry a prebuilt oracle at
`/opt/libpinyin-tkrzw` for the perf work.

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

The oracle libraries build reproducibly: inside the pin-reference image
(`tools/oracle/Dockerfile.oracle`: `debian:trixie`, the current Debian stable
codename, from the default mirrors with normal security updates)
`build-oracle.sh --libs-only` produces byte-identical
`libpinyin.so` and `libzhuyin.so` wherever it runs on the same platform, so
their hashes are recorded once per platform in
`tools/oracle/oracle-pin.txt` (`libpinyin_sha256_<dbm>`,
`libzhuyin_sha256_<dbm>`). The historical unqualified fields are
**linux/amd64**; linux/arm64 adds the same fields with a `_linux_arm64`
suffix. `python -m tools.oracle.image pin-field --platform <platform> <field>` selects them;
`tools/cloud/setup.sh` derives the platform from `uname -m` and refuses an
architecture with no frozen hashes. The Dockerfile and the checks take
`--platform`. The image holds the bdb, kc and tkrzw cells and is built on a
native amd64 or arm64 host. The amd64 tags are the historical

    ghcr.io/shenghaoc/oxpinyin-oracle:<tag>

and arm64 publishes the siblings `<tag>-arm64` and `<tag>-deps-arm64`.
`<tag>` is printed by `python -m tools.oracle.image image-tag`: the libpinyin
commit and the base image codename (both from `oracle-pin.txt`) and a hash
of `Dockerfile.oracle`, `build-oracle.sh`, the Bash floor, the version-check inputs and
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

Check the libraries against the recorded recipe hashes (a mismatch means
the image is not this repository's recipe — stop and report it):

```sh
for lib in libpinyin libzhuyin; do
    want=$(uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image pin-field --platform "$platform" "${lib}_sha256_bdb")
    echo "$want  $dest/bdb/lib/$lib.so" | sha256sum --check
done
```

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

The recipe is `SOURCE_DATE_EPOCH` = the pin's commit time,
`-ffile-prefix-map` and `-fdebug-prefix-map` on the source path, and
`-Wl,--build-id=none`; the `.la` and `.pc` files still record the install
prefix and are not part of the contract. When the recipe or the base image
changes, rebuild each cell twice from different work directories and
prefixes, confirm the two builds and the platform's `oracle-pin.txt` fields
agree, and update the recorded hashes in the same commit.

The two-clean-builds reproducibility check is retired
(2026-11-06 UTC): nothing runs it any more, so do the comparison above by hand
when the recipe or base image changes. `python -m tools.oracle.image
verify-image` still runs on a built image. It hashes each cell's
`libpinyin.so` and `libzhuyin.so` in the image against the platform's
`oracle-pin.txt` fields and rejects any model-derived data in the image. It
does not rebuild anything, so it does not show that a build is reproducible.

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

Run the `#[ignore]`d differential tests with `--include-ignored`
(`libpinyin_data_dir` on tkrzw, KMM, segment, lambda, counter, and eval when
its inputs exist), as in the tkrzw example under "Pre-release
real-installation checks" below. Export `PINYIN_EXPORT_DIR` and
`PINYIN_MODEL_DIR` from steps 1 and 3 and `OXPINYIN_LIBPINYIN_DATA_DIR`
for the data directory the test opens unchanged. A test that panics with
`missing input: …` names the input it lacked — that is a provisioning gap,
not a divergence. The retired `just compat differentials` wrapper ran these
suites and accumulated their statuses.

The C-ABI drop-in gate, both libraries opened on one unchanged data
directory:

```sh
cargo build --locked -p oxpinyin-capi
just runner same-data-dir-diff \
    "$PINYIN_ORACLE_PREFIX/lib/libpinyin.so" target/debug/libpinyin_capi.so \
    "$PINYIN_ORACLE_PREFIX/lib/libpinyin/data"
```

Exit 0 is identical on every driver; 2 prints each divergence. The
per-surface drivers (`tools/bisection/run-*-diff.sh`) run one surface
each; their headers say what they prove.

### Pre-release real-installation checks

Before each release, run the three-cell sweep required by [verification rule 3](verification.md#3-sweep-and-release-coverage). The tkrzw suites above own
`libpinyin_data_dir` on tkrzw. This pre-release procedure owns
`bdb_libpinyin_files` (BDB),
`reads_a_real_libpinyin_installation` (the BDB reference installation),
`libpinyin_data_dir` on KC, and `just w3-fixtures check --cells
bdb,kc,tkrzw` with model20, which regenerates `fixtures/w3` through the
pin's program sequence built for each backend and compares records and
chunk bytes with the committed tree. They need model20 or real installation
files; the committed synthetic fixtures do not replace these inputs. Ordinary `cargo test` leaves them ignored. Explicitly selecting
one fails when a required input is missing (`docs/testing/README.md`).
Report each result or outstanding input limitation before release.

Use prefixes verified by `tools/cloud/setup.sh` ([remote Linux setup](cloud.md)),
and set `TMPDIR` on disk with room for scratch data. On Ubuntu, tkrzw
must use setup's sound libtkrzw, pass `tools/tkrzw/distro-probe.sh`, and
resolve both the oracle and subject to that library with `ldd`
(verification rule 9). Keep one target directory per cell. When setup is
needed in a second worktree on the same host, set `OXPINYIN_ORACLE_ENV` to a
worktree-private file; do not overwrite another worktree's environment registration.

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
opens the KC or tkrzw installation's complete data directory. The
fixture check consumes model20 and produces its own backend files; it
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

# Every backend: the pin's program sequence over the verified model20
# extraction reproduces the committed fixtures (records and chunk bytes).
just w3-fixtures check --cells bdb,kc,tkrzw

# BDB: four real-file storage tests.
OXPINYIN_LIBPINYIN_DATA_DIR="$prefix_bdb/lib/libpinyin/data" \
CARGO_TARGET_DIR="$PWD/target/real-installation/bdb" \
cargo test -p oxpinyin-store --no-default-features --features bdb \
    --test bdb_libpinyin_files -- --include-ignored --nocapture

# BDB reference installation: real per-library chunks.
OXPINYIN_LIBPINYIN_DATA="$prefix_bdb/lib/libpinyin/data" \
CARGO_TARGET_DIR="$PWD/target/real-installation/bdb" \
cargo test -p oxpinyin-data --no-default-features --features bdb \
    --test phrase_library -- reads_a_real_libpinyin_installation --exact --include-ignored --nocapture

# KC: an unchanged pin-built data directory.
OXPINYIN_LIBPINYIN_DATA_DIR="$prefix_kc/lib/libpinyin/data" \
CARGO_TARGET_DIR="$PWD/target/real-installation/kc" \
cargo test -p oxpinyin-runtime --no-default-features --features kyotocabinet \
    --test libpinyin_data_dir -- --include-ignored --nocapture
```

The tkrzw runtime suite can be run on its own with the same inputs, after
loading setup's tkrzw environment:

```sh
export PINYIN_MODEL_DIR="$HOME/.cache/oxpinyin/model20/extracted"
export OXPINYIN_LIBPINYIN_DATA_DIR="$PINYIN_ORACLE_PREFIX/lib/libpinyin/data"
export CARGO_TARGET_DIR="$PWD/target/real-installation/tkrzw"
cargo test -p oxpinyin-runtime \
    --test libpinyin_data_dir -- --include-ignored --nocapture
```

### The full default contract selection

"The full default contract selection" is `verify` with no `--cases`, `--issue`,
`--family` or `--cases-prefix`, on one cell. It runs the subject only, against
the pin observations frozen in `fixtures/contract-refs` (see
[contract references](contract-references.md)); no pin prefix, pin library or
container is involved:

```sh
just runner contract-diff verify tkrzw \
    <target>/debug/libpinyin_capi.so <target>/debug/libzhuyin_capi.so -- --jobs 4
```

It runs the registered default suite in sorted order. Print the current case count
without running cases by adding `-- --plan-only` to the command above. Run the
command without that option to print the current `verdict_counts` summary.
The KC and BDB cells run only cases that read no system data; the package data's
`bigram.db` is a tkrzw file. Select the Axis C battery with
`-- --cases-prefix axis-c-`; add `--plan-only` to print its current count.

For a smaller selection, `--cases-prefix P,Q,...` selects literal ID prefixes
from the full registry, including untagged cases and IDs without `/`, and
`--cases @FILE` reads the IDs one per line:

```sh
just runner contract-diff verify tkrzw <target>/debug/libpinyin_capi.so \
    <target>/debug/libzhuyin_capi.so -- \
    --cases-prefix table-conf-,user-conf-,glib-922-save-,stderr-save-,stderr-fini- --plan-only
```

Every prefix must match; an empty component or an unmatched prefix is an
error, even alongside another selector. The driver unions explicit `--cases`
IDs, repeatable `--issue` tags, repeatable `--family F` selections (`F/` IDs),
then prefixes, in that order. Each issue, family and prefix contributes
sorted IDs; overlapping selections keep the first position and all reasons
in the plan.

Ordinary cases default to `OXPINYIN_CONTRACT_JOBS`, else the CPU count.
Add `-- --jobs N` to request
at most N active cases. Each case has its own scratch directory and deadline,
and verdicts and `--observations` remain in selection order. A `--jobs` well
above the host's CPU count can make a case hit its wall-clock deadline and
report TIMEOUT; keep `--jobs` at or below the CPU count.

`--expect-parent` verifies a parent build against the same references: a case
marked `control` must still pass and every other case must fail
(revert-and-check).

Three kinds of case follow the host, and the references record what they
depended on at capture:

- A `fatal=True` case (GLib's fatal critical) records the capture GLib. The
  comparison always comes first: an equal result is `MATCH` on any host. Only
  when the comparison `DIFFER`s and the host GLib differs from the capture's is
  the case reported as `HOST-DEPENDENT`, the documented cross-host difference.
- The lambda scanner cases (`table-conf-lambda-*`, `table-conf-919-lambda-*`)
  read `%f` with the host's libc and record the capture glibc, with the same
  rule: compare first, `HOST-DEPENDENT` only for a mismatch on another glibc.
  Every other mismatch, and every mismatch of any other case, is `DIFFER`.
  `HOST-DEPENDENT` is counted apart from `MATCH` and never added to it.

The two bigram-export cases (`user-gram-*-export`) and the six cases whose pin
stderr came from freed memory are Rust tests, because no frozen reference can
hold them: see `e2e_tests` in `crates/oxpinyin-capi` and
`crates/oxpinyin-zhuyin-capi`.

### Training through the C API

`pinyin_train(instance, index)` trains the n-best result `index` and
returns false unless `pinyin_guess_sentence` filled the n-best results
first (`pinyin.cpp:2676` at the pin) — it does not consume the candidate
list from `pinyin_guess_candidates`. Harnesses and benches therefore reset,
parse, check full consumption and guess the sentence before training row 0
(see [verification](verification.md#procedure-specific-rules)).

### User-profile interchange

The two-way user-profile interchange property is no longer measured live. Its
two ignored tests in `crates/oxpinyin-runtime/tests/user_dir_round_trip.rs`
and the launcher that ran them are removed (2026-11-06 UTC). The property
moves to frozen captures, to be added in the next pin capture. One capture
certifies both directions:

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
