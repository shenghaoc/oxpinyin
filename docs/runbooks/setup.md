# Native setup

This guide is for contributors. It sets up native builds, tests, fuzzing and the contribution checks.

oxpinyin builds and runs natively on Linux x86_64, macOS arm64 and Windows x86_64.
This procedure sets up native development on each of them.
It starts from [CONTRIBUTING](../../CONTRIBUTING.md). These instructions build
and test local files. They do not install an input method or replace your
system libpinyin. A failed command stops the setup; that command and its full
error are kept before the work continues. In Bash, `echo $?` immediately after a command
shows its exit status (zero means success); PowerShell uses `$LASTEXITCODE`.
Each operating system has its own section below. Ordinary native work and native-only
measurements require no container, oracle or model20 (upstream reference model data).
Pin-comparison measurements run natively on a Linux host, with no container
([where measurements run](benches.md#where-measurements-run)).

## Storage backend

The [backend selection rule](../architecture.md#backend-selection) states the default backend.
tkrzw is the pin oracle's [reference cell](verification.md#1-reference-cell).
[Verification rule 10.2](verification.md#10-ci-merge-conditions) states the backend of Linux CI.
The build finds tkrzw through pkg-config, and the macOS and Windows sections below install it.
The [backends runbook](backends.md) gives the commands for the other backends.

Native Windows recipes use pwsh with `-CommandWithArgs`; doctor checks its
declared minimum.

The Rust **toolchain** is the compiler and its tools. rustup installs
and selects it. `rust-toolchain.toml` selects the version and components. The workspace `rust-version` in `Cargo.toml` declares the minimum supported Rust version. The MSRV is the pinned stable toolchain. The nightly in `fuzz/rust-toolchain.toml` also builds the workspace; that file states why the nightly is pinned. Toolchain bumps are manual: a pull request changes the toolchain files, and CI checks it.
The **linker** joins compiled code into executables and native libraries.

## Linux

**Backends on Linux.** Locally, Linux supports kc, bdb and tkrzw, and follows the
[default backend](../architecture.md#backend-selection). tkrzw uses the distribution's libtkrzw,
which must pass [verification rule 9](verification.md#9-sound-tkrzw-library).

With no backend named, the local recipes use tkrzw:

- `just test` and `just clippy` build the tkrzw backend. On a defective library,
  `tkrzw_library_honours_its_sentinel_protocol` fails and names the kc and bdb selections.
- `just sanitize` runs the tkrzw gate on the host libtkrzw; `just sanitize all` runs the kc,
  bdb and tkrzw gates.
- `just provision` finds the host libtkrzw, and `just provision-oracle` builds the tkrzw oracle
  prefix natively against it. `PINYIN_ORACLE_DBM=kc` (or `bdb`) selects another cell for both,
  and the oracle environment file then holds that cell.
- The measurement recipes and the oracle reproducibility check use the tkrzw cell; `--cells` and
  `--cell` select the others ([benches](benches.md#cells-and-settings)).

To select another backend, pass `--no-default-features --features kyotocabinet` (or `bdb`) to
`just test`, `just clippy` or Cargo. A [pin capture](pin-captures.md) names its cells with
`--cells`, which the Debian deps image builds and checks. Berkeley DB stays a CI peer, tested
by the `store` job.

Linux setup needs:

- Git, curl and CA certificates;
- a C/C++ compiler and linker, Make and pkg-config;
- tkrzw development files, for the tkrzw cell;
- Kyoto Cabinet development files and utilities, for the kc cell;
- GLib development files.

Berkeley DB **5.3** is needed only to build the `bdb` backend, and libclang
with it: bindgen generates the Berkeley DB bindings. The tkrzw and Kyoto Cabinet
bridges use cxx. [Building with Berkeley DB](#building-with-berkeley-db) installs both.
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
sudo apt-get install -y build-essential git curl ca-certificates pkg-config libglib2.0-dev \
  libtkrzw-dev libkyotocabinet-dev kyotocabinet-utils
```

```bash
# Systems using dnf:
sudo dnf install gcc gcc-c++ make git curl ca-certificates pkgconf-pkg-config glib2-devel \
  tkrzw-devel kyotocabinet-devel kyotocabinet
```

The build discovers tkrzw and Kyoto Cabinet through `pkg-config tkrzw` and
`pkg-config kyotocabinet`; the distribution's libraries are on the default loader path, so
no path export is needed. GLib stays on the system package manager; the [macOS](#macos) and
[Windows](#native-windows) sections name those platforms' tkrzw installation, and
Windows alone uses vcpkg. Check the backend with `just test`, which runs tkrzw on Linux:

```bash
just test
```

For `just abi`, install libclang (`libclang-dev` on apt, `clang-devel` on dnf)
and the nightly named in `fuzz/rust-toolchain.toml`. The test uses libclang to
read both C headers and nextest to run the AddressSanitizer cases.

```bash
(cd fuzz && rustup toolchain install)
just abi
```

Release packaging and oracle/reference images keep Debian's tkrzw, in their own
containers; see [tkrzw runtime enforcement](cloud.md#runtime-enforcement-verification-rule-9)
for how `tools/tkrzw/runtime.py` checks the library they resolve.

### Building with Berkeley DB

A build with the `bdb` feature needs Berkeley DB 5.3 and libclang:

```bash
# Systems using apt:
sudo apt-get install -y libdb-dev libclang-dev
```

```bash
# Systems using dnf:
sudo dnf install libdb-devel clang-devel
```

Then run `cargo nextest run --locked --no-default-features --features bdb`
([backends](backends.md)).

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

Install cargo-c when you build the shipping libraries. Use a version at or above
the minimum that `just doctor` enforces.

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
uv run --managed-python python -X utf8 -m tools.dev.provision --dev-tools
```

`--dev-tools` keeps each of just, cargo-nextest, cargo-c, cargo-deny and the rust-analyzer
component that meets its requirement. It installs the others: just and cargo-c
with `cargo install` at their pinned versions, cargo-deny with `cargo install` at
the version in `.github/actions/bootstrap/cargo-deny-version.txt` (also when the installed
one is older), cargo-nextest with `cargo install` at its newest release, and rust-analyzer with
`rustup component add`. It skips the Rust and Valgrind checks and the tkrzw install of
`just provision`, which runs the same tool step. Neither starts a container. It needs Cargo or
rustup only for a tool that it must install; without that program it exits 1 and
names the tool. `--python-only` alone installs none of these tools and needs
neither program. See [cloud setup](cloud.md#python-only-toolsdev-hosts).

Install cargo-shear locally at CI's pinned version before `just shear`:

```sh
cargo install --locked cargo-shear --version \
  "$(cat .github/actions/bootstrap/cargo-shear-version.txt)"
```

Repository Python uses the managed CPython named in `.python-version`.

`just doctor` reports the tools it finds, including their paths and versions.
On Linux, doctor also checks GNU ld, gcov and Valgrind.

The native command interface needs just at or above the `minimum-version` in the
root `justfile`. `just` enforces this itself, and `just doctor` reports it.
just comes through the route of the host: `brew install just` on macOS, the
distribution package on Linux, or `cargo install just --locked`.

The first `uv run` provisions the interpreter. uv reads `.python-version` and,
when that interpreter is missing, downloads it; `uv.toml` permits only managed
interpreters. No recipe runs `uv python install`.
On Windows, that command deletes and creates uv's minor-version junction again
at every call, and a Python that starts through the junction in that moment
cannot import its standard library. `just doctor` installs nothing, and names
`uv python install` when the interpreter is missing.

`.python-version` is the one place where the interpreter request is written.
Inside the checkout, `uv run --managed-python python` and
`uv python find --managed-python` read it. A script that changes directory
first, or a container build, copies the file in or passes its contents on; it
does not write the version again. `pyproject.toml` declares the minimum
supported version in its `requires-python` field; uv refuses to run when the
request does not satisfy it. The CI Python version check fails when a
version request or a second declaration appears anywhere else, outside
`docs/findings/`.
A `tools.dev` module runs from the checkout with the managed interpreter and
the invocation in the `justfile`.

Only this interpreter is used: no system Python. Commands use `-X utf8`.

The repository's Python tooling is one uv project with one lockfile.
`pyproject.toml` names the third-party tools in dependency groups, each with
its minimum version. `uv.lock` records the exact version of each tool and of
its transitive dependencies. The project is not built or installed: uv
installs only the dependencies, into `.venv` in the checkout. `uv.toml` is
the only uv configuration file.

Two alternatives are rejected:

- `uv run --with`: it locks no transitive dependencies.
- A PEP 723 lockfile for each script: one lockfile per tool, and no
  Dependabot updates.

`just build`, `just clippy` and `just fmt` pass Cargo arguments to
Cargo unchanged ([limit parallel builds](verification.md#8-builds) with
`CARGO_BUILD_JOBS`). The selection of native packages and backend is the same as for the Cargo
command that the recipe replaces:

```bash
just build --locked -p oxpinyin
just clippy --locked -p oxpinyin --all-targets -- -D warnings
just fmt --all --check
```

### Native test entry points

A full test run needs one `just fetch-contract-data` beforehand: the
`candidate_surface` test of `oxpinyin` reads the pinned data that it
fetches ([test inputs](../testing/README.md#test-inputs)).

cargo-nextest is the only supported test runner. It runs each test in its
own process, which the suite relies on, and its `terminate-after` limit in
`.config/nextest.toml` is the only limit on a hung test: the test helpers set
none. Plain `cargo test` has neither, so the commands below use nextest.

The native Rust test entry point uses cargo-nextest at or above the
`nextest-version` in `.config/nextest.toml` ([profile inheritance](https://nexte.st/docs/configuration/#profile-inheritance)
needs it); `just doctor` reports it. `cargo install
cargo-nextest --locked` (or the prebuilt release) installs it. `just test` runs `cargo nextest run --profile local`
with the given arguments, which take the same crate and
feature selection as other Cargo commands (see [backend selection](backends.md)). `--profile ci`
selects the CI limits.

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
cargo nextest run --locked -p oxpinyin
just test
```

The `cargo nextest run` command tests native Rust packages. `just test` runs the nextest
suite with Cargo's default backend, tkrzw, unless the arguments name another; it also links
and runs storage and C-library tests. Headers
are the native library declarations that the compiler uses. For `bdb`, bindgen
also reads `db.h` with libclang. pkg-config locates native library installations.

Success means that all test summaries have zero failures. Negative
tests can deliberately print warnings, assertions, `failed` or `CRITICAL`.
The final summaries and the process exit status decide success.

In a fresh Bash terminal, use your checkout directory if it is different:

```bash
cd ~/src/oxpinyin
. "$HOME/.cargo/env"
just test
```

Cargo reuses downloaded dependencies and the `target/` outputs of that checkout.
Outputs stay separate across worktrees (separate checkouts of branches).

### Fuzzing

AFL++ fuzzing ([fuzz harnesses](../testing/upstream-test-coverage.md#fuzz-harnesses))
is supported on Linux only, as in libchewing, and the fuzz soak runs in the
Linux nightly. On macOS, each input's new process costs about 200 ms in
`fork()`, and the default System V shared-memory limit (4 MiB per segment,
`kern.sysv.shmmax`) blocks CmpLog's 144 MiB map, so a native macOS run would
only be a smoke test. AFL++ does not support Windows. `just fuzz-setup`,
`just fuzz-minimize` and `just fuzz` are Linux recipes, so just does not find
them on macOS or Windows. Run directly on any host other than Linux,
`tools/dev/fuzz.py` refuses with one message that names this section.

`just fuzz-setup` installs cargo-afl at the version that `fuzz/Cargo.lock`
resolves for the `afl` crate, and the AFL++ release that it bundles, with
its LLVM plugins. The plugins build against an LLVM of the fuzz nightly's
major version, LLVM 22: the
`llvm-config-22` of `llvm-22-dev`, which Ubuntu 26.04's archive carries and
which needs no clang, or the `llvm-config` that `LLVM_CONFIG` names.
afl-fuzz refuses a `core_pattern` that pipes crashes to a handler, so set it to
`core` first, as the nightly does:

```bash
sudo apt-get install llvm-22-dev
echo core | sudo tee /proc/sys/kernel/core_pattern
(cd fuzz && rustup toolchain install)
just fuzz-setup
just fuzz chewing-parser -V 60
FUZZ_CMPLOG=1 just fuzz capi-commands -V 60
```

#### From macOS or Windows

Fuzz in a native Linux container of the host's own architecture, with no
emulation:

- On an Apple silicon Mac, a native arm64 Linux container: Apple `container`,
  Podman or Docker with `linux/arm64`. Use no amd64 image and no Rosetta. In
  the VM of Apple `container`, `core_pattern` is already `core`.
- On an amd64 Windows host, a native amd64 Linux container: WSL or `wslc`.
  Use no arm64 image.

In the container, install the [Linux](#linux) build dependencies, rustup, uv
and just, then run the commands above from the checkout. Give the container
its own cargo target directory (`CARGO_TARGET_DIR`), so its Linux build
scripts do not mix with the host's in `fuzz/target`.

## macOS

macOS setup uses Terminal, Git, Apple's
[Command Line Tools](https://developer.apple.com/xcode/resources/),
[Homebrew](https://docs.brew.sh/Installation) and rustup. If the Command Line
Tools are absent, run `xcode-select --install` and complete the system dialog.

```bash
brew install glib pkgconf tkrzw
mkdir -p ~/src
cd ~/src
git clone https://github.com/shenghaoc/oxpinyin.git
cd oxpinyin
```

On macOS, use Homebrew's tkrzw. `pkg-config tkrzw` selects its headers and
library. Homebrew's `tkrzw.pc` names lz4 and zstd without their library search
paths; the build also probes `liblz4.pc` and `libzstd.pc` through `system-deps`.
No `VCPKG_ROOT`, `PKG_CONFIG_PATH`, `LIBRARY_PATH` or dyld override is needed.
The nightly macOS job installs `tkrzw` with Homebrew; Homebrew installs its
compression dependencies. Check the installed modules with:

```bash
pkg-config --modversion tkrzw liblz4 libzstd
```

Get uv as [Installing uv and cargo-c](#installing-uv-and-cargo-c) describes, then
run `brew install just`.
Then, if rustup is absent, install it with the Linux rustup commands above,
and run:

```bash
. "$HOME/.cargo/env"
rustup show
cargo nextest run --locked -p oxpinyin
export CARGO_TARGET_DIR=target/tkrzw
just test
```

The `cargo nextest` command is a real storage link/run check; a `cargo check`
alone does not run the tests. In a fresh terminal, go back to the checkout,
source the Cargo environment, and repeat the check.

The nightly macOS CI job runs
`cargo nextest run --locked --workspace`. Berkeley DB is a non-default
backend. To use it:

1. Run `brew install berkeley-db@5`.
2. Export the keg-only `SYSTEM_DEPS_BDB_*` overrides
   ([backends](backends.md#discovery-and-overrides)).
3. Run `export CARGO_TARGET_DIR=target/bdb`.
4. Run `cargo nextest run --locked --no-default-features --features bdb`.

The target and user-data directories of each backend stay separate.

The backend sanitizer gates run natively on macOS. The recipes select Homebrew's Berkeley DB 5.3, Kyoto Cabinet and tkrzw. They use the one
nightly that `fuzz/rust-toolchain.toml` names, with `rust-src`, and no Linux
`setarch`. In `fuzz/`, rustup installs the toolchain that the file names:

```bash
brew install berkeley-db@5 kyoto-cabinet
(cd fuzz && rustup toolchain install)
just sanitize bdb address
just sanitize bdb thread
just sanitize kc address
just sanitize kc thread
just sanitize tkrzw address
just sanitize tkrzw thread
```

AFL++ fuzzing does not run natively on macOS; see [fuzzing](#fuzzing) for
the reason and for fuzzing in a native Linux container.

In the sanitizer gates, AddressSanitizer runs with leak detection off on
Darwin. Standalone LeakSanitizer is refused on macOS arm64, and
MemorySanitizer has no Darwin target. Both refusals name the host. Linux
keeps its existing sanitizer commands and paths.

### Nightly macOS and Windows jobs

The macOS arm64 and Windows x86_64 workspace jobs are `test-macos` and `test-windows` in
`verify-nightly.yml`. Their schedule and merge status are in
[verification rule 10](verification.md#10-ci-merge-conditions); pull-request CI
tests Linux x86_64 only. A failure on one platform is reproduced by dispatching the
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
[pwsh-floor.txt](../../tools/dev/pwsh-floor.txt). Just recipes are called from pwsh.
Windows PowerShell can change
empty arguments and embedded quotes before just receives them.

The repository tracks symbolic links, so enable git symlink support before you clone: turn on Developer Mode, or make the clone from an elevated shell (Run as administrator), and run `git config --global core.symlinks true`.

Repository Python tooling on Windows runs through the pwsh recipes.
AFL++ does not support Windows: the fuzz recipes are Linux recipes and are
not available on Windows ([fuzzing](#fuzzing)).
Install uv with its native Windows installer, then just (`cargo install just
--locked`). The interpreter rule of the Linux section applies unchanged.

Clone the repository in pwsh:

```powershell
New-Item -ItemType Directory -Force "$HOME/src" | Out-Null
Set-Location "$HOME/src"
git -c core.autocrlf=false clone https://github.com/shenghaoc/oxpinyin.git
Set-Location oxpinyin
rustup show
```

Rustup selects the repository toolchain automatically.

The core library requires GLib and the default backend requires tkrzw.
Every build and test needs them:

- GLib and pkgconf come from vcpkg. Nothing packages tkrzw for Windows, so
  the repository builds it: `just tkrzw` (`tools/tkrzw/windows.py`) fetches the pinned
  revision (`tools/tkrzw/pin.json`) and builds the library target of upstream's
  `VCMakefile` through nmake. It overrides one flag: the DLL C runtime (`/MD`)
  in place of upstream's static one, matching the Rust MSVC targets and
  oxpinyin's C++ bridge. The result is a static `tkrzw.lib`; no tkrzw DLL exists.
- The install is `include`, `lib\tkrzw.lib` and `lib\pkgconfig\tkrzw.pc` under
  the cache root's `tkrzw-<revision>` entry ([cache directory](#cache-directory)),
  or under `--prefix`. The archive is linked into every binary, so no tkrzw
  directory goes on PATH.
- `$env:VCPKG_ROOT` names the vcpkg checkout. The repository `vcpkg.json`
  manifest names GLib and pkgconf; the install command selects the `x64-windows`
  triplet. The commands below run from the repository root in a plain pwsh
  session: the tkrzw build finds MSVC by itself through vswhere. Do not use the
  stock Developer PowerShell for VS: it targets x86, and `just tkrzw` refuses it.

Install the libraries and set the environment:

```powershell
& "$env:VCPKG_ROOT\vcpkg.exe" install --triplet x64-windows
$installed = "$PWD\vcpkg_installed\x64-windows"
just tkrzw
$env:PKG_CONFIG = "$installed\tools\pkgconf\pkgconf.exe"
$env:PKG_CONFIG_PATH = "<the PKG_CONFIG_PATH entry just tkrzw printed>;$installed\lib\pkgconfig"
$env:PATH = "$installed\bin;$env:PATH"
& $env:PKG_CONFIG --modversion glib-2.0 tkrzw
```

`just tkrzw` ends by printing the `PKG_CONFIG_PATH` entry of its install,
and reuses a complete install on the next run. vcpkg's `bin` puts the GLib
DLLs on the path of the test binaries; tkrzw needs no path entry.

Then test and lint the library:

```powershell
cargo nextest run --locked -p oxpinyin
cargo fmt --all --check
cargo clippy --locked -p oxpinyin --all-targets -- -D warnings
```

Success is zero failed tests and exit code zero (`$LASTEXITCODE`). In a fresh
pwsh session, run `Set-Location "$HOME/src/oxpinyin"` (or your real
checkout directory), run the `$installed` and three `$env:` lines again, and repeat the test.
Editors and debuggers use this same native MSVC
environment and toolchain; no project-specific IDE is required.
`just test` and a full local build need the same environment.

**Native Windows CI scope:** the Windows CI job runs clippy,
`cargo nextest run --profile ci` over the portable
workspace on the [default backend](backends.md).
The job finds GLib and tkrzw through vcpkg's `pkgconf` and one
`PKG_CONFIG_PATH`. It therefore builds and tests
the full workspace, including `oxpinyin-utils`, KMM
and both C-ABI crates. The locale tests select `.UTF-8` on Windows.
Text expectations use the newline of the host.
The `test-windows` job in `verify-nightly.yml` excludes no crate. The Linux FFI feature of the oracle is
not selected, so those tests do not establish native Windows oracle behavior.

Windows is a supported native target; the frozen oracle does not define which
native operating systems oxpinyin can support. To dispatch the nightly Windows
job alone, see [nightly macOS and Windows jobs](#nightly-macos-and-windows-jobs).

### Native Windows and WSL containers

Every recipe has a Windows form, or prints why it is refused on Windows; see the `justfile`.

The pin runs only in a Linux container. On Windows, that container is a
[WSL container](https://learn.microsoft.com/en-us/windows/wsl/wsl-container)
driven by `wslc`, the first runtime that `select_runtime` tries there. `$env:OXPINYIN_CAPTURE_RUNTIME` or `$env:OXPINYIN_CONTAINER` names another runtime:

| Task | Command |
| --- | --- |
| Build and verify the oracle image | `uv run --managed-python python -X utf8 -m tools.oracle.image build-image` |
| Verify an oracle image | `uv run --managed-python python -X utf8 -m tools.oracle.image verify-image --engine wslc IMAGE` |
| Pin capture ([pin-captures.md](pin-captures.md)) | `just capture-pin RUN` |
| Provision the pin oracle in containers, and write its environment file | `just provision-oracle` |
| Native provisioning, no container ([cloud.md](cloud.md)) | `just provision` |

`tools/oracle/image.py` translates the docker commands for `wslc`
([container runtimes](#container-runtimes)). Docker and podman get the
commands unchanged. These `wslc` limits apply:

- **No `--platform`.** The adapter drops the flag; every image runs at the
  host architecture in any case ([container runtimes](#container-runtimes)).
- **`inspect` prints JSON only.** It has no Go templates, so
  `Architecture` and `Id` are read from the JSON record
  (`uv run --managed-python python -X utf8 -m tools.oracle.image image-field`).
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
- **One cell at a time.** `wslc` 3.0.5.0 fails a run with "Catastrophic failure"
  (`E_UNEXPECTED`) when containers that bind the same host directories start at
  the same moment. Provisioning therefore runs the library and data builds one
  cell at a time on `wslc`, each with the whole CPU budget.
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
- `tkrzw-<revision>`: the static tkrzw of `just tkrzw` ([native Windows](#native-windows)).
- `pin-<commit>`: the provisioning cache, with the pin prefixes and their build trees.

`oracle.env`, the [environment file](cloud.md#the-environment-file) of `just provision-oracle`,
is also in the root. It carries no pin in its name: every run rewrites it.

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
prints the runtime that it chose on a `Runtime:` line. Oracle provisioning
(`just provision-oracle`), `verify-image` and `build-image` work through each
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

`just provision-oracle` copies the libraries out of the published image, or builds
them in the `deps` image when no published image matches, and then regenerates the
data in the `deps` image. On rootful Docker, these runs are the invoking user, so
the cache stays theirs, and their home is `/tmp`. They use the `pin-<commit>` entry under
`OXPINYIN_CACHE` ([cache directory](#cache-directory)). `just provision` starts no
container. Some host steps of both recipes run only on Linux
([cloud provisioning](cloud.md#what-it-does)).

## Development debug information

Normal builds and tests keep file and line numbers for workspace code.
Panic backtraces show these locations when `RUST_BACKTRACE=1` is set.
External dependencies have no debug information in these builds.
Types and local variables need the `dev-full` profile:

```bash
cargo build --locked --profile dev-full -p oxpinyin-capi
cargo test --locked --profile dev-full -p oxpinyin --no-run
```

The debugger reads the files in `target/dev-full/`.
This profile includes full debug information for workspace code and dependencies.
It keeps the normal development optimization level, debug assertions and overflow checks.
It does not change release or profiling builds.

Verification cells continue to use `dev` at optimization level 1.
Their identity record still names that profile and level.
A `dev-full` build does not go in a claimed cell target directory.
A diagnostic runner that accepts `OXPINYIN_CAPI_SO` gets a separate build
and the path to `target/dev-full/liboxpinyin_capi.so`.

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
  Windows, open a fresh pwsh session after you install rustup. A
  permission panic from an installed rustup is an environment access problem:
  the error goes to the environment owner, and rustup is not reinstalled over it.
- Missing MSVC `link.exe` or Windows SDK: the Visual Studio C++ build tools or
  the Windows SDK are not installed. Install them. rustc finds them through the
  Visual Studio installer and the tkrzw build through vswhere, so no Developer
  PowerShell is needed. The stock Developer PowerShell for VS targets x86, and
  `just tkrzw` refuses it.
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
