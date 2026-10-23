# Cloud sessions — provisioning an agent-platform cloud host

Cloud sessions meeting the host requirements below can run every gate.
Provision one with `tools/cloud/setup.sh`; this route includes Valgrind for
measurement tasks. A cloud session is not automatically provisionable just
because it is supplied by an agent platform. Ordinary native development
uses [native setup](setup.md) and does not require it. The
[canonical compatibility container](compatibility-container.md) is a second
supported route, not a replacement for cloud provisioning.

Agent-platform cloud hosts may supply root containers, a container runtime,
and an egress proxy. The provisioner handles those capabilities, scopes custom
trust to existing session certificates or an explicit CA path, and verifies
the same frozen oracle/data contracts. It does not require a particular IDE.
Use task-owned `OXPINYIN_ORACLE_ENV` and `--cache-dir` paths for concurrent
worktrees. `--in-container` is the behavioural adapter for the second route;
it neither installs nor requires Valgrind.

## Host requirements

Full setup requires Linux x86_64. This is a limit of `tools/cloud/setup.sh`.
The script also requires:

- Cargo on `PATH`, rustup and the installed toolchain/components selected by
  `rust-toolchain.toml`. Setup checks these before provisioning; it does not
  install Rust. Follow [native setup](setup.md) when they are missing.
- A Valgrind at or above its minimum (`just doctor` shows it), or root/sudo
  access plus `apt-get` to install or upgrade it. Root uses apt directly; a
  non-root account with sudo retains the sudo install path. Without either
  privilege route, setup prints `run as root: apt-get install -y valgrind` and
  a specific missing- or outdated-Valgrind failure before Python downloads.
  Ask the environment owner to install it or choose a host with a suitable
  Valgrind already available.
- A writable uv installation directory when uv needs installing or replacing
  (default `$HOME/.local/bin`), plus writable managed-Python/cache locations.
  Setup probes the uv destination before downloading. If a cloud image mounts
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
  and its selected `*.dl.sourceforge.net` redirect target. Cloud policy can
  refuse either host. Setup resolves model20 through the existing fetcher
  before image pulls or library builds when a requested cell needs data;
  verified cached data/model inputs avoid that download.

On a host lacking these capabilities, stop at the named prerequisite and
ask the environment owner to provide it, or move to a suitable cloud host.
Do not route around network policy. The canonical compatibility container
can supply the reference/subject environment for compatibility work; it
does not establish the host Valgrind measurement gate. `--python-only`
has the narrower requirements documented below and needs no Cargo, Rust,
Valgrind, backend utilities or container runtime.

## Setup command

**When to run it.** The environment's setup script stays empty. A lane runs
the setup only when its task needs the pin oracle (a differential, a
same-run measurement against the pin, a data check); a lane that does not
touch the oracle never pays for it. From the checkout:

```sh
tools/cloud/setup.sh
```

It is idempotent: every step checks its result and skips when it holds,
so a second run only re-verifies (under a second here). Options:
`--cache-dir DIR` (oracle prefixes, the model cache and the tkrzw build;
default `~/.cache/oxpinyin`), `--cells`, `--jobs`, `--no-pull` (build the
libraries even if the published image exists). `OXPINYIN_CONTAINER`
names the container command (see step 1). Progress goes to stderr;
stdout is one line per cell:

```
bdb prefix=<dir> libpinyin=<sha256> libzhuyin=<sha256> data manifest OK
```

### Python-only tools.dev sessions

For a Linux x86_64 session that only needs `tools.dev`, provision Python
without the oracle setup:

```sh
tools/cloud/setup.sh --python-only
export PATH="${UV_INSTALL_DIR:-$HOME/.local/bin}:$PATH"
uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev doctor
```

This mode runs the existing argument and Linux x86_64 checks, then the
same uv and managed CPython (3.15 series, GIL build) provisioning and
interpreter verification as full setup. Neither is installed when it is
already present at or above its minimum; `just doctor` reports both. It
prints the verified-interpreter line, logs that it stops after Python
provisioning and exits successfully. It does
not select, start or pull a container runtime, install Valgrind or backend
utilities, provision oracle libraries or model data, or write the oracle
environment file. See [native setup](setup.md) for the interpreter policy.

