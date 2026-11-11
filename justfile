# Native command interface. Thin recipes only: selection and orchestration
# stay in tools.dev. Native recipes never start a container.
set positional-arguments
set minimum-version := "1.59.0"

# Windows uses pwsh; doctor checks the floor in tools/dev/pwsh-floor.txt.
[windows]
set shell := ["pwsh", "-NoLogo", "-NoProfile", "-CommandWithArgs"]

# Discard just's recipe name once, before native argument forwarding.
windows_args := '$null, [object[]]$forwarded = $args;'

# The cache root of everything the tooling fetches or builds from pinned sources,
# shared by all worktrees (docs/runbooks/setup.md#cache-directory).
export OXPINYIN_CACHE := if env("OXPINYIN_CACHE", "") == "" { join(cache_directory(), "oxpinyin") } else { env("OXPINYIN_CACHE") }

default: doctor

# Print the cache root used by the fetch and provisioning recipes.
[unix]
cache-root:
    @printf '%s\n' "$OXPINYIN_CACHE"

[windows]
cache-root:
    @[Console]::WriteLine($env:OXPINYIN_CACHE)

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

[windows]
provision *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev provision @forwarded; exit $LASTEXITCODE

# Fetch, verify and extract the pinned model20 archive into the cache root.
[unix]
fetch-model *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev fetch-model "$@"

[windows]
fetch-model *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev fetch-model @forwarded; exit $LASTEXITCODE

# Fetch, verify and extract the pinned libpinyin-data package (the contract system data) into the cache root.
[unix]
fetch-contract-data *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev fetch-contract-data "$@"

[windows]
fetch-contract-data *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev fetch-contract-data @forwarded; exit $LASTEXITCODE

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

[windows]
packaging *args:
    @{{windows_args}} echo "packaging is unsupported on Windows: the assemblies need chmod, readelf, dpkg and the Linux package layout."
    @{{windows_args}} exit 1

# Stage built utilities in the tree expected by epico/trainer.
[unix]
trainer-layout target *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev packaging trainer-layout "$@"

[windows]
trainer-layout target *args:
    @{{windows_args}} echo "trainer-layout is unsupported on Windows: it copies extensionless executables for a Unix make tree."
    @{{windows_args}} exit 1

[unix]
check backend="": python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev check {{backend}}

[windows]
check backend="": python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev check {{backend}}; exit $LASTEXITCODE

# A distro tkrzw is found through pkg-config with nothing more. To use this
# build instead, put DIR/lib/pkgconfig on PKG_CONFIG_PATH and DIR/lib on the
# runtime library path (DIR/bin on Windows), as CI's tkrzw action does.
# Build upstream tkrzw at the provisioning revision into an empty DIR.
[unix]
tkrzw-build prefix: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.tkrzw.build build --prefix "$1"

[windows]
tkrzw-build prefix: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.tkrzw.build build --prefix @forwarded; exit $LASTEXITCODE

# Run the native Rust tests through nextest.
[unix]
test *args:
    @cargo nextest run --profile local "$@"

[windows]
test *args:
    @{{windows_args}} cargo nextest run --profile local @forwarded; exit $LASTEXITCODE

# Run the CI Python harness discovery and exclusions (Linux, macOS and Windows).
[unix]
test-tools *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test-tools "$@"

[windows]
test-tools *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev test-tools @forwarded; exit $LASTEXITCODE

# Run doctests separately; nextest does not run them.
[unix]
test-doc *args:
    @cargo test --doc "$@"

[windows]
test-doc *args:
    @{{windows_args}} cargo test --doc @forwarded; exit $LASTEXITCODE

# Build the workspace.
[unix]
build *args:
    @cargo build "$@"

[windows]
build *args:
    @{{windows_args}} cargo build @forwarded; exit $LASTEXITCODE

# Format directly.
[unix]
fmt *args:
    @cargo fmt "$@"

[windows]
fmt *args:
    @{{windows_args}} cargo fmt @forwarded; exit $LASTEXITCODE

# Regenerate the GLib/libc call-site TSV from a pinned libpinyin checkout.
[unix]
audit-glib-call-sites pin *args: python
    @uv run --no-project --managed-python --with "$(uv run --no-project --managed-python python tools/audit/glib_call_sites.py --print-clang-requirement)" python -X lazy_imports=normal tools/audit/glib_call_sites.py "$@"

# The audit needs bear, make and Unix linker options.
[windows]
audit-glib-call-sites pin *args:
    @{{windows_args}} echo "audit-glib-call-sites is unsupported on Windows: the audit needs bear, make and Unix linker options."
    @{{windows_args}} exit 1

# Lint the workspace.
[unix]
clippy *args:
    @cargo clippy "$@"

[windows]
clippy *args:
    @{{windows_args}} cargo clippy @forwarded; exit $LASTEXITCODE

# Local and opt-in: no workflow runs it. Example: just mutants --package oxpinyin-core
# Run cargo-mutants (.cargo/mutants.toml); the arguments pass through unchanged.
[unix]
mutants *args:
    @cargo mutants "$@"

