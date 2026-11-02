# tkrzw native-call inventory before the CXX transition

Recorded 2026-11-02T01:12:27Z (`date -u`). Status: source inventory and
proposal, not a binding implementation or compatibility closure.

This is the inventory required by the **2026-11-02 Rust/C++ interoperability
ruling**, recorded in [#1529](https://github.com/shenghaoc/oxpinyin/pull/1529)
at the explicit [rust-cpp-interoperability anchor](https://github.com/shenghaoc/oxpinyin/blob/9ae27ccd0862b4fdeaf385d750e3db6fbde8cf36/docs/findings/compatibility-policy.md#rust-cpp-interoperability). It supports [#1516](https://github.com/shenghaoc/oxpinyin/issues/1516)
without closing it. The tkrzw synthetic failed-state proposal in that lane is
withdrawn; its source derivation and cases remain inputs to the later binding
over the pin's concrete objects. This document changes no backend, policy row,
case disposition, dependency, or Kyoto Cabinet FFI contract.

## Trees and completeness

The pin was read from a Git archive of libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`. Library citations name tkrzw
`9ee84164cf45e441aa8186418c03a02c9b89f08b` (1.0.34), verified against the local
source checkout and the recorded source-build provisioning revision in
[`tools/tkrzw/build.py`](../../tools/tkrzw/build.py), `TKRZW_REV`. No pin was
executed. Source settled the call and ownership inventory; no disassembly was
needed. This is not a new runtime-parity result.

The Rust baseline is main `f3d08577b7a5f159f0d1624c52aedfb7d8462c26`.
Unqualified Rust paths and item names below refer to that tree. Pending family C
[#1528](https://github.com/shenghaoc/oxpinyin/pull/1528) is examined separately at
`473c1654d381881041ad46fe0b5a3e0eb593da23`; its relevant tkrzw, KMM and tools
paths are unchanged from the initially supplied
`4770fbd36ef037730b604bbdad81412b499b4f24`. Neither revision is treated as main.

The native-call census covers all ten tkrzw-specific pin files: the four
`chewing_large_table2`, `phrase_large_table3`, `ngram`, and `punct_table`
`_tkrzwdb.cpp`/`.h` pairs, `flexible_ngram_tkrzwdb.h`, and `tkrzwdb_utils.h`.
The four ordinary headers hold the owning `DBM*` and destructor/reset contract
([src/storage/chewing_large_table2_tkrzwdb.h:35-48](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.h#L35-L48),
[src/storage/chewing_large_table2_tkrzwdb.h:93-107](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.h#L93-L107),
[src/storage/phrase_large_table3_tkrzwdb.h:32-56](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.h#L32-L56),
[src/storage/ngram_tkrzwdb.h:37-61](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.h#L37-L61),
[src/storage/punct_table_tkrzwdb.h:33-56](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_tkrzwdb.h#L33-L56)).
The other six files contain every explicit native operation inventoried below.

Selection and build wiring are also accounted for:
[configure.ac:124-131](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/configure.ac#L124-L131), [src/Makefile.am:76-81](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/Makefile.am#L76-L81),
[src/storage/Makefile.am:107-112](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/Makefile.am#L107-L112), and the selectors at
[src/storage/chewing_large_table2.h:37-38](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2.h#L37-L38),
[src/storage/phrase_large_table3.h:37-38](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3.h#L37-L38),
[src/storage/ngram.h:37-38](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram.h#L37-L38), [src/storage/punct_table.h:37-38](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table.h#L37-L38),
[src/storage/flexible_ngram.h:42-43](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram.h#L42-L43).
The utility/test and shared-facade callers are listed later; they add no other
tkrzw class or native entry point.

To repeat the source census in a pin checkout:

```sh
git grep -n -E 'tkrzw::|tkrzw_dbm_|using namespace tkrzw|copy_tkrzwdb' 074a2219 -- src utils tests
git grep -n -E 'm_db->|tmp_db\.|iter->|new (TreeDBM|HashDBM|BabyDBM|TinyDBM)|RemoveFile' 074a2219 -- 'src/storage/*tkrzw*'
git grep -n -E 'ChewingLargeTable2|PhraseLargeTable3|PunctTable|KMixtureModelBigram|FlexibleBigram|Bigram' 074a2219 -- utils tests
```

## Concrete classes, options and tuning

The pin never constructs `PolyDBM`, `ParamDBM`, `ShardDBM`, or another DBM
class. Its member has static type `DBM*`, but the allocation fixes the concrete
type. No pin call sets a tuning parameter, comparator, update logger or file
processor. Every two-argument `Set` means `overwrite=true, old_value=nullptr`;
every one-argument `Remove` means `old_value=nullptr`; every
`Synchronize(false)` means `proc=nullptr`.

| Pin owner/use | Concrete allocation and defaults | Current Rust route |
| --- | --- | --- |
| Chewing, phrase and punctuation construction/load destination | `new BabyDBM`, no `Open`; lexical comparator. [src/storage/chewing_large_table2_tkrzwdb.cpp:56-61](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L56-L61), [src/storage/phrase_large_table3_tkrzwdb.cpp:45-48](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.cpp#L45-L48), [src/storage/punct_table_tkrzwdb.cpp:36-41](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_tkrzwdb.cpp#L36-L41). | `TkrzwStore::create_in_memory` → `open_in_memory` → C open with empty path, writable true, `dbm=baby`; shipping user indexes also live in Rust session structures. |
| Those three attached files and load/save temporaries | Default `TreeDBM`. No extension-based selection or path-parameter syntax. | `open(path, writable)` → C open with `dbm=tree` and `no_create=true` only for read-only. |
| Bigram attached file and load/save temporary; FlexibleBigram attached file | Default `HashDBM`. Bigram starts with null owner; FlexibleBigram does too. [src/storage/ngram_tkrzwdb.cpp:32-34](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L32-L34), [src/storage/ngram_tkrzwdb.cpp:96-98](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L96-L98), [src/storage/flexible_ngram_tkrzwdb.h:101-106](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L101-L106), [src/storage/flexible_ngram_tkrzwdb.h:145-155](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L145-L155). | `open_hash` / `open_hash_create`, and the shared flexible container. |
| Bigram load destination | `new TinyDBM`, no `Open`, default bucket count. [src/storage/ngram_tkrzwdb.cpp:48-60](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L48-L60). | `TkrzwUserBigramDb::empty` → C open with empty path, writable true, `dbm=tiny`. |

At this library revision, Tree defaults include offset width 4, alignment power
10, 131101 buckets, free-block pool 2048, page size 8130, 256 branches and
10000 cached pages ([tkrzw_dbm_tree.h:43-56](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tree.h#L43-L56)). Hash defaults include offset
width 4, alignment power 3, 1048583 buckets, free-block pool 2048 and minimum
read size 48 ([tkrzw_dbm_hash.h:42-51](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.h#L42-L51)). Tiny defaults to 1048583 buckets
([tkrzw_dbm_tiny.h:150-163](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tiny.h#L150-L163)); Baby's constructor defaults to
`LexicalKeyComparator` ([tkrzw_dbm_baby.h:145-157](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_baby.h#L145-L157)). Hash tuning retains default
update/CRC/compression/restore modes, negative unspecified numeric fields and
an empty cipher key ([tkrzw_dbm_hash.h:237-322](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.h#L237-L322)). Tree adds default page-update
mode and a null comparator selecting lexical order
([tkrzw_dbm_tree.h:185-225](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tree.h#L185-L225)). Do not turn these into separately maintained Rust
constants: call the concrete default constructors and `Open` used by the pin.
For existing files, stored metadata and native open logic still govern.

`TreeDBM::Open` and `HashDBM::Open` themselves delegate to their own
`OpenAdvanced` with default tuning ([tkrzw_dbm_tree.h:259-261](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tree.h#L259-L261),
[tkrzw_dbm_hash.h:356-358](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.h#L356-L358)). The mismatch is the outer `PolyDBM` factory and
parameter-map route, not the mere presence of `OpenAdvanced` inside tkrzw.
Default file objects are `MemoryMapParallelFile` in the concrete constructors
([tkrzw_dbm_hash.cc:2401-2406](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L2401-L2406), [tkrzw_dbm_tree.cc:2558-2564](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tree.cc#L2558-L2564),
[tkrzw_dbm_baby.cc:1470-1478](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_baby.cc#L1470-L1478), [tkrzw_dbm_tiny.cc:864-871](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tiny.cc#L864-L871)).

For ordinary `attach`, `writable` starts false; READWRITE asserts that READONLY
is absent, then makes it true; without CREATE, add `OPEN_NO_CREATE`; otherwise
options are `OPEN_DEFAULT`/zero ([src/storage/tkrzwdb_utils.h:32-45](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/tkrzwdb_utils.h#L32-L45)).
Flexible attach has its own two-open sequence, below. In particular, CREATE
does not by itself imply writable.

## Call-site map: pin → Rust → difference

Rust abbreviations in the tables refer to these source files and their named
items, not a promised stable Rust API:

- **TK**: [`store/tkrzw/mod.rs`](../../crates/oxpinyin-store/src/tkrzw/mod.rs).
  **FFI**: [`store/tkrzw/ffi.rs`](../../crates/oxpinyin-store/src/tkrzw/ffi.rs),
  generated C bindings selected by [`store/build.rs`](../../crates/oxpinyin-store/build.rs).
- **FLEX**: [`store/flexible_ngram.rs`](../../crates/oxpinyin-store/src/flexible_ngram.rs).
  **STORE**: [`store/lib.rs`](../../crates/oxpinyin-store/src/lib.rs), the raw,
  read/write and user-bigram traits.
- **CHEW**: [`data/chewing_table.rs`](../../crates/oxpinyin-data/src/chewing_table.rs),
  `RawChewingDbm`, `ChewingTable`; **PHRASE**:
  [`data/phrase_table.rs`](../../crates/oxpinyin-data/src/phrase_table.rs), `PhraseTable`;
  **GRAM**: [`data/bigram_table.rs`](../../crates/oxpinyin-data/src/bigram_table.rs),
  `BigramTable`; **PUNCT**: [`data/punct.rs`](../../crates/oxpinyin-data/src/punct.rs),
  `PunctTable`.
- **USER**: [`user/persistence.rs`](../../crates/oxpinyin-user/src/persistence.rs)
  (`load_user_pinyin_index`, `load_user_phrase_index`, `load_user_bigram_db`,
  `stage_dbm`, `stage_user_bigram`) and
  [`user/store.rs`](../../crates/oxpinyin-user/src/store.rs) (`GenericUserStore`,
  `ContainerGrams`). **TOOLS**:
  [`utils/storage/dbm.rs`](../../crates/oxpinyin-utils/src/storage/dbm.rs), `Tree`, `Hash`;
  **GEN**: [`utils/storage/gen_binary_files.rs`](../../crates/oxpinyin-utils/src/storage/gen_binary_files.rs).
- **KMM**: [`kmm/database.rs`](../../crates/oxpinyin-kmm/src/database.rs), `Database`;
  [`kmm/records.rs`](../../crates/oxpinyin-kmm/src/records.rs), `KmmBigram` alias.
  **C** means only the explicitly revisioned pending family C changes below.

All named TK native operations cross **FFI**. Its current allowlist has C open/close,
check, process, process-multi, process-each, set, count, is-healthy,
synchronize, rebuild, make/free iterator, iterator first/jump/process/next,
last-status readers and the two sentinel globals. There is no C `get`,
`remove`, `append`, or iterator `get` binding. The substitutions are explicit
in the following rows.

TK `db_synchronize` passes the C API `(hard, nullptr, nullptr, "")`, but that
wrapper creates a `FileProcessor` even for a null callback and calls
`SynchronizeAdvanced(hard, &processor, empty_map)`
([tkrzw_langc.cc:1334-1358](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L1334-L1358)).
The pin passes the concrete default `proc=nullptr`. TK `commit` performs this
soft sync after each nonempty ordinary `write` batch (the flexible container's `write_deferred` applies none); `compact` calls
C Rebuild with empty parameters, then hard sync. Neither sequence is an
explicit pin DBM operation sequence. `holds_no_record` adds IsHealthy, Count,
and a separate file-header read before deciding whether to skip copying.
These are existing Rust paths to audit during rebinding, not additions or
removals approved by this inventory.

### Shared copy helper and lifecycle

| Pin call site and arguments | Pin handling and lifetime | Rust counterpart and difference to carry into rebinding |
| --- | --- | --- |
| [src/storage/tkrzwdb_utils.h:48-56](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/tkrzwdb_utils.h#L48-L56): `src->MakeIterator(); First()` | Null source/destination or null iterator returns false. `First` status ignored. `unique_ptr<DBM::Iterator>` dies before either DB. | TK `visit_db` / `scan_rows` / `drive_scan`: C make-iterator, first; null/error becomes `StoreError`, NOT_FOUND means empty. Preserve each pin caller's handling rather than globally equating every error with end. |
| [src/storage/tkrzwdb_utils.h:62-73](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/tkrzwdb_utils.h#L62-L73): repeated `iter->Get(&key,&value)`, `dest->Set(key,value)`, `Next()` | Set SUCCESS is asserted and also checked; Next ignored. Final Get must be NOT_FOUND (assert and return). No sync here. | TK `visit_db` borrows rows through C iterator `Process`; copy destinations use `db_set_record` → C Set. Rust propagates errors; pin load/save callers ignore the helper's bool. Per-record order matters to file layout and Tiny hash chains. |
| [src/storage/chewing_large_table2_tkrzwdb.cpp:63-71](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L63-L71); [src/storage/phrase_large_table3_tkrzwdb.cpp:50-61](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.cpp#L50-L61); [src/storage/punct_table_tkrzwdb.cpp:43-54](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_tkrzwdb.cpp#L43-L54): reset `Synchronize(false); Close(); delete` | Both statuses ignored; owner nulled; entry storage freed afterward. Runs from destructor and before attach/load. | TK `Db::drop` closes and deletes with ignored answer, and first makes `Synchronize(false)` (answer ignored) for a system `TreeDBM` that opened successfully (`open_system`, `Db::sync_before_close`) and, in `TkrzwUnopenedHandle::drop`, for the retained Tree object whose Open failed. Every other holder, including the bigram's `HashDBM`, closes with no explicit sync. USER Rust-owned entries have their own lifetimes. |
| [src/storage/ngram_tkrzwdb.cpp:36-45](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L36-L45): reset `Close(); delete` | No explicit Synchronize; Close ignored; owner nulled. | TK `Db::drop` matches the explicit close/delete sequence through C wrapper for opened files, but PolyDBM skips concrete Close for never-opened memory DBs. |
| [src/storage/flexible_ngram_tkrzwdb.h:84-90](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L84-L90), [src/storage/flexible_ngram_tkrzwdb.h:115-116](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L115-L116): reset `Synchronize(false); Close(); delete` | Both statuses ignored, then null. | FLEX `close` and `Drop for FlexibleBigram` call `flush`, which TK implements as `Synchronize(false)`; the writes before it do not synchronize (rows below), so the one explicit sync sits at the reset boundary as in the pin. |

### Chewing index (`chewing_large_table2_tkrzwdb.cpp`)

| Pin call site and arguments | Status handling | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/chewing_large_table2_tkrzwdb.cpp:75-88](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L75-L88): reset, new Tree, `Open(dbfile,writable,options)` | Null path false; return IsOK; failed-open object retained. | CHEW via `data/dict.rs::open_tree` and TK `open`; GEN/TOOLS `Tree::create`. C open deletes failed objects; current Rust optional/error paths cannot retain this concrete Tree. |
| [src/storage/chewing_large_table2_tkrzwdb.cpp:92-108](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L92-L108): reset, new Baby; stack Tree `Open(filename,false,OPEN_NO_CREATE)`; copy; `Close()` | Open failure false with Baby still owned; copy and Close ignored; true afterward; stack Tree destroyed on every return. | USER load into Rust user-index structures via raw open/scan; TK `open_in_memory` exists for generic stores. The pin's destination survives source-open failure. Do not replace that with a failed destination object. |
| [src/storage/chewing_large_table2_tkrzwdb.cpp:111-129](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L111-L129): `RemoveFile`; stack Tree `Open(new_filename,true,OPEN_DEFAULT)`; copy; `Synchronize(false); Close()` | Null member false; remove must be SUCCESS or NOT_FOUND; open checked; copy/sync/close ignored. | USER `stage_dbm` → TK `write_user_index`: remove, tree open, one C Set per row, soft sync, Drop close. Rust reports Set/sync failures; pin helper asserts Set but caller ignores copy bool and sync. |
| [src/storage/chewing_large_table2_tkrzwdb.cpp:143-159](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L143-L159): `Get(index bytes, &value)` | Any non-OK → SEARCH_NONE; success sets CONTINUED, even empty prefix value. | CHEW `search`/`search_continued` → `RawChewingDbm::get` → TK `db_get` (`Process`, read-only, `get_value` callback). Miss becomes false; other errors propagate to upper layer. |
| [src/storage/chewing_large_table2_tkrzwdb.cpp:269-310](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L269-L310): Get full key; Set new/existing chunk; for shorter keys Get(key,nullptr), Set(key,empty view) | Any Get failure enters new-entry branch; prefix Get success stops. Each Set failure → ERROR_FILE_CORRUPTION. Set overwrites; empty value is a real record. | GEN `add_index`/`add_index_space` → TOOLS get/put; USER changes indexes in Rust and persists through TK `write_user_index`. Main TOOLS put batches with ProcessMulti + soft sync; C's Tree put uses direct `tool_set`. |
| [src/storage/chewing_large_table2_tkrzwdb.cpp:359-378](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L359-L378): Get; Set modified chunk | Failed/short Get → REMOVE_ITEM_DONOT_EXISTS; local remove error returned; Set error → FILE_CORRUPTION. Does not native-Remove a now-empty prefix row. | USER index editing / persistence and TK raw write paths; keep empty row distinct from removal. |
| [src/storage/chewing_large_table2_tkrzwdb.cpp:419-490](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L419-L490): stack `MaskOutProcessor2`; `ProcessEach(&processor,true)`; `Synchronize(false)` | Null DB false; both statuses ignored, then true. ProcessFull copies/masks entry and returns its bytes; invalid key-derived length aborts; ProcessEmpty returns NOOP. | USER masking is Rust product logic and later persistence; generic TK `write` uses buffered ProcessMulti, not this native writable walk. Callback-return storage belongs to the entry and survives native consumption. |
| [src/storage/chewing_large_table2_tkrzwdb.cpp:509-538](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_tkrzwdb.cpp#L509-L538): `Get(query,nullptr)`; MakeIterator; `Jump(query)`; initial Next; Get loop; later Next | Existence/Jump/initial Next non-OK returns SEARCH_NONE; Get non-OK ends; loop Next ignored. No null-iterator check here. Prefix filter can end early. | CHEW suggestion path → `contains`/`walk` → TK `db_check`, `drive_scan`; bound filtering substitutes for initial-next sequencing, iterator Process for Get. CXX must retain native statuses long enough to make the caller's exact choice. |

### Phrase index (`phrase_large_table3_tkrzwdb.cpp`)

| Pin call site and arguments | Status handling | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/phrase_large_table3_tkrzwdb.cpp:65-79](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.cpp#L65-L79): reset, new Tree, Open with attach options | Null filename false; IsOK; retain failed-open Tree. | PHRASE through `data/dict.rs::open_phrase_table`, TK `open`; TOOLS Tree for generators/KMM phrase lookup. C factory loses failed object. |
| [src/storage/phrase_large_table3_tkrzwdb.cpp:83-99](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.cpp#L83-L99): new Baby, stack Tree Open(false,NO_CREATE), copy, Close | Open checked; copy/close ignored; Baby retained on failure. | USER `load_user_phrase_index` and session Rust structures; generic TK Baby path is available, not a literal one-to-one owner translation. |
| [src/storage/phrase_large_table3_tkrzwdb.cpp:102-120](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.cpp#L102-L120): RemoveFile; Tree Open(true,DEFAULT); copy; soft sync; Close | Null member false; remove/open checked; remaining statuses ignored. | USER `stage_dbm` → TK `write_user_index`; same structural save sequence, differing failure propagation as above. |
| [src/storage/phrase_large_table3_tkrzwdb.cpp:133-150](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.cpp#L133-L150): Get(raw UCS-4 key,&value) | Non-OK SEARCH_NONE; empty value still CONTINUED. | PHRASE `search_key` → CHEW raw adapter → TK `db_get`/Process. Preserve raw key bytes and empty prefix records. |
| [src/storage/phrase_large_table3_tkrzwdb.cpp:164-186](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.cpp#L164-L186): Get(query,nullptr), MakeIterator, Jump, first Next, Get loop, Next | Same checked/ignored split as chewing suggestion; prefix predicate uses byte prefix. | PHRASE `search_suggestion` → raw contains/scan; TK C check and iterator Process substitutions. |
| [src/storage/phrase_large_table3_tkrzwdb.cpp:199-239](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.cpp#L199-L239): Get, Set new entry, prefix Get(nullptr)/Set(empty), Set replacement | Same new-entry-on-any-Get-failure and Set-to-FILE_CORRUPTION rules as chewing. | GEN `add_phrase_index`, TOOLS Tree; USER phrase-index updates then persistence. Main per-write ProcessMulti/sync, C direct Tree Set. |
| [src/storage/phrase_large_table3_tkrzwdb.cpp:250-272](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.cpp#L250-L272): Get, Set reduced value | Get fail or shorter than token → REMOVE_ITEM_DONOT_EXISTS; Set fail → FILE_CORRUPTION. | USER phrase edits / raw writes; no native Remove in this pin method. |
| [src/storage/phrase_large_table3_tkrzwdb.cpp:277-314](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_tkrzwdb.cpp#L277-L314): stack MaskOutProcessor; ProcessEach(true), Synchronize(false) | Both statuses ignored; null DB false. Full callback returns entry-owned replacement; empty callback NOOP. | USER masking and persistence; generic TK transaction path uses ProcessMulti. Needs typed virtual processor adapter only if this native traversal is exposed. |

### Bigram (`ngram_tkrzwdb.cpp`)

| Pin call site and arguments | Status handling | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/ngram_tkrzwdb.cpp:48-62](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L48-L62): new Tiny, stack Hash Open(false,NO_CREATE), copy, Close | Open failure false but Tiny retained; copy/Close ignored. | TK `TkrzwUserBigramDb::load_db`: constructs Tiny through C factory, opens Hash, uses C iterator Process and direct C Set; keeps empty destination if open fails. `holds_no_record` can skip the walk; read errors print `user-bigram load …` and keep partial contents. This is an existing Rust diagnostic/control-flow difference to review, not new parity evidence. |
| [src/storage/ngram_tkrzwdb.cpp:65-83](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L65-L83): RemoveFile; Hash Open(true,DEFAULT); copy; Synchronize(false); Close | Null owner false; remove/open checked; copy/sync/close ignored. | TK `TkrzwUserBigramDb::save_db`, C extracts `save_bigram_db`; direct Set per row, then checked soft sync and ignored close through Drop. |
| [src/storage/ngram_tkrzwdb.cpp:86-98](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L86-L98): reset, new Hash, Open with attach options | Null path false; IsOK; retains failed-open Hash. | GRAM `open`, TOOLS Hash → TK hash open helpers; current tkrzw failed-open tools path becomes Missing/error, unlike the retained concrete Hash. #1516 must preserve native state via CXX, not synthetic answers. |
| [src/storage/ngram_tkrzwdb.cpp:102-120](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L102-L120): Get(native token bytes,&value) | Null DB/non-OK false; copies to reusable chunk; SingleGram may borrow chunk when copy=false. | GRAM/raw store; TK user-bigram get → db_get Process callback. Rust owns retained bytes/decoded grams; native callback borrow cannot escape. |
| [src/storage/ngram_tkrzwdb.cpp:124-140](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L124-L140): Set(token,gram bytes), Remove(token) | Null false; return IsOK, so missing Remove is false. | TK user-bigram store/remove → db_set_one → ProcessMulti. Remove-on-absent is NOOP success at this layer; callers may precheck. TOOLS Hash main put additionally soft-syncs; C test-ngram uses direct tool_set. |
| [src/storage/ngram_tkrzwdb.cpp:143-171](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L143-L171): stack KeyCollectProcessor; ProcessEach(false) | Key length must be sizeof(token), assert; callbacks return NOOP; return native IsOK. | TK `TkrzwUserBigramDb::keys` reserves via extra Count, then C ProcessEach; invalid key becomes `None` for the upper gate. TOOLS/GRAM `native_keys` uses `process_each_keys`. No native ordering normalization is permitted. |
| [src/storage/ngram_tkrzwdb.cpp:175-210](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_tkrzwdb.cpp#L175-L210): get_all_items, then remove/load/store | Initial walk failure false; every selected remove/load/store is `check_result` (assert in reference build); no explicit sync. | USER masking through `ContainerGrams` and user store logic; C `utils/dbm_tests/ngram.rs` exercises these operations. Preserve check points and preceding writes on abort. |

### Punctuation (`punct_table_tkrzwdb.cpp`)

| Pin call site and arguments | Status handling | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/punct_table_tkrzwdb.cpp:58-72](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_tkrzwdb.cpp#L58-L72): reset, Tree Open with attach options | Null path false; compare SUCCESS; retain failed-open Tree. | PUNCT open/open_optional → TK open; optional path can precheck file and replace failed DB with an empty Rust table. TOOLS Tree for generator; C test tree attachment. |
| [src/storage/punct_table_tkrzwdb.cpp:77-93](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_tkrzwdb.cpp#L77-L93): new Baby, stack Tree Open(filename,false,OPEN_DEFAULT), copy, Close | Note DEFAULT, not NO_CREATE. Open checked; copy/Close ignored; Baby remains on failure. | C TOOLS `Tree::load_db` uses create_in_memory, ordinary read-only open and scan/write. Main has no punctuation snapshot-load tool path. Ordinary TK read-only open adds NO_CREATE; preserve the pin's actual option in the bridge. |
| [src/storage/punct_table_tkrzwdb.cpp:96-111](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_tkrzwdb.cpp#L96-L111): RemoveFile, Tree Open(true,DEFAULT), copy, soft sync, Close | No null-member guard before remove/open; copy can return false on null source and is ignored; sync/close ignored. | C TOOLS Tree save → TK write_user_index and source flush. Preserve the pin's side effects even when source is absent; do not infer chewing/phrase's null guard. |
| [src/storage/punct_table_tkrzwdb.cpp:114-136](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_tkrzwdb.cpp#L114-L136): clear chunk, Get(token,&value) | NOT_FOUND **or value size zero** returns true before other status check; nonempty non-SUCCESS false. | PUNCT `get_all`/TK db_get propagates backend errors before its empty check; GEN append / TOOLS get collapse errors to missing. The exact status/empty-value order needs caller-specific treatment. |
| [src/storage/punct_table_tkrzwdb.cpp:139-148](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_tkrzwdb.cpp#L139-L148): Set(token,entry bytes) | Null false; return SUCCESS comparison. | GEN `append_punctuation` on main; C `utils/storage/punctuation.rs` factors append/remove and TOOLS Tree put uses native tool_set. |
| [src/storage/punct_table_tkrzwdb.cpp:151-159](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_tkrzwdb.cpp#L151-L159): Remove(token) | SUCCESS **or NOT_FOUND** true. This intentionally differs from Bigram/FlexibleBigram Remove. | Generic TK buffered remove has idempotent semantics; later bridge must retain raw status and let this caller accept NOT_FOUND. |
| [src/storage/punct_table_tkrzwdb.cpp:162-184](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_tkrzwdb.cpp#L162-L184): MakeIterator, First, Get(&key), Next | Null iterator false; First/Next ignored; any non-OK Get ends successfully; assert four-byte key. | C TOOLS Tree native_keys uses TK ProcessEach rather than pin's iterator; records/status behavior must be reviewed separately. No key/value C allocations currently cross FFI. |

`PunctTable::append_punctuation` and `remove_punctuation` perform
`load_entry`, local chunk edit, `store_entry`; text load ignores each append
answer ([src/storage/punct_table.cpp:148-190](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table.cpp#L148-L190)). They do **not** call native
`DBM::Append`. The entire census has no explicit pin `Append`, `Process`,
`ProcessMulti`, `Count`, `IsHealthy`, `Rebuild`, hard synchronization, iterator
Set/Remove, async DBM, transaction API, or tuning setter. Some of these are
native implementation callees of Get/Set/Remove; others are Rust additions.
Absence from the pin census is not permission to delete an in-tree Rust
capability without auditing its callers.

### FlexibleBigram (`flexible_ngram_tkrzwdb.h`)

| Pin call site and arguments | Status handling and ownership | Rust counterpart and difference |
| --- | --- | --- |
| [src/storage/flexible_ngram_tkrzwdb.h:128-159](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L128-L159): reset; allocate Hash; Open(path,writable,options OR NO_CREATE); if failed and CREATE, Open same object with options | Without CREATE after failure, or after retry failure: delete and null. **No explicit Close at these delete sites.** Null filename returns before allocation. | Main FLEX open uses metadata to decide fresh and one create-capable C open; read-only open uses C hash/no-create. C `FlexibleBackend::open_flexible` makes two C opens when needed; each failed C open already destroyed its object, so retry uses a new PolyDBM/Hash. This is a binding difference, not the BDB misuse ruling applied by analogy. |
| [src/storage/flexible_ngram_tkrzwdb.h:161-167](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L161-L167): after successful retry, Set(two-null-token key, four magic bytes) | Set status ignored; return true regardless. | Main checks signature write; C `put_flexible_signature` → TK db_set_record, shared attach intentionally ignores tkrzw Set answer. |
| [src/storage/flexible_ngram_tkrzwdb.h:170-187](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L170-L187): successful first Open, Get(eight-byte magic key,&string), compare first four bytes | Failed Get or bad signature false **with opened Hash retained**. Short values reach an unsafe read; do not infer that every false attach means null owner. | Main FLEX open returns Err and drops local store; KMM Database `.ok()` loses it. C attach retains the handle and stores attach result separately in KMM Database. Existing short-input class-(b) obligations remain; this inventory admits no new one. |
| [src/storage/flexible_ngram_tkrzwdb.h:200-224](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L200-L224): Get(four-byte token,&string) | Null/non-OK false; output null first; copy to chunk, construct borrowed/copy gram. | FLEX load/read_gram → get_bigram_raw → TK db_get; KMM load vs load_checked determines ignore/abort. Rust validates record layout before decoding. |
| [src/storage/flexible_ngram_tkrzwdb.h:236-246](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L236-L246): Set(token,entire gram) | Null false, otherwise IsOK. | FLEX write_value → write_deferred (main); C FlexibleBackend put_flexible_value defaults to same on TK. FLEX write_value → `write_deferred`. TK issues the pin's call, one `DBM::Set(key, value)` per buffered put in framed-key order (`crates/oxpinyin-store/src/tkrzw/mod.rs`, `commit` with `sync` false), and no `Synchronize`; the sync is the reset boundary's `flush`. Ordinary `WriteStore::write` still ends a non-empty batch with `Synchronize(false)`. |
| [src/storage/flexible_ngram_tkrzwdb.h:257-263](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L257-L263): Remove(token) | Null false, otherwise IsOK; missing false. | FLEX remove prechecks existence and read-only, then `delete_value` → `write_deferred` → one `DBM::Remove(key)` with no `Synchronize`, as for Set. The existence precheck is an extra Get with a distinct absent-status path. |
| [src/storage/flexible_ngram_tkrzwdb.h:39-60](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L39-L60), [src/storage/flexible_ngram_tkrzwdb.h:274-281](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L274-L281): ProcessEach(FlexibleKeyCollectProcessor,false) | Clears array; null false; full callback skips all keys not four bytes, returns NOOP; empty callback NOOP; return IsOK. | FLEX get_all_items → TK native_keys/process_each_keys; key lengths filtered in Rust. Main maps backend error to training-db exit inherited from BDB, not tkrzw's false; C only adds a failed-open guard. Do not treat a shared trait default as proof of the tkrzw error contract. |
| [src/storage/flexible_ngram_tkrzwdb.h:292-322](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L292-L322): Get(magic key,&string) | Zero header first. Failed Get false. Wrong successful size asserts size==4 then false; full expected size asserts magic then copies header. | FLEX get_magic_header: main only size-checks; C restores tkrzw size/magic aborts. Raw status and exact bytes must remain distinguishable. |
| [src/storage/flexible_ngram_tkrzwdb.h:333-356](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L333-L356): Set(magic key,magic+header bytes) | Null false; IsOK. | FLEX set_magic_header → write_value; KMM ignored/checked variants. Same deferred write as gram Set. |
| [src/storage/flexible_ngram_tkrzwdb.h:368-390](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L368-L390): Get(token,&string) | Zero header first; null/non-OK false; assert stored size >= header, then copy. | FLEX get_array_header uses Rust `assert!`; its panic/abort boundary remains a caller-review obligation. C retains this assertion. This inventory declares no malformed-header closure. |
| [src/storage/flexible_ngram_tkrzwdb.h:402-429](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/flexible_ngram_tkrzwdb.h#L402-L429): Get(token,&string); overwrite header; Set(token,buffer,vsiz) | Any Get failure sets output size to header size; success preserves original value length, including a successful short record. Set IsOK returned. | FLEX set_array_header grows a successful short value to header size and propagates some Get errors; pin uses the original successful length. This is a source-visible difference to resolve/retain under an applicable existing record, not blanket equivalence. |

## Retained objects and teardown are part of the inventory

| Event | Pin object after return | Teardown obligation |
| --- | --- | --- |
| Ordinary Tree/Hash attach has null filename | No DB after reset. | Previous object's reset already ran. |
| Ordinary Tree/Hash attach Open returns failure | The newly allocated concrete object is still owned. | Next reset/destructor invokes the caller's explicit sync (if any), Close, then delete, regardless of Open's return code. |
| Index/punct/Bigram load source Open fails | Empty Baby/Tiny destination remains; stack source dies immediately. | Temporary destructor follows its actual native state; destination follows owner reset later. |
| Flexible first Open fails, CREATE absent | Hash deleted, owner null. | Destructor of the actual concrete object; no synthetic sync/close at the pin call site. |
| Flexible first Open fails, CREATE present | Same Hash object receives second Open. | Retry success writes signature; retry failure deletes and nulls. |
| Flexible first Open succeeds, signature Get/compare fails | Opened Hash retained even though attach returns false. | Later Get/Set/Remove/walk still reach it; reset syncs/closes/deletes. |

The C factory destroys failed concrete Tree/Hash allocations inside
`PolyDBM::OpenAdvanced` ([tkrzw_dbm_poly.cc:384-423](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_poly.cc#L384-L423)), then deletes the outer
object when OpenAdvanced returns a non-SUCCESS status
([tkrzw_langc.cc:556-570](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L556-L570)). This prevents Rust from keeping the pin's object.
Concrete file methods often return PRECONDITION for an unopened DB
([tkrzw_dbm_hash.cc:384-408](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L384-L408), [tkrzw_dbm_tree.cc:511-568](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tree.cc#L511-L568)), but that is not a
license to implement an all-purpose Rust "failed" state. Hash `OpenImpl`
sets `open_` before outer restoration finishes
([tkrzw_dbm_hash.cc:1230-1234](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L1230-L1234)); later truncate-failure branches close the file
and return without the ordinary CloseImpl reset
([tkrzw_dbm_hash.cc:306-310](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L306-L310), [tkrzw_dbm_hash.cc:325-329](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L325-L329)).
Retaining the real object avoids inventing uniform post-failure state.

Concrete destructors conditionally Close when their own state says open, then
invalidate iterators/free memory ([tkrzw_dbm_hash.cc:248-254](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L248-L254),
[tkrzw_dbm_tree.cc:382-388](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tree.cc#L382-L388), [tkrzw_dbm_baby.cc:359-366](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_baby.cc#L359-L366),
[tkrzw_dbm_tiny.cc:220-228](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tiny.cc#L220-L228)). Explicit pin Close happens **before** deletion;
destruction is not a replacement for that call or its position. Close itself
can write metadata; Hash CloseImpl updates the timestamp and metadata before
resetting state ([tkrzw_dbm_hash.cc:1237-1267](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L1237-L1267)).

Never-opened Baby/Tiny objects are usable memory databases. Baby Synchronize
returns success without file I/O, while Close returns PRECONDITION if it was
never opened ([tkrzw_dbm_baby.cc:394-415](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_baby.cc#L394-L415), [tkrzw_dbm_baby.cc:583-596](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_baby.cc#L583-L596);
Tiny equivalent [tkrzw_dbm_tiny.cc:256-278](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tiny.cc#L256-L278), [tkrzw_dbm_tiny.cc:455-468](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tiny.cc#L455-L468)).
The pin ignores these reset results. PolyDBM's empty-path construction skips
concrete Open and its Close skips concrete Close if `open_` is false, then
destroys the concrete object ([tkrzw_dbm_poly.cc:443-489](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_poly.cc#L443-L489),
[tkrzw_dbm_poly.cc:570-587](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_poly.cc#L570-L587)). A future bridge should expose construction
without requiring a file-open success, and preserve explicit calls even when
their ignored statuses differ from a convenience wrapper.

## Utilities, tests and facade reachability

These are callers of the native rows above, not additional native DBM APIs.
`check_result` is an assert in the reference build
([src/include/pinyin_utils.h:27-31](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/include/pinyin_utils.h#L27-L31)). Ignored attach must remain distinct from
checked attach, and unchecked uses can still reach later checked operations.

| Pin file/call sites | Reach and handling | Rust destination |
| --- | --- | --- |
| [src/storage/facade_chewing_table2.h:75-96](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/facade_chewing_table2.h#L75-L96); [src/storage/facade_phrase_table3.h:74-95](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/facade_phrase_table3.h#L74-L95) | Own system and user tables; system attach READONLY, user load_db; return accumulated bool; store uses user save_db. Reset deletes both owners. | `data/dict.rs`, CHEW/PHRASE, USER load/stage methods. |
| [src/pinyin.cpp:351-440](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L351-L440), [src/pinyin.cpp:1014-1022](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L1014-L1022), [src/pinyin.cpp:1205-1215](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/pinyin.cpp#L1205-L1215); [src/zhuyin.cpp:295-343](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/zhuyin.cpp#L295-L343), [src/zhuyin.cpp:680-690](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/zhuyin.cpp#L680-L690), [src/zhuyin.cpp:744-748](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/zhuyin.cpp#L744-L748) | Init ignores table/bigram attach/load answers; pinyin also attaches punctuation; save calls user DB writers; fini deletes owners. | Shared engine/session loader, data tables and USER; exported facade glue remains in the capi crates. |
| [src/storage/chewing_large_table2.cpp:112-218](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2.cpp#L112-L218); [src/storage/phrase_large_table3.cpp:125-140](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3.cpp#L125-L140); [src/storage/punct_table.cpp:148-190](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table.cpp#L148-L190); [src/storage/tag_utility.cpp:335-354](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/tag_utility.cpp#L335-L354) | Text readers and public add/remove/search/append delegate to backend methods; tag lookup uses phrase search. No DBM Append. | GEN index/punct encoding; CHEW/PHRASE/PUNCT; `kmm/taglib.rs` and `kmm/phrase.rs`. |
| [utils/storage/gen_binary_files.cpp:41-70](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/storage/gen_binary_files.cpp#L41-L70), [utils/storage/gen_binary_files.cpp:89-104](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/storage/gen_binary_files.cpp#L89-L104) | Index attach RW+CREATE ignored; load_text fills them; punct attach checked before text load. | GEN and TOOLS Tree; C extracts punctuation operations into `utils/storage/punctuation.rs`. |
| [utils/storage/import_interpolation.cpp:259-280](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/storage/import_interpolation.cpp#L259-L280), [utils/storage/import_interpolation.cpp:158-231](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/storage/import_interpolation.cpp#L158-L231) | Phrase READONLY and Bigram RW+CREATE attaches checked; parse reads/replaces grams. | `utils/storage/import_interpolation.rs`, TOOLS Tree/Hash. |
| [utils/storage/export_interpolation.cpp:63-64](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/storage/export_interpolation.cpp#L63-L64), [utils/storage/export_interpolation.cpp:106-143](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/storage/export_interpolation.cpp#L106-L143) | Bigram READONLY attach ignored; native key walk and gram loads. | `utils/storage/export_interpolation.rs`, TOOLS Hash. |
| [utils/training/gen_ngram.cpp:74-116](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/gen_ngram.cpp#L74-L116); [utils/training/gen_deleted_ngram.cpp:75-114](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/gen_deleted_ngram.cpp#L75-L114) | Bigram RW+CREATE attach, loads and stores ignored; repeated updates, owner closes at end. | Corresponding `utils/training` modules, TOOLS Hash. |
| [utils/training/estimate_interpolation.cpp:107-125](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/estimate_interpolation.cpp#L107-L125) | Two Bigram READONLY attaches ignored; enumerate deleted and load both. | `utils/training/estimate_interpolation.rs`, TOOLS Hash. |
| [utils/training/eval_correction_rate.cpp:144-157](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/eval_correction_rate.cpp#L144-L157); [utils/segment/ngseg.cpp:155-170](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/segment/ngseg.cpp#L155-L170); [utils/segment/spseg.cpp:250-296](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/segment/spseg.cpp#L250-L296); [utils/segment/mergeseq.cpp:249-260](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/segment/mergeseq.cpp#L249-L260) | Facade index load, searches/lookup; evaluation/ngseg also system Bigram; null-path user attach or null initial Bigram. | Corresponding utils modules, `segment/data.rs`, data lookup tables and bigram reads. |
| [utils/training/k_mixture_model.h:156-159](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/k_mixture_model.h#L156-L159) | FlexibleBigram specialization with KMM headers/items, same native Hash operations. | KMM records `KmmSpec` / `KmmBigram`, FLEX. |
| [utils/training/gen_k_mixture_model.cpp:246-308](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/gen_k_mixture_model.cpp#L246-L308), [utils/training/gen_k_mixture_model.cpp:336-382](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/gen_k_mixture_model.cpp#L336-L382) | Phrase READONLY and KMM RW+CREATE attach ignored; load chooses existing/new; store and selected magic reads/writes checked; some array-header updates ignored. | `kmm/generate.rs`, KMM `Database` checked/unchecked methods, FLEX. |
| [utils/training/import_k_mixture_model.cpp:72-98](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/import_k_mixture_model.cpp#L72-L98), [utils/training/import_k_mixture_model.cpp:136-247](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/import_k_mixture_model.cpp#L136-L247), [utils/training/import_k_mixture_model.cpp:281-293](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/import_k_mixture_model.cpp#L281-L293) | Phrase and KMM attaches ignored; header writes, loads/stores mostly ignored. | `kmm/import.rs`, `kmm/phrase.rs`, KMM Database. |
| [utils/training/export_k_mixture_model.cpp:36-92](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/export_k_mixture_model.cpp#L36-L92), [utils/training/export_k_mixture_model.cpp:142-148](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/export_k_mixture_model.cpp#L142-L148) | KMM READONLY attach explicitly checked; missing header branches; walks ignored, array-header/load checked. | `kmm/export.rs`, KMM Database; C distinguishes `attached()` from retained `is_open()`. |
| [utils/training/merge_k_mixture_model.cpp:96-193](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/merge_k_mixture_model.cpp#L96-L193), [utils/training/merge_k_mixture_model.cpp:225-231](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/merge_k_mixture_model.cpp#L225-L231) | Target RW+CREATE and each source READONLY attach ignored; source/header/load and merged writes checked at their individual sites, some target stores ignored. | `kmm/merge.rs`, KMM Database. |
| [utils/training/prune_k_mixture_model.cpp:128-185](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/prune_k_mixture_model.cpp#L128-L185) | RW **without CREATE**, attach ignored; header/read branches, walks/loads/store ignored, selected array-header/remove calls checked. | `kmm/prune.rs`, KMM Database; main uses create-capable open, C introduces `open_read_write_existing`. |
| [utils/training/validate_k_mixture_model.cpp:28-91](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/validate_k_mixture_model.cpp#L28-L91), [utils/training/validate_k_mixture_model.cpp:159-160](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/validate_k_mixture_model.cpp#L159-L160) | READONLY attach ignored; missing magic header handled; walks ignored, array-header and load checked. | `kmm/validate.rs`, KMM Database. |
| [utils/training/estimate_k_mixture_model.cpp:112-130](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/utils/training/estimate_k_mixture_model.cpp#L112-L130) | Two READONLY attaches ignored; deleted-key enumeration and both loads, including copy=true on source. | `kmm/estimate.rs`, KMM Database. |
| [tests/storage/test_ngram.cpp:39-84](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/storage/test_ngram.cpp#L39-L84) | Checked RW+CREATE attach; Set/Get/enumeration; checked save/load snapshot; mask_out. | C `utils/dbm_tests/ngram.rs` and `bin/test_ngram.rs`; absent on main. |
| [tests/storage/test_flexible_ngram.cpp:59-127](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/storage/test_flexible_ngram.cpp#L59-L127) | Checked attach; gram store/load/walk; magic and array headers; checked Remove. | C `utils/dbm_tests/flexible.rs`, `bin/test_flexible_ngram.rs`; main `store/tests/flexible_bigram.rs` is related coverage, not that drop-in program. |
| [tests/storage/test_punct_table.cpp:25-77](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/storage/test_punct_table.cpp#L25-L77) | Checked attach and snapshot save/load; append/remove punctuation and native key enumeration. | C `utils/dbm_tests/punct.rs`, `bin/test_punct_table.rs`, `utils/storage/punctuation.rs`; absent on main. |
| [tests/tests_helper.h:51-80](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/tests_helper.h#L51-L80); [tests/storage/test_chewing_table.cpp:44-110](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/storage/test_chewing_table.cpp#L44-L110); [tests/storage/test_phrase_table.cpp:23-96](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/storage/test_phrase_table.cpp#L23-L96) | Text loaders populate default Baby objects, search and mask; old chunk store/load blocks are `#if 0` at chewing 55-59 and phrase 34-38 and add no call. | Data/unit test coverage and GEN helpers; no claim that these programs are ported by C. |
| [tests/storage/test_matrix.cpp:99-105](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/storage/test_matrix.cpp#L99-L105); [tests/lookup/test_pinyin_lookup.cpp:48-60](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/lookup/test_pinyin_lookup.cpp#L48-L60); [tests/lookup/test_phrase_lookup.cpp:71-84](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/lookup/test_phrase_lookup.cpp#L71-L84) | Facade loading/search, Bigram system attaches and user null state. | Existing engine/data/store test suites; port/gap status remains in the upstream test coverage ledger. |

`test_table_info` has no DBM handle operations; merely adding it beside these
programs does not add native calls. Test registration is
[tests/storage/Makefile.am:31-46](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/storage/Makefile.am#L31-L46) and [tests/lookup/Makefile.am:31-35](https://github.com/shenghaoc/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/tests/lookup/Makefile.am#L31-L35).
Family E and other coverage gaps are unchanged. Further consumers call these
same facade methods; see the existing
[`axis-j-tkrzw-ledger`](axis-j-tkrzw-ledger.md) for C API root reachability.

## Pending family C: preserve and port, do not rewrite here

At the inspected #1528 revision:

- TK `open_hash_with` and `FlexibleBackend::open_flexible` retain the C API and
  implement no-create then optional create retry. Terminal failure has no
  tkrzw handle. A successful open with failed signature remains in FLEX
  `attach`; KMM Database keeps both the container and its attach answer.
- `put_flexible_signature` is direct C Set; the shared attach ignores its
  answer for tkrzw. The other TK flexible write/remove trait methods still
  inherit transaction defaults. Do not describe all flexible writes as
  converted to plain Set by C.
- The tools-only TK `tool_set` calls existing `db_set_record` without sync.
  C TOOLS Tree put and `dbm_tests/ngram.rs` use it. Shipping `write`, `commit`
  and their durability are unchanged. TK `save_bigram_db` shares the existing
  snapshot implementation with the new test tool.
- C adds the three DBM test programs, punctuation edit helpers, shared
  FlexibleBigram attach/header handling, and KMM caller corrections. Its
  tools cases and diagnostics are migration inputs, not evidence that every
  native lifetime/status gap is closed.

Source links: [TK at C](https://github.com/shenghaoc/oxpinyin/blob/473c1654d381881041ad46fe0b5a3e0eb593da23/crates/oxpinyin-store/src/tkrzw/mod.rs),
[FLEX at C](https://github.com/shenghaoc/oxpinyin/blob/473c1654d381881041ad46fe0b5a3e0eb593da23/crates/oxpinyin-store/src/flexible_ngram.rs),
[TOOLS at C](https://github.com/shenghaoc/oxpinyin/blob/473c1654d381881041ad46fe0b5a3e0eb593da23/crates/oxpinyin-utils/src/storage/dbm.rs),
[KMM at C](https://github.com/shenghaoc/oxpinyin/blob/473c1654d381881041ad46fe0b5a3e0eb593da23/crates/oxpinyin-kmm/src/database.rs),
[ngram test at C](https://github.com/shenghaoc/oxpinyin/blob/473c1654d381881041ad46fe0b5a3e0eb593da23/crates/oxpinyin-utils/src/dbm_tests/ngram.rs).
The later CXX PR must port these approved paths. This inventory makes no edit
to them or to KC's `shim.h`, `shim.cc`, `ffi.rs`, or build configuration.

## Status and exception channels

Native Get/Set/Remove return `Status`; library wrappers construct processors
and combine their result with the underlying operation's status
([tkrzw_dbm.h:1031-1045](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm.h#L1031-L1045), [tkrzw_dbm.h:1113-1131](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm.h#L1113-L1131),
[tkrzw_dbm.h:1189-1203](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm.h#L1189-L1203); Hash specializations
[tkrzw_dbm_hash.cc:2427-2455](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L2427-L2455)). Iterator Get similarly wraps a processor
([tkrzw_dbm.h:822-829](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm.h#L822-L829)). `Process` succeeding does not itself mean a key was
present: Rust `get_value` tracks callback visitation for that reason. A Remove
processor that simply returns REMOVE for an absent key is not equivalent to
native Remove's NOT_FOUND result unless its caller intentionally ignores it.

Ordinary OS failures are statuses, e.g. mmap-file open/fcntl/fstat/mmap return
`GetErrnoStatus` ([tkrzw_file_mmap.cc:110-177](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_file_mmap.cc#L110-L177)); `GetErrnoStatus` classifies
errno ([tkrzw_lib_common.cc:247-278](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_lib_common.cc#L247-L278)). Allocation can throw: `xmalloc`,
`xcalloc`, `xrealloc` explicitly throw `std::bad_alloc`
([tkrzw_lib_common.h:147-183](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_lib_common.h#L147-L183)), and native objects/strings/vectors allocate.
The pin does not invoke `Status::OrDie` and has no catch around the listed
DBM calls. `OrDie` is a separate throwing API
([tkrzw_lib_common.cc:240-245](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_lib_common.cc#L240-L245)), not the semantics of `IsOK` or SUCCESS tests.

The current C wrappers catch **`const std::exception&`**, not a blanket
catch-all: open, process, check, set, process-multi, process-each, synchronize
and iterator creation all set thread-local SYSTEM_ERROR and return a C
failure value ([tkrzw_langc.cc:530-620](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L530-L620), [tkrzw_langc.cc:684-702](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L684-L702),
[tkrzw_langc.cc:1031-1054](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L1031-L1054), [tkrzw_langc.cc:1214-1227](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L1214-L1227),
[tkrzw_langc.cc:1334-1358](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L1334-L1358), [tkrzw_langc.cc:1519-1537](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L1519-L1537)).
TK `last_status` copies the thread-local message immediately; `status_error`
maps SYSTEM_ERROR to Io and other errors to Backend; `check_end` treats only
NOT_FOUND as normal end. Thus exceptions and system-origin statuses cannot be
distinguished reliably through this ABI. Do not infer origin by parsing text.

Close and the bound iterator first/jump/next/process operations use the same
`std::exception` → SYSTEM_ERROR catch
([tkrzw_langc.cc:574-585](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L574-L585),
[tkrzw_langc.cc:1540-1576](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L1540-L1576),
[tkrzw_langc.cc:1611-1648](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L1611-L1648)).
The extra Count path has a distinct wrinkle: native status failure returns
-1, but a caught exception returns **0** while setting SYSTEM_ERROR
([tkrzw_langc.cc:1230-1243](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L1230-L1243)).
TK `db_count` accepts any nonnegative value, so it does not consult that status
on the exception return. IsHealthy returns false on a caught exception
([tkrzw_langc.cc:1464-1472](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L1464-L1472)).
These are source-derived exceptional paths, not exercised cases or closure
evidence.

[Row 40](compatibility-policy.md#registered-standing-divergences--outside-the-classes)
and its [historical binding finding](tkrzw-langc-exception-classification.md)
remain standing; this source inventory does not close, reclassify, or erase
them. CXX can transport a caught exception in a separate `Result` channel;
that capability is not evidence that a future binding has already preserved
caller behavior. Native non-success statuses must remain data, independent of
the exception channel. Exception/destructor behavior, allocation failure and
the public abort boundary require explicit review in the implementation PR.

## Proposed bridge surface, not an implementation decision

Expose opaque concrete `TreeDBM`, `HashDBM`, `BabyDBM`, `TinyDBM` owners and a
DB-borrowing cursor, with separate construction and Open. A small Rust enum
may choose among concrete owners; it must not replace the native object's
post-failure state. Candidate operations are:

| Proposed types/functions | Purpose / minimum semantic adapter |
| --- | --- |
| `new_tree`, `new_hash`, `new_baby`, `new_tiny` → `Result<UniquePtr<Concrete>>` | Preserve concrete defaults and never-opened memory DBs. Tiny/Baby do not need a dummy file-open result. |
| Concrete `open(path bytes,writable,options)` → `Result<NativeStatus>` | Native Open failure leaves the same owner available. Byte-to-native-string conversion only; no PolyDBM map parsing. Path bytes and embedded-NUL policy must be reviewed at the caller boundary. |
| `get`, `contains` (`Get` with null destination), `set`, `remove` → `Result<NativeStatus>` with owned result bytes or scoped sink | Preserve status codes/messages and optional output semantics, default overwrite=true; no invented not-found conflation. A byte-view adapter is needed because Rust slices and `std::string_view` are different boundary types. |
| `make_iterator` → `Result<UniquePtr<Cursor<'db>>>`; `first`, `jump`, `next`, `get`/scoped read | Cursor owns native iterator and borrows DB. Preserve which statuses each pin caller ignores. Do not return a cursor detached from its DB lifetime. |
| `process_each` with a scoped Rust visitor / mutator; optional `process_multi` for existing Rust transactions | A small C++ `RecordProcessor` subclass supplies virtual ProcessFull/ProcessEmpty and keeps replacement storage valid. It forwards calls; it does not port tkrzw algorithms or libpinyin product logic into the adapter. |
| `synchronize(hard)`, `close` → `Result<NativeStatus>`; owner destruction separately | Caller controls explicit sync/close/delete order. Generated UniquePtr destruction is not an implicit catch boundary or a substitute for ignored close. |
| `NativeStatus { code, message }`; callback outcome `Keep`, `Remove`, `Replace(bytes)` | Status conversion and a separate exception channel. Translate enum tags to the library's own C++ NOOP/REMOVE addresses only inside the native adapter. Never manufacture sentinel bytes or integer pointers. |
| `count`, `is_healthy`, `rebuild` if retained by audited Rust callers | Existing optimizations/generic capabilities, not pin requirements; separately account for their call/failure effects. No Append binding is needed for this census. |

This is the proposed minimal handwritten semantic work: constructors and
byte/status adapters, virtual callbacks, real sentinel translation, and
caller-controlled lifecycle. CXX generates the ABI glue. The approved ruling
governs which adapters are allowed; this table grants no additional exception
or dependency approval. Keep libpinyin's byte layouts, signatures, header
validation, key filtering and checked/ignored-result decisions in Rust.

CXX opaque mutation uses pinned references and supports borrowed opaque
types; safe-call declarations still carry an author-supplied safety claim.
Its default opaque types have no Send/Sync implementations.
([CXX extern-C++ reference](https://cxx.rs/extern-c%2B%2B.html).)
`Result<T>` supplies an exception channel, not automatic translation of a
library's status type; an exception from a function not declared returning
Result terminates. Rust callback panic aborts.
([CXX Result reference](https://cxx.rs/binding/result.html).)
UniquePtr represents ordinary `std::unique_ptr` ownership; a custom deletion
policy is not supplied automatically.
([CXX UniquePtr reference](https://cxx.rs/binding/uniqueptr.html).)

## Six manual safety review points for the CXX PR

1. **Safe declarations.** Review every safe-to-call declaration for pointer,
   aliasing, precondition and exception requirements; generated signature
   checks do not prove them. Separate construction, Open and ownership, keep
   `NativeStatus` independent from `Result`, and justify every mutation
   receiver. A null owner, a never-opened Baby and a failed Tree are distinct.
2. **Send/Sync.** Re-prove the marker impls for each concrete owner and adapter
   state, not the old PolyDBM argument. The class headers document concurrent
   operations ([tkrzw_dbm_tree.h:36-40](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tree.h#L36-L40), [tkrzw_dbm_hash.h:35-39](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.h#L35-L39),
   [tkrzw_dbm_baby.h:35-39](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_baby.h#L35-L39), [tkrzw_dbm_tiny.h:34-38](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tiny.h#L34-L38)); cursor sharing is forbidden
   ([tkrzw_dbm_tree.h:60-64](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_tree.h#L60-L64) and peer iterator declarations). Exclude
   open/close/destruction races and unsynchronized Rust callback contexts.
3. **Cursor → DB lifetime.** Encode the borrow in Rust or keep the entire
   cursor walk scoped inside the bridge. Native iterators hold DB back-pointers;
   destruction unregisters them ([tkrzw_dbm_hash.cc:2285-2296](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L2285-L2296)). Close can
   invalidate positions. A DB owner must outlive every iterator and callback;
   a cursor must not be Send/Sync merely because its DB is.
4. **Callback pointers and lengths.** Input key/value views are borrowed for
   native processing only. Never return an escaping Rust reference. Handle
   empty records separately from absent records and the ProcessEach begin/end
   notifications. Native replacement views must remain alive until processor
   destruction or next invocation ([tkrzw_dbm.h:67-87](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm.h#L67-L87)); hold replacement
   bytes in adapter-owned storage. Current C callbacks use signed 32-bit
   lengths and negative/null absence markers ([tkrzw_langc.cc:64-121](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L64-L121));
   the new size_t/slice boundary needs its own bounds, null and alignment proof.
5. **Synchronize, Close, destruction.** Preserve the per-caller sequences above,
   including ignored failures and temp cleanup on early return. Consume explicit
   close errors before destruction; do not let C++ unwind into Rust or assume
   a destructor's implicit exception specification makes it harmless. Retained
   objects after Open/signature failure must still execute native operations.
6. **Sentinels.** Keep/Remove are identity tags, not content. Source defines
   named backing arrays at [tkrzw_dbm.cc:26-37](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm.cc#L26-L37) and compares `data()` addresses
   (e.g. [tkrzw_dbm_hash.cc:1474-1501](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_dbm_hash.cc#L1474-L1501)). Bind the library's real C++ sentinel
   through the adapter, never copied bytes or a guessed pointer. The current
   C API's -1/-2 tags are different and substituted internally
   ([tkrzw_langc.cc:60-89](https://github.com/estraier/tkrzw/blob/9ee84164cf45e441aa8186418c03a02c9b89f08b/tkrzw_langc.cc#L60-L89)). Retain the sound-library prerequisite and existing
   sentinel regression coverage; do not resurrect the pre-1.0.34 LTO fault.

## Result and follow-up boundaries

The main migration obligations are concrete construction/failed-open ownership,
per-caller status handling, explicit sync/close/destruction, C callback and
sentinel replacement, and separate exception/status transport. Further
source-visible differences include shared flexible-container defaults,
successful short-header replacement length, punctuation's empty-result/status
order, extra Count/health checks and soft syncs, and snapshot error handling.
The tables locate them; they are not all newly measured outward divergences.

The implementation PR must use this inventory plus #1516's retained cases and
the then-current family C paths. This PR neither decides those fixes nor
changes their dispositions. It does not change BDB retry ownership, KC's frozen
FFI contract, or row 40. Verification for this documentation-only change is
changed-text/link review and `git diff --check`; no build, test, sanitizer,
measurement, backend-selection run or pin execution was performed.
