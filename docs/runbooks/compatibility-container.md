# Canonical Debian compatibility verification

[Provisioned cloud sessions](cloud.md) are first-class supported hosts for
every gate. This container procedure is a second supported route. First
[develop and test natively](setup.md). Use this procedure only when a
change requires comparison with the pinned upstream implementation. It
executes **subject** (oxpinyin), **reference** (the oracle) and the existing
comparison **harness** together in the existing canonical pinned **Debian Linux amd64** environment.
Its base digest, dated package snapshot and reference hashes remain defined
by [oracle-pin.txt](../../tools/oracle/oracle-pin.txt) and the existing
[oracle recipe](../../tools/oracle/Dockerfile.oracle). Every host uses this
same reference environment; there are no host-specific reference images.
A passing differential covers its selected behavior/reference environment;
it does not prove native Windows/macOS ABI behavior. Native tests provide
separate platform evidence.

## Container memory budget

Each execution uses a runtime hard limit and a matching native reservation.
`--memory-mib N` overrides `OXPINYIN_CONTAINER_MEMORY_MIB`; both accept only a
positive integer in MiB. The default is **2560 MiB**. A serial base/candidate
measurement on Ubuntu with rootless Podman and BDB `cpp-smoke --jobs 2`
(2026-10-21 UTC) measured a base cold-build peak of 2,232,991,744 bytes
(rounded up to 2130 MiB), including container preparation. Add a 20% margin
(426 MiB) and round up to 256 MiB: 2560 MiB. This also exceeds the two-job
build estimate (2 × 600 MiB) and runner estimate (2 × 1024 MiB). The candidate
peaked at 1,860,026,368 bytes and passed without reaching its limit. The PR
records the commands and separate gate-phase peaks; raw capture was not kept.
Larger explicit job counts can need a larger limit.
Desktop headroom applies once, on the host: host admission requires the full
container limit plus the desktop headroom, after other reservations. The
headroom is 10% of total memory, rounded down, at most 2048 MiB, and 2048 MiB
when total memory is unknown. A 16 GiB Linux host reports a MemTotal of about
15,688 MiB, so by default admission needs 2560 + min(2048, 1568) = 4128 MiB;
from 20,480 MiB of total memory it needs 2560 + 2048 = 4608 MiB.
Inside the compatibility container, `OXPINYIN_COMPAT_CONTAINER=1` and a visible finite cgroup `memory.max`
select zero desktop headroom. Native work, an unlimited guest and an unknown
cgroup limit retain the normal desktop headroom. Admission uses the
native resource measurements and `OXPINYIN_RESOURCE_WAIT_SECONDS` (default
1800); unknown or inadequate memory waits, then fails without launching.
The host needs uv and its managed CPython for this shared code; `just doctor` checks both against their minimums.

`tools.dev.slot` serializes native admission and container budget changes with
`heavy-admission` in the native lock directory (`OXPINYIN_LOCK_DIR`, otherwise
the existing host default). It subtracts the full container limit throughout
its lifetime. No native heavy slot is acquired for transport. If transport
inherits `OXPINYIN_HEAVY_SLOT_HELD`, its validated live ancestor slot is excluded
from the container budget; other native slots still count. The inner task
retains independent admission, effective cgroup/guest memory measurements and
container-only reservations. The Apple result-reader container uses the same
hard-limit/reservation path and gets its own record. The helper writes an exit
confirmation only after the main container is confirmed gone and its record
released. The shell requires that confirmation before starting the reader;
an uncertain main exit starts no reader. The two cannot overlap through this
launcher. Main task, Apple reader and memory smoke all call `reserved_run`,
which delegates to the single `tools.dev.compat.memory.run` launch function.

`container-memory-<token>.json` is an atomic JSON record with `schema: 1`, an
absolute `runtime` executable, `kind` (`container`, `podman`, `docker`), `name`,
random `token`, positive `memory_mib`, `phase` (`creating`, `created`, or
`create-failed`), and
UTC `since`. It contains no PID/slot ownership or inner lock state. The token
must match the runtime label `io.oxpinyin.memory-reservation`. Its `.guard`
file fences creation and attached runtime clients; they inherit the descriptor
so launcher SIGKILL cannot expose a launch/recovery gap. Records are written
before creation, under the same admission lock used for updates/removal.

