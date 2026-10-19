# Native contributor setup

Start from [CONTRIBUTING](../../CONTRIBUTING.md). These instructions build
and test local files; they do not install an input method or replace your
system libpinyin. Stop if a command fails and keep that command and its full
error before continuing. In Bash, `echo $?` immediately after a command
shows its exit status (zero means success); PowerShell uses `$LASTEXITCODE`.
Choose your operating system below. Ordinary native work
requires no container, oracle or model20 (upstream reference model data).

Rust's **toolchain** consists of the compiler and its tools. rustup installs
and selects it; `rust-toolchain.toml` is the version/component authority.
The **linker** connects compiled code into executables and native libraries.

## Linux

Use Bash. Linux is one native platform target. You need Git, curl and CA
certificates, a C/C++ compiler and linker, Make, pkg-config, libclang,
Berkeley DB **5.3** development headers/libraries and GLib development files.
A development package supplies the headers and libraries needed to
compile against a native library. Package names depend on your system; that
does not change the build or create another compatibility environment.
Repository Python does not use the system interpreter; provision it with uv
as described below.

If these prerequisites are missing, install them with your system's package
manager. `sudo` asks for administrator permission to install system packages.
These are common package-manager examples, not separate supported/tested
distribution targets; check that your packages provide Berkeley DB 5.3:

```bash
# Systems using apt:
sudo apt-get update
sudo apt-get install -y build-essential git curl ca-certificates pkg-config libclang-dev libdb-dev libglib2.0-dev
```

```bash
# Systems using dnf:
sudo dnf install gcc gcc-c++ make git curl ca-certificates pkgconf-pkg-config clang-devel libdb-devel glib2-devel
```

Repository Python uses uv 0.13.1 and its managed CPython 3.15.0 with the GIL.
Install uv from its documented host route, then provision the interpreter:

```bash
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
uv python install cpython@3.15.0
uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal - <<'PYTHON'
from pathlib import Path
import subprocess
import sys
import sysconfig
assert sys.implementation.name == "cpython"
assert sys.version_info[:3] == (3, 15, 0)
assert sysconfig.get_config_var("Py_GIL_DISABLED") == 0
assert sys._is_gil_enabled()
managed = subprocess.check_output(
    ["uv", "python", "find", "--managed-python", "cpython@3.15.0+gil"],
    text=True).strip()
assert Path(sys.executable).resolve() == Path(managed).resolve()
PYTHON
```

