# Verification

This procedure selects the cells, local checks and CI merge conditions for a change.
A cell uses one DBM backend on both sides of a comparison.
[`tools/dev/cell.py`](../../tools/dev/cell.py) manages cells.

## 1. Reference cell

Use tkrzw (`PINYIN_ORACLE_DBM=tkrzw`, the default), because upstream maintains tkrzw.
By default, run baselines, differentials, revert-and-check and the gate on this cell.

## 2. Backend selection

Run all three cells, tkrzw, bdb and kc, only when a backend path check selects them.
This rule makes sure that the non-default bdb and kc cells gate every change that can reach them.
Use these three checks:

1. The diff lists a file under `crates/oxpinyin-store/`.
2. A changed non-Markdown file contains `feature = "bdb"`, `feature = "tkrzw"` or `feature = "kyotocabinet"`.
3. A changed line in a `Cargo.toml` names a backend.

Check 2 excludes Markdown because documentation quotes these patterns.
To apply the checks to `base...HEAD`, run `tools/bisection/cells-for-diff.py [base]`.
The default base is `origin/main`.
The command prints the selected cells, the check that selected them, and the matching paths and lines.
Rule 7's documentation and comments case needs no backend-selection run.
If a change seems to depend on a backend but fails these checks, report that as a finding.
A finding of this kind does not authorize a three-cell run.

