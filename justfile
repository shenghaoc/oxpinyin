# Native command interface. Thin recipes only: selection and orchestration
# stay in tools.dev. Native recipes never start a container.
set positional-arguments
set minimum-version := "1.59.0"

# Windows uses pwsh; doctor checks the floor in tools/dev/pwsh-floor.txt.
[windows]
set shell := ["pwsh", "-NoLogo", "-NoProfile", "-CommandWithArgs"]

# Discard just's recipe name once, before native argument forwarding.
windows_args := '$null, [object[]]$forwarded = $args;'

default: doctor

# Compare scripts2-derived Rust tables offline; pass --write to regenerate.
[unix]
rust-tables *args:
    @uv run --offline --no-python-downloads --no-project --managed-python python -X lazy_imports=normal -m tools.dev rust-tables "$@"

[windows]
rust-tables *args:
    @{{windows_args}} uv run --offline --no-python-downloads --no-project --managed-python python -X lazy_imports=normal -m tools.dev rust-tables @forwarded; exit $LASTEXITCODE

# Data-only transport target for the real-just smoke tests; never starts work.
[unix]
[private]
_test-argv status *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -c 'import json, sys; print(json.dumps(sys.argv[2:])); sys.exit(int(sys.argv[1]))' "$@"

[windows]
[private]
_test-argv status *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -c 'import json, sys; print(json.dumps(sys.argv[2:])); sys.exit(int(sys.argv[1]))' @forwarded; exit $LASTEXITCODE

# Record or check the pin-reference image's tool versions. Requires the
# Debian image environment (dpkg-query); the image builds run the same check.
[unix]
oracle-versions *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev oracle-versions "$@"

[windows]
oracle-versions *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev oracle-versions @forwarded; exit $LASTEXITCODE

# Provision the interpreter .python-version names, the one place the version is
# stated. uv reads the file itself and does nothing when the interpreter is there.
[unix]
python:
    @uv python install --quiet

[windows]
python:
    @uv python install --quiet; exit $LASTEXITCODE

# Post-bootstrap cloud provisioning; the shell entry also installs uv if needed.
[unix]
provision *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev provision "$@"

# Fetch, verify and extract the pinned model20 archive into target/model20.
[unix]
fetch-model *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev fetch-model "$@"

# Fetch, verify and extract the pinned libpinyin-data package (the contract system data) into target/contract-data.
[unix]
fetch-contract-data *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev fetch-contract-data "$@"

[windows]
fetch-contract-data *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev fetch-contract-data @forwarded; exit $LASTEXITCODE

# Regenerate (`generate`) or check (`check`) the committed fixtures/w3 data
# sets from the model20 cache; `content` truncates the oracle's content tables.
[unix]
w3-fixtures *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev w3-fixtures "$@"

# Report native capabilities and the managed interpreter identity. Doctor
# installs nothing: it names `just python` when the interpreter is missing.
[unix]
doctor:
    @command -v uv >/dev/null 2>&1 || { echo 'uv is not installed: see docs/runbooks/setup.md' >&2; exit 1; }
    @uv python find --managed-python >/dev/null 2>&1 || { echo 'the managed interpreter in .python-version is not installed: run `just python`' >&2; exit 1; }
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev doctor

[windows]
doctor:
    @{{windows_args}} if (-not (Get-Command uv -ErrorAction SilentlyContinue)) { [Console]::Error.WriteLine('uv is not installed: see docs/runbooks/setup.md'); exit 1 }
    @{{windows_args}} uv python find --managed-python *> $null; if ($LASTEXITCODE -ne 0) { [Console]::Error.WriteLine('the managed interpreter in .python-version is not installed: run `just python`'); exit 1 }
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev doctor; exit $LASTEXITCODE

# Install, validate and assemble drop-in packages through the shared service.
[unix]
packaging *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev packaging "$@"

# Stage built utilities in the tree expected by epico/trainer.
[unix]
trainer-layout target *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev packaging trainer-layout "$@"

check backend="": python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev check {{backend}}

# A distro tkrzw is found through pkg-config with nothing more. To use this
# build instead, put DIR/lib/pkgconfig on PKG_CONFIG_PATH and DIR/lib on the
# runtime library path (DIR/bin on Windows), as CI's tkrzw action does.
# Build upstream tkrzw at the provisioning revision into an empty DIR.
[unix]
tkrzw-build prefix: python
    @just heavy -- uv run --no-project --managed-python python -X lazy_imports=normal -m tools.tkrzw.build build --prefix "$1"

[windows]
tkrzw-build prefix: python
    @{{windows_args}} just heavy -- uv run --no-project --managed-python python -X lazy_imports=normal -m tools.tkrzw.build build --prefix @forwarded; exit $LASTEXITCODE

