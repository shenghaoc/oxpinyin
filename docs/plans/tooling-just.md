# Tooling layer 2: native just interface

Status: partial implementation. Reconciled: 2026-10-23 UTC (`date -u`).
This layer builds on [native Python](tooling-python.md).
PR [#942](https://github.com/shenghaoc/oxpinyin/pull/942) added the native
`justfile`, doctor, check and heavy recipes. PR
[#943](https://github.com/shenghaoc/oxpinyin/pull/943) added test and test-doc.
PR [#948](https://github.com/shenghaoc/oxpinyin/pull/948) (merge `219f4205`)
added build, fmt, clippy, list and plan. PR
[#1038](https://github.com/shenghaoc/oxpinyin/pull/1038) added bench and
bench-smoke (maintainer ruling 2026-10-22 UTC).
The recipe table distinguishes these from the remaining surface.

macOS: bounded checks passed on 2026-10-20 UTC at
`b034e8342e70aab874b32414976cc9b6d232dc1d`.
`just doctor` and the managed-Python doctor had identical output and status.
For BDB, Kyoto Cabinet and tkrzw, the old shell check under `just heavy`
and `just check <backend>` both exited 0. The BDB row used
Homebrew `berkeley-db@5` 5.3.28 with its include and library directories.
The rows passed 51 tests with four ignored, 43 tests and 49 tests,
respectively. Their stdout matched after elapsed test times and two
intentional old oracle-directory display lines were removed. Those exact
lines were:

```text
PINYIN_MODEL_DIR: not selected (optional for setup)
PINYIN_ORACLE_PREFIX: not selected (optional for setup)
```

The removal follows §2's native boundary: the earlier optional
oracle-directory display is superseded. Every other stdout line matched.
An unconfigured `just check bdb` attempt returned 101. Its diagnostic began:

```text
libdb required: the `bdb` feature links the SYSTEM Berkeley DB and needs its header db.h, which was found nowhere on the include path.
```

The configured BDB result supersedes that attempt.

Windows: TODO (no host); tooling verification is deferred. It does not block
T2N (maintainer ruling 2026-10-21 UTC).
Shell and argument acceptance remains partial. Hosted Rust CI does not verify
these recipes. Historical macOS checks in #942/#943 retain their original
heads and failures; they do not verify this head.

## 1. Interface boundary

The root `justfile` is the native command interface on Linux, macOS and
Windows. Native recipes use the host toolchain and native state.
They require no oracle or pin installation. They never start a container.
Reserve the `container` prefix for Layer 4. This layer defines no behaviour
for that prefix.

Keep recipes thin. Each recipe calls the `tools.dev` CLI or one tool command.
Use Layer 1's managed-Python invocation for Python commands.
Put selection, process, path, environment and orchestration logic in Python.
Do not add recipe loops, conditions, pipes, redirects, shell assignments,
`&&` or `||`. A retained native tool may remain a direct command during migration.
Use one CLI and one registry; do not copy runner metadata into recipes.

## 2. Native recipe surface

Tooling stages use the T prefix. Layer 5 defines these delivery milestones.
They differ from the repository roadmap stages.

PR #942 fixes the minimum `just` version at 1.59.0 and records it in setup.
CI image provisioning remains proposed. Complete §3 acceptance remains open.
The recipe names below define interfaces, not a second test catalog.
Layer 5 schedules the old-to-new handoffs.

| Native recipe | Purpose | Existing entry point or replacement | Status |
| --- | --- | --- | --- |
| `just --list` | List tasks; bare `just` runs doctor. | README/runbook discovery | Implemented, #942. |
| `just doctor` | Call Layer 1's native doctor. | Bootstrap doctor | Implemented, #942. |
| `just list [--issue X] [--kind K]` | Read registry rows and implementation state; execute nothing. | `run-all.py --list` | Partial: #941 supplies `tools.dev list [--heavy]`; #948 implemented the recipe, issue/kind filters and selection tests. [#1033](https://github.com/shenghaoc/oxpinyin/pull/1033) (maintainer ruling 2026-10-22 UTC) implemented the `implementation` field: `list` prints it in place of the reason, and `plan` prints it with each row. |
| `just plan [selection ...]` | Print the native plan without execution. | Native command planning | Partial: #941 supplies read-only `tools.dev plan --runners/--cells`; #948 implemented the recipe and issue/kind selection. Full command/input identities and digests (stage T3) remain proposed. |
| `just check [backend]` | Run native setup checks; default bdb. | `tools/dev/check.sh` | Implemented, #942. The shell remains an old/new acceptance input. |
| `just heavy -- <command>` | Use Layer 1's slot and admission. | Merged #905/#906/#903 `run-all.py --heavy-slot --` | Implemented, #942; shared core from #941. |
| `just test [args ...]` | Enter Layer 3's native Rust test path. | Native `cargo test` orchestration | Implemented, #943; the `ci.yml` Linux and macOS test cutover is #968 (merge `c017ebee`). Windows and other workflows remain proposed. |
| `just test-doc [args ...]` | Run doctests through the heavy slot. | `cargo test --doc` | Implemented, #943. |
| `just build [args ...]` | Run a native build through the heavy wrapper. | `cargo build` | Implemented, #948. |
| `just fmt [args ...]` | Run formatting. | `cargo fmt` | Implemented, #948. |
| `just clippy [args ...]` | Run lints through the heavy wrapper. | `cargo clippy` | Implemented, #948. |
| `just bench [args ...]` | Run the workspace benches through the heavy wrapper: `cargo bench --workspace --benches <args>`. | `cargo bench` | Implemented, #1038 (maintainer ruling 2026-10-22 UTC). `--benches` skips the four `required-features` targets: `backend_matrix_kyotocabinet`, `backend_matrix_tkrzw` and `kyotocabinet_bnum` (oxpinyin-store), and `dbm_bench` (pinyin-oracle). |
| `just bench-smoke` | Build every selected bench harness without running it: `just bench --no-run`. | `cargo bench --no-run` | Implemented, #1038 (maintainer ruling 2026-10-22 UTC). On a host with model20, `just bench -- --test` is the full start-up check. |
| `just measure <recipe>` / `just initbench` | Enter native oxpinyin-only measurement; `initbench` is `measure startup`. Pin rows are refused. | Native startup controller and worker (layer 5 §7.8) | Proposed for T2N (maintainer ruling 2026-10-22 UTC). |
| `just fuzz <target> [args ...]` | Run one native fuzz target through the heavy wrapper. | `setarch "$(uname -m)" -R cargo +nightly-2026-08-01 fuzz run <args>` from `verify-nightly.yml`'s `fuzz-soak` job. | Implemented (maintainer ruling 2026-10-23 UTC). Linux execution; macOS exits 1 because the fuzz workspace links the Linux-only C ABI. Windows transport unverified (#950). |
| `just coverage [args ...]` | Run native coverage through the heavy wrapper; the tool owns crate, backend and report selection. | `tools.dev coverage` runs `cargo llvm-cov <args>` from `coverage.yml`'s `coverage` job with temporary per-invocation state. | Implemented (maintainer ruling 2026-10-23 UTC). Run and report in one invocation; pass the desired workflow scope, report and floor options together. macOS execution unverified; Windows transport unverified (#950). |
| `just sanitize [bdb\|kc\|tkrzw\|all]` | Run one store backend's sanitizer gate, or all three, through the heavy wrapper: `tools/sanitize.sh <backend>`, default `bdb`. | `tools/bdb/run-sanitizers.sh`, `tools/kc/run-sanitizers.sh`, and `tools/tkrzw/run-sanitizers.sh` (moved from `store-backends.yml`'s inline `tkrzw-sanitizers` command). | Implemented (maintainer approval 2026-10-23 UTC, #1085). Linux execution; macOS and Windows exit 1. The workflows do not call the dispatcher yet. |
| `just ci <job>` | Select an approved native job. | Workflow-specific commands | Proposed. |

`just check` keeps `check.sh`'s backend selection and selected crate sets.
Keep the platform-specific native dependency checks and exit results.
The #942 check uses `slot.run_command` for its cargo steps. It still uses
`cargo test`. Layer 3 may change that executor
only after its equivalence check passes. Native checks must not probe an
oracle installation or pass oracle variables to tests. The earlier optional
oracle-directory display is superseded by this native boundary.
Report unsupported native capabilities explicitly. Do not dispatch elsewhere.

`just heavy` has one command line that calls the slot CLI.
It holds no slot or admission logic itself. Pass the command vector unchanged.
The Python code owns process creation, inheritance and status propagation.
Keep status 0, 1 and 77 unchanged. Do not append `|| true` or turn a failure
into a successful skip. A requested missing input is a failed verification.

Print each native command's resolved inputs, selected crate/backend, native
target path and relevant environment overrides before execution.
Layer 5 owns registry selection by issue, row, runner, cell and family.
Native recipes do not acquire pin or cell semantics from that vocabulary.

The fuzz and coverage wrappers forward raw arguments, including `--`, without
adding target, crate, backend or report selection. Their `not-a-gate` registry
rows name `legacy:justfile` and call that exact justfile; compatibility
aggregate gates do not execute them. Fuzz uses the pinned nightly and Linux
ASLR wrapper from [verify-nightly.yml](../../.github/workflows/verify-nightly.yml),
job `fuzz-soak`, step `Soak each target`. The targets and Linux-only C ABI
dependency are in [fuzz/Cargo.toml](../../fuzz/Cargo.toml).
Coverage delegates to the existing tool from
[coverage.yml](../../.github/workflows/coverage.yml), job `coverage`:

```sh
just fuzz parser -- -max_total_time=5 -timeout=2 -max_len=4096 -print_final_stats=1
just coverage --locked -p oxpinyin-chewing -- --test-threads=1
```

Coverage state belongs to one invocation. `tools.dev.coverage.run` allocates a
private target path beneath a fresh temporary parent for both
`CARGO_LLVM_COV_TARGET_DIR` and `CARGO_LLVM_COV_BUILD_DIR`, preserving raw
arguments and the child's status. Cargo creates its own tagged target directory.
It uses the existing `slot.run_command` for process-group cleanup and inherited
slot handling, and removes the directory after the child is reaped, including
on failure or cancellation. Caller coverage directory overrides do not make
these invocations share state. No checkout lock is added: `slot.run_command`
uses `HeavyAdmission` and starts the child directly; it has no checkout lock.

Run tests and produce their report in the **same invocation**. Profiles and
instrumented artifacts are not retained for a later `just coverage report`,
`--no-report` / `report`, or `--no-clean` sequence. A report output requested
with `--output-path` is owned by the caller. Combine the workflow's scope,
LCOV output and line floor in one call:

```sh
just coverage --locked --workspace --no-default-features --features kyotocabinet --lcov --output-path lcov.info --fail-under-lines 77
```

Linux recipe acceptance is complete for **fuzz and coverage only**. At
`220ba5142c9249d14b9a919a03626cca6661ade4` on Ubuntu Linux amd64, after
installing the workflow-pinned tools, these exact smoke commands passed:

| Historical command | Result |
| --- | --- |
| `just fuzz parser -- -max_total_time=5 -timeout=2 -max_len=4096 -print_final_stats=1` | Exit 0; 20,233 executions in 6 seconds, no finding. |
| `just coverage --locked -p oxpinyin-chewing -- --test-threads=1` | Exit 0; 6 unit and 4 integration tests passed; selected-crate line coverage 91.16% (249 lines, 22 missed). |

Those runs used nightly-2026-08-01 with rust-src (rustc
`1.99.0-nightly (ad3d0bc14 2026-07-31)`), cargo-fuzz 0.13.2,
cargo-llvm-cov 0.9.0, and llvm-tools-preview for Rust 1.97.1
(LLVM `22.1.6-rust-1.97.1-stable`). This is historical evidence for that exact
revision; it does not relabel the smoke as a run of later state-isolation
changes. The earlier pre-provisioning attempts ran no workload and are
superseded for Linux acceptance. Argument transport (empty arguments, spaced
paths and `--`) and statuses 0, 1 and 77 also passed with a temporary Cargo
substitute through the real heavy slot. These bounded results close neither
other recipes' acceptance nor T2N's fresh-install/offline checks.

The macOS fuzz refusal body returned 1 when exercised on Linux. Native macOS
fuzz/coverage recipe execution was not run; fuzz remains unsupported there
because the workspace links the Linux-only C ABI. Windows transport and runtime
remain unverified (#950).

These recipes do not change either workflow.

`just sanitize` selects only. `tools/sanitize.sh <bdb|kc|tkrzw|all>` runs the
backend's own script, which keeps its instrumentation, flags and pass
criteria; the three share the nightly that the `NIGHTLY` line in
`tools/sanitize.sh` names. `all` runs the three in turn and exits 1 if any
gate failed. The
tkrzw script runs `tkrzw-sanitizers`' inline command with its flags, sanitizer
options and ASLR wrapper unchanged, and adds one precondition, checked by
`tools/tkrzw/runtime.sh`: every test binary must load the verified libtkrzw,
the one `tools/cloud/setup.sh`'s environment file names or a host library
that passes `tools/tkrzw/distro-probe.sh`. Ubuntu's packaged libtkrzw fails
that probe. The workflows
are unchanged: `bdb-sanitizers` still calls the bdb script, `tkrzw-sanitizers`
still runs its inline command, and no job runs the kc gate. Moving them onto
the dispatcher is a workflow change and needs its own approval (#1085).

## 3. Cross-platform command transport

Configure a native Windows shell with `[windows]` on `set shell`.
Do not require Git Bash, Cygwin, WSL or another POSIX shell.
Do not use deprecated `set windows-shell` or `set windows-powershell`.
PR #942 added `powershell.exe -NoLogo -NoProfile -Command` and variadic
recipes that use `@args` with `exit $LASTEXITCODE`. Windows variadic transport
is partial and unverified. With `-Command`, trailing tokens may be parsed as
command text rather than preserved as separate arguments. A recipe fix needs
a Windows host and a separate implementation PR. Native Windows acceptance
remains TODO. Do not infer acceptance from the syntax.
See the [just shell manual](https://just.systems/man/en/configuring-the-shell.html).

Preserve argument boundaries and paths with spaces. Do not use POSIX `quote()`
as Windows escaping. A PowerShell `-Command` path must preserve the child's
actual exit code; a generic status 1 is insufficient. Keep shell quoting and
transport separate from Python selection logic. Use platform-specific recipe
attributes only where the host command transport differs.

Native Windows acceptance needs a real Python-backed recipe with no POSIX
shell on PATH. Pass multiple arguments, an empty argument, and a spaced path.
Check statuses 0, 1 and 77 exactly. Listing tasks or running doctor alone does
not satisfy acceptance. Run the same contract on native Linux and macOS.
Compare verdicts, case counts, digests, input identities and exit codes.
Only OS path separators and line endings may differ where the format permits.
Do not weaken byte-exact fixtures or wire formats. Record untested cases.

## 4. Implemented wrapper rule

The #905/#906/#903 stack is merged. `run-all.py --heavy-slot --` remains
available through #941's adapter to the shared core. PR #942 changed
AGENTS.md to use `just heavy --` for direct heavy commands and runner builds.
Layer 1 limits the native slot to its environment. The recipe migrations above
that remain open are measure/initbench and ci;
the `ci.yml` Linux and macOS test cutover landed in #968 (merge `c017ebee`).
This reconciliation changes no production rule.

## 5. Inspected source pattern

Inspected openai/codex at `1b89beea82ae60e0dbeacb38f1ad2eae9dfed377`
on 2026-10-19 UTC. Its
[justfile](https://github.com/openai/codex/blob/1b89beea82ae60e0dbeacb38f1ad2eae9dfed377/justfile)
has thin native test recipes that delegate to nextest.
Its recipes transport arguments separately for Unix and Windows.
Adopt delegation and explicit argument handling. Do not copy its shell
settings, platform syntax or workflow scope. Our managed interpreter and
native Windows acceptance remain the authority.

Later CI changes must use these same recipes. Each workflow edit still needs
separate approval and side-by-side comparison under Layer 5's migration gate.
