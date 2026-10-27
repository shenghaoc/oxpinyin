# Native command interface. Thin recipes only: selection and orchestration
# stay in tools.dev. Native recipes never start a container.
set positional-arguments
set minimum-version := "1.59.0"

# Windows uses PowerShell, not a POSIX shell. This path is written but
# unverified: there is no Windows host for this stack.
[windows]
set shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

default: doctor

# Data-only transport target for the real-just smoke tests; never starts work.
[unix]
[private]
_test-argv status *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -c 'import json, sys; print(json.dumps(sys.argv[2:])); sys.exit(int(sys.argv[1]))' "$@"

# Record or check the pin-reference image's tool versions. Requires the
# Debian image environment (dpkg-query); the image builds run the same check.
[unix]
oracle-versions *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev oracle-versions "$@"

[windows]
oracle-versions *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev oracle-versions @args; exit $LASTEXITCODE

# Provision the interpreter .python-version names, the one place the version is
# stated. uv has no download keyed `+gil` (#1230), so tools/install-python.sh
# installs the file's request without `+gil`; selection keeps the file's request.
[unix]
python:
    @tools/install-python.sh

# PowerShell cannot run the shell script; this derives the same install request.
[windows]
python:
    @uv python find --managed-python *> $null; if ($LASTEXITCODE -ne 0) { uv python install ((Get-Content .python-version -Raw).Trim() -replace '\+gil$', ''); if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE } }
    @uv python find --managed-python *> $null; if ($LASTEXITCODE -ne 0) { [Console]::Error.WriteLine('the interpreter in .python-version does not resolve after installing it'); exit 1 }

# Post-bootstrap cloud provisioning; the shell entry also installs uv if needed.
[unix]
provision *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev provision "$@"

# Report native capabilities and the managed interpreter identity. Doctor
# installs nothing: it names `just python` when the interpreter is missing.
[unix]
doctor:
    @command -v uv >/dev/null 2>&1 || { echo 'uv is not installed: see docs/runbooks/setup.md' >&2; exit 1; }
    @uv python find --managed-python >/dev/null 2>&1 || { echo 'the managed interpreter in .python-version is not installed: run `just python`' >&2; exit 1; }
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev doctor

[windows]
doctor:
    @if (-not (Get-Command uv -ErrorAction SilentlyContinue)) { [Console]::Error.WriteLine('uv is not installed: see docs/runbooks/setup.md'); exit 1 }
    @uv python find --managed-python *> $null; if ($LASTEXITCODE -ne 0) { [Console]::Error.WriteLine('the managed interpreter in .python-version is not installed: run `just python`'); exit 1 }
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev doctor; exit $LASTEXITCODE

# Install, validate and assemble drop-in packages through the shared service.
[unix]
packaging *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev packaging "$@"

# Run one native setup check: bdb, tkrzw or kyotocabinet.
check backend="bdb": python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev check {{backend}}

# Run a command inside the native heavy slot and memory admission.
[unix]
heavy *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev slot run "$@"

[windows]
heavy *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev slot run @args; exit $LASTEXITCODE

# Run the native Rust tests through nextest and the heavy slot.
[unix]
test *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test "$@"

[windows]
test *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test @args; exit $LASTEXITCODE

# Run the CI Python harness discovery and exclusions (Linux and macOS).
[unix]
test-tools *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test-tools "$@"

# Run doctests separately; nextest does not run them.
[unix]
test-doc *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test-doc "$@"

[windows]
test-doc *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test-doc @args; exit $LASTEXITCODE

# Build with the native heavy slot and memory admission.
[unix]
build *args:
    @just heavy -- cargo build "$@"

[windows]
build *args:
    @just heavy -- cargo build @args; exit $LASTEXITCODE

# Format directly; formatting does not take the heavy slot.
[unix]
fmt *args:
    @cargo fmt "$@"

[windows]
fmt *args:
    @cargo fmt @args; exit $LASTEXITCODE

# Regenerate the GLib/libc call-site TSV from a pinned libpinyin checkout.
audit-glib-call-sites pin: python
    @uv run --no-project --managed-python python -X lazy_imports=normal tools/audit/glib_call_sites.py "$1"

# Lint with the native heavy slot and memory admission.
[unix]
clippy *args:
    @just heavy -- cargo clippy "$@"

[windows]
clippy *args:
    @just heavy -- cargo clippy @args; exit $LASTEXITCODE

# Run the workspace benches with the native heavy slot and memory admission.
[unix]
bench *args:
    @just heavy -- cargo bench --workspace --benches "$@"

[windows]
bench *args:
    @just heavy -- cargo bench --workspace --benches @args; exit $LASTEXITCODE

# Build every selected bench harness without running it.
[unix]
bench-smoke:
    @just bench --no-run

[windows]
bench-smoke:
    @just bench --no-run; exit $LASTEXITCODE

# Profile the W8 keystroke cycle with samply. macOS uses Homebrew's native
# build inputs; tools.dev.measure.profile refuses Linux-only pinning and
# fallbacks there.
[linux]
profile-w8 *args: python
    @just heavy -- uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure profile-w8 "$@"

[macos]
profile-w8 *args: python
    @just heavy -- env \
        OXPINYIN_BDB_INCLUDE_DIR="$(brew --prefix berkeley-db@5)/include" \
        OXPINYIN_BDB_LIB_DIR="$(brew --prefix berkeley-db@5)/lib" \
        PKG_CONFIG_PATH="$(brew --prefix glib)/lib/pkgconfig:$(brew --prefix kyoto-cabinet)/lib/pkgconfig:$(brew --prefix tkrzw)/lib/pkgconfig:$(brew --prefix berkeley-db@5)/lib/pkgconfig" \
        LIBRARY_PATH="$(brew --prefix berkeley-db@5)/lib:$(brew --prefix kyoto-cabinet)/lib:$(brew --prefix tkrzw)/lib:$(brew --prefix lz4)/lib:$(brew --prefix xz)/lib:$(brew --prefix zstd)/lib" \
        OXPINYIN_TKRZW_PREFIX="$(brew --prefix tkrzw)" \
        uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure profile-w8 "$@"