# Run a command inside the native heavy slot and memory admission.
[unix]
heavy *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev slot run "$@"

[windows]
heavy *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev slot run @forwarded; exit $LASTEXITCODE

# Run the native Rust tests through nextest and the heavy slot.
[unix]
test *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test "$@"

[windows]
test *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test @forwarded; exit $LASTEXITCODE

# Run the CI Python harness discovery and exclusions (Linux and macOS).
[unix]
test-tools *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test-tools "$@"

# Record policy-accepted observations from contract-diff --observations captures.
[unix]
policy-record *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev policy-record "$@"

[windows]
policy-record *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev policy-record @forwarded; exit $LASTEXITCODE

# Run doctests separately; nextest does not run them.
[unix]
test-doc *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test-doc "$@"

[windows]
test-doc *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test-doc @forwarded; exit $LASTEXITCODE

# Build with the native heavy slot and memory admission.
[unix]
build *args:
    @just heavy -- cargo build "$@"

[windows]
build *args:
    @{{windows_args}} just heavy -- cargo build @forwarded; exit $LASTEXITCODE

# Format directly; formatting does not take the heavy slot.
[unix]
fmt *args:
    @cargo fmt "$@"

[windows]
fmt *args:
    @{{windows_args}} cargo fmt @forwarded; exit $LASTEXITCODE

# Regenerate the GLib/libc call-site TSV from a pinned libpinyin checkout.
audit-glib-call-sites pin *args: python
    @CARGO_BUILD_JOBS=2 just heavy -- uv run --no-project --managed-python --with clang==21.1.7 python -X lazy_imports=normal tools/audit/glib_call_sites.py "$@"

# Lint with the native heavy slot and memory admission.
[unix]
clippy *args:
    @just heavy -- cargo clippy "$@"

[windows]
clippy *args:
    @{{windows_args}} just heavy -- cargo clippy @forwarded; exit $LASTEXITCODE

# Run the workspace benches with the native heavy slot and memory admission.
[unix]
bench *args:
    @just heavy -- cargo bench --workspace --benches "$@"

[windows]
bench *args:
    @{{windows_args}} just heavy -- cargo bench --workspace --benches @forwarded; exit $LASTEXITCODE

# Build every selected bench harness without running it.
[unix]
bench-smoke:
    @just bench --no-run

[windows]
bench-smoke:
    @{{windows_args}} just bench --no-run; exit $LASTEXITCODE

# Profile the W8 keystroke cycle with samply. macOS uses Homebrew's native
# build inputs; tools.dev.measure.profile refuses Linux-only pinning and
# fallbacks there.
[linux]
profile-w8 *args: python
    @just heavy -- uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure profile-w8 "$@"

[macos]
profile-w8 *args: python
    @just heavy -- env \
        SYSTEM_DEPS_BDB_NO_PKG_CONFIG=1 \
        SYSTEM_DEPS_BDB_LIB=db \
        SYSTEM_DEPS_BDB_SEARCH_NATIVE="$(brew --prefix berkeley-db@5)/lib" \
        SYSTEM_DEPS_BDB_INCLUDE="$(brew --prefix berkeley-db@5)/include" \
        PKG_CONFIG_PATH="$(brew --prefix glib)/lib/pkgconfig:$(brew --prefix kyoto-cabinet)/lib/pkgconfig:$(brew --prefix tkrzw)/lib/pkgconfig" \
        LIBRARY_PATH="$(brew --prefix berkeley-db@5)/lib:$(brew --prefix kyoto-cabinet)/lib:$(brew --prefix tkrzw)/lib:$(brew --prefix lz4)/lib:$(brew --prefix xz)/lib:$(brew --prefix zstd)/lib" \
        OXPINYIN_TKRZW_PREFIX="$(brew --prefix tkrzw)" \
        uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure profile-w8 "$@"

[windows]
profile-w8 *args:
    @{{windows_args}} echo "profile-w8: native macOS and Linux only; Windows is unsupported." >&2; exit 1

# Run one caller-selected fuzz target with the nightly workflow's ASLR wrapper.
# cargo fuzz runs in fuzz/, where rustup selects the nightly that
# fuzz/rust-toolchain.toml names. No recipe names the nightly.
[linux]
fuzz *args:
    @just heavy -- sh -c 'cd fuzz && exec setarch "$(uname -m)" -R cargo fuzz run "$@"' fuzz "$@"

