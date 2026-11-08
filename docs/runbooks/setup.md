# Native contributor setup

Pin-comparison campaigns use the local oracle `performance` stage; see
[where measurements run](benches.md#where-measurements-run). Native development
and native-only measurements are unchanged. The oxpinyin-only nightly snapshot
uses plain unpinned Debian testing, like every other Linux environment.


Start from [CONTRIBUTING](../../CONTRIBUTING.md). These instructions build
and test local files; they do not install an input method or replace your
system libpinyin. Stop if a command fails and keep that command and its full
error before continuing. In Bash, `echo $?` immediately after a command
shows its exit status (zero means success); PowerShell uses `$LASTEXITCODE`.
Choose your operating system below. Ordinary native work
requires no container, oracle or model20 (upstream reference model data).

## Storage backend

Storage-dependent crates build against one DBM backend per build. The
[default backend](backends.md) is the reference cell on every platform. The build finds tkrzw through pkg-config;
the platform sections below install it. Berkeley DB 5.3 and Kyoto Cabinet
are non-default backends ([backends](backends.md)).

Every repository shell script requires Bash 5.0 or newer on Linux and macOS.
`just doctor` reports the `bash` that `env bash` selects and fails below 5.0.
Linux distributions provide it; the macOS section installs Homebrew's Bash and
checks the selection. On Windows doctor reports Bash as not applicable and
never fails on its version. Native Windows recipes use pwsh with
`-CommandWithArgs`; doctor checks its declared minimum.

Rust's **toolchain** consists of the compiler and its tools. rustup installs
and selects it; `rust-toolchain.toml` is the version/component authority.
The **linker** connects compiled code into executables and native libraries.

## Linux

Use Bash 5.0 or newer. Linux is one native platform target. You need Git,
curl and CA certificates, a C/C++ compiler and linker, Make, pkg-config,
libclang, tkrzw development files (the [default store backend](backends.md)) and GLib development
files. Berkeley DB **5.3** and Kyoto Cabinet are needed only to build those
non-default backends.
A development package supplies the headers and libraries needed to
compile against a native library. Package names depend on your system; that
does not change the build or create another compatibility environment.
Repository Python does not use the system interpreter; provision it with uv
as described below.

If these prerequisites are missing, install them with your system's package
manager. `sudo` asks for administrator permission to install system packages.
These are common package-manager examples, not separate supported/tested
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

The build finds tkrzw only through pkg-config (`pkg-config tkrzw`), as
libpinyin's configure does. Debian's `tkrzw.pc` names `-llzma -llz4 -lzstd
-lz`, but `libtkrzw-dev` does not depend on their development packages, so
the apt line installs them; without them every link fails with `cannot find
-llzma`. To use a tkrzw you built yourself instead, put its `lib/pkgconfig`
first on `PKG_CONFIG_PATH` and its `lib` on `LD_LIBRARY_PATH`;
`just tkrzw-build DIR` builds the upstream revision CI uses into `DIR`.
Ubuntu's packaged libtkrzw is defective
([`tkrzw-distro-compat.md`](../findings/tkrzw-distro-compat.md));
`tools/tkrzw/distro-probe.sh` checks the one on `PATH`, and provisioning runs
it on an Ubuntu host. A `just tkrzw-build` build, Debian's package and the
MSVC `tkrzw.lib` need no probe.

Repository Python uses uv and its managed CPython 3.15 series with the GIL.
`uv.toml` sets uv's floor, 0.13.1; CI installs the newest uv that satisfies it.

`just doctor` is the one check of every tool's minimum: Bash 5.0, uv,
CPython, just, cargo-nextest, cargo-c, cargo-fuzz, cargo-llvm-cov, and
ShellCheck 0.11.0. ShellCheck is advisory; run it when a shell script
changes ([verification rule 5](verification.md#5-local-test-gate)).
On Linux it also checks GNU ld, gcov and Valgrind. Each line names the resolved
executable, the minimum and the version found. A newer tool passes, an older
required tool fails, and a missing one comes with its install command.
Semantic versions compare as semver does: a pre-release of the minimum
(`1.59.0-beta.1`) is older than it and build metadata (`+build`) does not
count. GNU ld, gcov and Valgrind compare by number, and a development build of
one counts as its number. Install or upgrade a tool only when `just doctor`
says it is missing or too old.

Until uv exists, `just doctor` cannot run. Install uv only
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
root `justfile`, which `just` enforces itself and `just doctor` reports.
Install it through the host route: `brew install just` on macOS, the
distribution package on Linux, or `cargo install just --locked`.

Provision the interpreter through `just python`. It runs `uv python install --quiet`,
which reads `.python-version` and does nothing when that interpreter is present.
A plain version request prefers the GIL-enabled build, and the check below rejects
a free-threaded one. Every other Python recipe depends on it, so a recipe
run on a bare host provisions the interpreter itself; `just doctor` installs
nothing and names `just python` when the interpreter is missing. Then verify
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

`.python-version` is the one place the interpreter version is written. Inside the
checkout, `uv run --no-project --managed-python python` and `uv python find
--managed-python` read it; a script that changes directory first, or a container
build, copies the file in or passes its contents on, and does not restate the
version. From outside the checkout, pass `--python` the contents of the checkout's
`.python-version`, then pass `tools/dev/__main__.py` by absolute path to that
interpreter (with `-X lazy_imports=normal`), followed by the subcommand and its
arguments. This preserves the caller's working directory; its `.python-version`
does not decide the interpreter. Direct-run Python
harness files rely on their `uv` shebang and a working directory inside the
checkout. The check `tools/dev/python-request.test.py` fails when a version request
appears anywhere else, outside the dated records in `docs/findings/`.

**Rule (#1230).** Provision the repository's managed Python interpreter
through `just python`. Do not substitute another interpreter. A later series is a deliberate change: the tooling
relies on 3.15's `-X lazy_imports` modes. Do not use a system Python, a virtual
environment, `pyproject.toml` or `uv.lock`; the checkout's `.python-version`
and `uv.toml` name the same interpreter series and uv floor.

To regenerate the W3 content fixtures, run this command from the checkout root:

```bash
uv run --no-project --managed-python python -X lazy_imports=normal \
  tools/generate_w3_fixtures.py
```

`just build`, `just clippy` and `just fmt` pass Cargo arguments unchanged to
Cargo. To limit parallel builds, set `CARGO_BUILD_JOBS` or `[build] jobs` in
your own `~/.cargo/config.toml`. Select the same native packages
and backend as the Cargo command you would otherwise run:

```bash
just build --locked -p oxpinyin-core
just clippy --locked -p oxpinyin-core --all-targets -- -D warnings
just fmt --all --check
```

### Registry listing and test entry points

`just test-tools` runs every non-excluded Python harness file, concurrently up
to the host's allowed CPU count. Each file has private temporary and lock
directories; output appears as complete blocks in path order, and failures do
not prevent the remaining files from running. The same managed interpreter runs
the controller and each file.

For iteration, pass files and optionally a unittest name pattern:

```sh
just test-tools tools/dev/test_runner.test.py
just test-tools tools/dev/test_runner.test.py -k HarnessTests
just test-tools --changed
just test-tools --changed origin/main
```

`--changed [BASE]` compares tracked files against BASE (HEAD by default),
including staged and unstaged edits, and includes untracked files. Changed test
files select themselves; `HARNESS_DEPENDENCIES` in `tools/dev/test_runner.py`
maps known modules to their tests. Any unknown path or missing mapped test
selects the full suite. An empty diff selects no files. Explicit files and
`--changed` cannot be combined. Every mode retains the credential filter,
existing exclusions. Memory-backed TMPDIRs are refused only when the selection
includes `tools/bisection/teardown.test.py`, whose cold tests need real eviction.

`--changed` is iteration feedback only: `HARNESS_DEPENDENCIES` is hand-maintained
and can under-select. The full `just test-tools` remains the completion gate.
With `-k` and no explicit FILE, files with no matching tests are reported as
unselected. A named file with no matches remains a failure, as does a pattern
that matches no tests across all selected files.

The surviving checks run through their own recipes or cargo commands.

The Windows recipes use PowerShell and preserve the child status explicitly.
Their argument transport remains TODO (no Windows host).

Run `just fetch-contract-data` once before a full test run: `oxpinyin-runtime`'s
`candidate_surface` test reads the pinned system data it fetches, and fails without it.

The native Rust test entry point uses cargo-nextest at or above the
`nextest-version` in `.config/nextest.toml` ([profile inheritance](https://nexte.st/docs/configuration/#profile-inheritance)
needs it); `just doctor` reports it. Install it with
`cargo install cargo-nextest --locked` (or the prebuilt release). `just test` runs `cargo nextest run --profile local`
with your arguments. Pass the same crate and
feature selection as cargo test (see [backend selection](backends.md)). Pass `--profile ci` for the CI limits. `just test-doc` runs
`cargo test --doc`.

Use your system's official package documentation if names or enabled
repositories differ. The native check below prints the tkrzw version
pkg-config reports.

In a managed environment with these packages already present, skip their
installation. Root development environments omit `sudo`. Without installation
rights, ask the environment owner for these packages; ordinary native work
does not require nested Docker or Podman.

If you already have a checkout, enter its root: the directory containing
`CONTRIBUTING.md` and `Cargo.toml`. Replace `/path/to/oxpinyin` below with
that actual directory, then skip the cloning block:

```bash
cd /path/to/oxpinyin
```

Otherwise obtain a checkout, then remain at its root:

```bash
mkdir -p ~/src
cd ~/src
git clone https://github.com/shenghaoc/oxpinyin.git
cd oxpinyin
```

Check `command -v rustup`. If it reports no path, install
[rustup](https://rust-lang.github.io/rustup/installation/index.html):

```bash
installer="$(mktemp)"
curl --fail --location --retry 3 https://sh.rustup.rs -o "$installer"
sh "$installer" -y --profile minimal --default-toolchain none
rm -f "$installer"
```

The `.` command below loads Cargo's PATH settings into the current shell,
so it can find `cargo` and `rustc`. With either a new or existing rustup
installation:

```bash
. "$HOME/.cargo/env"
rustup show
cargo test --locked -p oxpinyin-chewing -p oxpinyin-core -p oxpinyin-engine
just check
```

The `cargo test` command tests native Rust packages; `just check` (tkrzw by
default; `just check bdb` or `just check kyotocabinet` for the others)
additionally links and executes storage and C-library tests. Headers
are the native library declarations used by the compiler; libclang reads
them, and pkg-config locates native library installations. The check prints
the selected toolchain, architecture and backend. It installs nothing.

Success means all test summaries have zero failures; the storage check ends
with `Setup check passed`. Ignored tests need optional inputs. Negative
tests can deliberately print warnings, assertions, `failed` or `CRITICAL`;
use the final summaries and process exit status to determine success.

In a fresh Bash terminal, use your actual checkout directory if different:

```bash
cd ~/src/oxpinyin
. "$HOME/.cargo/env"
just check
```

Cargo reuses downloaded dependencies and that checkout's `target/` outputs.
Keep outputs separate across worktrees (separate checkouts of branches).

## macOS

Use Terminal with Bash 5.0 or newer (`bash`), Git, Apple's
[Command Line Tools](https://developer.apple.com/xcode/resources/),
[Homebrew](https://docs.brew.sh/Installation) and rustup. If Command Line
Tools are absent, run `xcode-select --install` and complete the system dialog.

macOS ships Bash 3.2 at `/bin/bash`. Install Homebrew's Bash and check that
`env bash` selects it:

```bash
brew install bash
env bash --version
command -v bash
```

`env bash --version` must print 5.0 or newer. `command -v bash` must print the
Homebrew path (`/opt/homebrew/bin/bash` on Apple silicon or
`/usr/local/bin/bash` on Intel), not `/bin/bash`. If it prints `/bin/bash`,
put Homebrew's `bin` directory before `/bin` on `PATH` in your shell profile
and open a new terminal.

```bash
brew install tkrzw glib pkgconf
mkdir -p ~/src
cd ~/src
git clone https://github.com/shenghaoc/oxpinyin.git
cd oxpinyin
```

Run the uv, just and interpreter commands from the Linux section, in that
order (`brew install just` supplies just). For the checksum step, replace
`sha256sum -c -` with `shasum -a256 -c -` on macOS.
Then install rustup if absent using the Linux rustup commands above
and run:

```bash
. "$HOME/.cargo/env"
rustup show
cargo test --locked -p oxpinyin-chewing -p oxpinyin-core -p oxpinyin-engine
export LIBRARY_PATH="$(brew --prefix)/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
export CARGO_TARGET_DIR=target/tkrzw
just check
```

`export` sets a setting inherited by commands in this terminal. pkg-config
finds Homebrew's tkrzw, the selected backend; its `tkrzw.pc` names
the lz4, lzma and zstd libraries without their directory, which
`LIBRARY_PATH` supplies. To use a tkrzw you built yourself (`just tkrzw-build
DIR`), put `DIR/lib/pkgconfig` first on `PKG_CONFIG_PATH` and `DIR/lib` on
`DYLD_LIBRARY_PATH` instead. This is a real storage link/run check; a mere
`cargo check` does not execute the tests.
In a fresh Bash terminal, return to the checkout, source Cargo's environment,
repeat both exports and the check. Linux C-ABI packaging and Linux reference
differentials are separate evidence, not permanent restrictions on portability.

The nightly macOS CI job uses this broader portable scope:

```bash
cargo test --locked --workspace
```

The [validation status](#validated-on--not-validated) distinguishes native macOS CI
from hands-on setup evidence.

CI's macOS job builds tkrzw with `.github/actions/tkrzw` instead of
Homebrew's. Berkeley DB is a non-default backend: `brew install berkeley-db@5`,
export the keg-only `SYSTEM_DEPS_BDB_*` overrides
([backends](backends.md#discovery-and-overrides)),
`export CARGO_TARGET_DIR=target/bdb`, then `just check bdb`.
Keep each backend's target and user-data directories separate.

Fuzzing and the backend sanitizer gates run natively on macOS. The recipes
select Homebrew's Berkeley DB 5.3, Kyoto Cabinet and tkrzw and use the one
nightly that `fuzz/rust-toolchain.toml` names, with `rust-src`; no Linux
`setarch`. In `fuzz/`, rustup installs the toolchain that file names:

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

`capi-commands` also runs natively; pkg-config finds the facade's GLib and
tkrzw, and the fuzz recipe adds Homebrew's lib to `LIBRARY_PATH`. AddressSanitizer runs with leak detection off on
Darwin. Standalone LeakSanitizer is refused on macOS arm64, and
MemorySanitizer has no Darwin target; both refusals name the host. Linux
keeps its existing sanitizer commands and paths.

### When the macOS job runs

The macOS workspace job is the `test-macos` job in `verify-nightly.yml`. It
runs every night and on manual dispatch, not on every pull request: platforms
other than Linux are unstable but not unusable (maintainer decision
2026-10-31). Pull-request CI tests Linux only. Before merging a change that
can behave differently on macOS (the cases listed for Windows below apply),
dispatch the job on the branch with `macos_only` set:

```bash
gh workflow run verify-nightly.yml --ref <branch> -f macos_only=true
```

With `macos_only` set, every other nightly job is skipped, the Windows job
included. A dispatch without it or `windows_only` runs the whole nightly
workflow, including the macOS job.

## Native Windows

This is native Windows Rust development, not Linux under WSL. Install Git
and [rustup's MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html):
Visual Studio Build Tools with **Desktop development with C++** and the
Windows SDK. [Microsoft's Rust setup](https://learn.microsoft.com/en-us/windows/dev-environment/rust/setup)
explains installation. Cargo finds MSVC through the Visual Studio installer;
MSVC does not need to be on PATH. Doctor uses the installer's fixed
`vswhere.exe` location and requires the VC tools component.
Install rustup with the official Windows installer
if it is not already available.

Use pwsh at or above the minimum in
[pwsh-floor.txt](../../tools/dev/pwsh-floor.txt). Call just recipes from pwsh,
including in a Developer PowerShell session. Windows PowerShell can change
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
Job Object lifetime, fuzz, coverage and full tooling acceptance remain unverified.
Install uv with its native Windows installer, then just (`cargo install just
--locked`), then run `just python`; the rule above applies unchanged.

All three packages link and run natively without a DBM library or container.
Success is zero failed tests and exit code zero (`$LASTEXITCODE`). In a fresh
Developer PowerShell, `Set-Location "$HOME/src/oxpinyin"` and repeat the test;
use your real directory for an existing checkout. Rustup chooses the same
repository toolchain automatically. Editors/debuggers use this same native
MSVC environment and toolchain; no project-specific IDE is required.

**Storage-dependent native Windows scope:** the Windows CI job runs clippy,
`cargo nextest run --profile ci` and the doctests over the portable
workspace on the [default backend](backends.md). `.github/actions/tkrzw` builds the
upstream revision `tools/cloud/setup.sh` provisions with `nmake -f
VCMakefile` in the Visual Studio developer environment (a static
`tkrzw.lib` with the DLL C runtime that Rust's MSVC targets link) and
writes a `tkrzw.pc`, which the store build script finds through
`PKG_CONFIG_PATH`. The Windows image has no pkg-config, so the action
installs vcpkg's `pkgconf` and sets `PKG_CONFIG` to its `pkgconf.exe`.
To do the same
locally, run `just tkrzw-build DIR` from Developer PowerShell (it loads the
developer environment through `vswhere` when it is not already loaded),
put `DIR\lib\pkgconfig` on `PKG_CONFIG_PATH` and `DIR\bin` on `PATH`, and
set `PKG_CONFIG` to a `pkgconf.exe` (for example from `vcpkg install
pkgconf:x64-windows`). The recipe runs no `distro-probe.sh` and needs no `sh`.

The job also installs vcpkg's `glib` port, found through the same
`PKG_CONFIG` and `PKG_CONFIG_PATH`, so it builds and tests
the full workspace, including `oxpinyin-utils`, `oxpinyin-training`, KMM
and both C-ABI crates. The locale tests select `.UTF-8` on Windows.
Text expectations use the host newline. All 18 local KMM tests passed.
`WINDOWS_EXCLUDES` in `verify-nightly.yml` is empty. The oracle's Linux FFI feature is
not selected. Native Windows oracle behavior is not established by those
tests. Physical Windows hardware is not needed to use
this CI coverage.

A full local build or `just test` needs the same two vcpkg ports as the
CI job; without GLib, `glib-sys` fails and every crate but
`oxpinyin-chewing`, `oxpinyin-core`, `oxpinyin-store` and
`oxpinyin-engine` fails to build. From Developer PowerShell, with
`$vcpkg` your vcpkg checkout and `DIR` the `just tkrzw-build` prefix:

```powershell
& "$vcpkg\vcpkg.exe" install pkgconf:x64-windows glib:x64-windows
$installed = "$vcpkg\installed\x64-windows"
$env:PKG_CONFIG = "$installed\tools\pkgconf\pkgconf.exe"
$env:PKG_CONFIG_PATH = "DIR\lib\pkgconfig;$installed\lib\pkgconfig"
$env:PATH = "DIR\bin;$installed\bin;$env:PATH"
& $env:PKG_CONFIG --modversion glib-2.0 tkrzw
```

The `bin` directories put the GLib and tkrzw DLLs on the test binaries'
path. Bash scripts run from the same session through Git for Windows'
`bash.exe` (`& 'C:\Program Files\Git\bin\bash.exe' SCRIPT`).

**When the Windows job runs:** the full Windows workspace job is the
`test-windows` job in `verify-nightly.yml`. It runs every night and on manual
dispatch, not on every pull request. Windows remains a supported native
target; only the verification frequency changed. Before merging any of the
following, dispatch the job on the branch with `windows_only` set:

- changes to Windows-sensitive storage, filesystem or native-build code
  (the store backends and their build scripts, path and file handling,
  build scripts that compile or link native code);
- toolchain updates and relevant dependency updates (`rust-toolchain.toml`,
  `Cargo.lock` changes that reach the portable workspace, the tkrzw
  revision in `tools/tkrzw/build.py`);
- releases.

```bash
gh workflow run verify-nightly.yml --ref <branch> -f windows_only=true
```

With `windows_only` set, every other nightly job is skipped, the macOS job
included. A dispatch without it runs the whole nightly workflow, including
the Windows job.

Recent upstream MinGW portability changes were checked separately from
oxpinyin's MSVC support; see the [validation status](#validated-on--not-validated).
The frozen oracle does not define which native operating systems oxpinyin
can support, and its pins are unchanged.

### Native Windows and WSL containers

These run natively on Windows through the `[windows]` pwsh recipes: `build`,
`test`, `test-doc`, `fmt`, `clippy`, `fuzz`, `coverage`, `python`, `doctor`,
`fetch-contract-data`, `tkrzw-build`, `oracle-versions`, `rust-tables`,
`cpp-smoke`, `abi` and `cell-artifacts`. None
of them starts a container. `sanitize` is Linux and Darwin only.

The pin runs only in a Linux container. On Windows that container is a
[WSL container](https://learn.microsoft.com/en-us/windows/wsl/wsl-container)
driven by `wslc`, the first runtime `select_runtime` tries there. Run these from Developer
PowerShell; `$env:OXPINYIN_CAPTURE_RUNTIME` or `$env:OXPINYIN_CONTAINER` names another runtime:

| Task | Command |
| --- | --- |
| Build and verify the oracle image | `uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image build-image` |
| Verify an oracle image | `uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image verify-image --engine wslc IMAGE` |
| Pin capture ([pin-captures.md](pin-captures.md)) | `just capture-pin RUN` |
| Provision the oracle cells in the performance image | `just provision-oracle` |
| Cloud provisioning ([cloud.md](cloud.md)) | `just provision` |

`tools/oracle/image.py` translates the docker commands for `wslc`
([container runtimes](#container-runtimes)); docker and podman get the
commands unchanged. The `wslc` limits that apply:

- **No `--platform`.** The adapter drops the flag; every image runs at the
  host architecture anyway ([container runtimes](#container-runtimes)).
- **`inspect` prints JSON only.** It has no Go templates, so
  `Architecture` and `Id` are read from the JSON record
  (`uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image image-field`).
- **No top-level `cp` and no `rm`.** `verify-image` runs `wslc export` and
  extracts only `opt/pinyin-oracle` from the tar. Containers are removed with
  `wslc remove -f`.
- **`build --pull` is an on/off switch, and `build` has no `--network`.**
  Capture builds pass no pull flag. A build's `--network host` is dropped
  with a notice; any other build `--network` value is refused.
- **No `z` mount option.** The adapter drops `,z` and `:z`.
- **No host networking.** `wslc run --network host` fails with "host mode
  networking is not supported"; the adapter drops `--network host` and prints
  a one-line notice that the runtime's default network is used. `--network
  none` is kept.
- **Mounts need Windows paths.** `wslc` reads an MSYS path such as
  `/c/...` as a path inside its VM and mounts an empty directory. The capture
  launcher passes the checkout and data as native Windows paths. `:ro` and a
  read-write mount nested in a read-only one both work.

Host Python cannot read the symlinks a container makes on an NTFS mount
(`[Errno 22] Invalid argument`). On Windows, provisioning therefore never
reads inside a provisioned prefix from the host: each check, removal and
summary runs in the `deps` image.

WSL is a separate Linux development option: follow the Linux section from
its Bash terminal. [Microsoft recommends the Linux filesystem](https://learn.microsoft.com/en-us/windows/wsl/filesystems)
for Linux checkouts/builds (`~/src`, not `/mnt/c`). WSL may also host the
compatibility runtime without replacing the native Windows workflow.

## Container runtimes

The pin runs only in a Linux container. `select_runtime` in
`tools/oracle/image.py` picks the first runtime found, in this order:

| Host | Order |
| --- | --- |
| Windows | `wslc` (WSL containers), Podman, Docker |
| macOS | Apple `container`, Podman, Docker |
| Linux | Podman, Docker |

Every image runs at the host's architecture, with no emulation: x86_64 hosts
use `linux/amd64` and aarch64 hosts `linux/arm64`. `verify-image` defaults to
the host's platform and refuses any other `--platform` on every runtime. On
Windows, `wslc` also ships as `container.exe`, so `container` names `wslc`
there; it names Apple `container` only on macOS.

`OXPINYIN_CONTAINER` names one explicitly; for `build-image` and the captures
(`just capture-pin`) `OXPINYIN_CAPTURE_RUNTIME` comes first. Each
prints the runtime it chose on a `Runtime:` line. Provisioning (`just provision`,
`just provision-oracle`), `verify-image` and `build-image` work through each
of them. Every command is written in docker's grammar and passed through
`runtime_command`. Docker and Podman take it as written. `wslc` drops
`--platform` (see [native Windows](#native-windows-and-wsl-containers)).
Apple `container`, as a macOS run with it verified:

- **Commands.** `image pull` (without `--quiet`), `image tag`, `delete` and
  `system status`; JSON inspect records, whose image `Id` is the config
  descriptor's digest.
- **Mounts.** Read-only binds are `--mount ...,readonly`. It has no
  single-file binds, so the library and data runs run `build-oracle.sh` from
  the mounted recipe at `/opt/oracle-recipe` instead of binding the script,
  the Bash floor and the pin one by one.
- **Memory.** Its VM is small by default: image builds get `--cpus 4
  --memory 6G`, and the library and data runs `--memory 6G`.
- **Network.** There is no `--network host`: it is dropped with a one-line
  notice that the default network is used. Its build has no `--network`, so
  any other build value is refused. `--network none` was confirmed to block
  traffic, so the capture run has no network.
- **Pull.** The capture build gets no valued `--pull`: its `--pull` is an
  on/off switch, as on `wslc`.
- **Capture.** The capture runs the capture image by its tag, and checks
  after the run that the tag still names the digest it read before.
- **`verify-image`.** It can neither `cp` nor `export` a container that never
  started, and the oracle image (`FROM scratch`) cannot start. So it creates
  no container: it runs `container image save` and reads the OCI image layout
  (each blob checked against its digest, the layers applied in order).
- **Provisioning.** The libraries are built in the `deps` image instead of
  copied from the published image.

`just provision-oracle` builds the performance image when absent and runs
`provision oracle` in it with the provisioning cache mounted at
`/provision`. On rootful Docker the run is the invoking user, so the cache
stays theirs; uv's home and cache are then in `/tmp`. `just provision` and
`just provision-oracle` use one cache: `OXPINYIN_PROVISION_CACHE`, else the
older `OXPINYIN_CLOUD_CACHE` if something still sets it, else
`~/.cache/oxpinyin`.

These host steps of `just provision` run only on Linux: Valgrind's check and
apt install, the apt install of the dump utilities, the start of `dockerd`,
the ext4 report, the host libtkrzw check, the `.pc` rewrite and the
environment file. On macOS and Windows these are skipped. Provisioning there
does not require the dump utilities (`db5.3_dump`, `kchashmgr`) either:
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
libraries and retain their existing debug settings.

## Contribution checks

Run the local checks in [verification rule 5](verification.md#5-local-test-gate); they scale with the change and its stage.

CI is the full gate: a passing CI run on your final source tree satisfies the required checks. For a changed file under `crates/`, find the containing package's `Cargo.toml` and use its `[package]` → `name` value as the `-p` argument in the iteration commands in verification rule 5. For a storage-dependent package on a non-default backend, add `--no-default-features --features bdb` to the Clippy and nextest iteration commands in verification rule 5. For documentation-only edits, follow the Markdown-only and source-comment rules in verification rule 5.
For repository build/config changes, identify affected packages or ask the maintainer when scope is unclear; CI stays the full gate.

`--no-default-features` turns off the [default selection](backends.md);
`--features bdb` selects Berkeley DB explicitly (exactly one backend is allowed).
Clippy's `--` separates Cargo options from the lint options that follow.
Use your preferred native editor/debugger with this same toolchain and
terminal environment; no particular IDE or container is required. CI owns
the full workspace test sweep. Use separate target and user-data directories
when changing backends; see [backends](backends.md). Ordinary native tests
are not proof of bug-for-bug compatibility with upstream.

Changes to parsing/candidate behavior, native data/user files or C API
behavior need a relevant differential. Use a [provisioned remote Linux host](cloud.md).
For example, key parsing/surface changes use the C-boundary tests
(see [oracle](oracle.md#the-c-boundary-tests)). Use the associated check, or ask the maintainer if no matching check is
apparent rather than guessing that ordinary unit tests establish parity. The normal cell
(one backend on both sides) is tkrzw, the reference cell. Backend-affecting changes use the
[backend selection rules](verification.md#2-backend-selection); they do not require every ordinary
contributor to provision all backends. Packaging and performance tools belong
to their existing task runbooks.

Report the applicable checks and final outcomes under
[results and reproduction](../../CONTRIBUTING.md#results-and-reproduction).
Stop after the required checks; full logs and evidence uploads are not part
of completing a contribution.

## Troubleshooting and resumption

- Missing `cargo`/`rustup`: on Linux/macOS source `"$HOME/.cargo/env"`; on
  Windows open a fresh Developer PowerShell after installing rustup. A
  permission panic from an installed rustup is an environment access problem:
  retain the error and ask the environment owner; do not reinstall over it.
- Missing MSVC `link.exe` or Windows SDK: use Developer PowerShell and check
  the Visual Studio workload. Native Windows does not use Unix `cc` or
  `pkg-config` for the Rust-only command above.
- Missing tkrzw headers (`pkg-config ... tkrzw` failed), `db.h`, libclang
  or native libraries: repeat your OS prerequisite
  checks. The error identifies the missing header/library; ordinary users
  need the environment owner if package installation is restricted. Send
  that owner the exact failing command and complete error. On Linux,
  `pkg-config --modversion glib-2.0` and `pkg-config --modversion tkrzw`
  check discovery; the setup check prints the backend version and bindgen's
  missing libclang diagnostic, so retain those lines too.
- macOS cannot link tkrzw's lz4/lzma/zstd: repeat the `LIBRARY_PATH` export
  above. For Berkeley DB, repeat the `SYSTEM_DEPS_BDB_*` exports
  ([backends](backends.md#discovery-and-overrides)). Do not treat a
  successful Clippy/check as a link test.
- Missing oracle/model inputs do not block ordinary native tests. They must
  be supplied for the selected compatibility checks, where a skipped/missing
  prerequisite is a failed verification, not a pass.

## Validated on / not validated

- Native Linux x86_64: Rust-only first check (410 passed), BDB link/run
  (172 passed, four ignored), and fresh-terminal resume passed.
- Native macOS arm64: the full portable CI command above passed locally with
  Homebrew Berkeley DB 5.3.28 (1,218 passed, zero failed, 22 ignored; doctests:
  four passed, zero failed, two ignored). This exercised the full six-exclude
  workspace scope; the narrower `just check bdb` was not run separately.
- Native Windows: the maintained CI scope passed at `56618e7f`.
  No local Windows storage installation was executed.
  That run used vcpkg's Berkeley DB 4.8, then the default; since 2026-10-30
  the job runs the [default backend](backends.md) built with MSVC by `.github/actions/tkrzw`.
- Canonical Debian/Linux amd64 compatibility environment: BDB `same-data-dir-diff` (retired since) and cache/
  environment-isolation resume passed on rootless Podman. Other runtime
  interoperability was not executed. Apple's volume-reader result retrieval
  is source-verified only, not runtime-tested.
- Native Linux arm64: Apple container, Debian 13 arm64, 4 CPUs, 16 GiB,
  ext4, no emulation. Two independent builds of each cell produced identical
  arm64 library hashes. a historical `tools/cloud/setup.sh --cells
  "bdb kc tkrzw"` run in a container verified all three cells against the arm64 pin fields,
  regenerated the recorded data manifests, and replaced the host's broken
  libtkrzw with the healthy upstream build. The full gate then reported
  0 DIFFER on every cell: bdb 2,880 MATCH / 15 SKIP / 4 HOST-DEPENDENT, kc
  2,875 / 20 / 4, tkrzw 2,877 / 18 / 4. Each cell's SKIPs include the 13
  arm64-only #1137 exclusions; same-data was identical on 15/15 drivers.
  See [the arm64 oracle finding](../findings/oracle-arm64-1132.md).
- Historical Debian/Linux amd64 compatibility run (glibc 2.41, GLib 2.84).
  The current oracle uses unpinned `debian:testing`; the compat launcher is retired.
  At `8ba13c53` (preserved on wip/1229-pre-squash), before `0x.`
  was registered with `host_scanf`: the contract differential on 3,017 cases per cell
  reported bdb 2,988 MATCH / 15 POLICY-ACCEPTED / 10 HOST-DEPENDENT / 2 SKIP
  / 2 DIFFER, kc 2,983 / 15 / 10 / 7 / 2, tkrzw 2,985 / 15 / 10 / 5 / 2. The
  2 DIFFER are the `0x.` glibc-version host-dependence finding; the 10
  HOST-DEPENDENT are the exponent spellings. A targeted bdb rerun of the
  eight affected cases at the same revision returned the same 2 DIFFER and
  6 HOST-DEPENDENT. The
  perf-matrix and callgrind images built on debian:testing and their smoke
  steps passed.
- Anonymous oracle artifact retrieval/hash verification passed. Anonymous
  `-deps` retrieval awaits normal publication. See the PR for final-head CI
  and the current cloud-provisioning/compatibility smoke status.