[windows]
profile-w8 *args:
    @echo "profile-w8: native macOS and Linux only; Windows is unsupported." >&2; exit 1

# Run one caller-selected fuzz target with the nightly workflow's ASLR wrapper.
# Exact on purpose (#1074): a nightly is identified only by its date.
[linux]
fuzz *args:
    @just heavy -- setarch "$(uname -m)" -R cargo +nightly-2026-08-01 fuzz run "$@"

# cargo-fuzz supports Darwin. Homebrew supplies the facade's GLib and the
# default store backend; the paths are selected explicitly, as in CI.
[macos]
fuzz *args:
    @just heavy -- env \
        OXPINYIN_BDB_INCLUDE_DIR="$(brew --prefix berkeley-db@5)/include" \
        OXPINYIN_BDB_LIB_DIR="$(brew --prefix berkeley-db@5)/lib" \
        PKG_CONFIG_PATH="$(brew --prefix glib)/lib/pkgconfig:$(brew --prefix berkeley-db@5)/lib/pkgconfig" \
        LIBRARY_PATH="$(brew --prefix berkeley-db@5)/lib" \
        cargo +nightly-2026-08-01 fuzz run "$@"

# Windows transport is unverified (#950); setarch is Linux-only.
[windows]
fuzz *args:
    @just heavy -- cargo +nightly-2026-08-01 fuzz run @args; exit $LASTEXITCODE

# Run native coverage with temporary state; run and report in one invocation.
[unix]
coverage *args: python
    @just heavy -- uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev coverage "$@"

# Windows transport is unverified (#950).
[windows]
coverage *args: python
    @just heavy -- uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev coverage @args; exit $LASTEXITCODE

# Run one store backend's sanitizer gate (bdb, kc or tkrzw), or all three,
# with address (default), thread, leak or memory instrumentation.
[linux]
sanitize backend="bdb" sanitizer="address": python
    @just heavy -- uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev sanitize "$1" "$2"

# Darwin uses Homebrew's Berkeley DB 5.3, Kyoto Cabinet and tkrzw. The recipe
# selects Darwin paths and omits Linux setarch. The tkrzw gate certifies the
# host library through distro-probe.sh; put Homebrew's tkrzw_dbm_util on PATH.
[macos]
sanitize backend="bdb" sanitizer="address": python
    @just heavy -- env \
        OXPINYIN_BDB_INCLUDE_DIR="$(brew --prefix berkeley-db@5)/include" \
        OXPINYIN_BDB_LIB_DIR="$(brew --prefix berkeley-db@5)/lib" \
        PKG_CONFIG_PATH="$(brew --prefix glib)/lib/pkgconfig:$(brew --prefix kyoto-cabinet)/lib/pkgconfig:$(brew --prefix tkrzw)/lib/pkgconfig:$(brew --prefix berkeley-db@5)/lib/pkgconfig" \
        LIBRARY_PATH="$(brew --prefix berkeley-db@5)/lib:$(brew --prefix kyoto-cabinet)/lib:$(brew --prefix tkrzw)/lib:$(brew --prefix lz4)/lib:$(brew --prefix xz)/lib:$(brew --prefix zstd)/lib" \
        CPATH="$(brew --prefix berkeley-db@5)/include:$(brew --prefix kyoto-cabinet)/include" \
        PATH="$(brew --prefix tkrzw)/bin:$PATH" \
        uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev sanitize "$1" "$2"

# The gates are Linux/Darwin native builds; Windows transport is unverified (#950).
[windows]
sanitize backend="bdb" sanitizer="address":
    @echo "sanitize is unsupported on Windows: the sanitizer gates are Linux/Darwin native builds."
    @exit 1

# Run shared ABI export and allocation gates.
[unix]
runner *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev runner "$@"

[windows]
runner *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev runner @args; exit $LASTEXITCODE

[unix]
abi *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev abi "$@"

[windows]
abi *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev abi @args; exit $LASTEXITCODE

# Build or describe the registry's selected cell artifacts.
[unix]
cell-artifacts *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev cell-artifacts "$@"

[windows]
cell-artifacts *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev cell-artifacts @args; exit $LASTEXITCODE

# Read registry metadata, optionally filtered by issue or kind.
[unix]
list *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev list "$@"

[windows]
list *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev list @args; exit $LASTEXITCODE

# Describe an explicit registry selection without executing it.
[unix]
plan *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev plan "$@"

[windows]
plan *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev plan @args; exit $LASTEXITCODE

# Run a measure recipe: startup (the controller holds the heavy slot), cold-qualify, or a measurement
# or profile recipe such as perf-baseline or snapshot (the list is tools/dev/measure/recipes.py).
[unix]
measure recipe *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure "$@"

# Startup measurement alias (Windows deferred, #950).
[unix]
initbench *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure startup "$@"

# Qualify APFS cold preparation on named files; read-only, runs no worker.
[unix]
cold-qualify *paths: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure cold-qualify "$@"

[windows]
measure recipe *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure @args; exit $LASTEXITCODE

[windows]
initbench *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure startup @args; exit $LASTEXITCODE

# Oracle and compatibility recipes share host/runtime/fixture services.
[unix]
compat *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev compat "$@"
