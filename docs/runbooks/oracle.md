# Oracle — building the pin and running the differentials

The oracle is libpinyin built from the pinned commit, with the pinned
model20 data, on the tkrzw backend. Every parity claim in this repository
is a differential against it. Its identity is `tools/oracle/oracle-pin.txt`
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
SQLite 3 and the DBM (`libtkrzw-dev` by default; `--dbm` selects Kyoto
Cabinet or Berkeley DB for the bench oracles).

```sh
tools/oracle/build-oracle.sh            # fetches libpinyin by commit SHA, verifies, builds
```

Default prefix `~/.local/opt/pinyin-oracle` (`PINYIN_ORACLE_PREFIX`).
The script writes a manifest the differential runners compare against
`oracle-pin.txt` before trusting the prefix. The container recipes in
`tools/bisection/Dockerfile.perf-matrix` carry a prebuilt oracle at
`/opt/libpinyin-tkrzw` for the perf work.

## 3. Produce the export

The Rust side reads a system data directory it compiled itself:

```sh
cargo run -p oxpinyin-datagen -- compile --backend redb \
    --model-dir "$PINYIN_MODEL_DIR" --out-dir /tmp/oxpinyin-export
export PINYIN_EXPORT_DIR=/tmp/oxpinyin-export
```

`/tmp/oxpinyin-export` is the default every harness reads
(`oxpinyin_testsupport::model_cache::DEFAULT_EXPORT_DIR`).

## 4. Run the differentials

```sh
tools/oracle/run-differentials.sh
```

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
