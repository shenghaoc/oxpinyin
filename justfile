# Native command interface. Thin recipes only: selection and orchestration
# stay in tools.dev. Native recipes never start a container.
set positional-arguments
set minimum-version := "1.59.0"

# Windows uses PowerShell, not a POSIX shell. This path is written but
# unverified: there is no Windows host for this stack.
[windows]
set shell := ["powershell.exe", "-NoLogo", "-NoProfile", "-Command"]

default: doctor

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

# Run one caller-selected fuzz target with the nightly workflow's ASLR wrapper.
# Exact on purpose (#1074): a nightly is identified only by its date.
[linux]
fuzz *args:
    @just heavy -- setarch "$(uname -m)" -R cargo +nightly-2026-08-01 fuzz run "$@"

# The fuzz workspace links the Linux-only C ABI, even for portable targets.
[macos]
fuzz *args:
    @echo "fuzz is unsupported on macOS: the fuzz workspace requires the Linux-only C ABI."
    @exit 1

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

# Run one store backend's sanitizer gate (bdb, kc or tkrzw), or all three.
[linux]
sanitize backend="bdb":
    @just heavy -- bash tools/sanitize.sh "$1"

# The gates need Linux: libdb 5.3, a loadable Kyoto Cabinet and setarch.
[macos]
sanitize backend="bdb":
    @echo "sanitize is unsupported on macOS: the sanitizer gates need Linux (libdb 5.3, Kyoto Cabinet, setarch)."
    @exit 1

# The gates are Linux bash scripts; Windows transport is unverified (#950).
[windows]
sanitize backend="bdb":
    @echo "sanitize is unsupported on Windows: the sanitizer gates are Linux bash scripts."
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

[windows]
measure recipe *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev measure @args; exit $LASTEXITCODE

[windows]
initbench *args:
    @uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -m tools.dev measure startup @args; exit $LASTEXITCODE
