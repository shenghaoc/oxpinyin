# Tooling layer 2: native just interface

Status: partial implementation. Reconciled: 2026-10-20 UTC (`date -u`).
This layer builds on [native Python](tooling-python.md).
PR [#942](https://github.com/shenghaoc/oxpinyin/pull/942) added the native
`justfile`, doctor, check and heavy recipes. PR
[#943](https://github.com/shenghaoc/oxpinyin/pull/943) added test and test-doc.
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

Windows: TODO (no host); tooling verification is deferred.
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
| `just list [--issue X] [--kind K]` | Read registry rows and implementation state; execute nothing. | `run-all.py --list` | Partial: #941 supplies `tools.dev list [--heavy]`; this recipe, issue/kind filters and implementation state are proposed. |
| `just plan [selection ...]` | Print the native plan without execution. | Native command planning | Partial: #941 supplies read-only `tools.dev plan --runners/--cells`; the recipe and full command/input identities are proposed. |
| `just check [backend]` | Run native setup checks; default bdb. | `tools/dev/check.sh` | Implemented, #942. The shell remains an old/new acceptance input. |
| `just heavy -- <command>` | Use Layer 1's slot and admission. | Merged #905/#906/#903 `run-all.py --heavy-slot --` | Implemented, #942; shared core from #941. |
| `just test [args ...]` | Enter Layer 3's native Rust test path. | Native `cargo test` orchestration | Implemented, #943; CI cutover is proposed. |
| `just test-doc [args ...]` | Run doctests through the heavy slot. | `cargo test --doc` | Implemented, #943. |
| `just build [args ...]` | Run a native build through the heavy wrapper. | `cargo build` | Proposed. |
| `just fmt [args ...]` | Run formatting. | `cargo fmt` | Proposed. |
| `just clippy [args ...]` | Run lints through the heavy wrapper. | `cargo clippy` | Proposed. |
| `just bench <recipe>` | Enter a native benchmark without pin inputs. | Native Criterion/perf entry points | Proposed. |
| `just measure <recipe>` / `just initbench` | Enter optional native measurement. | Native oxpinyin-only measurement | Proposed. |
| `just fuzz` / `just coverage` / `just sanitize` | Call native tools for native inputs. | Existing fuzz/coverage/sanitizer commands | Proposed. |
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
Layer 1 limits the native slot to its environment. The remaining recipe and
CI migrations above are proposed. This reconciliation changes no production rule.

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