# cargo-fuzz supports Darwin. Homebrew's tkrzw.pc names its compression
# libraries without their directory, so Homebrew's lib joins LIBRARY_PATH.
[macos]
fuzz *args:
    @just heavy -- env \
        LIBRARY_PATH="$(brew --prefix)/lib${LIBRARY_PATH:+:$LIBRARY_PATH}" \
        sh -c 'cd fuzz && exec cargo fuzz run "$@"' fuzz "$@"

# Windows fuzz execution is unverified; setarch is Linux-only.
[windows]
fuzz *args:
    @{{windows_args}} just heavy -- pwsh -NoLogo -NoProfile -CommandWithArgs 'Set-Location fuzz; cargo fuzz run @args; exit $LASTEXITCODE' @forwarded; exit $LASTEXITCODE

# Run native coverage with temporary state; run and report in one invocation.
[unix]
coverage *args: python
    @just heavy -- uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev coverage "$@"

# Windows coverage execution is unverified.
[windows]
coverage *args: python
    @{{windows_args}} just heavy -- uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev coverage @forwarded; exit $LASTEXITCODE

# Run a sanitizer gate with address, thread, leak or memory instrumentation.
[linux]
sanitize backend="" sanitizer="address": python
    @just heavy -- uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev sanitize "$1" "$2"

# Darwin uses Homebrew's Berkeley DB 5.3, Kyoto Cabinet and tkrzw. The recipe
# selects Darwin paths and omits Linux setarch. The tkrzw gate certifies the
# host library through distro-probe.sh; put Homebrew's tkrzw_dbm_util on PATH.
[macos]
sanitize backend="" sanitizer="address": python
    @just heavy -- env \
        SYSTEM_DEPS_BDB_NO_PKG_CONFIG=1 \
        SYSTEM_DEPS_BDB_LIB=db \
        SYSTEM_DEPS_BDB_SEARCH_NATIVE="$(brew --prefix berkeley-db@5)/lib" \
        SYSTEM_DEPS_BDB_INCLUDE="$(brew --prefix berkeley-db@5)/include" \
        PKG_CONFIG_PATH="$(brew --prefix glib)/lib/pkgconfig:$(brew --prefix kyoto-cabinet)/lib/pkgconfig:$(brew --prefix tkrzw)/lib/pkgconfig" \
        LIBRARY_PATH="$(brew --prefix berkeley-db@5)/lib:$(brew --prefix kyoto-cabinet)/lib:$(brew --prefix tkrzw)/lib:$(brew --prefix lz4)/lib:$(brew --prefix xz)/lib:$(brew --prefix zstd)/lib" \
        CPATH="$(brew --prefix berkeley-db@5)/include:$(brew --prefix kyoto-cabinet)/include" \
        PATH="$(brew --prefix tkrzw)/bin:$PATH" \
        uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev sanitize "$1" "$2"

# The gates are Linux/Darwin native builds; Windows sanitizers are unsupported.
[windows]
sanitize backend="" sanitizer="address":
    @{{windows_args}} echo "sanitize is unsupported on Windows: the sanitizer gates are Linux/Darwin native builds."
    @{{windows_args}} exit 1

# Run shared ABI export and allocation gates.
[unix]
runner *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev runner "$@"

[windows]
runner *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev runner @forwarded; exit $LASTEXITCODE

[unix]
abi *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev abi "$@"

[windows]
abi *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev abi @forwarded; exit $LASTEXITCODE

# Build or describe the registry's selected cell artifacts.
[unix]
cell-artifacts *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev cell-artifacts "$@"

[windows]
cell-artifacts *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev cell-artifacts @forwarded; exit $LASTEXITCODE

# Read registry metadata, optionally filtered by issue or kind.
[unix]
list *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev list "$@"

[windows]
list *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev list @forwarded; exit $LASTEXITCODE

# Describe an explicit registry selection without executing it.
[unix]
plan *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev plan "$@"

[windows]
plan *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev plan @forwarded; exit $LASTEXITCODE

# Run a measure recipe: startup (the controller holds the heavy slot), cold-qualify, or a measurement
# or profile recipe such as perf-baseline or snapshot (the list is tools/dev/measure/recipes.py).
[unix]
measure recipe *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure "$@"

# Startup measurement requires the Linux/macOS worker and measurement services.
[unix]
initbench *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure startup "$@"

# Qualify APFS cold preparation on named files; read-only, runs no worker.
[unix]
cold-qualify *paths: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure cold-qualify "$@"

[windows]
measure recipe *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure @forwarded; exit $LASTEXITCODE

[windows]
initbench *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure startup @forwarded; exit $LASTEXITCODE

# Oracle and compatibility recipes share host/runtime/fixture services.
[unix]
compat *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev compat "$@"
