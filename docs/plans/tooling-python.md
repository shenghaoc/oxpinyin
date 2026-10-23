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
separate. This reconciliation changes no code or AGENTS.md rule.
Each remaining implementation PR needs its own approval.

## 2. Interpreter contract

Use uv-managed CPython of the 3.15 series only: 3.15.0 or newer, below 3.16
(maintainer ruling 2026-10-23 UTC, #1074). Use the standard library only.
Require uv 0.13.1 or newer (maintainer ruling 2026-10-22 UTC). This floor
replaces the earlier exact uv 0.13.1 pin. CI installs exactly uv 0.13.1 from
the checksummed release. Hosts may use any later uv.
Use the GIL-enabled CPython request `cpython@3.15+gil`.
uv 0.13.1 documents the implementation/version and `+gil` request forms in
its [versioned Python guide](https://github.com/astral-sh/uv/blob/0.13.1/docs/concepts/python-versions.md).
Support no older version, system-Python fallback or transition implementation.
A later ruling must approve a Python package dependency.

PR #939 added these root files:

| Implemented file | Required content |
| --- | --- |
| `.python-version` | exactly `cpython@3.15+gil` |
| `uv.toml` | `python-preference = "only-managed"` and `required-version = ">=0.13.1"` on separate lines |

Keep the interpreter series and the uv floor. Do not vendor Python. Do not add `pyproject.toml`,
a virtual environment or `uv.lock` for interpreter provisioning alone.
Every repository-owned uv call passes the exact interpreter request explicitly.
Use `uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal ...`
for execution. An installed system Python 3.15 does not satisfy this rule.
Reject another implementation or series. A later 3.15 patch release is
accepted without a change; 3.16 needs a separate verified change, because the
tooling relies on 3.15's `-X lazy_imports` modes.

On Linux and macOS, install uv through its documented host route.
On Windows, use a native uv executable from PowerShell or the native installer.
Neither route needs an older Python first.
Then run `uv python install cpython@3.15`, only when
`uv python find --managed-python cpython@3.15+gil` finds no interpreter.
uv 0.13.1 has no separate `+gil` download and rejects `cpython@3.15+gil` for
installation. Installation uses the plain 3.15 distribution. Execution keeps
`cpython@3.15+gil`.
Check `Py_GIL_DISABLED == 0` and the enabled runtime GIL. Check CPython
3.15.0 or newer, below 3.16, and the resolved executable against
`uv python find --managed-python cpython@3.15+gil`.
These checks enforce the managed GIL build after installation.
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
`uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev doctor`.
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
`sys.get_lazy_imports` is missing, it names `cpython@3.15+gil` and exits
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
the existing `resource-blocked` FAIL reason and a nonzero status.
Keep the inherited-slot immediate resource-blocked FAIL. Never wait for memory
while the ancestor holds the slot. Cancellation, expiry or a killed child
cannot pass or skip. Start the workload timeout only after admission.

Layer 2 owns the public heavy recipe. Layer 3 owns nextest's internal limits.
Layer 5 schedules the runner refactor at T5. T2N already extracted this code;
the refactor keeps the adapter import, with separate state in each environment.

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
