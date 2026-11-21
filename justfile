# Native command interface. Recipes call their implementations directly.
# Native recipes never start a container.
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
    @uv run --offline --no-python-downloads --managed-python python -X utf8 -m tools.dev.rust_tables "$@"

[windows]
rust-tables *args:
    @{{windows_args}} uv run --offline --no-python-downloads --managed-python python -X utf8 -m tools.dev.rust_tables @forwarded; exit $LASTEXITCODE

# Data-only transport target for the real-just smoke tests; never starts work.
[unix]
[private]
_test-argv status *args:
    @uv run --managed-python python -X utf8 -c 'import json, sys; print(json.dumps(sys.argv[2:])); sys.exit(int(sys.argv[1]))' "$@"

[windows]
[private]
_test-argv status *args:
    @{{windows_args}} uv run --managed-python python -X utf8 -c 'import json, sys; print(json.dumps(sys.argv[2:])); sys.exit(int(sys.argv[1]))' @forwarded; exit $LASTEXITCODE


# Native provisioning after `uv python install`; uv itself must already be installed.
# It never starts a container: `just provision-oracle` provisions the pin oracle.
[unix]
provision *args:
    @uv run --managed-python python -X utf8 -m tools.dev.provision "$@"

[windows]
provision *args:
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.provision @forwarded; exit $LASTEXITCODE

# Fetch, verify and extract the pinned model20 archive into the cache root.
[unix]
fetch-model *args:
    @uv run --managed-python python -X utf8 -m tools.dev.model "$@"

[windows]
fetch-model *args:
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.model @forwarded; exit $LASTEXITCODE

# Fetch, verify and extract the pinned libpinyin-data package (the contract system data) into the cache root.
[unix]
fetch-contract-data *args:
    @uv run --managed-python python -X utf8 -m tools.dev.contract_data "$@"

[windows]
fetch-contract-data *args:
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.contract_data @forwarded; exit $LASTEXITCODE

# Report native capabilities and the managed interpreter identity. Doctor
# installs nothing: it names `uv python install` when the interpreter is missing.
[unix]
doctor:
    @command -v uv >/dev/null 2>&1 || { echo 'uv is not installed: see docs/runbooks/setup.md' >&2; exit 1; }
    @uv python find --managed-python >/dev/null 2>&1 || { echo 'the managed interpreter in .python-version is not installed: run `uv python install`' >&2; exit 1; }
    @uv run --managed-python python -X utf8 -m tools.dev.doctor

[windows]
doctor:
    @{{windows_args}} if (-not (Get-Command uv -ErrorAction SilentlyContinue)) { [Console]::Error.WriteLine('uv is not installed: see docs/runbooks/setup.md'); exit 1 }
    @{{windows_args}} uv python find --managed-python *> $null; if ($LASTEXITCODE -ne 0) { [Console]::Error.WriteLine('the managed interpreter in .python-version is not installed: run `uv python install`'); exit 1 }
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.doctor; exit $LASTEXITCODE

# Install, validate and assemble drop-in packages through the shared service.
[unix]
packaging *args:
    @uv run --managed-python python -X utf8 -m tools.dev.packaging "$@"

[windows]
packaging *args:
    @{{windows_args}} echo "packaging is unsupported on Windows: the assemblies need chmod, readelf, dpkg and the Linux package layout."
    @{{windows_args}} exit 1

# Nonshipping interactive modes use disposable W3 data. Linux and macOS only.
[unix]
repl mode *args:
    @uv run --managed-python python -X utf8 -m tools.dev.repl "$@"

# Stage built utilities in the tree expected by epico/trainer.
[unix]
trainer-layout target *args:
    @uv run --managed-python python -X utf8 -m tools.dev.packaging trainer-layout "$@"

[windows]
trainer-layout target *args:
    @{{windows_args}} echo "trainer-layout is unsupported on Windows: it copies extensionless executables for a Unix make tree."
    @{{windows_args}} exit 1

[unix]
check backend="":
    @uv run --managed-python python -X utf8 -m tools.dev.check "$1"

[windows]
check backend="":
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.check @forwarded; exit $LASTEXITCODE


# Native source-revision comparison and xctrace profiling.
[unix]
measure *args:
    @uv run --managed-python python -X utf8 -c 'from tools.dev.entry import require_utf8_mode; require_utf8_mode(); from tools.dev.measure.recipes import native, run; raise SystemExit(run(native))' "$@"

[windows]
measure *args:
    @{{windows_args}} uv run --managed-python python -X utf8 -c 'from tools.dev.entry import require_utf8_mode; require_utf8_mode(); from tools.dev.measure.recipes import native, run; raise SystemExit(run(native))' @forwarded; exit $LASTEXITCODE

# Run the native Rust tests through nextest.
[unix]
test *args:
    @cargo nextest run --profile local "$@"

[windows]
test *args:
    @{{windows_args}} cargo nextest run --profile local @forwarded; exit $LASTEXITCODE