Create does not run the payload. The launcher checks the reported hard limit
before start. Docker/Podman additionally cap memory+swap and verify an effective
kernel cgroup limit before executing the payload. Unsupported/unreadable limits
fail. Cancellation stops the container and waits for confirmed exit before
release. Recovery removes only a settled record whose labeled container is
confirmed stopped or absent from a successful complete runtime listing. When
no launch client holds the guard, a token-labeled `created` (never-started)
container is removed without force and absence is confirmed before releasing
its budget. A live guard prevents this cleanup. A normally returned create
failure is recorded as `create-failed`; a complete token-label listing must
prove absence before release, including on subsequent recovery.
Unavailable runtimes, unknown states and mismatched labels retain the budget
and report uncertainty. An interrupted `creating` record remains reserved even
when its name is absent: a daemon request may still finish later. Such records
need operator resolution after all creation requests are known to have settled;
launcher PID death alone is insufficient. Malformed records block admission.
Apple liveness accepts the `status` object with a `state` string observed from
`container inspect` on container 1.5.0, and the documented scalar status form.
Other status shapes remain unknown. If stdout closes during a console-backed
run, the sole pipe reader keeps draining into the console file until the
runtime client exits; it stops only mirroring to the closed stdout sink.

For a short transport/memory check using an **already cached** compatibility
image, without oracle provisioning/builds or evidence files:

```bash
bash tools/oracle/compat.sh --runtime container --memory-mib 512 --memory-smoke
```

Use `podman` or `docker` on Linux. During the 12-second alive window, native
admission must report `container reservations=512 MiB`. The guest then attempts
an allocation twice its limit; the smoke requires its child to be killed (137).
After confirmed container exit, no smoke reservation should remain. This smoke
does not establish differential compatibility or full runtime/emulation parity.

## Prerequisites

A container **runtime** runs a packaged Linux environment. Its **image**
supplies the starting tools/files; a named **volume** retains local working
data and caches (reused downloads/build outputs) between runs. A **mount**
makes a directory available inside it. The launcher supplies a **snapshot**
(copy) of your current source, then runs the harness (comparison test driver).
You need not manage those internal paths to use the command below.

