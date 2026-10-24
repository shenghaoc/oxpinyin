# tkrzw concrete-class shim: uncompiled prototype (#1125)

Recorded 2026-10-24T04:04:52Z. Branch `wip/1125-tkrzw-shim`, based directly on
`wip/1114-tkrzw-record-copy` at `838dabc2fb86c9b221ba3bde914584596de075da`.
This is an unfinished, uncompiled prototype for cloud correctness work.
No performance or compatibility result exists for this prototype yet.
The original local prototype `af357bdef8b993169f829a52583a7a7fe367db0e`
is preserved under `local/1125-unvalidated-af357bde`.

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
