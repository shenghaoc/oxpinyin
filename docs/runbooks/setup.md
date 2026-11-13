# Native contributor setup

This procedure sets up native development on Linux, macOS or Windows.
It starts from [CONTRIBUTING](../../CONTRIBUTING.md). These instructions build
and test local files. They do not install an input method or replace your
system libpinyin. A failed command stops the setup; that command and its full
error are kept before the work continues. In Bash, `echo $?` immediately after a command
shows its exit status (zero means success); PowerShell uses `$LASTEXITCODE`.
Each operating system has its own section below. Ordinary native work and native-only
measurements require no container, oracle or model20 (upstream reference model data).
Pin-comparison measurements run in the oracle `performance` stage
([where measurements run](benches.md#where-measurements-run)).

## Storage backend

Storage-dependent crates build against one DBM backend per build. The
[default backend](backends.md) is the reference cell on every platform. The build finds tkrzw through pkg-config,
and the platform sections below install it. Berkeley DB 5.3 and Kyoto Cabinet
are non-default backends ([backends](backends.md)).

Native Windows recipes use pwsh with `-CommandWithArgs`; doctor checks its
declared minimum.

The Rust **toolchain** is the compiler and its tools. rustup installs
and selects it. `rust-toolchain.toml` selects the version and components. The workspace `rust-version` in `Cargo.toml` declares the minimum supported Rust version.
The **linker** joins compiled code into executables and native libraries.

## Linux

Linux setup needs:

- Git, curl and CA certificates;
- a C/C++ compiler and linker, Make and pkg-config;
- libclang;
- tkrzw development files (the [default store backend](backends.md));
- GLib development files.

Berkeley DB **5.3** and Kyoto Cabinet are needed only to build those
non-default backends.
A development package supplies the headers and libraries that you need to
compile against a native library. Package names depend on your system.
Repository Python does not use the system interpreter. Provision it with uv
as described below.

If these prerequisites are missing, install them with the package
manager of your system. These are common examples, not separately supported
distribution targets:

```bash
# Systems using apt:
sudo apt-get update
sudo apt-get install -y build-essential git curl ca-certificates pkg-config libclang-dev zip unzip cmake ninja-build libglib2.0-dev
```

```bash
# Systems using dnf:
sudo dnf install gcc gcc-c++ make git curl ca-certificates pkgconf-pkg-config clang-devel zip unzip cmake ninja-build glib2-devel
```

tkrzw comes from the repository's vcpkg overlay port on Linux, macOS and
Windows. GLib stays on the system package manager (Homebrew on macOS).
Windows also uses vcpkg for GLib because it has no system package route.
Install [vcpkg](https://learn.microsoft.com/vcpkg/get_started/get-started),
run `bootstrap-vcpkg.bat` on Windows or `bootstrap-vcpkg.sh` on Linux and
macOS, and set `VCPKG_ROOT` to that checkout.
On Linux x86_64, run from the repository root:

```bash
"$VCPKG_ROOT/vcpkg" install tkrzw:x64-linux-oxpinyin --overlay-ports=vcpkg-overlays --overlay-triplets=triplets
export OXPINYIN_TKRZW_PREFIX="$VCPKG_ROOT/installed/x64-linux-oxpinyin"
export PKG_CONFIG_PATH="$OXPINYIN_TKRZW_PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$OXPINYIN_TKRZW_PREFIX/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export PATH="$OXPINYIN_TKRZW_PREFIX/tools/tkrzw/bin:$PATH"
pkg-config --modversion tkrzw
```

The build discovers tkrzw through pkg-config. The Linux triplet selects shared
libraries and release-only builds; the port itself keeps vcpkg's linkage
defaults, and stock `x64-linux` installs static debug and release libraries.
The loader uses `LD_LIBRARY_PATH`; oxpinyin adds no rpath. Product binaries
and tests, including `just sanitize tkrzw address`, run with these exports.
See [tkrzw runtime enforcement](cloud.md#runtime-enforcement-verification-rule-9) for how `tools/tkrzw/runtime.py` checks the resolved library.
Linux CI uses the same overlay and a vcpkg binary cache. Release packaging,
shipping-size measurements and oracle/reference images keep Debian's tkrzw.
Ubuntu's packaged tkrzw is defective and is not used for development
([finding](../findings/tkrzw-distro-compat.md)).

### Comparing against another revision

A comparison needs a uv that can install the interpreter named in the compared revision’s `.python-version` and meets that revision’s `uv.toml` requirement.
When the revision's recipe fails, the command names the revision and prints the
message of just.

### Installing uv and cargo-c

Use `just doctor` to identify missing or outdated tools before installing or upgrading them.

Install uv first if it is missing or falls below the requirement in `uv.toml`:

```sh
curl -LsSf https://astral.sh/uv/install.sh | sh
```

On Windows:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://astral.sh/uv/install.ps1 | iex"
```

Then put the install directory first on `PATH`, ahead of any older uv:
`~/.local/bin` on macOS and Linux, `%USERPROFILE%\.local\bin` on Windows.
When `HOME` is read-only, set `UV_INSTALL_DIR` to a writable directory before you
run the installer, and put that directory first on `PATH` instead. If astral.sh
is blocked, stop and report it; do not route around the block. Most environments
already provide a uv that meets the floor.

uv enforces its own floor: `uv.toml` sets `required-version`, and every uv
command run in the checkout refuses an older uv with an error that names both
versions. The repository parses no uv version. CI installs uv with
`astral-sh/setup-uv`, which reads the same `required-version`.

Unset `UV_PYTHON` and the other interpreter overrides before the first uv
command. uv reads them before any repository Python runs, so a `UV_PYTHON` set
before `uv python install` makes it install that interpreter instead of the
one `.python-version` names. Provisioning and `just doctor` then refuse the
override, but the other interpreter is already installed.

Install cargo-c when you build the shipping libraries. `just doctor` enforces
cargo-c 0.10.11.

| Host | cargo-c |
|---|---|
| Debian and Ubuntu | `sudo apt install cargo-c` |
| Fedora | `sudo dnf install cargo-c` |
| Arch Linux | `sudo pacman -S cargo-c` |
| macOS | `brew install cargo-c` |
| Windows | `cargo install --locked cargo-c` |

Provisioning can keep or install the development tools for you. From the
repository root, after `uv python install`:

```sh
uv run --no-project --managed-python python -X utf8 -m tools.dev.provision --dev-tools
```

`--dev-tools` keeps each of just, cargo-nextest, cargo-c and the rust-analyzer
component that meets its requirement. It installs the others: just and cargo-c
with `cargo install` at their pinned versions, cargo-nextest with
`cargo install` at its newest release, and rust-analyzer with
`rustup component add`. It does not run the oracle setup. It needs Cargo or
rustup only for a tool that it must install; without that program it exits 1 and
names the tool. `--python-only` alone installs none of these tools and needs
neither program. See [cloud setup](cloud.md#python-only-toolsdev-hosts).

Repository Python uses the managed CPython named in `.python-version`.

`just doctor` reports the tools it finds, including their paths and versions.
On Linux, doctor also checks GNU ld, gcov and Valgrind.

The native command interface needs just at or above the `minimum-version` in the
root `justfile`. `just` enforces this itself, and `just doctor` reports it.
just comes through the route of the host: `brew install just` on macOS, the
distribution package on Linux, or `cargo install just --locked`.

`just python` provisions the interpreter. It runs `uv python install --quiet`,
which reads `.python-version` and does nothing when that interpreter is present.
A plain version request prefers the GIL-enabled build, and the check below rejects
a free-threaded build. Every other Python recipe depends on `just python`, so a recipe
run on a bare host provisions the interpreter itself. `just doctor` installs
nothing, and names `just python` when the interpreter is missing. Then verify
the interpreter:

```bash
just python
uv run --no-project --managed-python python -X utf8 - <<'PYTHON'
from pathlib import Path
import subprocess
import sys
import sysconfig
assert sys.implementation.name == "cpython"
assert sys.version_info.releaselevel == "final" and (3, 14, 0) <= sys.version_info[:3] < (3, 15, 0)
assert sysconfig.get_config_var("Py_GIL_DISABLED") == 0
assert sys._is_gil_enabled()
managed = subprocess.check_output(
    ["uv", "python", "find", "--managed-python"],
    text=True).strip()
assert Path(sys.executable).resolve() == Path(managed).resolve()
PYTHON
```

`.python-version` is the one place where the interpreter version is written.
Inside the checkout, `uv run --no-project --managed-python python` and
`uv python find --managed-python` read it. A script that changes directory
first, or a container build, copies the file in or passes its contents on; it
does not write the version again. `tools/dev/python-request.test.py` fails
when a version request appears anywhere else, outside `docs/findings/`.
A `tools.dev` module runs from the checkout with the managed interpreter and
the invocation in the `justfile`.

Only this interpreter is used: no system Python, virtual environment,
`pyproject.toml` or `uv.lock`. Commands use `-X utf8`.

`just build`, `just clippy` and `just fmt` pass Cargo arguments to
Cargo unchanged ([limit parallel builds](verification.md#8-builds) with
`CARGO_BUILD_JOBS`). The selection of native packages and backend is the same as for the Cargo
command that the recipe replaces:

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
- Explicit files and `--changed` cannot be
  combined.
- Every mode keeps the credential filter and the
  existing exclusions.

`--changed` is iteration feedback only: `HARNESS_DEPENDENCIES` is maintained by hand
and can select too few tests. The full `just test-tools` stays the completion gate.
With `-k` and no explicit FILE, the runner reports files with no matching tests as
unselected. A named file with no matches stays a failure, as does a pattern
that matches no tests across all selected files.

A full test run needs one `just fetch-contract-data` beforehand: the
`candidate_surface` test of `oxpinyin-runtime` reads the pinned data that it
fetches ([test inputs](../testing/README.md#test-inputs)).

The native Rust test entry point uses cargo-nextest at or above the
`nextest-version` in `.config/nextest.toml` ([profile inheritance](https://nexte.st/docs/configuration/#profile-inheritance)
needs it); `just doctor` reports it. `cargo install
cargo-nextest --locked` (or the prebuilt release) installs it. `just test` runs `cargo nextest run --profile local`
with the given arguments, which take the same crate and
feature selection as cargo test (see [backend selection](backends.md)). `--profile ci` selects the CI limits. `just test-doc` runs
`cargo test --doc`.

`just packaging install` and `just measure compare` build the shipping library
with cargo-c, on Linux as on macOS. `just doctor` states its minimum and
reports the version found. `cargo install cargo-c --locked`, the distribution
package or the upstream release archive installs it.

Where names or enabled repositories differ, the official package documentation of the system applies.
A managed environment that already has these packages skips their
installation. Root development environments omit `sudo`. Without installation
rights, the environment owner supplies these packages.

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
cargo test --locked -p oxpinyin-core -p oxpinyin-engine
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
The final summaries and the process exit status decide success.

In a fresh Bash terminal, use your checkout directory if it is different:

```bash
cd ~/src/oxpinyin
. "$HOME/.cargo/env"
just check
```

Cargo reuses downloaded dependencies and the `target/` outputs of that checkout.
Outputs stay separate across worktrees (separate checkouts of branches).

## macOS

macOS setup uses Terminal, Git, Apple's
[Command Line Tools](https://developer.apple.com/xcode/resources/),
[Homebrew](https://docs.brew.sh/Installation) and rustup. If the Command Line
Tools are absent, run `xcode-select --install` and complete the system dialog.

```bash
brew install glib pkgconf
mkdir -p ~/src
cd ~/src
git clone https://github.com/shenghaoc/oxpinyin.git
cd oxpinyin
```

The vcpkg route for tkrzw supports Apple silicon only; there is no Intel
triplet. tkrzw comes from the repository's vcpkg overlay port, which builds the pinned
upstream revision with upstream's own configure script and Makefile. Clone
[vcpkg](https://learn.microsoft.com/vcpkg/get_started/get-started) and run its
`bootstrap-vcpkg.sh`. Export `VCPKG_ROOT` as that checkout, here and in your
shell profile; the `just` recipes read it. Then, from the repository root:

```bash
export VCPKG_ROOT=<your vcpkg checkout>
"$VCPKG_ROOT/vcpkg" install tkrzw:arm64-osx-oxpinyin --overlay-ports=vcpkg-overlays --overlay-triplets=triplets
export PKG_CONFIG_PATH="$VCPKG_ROOT/installed/arm64-osx-oxpinyin/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export PATH="$VCPKG_ROOT/installed/arm64-osx-oxpinyin/tools/tkrzw/bin:$PATH"
pkg-config --modversion tkrzw
```

`triplets/arm64-osx-oxpinyin.cmake` is vcpkg's `arm64-osx-dynamic` triplet with
two settings. It builds the release variant only. It also leaves the install
name of `libtkrzw` as upstream's build records it, the absolute path in
`$VCPKG_ROOT/installed/arm64-osx-oxpinyin/lib`, so every binary linked against
it loads it with no rpath and no `DYLD_LIBRARY_PATH`. The vcpkg
checkout therefore stays where it is; a moved checkout needs the port reinstalled. The triplet makes
`VCPKG_ROOT` part of the package's identity, so an install from a new root
builds from source and never restores the library of another root from
vcpkg's binary cache.

Get uv as [Installing uv and cargo-c](#installing-uv-and-cargo-c) describes, run
`brew install just`, then run `just python`.
Then, if rustup is absent, install it with the Linux rustup commands above,
and run:

```bash
. "$HOME/.cargo/env"
rustup show
cargo test --locked -p oxpinyin-core -p oxpinyin-engine
export CARGO_TARGET_DIR=target/tkrzw
just check
```

pkg-config finds the vcpkg install of tkrzw through the `PKG_CONFIG_PATH`
export above. This is a real storage link/run check; a `cargo check` alone
does not run the tests. In a fresh Bash terminal, go back to the checkout,
source the Cargo environment, and repeat the exports and the check.

The nightly macOS CI job installs tkrzw the same way and runs
`cargo test --locked --workspace`. Berkeley DB is a non-default
backend. To use it:

1. Run `brew install berkeley-db@5`.
2. Export the keg-only `SYSTEM_DEPS_BDB_*` overrides
   ([backends](backends.md#discovery-and-overrides)).
3. Run `export CARGO_TARGET_DIR=target/bdb`.
4. Run `just check bdb`.

The target and user-data directories of each backend stay separate.

Fuzzing and the backend sanitizer gates run natively on macOS. The recipes
select Homebrew's Berkeley DB 5.3 and Kyoto Cabinet, and the vcpkg install of
tkrzw under `VCPKG_ROOT`. They use the one
nightly that `fuzz/rust-toolchain.toml` names, with `rust-src`, and no Linux
`setarch`. In `fuzz/`, rustup installs the toolchain that the file names:

```bash
brew install berkeley-db@5 kyoto-cabinet
(cd fuzz && rustup toolchain install)
just fuzz chewing-parser -- -runs=100
just sanitize bdb address
just sanitize bdb thread
just sanitize kc address
just sanitize kc thread
just sanitize tkrzw address
just sanitize tkrzw thread
```

`capi-commands` also runs natively. pkg-config finds the GLib and
tkrzw of the facade. AddressSanitizer runs with leak detection off on
Darwin. Standalone LeakSanitizer is refused on macOS arm64, and
MemorySanitizer has no Darwin target. Both refusals name the host. Linux
keeps its existing sanitizer commands and paths.

### Nightly macOS and Windows jobs

The macOS and Windows workspace jobs are `test-macos` and `test-windows` in
`verify-nightly.yml`. Their schedule and merge status are in
[verification rule 10](verification.md#10-ci-merge-conditions); pull-request CI
tests Linux only. A failure on one platform is reproduced by dispatching the
workflow on a branch with `macos_only` or `windows_only` set:

```bash
gh workflow run verify-nightly.yml --ref <branch> -f macos_only=true
gh workflow run verify-nightly.yml --ref <branch> -f windows_only=true
```

Either option skips every other nightly job, the other platform's job
included. A dispatch with neither runs the whole nightly workflow.

## Native Windows

This section is for native Windows Rust development, not Linux under WSL. Install Git
and [rustup's MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html):
Visual Studio Build Tools with **Desktop development with C++** and the
Windows SDK. [Microsoft's Rust setup](https://learn.microsoft.com/en-us/windows/dev-environment/rust/setup)
explains the installation. Cargo finds MSVC through the Visual Studio installer;
MSVC does not need to be on PATH. Doctor uses the fixed
`vswhere.exe` location of the installer and requires the VC tools component.
If rustup is not already available, install it with the official Windows installer.

Native Windows uses pwsh at or above the minimum in
[pwsh-floor.txt](../../tools/dev/pwsh-floor.txt). Just recipes are called from pwsh,
also in a Developer PowerShell session. Windows PowerShell can change
empty arguments and embedded quotes before just receives them.

In pwsh:

```powershell
New-Item -ItemType Directory -Force "$HOME/src" | Out-Null
Set-Location "$HOME/src"
git -c core.autocrlf=false clone https://github.com/shenghaoc/oxpinyin.git
Set-Location oxpinyin
rustup show
cargo test --locked -p oxpinyin-core -p oxpinyin-engine
cargo fmt --all --check
cargo clippy --locked -p oxpinyin-core -p oxpinyin-engine --all-targets -- -D warnings
```

Repository Python tooling on Windows runs through the pwsh recipes.
The Python-backed `_test-argv` recipe verifies argument and exit-status transport.
Job Object lifetime, fuzz, coverage and full tooling acceptance stay unverified.
Install uv with its native Windows installer, then just (`cargo install just
--locked`), then run `just python`. The interpreter rule of the Linux section applies unchanged.

These two packages link and run natively without a DBM library or container.
Success is zero failed tests and exit code zero (`$LASTEXITCODE`). In a fresh
Developer PowerShell, run `Set-Location "$HOME/src/oxpinyin"` (or your real
checkout directory) and repeat the test. Rustup selects the repository
toolchain automatically. Editors and debuggers use this same native MSVC
environment and toolchain; no project-specific IDE is required.

**Storage-dependent native Windows scope:** the Windows CI job runs clippy,
`cargo nextest run --profile ci` and the doctests over the portable
workspace on the [default backend](backends.md). It also runs the tools Python
harness (`just test-tools`).

- Windows installs the repository's tkrzw overlay port and GLib through vcpkg.
  The overlay reads the same pinned revision as the Linux build recipe.
  CMake exports functions and vtables; patched public data declarations export
  from the DLL and import in consumers. The release library uses the dynamic CRT
  (`/MD`), matching oxpinyin's C++ bridge and vcpkg's `x64-windows` triplet.
- The DLL is in `installed/x64-windows/bin`, the import library and pkg-config
  metadata in `lib` and `lib/pkgconfig`, and public headers in `include`.
- `$env:VCPKG_ROOT` names the vcpkg checkout. The commands below run from
  the repository root; `--overlay-ports` selects the repository port.

The job also installs the vcpkg `glib` port, which it finds through the same
`PKG_CONFIG` and `PKG_CONFIG_PATH`. It therefore builds and tests
the full workspace, including `oxpinyin-utils`, KMM
and both C-ABI crates. The locale tests select `.UTF-8` on Windows.
Text expectations use the newline of the host.
The `test-windows` job in `verify-nightly.yml` excludes no crate. The Linux FFI feature of the oracle is
not selected, so those tests do not establish native Windows oracle behavior.

A full local build or `just test` needs the same vcpkg ports as the
CI job. Without GLib, `glib-sys` fails, and every crate except
`oxpinyin-core`, `oxpinyin-store` and
`oxpinyin-engine` fails to build. From Developer PowerShell, run:

```powershell
& "$env:VCPKG_ROOT\vcpkg.exe" install tkrzw:x64-windows pkgconf:x64-windows glib:x64-windows --overlay-ports=vcpkg-overlays
$installed = "$env:VCPKG_ROOT\installed\x64-windows"
$env:PKG_CONFIG = "$installed\tools\pkgconf\pkgconf.exe"
$env:PKG_CONFIG_PATH = "$installed\lib\pkgconfig"
$env:PATH = "$installed\bin;$env:PATH"
& $env:PKG_CONFIG --modversion glib-2.0 tkrzw
```

The `bin` directories put the GLib and tkrzw DLLs on the path of the test binaries.

Windows is a supported native target; the frozen oracle does not define which
native operating systems oxpinyin can support. To dispatch the nightly Windows
job alone, see [nightly macOS and Windows jobs](#nightly-macos-and-windows-jobs).

### Native Windows and WSL containers

Every recipe has a Windows form, or prints why it is refused on Windows; see the `justfile`.

The pin runs only in a Linux container. On Windows, that container is a
[WSL container](https://learn.microsoft.com/en-us/windows/wsl/wsl-container)
driven by `wslc`, the first runtime that `select_runtime` tries there. These commands run from Developer
PowerShell. `$env:OXPINYIN_CAPTURE_RUNTIME` or `$env:OXPINYIN_CONTAINER` names another runtime:

| Task | Command |
| --- | --- |
| Build and verify the oracle image | `uv run --no-project --managed-python python -X utf8 -m tools.oracle.image build-image` |
| Verify an oracle image | `uv run --no-project --managed-python python -X utf8 -m tools.oracle.image verify-image --engine wslc IMAGE` |
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
  (`uv run --no-project --managed-python python -X utf8 -m tools.oracle.image image-field`).
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

On Windows, provisioning checks prefixes in the `deps` image ([cloud provisioning](cloud.md#what-it-does), step 3).

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

A host without a home directory, and a direct `cargo` or `uv run` command,
needs a non-empty absolute `OXPINYIN_CACHE`. `tools.dev.provision` asks
`just` for the root when `just` is installed.

The name of each entry carries its pin, so a new pin gets a new entry:

- `contract-data/<version>`: the pinned libpinyin-data package.
- `model20-<digest>`: the model20 archive and its extraction.
- `vcpkg/` and `vcpkg-binary-cache/`: the overlay checkout and binary cache of `just provision`, unless `VCPKG_ROOT` selects an existing checkout.
- `pin-<commit>`: the provisioning cache, with the pin prefixes and their build trees.

The model and contract-data fetchers allow one active fetch per pinned entry on a
shared filesystem that supports file locking and atomic rename. An overlapping
fetch of that entry fails as busy before touching the data. `fetch-model
--force` needs the entry idle, including tools that read the extraction
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
Apple `container` works as follows:

- **Commands.** `image pull` (without `--quiet`), `image tag`, `delete` and
  `system status`; JSON inspect records, whose image `Id` is the digest of the config
  descriptor.
- **Mounts.** Read-only binds are `--mount ...,readonly`. It has no
  single-file binds.
- **Memory.** Its VM is small by default. Image builds get `--cpus 4
  --memory 6G`, and the library and data runs get `--memory 6G`.
- **Network.** There is no `--network host`. It is dropped with a one-line
  notice that the default network is used. Its build has no `--network`, so
  any other build value is refused. `--network none` blocks traffic, so the
  capture run has no network.
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
`/opt/oracle-recipe` and run `tools/oracle/build.py` with Debian’s Python in the image
in the `deps` image. No run binds a single file, and no host needs Bash for them.

`just provision-oracle` builds the performance image when it is absent, and runs
`provision oracle` in it with the provisioning cache mounted at
`/provision`. On rootful Docker, the run is the invoking user, so the cache
stays theirs. The home and cache of uv are then in `/tmp`. `just provision` and
`just provision-oracle` use the `pin-<commit>` entry under
`OXPINYIN_CACHE` ([cache directory](#cache-directory)). Some host steps of
`just provision` run only on Linux ([cloud provisioning](cloud.md#what-it-does)).

## Development debug information

Normal builds and tests keep file and line numbers for workspace code.
Panic backtraces show these locations when `RUST_BACKTRACE=1` is set.
External dependencies have no debug information in these builds.
Types and local variables need the `dev-full` profile:

```bash
cargo build --locked --profile dev-full -p oxpinyin-capi
cargo test --locked --profile dev-full -p oxpinyin-engine --no-run
```

The debugger reads the files in `target/dev-full/`.
This profile includes full debug information for workspace code and dependencies.
It keeps the normal development optimization level, debug assertions and overflow checks.
It does not change release or profiling builds.

Verification cells continue to use `dev` at optimization level 1.
Their identity record still names that profile and level.
A `dev-full` build does not go in a claimed cell target directory.
A diagnostic runner that accepts `OXPINYIN_CAPI_SO` gets a separate build
and the path to `target/dev-full/libpinyin_capi.so`.

Sanitizer gates use their existing compiler flags for symbolisation.
Coverage uses LLVM coverage maps, which do not need type or variable information.
The Callgrind and Massif shipping-library procedures use their own supplied
libraries and keep their existing debug settings.

## Contribution checks

The [verification procedure](verification.md) states the checks for each change:
the [local test gate](verification.md#5-local-test-gate), the
[backend selection](verification.md#2-backend-selection) and the
[reporting rules](verification.md#7-proportionate-verification).
Packages are named by their `Cargo.toml` name; a build or configuration change
identifies the affected packages, or the maintainer names them.
Ordinary native tests do not establish pin compatibility; a change to parsing,
candidates, native data, user files or the C API also needs its differential on a
[provisioned remote Linux host](cloud.md). When no matching differential is
apparent, the maintainer names one.

## Troubleshooting and resumption

- Missing `cargo`/`rustup`: on Linux/macOS, source `"$HOME/.cargo/env"`. On
  Windows, open a fresh Developer PowerShell after you install rustup. A
  permission panic from an installed rustup is an environment access problem:
  the error goes to the environment owner, and rustup is not reinstalled over it.
- Missing MSVC `link.exe` or Windows SDK: use Developer PowerShell and check
  the Visual Studio workload. Native Windows does not use Unix `cc` or
  `pkg-config` for the Rust-only command above.
- Missing tkrzw headers (`pkg-config ... tkrzw` failed), `db.h`, libclang
  or native libraries: repeat the prerequisite checks of your OS.
  The error identifies the missing header or library. If package installation is restricted,
  ordinary users need the environment owner. That
  owner receives the exact failing command and the complete error. On Linux,
  `pkg-config --modversion glib-2.0` and `pkg-config --modversion tkrzw`
  check discovery. The setup check prints the backend version and the missing-libclang diagnostic of bindgen,
  and those lines go with the error too.
- macOS cannot find tkrzw: repeat the `PKG_CONFIG_PATH` export
  above. For Berkeley DB, repeat the `SYSTEM_DEPS_BDB_*` exports
  ([backends](backends.md#discovery-and-overrides)). A successful
  Clippy/check is not a link test.
- Missing oracle or model inputs do not block ordinary native tests. For a
  selected compatibility check, a missing input is a failure
  ([test inputs](../testing/README.md#test-inputs)).
