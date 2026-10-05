# Live parity gates

`tools/bisection/run-all.sh` runs the registered ABI differentials against the
pin-built oracle. It does not provision or patch the oracle, cache oracle output,
run a workspace test sweep, or execute performance recipes. The registry is
`tools/bisection/runners.json`; `--list` prints its issue/register references,
supported cells, required inputs, oracle variant, timeout and exclusive resources. Numeric tags
refer to repository issues/PRs; `row:N` refers to the divergence register,
`coverage:N` to the numbered ABI coverage matrix, and W/task tags retain the
roadmap identifiers. Named `register:` tags identify the relevant ABI or pin
register. The table reports the expected pin reference for the selected cell;
the manifest must match it before any oracle gate executes.

Use Berkeley DB for a lane's ordinary before/after verification. Select all
three cells for the start-of-sweep and pre-release differential run. The
aggregate changes no CI policy; direct `run-live-typing-diff.sh` keeps its legacy
missing-oracle exit behavior for CI.

```sh
# A lane's named runners on the reference cell.
tools/bisection/run-all.sh --cells bdb \
  --runners addon-candidate-diff,union-diff \
  --oracle bdb=/path/to/oracle/bdb --data bdb=/path/to/bdb-data

# Find and run the differentials associated with an issue or register row.
tools/bisection/run-all.sh --list --issue 599
tools/bisection/run-all.sh --cells bdb --issue row:35 \
  --oracle bdb=/path/to/oracle/bdb --data bdb=/path/to/bdb-data

# One sweep, including both facades' SYSTEM_FILE interchange.
tools/bisection/run-all.sh --cells bdb,kc,tkrzw --jobs 2 \
  --oracle-root /path/to/oracle --target-root /path/to/retained/targets \
  --data bdb=/path/to/bdb-data --data kc=/path/to/kc-data \
  --data tkrzw=/path/to/tkrzw-data \
  --ibus-build bdb=/path/to/work-bdb --ibus-build kc=/path/to/work-kc \
  --ibus-build tkrzw=/path/to/work-tkrzw
```

The three oracle prefixes must carry their own exact `oracle-pin.txt` reference
and matching native data. The subject data directories must use that cell's
backend. The key/dictionary/phrase and zhuyin batteries also need the pinned
`interpolation2.text`; an oracle install that omits it needs a separate data copy
containing it. Do not modify the reference installation to satisfy a runner.
The import gate additionally requires the pin-built IBus frontend objects;
`--ibus-build CELL=DIR` supplies those per cell. A generated UTF-8 locale and
native backend dump utilities are among the stateful gates' other prerequisites.
The requested-row training gates use `db5.3_dump` for bdb, `kchashmgr` for kc,
and `tkrzw_dbm_util` for tkrzw; installing a backend development library alone
does not provide those command-line tools.

The registry's `oracle_variant` defaults to `unpatched`. Only
`bigram-export-diff` and `residue-a-tail-diff` select `bigram-export-strjoinv`.
For an unpatched base prefix `ROOT/CELL` (including `--oracle CELL=PREFIX`),
`oracle-cell.sh` resolves that variant to `ROOT/CELL-bigram-export-strjoinv`.
It derives the expected patch-qualified pin reference from the committed patch
manifest recipe. The aggregate uses this selected prefix in both runner argv
and `PINYIN_ORACLE_PREFIX`; every other runner retains the unpatched base.
Subject data and Cargo targets are independent of oracle variant selection.

Provision separate prefixes; never patch the original installations. From the
repository root, for CELL=bdb,kc,tkrzw (replace ROOT with the oracle parent):

```sh
tools/oracle/build-oracle.sh --dbm CELL --enable-libzhuyin --jobs 4 \
  --apply-patches tools/bisection/patches/bigram-export-strjoinv \
  --prefix ROOT/CELL-bigram-export-strjoinv \
  --work-dir ROOT/work-CELL-bigram-export-strjoinv
```

The unpatched residue-A crash is register row 1, class (b): the bigram iterator
passes an unterminated pointer array to `g_strjoinv`, causing a heap over-read.
The lane GDB frame is `pinyin_bigram_iterator_has_next_phrase`, pinned
`pinyin.cpp:862`, during `X2-train1`; a passing unpatched run is chance under UB.
The direct residue-A runner requires the patch marker in
`oracle-patches.sha256` and fails if it, the prefix, or required oracle data is
missing. Both patched registry entries require the exact patch-qualified pin
identity. An absent patched prefix is FAIL, never an allowable skip.

