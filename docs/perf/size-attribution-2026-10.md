# Shipping-library size attribution and candidate inventory

Recorded 2026-10-23 UTC from native Ubuntu BDB release attribution for
[issue #1084](https://github.com/shenghaoc/oxpinyin/issues/1084), phases 1
(attribute) and 2 (candidates). The stripped regular files are
**2,441,968 B for libpinyin** and **2,200,304 B for libzhuyin**. Most bytes
are `.text`; user-store definitions and generic sort/drop/container
families are the principal retained-code leads. The thirty-sort audit
identifies a conditional 17,077 B Q/D sharing estimate with net savings
unmeasured. No candidate was implemented.

The follow-up resolves actual object references into std's symbolizer from
both panic and allocation failure. The four named component definition
buckets total 78,742/78,732 B; their broader tagged support union is
176,090/176,080 B, not a removable-byte claim. The disjoint core/alloc/std
split and per-component stable-toolchain decisions are below.

The findings contain the requested sections, crate attribution, largest
thirty symbols per library, top twenty generic families per library, and
all six candidate rows. Commands C1–C6 below produce the measured byte/count
figures; the table values are decimal bytes. Family footprints and gross
ceilings are not measured savings.

## Maintainer ruling: symbolizer removal closed

Maintainer ruling, typed 2026-10-23 11:37 UTC: **backtrace/symbolizer code
is not removable on the supported stable toolchain; perform no further
work on its removal**. This supersedes the earlier open questions about
changing both failure roots or supplying a custom std. The completed
reference evidence below remains a historical record, not a pending
removal investigation. The later stable-sort build and thirty-pair census do not investigate
symbolizer removal; its five sort pairs remain retained and closed.

## Policy and measured source

The [2026-10-18 UTC amendment](ci-perf-size-gate-proposal-2026-09-09.md#amended-2026-10-18-utc-binary-size-recording)
retired the pin +10% file-size cap. No CI gate returns. The pin remains
the behaviour and ABI oracle; a standalone Rust DSO that includes static
`std`/`alloc` is not the same accounting boundary as the pin plus its
shared C++ dependencies. No cross-run pin ratio or historical issue size
is used as this campaign's baseline.

Build source: `9ce07663a7fa52cb0db23f733e6b0b9dca26b7a6`, the findings-only
checkpoint rebased onto main `747b00cd1de4889f093c6b50ceb5161b7d7619a3`
after #1096. `git diff --name-only 747b00cd..9ce07663 -- ':!*.md'`
was empty. The BDB policies came from this same source and were selected
by the unchanged relinker under `tools/packaging/function-order/bdb/`.
Their contents matched the pre-move BDB files at `1cdb810c` with `cmp`.
No ordering opt-out or foreign-backend policy was used.

Policy blob identities, from `git rev-parse '9ce07663:PATH'`:

- `tools/packaging/function-order/bdb/libpinyin.order`:
  `8a942c135ce1153a0e63fe7c67c68493fd1ce61a`.
- `tools/packaging/function-order/bdb/libzhuyin.order`:
  `abb186241653878585a880d79172d0c3c4b640b2`.

The release build started at `2026-10-23T10:22:03Z`; attribution completed
at `2026-10-23T10:24:14Z`. These timestamps were captured by
`date -u +%Y-%m-%dT%H:%M:%SZ`. Later table rendering only consolidated the
facade crate labels; it used the same objects without another build.

## Host, recipe and same-build identity

Native Ubuntu 26.10, Linux `7.3.0-9-generic`, x86_64,
Intel Core i7-9750H. Worktree `/home/sheng/wt/1084` and
`TMPDIR=/home/sheng/wt/1084/target/tmp/1084` were on home-disk ext4,
`/dev/nvme0n1p2`, confirmed with `findmnt -T`. The shared checkout was
left untouched. Host identity commands: `uname -srmo`, the `PRETTY_NAME`
entry in `/etc/os-release`, and the first `model name` in `/proc/cpuinfo`.

Existing tools, queried with `rustc -vV`, `cargo cinstall --version`,
`cc --version`, `ld --version`, `strip --version`, `nm --version`,
`uv --version` and `just --version`:

| Tool | Version |
| --- | --- |
| repository Rust | 1.97.1, `8bab26f4f68e0e26f0bb7960be334d5b520ea452`, LLVM 22.1.6 |
| cargo-c | 0.10.24+cargo-0.98.0, existing `/home/sheng/.cache/oxpinyin/fo-1018/cargo-c/bin` |
| cc | Ubuntu 15.3.0-4ubuntu1 |
| GNU BFD ld / strip / nm | Ubuntu binutils 2.47 |
| uv | 0.13.1 |
| just | 1.59.0 |

`rust-toolchain.toml` was honored; `Cargo.toml` retained fat LTO, one
codegen unit and default release unwind strategy. No profile, panic,
linker flag, version script or dependency changed. `RUSTFLAGS`,
`CARGO_ENCODED_RUSTFLAGS`, `CC`, `LD`, `CARGO_BUILD_TARGET`,
`OXPINYIN_FUNCTION_ORDER_POLICY`, and caller job/thread overrides were
unset. `CARGO_NET_OFFLINE=true` used the existing dependency cache.
Normal `just heavy` admission selected three Cargo jobs and twelve test
threads for the build; no test workload was run. The campaign and later
same-object analysis used normal shared admission, with no lock bypass
or process termination.

`release-stage.sh` built pinyin with `--release --locked
--no-default-features --features bdb,shipped` and zhuyin with
`--release --locked --no-default-features --features bdb`, through
`install.sh` and `relink-versioned.sh`. Its package metadata, SONAME,
export and C smoke gates passed. The expected smoke calls returned null
for the empty data path and printed the missing-table diagnostics.

A private `strip` forwarding shim copied each exact versioned object
immediately before the release recipe stripped it, then delegated the
unchanged arguments to `/usr/bin/strip`. There was no second relink or
build to obtain symbols. Re-stripping each preserved copy with
`--strip-unneeded` and comparing it to the staged file passed `cmp`.
`file` reported **stripped** for both shipping files, and **not stripped**
for their symbol copies. Both members of each pair had the same build ID.

The existing linked-object check reported **ordered**, backend **bdb**,
for both preserved objects: pinyin had 265 policy paths at `.text` start
`0x10060`; zhuyin had 249 at `0xe060` (C2). The exact defined dynamic
export sets matched their version scripts: 79 and 52 ABI functions,
respectively, with the proper default version; the version-node carrier
is counted separately from the ABI functions (C4). Imported dynamic
symbols are not leaked exports.

Each stripped regular file is counted once. Model data, symlinks, headers,
`.pc`, `.a`, debug packages and `DT_NEEDED` libraries are excluded. Both
objects needed `libglib-2.0.so.0`, `libdb-5.3.so`, `libgcc_s.so.1`,
`libc.so.6` and `ld-linux-x86-64.so.2` (`readelf -dW`, C3). No runtime
speed, residency or pin workload was measured.

Operational objects and scripts were private scratch and are removed at
lane end. No archive, log, capture or evidence file is committed,
attached, uploaded or retained for sharing. The paths below describe
reproduction, not retained downloadable evidence.

## How to read the attribution

The `.text` accounting uses declared function intervals from the
unstripped versioned objects, checked against the stripped `.text`
address and size. Exact aliases are deduplicated, overlaps are rejected,
and gaps remain explicit. GNU nm's `--size-sort` inferred lengths for
four zero-size CRT records in each library; those inferred lengths are
excluded rather than presenting alignment gaps as declared function
bytes. C4 checks that these account for the difference between the
size-sorted and unsorted interval sets.

Crate ownership is the first crate-qualified path of the surviving
symbol definition, with ABI names and qualified facade paths assigned
to their facade crate. Inlining after fat LTO prevents reconstruction of
original pre-inlining source contribution from `nm`. In particular,
`core::ptr::drop_glue::<oxpinyin_...>` belongs to the `core` definition
bucket while still doing work for an oxpinyin type. The tables do not
claim that every generic instance is duplicate code.

The two same-type `seed_txn_with_replay<SessionTxn>` bodies are 26,409 B
and 17,607 B in each DSO, distinguished by their instantiating crates
(runtime and facade). Their combined 44,016 B makes them a concrete
source-audit lead; their different sizes are not proof that the bodies
are interchangeable. Stable quicksort totals 84,529/76,308 B,
`drop_glue` 79,857/76,159 B, and BTreeMap insert 50,420 B per DSO (C4).

Directly named `read_with` bodies total only 596/603 B (C4). This does
not isolate the historical increase reported in #1084: generic work can
be inlined into callers, and no before/after ablation ran here. Required
error paths also use formatting. `Result` alone proves neither that
formatting is unreachable nor that panic support can be discarded.

The largest named table, `ZHUYIN_PINYIN_MAP`, is already a 17,916 B
integer-span array; `CONTENT_STRINGS` is 10,584 B and `CONTENT_KEYS`
7,056 B per library (C4). Current source already pools strings with
`StringPool` and uses `StringSpan`/`StringTable`; the September report's
pointer-heavy original tables are now test-only references. No saving
from that already-landed representation change is claimed here.

## Measured file and section sizes

All sizes below are bytes. S = stripped regular files; U = the exact same-build,
versioned objects preserved immediately before stripping. Commands C1–C4 below
produced these tables. Absent means no such ELF section, not an unmeasured zero.

| Quantity | libpinyin.so.15.0.0 | libzhuyin.so.15.0.0 |
| --- | --- | --- |
| stripped file (stat) | 2441968 | 2200304 |
| unstripped operational object (stat) | 5141136 | 4867976 |
| .text | 1971958 | 1772278 |
| .rodata | 151528 | 147672 |
| .data.rel.ro | 48504 | 39800 |
| .eh_frame | 130096 | 115712 |
| .eh_frame_hdr | 14716 | 13372 |
| .gcc_except_table | 51088 | 45416 |
| .dynsym | 3408 | 2760 |
| .dynstr | 3113 | 2264 |
| .gnu.version | 284 | 230 |
| .gnu.version_d | 56 | 56 |
| .gnu.version_r | 272 | 272 |
| .gnu.hash | 668 | 440 |
| .hash | absent | absent |
| .rela.dyn | 56424 | 46584 |
| .rela.plt | 48 | 48 |
| unwind metadata total | 195900 | 174500 |
| dynamic symbol metadata total | 7801 | 6022 |

## Retained `.text` attribution by crate

C4 groups non-overlapping function intervals, deduplicating exact aliases.
The first crate-qualified path in the demangled definition owns a body; for
qualified impls this is the receiver crate, or the trait crate for primitive
receivers. Unmangled ABI exports map to their facade. This counts surviving
bodies after fat LTO, including inlined callees, not original source bytes.

| Definition crate/group | pinyin bytes | zhuyin bytes |
| --- | --- | --- |
| oxpinyin_user | 636008 | 567686 |
| core | 322796 | 298869 |
| alloc | 182884 | 175918 |
| oxpinyin_engine | 151945 | 155232 |
| oxpinyin_runtime | 124246 | 117950 |
| oxpinyin-capi (pinyin) | 189992 | 0 |
| std | 76366 | 76090 |
| oxpinyin-zhuyin-capi (zhuyin) | 0 | 136080 |
| oxpinyin_data | 67549 | 61600 |
| gimli | 36052 | 36052 |
| oxpinyin_core | 29941 | 25679 |
| oxpinyin_store | 24721 | 24149 |
| hashbrown | 26038 | 21386 |
| oxpinyin_facade | 32825 | 7604 |
| rustc_demangle | 19771 | 19771 |
| addr2line | 13312 | 13312 |
| miniz_oxide | 9607 | 9597 |
| smallvec | 4345 | 4345 |
| C/CRT/unknown | 2676 | 2676 |
| compact_str | 2394 | 2394 |
| oxpinyin_chewing | 1910 | 614 |
| phf | 901 | 901 |
| memchr | 840 | 840 |
| __rustc | 745 | 745 |
| compiler_builtins (C/asm names) | 387 | 387 |
| memmap2 | 141 | 141 |
| panic_unwind | 22 | 22 |
| named body coverage | 1958414 | 1760040 |
| .text gaps/padding/unnamed bytes | 13544 | 12238 |
| .text total | 1971958 | 1772278 |

| Reconciliation | pinyin | zhuyin |
| --- | --- | --- |
| unique nonzero body intervals | 1836 | 1668 |
| duplicate alias records not counted twice | 0 | 0 |
| nm size-sort inferred CRT intervals excluded | 4 | 4 |
| partial interval overlaps | 0 | 0 |
| defined ABI exports (version-node carrier excluded) | 79 | 52 |

## Largest thirty `.text` bodies per library

C4 sorts unique intervals by size descending, then address. Names retain
generic arguments; the existing packaging helper adds a validated v0
instantiating-crate annotation where GNU nm omits it. Hash suffixes are
canonicalized by that helper. Aliases, if present, share one row.

### libpinyin

| Rank | Bytes | Demangled symbol |
| --- | --- | --- |
| 1 | 47116 | `<oxpinyin_user::lookup::UserLookup>::from_store` |
| 2 | 37408 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::mask_out::{closure#0} [instantiating crate: oxpinyin_facade]` |
| 3 | 32319 | `oxpinyin_user::persistence::load_logs_with_mask` |
| 4 | 31243 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::open_session::<oxpinyin_user::persistence::SystemOriginals> [instantiating crate: oxpinyin_runtime]` |
| 5 | 26409 | `oxpinyin_user::store_libpinyin::seed_txn_with_replay::<oxpinyin_user::session_values::SessionTxn> [instantiating crate: oxpinyin_runtime]` |
| 6 | 25573 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::replace_system_libraries [instantiating crate: oxpinyin_facade]` |
| 7 | 25356 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::save_reporting_inner::<<oxpinyin_facade::context::ContextCore>::save_user_with_layout_faults::{closure#0}>` |
| 8 | 22656 | `<oxpinyin_runtime::RuntimeDict>::user_trellis_index` |
| 9 | 20690 | `<oxpinyin_runtime::Runtime>::open_with_law` |
| 10 | 19727 | `pinyin::predict::guess_predicted_result` |
| 11 | 18280 | `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` |
| 12 | 18049 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::replace_system_libraries::{closure#1} [instantiating crate: oxpinyin_facade]` |
| 13 | 18035 | `oxpinyin_engine::nbest::nbest_sentences_with_seeds_generation::<oxpinyin_runtime::RuntimeDict, oxpinyin_runtime::RuntimeLm> [instantiating crate: pinyin]` |
| 14 | 17607 | `oxpinyin_user::store_libpinyin::seed_txn_with_replay::<oxpinyin_user::session_values::SessionTxn> [instantiating crate: oxpinyin_facade]` |
| 15 | 16146 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::mask_out [instantiating crate: oxpinyin_facade]` |
| 16 | 13828 | `<oxpinyin_engine::session::Session<oxpinyin_runtime::RuntimeDict, oxpinyin_runtime::RuntimeLm>>::train_spans::<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>> [instantiating crate: oxpinyin_facade]` |
| 17 | 11995 | `<std::backtrace_rs::symbolize::gimli::Context>::new` |
| 18 | 11236 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::remove_user_phrase [instantiating crate: pinyin]` |
| 19 | 11091 | `pinyin_train` |
| 20 | 10512 | `oxpinyin_user::persistence::load_libraries` |
| 21 | 10221 | `<oxpinyin_runtime::RuntimeDict as oxpinyin_core::Dictionary>::checked_extension_tokens` |
| 22 | 9905 | `pinyin_phrase_segment` |
| 23 | 9843 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::train_pronunciations::<&mut oxpinyin_engine::session::selection::TrainingReadings>::{closure#2} [instantiating crate: oxpinyin_facade]` |
| 24 | 9605 | `pinyin_mask_out` |
| 25 | 9186 | `<gimli::read::dwarf::Unit<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>, usize>>::new [instantiating crate: std]` |
| 26 | 9041 | `<oxpinyin_engine::session::Session<oxpinyin_runtime::RuntimeDict, oxpinyin_runtime::RuntimeLm>>::reconcile_replaced_selection [instantiating crate: oxpinyin_facade]` |
| 27 | 8842 | `oxpinyin_user::persistence::chewing_rows` |
| 28 | 8160 | `oxpinyin_data::table_entries::pinyin_index_entries` |
| 29 | 8079 | `oxpinyin_user::store::sync_system_rows::<oxpinyin_user::session_values::SessionValues<oxpinyin_store::bdb::BdbStore>> [instantiating crate: pinyin]` |
| 30 | 8010 | `oxpinyin_engine::session::build_scan_matrix` |

### libzhuyin

| Rank | Bytes | Demangled symbol |
| --- | --- | --- |
| 1 | 47116 | `<oxpinyin_user::lookup::UserLookup>::from_store` |
| 2 | 37408 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::mask_out::{closure#0} [instantiating crate: oxpinyin_facade]` |
| 3 | 32319 | `oxpinyin_user::persistence::load_logs_with_mask` |
| 4 | 31227 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::open_session::<oxpinyin_user::persistence::SystemOriginals> [instantiating crate: oxpinyin_runtime]` |
| 5 | 26409 | `oxpinyin_user::store_libpinyin::seed_txn_with_replay::<oxpinyin_user::session_values::SessionTxn> [instantiating crate: oxpinyin_runtime]` |
| 6 | 25573 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::replace_system_libraries [instantiating crate: oxpinyin_facade]` |
| 7 | 25356 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::save_reporting_inner::<<oxpinyin_facade::context::ContextCore>::save_user_with_layout_faults::{closure#0}>` |
| 8 | 22656 | `<oxpinyin_runtime::RuntimeDict>::user_trellis_index` |
| 9 | 21563 | `<oxpinyin_runtime::Runtime>::open_with_law` |
| 10 | 18280 | `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` |
| 11 | 18049 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::replace_system_libraries::{closure#1} [instantiating crate: oxpinyin_facade]` |
| 12 | 18035 | `oxpinyin_engine::nbest::nbest_sentences_with_seeds_generation::<oxpinyin_runtime::RuntimeDict, oxpinyin_runtime::RuntimeLm> [instantiating crate: zhuyin]` |
| 13 | 17607 | `oxpinyin_user::store_libpinyin::seed_txn_with_replay::<oxpinyin_user::session_values::SessionTxn> [instantiating crate: oxpinyin_facade]` |
| 14 | 16146 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::mask_out [instantiating crate: oxpinyin_facade]` |
| 15 | 14258 | `<oxpinyin_engine::session::Session<oxpinyin_runtime::RuntimeDict, oxpinyin_runtime::RuntimeLm>>::train_spans::<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>> [instantiating crate: oxpinyin_facade]` |
| 16 | 11995 | `<std::backtrace_rs::symbolize::gimli::Context>::new` |
| 17 | 11164 | `zhuyin_train` |
| 18 | 11040 | `zhuyin_phrase_segment` |
| 19 | 10512 | `oxpinyin_user::persistence::load_libraries` |
| 20 | 10221 | `<oxpinyin_runtime::RuntimeDict as oxpinyin_core::Dictionary>::checked_extension_tokens` |
| 21 | 9843 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::train_pronunciations::<&mut oxpinyin_engine::session::selection::TrainingReadings>::{closure#2} [instantiating crate: oxpinyin_facade]` |
| 22 | 9605 | `zhuyin_mask_out` |
| 23 | 9186 | `<gimli::read::dwarf::Unit<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>, usize>>::new [instantiating crate: std]` |
| 24 | 9041 | `<oxpinyin_engine::session::Session<oxpinyin_runtime::RuntimeDict, oxpinyin_runtime::RuntimeLm>>::reconcile_replaced_selection [instantiating crate: oxpinyin_facade]` |
| 25 | 9030 | `zhuyin_token_get_unigram_frequency` |
| 26 | 8842 | `oxpinyin_user::persistence::chewing_rows` |
| 27 | 8160 | `oxpinyin_data::table_entries::pinyin_index_entries` |
| 28 | 8079 | `oxpinyin_user::store::sync_system_rows::<oxpinyin_user::session_values::SessionValues<oxpinyin_store::bdb::BdbStore>> [instantiating crate: zhuyin]` |
| 29 | 8010 | `oxpinyin_engine::session::build_scan_matrix` |
| 30 | 8006 | `zhuyin_guess_sentence_with_prefix` |

## Top twenty generic families per library

C4 removes concrete angle-bracket parameter lists while preserving outer
qualified receiver/trait paths and method/closure names. It removes the
instantiating-crate annotation for grouping. An outer receiver bracket alone
does not make a non-generic method a generic family. Counts are retained
bodies; totals are their current bytes, not removable bytes.

### libpinyin

| Family | Bodies | Bytes |
| --- | --- | --- |
| `core::slice::sort::stable::quicksort::quicksort::<_>` | 30 | 84529 |
| `core::ptr::drop_glue::<_>` | 346 | 79857 |
| `core::slice::sort::stable::drift::sort::<_>` | 30 | 51251 |
| `<alloc::collections::btree::map::BTreeMap<_>>::insert` | 16 | 50420 |
| `oxpinyin_user::store_libpinyin::seed_txn_with_replay::<_>` | 2 | 44016 |
| `<oxpinyin_user::store::GenericUserStore<_>>::mask_out::{closure#0}` | 1 | 37408 |
| `<oxpinyin_user::store::GenericUserStore<_>>::open_session::<_>` | 1 | 31243 |
| `<oxpinyin_user::store::GenericUserStore<_>>::replace_system_libraries` | 1 | 25573 |
| `<oxpinyin_user::store::GenericUserStore<_>>::save_reporting_inner::<_>` | 1 | 25356 |
| `<hashbrown::raw::RawTable<_>>::reserve_rehash::<_>` | 19 | 24076 |
| `<oxpinyin_user::session_values::SessionValues<_> as oxpinyin_store::ReadStore>::range::<_>::{closure#0}` | 11 | 23067 |
| `<oxpinyin_user::session_values::SessionTxn as oxpinyin_store::WriteTxn>::range::<_>` | 4 | 20469 |
| `<alloc::collections::btree::map::entry::Entry<_>>::or_default` | 5 | 19141 |
| `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<_>` | 1 | 18280 |
| `<oxpinyin_user::store::GenericUserStore<_>>::replace_system_libraries::{closure#1}` | 1 | 18049 |
| `oxpinyin_engine::nbest::nbest_sentences_with_seeds_generation::<_>` | 1 | 18035 |
| `<oxpinyin_user::store::GenericUserStore<_>>::mask_out` | 1 | 16146 |
| `<alloc::collections::btree::node::Handle<_>>::remove_leaf_kv::<_>` | 10 | 15230 |
| `<core::cell::once::OnceCell<_>>::try_init::<_>` | 7 | 14123 |
| `<oxpinyin_engine::session::Session<_>>::train_spans::<_>` | 1 | 13828 |

### libzhuyin

| Family | Bodies | Bytes |
| --- | --- | --- |
| `core::slice::sort::stable::quicksort::quicksort::<_>` | 27 | 76308 |
| `core::ptr::drop_glue::<_>` | 320 | 76159 |
| `<alloc::collections::btree::map::BTreeMap<_>>::insert` | 16 | 50420 |
| `core::slice::sort::stable::drift::sort::<_>` | 27 | 46274 |
| `oxpinyin_user::store_libpinyin::seed_txn_with_replay::<_>` | 2 | 44016 |
| `<oxpinyin_user::store::GenericUserStore<_>>::mask_out::{closure#0}` | 1 | 37408 |
| `<oxpinyin_user::store::GenericUserStore<_>>::open_session::<_>` | 1 | 31227 |
| `<oxpinyin_user::store::GenericUserStore<_>>::replace_system_libraries` | 1 | 25573 |
| `<oxpinyin_user::store::GenericUserStore<_>>::save_reporting_inner::<_>` | 1 | 25356 |
| `<oxpinyin_user::session_values::SessionValues<_> as oxpinyin_store::ReadStore>::range::<_>::{closure#0}` | 11 | 23067 |
| `<hashbrown::raw::RawTable<_>>::reserve_rehash::<_>` | 16 | 19436 |
| `<alloc::collections::btree::map::entry::Entry<_>>::or_default` | 5 | 19141 |
| `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<_>` | 1 | 18280 |
| `<oxpinyin_user::store::GenericUserStore<_>>::replace_system_libraries::{closure#1}` | 1 | 18049 |
| `oxpinyin_engine::nbest::nbest_sentences_with_seeds_generation::<_>` | 1 | 18035 |
| `<oxpinyin_user::store::GenericUserStore<_>>::mask_out` | 1 | 16146 |
| `<oxpinyin_user::session_values::SessionTxn as oxpinyin_store::WriteTxn>::range::<_>` | 3 | 14705 |
| `<oxpinyin_engine::session::Session<_>>::train_spans::<_>` | 1 | 14258 |
| `<core::cell::once::OnceCell<_>>::try_init::<_>` | 7 | 14123 |
| `<alloc::collections::btree::node::Handle<_>>::remove_leaf_kv::<_>` | 9 | 13485 |

## Candidate audit envelopes

C4 selects unique intervals by the literal demangled-name predicates in the
reproduction code. Envelopes can overlap and include inlined work. They
neither prove reachability nor predict a candidate delta.

| Named envelope | pinyin bytes | zhuyin bytes |
| --- | --- | --- |
| read_with | 596 | 603 |
| formatting-name envelope | 31476 | 31476 |
| panic/unwind-name envelope | 4415 | 4415 |
| symbolizer-name envelope | 166483 | 166483 |

### Largest named oxpinyin data objects

C4 selects OBJECT records in `.rodata`/`.data.rel.ro` with an oxpinyin
definition name. Anonymous blobs, merged literals and inlined constants
need not have named records; this is not complete `.rodata` attribution.

#### libpinyin

| Section | Bytes | Named object |
| --- | --- | --- |
| .rodata | 17916 | `oxpinyin_core::string_tables::ZHUYIN_PINYIN_MAP` |
| .rodata | 10584 | `oxpinyin_chewing::chewing_key_data::CONTENT_STRINGS` |
| .rodata | 7056 | `oxpinyin_chewing::chewing_key_data::CONTENT_KEYS` |
| .rodata | 6000 | `oxpinyin_core::string_tables::HSU_ZHUYIN_INDEX` |
| .rodata | 5972 | `oxpinyin_core::string_tables::ZHUYIN_PINYIN_MAP_FLAGS` |
| .rodata | 5784 | `oxpinyin_core::string_tables::ETEN26_ZHUYIN_INDEX` |
| .rodata | 3456 | `oxpinyin_chewing::chewing_key_data::CHEWING_KEY_TABLE` |
| .rodata | 3248 | `oxpinyin_core::string_tables::LUOMA_PINYIN_INDEX` |
| .rodata | 3248 | `oxpinyin_core::string_tables::SECONDARY_ZHUYIN_INDEX` |
| .data.rel.ro | 2760 | `oxpinyin_engine::config::UPSTREAM_DEFAULTS` |
| .rodata | 2000 | `oxpinyin_core::string_tables::HSU_ZHUYIN_INDEX_FLAGS` |
| .rodata | 1928 | `oxpinyin_core::string_tables::ETEN26_ZHUYIN_INDEX_FLAGS` |

#### libzhuyin

| Section | Bytes | Named object |
| --- | --- | --- |
| .rodata | 17916 | `oxpinyin_core::string_tables::ZHUYIN_PINYIN_MAP` |
| .rodata | 10584 | `oxpinyin_chewing::chewing_key_data::CONTENT_STRINGS` |
| .rodata | 7056 | `oxpinyin_chewing::chewing_key_data::CONTENT_KEYS` |
| .rodata | 6000 | `oxpinyin_core::string_tables::HSU_ZHUYIN_INDEX` |
| .rodata | 5972 | `oxpinyin_core::string_tables::ZHUYIN_PINYIN_MAP_FLAGS` |
| .rodata | 5784 | `oxpinyin_core::string_tables::ETEN26_ZHUYIN_INDEX` |
| .rodata | 3456 | `oxpinyin_chewing::chewing_key_data::CHEWING_KEY_TABLE` |
| .rodata | 3248 | `oxpinyin_core::string_tables::LUOMA_PINYIN_INDEX` |
| .rodata | 3248 | `oxpinyin_core::string_tables::SECONDARY_ZHUYIN_INDEX` |
| .data.rel.ro | 2760 | `oxpinyin_engine::config::UPSTREAM_DEFAULTS` |
| .rodata | 2000 | `oxpinyin_core::string_tables::HSU_ZHUYIN_INDEX_FLAGS` |
| .rodata | 1928 | `oxpinyin_core::string_tables::ETEN26_ZHUYIN_INDEX_FLAGS` |

## Follow-up provenance and result

The deeper object-reference analysis was recorded at `2026-10-23T11:19:13Z`. Because
the original operational objects had been removed at the previous lane
end, the same-code release was recreated at
`b02c62934ea7ffd3e0f2ec1c1d8997149ce2feda` in the same worktree path, with
the same pinned tools, recipe, policies and home-disk `TMPDIR`.
`git diff --name-only 9ce07663..b02c6293 -- ':!*.md'` was empty.
The admitted rebuild ran from `2026-10-23T10:58:27Z` to
`2026-10-23T10:59:44Z` (machine `date -u`), with the normal admission
wrapper choosing two Cargo jobs. No release setting or link command was
changed, and no analysis-only linker flag was used.

The recreated pair passed the same release gates, linked ordering checks,
`file` classification and within-build strip/`cmp` identity check. The
file sizes and relevant crate totals match the earlier tables. Build IDs
from `readelf -n` also match the prior run's recorded `file` output:

| DSO | GNU build ID |
| --- | --- |
| pinyin | `8f708849f6e92f9e165fb9c8ea9cc68dda0874fa` |
| zhuyin | `c265ef3d6220b3fb01cfaabff6c24a5dd1e1dd59` |

The removed original files cannot be compared byte-for-byte across runs;
no such comparison is claimed. The new reference evidence belongs to the
recreated objects, whose shipping source, sizes and build IDs are named
here. The existing same-build unstripped/stripped pair was again compared
byte-for-byte. C1–C3 reproduce the unchanged build route; C5 below is the
complete historical in-memory runtime-analysis command. No workload was run to provoke a
panic or allocation failure.

The object has **two independent roots into backtrace printing**: its
default panic branch and its allocation-failure handler. This corrects
any reading of the earlier candidate table as assigning all symbolizer
retention solely to the panic hook. The observed stock-std byte saving
from runtime backtrace selection is zero: it selects already-linked
branches. A panic-hook-only source change would also leave the observed
OOM root.

## Runtime retention and disjoint split (follow-up)

C5 uses the recreated same-code objects, not an assumed Rust call graph. All figures are bytes of declared non-overlapping `.text` bodies. The split preserves C4's first-qualified-crate accounting; it does not recover pre-inlining source contribution. Categories are disjoint. `core::fmt` includes primitive fmt implementations; fmt implementations on other core-owned types are separate. Container bodies can contain failure branches that cannot be separated by symbol-size accounting.

| Nonzero `.text` bodies matching name | pinyin | zhuyin |
| --- | --- | --- |
| catch_unwind | 0 | 0 |
| rust_try | 0 | 0 |

### `core`

| Category | pinyin | zhuyin |
| --- | --- | --- |
| cells/once initialization | 14169 | 14169 |
| drop glue | 79857 | 76159 |
| fmt implementations on core types | 4453 | 4453 |
| formatting | 8740 | 8740 |
| hashing | 471 | 471 |
| iterators | 8319 | 8104 |
| numeric | 1670 | 1670 |
| other core | 5999 | 1583 |
| other slice operations | 744 | 751 |
| panic machinery | 304 | 304 |
| sorting | 184362 | 168757 |
| strings/parsing | 13708 | 13708 |
| total = previous crate bucket | 322796 | 298869 |

### `alloc`

| Category | pinyin | zhuyin |
| --- | --- | --- |
| Arc | 6544 | 6544 |
| BTree collections | 145824 | 143047 |
| Box | 209 | 213 |
| CString | 953 | 953 |
| String | 1948 | 562 |
| Vec | 12709 | 10298 |
| allocation/capacity failure handlers | 65 | 65 |
| formatting / fmt trait implementations | 3161 | 3161 |
| other alloc | 328 | 328 |
| raw Vec growth | 11143 | 10747 |
| total = previous crate bucket | 182884 | 175918 |

### `std`

| Category | pinyin | zhuyin |
| --- | --- | --- |
| I/O | 1964 | 1964 |
| allocator / OOM hook | 340 | 340 |
| backtrace / symbolization | 46240 | 46240 |
| filesystem | 5469 | 5469 |
| fmt implementations on std types | 5690 | 5690 |
| other std | 5746 | 5746 |
| panic / panic hooks | 2900 | 2900 |
| paths | 4448 | 4172 |
| synchronization | 3541 | 3541 |
| threading | 28 | 28 |
| total = previous crate bucket | 76366 | 76090 |

### Direct component footprints and external callers

Caller counts below are distinct source bodies outside the component; pair counts deduplicate repeated instructions to the same target body. They include direct control references and explicit function-address references. They are not runtime call counts. This enumerates the static instruction references resolved by C5; arbitrary register-based indirect calls are not a complete semantic call graph. Missing edges are not proof of deadness. The particular backtrace callback indirection is resolved explicitly below.

| Component | pinyin bytes | zhuyin bytes | outside callers per DSO | source/target pairs per DSO |
| --- | --- | --- | --- | --- |
| gimli | 36052 | 36052 | 9 | 32 |
| addr2line | 13312 | 13312 | 3 | 6 |
| rustc_demangle | 19771 | 19771 | 2 | 2 |
| miniz_oxide | 9607 | 9597 | 2 | 2 |

| DSO | four component definition buckets | broader symbolizer-tagged body union including miniz |
| --- | --- | --- |
| pinyin | 78742 | 176090 |
| zhuyin | 78732 | 176080 |

The broader union includes std backtrace functions and type-specific core/alloc support, and can overlap formatting, containers and drop glue. It is an audit footprint, not a garbage-collection result or a proven saving.

#### All resolved outside callers into `gimli`

| Caller symbol | pinyin start | zhuyin start |
| --- | --- | --- |
| `<core::cell::once::OnceCell<core::result::Result<addr2line::line::Lines, gimli::read::Error>>>::try_init::<<core::cell::once::OnceCell<core::result::Result<addr2line::line::Lines, gimli::read::Error>>>::get_or_init<<addr2line::line::LazyLines>::borrow<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>::{closure#0}>::{closure#0}, !>` | 0x83e60 | 0x7ea70 |
| `<core::cell::once::OnceCell<core::result::Result<addr2line::function::Functions<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>, gimli::read::Error>>>::try_init::<<core::cell::once::OnceCell<core::result::Result<addr2line::function::Functions<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>, gimli::read::Error>>>::get_or_init<<addr2line::function::LazyFunctions<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::borrow::{closure#0}>::{closure#0}, !>` | 0x85d50 | 0x80960 |
| `<core::cell::once::OnceCell<core::result::Result<addr2line::function::Function<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>, gimli::read::Error>>>::try_init::<<core::cell::once::OnceCell<core::result::Result<addr2line::function::Function<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>, gimli::read::Error>>>::get_or_init<<addr2line::function::LazyFunction<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::borrow::{closure#0}>::{closure#0}, !>` | 0x86cd0 | 0x818e0 |
| `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` | 0x1cdfd5 | 0x19b3d5 |
| `<std::backtrace_rs::symbolize::gimli::Context>::new` | 0x1d5db3 | 0x1a31b3 |
| `<addr2line::unit::ResUnit<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::find_function_or_location` | 0x1d8d80 | 0x1a6180 |
| `addr2line::line::render_file::<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>` | 0x1dd3a0 | 0x1aa7a0 |
| `<addr2line::function::Function<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::parse_children` | 0x1e04f0 | 0x1ad8f0 |
| `addr2line::function::name_entry::<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>` | 0x1e3810 | 0x1b0c10 |

#### All resolved outside callers into `addr2line`

| Caller symbol | pinyin start | zhuyin start |
| --- | --- | --- |
| `<core::cell::once::OnceCell<core::result::Result<addr2line::line::Lines, gimli::read::Error>>>::try_init::<<core::cell::once::OnceCell<core::result::Result<addr2line::line::Lines, gimli::read::Error>>>::get_or_init<<addr2line::line::LazyLines>::borrow<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>::{closure#0}>::{closure#0}, !>` | 0x83e60 | 0x7ea70 |
| `<core::cell::once::OnceCell<core::result::Result<addr2line::function::Function<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>, gimli::read::Error>>>::try_init::<<core::cell::once::OnceCell<core::result::Result<addr2line::function::Function<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>, gimli::read::Error>>>::get_or_init<<addr2line::function::LazyFunction<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::borrow::{closure#0}>::{closure#0}, !>` | 0x86cd0 | 0x818e0 |
| `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` | 0x1cdfd5 | 0x19b3d5 |

#### All resolved outside callers into `rustc_demangle`

| Caller symbol | pinyin start | zhuyin start |
| --- | --- | --- |
| `<std::backtrace_rs::symbolize::SymbolName as core::fmt::Display>::fmt` | 0x1d2bf0 | 0x19fff0 |
| `<std::backtrace_rs::symbolize::Symbol>::name` | 0x1edfb0 | 0x1bb3b0 |

#### All resolved outside callers into `miniz_oxide`

| Caller symbol | pinyin start | zhuyin start |
| --- | --- | --- |
| `<std::backtrace_rs::symbolize::gimli::elf::Object>::section` | 0x1d9960 | 0x1a6d60 |
| `std::backtrace_rs::symbolize::gimli::elf::decompress_zlib` | 0x1e84f0 | 0x1b58f0 |

### Concrete root and callback references

Offsets in this table are relative to the named containing function. “Address” means the object stores/passes a function address, not that the instruction calls that function. The callback bridge below resolves the actual indirect dispatch.

| DSO | Source | offset | reference kind | Target |
| --- | --- | --- | --- | --- |
| pinyin | `std::panicking::panic_with_hook` | +0x555 | function address | `<<std::sys::backtrace::BacktraceLock>::print::DisplayBacktrace as core::fmt::Display>::fmt` |
| zhuyin | `std::panicking::panic_with_hook` | +0x555 | function address | `<<std::sys::backtrace::BacktraceLock>::print::DisplayBacktrace as core::fmt::Display>::fmt` |
| pinyin | `std::alloc::default_alloc_error_hook` | +0xc0 | function address | `<<std::sys::backtrace::BacktraceLock>::print::DisplayBacktrace as core::fmt::Display>::fmt` |
| zhuyin | `std::alloc::default_alloc_error_hook` | +0xc0 | function address | `<<std::sys::backtrace::BacktraceLock>::print::DisplayBacktrace as core::fmt::Display>::fmt` |
| pinyin | `std::alloc::default_alloc_error_hook` | +0xe8 | function address | `<<std::sys::backtrace::BacktraceLock>::print::DisplayBacktrace as core::fmt::Display>::fmt` |
| zhuyin | `std::alloc::default_alloc_error_hook` | +0xe8 | function address | `<<std::sys::backtrace::BacktraceLock>::print::DisplayBacktrace as core::fmt::Display>::fmt` |
| pinyin | `std::sys::backtrace::_print_fmt::{closure#1}` | +0xa0 | call | `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` |
| zhuyin | `std::sys::backtrace::_print_fmt::{closure#1}` | +0xa0 | call | `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` |

| DSO | DisplayBacktrace::fmt start | vtable loaded at +0x254 | R_X86_64_RELATIVE slot (+0x20) | callback body | libunwind trace_fn |
| --- | --- | --- | --- | --- | --- |
| pinyin | 0x1cdb00 | 0x2534d0 | 0x2534f0 | 0x1cde90 | 0x1cde40 |
| zhuyin | 0x19af00 | 0x2175e0 | 0x217600 | 0x19b290 | 0x19b240 |

`DisplayBacktrace::fmt+0x262` takes the `trace_fn` address; `+0x270` calls imported `_Unwind_Backtrace`. `trace_fn+0x1f` dispatches through `*0x20(%rax)` to the relocated callback. The callback then calls `Cache::with_global` at `+0xa0`. Objdump's nearest-preceding-object label for the anonymous vtable is not its ownership; the addresses and relocations above establish the connection.

### ABI-to-runtime reference chains

#### libpinyin

| Entry path | Resolved references |
| --- | --- |
| allocation failure | `pinyin_alloc_instance+0x15ab` [call] → `alloc::alloc::handle_alloc_error`; `alloc::alloc::handle_alloc_error+0xd` [call] → `__rustc::__rust_alloc_error_handler`; `__rustc::__rust_alloc_error_handler+0xd` [call] → `std::alloc::rust_oom`; `std::alloc::rust_oom+0x4` [call] → `std::sys::backtrace::__rust_end_short_backtrace::<std::alloc::rust_oom::{closure#0}, !>`; `std::sys::backtrace::__rust_end_short_backtrace::<std::alloc::rust_oom::{closure#0}, !>+0x4` [call] → `std::alloc::rust_oom::{closure#0}`; `std::alloc::rust_oom::{closure#0}+0x4` [call] → `std::alloc::default_alloc_error_hook` |
| panic hook | `pinyin_alloc_instance+0x225` [call] → `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore> as core::clone::Clone>::clone`; `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore> as core::clone::Clone>::clone+0xaa` [call] → `std::sys::random::linux::hashmap_random_keys`; `std::sys::random::linux::hashmap_random_keys+0x212` [call] → `core::panicking::panic_fmt`; `core::panicking::panic_fmt+0x26` [call] → `__rustc::rust_begin_unwind`; `__rustc::rust_begin_unwind+0x17` [call] → `std::sys::backtrace::__rust_end_short_backtrace::<std::panicking::panic_handler::{closure#0}, !>`; `std::sys::backtrace::__rust_end_short_backtrace::<std::panicking::panic_handler::{closure#0}, !>+0x4` [call] → `std::panicking::panic_handler::{closure#0}`; `std::panicking::panic_handler::{closure#0}+0x3c` [call] → `std::panicking::panic_with_hook` |
| C boundary abort path | `pinyin_alloc_instance+0x162c` [call] → `core::panicking::panic_cannot_unwind` |
| normal error formatting | `pinyin_init+0xe10` [function address] → `<oxpinyin_facade::context::OpenFailure as core::fmt::Display>::fmt`; `<oxpinyin_facade::context::OpenFailure as core::fmt::Display>::fmt+0x34` [call] → `core::fmt::write` |

#### libzhuyin

| Entry path | Resolved references |
| --- | --- |
| allocation failure | `zhuyin_alloc_instance+0x1568` [call] → `alloc::alloc::handle_alloc_error`; `alloc::alloc::handle_alloc_error+0xd` [call] → `__rustc::__rust_alloc_error_handler`; `__rustc::__rust_alloc_error_handler+0xd` [call] → `std::alloc::rust_oom`; `std::alloc::rust_oom+0x4` [call] → `std::sys::backtrace::__rust_end_short_backtrace::<std::alloc::rust_oom::{closure#0}, !>`; `std::sys::backtrace::__rust_end_short_backtrace::<std::alloc::rust_oom::{closure#0}, !>+0x4` [call] → `std::alloc::rust_oom::{closure#0}`; `std::alloc::rust_oom::{closure#0}+0x4` [call] → `std::alloc::default_alloc_error_hook` |
| panic hook | `zhuyin_alloc_instance+0x1df` [call] → `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore> as core::clone::Clone>::clone`; `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore> as core::clone::Clone>::clone+0xaa` [call] → `std::sys::random::linux::hashmap_random_keys`; `std::sys::random::linux::hashmap_random_keys+0x212` [call] → `core::panicking::panic_fmt`; `core::panicking::panic_fmt+0x26` [call] → `__rustc::rust_begin_unwind`; `__rustc::rust_begin_unwind+0x17` [call] → `std::sys::backtrace::__rust_end_short_backtrace::<std::panicking::panic_handler::{closure#0}, !>`; `std::sys::backtrace::__rust_end_short_backtrace::<std::panicking::panic_handler::{closure#0}, !>+0x4` [call] → `std::panicking::panic_handler::{closure#0}`; `std::panicking::panic_handler::{closure#0}+0x3c` [call] → `std::panicking::panic_with_hook` |
| C boundary abort path | `zhuyin_alloc_instance+0x15dc` [call] → `core::panicking::panic_cannot_unwind` |
| normal error formatting | `zhuyin_init+0xd90` [function address] → `<oxpinyin_facade::context::OpenFailure as core::fmt::Display>::fmt`; `<oxpinyin_facade::context::OpenFailure as core::fmt::Display>::fmt+0x34` [call] → `core::fmt::write` |

### References from the symbolization callback into each component

#### `gimli` (both libraries)

| Source | offset | kind | Target |
| --- | --- | --- | --- |
| `std::sys::backtrace::_print_fmt::{closure#1}` | +0xa0 | call | `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` |
| `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` | +0x24e3 | call | `<gimli::read::unit::DebugInfoUnitHeadersIter<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::next` |

#### `addr2line` (both libraries)

| Source | offset | kind | Target |
| --- | --- | --- | --- |
| `std::sys::backtrace::_print_fmt::{closure#1}` | +0xa0 | call | `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` |
| `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` | +0x115a | call | `<addr2line::unit::ResUnit<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::find_function_or_location` |

#### `rustc_demangle` (both libraries)

| Source | offset | kind | Target |
| --- | --- | --- | --- |
| `std::sys::backtrace::_print_fmt::{closure#1}` | +0x106 | call | `<std::backtrace_rs::print::BacktraceFrameFmt>::print_raw_with_column` |
| `<std::backtrace_rs::print::BacktraceFrameFmt>::print_raw_with_column` | +0x19b | function address | `<std::backtrace_rs::symbolize::SymbolName as core::fmt::Display>::fmt` |
| `<std::backtrace_rs::symbolize::SymbolName as core::fmt::Display>::fmt` | +0xce | function address | `<&rustc_demangle::DemangleStyle as core::fmt::Display>::fmt` |

#### `miniz_oxide` (both libraries)

| Source | offset | kind | Target |
| --- | --- | --- | --- |
| `std::sys::backtrace::_print_fmt::{closure#1}` | +0xa0 | call | `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` |
| `<std::backtrace_rs::symbolize::gimli::Cache>::with_global::<std::backtrace_rs::symbolize::gimli::resolve::{closure#1}>` | +0x1a8e | call | `<std::backtrace_rs::symbolize::gimli::elf::Object>::section` |
| `<std::backtrace_rs::symbolize::gimli::elf::Object>::section` | +0x1df | call | `miniz_oxide::inflate::core::decompress` |

### Representative ordinary-use references

| Category | pinyin source | offset | kind | Target |
| --- | --- | --- | --- | --- |
| core/sorting | `<oxpinyin_engine::session::Session<oxpinyin_runtime::RuntimeDict, oxpinyin_runtime::RuntimeLm>>::scan_parsed_paths` | +0xb58 | call | `core::slice::sort::unstable::ipnsort::<u32, <u32 as core::cmp::PartialOrd>::lt>` |
| core/drop glue | `<oxpinyin_data::dict::AddonDictionary>::open_with` | +0x1a6b | call | `core::ptr::drop_glue::<alloc::vec::Vec<oxpinyin_data::table_info::TableRow>>` |
| core/iterators | `<oxpinyin_user::lookup::UserLookup>::lookup` | +0x370 | call | `<core::iter::adapters::map::Map<core::iter::adapters::filter::Filter<core::slice::iter::Iter<(alloc::string::String, oxpinyin_core::vocab::PhraseEntry)>, <oxpinyin_user::lookup::UserLookup>::lookup::{closure#1}>, <oxpinyin_user::lookup::UserLookup>::lookup::{closure#2}> as core::iter::traits::iterator::Iterator>::next` |
| alloc/BTree collections | `<oxpinyin_user::lookup::UserLookup>::from_store` | +0x4ea | call | `<alloc::collections::btree::map::BTreeMap<alloc::vec::Vec<u8>, alloc::vec::Vec<u8>>>::range::<[u8], (core::ops::range::Bound<&[u8]>, core::ops::range::Bound<&[u8]>)>` |
| alloc/Vec | `<oxpinyin_engine::session::Session<oxpinyin_runtime::RuntimeDict, oxpinyin_runtime::RuntimeLm>>::scan_window` | +0x12b1 | call | `<alloc::vec::Vec<(oxpinyin_engine::session::RankKey, u32)> as alloc::vec::spec_from_iter::SpecFromIter<(oxpinyin_engine::session::RankKey, u32), core::iter::adapters::map::Map<core::iter::adapters::enumerate::Enumerate<core::iter::adapters::zip::Zip<core::slice::iter::Iter<oxpinyin_engine::candidate::Candidate>, alloc::vec::into_iter::IntoIter<u64>>>, <oxpinyin_engine::session::Session<oxpinyin_runtime::RuntimeDict, oxpinyin_runtime::RuntimeLm>>::scan_window::{closure#2}>>>::from_iter` |
| alloc/raw Vec growth | `<oxpinyin_core::graph::SegmentGraph>::build_with_options` | +0x16e | call | `<alloc::raw_vec::RawVec<u32>>::grow_one` |


## What can be removed on the permitted stable configuration

These decisions distinguish a supported stable API from a static
link-time removal. They do not declare source-level removal impossible
in every redesigned program. No behavior-preserving cut of all observed
roots has been established in this program, and none was implemented.

| Runtime candidate | Current local `.text` footprint (pinyin / zhuyin B) | Removable with the existing stock stable toolchain and unchanged policies? | Saving supported by this analysis; ruling needed |
| --- | --- | --- | --- |
| `gimli` reader code | 36,052 / 36,052 | **Not removable on the supported stable toolchain**, per the 2026-10-23 maintainer ruling. No further removal work. | Closed. The recorded bytes are retained-code footprints, not savings. The earlier policy/toolchain questions are superseded by the ruling. |
| `addr2line` resolution | 13,312 / 13,312 | **Not removable on the supported stable toolchain**, per the 2026-10-23 maintainer ruling. No further removal work. | Closed. The recorded bytes are retained-code footprints, not savings. The earlier policy/toolchain questions are superseded by the ruling. |
| `rustc_demangle` | 19,771 / 19,771 | **Not removable on the supported stable toolchain**, per the 2026-10-23 maintainer ruling. No further removal work. | Closed. The recorded bytes are retained-code footprints, not savings. The earlier policy/toolchain questions are superseded by the ruling. |
| `miniz_oxide` | 9,607 / 9,597 | **Not removable on the supported stable toolchain**, per the 2026-10-23 maintainer ruling. No further removal work. | Closed. The recorded bytes are retained-code footprints, not savings. The earlier policy/toolchain questions are superseded by the ruling. |
| `std` backtrace printer and type-specific support | 46,240 / 46,240 in the std definition bucket; broader tagged union 176,090 / 176,080 includes the four components and shared core/alloc bodies | **Not removable on the supported stable toolchain**, per the 2026-10-23 maintainer ruling. No further removal work. | Closed. The recorded bytes are retained-code footprints, not savings. The earlier policy/toolchain questions are superseded by the ruling. |
| Core formatting and formatting of core types; alloc formatting | 8,740 + 4,453 core bytes and 3,161 alloc bytes, each per DSO | Normal init-error formatting references `OpenFailure::fmt`, `alloc::fmt::format_inner` and `core::fmt::write`; store/data errors and engine callers also reference them. These are not all unreachable panic-only code. Stable local deduplication is possible in principle, with diagnostics preserved, but no safe whole-formatting cut is identified. | No proven removable total. Removing required diagnostics is not a permitted reduction. A separately authorized local refactoring must keep byte-level diagnostics and prove its actual net saving; STOP before implementation. |
| Core panic helpers and alloc failure handlers | 304 core bytes and 65 alloc bytes per DSO; std panic/hook bucket is 2,900 B and std OOM bucket 340 B | C-boundary cleanup references `panic_cannot_unwind`. The ordinary collection path reaches `handle_alloc_error` and `rust_oom`; HashMap seeding has a retained branch to `panic_fmt`. A no-panic source policy and `Result` APIs do not erase these branches. | No safe wholesale removal is shown. Stable `panic=abort` would be a profile/panic policy change and still does not establish elimination of either printing root; no variant was built. STOP for strategy/flag changes, with new size/time/residency evidence before adoption. |
| Core sorting/drop/iterators and alloc collections/buffers | Disjoint rows above: core sorting 184,362 / 168,757; drop glue 79,857 / 76,159; iterators 8,319 / 8,104; alloc BTree code 145,824 / 143,047, plus Vec/raw growth | Actual oxpinyin bodies reference them; these implement data structure and lifetime work. Dropping an entire crate/module is not a semantics-preserving stable toggle. Refactoring duplicated instances without policy changes remains an internal code candidate, not an approved deletion. | No blanket removal budget. Current footprints are not waste totals. A per-family stable source change needs separate implementation authorization and measured deltas; a size-versus-speed/residency trade or profile change is a STOP. |

The maintainer has resolved the backtrace/symbolizer question as not
removable on the supported stable toolchain, with no further work.
No custom-std, hook, failure-diagnostic or symbolizer-removal experiment
is planned. The source and reference observations below are the
completed historical evidence for that decision.

The installed Rust 1.97.1 source documentation corroborates the two
observed roots: `std/panicking.rs`, `panic_with_hook` dispatches
`Hook::Default` to `default_hook`; `default_hook` can print a backtrace.
That default hook is inlined into the observed `panic_with_hook` body,
which takes `DisplayBacktrace::fmt` at `+0x555` and `+0x583`.
`std/alloc.rs`, `rust_oom` invokes `default_alloc_error_hook`; that hook
also obtains the backtrace style and prints the backtrace. Its two
formatter address sites are `+0xc0` and `+0xe8` in the linked object.
These statements are supported by the actual references above, not by
assuming every std version has the same implementation.

Source check: `git grep -n catch_unwind b02c6293 --
crates/oxpinyin-capi/src crates/oxpinyin-zhuyin-capi/src` returned no
matches. C5's defined-symbol inspection also found no nonzero `.text` body whose
name contains `catch_unwind` or `rust_try` in either object. Symbol absence alone would not exclude
inlining; the facade source check is why this report does not attribute
retention to a C-boundary catch wrapper. The retained abort-on-unwind
boundary path is instead shown by its actual call sites.

The lockfile also lists a `backtrace` dependency through `dhat`; lockfile
membership alone therefore does not establish a shipping-code root.
The outside callers listed above, and the release recipe's actual object,
establish the std backtrace retention path being analyzed here. The exact
net effect of removing or replacing any path remains unmeasured; no
runtime failure probe, candidate implementation or new dependency ran.

**Closed by ruling:** backtrace/symbolizer removal is not pursued on the
supported stable toolchain. Remaining size candidates still stop before
implementation. Profile, panic, linker, version-script and
size-versus-speed/residency changes retain their separate STOPs.

## Stable-sort audit: thirty object instantiations

The object audit resumed from source checkpoint `7829f247` after #1031
released all owned heavy jobs. C1–C3 reconstructed the same shipping code through
normal shared admission from `2026-10-23T12:06:23Z` to
`2026-10-23T12:07:28Z`, with three admitted Cargo jobs. The only
non-Markdown difference from `9ce07663` is
`tools/bisection/heavy-slot.test.py`; no shipping source, policy, tool,
profile or release command changed. Home worktree and TMPDIR were again
confirmed ext4. Both stripped sizes, section sizes and GNU build IDs
match the earlier tables; within-build re-strip/`cmp`, release gates and
linked ordering passed. This is same-code reconstruction, not a
candidate variant or a cross-run performance baseline.

C6 pairs **exactly thirty** quicksort and drift identities in libpinyin,
reconciling to **84,529 B + 51,251 B = 135,780 B** of distinct declared
function bodies. Seventeen pairs are direct project sort sites, six are
implicit BTreeMap collections, two are implicit BTreeSet collections,
and five belong to the closed std symbolizer path. Those five are
included only to reconcile the requested enumeration: their 20,859 B
are retained under the ruling; no removal or tie investigation is
performed there. The other twenty-five pairs total 114,921 B, also a
footprint rather than a saving. No cargo-bloat install or dependency
was used.

The matrix below records complete demangled element/comparator types,
validated instantiating-crate annotations, addresses, declared sizes and
actual external call roots. Q and D are respectively
`core::slice::sort::stable::quicksort::quicksort::<E,F>` and
`core::slice::sort::stable::drift::sort::<E,F>`. Every pair has the same
E/F identity; the IDs sort that exact identity lexically. C6 walks direct
call/branch references backwards only through stable-sort internals to
find the first outside callers. A root offset is an actual instruction
in that caller, not a guessed source stack. Inline producer/closure
paths in F connect those roots to the source operations below. All
roots in this table call a retained stable-sort entry; it then reaches
Q/D through its observed internal calls. Register-based callers need not
be resolved for the body-size accounting.

| ID / source site | Element E | Exact comparator F / instantiating annotation | Q B | D B | Q/D addresses (hex) | First outside call roots and instruction offsets |
| --- | --- | --- | --- | --- | --- | --- |
| S01: config::default_values / BTreeMap | `(alloc::string::String, oxpinyin_engine::config::ConfigValue)` | `<[(alloc::string::String, oxpinyin_engine::config::ConfigValue)]>::sort_by<<alloc::collections::btree::map::BTreeMap<alloc::string::String, oxpinyin_engine::config::ConfigValue> as core::iter::traits::collect::FromIterator<(alloc::string::String, oxpinyin_engine::config::ConfigValue)>>::from_iter<core::iter::adapters::map::Map<core::slice::iter::Iter<(&str, oxpinyin_engine::config::Seed)>, oxpinyin_engine::config::default_values::{closure#0}>>::{closure#0}>::{closure#0}` | 3241 | 1898 | 104340 / 4db30 | `pinyin_set_options +0xd94` (call) |
| S02: with_kept_keys / BTreeMap | `(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)` | `<[(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)]>::sort_by<<alloc::collections::btree::map::BTreeMap<alloc::vec::Vec<u8>, alloc::vec::Vec<u8>> as core::iter::traits::collect::FromIterator<(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)>>::from_iter<alloc::vec::into_iter::IntoIter<(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)>>::{closure#0}>::{closure#0} [instantiating crate: oxpinyin_user]` | 3241 | 1898 | 18e990 / 18e220 | `oxpinyin_user::persistence::with_kept_keys +0xee2` (call) |
| S03: ST17 | `(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)` | `<[(alloc::vec::Vec<u8>, alloc::vec::Vec<u8>)]>::sort_by<<oxpinyin_store::bdb::BdbStore>::sorted_rows::{closure#0}>::{closure#0}` | 3241 | 1898 | 187a00 / 187210 | `<oxpinyin_store::bdb::BdbStore>::sorted_rows +0x3b7` (call) |
| S04: Closed std: ResUnits::parse c0 | `(gimli::common::DebugInfoOffset, gimli::common::DebugArangesOffset)` | `<[(gimli::common::DebugInfoOffset, gimli::common::DebugArangesOffset)]>::sort_by_key<gimli::common::DebugInfoOffset, <addr2line::unit::ResUnits<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::parse::{closure#0}>::{closure#0} [instantiating crate: std]` | 2662 | 1698 | 1eb730 / 1eb080 | `<std::backtrace_rs::symbolize::gimli::Context>::new +0x2bc2` (call) |
| S05: ST02 | `(u32, alloc::string::String)` | `<[(u32, alloc::string::String)]>::sort_by_key<u32, oxpinyin_data::dict::resolve_suggestions::{closure#1}>::{closure#0}` | 2690 | 1589 | f8140 / f7b00 | `pinyin::predict::guess_predicted_result +0x3ac3` (call) |
| S06: collect_pronunciations_for_tokens result / BTreeMap | `(u32, alloc::vec::Vec<oxpinyin_user::phrase::UserPronunciation>)` | `<[(u32, alloc::vec::Vec<oxpinyin_user::phrase::UserPronunciation>)]>::sort_by<<alloc::collections::btree::map::BTreeMap<u32, alloc::vec::Vec<oxpinyin_user::phrase::UserPronunciation>> as core::iter::traits::collect::FromIterator<(u32, alloc::vec::Vec<oxpinyin_user::phrase::UserPronunciation>)>>::from_iter<core::iter::adapters::map::Map<alloc::collections::btree::map::IntoIter<u32, alloc::vec::Vec<(u32, oxpinyin_user::phrase::UserPronunciation)>>, oxpinyin_user::store::collect_pronunciations_for_tokens<oxpinyin_user::session_values::SessionValues<oxpinyin_store::bdb::BdbStore>>::{closure#1}>>::{closure#0}>::{closure#0}` | 2534 | 1573 | 192d50 / 192640 | `<oxpinyin_user::lookup::UserLookup>::from_store +0xa514` (call)<br>`<oxpinyin_runtime::RuntimeDict>::user_trellis_index +0x50f9` (call) |
| S07: chewing_rows written / BTreeSet | `(u32, alloc::vec::Vec<u16>)` | `<(u32, alloc::vec::Vec<u16>) as core::cmp::PartialOrd>::lt [instantiating crate: oxpinyin_user]` | 3246 | 2071 | 194130 / 193910 | `oxpinyin_user::persistence::chewing_rows +0x2165` (call) |
| S08: ST15 | `(u32, alloc::vec::Vec<u16>, u64, bool)` | `<[(u32, alloc::vec::Vec<u16>, u64, bool)]>::sort_by_key<u32, oxpinyin_user::store_libpinyin::export_state<oxpinyin_store::bdb::BdbStore>::{closure#2}>::{closure#0} [instantiating crate: oxpinyin_facade]` | 2874 | 1605 | 10b120 / 10aa60 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::save_reporting_inner::<<oxpinyin_facade::context::ContextCore>::save_user_with_layout_faults::{closure#0}> +0x113b` (call) |
| S09: ST05 | `(u32, alloc::vec::Vec<u8>)` | `<[(u32, alloc::vec::Vec<u8>)]>::sort_by<oxpinyin_runtime::merge_suggestion_rows::{closure#0}>::{closure#0}` | 2841 | 1799 | 163930 / 163220 | `pinyin::predict::guess_predicted_result +0x3aa5` (call) |
| S10: ST06 | `(u32, alloc::vec::Vec<u8>)` | `<[(u32, alloc::vec::Vec<u8>)]>::sort_by_key<u32, oxpinyin_runtime::merge_suggestion_rows::{closure#2}>::{closure#0}` | 2690 | 1589 | 1625c0 / 161f80 | `pinyin::predict::guess_predicted_result +0x3ad8` (call) |
| S11: Merge::finish / BTreeMap | `(u32, core::option::Option<oxpinyin_data::chunk_write::ChunkItem>)` | `<[(u32, core::option::Option<oxpinyin_data::chunk_write::ChunkItem>)]>::sort_by<<alloc::collections::btree::map::BTreeMap<u32, core::option::Option<oxpinyin_data::chunk_write::ChunkItem>> as core::iter::traits::collect::FromIterator<(u32, core::option::Option<oxpinyin_data::chunk_write::ChunkItem>)>>::from_iter<alloc::vec::into_iter::IntoIter<(u32, core::option::Option<oxpinyin_data::chunk_write::ChunkItem>)>>::{closure#0}>::{closure#0} [instantiating crate: oxpinyin_user]` | 3221 | 1637 | 1917d0 / 191070 | `oxpinyin_user::persistence::load_logs_with_mask +0x6bf8` (call) |
| S12: load_libraries + open_session / BTreeMap | `(u32, oxpinyin_data::chunk_write::ChunkItem)` | `<[(u32, oxpinyin_data::chunk_write::ChunkItem)]>::sort_by<<alloc::collections::btree::map::BTreeMap<u32, oxpinyin_data::chunk_write::ChunkItem> as core::iter::traits::collect::FromIterator<(u32, oxpinyin_data::chunk_write::ChunkItem)>>::from_iter<alloc::vec::into_iter::IntoIter<(u32, oxpinyin_data::chunk_write::ChunkItem)>>::{closure#0}>::{closure#0} [instantiating crate: oxpinyin_user]` | 3221 | 1637 | 196cd0 / 196660 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::open_session::<oxpinyin_user::persistence::SystemOriginals> [instantiating crate: oxpinyin_runtime] +0x6dc1` (call)<br>`oxpinyin_user::persistence::load_libraries +0x24c4` (call) |
| S13: ST14 | `(u32, oxpinyin_user::phrase::UserPronunciation)` | `<[(u32, oxpinyin_user::phrase::UserPronunciation)]>::sort_by_key<u32, oxpinyin_user::store::sort_by_insertion<(u32, oxpinyin_user::phrase::UserPronunciation), oxpinyin_user::store::collect_pronunciations_for_tokens<oxpinyin_user::session_values::SessionValues<oxpinyin_store::bdb::BdbStore>>::{closure#1}::{closure#0}>::{closure#0}>::{closure#0}` | 2974 | 1605 | 1959a0 / 1952e0 | `<oxpinyin_user::lookup::UserLookup>::from_store +0x7865` (call)<br>`<oxpinyin_runtime::RuntimeDict>::user_trellis_index +0x1a2e` (call) |
| S14: ST12 | `(u32, oxpinyin_user::phrase::UserPronunciation)` | `<[(u32, oxpinyin_user::phrase::UserPronunciation)]>::sort_by_key<u32, oxpinyin_user::store::sort_by_insertion<(u32, oxpinyin_user::phrase::UserPronunciation), oxpinyin_user::store::collect_pronunciations_from_store<oxpinyin_user::session_values::SessionValues<oxpinyin_store::bdb::BdbStore>>::{closure#1}>::{closure#0}>::{closure#0} [instantiating crate: oxpinyin_runtime]` | 2974 | 1605 | 164ec0 / 164870 | `<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::phrase [instantiating crate: pinyin] +0x1110` (call)<br>`<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::phrase [instantiating crate: oxpinyin_runtime] +0x1110` (call) |
| S15: ST11 | `(u8, &oxpinyin_data::table_info::FileName, oxpinyin_user::persistence::LibraryFile)` | `<[(u8, &oxpinyin_data::table_info::FileName, oxpinyin_user::persistence::LibraryFile)]>::sort_by_key<u8, oxpinyin_user::persistence::libraries_in_pin_order::{closure#2}>::{closure#0}` | 2768 | 1477 | 190040 / 18fa70 | `oxpinyin_user::persistence::libraries_in_pin_order +0x1e6` (call) |
| S16: seed_txn_with_replay offsets / BTreeMap | `(u8, u32)` | `<[(u8, u32)]>::sort_by<<alloc::collections::btree::map::BTreeMap<u8, u32> as core::iter::traits::collect::FromIterator<(u8, u32)>>::from_iter<core::iter::adapters::filter::Filter<core::iter::adapters::map::Map<alloc::collections::btree::map::Iter<u8, u32>, oxpinyin_user::store_libpinyin::seed_txn_with_replay<oxpinyin_user::session_values::SessionTxn>::{closure#2}>, oxpinyin_user::store_libpinyin::seed_txn_with_replay<oxpinyin_user::session_values::SessionTxn>::{closure#3}>>::{closure#0}>::{closure#0} [instantiating crate: oxpinyin_runtime]` | 2238 | 1640 | 161200 / 160b90 | `oxpinyin_user::store_libpinyin::seed_txn_with_replay::<oxpinyin_user::session_values::SessionTxn> [instantiating crate: oxpinyin_runtime] +0x627a` (call)<br>`oxpinyin_user::store_libpinyin::seed_txn_with_replay::<oxpinyin_user::session_values::SessionTxn> [instantiating crate: oxpinyin_facade] +0x42b1` (call) |
| S17: Closed std: Functions::parse c1 | `addr2line::function::FunctionAddress` | `<[addr2line::function::FunctionAddress]>::sort_by_key<u64, <addr2line::function::Functions<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::parse::{closure#1}>::{closure#0} [instantiating crate: std]` | 2438 | 1586 | 1e5e00 / 1e57c0 | `<core::cell::once::OnceCell<core::result::Result<addr2line::function::Functions<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>, gimli::read::Error>>>::try_init::<<core::cell::once::OnceCell<core::result::Result<addr2line::function::Functions<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>, gimli::read::Error>>>::get_or_init<<addr2line::function::LazyFunctions<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::borrow::{closure#0}>::{closure#0}, !> [instantiating crate: std] +0xefb` (call) |
| S18: Closed std: Function::parse c0 | `addr2line::function::InlinedFunctionAddress` | `<[addr2line::function::InlinedFunctionAddress]>::sort_by<<addr2line::function::Function<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::parse::{closure#0}>::{closure#0} [instantiating crate: std]` | 2499 | 1678 | 1e2bc0 / 1e2530 | `<core::cell::once::OnceCell<core::result::Result<addr2line::function::Function<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>, gimli::read::Error>>>::try_init::<<core::cell::once::OnceCell<core::result::Result<addr2line::function::Function<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>, gimli::read::Error>>>::get_or_init<<addr2line::function::LazyFunction<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::borrow::{closure#0}>::{closure#0}, !> [instantiating crate: std] +0x77d` (call) |
| S19: Closed std: Lines::parse c1 | `addr2line::line::LineSequence` | `<[addr2line::line::LineSequence]>::sort_by_key<u64, <addr2line::line::Lines>::parse<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>::{closure#1}>::{closure#0} [instantiating crate: std]` | 2547 | 1602 | 1de660 / 1de010 | `<core::cell::once::OnceCell<core::result::Result<addr2line::line::Lines, gimli::read::Error>>>::try_init::<<core::cell::once::OnceCell<core::result::Result<addr2line::line::Lines, gimli::read::Error>>>::get_or_init<<addr2line::line::LazyLines>::borrow<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>::{closure#0}>::{closure#0}, !> [instantiating crate: std] +0x1cc5` (call) |
| S20: Closed std: ResUnits::parse c4 | `addr2line::unit::UnitRange` | `<[addr2line::unit::UnitRange]>::sort_by_key<u64, <addr2line::unit::ResUnits<gimli::read::endian_slice::EndianSlice<gimli::endianity::LittleEndian>>>::parse::{closure#4}>::{closure#0} [instantiating crate: std]` | 2547 | 1602 | 1ea5c0 / 1e9f70 | `<std::backtrace_rs::symbolize::gimli::Context>::new +0x2bdf` (call) |
| S21: ST10 | `oxpinyin_core::vocab::PhraseEntry` | `<[oxpinyin_core::vocab::PhraseEntry]>::sort_by_key<u32, <oxpinyin_user::lookup::UserLookup>::from_store::{closure#0}>::{closure#0}` | 3037 | 1619 | 18d470 / 18cd90 | `<oxpinyin_user::lookup::UserLookup>::from_store +0x55bb` (call) |
| S22: ST07 | `oxpinyin_core::vocab::PhraseToken` | `<[oxpinyin_core::vocab::PhraseToken]>::sort_by_key<u32, <oxpinyin_runtime::RuntimeDict as oxpinyin_core::Dictionary>::tokens_for_text::{closure#3}>::{closure#0}` | 2526 | 1640 | 15fc60 / 15f5f0 | `<oxpinyin_runtime::RuntimeDict as oxpinyin_core::Dictionary>::tokens_for_text +0xf0f` (call) |
| S23: ST16 | `oxpinyin_data::row_format::pinyin_index::PinyinIndexItem` | `<[oxpinyin_data::row_format::pinyin_index::PinyinIndexItem]>::sort_by<oxpinyin_data::table_entries::pinyin_index_entries::{closure#0}>::{closure#0}` | 3659 | 2807 | f6100 / f5590 | `oxpinyin_data::table_entries::pinyin_index_entries +0x1890` (call) |
| S24: ST09 | `oxpinyin_engine::candidate::Candidate` | `<[oxpinyin_engine::candidate::Candidate]>::sort_by_key<u32, oxpinyin_engine::session::order_window_tail::{closure#0}>::{closure#0}` | 3496 | 1773 | 779f0 / 4e2a0 | `oxpinyin_engine::session::order_window_tail +0x3d5` (call) |
| S25: ST13 | `oxpinyin_user::store::PronRow` | `<[oxpinyin_user::store::PronRow]>::sort_by_key<u32, oxpinyin_user::store::sort_by_insertion<oxpinyin_user::store::PronRow, oxpinyin_user::store::collect_pronunciations_from_txn<oxpinyin_user::session_values::SessionTxn>::{closure#1}>::{closure#0}>::{closure#0} [instantiating crate: oxpinyin_facade]` | 2874 | 1605 | 109d50 / 109700 | `oxpinyin_user::store::collect_pronunciations_from_txn::<oxpinyin_user::session_values::SessionTxn> [instantiating crate: pinyin] +0x14d9` (call)<br>`<oxpinyin_user::store::GenericUserStore<oxpinyin_store::bdb::BdbStore>>::train_pronunciations::<&mut oxpinyin_engine::session::selection::TrainingReadings>::{closure#2} [instantiating crate: oxpinyin_facade] +0x2452` (call) |
| S26: ST03 | `u32` | `<[u32]>::sort_by_key<u32, <oxpinyin_data::dict::SystemDictionary>::checked_extension_tokens::{closure#1}>::{closure#0}` | 2526 | 1640 | f93f0 / f8d80 | `<oxpinyin_runtime::RuntimeDict as oxpinyin_core::Dictionary>::checked_extension_tokens +0x25c8` (call) |
| S27: ST08 | `u32` | `<[u32]>::sort_by_key<u32, <oxpinyin_runtime::RuntimeDict as oxpinyin_core::Dictionary>::checked_extension_tokens::{closure#2}>::{closure#0}` | 2526 | 1640 | 166280 / 165c10 | `<oxpinyin_runtime::RuntimeDict as oxpinyin_core::Dictionary>::checked_extension_tokens +0x25a0` (call) |
| S28: ST04 | `u32` | `<[u32]>::sort_by_key<u32, oxpinyin_facade::predict::compute_prefixes::{closure#2}>::{closure#0}` | 2526 | 1640 | 10c480 / 10be10 | `oxpinyin_facade::predict::compute_prefixes +0xd4e` (call) |
| S29: ST01 | `u32` | `<[u32]>::sort_by_key<u32, pinyin::iterators::phrase_table_tokens::{closure#3}>::{closure#0}` | 2526 | 1640 | a14b0 / a0e40 | `pinyin_iterator_add_phrase +0x170e` (call) |
| S30: Merge::finish_overreach processed / BTreeSet | `u32` | `<u32 as core::cmp::PartialOrd>::lt [instantiating crate: oxpinyin_user]` | 2146 | 1560 | 198140 / 197b20 | `<oxpinyin_user::replay::Merge>::finish_overreach::<alloc::borrow::Cow<str>> +0xd7b` (call) |
| **Total: 30 pairs / 60 bodies** | | | **84,529** | **51,251** | | |

### Direct source operations to correlate

`by_key(f)` below means the source uses `sort_by_key(f)`; the actual
comparator type also contains the standard slice adapter closure. The
extractor expression identifies the source site; the S matrix gives its
concrete compiler closure identity and instantiating-crate distinction. Types are:
`Candidate = oxpinyin_engine::candidate::Candidate`,
`PhraseEntry/PhraseToken = oxpinyin_core` types,
`PronRow/UserPronunciation = oxpinyin_user` types, and
`FileName = oxpinyin_data::table_info::FileName`.

| Source ID | Source item | Element type | Comparator / key rule | Tie conclusion |
| --- | --- | --- | --- | --- |
| ST01 | capi `iterators::phrase_table_tokens` | `u32` | `by_key(token >> 24)` | Ties possible and required: system then user rows; same-library distinct tokens and preserved duplicates. Keep encounter order. |
| ST02 | data `dict::resolve_suggestions` | `(u32, String)` | `by_key(token >> 24)` | Ties possible within every library. Cursor order within the library is the documented output law. |
| ST03 | data `SystemDictionary::checked_extension_tokens` | `u32` | `by_key(token >> 24)` | Same-library tokens from multiple extension records tie. Stable cursor/record order is retained. |
| ST04 | facade `predict::compute_prefixes` | `u32` | `by_key(token >> 24)` | Each suffix concatenates system then user tokens. Nibble is not a unique token ID; ties preserve that per-suffix order. |
| ST05 | runtime `merge_suggestion_rows`, first sort | `(u32, Vec<u8>)` | compare raw UCS-4 key bytes `a.1.cmp(&b.1)` | Multiple stored tokens may share one text key. Stored order within a key is retained before the next sort. |
| ST06 | runtime `merge_suggestion_rows`, second sort | `(u32, Vec<u8>)` | `by_key(token >> 24)` | Same-library ties deliberately retain the first sort's key order and the system/user concatenation order. |
| ST07 | runtime `Dictionary::tokens_for_text` | `PhraseToken` | `by_key(token.value() >> 24)` | Distinct tokens in one library tie; filtering does not make library IDs unique. Stable system/user ordering is retained. |
| ST08 | runtime `Dictionary::checked_extension_tokens` | `u32` | `by_key(token >> 24)` | The system extension walk plus user lookup can yield multiple tokens of one library. Stable encounter order is retained. |
| ST09 | engine `session::order_window_tail` | `Candidate` | token library key; `None` maps to `u32::MAX` | Same-library and no-token candidates can tie; the later first-token membership pass depends on encounter order. Stable requirement. |
| ST10 | user `UserLookup::from_store`, exact buckets | `PhraseEntry` | `by_key(entry.token().value())` | Two tone-distinct readings project to the same key and token before deduplication. Ties are expressly possible. No no-ties substitution is claimed even if a separate equal-value proof might be possible. |
| ST11 | user persistence `libraries_in_pin_order` | `(u8, &FileName, LibraryFile)` | `by_key(nibble)` | **No ties among distinct records on all current constructors**; detailed proof below. Source no-ties proof holds; C6 records this pair as S15. |
| ST12 | user store `collect_pronunciations_from_store` via `sort_by_insertion` | `(u32, UserPronunciation)` | sequence `row.0` | A no-ties proof is unavailable. `PronValue::decode` accepts legacy values with sequence MAX and does not validate distinct explicit sequences. Stable key order is the helper's specified tie law. |
| ST13 | user store `collect_pronunciations_from_txn` via `sort_by_insertion` | `PronRow` | sequence `row.value.seq` | Same sequence issue; a transaction's pending writes do not by themselves establish uniqueness over existing rows. Retain stable requirement. |
| ST14 | user store `collect_pronunciations_for_tokens`, per-token vectors | `(u32, UserPronunciation)` | sequence `row.0` | Same per-token sequence issue as ST12. Do not infer uniqueness merely from a unique table key. |
| ST15 | user `store_libpinyin::export_state` | `(u32, Vec<u16>, u64, bool)` (`StoredReading`) | sequence `row.0` | Export walks stored sequence values. No all-path sequence-uniqueness proof; keep stable table-key order for equal sequences. |
| ST16 | data `table_entries::pinyin_index_entries` | `PinyinIndexItem` | `exact_compare2(keys).then(token.cmp)` | The input slice can contain equal key/token records; incomplete/complete keyspace collision also concatenates records without uniqueness validation. No no-ties proof. Equal serialized values would require a different equivalence argument, not a claim that ties are impossible. |
| ST17 | store BDB `BdbStore::sorted_rows` | `(Vec<u8>, Vec<u8>)` (`Row`) | raw key `a.0.cmp(&b.0)` | Own-created DBs do not request duplicate keys, but an all-path proof for accepted existing DB_HASH files is unavailable: `open_at` does not explicitly establish/reject duplicate configuration. Keep stable until the accepted-file invariant is settled by source, not an exploratory probe. |

Each ST01–ST17 site has one retained S pair, as mapped in the matrix.
These are libpinyin source sites. The corresponding zhuyin iterator
uses its own facade closure in the other DSO; it is not charged as an
additional libpinyin instantiation merely because the source exists.

### Proof ST11: library-index equality is impossible

`libraries_in_pin_order` receives `&UserFileLayout`. Its fields are
private and its current construction paths are `stock`, `from_conf`, and
`Clone`; returned library/log slices are immutable. `stock` delegates to
`from_conf`. That constructor obtains both lists from one `TableConf`
default-row array. `TableConf::rows_of` zips that array with increasing
indices and filters by one `FileType`. Thus each index occurs at most
once in each list, and a row's one enum value cannot be both `SystemFile`
and `UserFile`. Removing index zero or nameless rows cannot add a
duplicate. Repeated parsed config records overwrite an array slot;
there is still one row at that index. Concatenating the two filtered
lists therefore has unique `u8` keys for every current valid Rust value
of `UserFileLayout`, including non-stock parsed configurations.

Consequently, equal comparator keys for distinct ST11 records cannot
occur, and a correct total-order unstable sort would produce the same
index sequence. The helper's comment anticipates a layout with a log
and chunk at one index, but current constructors cannot create that
state. Any future constructor/mutator allowing overlapping indices
would invalidate this proof. C6 identifies this as S15 (4,245 B of Q/D
bodies). That is a replacement footprint, not a net saving: unstable sorting may introduce
new code and has different runtime/storage behavior. No variant was
authorized or built.

### Implicit stable sorts in map collection

The installed Rust 1.97.1 `alloc/collections/btree/map.rs`,
`BTreeMap::from_iter`, first collects a `Vec<(K,V)>`, then uses a **stable**
key comparison before its private bulk builder. These implicit sites
are included in the thirty-body audit; grepping project `.sort*`
calls alone is not an inventory. Stable key ties preserve duplicate-key
value selection, so generic map-key uniqueness must not be assumed.

The S matrix and per-instantiation decision table below cover the six
retained BDB map sorts. The two `pinyin_chain::file_tails` map
collections in the source checkpoint are excluded from this BDB path by
`cfg(any(feature = "kyotocabinet", feature = "tkrzw"))` at the caller in
`persistence::load_for_open`. They are not extra libpinyin BDB bodies,
nor extra saving budgets for S02. Their other-backend behavior is outside
this measured artifact's thirty-pair audit.

An input-uniqueness proof does not expose a stable public switch for
BTreeMap's internal sort. Replacing the collection with another builder
is a different candidate with allocation/complexity/codegen costs; it
must not be presented as a one-line unstable-sort substitution or an
already measured saving.

### Source operations that must not inflate the thirty-body count

`RuntimeLm::has_real_unigrams` delegates to the concrete
`BigramLanguageModel`, whose implementation returns true. The fallback
sorts in engine `guess_over_remaining`/`lookup` and their core k-best /
`Scorer::rank_phrases` helpers are therefore not assumed to contribute
shipping bodies merely because their generic source contains sorts.
Their fallback cost sorts can tie (including saturated costs), so no
unstable replacement is inferred for other in-tree uses either. None of
those fallback cost sorts appears among the thirty identities.
Do not charge dead source paths to the retained families.

The two `sort_by_cached_key` calls in runtime `user_trellis_index` and
user `UserLookup::from_store` already use the pinned std algorithm that
sorts `(cached_key, original_index)` with `sort_unstable`. The index makes
that internal pair unique and preserves outer stable semantics even
when cached keys tie. They are not fresh drift/quicksort candidates.
Test-only sort calls likewise are not shipping-family evidence.

### Per-instantiation tie decisions, including the implicit sets

ST01–ST17 above supply a per-site decision for all seventeen direct
pairs. Equal library keys are not unique token IDs; equal sequence
numbers are not unique pronunciation keys. Neither a later dedup nor a
possible equal-value equivalence is a proof that comparator ties cannot
occur. In particular `PronValue::decode` accepts multiple legacy rows
with `seq = u32::MAX` and arbitrary explicit sequences; `next_pron_seq`
compacts when appending but does not prove all pre-existing reads unique.
The stable table-key tie law is expressly documented by
`sort_by_insertion`. No unstable replacement is proposed for S08,
S13/S14 or S25. The accepted-existing-file duplicate configuration for
S03 and the full-key/token uniqueness needed by S23 remain unproved;
retain stable sorting there under this request's no-ties criterion.

| S pair | Actual source operation and all relevant inputs | No-ties result / substitution limit |
| --- | --- | --- |
| S01 | `config::default_values`: only the fixed 69 `UPSTREAM_DEFAULTS` keys; the source query found 69 distinct keys; `to_owned` preserves equality | Proven unique. No user/config override is input to this collection. The stable sort is private inside BTreeMap collection; changing the builder is a separate runtime/allocation candidate, not a public unstable-sort switch. |
| S02 | `persistence::with_kept_keys`: current shipping callers are `stage_rest` and two `store::mask_out` compositions, each passing `pinyin_index_entries`; it emits exactly one row per raw key from its merged BTreeMap | Proven unique on these callers, even with repeated input readings or complete/incomplete-key collisions. `kept` entries are added after the collection and cannot invalidate its input proof. `pinyin_chain::file_tails` is cfg-excluded from this BDB call path. An alternative map builder still needs separate cost analysis. |
| S06 | Result collection in `collect_pronunciations_for_tokens`, reached from both UserLookup and runtime trellis construction | Proven unique: consumes a BTreeMap's distinct token keys once and carries each unchanged. Inner sequence ties in S13 do not duplicate outer keys. Both object roots already share this one pair. |
| S07 | BTreeSet `written` in `persistence::chewing_rows`, input pairs `(token, packed keys)` projected from the accumulated rows | No no-ties proof. Accepted chunk decoding/validation checks extents, not uniqueness of repeated pronunciation keys; `chewing_rows` can append the same pair twice before constructing `written`. This set removes duplicates. Value equivalence is not the requested impossible-ties proof. Retain the std implementation. |
| S11 | BTreeMap collection in `Merge::finish`, reached from `load_logs_with_mask` | Proven unique: `sort_touched` sorts/deduplicates, `expand_overlong` finishes with `sort_touched` again, and the typed loop emits at most one pair per slot. Arbitrary journal repetition cannot duplicate the final slot keys. Hidden std sort has no public unstable toggle. |
| S12 | Shared pair for `load_libraries` and kept-system-items collection in `GenericUserStore::open_session` | `load_libraries` alone is proven unique: framed payload length is u32, so the offset-array slot count is below 2^30; enumeration cannot wrap its u32 conversion. Repeated offsets do not repeat slot keys. The other caller forms `(nibble << 24) OR slot`; nested map uniqueness alone does not prove this projection injective without a complete high-slot-bit invariant. That proof is unavailable here. Retain this shared pair; changing only load_libraries cannot remove it. |
| S15 | ST11 `libraries_in_pin_order`, system-log/user lists from one TableConf default array | Proven unique across current `stock`, `from_conf`, Clone construction; detailed proof above. This is the one direct sort eligible under the no-ties criterion, but no unstable variant or net saving is measured. |
| S16 | Offsets BTreeMap in `seed_txn_with_replay<SessionTxn>`, both facade/runtime object roots | Proven unique: input is `library_totals.iter()` from a BTreeMap; map preserves each nibble once, filter only removes entries. Both generic callers already share the one sort pair. Hidden std sort cannot be switched independently on stable. |
| S30 | `processed: BTreeSet<u32>` at `Merge::finish_overreach`, called only from `finish_lazy` | Proven unique: `finish_lazy` first calls `sort_touched` (sort+dedup), performs no intervening touched-vector mutation on the overreach branch, then `finish_overreach` takes that vector and immediately collects it. Later inserts into processed occur after collection. Arbitrary repeated log entries therefore cannot make its initial input tie. The BTreeSet sort is private; no public unstable toggle. |
| S04/S17–S20 | Five std symbolizer sorts, with exact types and closure sites retained in the census | Closed by ruling. No further removal or tie analysis; no substitution or saving assigned. |

Pinned `alloc/collections/btree/set.rs`, `BTreeSet::from_iter`, first
collects a Vec, calls stable `inputs.sort()`, then a private sorted bulk
builder. Thus both S07 and S30 have `F = <E as PartialOrd>::lt`, rather
than a source closure. Sharing an ordinary comparison rule does not make
S07's tuple element equal to S30's u32. The two cached-key paths already
use an unstable internal sort with original indices, as noted above.

### Sharing groups and conditional savings

All figures in the next table refer only to the disjoint Q/D body
intervals in the S matrix. They exclude `driftsort_main`, small-sort
helpers, wrapper/call overhead, unwind metadata and final-file layout.
"Retain one" is an explicit unbuilt codegen assumption, not a claim that
LTO guarantees one body. Net file saving can be zero or negative if new
code/metadata exceeds retired code. A generic wrapper that still accepts
each distinct closure F cannot unify these instantiations.

| Candidate group | Exact semantics and mechanism needed | Conditional local Q/D estimate | Cost/risk and decision |
| --- | --- | --- | --- |
| S26–S29 (ST03/ST08/ST04/ST01) | Exactly `E=u32`, all compare the upper library byte, capture no caller state and require stable encounter order for equal bytes. One concrete out-of-line helper with its own fixed extractor can express all four sorts without changing their slice contents or tie law. | Current union 4 × 4,166 = 16,664 B. If four pairs become one pair of the current size, Q/D reduction estimate **12,498 B**, before added wrapper/call/other code. Conditional range 0–12,498 B under that fixed-size replacement assumption; net file delta unknown. | Medium. Compiler inlining/cloning could recreate copies; a new call can affect speed. This is a stable-sharing lead, never an unstable substitution. STOP before implementation and before accepting a speed/residency trade. |
| S13/S14 (ST14/ST12) | Exactly `E=(u32,UserPronunciation)`, both key on tuple field zero. One concrete stable sequence helper can replace the two distinct nested extractor closure types while preserving each caller's table-key tie order. S08/S25 are different element types. | Current union 9,158 B. Retain one current-sized pair: **4,579 B** Q/D reduction estimate, conditional 0–4,579 B before overhead; net file delta unknown. Both runtime/UserLookup consumers of S13 and both phrase bodies using S14 must route through it. | Medium. Borrowing/error behavior unchanged; no representation casts. Closure consolidation is semantically eligible, but emitted sharing and runtime effects remain unbuilt. STOP. |
| Above two groups together | Groups have different element types and disjoint intervals. Count their union once. | Conditional Q/D reduction estimate **17,077 B** (12,498 + 4,579), not a measured saving. No other row may charge any of S13/S14/S26–S29 again. | Same STOPs; no combined file-size promise. |
| S09/S10 (ST05/ST06) | Same tuple E but different rules: raw key bytes versus token library byte. One literal comparator is wrong. A uniform fn-pointer/dynamic comparator could carry either rule while sharing an algorithm; direct closure unification alone cannot. | Current union 8,919 B. If a common replacement occupied the larger current pair (4,640 B), illustrative retirement is **4,279 B** before indirect-comparison/wrapper overhead. New generic comparator code could be larger; no defensible positive net saving. | High. Indirect comparisons are a size/runtime trade; explicit human ruling required before this route. STOP. Not included in the 17,077 B estimate. |
| S02/S03 | Both E are `(Vec<u8>,Vec<u8>)`, both order raw keys and need stable equal-key treatment where ties are accepted. S02's F is std's private FromIterator closure; S03's is BDB's source closure. | Each pair is 5,139 B, but there is **no supported way to pass S03's comparator into std's private bulk builder**. No direct sharing saving assigned. | A construction rewrite could remove S02's hidden sort, but public incremental insertion has different build/allocation costs; collecting BDB rows through a map would also collapse duplicate rows. Do not present those as a semantics-preserving common-helper edit. STOP. |
| Direct unstable ST11/S15 | No-ties proof holds, so output order does not require stability. No other E matches its tuple. | Current pair 4,245 B is only the gross old-code footprint. Conditional old-body retirement 0–4,245 B, **minus unknown newly emitted unstable code and other effects**. No positive net estimate is established. | Algorithm/time/scratch-storage behavior changes. Separate variant and human runtime-trade ruling before implementation; STOP. |
| Unique-input implicit collections S01/S02/S06/S11/S16/S30 | No-ties proofs above, but stable std provides no switch to bypass each private stable sort/bulk builder. Replacing construction with insertion/another structure is distinct work. | Distinct current-pair union **26,827 B** (5,139 + 5,139 + 4,107 + 4,858 + 3,878 + 3,706). This is a gross footprint, not expected savings; replacement BTree code/calls can reverse it. No net estimate. S12 is excluded because it has a second unproved caller. | High for any builder/structure substitution: allocation, speed and complexity may change. No std patch, flag, dependency or implementation. STOP for a separate candidate/ruling. |
| All other pairs | Different E, comparison semantics or closed std ownership; similar layout/key width is insufficient. S06/S12/S14/S16/S25 already serve multiple object callers. | **0 B additional sharing assigned by this audit.** No per-caller duplicate budget for an already shared body; no claim that all remaining code is irreducible. | Keep current stable requirements or mark proof unavailable as above. No type erasure/casting proposed. |

The 26,827 B and 4,245 B gross footprints are separate alternatives,
not additions to a claimed 17,077 B measured result. Nor may the 135,780 B
family total be added to any subgroup estimate. These numbers answer
"which current Q/D bytes might be reduced under the stated assumption"; only
a separately authorized same-run variant can establish actual savings.

**STOP:** no candidate was implemented. Plain concrete stable-helper
sharing needs implementation authorization and codegen verification.
Comparator indirection, unstable-algorithm substitution or collection
builder/structure changes also need a human ruling if they trade size
for speed or residency. All profile/panic/linker/version-script STOPs
remain. The symbolizer removal question remains closed.

Source provenance: `git grep -n -E
'\.sort(_by_key|_by|_by_cached_key)?\(' -- crates/*/src`, followed by
reading the named item and its producers/constructors in the same tree.
The implicit collection rules and cached-key algorithm were read from
the pinned toolchain's installed source HTML, with the same `HTMLParser`
method described for source reads above, restricted to
`alloc/collections/btree/map.rs.html`, `alloc/collections/btree/set.rs.html`
and `alloc/slice.rs.html`.
The fixed-key source check extracts the `UPSTREAM_DEFAULTS` array with
`re.findall(r'^\s*\("([^"]+)", Seed::', source, re.M)` and compares the
key count to `len(set(keys))`; this is an audit query, not a CI gate.
C6 independently remeasured and reconciled the 135,780 B union to the
existing C4 totals; it does not measure a candidate delta.

Source paths for the named items are
`crates/oxpinyin-capi/src/iterators.rs`,
`crates/oxpinyin-data/src/dict.rs`,
`crates/oxpinyin-facade/src/predict.rs`,
`crates/oxpinyin-runtime/src/lib.rs`,
`crates/oxpinyin-engine/src/session/mod.rs`,
`crates/oxpinyin-user/src/lookup.rs`,
`crates/oxpinyin-user/src/persistence.rs`,
`crates/oxpinyin-user/src/store.rs`,
`crates/oxpinyin-user/src/store_libpinyin.rs`,
`crates/oxpinyin-data/src/table_entries.rs`,
`crates/oxpinyin-store/src/bdb/mod.rs`,
`crates/oxpinyin-data/src/table_info.rs`,
`crates/oxpinyin-data/src/user_files.rs`,
`crates/oxpinyin-data/src/chunk_write.rs`,
`crates/oxpinyin-user/src/replay.rs`,
`crates/oxpinyin-user/src/pinyin_chain.rs`, and
`crates/oxpinyin-engine/src/config.rs`. Reproduce these source reads with
`git show 5974c471:PATH`, then locate the item named in the tables.

## Candidate table — estimates, not measured reductions

The rows below exactly follow the six categories in #1084. Gross byte
ceilings describe current storage under consideration. They are not
promised net file reductions: a replacement may add code, retain the
original through other callers, change layout, or increase size. The
conditional estimates are not additive. No candidate variants were built,
so realized savings and effects on time/residency remain unmeasured.

| Issue candidate | Measured/source basis | Expected saving and uncertainty | Cost | Risk and STOP boundary |
| --- | --- | --- | --- | --- |
| Duplicate monomorphisation, including `read_with` | C4: two same-type `seed_txn_with_replay<SessionTxn>` bodies total 44,016 B per library; named `read_with` bodies are only 596/603 B. C6 reconciles thirty stable-sort pairs, identifies two semantically compatible concrete stable-helper groups, and separates five closed std pairs. Different element types do not establish duplication. `ReadStore::read_with`, `WriteTxn::read_with`, `SessionValues::read_with` and `SessionTxn::read_with` are the source leads. | A conditional **0–17,607 B of local `.text` per library** for the replay pair, only if the smaller body can be removed while reusing the larger unchanged; no such equivalence is established. New helper overhead or lost specialization can erase the saving. For sort sharing, two disjoint groups have a conditional 17,077 B Q/D reduction estimate if each group emits one current-sized pair; new code/metadata and runtime effects remain unknown. No measured net saving. A numeric `read_with` net estimate is unknown because its inlined contribution is not isolated. High uncertainty; estimates are not additive across competing routes. | Medium to high: compare specialized bodies and callers, preserve borrowing and transaction visibility, update in-tree callers and verify the relevant operations. | Reintroducing copies, allocations or indirection can lose speed or increase residency. No blanket type erasure. STOP before implementation; any time/residency trade needs a human ruling. |
| `core::fmt` and panic-location paths considered unreachable under `Result` APIs, including retained std symbolization | C5 splits core formatting into 8,740 B of mechanism/primitive formatting plus 4,453 B of core-type formatting, core panic helpers 304 B, alloc formatting 3,161 B and alloc failure handlers 65 B, each per DSO. Actual normal error callers, panic and independent OOM roots are traced above. Dedicated gimli/addr2line/demangle/miniz rows and the broader union are in the runtime candidate table. | Backtrace/symbolizer removal is **closed as not removable on the supported stable toolchain** by the 2026-10-23 ruling; no further removal work. Runtime backtrace selection does not remove the existing code. The remaining formatting footprints are not an approved saving budget. | No further symbolizer-removal work. The existing source/object evidence is retained; the stable-sort audit is completed below without an implementation. | No implementation. Profile, panic, linker, version-script and time/residency changes remain STOPs. |
| `.eh_frame` and unwind tables | C3/C4: `.eh_frame` 130,096/115,712 B; full `.eh_frame` + header + exception-table metadata 195,900/174,500 B. Release strategy remains default unwind; facade roots document the non-unwinding C boundary. | **0–195,900 B pinyin / 0–174,500 B zhuyin gross metadata ceiling**; complete removal is not expected or demonstrated. Net saving is unknown because some metadata remains necessary and text/layout also change. Historical abort results are not this build's estimate. High uncertainty. | High: establish unwind requirements and obtain new same-run size, speed and residency evidence for any separately authorized policy experiment. | Profile, panic and linker changes are STOPs. No unwind across the C ABI, no post-link deletion of required metadata, and no strategy change in this task. A previous size/time trade was reverted. |
| `opt-level` per crate or per function for cold code | C4: `UserLookup::from_store` is the largest retained body at 47,116 B per DSO; user-store open/save/load bodies are further source leads. Current release already uses fat LTO and one codegen unit. Source role is not measured workload coldness. | **Unknown, including direction**. No defensible numeric saving without a permitted variant and workload evidence; the size of a selected body is not a prediction of a different optimization level. Do not assume size-oriented optimization always shrinks it. | Medium to high: identify actual cold paths, isolate a permitted experiment and compare size plus runtime time/residency. | A `[profile.*]` change is a STOP. Function-level layout/inlining work also requires verification; a size-for-speed/residency trade is a STOP. No experiment or implementation here. |
| Exported symbols outside the libpinyin ABI (version script) | C2/C4: exact versioned defined export sets are 79 pinyin and 52 zhuyin ABI functions, with no extra defined export beyond the version-node carrier. Both scripts use `local: *;`, and the relinker verifies both directions. Dynamic symbol metadata totals 7,801/6,022 B, including strings, versions and hash indexes. | **0 B expected from pruning leaked defined exports: none were found.** The existing dynamic metadata is not a saving budget. Removing required exports is outside compatibility; undefined imports and the version-node carrier are not leaks. Low uncertainty for the audited objects. | Low for this completed audit; changing the supported export surface is not an internal cleanup. | Do not reason from cargo's separate unversioned cdylib. Version-script, linker and C ABI changes are STOPs. No such changes proposed for implementation. |
| `.rodata` tables that can be generated, shared or compressed | C4: largest named `ZHUYIN_PINYIN_MAP` is 17,916 B; `CONTENT_STRINGS` 10,584 B; `CONTENT_KEYS` 7,056 B per DSO. Current `StringPool`, `StringSpan` and `StringTable` already intern strings and store compact integer spans. | For the largest map alone, **0–17,916 B gross data ceiling per library**, with actual incremental saving unknown and necessarily below whole-array removal if the data remain represented. Decoder code and runtime scratch can offset or reverse a gain. `.rodata` totals are not attainable savings; neither is sharing between independently shipped DSOs free. High uncertainty. | Medium to high: inspect remaining representations and anonymous blobs, prove row/order identity, and account for decoder code, relocations and scratch memory. | Runtime generation/decompression may trade file bytes for startup time, allocation or residency; all such trades are STOPs. Preserve exact contents, row order and lookup semantics. STOP before implementation. |

Source items were read from the named build tree: `Cargo.toml`,
`crates/oxpinyin-store/src/lib.rs` (`ReadStore`, `WriteTxn`),
`crates/oxpinyin-user/src/session_values.rs` (`SessionValues`, `SessionTxn`),
`crates/oxpinyin-user/src/store.rs` (decoder call sites),
`crates/oxpinyin-user/src/phrase_table.rs` (`search`, `search_suggestion`),
`crates/oxpinyin-capi/src/context.rs` (`init_context`),
`crates/oxpinyin-runtime/src/lib.rs` (`OpenError::fmt`), both facade crate
roots and version scripts, `tools/packaging/relink-versioned.sh`,
`crates/oxpinyin-core/build.rs` (`generate_string_tables`),
`crates/oxpinyin-chewing/build.rs` (`main`),
`tools/codegen/string_tables.rs` (`StringPool`), and
`crates/oxpinyin-chewing/src/static_strings.rs` (`StringSpan`, `StringTable`).
Reproduce each source read with `git show 9ce07663:PATH`.
The [September study](perf-so-size-2026-09.md)'s corrected panic/symbolizer
sections are historical context only.

## Verification and limits

This is a findings-only diff. The requested release build and binary
inspection supply the attribution; they are not an unrelated completion
gate. The completion gate, including the completed C6 update, is AGENTS rule 7's "Docs, comments or findings
only": `just fmt --all --check` and `git diff --check` passed; then push. No Rust
unit tests, Clippy, compatibility differentials, sanitizer work, pin build,
KC/tkrzw build or speed/residency campaign ran for this documentation diff.
All three backend path checks below are empty; no peer-cell work is owed.

```sh
git diff --name-only origin/main...HEAD -- crates/oxpinyin-store/
git diff --name-only origin/main...HEAD -- ':!*.md' | xargs -r \
  grep -lE 'feature *= *"(bdb|tkrzw|kyotocabinet)"'
git diff -U0 origin/main...HEAD -- '*Cargo.toml' | \
  grep -E '^[+-][^+-].*(bdb|tkrzw|kyotocabinet)'
```

The C ABI, pinned behaviour, persistent data, profile and dependency set are
unchanged. A base/PR/pin performance comparison is not applicable: no
candidate exists, no before/after effect is claimed, and no absolute
number from another run is a baseline. The measured source is identified
above independently of the later documentation-only head. There is no
policy change or new size gate. Stop here before candidate implementation;
only a new human instruction clears the applicable STOPs.

## Command provenance

Run from a private checkout of the measured revision. The original build
used the environment below and one admitted shell,
`just heavy -- bash target/size-attribution-1084/campaign.sh`.
C1–C3 show its build, preservation and verification body. C4 was run
against those retained exact objects through normal `just heavy`
admission; fixing the zero-size CRT reconciliation and consolidating
facade labels required no rebuild. No raw command output was saved as
an evidence file. All subprocess text in C4 remains in memory and is
reduced to the requested plain-text tables.

### C1–C3: release, same-build preservation and section queries

```sh
source /home/sheng/.oxpinyin-oracle.env
export TMPDIR=/home/sheng/wt/1084/target/tmp/1084
export CARGO_TARGET_DIR=/home/sheng/wt/1084/target/size-attribution-1084/build
export CARGO_NET_OFFLINE=true
export SIZE_STAGE=/home/sheng/wt/1084/target/size-attribution-1084/stage
export SIZE_SYMBOLS=/home/sheng/wt/1084/target/size-attribution-1084/unstripped
mkdir -p "$TMPDIR" "$SIZE_SYMBOLS" target/size-attribution-1084/bin
cat > target/size-attribution-1084/bin/strip <<'STRIP'
#!/usr/bin/env bash
set -euo pipefail
for object in "$@"; do
  case "$object" in
    "$SIZE_STAGE/usr/lib/libpinyin.so.15.0.0"|"$SIZE_STAGE/usr/lib/libzhuyin.so.15.0.0")
      cp -- "$object" "$SIZE_SYMBOLS/${object##*/}" ;;
  esac
done
exec /usr/bin/strip "$@"
STRIP
chmod +x target/size-attribution-1084/bin/strip
export PATH="/home/sheng/wt/1084/target/size-attribution-1084/bin:/home/sheng/.cache/oxpinyin/fo-1018/cargo-c/bin:$PATH"
just heavy -- bash -eu -o pipefail <<'CAMPAIGN'
# C1: unchanged release recipe, including install.sh and relink-versioned.sh.
git rev-parse HEAD origin/main
date -u +%Y-%m-%dT%H:%M:%SZ
./tools/packaging/release-stage.sh bdb --prefix=/usr --libdir=/usr/lib \
  --dest="$SIZE_STAGE"
# C2: linked policy, file classification and identical stripping.
uv run --no-project --managed-python --python cpython@3.15+gil \
  python -B -X lazy_imports=normal tools/packaging/function_order.py \
  --arm subject --library "pinyin=$SIZE_SYMBOLS/libpinyin.so.15.0.0" \
  --library "zhuyin=$SIZE_SYMBOLS/libzhuyin.so.15.0.0"
for facade in pinyin zhuyin; do
  object="lib$facade.so.15.0.0"
  file "$SIZE_STAGE/usr/lib/$object" "$SIZE_SYMBOLS/$object"
  /usr/bin/strip --strip-unneeded -o "$TMPDIR/$object" "$SIZE_SYMBOLS/$object"
  cmp -- "$SIZE_STAGE/usr/lib/$object" "$TMPDIR/$object"
  rm -- "$TMPDIR/$object"
  # C3: regular-file bytes, dynamic dependencies and decimal sections.
  stat -c '%n %s' "$SIZE_STAGE/usr/lib/$object"
  readelf -dW "$SIZE_STAGE/usr/lib/$object" | grep -E 'NEEDED|SONAME'
  size -A -d "$SIZE_STAGE/usr/lib/$object"
done
CAMPAIGN
```

### C4: section, crate, symbol and generic-family aggregation

The following is the complete operational summarizer, embedded so the
figures do not depend on an uncommitted helper or deleted evidence path.
It parses `readelf -SW`, exercises `nm -S --size-sort -C`, pairs raw and
demangled unsorted records for exact identities, and independently checks
exports. Run after C1–C3; stdout is already summarized Markdown.

```sh
just heavy -- uv run --no-project --managed-python --python cpython@3.15+gil \
  python -B -X lazy_imports=normal - <<'PYTHON'
from collections import defaultdict
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys

root = Path.cwd()
base = root / 'target/size-attribution-1084'
helpers = runpy.run_path(str(root / 'tools/generate-function-order.py'))

def run(*args):
    return subprocess.check_output([str(x) for x in args], text=True,
                                   env={**os.environ, 'LC_ALL': 'C'})

def sections(path):
    result = {}
    for line in run('readelf', '-SW', path).splitlines():
        m = re.match(r'\s*\[\s*(\d+)\]\s+(\S+)\s+(\S+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s+([0-9a-f]+)\s', line)
        if m:
            index, name, kind, address, offset, size = m.groups()
            result[name] = (int(address, 16), int(size, 16), int(offset, 16), kind)
    return result

def records(text):
    result = []
    for line in text.splitlines():
        fields = [x.strip() for x in line.rsplit('|', 6)]
        if len(fields) == 7 and fields[3] in ('FUNC', 'OBJECT'):
            result.append(fields)
    return result

def crate(name):
    if name.startswith('pinyin_') or name.startswith('pinyin::'):
        return 'oxpinyin-capi (pinyin)'
    if name.startswith('zhuyin_') or name.startswith('zhuyin::'):
        return 'oxpinyin-zhuyin-capi (zhuyin)'
    if name.startswith(('__udiv', '__umod', '__div', '__mod', '__multi', '__float', '__fix', '__extend', '__trunc')):
        return 'compiler_builtins (C/asm names)'
    m = re.search(r'([A-Za-z_]\w*)::', name)
    owner = m.group(1) if m else 'C/CRT/unknown'
    return {'pinyin': 'oxpinyin-capi (pinyin)', 'zhuyin': 'oxpinyin-zhuyin-capi (zhuyin)'}.get(owner, owner)

def angle_end(text, start):
    depth = 0
    for index in range(start, len(text)):
        if text[index] == '<':
            depth += 1
        elif text[index] == '>' and (index == 0 or text[index-1] != '-'):
            depth -= 1
            if depth == 0:
                return index
    raise ValueError('unbalanced demangled generic: ' + text)

def erase_parameters(text):
    result, index = '', 0
    while index < len(text):
        if text[index] == '<':
            index = angle_end(text, index) + 1
            result += '<_>'
        else:
            result += text[index]
            index += 1
    return result

def family(name):
    name = re.sub(r' \[instantiating crate: [^]]+\]$', '', name)
    name = helpers['canonical'](name)
    if name.startswith('<'):
        end = angle_end(name, 0)
        receiver, suffix = name[1:end], name[end+1:]
        generic = '<' in receiver or '<' in suffix
        return '<' + erase_parameters(receiver) + '>' + erase_parameters(suffix), generic
    return erase_parameters(name), '<' in name

def md(value):
    return str(value).replace('|', '&#124;').replace('\n', ' ')

def table(headers, rows):
    print('| ' + ' | '.join(headers) + ' |')
    print('| ' + ' | '.join('---' for _ in headers) + ' |')
    for row in rows:
        print('| ' + ' | '.join(md(x) for x in row) + ' |')
    print()

libraries = {}
for facade in ('pinyin', 'zhuyin'):
    shipped = base / f'stage/usr/lib/lib{facade}.so.15.0.0'
    symbols = base / f'unstripped/lib{facade}.so.15.0.0'
    secs = sections(shipped)
    assert sections(symbols)['.text'][:2] == secs['.text'][:2]
    # Exercise the requested size-sorted, demangled view. Pair unsorted raw
    # and decoded records separately, because same-size name ties can reorder.
    ranked = records(run('nm', '-S', '--size-sort', '-C', '--defined-only', '--format=sysv', symbols))
    raw = records(run('nm', '-S', '--no-sort', '--no-demangle', '--defined-only', '--format=sysv', symbols))
    decoded = records(run('nm', '-S', '--no-sort', '-C', '--defined-only', '--format=sysv', symbols))
    assert len(raw) == len(decoded)
    bodies, data = defaultdict(set), defaultdict(set)
    for original, human in zip(raw, decoded, strict=True):
        assert original[1:] == human[1:]
        name, address, cls, kind, size, line, section = human
        if not address or not size:
            continue
        start, length = int(address, 16), int(size, 16)
        if length == 0:
            continue
        identity = helpers['stable_identity'](name, original[0])
        if kind == 'FUNC' and section == '.text':
            low, extent = secs['.text'][:2]
            assert low <= start < start + length <= low + extent
            bodies[(start, start + length)].add(identity)
        if kind == 'OBJECT' and section in ('.rodata', '.data.rel.ro'):
            data[(section, start, length)].add(identity)
    ranked_intervals = {(int(r[1], 16), int(r[1], 16) + int(r[4], 16)) for r in ranked
                        if r[3] == 'FUNC' and r[6] == '.text' and r[4] and int(r[4], 16)}
    # nm --size-sort infers sizes for some zero-size CRT records. Do not
    # attribute those inferred intervals as declared function bytes.
    zero_addresses = {int(r[1], 16) for r in decoded if r[3] == 'FUNC' and r[6] == '.text' and (not r[4] or int(r[4], 16) == 0)}
    assert set(bodies) <= ranked_intervals
    inferred = ranked_intervals - set(bodies)
    assert all(start in zero_addresses for start, end in inferred)
    ordered = sorted(bodies)
    overlaps = [(a, b) for a, b in zip(ordered, ordered[1:]) if a[1] > b[0]]
    if overlaps:
        raise ValueError('partial overlapping function bodies need explicit treatment: ' + str(overlaps[:3]))
    totals, families, named = defaultdict(int), defaultdict(list), defaultdict(int)
    aliases = 0
    for interval, names in bodies.items():
        size = interval[1] - interval[0]
        owners = {crate(n) for n in names}
        totals[next(iter(owners)) if len(owners) == 1 else 'shared/ambiguous aliases'] += size
        aliases += len(names) - 1
        fs = {(family(n)[0], family(n)[1]) for n in names}
        if len(fs) == 1:
            f, generic = next(iter(fs))
            if generic:
                families[f].append((size, tuple(sorted(names))))
        else:
            families['shared/ambiguous generic aliases'].append((size, tuple(sorted(names))))
        tests = {
            'read_with': lambda n: '::read_with' in n,
            'formatting-name envelope': lambda n: 'core::fmt::' in n or 'alloc::fmt::' in n,
            'panic/unwind-name envelope': lambda n: any(s in n for s in ('core::panicking::', 'std::panicking::', 'panic_unwind::', '_Unwind_', 'rust_eh_personality')),
            'symbolizer-name envelope': lambda n: any(s in n for s in ('gimli::', 'addr2line::', 'rustc_demangle::', 'std::backtrace', 'std::sys::backtrace')),
        }
        for label, predicate in tests.items():
            if any(predicate(n) for n in names):
                named[label] += size
    covered = sum(totals.values())
    assert covered <= secs['.text'][1]
    dynamic = run('nm', '-D', '--defined-only', symbols)
    definitions = [line.split()[-1] for line in dynamic.splitlines()]
    node = f'LIB{facade.upper()}'
    definitions = [n for n in definitions if n != node]
    expected = run('sed', '-n', '/global:/,/local:/p', root / f'crates/oxpinyin-{ "capi" if facade == "pinyin" else "zhuyin-capi"}/lib{facade}.ver')
    expected = {s.strip() for s in expected.replace('global:', '').replace('local:', '').split(';') if s.strip()}
    assert {n.split('@')[0] for n in definitions} == expected
    assert all(n.endswith('@@' + node) for n in definitions)
    libraries[facade] = dict(shipped=shipped, symbols=symbols, secs=secs,
                            bodies=bodies, totals=totals, families=families,
                            named=named, covered=covered, aliases=aliases,
                            exports=len(definitions), data=data, inferred=len(inferred))

print('\n## Measured file and section sizes\n')
print('All sizes below are bytes. S = stripped regular files; U = the exact same-build,')
print('versioned objects preserved immediately before stripping. Commands C1–C4 below')
print('produced these tables. Absent means no such ELF section, not an unmeasured zero.\n')
section_names = ['.text', '.rodata', '.data.rel.ro', '.eh_frame', '.eh_frame_hdr',
                 '.gcc_except_table', '.dynsym', '.dynstr', '.gnu.version',
                 '.gnu.version_d', '.gnu.version_r', '.gnu.hash', '.hash',
                 '.rela.dyn', '.rela.plt']
rows = [('stripped file (stat)', *(libraries[f]['shipped'].stat().st_size for f in libraries)),
        ('unstripped operational object (stat)', *(libraries[f]['symbols'].stat().st_size for f in libraries))]
for section in section_names:
    rows.append((section, *(libraries[f]['secs'].get(section, (0, 'absent'))[1] for f in libraries)))
for label, names in [('unwind metadata total', ['.eh_frame', '.eh_frame_hdr', '.gcc_except_table']),
                     ('dynamic symbol metadata total', ['.dynsym', '.dynstr', '.gnu.version', '.gnu.version_d', '.gnu.version_r', '.gnu.hash', '.hash'])]:
    rows.append((label, *(sum(libraries[f]['secs'].get(s, (0, 0))[1] for s in names) for f in libraries)))
table(['Quantity', 'libpinyin.so.15.0.0', 'libzhuyin.so.15.0.0'], rows)

print('## Retained `.text` attribution by crate\n')
print('C4 groups non-overlapping function intervals, deduplicating exact aliases.')
print('The first crate-qualified path in the demangled definition owns a body; for')
print('qualified impls this is the receiver crate, or the trait crate for primitive')
print('receivers. Unmangled ABI exports map to their facade. This counts surviving')
print('bodies after fat LTO, including inlined callees, not original source bytes.\n')
keys = set().union(*(d['totals'] for d in libraries.values()))
keys = sorted(keys, key=lambda k: (-sum(d['totals'].get(k, 0) for d in libraries.values()), k))
table(['Definition crate/group', 'pinyin bytes', 'zhuyin bytes'],
      [(k, *(libraries[f]['totals'].get(k, 0) for f in libraries)) for k in keys] +
      [('named body coverage', *(libraries[f]['covered'] for f in libraries)),
       ('.text gaps/padding/unnamed bytes', *(libraries[f]['secs']['.text'][1] - libraries[f]['covered'] for f in libraries)),
       ('.text total', *(libraries[f]['secs']['.text'][1] for f in libraries))])
table(['Reconciliation', 'pinyin', 'zhuyin'],
      [('unique nonzero body intervals', *(len(libraries[f]['bodies']) for f in libraries)),
       ('duplicate alias records not counted twice', *(libraries[f]['aliases'] for f in libraries)),
       ('nm size-sort inferred CRT intervals excluded', *(libraries[f]['inferred'] for f in libraries)),
       ('partial interval overlaps', 0, 0),
       ('defined ABI exports (version-node carrier excluded)', *(libraries[f]['exports'] for f in libraries))])

print('## Largest thirty `.text` bodies per library\n')
print('C4 sorts unique intervals by size descending, then address. Names retain')
print('generic arguments; the existing packaging helper adds a validated v0')
print('instantiating-crate annotation where GNU nm omits it. Hash suffixes are')
print('canonicalized by that helper. Aliases, if present, share one row.\n')
for facade, d in libraries.items():
    print(f'### lib{facade}\n')
    entries = sorted(d['bodies'].items(), key=lambda x: (-(x[0][1]-x[0][0]), x[0][0]))[:30]
    table(['Rank', 'Bytes', 'Demangled symbol'],
          [(i, interval[1]-interval[0], '<br>'.join('`'+n+'`' for n in sorted(names)))
           for i, (interval, names) in enumerate(entries, 1)])

print('## Top twenty generic families per library\n')
print('C4 removes concrete angle-bracket parameter lists while preserving outer')
print('qualified receiver/trait paths and method/closure names. It removes the')
print('instantiating-crate annotation for grouping. An outer receiver bracket alone')
print('does not make a non-generic method a generic family. Counts are retained')
print('bodies; totals are their current bytes, not removable bytes.\n')
for facade, d in libraries.items():
    print(f'### lib{facade}\n')
    entries = sorted(d['families'].items(), key=lambda x: (-sum(r[0] for r in x[1]), x[0]))[:20]
    table(['Family', 'Bodies', 'Bytes'], [(f'`{name}`', len(rows), sum(r[0] for r in rows)) for name, rows in entries])

print('## Candidate audit envelopes\n')
print('C4 selects unique intervals by the literal demangled-name predicates in the')
print('reproduction code. Envelopes can overlap and include inlined work. They')
print('neither prove reachability nor predict a candidate delta.\n')
table(['Named envelope', 'pinyin bytes', 'zhuyin bytes'],
      [(label, *(libraries[f]['named'][label] for f in libraries)) for label in
       ('read_with', 'formatting-name envelope', 'panic/unwind-name envelope', 'symbolizer-name envelope')])
print('### Largest named oxpinyin data objects\n')
print('C4 selects OBJECT records in `.rodata`/`.data.rel.ro` with an oxpinyin')
print('definition name. Anonymous blobs, merged literals and inlined constants')
print('need not have named records; this is not complete `.rodata` attribution.\n')
for facade, d in libraries.items():
    print(f'#### lib{facade}\n')
    entries = [(key, names) for key, names in d['data'].items() if any('oxpinyin_' in n for n in names)]
    entries.sort(key=lambda x: (-x[0][2], x[0][1]))
    table(['Section', 'Bytes', 'Named object'], [(key[0], key[2], '<br>'.join('`'+n+'`' for n in sorted(names))) for key, names in entries[:12]])
for facade, d in libraries.items():
    print(f'{facade}: file={d["shipped"].stat().st_size}; text={d["secs"][".text"][1]}; covered={d["covered"]}; envelopes={dict(d["named"])}', file=sys.stderr)
PYTHON
```

### C5: disjoint runtime accounting and object-reference evidence

The follow-up recreated C1–C3 on `b02c6293`, whose shipping source is
unchanged from `9ce07663`, because the original scratch objects had been
removed. C5 ran sequentially through normal heavy admission. It does not
request a linker map, change any linker option, install a package or
write raw disassembly/relocation files. Objdump, nm and readelf text is
held in memory; stdout contains only the summary tables above.

The reference model includes direct calls/branches, explicit function
addresses, and references through relative relocations. It labels these
separately and does not pretend to resolve every register-based call.
For the critical backtrace callback it checks the formatter's vtable
load, the actual relative relocation, `_Unwind_Backtrace` call and the
callback's `*0x20` dispatch. This is a static retention analysis; none of
these paths was executed as an exploratory failure probe.

```sh
just heavy -- uv run --no-project --managed-python --python cpython@3.15+gil \
  python -B -X lazy_imports=normal - <<'PYTHON'
from bisect import bisect_right
from collections import defaultdict, deque
import os
from pathlib import Path
import re
import subprocess
import sys

ROOT=Path.cwd()
BASE=ROOT/'target/size-attribution-1084'

def out(*args):
 return subprocess.check_output([str(v) for v in args],text=True,env={**os.environ,'LC_ALL':'C'})

def records(text):
 result=[]
 for line in text.splitlines():
  f=[v.strip() for v in line.rsplit('|',6)]
  if len(f)==7 and f[3]=='FUNC' and f[6]=='.text' and f[4] and int(f[4],16):
   result.append((int(f[1],16),int(f[4],16),f[0]))
 return result

def path(name):
 m=re.search(r'([A-Za-z_]\w*)::',name)
 return name[m.start():] if m else ''

def owner(name):
 return path(name).split('::',1)[0]

def fmt_impl(name):
 if not name.startswith('<'):return False
 depth=0
 for i,char in enumerate(name):
  if char=='<':depth+=1
  elif char=='>' and (i==0 or name[i-1]!='-'):depth-=1
  if depth==1 and name.startswith(' as core::fmt::',i):return True
  if depth==0:return False
 return False

def category(name):
 p=path(name); c=owner(name)
 if c=='core':
  if not p.startswith('core::fmt::') and fmt_impl(name):return c,'fmt implementations on core types'
  if p.startswith('core::panic::'):return c,'panic machinery'
  for stem,label in [('core::fmt::','formatting'),('core::panicking::','panic machinery'),('core::slice::sort::','sorting'),('core::ptr::drop_glue','drop glue'),('core::iter::','iterators'),('core::slice::','other slice operations'),('core::str::','strings/parsing'),('core::hash::','hashing'),('core::num::','numeric'),('core::cell::','cells/once initialization')]:
   if p.startswith(stem): return c,label
  return c,'other core'
 if c=='alloc':
  if p.startswith('alloc::fmt::') or fmt_impl(name): return c,'formatting / fmt trait implementations'
  if any(v in name for v in ('::capacity_overflow','alloc::alloc::handle_alloc_error','alloc::raw_vec::handle_error')): return c,'allocation/capacity failure handlers'
  for stem,label in [('alloc::collections::btree::','BTree collections'),('alloc::vec::','Vec'),('alloc::raw_vec::','raw Vec growth'),('alloc::string::','String'),('alloc::ffi::','CString'),('alloc::sync::','Arc'),('alloc::rc::','Rc'),('alloc::boxed::','Box'),('alloc::alloc::','allocation')]:
   if p.startswith(stem): return c,label
  return c,'other alloc'
 if c=='std':
  if p.startswith(('std::backtrace','std::sys::backtrace')): return c,'backtrace / symbolization'
  if p.startswith(('std::panicking::','std::panic::')): return c,'panic / panic hooks'
  if p.startswith('std::alloc::'): return c,'allocator / OOM hook'
  if fmt_impl(name): return c,'fmt implementations on std types'
  for stem,label in [('std::io::','I/O'),('std::fs::','filesystem'),('std::sys::fs::','filesystem'),('std::path::','paths'),('std::sync::','synchronization'),('std::sys::sync::','synchronization'),('std::thread::','threading')]:
   if p.startswith(stem): return c,label
  return c,'other std'
 return c,'other'

class Object:
 def __init__(self,facade):
  self.facade=facade; self.object=BASE/f'unstripped/lib{facade}.so.15.0.0'
  rows=records(out('nm','-S','--no-sort','-C','--defined-only','--format=sysv',self.object))
  self.bodies={start:dict(size=size,name=name) for start,size,name in rows}
  assert len(self.bodies)==len(rows)
  self.starts=sorted(self.bodies)
  self.edges=set(); self.forward=defaultdict(set); self.reverse=defaultdict(set)
  self.reloc={}
  for line in out('readelf','-rW',self.object).splitlines():
   m=re.fullmatch(r'\s*([0-9a-f]+)\s+[0-9a-f]+\s+R_X86_64_RELATIVE\s+([0-9a-f]+)\s*',line)
   if m:self.reloc[int(m[1],16)]=int(m[2],16)
  self.disasm=out('objdump','-d','-w','-C','--no-show-raw-insn','--section=.text',self.object)
  self.instructions={}
  for line in self.disasm.splitlines():
   m=re.match(r'\s*([0-9a-f]+):\s+(.+)',line)
   if not m:continue
   pc=int(m[1],16); instruction=m[2]; source=self.containing(pc)
   if source is None:continue
   self.instructions[pc]=instruction
   branch=re.match(r'(call|j[a-z]+)\s+([0-9a-f]+)\s+<',instruction)
   if branch:self.add(source,int(branch[2],16),pc,'call' if branch[1]=='call' else 'branch/tail')
   comment=re.search(r'#\s+([0-9a-f]+)\s+<',instruction)
   if comment:
    address=int(comment[1],16)
    if address in self.reloc:self.add(source,self.reloc[address],pc,'relocated function pointer')
    elif instruction.startswith(('lea','mov')):self.add(source,address,pc,'function address')
 def containing(self,address):
  i=bisect_right(self.starts,address)-1
  if i>=0:
   start=self.starts[i]
   if start<=address<start+self.bodies[start]['size']:return start
  return None
 def add(self,source,target,pc,kind):
  target=self.containing(target)
  if target is None or source==target:return
  edge=(source,target,pc,kind)
  self.edges.add(edge);self.forward[source].add(edge);self.reverse[target].add(edge)
 def name(self,start):return self.bodies[start]['name']
 def short(self,start):
  s=self.name(start)
  return s if len(s)<210 else s[:207]+'...'
 def route(self,source_predicate,target_predicate):
  roots=[s for s in self.starts if source_predicate(self.name(s))]
  q=deque(roots); previous={s:None for s in roots}
  while q:
   s=q.popleft()
   if target_predicate(self.name(s)):
    result=[]
    while previous[s] is not None:
     e=previous[s];result.append(e);s=e[0]
    return list(reversed(result))
   for edge in sorted(self.forward[s],key=lambda e:(e[2],e[1])):
    if edge[1] not in previous:previous[edge[1]]=edge;q.append(edge[1])
  return None

def print_route(o,edges):
 if edges is None:print('  NO RESOLVED ROUTE');return
 for source,target,pc,kind in edges:
  print(f'  {o.short(source)} +0x{pc-source:x} --{kind}--> {o.short(target)}')

def markdown_table(headers,rows):
 def cell(v):return str(v).replace('|','&#124;').replace('\n',' ')
 lines=['| '+' | '.join(headers)+' |','| '+' | '.join('---' for x in headers)+' |']
 lines += ['| '+' | '.join(cell(v) for v in row)+' |' for row in rows]
 return '\n'.join(lines)+'\n'

def runtime_report():
 objects={f:Object(f) for f in ('pinyin','zhuyin')}
 lines=['## Runtime retention and disjoint split (follow-up)\n',
 'C5 uses the recreated same-code objects, not an assumed Rust call graph. All figures are bytes of declared non-overlapping `.text` bodies. The split preserves C4\'s first-qualified-crate accounting; it does not recover pre-inlining source contribution. Categories are disjoint. `core::fmt` includes primitive fmt implementations; fmt implementations on other core-owned types are separate. Container bodies can contain failure branches that cannot be separated by symbol-size accounting.\n']
 splits={}
 for f,o in objects.items():
  d=defaultdict(lambda:[0,0])
  for start,body in o.bodies.items():
   c,k=category(body['name'])
   if c in ('core','alloc','std'):d[c,k][0]+=body['size'];d[c,k][1]+=1
  splits[f]=d
 lines.append(markdown_table(['Nonzero `.text` bodies matching name','pinyin','zhuyin'],[(word,*(sum(word in o.name(start) for start in o.starts) for o in objects.values())) for word in ('catch_unwind','rust_try')]))
 for crate in ('core','alloc','std'):
  keys=sorted({k for d in splits.values() for c,k in d if c==crate})
  rows=[(k,splits['pinyin'][crate,k][0],splits['zhuyin'][crate,k][0]) for k in keys]
  total=tuple(sum(splits[f][crate,k][0] for k in keys) for f in objects)
  assert total=={'core':(322796,298869),'alloc':(182884,175918),'std':(76366,76090)}[crate]
  rows.append(('total = previous crate bucket',*total))
  lines += [f'### `{crate}`\n',markdown_table(['Category','pinyin','zhuyin'],rows)]
 lines += ['### Direct component footprints and external callers\n',
 'Caller counts below are distinct source bodies outside the component; pair counts deduplicate repeated instructions to the same target body. They include direct control references and explicit function-address references. They are not runtime call counts. This enumerates static instruction references resolved by C5, not every possible indirect call; missing edges do not prove deadness.\n']
 rows=[]
 for c in ('gimli','addr2line','rustc_demangle','miniz_oxide'):
  sizes=[];counts=[]
  for f,o in objects.items():
   selected={s for s in o.starts if owner(o.name(s))==c}
   edges=[e for e in o.edges if e[1] in selected and e[0] not in selected]
   sizes.append(sum(o.bodies[s]['size'] for s in selected))
   counts.append((len({e[0] for e in edges}),len({e[:2] for e in edges})))
  assert counts[0]==counts[1]
  rows.append((c,*sizes,*counts[0]))
 lines.append(markdown_table(['Component','pinyin bytes','zhuyin bytes','outside callers per DSO','source/target pairs per DSO'],rows))
 rows=[]
 for f,o in objects.items():
  deps=sum(d['size'] for s,d in o.bodies.items() if owner(d['name']) in ('gimli','addr2line','rustc_demangle','miniz_oxide'))
  tagged=sum(d['size'] for s,d in o.bodies.items() if owner(d['name'])=='miniz_oxide' or any(t in d['name'] for t in ('gimli::','addr2line::','rustc_demangle::','std::backtrace','std::sys::backtrace')))
  rows.append((f,deps,tagged))
 lines += [markdown_table(['DSO','four component definition buckets','broader symbolizer-tagged body union including miniz'],rows),
 'The broader union includes std backtrace functions and type-specific core/alloc support, and can overlap formatting, containers and drop glue. It is an audit footprint, not a garbage-collection result or a proven saving.\n']
 # Exact component boundary callers; addresses distinguish same-demangled-name instances.
 for c in ('gimli','addr2line','rustc_demangle','miniz_oxide'):
  callers={}
  for f,o in objects.items():
   d=defaultdict(list)
   for a,b,pc,kind in o.edges:
    if owner(o.name(b))==c and owner(o.name(a))!=c:d[a].append((b,pc,kind))
   callers[f]=d
  p,z=objects['pinyin'],objects['zhuyin']
  ps,zs=sorted(callers['pinyin']),sorted(callers['zhuyin'])
  assert [p.name(s) for s in ps]==[z.name(s) for s in zs]
  lines.append(f'#### All resolved outside callers into `{c}`\n')
  lines.append(markdown_table(['Caller symbol','pinyin start','zhuyin start'],[(f'`{p.name(a)}`',hex(a),hex(b)) for a,b in zip(ps,zs,strict=True)]))
 # Selected actual references avoid falsely treating an anonymous vtable as the preceding named object.
 lines += ['### Concrete root and callback references\n',
 'Offsets in this table are relative to the named containing function. “Address” means the object stores/passes a function address, not that the instruction calls that function. The callback bridge below resolves the actual indirect dispatch.\n']
 def root(o,needle):
  matches=[s for s in o.starts if o.name(s)==needle]
  assert len(matches)==1,(needle,len(matches))
  return matches[0]
 rows=[]
 checks=[('std::panicking::panic_with_hook','DisplayBacktrace',0x555),('std::alloc::default_alloc_error_hook','DisplayBacktrace',0xc0),('std::alloc::default_alloc_error_hook','DisplayBacktrace',0xe8),('std::sys::backtrace::_print_fmt::{closure#1}','Cache',0xa0)]
 for name,needle,offset in checks:
  for f,o in objects.items():
   start=root(o,name);matches=[e for e in o.forward[start] if e[2]-start==offset and needle in o.name(e[1])]
   assert len(matches)==1,(name,f,offset)
   e=matches[0];rows.append((f,f'`{name}`',f'+0x{offset:x}',e[3],f'`{o.name(e[1])}`'))
 lines.append(markdown_table(['DSO','Source','offset','reference kind','Target'],rows))
 rows=[]
 for f,o in objects.items():
  fmt=next(s for s in o.starts if 'BacktraceLock' in o.name(s) and o.name(s).endswith('>::fmt'))
  ins=o.instructions[fmt+0x254];m=re.search(r'#\s+([0-9a-f]+)\s+<',ins);assert m
  base=int(m[1],16);callback=o.reloc[base+0x20]
  assert o.name(callback)=='std::sys::backtrace::_print_fmt::{closure#1}'
  trace=root(o,'std::backtrace_rs::backtrace::libunwind::trace::trace_fn')
  assert '*0x20(%rax)' in o.instructions[trace+0x1f]
  assert '_Unwind_Backtrace' in o.instructions[fmt+0x270]
  rows.append((f,hex(fmt),hex(base),hex(base+0x20),hex(callback),hex(trace)))
 lines.append(markdown_table(['DSO','DisplayBacktrace::fmt start','vtable loaded at +0x254','R_X86_64_RELATIVE slot (+0x20)','callback body','libunwind trace_fn'],rows))
 lines += ['`DisplayBacktrace::fmt+0x262` takes the `trace_fn` address; `+0x270` calls imported `_Unwind_Backtrace`. `trace_fn+0x1f` dispatches through `*0x20(%rax)` to the relocated callback. The callback then calls `Cache::with_global` at `+0xa0`. Objdump\'s nearest-preceding-object label for the anonymous vtable is not its ownership; the addresses and relocations above establish the connection.\n',
 '### ABI-to-runtime reference chains\n']
 for f,o in objects.items():
  rows=[]
  for label,predicate in [('allocation failure',lambda n:n=='std::alloc::default_alloc_error_hook'),('panic hook',lambda n:n=='std::panicking::panic_with_hook'),('C boundary abort path',lambda n:n=='core::panicking::panic_cannot_unwind'),('normal error formatting',lambda n:n=='core::fmt::write')]:
   source=f+'_init' if label=='normal error formatting' else f+'_alloc_instance'
   route=o.route(lambda n:n==source,predicate)
   assert route is not None,label
   chain='; '.join(f'`{o.name(a)}+0x{pc-a:x}` [{kind}] → `{o.name(b)}`' for a,b,pc,kind in route)
   rows.append((label,chain))
  lines += [f'#### lib{f}\n',markdown_table(['Entry path','Resolved references'],rows)]
 lines += ['### References from the symbolization callback into each component\n']
 p=objects['pinyin'];z=objects['zhuyin']
 for c in ('gimli','addr2line','rustc_demangle','miniz_oxide'):
  route=p.route(lambda n:n=='std::sys::backtrace::_print_fmt::{closure#1}',lambda n:owner(n)==c)
  other=z.route(lambda n:n=='std::sys::backtrace::_print_fmt::{closure#1}',lambda n:owner(n)==c)
  assert route is not None and other is not None
  first=[(p.name(a),p.name(b),pc-a,kind) for a,b,pc,kind in route]
  second=[(z.name(a),z.name(b),pc-a,kind) for a,b,pc,kind in other]
  assert first==second
  lines += [f'#### `{c}` (both libraries)\n',markdown_table(['Source','offset','kind','Target'],[(f'`{a}`',f'+0x{off:x}',kind,f'`{b}`') for a,b,off,kind in first])]
 lines += ['### Representative ordinary-use references\n']
 rows=[]
 p=objects['pinyin']
 for wanted in [('core','sorting'),('core','drop glue'),('core','iterators'),('alloc','BTree collections'),('alloc','Vec'),('alloc','raw Vec growth')]:
  refs=[e for e in p.edges if category(p.name(e[1]))==wanted and ('oxpinyin_' in path(p.name(e[0])).split('<',1)[0] or p.name(e[0]).startswith('pinyin_'))]
  assert refs,wanted
  a,b,pc,kind=min(refs,key=lambda e:e[2]);rows.append(('/'.join(wanted),f'`{p.name(a)}`',f'+0x{pc-a:x}',kind,f'`{p.name(b)}`'))
 lines.append(markdown_table(['Category','pinyin source','offset','kind','Target'],rows))
 return '\n'.join(lines)

if __name__=='__main__':
 print(runtime_report())
PYTHON
```

The source documentation read for the API-stability and hook-dispatch
statements is installed with the pinned toolchain under
`$(rustc --print sysroot)/share/doc/rust/html/src/std/`:
`panicking.rs.html` (`set_hook`, `default_hook`, `panic_with_hook`) and
`alloc.rs.html` (`set_alloc_error_hook`, `default_alloc_error_hook`,
`rust_oom`). The HTML source is read as text, without running examples.
The installed source and the object's own references are the primary
evidence; no different std version is substituted.

### C6: thirty stable-sort identities, declared bytes and caller roots

Run C1–C3 on shipping source `7829f247`, then execute the following through
normal admission. The script holds nm/objdump output in memory and emits only
the plain-text result matrix above. No object, raw disassembly, capture, log
or archive is committed or uploaded. The five std rows are enumerated for
reconciliation only; the closed symbolizer-removal question is not reopened.

The reconstructed operational objects and helper scripts are removed at lane
end. No retained artifact or raw-evidence download is available; the source,
build identities, commands and plain-text findings above are the record.

```sh
just heavy -- uv run --no-project --managed-python --python cpython@3.15+gil \
  python -B -X lazy_imports=normal - <<'PYTHON'
from bisect import bisect_right
from collections import defaultdict
from pathlib import Path
import os, re, runpy, subprocess
ROOT=Path.cwd()
OBJ=ROOT/'target/size-attribution-1084/unstripped/libpinyin.so.15.0.0'
helpers=runpy.run_path(str(ROOT/'tools/generate-function-order.py'))
def run(*a):return subprocess.check_output([str(x) for x in a],text=True,env={**os.environ,'LC_ALL':'C'})
def records(t):
 out=[]
 for line in t.splitlines():
  f=[s.strip() for s in line.rsplit('|',6)]
  if len(f)==7 and f[3]=='FUNC' and f[6]=='.text' and f[4] and int(f[4],16):out.append(f)
 return out
raw=records(run('nm','-S','--no-sort','--no-demangle','--defined-only','--format=sysv',OBJ))
dem=records(run('nm','-S','--no-sort','-C','--defined-only','--format=sysv',OBJ))
assert len(raw)==len(dem)
bodies={}
for r,d in zip(raw,dem,strict=True):
 assert r[1:]==d[1:]
 bodies[int(d[1],16)]=(int(d[4],16),helpers['stable_identity'](d[0],r[0]),r[0])
starts=sorted(bodies)
def containing(pc):
 i=bisect_right(starts,pc)-1
 if i>=0 and pc<starts[i]+bodies[starts[i]][0]:return starts[i]
 return None
sorts={}
for addr,(size,name,rawname) in bodies.items():
 for family in ('quicksort::quicksort','drift::sort'):
  prefix='core::slice::sort::stable::'+family+'::<'
  if name.startswith(prefix):
   key=name[len(prefix):]
   sorts.setdefault(key,{})['Q' if family.startswith('quick') else 'D']=(addr,size,name)
assert len(sorts)==30
assert all(set(v)=={'Q','D'} for v in sorts.values())
ordered=sorted(sorts.items(),key=lambda kv:kv[0])
ids={key:i for i,(key,_) in enumerate(ordered,1)}
targets={v[x][0]:(ids[k],x) for k,v in ordered for x in ('Q','D')}
edges=defaultdict(set)
for line in run('objdump','-d','-w','-C','--no-show-raw-insn','--section=.text',OBJ).splitlines():
 m=re.match(r'\s*([0-9a-f]+):\s+(call|j[a-z]+)\s+([0-9a-f]+)\s+<',line)
 if m:
  pc=int(m[1],16); dst=containing(int(m[3],16)); src=containing(pc)
  if dst is not None and src is not None and src!=dst:edges[dst].add((src,pc-src,m[2]))
def callers(target):
 todo=[target]; seen=set(todo); result=set()
 while todo:
  dst=todo.pop()
  for src,off,op in edges[dst]:
   if bodies[src][1].startswith('core::slice::sort::stable::'):
    if src not in seen:seen.add(src);todo.append(src)
   else:result.add((src,off,op))
 return sorted(result)
labels = [
 'config::default_values / BTreeMap', 'with_kept_keys / BTreeMap',
 'ST17', 'Closed std: ResUnits::parse c0', 'ST02',
 'collect_pronunciations_for_tokens result / BTreeMap',
 'chewing_rows written / BTreeSet', 'ST15', 'ST05', 'ST06',
 'Merge::finish / BTreeMap', 'load_libraries + open_session / BTreeMap',
 'ST14', 'ST12', 'ST11', 'seed_txn_with_replay offsets / BTreeMap',
 'Closed std: Functions::parse c1', 'Closed std: Function::parse c0',
 'Closed std: Lines::parse c1', 'Closed std: ResUnits::parse c4',
 'ST10', 'ST07', 'ST16', 'ST09', 'ST13', 'ST03', 'ST08', 'ST04', 'ST01',
 'Merge::finish_overreach processed / BTreeSet']
assert len(labels)==30
intervals=sorted((a,a+n) for _,v in ordered for a,n,_ in v.values())
assert len(set(intervals))==60
assert all(a[1]<=b[0] for a,b in zip(intervals,intervals[1:]))
ranked=records(run('nm','-S','--size-sort','-C','--defined-only','--format=sysv',OBJ))
assert set(a for a,b in intervals)<=set(int(r[1],16) for r in ranked)
assert sum(v['Q'][1] for _,v in ordered)==84529
assert sum(v['D'][1] for _,v in ordered)==51251

def md(s):return str(s).replace('|','&#124;').replace('\n',' ')
def split_type(k):
 text=k.split(' [instantiating crate:',1)[0][:-1]
 depth=0
 for i,c in enumerate(text):
  if c in '(<[':depth+=1
  elif c in ')>]':depth-=1
  elif c==',' and depth==0:return text[:i],text[i+2:]
 raise ValueError(text)
lines=['| ID / source site | Element E | Exact comparator F / instantiating annotation | Q B | D B | Q/D addresses (hex) | First outside call roots and instruction offsets |',
 '| --- | --- | --- | --- | --- | --- | --- |']
for (key,v),label in zip(ordered,labels,strict=True):
 i=ids[key]; e,f=split_type(key)
 annotation=' [instantiating crate:'+key.split(' [instantiating crate:',1)[1] if ' [instantiating crate:' in key else ''
 roots=sorted(set(callers(v['D'][0])+callers(v['Q'][0])))
 assert roots
 roottext='<br>'.join('`'+bodies[src][1]+f' +0x{off:x}` ({op})' for src,off,op in roots)
 cells=[f'S{i:02d}: {label}',f'`{e}`',f'`{f}{annotation}`',v['Q'][1],v['D'][1],f'{v["Q"][0]:x} / {v["D"][0]:x}',roottext]
 lines.append('| '+' | '.join(md(c) for c in cells)+' |')
lines.append('| **Total: 30 pairs / 60 bodies** | | | **84,529** | **51,251** | | |')
print('\n'.join(lines))

PYTHON
```
