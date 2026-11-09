# Native contributor setup

This procedure sets up native development on Linux, macOS or Windows and gives the contribution checks.
Pin-comparison campaigns use the local oracle `performance` stage; see
[where measurements run](benches.md#where-measurements-run). Native development
and native-only measurements need no container. The oxpinyin-only nightly
snapshot uses plain unpinned Debian testing, like every other Linux environment.

Start from [CONTRIBUTING](../../CONTRIBUTING.md). These instructions build
and test local files. They do not install an input method or replace your
system libpinyin. If a command fails, stop, and keep that command and its full
error before you continue. In Bash, `echo $?` immediately after a command
shows its exit status (zero means success); PowerShell uses `$LASTEXITCODE`.
Choose your operating system below. Ordinary native work
requires no container, oracle or model20 (upstream reference model data).

## Storage backend

Storage-dependent crates build against one DBM backend per build. The
[default backend](backends.md) is the reference cell on every platform. The build finds tkrzw through pkg-config,
and the platform sections below install it. Berkeley DB 5.3 and Kyoto Cabinet
are non-default backends ([backends](backends.md)).

Every repository shell script requires Bash 5.0 or newer on Linux and macOS.
`just doctor` reports the `bash` that `env bash` selects, and fails below 5.0.
Linux distributions provide it. The macOS section installs Homebrew's Bash and
checks the selection. On Windows, doctor reports Bash as not applicable and
never fails on its version. Native Windows recipes use pwsh with
`-CommandWithArgs`; doctor checks its declared minimum.

The Rust **toolchain** is the compiler and its tools. rustup installs
and selects it. `rust-toolchain.toml` is the authority for the version and components.
The **linker** joins compiled code into executables and native libraries.

## Linux

Use Bash 5.0 or newer. Linux is one native platform target. You need:

- Git, curl and CA certificates;
- a C/C++ compiler and linker, Make and pkg-config;
- libclang;
- tkrzw development files (the [default store backend](backends.md));
- GLib development files.

Berkeley DB **5.3** and Kyoto Cabinet are needed only to build those
non-default backends.
A development package supplies the headers and libraries that you need to
compile against a native library. Package names depend on your system. That
does not change the build or create another compatibility environment.
Repository Python does not use the system interpreter. Provision it with uv
as described below.

If these prerequisites are missing, install them with the package
manager of your system. `sudo` asks for administrator permission to install system packages.
These are common package-manager examples, not separate supported or tested
distribution targets:

```bash
# Systems using apt:
sudo apt-get update
sudo apt-get install -y build-essential git curl ca-certificates pkg-config libclang-dev libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev tkrzw-utils libglib2.0-dev
```

```bash
# Systems using dnf:
sudo dnf install gcc gcc-c++ make git curl ca-certificates pkgconf-pkg-config clang-devel tkrzw-devel glib2-devel
```

The build finds tkrzw only through pkg-config (`pkg-config tkrzw`), as the
configure of libpinyin does. Debian's `tkrzw.pc` names `-llzma -llz4 -lzstd
-lz`, but `libtkrzw-dev` does not depend on their development packages. The
apt line therefore installs them. Without them, every link fails with `cannot find
-llzma`. To use a tkrzw that you built yourself, put its `lib/pkgconfig`
first on `PKG_CONFIG_PATH` and its `lib` on `LD_LIBRARY_PATH`.
`just tkrzw-build DIR` builds the upstream revision that CI uses into `DIR`.
Ubuntu's packaged libtkrzw is defective
([`tkrzw-distro-compat.md`](../findings/tkrzw-distro-compat.md)).
`tools/tkrzw/distro-probe.sh` checks the library on `PATH`, and provisioning runs
it on an Ubuntu host. A `just tkrzw-build` build, the Debian package and the
MSVC `tkrzw.lib` need no probe.

Repository Python uses uv and its managed CPython 3.15 series with the GIL.
`uv.toml` sets the uv floor, 0.13.1. CI installs the newest uv that satisfies it.

`just doctor` is the one check of the minimum of every tool: Bash 5.0, uv,
CPython, just, cargo-nextest, cargo-c, cargo-fuzz, cargo-llvm-cov, and
ShellCheck 0.11.0. ShellCheck is advisory; run it when a shell script
changes ([verification rule 5](verification.md#5-local-test-gate)).
On Linux, doctor also checks GNU ld, gcov and Valgrind. These rules apply:

- Each line names the resolved executable, the minimum and the version found.
- A newer tool passes, an older required tool fails, and a missing tool comes with its install command.
- Semantic versions compare as semver does: a pre-release of the minimum
  (`1.59.0-beta.1`) is older than it, and build metadata (`+build`) does not
  count.
- GNU ld, gcov and Valgrind compare by number, and a development build of
  one of them counts as its number.

Install or upgrade a tool only when `just doctor`
says that it is missing or too old.

`just doctor` cannot run until uv exists. Install uv only
when it is missing or older than 0.13.1 (`uv --version` shows the version):

```bash
# Exact on purpose (#1074): a checksum is only known for a known version.
installer="$(mktemp)"
curl --fail --location --retry 3 \
  https://github.com/astral-sh/uv/releases/download/0.13.1/uv-installer.sh -o "$installer"
printf '%s  %s\n' \
  7c482d409673a53f4d9bd34ae95c9166be3b8d89ca82f27dd7bca3cab457afff "$installer" \
  | sha256sum -c -
sh "$installer"
rm -f "$installer"
export PATH="$HOME/.local/bin:$PATH"
hash -r
```

The native command interface needs just at or above the `minimum-version` in the
root `justfile`. `just` enforces this itself, and `just doctor` reports it.
Install just through the route of your host: `brew install just` on macOS, the
distribution package on Linux, or `cargo install just --locked`.

Provision the interpreter through `just python`. It runs `uv python install --quiet`,
which reads `.python-version` and does nothing when that interpreter is present.
A plain version request prefers the GIL-enabled build, and the check below rejects
a free-threaded build. Every other Python recipe depends on `just python`, so a recipe
run on a bare host provisions the interpreter itself. `just doctor` installs
nothing, and names `just python` when the interpreter is missing. Then verify
the interpreter:

```bash
just python
uv run --no-project --managed-python python -X lazy_imports=normal - <<'PYTHON'
from pathlib import Path
import subprocess
import sys
import sysconfig
assert sys.implementation.name == "cpython"
assert sys.version_info.releaselevel == "final" and (3, 15, 0) <= sys.version_info[:3] < (3, 16, 0)
assert sysconfig.get_config_var("Py_GIL_DISABLED") == 0
assert sys._is_gil_enabled()
managed = subprocess.check_output(
    ["uv", "python", "find", "--managed-python"],
    text=True).strip()
assert Path(sys.executable).resolve() == Path(managed).resolve()
PYTHON
```

The installer digest comes from its `digest` field in the official
[uv 0.13.1 release API](https://api.github.com/repos/astral-sh/uv/releases/tags/0.13.1).
Automated Linux setup on x86_64 and arm64 verifies the release archive against the pinned
[published checksum](https://github.com/astral-sh/uv/releases/download/0.13.1/sha256.sum).

`.python-version` is the one place where the interpreter version is written.

- Inside the
  checkout, `uv run --no-project --managed-python python` and `uv python find
  --managed-python` read it.
- A script that changes directory first, or a container
  build, copies the file in or passes its contents on. It does not write the
  version again.
- Run the required `tools.dev` module from the checkout with the managed interpreter
  and the invocation in the `justfile`.

The check `tools/dev/python-request.test.py` fails when a version request
appears anywhere else, outside the dated records in `docs/findings/`.

**Rule (#1230).** Provision the managed Python interpreter of the repository
through `just python`. Do not substitute another interpreter. A later series is a deliberate change: the tooling
relies on the `-X lazy_imports` modes of 3.15. Do not use a system Python, a virtual
environment, `pyproject.toml` or `uv.lock`. The `.python-version`
and `uv.toml` of the checkout name the same interpreter series and uv floor.

`just build`, `just clippy` and `just fmt` pass Cargo arguments to
Cargo unchanged. To limit parallel builds, set `CARGO_BUILD_JOBS` or `[build] jobs` in
your own `~/.cargo/config.toml`. Select the same native packages
and backend as the Cargo command that you would otherwise run:

```bash
just build --locked -p oxpinyin-core
just clippy --locked -p oxpinyin-core --all-targets -- -D warnings
just fmt --all --check
```

### Registry listing and test entry points

`just test-tools` runs every non-excluded Python harness file, concurrently up
to the allowed CPU count of the host. Each file has private temporary and lock
directories. Output appears as complete blocks in path order, and failures do
not stop the remaining files. The same managed interpreter runs
the controller and each file.

For iteration, pass files and, optionally, a unittest name pattern:

```sh
just test-tools tools/dev/test_runner.test.py
just test-tools tools/dev/test_runner.test.py -k HarnessTests
just test-tools --changed
just test-tools --changed origin/main
```

These rules apply to the selection:

- `--changed [BASE]` compares tracked files against BASE (HEAD by default),
  including staged and unstaged edits, and includes untracked files.
- Changed test
  files select themselves. `HARNESS_DEPENDENCIES` in `tools/dev/test_runner.py`
  maps known modules to their tests.
- Any unknown path or missing mapped test
  selects the full suite. An empty diff selects no files.
- You cannot combine explicit files and
  `--changed`.
- Every mode keeps the credential filter and the
  existing exclusions.

`--changed` is iteration feedback only: `HARNESS_DEPENDENCIES` is maintained by hand
and can select too few tests. The full `just test-tools` stays the completion gate.
With `-k` and no explicit FILE, the runner reports files with no matching tests as
unselected. A named file with no matches stays a failure, as does a pattern
that matches no tests across all selected files.

The Windows recipes use PowerShell and keep the child status explicitly.
Their argument transport stays TODO (no Windows host).

Run `just fetch-contract-data` once before a full test run. The
`candidate_surface` test of `oxpinyin-runtime` reads the pinned system data that it fetches, and fails without it.

The native Rust test entry point uses cargo-nextest at or above the
`nextest-version` in `.config/nextest.toml` ([profile inheritance](https://nexte.st/docs/configuration/#profile-inheritance)
needs it); `just doctor` reports it. Install it with
`cargo install cargo-nextest --locked` (or the prebuilt release). `just test` runs `cargo nextest run --profile local`
with your arguments. Pass the same crate and
feature selection as for cargo test (see [backend selection](backends.md)). For the CI limits, pass `--profile ci`. `just test-doc` runs
`cargo test --doc`.

If names or enabled repositories differ, use the official package documentation of your system.
The native check below prints the tkrzw version that
pkg-config reports.

In a managed environment that already has these packages, skip their
installation. Root development environments omit `sudo`. Without installation
rights, ask the environment owner for these packages. Ordinary native work
does not require nested Docker or Podman.

If you already have a checkout, go to its root: the directory that contains
`CONTRIBUTING.md` and `Cargo.toml`. Replace `/path/to/oxpinyin` below with
that directory, then skip the cloning block:

```bash
cd /path/to/oxpinyin
```

Otherwise, get a checkout, then stay at its root:

```bash
mkdir -p ~/src
cd ~/src
git clone https://github.com/shenghaoc/oxpinyin.git
cd oxpinyin
```

Run `command -v rustup`. If it reports no path, install
[rustup](https://rust-lang.github.io/rustup/installation/index.html):

```bash
installer="$(mktemp)"
curl --fail --location --retry 3 https://sh.rustup.rs -o "$installer"
sh "$installer" -y --profile minimal --default-toolchain none
rm -f "$installer"
```

The `.` command below loads the PATH settings of Cargo into the current shell,
so that the shell can find `cargo` and `rustc`. With a new or an existing rustup
installation, run:

```bash
. "$HOME/.cargo/env"
rustup show
cargo test --locked -p oxpinyin-chewing -p oxpinyin-core -p oxpinyin-engine
just check
```

The `cargo test` command tests native Rust packages. `just check` (tkrzw by
default; `just check bdb` or `just check kyotocabinet` for the others)
also links and runs storage and C-library tests. Headers
are the native library declarations that the compiler uses. libclang reads
them, and pkg-config locates native library installations. The check prints
the selected toolchain, architecture and backend. It installs nothing.

Success means that all test summaries have zero failures, and that the storage check ends
with `Setup check passed`. Negative
tests can deliberately print warnings, assertions, `failed` or `CRITICAL`.
Use the final summaries and the process exit status to decide success.

In a fresh Bash terminal, use your checkout directory if it is different:

```bash
cd ~/src/oxpinyin
. "$HOME/.cargo/env"
just check
```

Cargo reuses downloaded dependencies and the `target/` outputs of that checkout.
Keep outputs separate across worktrees (separate checkouts of branches).

## macOS

Use Terminal with Bash 5.0 or newer (`bash`), Git, Apple's
[Command Line Tools](https://developer.apple.com/xcode/resources/),
[Homebrew](https://docs.brew.sh/Installation) and rustup. If the Command Line
Tools are absent, run `xcode-select --install` and complete the system dialog.

macOS ships Bash 3.2 at `/bin/bash`. Install Homebrew's Bash and make sure that
`env bash` selects it:

```bash
brew install bash
env bash --version
command -v bash
```

`env bash --version` must print 5.0 or newer. `command -v bash` must print the
Homebrew path (`/opt/homebrew/bin/bash` on Apple silicon or
`/usr/local/bin/bash` on Intel), not `/bin/bash`. If it prints `/bin/bash`,
put the Homebrew `bin` directory before `/bin` on `PATH` in your shell profile,
and open a new terminal.

```bash
brew install tkrzw glib pkgconf
mkdir -p ~/src
cd ~/src
git clone https://github.com/shenghaoc/oxpinyin.git
cd oxpinyin
```

Run the uv, just and interpreter commands from the Linux section, in that
order (`brew install just` supplies just). For the checksum step on macOS, replace
`sha256sum -c -` with `shasum -a256 -c -`.
Then, if rustup is absent, install it with the Linux rustup commands above,
and run:

```bash
. "$HOME/.cargo/env"
rustup show
cargo test --locked -p oxpinyin-chewing -p oxpinyin-core -p oxpinyin-engine
export LIBRARY_PATH="$(brew --prefix)/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
export CARGO_TARGET_DIR=target/tkrzw
just check
```

`export` sets a value that the commands in this terminal inherit. pkg-config
finds Homebrew's tkrzw, the selected backend. Its `tkrzw.pc` names
the lz4, lzma and zstd libraries without their directory, and
`LIBRARY_PATH` supplies that directory. To use a tkrzw that you built yourself (`just tkrzw-build
DIR`), put `DIR/lib/pkgconfig` first on `PKG_CONFIG_PATH` and `DIR/lib` on
`DYLD_LIBRARY_PATH` instead. This is a real storage link/run check; a
`cargo check` alone does not run the tests.
In a fresh Bash terminal, go back to the checkout, source the Cargo environment,
and repeat both exports and the check. Linux C-ABI packaging and Linux reference
differentials are separate evidence, not permanent restrictions on portability.

The nightly macOS CI job uses this broader portable scope:

```bash
cargo test --locked --workspace
```

The macOS job of CI builds tkrzw with `.github/actions/tkrzw` instead of
using Homebrew's. Berkeley DB is a non-default backend. To use it:

1. Run `brew install berkeley-db@5`.
2. Export the keg-only `SYSTEM_DEPS_BDB_*` overrides
   ([backends](backends.md#discovery-and-overrides)).
3. Run `export CARGO_TARGET_DIR=target/bdb`.
4. Run `just check bdb`.

Keep the target and user-data directories of each backend separate.

Fuzzing and the backend sanitizer gates run natively on macOS. The recipes
select Homebrew's Berkeley DB 5.3, Kyoto Cabinet and tkrzw. They use the one
nightly that `fuzz/rust-toolchain.toml` names, with `rust-src`, and no Linux
`setarch`. In `fuzz/`, rustup installs the toolchain that the file names:

```bash
brew install berkeley-db@5 kyoto-cabinet
(cd fuzz && rustup toolchain install)
just fuzz parser -- -runs=100
just sanitize bdb address
just sanitize bdb thread
just sanitize kc address
just sanitize kc thread
just sanitize tkrzw address
just sanitize tkrzw thread
```

`capi-commands` also runs natively. pkg-config finds the GLib and
tkrzw of the facade, and the fuzz recipe adds Homebrew's lib to `LIBRARY_PATH`. AddressSanitizer runs with leak detection off on
Darwin. Standalone LeakSanitizer is refused on macOS arm64, and
MemorySanitizer has no Darwin target. Both refusals name the host. Linux
keeps its existing sanitizer commands and paths.

### When the macOS job runs

The macOS workspace job is the `test-macos` job in `verify-nightly.yml`.
Its schedule and merge status are in [verification rule 10](verification.md#10-ci-merge-conditions).
Pull-request CI tests Linux only. To reproduce a macOS-only failure, dispatch
the job on a branch with `macos_only` set:

```bash
gh workflow run verify-nightly.yml --ref <branch> -f macos_only=true
```

With `macos_only` set, every other nightly job is skipped, the Windows job
included. A dispatch without it or `windows_only` runs the whole nightly
workflow, including the macOS job.

## Native Windows

This section is for native Windows Rust development, not Linux under WSL. Install Git
and [rustup's MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html):
Visual Studio Build Tools with **Desktop development with C++** and the
Windows SDK. [Microsoft's Rust setup](https://learn.microsoft.com/en-us/windows/dev-environment/rust/setup)
explains the installation. Cargo finds MSVC through the Visual Studio installer;
MSVC does not need to be on PATH. Doctor uses the fixed
`vswhere.exe` location of the installer and requires the VC tools component.
If rustup is not already available, install it with the official Windows installer.

Use pwsh at or above the minimum in
[pwsh-floor.txt](../../tools/dev/pwsh-floor.txt). Call just recipes from pwsh,
also in a Developer PowerShell session. Windows PowerShell can change
empty arguments and embedded quotes before just receives them.

In pwsh:

```powershell
New-Item -ItemType Directory -Force "$HOME/src" | Out-Null
Set-Location "$HOME/src"
git -c core.autocrlf=false clone https://github.com/shenghaoc/oxpinyin.git
Set-Location oxpinyin
rustup show
cargo test --locked -p oxpinyin-chewing -p oxpinyin-core -p oxpinyin-engine
cargo fmt --all --check
cargo clippy --locked -p oxpinyin-chewing -p oxpinyin-core -p oxpinyin-engine --all-targets -- -D warnings
```

Repository Python tooling on Windows runs through the pwsh recipes.
The Python-backed `_test-argv` recipe verifies argument and exit-status transport.
Job Object lifetime, fuzz, coverage and full tooling acceptance stay unverified.
Install uv with its native Windows installer, then just (`cargo install just
--locked`), then run `just python`. The rule above applies unchanged.

All three packages link and run natively without a DBM library or container.
Success is zero failed tests and exit code zero (`$LASTEXITCODE`). In a fresh
Developer PowerShell, run `Set-Location "$HOME/src/oxpinyin"` and repeat the test.
For an existing checkout, use your real directory. Rustup selects the same
repository toolchain automatically. Editors and debuggers use this same native
MSVC environment and toolchain; no project-specific IDE is required.

**Storage-dependent native Windows scope:** the Windows CI job runs clippy,
`cargo nextest run --profile ci` and the doctests over the portable
workspace on the [default backend](backends.md). It also runs the tools Python
harness (`just test-tools`).

- `.github/actions/tkrzw` builds the
  upstream revision that `tools/cloud/setup.sh` provisions, with `nmake -f
  VCMakefile` in the Visual Studio developer environment. The result is a static
  `tkrzw.lib` with the DLL C runtime that the Rust MSVC targets link.
- The action writes a `tkrzw.pc`, which the store build script finds through
  `PKG_CONFIG_PATH`.
- The Windows image has no pkg-config, so the action
  installs the vcpkg `pkgconf` and sets `PKG_CONFIG` to its `pkgconf.exe`.

To do the same locally:

1. Run `just tkrzw-build DIR` from Developer PowerShell. The recipe loads the
   developer environment through `vswhere` when it is not already loaded.
2. Put `DIR\lib\pkgconfig` on `PKG_CONFIG_PATH` and `DIR\bin` on `PATH`.
3. Set `PKG_CONFIG` to a `pkgconf.exe` (for example, from `vcpkg install
   pkgconf:x64-windows`).

The recipe runs no `distro-probe.sh` and needs no `sh`.

The job also installs the vcpkg `glib` port, which it finds through the same
`PKG_CONFIG` and `PKG_CONFIG_PATH`. It therefore builds and tests
the full workspace, including `oxpinyin-utils`, `oxpinyin-training`, KMM
and both C-ABI crates. The locale tests select `.UTF-8` on Windows.
Text expectations use the newline of the host.
The `test-windows` job in `verify-nightly.yml` excludes no crate. The Linux FFI feature of the oracle is
not selected. Those tests do not establish native Windows oracle behavior.
You do not need physical Windows hardware to use
this CI coverage.

A full local build or `just test` needs the same two vcpkg ports as the
CI job. Without GLib, `glib-sys` fails, and every crate except
`oxpinyin-chewing`, `oxpinyin-core`, `oxpinyin-store` and
`oxpinyin-engine` fails to build. From Developer PowerShell, with
`$vcpkg` as your vcpkg checkout and `DIR` as the `just tkrzw-build` prefix, run:

```powershell
& "$vcpkg\vcpkg.exe" install pkgconf:x64-windows glib:x64-windows
$installed = "$vcpkg\installed\x64-windows"
$env:PKG_CONFIG = "$installed\tools\pkgconf\pkgconf.exe"
$env:PKG_CONFIG_PATH = "DIR\lib\pkgconfig;$installed\lib\pkgconfig"
$env:PATH = "DIR\bin;$installed\bin;$env:PATH"
& $env:PKG_CONFIG --modversion glib-2.0 tkrzw
```

The `bin` directories put the GLib and tkrzw DLLs on the path of the test binaries.
Bash scripts run from the same session through the
`bash.exe` of Git for Windows (`& 'C:\Program Files\Git\bin\bash.exe' SCRIPT`).

**When the Windows job runs:** the full Windows workspace job is the
`test-windows` job in `verify-nightly.yml`. Its schedule and merge status are in
[verification rule 10](verification.md#10-ci-merge-conditions). Windows
is a supported native target.
To reproduce a Windows-only failure, dispatch the job on a branch with
`windows_only` set:

```bash
gh workflow run verify-nightly.yml --ref <branch> -f windows_only=true
```

With `windows_only` set, every other nightly job is skipped, the macOS job
included. A dispatch without it runs the whole nightly workflow, including
the Windows job.

The frozen oracle does not define which native operating systems oxpinyin
can support.

### Native Windows and WSL containers

Every recipe has a Windows form, or prints why it is refused on Windows; see the `justfile`.

The pin runs only in a Linux container. On Windows, that container is a
[WSL container](https://learn.microsoft.com/en-us/windows/wsl/wsl-container)
driven by `wslc`, the first runtime that `select_runtime` tries there. Run these commands from Developer
PowerShell. `$env:OXPINYIN_CAPTURE_RUNTIME` or `$env:OXPINYIN_CONTAINER` names another runtime:

| Task | Command |
| --- | --- |
| Build and verify the oracle image | `uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image build-image` |
| Verify an oracle image | `uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image verify-image --engine wslc IMAGE` |
| Pin capture ([pin-captures.md](pin-captures.md)) | `just capture-pin RUN` |
| Provision the oracle cells in the performance image | `just provision-oracle` |
| Cloud provisioning ([cloud.md](cloud.md)) | `just provision` |

`tools/oracle/image.py` translates the docker commands for `wslc`
([container runtimes](#container-runtimes)). Docker and podman get the
commands unchanged. These `wslc` limits apply:

- **No `--platform`.** The adapter drops the flag; every image runs at the
  host architecture in any case ([container runtimes](#container-runtimes)).
- **`inspect` prints JSON only.** It has no Go templates, so
  `Architecture` and `Id` are read from the JSON record
  (`uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image image-field`).
- **No top-level `cp` and no `rm`.** `verify-image` runs `wslc export` and
  extracts only `opt/pinyin-oracle` from the tar. Containers are removed with
  `wslc remove -f`.
- **`build --pull` is an on/off switch, and `build` has no `--network`.**
  Capture builds pass no pull flag. A build's `--network host` is dropped
  with a notice. Any other build `--network` value is refused.
- **No `z` mount option.** The adapter drops `,z` and `:z`.
- **No host networking.** `wslc run --network host` fails with "host mode
  networking is not supported". The adapter drops `--network host` and prints
  a one-line notice that the default network of the runtime is used. `--network
  none` is kept.
- **Mounts need Windows paths.** `wslc` reads an MSYS path such as
  `/c/...` as a path inside its VM and mounts an empty directory. The capture
  launcher therefore passes the checkout and data as native Windows paths. `:ro` and a
  read-write mount nested in a read-only one both work.

Host Python cannot read the symlinks that a container makes on an NTFS mount
(`[Errno 22] Invalid argument`). On Windows, provisioning therefore never
reads inside a provisioned prefix from the host. Each check, removal and
summary runs in the `deps` image.

WSL is a separate Linux development option: follow the Linux section from
its Bash terminal. [Microsoft recommends the Linux filesystem](https://learn.microsoft.com/en-us/windows/wsl/filesystems)
for Linux checkouts and builds (`~/src`, not `/mnt/c`). WSL may also host the
compatibility runtime without replacing the native Windows workflow.

## Cache directory

Pinned downloads and builds share `OXPINYIN_CACHE` across worktrees.
The justfile exports a non-empty override or `join(cache_directory(), "oxpinyin")`.
The OS cache directory is `$XDG_CACHE_HOME` or `~/.cache` on Linux,
`~/Library/Caches` on macOS and `%LOCALAPPDATA%` on Windows.
`just cache-root` prints the root.

Set a non-empty absolute `OXPINYIN_CACHE` on a host without a home directory
and for direct `cargo` or `uv run` commands. `tools/cloud/setup.sh` asks
`just` for the root when `just` is installed.

The name of each entry carries its pin, so a new pin gets a new entry:

- `contract-data/<version>`: the pinned libpinyin-data package.
- `model20-<digest>`: the model20 archive and its extraction.
- `tkrzw-<revision>`: the tkrzw build of `just provision`.
- `pin-<commit>`: the provisioning cache, with the pin prefixes and their build trees.

The model and contract-data fetchers allow one active fetch per pinned entry on a
shared filesystem that supports file locking and atomic rename. An overlapping
fetch of that entry fails as busy before touching the data. Run `fetch-model
--force` while the entry is idle, including tools that read the extraction
directly, because it replaces the extraction at the same path.

The operating system releases the lock when a fetch crashes.

After an interrupted publication, confirm that no fetch or reader is using the entry.
If a model entry contains `.extracted-backup` and `extracted` is missing, rename
the backup to `extracted`. If both exist, move the backup aside, then run
`just fetch-model --force`.

## Container runtimes

The pin runs only in a Linux container. `select_runtime` in
`tools/oracle/image.py` selects the first runtime that it finds, in this order:

| Host | Order |
| --- | --- |
| Windows | `wslc` (WSL containers), Podman, Docker |
| macOS | Apple `container`, Podman, Docker |
| Linux | Podman, Docker |

Every image runs at the architecture of the host, with no emulation: x86_64 hosts
use `linux/amd64` and aarch64 hosts use `linux/arm64`. `verify-image` defaults to
the platform of the host and refuses any other `--platform` on every runtime. On
Windows, `wslc` also ships as `container.exe`, so `container` names `wslc`
there. It names Apple `container` only on macOS.

`OXPINYIN_CONTAINER` names one runtime explicitly. For `build-image` and the captures
(`just capture-pin`), `OXPINYIN_CAPTURE_RUNTIME` comes first. Each command
prints the runtime that it chose on a `Runtime:` line. Provisioning (`just provision`,
`just provision-oracle`), `verify-image` and `build-image` work through each
runtime. Every command is written in the docker grammar and passed through
`runtime_command`. Docker and Podman take it as written. `wslc` drops
`--platform` (see [native Windows](#native-windows-and-wsl-containers)).
Apple `container` works as follows, as a macOS run with it verified:

- **Commands.** `image pull` (without `--quiet`), `image tag`, `delete` and
  `system status`; JSON inspect records, whose image `Id` is the digest of the config
  descriptor.
- **Mounts.** Read-only binds are `--mount ...,readonly`. It has no
  single-file binds.
- **Memory.** Its VM is small by default. Image builds get `--cpus 4
  --memory 6G`, and the library and data runs get `--memory 6G`.
- **Network.** There is no `--network host`. It is dropped with a one-line
  notice that the default network is used. Its build has no `--network`, so
  any other build value is refused. `--network none` was confirmed to block
  traffic, so the capture run has no network.
- **Pull.** The capture build gets no valued `--pull`. Its `--pull` is an
  on/off switch, as on `wslc`.
- **Capture.** The capture runs the capture image by its tag. After the run, it checks
  that the tag still names the digest that it read before.
- **`verify-image`.** It can neither `cp` nor `export` a container that never
  started, and the oracle image (`FROM scratch`) cannot start. It therefore creates
  no container. It runs `container image save` and reads the OCI image layout
  (each blob checked against its digest, the layers applied in order).
- **Provisioning.** The libraries are built in the `deps` image instead of
  copied from the published image.

On every runtime, the library and data runs mount the checkout read-only at
`/opt/oracle-recipe` and run `python3 /opt/oracle-recipe/tools/oracle/build.py`
in the `deps` image. No run binds a single file, and no host needs Bash for them.

`just provision-oracle` builds the performance image when it is absent, and runs
`provision oracle` in it with the provisioning cache mounted at
`/provision`. On rootful Docker, the run is the invoking user, so the cache
stays theirs. The home and cache of uv are then in `/tmp`. `just provision` and
`just provision-oracle` use the `pin-<commit>` entry under
`OXPINYIN_CACHE` ([cache directory](#cache-directory)).

These host steps of `just provision` run only on Linux:

- the Valgrind check and apt install;
- the apt install of the dump utilities;
- the start of `dockerd`;
- the ext4 report;
- the host libtkrzw check;
- the `.pc` rewrite;
- the environment file.

On macOS and Windows, provisioning skips these steps. It also
does not require the dump utilities (`db5.3_dump`, `kchashmgr`) there:
it runs neither, and its prefix checks run in the `deps` image.

## Development debug information

Normal builds and tests keep file and line numbers for workspace code.
Panic backtraces show these locations when `RUST_BACKTRACE=1` is set.
External dependencies have no debug information in these builds.
Types and local variables need the `dev-full` profile:

```bash
cargo build --locked --profile dev-full -p oxpinyin-capi
cargo test --locked --profile dev-full -p oxpinyin-engine --no-run
```

Use the files in `target/dev-full/` with your debugger.
This profile includes full debug information for workspace code and dependencies.
It keeps the normal development optimization level, debug assertions and overflow checks.
It does not change release or profiling builds.

Verification cells continue to use `dev` at optimization level 1.
Their identity record still names that profile and level.
Do not put a `dev-full` build in a claimed cell target directory.
For a diagnostic runner that accepts `OXPINYIN_CAPI_SO`, build separately
and give it the path to `target/dev-full/libpinyin_capi.so`.

Sanitizer gates use their existing compiler flags for symbolisation.
Coverage uses LLVM coverage maps, which do not need type or variable information.
The Callgrind and Massif shipping-library procedures use their own supplied
libraries and keep their existing debug settings.

## Contribution checks

Run the local checks in [verification rule 5](verification.md#5-local-test-gate); they scale with the change and its stage.

Use the containing package's `Cargo.toml` name for iteration commands.
Select a non-default backend with `--no-default-features --features bdb`;
keep separate target and user-data directories ([backends](backends.md)).
For build/config changes, identify the affected packages or ask the maintainer.
CI is the full gate.

Ordinary native tests do not establish pin compatibility. Changes to parsing,
candidates, native data, user files or the C API need the relevant differential
on a [provisioned remote Linux host](cloud.md). Use the reference tkrzw cell
unless the [backend rules](verification.md#2-backend-selection) select others.
If no matching differential is apparent, ask the maintainer.

Follow [results and reproduction](../../CONTRIBUTING.md#results-and-reproduction),
and stop after the required checks.

## Troubleshooting and resumption

- Missing `cargo`/`rustup`: on Linux/macOS, source `"$HOME/.cargo/env"`. On
  Windows, open a fresh Developer PowerShell after you install rustup. A
  permission panic from an installed rustup is an environment access problem:
  keep the error and ask the environment owner. Do not reinstall over it.
- Missing MSVC `link.exe` or Windows SDK: use Developer PowerShell and check
  the Visual Studio workload. Native Windows does not use Unix `cc` or
  `pkg-config` for the Rust-only command above.
- Missing tkrzw headers (`pkg-config ... tkrzw` failed), `db.h`, libclang
  or native libraries: repeat the prerequisite checks of your OS.
  The error identifies the missing header or library. If package installation is restricted,
  ordinary users need the environment owner. Send
  that owner the exact failing command and the complete error. On Linux,
  `pkg-config --modversion glib-2.0` and `pkg-config --modversion tkrzw`
  check discovery. The setup check prints the backend version and the missing-libclang diagnostic of bindgen,
  so keep those lines too.
- macOS cannot link the lz4/lzma/zstd libraries of tkrzw: repeat the `LIBRARY_PATH` export
  above. For Berkeley DB, repeat the `SYSTEM_DEPS_BDB_*` exports
  ([backends](backends.md#discovery-and-overrides)). Do not treat a
  successful Clippy/check as a link test.
- Missing oracle or model inputs do not block ordinary native tests. They must
  be supplied for the selected compatibility checks, where a skipped or missing
  prerequisite is a failed verification, not a pass.
