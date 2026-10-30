# Tooling layer 1: native Python

Status: partial implementation. Reconciled: 2026-10-21 UTC (`date -u`).
The #938 → #939 → #941 → #942 → #943 stack is merged on `main`.
This document records current code and remaining requirements. It installs nothing.

| Capability | Status on main |
| --- | --- |
| Exact interpreter, uv pins, managed execution and setup instructions (§2) | Implemented in [#939](https://github.com/shenghaoc/oxpinyin/pull/939). Interpreter provisioning does not provision the full native or container environment. |
| Package, CLI, host report, bootstrap doctor and identity checks (§3) | Implemented in [#941](https://github.com/shenghaoc/oxpinyin/pull/941); nextest version check added in [#943](https://github.com/shenghaoc/oxpinyin/pull/943). Doctor reports capabilities; it does not install them. |
| Registry service (§3) | Implemented in [#941](https://github.com/shenghaoc/oxpinyin/pull/941) and [#948](https://github.com/shenghaoc/oxpinyin/pull/948) (merge `219f4205`): read, validate and select by runner, cell, issue and kind through `registry.select`, with tests. The `implementation` field is implemented in [#1033](https://github.com/shenghaoc/oxpinyin/pull/1033) (maintainer ruling 2026-10-22 UTC): every `runners.json` row names `legacy:<path>`, `python:<module>` or `instrument:<path>`, and `registry.load` rejects a missing field, an unknown form and a path or module that does not exist. |
| Python feature policy (§4) | Partial: #939 sets normal import mode in execution commands. #1013 adds the entry-point `sys.get_lazy_imports()` guard in `tools/dev/__main__.py`, tested by `tools/dev/entry.test.py`. Optional lazy imports, frozendict, sentinel and sampling use remain proposed. |
| Shared native slot, admission, process cleanup and legacy adapter (§5) | Implemented in #941, from the merged #905/#906/#903 contracts and #938 acceptance. Platform acceptance remains partial. |
| Container lifetime budget integration (§5) | Implemented in [#957](https://github.com/shenghaoc/oxpinyin/pull/957) (merge `af9df745`), [#969](https://github.com/shenghaoc/oxpinyin/pull/969) (merge `bdb520f4`) and [#978](https://github.com/shenghaoc/oxpinyin/pull/978) (merge `e219d33d`): Layer 4's hard limit and native lifetime reservation exist, with one desktop-headroom application and 2560/4608 MiB defaults. [#998](https://github.com/shenghaoc/oxpinyin/pull/998) (merge `3e280c8c`) adds cache-aware admission. Platform acceptance stays Layer 4's record. |
| Full environment provisioning and complete three-platform acceptance (§§2, 6) | Proposed provisioning; partial acceptance. [#955](https://github.com/shenghaoc/oxpinyin/pull/955) (merge `21be16aa`) adds a Python-only cloud provisioning mode, not full environment provisioning. Native compiler/backend setup remains a separate runbook procedure. |

macOS: bounded checks passed on 2026-10-20 UTC at
`b034e8342e70aab874b32414976cc9b6d232dc1d`. uv 0.13.1 selected managed
CPython 3.15.0 with `Py_GIL_DISABLED == 0` and an enabled runtime GIL.
`UV_PYTHON`, `PYTHONHOME` and `PYTHONPATH` overrides each failed with status 1.
`uv ... -m tools.dev doctor` and `just doctor` had identical status and output.
The slot, CLI, host and test-runner suites passed 6, 7, 4 and 16 tests.
The slot suite included wrapper-kill handoff. Live `just heavy` children
returned 143 for SIGTERM and 137 for SIGKILL. The macOS host reader returned
positive `available_mib`; `sysctl` and `vm_stat` reads were available.
Fresh-install, offline cache and full environment provisioning acceptance
remain unverified.
Windows: TODO (no host); tooling verification is deferred. It does not block
T2N (maintainer ruling 2026-10-21 UTC).
Hosted CI runs the repository Python harnesses on Linux and macOS ([#988](https://github.com/shenghaoc/oxpinyin/pull/988), merge `96ca8c8e`). It does not run the container launcher's runtime paths. Earlier host checks retain
their original heads and limitations in #939/#941; they do not verify this stack.
The requirements below remain proposed where the table does not mark them implemented.

## 1. Scope and ownership

Use the host toolchain for native builds and tests. Native commands need no
oracle, pin installation or container. They never start a container.
Keep native targets, locks, interpreters, caches and results in native state.
The later container layer owns its separate environment.

Python provides the scripting layer. `just` provides the command interface.
The nextest layer owns Rust test orchestration. Keep these responsibilities
separate. This reconciliation changes no code or verification rule.
Each remaining implementation PR needs its own approval.

## 2. Interpreter contract

Use uv-managed CPython of the 3.15 series only: 3.15.0 or newer, below 3.16
(maintainer ruling 2026-10-23 UTC, #1074). Use the standard library only.
Require uv 0.13.1 or newer (maintainer ruling 2026-10-22 UTC). This floor
replaces the earlier exact uv 0.13.1 pin. CI installs exactly uv 0.13.1 from
the checksummed release. Hosts may use any later uv.
Use the GIL-enabled CPython request that `.python-version` states, the one place
the interpreter version is written (#1246).
uv 0.13.1 documents the implementation/version and `+gil` request forms in
its [versioned Python guide](https://github.com/astral-sh/uv/blob/0.13.1/docs/concepts/python-versions.md).
Support no older version, system-Python fallback or transition implementation.
A later ruling must approve a Python package dependency.

PR #939 added these root files:

| Implemented file | Required content |
| --- | --- |
| `.python-version` | the GIL-enabled request: the series, `+gil` and nothing else |
| `uv.toml` | `python-preference = "only-managed"` and `required-version = ">=0.13.1"` on separate lines |

Keep the interpreter series and the uv floor. Do not vendor Python. Do not add `pyproject.toml`,
a virtual environment or `uv.lock` for interpreter provisioning alone.
Inside the checkout, repository-owned uv calls let `.python-version` decide: no call
restates the request. A script that changes directory first, or a container build,
copies the file in or passes its contents through. Use `uv run --no-project --managed-python python -X lazy_imports=normal ...`
for execution. An installed system Python 3.15 does not satisfy this rule.
Reject another implementation or series. A later 3.15 patch release is
accepted without a change; 3.16 needs a separate verified change, because the
tooling relies on 3.15's `-X lazy_imports` modes.

On Linux and macOS, install uv through its documented host route.
On Windows, use a native uv executable from PowerShell or the native installer.
Neither route needs an older Python first.
Then run `just python` (#1230), the provisioning entry point: it runs
`tools/install-python.sh`, which installs only when
`uv python find --managed-python` finds no interpreter and fails if that find
still fails afterwards. Every `uv run` recipe except
`doctor` depends on it; doctor installs nothing and names it.
uv 0.13.1 has no separate `+gil` download, and `uv python install` with no
arguments fails on the file's `+gil` request. The install script derives the
install request from the file by dropping `+gil`, the one place that does (the
PowerShell recipe repeats the derivation, as PowerShell cannot run the script);
execution and selection keep the file's request.
Check `Py_GIL_DISABLED == 0` and the enabled runtime GIL. Check CPython
3.15.0 or newer, below 3.16, and the resolved executable against
`uv python find --managed-python`.
These checks enforce the managed GIL build after installation.
`tools/dev/python-request.test.py` fails when a Python version request is written
anywhere but `.python-version`; the dated records in `docs/findings/` and
`docs/perf/` keep their wording.
Keep the managed interpreter and uv cache on persistent native storage.
The setup enforces the uv 0.13.1 floor and records the interpreter provisioning route.
See uv's [Python guide](https://docs.astral.sh/uv/concepts/python-versions/)
and [managed-only setting](https://docs.astral.sh/uv/reference/settings/#python-preference).

Report a missing uv executable before starting Python. Report network failure,
an unavailable managed build or a missing cached interpreter precisely.
Do not select system Python after a provisioning failure.
After a fresh OS install and in a fresh terminal, repeat the same setup.
Keep [native contributor setup](../runbooks/setup.md) as the Rust and native
compiler route. Python setup does not install an oracle or native DBM library.

## 3. Package, CLI and doctor

Run `tools.dev` from the checkout. Use a library package under `tools/dev/`
with a thin `__main__` entry point. Keep one CLI. Make each subcommand
callable as a function. Put process, path and environment logic in Python.
Publish or install no Python package. A console entry point needs a new ruling.
Do not import product Rust crates into Python.

This layer owns native `cli`, `host` and `slot` services.
Layer 5 owns the registry, observation, comparison and migration semantics.
Use one implementation of each service. A later stage may reuse code inside
another environment; it must not reuse that environment's state.

Bootstrap in this order: check uv, provision managed CPython, then run
`uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev doctor`.
This command works before `just` installation. It reports missing `just`
and its installation steps. Normal `just doctor` calls the same code.

Doctor reports native capabilities only. Include OS, architecture, the selected
Rust toolchain, compiler and native backend utilities. Report uv, interpreter
version, implementation, executable and build identity. Report whether a
container runtime is present; do not provision, enter or validate a container
from native doctor. Presence of Valgrind does not establish a supported
runtime adapter or a passing compatibility gate.

Doctor is also the one check of tool minimums (maintainer ruling 2026-10-23
UTC, #1074). It checks just, cargo-nextest, cargo-c, cargo-fuzz and
cargo-llvm-cov everywhere, and GNU ld, gcov and Valgrind on Linux. Each line
names the minimum and the version found: a newer tool passes, an older one
fails, and a missing one is reported with its installation step. Semantic
versions compare as semver does (a pre-release of the minimum is older than it,
build metadata does not count); GNU ld, gcov and Valgrind compare by number,
and a development build counts as its number. Runbooks and
`tools/cloud/setup.sh` point to doctor instead of repeating version lists, and
setup installs uv or Valgrind only when it is missing or older than its minimum.

Doctor rejects `UV_PYTHON` and other external interpreter-selection overrides.
Include version, implementation, variant, architecture and executable-path
overrides. Reject `PYTHONHOME` and `PYTHONPATH` injection too.
Name the setting and explain how to remove it. Do not silently accept it.
Verify the CPython version, managed executable and GIL-enabled build identity.
Do not accept a free-threaded build merely because its GIL is enabled at runtime.

## 4. Python 3.15 features

Force normal import mode before importing any application module.
The execution command above uses `-X lazy_imports=normal` at interpreter start.
Implemented in #1013: `tools/dev/__main__.py` checks `sys.get_lazy_imports()`
before any application import. Unless the mode is `normal`, it writes one
line to stderr naming the mode and `-X lazy_imports=normal` (or
`PYTHON_LAZY_IMPORTS=normal`) and exits with status 2. When
`sys.get_lazy_imports` is missing, it names `.python-version` and exits
with status 2. The guard also protects direct entry-point use. CPython 3.15.0
accepts only `normal` and `all` at startup and stops on `none` before any
code runs. See
[PEP 810's global control](https://peps.python.org/pep-0810/#global-lazy-imports-control).

Use explicit lazy imports (PEP 810) only for modules that `doctor`, `list`
and `plan` do not need. Do not lazily import case-registration or required-state
modules without a test that proves unchanged registration and behaviour.
Use `frozendict` (PEP 814) only for configuration that stays read-only.
Use `sentinel` (PEP 661) only when a named singleton must differ from `None`.
Use `profiling.sampling` only for optional diagnostics. Keep it outside native
measurement intervals. Exclude the experimental JIT and free-threaded build
until a separate measured decision.

## 5. Native heavy slot and memory admission

PR #941 delivered T2N's `tools.dev.slot` by extracting the slot and admission
core from `tools/bisection/run-all.py`. It is the one implementation. Preserve that
script's slot and admission semantics from #905/#906/#903, subject to the
fail-closed ruling below. `run-all.py` imports the core through an adapter and
keeps its CLI, its rows, its environment variables and its cancellation
semantics unchanged. It remains authoritative for its legacy behaviour until
T5. Support one host, one filesystem and one environment per heavy lock domain.
Do not claim distributed locking or shared native/container slot ownership.
Layer 4's host budget record does not share a heavy lock domain.

Keep `heavy-slot-<n>`, `heavy-admission`, holder records and memory reservations.
Keep `OXPINYIN_HEAVY_SLOT_HELD` and its validated inheritance contract.
Keep `OXPINYIN_LOCK_DIR`, `XDG_CACHE_HOME`, `OXPINYIN_HEAVY_SLOTS`,
`OXPINYIN_RESOURCE_WAIT_SECONDS`, `CARGO_BUILD_JOBS` and `RUST_TEST_THREADS`.
Keep their meanings and defaults. Preserve explicit child settings.
Choose only unset settings. Share native lock files across native worktrees
on that host. Never select a container lock directory.

Use `fcntl.flock` on Linux and macOS. On Windows, use `LockFileEx` through
`ctypes`, with `LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY`.
All native Windows contenders lock one fixed byte in each slot file.
Use offset 4294967295 and length 1, beyond the bounded JSON holder record.
Use the same range for the admission lock. This avoids locking record bytes.
Microsoft permits locks beyond EOF and denies other processes access to an
exclusively locked range. This gives an explicit exclusive kernel lock.
Release it with the matching `UnlockFileEx` range. Do not truncate lock files
to replace locks. See [LockFileEx](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex).

Implement Windows ancestry and liveness checks with Windows process APIs,
without `/proc` or a POSIX `ps`. Validate the holder record, live process and
ancestor relationship before accepting a nested marker.
Preserve Unix child handoff and cleanup. Windows does not inherit LockFileEx
ownership: the kernel releases the range when its owning process ends.

Use this Windows handoff protocol. The slot holder owns a non-inheritable
Job Object handle with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
Create the workload suspended, assign it to the job, then resume it.
Let no workload process inherit the job handle. Allow no breakaway.
Fail closed if job creation or assignment fails.
Nested work stays in that job and inherits admission context, not lock ownership.
When the controller ends, closing its job handle terminates the workload tree.
Do not admit later heavy work until the previous workload has ended.
If termination cannot be proved, keep admission blocked.
See Microsoft's [job lifetime rules](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects#managing-job-objects).

Keep the Linux MemAvailable and cgroup readers, macOS `vm_stat` reader and
Windows GlobalMemoryStatusEx reader. Take the slot before the final resource
read. Keep estimates, headroom, CPU ceilings and explicit-setting validation.
Serialize reservation updates. Count live native slot reservations.
Proposed integration: subtract Layer 4's unreleased native memory reservations
for containers.
Layer 4 owns their creation, lifetime and release; do not read inner slot state.
Hold no checkout lock while waiting for resources.

Apply the 2026-10-19 fail-closed ruling. If available memory cannot be measured,
treat it as no room. Start no heavy process. Release the acquired slot while
waiting, then retry slot acquisition and admission. At the wait limit, emit
the existing `resource-blocked` FAIL reason when memory was the cause of the
last attempt, or the slot-wait reason (the holder, the time waited and the
limit) when the slot was still held, and a nonzero status either way.
Keep the inherited-slot immediate resource-blocked FAIL. Never wait for memory
while the ancestor holds the slot. Cancellation, expiry or a killed child
cannot pass or skip. Start the workload timeout only after admission.

Layer 2 owns the public heavy recipe. Layer 3 owns nextest's internal limits.
Layer 5 schedules the runner refactor at T5. T2N already extracted this code;
the refactor keeps the adapter import, with separate state in each environment.

### Independent campaign work (#1232)

A tool that launches many independent subprocesses or builds uses a bounded
pool under the shared heavy-slot and memory admission. Choose its worker
count from admitted CPU and memory capacity, not a fixed count. Account for
nested compiler jobs and every live subprocess; multiple pools cannot share
a reservation whose estimate only covers one of them. Give each worker
private writable state, propagate errors, and reap children before releasing
admission. Preserve deterministic output by combining results independently
of completion order.

CPU shares are per pool, or per `run-all.py` invocation: a pool keeps workers
times make jobs within the CPUs it was admitted or measured, and does not
reserve those CPUs against other, independent slot holders, which plan their own
share the same way. Memory is admitted host-wide, counting every holder's
reservation. There is no global CPU scheduler. `tools/dev/pool.py` also names,
labels and records the containers it starts (a per-invocation random token and
`--cidfile`) and removes only those on a failure or signal. Its unit runner
(`run_admitted`) takes callables, in-process steps and launched processes alike,
under the same admission and stop rule: the first failure or a cancellation starts
nothing more, stops the running units, and joins them (each reaps its own children)
before the admission is released, and results and output follow unit order.

Serialize dependent steps and work requiring isolation, and state the reason
in the tool or its runbook. Timing, residency, cache-sensitive observations
and sampling profiles require isolation. Untimed set collection does not.

Audit at `27c785c0` (follow-ups retain their own implementation gates):

| Tool | Classification and disposition |
| --- | --- |
| `cloud/setup.sh` | Common dependency-image/model setup and final environment publication are dependencies. Per-cell builds/data regeneration run through the shared pool, with nested make jobs included in the admitted CPU and memory budget (merged #1261): [#1233](https://github.com/shenghaoc/oxpinyin/issues/1233). |
| `oracle/Dockerfile.oracle` | Backend builds have distinct work/prefix directories. The maintainer retained the serial backend loop: copying host tooling into the image would couple image rebuilds to unrelated tooling changes; the measured saving did not justify that cost (#1263 closed unmerged): [#1234](https://github.com/shenghaoc/oxpinyin/issues/1234). |
| `bisection/teardown.py` | Export must precede recipe selection; mutation within a profile is ordered. Independent profile population and witness checks run as units in one admitted pool, each with its own sample names, counter and CPU lease; records print in profile order, and `--jobs 1` is the serial reference ([#1235](https://github.com/shenghaoc/oxpinyin/issues/1235)). Timing, residency and measurement phases remain isolated and start only after the pool is released. |
| `just profile-w8` (`tools.dev.measure.profile`, formerly `profile/run-w8-cycle.sh`) | Profiler attempts form a fallback chain, each conditional on failure of its predecessor. Sampling and native timing need isolation; no independent campaign loop to parallelize. |
| `measure system-import-profile` (formerly `bisection/run-system-import-profile.sh`) | Side/mode collections use private users and instruction/allocation counters; the serialization was accidental. `system_import_profile.py` now runs the six collections in one admitted pool and reports in a fixed order; `--jobs 1` is the serial reference ([#1236](https://github.com/shenghaoc/oxpinyin/issues/1236)). Attempts within each retry chain remain dependent and sequential. |
| `measure perf-baseline`, `perf-matrix`, `perf-same-data`, `rss-diag`, `callgrind-differential`, `snapshot` | Every sample is a timed, resident-set or hardware-counter observation of one arm, pinned to the caller's one `PERF_CPU`; `bench.round_robin` runs them one process at a time and interleaves the arms so host drift lands on all of them. Serial by measurement isolation. `callgrind-differential` stage 4 (Callgrind is exact) pins its runs to that same CPU, so pooling them would only time-share it. |
| `measure shipped-check`, `tree-recipe-control` (#1290) | The two or four builds are independent (private target and stage directories): units of one admitted pool (`bench.build_all`) that `pool.plan` sizes from the admitted CPUs and memory at a rustc allowance (600 MiB) per Cargo job. Both how many builds overlap and the jobs each gets come from the plan, so a host with many CPUs and little memory builds one at a time with fewer jobs. An explicit `CARGO_BUILD_JOBS` is kept as given and charged in full (rule 8): only the overlap adapts, and a host that cannot hold one build at that count refuses it. The sampling that follows is serial, as above. |
| `measure callgrind-differential` annotations (#1290) | The two `callgrind_annotate` runs (self and inclusive) over each profile are independent post-processing of files on disk: units of one admitted pool (`pool.run_admitted`), each charged the runner allowance (1,024 MiB; the old script ran them one at a time and Valgrind documents no figure). The slot is taken, or inherited, and the memory reserved before any starts, so a host short of memory annotates one at a time and a direct run, such as the image's command, never starts more than it holds. |
| `measure perf-same-data` builds (#1290) | The two builds share one target directory and copy `libpinyin_capi.so` out before the next replaces it: dependent, so serial. |
| `measure snapshot` builds (#1290) | The shipping artifact and the alloc-count fixture use separate build steps. |
| `measure probe-egress` (#1290) | A diagnostic: eight short requests whose table prints in a fixed order. It must not hold the heavy slot or wait behind a build to make them, so it takes no admission. |
| `measure probe-debuginfo-neutrality` builds (#1290) | The three builds share `target/release` and each artifact is copied out before the next replaces it: serial. |

Maintainer decision after this audit: [#1234](https://github.com/shenghaoc/oxpinyin/issues/1234) was closed as not planned. The serial backend build in `Dockerfile.oracle` stays, and no `tools/dev` module is copied into an image build.

## 6. Implementation acceptance

Run setup, bootstrap doctor and interpreter identity checks on all three native
platforms. Check a fresh terminal and unavailable-network/cache cases.
Record untested cases and their reasons. Do not claim execution from this plan.

On native Windows without a POSIX shell, require two concurrent one-slot sleep
commands to serialise. Check nested inheritance, stale markers, live-holder
validation and cancellation. Kill the controller while its workload and a
descendant remain live. Require the Job Object to terminate both.
Start a contending heavy call and prove that its work never overlaps the old
workload. Check no leaked job handle, breakaway or premature slot reuse.
Check low and unmeasurable memory, wait expiry, reservations and explicit
settings.
Use the same semantic cases on Linux and macOS. Layer 2 supplies recipe-level
argument and status acceptance. No cross-OS shared-directory case is required.

## 7. Inspected source patterns

Inspected openai/codex at `1b89beea82ae60e0dbeacb38f1ad2eae9dfed377`
on 2026-10-19 UTC. Its
[Unix lock helper](https://github.com/openai/codex/blob/1b89beea82ae60e0dbeacb38f1ad2eae9dfed377/codex-rs/app-server-daemon/src/backend/pid.rs)
uses an exclusive nonblocking flock. Its
[Windows helper](https://github.com/openai/codex/blob/1b89beea82ae60e0dbeacb38f1ad2eae9dfed377/codex-rs/app-server-daemon/src/backend/windows.rs)
uses an exclusive nonblocking LockFileEx byte range.
Adopt those OS-lock patterns. The distant byte offset above adapts the pattern
to oxpinyin's holder-record file. No source code is copied.

## Shell placement rule (#1260)

Maintainer rule: orchestrate in Python. Keep shell only for one of these
reasons, recorded at the **top of each retained script**, naming its concrete
caller and boundary:

1. **Bootstrap:** runs before the managed CPython exists, to provision it or
   support that provisioning.
2. **Environment:** runs where managed CPython is unavailable, such as a
   reference-image build or a container before provisioning.
3. **External interface:** an outside tool requires a shell entry point, and
   the script is only a thin wrapper.

Everything else belongs in `tools.dev`, an existing harness, or the run-all
runner registry. Internal callers can be updated: a `just` recipe, workflow
`run:` block, Docker `CMD`, sourced helper or existing shell filename does not
by itself require shell. Provision Python before migrated calls. Keep `just`
recipes as one-line delegation in `sh -cu`; this audit does not convert recipes.
No inventoried script needs an external-interface exception solely because of
its current callers. Keep the Bash floor in `tools/lib/bash-floor.sh` rather
than copying guards. Record reasons when each implementation unit touches its
kept scripts; this docs-only audit does not edit them.

The [full per-script audit](tooling-shell-audit.md) covers **100 scripts** at
`a04c91afa343428ba7294e86f217f3e4e286e890`, current `origin/main` when fetched,
on 2026-10-26 UTC (`date -u`). Five whole scripts have concrete keep
reasons: `install-python.sh`, `lib/bash-floor.sh`, `oracle/build-oracle.sh`,
`model/fetch-model.sh` and `tkrzw/distro-probe.sh` (all under `tools/`). The
closing check moves the model fetcher to `tools/dev/model.py` (`just
fetch-model`), since its only shell callers, the two bisection Dockerfiles,
run after managed Python. The pin reader `oracle/pin.sh` was kept at that base
and is removed by the closing check: its one live caller, the arm64 metadata step of `oracle-image.yml`,
runs after managed Python is provisioned and reads the platform-keyed fields
through `just compat pin-field`. The remaining 94 enter six units below. Cloud
setup retains only its pre-interpreter portion; its post-bootstrap work
migrates. The 98 count in #1260 predates this base.

The image `RUN` exceptions are concrete: `Dockerfile.oracle` has system
`python3`, not uv-managed CPython, and calls `build-oracle.sh --libs-only`;
`versions.py` calls the copied distro probe in that image and before Python
provisioning in `Dockerfile.compat`; both bisection image builds run model
fetching without managed CPython. The full `build-oracle.sh` remains usable
inside the dependency image for `--regen-data`. These are not reasons to retain
unrelated host orchestration. The #1234 rejection of parallel image builds
also remains: no `tools.dev` package is copied into the reference image.

### Migration units

Each unit has one shared implementation per concern, with small entry points
and case/recipe data. It does not get one Python file for each shell script.
The audit table assigns every script, including tests and sourced libraries.
New child issues cover shell migration; related existing issues retain their
own scopes and decisions.

| Unit | Scripts | Shared Python layer and caller boundary | Child |
| --- | --- | --- | --- |
| U1 | 43 | Registry/ABI runners: run-all.py and runners.json; shared build, artifact identity, cell/system-data, C-driver execution and comparison services; reuse contract-diff. | [#1289](https://github.com/shenghaoc/oxpinyin/issues/1289) |
| U2 | 15 | Measurement/profile controllers: tools.dev.measure and existing perf-gate/bisection parsers; registry, nightly snapshot, profile-w8 and image CMD callers. | [#1290](https://github.com/shenghaoc/oxpinyin/issues/1290) |
| U3 | 14 | Oracle/compat orchestration: tools.dev.compat and existing identity/boundary/memory, oracle fixture and tkrzw runtime services; host launchers, workflows and aggregate harnesses. | [#1291](https://github.com/shenghaoc/oxpinyin/issues/1291) |
| U4 | 10 | Packaging: one tools.dev packaging layer for version/layout/install/relink/metadata/package assembly; release workflow callers. | [#1292](https://github.com/shenghaoc/oxpinyin/issues/1292) |
| U5 | 9 | Native validation: tools.dev.check, test_runner and native; shared lint/backend/sanitizer services; just check/sanitize, registry, workflow and fixture-test callers (migrated). | [#1293](https://github.com/shenghaoc/oxpinyin/issues/1293) |
| U6 | 3 | Post-bootstrap provisioning: one tools.dev provisioning layer using pool and compat services; setup.sh handoff, environment writer and synthetic-fixture tests. | [#1294](https://github.com/shenghaoc/oxpinyin/issues/1294) |

U1 preserves runner IDs, registry commands/implementation metadata, artifact
and identity JSON, compilation flags, fresh user directories, loader precedence,
complete log comparisons, controls and build/divergence/skip/usage statuses.
Its shared services are `tools.dev.cell`, `system_data`, `drivers`, `runners`
and the recipe modules under `tools/dev/`. `just runner RECIPE ...` retains the
audited runner arguments; `just abi ...` and `just cell-artifacts ...` expose
the ABI gates and cell builder. The registry calls the same services. Independent
case groups use `pool.run_admitted`; per-side comparisons and mutation/control
chains remain ordered within a unit, with private writable state.
The former wrappers (`run-all.sh`, `run-contract-diff.sh`) use the shared CLI
routes, retaining interpreter-injection rejection and exec status semantics. #1175 is the
existing contract-driver split, not a duplicate runner migration. The legacy
`check.sh` in U5 was already bypassed by `just check`; its comparison harness
retired the legacy control without restoring oracle probing to native check.

U2 preserves sample/warmup counts, arm order, CPU choices/refusals, cache and
suspend qualification, retries, policy validation and report schemas. Timing,
RSS/residency and sampling stay isolated. Independent instruction/allocation
collections use the shared admitted pool; a retry follows its own discarded
attempt. Its entry points are `just measure RECIPE ...` (`python -m tools.dev
measure RECIPE`) for `perf-baseline`, `perf-matrix`, `perf-same-data`,
`shipped-check`, `tree-recipe-control`, `rss-diag`, `callgrind-differential`,
`system-import-profile`, `snapshot`, `probe-debuginfo-neutrality` and
`probe-egress`, and `just profile-w8`. The shared services are
`tools/dev/measure/bench.py` (counts, the libtkrzw
check through its adapter, harness builds, pinned launches, heavy builds, the
pool of independent builds, arm scheduling), `identity.py`, `reports.py`,
`neutrality.py` and the recipe modules `campaigns.py`, `valgrind.py`, `probes.py`,
`profile.py` and `snapshot.py`; the report parsers stay the existing
`tools/bisection` and `tools/perf-gate` scripts. A diagnostic keeps the legacy
script name (`run-perf-baseline.sh: PERF_RUNS must be a decimal count ...`)
because reports and runbooks quote it. The two measurement images
(`Dockerfile.callgrind`, `Dockerfile.perf-matrix`) now install uv and the managed
interpreter and run their recipe as `CMD`; the libtkrzw runtime check now lives
in `tools/tkrzw/runtime.py` (U3).

U2's recorded differences from the shell runners change no measured value,
report schema, option or documented status. A missing command is Python's
`FileNotFoundError` text with status 127 where bash said `line N: cmd: command
not found`, and the bash-only diagnostics (`unbound variable`, `${1:?}`) are
reworded. `rss-diag --rounds 010` and `--rounds 08` are decimal, where bash
arithmetic read the first as octal 8 and failed the second, so the old identity
record said 010 beside a run of 8 (or no) rounds; its usage text names the
recipe. `snapshot` validates `PERF_GATE_RSS_PROCESSES` as a decimal count before
building. The `nproc` count in the identity and environment records is the CPU
budget of `host.resource_snapshot`, the affinity set held to the smallest cgroup
CPU quota rounded up, where the shell runners printed GNU `nproc` (coreutils
9.10, `lib/nproc.c`). The two agree where no quota applies, for a quota below one
CPU and for a quota that is whole or has a fraction of .5 or more. They can differ
for a cgroup v2 quota above one CPU with a smaller fraction (1.25 CPUs: `nproc` 1,
here 2), for a cgroup v1 CFS quota (counted here; `nproc` reads cgroup v2 only)
and under `OMP_NUM_THREADS` or `OMP_THREAD_LIMIT` (applied by `nproc`, ignored
here).
Independent builds are pool units: a failing build lets its siblings
finish, the pool names the failed unit and prints its accounting lines in place
of one slot trailer per build, and the build and neutrality scratch files are
removed. The callgrind annotations, which the old script ran one at a time and
without a slot, are pool units too and print the same accounting lines. A signal sent to the controller alone terminates and reaps the children it started
and exits 128 plus the signal, where the shell runner died of it and left them
running; the cancellation messages differ. A measured launch of a harness that
is not there leaves taskset's own line in the arm's `.err` file, with status 1
as before.

U3 preserves image tags and pin hashes, the compat CLI/environment transport,
container state and reservation lifetimes, runtime precedence/refusals, fixture
formats, both-direction profile interchange and aggregate failures. `just compat`
retains the `bash tools/oracle/compat.sh` interface. The other recipes are
`image-tag`, `pin-field`, `verify-image`, `reproducible`, `capture`, `user-round-trip`,
`docker-user-round-trip`, `drop-in`, `differentials` and `nightly-fixtures`.
Shared controllers are `tools/dev/compat/{launcher,images,fixtures,campaigns}.py`,
with the existing identity, boundary and memory services. The host loader reader
and qualifier live only in `tools/tkrzw/runtime.py`; native checks, measurements,
packaging, capture and provisioning reuse them. `runtime.check(cell, runner,
side, object, env=..., cwd=...)` keeps its interface. Provisioning's unchecked
lookup uses the same reader before establishing a healthy host identity.

The compat shell entry point remains an external interface. Its staged guest
bootstrap keeps the adjacent Bash floor and fail-closed guard from #1313, sets
image-owned interpreter paths, then hands off to a separately hashed `guest.py`.
The supplied-image round-trip bootstrap retains only pre-interpreter provisioning.
No `tools.dev` module enters the reference image, and its serial backend loop is
unchanged. Heavy launches use `slot.run_command`; independent reproducibility
builds use the existing admitted pool. Container workloads retain the existing
memory reservation transport, including confirmed-exit fencing before a reader.

U3's deliberate diagnostic wording differences change no status. Bash's
`script: line N: command: command not found`, permission errors, `unbound variable`
and `${n:?}` messages become the command's OS error or a short missing-argument
message, retaining 126/127 for launch errors and 1 for missing required arguments.
A differing phrase-set dump names owned temporary files where Bash named its
`/dev/fd` process substitutions. Legacy usage/help text and runtime refusal text
keep their script names, as U2's diagnostic prefixes do. Scratch paths, PIDs and
UTC run identifiers retain their formats; comparison controls normalize only the
base/worktree and private scratch locations.

U4 preserves the complete staged tree and package identity, one-facade install,
Cargo target/profile and argument forwarding, DESTDIR/prefix/libdir/includedir,
pkg-config/header bytes and ELF version/export/SONAME checks.
For function layout, see the [maintainer ruling](../packaging.md#function-layout).

U4 is implemented by `just packaging COMMAND` (`python -m tools.dev packaging
COMMAND`), `tools/dev/packaging.py` and the package metadata in
`tools/dev/packaging_templates.py`. The ten shell entry points are removed.
Heavy work uses `slot.run_command`; captured processes use the pool.
Stages share their install tree/target, so their dependent facade installs stay
serial. The release CI callers provision managed Python first.

U4's deliberate diagnostic differences change no status. A missing required
package argument prints `error: NAME required` with status 1, replacing Bash's
`script: line N: K: NAME required`. Missing executables retain 127 and
non-executable commands retain 126 with Python's OS error text in place of
Bash's script/line diagnostic. Missing metadata roots retain status 2 and their
`error: no pin/subject root` line without Bash's additional `cd` diagnostic.
A malformed policy in linked-object measurement is attributed to `awk:` in
place of the removed `bash:` launcher. The metadata usage restores the complete
preserved shell header, including "unified diff); 2 on a usage error or a
missing input."; the old sed range stopped at the preceding "Exits" line.
Other usage, resolver diagnostics, package metadata, explicit refusals and
cancellation statuses remain the same.

U5 preserves lint diagnostics and 0/1/2 meanings, backend positive/negative
checks, backend-specific sanitizer flags, toolchain/target selection, allow-skip
behavior, Linux setarch and Darwin refusals/paths. Its entry points are
`python -m tools.dev lint-crate-cfg-placement`, `backend-matrix`,
`backend-probe` and `sanitize [--check] ...`; `just check` and `just sanitize`
call the same services. The shared services are `tools/dev/check.py`,
`tools/dev/test_runner.py` and `tools/dev/native.py`; the tkrzw qualification
uses `tools/tkrzw/runtime.py`. The shell entry points and the
standalone shell fixture are retired; the fixture cases live in
`tools/dev/lint-crate-cfg-placement.test.py`, `tools/dev/backend.test.py` and
`tools/dev/sanitize.test.py` under the existing `just test-tools` discovery.

U5's deliberate differences from the shell entry points change no status. A
missing command is `cmd: command not found` with status 127 where bash said
`script: line N: cmd: command not found`. A command that exists but is not
executable is Python's `PermissionError` text with status 126 where bash said
`script: line N: path: Permission denied`.

U6 keeps bootstrap and
`--python-only` behavior, option exclusions, cache/cell/job/no-pull/in-container
selection, proxy/CA precedence and private-copy cleanup, hash-mismatch status 3,
progress/result streams and order, shell environment-file quoting/selection and
atomic publication. Its `pool_phase` already delegates to `tools.dev.pool`:
[#1233](https://github.com/shenghaoc/oxpinyin/issues/1233) is complete at this
base and is reused, not repeated.

U6's deliberate diagnostic wording differences change no status. An
unsuccessful native host qualification reports `no healthy host libtkrzw was
certified` in place of `probe_host exited 1`, which described the retired shell
function as a child process. Every empty host-probe result also names its cause
on stderr, including a missing `tkrzw_dbm_util`; host-probe refusal retains
status 1.

U6's deliberate prerequisite-check ordering difference moves the Cargo,
toolchain and Valgrind checks after uv and managed-interpreter bootstrap because
those checks now run in Python. They still precede oracle downloads. An
unwritable uv destination still fails before any download, and `--python-only`
still omits the Cargo/toolchain and Valgrind checks.

### Shared machinery and active lanes

| Concern | Single owner and migration disposition |
| --- | --- |
| Admission, reservations, CPU/job budgets and bounded fan-out | `tools/dev/slot.py` and `tools/dev/pool.py`; migrate shell glue into calls to these services. #1232's rule remains; do not reopen completed #1233. |
| mmap/munmap/mincore and page residency | `tools/dev/mman.py`, landed in [#1286](https://github.com/shenghaoc/oxpinyin/pull/1286). Reuse it; move teardown's remaining separate binding after its owning lane lands. |
| Mount-type parsing and disk-backed TMPDIR preflight | `tools/dev/mounts.py`, landed in [#1283](https://github.com/shenghaoc/oxpinyin/pull/1283), implementing [#1281](https://github.com/shenghaoc/oxpinyin/issues/1281); do not duplicate either reader or preflight. |
| Bash version checks | `tools/lib/bash-floor.sh`; retained Bash entry points source the one declaration, and Python doctor reads it. |
| Container identity, state, lifetime reservation and runtime selection | Existing `tools/dev/compat/` services, extended by U3/U6, with separate native and container state. |
| Build/driver launch, temporary profiles and failure cleanup | Shared runner services in U1, reused by the other units; registry keeps cases/recipes, not separate controllers. |

Lane status at the rebased audit base: [#1284](https://github.com/shenghaoc/oxpinyin/pull/1284)
extends pool.py with admitted callable units and moves teardown fixture/witness
fan-out onto it. Its child [#1287](https://github.com/shenghaoc/oxpinyin/pull/1287)
extracts system-import profile collection from embedded Python to
`tools/bisection/system_import_profile.py`. That reduces U2's remaining work to
its shell build/layout/argument orchestration; it does not make the whole
script a justified shell exception. #1284 and #1287 remain open. #1286 is merged
and also changes `tools/dev/measure/`; #1283 is merged and changes
`test_runner.py`, `teardown.py` and the shared mount reader. The latter two
landed during this audit; no shell script changed between the initial source
snapshot and this rebased base. None of these PRs is changed by this audit.
Use the landed helpers and sequence the remaining migration after the
relevant open shared-layer PR lands; recheck status then. #1284, #1286 and #1287
have since merged, and U2 (#1290) builds on them.

Existing [#1175](https://github.com/shenghaoc/oxpinyin/issues/1175),
[#1232](https://github.com/shenghaoc/oxpinyin/issues/1232),
[#1259](https://github.com/shenghaoc/oxpinyin/issues/1259) and #1281 cover,
respectively, contract-driver internals, bounded parallelism, PowerShell 7
transport and TMPDIR preflight. Link them; do not recreate them as shell
migration tasks. Native macOS capabilities remain under #1171. Closed
[#1234](https://github.com/shenghaoc/oxpinyin/issues/1234) is not planned;
its reference-image serial build decision stands.

### Migration acceptance

Run old and new entry points on the same inputs, environment and artifacts;
show unchanged behavior, output and exit statuses, including invalid inputs,
child failures and cancellation/cleanup. Preserve per-script interfaces in the
audit. Variable timing/identity fields keep their meaning and format; do not
claim bit-identical timing samples. Update all callers, registry metadata and
documented commands. Remove migrated scripts or reduce them to an entry point
with a concrete allowed reason recorded at its top. Use existing harnesses and
case data. Require `just test-tools` (disk-backed TMPDIR where needed),
`just fmt --all --check` and tracked-shell `shellcheck --severity=warning` at
the repository's declared floor. Follow the current verification-cell policy
for functional checks affected by an implementation. This plan reports no
migration execution and authorizes no new dependency or frozen-input change.
