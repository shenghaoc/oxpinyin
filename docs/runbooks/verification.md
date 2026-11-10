# Verification

Maintainer ruling 2026-10-04 UTC.
The maintainer decision of 2026-10-30 UTC supersedes its choice of Berkeley DB as the reference cell, because tkrzw is maintained upstream.

A cell uses one DBM backend on both sides of a comparison.
[`tools/dev/cell.py`](../../tools/dev/cell.py) manages cells.
These rules replace older instructions that require all three cells for every change.

## 1. Reference cell

Use tkrzw (`PINYIN_ORACLE_DBM=tkrzw`, the default; maintainer decision 2026-10-30 UTC).
Berkeley DB was the reference cell from 2026-10-04 to 2026-10-30 UTC.
Run baselines, differentials, revert-and-check and the gate there by default.

## 2. Backend selection

Run all three cells, tkrzw, bdb and kc, only when a backend path check selects them.
The non-default bdb and kc cells thereby still gate any change that can reach them.
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

See the [function-layout ruling](../packaging.md#function-layout) for shipping builds.

## 3. Sweep and release coverage

Run every differential on all three cells once at the start of each sweep.
Repeat that coverage once before a release.
These runs cover bdb and kc outside the per-change gate.

## 4. Revert-and-check

Run revert-and-check only on the reference cell, tkrzw.
Use a deliberate, named mutation of oxpinyin when there is no fix to revert.
This applies when the differential covers behaviour that oxpinyin already reproduces, as in the round-2 audit.
Never mutate the pin.

## 5. Local test gate

Maintainer ruling 2026-11-08 UTC: run the local checks proportionally.

While iterating, run `cargo fmt --all --check`, `cargo clippy --locked -p <crate> --all-targets -- -D warnings` and fail-fast `cargo nextest run -p <crate>` for each crate you changed. Fix diagnostics as they appear.

Before a push for review or merge, and after any rebase, run one `cargo clippy --locked --workspace --all-targets -- -D warnings` on the final tree. Also run, only when the change calls for it:

- `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --workspace` when doc comments or public items changed.
- When a `Cargo.toml`, `Cargo.lock` or `deny.toml` changed, run:
  - `cargo deny --locked --log-level error check`
  - `cargo deny --locked --all-features --log-level error check`
  - `cargo deny --locked --log-level error --manifest-path fuzz/Cargo.toml check`
- ShellCheck when a shell script changed.
- Lizard 1.24.0 (`lizard crates/ -l rust -C 40 -w`) when Rust functions changed. CCN 40 is the limit. A function above CCN 15 is worth a second look in review; it is not a refactoring target.
- `just test-tools` when `tools/` changed.

WIP checkpoint pushes: `cargo fmt --all --check` only.

Markdown-only changes: `git diff --check` and a check of their links. Source-comment changes also run `cargo fmt --all --check`.

These WIP-checkpoint and Markdown-only rules override the pre-push requirement above.

CI is the full gate. These checks cover only the host's cfg arms and backend features. A Linux pass is enough to merge: [section 10](#10-ci-merge-conditions) decides the platform and backend conditions.

Each stack member must pass its required verification independently.
Successful CI on that member's final source tree satisfies the full-gate checks for the required package, feature and platform scope.
A check that is missing, cancelled or skipped when it applies does not count as a pass.
A check skipped because it does not apply to the change is not a failure.
Run locally the checks that CI does not cover: the issue's differentials, revert-and-check and any measurement that a concrete unresolved question affecting a decision requires ([controlling rule](../source-policy.md#controlling-rule)).
These checks keep their current rules.
Stripped shipping-library sizes ([constitution §2](../constitution.md)) are not a local check.
The `shipping-sizes` CI job reports base and PR sizes in its job summary, for each PR that can change a shipping library.
Copy the table into the PR body and report any increase over base.
Do not build release libraries locally for sizes.
[`just packaging sizes run`](../packaging.md#shipping-library-sizes) reproduces the table; use it to debug a failure of the job.
All commands stay documented and runnable without CI.
Run them locally to reproduce a CI failure.

## 6. Shared tooling

Add case files to an existing driver when it can cover the behaviour.
Do not add a bespoke driver and runner for each issue.
If a new driver is necessary, explain why no existing driver could cover the case in the PR body.

Use repository `just` recipes and in-tree Python tools for builds, gates, differentials and measurements.
Use managed CPython through `uv`.
The tooling consolidation plan describes the migration.
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

Run the local checks in [verification rule 5](#5-local-test-gate); they scale with the change and its stage.

Choose checks from the changes since the last green gate:

- **Documentation, comments or findings only:** review the changed text and links.
  Follow the Markdown-only and source-comment rules in verification rule 5.
  Do not run builds, tests, differentials or backend-selection checks.
  This case overrides rule 2, including comments-only changes in backend files.
- **A review fix in code:** use the iteration checks in verification rule 5, then its pre-push checks on the final tree.
  Run the issue's differentials on the reference cell.
  CI is the full gate.
- **Revert-and-check:** repeat it only if the differential changed.
- **A conflict-free rebase:** run the workspace Clippy check in verification rule 5 on the new head.
  CI is the full gate.
  If the rebase brings in no file the branch touches, only the differentials and other change-specific checks are unnecessary.
- **A stack:** run the checks in verification rule 5 on each member's new head.
  CI is the full gate.
  Repeat the local checks CI does not run only for the changed commit and higher commits that touch the same files.

Stop when applicable checks pass and concrete failures are addressed.
Additional runs require a relevant input change, a specific unresolved failure or an explicit maintainer request.
Do not repeat runs merely to collect more evidence.

Rules 5 and 7 describe the **completion gate** (maintainer rulings
2026-10-19 UTC, 2026-10-30 UTC and 2026-11-08 UTC).

Run the local checks that CI does not cover when a coherent fix is ready, before a push for review.
Between completion gates, use iteration feedback while the fix is taking shape:

1. Run named unit tests of the touched crate.
   Select the test target as well as the test name: `cargo test -p <crate> --lib <name>` or `cargo test -p <crate> --test <target> <name>`.
   A name filter alone still compiles every test target in the crate.
2. Select a small, explicit set of differential cases that reaches the changed behaviour.
3. Name the defect's witness case and its neighbours by case ID.

For example, run the contract test that names the changed decision with `cargo test -p <crate> --test contract <name>`.
Do not use the default differential suite for iteration feedback.
Iteration feedback is not a gate result.
Do not report it as one.
Report final results and reproduction commands.
Disclose failed or unrun required checks as limitations.
A fix is ready for review when the pre-push checks in verification rule 5 and the local checks that CI does not cover pass.
A PR is not ready to merge until CI passes on its head.
A `wip/` checkpoint push runs only the checkpoint check in verification rule 5.

## 8. Builds

Build differentials in debug, never release.
Keep and reuse one target directory per cell.
Do not delete target directories between issues.
Clean once when the work is finished.

Maintainer ruling 2026-11-06 UTC: no memory admission; it withdraws the 2026-10-19 UTC ruling.
Run cargo build, test and clippy directly.
Anyone who wants to limit builds sets `CARGO_BUILD_JOBS` in their own Cargo configuration.

`CARGO_BUILD_JOBS` bounds only Cargo's own jobs.
Cargo runs the tests directly; no aggregate runner adds a second pool.
Bound Cargo with `CARGO_BUILD_JOBS` when the host needs it.

Cancellation, expiry and an OOM-killed child never count as PASS or SKIP.

## 9. Sound tkrzw library

Run the tkrzw cell, the reference cell, only against a sound libtkrzw.
Ubuntu's packaged libtkrzw is used only when `tools/tkrzw/distro-probe.sh`
reports it healthy. A build from `tools/tkrzw/build.py` and Debian's package
need no probe.
`tools/cloud/setup.sh` provides a sound library when the packaged one is defective.
CI's tkrzw action provides one on every OS.
See the [Ubuntu library finding](../findings/tkrzw-distro-compat.md).

A result against a defective libtkrzw is invalid.
Do not report or triage its DIFFERs.
Do not open follow-up work for those differences.
Rerun with the sound library on `LD_LIBRARY_PATH`.
Use `ldd` to check that both oracle and subject resolve `libtkrzw.so.1` to that library.
If none is available, report "tkrzw: not run (no sound libtkrzw on this host)" and continue.

## Procedure-specific rules

- Run independent campaign subprocesses and builds in bounded pools.
  See [independent campaign work](../plans/tooling-python.md#independent-campaign-work-1232) for isolation exceptions and the audit.
- Use `.python-version` as the sole managed Python version declaration.
  Provision Python with `just python`.
  Do not repeat the version elsewhere.
  See [setup](setup.md).
- Follow the [test input rule](../testing/README.md#test-inputs).
- Train an oracle C-API session by resetting, parsing, checking full consumption and guessing the sentence before `pinyin_train` on row 0.
  See the [oracle procedure](oracle.md).
- Report results and reproduction commands under the [reporting policy](../../CONTRIBUTING.md#results-and-reproduction).
  Use the benches procedure for measurement interpretation.
  Raw captures are not submission requirements.
- Use Bash 5.0 or newer for shell scripts.
  ShellCheck 0.11.0 is the lint floor.
  See [setup](setup.md).

## 10. CI merge conditions

These rules govern the CI gates. State each rule here only. A workflow or a runbook that needs a rule links to this section.

1. **A Linux pass is enough to merge.** The required check is `ci-aggregate`, which also requires the called store-backends gate.
   macOS and Windows are best-effort: they run nightly on main and before a release, never as a PR merge condition.
   A lane never dispatches a workflow.
2. **tkrzw is the default backend.** Berkeley DB and Kyoto Cabinet are tested only in CI, and only when their KV code changes.
   Rule 2 decides that by path, not by judgement.
   The full-data contract tests run on tkrzw only.
3. **Fuzzing runs nightly, never on the PR gate.** Do not change its schedule.
4. **Coverage is advisory.** It never drives new tests.
5. **A job runs when its inputs changed.** `CI_INPUTS` in `tools/dev/ci_tree.py` holds each job's paths.
   A Rust-only change does not run the Python harness: that job runs when `tools/` or a file its tests read changed, and then runs every harness test.
   The generated Rust tables are such files.
   Within a job, no step depends on the changed paths, so a job's success always means the same checks.
   The Python version request rule reads every tracked file, so it runs on every change.
   A later run of the same tree reuses a pull request run only when that run passed every job the later run selects, within 24 hours.
