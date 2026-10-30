# Remote Linux development

A remote Linux host that meets the requirements below can run every
non-measurement gate; measurement gates also need the ext4 paths below.
Provision it with `tools/cloud/setup.sh`, which also installs Valgrind for
measurement work. Ordinary native development uses [native setup](setup.md)
and does not need a remote host. The
[canonical compatibility container](compatibility-container.md) is a second
supported route for compatibility work, not a replacement for this one.

The host may be a root container, may supply its own container runtime, and
may reach the network only through an HTTPS proxy. The provisioner handles
each case, scopes custom trust to an existing proxy certificate or an
explicit CA path, and verifies the same frozen oracle and data contracts.
No particular editor or IDE is required. Give each worktree its own
`OXPINYIN_ORACLE_ENV` and `--cache-dir` paths when several share a host.
`--in-container` is the behavioural adapter for the container route; it
neither installs nor requires Valgrind.

## Host requirements

Full setup requires Linux on a platform with frozen oracle hashes:
linux/amd64 or linux/arm64. `tools/cloud/setup.sh` selects the platform from
`uname -m` and refuses any other architecture.
The script also requires:

- Cargo on `PATH`, rustup and the installed toolchain/components selected by
  `rust-toolchain.toml`. Setup checks these before provisioning; it does not
  install Rust. Follow [native setup](setup.md) when they are missing.
- A Valgrind at or above its minimum (`just doctor` shows it), or root/sudo
  access plus `apt-get` to install or upgrade it. Root uses apt directly; a
  non-root account with sudo retains the sudo install path. Without either
  privilege route, setup prints `run as root: apt-get install -y valgrind` and
  a specific missing- or outdated-Valgrind failure before oracle downloads.
  Ask the environment owner to install it or choose a host with a suitable
  Valgrind already available.
- A writable uv installation directory when uv needs installing or replacing
  (default `$HOME/.local/bin`), plus writable managed-Python/cache locations.
  Setup probes the uv destination before downloading. If the host image mounts
  it read-only, set `UV_INSTALL_DIR`, `UV_PYTHON_INSTALL_DIR` and `UV_CACHE_DIR`
  to task-owned writable paths; put `UV_INSTALL_DIR` on `PATH` afterward.
  An existing suitable uv can be reused without writing its installation
  directory. Setup installs the checksummed uv 0.13.1 only when uv is missing
  or older than its minimum; a newer uv is kept.
- A usable Podman or Docker runtime, the backend dump utilities described
  below, and the native build prerequisites in [native setup](setup.md).
  Non-root accounts must arrange missing backend utilities with the owner;
  that step prints its apt command and does not use sudo.