The installer digest comes from its `digest` field in the official
[uv 0.13.1 release API](https://api.github.com/repos/astral-sh/uv/releases/tags/0.13.1).
Automated Linux x86_64 setup verifies the release archive against the pinned
[published checksum](https://github.com/astral-sh/uv/releases/download/0.13.1/sha256.sum).

The `+gil` request requires the GIL-enabled build. uv 0.13.1 has no separate
`+gil` archive, so install the plain 3.15.0 build and verify that the request
resolves to it. Do not use a system Python, a virtual environment,
`pyproject.toml` or `uv.lock`; the checkout's `.python-version` and
`uv.toml` pin the same interpreter and uv version.

To regenerate the W3 content fixtures, run this command from the checkout root:

```bash
uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal \
  tools/generate_w3_fixtures.py
```

The native command interface uses just 1.59.0 or newer. Install it through the
host route: `brew install just` on macOS, the distribution package on Linux,
or `cargo install just --version 1.59.0 --locked`. The root `justfile`
enforces the minimum version.

Use your system's official package documentation if names or enabled
repositories differ. The native check below prints the actual Berkeley DB
header version and rejects unsupported versions during the build; do not
copy CI-only version overrides to work around that check.

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
just check bdb
```

The `cargo test` command tests native Rust packages; `just check bdb` additionally links
and executes storage and C-library tests against Berkeley DB 5.3. Headers
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
just check bdb
```

Cargo reuses downloaded dependencies and that checkout's `target/` outputs.
Keep outputs separate across worktrees (separate checkouts of branches).

## macOS

Use Terminal with Bash (`bash`), Git, Apple's
[Command Line Tools](https://developer.apple.com/xcode/resources/),
[Homebrew](https://docs.brew.sh/Installation) and rustup. If Command Line
Tools are absent, run `xcode-select --install` and complete the system dialog.

```bash
brew install tkrzw glib pkgconf
mkdir -p ~/src
cd ~/src
git clone https://github.com/shenghaoc/oxpinyin.git
cd oxpinyin
```

Run the uv provisioning commands from the Linux section. For the checksum
step, replace `sha256sum -c -` with `shasum -a256 -c -` on macOS.
Then install rustup if absent using the Linux rustup commands above
and run:

```bash
. "$HOME/.cargo/env"
rustup show
cargo test --locked -p oxpinyin-chewing -p oxpinyin-core -p oxpinyin-engine
export LIBRARY_PATH="$(brew --prefix)/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
export CARGO_TARGET_DIR=target/tkrzw
just check tkrzw
```

`export` sets a setting inherited by commands in this terminal. The library
path lets the native linker find tkrzw's lz4/lzma/zstd dependencies; a mere
`cargo check` can miss this failure. This is a real storage link/run check.
In a fresh Bash terminal, return to the checkout, source Cargo's environment,
repeat both exports and the check. Linux C-ABI packaging and Linux reference
differentials are separate evidence, not permanent restrictions on portability.

The existing macOS CI uses this broader portable scope:

```bash
cargo test --locked --workspace --no-default-features --features tkrzw \
  --exclude oxpinyin-capi --exclude oxpinyin-zhuyin-capi \
  --exclude oxpinyin-corpus --exclude oxpinyin-kmm \
  --exclude oxpinyin-punct --exclude oxpinyin-word
```

The [validation status](#validated-on--not-validated) distinguishes native macOS CI
from hands-on setup evidence. The current CI exclusions describe maintained
build integration, not permanent limitations of the portable Rust code.

## Native Windows

This is native Windows Rust development, not Linux under WSL. Install Git
and [rustup's MSVC prerequisites](https://rust-lang.github.io/rustup/installation/windows-msvc.html):
Visual Studio Build Tools with **Desktop development with C++** and the
Windows SDK. [Microsoft's Rust setup](https://learn.microsoft.com/en-us/windows/dev-environment/rust/setup)
explains installation. Use a new **Developer PowerShell for VS** so the MSVC
linker and SDK are on PATH. Install rustup with the official Windows installer
if it is not already available.

In Developer PowerShell:

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

All three packages link and run natively without a DBM library or container.
Success is zero failed tests and exit code zero (`$LASTEXITCODE`). In a fresh
Developer PowerShell, `Set-Location "$HOME/src/oxpinyin"` and repeat the test;
use your real directory for an existing checkout. Rustup chooses the same
repository toolchain automatically. Editors/debuggers use this same native
MSVC environment and toolchain; no project-specific IDE is required.

**Storage-dependent native Windows scope:** existing Windows CI runs the
portable workspace with vcpkg `berkeleydb:x64-windows` (4.8), explicit include,
library, DLL and LLVM paths, and an **unsurveyed-version exception reserved
for CI**. Do not copy that exception into developer or production setup.
The repository has no validated contributor recipe for surveyed Berkeley DB
5.3 on MSVC yet. That is a native dependency/version-survey gap, not evidence
that Windows is inherently unsupported. Native Rust-only work above is
available now; storage-dependent work uses the Windows CI evidence and needs
a validated 5.3 installation before claiming a supported native local run.

The existing CI exclusion set is the same six packages shown in the macOS
command, with default BDB instead of `--features tkrzw`. The oracle's Linux
FFI feature is not selected. Native Windows C-ABI/oracle behavior is not
established by those tests. Physical Windows hardware is not needed to use
this CI coverage.

Recent upstream MinGW portability changes were checked separately from
oxpinyin's MSVC support; see the [validation status](#validated-on--not-validated).
The frozen oracle does not define which native operating systems oxpinyin
can support, and its pins are unchanged.

WSL is a separate Linux development option: follow the Linux section from
its Bash terminal. [Microsoft recommends the Linux filesystem](https://learn.microsoft.com/en-us/windows/wsl/filesystems)
for Linux checkouts/builds (`~/src`, not `/mnt/c`). WSL may also host the
compatibility runtime without replacing the native Windows workflow.

## Contribution checks

From the native repository root, run fmt, tests and Clippy for each touched
package. For a changed file under `crates/`, find the containing package's
`Cargo.toml` and copy its `[package]` → `name` value into the `-p` argument.
Repeat the package commands for each affected package; the example below
uses `oxpinyin-core`. For documentation-only edits, run fmt and check the
changed instructions/links; Rust tests need not be repeated. For repository
build/config changes, identify affected packages or ask the maintainer when
scope is unclear; CI owns the full workspace sweep:

```bash
cargo fmt --all --check
cargo clippy --locked -p oxpinyin-core --all-targets -- -D warnings
cargo test --locked -p oxpinyin-core
```

For a storage-dependent macOS package, use the explicit backend arguments:

```bash
cargo clippy --locked -p oxpinyin-store --all-targets --no-default-features --features tkrzw -- -D warnings
cargo test --locked -p oxpinyin-store --no-default-features --features tkrzw
```

`--no-default-features` turns off the default Berkeley DB selection;
`--features tkrzw` selects its peer instead (exactly one is allowed).
Clippy's `--` separates Cargo options from the lint options that follow.
Use your preferred native editor/debugger with this same toolchain and
terminal environment; no particular IDE or container is required. CI owns
full workspace sweeps. Use separate target and user-data directories
when changing backends; see [backends](backends.md). Ordinary native tests
are not proof of bug-for-bug compatibility with upstream.

Changes to parsing/candidate behavior, native data/user files or C API
behavior need a relevant differential. Use a [provisioned cloud session](cloud.md)
or the second supported [Debian-container route](compatibility-container.md).
For example, key parsing/surface changes use the existing
`same-data-dir-diff` gate shown there. The registry records issue associations;
use an associated issue/check, or ask the maintainer if no matching check is
apparent rather than guessing that ordinary unit tests establish parity. The normal cell
(one backend on both sides) is BDB. Backend-affecting changes use the existing
path rules in [oracle](oracle.md); they do not require every ordinary
contributor to provision all backends. Packaging and performance tools belong
to their existing task runbooks.

## Troubleshooting and resumption

- Missing `cargo`/`rustup`: on Linux/macOS source `"$HOME/.cargo/env"`; on
  Windows open a fresh Developer PowerShell after installing rustup. A
  permission panic from an installed rustup is an environment access problem:
  retain the error and ask the environment owner; do not reinstall over it.
- Missing MSVC `link.exe` or Windows SDK: use Developer PowerShell and check
  the Visual Studio workload. Native Windows does not use Unix `cc` or
  `pkg-config` for the Rust-only command above.
- Missing `db.h`, libclang or native libraries: repeat your OS prerequisite
  checks. The error identifies the missing header/library; ordinary users
  need the environment owner if package installation is restricted. Send
  that owner the exact failing command and complete error. On Linux,
  `pkg-config --modversion glib-2.0` checks GLib discovery; the setup check
  prints the compiler's Berkeley DB header version and bindgen's missing
  libclang diagnostic, so retain those lines too.
- macOS linker cannot find lz4/lzma/zstd: repeat `LIBRARY_PATH` before running
  tests. Do not treat a successful Clippy/check as a link test.
- Missing oracle/model inputs do not block ordinary native tests. They must
  be supplied for the selected compatibility checks, where a skipped/missing
  prerequisite is a failed verification, not a pass.

## Validated on / not validated

- Native Linux x86_64: Rust-only first check (410 passed), BDB link/run
  (172 passed, four ignored), and fresh-terminal resume passed.
- Native macOS/Windows: maintained CI scopes passed at `56618e7f`.
  No hands-on Mac setup or local Windows storage installation was executed.
  Supported local BDB5.3/MSVC provisioning remains a gap; CI's BDB4.8
  exception is not a contributor default.
- Canonical pinned Debian/Linux amd64: BDB `same-data-dir-diff` and cache/
  environment-isolation resume passed on rootless Podman. Other runtime
  interoperability and arm64-host emulation were not executed. Apple's
  volume-reader result retrieval is source-verified only, not runtime-tested.
- Anonymous oracle artifact retrieval/hash verification passed. Anonymous
  `-deps` retrieval awaits normal publication. See the PR for final-head CI
  and the current cloud-provisioning/compatibility smoke status.