`--python-only` cannot be combined with `--cells`, `--cache-dir`, `--jobs`,
`--no-pull` or `--in-container`; these combinations fail with exit 2 before
provisioning. For concurrent worktrees, `UV_INSTALL_DIR`,
`UV_PYTHON_INSTALL_DIR` and `UV_CACHE_DIR` can select task-owned uv,
interpreter and download-cache locations.

### The environment file

On success full setup writes `~/.oxpinyin-oracle.env` (`OXPINYIN_ORACLE_ENV`
overrides the path), rewritten on every run. A lane sources it before a
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

`data_ok` in `tools/cloud/env.sh` checks the same data contract when setup
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
(
    source tools/cloud/env.sh
    pin=tools/oracle/oracle-pin.txt
    pinval() { sed -n "s/^$1=//p" "$pin"; }
    data_ok "$PINYIN_ORACLE_DBM" "$PINYIN_ORACLE_PREFIX"
)
```

### Runtime enforcement (verification rule 9)

Any object that resolves `libtkrzw` must use the runtime verified by setup,
even when `PINYIN_ORACLE_DBM` is unset, `bdb` or `kc`.
`tools/tkrzw/runtime.sh` contains the single check, using bash, `ldd`, awk
and readlink; `runtime.py` is only an adapter for Python tools. Native
runners and the callgrind/perf-matrix images need no uv or Python to check
the runtime. The environment writer exports `OXPINYIN_TKRZW_LIBRARY`, the
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
directly launched ELF executable is checked too.

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
| `run-callgrind-differential.sh`: all five launch sites | Defects; use each selected object's directory, then inheritance. |
| `run-perf-baseline.sh`, `run-perf-matrix.sh`, `run-perf-same-data.sh` | Defects, including reused stages and both taskset branches; preserve inheritance. |
| `run-shipped-check.sh`, `run-tree-recipe-control.sh` | Defects; use the selected object's directory, then inheritance. |
| `release-stage.sh` and `capture/run-capture.sh` | Additional defects; linked smoke/capture processes also need inheritance and the check. |
| `run-cpp-smoke.sh`: cleared-path `ldd` assertion | Intentional SONAME/RPATH test; the subsequent executable launch is checked with its actual environment. |
| `build-oracle.sh`: generation-prefix assignment | Intentional isolation in the pinned build recipe; not a differential launch. |
| `cloud/env.sh`: generated-path parsing and host-probe filtering; setup's cache-probe assignments | Intentional parsing without caller inheritance, or isolation of the host or selected cache while provisioning. |
| `tools/dev/compat/boundary.py`: removal of host loader variables | Intentional host/container boundary; the container supplies its own runtime. |

No listed remaining measurement overwrite is exempt as never-tkrzw.
`teardown.py` is explicitly BDB-only in `run`; it needs no tkrzw guard.
The shell surface runners, same-data/import/round-trip runners, profiling and
packaging launches, and Python contract/gcov/training/ordering tools share
the same implementation. A refusal is a failed run, never MATCH or SKIP.

Run loader fixtures with:

```sh
uv run --no-project --managed-python --python cpython@3.15.0+gil python -B -X lazy_imports=normal tools/tkrzw/runtime.test.py
```

## What it does

0. Checks Cargo and the repository toolchain, and requires or installs
   Valgrind before Python provisioning. Then provisions uv and managed
   CPython, checking the uv installation destination before its download.
   Requires backend dump utilities for the requested cells: `db5.3_dump`
   (`db5.3-util`) and `kchashmgr` (`kyotocabinet-utils`). Root containers may
   install them with apt; ordinary accounts get an explicit missing-package
   error. Install them deliberately before running setup. Normal cloud
   provisioning installs Valgrind idempotently when
   `valgrind --version` fails and prints its version in the summary. The
   `--in-container` behavioural path omits that requirement; see [benches](benches.md).
1. Resolves and prints the runtime/version before container provisioning. Platform order
   is Windows: wslc/Podman/Docker; macOS: Apple container/Podman/Docker;
   Linux: Podman/Docker. Explicit `OXPINYIN_CONTAINER` wins; no available candidate
   is an error. wslc is blocked as not yet verified, and the Apple adapter is
   not yet verified (use compat.sh rather than the Linux cloud provisioner).
   Every container call uses the resolved runtime. Only the Docker path
   starts `dockerd` (detached, `setsid
   nohup dockerd`) unless `docker info` already answers, and waits for it.
   If the session certificate
   `/root/.ccr/agent-proxy-ca.crt` exists, this path installs it into the system
   trust store for the daemon's own proxy access; an ordinary proxy setting
   does not imply that this file exists. podman needs no daemon, no
   root and no `sudo`: `podman info` must answer, and the pulls, the
   `deps` build and the data runs are the same commands rootless.
2. Pulls the `base_image` digest of `tools/oracle/oracle-pin.txt`
   (`debian:testing@sha256:dab11cdb…`) only when an uncached local fallback
   build needs it. Published artifacts/cached stages avoid that pull. Registry
   retries retain the existing 2, 4, 8 and 16 s backoff.
3. Gets the libraries for bdb, kc and tkrzw (linux/amd64, the platform
   the recorded hashes are for). If
   `ghcr.io/shenghaoc/oxpinyin-oracle:<tag>` exists, with `<tag>` printed by
   `tools/oracle/image-tag.sh` (pin, base digest, apt snapshot and a hash
   of the recipe), it is pulled and the cells are copied out of it. A cell
   whose pulled hashes do not match `oracle-pin.txt` is discarded and built
   instead, as is every cell when no image exists: the pinned image's
   `deps` stage is built and `tools/oracle/build-oracle.sh --libs-only
   --enable-libzhuyin --dbm <cell>` runs inside it, at the same in-image
   paths as the Dockerfile (`/build/<cell>`, `/opt/pinyin-oracle/<cell>`).
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
4. Uses model20 fetched early with `tools/model/fetch-model.sh` into
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
trust its CA (`/root/.ccr/README.md`, "docker build / docker run"), so
every Docker/Podman run here uses `--network host`, passes `https_proxy` and
the CA (`GIT_SSL_CAINFO`, `SSL_CERT_FILE`, `CURL_CA_BUNDLE`) and mounts
`/root/.ccr/ca-bundle.crt` only when that bundle exists (or an explicit
`OXPINYIN_PROXY_CA` is supplied). An ordinary HTTPS proxy does not require
Claude files. The `deps` build passes the proxy as a build
arg and the CA as the optional BuildKit secret `cacert`
(`tools/oracle/Dockerfile.oracle`); with the secret, apt reads the same
dated snapshot over https instead of http. Without it (CI, a workstation)
nothing changes, and the installed packages, hence the toolchain, are the
same.

## Allowlist domains

Network access must be Limited-with-allowed-domains or broader. Hosts the
setup contacts, and what a session showed on 2026-10-16 UTC:

| Host | Used for | Seen |
| --- | --- | --- |
| `registry-1.docker.io`, `auth.docker.io`, `production.cloudflare.docker.com` | the pinned `debian:testing` digest | reachable (429 under load; retried) |
| `snapshot.debian.org` | the apt snapshot the toolchain is installed from | reachable once allowed (403 before) |
| `github.com` | libpinyin at the pinned commit (`git fetch --depth=1 <url> <sha>`) | reachable |
| `github.com`, `release-assets.githubusercontent.com` | uv fallback archive and uv-managed CPython release assets | required when uncached; no fresh download measured for #1055 |
| `downloads.sourceforge.net` and its redirect target, a rotating `*.dl.sourceforge.net` mirror | model20 archive | reachable (redirected to `cytranet-dal.dl.sourceforge.net`) |
| `ghcr.io` (and the blob host it redirects to) | the published oracle image, when it exists | token endpoint answers; no image published yet, blob host untested |
| `index.crates.io`, `static.crates.io` | cargo | reachable |

The reachable entries are historical observations, not guarantees for another
session. The #1055 report records a cloud session refusing
`downloads.sourceforge.net` on 2026-10-22 UTC. Request the required allowlist
change from the environment owner or use an already verified local cache;
model20 must not be mirrored or published by the project.

`snapshot.debian.org` is the one that is easy to miss: the pinned
toolchain is installed from it, and `--regen-data` compiles the pin's data
build, so both the library build and the data step need the `deps` image.
The normal image workflow now also publishes that exact `deps` stage as
`ghcr.io/shenghaoc/oxpinyin-oracle:<oracle-tag>-deps`. Setup tries it before
building locally, so a successful pull removes the snapshot cold-start
requirement. If unavailable, the existing snapshot build remains the fallback;
its pull stderr is retained in `<cache>/pull-deps.err`. Anonymous access to
this newly published tag must be checked after normal publication. Historical
measurements below predate this optimization.

## Setup time and disk use

Measured on 2026-10-16 UTC in a 4-core, 15 GiB cloud session (x86_64,
`/dev/vda`, `docker` 29.8.2). No oracle image was reachable in that run, so
only the build path ("without the published image") was measured here; the
"with" column is measured end to end in "With the published image, from
scratch" below, not step by step.

**Starting state of the from-scratch figures.** Docker emptied and stopped,
and the cache removed, then `df` read before and after:

```sh
docker rm -f $(docker ps -aq); docker system prune -af; docker builder prune -af
pkill -x dockerd; sleep 3; rm -rf /var/lib/docker/* ~/.cache/oxpinyin
df --output=used -k / | tail -1        # before: 12716192
SECONDS=0; tools/cloud/setup.sh > /dev/null 2> setup.err
echo "total=${SECONDS}s"               # bash's elapsed seconds: 353
df --output=used -k / | tail -1        # after: 14773784
```

Each step's time is the difference between consecutive `[ Ns ]` offsets
that `setup.sh` prints on stderr (the offset is bash's `$SECONDS` since the
script started); a step ends where the next one starts.

| Step (stderr offsets) | Without the published image | With the published image |
| --- | --- | --- |
| `dockerd` start and pinned-digest pull (`0` to `6`) | 6 s | same |
| `deps` image build (`6` to `68`) | 62 s | same (the data step needs it) |
| libraries bdb, kc, tkrzw (`68` to `96`, `96` to `148`, `148` to `180`) | 28 s, 52 s, 32 s (112 s) | not measured |
| model20 fetch, once, and `--regen-data` bdb, kc, tkrzw (`180` to `224`, `224` to `301`, `301` to `353`) | 44 s, 77 s, 52 s (173 s) | not measured |
| setup, end to end (`total=` above) | **353 s** | not measured |

The model fetch (`tools/model/fetch-model.sh --cache-dir
~/.cache/oxpinyin/model20`) runs inside the bdb data step and is not timed
on its own.

**Second run, everything present.** Run immediately after the first, with
nothing removed: `time tools/cloud/setup.sh > /dev/null 2>&1` reported
`real 0m0.332s`.

**Disk.** `df` used space rose by 2,057,592 KiB (about 2.0 GB: 14773784
minus 12716192 above). Its parts, read after the run:

```sh
du -sh ~/.cache/oxpinyin/model20                  # 105M (archive and extraction)
du -sh ~/.cache/oxpinyin/oracle/{bdb,kc,tkrzw}    # 63M, 78M, 54M (193M together, with data)
docker system df                                  # images 1.03GB, build cache 1.028GB
```

The build cache shares its layers with the images (`docker system df`
reports 12.29 kB of it reclaimable), so the two Docker figures are not
additive. `docker builder prune -af` after the setup drops the cache copy.

### tkrzw step and the CI child

CI's tkrzw jobs use the `libtkrzw` that `debian:testing` ships, built
without LTO: 1.0.34-1 on 2026-10-23 UTC (`apt-cache policy libtkrzw-dev` in
that image; it is unpinned, so the version moves). The cloud setup builds
upstream 1.0.34 (`9ee8416`) when the host's library fails the probe. Both
pass `tools/tkrzw/distro-probe.sh` (`RESULT : healthy`), which is the
criterion the setup enforces. With no setup environment in CI, the
`tkrzw-sanitizers` job runs that probe through `probe_host` before its gate.

### Rootless podman on a workstation

Measured 2026-10-16 UTC on the maintainer's own host (Ubuntu 26.10, 12
cores, 15 GiB, podman 5.8.6 rootless, no `sudo`, no Docker; the host's
`libtkrzw` 1.0.32 is Ubuntu's and fails the probe). Not a cloud session, so
these figures are **not** the cloud figures above. The podman image store
already held the pinned base image and the `deps` image, so neither was
pulled or built; the cache directory was new and empty. `db5.3_dump` and
`kchashmgr` were unpacked from the distribution `.deb`s onto `PATH`
(`apt-get download`, `dpkg -x`) because the host user cannot install
packages. The published oracle image existed on this host's registry
path and was pulled, so the libraries came from it, not from a build.

`SECONDS` offsets from the stderr of `tools/cloud/setup.sh --cache-dir
<new>`:

| Step | Seconds |
| --- | --- |
| podman check, base image present (`0` to `1`) | 1 |
| published image pull, three cells copied out, hashes match (`1` to `7`) | 6 |
| tkrzw: probe, source fetch, configure, `make -j12`, install, probe, `ldd` (`7` to `45`) | 38 |
| model20 fetch and `--regen-data` bdb, kc, tkrzw (`45` to `114`, `114` to `205`, `205` to `279`) | 69 (including the 20 s fetch), 91, 74 |
| setup, end to end (`time`) | **278.6 s** |

Rerun with nothing removed: `real 0m1.959s`; it re-probes the host
library, probes the cached build, and re-runs the `ldd` check. The cached
build is `120M`.

After sourcing the generated environment file, the tkrzw oracle
`libpinyin.so`, the `oxpinyin-capi` tkrzw `libpinyin_capi.so` and an
`oxpinyin-store` test binary built with `--features tkrzw`
(`cargo test -p oxpinyin-store --no-default-features --features tkrzw
--no-run`) all resolve `libtkrzw.so.1` to `<cache>/tkrzw/lib`; the store
test binary also carries that directory as its `RUNPATH`. On this host the
`issue784-index-keys-*` contract-diff cases (26, pinyin and zhuyin) ran
`MATCH` 26 of 26 on bdb, kc and tkrzw.

### This change in a Claude Code cloud session

Measured 2026-10-16 UTC in a Claude Code cloud session with this change at
`c270ad8`: 4 cores, x86_64, root, `docker` 29.8.2, no `podman`. This is the
session's own host, not the maintainer's workstation above, and not the
earlier 15 GiB session in "Setup time and disk use". `~/.cache/oxpinyin`
did not exist before the first run.

**Image and tkrzw.** In this run no oracle image was found at the tag the script
tries (`ghcr.io/shenghaoc/oxpinyin-oracle:074a2219c90f-…`), so the libraries
were **built**, not pulled, and the `deps` image was built. The host
`libtkrzw` failed the probe, so the **tkrzw source build ran** (upstream
`9ee8416`). `db5.3-util` and `kyotocabinet-utils` were installed with
`apt-get` (root).

`SECONDS` offsets from the stderr of `bash tools/cloud/setup.sh`:

| Step | Seconds |
| --- | --- |
| dump utilities (`1` to `4`, including the `dockerd` start) | 3 |
| base image pull and the published-image attempt (`4` to `8`) | 4 |
| `deps` image build (`8` to `94`) | 86 |
| libraries bdb, kc, tkrzw (`94` to `142`, `142` to `212`, `212` to `267`) | 48, 70, 55 |
| tkrzw: probe, source build, install, probe, `ldd` (`267` to `329`) | 62 |
| model20 fetch and `--regen-data` bdb, kc, tkrzw (`329` to `398`, `398` to `516`, `516` to `596`) | 69, 118, 80 |
| setup, end to end (`time`) | **595.98 s** |

Rerun with nothing removed: `real 0m1.164s`.

**Capi and contract diff.** `oxpinyin-capi` and `oxpinyin-zhuyin-capi` debug
builds, one target directory per cell (`--no-default-features --features
bdb|kyotocabinet|tkrzw`), then `tools/bisection/run-contract-diff.sh` with
`--cases` set to the 26 `issue784-index-keys-*` cases (pinyin and zhuyin),
after `source ~/.oxpinyin-oracle.env`:

| Cell | Capi build (`oxpinyin-capi`) | `issue784-index-keys-*` |
| --- | --- | --- |
| bdb | 24.3 s | 26 of 26 `MATCH` |
| kc | 20.0 s | 26 of 26 `MATCH` |
| tkrzw | 21.1 s | 26 of 26 `MATCH` |

`ldd` on the tkrzw `libpinyin_capi.so` resolves `libtkrzw.so.1` to
`~/.cache/oxpinyin/tkrzw/lib/libtkrzw.so.1`; the bdb and kc ones resolve
`libdb-5.3.so` and `libkyotocabinet.so.16` from `/lib/x86_64-linux-gnu`.
`issue785` has no cases of its own in `contract-diff.py`; its mask behaviour
is inside the 784 mask cases.

### With the published image, from scratch

Measured 2026-10-16 UTC on the same 4-core cloud host as above, after the
session's network policy allowed `ghcr.io` and
`pkg-containers.githubusercontent.com`. The container was restarted, so
`db5.3-util` and `kyotocabinet-utils` were still installed from the earlier
run (no install step), and `~/.cache/oxpinyin` and `/var/lib/docker` were
emptied before the run, so everything else started from nothing.

The published oracle image **was pulled** (`ghcr.io/shenghaoc/oxpinyin-oracle:074a2219c90f-…`),
and all three cells' libraries matched `oracle-pin.txt`, so no library was
built. The tkrzw source build still ran (the host library failed the probe),
and the `deps` image was still built, because `--regen-data` needs it.

`SECONDS` offsets from the stderr of `bash tools/cloud/setup.sh`:

| Step | Seconds |
| --- | --- |
| `dockerd` start and pinned-digest pull, two retries (`1` to `17`) | 16 |
| published image pull, three cells copied out, hashes match (`17` to `21`) | 4 |
| tkrzw: probe, source build, install, probe, `ldd` (`21` to `86`) | 65 |
| `deps` image build (starts at `89`) and model20 fetch, inside the bdb data step (`86` to `244`) | 158 for the step |
| `--regen-data` kc, tkrzw (`244` to `359`, `359` to `441`) | 115, 82 |
| setup, end to end (`time`) | **440.71 s** |

Against the 595.98 s build-only run above, the image saves the three
library builds (173 s there) and adds a 4 s pull; the other steps differ
between runs by amounts the two runs do not separate.

### When the published image cannot be pulled

`setup.sh` keeps the pull's stderr in `<cache>/pull-published.err`, logs its
last five lines, and says which case it is before building the libraries:
`manifest unknown / not found` (the tag is not published) or `pull FAILED
(network, auth or proxy, not "not found")` for anything else. Both fall
back to the build.

### Not measured here

Disk use in this session and whether `podman info` or `docker info` works
in a Codex cloud environment were not measured.

## Gates in a cloud session

On a host meeting the requirements above, `AGENTS.md` ("Verification cells")
runs as on any Linux host; the cells are the prefixes setup prints.
Do not assume a session already carries Cargo, the repository Rust toolchain,
Valgrind or a native compiler. Same-run callgrind measurements (`benches.md`)
require those host tools and the oracle for the pin side.
A main-against-main control (two builds of one commit, measured in
one run, spread 0) needs only the Rust side.

## Failure modes

- Missing Cargo/toolchain, Valgrind without root/sudo, or an unwritable uv
  destination: setup exits 1 with the prerequisite and next action before
  downloads. `--python-only` omits the Cargo/toolchain and Valgrind checks.
- The #1024 lane reported an OS read-only `/home/agent/.local/bin`, then
  successful pinned uv/managed-CPython provisioning using task-private
  overrides; full setup next stopped on missing Valgrind/sudo, and Cargo
  was absent. That cloud image is unavailable locally. The OS read-only
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