- Network access to the [download hosts](#allowlist-domains) needed by any
  uncached step. In particular, model20 needs `downloads.sourceforge.net`
  and its selected `*.dl.sourceforge.net` redirect target. Network policy can
  refuse either host. Setup resolves model20 through the existing fetcher
  before image pulls or library builds when a requested cell needs data;
  verified cached data/model inputs avoid that download.

Measurement tasks (teardown, startup controller and cold mode) additionally
need an accessible, writable ext4 path for libraries, data, profiles, the
work directory and `TMPDIR`. Check each actual path with `findmnt -T <path>`;
the reported `FSTYPE` must be `ext4`. An ext4 mount elsewhere does not prove
that the measured files are on ext4. Some hosts have only overlay
and tmpfs paths. Without an ext4 path, measurement gates cannot run; move
measurement work to a suitable host. Non-measurement setup remains supported.
Setup reports accessible ext4 candidates, or explicitly none/unknown, without
failing setup or creating a filesystem. Verify all measurement paths yourself
even when it reports a candidate.

On a host lacking these capabilities, stop at the named prerequisite and
ask the environment owner to provide it, or move to a suitable host.
Do not route around network policy. The canonical compatibility container
can supply the reference/subject environment for compatibility work; it
does not establish the host Valgrind measurement gate. `--python-only`
has the narrower requirements documented below and needs no Cargo, Rust,
Valgrind, backend utilities or container runtime.

## Setup command

**When to run it.** Run the setup only when the work needs the pin oracle
(a differential, a same-run measurement against the pin, a data check);
work that does not touch the oracle does not need it. From the checkout:

```sh
tools/cloud/setup.sh
```

It is idempotent: every step checks its result and skips when it holds,
so a second run only re-verifies (under a second here). Options:
`--cache-dir DIR` (oracle prefixes, the model cache and the tkrzw build;
default `~/.cache/oxpinyin`), `--cells`, `--jobs`, `--no-pull` (build the
libraries even if the published image exists). `OXPINYIN_CONTAINER`
names the container command (see step 1). Progress goes to stderr;
stdout includes an informational ext4 path report and one line per cell:

```
bdb prefix=<dir> libpinyin=<sha256> libzhuyin=<sha256> data manifest OK
```

### Python-only tools.dev hosts

For a Linux x86_64 or arm64 host that only needs `tools.dev`, provision Python
without the oracle setup:

```sh
tools/cloud/setup.sh --python-only
export PATH="${UV_INSTALL_DIR:-$HOME/.local/bin}:$PATH"
uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev doctor
```

This mode runs the existing Linux and frozen-platform checks, then the
same uv and managed CPython (3.15 series, GIL build) provisioning and
interpreter verification as full setup. Neither is installed when it is
already present at or above its minimum; `just doctor` reports both. It
prints the verified-interpreter line and the informational ext4 path report,
logs that it stops after Python provisioning and exits successfully. It does
not select, start or pull a container runtime, install Valgrind or backend
utilities, provision oracle libraries or model data, or write the oracle
environment file. See [native setup](setup.md) for the interpreter policy;
on a host that has `just`, `just python` provisions the same interpreter.

`--python-only` cannot be combined with `--cells`, `--cache-dir`, `--jobs`,
`--no-pull` or `--in-container`; these combinations fail with exit 2 before
provisioning. For concurrent worktrees, `UV_INSTALL_DIR`,
`UV_PYTHON_INSTALL_DIR` and `UV_CACHE_DIR` can select task-owned uv,
interpreter and download-cache locations.

### The environment file

On success full setup writes `~/.oxpinyin-oracle.env` (`OXPINYIN_ORACLE_ENV`
overrides the path), rewritten on every run. Source it before a
runner:

```sh
. ~/.oxpinyin-oracle.env                       # reference cell, bdb
PINYIN_ORACLE_DBM=kc . ~/.oxpinyin-oracle.env  # another cell
```

A subset run, such as `--cells bdb`, keeps the other cells named by the
existing file when their saved prefixes still pass the library hashes in
`oracle-pin.txt` and the pinned data-manifest checks. A cell that fails
either check is removed with one diagnostic naming the cell and check.
The file contains the verified retained cells plus this run's cells, even
when they use different cache directories. A retained tkrzw cell also
keeps its local runtime paths. Setup publishes the complete file with an
atomic rename.

It exports, for each provisioned cell `<C>` in `BDB`, `KC`, `TKRZW`:
`OXPINYIN_ORACLE_PREFIX_<C>` (the oracle prefix) and
`OXPINYIN_ORACLE_DATA_<C>` (its `lib/libpinyin/data`); `PINYIN_MODEL_DIR`
(the extracted model20 cache); and, for the cell named by
`PINYIN_ORACLE_DBM` (the caller's value, else `bdb`, else the first
provisioned cell), `PINYIN_ORACLE_PREFIX` and `OXPINYIN_LIBPINYIN_DATA_DIR`,
recomputed each time the file is sourced. Naming a cell the setup did not
provision stops the `source` with an error. The file needs bash.

### Data verification

`Provision.data_ok` in `tools/dev/provision.py` checks the same data contract when setup
reuses a cache, verifies regenerated data, or retains an existing cell in
the environment file. It compares the SHA-256 of `oracle-data.sha256`
with `data_manifest_sha256_<cell>` in the repository's pin file and checks
every listed stable file. It also compares the SHA-256 of
`oracle-data-unstable.sha256` with `data_unstable_manifest_sha256` in that
prefix's own `oracle-pin.txt`, then checks every generated file it lists.
A missing or changed unstable manifest fails; a generated-file failure
names the file.

The unstable manifest covers `bigram.db`, `phrase_index.bin`,
`pinyin_index.bin`, `punct.bin` and both addon indexes. Their bytes differ
between builds, so this digest is checked against the prefix's local
record, never against the repository pin or another prefix.

For a read-only check of a selected prefix, load its matching setup
environment and run from the repository root:

```sh
just provision data-check "$PINYIN_ORACLE_DBM" "$PINYIN_ORACLE_PREFIX"
```

### Runtime enforcement (verification rule 9)

Any object that resolves `libtkrzw` must use the runtime verified by setup,
even when `PINYIN_ORACLE_DBM` is unset, `bdb` or `kc`.
`tools/tkrzw/runtime.py` contains the single Python loader check, shared by
provisioning, native gates and compatibility recipes. The reference images
retain their environment-only shell check and do not receive `tools.dev`.
The environment writer exports `OXPINYIN_TKRZW_LIBRARY`, the
exact canonical library verified by setup. A healthy host uses the existing
`probe_host` result; an unhealthy host uses the provisioned cache. An older
generated `OXPINYIN_TKRZW_PREFIX` or `OXPINYIN_TKRZW_PREFIXES` still selects
its cache object. No implicit home cache or arbitrary system prefix is
trusted. Select alternate caches through their generated environment files.

The check runs `ldd` on the exact object the driver opens, in the driver's
working directory and environment, before launching or comparing results.
These drivers introduce no other libtkrzw into their loader namespace before
`dlopen`, so the object's dependency search (including RPATH/RUNPATH and
relative loader paths) is the same. Linked executables are checked directly.
Resolved real paths must equal that exact verified object; a different path,
missing dependency, unconfigured runtime or failed resolver refuses the run.
The diagnostic names the runner, pin/subject side and path found. Every
label uses the full resolved `ldd` dependency set, including transitive and
preloaded libraries. Objects without tkrzw dependencies need no trusted
tkrzw runtime. For a dlopen driver the subject library is checked, and a
directly launched ELF executable is checked too. A launched `#!` script,
such as the POSIX shell `valgrind` wrapper Debian and Ubuntu install, is
checked through its interpreter, the ELF the kernel starts; `ldd` cannot
read the script itself. What the script execs is not inspected, so the
caller checks the subject it runs explicitly. Any other launched file is
checked as itself, so a file the resolver cannot read is refused.

