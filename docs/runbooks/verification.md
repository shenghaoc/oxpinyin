# Verification

This procedure selects the cells, local checks and CI merge conditions for a change.
A cell uses one DBM backend on both sides of a comparison.
[`tools/dev/cell.py`](../../tools/dev/cell.py) manages cells.

## 1. Reference cell

The reference cell is tkrzw (`PINYIN_ORACLE_DBM=tkrzw`, the default), because upstream maintains tkrzw.
By default, baselines, differentials, revert-and-check and the gate run on this cell.

## 2. Backend selection

All three cells, tkrzw, bdb and kc, run only when a backend path check selects them.
This rule makes sure that the non-default bdb and kc cells gate every change that can reach them.
The three checks are:

1. The diff lists a file under `crates/oxpinyin-store/`.
2. A changed non-Markdown file contains `feature = "bdb"`, `feature = "tkrzw"` or `feature = "kyotocabinet"`.
3. A changed line in a `Cargo.toml` names a backend.

Check 2 excludes Markdown because documentation quotes these patterns.
`tools/bisection/cells-for-diff.py [base]` applies the checks to `base...HEAD`.
The default base is `origin/main`.
The command prints the selected cells, the check that selected them, and the matching paths and lines.
Rule 7's documentation and comments case needs no backend-selection run.
A change that seems to depend on a backend but fails these checks is reported as a finding.
A finding of this kind does not authorize a three-cell run.

