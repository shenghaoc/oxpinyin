# Tooling layer 3: native nextest

Status: proposed plan. Docs only. Date: 2026-10-19 UTC (`date -u`).
This layer builds on the [native just interface](tooling-just.md).
It adds no nextest installation, configuration, code or workflow change.

## 1. Test entry point

Use cargo-nextest as the native Rust test orchestrator.
`just test` runs `cargo nextest run` under the shared heavy slot.
Keep the recipe thin. The `tools.dev` test command selects the native profile
and calls `slot.run_command` directly. The process path is:

```text
just test -> tools.dev test -> slot.run_command -> cargo nextest run --profile local <args>
```

Use `--no-fail-fast` for complete selected-run feedback.
Keep the selected crates, features and cases unchanged. Native tests use the
host toolchain. They load no oracle or pin installation and start no container.
Pin-dependent Rust tests belong to Layer 4's separate environment.

Cargo-nextest and `just` are approved host-tool choices in this plan.
Require cargo-nextest 0.9.115 or later for the `inherits` key.
See the [profile inheritance documentation](https://nexte.st/docs/configuration/#profile-inheritance).
Set `nextest-version = "0.9.115"` in `.config/nextest.toml`.
Keep `--run-ignored=only` for ignored-only selections.
The implementation records compatible versions and provisioning commands.
Do not install them in this docs-only stack. Keep `rust-toolchain.toml`
as the Rust toolchain authority.

## 2. Planned configuration

The implementation adds `.config/nextest.toml`.
Define native `local` and `ci` profiles explicitly. Let both inherit a shared
base where their behaviour agrees. Local limits serve developer machines;
CI limits serve the existing native CI scopes. Do not copy CI concurrency
to every local host. Record each chosen limit and its resource reason.
Keep retries at zero for the equivalence run, so a retry cannot hide a failure.
Do not narrow test discovery through an unreviewed default filter.

Use named test groups with `max-threads` for tests that interfere or use much
memory. Plan a `store-backend` group with an initial ceiling of 1 for selected
storage integration cases. Use an exact package/binary/test filter after
reading those cases. Keep independent unit tests outside the group.
Name other shared resources before adding groups. An implementation must
list each matched test and justify every excluded case.

Use `threads-required = "num-test-threads"` for a test that must run alone
within that nextest invocation. Use a smaller weight only for a measured
resource estimate. A group's ceiling limits its subset; `threads-required`
uses the global test budget. Check both local and CI overrides.
See nextest's [groups](https://nexte.st/docs/configuration/test-groups/)
and [heavy-test rules](https://nexte.st/docs/configuration/threads-required/).

## 3. Responsibility boundaries

Nextest limits concurrency inside one test invocation.
[Layer 1's slot](tooling-python.md#5-native-heavy-slot-and-memory-admission)
limits heavy work across native processes. Groups do not replace file locks
for resources that separate processes share. Use the OS locks from Layer 1.
Keep resource locks even when a group has `max-threads = 1`.

Account for nextest's `-j` / `NEXTEST_TEST_THREADS` in memory admission.
Keep explicit settings. If the required budget does not fit, block or fail
under the slot contract; do not silently reduce a caller's value.
Keep cargo build concurrency separate from nextest test concurrency.
Do not assume `RUST_TEST_THREADS` alone controls nextest's process pool.

## 4. Doctests and ignored tests

Keep doctests on `cargo test --doc`, through `just test-doc` and `just heavy`.
Nextest does not replace that separate step. See its
[running guide](https://nexte.st/docs/running/).

Keep `#[ignore = "needs …"]` tests and their existing reasons.
Map `--include-ignored` to nextest `--run-ignored=all`.
Map ignored-only selections to `--run-ignored=only`.
When selected, a missing required input must fail; it must never self-skip
and pass. Preserve [the testing policy](../testing/README.md).
Pin-dependent ignored tests stay out of native execution and run in Layer 4.
Keep ordinary ignored tests visible in native discovery. Ignored status does
not establish completed verification. Do not convert nextest exclusions into
successful gate rows or alter the gate's status-77 contract.

## 5. Equivalence and native CI scopes

Before cutover, compare cargo test and nextest on the same host, source,
toolchain, inputs, crate selection and feature set. List discovered test IDs
on both sides. Account for doctests separately. Require the same tests to
pass and fail, including deliberate failures and selected missing-input cases.
Keep ignore reasons and optional-input behaviour. Treat a timeout, crash or
empty unintended selection as a failure, not a passing skip.

Cover every crate/feature set that current CI runs. At oxpinyin `7c80bdca`:

| Existing native job/scope | Required nextest comparison |
| --- | --- |
| `ci.yml` test | Default-feature native workspace tests; keep non-Rust gates separate. |
| `ci.yml` test-macos | Workspace with `--no-default-features --features tkrzw` and the same six exclusions. |
| `ci.yml` test-windows | Default BDB workspace and the same six exclusions; preserve its current dependency setup. |
| `store-backends.yml` store-features | Existing bdb, kyotocabinet and tkrzw crate/feature selections. |
| `verify-nightly.yml` overflow-lane | Kyotocabinet workspace with `--locked --workspace --release --no-default-features --features kyotocabinet`. Set `RUSTFLAGS=-C overflow-checks=y -C debug-assertions=y -D warnings`. Preserve both profile and flags. |
| Native sanitizer test selections | Compare only supported libtest targets; keep specialized sanitizer tools where nextest cannot replace them. |

The six macOS/Windows exclusions are `oxpinyin-capi`, `oxpinyin-zhuyin-capi`,
`oxpinyin-corpus`, `oxpinyin-kmm`, `oxpinyin-punct` and `oxpinyin-word`.
Keep the Windows CI dependency/version exception confined to that CI scope.
Do not claim a validated local storage setup from CI alone.
The local completion gate stays limited to touched crates under AGENTS.md.
The workspace sweep remains CI's job. Each workflow edit needs separate
approval; this plan changes neither current policy nor workflow files.

After equivalence passes, the cutover implementation updates AGENTS.md rule 5.
Replace its `cargo test -p <crate>` completion gate with the native `just test`
nextest route. Keep the same touched-crate scope and feature selection.
Keep doctests on `cargo test --doc` through `just test-doc` and `just heavy`.
Keep fmt and clippy obligations. Do not add a local workspace sweep.
This is a planned policy edit; this docs-only PR changes no AGENTS.md rule.

## 6. Inspected source pattern

Inspected openai/codex at `1b89beea82ae60e0dbeacb38f1ad2eae9dfed377`
on 2026-10-19 UTC. Read its
[justfile](https://github.com/openai/codex/blob/1b89beea82ae60e0dbeacb38f1ad2eae9dfed377/justfile)
and [nextest config](https://github.com/openai/codex/blob/1b89beea82ae60e0dbeacb38f1ad2eae9dfed377/codex-rs/.config/nextest.toml).
Its routine test recipe selects nextest's local profile.
Its config gives integration groups distinct ceilings and uses global weights
for cases that must run alone. It keeps cross-process locks as a separate
guard. Adopt those patterns, not its test names, retries or numeric ceilings.
Codex uses `default` for CI and a `local` override. Our named `ci` profile
is an explicit adaptation, not a claim about its source.