For shipping builds, see the [function-layout ruling](../packaging.md#function-layout).

## 3. Sweep and release coverage

Run every differential on all three cells once at the start of each sweep.
Repeat that coverage once before a release.
These runs cover bdb and kc outside the per-change gate.

## 4. Revert-and-check

Run revert-and-check only on the reference cell, tkrzw.
When there is no fix to revert, use a deliberate, named mutation of oxpinyin.
This applies when the differential covers behaviour that oxpinyin already reproduces.
Never mutate the pin.

## 5. Local test gate

Run the local checks in proportion to the change.

While you iterate, run these for each crate that you changed:

- `cargo fmt --all --check`
- `cargo clippy --locked -p <crate> --all-targets -- -D warnings`
- fail-fast `cargo nextest run -p <crate>`

Fix diagnostics when they appear.

Subject to [verification scope and precedence](#verification-scope-and-precedence), before a push for review or merge and after each rebase, run one `cargo clippy --locked --workspace --all-targets -- -D warnings` on the final tree. Also run these, only when the change calls for them:

- `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --workspace` when doc comments or public items changed.
- When a `Cargo.toml`, `Cargo.lock` or `deny.toml` changed, run:
  - `cargo deny --locked --log-level error check`
  - `cargo deny --locked --all-features --log-level error check`
  - `cargo deny --locked --log-level error --manifest-path fuzz/Cargo.toml check`
- Lizard 1.24.0 (`lizard crates/ -l rust -C 40 -w`) when Rust functions changed. CCN 40 is the limit. A function above CCN 15 is worth a second look in review; it is not a refactoring target.
- `just test-tools` when `tools/` changed.

WIP checkpoint pushes: `cargo fmt --all --check` only.

For documentation and source comments, follow [verification scope and precedence](#verification-scope-and-precedence).

The WIP-checkpoint rule and [verification scope and precedence](#verification-scope-and-precedence) override the pre-push requirement above.

CI is the full gate. These local checks cover only the cfg arms and backend features of the host. A Linux pass is enough to merge: [section 10](#10-ci-merge-conditions) decides the platform and backend conditions.

Each stack member must pass its required verification independently.
Successful CI on the final source tree of that member satisfies the full-gate checks for the required package, feature and platform scope.
A check that is missing, cancelled or skipped when it applies does not count as a pass.
A check that is skipped because it does not apply to the change is not a failure.
Run locally the checks that CI does not cover: the differentials of the issue, revert-and-check, and any measurement that a concrete unresolved question requires for a decision ([controlling rule](../source-policy.md#controlling-rule)).
These checks keep their current rules.
Stripped shipping-library sizes ([constitution §2](../constitution.md)) are not a local check.
For each PR that can change a shipping library, the `shipping-sizes` CI job reports the base and PR sizes in its job summary.
Copy the table into the PR body and report any increase over base.
Do not build release libraries locally to get sizes.
[`just packaging sizes run`](../packaging.md#shipping-library-sizes) reproduces the table; use it to debug a failure of the job.
All commands stay documented and runnable without CI.
To reproduce a CI failure, run them locally.

## 6. Shared tooling

When an existing driver can cover the behaviour, add case files to it.
Do not add a bespoke driver and runner for each issue.
If a new driver is necessary, state in the PR body why no existing driver can cover the case.

Use the repository `just` recipes and in-tree Python tools for builds, gates, differentials and measurements.
Use managed CPython through `uv`.
The documented shell entry points remain supported.
Put new repeatable orchestration in shared Python tooling.
Expose it through `just`.
Do not use ad-hoc drivers, private operators or out-of-tree controllers.
Add only the missing capability that the task needs, with proportionate tests.
Do not expand unrelated tooling or build reporting infrastructure.
Report an unavailable required check as not run.
State its blocker.
Do not substitute a private procedure, and do not claim that an unrun check passed.

## 7. Proportionate verification

### Verification scope and precedence

Re-verification is proportional to what changed since the last green gate.
Documentation or comments only need the format check: `git diff --check`, plus `cargo fmt --all --check` for source comments; review changed text and check its links.
Do not run builds, tests, differentials or backend-selection checks for these changes, including comments in backend files.
A conflict-free rebase that brings in no file the change touches needs no local re-gate; CI covers it.
These rules take precedence over the general local-gate and rebase requirements.
For code changes and rebases that affect touched files, all applicable code-verification requirements remain: use rule 5, re-run affected pin checks when engine, capi or data changes affect the branch, and verify changed generators and their artifacts.

Run the local checks in [verification rule 5](#5-local-test-gate); they scale with the change and its stage.

Choose the checks from the changes since the last green gate:

- **Documentation, comments or findings only:** follow [verification scope and precedence](#verification-scope-and-precedence).
- **A review fix in code:** use the iteration checks in verification rule 5, then its pre-push checks on the final tree.
  Run the differentials of the issue on the reference cell.
  CI is the full gate.
- **Revert-and-check:** repeat it only if the differential changed.
- **A conflict-free rebase:** follow [verification scope and precedence](#verification-scope-and-precedence).
- **A stack:** run the checks in verification rule 5 on the new head of each member.
  CI is the full gate.
  Repeat the local checks that CI does not run only for the changed commit and for higher commits that touch the same files.

Stop when the applicable checks pass and concrete failures are addressed.
An additional run requires a relevant input change, a specific unresolved failure or an explicit maintainer request.
Do not repeat runs only to collect more evidence.

Rules 5 and 7 describe the **completion gate**.

When a coherent fix is ready, and before a push for review, run the local checks that CI does not cover.
Between completion gates, use iteration feedback while the fix takes shape:

1. Run named unit tests of the touched crate.
   Select the test target and the test name: `cargo test -p <crate> --lib <name>` or `cargo test -p <crate> --test <target> <name>`.
   A name filter alone still compiles every test target in the crate.
2. Select a small, explicit set of differential cases that reaches the changed behaviour.
3. Name the witness case of the defect and its neighbours by case ID.

For example, run the contract test that names the changed decision with `cargo test -p <crate> --test contract <name>`.
Do not use the default differential suite for iteration feedback.
Iteration feedback is not a gate result.
Do not report it as one.
Report the final results and the reproduction commands.
Disclose failed or unrun required checks as limitations.
A fix is ready for review when the pre-push checks in verification rule 5 and the local checks that CI does not cover pass.
A PR is not ready to merge until CI passes on its head.
A `wip/` checkpoint push runs only the checkpoint check in verification rule 5.

## 8. Builds

Build differentials in debug, never in release.
Keep and reuse one target directory for each cell.
Do not delete target directories between issues.
Clean once when the work is finished.

No memory admission applies.
Run cargo build, test and clippy directly.
To limit builds, set `CARGO_BUILD_JOBS` in your own Cargo configuration.

`CARGO_BUILD_JOBS` bounds only the jobs of Cargo itself.
Cargo runs the tests directly; no aggregate runner adds a second pool.
When the host needs it, bound Cargo with `CARGO_BUILD_JOBS`.

Cancellation, expiry and an OOM-killed child never count as PASS or SKIP.

## 9. Sound tkrzw library

Run the tkrzw cell, the reference cell, only against a sound libtkrzw.
Linux development and CI use the repository's vcpkg overlay port.
`tools.dev.provision` installs it and writes the library path and certification
prefix. Debian release packaging, shipping-size measurements and oracle/reference
images retain Debian's library. Ubuntu's packaged library is not a development route.
See the [Ubuntu library finding](../findings/tkrzw-distro-compat.md).

A result against a defective libtkrzw is invalid.
Do not report or triage its DIFFERs.
Do not open follow-up work for those differences.
Run again with the sound library on `LD_LIBRARY_PATH`.
Use `ldd` to make sure that both the oracle and the subject resolve `libtkrzw.so.1` to that library.
If no sound library is available, report "tkrzw: not run (no sound libtkrzw on this host)" and continue.

## Procedure-specific rules

- Run independent subprocesses and builds in the bounded pool under the [parallel-work rule](../../CONTRIBUTING.md#independent-work-in-parallel).
- Use `.python-version` as the only managed Python version declaration.
  Provision Python with `just python`.
  Do not write the version anywhere else.
  See [setup](setup.md).
- Follow the [test input rule](../testing/README.md#test-inputs).
- To train an oracle C-API session, reset, parse, check full consumption and guess the sentence before `pinyin_train` on row 0.
  See the [oracle procedure](oracle.md).
- Report results and reproduction commands under the [reporting policy](../../CONTRIBUTING.md#results-and-reproduction).
  Use the benches procedure to interpret measurements.
  Raw captures are not submission requirements.

## 10. CI merge conditions

These rules govern the CI gates. This section is the only place that states them. A workflow or a runbook that needs a rule links to this section.

1. **A Linux pass is enough to merge.** The required check is `ci-aggregate`, which also requires the called store-backends gate.
   macOS and Windows are best-effort: they run nightly on main and before a release, never as a PR merge condition.
   The nightly runs their Rust checks. It runs the tools Python harness on those platforms only on a manual dispatch of `verify-nightly.yml` with `tools_harness` set.
   Dispatch it on the branch for a tooling change that can behave differently there (process control, command forwarding, cleanup), and on the release commit before a release.
   A lane never dispatches a workflow.
2. **tkrzw is the default backend.** CI alone tests Berkeley DB and Kyoto Cabinet, and only when their KV code changes.
   Rule 2 decides that by path, not by judgement.
   The full-data contract tests run on tkrzw only.
3. **Fuzzing runs nightly, never on the PR gate.** Do not change its schedule.
4. **No workflow measures coverage.** `just coverage` is for a targeted local question. Coverage never drives new tests.
5. **A job runs when its inputs changed.** `CI_INPUTS` in `tools/dev/ci_tree.py` holds each job's paths.
   A Rust-only change does not run the Python harness: that job runs when `tools/` or a file its tests read changed, and then runs every harness test.
   The generated Rust tables are such files.
   Within a job, no step depends on the changed paths, so a job's success always means the same checks.
   The Python version request rule reads every tracked file, so it runs on every change.
   A later run of the same tree reuses a pull request run only when that run passed every job the later run selects, within 24 hours.