The measurement images use the same shell check. Their entrypoints require
an explicitly supplied verified runtime; they do not infer trust from their
packaged `libtkrzw`. For example, after setup verifies a cache, mount that
runtime read-only and name its exact library in the container:

```sh
podman run --rm -v "$HOME/.cache/oxpinyin/tkrzw:/runtime:ro" \
    -e OXPINYIN_TKRZW_LIBRARY=/runtime/lib/libtkrzw.so.1 \
    -e LD_LIBRARY_PATH=/runtime/lib \
    -v "$OUT:/out" oxpinyin-callgrind
```

The same runtime mount and two environment assignments apply to
`oxpinyin-matrix` and its baseline/same-data runners. The mounted library
must match the image's architecture and ABI. Without a separately verified
runtime the image run refuses before measurement; installing uv does not
establish trust.

Keep the selected object's directory first in `LD_LIBRARY_PATH`, followed
by the inherited value. The remaining overwrites were classified as follows:

| Sites | Classification |
| --- | --- |
| `gcov_measure.py`: same-data and user-store environments | Defects; preserve inheritance and check the actual object/executable. |
| `measure callgrind-differential`: all five launch sites | Defects; use each selected object's directory, then inheritance. |
| `measure perf-baseline`, `perf-matrix`, `perf-same-data` | Defects, including reused stages and both taskset branches; preserve inheritance. |
| `measure shipped-check`, `tree-recipe-control` | Defects; use the selected object's directory, then inheritance. |
| `release-stage.sh` and `capture/run-capture.sh` | Additional defects; linked smoke/capture processes also need inheritance and the check. |
| `just runner cpp-smoke`: cleared-path `ldd` assertion | Intentional SONAME/RPATH test; the subsequent executable launch is checked with its actual environment. |
| `build-oracle.sh`: generation-prefix assignment | Intentional isolation in the pinned build recipe; not a differential launch. |
| `tools/dev/provision.py`: generated-path parsing and host-probe filtering; setup's cache-probe assignments | Intentional parsing without caller inheritance, or isolation of the host or selected cache while provisioning. |
| `tools/dev/compat/boundary.py`: removal of host loader variables | Intentional host/container boundary; the container supplies its own runtime. |

