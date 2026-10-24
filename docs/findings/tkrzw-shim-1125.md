# tkrzw concrete-class shim: correctness work (#1125)

Recorded 2026-10-24T04:04:52Z. Branch `wip/1125-tkrzw-shim`, based directly on
`wip/1114-tkrzw-record-copy` at `838dabc2fb86c9b221ba3bde914584596de075da`.
Source checkpoint `58566b3c56c738981bdf5a199e954eb9598a0651` is an unfinished
prototype. The cloud checks below supersede its original uncompiled status.
No performance result exists for this prototype.
The original local prototype `af357bdef8b993169f829a52583a7a7fe367db0e`
is preserved under `local/1125-unvalidated-af357bde`.

## Cloud status and policy STOP

Recorded 2026-10-24T04:16:00Z in the fresh Codex Debian 13 x86-64 session.
The source checkpoint was fetched from the authorized branch and checked
to have exactly `838dabc2` as parent, with Shenghao Chen as both author
and committer. Work uses `/tmp/oxpinyin-1125`; the shared checkout stays
on its original branch. No checkout `.agents/skills` directory exists.

Function-order regeneration is **blocked, not passed**. The required
`docs/packaging.md` Regeneration workflow uses ext4, teardown-profile
provisioning and startup/steady profiling through the teardown boundary.
The current instruction holds those campaigns; this host also reports
tmpfs for the worktree/TMPDIR and overlay for `/workspace`. No generator,
ordered shipping relink or layout-resolution claim has been made. No
unordered shipping fallback was used. The existing tkrzw policies remain
stale. Final regenerated policies must be folded into the prototype code
commit once that required workflow is authorized and available.

Native completion checks at `58566b3c` passed:

- `cargo fmt --all --check`, `git diff --check`, and
  `bash -n tools/packaging/relink-versioned.sh`.
- Through `just heavy`, `cargo clippy --locked -p oxpinyin-store
  --all-targets -- -D warnings` and `cargo test --locked -p oxpinyin-store`:
  57 tests passed; four existing real-data BDB tests ignored.
- The same clippy and test commands with `--no-default-features --features
  tkrzw`: 59 tests passed. C++ compilation emits upstream header unused
  parameter warnings; no warning suppression was introduced. Bindgen and
  linking succeeded. Before execution, the three test executables were
  checked through `bash tools/tkrzw/runtime.sh --print-path tkrzw 1125
  subject OBJECT` against setup's certified library.
- `uv run --no-project --managed-python --python cpython@3.15+gil python
  -B -X lazy_imports=normal tools/bisection/contract-diff.test.py
  TkrzwSavedBigramTests`: six passed. The comparator and its mutation
  assertions are unchanged from #1114.

Native targets are `/workspace/oxpinyin-1125-targets/bdb` and `.../tkrzw`.
Each shell sources the existing native activation, unsets its fixed
`CARGO_BUILD_JOBS`, `RUST_TEST_THREADS` and `PKG_CONFIG_SYSROOT_DIR`, and
selects the certified tkrzw `PKG_CONFIG_PATH` and `LD_LIBRARY_PATH`.
TMPDIR is `/tmp/oxpinyin-1125-tmp`. The writable host-wide slot directory
is `/workspace/oxpinyin-env/locks`; the default under the read-only home
failed before starting a process. `just` 1.59.0 was installed through
the same Python slot implementation to bootstrap `just heavy`.
Rust 1.97.1 was already present. Setup provisions checksum-verified uv
0.13.1 and managed CPython 3.15.0 with the GIL in task-owned paths.
TLS verification stays enabled; `OXPINYIN_PROXY_CA` names the existing
session CA bundle `/etc/ssl/certs/ca-certificates.crt`.

The explicit certified-runtime probe passed: 18 comparator GOT
relocations, hash/tree/skip create/read, removal, comparator type 1,
`RESULT : healthy`. Both pin facades resolve through `runtime.sh` to
`/workspace/oxpinyin-env/cloud-cache/tkrzw/lib/libtkrzw.so.1.77.0`.

