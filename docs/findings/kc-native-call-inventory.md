# Kyoto Cabinet native-call inventory before the CXX migration

Recorded 2026-11-02T13:23:29Z (`date -u`). Status: source inventory and
proposal. It is not a binding implementation and it closes no
compatibility record.

This is the Kyoto Cabinet counterpart of the
[tkrzw native-call inventory](tkrzw-native-call-inventory.md). The
[Rust/C++ interoperability ruling](compatibility-policy.md#rust-cpp-interoperability)
freezes the Kyoto Cabinet FFI contract (`shim.h`, `shim.cc`, `ffi.rs`, and
the build and link configuration). It puts the CXX migration in a separate
PR after the tkrzw rebinding. This document is the prerequisite for that
PR. It makes no implementation decision. It changes no code, no register
row, no case disposition and no dependency. The same PR corrects two
statements of the ruling: the [FlexibleBigram failed-attach rule](compatibility-policy.md#flexiblebigram-failed-attach)
and the [KC migration scope](compatibility-policy.md#kc-migration-native-semantics).

Earlier Kyoto Cabinet records stay as they are. This document links them
and does not restate them:

- [Kyoto Cabinet compat backend](kyotocabinet-backend.md): the backend,
  its file names, and the `Send`/`Sync` verification.
- [Kyoto Cabinet through a C++ shim (#1118)](kc-shim-1118.md): why the shim
  constructs the classes directly, and its size and memory measurements.
- [Kyoto Cabinet adapters: C binding against a cxx bridge](kc-c-vs-cxx-adapter-2026-10-22.md):
  the earlier cxx experiment, its instruction counts and its verdict.

## Trees and completeness

The pin was read from a Git archive of libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`. No pin was executed. No test was
run to match behaviour. Source settled every item below.

Kyoto Cabinet citations name version 1.2.80. The headers were read from
Ubuntu's `libkyotocabinet-dev` 1.2.80-2build1 under `/usr/include`. The
oracle basis is Debian trixie `libkyotocabinet16v5` 1.2.80-2+b1
([version manifest](../../tools/oracle/version-manifest.json)). Both are
builds of the same 1.2.80 source. The
[GLib/libc verification record](glib-libc-verification.md) states that the
Debian development package installs `kcdb.h` and `kcstashdb.h` byte-identical
to the original tarball. The line numbers that `ffi.rs` already cites
(`kcplantdb.h:1050,1593,1645`, `kchashdb.h:559,936,973`) agree with the headers
read here. Kyoto Cabinet has no maintained public source repository, so its
citations are plain `file:line` text, as in the existing KC findings.

`DB::Visitor::NOP` and `DB::Visitor::REMOVE` are declared in `kcdb.h:44,46`
but defined in the library, not in a header. Their values were read from the
shipped library, not from a run: `nm -D -C` on
`/usr/lib/x86_64-linux-gnu/libkyotocabinet.so.16.14.0` gives `REMOVE` at
`.rodata` 0xc0af0 and `NOP` at 0xc0af8, and `objdump -s` of those 16 bytes
gives the little-endian values 1 and 0.

The Rust baseline is main `5a92558c10606c7ceb13c16b467f3d72d876d6ba`.
Unqualified Rust paths and item names below refer to that tree. It contains
the tkrzw rebinding [#1545](https://github.com/shenghaoc/oxpinyin/pull/1545)
(`cc266b7a`) and [#1550](https://github.com/shenghaoc/oxpinyin/pull/1550)
(`5a92558c`). The open [#1552](https://github.com/shenghaoc/oxpinyin/pull/1552)
is examined separately at its head `bbe1ec62f422c5fb12f39a13743b17601412b8f3`.

The census covers all ten Kyoto Cabinet pin files: the four
`chewing_large_table2`, `phrase_large_table3`, `ngram` and `punct_table`
`_kyotodb.cpp`/`.h` pairs, `flexible_ngram_kyotodb.h` and `kyotodb_utils.h`.
The four ordinary headers hold only the owning `kyotocabinet::BasicDB*`
member and the destructor that calls `reset()`
([src/storage/chewing_large_table2_kyotodb.h:35-48](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.h#L35-L48),
[src/storage/chewing_large_table2_kyotodb.h:96-98](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.h#L96-L98),
[src/storage/phrase_large_table3_kyotodb.h:32-47](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.h#L32-L47),
[src/storage/ngram_kyotodb.h:37-44](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.h#L37-L44),
[src/storage/punct_table_kyotodb.h:30-45](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.h#L30-L45)). The other six files hold every
native operation below.

Selection and build wiring:
[configure.ac:112-121](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/configure.ac#L112-L121), [src/Makefile.am:69-73](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/Makefile.am#L69-L73),
[src/storage/Makefile.am:100-104](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/Makefile.am#L100-L104), and the selectors at
[src/storage/chewing_large_table2.h:33-34](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2.h#L33-L34),
[src/storage/phrase_large_table3.h:33-34](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3.h#L33-L34), [src/storage/ngram.h:33-34](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram.h#L33-L34),
[src/storage/punct_table.h:33-34](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table.h#L33-L34), [src/storage/flexible_ngram.h:38-39](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram.h#L38-L39).
The facade, utility and test callers are the same as for tkrzw. The
[tkrzw inventory's utilities table](tkrzw-native-call-inventory.md#utilities-tests-and-facade-reachability)
lists them. They add no Kyoto Cabinet class or native entry point.

To repeat the census in a pin checkout:

```sh
git grep -n -E 'kyotocabinet::|using namespace kyotocabinet|BasicDB::|DB::Visitor|empty_vbuf' 074a2219 -- src utils tests
git grep -n -E 'm_db->|cursor->|new (TreeDB|HashDB|StashDB|ProtoTreeDB)|unlink' 074a2219 -- 'src/storage/*kyoto*'
```

## Concrete classes, options and tuning

The pin never constructs `PolyDB`, `CacheDB`, `GrassDB`, `ProtoHashDB`,
`DirDB` or `ForestDB`. Its members have static type `BasicDB*`. The
allocation fixes the concrete class. No pin call sets a tuning parameter,
a comparator, a logger, a meta trigger, a progress checker or a file
processor. Every `synchronize()` is the default `(hard=false, proc=NULL,
checker=NULL)`. `#include <kccachedb.h>` and `#include <kcprotodb.h>` bring
headers in, but no `CacheDB` is constructed.

| Pin owner and use | Concrete allocation and mode | Current Rust route |
| --- | --- | --- |
| Chewing, phrase and punctuation construction ([src/storage/chewing_large_table2_kyotodb.cpp:56-63](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L56-L63), [src/storage/phrase_large_table3_kyotodb.cpp:68-74](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L68-L74), [src/storage/punct_table_kyotodb.cpp:29-35](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L29-L35)) | `new ProtoTreeDB`, `open("-", OREADER\|OWRITER\|OCREATE)` under `check_result`. | `WriteStore::create_in_memory` → `Db::open_proto_tree`: `oxkc_db_new(OXKC_PROTO_TREE)`, then `oxkc_db_open("+", OWRITER\|OCREATE)`. See [other differences](#other-differences-found). |
| Chewing, phrase and punctuation `load_db` ([src/storage/chewing_large_table2_kyotodb.cpp:94-122](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L94-L122), [src/storage/phrase_large_table3_kyotodb.cpp:109-137](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L109-L137), [src/storage/punct_table_kyotodb.cpp:69-97](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L69-L97)) | `new ProtoTreeDB`, open `"-"` as above, then `load_snapshot(filename, NULL)`. | `RawReadStore::open_user_index`: `open_proto_tree`, `oxkc_db_load_snapshot`, then a Rust read-only seal. |
| Chewing, phrase and punctuation `attach` ([src/storage/chewing_large_table2_kyotodb.cpp:77-90](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L77-L90), [src/storage/phrase_large_table3_kyotodb.cpp:92-105](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L92-L105), [src/storage/punct_table_kyotodb.cpp:52-65](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L52-L65)) | `new TreeDB`, `open(dbfile, attach_options(flags))`. | System files: `open_system(path, false)` → `Db::open_retained(Tree, read_only=true)`. Tools: `Db::open(Tree, …)`. |
| Bigram `load_db` ([src/storage/ngram_kyotodb.cpp:54-80](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L54-L80)) | `new StashDB`, `open("-", OREADER\|OWRITER\|OCREATE)` (not checked by an assert), then `load_snapshot`. | `KcUserBigramDb::load_db` → `Db::open_stash`: `oxkc_db_open("-", OWRITER\|OCREATE)`, then `oxkc_db_load_snapshot` with its result ignored. `open_user_bigram` uses the same container. |
| Bigram `attach` ([src/storage/ngram_kyotodb.cpp:110-120](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L110-L120)) | `new HashDB`, `open(dbfile, attach_options(flags))`. The constructor leaves `m_db` null ([src/storage/ngram_kyotodb.cpp:35-37](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L35-L37)). | System file: `open_system(path, true)`. Tools: `open_hash_read_only`, `create_hash`. |
| FlexibleBigram `attach` ([src/storage/flexible_ngram_kyotodb.h:129-180](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L129-L180)) | `new HashDB`. First `open` without `OCREATE`; on failure with `ATTACH_CREATE`, a second `open` of the same object with `OCREATE`. | `FlexibleBigram::open` → `WriteStore::create_hash`: one `open(OWRITER\|OCREATE)`. `open_read_only` → `open_hash_read_only`. |

`attach_options` maps `ATTACH_READONLY` to `OREADER`, `ATTACH_READWRITE` to
`OREADER|OWRITER` after it asserts that READONLY is absent, and
`ATTACH_CREATE` to `OCREATE` ([src/storage/kyotodb_utils.h:32-45](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/kyotodb_utils.h#L32-L45)).
FlexibleBigram repeats the first two lines and leaves `OCREATE` out of the
first open ([src/storage/flexible_ngram_kyotodb.h:131-138](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L131-L138)). The Rust
`Db::open_retained` passes `OREADER` for read-only, `OWRITER|OCREATE` for
create, and `OWRITER` otherwise. Kyoto Cabinet ignores `OREADER` when
`OWRITER` is present: `HashDB::open` tests only `OWRITER`
(`kchashdb.h:799-822`), `PlantDB::open` does the same (`kcplantdb.h:1491-1520`),
and `ProtoDB`/`StashDB` store the mode but test only `OWRITER`
(`kcprotodb.h:750-763`, `kcstashdb.h:640-660`). The missing `OREADER` bit
changes no behaviour.

Defaults at 1.2.80, which the pin keeps because it tunes nothing: HashDB
alignment power 3, free-block pool power 10, 1048583 buckets, 64 MiB mapped
region (`kchashdb.h:101-112`). TreeDB is `PlantDB<HashDB, TYPETREE>`
(`kchashdb.h:3858`) with alignment power 8, free-block pool power 10, 65536
buckets, page size 8192 and a 64 MiB page cache (`kcplantdb.h:75-85`). Its
comparator is `LEXICALCOMP` when none is tuned (`kcplantdb.h:1552`).
ProtoTreeDB is `ProtoDB<StringTreeMap, TYPEPTREE>` (`kcprotodb.h:1351`).
StashDB has 1048583 buckets (`kcstashdb.h:60`). Do not copy these values into
Rust constants. Construct the concrete class and call its `open`, as the pin
does. For an existing file, stored metadata governs.

The shim adds `oxkc_db_tune_buckets`. Only the `bench-internal` feature
reaches it (`Db::open_with_tuning` with a non-empty `extra_tuning`). The
shipping path never calls it.

## Call-site map: pin → shim → difference

Rust abbreviations refer to these files and their named items. They are not
a stable Rust API:

- **SHIM**: [`shim.h`](../../crates/oxpinyin-store/src/kyotocabinet/shim.h) and
  [`shim.cc`](../../crates/oxpinyin-store/src/kyotocabinet/shim.cc), the C entry
  points `oxkc_*` and the `Visitor` adapter.
- **FFI**: [`ffi.rs`](../../crates/oxpinyin-store/src/kyotocabinet/ffi.rs), `Db`,
  `Cursor`, the callbacks `visit_full`/`visit_empty`, and `c_ptr`. The
  declarations are generated by bindgen from `shim.h` through
  [`wrapper.h`](../../crates/oxpinyin-store/src/kyotocabinet/wrapper.h) in
  [`store/build.rs`](../../crates/oxpinyin-store/build.rs).
- **KC**: [`kyotocabinet/mod.rs`](../../crates/oxpinyin-store/src/kyotocabinet/mod.rs),
  `KcStore`, `KcTxn`, `KcUserBigramDb`, `KcUnopenedHandle`, `open_system`.
- **ATTACH**: [`system_attach.rs`](../../crates/oxpinyin-store/src/system_attach.rs),
  `SystemAttachment`, `UnopenedSystemDbm`.
- **FLEX**: [`flexible_ngram.rs`](../../crates/oxpinyin-store/src/flexible_ngram.rs),
  `FlexibleBigram`. **STORE**: [`store/lib.rs`](../../crates/oxpinyin-store/src/lib.rs),
  `WriteStore::write_deferred` and `flush`.
- **CHEW**: [`data/chewing_table.rs`](../../crates/oxpinyin-data/src/chewing_table.rs),
  the `DbmState` adapter. **USER**:
  [`user/persistence.rs`](../../crates/oxpinyin-user/src/persistence.rs),
  `load_user_pinyin_index`, `load_user_phrase_index`, `load_user_bigram_db`.

The SHIM exports these native operations: construct, tune buckets, open,
close, delete, error code and message, `accept`, `check`, `iterate`, `set`,
`remove`, `synchronize`, `load_snapshot`, `dump_snapshot`, `count`,
`begin_transaction`, `end_transaction`, cursor create and delete, cursor
`jump()`, `jump(key)`, `accept(step)` and cursor error code. It has no
`get(kbuf, ksiz, vbuf, max)`, no `get(kbuf, ksiz, &sp)`, no cursor `step`,
`get_key` or `get_value`. The rows below state each substitution.

### Shared helpers and lifecycle

| Pin call site and arguments | Pin handling and lifetime | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/kyotodb_utils.h:49-50](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/kyotodb_utils.h#L49-L50): `empty_vbuf = (char*)UINTPTR_MAX` for a zero-length `set` | A value pointer that is neither `NOP` (0) nor `REMOVE` (1). See [sentinels](#sentinels-and-the-empty-value). | FFI `c_ptr` passes the address of a static byte for every empty slice. |
| [src/storage/kyotodb_utils.h:52-72](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/kyotodb_utils.h#L52-L72): `CopyVisitor` | `#if 0`. Every `load_db`/`save_db` also keeps its copy loop in `#if 0` ([src/storage/chewing_large_table2_kyotodb.cpp:108-119](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L108-L119), [src/storage/ngram_kyotodb.cpp:66-77](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L66-L77)). | No counterpart. No native call. |
| Chewing, phrase and punctuation `reset` ([src/storage/chewing_large_table2_kyotodb.cpp:65-74](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L65-L74), [src/storage/phrase_large_table3_kyotodb.cpp:76-88](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L76-L88), [src/storage/punct_table_kyotodb.cpp:37-49](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L37-L49)) | If `m_db` is not null: `synchronize()`, `close()`, `delete`, null. Both answers ignored. Runs from the destructor and before `attach`/`load_db`. | `Db::drop`: `oxkc_db_close`, then `oxkc_db_del`. No `synchronize`, except on `KcUnopenedHandle` (#1535). See [the #1552 checks](#the-two-1552-checks-on-kyoto-cabinet). |
| Bigram `reset` ([src/storage/ngram_kyotodb.cpp:43-50](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L43-L50)) | Same three calls as above, `synchronize()` included. The tkrzw Bigram reset has no sync; the KC one has. | Same as above. |
| FlexibleBigram `reset` ([src/storage/flexible_ngram_kyotodb.h:86-92](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L86-L92)) | `synchronize()`, `close()`, then `m_db = NULL` **with no `delete`**. The object is not freed. | FLEX `close` calls `flush`; KC inherits the STORE no-op `flush`. `Db::drop` then closes and deletes. No native `synchronize`. |

### Chewing index (`chewing_large_table2_kyotodb.cpp`)

| Pin call site and arguments | Status handling | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/chewing_large_table2_kyotodb.cpp:77-90](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L77-L90): `reset`, new `TreeDB`, `open(dbfile, mode)` | Null path returns false after reset. The bool is returned. A failed object stays owned. | ATTACH `SystemAttachment::open` → `open_system` keeps the object (#1535). Tools use `Db::open`, which drops a failed object. |
| [src/storage/chewing_large_table2_kyotodb.cpp:94-122](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L94-L122): `reset`, new `ProtoTreeDB`, open `"-"`, `load_snapshot(filename, NULL)` | Open or load failure returns false. The ProtoTreeDB stays owned with every record loaded before the failure (`BasicDB::load_snapshot` sets record by record, `kcdb.h:2339-2418`). | USER `load_user_pinyin_index` → `open_user_index`. A load failure is `Err`; the partly loaded container is dropped and the index is skipped. See [other differences](#other-differences-found). |
| [src/storage/chewing_large_table2_kyotodb.cpp:124-146](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L124-L146): `unlink(new_filename)`, `dump_snapshot(new_filename, NULL)` | `unlink` failure other than `ENOENT` returns false. No null-member guard before `m_db->dump_snapshot`. No sync. | KC `write_user_index`: `remove_if_present`, a new ProtoTreeDB, one `oxkc_db_set` per row, `oxkc_db_dump_snapshot`. The pin dumps the container it holds; Rust fills a fresh container from Rust state. |
| [src/storage/chewing_large_table2_kyotodb.cpp:148-179](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L148-L179): `check(index)`, then `get(index, vbuf, vsiz)` | `-1` → `SEARCH_NONE`. `0` → `SEARCH_CONTINUED` with no `get`. Otherwise `check_result(vsiz == get(...))`. | CHEW `get` → `get_raw` → `Db::get`: one read-only `oxkc_db_accept` with a copying visitor. One native `accept` instead of the pin's two (`check`, `get`), also for a zero-length value. A failed object takes `UnopenedSystemDbm::read` → `check` only, which matches the pin. |
| [src/storage/chewing_large_table2_kyotodb.cpp:277-338](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L277-L338): `check`; `set` new entry; prefix loop `check`, `set(kbuf, ksiz, empty_vbuf, 0)`; `get`; `set` | Any `check` of -1 enters the new-entry branch. A prefix `check` other than -1 stops the loop. A `set` failure → `ERROR_FILE_CORRUPTION`. | System tables are read-only. Tools and USER change Rust state. Writes cross as `oxkc_db_set` with `c_ptr`, inside `KcStore::write` (`begin_transaction`, `end_transaction`, `synchronize(false)`). |
| [src/storage/chewing_large_table2_kyotodb.cpp:375-409](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L375-L409): `check`, `get`, `set` | `check` below `sizeof(phrase_token_t)` → `ERROR_REMOVE_ITEM_DONOT_EXISTS`. No native `remove` of a row that becomes empty. | As above. Keep an empty row distinct from a removed row. |
| [src/storage/chewing_large_table2_kyotodb.cpp:446-521](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L446-L521): stack `MaskOutVisitor2`; `iterate(&visitor, true)`; `synchronize()` | No null-member check. Both answers ignored; returns true. `visit_full` returns the entry chunk with `*sp` set; an invalid length aborts. `visit_empty` returns `NOP`. | USER masks Rust state. No writable `iterate` crosses the SHIM: FFI always passes `writable=0`. |
| [src/storage/chewing_large_table2_kyotodb.cpp:524-590](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L524-L590): null check; `check(prefix)`; `cursor()`; `jump(prefix)`; `step()`; `get_key`; loop: `get_value`, `delete[]` both, `step()`, `get_key` | `check` -1, `jump` false or first `step` false → `SEARCH_NONE`. A loop `step` false returns the result. `get_key` and `get_value` allocate with `new[]`; the pin frees them. No null check on `cursor()`. | CHEW `contains` → `Db::check`, then `walk` → `KcStore::walk_raw`: `oxkc_db_cursor`, `oxkc_cur_jump_key`, then `oxkc_cur_accept(step=1)` per record. One native call per record instead of three. The pin steps past the prefix record before its first read; the Rust walk starts its reads at the prefix record. No Kyoto Cabinet allocation reaches Rust. |

### Phrase index (`phrase_large_table3_kyotodb.cpp`)

| Pin call site and arguments | Status handling | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/phrase_large_table3_kyotodb.cpp:92-105](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L92-L105): `reset`, new `TreeDB`, `open` | As chewing attach. | As chewing attach. |
| [src/storage/phrase_large_table3_kyotodb.cpp:109-137](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L109-L137): new `ProtoTreeDB`, open `"-"`, `load_snapshot` | As chewing `load_db`. | USER `load_user_phrase_index` → `open_user_index`, as chewing. |
| [src/storage/phrase_large_table3_kyotodb.cpp:139-161](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L139-L161): `unlink`, `dump_snapshot` | As chewing `save_db`, no null guard. | KC `write_user_index`, as chewing. |
| [src/storage/phrase_large_table3_kyotodb.cpp:164-193](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L164-L193): null check; `check`; `get` | -1 → `SEARCH_NONE`; 0 → `SEARCH_CONTINUED`; `check_result` on the `get` size. | CHEW raw adapter → `Db::get`, one `accept`. |
| [src/storage/phrase_large_table3_kyotodb.cpp:195-252](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L195-L252): `check`, cursor `jump`, `step`, `get_key`/`get_value` loop | As chewing suggestion. | As chewing suggestion. |
| [src/storage/phrase_large_table3_kyotodb.cpp:255-313](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L255-L313): `check`, `set`, prefix `check`/`set(empty_vbuf, 0)`, `get`, `set` | As chewing `add_index`; asserts a non-null `m_db`. | Tools and USER, as chewing. |
| [src/storage/phrase_large_table3_kyotodb.cpp:315-346](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L315-L346): `check`, `get`, `set` | As chewing `remove_index`. | As chewing. |
| [src/storage/phrase_large_table3_kyotodb.cpp:349-385](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L349-L385): `MaskOutVisitor`; `iterate(true)`; `synchronize()` | As chewing `mask_out`; the visitor owns one `PhraseTableEntry` whose chunk it returns. | USER masks Rust state. |

### Bigram (`ngram_kyotodb.cpp`)

| Pin call site and arguments | Status handling | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/ngram_kyotodb.cpp:54-80](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L54-L80): `reset`, new `StashDB`, open `"-"`, `load_snapshot` | Open or load failure returns false with the StashDB owned and its partial records kept. | KC `KcUserBigramDb::load_db`: `open_stash`, then `load_snapshot` with the result ignored. The container and its partial records stay. Same shape. |
| [src/storage/ngram_kyotodb.cpp:82-108](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L82-L108): null guard, `unlink`, `dump_snapshot` | Null owner false; `unlink` other than `ENOENT` false; dump result returned. | KC `KcUserBigramDb::save_db`: `remove_if_present`, `oxkc_db_dump_snapshot`. `write_user_bigram` dumps a new StashDB to a sibling temporary and renames it. |
| [src/storage/ngram_kyotodb.cpp:110-120](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L110-L120): `reset`, new `HashDB`, `open` | Null path false. A failed object stays owned. | ATTACH `open_system(path, true)` keeps it (#1535). Tools drop a failed object. |
| [src/storage/ngram_kyotodb.cpp:124-143](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L124-L143): null guard; `check`; `get(kbuf, 4, vbuf, vsiz)` | -1 → false. `check_result(vsiz == get(...))`. | KC `get_bigram_raw` → `Db::get_bigram`: `oxkc_db_check`, then `oxkc_db_accept`. A different size or a failed second read is `StoreError::ReadAfterCheck`. A failed object takes `check` only. |
| [src/storage/ngram_kyotodb.cpp:145-153](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L145-L153): `set(token, gram)` | Null owner false; the `set` bool returned. | `KcUserBigramDb::store` → `Db::set`. |
| [src/storage/ngram_kyotodb.cpp:155-161](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L155-L161): `remove(token)` | Null owner false. Absent key: `BasicDB::remove` sets `NOREC` and returns false (`kcdb.h:1862-1885`). | `KcUserBigramDb::remove` → `Db::remove`, which maps `NOREC` to `Ok`. |
| [src/storage/ngram_kyotodb.cpp:163-196](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L163-L196): stack `KeyCollectVisitor`; `iterate(&visitor, false)` | Clears the array first; null owner false. `iterate`'s answer is ignored; the method returns true. `visit_full` asserts a four-byte key. `visit_empty` asserts false. | `KcUserBigramDb::keys` → `Db::keys`: `oxkc_db_count` to reserve, then `oxkc_db_iterate`. An `iterate` failure is `Err`. A key of another length becomes `None` for the caller. |
| [src/storage/ngram_kyotodb.cpp:199-235](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L199-L235): `get_all_items`, then `remove`/`load`/`store` per key | Each call under `check_result`; no sync. | USER `ContainerGrams` and the user store. |

### Punctuation (`punct_table_kyotodb.cpp`)

| Pin call site and arguments | Status handling | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/punct_table_kyotodb.cpp:52-65](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L52-L65): `reset`, new `TreeDB`, `open` | Null path false. A failed object stays owned. | ATTACH `open_system(path, false)` keeps it. |
| [src/storage/punct_table_kyotodb.cpp:69-97](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L69-L97): new `ProtoTreeDB`, open `"-"`, `load_snapshot` | As chewing `load_db`. | No shipping caller loads a punctuation snapshot. |
| [src/storage/punct_table_kyotodb.cpp:99-121](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L99-L121): `unlink`, `dump_snapshot` | No null-member guard. | No shipping caller. |
| [src/storage/punct_table_kyotodb.cpp:123-142](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L123-L142): `set_size(0)`; `check`; `get` | Null owner false. `check` of **-1 or 0** returns true with an empty entry. Otherwise `check_result` on the `get` size. | Data `PunctTable` reads through the store `get`; one `accept`. The empty-value order must stay: an empty record and a missing record both give an empty entry and true. |
| [src/storage/punct_table_kyotodb.cpp:144-153](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L144-L153): `set(token, entry)` | Null owner false; the bool returned. The value pointer is the entry chunk's `begin()`. | Tools write through `Db::set`. |
| [src/storage/punct_table_kyotodb.cpp:155-161](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L155-L161): `remove(token)` | Null owner false. An absent key returns **false** (`NOREC`). This differs from the tkrzw punctuation `Remove`, which accepts `NOT_FOUND`. | `Db::remove` maps `NOREC` to `Ok`. A bridge must give this caller the raw answer. |
| [src/storage/punct_table_kyotodb.cpp:163-196](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L163-L196): `KeyCollectVisitor`; `iterate(false)` | As Bigram: answer ignored, true returned, four-byte assert. | `native_keys` → `Db::records`: `count`, then `iterate` that copies keys and values. |

The two `KeyCollectVisitor` classes in `ngram_kyotodb.cpp` and
`punct_table_kyotodb.cpp` have the same name at file scope and the same
tokens. They are one class under the one-definition rule.

### FlexibleBigram (`flexible_ngram_kyotodb.h`)

| Pin call site and arguments | Status handling and ownership | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/flexible_ngram_kyotodb.h:129-155](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L129-L155): `reset`; mode without `OCREATE`; null path; new `HashDB`; `open`; on failure without CREATE `delete`, null, false; with CREATE `open(mode\|OCREATE)` on the same object | Second open failure returns false **with the HashDB still owned**. It is not deleted. | FLEX `open`: `metadata` decides "fresh", then one `create_hash` (`OWRITER\|OCREATE`). A failed open is `Err` and `Db::drop` closes and deletes. One native open instead of two. |
| [src/storage/flexible_ngram_kyotodb.h:157-162](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L157-L162): after the second open, `set(magic key, magic number)` | `set` answer ignored; returns true. | FLEX writes the magic record through `write_value` only when "fresh" and absent, and checks the answer. |
| [src/storage/flexible_ngram_kyotodb.h:165-180](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L165-L180): first open succeeded: `check(magic key)`; `get`; `memcmp` | `check` -1 → false with the opened HashDB retained. Wrong magic → false, HashDB retained. | FLEX returns `Err` and drops the store. An **existing empty file** differs: see [other differences](#other-differences-found). |
| [src/storage/flexible_ngram_kyotodb.h:192-216](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L192-L216): `check`; `get`; new gram | -1 → false; `check_result` on the `get` size. | FLEX `read_gram` → `get_bigram_raw` → `check` then `accept`; `ReadAfterCheck` on a mismatch. |
| [src/storage/flexible_ngram_kyotodb.h:227-236](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L227-L236): `set(token, gram)` | Null false; the bool returned. No sync. | FLEX `write_value` → `write_deferred`, which is `write` on KC: `begin_transaction`, `set`, `end_transaction(commit)`, `synchronize(false)`. See [the #1552 checks](#the-two-1552-checks-on-kyoto-cabinet). |
| [src/storage/flexible_ngram_kyotodb.h:246-252](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L246-L252): `remove(token)` | Null false; absent → false (`NOREC`). No sync. | FLEX `delete_value` → `write`: transaction, `remove`, commit, `synchronize(false)`. `Db::remove` maps `NOREC` to `Ok`. |
| [src/storage/flexible_ngram_kyotodb.h:38-62](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L38-L62), [src/storage/flexible_ngram_kyotodb.h:262-272](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L262-L272): `FlexibleKeyCollectVisitor`; `iterate(false)` | Clears the array; null false. Skips keys not four bytes. `iterate`'s answer is ignored; returns true. | FLEX `get_all_items` → `native_keys` → `count`, `iterate`. A failure prints `training db error, exit!` and exits, as BDB's walk does; the KC pin returns true. |
| [src/storage/flexible_ngram_kyotodb.h:282-309](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L282-L309): `get(magic key, vbuf, 4 + header)` | `get` returns the record's full size, not the copied size (`kcdb.h:1985-2013`). A size other than `4 + header` asserts it is 4, then false. A right size asserts the magic. | FLEX `get_magic_header`: any other size is `None`. No assert, no magic check. The existing short-input records apply; this inventory adds none. |
| [src/storage/flexible_ngram_kyotodb.h:319-342](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L319-L342): `set(magic key, magic + header)` | Null false; the bool returned. | FLEX `set_magic_header` → `write_value`, with the transaction and sync above. |
| [src/storage/flexible_ngram_kyotodb.h:353-374](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L353-L374): `get(token, vbuf, header)` | -1 → false. Asserts the full size is at least the header. | FLEX `get_array_header`: `assert!` on a short value. |
| [src/storage/flexible_ngram_kyotodb.h:385-408](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L385-L408): `check`; `get`; overwrite header; `set(token, vbuf, vsiz)` | Absent → `vsiz = sizeof(ArrayHeader)`. Present → the **original** `vsiz`, also when it is shorter than the header. | FLEX `set_array_header` grows a short value to the header length. This is the same source-visible difference the tkrzw inventory records. |

## Retained objects and teardown

| Event | Pin object after return | Teardown in the pin | Rust at `5a92558c` |
| --- | --- | --- | --- |
| Ordinary `attach` with a null path | No DB after `reset`. | None. | Not reachable from the facade. |
| Ordinary TreeDB/HashDB `attach` fails | The new object stays owned with `omode_ == 0`. | Next `reset`: `synchronize()`, `close()`, `delete`. Each returns `INVALID` "not opened" on that object (`kcplantdb.h:1590-1596`, `kcplantdb.h:1641-1649`, `kchashdb.h:933-939`, `kchashdb.h:969-976`). | ATTACH keeps `KcUnopenedHandle`. Its `Drop` calls `synchronize_failed_system_attachment` (a native `synchronize(false)` with the answer ignored), then `Db::drop` closes and deletes. The order is one sync, one close, one delete. Test `retained_failed_system_attachments_synchronize_before_close_once` records it. |
| Ordinary TreeDB/HashDB `attach` succeeds (system file, `OREADER`) | Opened object owned. | `reset`: `synchronize()`, `close()`, `delete`. | `KcStore` → `Db::drop`: `close`, `delete`. **No `synchronize`.** See [the #1552 checks](#the-two-1552-checks-on-kyoto-cabinet). |
| `load_db` open or `load_snapshot` fails | ProtoTreeDB or StashDB stays owned, with partial records. | `reset` syncs, closes, deletes. | User bigram keeps the partial StashDB. User indexes drop it (`Err`). |
| FlexibleBigram first open fails, CREATE absent (the read-only attach) | `delete` then null. No explicit `close`. | `~HashDB` closes only when `omode_ != 0` (`kchashdb.h:532-546`); here it is 0. | FLEX `open_read_only` → `open_hash_read_only` fails; `Db::drop` runs `oxkc_db_close` (which returns `INVALID`) and then deletes. One extra native `close`. |
| FlexibleBigram second open fails | HashDB stays owned with `omode_ == 0`; `attach` returns false. | `reset`: `synchronize()`, `close()` (both `INVALID`), then null without `delete`. | `Err`; `Db::drop` closes and deletes. |
| FlexibleBigram opens, signature fails | Opened HashDB retained; later calls reach it. | `reset`: `synchronize()`, `close()`, null without `delete`. | `Err`; the store is dropped. |
| FlexibleBigram normal end | Opened HashDB. | `synchronize()`, `close()`, no `delete`. | `close` → no-op `flush` → `Db::drop`: `close`, `delete`. |

[#1535](https://github.com/shenghaoc/oxpinyin/pull/1535) (`553866cc`) gave KC
`Db::open_retained`, `KcUnopenedHandle` and `UnopenedSystemDbm`. A failed
system attach keeps the concrete object until facade finalization. A failed
point read makes the pin's first call, `check`, and nothing after it
([src/storage/ngram_kyotodb.cpp:131-134](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L131-L134)). A failed suggestion makes the
same `check` ([src/storage/chewing_large_table2_kyotodb.cpp:541-544](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L541-L544)).
`UnopenedSystemDbm::finish` runs the sync, close and delete once; a second
call does nothing. The `ffi.rs` SAFETY note was extended for the retained
object: `omode_` stays 0, and `accept`, `check`, `synchronize` and `close`
take the locks before they return `INVALID`.

Kyoto Cabinet destructors close an object that is still open and detach its
cursors (`kchashdb.h:532-546`, `kcplantdb.h:1018-1028`, `kcprotodb.h:352-362`,
`kcstashdb.h:324`). Destruction is not a substitute for the pin's explicit
`close` or its position. The FlexibleBigram object that the pin never deletes
is a leak with no file effect: `close` already ran.

## Callbacks

All pin visitors derive from `kyotocabinet::DB::Visitor` (`kcdb.h:41-92`).
None overrides `visit_before` or `visit_after`.

| Pin visitor | Call | `visit_full` input and result | `visit_empty` |
| --- | --- | --- | --- |
| `MaskOutVisitor2` ([src/storage/chewing_large_table2_kyotodb.cpp:446-511](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L446-L511)) | `iterate(&v, true)` | Copies `vbuf, vsiz` into the entry chunk, masks, sets `*sp`, returns the chunk's `begin()`. The chunk belongs to the entry table, so the region stays valid after the visit. A key length not 1–16 keys aborts. | `NOP`. `iterate` does not call it. |
| `MaskOutVisitor` ([src/storage/phrase_large_table3_kyotodb.cpp:349-375](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L349-L375)) | `iterate(&v, true)` | Same, with the visitor's own `PhraseTableEntry` chunk. | `NOP`. |
| `KeyCollectVisitor` ([src/storage/ngram_kyotodb.cpp:163-184](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_kyotodb.cpp#L163-L184), [src/storage/punct_table_kyotodb.cpp:163-184](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_kyotodb.cpp#L163-L184)) | `iterate(&v, false)` | Asserts `ksiz == 4`, reads four bytes at `kbuf`, returns `NOP`. | Asserts false. |
| `FlexibleKeyCollectVisitor` ([src/storage/flexible_ngram_kyotodb.h:38-62](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L38-L62)) | `iterate(&v, false)` | Skips `ksiz != 4`, else reads four bytes, returns `NOP`. | Asserts false. |
| `BasicDB`'s own visitors in `set`, `remove`, `get`, `check`, and the cursor `get_key`/`get_value` (`kcdb.h:1424-1445`, `kcdb.h:1862-1885`, `kcdb.h:1907-1943`, `kcdb.h:1985-2041`, `kcdb.h:600-700`) | Through `accept` | Library code; the pin calls the wrappers. | Library code. |

Pointer validity and length. Kyoto Cabinet passes `kbuf`/`vbuf` regions that
are valid only for the call, with the database or record lock held
(`kcdb.h:268`, `kcdb.h:1232`). `ksiz` and `vsiz` are `size_t` values no larger
than `MEMMAXSIZ`, `INT32MAX / 2` (`kcutil.h:133`); `check` and the
buffer `get` return `int32_t`. The headers assert non-null `kbuf`, `vbuf` and
`sp` only in a `_KCDEBUG` build (`kccommon.h:173-186`). A returned region must
stay valid until `accept` or `iterate` returns, and `*sp` must hold its size.
A visitor must make no database call ("to avoid deadlock", `kcdb.h:268`).

The SHIM's `Visitor` (`shim.cc`) forwards `visit_full` and `visit_empty` to
C function pointers with an opaque argument. It reads a null return, or a
null callback, as `NOP`. It passes any other pointer through unchanged, so a
C callback that returns address 1 asks for `REMOVE`. FFI `visit_full::<V>`
builds slices with `visited` (an empty slice for `len == 0`), copies them,
and always returns null. FFI `visit_empty` returns null. `oxkc_db_iterate`
and `oxkc_cur_accept` pass a null empty callback. FFI never asks for a
writable visit: every `accept`, `iterate` and cursor `accept` passes
`writable=0`. No Rust callback returns a replacement region, so no Rust
storage must outlive a visit. Allocation failure in a callback aborts the
process (`reserve_exact`), as `new[]` in the pin's visitors would throw past
a frame with no handler.

## Sentinels and the empty value

`Visitor::NOP` is address 0 and `Visitor::REMOVE` is address 1 (read from the
shipped library, [above](#trees-and-completeness); `kclangc.h:128-133`
documents `KCVISNOP` and `KCVISREMOVE`). The library compares the returned
pointer with these addresses. `BasicDB::set` returns its caller's `vbuf` from
both visitor methods (`kcdb.h:1424-1445`). So the value pointer of a `set`
**is** the visitor's return value:

- `vbuf == 0` is `NOP`. `set` returns true and changes nothing: an existing
  record keeps its value and a missing record is not created.
- `vbuf == 1` is `REMOVE`. `set` returns true and removes an existing record.
- Any other address stores `vsiz` bytes, an empty record for `vsiz == 0`.

The pin stores its empty prefix records with `empty_vbuf`, address
`UINTPTR_MAX` ([src/storage/kyotodb_utils.h:49-50](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/kyotodb_utils.h#L49-L50),
[src/storage/chewing_large_table2_kyotodb.cpp:314](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L314),
[src/storage/phrase_large_table3_kyotodb.cpp:289](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_kyotodb.cpp#L289)). Its other `set` calls
pass a `MemoryChunk::begin()`. A chunk that was never given space has
`begin() == NULL` ([src/include/memory_chunk.h:173-178](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/include/memory_chunk.h#L173-L178),
[src/include/memory_chunk.h:196-198](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/include/memory_chunk.h#L196-L198)), so a `set` of such an empty chunk is
`NOP`. The pin's shipping paths give the chunk space before they store it.
This is a pin property to keep in mind, not a case this inventory adds.

FFI `c_ptr` passes the address of a private `static u8` for every empty
slice, keys included. It never passes 0 or 1. Its comment records the
measured `kcdbset(k, 2, (char*)1, 0)` deletion. The CXX migration has the
same hazard in a new place: cxx passes an empty `&[u8]` as
`rust::Slice<const uint8_t>` with `data()` equal to `align_of<u8>()`, which
is 1 (`cxx` 1.0.205 `src/rust_slice.rs` `from_ref` keeps the Rust dangling
pointer; `include/cxx.h` `Slice<T>::Slice()` uses `align_of<T>()`). An
adapter that hands `slice.data()` to `set` deletes the record. The adapter
must substitute a non-sentinel address for an empty value before the call.

A visitor that the adapter writes must return `kyotocabinet::DB::Visitor::NOP`
or `REMOVE` by name, never a literal. Rust must not hold, copy or compare a
sentinel.

## Status and exception channels

Kyoto Cabinet has no status object. An operation returns `bool`, an
`int32_t`/`int64_t` with -1 for failure, or a null pointer. It records the
reason with `set_error` in a per-thread `Error` slot (`TSD<Error> error_`,
`kcprotodb.h:1257`, `kcstashdb.h:1444`; HashDB `error()` at
`kchashdb.h:756-759`; TreeDB forwards to its inner HashDB,
`kcplantdb.h:1455-1458`). The codes are `SUCCESS`, `NOIMPL`, `INVALID`,
`NOREPOS`, `NOPERM`, `BROKEN`, `DUPREC`, `NOREC`, `LOGIC`, `SYSTEM` and `MISC`
(`kcdb.h:916-932`). A miss is `NOREC`: `check` and the buffer `get` return
-1, `remove` returns false. The code is the only way to tell a miss from a
failure. The pin never reads the code. It compares with -1 or tests the
bool.

The pin installs no logger, so `set_error` has no other effect
(`kcprotodb.h:720-728`). Open failures classify `errno` text into `NOPERM`,
`NOREPOS` or `SYSTEM` (`kchashdb.h:823-832`). `load_snapshot` and
`dump_snapshot` use `std::ifstream`/`std::ofstream` with stream exceptions
off; they report `NOREPOS` and `SYSTEM` (`kcdb.h:2316-2338`,
`kcdb.h:2419-2440`).

Exceptions. Kyoto Cabinet's only own `throw` sites are `xmalloc`,
`xcalloc` and `xrealloc`, which throw `std::bad_alloc` (`kcutil.h:2863-2889`).
`new`, `std::string`, the STL containers and the streams can also throw
`std::bad_alloc`. The pin has no `try` around any call, so such an exception
leaves libpinyin and ends the process. Kyoto Cabinet's destructors are
implicitly `noexcept` and call `close` (`kchashdb.h:532-534`); an exception
there terminates.

The SHIM is different. Every entry point is `noexcept` and catches **every**
exception (`catch (...)`). It returns the C failure value (0, -1 or null),
and `fail()` records `SYSTEM` "C++ exception in the Kyoto Cabinet shim" on
the database when it can. `oxkc_db_ecode` and `oxkc_cur_ecode` return
`SYSTEM` if reading the error throws; `oxkc_db_emsg` returns the shim's
constant text. `oxkc_db_new` and `oxkc_db_cursor` return null. FFI maps
every failure to `StoreError::Backend("<call>: <code name>: <message>")`,
except that `last_was_no_record` turns `NOREC` into a miss for `check`,
`remove` and the cursor calls. So an allocation failure that ends the pin
becomes a Rust error on KC. The [ruling](compatibility-policy.md#kc-migration-native-semantics)
classifies this difference for the migration.

`Db::count` maps a negative `int64_t` to an error. `oxkc_db_count` returns -1
on an exception, the same value as a native failure. The tkrzw C API's
0-on-exception wrinkle has no KC counterpart.

## The two #1552 checks on Kyoto Cabinet

[#1552](https://github.com/shenghaoc/oxpinyin/pull/1552) fixes two tkrzw
native-call differences. The same questions on KC, at `5a92558c` and at the
#1552 head:

1. **A sync on FlexibleBigram writes that the pin does not make.** The KC pin
   `store`, `remove`, `set_magic_header` and `set_array_header` call
   `set`/`remove` only ([src/storage/flexible_ngram_kyotodb.h:227-252](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L227-L252),
   [src/storage/flexible_ngram_kyotodb.h:319-342](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L319-L342),
   [src/storage/flexible_ngram_kyotodb.h:385-408](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L385-L408)). The only sync is in
   `reset` ([src/storage/flexible_ngram_kyotodb.h:86-92](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_kyotodb.h#L86-L92)). On main, FLEX
   `write_value` calls `write_deferred` and `delete_value` calls `write`.
   KC does not override `write_deferred`, so both are `KcStore::write`:
   `oxkc_db_begin_transaction(hard=0)`, the `set` or `remove`,
   `oxkc_db_end_transaction(commit=1)` and `oxkc_db_sync(hard=0)`. That is
   three native calls per write that the pin does not make, and the sync can
   fail after a successful `set`. FLEX `close` calls `flush`, which KC
   inherits as a no-op, so the pin's one `synchronize()` at reset is absent.
   At the #1552 head, `delete_value` uses `write_deferred` and FLEX flushes on
   drop. KC still inherits `write_deferred = write` and the no-op `flush`, as
   that PR's body states. **KC has this difference, in a larger form, on main
   and at the #1552 head.**
2. **A missing sync before close on successfully opened system trees.** Every
   KC pin `reset` with a non-null `m_db` calls `synchronize()` before
   `close()`: chewing, phrase and punctuation TreeDB, and also the Bigram
   HashDB (the tkrzw Bigram has no sync). On main, `open_system` returns
   `KcStore`, whose `Db::drop` closes and deletes with no sync. `Db::sync`
   on a read-only handle returns `Ok` without a native call (test
   `ordinary_read_only_sync_does_not_call_native_synchronize`). **KC has this
   difference for TreeDB and for HashDB.** The same holds for the in-memory
   ProtoTreeDB and StashDB owners, whose pin `reset` also syncs.

The effect differs from tkrzw. On an `OREADER` TreeDB or HashDB, Kyoto
Cabinet's `synchronize` takes the locks, skips every writer step, and
returns true (`kchashdb.h:969-983`, `kchashdb.h:2384-2419`,
`kcplantdb.h:1641-1700`). tkrzw returned a precondition error. ProtoDB and
StashDB `synchronize` do nothing but the lock and the null-processor test
(`kcprotodb.h:804-830`, `kcstashdb.h:706-720`). On a writer HashDB,
`synchronize(false)` writes the header and `close` writes it again
(`kchashdb.h:933-957`, `kchashdb.h:2565-2594`). So the missing system sync is a
native-call difference with no file effect found in source. The FlexibleBigram
item changes I/O on every write and adds failure points. Neither item is
fixed here. Neither is registered as a divergence.

## Other differences found

These are source-visible differences in the current KC code. They are
reported, not fixed, and not classified:

1. **ProtoTreeDB path.** The pin opens its in-memory tree as `"-"`
   ([src/storage/chewing_large_table2_kyotodb.cpp:59](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_kyotodb.cpp#L59)). `Db::open_proto_tree`
   opens it as `"+"`. That name dates from `c4fb8d4a`, when the binding
   opened KC through PolyDB, where `"+"` selects ProtoTreeDB and `"-"`
   selects ProtoHashDB (`kcpolydb.h:384-387`). The shim kept the name. `ProtoDB::open` only stores the path (`kcprotodb.h:750-763`);
   it reaches a logger, `path()`, `status()` and a file processor, none of
   which the pin or Rust uses. The StashDB path `"-"` matches.
2. **FlexibleBigram on an existing empty file.** With `ATTACH_READWRITE |
   ATTACH_CREATE`, the pin's first open has `OWRITER` without `OCREATE`. For an
   existing zero-length file `HashDB::open` succeeds and writes a new header
   (`kchashdb.h:835-850`). The signature `check` then returns -1, so `attach`
   returns false with the HashDB retained, and no magic record is written.
   FLEX `open` treats a zero-length file as fresh, writes the magic record and
   returns `Ok`.
3. **FlexibleBigram failed attach.** FLEX drops the object in every failure
   case. The pin keeps it in two of them ([per-backend rule](compatibility-policy.md#flexiblebigram-failed-attach)).
4. **Partial user-index snapshot.** The pin keeps the records that
   `load_snapshot` set before a failure. USER skips the whole index.
5. **Remove of an absent key.** Pin Bigram, punctuation and FlexibleBigram
   `remove` return false on `NOREC`. `Db::remove` returns `Ok`.
6. **Walk answers.** The pin's three `get_all_items` ignore `iterate`'s answer
   and return true. Rust propagates the error, or exits for FlexibleBigram.
7. **Extra native calls.** `count` before every `records`/`keys` walk; one
   `accept` for a point read where the pin calls `check` and `get`; one
   `accept(step)` per cursor record where the pin calls `get_key`,
   `get_value` and `step`; transactions and a soft sync in `KcStore::write`;
   a fresh ProtoTreeDB filled row by row in `write_user_index`.
8. **Exception channel.** The SHIM catches every exception where the pin
   terminates ([above](#status-and-exception-channels)).

## What the tkrzw migration settled, for KC to reuse

[#1545](https://github.com/shenghaoc/oxpinyin/pull/1545),
[#1550](https://github.com/shenghaoc/oxpinyin/pull/1550) and the open
[#1552](https://github.com/shenghaoc/oxpinyin/pull/1552) settled these points.
The KC migration reuses them and states where KC differs.

- **Development cdylib exports.** Any cdylib whose graph holds the `cxx`
  crate exports the cxx runtime (`cxxbridge1$*`). Both capi build scripts pass
  `-Wl,--exclude-libs,ALL` on Linux. The condition is
  `CARGO_FEATURE_TKRZW` only
  ([`capi/build.rs`](../../crates/oxpinyin-capi/build.rs),
  [`zhuyin-capi/build.rs`](../../crates/oxpinyin-zhuyin-capi/build.rs)). A KC
  bridge must extend that condition to the `kyotocabinet` feature. The KC
  shim's own static library is already compiled with
  `-fvisibility=hidden`.
- **macOS exports.** The Apple linker has no `--exclude-libs`, and two
  `-exported_symbols_list` files form a union.
  [#1543](https://github.com/shenghaoc/oxpinyin/pull/1543) (`704c3374`)
  relinks the installed dylib under one list derived from the `.ver`
  globals ([macOS shipped exports](macos-shipped-exports.md),
  [export gate](../packaging.md#export-gate)). The development dylibs keep
  the cxx symbols; `just abi exports` there runs on the relinked files. A KC
  bridge needs no new macOS mechanism. The KC cell must go through that
  relink and its check.
- **Status versus exception.** The tkrzw bridge declares no `Result`. Every
  call returns `NativeStatus { code, message }`, and the caller checks or
  ignores it as the pin does. The adapter catches nothing, so an exception
  reaches the generated `noexcept` glue and terminates, as in the pin
  ([`tkrzw/bridge.rs`](../../crates/oxpinyin-store/src/tkrzw/bridge.rs),
  [`tkrzw/adapter.h`](../../crates/oxpinyin-store/src/tkrzw/adapter.h)). For KC
  the status is a bool or size plus the per-thread `Error`. The adapter must
  read `error()` in the same call, on the same thread. The tkrzw exception
  rule applies to KC ([ruling](compatibility-policy.md#kc-migration-native-semantics)); the
  migration PR states the removal of the SHIM's `catch (...)` under safety
  point 1.
- **Callback adapter.** One `final` C++ subclass per walk, which copies keys
  and returns the library's own `NOOP` by name (`KeyCollector` in
  [`tkrzw/adapter.cc`](../../crates/oxpinyin-store/src/tkrzw/adapter.cc)).
  Length checks stay in Rust. The adapter calls the library and copies none
  of it.
- **Send/Sync justification.** A `SAFETY: (ruling point 2)` comment at the
  `unsafe impl` that cites the class headers' concurrency statements and the
  per-call lock lines, and says that open and close run with exclusive
  access (`tkrzw/mod.rs`, at `unsafe impl Send for Db`). The KC note in
  `ffi.rs` makes the same argument from the 1.2.80 headers but names files
  and members without line numbers. The migration restates it in the tkrzw
  format.
- **Shared-reference mutation.** The tkrzw adapter takes `const DBM&` and
  documents one `const_cast`, because the objects are never defined `const`
  and the classes lock per call. KC record methods are non-`const` but
  lock `mlock_` per call (`kchashdb.h:556-575`, `kcplantdb.h:1042-1060`); the
  same pattern applies.
- **Include handling in `build.rs`.** The tkrzw build asks the compiler for
  its default `#include <...>` list and passes each probed directory as
  `-isystem` unless the compiler already searches it, because `-isystem
  /usr/include` breaks libstdc++'s `#include_next`. It sets `-std=c++17`,
  `-O2`, hidden visibility and the explicit ASan flags
  ([`store/build.rs`](../../crates/oxpinyin-store/build.rs), `mod tkrzw`). The
  KC shim uses `cc`'s `include` (`-I`), sets no `-std`, and has the same `-O2`,
  visibility and ASan flags (#1531). A KC bridge reuses the tkrzw include
  helper.
- **Teardown tests.** #1552 counts the system TreeDBM sync at the owner,
  because a read-only sync is an ignored native error and leaves no file
  trace. KC already records native sync, close and delete under `cfg(test)`
  for the failed-object path (#1535); its read-only sync also leaves no file
  trace, so the same counting approach applies.
- **Bridge shapes in use.** A shared struct holds `UniquePtr<CxxString>`
  (`NativeStatus`); an opaque type is a C++ `using` alias (`DbmIterator`);
  `cxx_build::bridge` returns a `cc::Build` that takes the same flags as the
  KC shim's `cc::Build`.

## Proposed bridge surface (proposal only)

This is a proposal. It is not an implementation decision, and the ruling's
approval conditions apply to the PR that implements it.

Expose opaque concrete owners behind `UniquePtr<BasicDB>` (its destructor
is virtual), and a cursor that borrows its database. Keep construction and
`open` separate.

| Proposed function | Purpose and minimum adapter |
| --- | --- |
| `new_tree_db`, `new_hash_db`, `new_stash_db`, `new_proto_tree_db` → `UniquePtr<BasicDB>` | The pin's four `new` expressions. No tuning. `tune_buckets` stays bench-only, if kept. |
| `open(db, path: &[u8], mode: u32)` → `NativeResult` | Byte path to `std::string`; the mode bits as the pin builds them, `OREADER` included. A failed open leaves the same owner. Rust rejects a path that contains a NUL byte before the conversion, with the current `StoreError::InvalidInput("store path contains NUL")` ([`ffi.rs`](../../crates/oxpinyin-store/src/kyotocabinet/ffi.rs) `open_retained`, lines 553-554). A `std::string` passed on as `c_str()` would open the prefix before the NUL. The pin cannot receive such a path, so this is the Rust input contract, not a pin semantic. |
| `close`, `synchronize(hard)` → `NativeResult` | The caller controls the sync, close and destruction order per owner. Destruction is `UniquePtr` drop and never replaces an explicit `close`. |
| `check(key)` → `NativeResult` with the `int32_t` size | Keeps -1 and `NOREC` distinct from other codes. |
| `get(key, out: Pin<&mut CxxString>, max)` or a copying visitor | Either the pin's buffer `get` (returns the full size) or one read-only `accept`. The caller chooses which pin sequence it reproduces. |
| `set(key, value)`, `remove(key)` → `NativeResult` | Substitute a non-sentinel address for an empty slice (see [sentinels](#sentinels-and-the-empty-value)). `remove` returns `NOREC` as data. |
| `collect_keys(db)` → keys in `iterate` order | One `final` visitor that copies keys and returns `NOP`. Length checks in Rust. A writable mask visitor only if a native mask walk is ever exposed. |
| `load_snapshot(path)`, `dump_snapshot(path)` → `NativeResult` | The `BasicDB` string-path wrappers, no progress checker. For both wrappers, Rust rejects a path that contains a NUL byte before the conversion, with the same `InvalidInput` ([`ffi.rs`](../../crates/oxpinyin-store/src/kyotocabinet/ffi.rs) `cstring_path`, line 185), as for `open`. |
| `make_cursor(db)` → `UniquePtr<Cursor>`; `jump(key)`, `jump()`, `step()`, `get_key`, `get_value` | Cursor borrows the DB in Rust (`PhantomData<&'db Db>`). `get_key`/`get_value` return owned bytes; the adapter frees Kyoto Cabinet's `new[]` buffer. |
| `count`, `begin_transaction`, `end_transaction` | Only while audited Rust callers keep them. They are not pin calls. |
| `NativeResult { ok, value, code, message }` | The answer and `error()` read together, inside the same adapter call. |

No function returns `Result`; the [ruling](compatibility-policy.md#kc-migration-native-semantics)
sets the exception rule.

## Six manual safety review points against the current code

1. **Safe declarations.** No function is declared safe today: every SHIM call
   is `unsafe` in `ffi.rs`, with a SAFETY comment per block, behind
   `#![expect(unsafe_code)]`. Their contract is "the handle is live" plus
   slice lifetimes. A CXX bridge declares each function safe, so each claim
   moves to the bridge: a live owner, a key/value no larger than `MEMMAXSIZ`,
   no visitor database call, and no sentinel value pointer. A never-opened
   owner is a valid receiver (every method tests `omode_` under the lock); a
   null owner is not.
2. **Send/Sync.** `Db` has `unsafe impl Send` and `Sync` in `ffi.rs`. The
   argument: every record operation takes the object's `mlock_`;
   transactions serialize on it; the error slot is per thread; `open`
   completes before the handle is shared; `close` and delete run in `Drop`.
   The [backend record](kyotocabinet-backend.md#sendsync-verified-against-the-kyoto-cabinet-source-pre-merge)
   holds the verification. `BasicDB`'s note forbids two objects on one file in
   one process and sharing with a child process (`kcdb.h:466-479`). Restate
   it per concrete class with line citations, as tkrzw does.
3. **Cursor → DB lifetime.** `Cursor<'db>` holds `PhantomData<&'db Db>`. Its
   raw handle makes it neither `Send` nor `Sync`. Kyoto Cabinet's destructors
   detach live cursors (`kchashdb.h:532-546`, `kcplantdb.h:1018-1028`), and a
   detached cursor's destructor returns early (`kchashdb.h:146-151`,
   `kcplantdb.h:134-140`), but every other cursor method dereferences its
   database (`kcplantdb.h:222-226`). Keep the borrow.
4. **Callback pointers and lengths.** Inputs are borrowed for the visit only;
   FFI copies them and returns null. `visited` returns an empty slice for
   `len == 0`. `opq` is an exclusive `&mut V` for one call. No replacement
   region crosses. A bridge with a replacement visitor must keep the region
   in adapter-owned storage until `accept`/`iterate` returns, and set `*sp`.
5. **Synchronize, close, destruction.** See [teardown](#retained-objects-and-teardown).
   The failed system object follows the pin (#1535). Opened system
   attachments, the in-memory owners and FlexibleBigram do not make the
   pin's `synchronize()` before `close()`. FlexibleBigram adds per-write
   syncs, and its failed read-only open makes one `close` that the pin
   does not make.
6. **Sentinels.** `NOP` = 0 and `REMOVE` = 1. FFI never passes 0 or 1 as a
   value pointer (`c_ptr`) and returns only null (`NOP`) from callbacks.
   cxx gives an empty slice the address 1. The bridge adapter must replace
   it before `set`.

## Result and boundaries

The KC migration obligations are: concrete construction separate from
`open`; the retained failed system object; per-owner sync, close and
destruction; per-caller status handling with `NOREC` kept as data; the
empty-value sentinel at the cxx boundary; and the [exception rule](compatibility-policy.md#kc-migration-native-semantics).
The [#1552 checks](#the-two-1552-checks-on-kyoto-cabinet) and the
[other differences](#other-differences-found) locate source-visible
differences. They are not measured outward divergences.

This PR changes no code. It does not change KC's frozen FFI contract, BDB,
tkrzw, or any register row. Verification for this documentation change is
link review and `git diff --check`. No build, test, sanitizer, measurement,
differential or pin execution was run.