No listed remaining measurement overwrite is exempt as never-tkrzw.
`teardown.py` checks each copied object before every native, fixture-population
and profile-reader launch.
The shell surface runners, same-data/import/round-trip runners, profiling and
packaging launches, and Python contract/gcov/training/ordering tools share
the same implementation. A refusal is a failed run, never MATCH or SKIP.

Run loader fixtures with:

```sh
uv run --no-project --managed-python python -B -X lazy_imports=normal tools/tkrzw/runtime.test.py
```

## What it does

0. Bootstraps uv and managed CPython, checking the uv installation
   destination before its download. The Python provisioning service then
   checks Cargo and the repository toolchain and requires or installs
   Valgrind.
   Requires backend dump utilities for the requested cells: `db5.3_dump`
   (`db5.3-util`) and `kchashmgr` (`kyotocabinet-utils`). Root containers may
   install them with apt; ordinary accounts get an explicit missing-package
   error. Install them deliberately before running setup. Normal
   provisioning installs Valgrind idempotently when
   `valgrind --version` fails and prints its version in the summary. The
   `--in-container` behavioural path omits that requirement; see [benches](benches.md).
1. Resolves and prints the runtime/version before container provisioning. Platform order
   is Windows: wslc/Podman/Docker; macOS: Apple container/Podman/Docker;
   Linux: Podman/Docker. Explicit `OXPINYIN_CONTAINER` wins; no available candidate
   is an error. wslc is blocked as not yet verified, and the Apple adapter is
   not yet verified (use compat.sh rather than this Linux provisioner).
   Every container call uses the resolved runtime. Only the Docker path
   starts `dockerd` (detached, `setsid
   nohup dockerd`) unless `docker info` already answers, and waits for it.
   If a proxy certificate exists at
   `/root/.ccr/agent-proxy-ca.crt` (the path some hosted Linux environments
   use), this path installs it into the system
   trust store for the daemon's own proxy access; an ordinary proxy setting
   does not imply that this file exists. podman needs no daemon, no
   root and no `sudo`: `podman info` must answer, and the pulls, the
   `deps` build and the data runs are the same commands rootless.
2. Pulls the `base_image` tag of `tools/oracle/oracle-pin.txt`
   (`debian:trixie`, the current Debian stable codename) only when an
   uncached local fallback build needs it. Published artifacts/cached stages
   avoid that pull. Registry retries retain the existing 2, 4, 8 and 16 s
   backoff.
