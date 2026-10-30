# Native contributor setup

Start from [CONTRIBUTING](../../CONTRIBUTING.md). These instructions build
and test local files; they do not install an input method or replace your
system libpinyin. Stop if a command fails and keep that command and its full
error before continuing. In Bash, `echo $?` immediately after a command
shows its exit status (zero means success); PowerShell uses `$LASTEXITCODE`.
Choose your operating system below. Ordinary native work
requires no container, oracle or model20 (upstream reference model data).

Every repository shell script requires Bash 5.0 or newer on Linux and macOS.
`just doctor` reports the `bash` that `env bash` selects and fails below 5.0.
Linux distributions provide it; the macOS section installs Homebrew's Bash and
checks the selection. On Windows doctor reports Bash as not applicable and
never fails on its version: native Windows recipes use PowerShell 7.

Rust's **toolchain** consists of the compiler and its tools. rustup installs
and selects it; `rust-toolchain.toml` is the version/component authority.
The **linker** connects compiled code into executables and native libraries.

## Linux

Use Bash 5.0 or newer. Linux is one native platform target. You need Git,
curl and CA certificates, a C/C++ compiler and linker, Make, pkg-config,
libclang, Berkeley DB **5.3** development headers/libraries and GLib
development files.
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

Repository Python uses uv and its managed CPython 3.15 series with the GIL.
`uv.toml` sets uv's floor, 0.13.1; CI installs exactly that version.

`just doctor` is the one check of every tool's minimum: Bash 5.0, uv,
CPython, just, cargo-nextest, cargo-c, cargo-fuzz, cargo-llvm-cov, and
ShellCheck 0.11.0. ShellCheck is advisory because only the lint job needs it.
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

Provision the interpreter through `just python`. It installs the interpreter
`.python-version` names only when none is present, and fails if that request
still does not resolve afterwards. Every other Python recipe depends on it, so a recipe
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
interpreter (with `-X lazy_imports=normal`), followed by `runner run-all` and its
arguments. This preserves the caller's working directory; its `.python-version`
does not decide the interpreter. Direct-run Python
harness files rely on their `uv` shebang and a working directory inside the
checkout. The two Axis J ledger generators keep the historical command their
tracked ledgers record, and the check exempts exactly those two files. The file's request ends in `+gil`, which requires the GIL-enabled build.
uv 0.13.1 has no separate `+gil` archive, and `uv python install` with no
arguments fails on that request, so `tools/install-python.sh` (the one place that
derives it) installs the same series without `+gil` and selection keeps the file's
request. The check `tools/dev/python-request.test.py` fails when a version request
appears anywhere else, outside the dated records in `docs/findings/` and
`docs/perf/`.

**Rule (#1230).** Provision the repository's managed Python interpreter
through `just python`. Never pass the file's `+gil` request, or its expanded
platform-specific identifier, to `uv python install`. Do not substitute another
interpreter or upgrade uv to work around this error. A later series is a deliberate change: the tooling
relies on 3.15's `-X lazy_imports` modes. Do not use a system Python, a virtual
environment, `pyproject.toml` or `uv.lock`; the checkout's `.python-version`
and `uv.toml` name the same interpreter series and uv floor.

To regenerate the W3 content fixtures, run this command from the checkout root:

```bash
uv run --no-project --managed-python python -X lazy_imports=normal \
  tools/generate_w3_fixtures.py
```

`just build` and `just clippy` pass Cargo arguments unchanged through
`just heavy`, including the host-wide slot and memory admission. `just fmt`
calls Cargo directly without taking the slot. Select the same native packages
and backend as the Cargo command you would otherwise run:

```bash
just build --locked -p oxpinyin-core
just clippy --locked -p oxpinyin-core --all-targets -- -D warnings
just fmt --all --check
```

### Heavy-work admission

`AGENTS.md`, "Verification cells", rule 8 (maintainer ruling 2026-10-19 UTC)
keeps the principles and the list of what is heavy. This section holds the
mechanics, as `tools/dev/slot.py` and `tools/dev/host.py` implement them;
`tools/bisection/run-all.py` applies them to cell builds and heavy runners.
They apply on Linux and macOS. The native slot is not validated on Windows.

**Slot, descriptor and signals.** The slot is an `flock` on
`heavy-slot-<n>` in the lock directory. The heavy child receives an
inheritable slot descriptor through `pass_fds` and runs in its own session.
The slot stays held until the child and its descendants close their copies.
The parent keeps its copy until it reaps the child. It closes that copy
without unlocking the shared file description. On SIGTERM, SIGINT, or SIGHUP,
terminate each child process group: SIGTERM to the group, five seconds to
exit, then SIGKILL to the group. Reap the child before releasing checkout
locks or the slot. `just heavy` exits 130 on cancellation, 1 on any other
admission error (a refusal or an expired wait included), and otherwise with
the child's status (128 plus the signal number for a child killed by a
signal). Cancellation, expiry, and an OOM-killed child never count as PASS or
SKIP.

