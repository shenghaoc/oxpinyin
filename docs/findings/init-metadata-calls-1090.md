# Init metadata calls: one fstat per chunk open, no DBM sidecar unlinks (2026-10-23 UTC)

#1090. Items 1 and 2 of the init contributors in
`init-gap-1090-2026-10-23.md` (#1101, "Contributors"). Branch:
`wip/1090-fstat-sidecars` on main `cb645dfa`: the code change, then the
regenerated policies, then this record. The gates ran on `eac1b473`, whose
crates equal the branch's except two tests the gates do not build
(`oxpinyin-runtime`'s `empty_user_dir`, `oxpinyin-store`'s `trait_laws`).
The policies were generated from `e1203aad`. It differs from the branch's
crates only in the store's `write_user_bigram` save path and in those
tests, and no generator workload runs any of them (below). Main's commits since base `6231baf4` touch only
`docs/` and `tools/bisection/`.

## Changes

- `oxpinyin_data::phrase_library::map_chunk` probed each chunk with
  `Path::is_dir` before opening it. It now opens first and takes the
  directory check and the size from one `File::metadata` (`fstat`) of the
  open file. A directory still reaches `directory_chunk_fault`. A failed
  open reports the same `LibraryError::Io` as before.
- `oxpinyin_user::persistence::remove_dbm_sidecars` and its calls are
  removed. They unlinked `<file>-lock` and `<file>-shm` beside
  `user_bigram.db`, `user_phrase_index.bin` and `user_pinyin_index.bin`
  after each load and save. The pin unlinks nothing there.
- `oxpinyin_store`'s default `write_user_bigram` loses the same unlinks
  beside its temporary after the rename, and the comment that said tkrzw
  creates `<file>-lock`. BDB and tkrzw ran it at every save
  (`persistence::stage_user_bigram`); KC overrides the method, so the KC
  libraries are byte-identical with or without this part.
- `oxpinyin-runtime`'s `empty_user_dir` test planted `user_bigram.db-lock`
  and `-shm` and expected the reopen to delete them. It now expects them
  to remain. The pin's init unlinks only the profile's own files, and only
  when `check_format` finds an old format (`pinyin.cpp:200-215`).
- `oxpinyin-store`'s `trait_laws` test said that a writable tkrzw session
  leaves `<file>-lock`. Its comments now say that no backend leaves a file
  beside the store, and its guard no longer removes one. The store's own
  test-helper comments in `lib.rs` made the same claim. They now say that
  the helpers remove any stale `<file>-lock` and that no current backend
  creates one.
- The six function-order policies are regenerated, in a commit after the
  code change. Each `init` tier loses `remove_dbm_sidecars`,
  `std::sys::fs::remove_file`, and the path, allocation and formatting
  helpers that only those unlinks reached in init. Nothing is added or
  reordered. The generator's startup toggles stop at the first candidate
  and its steady toggle is `bisect_perf_steady_unit`; neither saves, so
  the save-path change cannot alter a policy.

## Why the sidecar unlinks existed

`44f99645` (2026-09-11) added `DBM_SIDECAR_SUFFIXES = ["-lock", "-shm"]`.
Its comment reads: "Some backends keep a lock sidecar beside the database
(LMDB writes `<path>-lock`) … without this the user dir would accumulate
one orphan per save". The LMDB backend (`5fd0f681`, 2026-08-23) opened with
heed's `EnvFlags::NO_SUB_DIR`, which keeps `<file>-lock` beside the data
file. `1205080f` (2026-09-20) removed that backend. No store backend in
the history writes a `-shm` file; SQLite appears only in the training
tools.

No current backend creates either name:

- Berkeley DB: `db_create` with a null environment (`oxpinyin-store`
  `bdb::ffi`, the pin's use). Region files exist only in an environment
  home.
- Kyoto Cabinet: its companion file is the transaction log `<file>.wal`
  (`perf-baseline-kc-validation-2026-08-31.md`, the `.kct.wal` row).
  `libkyotocabinet.so.16.14.0` (1.2.80-2+b2, the compatibility image)
  contains neither string.
- tkrzw: locks the data file itself with `fcntl` (`tkrzw_file_mmap.cc:124-133`
  and `:691-700`, `tkrzw_file_pos.cc:160-165` and `:856-861`). Read from
  the tkrzw tree at `9ee8416` (1.0.34), the REV of the certified build.

All six base unlinks in the init window return `ENOENT` on BDB and KC
(#1101 table; KC again below). The store comment that said tkrzw creates
`<file>-lock` for a writable session is contradicted by the tkrzw source
above.

## What remains

After the change, init makes 27 `statx` (zhuyin) and 30 (pinyin) on KC:
14 path probes and 16 descriptor calls for pinyin. The zhuyin facade makes
no addon-index or punct probe, so it has 11 path probes.

| calls | count (pinyin) | why kept |
| --- | ---: | --- |
| `fstat` of an open descriptor | 16 | `std::fs::read` and `File::metadata` for chunk sizes |
| path type checks before opening a file the facade opens itself | 8 | `table.conf` (1), the four system chunks in `PhraseLibraries::open` (4), the addon indexes (2), `punct.bin` (1). They refuse a FIFO, because `File::open` without `O_NONBLOCK` blocks on one. Opening first needs `O_NONBLOCK`, so it needs `libc` or per-platform constants. |
| probes before files a DBM backend opens by path | 6 | `require_file` for the three system DBM files, `Path::exists` for the three user ones. The backend opens these files by path, so no descriptor of ours exists to `fstat`. |

## Results

Gates on `eac1b473`'s code, 15:28Z–16:07Z: `contract-diff` PASS on bdb, kc
and tkrzw (2,889 cases each), and `same-data-dir-diff` PASS on all three.
The tkrzw cell's subject and oracle each resolve one `libtkrzw.so.1`, the
certified 1.0.34 build.

KC cell, compatibility-image builds, ordered with each tree's own
policies. The PR libraries were built from `eac1b473`. Exact counts come
from one job on one host, 15:48:41Z–15:51:20Z: three Ir rounds, then three
syscall rounds, each with base, PR, control and pin. Every side gave one
value in all three rounds, and control (the PR's bytes) equals PR in every
cell: VALID. An earlier job on the `e1203aad` build gave the same figures;
its KC libraries are byte-identical, because KC overrides
`write_user_bigram`.

Init Ir (`<facade>_init` toggle, constructors excluded):

| facade | profile | base | PR | pin | PR − base | PR/base | base/pin | PR/pin |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| zhuyin | phrases | 11,749,065 | 11,734,030 | 12,143,205 | −15,035 | 0.99872 | 0.9675 | 0.9663 |
| zhuyin | journals | 11,771,475 | 11,756,160 | 12,214,396 | −15,315 | 0.99870 | 0.9637 | 0.9625 |
| pinyin | phrases | 15,233,525 | 15,218,405 | 15,622,714 | −15,120 | 0.99901 | 0.9751 | 0.9741 |
| pinyin | journals | 15,263,302 | 15,247,898 | 15,699,501 | −15,404 | 0.99899 | 0.9722 | 0.9712 |

zhuyin phrases splits the drop as −7,732 Ir in the facade and −7,291 in
libc. `map_chunk` inclusive drops by 2,293 Ir over its seven calls.

Syscalls between the init markers (strace, three rounds, identical):

| facade | profile | `statx` base / PR / pin | `unlink` base / PR / pin | all calls base / PR / pin |
| --- | --- | --- | --- | --- |
| zhuyin | phrases | 34 / 27 / 0 | 6 (6 `ENOENT`) / 0 / 0 | 151 / 138 / 130 |
| zhuyin | journals | 34 / 27 / 0 | 6 (6 `ENOENT`) / 0 / 0 | 151 / 138 / 134 |
| pinyin | phrases | 37 / 30 / 0 | 6 (6 `ENOENT`) / 0 / 0 | 194 / 181 / 170 |
| pinyin | journals | 37 / 30 / 0 | 6 (6 `ENOENT`) / 0 / 0 | 193 / 180 / 174 |

The removed `statx` are the `is_dir` probes of the four system chunks and
the three user `.bin` chunks.

Stripped shipping libraries, base / PR, in bytes, ordered builds of main
and of the branch's code and policies: bdb libpinyin 2,441,960 /
2,437,864, libzhuyin 2,200,296 / 2,200,296; kc libpinyin 2,482,912 /
2,482,912, libzhuyin 2,233,056 / 2,233,056; tkrzw libpinyin 2,487,008 /
2,487,008, libzhuyin 2,245,344 / 2,241,248.

## Commands

```sh
tools/cloud/setup.sh --cells "bdb kc tkrzw"
source ~/.oxpinyin-oracle.env
# Builds: inside oxpinyin-compat:ddd2c1a8e182d4d3 (GNU ld 2.47, cargo-c
# 0.10.24), from a git archive of each tree, one target directory per tree
# and backend, under `python -m tools.dev slot run --`:
tools/packaging/install.sh libpinyin --prefix="$STAGE" -- --locked --release \
  --no-default-features --features "$FEATURE,shipped"
tools/packaging/install.sh libzhuyin --prefix="$STAGE" -- --locked --release \
  --no-default-features --features "$FEATURE"
# Policies: docs/packaging.md "Regeneration" per backend, from the PR tree,
# with OXPINYIN_FUNCTION_ORDER_POLICY= builds, the teardown `provision`
# profiles of each cell, --cpu 3. Two runs into separate directories,
# cmp on both facade files.
# Gates (all three cells; the diff touches crates/oxpinyin-store/):
tools/bisection/run-all.py --runners contract-diff,same-data-dir-diff \
  --cells bdb,kc,tkrzw --oracle bdb=$OXPINYIN_ORACLE_PREFIX_BDB \
  --oracle kc=$OXPINYIN_ORACLE_PREFIX_KC --oracle tkrzw=$OXPINYIN_ORACLE_PREFIX_TKRZW
```

Init Ir used `tools/generate-function-order.py`'s `callgrind_prefix`
(toggle `<facade>_init`) on the in-tree worker's teardown `time` mode,
parsed in memory by `contract-diff.py`'s `per_call_profile`. Each side ran
from an equal-length copy of its library, on the KC `provision` profiles.
The syscall counts used the `trace` probe of the teardown `init` mode
(#1111) for the same libraries and profiles. The profiler streams were
parsed in memory and not kept.
