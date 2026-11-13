# Remote Linux development

This procedure provisions a remote Linux host with `tools.dev.provision`.
A host that meets the requirements below runs every non-measurement gate;
measurement gates also need the ext4 paths below. Ordinary native development
uses [native setup](setup.md) and needs no remote host. Pin-comparison
measurements run in the oracle `performance` stage
([where measurements run](benches.md#where-measurements-run)).

The host may be a root container, may supply its own container runtime, and
may reach the network only through an HTTPS proxy. The provisioner handles
each case. It limits custom trust to an existing proxy certificate or an
explicit CA path. It verifies the current pin/recipe provenance and the recorded build/data bytes.
When several worktrees share a host, each worktree has its own
`OXPINYIN_ORACLE_ENV` file. Worktrees share `OXPINYIN_CACHE` by default.

## Host requirements

Full setup requires Linux on a supported native platform:
linux/amd64 or linux/arm64. `tools.dev.provision` selects the platform from
the machine architecture and refuses any other.
It also requires:

- Cargo on `PATH`, rustup, and the installed toolchain and components that
  `rust-toolchain.toml` selects. Setup checks these before provisioning. It does not
  install Rust. When they are missing, [native setup](setup.md) installs them.
- A Valgrind at or above its minimum (`just doctor` shows it), or root/sudo
  access and `apt-get` to install or upgrade it. Without either, setup prints
  `run as root: apt-get install -y valgrind` and fails before the oracle
  downloads. The environment owner then installs it, or the work moves to a
  host that already has a suitable Valgrind.
- uv on `PATH` meeting the requirement in `uv.toml`, plus writable managed-Python and cache locations. See [native setup](setup.md) for installing uv.
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

Without an ext4 path, measurement gates cannot run; measurement work moves to a
suitable host. Setup reports accessible ext4 candidates, or none/unknown. It
does not fail or create a filesystem. Verify every measurement path by hand,
also when setup reports a candidate.

On a host that does not have these capabilities, the work stops at the named
prerequisite until the environment owner provides it or the work moves to a
suitable host. Network policy is not routed around. `--python-only`
has the narrower requirements documented below. It needs no Cargo, Rust,
Valgrind, backend utilities or container runtime. `--dev-tools` adds the
development tools to that mode. It needs Cargo or rustup only for a tool that
it must install.

## Setup command

Workspace tests in a cloud session need the tkrzw vcpkg overlay installed
and its paths exported as [Linux setup](setup.md#linux) describes; full setup
does both ([step 3b](#what-it-does)). The Debian packages
`build-essential curl zip unzip tar gzip ca-certificates git pkg-config cmake
ninja-build` must be installed first. GLib stays on the system.

**When to run it.** The setup is needed only when the work needs the pin oracle:
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
- `--no-pull`: builds the libraries even if the published image exists.

`OXPINYIN_CONTAINER` names the container command ([container runtimes](setup.md#container-runtimes)). Progress goes to stderr.
Stdout includes an informational ext4 path report and one line for each cell:

```
bdb prefix=<dir> libpinyin=<sha256> libzhuyin=<sha256> data manifest OK
```

The setup is done when it prints one such line for each requested cell.

### Python-only tools.dev hosts

A Linux x86_64 or arm64 host that needs only `tools.dev` provisions Python
without the oracle setup:

```sh
uv python install
uv run --no-project --managed-python python -X utf8 -m tools.dev.provision --python-only
uv run --no-project --managed-python python -X utf8 -m tools.dev.doctor
```

This mode runs the Linux platform checks, then the same managed-CPython
interpreter verification as full setup
([interpreter policy](setup.md#linux)). It prints the verified-interpreter line
and the ext4 path report, and exits successfully after Python provisioning.
It does not touch a container runtime, Valgrind, backend utilities, oracle
libraries, model data or the oracle environment file. On a host that has
`just`, `just python` provisions the same interpreter.

`--python-only` cannot be combined with `--cells`, `--jobs`
or `--no-pull`; these combinations fail with exit 2 before
provisioning. For concurrent worktrees, `UV_PYTHON_INSTALL_DIR` and `UV_CACHE_DIR` can select task-owned interpreter and download-cache locations.

To provision development tools without the oracle, use `--dev-tools` in place of `--python-only`.

### The environment file

When full setup succeeds, it writes `~/.oxpinyin-oracle.env` (`OXPINYIN_ORACLE_ENV`
overrides the path). Every run rewrites the file. A runner sources it
first:

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
names the file. The unstable digest is compared with the local record of the
prefix, never with the repository pin or another prefix
([the two manifests](oracle.md#regenerate-the-data-after-every-pull)).

A read-only check of a selected prefix loads its matching setup
environment and runs from the repository root:

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
exact canonical library that setup verified. A healthy host uses the
`probe_host` result; an unhealthy host uses the provisioned cache.
When `OXPINYIN_TKRZW_LIBRARY` is unset, the check takes the library under
`OXPINYIN_TKRZW_PREFIX`, or else under the first entry of `OXPINYIN_TKRZW_PREFIXES`.
No implicit home cache or arbitrary system prefix is
trusted. Alternate caches are selected through their generated environment files.

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

The directory of the selected object stays first in `LD_LIBRARY_PATH`, followed
by the inherited value. These sites set the path differently:

| Sites | Rule |
| --- | --- |
| `measure callgrind-differential`: all launch sites | Each selected object's directory, then inheritance. |
| `just cpp-smoke`: cleared-path `ldd` assertion | Intentional SONAME/RPATH test; the subsequent executable launch is checked with its actual environment. |
| `tools/oracle/build.py`: generation-prefix assignment | Intentional isolation in the pinned build recipe; not a differential launch. |
| `tools/dev/provision.py`: generated-path parsing and host-probe filtering; the cache-probe assignments | Intentional parsing without caller inheritance, or isolation of the host or selected cache while provisioning. |

No measurement path is exempt as never-tkrzw. The profiling and packaging
launches, and the Python contract, gcov, training and ordering tools, share
the same implementation. A refusal is a failed run, never MATCH or SKIP.

The loader fixtures run with:

```sh
uv run --no-project --managed-python python -B -X utf8 tools/tkrzw/runtime.test.py
```

## What it does

0. Bootstraps uv and managed CPython, after it checks the uv installation
   destination. The Python provisioning service then checks Cargo and the
   repository toolchain on every OS. On Linux it also installs Valgrind when
   `valgrind --version` fails, and prints its version in the summary.
   On Linux it requires the backend dump utilities for the requested cells:
   `db5.3_dump` (`db5.3-util`) and `kchashmgr` (`kyotocabinet-utils`). Root
   containers may install them with apt; ordinary accounts get an explicit
   missing-package error, so they are installed before setup runs. On
   macOS and Windows they are not required: provisioning runs neither, and its
   prefix checks run in the `deps` image.
1. Resolves and prints the runtime and its version
   ([container runtimes](setup.md#container-runtimes)). No available candidate is an error.
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
     `deps` stage of the pinned image and runs `tools/oracle/build.py --libs-only
     --enable-libzhuyin --dbm <cell>` with Debian’s Python in the image, at the same in-image
     paths as the Dockerfile (`/build/<cell>`, `/opt/pinyin-oracle/<cell>`).
   - Apple container skips the published image and always builds the cells
     in this way. On Windows, setup exports the filesystem of the published image to a
     tar in the cache, and the `deps` image unpacks it.

   The cells that need a build, and later the cells that need data, run at
   the same time in a bounded pool (`tools/dev/pool.py`). Workers multiplied
   by make jobs stay within the CPU count. The `deps` image, the model, the
   hash check, step 3b and the report stay serial, so the data step never runs
   on unverified libraries. A failed build, SIGINT or SIGTERM starts no further
   cell, stops the running containers and ends the run; it removes only the
   containers labelled with the random token of this invocation. Logs stay per
   cell in `<cache>/build-<cell>.log` and `<cache>/data-<cell>.log`.

   Setup validates the current pin/recipe stamp and the recorded
   library hashes and installed-version record of the prefix. Stale or damaged provenance
   stops provisioning; there is no frozen output digest. On Windows, host
   Python cannot read the symlinks that a container makes in a prefix on NTFS.
   Every check, removal and summary of a prefix therefore runs in the `deps` image
   (`provision prefix-check`, run by the managed interpreter of the image). Only a
   check that ran and found a prefix invalid leads to its removal. A check
   that does not run (the interpreter, an import, the runtime) stops
   provisioning with its error, and setup keeps the prefix.

   3b. Linux only: installs tkrzw through the repository's vcpkg overlay.
   macOS and Windows load the oracle only in containers and skip this step.
   - Uses `VCPKG_ROOT`, or bootstraps a checkout under `<root>/vcpkg`. An
     existing checkout stays unchanged: vcpkg installs tkrzw with the
     repository overlay ports and triplets, and classic-mode
     `vcpkg upgrade tkrzw:<triplet> --no-dry-run --classic` replaces an outdated
     installed version.
   - Installs `tkrzw:x64-linux-oxpinyin`, with binaries cached under
     `<root>/vcpkg-binary-cache` unless `VCPKG_BINARY_SOURCES` is configured.
     The port alone reads `vcpkg-overlays/tkrzw/pin.json`.
   - Certifies the installed utility and extracted oracle library against the
     installed `libtkrzw.so.1` with `tools/tkrzw/runtime.py`.
   - Writes `PKG_CONFIG_PATH`, `LD_LIBRARY_PATH`, the utility `PATH` and
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

These host steps of `just provision` run only on Linux, and macOS and Windows
skip them: the Valgrind check and apt install, the apt install of the dump
utilities, the start of `dockerd`, the ext4 report, the host libtkrzw check,
the `.pc` rewrite and the environment file.

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
TLS failure; step 0 says so. A failure there is resolved by setting
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

The network policy of a host can refuse any of these hosts. The remedy is the required
allowlist change from the environment owner, or an already verified local
cache. model20 is not mirrored or published.

`--regen-data` compiles the data build of the pin, so both the library build and
the data step need the `deps` image. The image workflow publishes that
`deps` stage as `ghcr.io/shenghaoc/oxpinyin-oracle:<oracle-tag>-deps` on
amd64 and `<oracle-tag>-deps-arm64` on arm64. Setup tries the tag of the host
platform before it builds locally, so a successful pull removes the
cold package install. If the pull fails, setup builds `deps` locally and
keeps the stderr of the pull in `<cache>/pull-deps.err`.

`docker builder prune -af` after setup drops the build-cache copy.

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
cells are the prefixes that setup prints.
Same-run callgrind measurements ([benches](benches.md)) need the host
tools above, and the oracle for the pin side. A main-against-main control (two
builds of one commit, measured in one run, spread 0) needs only the Rust
side.

## Failure modes

- An unwritable uv destination, such as a read-only `~/.local/bin`: setup
  exits 1 before any download. The `UV_*` overrides in
  [host requirements](#host-requirements) resolve it.
- Missing Cargo/toolchain, or Valgrind without root/sudo: setup exits 1 with
  the prerequisite and the next action after interpreter verification,
  but before the oracle downloads. `--python-only` omits these checks.
- `429 Too Many Requests` from the registry: the script retries, and a further run continues.
- `403` from the proxy: the host is not allowed for the environment. It
  is not routed around; the host (table above) is added to Allowed domains.
- `FAIL: <cell>: stale pin/recipe or damaged build provenance after provisioning`
  and exit 1: the libraries in the prefix do not agree with the record in the
  `oracle-pin.txt` of that prefix, or with the current pin and recipe. They stay
  in the oracle root. The recorded values are not edited to match.

### Provisioning implementation and fixture checks

`tools.dev.provision` is the single provisioning service. `just provision` runs
`just python` and then the same module.

The synthetic provisioning fixtures run through normal Python discovery:

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