**Nested work.** The child's environment carries `OXPINYIN_HEAVY_SLOT_HELD`,
a JSON object with the resolved `lock_dir`, the `slot` index, the holder
`pid` and the descriptor `fd`. Nested work uses it to identify the slot and
holder pid. Three outcomes are distinct:

- A malformed marker is a hard error, `Invalid OXPINYIN_HEAVY_SLOT_HELD`:
  it does not parse as JSON, is not an object with exactly the keys
  `lock_dir`, `slot`, `pid` and optionally `fd`, or has a `lock_dir` that is
  not an absolute string, a `slot` that is not a nonnegative integer, a `pid`
  that is not a positive integer, or an `fd` that is not a nonnegative
  integer.
- A well-formed marker that fails the ownership and liveness checks is
  reported stale (`stale OXPINYIN_HEAVY_SLOT_HELD: ...`), and the slot is
  acquired normally. The checks are: the marker's lock directory is this
  host's lock directory, the slot record names that pid, the pid is alive,
  and the pid is an ancestor.
- A marker that passes those checks is inherited, and valid nested work does
  not acquire another slot. If its optional descriptor does not name the slot
  file (or cannot be read), only the descriptor is discarded; the slot is
  still inherited (`HeavySlot._inherit`).

**Slot count.** Two slots are the default when total memory is at least
15 GiB; one below that or when total memory is unknown. `OXPINYIN_HEAVY_SLOTS`
sets the slot count; a value below one, or one that is not an integer, gives
one slot.

**Lock directory.** `OXPINYIN_LOCK_DIR` must name an absolute directory.
Reject a relative value with a clear message (`OXPINYIN_LOCK_DIR must be an
absolute path`); an empty value counts as unset. The default is
`$XDG_CACHE_HOME/oxpinyin/locks`, with `~/.cache` when `XDG_CACHE_HOME` is
unset, or `%LOCALAPPDATA%\oxpinyin\locks` on Windows. Resolve the default
directory once. Pass the resolved path in each heavy child's environment as
`OXPINYIN_LOCK_DIR`. Nested calls use that path after a directory change.
`python -m tools.dev slot status` prints the directory and the slot count.

**Job and thread variables.** The slot limits concurrent heavy tasks. The
defaults limit processes inside each task. Leave `CARGO_BUILD_JOBS`,
`RUST_TEST_THREADS`, and `--jobs` unset unless a measurement needs fixed
values. An explicit setting has one of two meanings, and the rest of this
section uses the same two:

- `CARGO_BUILD_JOBS` and `RUST_TEST_THREADS`: an explicit value is kept
  unchanged, and admission needs room for its full process count. (`--jobs` of
  `run-all.py` counts concurrent runner rows; each row is admitted on its
  own.)