On the host, install Git, Bash, tar, a SHA-256 utility (`sha256sum` or macOS
`shasum`), and one working free/open container runtime. The launcher does
not install packages, start background services or modify host trust stores. It needs
no host Rust, DBM development packages, libpinyin or remembered oracle env.
Network access is needed for uncached images/packages/toolchains/crates/model
inputs. The default is platform-native first, then Podman, then Docker: Windows
(`wslc`, `podman`, `docker`), macOS (`container`, `podman`, `docker`), Linux
(`podman`, `docker`). `--runtime` or `OXPINYIN_CONTAINER` overrides selection.
The selected runtime/version is printed before provisioning; none available
is an error naming the candidates. `wslc` has no verified adapter here and
stops with an explicit not-yet-verified message instead of silently falling
back. Apple interoperability and amd64 emulation on arm64 are also not yet
verified. For an exercised Linux route, use rootless Podman and its [official installation
instructions](https://podman.io/docs/installation). In this host terminal:

```bash
podman info
```

It must exit zero and describe the engine. If it fails, retain the error and
repair access using the official instructions or ask the environment owner.
Do not continue to a compatibility claim. No commercial desktop application,
IDE or coding-agent provider is required.

Small runtime alternatives use the same recipe: [Docker Engine](https://docs.docker.com/engine/install/)
(`docker info`, then `--runtime docker`) or [Apple container (not yet verified)](https://github.com/apple/container/blob/main/docs/tutorial.md)
(`container system status`, then `--runtime container`). Install/start the
chosen runtime using its official instructions; the launcher does not do so.

Windows contributors use WSL Bash for this Linux comparison, with a working
Docker/Podman engine available there. [WSL setup](https://learn.microsoft.com/en-us/windows/wsl/install)
is separate from ordinary [native Windows development](setup.md#native-windows).
The launcher diagnoses Git Bash and directs it to WSL rather than treating
MSYS paths as Linux mounts. Native Windows work still needs neither WSL nor
a container.

The reference is fixed at Linux amd64 and its recorded hashes remain
mandatory. On an arm64 host, configure a runtime capable of running/building
Linux amd64 images first; this is **emulated**, not native performance
evidence. Apple `container`'s command interfaces are source-checked, but its
amd64 build/run and restart behavior were not executed in this task. If its
installed version cannot execute this platform, use an appropriate Linux
amd64 VM/environment with Docker/Podman rather than changing pins or loading
Linux libraries on macOS.

## Select and run a check

All following commands are **host Bash commands at your checkout root**.
An ordinary clone or a linked worktree works: the launcher snapshots visible
source (including uncommitted edits), not the host `.git` pointer. It never
mounts another worktree or your whole home. Ignored files, model data and
native `target/` outputs do not enter the source snapshot.

Discover existing check names on Linux/macOS/WSL (uv and its managed CPython,
which `just doctor` checks, are needed only for this local listing):

```bash
bash tools/oracle/compat.sh --list
```

The [registry](../../tools/bisection/runners.json) records each check's
inputs, backend cells and issue association. Start with this representative
same-data-directory differential:

```bash
bash tools/oracle/compat.sh --runtime podman --cell bdb --runners same-data-dir-diff
```

The small runtime adapters use each CLI's actual documented interfaces
([Apple command reference](https://github.com/apple/container/blob/main/docs/command-reference.md),
[Podman run](https://docs.podman.io/en/latest/markdown/podman-run.1.html)).
Rootless Podman's input bind mount uses shared SELinux `:z`; all generated
files stay on the Linux volume. Runtime smoke checks establish launch/mount
interoperability only; the same Debian differential need not be repeated for
every host/runtime combination.

For another contribution, replace `same-data-dir-diff` with its existing
registry name, or select associated checks with `--issue NUMBER` (replace
NUMBER with the issue number). Explicit selection avoids accidentally
launching the entire registry. BDB is the default **cell**: both sides use
the same Berkeley DB backend. `--cell kc` and `--cell tkrzw` select the
existing peer backends. Follow the repository's touched-path backend rules
in [oracle procedures](oracle.md) before deciding which cells to run.

Use `--jobs 2` (the default) to bound compiler and runner concurrency and
the execution container to two CPUs. Podman image builds also receive that
CPU quota; Docker image-build CPU limits depend on your configured builder.
Apple builds use its `--cpus` setting. The
existing registry's `--timeout` and `--runner-timeout NAME=SECONDS` options
are accepted; no skip-as-pass override or arbitrary host artifact paths are
forwarded. Some specialized registry entries require extra installations or
patched reference variants; their missing-input result is a failure, not a
claim that the basic environment covers every advanced task.

## What happens and what success means

The launcher obtains the existing immutable GHCR oracle artifact and its
existing pinned `deps` stage, or builds an absent tag from
`tools/oracle/Dockerfile.oracle`. Access/auth/proxy errors stop with retained
pull diagnostics; they do not silently start an expensive fallback. The base
image is pulled only if a local fallback build needs it.

`tools/oracle/Dockerfile.compat` adds the repository-selected Rust toolchain,
libclang and backend dump utilities to that same pinned environment, with
no model bytes or contributor source baked into the image. There is no
second oracle implementation and no general development image.

Inside the container, existing `tools/cloud/setup.sh --in-container` verifies
reference hashes, reuses `tools/model/fetch-model.sh`, regenerates/validates
backend data when needed, and writes an internal environment file. Existing
cell selection, artifact building and `run-all.sh` perform the comparison.
No nested container runtime is required. A clean container environment prevents
host library/artifact/backend overrides from contaminating the comparison.

Success requires exit zero and **PASS for every selected registry row**.
FAIL, SKIPPED, a missing model/archive, hash mismatch, unavailable runtime or
behavioral mismatch is not passing verification. A missing optional input
in ordinary native tests is different from a missing required differential
input here.

## Inputs, results and resumption

The host preflight in `tools.dev.compat.boundary` runs before runtime access,
pulls, snapshot creation or memory reservation. Unknown options, runner/cell
selections, timeouts naming unselected runners, invalid model checksums and
undeclared `OXPINYIN_*`, `PINYIN_*` or `ORACLE_*` inputs fail there. Native
target, Cargo, interpreter, uv-cache and library-path overrides are rejected;
unset them before launch. `OXPINYIN_LOCK_DIR` remains an absolute native budget
directory only, and the host heavy-slot marker is never forwarded inside.
Runtime, memory, proxy/network and optional CA inputs retain their existing
meaning. No arbitrary inner path or prebuilt host artifact input is accepted.

The inner script assigns Cargo, uv cache, home and heavy locks under
`/state/container/<image-key>/`; the managed interpreter remains the image's
`/opt/uv/python` installation. Targets add the source content identity and cell
to this container namespace. The per-checkout `/state/run.lock` still serializes
source replacement and preparation. The native budget reservation is the only
shared admission state.

`tools.dev.compat.identity` seals the source member contents/modes, archive,
optional model, inner script and verifier before launch. The inner verifier
checks the expected identity before extraction; unsafe source members fail.
Container artifact records add source/image/reference identities and SHA-256
content digests, checked before runner execution. Missing identities, changed
content (including same-size/same-mtime changes) and artifacts outside the
container target fail. Native artifact records retain their existing contract.

The boundary crossings are limited to the following:

| Crossing | Classification |
| --- | --- |
| Source snapshot, manifest and optional checksum-pinned model input | Read-only task inputs mounted at `/input`. |
| Runtime selection, jobs, hard limit, six proxy variables and optional CA | Declared transport/build configuration; no native cache or artifact path is forwarded. |
| `/state` volume and generated oracle environment | Container-owned work, caches, targets, locks and results, private to the checkout. |
| Native container-memory reservation | Sole shared-state exception; lifetime remains tied to confirmed runtime exit. |
| Host result export | Verified container record under `.oxpinyin-container`, never native verification or native build state. |

Model20 and all derived files remain local under the existing data policy.
If you already have the pinned archive, avoid downloading it again:

```bash
bash tools/oracle/compat.sh --runtime podman --archive "$HOME/Downloads/model20.text.tar.gz" --cell bdb --runners same-data-dir-diff
```

Replace that archive path with your actual file. The existing fetcher checks
its frozen checksum. Otherwise it fetches/reuses the local volume's model
cache; no CI job or published image contains it.

Each checkout path gets a distinct named Linux volume. It contains the source
snapshot, model cache, pin-specific oracle/data cache, per-image/per-backend
compiled outputs, user files, Linux scratch and result history. A volume lock
serializes invocations from the same checkout; other worktrees have separate
volumes. Rust/toolchain/image changes separate compiled outputs while the
unchanged model cache remains reusable. Source changes do not require image
rebuilds. Interrupted preparation can be rerun; existing hash/manifest checks
prevent partial inputs from being treated as verified.

The launcher prints the volume name and a host result directory under
`.oxpinyin-container/results/<source-content-sha256>/<UTC-run-id>/`.
Its `transport/` directory retains pull/build diagnostics and the attached
console. Its `result/` directory contains exported comparison JSON and existing
registry logs plus `provenance.json`: the source content manifest,
image/reference identity, artifact content identities, the container state
paths and interpreter, and result-file digests.
The export must match that identity; a missing or mismatched record fails the
launcher. These remain container records outside the native Cargo target
namespace. The input source archive is temporary and is not retained in the
export. These are local evidence, not material for Git or model redistribution.
Do not commit these local files. The JSON/table's reason
column identifies missing prerequisites versus actual differences; the
container console records source identity, reference identity and toolchain.

In a fresh host terminal, enter the same checkout and repeat the same command.
No oracle env file is sourced on the host. Do not delete the printed volume
unless you intentionally want to discard that checkout's preparation/cache
state. A different machine recreates the same environment from the checked-in
recipe and verified external inputs.

## Network and failure diagnostics

- Runtime unavailable: check its `info` (Docker/Podman) or service status
  (Apple), permissions and Linux amd64 execution capability. Ordinary native
  development remains available without it. No host daemon is started for you.
- Image absent: the launcher reports the missing tag and uses the existing
  pinned recipe. Network/auth/proxy errors retain the registry's own message
  and stop; repair access and rerun. New `-deps` anonymous pulls remain
  publication-dependent until the normal workflow publishes that tag.
- `HTTP_PROXY`/`HTTPS_PROXY`/`NO_PROXY` and lowercase counterparts are passed
  explicitly. A proxy must be reachable from the runtime's network; host
  loopback is not automatically a container-reachable address. For a Linux
  Docker/Podman loopback proxy only, set `OXPINYIN_COMPAT_NETWORK=host` before
  invoking the command; this passes the supported network flag to build/run.
  Apple's builder has no equivalent Docker/Podman `--network` interface, so
  use a proxy reachable from its VM network or a suitable Linux environment.
- For a custom CA, set `OXPINYIN_PROXY_CA` to the actual certificate file.
  It is passed as a build secret and installed into this local execution image;
  adding or rotating that certificate changes the local image identity.
  Ordinary proxies require no extra certificate unless the network owner
  requires one. The runtime's own registry
  trust is configured separately using its official docs. Do not publish a
  locally configured compatibility image.
- Artifact/data checksum mismatch: preserve expected/actual diagnostics and
  report it; never replace the frozen hash or change the oracle pin.
- Model checksum mismatch: replace the invalid local archive or clear only
  that invalid download before retrying, using the fetcher's existing help.
- Incorrect backend/selection: choose `bdb`, `kc` or `tkrzw`, consult `--list`,
  and do not share backend-specific user files or native artifacts.

[Validation status](setup.md#validated-on--not-validated) separates executed
checks, applicable native CI and untested runtime interoperability. Apple
retrieves results with a second volume-reader container because its copy
command requires a running container; Docker/Podman retain the direct copy.