# Run the CI Python harness discovery and exclusions (Linux, macOS and Windows).
[unix]
test-tools *args:
    @uv run --managed-python python -X utf8 -m tools.dev.test_runner "$@"

[windows]
test-tools *args:
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.test_runner @forwarded; exit $LASTEXITCODE

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

# Lint the workspace.
[unix]
clippy *args:
    @cargo clippy "$@"

[windows]
clippy *args:
    @{{windows_args}} cargo clippy @forwarded; exit $LASTEXITCODE

# Run the cargo-deny checks of CI's rust job; deny.toml holds the policy. The
# default-feature graph, then the feature-gated graphs, then the fuzz
# workspace's own graph. --locked holds each to its committed lockfile, so a
# root change that the fuzz lockfile does not follow fails here.
# One definition serves every shell: the commands take no arguments.
deny:
    @cargo deny --locked --log-level error check
    @cargo deny --locked --all-features --log-level error check
    @cargo deny --locked --log-level error --manifest-path fuzz/Cargo.toml check

# Check tools/ Python with ruff's pyflakes rules (F) only. upstream/ holds
# unmodified pin copies (tools/scripts2/README.md), so it is excluded.
[unix]
ruff:
    @uv run --locked --only-group lint --managed-python ruff check --select F --extend-exclude tools/scripts2/upstream tools

[windows]
ruff:
    @{{windows_args}} uv run --locked --only-group lint --managed-python ruff check --select F --extend-exclude tools/scripts2/upstream tools; exit $LASTEXITCODE

# Fail when a Rust function is above cyclomatic complexity (CCN) 15, with the
# Lizard that uv.lock names. A function's own `#lizard forgives` comment is
# the one exception (docs/runbooks/verification.md#complexity-limit).
# One definition serves every shell: the command takes no arguments.
complexity:
    @uv run --locked --only-group complexity --managed-python python -X utf8 -m lizard src/ tests/ codegen/ build.rs capi/ zhuyin-capi/ utils/ repl/ -l rust -C 15 -w

# Local and opt-in: no workflow runs it. Example: just mutants --package oxpinyin
# Run cargo-mutants (.cargo/mutants.toml); the arguments pass through unchanged.
[unix]
mutants *args:
    @cargo mutants "$@"

[windows]
mutants *args:
    @{{windows_args}} cargo mutants @forwarded; exit $LASTEXITCODE

# fuzz/tool-versions.env pins cargo-afl, and the LLVM plugins build against the
# LLVM of the fuzz nightly. tools/dev/fuzz.py states the rules; on any host
# other than Linux it refuses with one message.
# Install cargo-afl and the AFL++ release that it bundles, with the LLVM plugins (Linux only).
[unix]
fuzz-setup:
    @uv run --managed-python python -X utf8 -m tools.dev.fuzz setup

[windows]
fuzz-setup:
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.fuzz setup; exit $LASTEXITCODE

# Each run writes a new directory under fuzz/out/<target>/. Arguments after the
# target go to afl-fuzz, for example `-V 60`. FUZZ_CMPLOG=1 adds CmpLog;
# FUZZ_GUARD_SECONDS limits the wall time. tools/dev/fuzz.py states the rules;
# on any host other than Linux it refuses with one message.
# Fuzz one target with AFL++ and ASan, and fail on any finding (Linux only).
[unix]
fuzz target *args:
    @uv run --managed-python python -X utf8 -m tools.dev.fuzz run "$@"

[windows]
fuzz target *args:
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.fuzz run @forwarded; exit $LASTEXITCODE

# Run native coverage with temporary state; run and report in one invocation.
# cargo llvm-cov runs the tests under nextest, the only supported test runner.
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
    cargo llvm-cov nextest "$@"

# Windows coverage execution is unverified.
[windows]
coverage *args:
    @{{windows_args}} $scratch = Join-Path ([IO.Path]::GetTempPath()) ('oxpinyin-coverage-' + [guid]::NewGuid().ToString('N')); $target = Join-Path $scratch 'target'; New-Item -ItemType Directory -Path $scratch | Out-Null; $env:CARGO_LLVM_COV_TARGET_DIR = $target; $env:CARGO_LLVM_COV_BUILD_DIR = $target; [Console]::Error.WriteLine("CARGO_LLVM_COV_TARGET_DIR: $target"); try { cargo llvm-cov nextest @forwarded; $status = $LASTEXITCODE } finally { Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $scratch }; exit $status

# Capture one run of the pin's apostrophe observations in the capture image
# (docs/runbooks/pin-captures.md). The runtime is the platform's default; OXPINYIN_CAPTURE_RUNTIME
# or OXPINYIN_CONTAINER names another.
[unix]
capture-pin run:
    @uv run --managed-python python -X utf8 -m tools.oracle.image capture-pin "$1"

[windows]
capture-pin run:
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.oracle.image capture-pin @forwarded; exit $LASTEXITCODE

