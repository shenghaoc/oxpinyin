# Tooling layer 3: native nextest

Status: partial implementation. Reconciled: 2026-10-21 UTC (`date -u`).
This layer builds on the [native just interface](tooling-just.md).
PR [#943](https://github.com/shenghaoc/oxpinyin/pull/943) is merged.

| Capability | Status on main |
| --- | --- |
| Native test entry point, minimum version and ignored-test mapping (§§1, 4) | Implemented in #943 through `tools.dev.test_runner`, `just test` and `just test-doc`. |
| Native profiles and resource groups (§2) | Implemented in #943: local uses admitted num-cpus; ci uses four threads; retries are zero and fail-fast is false. The store-backend group has one thread; selected real-data binaries take the full budget. |
| Shared admission and explicit nextest concurrency (§3) | Implemented in #943 on #941's shared slot. CLI counts precede NEXTEST_TEST_THREADS, then profile settings. Unsupported or ambiguous overrides fail closed. |
| Equivalence across every native CI scope (§5) | Partial. #943 records six accepted Linux rows only at `7991ec6b17509980f42a59d72531617059f109d6`. The macOS tkrzw equivalence at `b034e834` on 2026-10-20 UTC is historical. The current macOS CI BDB scope passed at `219f420` on 2026-10-20 UTC: 1,218 passed and 22 ignored; identities and verdicts match. [#968](https://github.com/shenghaoc/oxpinyin/pull/968) (merge `c017ebee`) compared all 1,531 Linux and 1,246 macOS crate/binary-qualified identities and verdicts against cargo test; all matched, and both doctest sets matched six identities (four passed, two ignored). Other scopes and sanitizer equivalence remain unverified. |
| CI and AGENTS.md rule 5 cutover (§5) | Partial T13N: [#968](https://github.com/shenghaoc/oxpinyin/pull/968) (merge `c017ebee`) switched `ci.yml` test (Linux) and test-macos to pinned nextest 0.9.143 with `--profile ci` and separate `cargo test --doc` steps. Features, exclusions and ignored-test behavior are preserved. Windows stays on cargo test (#950). Other workflows and AGENTS.md rule 5 are unchanged. |
| Pin-dependent test route | Proposed under Layer 4; no container profile exists yet. |

macOS: bounded checks passed on 2026-10-20 UTC at
`b034e8342e70aab874b32414976cc9b6d232dc1d`.
`tools.dev.test_runner.test` passed 16 tests. One live
`just test --locked -p oxpinyin-core` run passed 189 tests and printed
`nextest test threads: 1 (admitted)`. This verifies the selected local path,
not every CI or backend scope. For the historical macOS `ci.yml` tkrzw scope
at `b034e834`, Cargo and nextest each passed 1,222 tests and skipped the same
20. Discovery found 1,242 test identities in 85 binaries. Every per-binary
name, pass verdict and ignored status matched. The separate `just test-doc`
step matched Cargo's six doctest identities: four passed and two ignored.
The runs used one test thread through the shared slot.

The current `ci.yml` test-macos BDB scope passed equivalence on 2026-10-20 UTC
at `219f4205952a6db549331a90ec11501f6be0b68d`, with Homebrew
`berkeley-db@5` 5.3.28. Cargo and nextest each passed 1,218 tests and ignored
22, across 1,240 identities in 85 binaries. Identities and verdicts match.
The separate `cargo test --doc` comparison matched all six doctest
identities: four passed and two ignored. The commands used `just heavy --`,
one Cargo job, one test thread and the repository's nextest `local` profile.
Elapsed times were excluded. The earlier BDB `just check` result remains
separate evidence. Other CI scopes remain unverified.
Windows: TODO (no host); tooling verification is deferred.
Historical macOS comparisons in #943 belong to `e23367bc` and include failed
backend runs. They do not verify this head. Hosted CI runs the repository
Python harnesses on Linux and macOS ([#988](https://github.com/shenghaoc/oxpinyin/pull/988),
merge `96ca8c8e`), including the slot harness. It supplies no PowerShell or
Windows acceptance. The sections below keep pending requirements.

The approved T13N cutover covers only `ci.yml` test and test-macos.
Both reuse verify-nightly's checksum-verified release-archive installation
and version 0.9.143. Linux uses `x86_64-unknown-linux-musl`; macOS uses
`universal-apple-darwin`, with its published SHA-256 from the
[cargo-nextest 0.9.143 release](https://github.com/nextest-rs/nextest/releases/tag/cargo-nextest-0.9.143).
PR CI supplies the one-cycle comparison against main: compare complete
crate/binary-qualified identities and verdicts, then separate doctest counts.
Normalize nextest skipped to Cargo ignored only for the same ignored test.
Use `--status-level skip` to expose ignored identities without changing selection.
Record observed job times without a controlled performance claim. The implementing
PR records the one-cycle comparison and verified head. No logs, archives or evidence files are retained or uploaded.
Windows remains on cargo test under #950.

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
PR #943 records the minimum and provisioning commands in setup and doctor.
This reconciliation installs nothing. Keep `rust-toolchain.toml`
as the Rust toolchain authority.

## 2. Implemented native configuration

PR #943 added `.config/nextest.toml`.
Define native `local` and `ci` profiles explicitly. Let both inherit a shared
base where their behaviour agrees. Local limits serve developer machines;
CI limits serve the existing native CI scopes. Do not copy CI concurrency
to every local host. Record each chosen limit and its resource reason.
Keep retries at zero for the equivalence run, so a retry cannot hide a failure.
Do not narrow test discovery through an unreviewed default filter.

Use named test groups with `max-threads` for tests that interfere or use much
memory. The implemented `store-backend` group has a ceiling of 1. Its binary
filters are `assembly`, `empty_user_dir`, `trait_laws`, `user_store_acid`,
`user_store_semantics`, `bdb_libpinyin_files`, `libpinyin_data_dir` and
`user_dir_round_trip`. These cases create or recover backend files, fork or
change process state. Independent logic and in-memory tests stay outside.
Name other shared resources before adding groups. Any extension must list
its matched tests and justify exclusions.

The `bdb_libpinyin_files`, `libpinyin_data_dir` and `user_dir_round_trip`
binaries use `threads-required = "num-test-threads"` to run alone
within that invocation. Use a smaller weight only for a measured
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

Keep doctests on `cargo test --doc`, through `just test-doc` under heavy-slot admission.
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

Cover every crate/feature set that current CI runs. These scopes are
current on main at `219f4205`; the table states required comparisons, not
completed validation on each host:

| Existing native job/scope | Required nextest comparison |
| --- | --- |
| `ci.yml` test | Default-feature native workspace tests; keep non-Rust gates separate. |
| `ci.yml` test-macos | Workspace with `--no-default-features --features bdb`, Homebrew `berkeley-db@5`, and the same six exclusions. Passed at `219f420` on 2026-10-20 UTC: 1,218 passed and 22 ignored; identities and verdicts match. Separate doctests: four passed and two ignored. |
| `ci.yml` test-windows | Default BDB workspace and the same six exclusions; preserve its current dependency setup. |
| `store-backends.yml` store-features | Existing bdb, kyotocabinet and tkrzw crate/feature selections. |
| `verify-nightly.yml` overflow-lane | Kyotocabinet workspace with `--locked --workspace --release --no-default-features --features kyotocabinet`. Set `RUSTFLAGS=-C overflow-checks=y -C debug-assertions=y -D warnings`. Preserve both profile and flags. |
| `verify-nightly.yml` nextest | Existing scheduled Kyotocabinet workspace with `--locked --workspace --no-default-features --features kyotocabinet` and separate doctests. This predates the new native recipes. |
| Native sanitizer test selections | Compare only supported libtest targets; keep specialized sanitizer tools where nextest cannot replace them. |

The six macOS/Windows exclusions are `oxpinyin-capi`, `oxpinyin-zhuyin-capi`,
`oxpinyin-corpus`, `oxpinyin-kmm`, `oxpinyin-punct` and `oxpinyin-word`.
Keep the Windows CI dependency/version exception confined to that CI scope.
Do not claim a validated local storage setup from CI alone.
The local completion gate stays limited to touched crates under AGENTS.md.
The workspace sweep remains CI's job. Each workflow edit needs separate
approval; the T13N exception above covers only the two approved CI jobs.

After equivalence passes, the cutover implementation updates AGENTS.md rule 5.
Replace its `cargo test -p <crate>` completion gate with the native `just test`
nextest route. Keep the same touched-crate scope and feature selection.
Keep doctests on `cargo test --doc` through `just test-doc` under heavy-slot admission.
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