An isolated C++ boundary unit check compiled the actual `shim.cc` from
stdin with `just heavy -- c++ -std=c++17 -I<CERTIFIED>/include
-I<STORE>/src/tkrzw -x c++ - -L<CERTIFIED>/lib -ltkrzw
-o /tmp/oxpinyin-1125-shim-exceptions`, then launched it through
`runtime.sh`. It installed a throwing record processor on a TinyDBM:
`std::runtime_error`, `throw 42`, and a `std::exception` with a long
`what()` message after making global `operator new` throw `bad_alloc`.
All three calls returned false and SYSTEM_ERROR. The final error message
was empty when recording it could not allocate. Allocation was restored,
and Count and Close succeeded on that handle. Result: PASS for all three
containment cases. This checks the boundary's source-derived failure
contract; it does not run the pin or measure performance.

Static comparison with the parent also confirmed unchanged bodies for
load_db, save_db, db_set_one, holds_no_record, ends_before_records,
record_base and the collision-chain regression. db_set_record changes
only the FFI callee, retaining overwrite=true. No health/record-section
guard or comparator assertion was weakened.

After fetching current `origin/main` (`06195c1a`), rule 2 path checks give:

```text
git diff --name-only origin/main...HEAD -- crates/oxpinyin-store/
crates/oxpinyin-store/Cargo.toml
crates/oxpinyin-store/build.rs
crates/oxpinyin-store/src/tkrzw/ffi.rs
crates/oxpinyin-store/src/tkrzw/mod.rs
crates/oxpinyin-store/src/tkrzw/shim.cc
crates/oxpinyin-store/src/tkrzw/shim.h

git diff --name-only origin/main...HEAD -- ':!*.md' | xargs -r grep -lE 'feature *= *"(bdb|tkrzw|kyotocabinet)"'
crates/oxpinyin-store/build.rs

git diff -U0 origin/main...HEAD -- '*Cargo.toml' | grep -E '^[+-][^+-].*(bdb|tkrzw|kyotocabinet)'
+# Compiles the Kyoto Cabinet and tkrzw C++ shims (#1118, #1125).
```

Checks 1 and 2 select bdb, kc and tkrzw. Check 3 prints a comment;
its existing regex does not match the tkrzw feature line itself.
Contracts, same-data, cross-load saved bytes and revert/mutation gates
are pending. No green completion gate or adoption conclusion is claimed.

## Source inspection