3. Gets the libraries for bdb, kc and tkrzw (the host platform, linux/amd64 or linux/arm64, whose
   frozen hashes the pin records). If
   `ghcr.io/shenghaoc/oxpinyin-oracle:<tag>` exists for amd64, or
   `<tag>-arm64` for arm64, with `<tag>` printed by
   `just compat image-tag` (pin, base image codename and a hash
   of the recipe and `oracle-pin.txt`), it is pulled and the cells are copied out of it. A cell
   whose pulled hashes do not match `oracle-pin.txt` is discarded and built
   instead, as is every cell when no image exists: the pinned image's
   `deps` stage is built and `tools/oracle/build-oracle.sh --libs-only
   --enable-libzhuyin --dbm <cell>` runs inside it, at the same in-image
   paths as the Dockerfile (`/build/<cell>`, `/opt/pinyin-oracle/<cell>`).
   The cells that need a build, and later the cells that need data, run at
   once in a bounded pool (#1233, `python -m tools.dev pool run`,
   `tools/dev/pool.py`). Each pool run holds the host heavy slot as `just heavy`
   does, inheriting `OXPINYIN_HEAVY_SLOT_HELD` when `setup.sh` itself runs
   under one, and divides the budget the slot admits: the available memory
   after other slots' reservations and the CPUs (capped by `--jobs`). Workers
   times make jobs stays within the CPUs, so nested `make -j` is counted; every
   overlapped build keeps at least two make jobs; each build is counted at 300
   MiB plus 275 MiB per make job (`docs/runbooks/oracle.md`); and unknown
   memory builds one cell at a time. Each container is held to its make jobs
   with `--cpus`; the memory figure sizes the overlap and is not a container
   limit. The `deps` image, the model, the hash check, step 3b and the report
   stay serial: the data step must not run on libraries that were not
   verified. CPU shares are per pool run, not reserved against other slot
   holders; memory is admitted host-wide. Each container is named with this
   invocation's random token (not the shell's PID), labelled with it, and its ID
   is recorded with `--cidfile`. A failed build, or SIGINT or SIGTERM, starts no
   further cell, stops the running containers and ends the run, and removes only
   containers whose recorded ID and label match this invocation; a `run` that
   never created a container leaves nothing to remove, and a same-named
   container this run did not create is never touched. Logs stay per cell in
   `<cache>/build-<cell>.log` and `<cache>/data-<cell>.log`.
   Whichever way the libraries arrived, the script then compares
   `libpinyin.so` and `libzhuyin.so` with `libpinyin_sha256_<cell>` and
   `libzhuyin_sha256_<cell>` in `oracle-pin.txt` and **stops with exit 3
   after printing both values** if any differs: a library that was *built*
   here and differs means the recipe is not reproducible on this host.
   3b. The tkrzw cell's host library. `tools/tkrzw/distro-probe.sh` is run
   against the host's `libtkrzw` (`tkrzw_dbm_util` on `PATH`). If it
   exits 0 nothing more happens. If it reports a defect, or finds no
   tkrzw, upstream tkrzw is built from source at
   `9ee84164cf45e441aa8186418c03a02c9b89f08b` (1.0.34: its sentinel fix
   makes defect 1 immune to LTO; 1.0.32 is healthy only without LTO,
   `docs/findings/tkrzw-distro-compat.md`) with `env -u CFLAGS -u CXXFLAGS
   -u LDFLAGS ./configure --prefix=<cache>/tkrzw && make && make install`,
   so no LTO or `-Bsymbolic-functions` can come in from the environment.
   The probe is then run against that build and the build is refused
   unless it prints `RESULT : healthy` and resolves to the cache. The
   environment file puts `<cache>/tkrzw` first on `PKG_CONFIG_PATH`,
   `LD_LIBRARY_PATH` and `PATH` (so `tkrzw_dbm_util` is the built one), and
   the script checks with `ldd` that the tkrzw oracle `libpinyin.so`
   resolves `libtkrzw.so.1` to `<cache>/tkrzw/lib`. A rerun skips the
   build when the cached revision is present and its probe passes.
