# Oracle — building the pin and running the differentials

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
below runs on GitHub-hosted runners.

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
without running the CLI or any case:

```sh
python3 -c "import runpy; print(len(runpy.run_path('tools/bisection/contract-diff.py')['CASES']))"
```

The harness runs `sorted(CASES)` when `--cases` is omitted. Count the
registry at the tested commit rather than assuming an earlier count still
applies; a selected-case run does not establish a complete-suite result.

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
