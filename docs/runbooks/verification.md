# Verification

This procedure selects the cells, local checks and CI merge conditions for a change.
A cell uses one DBM backend on both sides of a comparison.
[`tools/dev/cell.py`](../../tools/dev/cell.py) manages cells.

## 1. Reference cell

The reference cell is tkrzw (`PINYIN_ORACLE_DBM=tkrzw`), because upstream maintains tkrzw.
By default, baselines, differentials, revert-and-check and the gate run on this cell.
A Linux host provisions its host cell, tkrzw unless `PINYIN_ORACLE_DBM` names another; its oracle environment file holds that cell alone ([Linux setup](setup.md#linux)).
The tkrzw cell runs in the pin capture image, on macOS and Windows, and on a Linux host with a sound libtkrzw ([rule 9](#9-sound-tkrzw-library)).

## 2. Backend selection

The [local test gate](#5-local-test-gate) and [verification scope](#verification-scope-and-precedence) select local checks.
[Backend reporting](#backend-reporting) states when separate tkrzw verification is needed.
[CI merge conditions](#10-ci-merge-conditions) define CI backend coverage.

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

`just clippy` and `just test` use Cargo's default backend, tkrzw, unless the arguments name another. With plain `cargo`, pass the flags yourself, as in the commands below.

Diagnostics are fixed when they appear.

For code changes, one `cargo clippy --locked --workspace --all-targets --no-default-features --features kyotocabinet -- -D warnings` runs on the final integrated tree on Linux, as CI runs it; elsewhere it runs without the backend flags. These also run, only when the change calls for them:

- `RUSTDOCFLAGS="-D warnings" cargo doc --locked --no-deps --workspace` when doc comments or public items changed.
- `just deny` when a `Cargo.toml`, `Cargo.lock` or `deny.toml` changed. The recipe runs the cargo-deny checks of CI's `deny` job.
- `just shear` when a `Cargo.toml` changed or a Rust file was added, moved or removed.
- `just complexity` when Rust functions changed: the [complexity limit](#complexity-limit).
- `just ruff` when Python under `tools/` changed.

[Verification scope and precedence](#verification-scope-and-precedence), which covers documentation and source comments, selects when these checks apply.

CI is the full gate. These local checks cover only the cfg arms and backend features of the host. A Linux pass is enough to merge: [section 10](#10-ci-merge-conditions) decides the platform and backend conditions.

### Complexity limit

A function stays at or below Lizard CCN 15, as `just complexity` measures it.
`uv.lock` records the Lizard release.

CCN counts the linearly independent paths through a function: the number of test cases that basis-path testing needs. It indicates testing effort; it does not count the tests that are sufficient, and it does not show correctness.
A split does not remove a path; it only spreads the paths over more functions.
A split helps only when each part can be understood alone.

An exception is a `#lizard forgives(cyclomatic_complexity)` comment in the function, with a one-sentence reason.
The reason is one of these: the function's paths are one ordered operation, an exhaustive mapping, or mutually exclusive implementations.
In each case, a split would increase what a reader must hold to follow the function.
The comment waives only the function that contains it, and only CCN.

Two alternatives are rejected:

- A limit with no exceptions: some splits would serve the metric, not the reader.
- An allowed count of warnings: a new violation could replace an old one without failing.

### Final integrated tree

Each independently reviewed change must pass its applicable verification on the final integrated tree.
A fix goes on the stack member that introduces or exposes the defect.
Successful CI on that tree satisfies the full-gate checks for the required package, feature and platform scope.
A check that is missing, cancelled or skipped when it applies does not count as a pass.
A check that is skipped because it does not apply to the change is not a failure.
The checks that CI does not cover run locally: the differentials of the issue, revert-and-check, and any measurement that a concrete unresolved question requires for a decision ([controlling rule](../source-policy.md#controlling-rule)).
Stripped shipping-library sizes ([constitution §2](../constitution.md)) are not a local check.
On every pull request, the `shipping-sizes` CI job rebuilds the base and the head and reports both sizes in its job summary.
The PR body states and justifies any increase over base; it does not copy the table.
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
Recipes run under `sh -cu` on Unix and the configured `pwsh` shell on Windows.
Most delegate to one command. Recipes such as `deny` and `shear` run several checks in sequence.

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
An integration that changes no relevant input needs no additional local gate; CI covers the final tree.
These rules take precedence over the general local-gate requirements.
For code changes and integrations that affect relevant inputs, all applicable code-verification requirements remain: rule 5 applies, affected pin checks re-run when engine, capi or data changes affect the integrated tree, and changed generators and their artifacts are verified.

The local checks are those in [verification rule 5](#5-local-test-gate); they scale with the change and its stage.

The changes since the last green gate select the checks:

- **Documentation, comments or findings only:** [verification scope and precedence](#verification-scope-and-precedence) applies.
- **A review fix in code:** the iteration checks in verification rule 5 apply, then its applicable checks on the final integrated tree.
  The differentials of the issue run on the reference cell.
  CI is the full gate.
- **Revert-and-check:** it is repeated only if the differential changed.
- **An integrated change:** the checks in verification rule 5 apply to its final tree.
  CI is the full gate.
  Local checks that CI does not run are repeated only when their relevant inputs changed.

Verification ends when the applicable checks pass and concrete failures are addressed.
An additional run requires a relevant input change, a specific unresolved failure or an explicit maintainer request.
Runs are not repeated only to collect more evidence.
A broken reproduction command, or a result that contradicts the claim beyond the method's expected variability, is investigated as a bug: the software, tooling, environment assumptions or documentation that caused it is fixed, and the claim is corrected or withdrawn.

Rules 5 and 7 describe the **completion gate**.

The completion gate includes the local checks that CI does not cover.
Between completion gates, iteration feedback guides the fix while it takes shape:

1. Run named unit tests of the touched crate.
   Select the test target and the test name: `cargo nextest run -p <crate> --lib -E 'test(<name>)'` or `cargo nextest run -p <crate> --test <target> -E 'test(<name>)'`.
   A name filter alone still compiles every test target in the crate.
2. Select a small, explicit set of differential cases that reaches the changed behaviour.
3. Name the witness case of the defect and its neighbours by case ID.

For example, `cargo nextest run -p <crate> --test contract -E 'test(<name>)'` runs the contract test that names the changed decision.
The default differential suite is not used for iteration feedback.
Iteration feedback is not a gate result.
It is not reported as one.
A report gives the final results and the reproduction commands.
Raw logs, profiler captures and intermediate output stay local: they are not committed, attached, uploaded or mirrored unless the maintainer asks for specific diagnostic material. Source, functional fixtures, pins, configuration files and release packages follow their own procedures.
Failed or unrun required checks are disclosed as limitations.
A fix is ready for review when the applicable checks in verification rule 5 and the local checks that CI does not cover pass on the final integrated tree.
A PR is not ready to merge until CI passes on its head.

### Backend reporting

A verification report names the backend used.
A local Linux Kyoto Cabinet result is reported as Kyoto Cabinet, never as the "default backend".
Separate tkrzw verification is needed only when the change touches a tkrzw-specific path or code gated on tkrzw with `cfg`.
Otherwise, tkrzw coverage follows the current [nightly coverage](#10-ci-merge-conditions).

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
macOS uses Homebrew's tkrzw and Windows the repository's static build, `just tkrzw` ([setup](setup.md)).
Linux CI selects Kyoto Cabinet ([section 10](#10-ci-merge-conditions)), because GitHub's Ubuntu
runners ship a defective libtkrzw. Local Linux follows the Cargo default, tkrzw
([Linux setup](setup.md#linux)), against the distribution's libtkrzw.
A distribution libtkrzw built with LTO is defective before tkrzw 1.0.34 ([upstream issue 71](https://github.com/estraier/tkrzw/issues/71)).
The manual probe is the sentinel test, `tkrzw_library_honours_its_sentinel_protocol`: it fails on a
defective library ([finding](../findings/tkrzw-distro-compat.md)).

A Linux host provisions its host cell, tkrzw unless `PINYIN_ORACLE_DBM`
names another; a [pin capture](pin-captures.md) names the other cells with `--cells`.
Debian release packaging and oracle/reference images retain Debian's library in their containers.
See the [distribution library finding](../findings/tkrzw-distro-compat.md).

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
     cargo nextest run -p oxpinyin -E 'test(tkrzw_library_honours_its_sentinel_protocol)'
   ```

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

1. **A Linux pass is enough to merge.** The required check is `ci-aggregate`, which requires the `checks`, `rust`, `store` and `shipping-sizes` jobs.
   macOS and Windows are best-effort: they run nightly on main and before a release, never as a PR merge condition.
   The nightly runs their Rust checks. Dispatch it on the release commit before tagging.
2. **Linux CI selects Kyoto Cabinet, from Ubuntu's packages.**
   tkrzw stays the Cargo default on every platform, and every Linux CI job selects Kyoto Cabinet with the backend flag instead: the `rust` lane (clippy, rustdoc, nextest, the archive build and the fuzz check), the cell gates, `shipping-sizes`, whose size table is therefore the Kyoto Cabinet libraries', and the nightly fuzz soak.
   `CI_LINUX_CELL` in `tools/dev/cell.py` names that cell. Every job that builds a backend names it explicitly; the local tooling follows the Cargo default ([Linux setup](setup.md#linux)).
   The reason is a CI one: GitHub's Ubuntu runners ship a libtkrzw that is defective at run time ([finding](../findings/tkrzw-distro-compat.md)), and Kyoto Cabinet is reproducible there, which the exact-count measurement controls need. It is not a Linux rule: locally, Linux supports kc, bdb and tkrzw, and tkrzw needs a library that passes [rule 9](#9-sound-tkrzw-library).
   The `store` job tests Berkeley DB and runs the Berkeley DB and Kyoto Cabinet sanitizers.
   It runs on every pull request.
   tkrzw's tests, including the full-data contract tests, which are tkrzw-only, run on the nightly macOS and Windows jobs and never in Linux CI; tkrzw's sanitizer run has no CI job.
3. **Fuzz execution runs nightly.**
   The PR `rust` job lints every fuzz target in `fuzz/` with Clippy, once with the Kyoto Cabinet feature and once with the `afl` feature added.
   The execution schedule is fixed; a change to it is a CI-policy change and needs the maintainer's [approval](../../CONTRIBUTING.md#approval-conditions).
4. **No workflow measures coverage.** `just coverage` is for a targeted local question. Coverage never drives new tests.
5. **Every lane runs on every PR.** `checks`, `rust`, `store` and `shipping-sizes` run on every pull request, and `ci-aggregate` requires their ordinary job results.
   No job or step depends on the changed paths, so a job's success always means the same checks.
   The `checks` job runs the Python version request check, Ruff and the offline generated-table comparison.
   Pushes to main run the same lanes.
6. **Linux CI runs natively.** Every Linux job of `ci.yml` and `verify-nightly.yml` runs directly on the `ubuntu-26.04` runner. No CI job runs in a container.
   Every system package, the store backends included, comes from Ubuntu's apt.
   Containers serve only the pin oracle, which is local work and never runs in CI, and the distribution packaging of `release-packages.yml`.
   Rejected: Linux CI in a Debian testing container.
