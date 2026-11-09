# Oracle — building the pin and running the differentials

For contributor compatibility verification, use a
[remote Linux host](cloud.md) provisioned for every gate.
Ordinary work uses [native setup](setup.md), without an oracle or container.
The direct build and runner operations below expose the existing machinery;
extracting libraries alone is not passing parity evidence.

The oracle is libpinyin built from the pinned commit, with the pinned
model20 data. Every parity claim in this repository is a differential
against it. The default backend is tkrzw, the [reference cell](verification.md#1-reference-cell).
Build Berkeley DB and Kyoto Cabinet prefixes with `--dbm bdb` and `--dbm kc`.
A differential selects its cell with `PINYIN_ORACLE_DBM`.

The oracle is built and run only in Linux. The commands that start a build
are Python and run on Windows, macOS and Linux:
`just provision-oracle`, `just provision` and
`python -m tools.oracle.image build-image` select a container runtime
(`wslc` on Windows, Apple `container` on macOS, Podman or Docker on Linux;
see [container runtimes](setup.md#container-runtimes)) and run the builder,
[`tools/oracle/build.py`](../../tools/oracle/build.py), in a Linux container.
No host needs Bash, Git Bash or a WSL shell for this.
`build.py` itself runs only in Linux, where git, autoconf, make and the
compilers are; it does not build libpinyin natively on Windows or macOS.

The model20 archive is not redistributable
([model provenance](../findings/model-provenance.md)): no CI job fetches it, and nothing
below that needs it runs on GitHub-hosted runners. No workflow in this
repository builds the reference image. Build it on a host with
`uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image build-image`.
The image holds the libraries only and ships no model-derived data (section 2a).

## Pin identity

[`tools/oracle/oracle-pin.txt`](../../tools/oracle/oracle-pin.txt) is the
authoritative pin record. The builder, host model fetcher, cell settings and
image tools read it. To change it, follow
[changing the pin](goldens-and-pins.md#3-the-oracle-pin). Its fields are:

| field | meaning |
| --- | --- |
| `schema` | the format version of the file |
| `libpinyin_tag` | the version that the pin's `configure.ac` reports. It names the installed header directory, the pin ref and oxpinyin's drop-in version. Upstream has no release tag for it. |
| `libpinyin_commit` | the libpinyin commit that the builder fetches and verifies |
| `ibus_libpinyin_tag` | the ibus-libpinyin release line; the upstream lightweight tag points at the commit below |
| `ibus_libpinyin_commit` | the ibus-libpinyin commit that the builder fetches and verifies |
| `model_archive_sha256` | the SHA-256 of `model20.text.tar.gz`, checked before extraction |
| `dbm` | the DBM backend of the default build (the reference cell) |
| `base_image` | the base image tag of `tools/oracle/Dockerfile.oracle`, a codename tag, not a digest |
| `source_date_epoch` | the commit time of the pinned libpinyin commit. The builder does not read this field: it takes `SOURCE_DATE_EPOCH` from `git log` of the verified checkout. |
| `platform`, `platform_linux_arm64` | the supported oracle platforms. A field with the suffix `_linux_arm64` applies to linux/arm64; a field without a suffix applies to linux/amd64 (`tools.oracle.image pin-field --platform`). |

The builder fetches each upstream by commit SHA (`git fetch --depth=1`) and
verifies it by `git rev-parse HEAD` equality. A commit SHA identifies the
git tree itself, so a regenerated tarball or a moved tag cannot change it.
The model archive is a plain file download, so its SHA-256 pins it.

The pin's system data for the contract tests is a separate pin:
[`tools/oracle/libpinyin-data.json`](../../tools/oracle/libpinyin-data.json)
(`just fetch-contract-data`).

### Reproducibility boundary

Six kinds of identity apply to a built prefix. Only the first is frozen in
the repository.

1. **Pinned inputs.** The upstream commits, the model archive, the
   libpinyin-data package and the base image tag. They stay fixed until a
   pin change.
2. **Recipe identity.** `recipe_sha256` is the SHA-256 of the bytes of five
   files in `tools/oracle/`, in this order: `Dockerfile.oracle`, `build.py`,
   `version-manifest.json`, `versions.py` and `oracle-pin.txt`.
   `RECIPE_INPUTS` in `build.py` is the one list; `tools.oracle.image` reads
   it for the image tag and for prefix checks. A change to any of the five
   files changes the hash and the image tag. Prefixes and oracle images
   from the old recipe fail the identity check and must be replaced. A recipe change is not a pin change: the pinned inputs and
   the frozen contract captures stay as they are.
3. **Installed toolchain versions.** The base image is unpinned Debian
   testing. Its contents change over time: libtkrzw, Berkeley DB, Kyoto
   Cabinet, glibc, GLib and the C and C++ toolchain.
   [`tools/oracle/version-manifest.json`](../../tools/oracle/version-manifest.json)
   lists these components. Each prefix records their installed versions in
   `versions.txt`; a version change does not fail the build.
4. **Built-library hashes.** The libraries depend on that toolchain. Each
   prefix records the hashes of its header, `libpinyin.so` and
   `libzhuyin.so`; the repository never freezes them. Two builds in one
   environment give the same libraries
   ([reproducing the libraries](#reproducing-the-libraries)).
5. **Model-derived data manifest.** `oracle-data.sha256` lists the data
   files that two builds in one environment reproduce.
6. **DBM-generated data manifest.** `oracle-data-unstable.sha256` lists the
   six files that differ between two builds of one pin
   ([regenerate the data](#regenerate-the-data-after-every-pull)).

A built prefix writes its own `oracle-pin.txt` (schema `pinyin-oracle-v1`),
which records:

- `pin_ref`: the libpinyin version and commit, the model SHA-256 and the DBM (`+dbm-<cell>`);
- `recipe_sha256`, `architecture` and `versions_sha256`;
- the source commits and the model SHA-256;
- the header, `libpinyin.so` and `libzhuyin.so` hashes;
- `data_manifest_sha256` and `data_unstable_manifest_sha256`, when the prefix holds data.

Tools reuse a prefix only when its pin and recipe identity are current and
its recorded bytes match. No tool compares a built digest with a frozen
repository value. Frozen contract captures stay valid until the next pin
change.

### Oracle boundary

- Only objects that the pin recipe builds are the oracle. Every parity run
  loads only those objects.
- A distribution build of libpinyin may be compared for information. It never
  gates a change and never becomes the oracle.
- ibus-libpinyin supplies only the frontend-called ABI subset and the GSettings
  schema. A full build also proves that the pinned frontend builds against the
  pinned backend. The frontend source is not a backend implementation reference.

## 1. Fetch the model (once per machine)

```sh
export PINYIN_MODEL_DIR="$(just fetch-model | tail -n 1)"
```

The command reads the expected SHA-256 from the pin and verifies the archive.
It extracts the archive into the `model20-<digest>` entry of the
[cache directory](setup.md#cache-directory). The last stdout line is the extracted
directory. Both `just fetch-model` and the builder's `--model-dir` check the
digest in the `verified` file beside `extracted/` before reusing the extraction.

## 2. Build the oracle

### From any host

`just provision-oracle` builds the local `performance` image when it is
absent, then provisions the cells in it. It builds missing or stale
libraries, fetches model20, regenerates missing or damaged data, and
validates every recorded output before it returns:

```sh
just provision-oracle --cells 'bdb kc tkrzw' --jobs 4
```

The cells land in the provisioning cache under `oracle/<cell>`:
the `pin-<commit>` entry of the [cache directory](setup.md#cache-directory).
`OXPINYIN_CONTAINER` names a runtime other than the host's first choice.
On Windows, run it from PowerShell 7; the runtime is `wslc`
([native Windows](setup.md#native-windows-and-wsl-containers)).
[`just provision`](cloud.md) does the same work for a full compatibility
host, with the builds pooled in the `deps` image.

### With the builder, inside Linux

`build.py` has three operations:

| Operation | Option | Result |
| --- | --- | --- |
| Full | none | libraries, headers, pkg-config files, model-derived data and a build of the pinned ibus-libpinyin |
| Libraries only | `--libs-only` | libraries, headers, pkg-config files and the prefix record; no model, no data. The published image holds this. |
| Data regeneration | `--regen-data` | model-derived data, added to an existing libraries-only prefix |

[`tools/oracle/dependencies.json`](../../tools/oracle/dependencies.json)
lists the Debian build dependencies by mode and backend; a full build needs
the `full` group. The `deps` stage of `tools/oracle/Dockerfile.oracle` has the
libraries-only and data-regeneration dependencies, and the `performance`
stage has all of them.
To build libzhuyin as well, add `--enable-libzhuyin`. The zhuyin,
candidate-assembly, two-context and open-counter differentials need it.

```sh
python3 tools/oracle/build.py --prefix ~/.local/opt/pinyin-oracle --model-dir "$PINYIN_MODEL_DIR"
```

To select another cell, use `--dbm bdb` or `--dbm kc` and a separate `--prefix`.
The prefix records the cell in its `pin_ref` (`+dbm-<cell>`) and `dbm=` field.
For a differential, set `PINYIN_ORACLE_DBM=tkrzw|bdb|kc` (default tkrzw).
[`tools/dev/cell.py`](../../tools/dev/cell.py) then builds oxpinyin with the
matching backend feature and expects the matching `pin_ref`.

The requirements are:

- **Prefix.** A full or libraries-only build needs an absent or empty
  prefix, so stale output cannot enter the oracle. `--regen-data` needs a
  libraries-only prefix whose record has the current `pin_ref` and
  `recipe_sha256`. Without `--prefix`, the builder installs to
  `${TMPDIR:-/tmp}/oxpinyin-oracle/prefix`. The runners read
  `PINYIN_ORACLE_PREFIX`.
- **Model.** A full build and `--regen-data` need model20. `--model-dir`
  takes the verified extraction of section 1 and copies only the eighteen
  inventory files. Without it, the builder downloads the pinned archive with
  `curl`, checks its SHA-256 and extracts the same eighteen files.
- **Patches.** `--apply-patches DIR` applies every `*.patch` in `DIR`, in
  name order, and needs an explicit `--prefix`. The prefix records the patch
  checksums in `oracle-patches.sha256`, and its `pin_ref` ends with
  `+patches-<hash>`. Provisioning never accepts a patched prefix. The option
  cannot be combined with `--regen-data`.
- **Environment.** The builder puts the prefix first in `PKG_CONFIG_PATH`,
  keeps an inherited `PKG_CONFIG_PATH` after it, and sets `LD_LIBRARY_PATH`
  to the prefix alone. It passes `CC`, `CXX`, `CFLAGS`, `CXXFLAGS` and
  `LDFLAGS` through. Unset `CFLAGS` and `CXXFLAGS` become autoconf's
  `-g -O2`; an empty value stays empty.

Run `python3 tools/oracle/build.py --help` for every option.
The last stdout line is the path of the built `libpinyin.so`.

The builder writes the prefix's `oracle-pin.txt` last. The differential
runners compare it with the repository pin before they trust the prefix.
The local `performance` stage of `tools/oracle/Dockerfile.oracle` holds
the prebuilt libraries. Enter it with `just performance` and prepare data
with the [measurement procedure](benches.md#where-measurements-run).
The oracle never runs on GitHub Actions.

## 2a. Or pull the prebuilt libraries (any machine)

The image uses unpinned Debian testing (`tools/oracle/Dockerfile.oracle`).
Debian is the only major distribution whose libpinyin uses tkrzw, oxpinyin's
default backend; Fedora and Arch still link Kyoto Cabinet (the
[release lanes table](../packaging.md#release-artifacts-release-packagesyml)). Debian testing's package
is therefore the reference, and its updates trigger pin changes. Both arms of a
performance comparison run in the same container on the same base.

The image holds the bdb, kc and tkrzw cells and builds natively on amd64 or
arm64 (see [native arm64](#native-arm64)). The amd64 image is

    ghcr.io/shenghaoc/oxpinyin-oracle:<tag>

and arm64 publishes the siblings `<tag>-arm64` and `<tag>-deps-arm64`.
`uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image image-tag` prints `<tag>`.
The tag holds:

- the libpinyin commit and the base image tag, both from `oracle-pin.txt`;
- the first twelve digits of `recipe_sha256`
  ([reproducibility boundary](#reproducibility-boundary)).

The tag therefore changes whenever the library hashes or their record can.
A published tag is never overwritten. `tools/cloud/setup.sh`
and the commands below derive the tag the same way. The
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
with `uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image verify-image --platform "$platform" "$img"`.
No tool compares a built digest with a frozen repository value.

### Native arm64

The arm64 oracle uses the same recipe and pin as amd64, built natively with
no emulation. Build the image with
`--build-arg ORACLE_PLATFORM=linux/arm64` and `--platform linux/arm64`;
`tools/dev/provision.py` and `tools/cloud/setup.sh` select the platform from
the host. On Apple `container`, provisioning builds the libraries in the deps
image and does not pull the published image
([container runtimes](setup.md#container-runtimes)).

Run the tkrzw cell only against a sound libtkrzw, and use `ldd` to make sure
that the oracle resolves `libtkrzw.so.1` to it
([verification rule 9](verification.md#9-sound-tkrzw-library)).

The pin's candidate frequency is target-dependent at two sites, so the arm64
pin orders some candidates differently from the amd64 pin. The amd64 pin is
the reference. oxpinyin's results and the affected cases are class (b)
divergences, recorded in [the arm64 portability record](../findings/arm64-pin-portability-1137.md)
and [upstream divergences](../findings/upstream-divergences.md).

### Regenerate the data (after every pull)

The data directory is built from the model on your own machine, with
this pin's data build. Run the builder where the build dependencies of
section 2 are, for example in the `deps` stage of the image:

```sh
model="$(dirname "$(just fetch-model | tail -n 1)")"
docker build --target deps -f tools/oracle/Dockerfile.oracle -t oxpinyin-oracle-deps .
docker run --rm \
    -v "$PWD":/opt/oracle-recipe:ro \
    -v "$dest":/opt/pinyin-oracle \
    -v "$model":/model-cache:ro \
    oxpinyin-oracle-deps \
    python3 /opt/oracle-recipe/tools/oracle/build.py --regen-data --dbm tkrzw \
        --prefix /opt/pinyin-oracle/tkrzw --model-dir /model-cache/extracted \
        --work-dir /build/tkrzw-data --jobs "$(nproc)"
```

The checkout is mounted over the image's own `/opt/oracle-recipe`, so the
builder, the pin and the recipe hash are the checkout's. `just provision`
runs this same command for each cell, through the selected runtime.
Use `--dbm bdb` or `--dbm kc` with the matching subdirectory for the other
cells.

`--regen-data` does these steps:

1. It checks that the prefix records the current `pin_ref` and
   `recipe_sha256`, and refuses any other prefix.
2. It fetches the pinned libpinyin commit and builds it in a work directory
   with the same configure arguments.
3. It runs the pin's data build (`data/Makefile.am`: `gen_binary_files`,
   `import_interpolation`, `gen_unigram`) over the model, into a staging
   area beside the prefix.
4. It writes `oracle-data.sha256`, `oracle-data-unstable.sha256` and the
   data fields of `oracle-pin.txt` in the staging area.
5. It moves the old pin record and the generated paths it replaces into a
   sibling `<prefix>.data-backup` directory. It renames the staged data,
   utilities and manifests into place, then installs the new pin record last.
   The libraries, headers, pkg-config files and `versions.txt` stay in place.

Failures before step 5 leave the prefix untouched. A publication I/O error
rolls back completed moves and restores the old pin record last. If rollback
fails or publication is interrupted, keep the sibling backup for recovery.
Another regeneration refuses to start while the backup exists. A prefix
without its pin record fails validation. A process killed before cleanup may
also leave a staging directory. The renames do not provide a crash-atomic
transaction or power-loss durability.

When a tool reuses the prefix, it checks both manifest digests and all listed
files against that prefix's record.

Then use the prefix as if it had been built locally:

```sh
export PINYIN_ORACLE_PREFIX="$dest/tkrzw"
export PINYIN_ORACLE_DBM=tkrzw
```

The generated data of libpinyin is not reproducible at a fixed pin. Two clean
builds of the same pin, with the same model and image, differ on six files:
`addon_phrase_index.bin`, `addon_pinyin_index.bin`, `bigram.db`,
`phrase_index.bin`, `pinyin_index.bin` and `punct.bin`. These are the files
that `gen_binary_files` and `gen_unigram` write through the DBM backend. The
variance comes from upstream, not from the recipe. The other files are
byte-identical across two builds in one environment.

The builder therefore splits the data manifest:

- `oracle-data.sha256` lists the stable files; its digest is `data_manifest_sha256`.
- `oracle-data-unstable.sha256` lists the six files; its digest is
  `data_unstable_manifest_sha256`. It is tamper-evident within one prefix,
  and no tool compares it across builds.

`DATA_UNSTABLE_FILES` in `build.py` fixes the list. To move a file
between the manifests, change the recipe. Neither manifest is frozen across
toolchain updates. Verify both against the prefix's local manifest before any
run that reads the oracle prefix's data.

### Reproducing the libraries

The library build is reproducible. `SOURCE_DATE_EPOCH` is the commit time of
the verified checkout, the source path is remapped to a fixed name and the
ELF build ID is omitted. The libraries then hash the same for any work
directory, given the same toolchain and `--prefix`. Inside `just performance`, run:

```sh
uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image check-reproducible --cells bdb kc tkrzw --work-dir /performance/repro-check
```

This command builds each cell twice in different work directories, with the same install
prefix in one environment, and compares only those two sets of library hashes. It never updates frozen input pins.
Image verification separately checks current pin/recipe provenance, recorded
library bytes and absence of model-derived data.

For oracle-only provisioning in the same native container:

```sh
uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev.provision oracle --cache-dir /performance/provision --cells 'bdb kc tkrzw' --jobs 4
```

The provisioner rebuilds missing or stale pin/recipe prefixes, regenerates
missing or damaged data, and validates all recorded outputs before returning.
`just performance` sets `OXPINYIN_CACHE=/performance/cache` in the container, so model20 is on the volume.

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

The C-boundary tests (below) hold the pin's frozen observations of the C
ABI. The per-call C drivers retired with their frozen Rust tests; see
[pin captures](pin-captures.md).

### Pre-release fixture check

Before each release, run the three-cell sweep that [verification rule 3](verification.md#3-sweep-and-release-coverage) requires.
The command that regenerated `fixtures/w3` from model20 and compared it with the committed tree is retired (#1764).
Before the release, report each result or each outstanding input limitation.

Every ordinary test run includes the tkrzw `libpinyin_data_dir` and `sentence_surface_parity` tests, and
`package_libraries` in `oxpinyin-data`. These tests read the pinned libpinyin-data package
(`just fetch-contract-data`).

### The C-boundary tests

The C-boundary tests with the pin's frozen observations are ordinary Rust tests in the
`src` test modules of `crates/oxpinyin-capi` and `crates/oxpinyin-zhuyin-capi`; they
never run the pin. Run them with `cargo test -p oxpinyin-capi --lib` and
`cargo test -p oxpinyin-zhuyin-capi --lib` after `just fetch-contract-data`. The
local-only cross-open check is in [contract references](contract-references.md).

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
