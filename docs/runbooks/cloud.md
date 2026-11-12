# Remote Linux development

This procedure provisions a remote Linux host so that every gate can run there.
Pin-comparison campaigns use the local oracle `performance` stage; see
[where measurements run](benches.md#where-measurements-run).

A remote Linux host that meets the requirements below can run every
non-measurement gate. Measurement gates also need the ext4 paths below.
Provision the host with `tools.dev.provision` (the [setup command](#setup-command)),
which also installs Valgrind for measurement work. Ordinary native development uses [native setup](setup.md)
and does not need a remote host.

The host may be a root container, may supply its own container runtime, and
may reach the network only through an HTTPS proxy. The provisioner handles
each case. It limits custom trust to an existing proxy certificate or an
explicit CA path. It verifies the current pin/recipe provenance and the recorded build/data bytes.
No particular editor or IDE is required. When several worktrees share a host, give each worktree its own
`OXPINYIN_ORACLE_ENV` files. Worktrees share `OXPINYIN_CACHE` by default.

## Host requirements

Full setup requires Linux on a supported native platform:
linux/amd64 or linux/arm64. `tools.dev.provision` selects the platform from
the machine architecture and refuses any other.
It also requires:

- Cargo on `PATH`, rustup, and the installed toolchain and components that
  `rust-toolchain.toml` selects. Setup checks these before provisioning. It does not
  install Rust. When they are missing, follow [native setup](setup.md).
- A Valgrind at or above its minimum (`just doctor` shows it), or root/sudo
  access and `apt-get` to install or upgrade it. Root uses apt directly. A
  non-root account with sudo keeps the sudo install path. Without either
  privilege route, setup prints `run as root: apt-get install -y valgrind` and
  a specific missing- or outdated-Valgrind failure before the oracle downloads.
  Ask the environment owner to install it, or choose a host that already has a suitable
  Valgrind.
- uv on `PATH`. If `uv --version` fails, or uv refuses to run because it is
  older than the `required-version` in `uv.toml`, run uv's official
  [standalone installer](https://docs.astral.sh/uv/getting-started/installation/#standalone-installer):
  `curl -LsSf https://astral.sh/uv/install.sh | sh` on macOS and Linux, or
  `powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex"`
  on Windows. Then put its install directory first on `PATH`, ahead of any older
  uv: `~/.local/bin`, or `%USERPROFILE%\.local\bin` on Windows. When `HOME` is
  read-only, set `UV_INSTALL_DIR` to a writable directory before you run the
  installer, and put that directory first on `PATH` instead. If astral.sh is
  blocked, stop and report it; most environments already provide a uv that
  meets the floor.
  Provisioning does not install uv: it starts through `uv run`, so a uv below the
  floor refuses before provisioning runs.
  [Installing uv and cargo-c](setup.md#installing-uv-and-cargo-c) has the details.
  Setup keeps just, cargo-nextest and cargo-c when they meet their floors and
  installs the pinned fallbacks otherwise. It needs
  writable managed-Python and cache locations. Cargo tool provisioning needs
  `index.crates.io` and `static.crates.io`.
  Setup adds the toolchain's `rust-analyzer` component with `rustup component add`
  only when it is missing. CI does not install it.
- A usable Podman or Docker runtime, the backend dump utilities described
  below, and the native build prerequisites in [native setup](setup.md).
  Non-root accounts must arrange missing backend utilities with the owner.
  That step prints its apt command and does not use sudo.
- Network access to the [download hosts](#allowlist-domains) that any
  uncached step needs. In particular, model20 needs `downloads.sourceforge.net`
  and the `*.dl.sourceforge.net` redirect target that it selects. Network policy can
  refuse either host. When a requested cell needs data, setup resolves model20 through the existing fetcher
  before image pulls or library builds.
  Verified cached data and model inputs avoid that download.

Measurement tasks also need an accessible, writable ext4 path for libraries, data, profiles, the
work directory and `TMPDIR`:

1. Check each actual path with `findmnt -T <path>`.
2. Make sure that the reported `FSTYPE` is `ext4`. An ext4 mount elsewhere does not prove
   that the measured files are on ext4.

Some hosts have only overlay
and tmpfs paths. Without an ext4 path, measurement gates cannot run; move
measurement work to a suitable host. Non-measurement setup stays supported.
Setup reports accessible ext4 candidates, or explicitly none/unknown. It does not
fail setup or create a filesystem. Verify all measurement paths yourself,
also when setup reports a candidate.

On a host that does not have these capabilities, stop at the named prerequisite.
Ask the environment owner to provide it, or move to a suitable host.
Do not route around network policy. `--python-only`
has the narrower requirements documented below. It needs no Cargo, Rust,
Valgrind, backend utilities or container runtime. `--dev-tools` adds the
development tools to that mode. It needs Cargo or rustup only for a tool that
it must install.

## Setup command

Before workspace tests in a cloud session, install the tkrzw vcpkg overlay
and export its compilation and runtime paths as described in
[Linux setup](setup.md#linux). The full oracle setup below also installs the
overlay and writes those exports. Set `VCPKG_ROOT` to reuse an existing vcpkg
checkout; provisioning leaves that checkout unchanged and uses vcpkg to install
and upgrade its tkrzw package with the repository overlay. Classic-mode
`vcpkg upgrade tkrzw:<triplet> --no-dry-run --classic` replaces an outdated
installed version; both install and upgrade receive the overlay ports and
triplets. Without `VCPKG_ROOT`, provisioning creates and bootstraps a checkout
under the cache root. Install
`build-essential curl zip unzip tar gzip ca-certificates git pkg-config cmake
ninja-build` from Debian packages before that step. GLib stays on the system.

**When to run it.** Run the setup only when the work needs the pin oracle:
a differential, a same-run measurement against the pin, or a data check.
Work that does not touch the oracle does not need it. From the repository
root, run these two commands. They are the same in sh, zsh and pwsh:

```sh
uv python install
uv run --no-project --managed-python python -X utf8 -m tools.dev.provision
```

The first installs the interpreter that `.python-version` names, and does nothing
when it is present. The second runs provisioning in that interpreter; pass the
options below after `tools.dev.provision`. Both must run from the repository root,
where uv finds `.python-version` and `uv.toml` and Python finds `tools`.

The setup is idempotent. Every step checks its result and skips when the result holds,
so a second run only verifies again. The options are:

- `--cells`;
- `--jobs`;
- `--no-pull`: build the libraries even if the published image exists.

`OXPINYIN_CONTAINER` names the container command (see step 1). Progress goes to stderr.
Stdout includes an informational ext4 path report and one line for each cell:

```
bdb prefix=<dir> libpinyin=<sha256> libzhuyin=<sha256> data manifest OK
```

The setup is done when it prints one such line for each requested cell.

### Python-only tools.dev hosts

For a Linux x86_64 or arm64 host that needs only `tools.dev`, provision Python
without the oracle setup:

```sh
uv python install
uv run --no-project --managed-python python -X utf8 -m tools.dev.provision --python-only
uv run --no-project --managed-python python -X utf8 -m tools.dev.doctor
```

This mode runs the existing Linux and supported-platform checks. It then runs the
same managed CPython verification as full setup.
See [Installing uv and cargo-c](setup.md#installing-uv-and-cargo-c)
for host installation. The mode
prints the verified-interpreter line and the informational ext4 path report.
It logs that it stops after Python provisioning, and it exits successfully. It does
not do these steps:

- install or check just, cargo-nextest, cargo-c or rust-analyzer;
- select, start or pull a container runtime;
- install Valgrind or backend utilities;
- provision oracle libraries or model data;
- write the oracle environment file.

See [native setup](setup.md) for the interpreter policy.
On a host that has `just`, `just python` provisions the same interpreter.

`--python-only` cannot be combined with `--cells`, `--jobs`
or `--no-pull`. These combinations fail with exit 2 before
provisioning.

### Development tools without the oracle

`--dev-tools` keeps or installs just, cargo-nextest, cargo-c and the
rust-analyzer component, as full setup does. It then verifies managed CPython,
prints the ext4 path report and stops before the oracle setup:

```sh
uv python install
uv run --no-project --managed-python python -X utf8 -m tools.dev.provision --dev-tools
```

A tool that meets its requirement is kept, and needs neither Cargo nor rustup.
A missing or outdated just, cargo-nextest or cargo-c needs Cargo for
`cargo install`. A missing rust-analyzer needs rustup. Without the program that
it needs, setup exits 1, names the tool and refers to
[native setup](setup.md#installing-uv-and-cargo-c). `--dev-tools` and
`--python-only --dev-tools` are the same mode. `--dev-tools` cannot be combined
with `--cells`, `--jobs` or `--no-pull`; these combinations fail with exit 2.

For concurrent worktrees, `UV_PYTHON_INSTALL_DIR` and
`UV_CACHE_DIR` can select task-owned interpreter and download-cache locations.

### The environment file

When full setup succeeds, it writes `~/.oxpinyin-oracle.env` (`OXPINYIN_ORACLE_ENV`
overrides the path). Every run rewrites the file. Source it before a
runner:

```sh
. ~/.oxpinyin-oracle.env                       # reference cell, tkrzw
PINYIN_ORACLE_DBM=kc . ~/.oxpinyin-oracle.env  # another cell
```

A subset run, such as `--cells bdb`, keeps the other cells that the
existing file names, if their saved prefixes still pass two checks:

- the library hashes in the current pin/recipe provenance;
- the prefix-local data-manifest checks.

A cell that fails
either check is removed, with one diagnostic that names the cell and the check.
The file contains the verified retained cells and the cells of this run, also
when they use different cache directories. A retained tkrzw cell also
keeps its local runtime paths. Setup publishes the complete file with an
atomic rename.

The file exports:

- for each provisioned cell `<C>` in `BDB`, `KC`, `TKRZW`:
  `OXPINYIN_ORACLE_PREFIX_<C>` (the oracle prefix) and
  `OXPINYIN_ORACLE_DATA_<C>` (its `lib/libpinyin/data`);
- `PINYIN_MODEL_DIR` (the extracted model20 cache);
- for the cell that
  `PINYIN_ORACLE_DBM` names (the value of the caller, else `tkrzw`, else the first
  provisioned cell): `PINYIN_ORACLE_PREFIX` and `OXPINYIN_LIBPINYIN_DATA_DIR`.
  These two are computed again each time the file is sourced.

A cell that the setup did not
provision stops the `source` with an error. The file sources alike in sh,
dash, bash and zsh: every value is single-quoted, and a `case` on the quoted
`PINYIN_ORACLE_DBM` selects the cell. A file written by the retired shell
setup is regenerated from the cells of the run, not read.

### Data verification

`Provision.data_ok` in `tools/dev/provision.py` checks the same data contract in three cases: when setup
reuses a cache, when it verifies regenerated data, and when it keeps an existing cell in
the environment file. The check does these steps:

1. It compares the SHA-256 of `oracle-data.sha256`
   with `data_manifest_sha256` in the own `oracle-pin.txt` of that prefix, and checks
   every listed stable file.
2. It compares the SHA-256 of
   `oracle-data-unstable.sha256` with `data_unstable_manifest_sha256` in the same
   `oracle-pin.txt`, then checks every generated file that it lists.

A missing or changed unstable manifest fails. A generated-file failure
names the file.

The unstable manifest covers `bigram.db`, `phrase_index.bin`,
`pinyin_index.bin`, `punct.bin` and both addon indexes. Their bytes differ
between builds, so the check compares this digest with the local record of the prefix,
never with the repository pin or another prefix.

For a read-only check of a selected prefix, load its matching setup
environment and run from the repository root:

```sh
just provision data-check "$PINYIN_ORACLE_DBM" "$PINYIN_ORACLE_PREFIX"
```

### Runtime enforcement (verification rule 9)

Any object that resolves `libtkrzw` must use the runtime that setup verified,
also when `PINYIN_ORACLE_DBM` is unset, `bdb` or `kc`.
`tools/tkrzw/runtime.py` contains the single Python loader check. Provisioning,
native gates and compatibility recipes share it. The reference images
keep their environment-only shell check and do not receive `tools.dev`.

The environment writer exports `OXPINYIN_TKRZW_LIBRARY`: the
exact canonical library that setup verified.

- A healthy host uses the existing `probe_host` result.
- An unhealthy host uses the provisioned cache.
- An older generated `OXPINYIN_TKRZW_PREFIX` or `OXPINYIN_TKRZW_PREFIXES` still selects
  its cache object.

No implicit home cache or arbitrary system prefix is
trusted. Select alternate caches through their generated environment files.

The check runs `ldd` on the exact object that the driver opens, in the working directory and environment
of the driver. It runs before the driver launches or compares results.
These drivers load no other libtkrzw into their loader namespace before
`dlopen`, so the dependency search of the object (including RPATH/RUNPATH and
relative loader paths) is the same. The check examines linked executables directly.
These rules apply:

- Resolved real paths must equal that exact verified object. A different path,
  a missing dependency, an unconfigured runtime or a failed resolver refuses the run.
- The diagnostic names the runner, the pin/subject side and the path found.
- Every label uses the full resolved `ldd` dependency set, including transitive and
  preloaded libraries.
- Objects without tkrzw dependencies need no trusted tkrzw runtime.
- For a dlopen driver, the check examines the subject library. It also examines a
  directly launched ELF executable.
- A launched `#!` script,
  such as the POSIX shell `valgrind` wrapper that Debian and Ubuntu install, is
  checked through its interpreter: the ELF that the kernel starts. `ldd` cannot
  read the script itself. The check does not inspect what the script execs, so the
  caller checks the subject that it runs explicitly.
- Any other launched file is
  checked as itself, so a file that the resolver cannot read is refused.

The `performance` stage of `tools/oracle/Dockerfile.oracle` sets
`OXPINYIN_TKRZW_LIBRARY` to the packaged `libtkrzw` of Debian, the library that its
`build` stage links the pin against. The same check certifies both arms
against that library. Installing uv does not establish trust in a runtime.

Keep the directory of the selected object first in `LD_LIBRARY_PATH`, followed
by the inherited value. These sites set the path differently:

| Sites | Rule |
| --- | --- |
| `measure callgrind-differential`: all launch sites | Use each selected object's directory, then inheritance. |
| `just cpp-smoke`: cleared-path `ldd` assertion | Intentional SONAME/RPATH test; the subsequent executable launch is checked with its actual environment. |
| `tools/oracle/build.py`: generation-prefix assignment | Intentional isolation in the pinned build recipe; not a differential launch. |
| `tools/dev/provision.py`: generated-path parsing and host-probe filtering; the cache-probe assignments | Intentional parsing without caller inheritance, or isolation of the host or selected cache while provisioning. |

No measurement path is exempt as never-tkrzw. The profiling and packaging
launches, and the Python contract, gcov, training and ordering tools, share
the same implementation. A refusal is a failed run, never MATCH or SKIP.

Run the loader fixtures with:

```sh
uv run --no-project --managed-python python -B -X utf8 tools/tkrzw/runtime.test.py
```

## What it does

0. `uv python install` provides managed CPython; uv itself must already be on
   `PATH`. The Python provisioning service refuses interpreter overrides first, then
   checks Cargo and the repository toolchain on every OS. On Linux it also
   requires or installs Valgrind.
   On Linux it requires the backend dump utilities for the requested cells:
   `db5.3_dump` (`db5.3-util`) and `kchashmgr` (`kyotocabinet-utils`). Root
   containers may install them with apt. Ordinary accounts get an explicit
   missing-package error. Install them deliberately before you run setup. On
   macOS and Windows they are not required: provisioning runs neither, and its
   prefix checks run in the `deps` image. Normal Linux provisioning installs Valgrind idempotently
   when `valgrind --version` fails, and prints its version in the summary.
1. Resolves and prints the runtime and its version before container provisioning,
   in the order and with the translations that
   [container runtimes](setup.md#container-runtimes) describes.
   An explicit `OXPINYIN_CONTAINER` wins. No available candidate is an error.
   Only the Docker path on Linux starts `dockerd` (detached, `setsid
   nohup dockerd`), unless `docker info` already answers. It then waits for it.
   On macOS and Windows, a Docker that does not answer `docker info` fails
   with a message to start Docker Desktop.
   If a proxy certificate exists at
   `/root/.ccr/agent-proxy-ca.crt` (the path that some hosted Linux environments
   use), this path installs it into the system
   trust store for the proxy access of the daemon. An ordinary proxy setting
   does not imply that this file exists. podman needs no daemon, no
   root and no `sudo`: `podman info` must answer. The pulls, the
   `deps` build and the data runs are the same commands rootless.
2. Pulls the `base_image` tag of `tools/oracle/oracle-pin.txt`
   (`debian:testing`, deliberately unpinned) only when an
   uncached local fallback build needs it. Published artifacts and cached stages
   avoid that pull. Registry retries back off for 2, 4, 8 and 16 s.
3. Gets the libraries for bdb, kc and tkrzw for the host platform, linux/amd64 or linux/arm64.
   Each prefix records their built hashes and installed versions.
   - If `ghcr.io/shenghaoc/oxpinyin-oracle:<tag>` exists for amd64, or
     `<tag>-arm64` for arm64, setup pulls it and copies the cells out of it.
     `uv run --no-project --managed-python python -X utf8 -m tools.oracle.image image-tag` prints `<tag>`
     (pin, base image tag and a hash of the recipe and `oracle-pin.txt`).
   - Setup discards a cell whose pin/recipe stamp or recorded bytes fail validation, and builds it
     instead. It builds every cell when no image exists. For the build, it builds the
     `deps` stage of the pinned image and runs `python3 tools/oracle/build.py --libs-only
     --enable-libzhuyin --dbm <cell>` inside it, at the same in-image
     paths as the Dockerfile (`/build/<cell>`, `/opt/pinyin-oracle/<cell>`).
   - Apple container skips the published image and always builds the cells
     in this way. On Windows, setup exports the filesystem of the published image to a
     tar in the cache, and the `deps` image unpacks it.

   The cells that need a build, and later the cells that need data, run at
   the same time in a bounded pool (`tools/dev/pool.py`):
   - Workers multiplied by make
     jobs stays within the CPU count, so nested `make -j` is counted. Every
     overlapped build keeps at least two make jobs. Each container is limited to its make jobs
     with `--cpus`.
   - The `deps` image, the model, the hash check, step 3b and the report
     stay serial: the data step must not run on libraries that are not
     verified.
   - Each container is named with the random token of this
     invocation (not the PID of the shell) and labelled with it. Its ID
     is recorded with `--cidfile`.
   - A failed build, SIGINT or SIGTERM starts no
     further cell, stops the running containers and ends the run. It removes only
     containers whose label carries the token of this invocation: the recorded ID, and
     any container that the runtime created before it wrote the `--cidfile`. A same-named
     container that this run did not create is never touched.
   - Logs stay per cell in
     `<cache>/build-<cell>.log` and `<cache>/data-<cell>.log`.

   Setup validates the current pin/recipe stamp and the recorded
   library hashes and installed-version record of the prefix. Stale or damaged provenance
   stops provisioning; there is no frozen output digest. On Windows, host
   Python cannot read the symlinks that a container makes in a prefix on NTFS.
   Every check, removal and summary of a prefix therefore runs in the `deps` image
   (`provision prefix-check`, run by the managed interpreter of the image). Only a
   check that ran and found a prefix invalid leads to its removal. A check
   that does not run (the interpreter, an import, the runtime) stops
   provisioning with its error, and setup keeps the prefix.

   3b. Linux only: install tkrzw through the repository's vcpkg overlay.
   macOS and Windows load the oracle only in containers and skip this step.
   - Use `VCPKG_ROOT`, or bootstrap a checkout under `<root>/vcpkg`.
   - Install `tkrzw:x64-linux-oxpinyin`, with binaries cached under
     `<root>/vcpkg-binary-cache` unless `VCPKG_BINARY_SOURCES` is configured.
     The port alone reads `vcpkg-overlays/tkrzw/pin.json`.
   - Certify the installed utility and extracted oracle library against the
     installed `libtkrzw.so.1` with `tools/tkrzw/runtime.py`.
   - Write `PKG_CONFIG_PATH`, `LD_LIBRARY_PATH`, the utility `PATH` and
     `OXPINYIN_TKRZW_PREFIX` for that install to the environment file.
   - Every invocation runs vcpkg install; its ABI checks decide cache reuse.
4. Uses model20, which the model fetch (`tools/dev/model.py`) got early into
   `<root>/model20-<digest>`. It runs `build.py --regen-data` for each cell inside
   the image, and verifies the installed files again with the shared
   [data verification](oracle.md#regenerate-the-data-after-every-pull). The data is generated in a
   staging area and published with its own manifest hashes. Reuse checks these
   recorded hashes and every listed file. The repository pin freezes no generated output.
5. Prints the per-cell lines. On Linux it first rewrites the `.pc` files of the prefixes
   to the host location and writes the environment file above.
   On macOS and Windows the host loads no oracle, so setup writes neither.

[Container runtimes](setup.md#container-runtimes) lists the host steps that
run only on Linux.

### How containers get out

Processes in a container cannot reach the proxy at `127.0.0.1`, and they do not
trust its CA. Every Docker/Podman run here therefore:

- uses `--network host`;
- passes `https_proxy` and the CA (`GIT_SSL_CAINFO`, `SSL_CERT_FILE`, `CURL_CA_BUNDLE`);
- mounts `/root/.ccr/ca-bundle.crt` only when that bundle exists (or when an explicit
  `OXPINYIN_PROXY_CA` is supplied).

The mount is a private copy of the selected
bundle (mode 0600, in the temporary directory of the run, removed on exit), mounted
`:ro,z`. The selected file itself, including a host system CA file, is never
bind-mounted or relabelled.

With `HTTPS_PROXY` set and that bundle
absent, step 0 uses the CA that the host already trusts, in this order:
`SSL_CERT_FILE`, `CURL_CA_BUNDLE`, then the system bundle. It logs which one it uses
(`proxy CA: ...`). When there is none, setup fails there, before any download
or build, and names `OXPINYIN_PROXY_CA`. It never disables verification.
The fallback proves only that a non-empty bundle is present. It does not show
that the interception certificate of the proxy is trusted, so it does not prevent a
TLS failure; step 0 says so. To resolve a failure there, set
`OXPINYIN_PROXY_CA` to the CA bundle of the proxy. No network probe checks it.

An ordinary HTTPS proxy needs no files under `/root/.ccr`. The `deps` build passes the proxy as a build
arg and the CA as the optional BuildKit secret `cacert`
(`tools/oracle/Dockerfile.oracle`). With the secret, apt trusts that CA for
any https source. The base image keeps its default Debian mirrors in both cases,
so the installed packages, and therefore the toolchain, are the same.

## Allowlist domains

The network policy of the host must allow at least these hosts:

| Host | Used for |
| --- | --- |
| `registry-1.docker.io`, `auth.docker.io`, `production.cloudflare.docker.com` | the Debian base image (`debian:testing`); the registry can answer 429 under load, and setup retries |
| `deb.debian.org` | the Debian testing package mirror for the `deps` image build |
| `github.com` | libpinyin at the pinned commit (`git fetch --depth=1 <url> <sha>`) |
| `github.com`, `release-assets.githubusercontent.com` | uv-managed CPython release assets, when uncached |
| `downloads.sourceforge.net` and its redirect target, a rotating `*.dl.sourceforge.net` mirror | model20 archive |
| `ghcr.io` (and the blob host it redirects to) | the published oracle and `deps` images |
| `index.crates.io`, `static.crates.io` | cargo |

The network policy of a host can refuse any of these hosts. Ask the environment owner for the required
allowlist change, or use an already verified local
cache. Do not mirror or publish model20.

`--regen-data` compiles the data build of the pin, so both the library build and
the data step need the `deps` image. The image workflow publishes that
`deps` stage as `ghcr.io/shenghaoc/oxpinyin-oracle:<oracle-tag>-deps` on
amd64 and `<oracle-tag>-deps-arm64` on arm64. Setup tries the tag of the host
platform before it builds locally, so a successful pull removes the
cold package install. If the pull fails, setup builds `deps` locally and
keeps the stderr of the pull in `<cache>/pull-deps.err`.

A second run with everything present only verifies again. To drop the build-cache copy, run
`docker builder prune -af` after setup.

### tkrzw step and the CI child

Linux development and CI use the vcpkg overlay port and binary cache.
The runtime is named by `OXPINYIN_TKRZW_PREFIX` and certified through the
actual loader resolution. Oracle/reference images, release packaging and
shipping-size measurements retain Debian's tkrzw.

### When the published image cannot be pulled

Setup keeps the stderr of the pull in `<cache>/pull-published.err` and logs its
last five lines. Before it builds the libraries, it states which case applies:

- `manifest unknown / not found`: the tag is not published;
- `pull FAILED
  (network, auth or proxy, not "not found")`: any other failure.

Both cases fall back to the build.

## Gates on a remote host

On a host that meets the requirements above, the
[verification procedure](verification.md) runs as on any Linux host. The
cells are the prefixes that setup prints. Do not assume that the host already has
Cargo, the repository Rust toolchain, Valgrind or a native compiler.
Same-run callgrind measurements ([benches](benches.md)) need those host
tools, and the oracle for the pin side. A main-against-main control (two
builds of one commit, measured in one run, spread 0) needs only the Rust
side.

## Failure modes

- uv missing or below the floor: the shell or uv reports it on the first
  command; install uv as the host requirements describe.
- An interpreter override such as `UV_PYTHON`: setup exits 1 before any other
  work. A `UV_PYTHON` set before `uv python install` has already installed
  that interpreter; unset it and run both commands again.
- Missing Cargo/toolchain, or Valgrind without root/sudo: setup exits 1 with
  the prerequisite and the next action, before the oracle downloads. `--python-only` omits the Cargo/toolchain
  and Valgrind checks. `--dev-tools` omits them too, and exits 1 only when a tool
  that it must install needs a missing Cargo or rustup.
- `429 Too Many Requests` from the registry: the script retries; run it again.
- `403` from the proxy: the host is not allowed for the environment. Do
  not route around it. Add the host (table above) to Allowed domains.
- `FAIL: <cell>: stale pin/recipe or damaged build provenance after provisioning`
  and exit 1: the libraries in the prefix do not agree with the record in the
  `oracle-pin.txt` of that prefix, or with the current pin and recipe. They stay
  in the oracle root. Do not edit the recorded values to match.

### Provisioning implementation and fixture checks

`tools.dev.provision` is the single provisioning service. `just provision` runs
`just python` and then the same module.

Run the synthetic provisioning fixtures through normal Python discovery:

```sh
just test-tools tools/dev/provision.test.py
```

The fixtures run the documented `uv run ... -m tools.dev.provision` command with
fake host tools, pins, data, proxy CA and generated environment paths, and source
the environment file in sh, dash and zsh. The zsh case is skipped where zsh is not
installed, except in CI's harness job, which installs zsh and sets
`OXPINYIN_REQUIRE_ZSH=1`. Missing option values return status 1
with an `--option: requires an argument` diagnostic. Interpreter overrides are
refused before the native prerequisite checks.