[Issue #1125](https://github.com/shenghaoc/oxpinyin/issues/1125) is open.
[KC shim #1121](https://github.com/shenghaoc/oxpinyin/pull/1121) is merged
as `6755fc5a257cf68da58f3743e450aa02cad212b9`.
[Issue #1114](https://github.com/shenghaoc/oxpinyin/issues/1114) remains
open; its exact checkpoint above is the parent of this prototype.

The pin's source was read with `git show 074a2219:<path>` from
`/home/sheng/repos/libpinyin`, whose checked-out HEAD is not the pin.

- `src/storage/ngram_tkrzwdb.cpp:48-80`: default TinyDBM without Open;
  load from HashDBM with OPEN_NO_CREATE; save to a new default HashDBM,
  copy in native iteration order and soft-sync before closing.
- `src/storage/tkrzwdb_utils.h`: attach_options maps read/write/create
  flags; copy_tkrzwdb walks source records and calls destination Set.
- `src/storage/chewing_large_table2_tkrzwdb.cpp:87-126` and
  `src/storage/phrase_large_table3_tkrzwdb.cpp:77-117`: default TreeDBM
  for files and default BabyDBM for the in-memory container.
- `crates/oxpinyin-store/src/kyotocabinet/{shim.h,shim.cc}`, its build.rs
  module, and `docs/findings/kc-tkrzw-pss-1098.md` (libtkrzw) were read.
- tkrzw 9ee8416's [C callback adapter](https://raw.githubusercontent.com/estraier/tkrzw/9ee8416/tkrzw_langc.cc)
  and [PolyDBM implementation](https://raw.githubusercontent.com/estraier/tkrzw/9ee8416/tkrzw_dbm_poly.cc)
  were read. The latter skips Open for empty-path memory containers and
  delegates record operations to their concrete DBM.

Cloud source verification used separate local checkouts exactly at
libpinyin `074a2219c90feaf962d0d24f034514033ece5f99` and tkrzw
`9ee84164cf45e441aa8186418c03a02c9b89f08b`. The installed five DBM
headers compare byte-identical to that tkrzw source. HashDBM and TreeDBM
Open delegate to OpenAdvanced with default TuningParameters
(`tkrzw_dbm_hash.h:356-375`, `tkrzw_dbm_tree.h:259-278`); their default
constructors use MemoryMapParallelFile (`tkrzw_dbm_hash.cc:2401-2403`,
`tkrzw_dbm_tree.cc:2558-2560`). TinyDBM's default -1 uses
DEFAULT_NUM_BUCKETS (`tkrzw_dbm_tiny.h:155`, `.cc:208-218`), with no Open
on the pin's memory container. BabyDBM defaults to LexicalKeyComparator
(`tkrzw_dbm_baby.h:149`), also without Open. The pin calls no alternative
tuning on these paths. The shim can reproduce all four classes' flags
and tuning. Its Set retains the C++ default overwrite=true
(`tkrzw_dbm.h:1123-1132`). These are source conclusions, independent of
the pending differential gates.

## Local changes

`crates/oxpinyin-store/src/tkrzw/shim.h` and `shim.cc` implement 18 C
entry points over concrete HashDBM, TreeDBM, TinyDBM and BabyDBM objects.
The boundary contains C++ exceptions and returns failure; error text is
best effort. The shim supplies status, callback tags, point processing,
ProcessMulti, Set, count, health, synchronize, rebuild and iterators.
The loading path retains #1114's db_set_record and calls the shim Set
entry point with overwrite=true. No single-record ProcessMulti batch is
introduced into load_db. #1114's collision-chain and value tests remain.

The tkrzw feature enables the already locked cc dependency. build.rs
compiles C++17 at O2 with hidden visibility and generates only shim C
bindings. The shipping relink includes libstdc++. No cxx dependency or
Cargo.lock change was introduced.

Static text comparison against the #1114 base confirmed unchanged bodies
for TkrzwUserBigramDb's load_db and save_db, db_set_one, holds_no_record,
ends_before_records and record_base. db_set_record changes only the FFI
callee to oxtkrzw_dbm_set, retaining the overwrite=true argument. is_healthy
still calls the backend health operation through the shim. Existing guard
and collision tests were retained.

## Commands and checks

Working directory:
`/home/sheng/Documents/Codex/2026-10-24/task-3/worktree`.
Home-disk TMPDIR:
`/home/sheng/Documents/Codex/2026-10-24/task-3/tmp`.

```sh
git -C /home/sheng/repos/oxpinyin fetch origin main
git -C /home/sheng/repos/oxpinyin worktree add -b wip/1125-tkrzw-shim \
  /home/sheng/Documents/Codex/2026-10-24/task-3/worktree origin/main
export TMPDIR=/home/sheng/Documents/Codex/2026-10-24/task-3/tmp
cargo fmt --all
cargo fmt --all --check
git diff --check
bash -n tools/packaging/relink-versioned.sh
```

At the original local prototype, the final three checks exited 0. An initial fmt check reported formatting
differences; cargo fmt corrected them before the passing check. These are
light checks, not a completion gate.

## Pending execution

Provisioning, compilation, oracle cases, function-order regeneration and
measurement were not started on Ubuntu. Ubuntu heavy admission remains
held for #1114, then #1031, then #1073. The authorized next execution phase
is correctness in a fresh cloud session; measurements remain held. No slot
lock or slot-state polling was introduced.
The checked-in tkrzw policies have not been regenerated for this prototype.

Contracts and same-data on bdb, kc and tkrzw, saved-byte compatibility in
both directions, and all base/prototype/pin measurements remain unrun.
The saved-byte mask remains limited to HashDBM META_OFFSET_TIMESTAMP
zero-based bytes 48–55 inclusive; byte 56 and every other byte must match.
Strict bucket-chain and saved-file checks remain required.

The measurement requirements remain one run with base, prototype and pin,
certified libtkrzw on every build and launch, resident libtkrzw and facade
text, paired-control total Pss at M1, exact-control init and store-operation
Ir, and stripped size. Build in the compatibility image if the host linker
lacks --section-ordering-file. Target and stage paths must be distinct,
and measured binaries must be verified to contain the actual prototype.
A time-for-residency trade remains a STOP. No adoption conclusion is made.
The final report must stop for the maintainer's decision without a PR.
