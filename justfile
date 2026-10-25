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
_test-argv status *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -c 'import json, sys; print(json.dumps(sys.argv[2:])); sys.exit(int(sys.argv[1]))' "$@"

# Record or check the pin-reference image's tool versions. Requires the
# Debian image environment (dpkg-query); the image builds run the same check.
[unix]
oracle-versions *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev oracle-versions "$@"

[windows]
oracle-versions *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev oracle-versions @args; exit $LASTEXITCODE

# Report native capabilities and the managed interpreter identity.
doctor:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev doctor

# Run one native setup check: bdb, tkrzw or kyotocabinet.
check backend="bdb":
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev check {{backend}}

# Run a command inside the native heavy slot and memory admission.
[unix]
heavy *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev slot run "$@"

[windows]
heavy *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev slot run @args; exit $LASTEXITCODE

# Run the native Rust tests through nextest and the heavy slot.
[unix]
test *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev test "$@"

[windows]
test *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev test @args; exit $LASTEXITCODE

# Run the CI Python harness discovery and exclusions (Linux and macOS).
[unix]
test-tools:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev test-tools

# Run doctests separately; nextest does not run them.
[unix]
test-doc *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev test-doc "$@"

[windows]
test-doc *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev test-doc @args; exit $LASTEXITCODE

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
audit-glib-call-sites pin:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal tools/audit/glib_call_sites.py "$1"

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
# build inputs; the script refuses Linux-only pinning and fallbacks there.
[linux]
profile-w8 *args:
    @just heavy -- bash tools/profile/run-w8-cycle.sh "$@"

[macos]
profile-w8 *args:
    @just heavy -- env \
        OXPINYIN_BDB_INCLUDE_DIR="$(brew --prefix berkeley-db@5)/include" \
        OXPINYIN_BDB_LIB_DIR="$(brew --prefix berkeley-db@5)/lib" \
        PKG_CONFIG_PATH="$(brew --prefix glib)/lib/pkgconfig:$(brew --prefix kyoto-cabinet)/lib/pkgconfig:$(brew --prefix tkrzw)/lib/pkgconfig:$(brew --prefix berkeley-db@5)/lib/pkgconfig" \
        LIBRARY_PATH="$(brew --prefix berkeley-db@5)/lib:$(brew --prefix kyoto-cabinet)/lib:$(brew --prefix tkrzw)/lib:$(brew --prefix lz4)/lib:$(brew --prefix xz)/lib:$(brew --prefix zstd)/lib" \
        OXPINYIN_TKRZW_PREFIX="$(brew --prefix tkrzw)" \
        bash tools/profile/run-w8-cycle.sh "$@"

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
coverage *args:
    @just heavy -- uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev coverage "$@"

# Windows transport is unverified (#950).
[windows]
coverage *args:
    @just heavy -- uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev coverage @args; exit $LASTEXITCODE

# Run one store backend's sanitizer gate (bdb, kc or tkrzw), or all three,
# with address (default), thread, leak or memory instrumentation.
[linux]
sanitize backend="bdb" sanitizer="address":
    @just heavy -- bash tools/sanitize.sh "$1" "$2"

# Darwin uses Homebrew's Berkeley DB 5.3, Kyoto Cabinet and tkrzw. The wrapper
# selects Darwin paths and omits Linux setarch. The tkrzw gate certifies the
# host library through distro-probe.sh; put Homebrew's tkrzw_dbm_util on PATH.
[macos]
sanitize backend="bdb" sanitizer="address":
    @just heavy -- env \
        OXPINYIN_BDB_INCLUDE_DIR="$(brew --prefix berkeley-db@5)/include" \
        OXPINYIN_BDB_LIB_DIR="$(brew --prefix berkeley-db@5)/lib" \
        PKG_CONFIG_PATH="$(brew --prefix glib)/lib/pkgconfig:$(brew --prefix kyoto-cabinet)/lib/pkgconfig:$(brew --prefix tkrzw)/lib/pkgconfig:$(brew --prefix berkeley-db@5)/lib/pkgconfig" \
        LIBRARY_PATH="$(brew --prefix berkeley-db@5)/lib:$(brew --prefix kyoto-cabinet)/lib:$(brew --prefix tkrzw)/lib:$(brew --prefix lz4)/lib:$(brew --prefix xz)/lib:$(brew --prefix zstd)/lib" \
        CPATH="$(brew --prefix berkeley-db@5)/include:$(brew --prefix kyoto-cabinet)/include" \
        PATH="$(brew --prefix tkrzw)/bin:$PATH" \
        bash tools/sanitize.sh "$1" "$2"

# The gates are Linux/Darwin bash scripts; Windows transport is unverified (#950).
[windows]
sanitize backend="bdb" sanitizer="address":
    @echo "sanitize is unsupported on Windows: the sanitizer gates are Linux/Darwin bash scripts."
    @exit 1

# Read registry metadata, optionally filtered by issue or kind.
[unix]
list *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev list "$@"

[windows]
list *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev list @args; exit $LASTEXITCODE

# Describe an explicit registry selection without executing it.
[unix]
plan *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev plan "$@"

[windows]
plan *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev plan @args; exit $LASTEXITCODE

# Measure native oxpinyin startup; the controller holds the heavy slot.
[unix]
measure recipe *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev measure "$@"

# Startup measurement alias (Windows deferred, #950).
[unix]
initbench *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev measure startup "$@"

# Qualify APFS cold preparation on named files; read-only, runs no worker.
[unix]
cold-qualify *paths:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev measure cold-qualify "$@"

[windows]
measure recipe *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev measure @args; exit $LASTEXITCODE

[windows]
initbench *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev measure startup @args; exit $LASTEXITCODE
