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
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev doctor

# Run one native setup check: bdb, tkrzw or kyotocabinet.
check backend="bdb":
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev check {{backend}}

# Run a command inside the native heavy slot and memory admission.
[unix]
heavy *args:
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev slot run "$@"

[windows]
heavy *args:
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev slot run @args; exit $LASTEXITCODE

# Run the native Rust tests through nextest and the heavy slot.
[unix]
test *args:
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev test "$@"

[windows]
test *args:
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev test @args; exit $LASTEXITCODE

# Run doctests separately; nextest does not run them.
[unix]
test-doc *args:
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev test-doc "$@"

[windows]
test-doc *args:
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev test-doc @args; exit $LASTEXITCODE

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

# Read registry metadata, optionally filtered by issue or kind.
[unix]
list *args:
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev list "$@"

[windows]
list *args:
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev list @args; exit $LASTEXITCODE

# Describe an explicit registry selection without executing it.
[unix]
plan *args:
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev plan "$@"

[windows]
plan *args:
    @uv run --no-project --managed-python --python cpython@3.15.0+gil python -X lazy_imports=normal -m tools.dev plan @args; exit $LASTEXITCODE