[windows]
mutants *args:
    @{{windows_args}} cargo mutants @forwarded; exit $LASTEXITCODE

# Run one caller-selected fuzz target with the nightly workflow's ASLR wrapper.
# cargo fuzz runs in fuzz/, where rustup selects the nightly that
# fuzz/rust-toolchain.toml names. No recipe names the nightly.
[linux]
fuzz *args:
    @sh -c 'cd fuzz && exec setarch "$(uname -m)" -R cargo fuzz run "$@"' fuzz "$@"

# cargo-fuzz supports Darwin. Homebrew's tkrzw.pc names its compression
# libraries without their directory, so Homebrew's lib joins LIBRARY_PATH.
[macos]
fuzz *args:
    @env \
        LIBRARY_PATH="$(brew --prefix)/lib${LIBRARY_PATH:+:$LIBRARY_PATH}" \
        sh -c 'cd fuzz && exec cargo fuzz run "$@"' fuzz "$@"

# Windows fuzz execution is unverified; setarch is Linux-only.
[windows]
fuzz *args:
    @{{windows_args}} pwsh -NoLogo -NoProfile -CommandWithArgs 'Set-Location fuzz; cargo fuzz run @args; exit $LASTEXITCODE' @forwarded; exit $LASTEXITCODE

# Run native coverage with temporary state; run and report in one invocation.
# Cargo owns argument and report selection; the arguments pass through unchanged.
# Both state directories live in one scratch directory that is removed on exit.
[unix]
coverage *args:
    #!/usr/bin/env bash
    set -euo pipefail
    scratch=$(mktemp -d "${TMPDIR:-/tmp}/oxpinyin-coverage-XXXXXX")
    trap 'rm -rf "$scratch"' EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    export CARGO_LLVM_COV_TARGET_DIR="$scratch/target" CARGO_LLVM_COV_BUILD_DIR="$scratch/target"
    echo "CARGO_LLVM_COV_TARGET_DIR: $CARGO_LLVM_COV_TARGET_DIR" >&2
    cargo llvm-cov "$@"

# Windows coverage execution is unverified.
[windows]
coverage *args:
    @{{windows_args}} $scratch = Join-Path ([IO.Path]::GetTempPath()) ('oxpinyin-coverage-' + [guid]::NewGuid().ToString('N')); $target = Join-Path $scratch 'target'; New-Item -ItemType Directory -Path $scratch | Out-Null; $env:CARGO_LLVM_COV_TARGET_DIR = $target; $env:CARGO_LLVM_COV_BUILD_DIR = $target; [Console]::Error.WriteLine("CARGO_LLVM_COV_TARGET_DIR: $target"); try { cargo llvm-cov @forwarded; $status = $LASTEXITCODE } finally { Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $scratch }; exit $status

# Capture one run of the pin's apostrophe observations in the capture image
# (docs/runbooks/pin-captures.md). The runtime is the platform's default; OXPINYIN_CAPTURE_RUNTIME
# or OXPINYIN_CONTAINER names another.
[unix]
capture-pin run: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image capture-pin "$1"

[windows]
capture-pin run: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image capture-pin @forwarded; exit $LASTEXITCODE

# Provision the oracle cells into the provisioning cache `just provision` also uses
# (the pin's entry of the cache root), running `provision oracle` in the
# performance image through the selected runtime (docs/runbooks/pin-captures.md).
[unix]
provision-oracle *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image provision-oracle "$@"

[windows]
provision-oracle *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image provision-oracle @forwarded; exit $LASTEXITCODE

# Run a sanitizer gate with address, thread, leak or memory instrumentation.
[linux]
sanitize backend="" sanitizer="address": python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev sanitize "$1" "$2"

# Darwin uses Homebrew's Berkeley DB 5.3, Kyoto Cabinet and tkrzw. The recipe
# selects Darwin paths and omits Linux setarch. The tkrzw gate certifies the
# host library by the library its tkrzw_dbm_util loads; put Homebrew's tkrzw_dbm_util on PATH.
[macos]
sanitize backend="" sanitizer="address": python
    @env \
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

# Compile and run the C++ pinyin.h smoke gate.
[unix]
cpp-smoke: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev cpp-smoke

[windows]
cpp-smoke:
    @{{windows_args}} echo "cpp-smoke is unsupported on Windows: it needs readelf, ldd and an ELF .so."
    @{{windows_args}} exit 1

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

# Run a Valgrind recipe (tools/dev/measure/recipes.py).
[unix]
measure recipe *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure "$@"

[windows]
measure recipe *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev measure @forwarded; exit $LASTEXITCODE

# Build and enter the local oracle-based performance environment on the host architecture.
[unix]
performance *args: python
    @uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image performance "$@"

[windows]
performance *args: python
    @{{windows_args}} uv run --no-project --managed-python python -X lazy_imports=normal -m tools.oracle.image performance @forwarded; exit $LASTEXITCODE
