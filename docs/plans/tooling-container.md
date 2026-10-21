# Tooling layer 4: pinned Linux container

Status: partial foundation; Python service proposed.
Reconciled: 2026-10-20 UTC (`date -u`).
This layer builds on [native nextest](tooling-nextest.md).
The shell compatibility route is implemented. It is not the planned Python service.

| Capability | Status on main |
| --- | --- |
| Shell launcher, inner script, compatibility image and runtime selection (§2) | Implemented in [#890](https://github.com/shenghaoc/oxpinyin/pull/890): `compat.sh`, `compat-inside.sh`, `Dockerfile.compat` and `runtime.sh`. Keep that PR's runtime limitations. |
| Container-owned uv and exact managed interpreter (§3) | Implemented in [#939](https://github.com/shenghaoc/oxpinyin/pull/939). This is interpreter provisioning, not delivery of all environment services below. Frozen oracle artifacts stay unchanged. |
| Environment isolation and identities (§§1, 3) | Slice 2 adds `tools.dev.compat.boundary` preflight, explicit container state paths, source and artifact content identities, and verified exports outside native `target/`. Python harnesses and the Podman `pc-version` transport smoke passed; artifact mutation/refusal is covered synthetically, without Rust builds. Docker was absent and Apple was not exercised in this task. The shell launcher and existing runner selection are retained. See [#969](https://github.com/shenghaoc/oxpinyin/pull/969) for provenance. |
| Hard memory limit and native lifetime reservation (§1) | T2C first slice implemented through `tools.dev.compat.memory`, `tools.dev.slot`/`host` and the retained shell launcher. Synthetic lifetime/recovery tests, the Apple memory-only smoke and the Linux Docker memory smoke passed in [#957](https://github.com/shenghaoc/oxpinyin/pull/957). Rootless Podman memory smoke passed on current main, closing the Podman-absent gap from #957/#966; see [#969](https://github.com/shenghaoc/oxpinyin/pull/969) for provenance. Windows TODO. |
| Python launcher, cell/pin/systemdir/artifacts modules, cloud daemon/network checks and just container surface (§§2–4) | Proposed. Existing shell provisioning is retained; there is no Python container service or `just container` recipe. |
| Two identified source snapshots and same-run builds (§4) | Proposed transport contract. |
| Container nextest profile and pin-test handoff (§4) | Proposed. #943 configures native profiles only. |
| Gate placement, full launcher parity, CI handoff and retirement (§§5–6) | Proposed migration. Current registry/workflow scopes below are inputs to acceptance, not completed cutovers. |

Requirements below marked proposed by this table do not describe current
`compat.sh` guarantees. No runtime, host or emulation acceptance is added here.
Historical findings keep their recorded heads and limitations.

## 1. Environment boundary

Use the pinned Debian amd64 container for every pin, oracle, cell and
contract-differential task. Apply this rule on Linux hosts too.
Native Linux pin work is superseded by this ruling.
The subject, pinned reference and harness run together inside the container.
Never load a host oracle or reuse a host subject build for this comparison.
Use the source snapshot as input; keep its identity in the result.
Do not start a Windows libpinyin build project.

[Layer 1](tooling-python.md) owns native scripting and slot semantics.
[Layer 2](tooling-just.md) owns native recipes.
Native recipes never start this environment. A user explicitly selects
`just container <task>` instead. Host launch is a transport operation;
it does not acquire a native heavy slot on behalf of the container.
The selected task takes its own slot inside the container.

Keep these state domains disjoint, even on one Linux host:

| State | Native | Container |
| --- | --- | --- |
| Rust artifacts | Native worktree/target/profile | Container volume, worktree/cell/profile |
| Heavy locks and inner reservations | Native lock directory | Separate container lock directory |
| Host memory budget | Native reservations, including each live container's hard limit | Inner admission stays within that limit |
| Managed interpreter | Host uv installation | Container-owned managed installation |
| Tool and data caches | Native caches | Container uv/Cargo/oracle/model caches |
| Result records | Native result namespace | Container result namespace |

Do not mount or select the native target, lock, Python, uv cache or Cargo cache
as container state. Reject overrides that cross the boundary.
Clear host artifact/library/interpreter overrides and the inherited heavy-slot
marker before starting the inner task. Build the inner environment explicitly.
Accept only declared task inputs and validate their content identities.
An exported container report remains a container record. Never reuse it as
native verification. The reverse is also prohibited.

Host-side container transport and later container services live in
`tools.dev.compat`, a domain subpackage of the single `tools.dev` package.
Admission remains owned by the shared `tools.dev.slot` implementation.
Use shared slot code with independent state. Preserve the fail-closed memory
rule from Layer 1. The container reads its own effective resources, including
cgroup limits, and counts only its environment's live reservations.
One environment never takes another environment's slot.
The native host budget record below is the sole state-separation exception.
Keep the current per-checkout volume lock for source/preparation serialization.
Use container-only heavy locks for tasks that share one container environment
and filesystem. Do not claim locking across different runtime VMs or kernels.
The former Linux/Windows shared-lock-directory test is superseded.

Before launch, enforce a hard container memory limit through the runtime.
Record the full limit as a native-memory reservation in the native lock
directory. Apply Layer 1's native memory admission to that full limit.
Wait, then fail closed if the budget cannot fit or cannot be measured.
Serialize the budget check and reservation update with native admission.
Native admission subtracts that reservation for the container's full lifetime.
Apply the 2048 MiB desktop headroom once, on the native host. A compatibility
guest with `OXPINYIN_COMPAT_CONTAINER=1` and a visible finite cgroup `memory.max`
uses zero desktop headroom. Without that finite limit, keep native headroom.
Inner admission uses the effective cgroup limit and its own reservations.
Size the default limit from a cold two-job build and the selected gates on one
host, including measured overhead and at least the two-job build and runner
estimates. The measured default is 2560 MiB; host admission requires 4608 MiB
including desktop headroom (the [runbook](../runbooks/compatibility-container.md#container-memory-budget)
records the sizing). A host launcher excludes only the native slot identified
by the existing validated live-ancestor marker; other live native slots still count.
Reject a runtime that cannot enforce and report the hard limit.
Do not launch without both the limit and the native reservation.
Layer 1 consumes the record; this layer owns its creation and lifetime.

Release the reservation only after confirmed container exit.
On cancellation, stop the container and wait for exit before release.
Launcher death must not clear a live container's reservation.
Check container liveness during recovery. Keep the reservation when liveness
is unknown. Accept no native slot ownership or inner lock state in this record.

## 2. Existing recipe and runtime order

Use the merged #890 inputs: `tools/oracle/compat.sh`,
`tools/oracle/compat-inside.sh`, `tools/oracle/Dockerfile.compat` and the
[compatibility runbook](../runbooks/compatibility-container.md).
That runbook describes current behaviour. This plan defines the later Python
launcher and stricter environment boundary. It does not change the launcher now.

Preserve runtime order:

| Host | Order |
| --- | --- |
| Windows | `wslc`, then `podman`, then `docker` |
| macOS | Apple `container`, then `podman`, then `docker` |
| Linux | `podman`, then `docker` |

An explicit `--runtime` or `OXPINYIN_CONTAINER` wins.
Print the selected runtime and version before execution.
If none is present, fail and name candidates in order.
If a selected adapter is not verified, report that status explicitly.
Do not claim that detection proves support or silently change runtimes.
Windows wslc, Apple launch and arm64-host amd64 emulation remain unverified
until their compatibility smoke passes. Task launchers do not install or start
host services. Cloud setup follows the rule below.
Do not dispatch work to a remote machine implicitly.

Cloud sessions, including Claude and Codex cloud environments, are supported
container hosts. Before any `just container` task, cloud setup starts the
Docker daemon or checks that it runs. It checks access to the image registry,
`snapshot.debian.org`, `downloads.sourceforge.net` and the SourceForge mirror
hosts reached by the [model20 fetcher](../../tools/model/fetch-model.sh).
If a host is blocked, provisioning fails and names that host. Never skip.
When this implementation lands, AGENTS.md's “cloud sessions are a supported
host for every gate” uses this container route instead of native provisioning.
This plan changes no current AGENTS.md rule.

Functional differentials may use amd64 emulation after runtime validation.
Pin timing, base/PR/pin gates and comparisons with Linux measurements require
non-emulated Linux amd64 execution. Valgrind stays inside this environment.
Add no native instruction-count backend or cross-OS instruction comparison.
Native macOS oxpinyin-only information remains a separate Layer 5 measurement.

## 3. Container provisioning and identities

Provision a separate uv-managed CPython 3.15.0 inside the container.
Use [Layer 1's interpreter contract](tooling-python.md#2-interpreter-contract)
with container-owned managed storage and cache. Do not share the host executable.
Provision failures name their cause and never select system Python.
Keep the frozen oracle libraries, manifests and model unchanged.
Do not rebuild them for the interpreter cutover.

Keep existing provisioning recipe bytes as hash inputs to image tags.
Keep immutable reference/deps selection, hash validation and local model policy.
Read both pin-record schemas. Check all six library hashes, data manifests and
record-versus-constant agreement. Check pin, patch and cell identity before
building a subject. Keep missing-input and wrong-cell refusal reasons.

The container modules own `cell`, `pin`, `systemdir` and `artifacts`.
Keep the five backend spellings in one mapping. Keep `system-dir.sh`'s
resolution order, completeness checks, provenance and refusal messages.
Retain one target directory per worktree, cell and build profile inside the
container. Reuse it across tasks; keep dev opt-level 1 and debug differential
builds. Never rebuild a prebuilt override. Validate its full recorded identity.
Keep the source snapshot, image, toolchain, interpreter, model and input digests
in container result provenance. Layer 5 owns result reuse and parser semantics.

## 4. Command surface and Rust tests

Each root recipe calls one Python entry point. Python selects and launches the
runtime, forwards the argument vector and propagates the child status.
Keep argument boundaries, spaced paths and statuses 0, 1 and 77 unchanged.
An unavailable runtime or required input never becomes a successful skip.
No root recipe contains orchestration logic.

| Container command | Task inside the pinned environment | Existing entry points |
| --- | --- | --- |
| `just container list` | List registry work without execution. | `run-all.py --list` |
| `just container plan` | Print selected work without execution. | Proposed wrapper. #941 implemented read-only `tools.dev plan --runners/--cells` for row metadata; resolved commands and identities remain proposed. |
| `just container doctor` | Report inner capabilities. | Cell/env probes |
| `just container run` | Run selected registry gates. | `run-all.sh`, `run-all.py` |
| `just container contract` | Run selected contract families/options/Axis C. | `contract-diff.py`, `transformed-options-sweep.py`, wrappers |
| `just container cells` | Describe/select cells, data and artifacts. | `oracle-cell.sh`, `system-dir.sh`, `cell-artifacts.sh` |
| `just container oracle build\|pull\|verify\|regen-data` | Provision or verify frozen-reference inputs. | Existing oracle provisioning scripts |
| `just container test --differential` | Run pin-dependent Rust tests. | `run-differentials.sh`, `nightly-fixture-differentials.sh` |
| `just container ci <job>` | Run a separately approved pin-dependent CI task. | Pin workflow-specific commands |
| `just container measure <recipe>` / `initbench` | Run pin-dependent instruments and measurement. | Existing perf recipes and recovered #842 controller |
| `just container abi` / `packaging` | Run pinned ABI and staged-install comparisons. | ABI and packaging differential wrappers |
| `just container compat-smoke` | Check image/runtime/argument/status integration. | `compat.sh`, `compat-inside.sh` |

The current `run-all.py` has `--list` but no `--plan` flag.
It prints its plan on the execution path. PR #941 added a read-only
`tools.dev plan --runners/--cells` that prints row metadata. Extending it to
`just container plan` with resolved commands and identities remains proposed.
It starts no build, worker, selected command, check or provisioning operation.
Container launch supplies transport; it does not turn plan mode into a run.

Pin-dependent measurement accepts two separate, content-identified source
snapshots: one base tree and one PR tree. Record each commit and full source
digest. Reject a missing snapshot or an identity mismatch before building.
Build both trees inside the same container run as the pin comparison.
Use separate source and build directories per tree, cell and profile.
Accept no host-built artifact in place of either source snapshot.
This layer owns the transport contract. Layer 5 owns measurement semantics.

Use nextest inside the container for supported pin-dependent Rust libtest
targets. Select a distinct `container` profile in the planned nextest config.
It must not inherit native `local` or native `ci` environment paths.
Use its own targets, interpreter, inputs, locks, cache and results.
Map selected ignored tests with `--run-ignored=all` or `only` as Layer 3 defines.
Keep doctests on a separate inner `cargo test --doc` step.
Keep Python contract differentials and native measured instruments outside
nextest. Compare the same pin-test IDs and pass/fail outcomes before cutover.

## 5. Current gate placement

Scope below is from `tools/bisection/runners.json` and workflow commands on
main at `b034e834`. No gate has moved through the proposed Python service.
The existing aggregate runner establishes a cell and reference environment.
Every registry invocation listed below therefore remains container-scoped.
The catalog stays singular. A leaf script can also have a separate native
subject-only use; that use never supplies the aggregate's container result.

Current CI also uses generic images for some subject-only commands.
Keep those commands as parity inputs. Such an image does not supply this
pinned reference environment or prove execution on a native host.
Planned native development uses the host toolchain from Layers 1–3.
Any later CI host change needs separate approval.

| Native gate or command | Native scope |
| --- | --- |
| `cargo fmt` including fuzz formatting; `cargo clippy`; `cargo deny` | Source/tool checks without a reference installation. |
| `ci.yml` test, test-macos and test-windows Rust tests | Existing native crate/feature scopes from Layer 3. |
| `tools/dev/check.sh` | Selected native storage/C-API setup tests. |
| `tools/store/backend-matrix.sh`; store-features | Native backend feature validity and tests. |
| `tools/bdb/run-sanitizers.sh`; `tools/kc/run-sanitizers.sh`; tkrzw-sanitizers | Native backend tests without a reference installation. |
| `coverage.yml`; native fuzz-soak selections | Existing coverage/fuzz targets without pin inputs. |
| `run-cpp-smoke.sh`; `check-exports.sh` including `--shipped`; `check-alloc-pairing.sh` | Direct subject-only CI gates against committed headers/fixtures. Keep existing platform limits. |
| `check-pc-version.sh` | Static checked-in version consistency; no reference installation or live pin execution. |
| `tests/lint-crate-cfg-placement.test.sh` | Keep the native lint-checker harness. |
| `.github/scripts/lint-crate-cfg-placement.sh` | Keep the native crate-configuration placement check. |
| `lizard crates/ -l rust -C 40` | Keep the native complexity gate with `lizard==1.24.0`. |
| `sha256sum -c fixtures/w3/fixtures.sha256` | Keep the W3 fixture checksum gate. |
| `verify-nightly.yml` overflow-lane | Run `cargo test --locked --workspace --release --no-default-features --features kyotocabinet` with `RUSTFLAGS=-C overflow-checks=y -C debug-assertions=y -D warnings`. Layer 3 owns nextest parity. |

Pin-dependent selections of an otherwise native tool belong below.
This includes `ci.yml`'s live-typing comparison, pin-built user-directory
round trips, staged pin/subject pkg-config comparisons, and reference sweeps.
Do not mix the two environments in one verification claim.

| Existing registry row | Environment | Kind |
| --- | --- | --- |
| `abi-probe-diff` | container | gate |
| `addon-candidate-diff` | container | gate |
| `bigram-export-diff` | container | gate |
| `bisect` | container | gate |
| `callgrind-differential` | container | not-a-gate |
| `candidate-assembly-diff` | container | gate |
| `contract-diff` | container | gate |
| `cpp-smoke` | container | gate |
| `dict-surface-diff` | container | gate |
| `dynamic-adjust-diff` | container | gate |
| `export-diff` | container | gate |
| `import-diff` | container | gate |
| `import-semantics-diff` | container | gate |
| `key-surface-diff` | container | gate |
| `live-typing-diff` | container | gate |
| `locale-diff` | container | gate |
| `nbest-choose-train-diff` | container | gate |
| `nbest-train-diff` | container | gate |
| `open-counter-diff` | container | gate |
| `option-sweep` | container | gate |
| `perf-baseline` | container | not-a-gate |
| `perf-matrix` | container | not-a-gate |
| `perf-same-data` | container | not-a-gate |
| `phrase-surface-diff` | container | gate |
| `pred-order-diff` | container | gate |
| `predict-diff` | container | gate |
| `punct-diff` | container | gate |
| `residue-a-tail-diff` | container | gate |
| `residue-mechanism-diff` | container | gate |
| `rss-diag` | container | not-a-gate |
| `same-data-dir-diff` | container | gate |
| `scheme-diff` | container | gate |
| `session-fork-diff` | container | gate |
| `shipped-check` | container | not-a-gate |
| `system-import-profile` | container | not-a-gate |
| `system-import-round-trip` | container | gate |
| `train-diff-dynamic-off` | container | gate |
| `train-diff` | container | gate |
| `tree-recipe-control` | container | not-a-gate |
| `two-context-diff` | container | gate |
| `uncovered-surface-diff` | container | gate |
| `union-diff` | container | gate |
| `user-candidate-diff` | container | gate |
| `user-file-modes-diff` | container | gate |
| `w11-diff` | container | gate |
| `zhuyin-diff` | container | gate |
| `zhuyin-import-diff` | container | gate |
| `system-import-round-trip-zhuyin` | container | gate |
| `train-index-diff` | container | gate |
| `transformed-options-sweep` | container | gate |
| `abi-exports` | container | gate |
| `abi-alloc` | container | gate |
| `pc-version` | container | gate |
| `pc-metadata` | container | gate |
| `user-dir-round-trip` | container | gate |

## 6. Launcher acceptance and migration

T2C first-slice status: hard-limit/reservation acceptance is covered by
`tools/dev/compat/memory.test.py` and `compat.sh --memory-smoke`. This is not
full launcher parity, a runner migration, pin handoff or retirement.

Keep both compatibility shell scripts until side-by-side launch acceptance
passes. Compare image identity, runtime order, explicit overrides, arguments
with spaces, status propagation and compatibility smoke. Check disjoint paths
and no inherited native marker/cache/artifact. Test launch from each host.
Check hard-limit enforcement and the matching native budget reservation.
Run native and container admission together; require the native side to
subtract the full container limit. Check cancellation and launcher death.
Keep a live or uncertain container reserved until confirmed exit.
Check plan-only mode with selected work that would build or start workers;
require no execution or provisioning. Check both source snapshots, their
identities and separate build paths in one base/PR/pin measurement run.
Record unavailable runtime and emulation cases as untested with reasons.
Tooling stage T6 in Layer 5 owns the handoff. Layer 5 §5.4 permits script
retirement in the replacement PR after proof, consumer migration and named
maintainer approval. T14 keeps final repository-wide reconciliation.

Keep BDB as the default comparison cell. Use all three cells only when
AGENTS.md's mechanical path check selects them. Keep the start-of-sweep and
pre-release cross-cell obligations. Use sound tkrzw only. Report suspected
backend dependence without a path trigger as a finding, not permission to
run extra cells. Each CI migration needs separate approval and comparison.