`oracle-cell.sh` owns backend selection and target resolution. It exports
`CARGO_PROFILE_DEV_OPT_LEVEL=1`; no Cargo manifest or shipped source is changed.
The aggregate builds the requested cells' capi, zhuyin-capi and dictool dev
artifacts once before scheduling execution. Each cell has a separate retained
Cargo target under `--target-root` (default `target/cells`). An explicit
`CARGO_TARGET_DIR` is accepted for one cell; using it with multiple cells is an
error. Keep these targets between issues. No runner should rebuild a supplied
library: `OXPINYIN_CAPI_SO`, `OXPINYIN_ZHUYIN_SO` and `OXPINYIN_DICTOOL` name the
prebuilt artifacts. Existing positional and runner-specific aliases remain
available. `--no-build` uses the existing artifacts at the selected cell targets.
A supplied artifact must match the selected backend and source revision; a path
alone cannot prove that provenance.

The runner protocol is exit 0 PASS, 77 SKIPPED, any other status FAIL. A missing
unpatched oracle is SKIPPED; a required patched oracle is FAIL when absent.
An off-pin or wrong-cell manifest is FAIL. The aggregate
prints one `runner | cell | status | seconds | pin_ref | reason` table and exits
nonzero on any FAIL or unapproved SKIPPED. `--allow-skipped runner:cell` permits
only that skip, `--allow-skipped runner` permits that runner's selected cells,
and `--allow-skipped '*'` explicitly permits all skips. Allowances never turn a
failure into success. Skipped rows remain in the table with their reason.

The default timeout is 900 seconds. The registry gives open-counter 1800
seconds for its repeated stateful launches. `--timeout N` overrides every
runner; `--runner-timeout NAME=N` overrides one runner and takes precedence.
A deadline kills the whole process group and produces FAIL, including when a
child ignores termination. Time includes execution, not waiting for an exclusive
resource lock. `--build-timeout N` bounds each cell's build separately.

`--jobs` bounds concurrent runner processes. Private native binaries, logs and
profiles permit independent supplied-artifact gates to overlap. The registry
locks the remaining source-directory outputs across cells and aggregate
invocations. These locks do not protect against someone directly launching a
legacy runner into the same checkout: use a lane-owned worktree. Explicit
caller-owned output/profile paths remain the caller's responsibility.

`--json FILE` writes the same result rows for machine consumption. Raw execution
captures are temporary and removed at aggregate exit; oracle results are always
fresh. Failure reasons are summarized from the runner's diagnostics. For a full
native diagnostic, rerun the named runner directly with the same cell and
artifacts.

The eight measurement recipes are listed as `not-a-gate`: callgrind-differential,
perf-baseline, perf-matrix, perf-same-data, rss-diag, shipped-check,
system-import-profile and tree-recipe-control. They are unchanged, omitted from
the default gate set, and rejected if selected for execution. For example,
`--list --runners perf-baseline` reports its metadata without running it.

Known failures stay observable. Bigram-export requires an instrumented oracle
and reports FAIL when its patch marker is absent. Residue-A likewise requires
the patched oracle and compares its live output without driver or expectation
adjustments. Bisect retains full C API smoke/Valgrind
coverage and checks the pin’s specific `nihao` right-tail assertion against the
C API’s safe false return (compatibility register row 14); this is not a full
oracle byte-parity claim. Dynamic-off training selects the cell’s complete
fixture directory. The original sweep and focused follow-up results are linked
from the tooling-lane findings.

The registry also includes the ABI export/allocation gates, static package
version check, installed metadata comparison, user-directory round-trip,
requested-row training and transformed-option sweep from the lane inventory.
The older Rust training suites, provisioning/capture scripts and sanitizer
build recipes remain standalone procedures; they are not ABI runner entries.

The user-directory gate consumes `OXPINYIN_USER_RT_TEST`, the existing
`user_dir_round_trip` integration-test executable. If that gate is selected,
the cell build stage compiles it once with `cargo test --profile dev --no-run`
and records a stable symlink in the cell target. Gate execution runs only its
two existing ignored cases; it never invokes Cargo. An independently supplied
capi library therefore also needs that matching prebuilt test executable.

Installed metadata is a supplied-tree gate. Its staging inputs may be set per
cell without changing the global process environment:

```sh
tools/bisection/run-all.sh --runners pc-metadata --cells bdb \
  --oracle bdb=/path/to/oracle/bdb \
  --cell-env bdb:OXPINYIN_METADATA_PIN_ROOT=/path/to/pin-stage \
  --cell-env bdb:OXPINYIN_METADATA_SUBJECT_ROOT=/path/to/subject-stage \
  --cell-env bdb:OXPINYIN_METADATA_PREFIX=/the/matching/install/prefix
```

These are trees staged with the same prefix/libdir/header layout, not just two
shared-object paths. The direct metadata runner's optional `--build-subject`
path now uses dev opt-level 1 too. The aggregate never installs or empties a
staging root. Missing staging configuration is visible as SKIPPED. The generic
`--cell-env CELL:NAME=VALUE` option is for additional runner inputs; core cell,
artifact and target settings must use their dedicated options.

The [recorded tooling-lane sweep](../findings/tooling-lane-gates.md) includes
the complete three-cell table and the residue-A invocation comparison.
