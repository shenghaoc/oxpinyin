# Verification

Maintainer ruling 2026-10-04 UTC.

A cell uses one DBM backend on both sides of a comparison.
[`tools/dev/cell.py`](../../tools/dev/cell.py) manages cells.
These rules replace older instructions that require all three cells for every change.

## 1. Reference cell

Use Berkeley DB (`PINYIN_ORACLE_DBM=bdb`, the default).
Run baselines, differentials, revert-and-check and the gate there by default.

## 2. Backend selection

Run all three cells, bdb, kc and tkrzw, only when a backend path check selects them.
Use these three checks:

1. The diff lists a file under `crates/oxpinyin-store/`.
2. A changed non-Markdown file contains `feature = "bdb"`, `feature = "tkrzw"` or `feature = "kyotocabinet"`.
3. A changed line in a `Cargo.toml` names a backend.

Markdown is excluded from check 2 because documentation quotes these patterns.
Run `tools/bisection/cells-for-diff.py [base]` to apply the checks to `base...HEAD`.
The default base is `origin/main`.
The command prints the selected cells, the triggered check and the matching paths and lines.
Rule 7's documentation and comments case needs no backend-selection run.
Report apparent backend dependence that fails these checks as a finding.
It does not authorize a three-cell run.

Function-order policies are separate for each backend.
Always regenerate or check them for bdb, kc and tkrzw, independently of `cells-for-diff.py`.
See [packaging regeneration](../packaging.md#regeneration).

## 3. Sweep and release coverage

Run every differential on all three cells once at the start of each sweep.
Repeat that coverage once before a release.
These runs cover kc and tkrzw outside the per-change gate.

## 4. Revert-and-check

Run revert-and-check only on the reference cell.
Use a deliberate, named mutation of oxpinyin when there is no fix to revert.
This applies when the differential covers behaviour that oxpinyin already reproduces, as in the round-2 audit.
Never mutate the pin.

## 5. Local test gate

Run `cargo test -p <crate>`, fmt and clippy for each crate the diff touches.
CI runs the workspace sweep.
Do not run that sweep as a local gate.
Apply the heavy-work admission procedure in rule 8 to commands that compile.

## 6. Shared tooling

Add case files to an existing driver when it can cover the behaviour.
Do not add a bespoke driver and runner for each issue.
If a new driver is necessary, explain why no existing driver could cover the case in the PR body.

Use repository `just` recipes and in-tree Python tools for builds, gates, differentials and measurements.
Use managed CPython through `uv`.
The [tooling consolidation plan](../findings/tooling-consolidation-python-just.md) describes the migration.
Existing documented shell entry points remain supported until migration.
Put new repeatable orchestration in shared Python tooling.
Expose it through `just`.
Do not use ad-hoc drivers, private operators or out-of-tree controllers.
Add only the missing capability needed for the task, with proportionate tests.
Do not expand an unrelated tooling migration or build reporting infrastructure.
Report an unavailable required check as not run.
State its blocker.
Do not substitute a private procedure or claim that an unrun check passed.

## 7. Proportionate verification

Choose checks from the changes since the last green gate:

- **Documentation, comments or findings only:** review the changed text and links.
  Run `git diff --check`, then push.
  Run fmt when source comments change.
  Markdown-only changes need no Rust formatter.
  Do not run builds, tests, clippy, differentials or backend-selection checks.
  This case overrides rule 2, including comments-only changes in backend files.
- **A review fix in code:** run fmt, clippy and `cargo test -p` for touched crates.
  Run only the issue's differentials, on the reference cell.
- **Revert-and-check:** repeat it only if the differential changed.
- **A conflict-free rebase:** no local gate is needed if it brings in no file the branch touches.
  CI covers this case.
- **A stack:** repeat the gate only for the changed commit and higher commits that touch the same files.

Stop when applicable checks pass and concrete failures are addressed.
Additional runs require a relevant input change, a specific unresolved failure or an explicit maintainer request.
Do not repeat runs merely to collect more evidence.
Keep repetitions required by the measurement method.

Rules 5 and 7 describe the **completion gate** (maintainer ruling
2026-10-19 UTC).

Run the completion gate when a coherent fix is ready, before a push for review.
Between completion gates, use iteration feedback while the fix is taking shape:

1. Run named unit tests of the touched crate: `cargo test -p <crate> <test name>`.
2. Select a small, explicit set of differential cases that reaches the changed behaviour.
3. Name the defect's witness case and its neighbours by case ID.

For example, use `just runner contract-diff ... -- --cases default-option-parse,default-option-single-key`.
Do not use the default differential suite for iteration feedback.
Iteration feedback is not a gate result.
Do not report it as one.
Report final results and reproduction commands.
Disclose failed or unrun required checks as limitations.
A fix without a completion gate is not ready to push for review.
A `wip/` checkpoint push does not require a completion gate.

## 8. Builds and resource admission

Build differentials in debug, never release.
Keep and reuse one target directory per cell.
Do not delete target directories between issues.
Clean once when the work is finished.

On Linux and macOS, heavy work uses the host-wide slot and memory admission check (maintainer ruling 2026-10-19 UTC).
The native slot is not validated on Windows.
Run direct cargo build, test and clippy commands through `just heavy`.
Cell builds and runners marked `"heavy": true` in `tools/bisection/runners.json` use the slot too.

Heavy work includes:

- The bisect, contract-diff, open-counter-diff and transformed-options-sweep runners, at every selection size.
- Direct `cargo build`, `cargo test` and `cargo clippy` commands, because they compile.
- Full or family-wide differentials.
- Mutation runs, Valgrind and Massif.
- Release and LTO builds.

`run-all.py` applies both admission checks to cell builds and heavy runners.
Start other heavy work with `just heavy -- <command>`.

Light work includes:

- `cargo fmt`, plans, listings and harness tests.
- `contract-diff.py --cases` against built artifacts.
- Registry runners that are not marked heavy.

Run light work without resource admission.
Runner scripts that require Cargo builds start those builds through `just heavy --`.
Leave `CARGO_BUILD_JOBS`, `RUST_TEST_THREADS` and `--jobs` unset unless a measurement requires fixed values.
Cancellation, expiry and an OOM-killed child never count as PASS or SKIP.
See [heavy-work admission](setup.md#heavy-work-admission) for mechanics, settings, estimates, waits and refusal reasons.

## 9. Sound tkrzw library

Run the tkrzw cell only against a sound libtkrzw.
`tools/tkrzw/distro-probe.sh` must report it healthy.
`tools/cloud/setup.sh` provides a sound library when the packaged one is defective.
See the [Ubuntu library finding](../findings/tkrzw-distro-compat.md).

A result against a defective libtkrzw is invalid.
Do not report or triage its DIFFERs.
Do not open follow-up work for those differences.
Rerun with the sound library on `LD_LIBRARY_PATH`.
Use `ldd` to check that both oracle and subject resolve `libtkrzw.so.1` to that library.
If none is available, report "tkrzw: not run (no sound libtkrzw on this host)" and continue.

## Procedure-specific rules

- Run independent campaign subprocesses and builds in bounded pools under heavy admission.
  See [independent campaign work](../plans/tooling-python.md#independent-campaign-work-1232) for isolation exceptions and the audit.
- Use `.python-version` as the sole managed Python version declaration.
  Provision Python with `just python`.
  Do not repeat the version elsewhere or pass the file's `+gil` request to `uv python install`.
  See [setup](setup.md).
- Mark tests that need inputs unavailable in CI with `#[ignore = "needs …"]`.
  Fail when an input is missing.
  Do not add self-skipping tests.
  Examples include model20, system-table exports, pin-built tools and opencc.
  See [testing](../testing/README.md).
- Train oracle C-API sessions through `Session::train_top`, never directly through `pinyin_train`.
  See the [oracle procedure](oracle.md).
- Give backend-specific benches `required-features`.
  Select a bench with `--bench <name>`.
  See [benches](benches.md).
- Report results and reproduction commands under the [reporting policy](../../CONTRIBUTING.md#results-and-reproduction).
  Use the benches procedure for measurement interpretation.
  Raw captures are not submission requirements.
- Use Bash 5.0 or newer for shell scripts.
  ShellCheck 0.11.0 is the lint floor.
  See [setup](setup.md).