For shipping builds, see the [function-layout ruling](../packaging.md#function-layout).

## 3. Sweep and release coverage

Every differential runs on all three cells once at the start of each sweep.
That coverage is repeated once before a release.
These runs cover bdb and kc outside the per-change gate.

## 4. Revert-and-check

Revert-and-check runs only on the reference cell, tkrzw.
When there is no fix to revert, it uses a deliberate, named mutation of oxpinyin.
This applies when the differential covers behaviour that oxpinyin already reproduces.
The pin is never mutated.

## 5. Local test gate

The local checks are proportionate to the change.

During iteration, run these checks for each changed crate:

- `cargo fmt --all --check`
- `cargo clippy --locked -p <crate> --all-targets -- -D warnings`
- fail-fast `cargo nextest run -p <crate>`

Diagnostics are fixed when they appear.

Before a push for review or merge and after each rebase, one `cargo clippy --locked --workspace --all-targets -- -D warnings` runs on the final tree. These also run, only when the change calls for them:

- `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --workspace` when doc comments or public items changed.
- When a `Cargo.toml`, `Cargo.lock` or `deny.toml` changed:
  - `cargo deny --locked --log-level error check`
  - `cargo deny --locked --all-features --log-level error check`
  - `cargo deny --locked --log-level error --manifest-path fuzz/Cargo.toml check`
- Lizard when Rust functions changed: the `run` command of the "Lizard complexity limit" step in [the CI workflow](../../.github/workflows/ci.yml), which pins the release. CCN 40 is the limit. A function above CCN 15 is worth a second look in review; it is not a refactoring target.
- `just test-tools` when `tools/` changed.
- `just ruff` when Python under `tools/` changed.

WIP checkpoint pushes: `cargo fmt --all --check` only.

The WIP-checkpoint rule and [verification scope and precedence](#verification-scope-and-precedence), which covers documentation and source comments, override the pre-push requirement above.

CI is the full gate. These local checks cover only the cfg arms and backend features of the host. A Linux pass is enough to merge: [section 10](#10-ci-merge-conditions) decides the platform and backend conditions.

### Rebases and stacks

Immediately before a push, fetch and rebase the branch onto its current base: `origin/main` for independent work, the current parent for a stack member.
Restack the descendants when a parent changes.
A fix goes on the stack member that introduces or exposes the defect.
Each stack member must pass its required verification independently.
Successful CI on the final source tree of that member satisfies the full-gate checks for the required package, feature and platform scope.
A check that is missing, cancelled or skipped when it applies does not count as a pass.
A check that is skipped because it does not apply to the change is not a failure.
The checks that CI does not cover run locally: the differentials of the issue, revert-and-check, and any measurement that a concrete unresolved question requires for a decision ([controlling rule](../source-policy.md#controlling-rule)).
Stripped shipping-library sizes ([constitution §2](../constitution.md)) are not a local check.
For each PR that can change a shipping library, the `shipping-sizes` CI job reports the base and PR sizes in its job summary.
The PR body carries a copy of the table and reports any increase over base.
Release libraries are not built locally to get sizes.
[`just packaging sizes run`](../packaging.md#shipping-library-sizes) reproduces the table, to debug a failure of the job.
All commands stay documented and runnable without CI.
To reproduce a CI failure, run the documented commands locally.

## 6. Shared tooling

When an existing driver can cover the behaviour, new cases are added to it as case files.
No bespoke driver and runner is added for each issue.
When a new driver is necessary, the PR body states why no existing driver can cover the case.

Builds, gates, differentials and measurements use the repository `just` recipes and in-tree Python tools.
CI runs every Python step on the managed CPython named in `.python-version`. Lizard and Ruff run through uv.
The documented shell entry points remain supported.
New repeatable orchestration goes in shared Python tooling.
`just` exposes it.
Ad-hoc drivers, private operators or out-of-tree controllers, and scripts and environments that exist only on one machine, are not used.
A task adds only the missing capability that it needs, with proportionate tests.
Unrelated tooling is not expanded, and no reporting infrastructure is built.
An unavailable required check is reported as not run.
The report states its blocker.
No private procedure substitutes for it, and an unrun check is never claimed to pass.
A missing input that a caller requested explicitly is a failure, never a successful skip.
No Python package is published or installed, and no console entry point is added without a new ruling.

### Shell placement

Orchestration is written in Python, in `tools.dev` or an existing harness.
A shell script stays only for one of these reasons:

1. **Bootstrap:** it runs before managed CPython exists, to provision it.
2. **Environment:** it runs where managed CPython is not available, such as a reference-image build or a container before provisioning.
3. **External interface:** an outside tool requires a shell entry point, and the script is only a thin wrapper.

Each retained script states the reason at its top, with its caller and boundary.
A `just` recipe, a workflow `run:` block, a Docker `CMD`, a sourced helper or an existing file name is not a reason to keep shell: the caller is updated instead.
Each `just` recipe is one line that delegates: under `sh -cu` on Unix, and under the `pwsh` shell that the `justfile` sets on Windows.

### Independent work in parallel

Independent subprocesses and builds run in the bounded pool, `tools/dev/pool.py`.
The pool is sized from the allowed CPUs.
Each worker has private writable state.
Output is independent of completion order.
The first failure or cancellation starts no new unit, stops the running units and reaps their children.
Dependent steps and work that needs isolation are serialized, and the tool or its runbook states the reason.
The backend builds in `tools/oracle/Dockerfile.oracle` stay serial: no `tools/dev` module is copied into the reference image.
Timing, residency, cache-sensitive and sampling-profile observations are serialised.
The host conditions of the [measurement procedure](benches.md) apply.

## 7. Proportionate verification

### Verification scope and precedence

Re-verification is proportional to what changed since the last green gate.
Documentation or comments only need the format check: `git diff --check`, plus `cargo fmt --all --check` for source comments; review changed text and check its links.
Builds, tests, differentials and backend-selection checks do not run for these changes, including comments in backend files.
A conflict-free rebase that brings in no file the change touches needs no local re-gate; CI covers it.
These rules take precedence over the general local-gate and rebase requirements.
For code changes and rebases that affect touched files, all applicable code-verification requirements remain: rule 5 applies, affected pin checks re-run when engine, capi or data changes affect the branch, and changed generators and their artifacts are verified.

The local checks are those in [verification rule 5](#5-local-test-gate); they scale with the change and its stage.

The changes since the last green gate select the checks:

- **Documentation, comments or findings only, or a conflict-free rebase:** [verification scope and precedence](#verification-scope-and-precedence) applies.
- **A review fix in code:** the iteration checks in verification rule 5 apply, then its pre-push checks on the final tree.
  The differentials of the issue run on the reference cell.
  CI is the full gate.
- **Revert-and-check:** it is repeated only if the differential changed.
- **A stack:** the checks in verification rule 5 run on the new head of each member.
  CI is the full gate.
  The local checks that CI does not run are repeated only for the changed commit and for higher commits that touch the same files.

Verification ends when the applicable checks pass and concrete failures are addressed.
An additional run requires a relevant input change, a specific unresolved failure or an explicit maintainer request.
Runs are not repeated only to collect more evidence.
A broken reproduction command, or a result that contradicts the claim beyond the method's expected variability, is investigated as a bug: the software, tooling, environment assumptions or documentation that caused it is fixed, and the claim is corrected or withdrawn.

Rules 5 and 7 describe the **completion gate**.

When a coherent fix is ready, and before a push for review, the local checks that CI does not cover run.
Between completion gates, iteration feedback guides the fix while it takes shape:

1. Run named unit tests of the touched crate.
   Select the test target and the test name: `cargo test -p <crate> --lib <name>` or `cargo test -p <crate> --test <target> <name>`.
   A name filter alone still compiles every test target in the crate.
2. Select a small, explicit set of differential cases that reaches the changed behaviour.
3. Name the witness case of the defect and its neighbours by case ID.

For example, `cargo test -p <crate> --test contract <name>` runs the contract test that names the changed decision.
The default differential suite is not used for iteration feedback.
Iteration feedback is not a gate result.
It is not reported as one.
A report gives the final results and the reproduction commands.
Raw logs, profiler captures and intermediate output stay local: they are not committed, attached, uploaded or mirrored unless the maintainer asks for specific diagnostic material. Source, functional fixtures, pins, configuration files and release packages follow their own procedures.
Failed or unrun required checks are disclosed as limitations.
A fix is ready for review when the pre-push checks in verification rule 5 and the local checks that CI does not cover pass.
A PR is not ready to merge until CI passes on its head.
A `wip/` checkpoint push runs only the checkpoint check in verification rule 5.

## 8. Builds

Differentials are built in debug, never in release.
Each cell keeps and reuses one target directory.
Target directories are not deleted between issues.
They are cleaned once when the work is finished.

No memory admission applies.
Cargo build, test and clippy run directly; no aggregate runner adds a second pool.
When the host needs a limit, `CARGO_BUILD_JOBS` goes in the contributor's own Cargo configuration.
It bounds only the jobs of Cargo itself.

Cancellation, expiry and an OOM-killed child never count as PASS or SKIP.

## 9. Sound tkrzw library

The tkrzw cell, the reference cell, runs only against a sound libtkrzw.
Linux development and CI use the repository's vcpkg overlay port.
`tools.dev.provision` installs it and writes the library path and certification
prefix. Debian release packaging, shipping-size measurements and oracle/reference
images retain Debian's library. Ubuntu's packaged library is not a development route.
See the [Ubuntu library finding](../findings/tkrzw-distro-compat.md).

A result against a defective libtkrzw is invalid.
Its DIFFERs are not reported or triaged.
No follow-up work is opened for those differences.
The run is repeated with the sound library on `LD_LIBRARY_PATH`.
`ldd` confirms that both the oracle and the subject resolve `libtkrzw.so.1` to that library.
If no sound library is available, the report says "tkrzw: not run (no sound libtkrzw on this host)" and the work continues.

If `pinyin_save` returns `false` on tkrzw while the pin saves `true`, suspect a defect-1 library.
On such a library no save fails until the context removes something (`pinyin_mask_out`, for example); then the next save with something to write fails.
Check the build of the library, not its version:

1. Find the libtkrzw that the failing process loaded.
   It need not be the one that the build linked: the C ABI objects and the test binaries carry no RUNPATH, so each loads the `libtkrzw.so.1` that `LD_LIBRARY_PATH` or the loader cache names.
   A store suite that passes and a C-ABI driver that fails in the same shell can use two different libraries.
   Use `ldd`, or `LD_DEBUG=libs` on the failing command.
2. Probe that library with the backend's own check:

   ```sh
   PKG_CONFIG_PATH=$P/lib/pkgconfig LD_LIBRARY_PATH=$P/lib \
     cargo test -p oxpinyin-store tkrzw_library_honours_its_sentinel_protocol
   ```

   `readelf -rW $P/lib/libtkrzw.so.1 | grep -c KeyComparator` prints 0 for defect 2.
3. Then read the version.
   1.0.34 is healthy under every flag set measured.
   1.0.33 and earlier are healthy from `./configure && make` and from Debian, Fedora and EPEL, and broken from Ubuntu and under Arch's default flags.

## Procedure-specific rules

- Independent subprocesses and builds run in the bounded pool under the [parallel-work rule](#independent-work-in-parallel).
- libpinyin on Linux/amd64 is the only reference for pin comparisons.
  Its behaviour in other environments is not ensured.
  An arm64 host captures the pin with a native arm64 image; the [known arm64 differences](../findings/upstream-divergences.md#outstanding-upstream-defects-report-back) are accepted.
- oxpinyin accepts differences that come from the operating system and the CPU architecture.
  Builds and containers match the host operating system and architecture; emulation is not used.
- oxpinyin accepts GLib behaviour across versions.
  Nothing checks which GLib version is present.
- Python is provisioned and its version declared as [setup](setup.md) states: `.python-version` is the only declaration.
- The [test input rule](../testing/README.md#test-inputs) applies.
- An oracle C-API session is trained as the [oracle procedure](oracle.md#training-through-the-c-api) states.
- Results and reproduction commands are reported under [rule 7](#7-proportionate-verification).
  The benches procedure interprets measurements.
  Raw captures are not submission requirements.

## 10. CI merge conditions

These rules govern the CI gates. This section is the only place that states them. A workflow or a runbook that needs a rule links to this section.

1. **A Linux pass is enough to merge.** The required check is `ci-aggregate`, which also requires the called store-backends gate.
   macOS and Windows are best-effort: they run nightly on main and before a release, never as a PR merge condition.
   The nightly runs their Rust checks. It runs the tools Python harness on those platforms only on a manual dispatch of `verify-nightly.yml` with `tools_harness` set.
   It is dispatched on the branch for a tooling change that can behave differently there (process control, command forwarding, cleanup), and on the release commit before a release.
   A lane never dispatches a workflow.
2. **tkrzw is the default backend.** CI alone tests Berkeley DB and Kyoto Cabinet, and only when their KV code changes.
   Rule 2 decides that by path, not by judgement.
   The full-data contract tests run on tkrzw only.
3. **Fuzz execution runs nightly.** The PR lint job compiles every fuzz target with `cargo check --locked --bins` in `fuzz/`, under the existing lint path gate.
   The execution schedule is fixed; a change to it is a CI-policy change and needs the maintainer's [approval](../../CONTRIBUTING.md#approval-conditions).
4. **No workflow measures coverage.** `just coverage` is for a targeted local question. Coverage never drives new tests.
5. **A job runs when its inputs changed.** `CI_INPUTS` in `tools/dev/ci_tree.py` holds each job's paths.
   A Rust-only change does not run the Python harness: that job runs when `tools/` or a file its tests read changed, and then runs every harness test.
   The generated Rust tables are such files.
   Within a job, no step depends on the changed paths, so a job's success always means the same checks.
   The Python version request rule reads every tracked file, so it runs on every change.
   A later run of the same tree reuses a pull request run only when that run passed every job the later run selects, within 24 hours.
