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