# Provision the pin oracle: native `just provision` first, then the oracle cells
# in containers through the selected runtime into the provisioning cache (the
# pin's entry of the cache root), then the environment file (docs/runbooks/oracle.md).
[unix]
provision-oracle *args: provision && _oracle-env
    @uv run --managed-python python -X utf8 -m tools.dev.provision oracle "$@"

[windows]
provision-oracle *args: provision && _oracle-env
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.provision oracle @forwarded; exit $LASTEXITCODE

# The one writer of the oracle environment file; only provision-oracle runs it.
[unix]
[private]
_oracle-env:
    @uv run --managed-python python -X utf8 -m tools.dev.provision oracle-env

[windows]
[private]
_oracle-env:
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.provision oracle-env; exit $LASTEXITCODE

# Run a sanitizer gate with address, thread, leak or memory instrumentation.
[linux]
sanitize backend="" sanitizer="address":
    @uv run --managed-python python -X utf8 -c 'import sys; from tools.dev.entry import require_utf8_mode; require_utf8_mode(); from tools.dev.test_runner import sanitize; raise SystemExit(sanitize(sys.argv[1:]))' "$1" "$2"

# Darwin uses Homebrew's Berkeley DB 5.3, Kyoto Cabinet and tkrzw. The recipe
# selects Darwin paths and omits Linux setarch. The tkrzw gate certifies the
# library loaded by Homebrew's tkrzw_dbm_util.
[macos]
sanitize backend="" sanitizer="address":
    @env \
        SYSTEM_DEPS_BDB_NO_PKG_CONFIG=1 \
        SYSTEM_DEPS_BDB_LIB=db \
        SYSTEM_DEPS_BDB_SEARCH_NATIVE="$(brew --prefix berkeley-db@5)/lib" \
        SYSTEM_DEPS_BDB_INCLUDE="$(brew --prefix berkeley-db@5)/include" \
        PKG_CONFIG_PATH="$(brew --prefix glib)/lib/pkgconfig:$(brew --prefix kyoto-cabinet)/lib/pkgconfig" \
        LIBRARY_PATH="$(brew --prefix berkeley-db@5)/lib:$(brew --prefix kyoto-cabinet)/lib" \
        CPATH="$(brew --prefix berkeley-db@5)/include:$(brew --prefix kyoto-cabinet)/include" \
        uv run --managed-python python -X utf8 -c 'import sys; from tools.dev.entry import require_utf8_mode; require_utf8_mode(); from tools.dev.test_runner import sanitize; raise SystemExit(sanitize(sys.argv[1:]))' "$1" "$2"

# The gates are Linux/Darwin native builds; Windows sanitizers are unsupported.
[windows]
sanitize backend="" sanitizer="address":
    @{{windows_args}} echo "sanitize is unsupported on Windows: the sanitizer gates are Linux/Darwin native builds."
    @{{windows_args}} exit 1

# Compile and run the C++ pinyin.h smoke gate.
[unix]
cpp-smoke:
    @uv run --managed-python python -X utf8 -c 'import sys; from tools.dev.entry import require_utf8_mode; require_utf8_mode(); from tools.dev.runners import cpp_smoke; raise SystemExit(cpp_smoke(sys.argv[1:]))'

[windows]
cpp-smoke:
    @{{windows_args}} echo "cpp-smoke is unsupported on Windows: it needs readelf, ldd and an ELF .so."
    @{{windows_args}} exit 1

[unix]
abi *args:
    @uv run --managed-python python -X utf8 -m tools.dev.abi "$@"

[windows]
abi *args:
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.abi @forwarded; exit $LASTEXITCODE

# Build or describe the registry's selected cell artifacts.
[unix]
cell-artifacts *args:
    @uv run --managed-python python -X utf8 -m tools.dev.cell "$@"

[windows]
cell-artifacts *args:
    @{{windows_args}} uv run --managed-python python -X utf8 -m tools.dev.cell @forwarded; exit $LASTEXITCODE

# Run Valgrind recipes under the shared cancellation and failure handlers.
[linux]
measure-callgrind-control *args:
    @uv run --managed-python python -X utf8 -c 'from tools.dev.entry import require_utf8_mode; require_utf8_mode(); from tools.dev.measure.valgrind import callgrind_control; from tools.dev.measure.recipes import run; raise SystemExit(run(callgrind_control))' "$@"

[linux]
measure-callgrind-differential *args:
    @uv run --managed-python python -X utf8 -c 'from tools.dev.entry import require_utf8_mode; require_utf8_mode(); from tools.dev.measure.valgrind import callgrind_differential; from tools.dev.measure.recipes import run; raise SystemExit(run(callgrind_differential, pin_comparison=True))' "$@"

[linux]
measure-system-import-profile *args:
    @uv run --managed-python python -X utf8 -c 'from tools.dev.entry import require_utf8_mode; require_utf8_mode(); from tools.dev.measure.valgrind import system_import_profile; from tools.dev.measure.recipes import run; raise SystemExit(run(system_import_profile, pin_comparison=True))' "$@"
