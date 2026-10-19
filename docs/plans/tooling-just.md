# Tooling layer 2: native just interface

Status: proposed plan. Docs only. Date: 2026-10-19 UTC (`date -u`).
This layer builds on [native Python](tooling-python.md).
It adds no `justfile`, configuration or implementation.

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

Tooling stage T2 fixes the minimum `just` version.
Record it in the runbook and approved CI image.
The implementation must support the acceptance below.
The recipe names below define interfaces, not a second test catalog.
Layer 5 schedules the old-to-new handoffs.

| Native recipe | Purpose | Existing entry point or replacement |
| --- | --- | --- |
| `just --list` | List tasks; bare `just` runs the default recipe. | README/runbook discovery |
| `just doctor` | Call Layer 1's native doctor. | Ad-hoc host probes and bootstrap doctor |
| `just list [--issue X] [--kind K]` | Read static registry rows and implementation state; execute nothing. | Catalog inspection through `run-all.py --list` |
| `just plan [selection ...]` | Print the native check plan without executing it. | Ad-hoc native command planning |
| `just check [backend]` | Run the selected native setup checks. | `tools/dev/check.sh` |
| `just heavy -- <command>` | Run a command inside Layer 1's native slot and memory admission. | The #905/#906/#903 stack's `run-all.py --heavy-slot --` interface; that stack is not on inspected main |
| `just test [args ...]` | Enter the native Rust test path. Layer 3 owns execution. | Direct native `cargo test` orchestration |
| `just test-doc [args ...]` | Run doctests through the heavy wrapper. | `cargo test --doc` |
| `just build [args ...]` | Run a native build through the heavy wrapper. | Direct `cargo build` |
| `just fmt [args ...]` | Run formatting. | Direct `cargo fmt` |
| `just clippy [args ...]` | Run lints through the heavy wrapper. | Direct `cargo clippy` |
| `just bench <recipe>` | Enter a native benchmark without pin inputs. | Native Criterion/perf entry points |
| `just measure <recipe>` / `just initbench` | Enter optional native measurement. Layer 5 owns its limits. | Native oxpinyin-only measurement entry points |
| `just fuzz` / `just coverage` / `just sanitize` | Call existing native tools for native inputs. | cargo-fuzz, cargo-llvm-cov and native sanitizer entry points |
| `just ci <job>` | Select an approved native job. | Native workflow-specific commands |

`just check` keeps `check.sh`'s backend selection and selected crate sets.
Keep the platform-specific native dependency checks and exit results.
Use `just heavy` for its cargo steps. Layer 3 may change the test executor
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
Choose the concrete shell in the implementation PR after the acceptance case.
`cmd.exe` remains a candidate, not an assumed solution.
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

## 4. Planned wrapper rule

`run-all.py --heavy-slot` belongs to the unmerged #905/#906/#903 stack.
It is not an interface on inspected main at `7c80bdca`.
The transition rule below applies to that stack's wrapper once it lands.
It does not claim that the wrapper exists on main now.

Tooling stage T2's implementation updates the AGENTS.md wrapper text:
“Once tooling stage T2 lands, run direct cargo build, test and clippy commands
through `just heavy`. Until then, use `run-all.py --heavy-slot --` on Linux
and macOS only.” Layer 1 limits this native slot to its environment.
This docs-only layer does not change the current production rule.

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