4. Uses model20 fetched early with the model fetch (`tools/dev/model.py`) into
   `<cache>/model20`, runs `build-oracle.sh --regen-data` per cell inside
   the image, and re-verifies the installed files with the shared
   [data verification](#data-verification). The data is generated in a
   staging area and swapped
   in only if the regenerated stable manifest equals
   `data_manifest_sha256_<cell>`; otherwise the step fails and the prefix is
   untouched.
5. Rewrites the prefixes' `.pc` files to the host location, writes the
   environment file above and prints the per-cell lines.

### How containers get out

Processes in a container cannot reach the proxy at `127.0.0.1` and do not
trust its CA, so
every Docker/Podman run here uses `--network host`, passes `https_proxy` and
the CA (`GIT_SSL_CAINFO`, `SSL_CERT_FILE`, `CURL_CA_BUNDLE`) and mounts
`/root/.ccr/ca-bundle.crt` only when that bundle exists (or an explicit
`OXPINYIN_PROXY_CA` is supplied). The mount is a private copy of the selected
bundle (mode 0600, in the run's temporary directory, removed on exit) mounted
`:ro,z`; the selected file itself, including a host system CA file, is never
bind-mounted or relabelled. With `HTTPS_PROXY` set and that bundle
absent, step 0 uses the CA the host already trusts, in this order:
`SSL_CERT_FILE`, `CURL_CA_BUNDLE`, then the system bundle, and logs which one
(`proxy CA: ...`). When there is none, setup fails there, before any download
or build, and names `OXPINYIN_PROXY_CA`; it never disables verification.
The fallback proves only that a non-empty bundle is present. It does not show
that the proxy's interception certificate is trusted, so it does not prevent a
TLS failure; step 0 says so, and a failure there is resolved by setting
`OXPINYIN_PROXY_CA` to the proxy's CA bundle. No network probe checks it.
An ordinary HTTPS proxy needs no files under `/root/.ccr`. The `deps` build passes the proxy as a build
arg and the CA as the optional BuildKit secret `cacert`
(`tools/oracle/Dockerfile.oracle`); with the secret, apt trusts that CA for
any https source. The base image keeps its default Debian mirrors either
way, so the installed packages, hence the toolchain, are the same.

## Allowlist domains

The host's network policy must allow at least the hosts below. Hosts the
setup contacts, and what one host showed on 2026-10-16 UTC:

| Host | Used for | Seen |
| --- | --- | --- |
| `registry-1.docker.io`, `auth.docker.io`, `production.cloudflare.docker.com` | the `debian:trixie` base image | reachable (429 under load; retried) |
| `snapshot.debian.org` | the apt snapshot the toolchain was installed from | reachable once allowed (403 before) |
| `github.com` | libpinyin at the pinned commit (`git fetch --depth=1 <url> <sha>`) | reachable |
| `github.com`, `release-assets.githubusercontent.com` | uv fallback archive and uv-managed CPython release assets | required when uncached; no fresh download measured for #1055 |
| `downloads.sourceforge.net` and its redirect target, a rotating `*.dl.sourceforge.net` mirror | model20 archive | reachable (redirected to `cytranet-dal.dl.sourceforge.net`) |
| `ghcr.io` (and the blob host it redirects to) | the published oracle image, when it exists | token endpoint answers; no image published yet, blob host untested |
| `index.crates.io`, `static.crates.io` | cargo | reachable |

The reachable entries are historical observations, not guarantees for another
host. The #1055 report records a host refusing
`downloads.sourceforge.net` on 2026-10-22 UTC. Request the required allowlist
change from the environment owner or use an already verified local cache;
model20 must not be mirrored or published by the project.

**Current package mirror.** The trixie toolchain installs from
`deb.debian.org`, the Debian stable package mirror, which the host's network
policy has to allow; setup no longer contacts `snapshot.debian.org`. No
host has been recorded reaching `deb.debian.org`, so that access is
unverified. Observed on a remote host on 2026-10-25 UTC: `deb.debian.org`
was blocked and `snapshot.debian.org` was reachable. The host's network
policy therefore has to allow `deb.debian.org` for the trixie image builds;
reaching `snapshot.debian.org` does not substitute for it. `--regen-data` compiles the pin's data build, so both the library
build and the data step need the `deps` image. The normal image workflow now
also publishes that exact `deps` stage as
`ghcr.io/shenghaoc/oxpinyin-oracle:<oracle-tag>-deps` on amd64 and
`<oracle-tag>-deps-arm64` on arm64. Setup tries the host platform's tag
before building locally, so a successful pull removes the cold package-install
requirement. If unavailable, the local `deps` build remains the fallback; its
pull stderr is retained in `<cache>/pull-deps.err`. Anonymous access to this
newly published tag must be checked after normal publication. Historical
measurements below predate this optimization.

## Time and disk use

A cold full setup on a 4-core x86_64 host took 5 to 10 minutes: about 4.5
minutes when the published oracle image was pulled and the host's tkrzw
passed the probe, and up to 10 minutes when the libraries, the `deps` image
and tkrzw were all built locally. A rerun with everything present only
re-verifies and finishes in a few seconds. A full setup adds about 2 GB:
the model20 cache (about 105 MB), the three oracle prefixes (about 195 MB
together, with data) and the container images and build cache (about 1 GB
each, sharing layers). `docker builder prune -af` after setup drops the
build-cache copy. These figures predate the move to the trixie base image.

### tkrzw step and the CI child

CI's tkrzw jobs use the `libtkrzw` that `debian:testing` ships, built
without LTO: 1.0.34-1 on 2026-10-23 UTC (`apt-cache policy libtkrzw-dev` in
that image; it is unpinned, so the version moves). Setup builds
upstream 1.0.34 (`9ee8416`) when the host's library fails the probe. Both
pass `tools/tkrzw/distro-probe.sh` (`RESULT : healthy`), which is the
criterion the setup enforces. With no setup environment in CI, the
`tkrzw-sanitizers` job runs that probe through `probe_host` before its gate.

### When the published image cannot be pulled

`setup.sh` keeps the pull's stderr in `<cache>/pull-published.err`, logs its
last five lines, and says which case it is before building the libraries:
`manifest unknown / not found` (the tag is not published) or `pull FAILED
(network, auth or proxy, not "not found")` for anything else. Both fall
back to the build.

## Gates on a remote host

On a host meeting the requirements above, the
[verification procedure](verification.md) runs as on any Linux host; the
cells are the prefixes setup prints. Do not assume the host already has
Cargo, the repository Rust toolchain, Valgrind or a native compiler.
Same-run callgrind measurements ([benches](benches.md)) need those host
tools and the oracle for the pin side. A main-against-main control (two
builds of one commit, measured in one run, spread 0) needs only the Rust
side.

## Failure modes

- An unwritable uv destination: setup exits 1 with the prerequisite and
  next action before any download.
- Missing Cargo/toolchain, or Valgrind without root/sudo: setup exits 1 with
  the prerequisite and next action after uv and managed-Python bootstrap,
  but before oracle downloads. `--python-only` omits the Cargo/toolchain
  and Valgrind checks.
- #1024 reported a read-only `~/.local/bin` on a remote image, then
  successful pinned uv/managed-CPython provisioning using private
  overrides; full setup next stopped on missing Valgrind/sudo, and Cargo
  was absent. That image is unavailable locally. The OS read-only
  mount was not reproduced on the Ubuntu maintainer host; synthetic PATH
  stubs and an unwritable uv directory cover the failure paths without
  installing prerequisites on that host.
- `429 Too Many Requests` from the registry: the script retries; rerun it.
- `403` from the proxy: the host is not allowed for the environment. Do
  not route around it; add the host (table above) to Allowed domains.
- `MISMATCH` and exit 3: the libraries built here do not hash to
  `oracle-pin.txt`. They are left in the oracle root; compare them with
  `readelf -S -W` and `cmp -l` against a reference build and report the
  difference. Do not edit the recorded values to match.

### Provisioning implementation and fixture checks

`tools/cloud/setup.sh` retains the pre-interpreter uv/bootstrap step and then
calls the single `tools.dev provision` service. `just provision` uses an already
provisioned interpreter. The function-order policy action continues to call
`tools/cloud/setup.sh --python-only` unchanged.

Run the synthetic provisioning fixtures through normal Python discovery:

```sh
just test-tools tools/dev/provision.test.py
```

Fixtures check the setup entry point with fake host tools, pins, data, proxy CA
and generated environment paths. Missing option values return status 1 with an
`--option: requires an argument` diagnostic. On a fresh host,
uv/interpreter bootstrap precedes Python-owned native prerequisite checks.