- Contract-case concurrency, which is `OXPINYIN_CONTRACT_JOBS`,
  `contract-diff.py --jobs` and `run-all.py --contract-jobs`: a requested
  maximum that admission may reduce (#1143; `docs/runbooks/oracle.md`). It is
  reduced to the cases that fit, at 1,024 MiB each, in the room left after
  headroom and other reservations. It never forces a worker when none fits:
  one worker's estimate is kept, so admission can refuse the work instead of
  launching it.
  Without a setting, a contract-diff run of `run-all.py` gets a default
  maximum: its share of the CPUs (the CPU count divided by the contract-diff
  runs that can be live together, at least one), reduced the same way
  (#1240). `--contract-jobs 1` keeps it serial.

Cargo job counts accept positive integers, negative integers, and `default`;
zero and values outside the signed 32-bit range are rejected. Test thread
counts and `OXPINYIN_CONTRACT_JOBS` must be positive integers. Invalid values
cause a clear failure before a slot is taken.

**Memory and CPU readers.** On Linux, admission takes the smallest known memory room from `MemAvailable`
and the current cgroup's visible ancestors. It reads v2 `memory.max` and
`memory.current` with the existing clean-cache accounting. For v1 it finds
the memory and CPU controller mounts through `/proc/self/mountinfo`, including
mounts outside `/sys/fs/cgroup` and mounts exposing only a subtree.
V1 room is `memory.limit_in_bytes` minus usage after subtracting reclaimable
cache from `memory.usage_in_bytes`. The cache candidates are the smaller of
`cache` minus `shmem` and `active_file` plus `inactive_file`; reserve `dirty`
plus `writeback`, capped at those candidates. Use all six `total_*` counters
from `memory.stat` when `total_cache` is present, otherwise all six local
counters. Missing, unreadable, malformed, incomplete, duplicate or inconsistent
statistics leave the reclaimable-cache allowance unknown; readable limit and
usage retain the conservative room `max(0, limit - usage)`.
Near-maximum v1 sentinel limits are unlimited. CPU affinity is
combined with v2 `cpu.max` or v1 `cpu.cfs_quota_us` / `cpu.cfs_period_us`;
a v1 quota of `-1` is unlimited. Hybrid hosts use the controllers present in
each hierarchy. Unreadable values stay unknown in the admission notes; only
known room participates in the minimum, and no known memory means no admission.

macOS reads available memory as the Mach `host_statistics64` free plus
inactive pages and total memory from `hw.memsize`. Windows reads both from
`GlobalMemoryStatusEx`. Neither reads cgroups; the CPU count there is
`os.cpu_count()`, where Linux combines affinity with the cgroup quotas as
above. Report unreadable limits as unknown. Use the limits that can be read.
If no memory reading is available, treat the capacity as zero. Use no fixed
fallback for heavy work. An explicit setting does not bypass this check: a
kept `CARGO_BUILD_JOBS` or `RUST_TEST_THREADS` needs room for its full
process count, and a requested contract-case maximum is reduced to what fits.

**Admission sequence.** The plan before admission is provisional: it reads
resources, prints them and the default settings, and takes no slot. Take the
slot and read resources again. Print the effective settings and any change
(`defaults changed after admission`). Take the short host-wide
`heavy-admission` lock before the final resource read and decision. The lock
is attempted without blocking; if another process holds it, release a newly
acquired slot and retry. Within that lock:

- Charge each live holder in another locked slot its estimate less the
  current use of its process tree (summed Pss on Linux, physical footprint on
  macOS, working sets on Windows), at least zero; charge the full estimate
  when any process's use cannot be read. A holder with no recorded estimate is
  charged nothing, because its record is provisional: `HeavySlot.try_acquire`
  writes the holder record (pid, checkout, command, start time, token) without
  `estimate_mib`. The estimate is published under the `heavy-admission` lock
  only when the work fits, and no heavy child starts before that. A live
  holder without an estimate therefore cannot launch without taking the same
  lock and accounting for every reservation made meanwhile.
- Subtract container memory reservations (the `container-memory-*.json`
  records of the compatibility container launcher) as well. An invalid record
  makes available memory unknown.
- Record this holder's estimate in its slot file before releasing the
  admission lock. An inherited holder keeps the larger of its previous and new
  estimates. Ignore records whose pid is dead.

**Headroom and estimates.** The headroom is 10% of total memory, at most
2,048 MiB, and 2,048 MiB when total memory is unknown. It is 0 only for a
compatibility-container guest (`OXPINYIN_COMPAT_CONTAINER=1`) with a known hard
memory limit. The host already applies desktop headroom when it reserves the
container's full limit (`ContainerReservation.acquire`), so the guest charges
it once (#978). Admission inside the guest still applies: its memory
measurements, other reservations and the explicit-setting checks above. The cell build requires its build estimate plus the headroom.
The build estimate is the largest of one rustc (600 MiB), the Cargo job count
times 600 MiB, and the test thread count times 64 MiB. A Cargo job count of
`default` counts the CPUs and a negative count is subtracted from them, at
least one. The minimum build estimate is one rustc. Heavy runners require the
larger of `RUNNER_PEAK_MIB` (1,024 MiB) and their build estimate. Unset
values default to a per-slot share of the room after headroom: Cargo jobs at
most the CPU count and a rustc each, test threads at most the CPU count and
64 MiB each; one process is the minimum while it fits the whole room. A kept
`CARGO_BUILD_JOBS` or `RUST_TEST_THREADS` requires room for its full process
count; a requested contract-case maximum is instead reduced to what fits,
with one worker's estimate kept (see the job and thread variables above).

**Waiting and refusal.** Under an inherited slot, insufficient memory causes
immediate FAIL:
`resource-blocked: inherited slot; <estimate> MiB needed, <available> MiB available`.
Unknown memory under an inherited slot also causes immediate FAIL:
`resource-blocked: available memory unknown`.
Do not wait for memory while an ancestor holds that slot.

If memory is insufficient or unknown, release this task's acquired slot
before waiting. Hold no checkout lock during this wait (checkout locks are
taken after admission). Print the estimate, available memory after headroom,
and `no process started` (`waiting for memory: estimate ... MiB, ... MiB
available after headroom; no process started: <command>`, at the start and
every 30 seconds). While every slot is held, print who holds them
(`waiting for a heavy slot ...; held by: ...`) on the same schedule. Retry slot
acquisition and resource checks, every half second, until admission or expiry.
`OXPINYIN_RESOURCE_WAIT_SECONDS` limits admission waits; its default is 1800
seconds. It must be finite and nonnegative; zero permits one immediate
attempt. One limit covers the slot wait, the admission-mutex wait and the
memory wait.

Expiry gives FAIL with the cause of the last attempt, and the wrapper exits
nonzero. The available figure below is after headroom and after the other
reservations:

- Memory was the cause: `resource-blocked: <estimate> MiB needed, <available> MiB available`,
  from the figures of that memory decision, after a `memory refused:` line
  that shows what the decision subtracted.
- Memory stays unknown: `resource-blocked: available memory unknown`.
- The slot was still held: the reason starts `slot-wait-expired:`, gives the
  time waited and the limit, and names each slot's holder from its slot record
  (`slot <n>: pid <pid> in <checkout> since <time>: <command>`), with no memory
  figures.
- The admission mutex was held elsewhere:
  `admission-wait-expired: heavy-admission mutex; no process started`.

**Runner scheduling and reporting.** The runner timeout starts after
admission. Heavy rows wait before entering the worker pool. Light rows can
start while heavy rows wait; the pool keeps the requested size. Acquire the
slot before sorted checkout locks. Report slot wait in `slot_wait_seconds`,
checkout lock wait in `lock_wait_seconds`, and cell build slot wait in
`build_slot_wait_seconds`. Report build and runner memory wait in
`memory_wait_seconds`.

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

`just list` reads `tools/bisection/runners.json`. Use `--issue 541`,
`--kind gate` or `--heavy` to filter its static rows. `just plan` requires
an explicit selection, for example `just plan --runners bisect --cells bdb`.
Both commands also accept `--runners`, `--cells`, repeated `--issue` and
`--kind`; different filters intersect. Empty values, unknown selections and
filters matching no rows fail. Plans print command and environment templates
as registry metadata. They do not resolve oracle inputs, build artifacts,
take a heavy slot or run commands. Cell names describe compatibility metadata;
they do not make the native interface execute a container or oracle check.

The Windows recipes use PowerShell and preserve the child status explicitly.
Their argument transport and slot behavior remain TODO (no Windows host).

The native Rust test entry point uses cargo-nextest at or above the
`nextest-version` in `.config/nextest.toml` ([profile inheritance](https://nexte.st/docs/configuration/#profile-inheritance)
needs it); `just doctor` reports it. Install it with
`cargo install cargo-nextest --locked` (or the prebuilt release). `just test` calls `slot.run_command` directly through
`tools.dev.test_runner` with the local profile. Pass the same crate and
feature selection as cargo test (the default workspace feature is Berkeley
DB). Set `NEXTEST_PROFILE=ci` for the CI limits. `just test-doc` runs
`cargo test --doc` under heavy-slot admission.

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
brew install berkeley-db@5 glib pkgconf
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
export OXPINYIN_BDB_INCLUDE_DIR="$(brew --prefix berkeley-db@5)/include"
export OXPINYIN_BDB_LIB_DIR="$(brew --prefix berkeley-db@5)/lib"
export CARGO_TARGET_DIR=target/bdb
just check bdb
```

`export` sets a setting inherited by commands in this terminal. Both Berkeley
DB paths are needed because Homebrew's `berkeley-db@5` is keg-only. macOS uses
the same Berkeley DB 5.3 reference backend as Linux. This is a real storage
link/run check; a mere `cargo check` does not execute the tests.
In a fresh Bash terminal, return to the checkout, source Cargo's environment,
repeat all three exports and the check. Linux C-ABI packaging and Linux reference
differentials are separate evidence, not permanent restrictions on portability.

The existing macOS CI uses this broader portable scope:

```bash
cargo test --locked --workspace --no-default-features --features bdb \
  --exclude oxpinyin-capi --exclude oxpinyin-zhuyin-capi \
  --exclude oxpinyin-corpus --exclude oxpinyin-kmm \
  --exclude oxpinyin-punct --exclude oxpinyin-word
```

The [validation status](#validated-on--not-validated) distinguishes native macOS CI
from hands-on setup evidence. The current CI exclusions describe maintained
build integration, not permanent limitations of the portable Rust code.

tkrzw remains optional and stays covered on Linux CI. To use it locally,
install `brew install tkrzw`, set
`export LIBRARY_PATH="$(brew --prefix)/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"`
so its lz4/lzma/zstd dependencies link, and select
`export CARGO_TARGET_DIR=target/tkrzw` followed by `just check tkrzw`.
Keep each backend's target and user-data directories separate.

Fuzzing and the backend sanitizer gates run natively on macOS. The recipes
select Homebrew's Berkeley DB 5.3, Kyoto Cabinet and tkrzw and use the pinned
nightlies; no Linux `setarch`:

```bash
brew install kyoto-cabinet tkrzw
rustup toolchain install nightly-2026-08-01 --profile minimal
rustup toolchain install nightly-2026-09-07 --profile minimal
just fuzz parser -- -runs=100
just sanitize bdb address
just sanitize bdb thread
just sanitize kc address
just sanitize kc thread
just sanitize tkrzw address
just sanitize tkrzw thread
```

`capi-commands` also runs natively; the fuzz recipe supplies the facade's
GLib and Berkeley DB paths. AddressSanitizer runs with leak detection off on
Darwin. Standalone LeakSanitizer is refused on macOS arm64, and
MemorySanitizer has no Darwin target; both refusals name the host. Linux
keeps its existing sanitizer commands and paths.

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

Repository Python tooling on Windows runs through the PowerShell recipes
(unverified: no Windows host, [`tooling-python.md`](../plans/tooling-python.md)).
Install uv with its native Windows installer, then just (`cargo install just
--locked`), then run `just python`; the rule above applies unchanged.

All three packages link and run natively without a DBM library or container.
Success is zero failed tests and exit code zero (`$LASTEXITCODE`). In a fresh
Developer PowerShell, `Set-Location "$HOME/src/oxpinyin"` and repeat the test;
use your real directory for an existing checkout. Rustup chooses the same
repository toolchain automatically. Editors/debuggers use this same native
MSVC environment and toolchain; no project-specific IDE is required.

**Storage-dependent native Windows scope:** the Windows CI job runs clippy,
`cargo nextest run --profile ci` and the doctests over the
portable workspace with vcpkg `berkeleydb:x64-windows` (4.8), explicit include,
library, DLL and LLVM paths, and an **unsurveyed-version exception reserved
for CI**. Do not copy that exception into developer or production setup.
The repository has no validated contributor recipe for surveyed Berkeley DB
5.3 on MSVC yet. That is a native dependency/version-survey gap, not evidence
that Windows is inherently unsupported. Native Rust-only work above is
available now; storage-dependent work uses the Windows CI evidence and needs
a validated 5.3 installation before claiming a supported native local run.

The existing CI exclusion set is the same six packages shown in the macOS
command, with default BDB instead of explicit backend selection. The oracle's Linux
FFI feature is not selected. Native Windows C-ABI/oracle behavior is not
established by those tests. Physical Windows hardware is not needed to use
this CI coverage.

**When the Windows job runs:** the full Windows workspace job is the
`test-windows` job in `verify-nightly.yml`. It runs every night and on manual
dispatch, not on every pull request. Windows remains a supported native
target; only the verification frequency changed. Before merging any of the
following, dispatch the job on the branch with `windows_only` set:

- changes to Windows-sensitive storage, filesystem or native-build code
  (the store backends and their build scripts, path and file handling,
  build scripts that compile or link native code);
- toolchain updates and relevant dependency updates (`rust-toolchain.toml`,
  `Cargo.lock` changes that reach the portable workspace, the vcpkg port);
- releases.

```bash
gh workflow run verify-nightly.yml --ref <branch> -f windows_only=true
```

With `windows_only` set, every other nightly job is skipped. A dispatch
without it runs the whole nightly workflow, including the Windows job.

Recent upstream MinGW portability changes were checked separately from
oxpinyin's MSVC support; see the [validation status](#validated-on--not-validated).
The frozen oracle does not define which native operating systems oxpinyin
can support, and its pins are unchanged.

WSL is a separate Linux development option: follow the Linux section from
its Bash terminal. [Microsoft recommends the Linux filesystem](https://learn.microsoft.com/en-us/windows/wsl/filesystems)
for Linux checkouts/builds (`~/src`, not `/mnt/c`). WSL may also host the
compatibility runtime without replacing the native Windows workflow.

## Contribution checks

From the native repository root, use `just` for fmt, tests and Clippy for each
touched package. For a changed file under `crates/`, find the containing package's
`Cargo.toml` and copy its `[package]` → `name` value into the `-p` argument.
Repeat the package commands for each affected package; the example below
uses `oxpinyin-core`. For documentation-only edits, check the changed text,
instructions/links and `git diff --check`; no Rust formatter, build, tests,
Clippy or backend selection is needed. Source-comment edits also run fmt.
For repository build/config changes, identify affected packages or ask the maintainer when
scope is unclear; CI owns the full workspace sweep:

```bash
just fmt --all --check
just clippy --locked -p oxpinyin-core --all-targets -- -D warnings
just heavy -- cargo test --locked -p oxpinyin-core
```

For a storage-dependent macOS package, use the explicit backend arguments:

```bash
just clippy --locked -p oxpinyin-store --all-targets --no-default-features --features bdb -- -D warnings
just heavy -- cargo test --locked -p oxpinyin-store --no-default-features --features bdb
```

`--no-default-features` turns off the default Berkeley DB selection;
`--features bdb` selects Berkeley DB explicitly (exactly one backend is allowed).
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
- Missing `db.h`, libclang or native libraries: repeat your OS prerequisite
  checks. The error identifies the missing header/library; ordinary users
  need the environment owner if package installation is restricted. Send
  that owner the exact failing command and complete error. On Linux,
  `pkg-config --modversion glib-2.0` checks GLib discovery; the setup check
  prints the compiler's Berkeley DB header version and bindgen's missing
  libclang diagnostic, so retain those lines too.
- macOS cannot find Berkeley DB headers/libraries: repeat both
  `OXPINYIN_BDB_INCLUDE_DIR` and `OXPINYIN_BDB_LIB_DIR` exports above. For optional
  tkrzw, repeat `LIBRARY_PATH` if lz4/lzma/zstd cannot link. Do not treat a
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
  Supported local BDB5.3/MSVC provisioning remains a gap; CI's BDB4.8
  exception is not a contributor default.
- Canonical Debian/Linux amd64 compatibility environment: BDB `same-data-dir-diff` and cache/
  environment-isolation resume passed on rootless Podman. Other runtime
  interoperability was not executed. Apple's volume-reader result retrieval
  is source-verified only, not runtime-tested.
- Native Linux arm64: Apple container, Debian 13 arm64, 4 CPUs, 16 GiB,
  ext4, no emulation. Two independent builds of each cell produced identical
  arm64 library hashes. `tools/cloud/setup.sh --in-container --cells
  "bdb kc tkrzw"` verified all three cells against the arm64 pin fields,
  regenerated the recorded data manifests, and replaced the host's broken
  libtkrzw with the healthy upstream build. The full gate then reported
  0 DIFFER on every cell: bdb 2,880 MATCH / 15 SKIP / 4 HOST-DEPENDENT, kc
  2,875 / 20 / 4, tkrzw 2,877 / 18 / 4. Each cell's SKIPs include the 13
  arm64-only #1137 exclusions; same-data was identical on 15/15 drivers.
  See [the arm64 oracle finding](../findings/oracle-arm64-1132.md).
- Canonical Debian/Linux amd64 compatibility environment (debian:trixie,
  glibc 2.41, GLib 2.84): the oracle and compat images rebuilt on trixie.
  Historical, at `8ba13c53` (preserved on wip/1229-pre-squash), before `0x.`
  was registered with `host_scanf`: `contract-diff` on 3,017 cases per cell
  reported bdb 2,988 MATCH / 15 POLICY-ACCEPTED / 10 HOST-DEPENDENT / 2 SKIP
  / 2 DIFFER, kc 2,983 / 15 / 10 / 7 / 2, tkrzw 2,985 / 15 / 10 / 5 / 2. The
  2 DIFFER are the `0x.` glibc-version host-dependence finding; the 10
  HOST-DEPENDENT are the exponent spellings. A targeted bdb rerun of the
  eight affected cases at the same revision returned the same 2 DIFFER and
  6 HOST-DEPENDENT. The current checker registers `0x.` with `host_scanf`,
  so on a host whose scanner accepts it those two cases report
  HOST-DEPENDENT when the pin loads and the subject refuses. The
  perf-matrix and callgrind images built on debian:testing and their smoke
  steps passed.
- Anonymous oracle artifact retrieval/hash verification passed. Anonymous
  `-deps` retrieval awaits normal publication. See the PR for final-head CI
  and the current cloud-provisioning/compatibility smoke status.
