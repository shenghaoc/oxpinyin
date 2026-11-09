# Store key-ordering contract — one place for the whole stack

Status: **contract** (the store's key-ordering rule) · Backends: tkrzw
(default), Kyoto Cabinet, Berkeley DB.

This note states, in one place, the single ordering rule that the store
guarantees, the encoding that each layer above it chooses, and why those
choices are consistent. With it, a new backend or a future encode site
cannot leave the contract without a test failing.

`oxpinyin-store` is an ordered byte key–value store. Each store file is one
raw keyspace, as each pin DBM file is one database opened with no
subdatabase name. The interface has two tiers:

- `RawReadStore`: open, point get, ranged scan, full scan.
- `WriteStore`, which extends `RawReadStore`: creation, atomic writes,
  compaction.

The framed `ReadStore` tier and its `table || 0x00 || key` framing no
longer exist. Where a section below names `ReadStore`, its
`range`/`for_each`/`is_empty` or the framing, it describes that removed
tier.

## The one rule

**The store orders keys by ascending byte order — `memcmp` on the raw stored
key bytes — and nothing else.** The raw walks (`scan_raw`, `range_raw`) visit
rows in exactly that order. `range` bounds are compared the same way. This
is a property of the stored bytes only. The store never decodes a key, so
it has no concept of "integer order". The layer that encoded a key gives its
bytes any meaning they carry.

Every backend satisfies exactly this rule.

- **tkrzw** (`cxx` adapter over TreeDBM, feature `tkrzw`; the default
  backend). The adapter (`crates/oxpinyin-store/src/tkrzw/adapter.cc`) calls
  `db->dbm.Open(path, writable, options)`. `options` is only
  `File::OPEN_DEFAULT` or `File::OPEN_NO_CREATE`. **The fourth argument,
  `TreeDBM::TuningParameters`, is omitted.** It is therefore
  default-constructed, and its `key_comparator` stays `nullptr`. `TreeDBM`
  resolves `nullptr` to its default **`LexicalKeyComparator`: plain unsigned
  byte order (`memcmp`)**. No `Decimal`, `Hexadecimal`, `RealNumber` or other
  comparator is installed, exactly as libpinyin's `tkrzwdb_utils.h` leaves
  it. `tkrzw/mod.rs` states this. Two kinds of test check it directly:
  - `tkrzw_orders_keys_as_unsigned_bytes`, which probes `0x80..=0xff`, the
    high half that a *signed*-char comparison would misplace;
  - the per-peer byte-order tests below.
- **Kyoto Cabinet** (feature `kyotocabinet`). `KcStore` opens a `TreeDB`
  with no `rcomp` tuning parameter
  (`crates/oxpinyin-store/src/kyotocabinet/`). Kyoto Cabinet's default
  record comparator therefore applies: **`LEXICALCOMP` — byte-wise, shorter
  key first on a shared prefix**. This is exactly libpinyin's own
  configuration: `phrase_large_table3_kyotodb.cpp` and
  `chewing_large_table2_kyotodb.cpp` install no comparator either. The same
  per-peer byte-order suite verifies it, over keys that cross 256 in the
  first element and in a later element.
- **Berkeley DB** (feature `bdb`; libpinyin's original DBM). `BdbStore`
  opens a `DB_BTREE` with **no `set_bt_compare`**
  (`crates/oxpinyin-store/src/bdb/ffi.rs`). libdb's default B-tree
  comparator therefore applies: **byte-wise `memcmp`, shorter key first on a
  shared prefix**. This is exactly the configuration that libpinyin uses:
  `phrase_large_table3_bdb.cpp` and `chewing_large_table2_bdb.cpp` install
  no comparator either. An experiment confirms it; the documentation alone
  is not the source. `tools/bdb/btree-order.c` opens a `DB_BTREE` as
  libpinyin does (no environment, no transaction, no comparator). It prints
  the walk over little-endian `u32` array keys that cross 256 in the first
  element and in a later element. The walk is in raw-byte order, not
  integer order (`docs/findings/berkeleydb-backend.md`). Setting a
  comparator here would silently reorder files that libpinyin wrote.

**Obligation.** Every backend above is verified against this requirement.
**Any further backend must likewise leave the default lexicographic (memcmp)
comparator in place.** Integer comparators, locale collation, reverse order
and a signed-char compare all break cross-backend parity and are out of
contract.

## What each layer encodes, and why it is consistent

Because the store is pure memcmp, each layer picks an encoding whose byte
order gives the logical order that layer needs.

| Layer | Site | Integer encoding | memcmp of those bytes gives | Why |
|---|---|---|---|---|
| `oxpinyin-data` | `table.rs` (`LeByteKey`) | `to_le_bytes()` | **byte order — deliberately NOT integer order** | matches the frozen exported tables (`data-layer-export`: "all multi-byte integers are little-endian; entries written in ascending key order") |
| `oxpinyin-user` | `codec.rs` (`encode_token`, `encode_u64`, `encode_token_pair`, …) | `to_be_bytes()` | **integer order** | so memcmp reproduces libpinyin's integer `phrase_token_t` order (and redb's typed-integer order); successor/range scans are integer-meaningful |
| `oxpinyin-user` | `phrase.rs` (`encode_keys`) | `to_le_bytes()` u16 tail | byte order of the tail | it is a *payload* inside the composite pronunciation **key**, collected by a token-prefix range; its internal order carries no cross-key meaning |

### data layer: byte order is intentional, and load-without-sort depends on it

The exported system tables key tokens as 4-byte little-endian. Under memcmp
that is **byte order, not integer order**. `0x0000_0100` (bytes
`00 01 00 00`) sorts *before* `0x0000_00FF` (bytes `FF 00 00 00`), the
reverse of integer order. `LeByteKey` (`table.rs`) reproduces exactly this
with `self.0.swap_bytes().cmp(&other.0.swap_bytes())`.

This gives the **load-without-sort invariant**. The store walks its rows in
memcmp order, and for these LE keys that equals ascending `LeByteKey`
order. So the typed loaders (`dict.rs::load_phrase_index`,
`dict.rs::load_pinyin_index`, `lm/mod.rs`) *append* walk rows into a vector
that is already sorted for binary search. There is no per-row
`BTreeMap::insert`, and lookups stay O(log n).

`ensure_sorted_unique` is a self-healing O(n) guard. It re-sorts only if a
walk arrives out of order, and it keeps the last row per key, as
`BTreeMap::insert` does. A drift therefore costs performance and does not
silently return wrong results. The invariant is what keeps the fast path in
use.

String-keyed loaders (`pinyin_index`) need no wrapper: UTF-8 byte order is
the `str`/`Box<str>` `Ord`, which already equals the walk order. `punct.rs`
and the interpolation readers in `oxpinyin-utils` are not part of this
invariant. `punct.rs` reads into a `BTreeMap`. The interpolation readers
parse a text file and sort by integer. Each is internally consistent.

> **Trap — do not "simplify" a data table to a redb typed integer key.**
> The data tables use the raw `&[u8]` path (`TableDefinition<&[u8], &[u8]>`),
> which compares by `memcmp`. redb's *typed* integer keys
> (`TableDefinition<u32, …>`) look like a simpler replacement for the
> hand-written LE bytes. But they **store `to_le_bytes()` and compare by the
> *decoded* integer value** (`redb-4.1.0/src/types.rs:645`). A typed `u32`
> key would silently change a data table's ordering from byte order to
> integer order. The store walk would then no longer match ascending
> `LeByteKey` order. The `load-without-sort` fast path would stop being used,
> and every load would pay a full re-sort in `ensure_sorted_unique`. The
> keys that the loaders *write* would also move: a data-format change, not
> a refactor.
> Keep data tables on the raw `&[u8]` key with explicit `to_le_bytes`. This
> is the API choice that a future contributor is most likely to get wrong.

### user layer: big-endian so memcmp == integer order

Every **token and bigram key field** is `to_be_bytes()`. This covers bare
tokens (`encode_token`: the unigram, phrase and bigram-total keys) and the
`(prev, cur)` pairs of the bigram key (`encode_token_pair`). memcmp on
big-endian bytes equals integer order. So the user store's logical order
over those fields *is* integer order, which libpinyin's token space and the
bigram successor scan assume.

The one field that this does **not** cover is the pinyin-key tail of the
pronunciation key. `encode_token_bytes(token, phrase::encode_keys(keys))`
is a big-endian token prefix followed by the key sequence packed
**little-endian `u16`** (`phrase.rs`). That tail is a within-token payload.
Pronunciation rows are collected by the token-prefix range (see the
data-layer section above). So the tail's byte order carries no cross-key
integer meaning and does not affect the successor or count scans.
`decode_keys` reads it back little-endian, so encode and decode agree.

`bigram_successors(prev)` ranges
`[encode_token_pair(prev, MIN) ..= encode_token_pair(prev, MAX)]`. The
fixed-width big-endian `prev` prefix brackets exactly the successors of
`prev`, and they come back in ascending integer `cur` order. `codec.rs`'s
`order` proptest module checks this encoding against redb's typed
`(u32,u32)` compare order. The tests in the inventory below extend it to
the per-peer and 256-boundary cases in the backend that the build compiles
in. Each binary of the store and user suites has one peer, and each CI
matrix child builds one peer.

## Layer consistency (encode ↔ decode)

Every encode site pairs with a decode site under the same convention:

- data LE: `to_le_bytes` written (frozen tables / `load_profile` example) ↔
  `u32::from_le_bytes` read in `dict.rs`, `lm/mod.rs`, `punct.rs`, and
  `LeByteKey` lookups. Consistent.
- user BE: `codec::encode_token`/`encode_u64`/`encode_token_pair`/… ↔
  `codec::decode_token`/`decode_u64`/`decode_token_pair`/…. All
  `to_be_bytes`/`from_be_bytes`. Consistent (round-trip + order proptests in
  `codec.rs`).
- phrase LE-u16: `phrase::encode_keys` (`to_le_bytes`) ↔ `phrase::decode_keys`
  (`from_le_bytes`). Consistent.

No site has an encode and a decode that disagree. **There is no
inconsistency, and no key encoding was changed.**

## Tests that pin this (added by the audit)

- `crates/oxpinyin-store/src/lib.rs`: the `tests::key_ordering` module. It
  is part of the store suite, beside the per-tier groups, not a separate
  file.
  - The exactly-one-backend invariant is a `compile_error!` guard. A single
    binary therefore holds exactly one peer, and the suite **cannot** prove
    cross-peer equivalence in process.
  - Each build proves that *its* `DefaultStore` yields `for_each` and
    `range` sequences equal to the mathematical byte-ordered sequence on
    key sets that cross 256. The store-backends CI matrix runs every peer
    build, and so turns those independent results into **Kyoto Cabinet ==
    tkrzw == Berkeley DB**.
  - Every build also proves that swapping an encode site's endianness
    changes the observed walk order (non-vacuity).
  - The invariant is the two `compile_error!` guards in
    `crates/oxpinyin-store/src/lib.rs`. They refuse every pairwise
    combination of backends and the zero-backend build.
- `crates/oxpinyin-data/src/table.rs` tests: the load-without-sort
  invariant. The store walk of 256-crossing LE keys is already
  `LeByteKey`-sorted, so the fast path is used. The same walk is *not*
  integer-sorted, so a loader that assumes integer order would break.
- `crates/oxpinyin-user/src/store.rs` tests: the bigram successor scan
  returns the complete, correctly ordered successor set across 256.
  - The `user_store_tests!` macro carries an arm for **every peer**
    (`tkrzw`, `kc`, `bdb`). Each arm is `#[cfg]`-gated on its feature, so
    exactly one suite compiles per build. Kyoto Cabinet has its own arm,
    not a separate conformance suite.
  - `bigram_walks_and_successors_follow_be_integer_order` asserts that the
    current peer's raw bigram walk and successor scans come back in
    ascending `(prev, cur)` integer order. It also asserts that the set
    walked equals the set written.
  - A change to `encode_token_pair`'s endianness fails that check on the
    peer that the build compiles in, and so on every peer across CI.

## 256-boundary blind spot

All of the above ordering distinctions disappear for keys below 256: a
single non-zero byte sorts the same under byte order and integer order.
Byte order and integer order differ only for a key set that spans below
**and** above 256 in the byte position under test. Only such a set can tell
a correct encoding from a broken one. Every ordering test here crosses 256
on purpose. A fixture with only small ids is the blind spot that would let
an ordering defect pass unseen.

## FlexibleBigram: the pin's hash training container

`crates/oxpinyin-store/src/flexible_ngram.rs` carries libpinyin's
`FlexibleBigram` training container (`src/storage/flexible_ngram*.h`,
`src/storage/flexible_single_gram.h` at 074a2219) on all three backends. It
is **not** an ordered table, and the one rule above deliberately does not
apply to it. The pin opens a hash container in every backend and walks it
with the library's own cursor for `get_all_items`. The layer therefore uses
the hash constructors and reproduces that native walk; it does not sort.

The layer reads a magic or array header with the pin's own access:

- Berkeley DB uses `DB_DBT_PARTIAL` (`flexible_ngram_bdb.h:309-326`,
  `:379-389`). An over-long magic record still answers its header, and a
  short record is the pin's assert.
- Kyoto Cabinet and tkrzw read the whole value, as their implementations do.

Writes are deferred. The file is synced once on close, in the pin's
`reset()` order (`:51-57`), not after every record. (2026-10-31)

### Container class and open

| Pin backend | Pin open | Store constructor |
|---|---|---|
| Berkeley DB | `DB_HASH`, `0644`, `ATTACH_CREATE` adds `DB_CREATE` (`flexible_ngram_bdb.h:94-153`) | `WriteStore::create_hash` / `RawReadStore::open_hash_read_only` |
| Kyoto Cabinet | `HashDB`, `OREADER\|OWRITER` then `\|OCREATE` (`flexible_ngram_kyotodb.h:129-180`) | same |
| tkrzw | `HashDBM`, `Open(path, writable, OPEN_NO_CREATE)` then the create form (`flexible_ngram_tkrzwdb.h:128-188`) | same |

These are the same constructors that write libpinyin's `bigram.db`, so the
container class matches the pin's `attach`.

### Records

All integers are host-native `guint32`/`phrase_token_t` fields, `memcpy`d
as structs. There is no length prefix, padding or normalised byte order.
On the linux/amd64 reference that is little-endian and the KMM types below
are 8, 16, 12 and 20 bytes.

| Record | Key bytes | Value bytes | Pin site |
|---|---|---|---|
| Magic | 8 zero bytes — `phrase_token_t m_magic_header_index[2]`, both `null_token` (`:47,69-70`) | `char m_magic_number[4]` (KMM: `KMMP`), then `MagicHeader` (KMM: 12 bytes); a fresh container holds the magic number alone (4 bytes) | `flexible_ngram_bdb.h:118-131`; `flexible_ngram_kyotodb.h:157-161`; `flexible_ngram_tkrzwdb.h:161-167` |
| Single gram | one 4-byte `phrase_token_t` | `ArrayHeader` (KMM: 8 bytes), then one `ArrayItemWithToken` per item, ascending by numeric token: 4-byte token + `ArrayItem` (KMM: 16 bytes) | `flexible_single_gram.h:57-60,75-78,151-181`; value write `flexible_ngram_*db.h` `store` |

The magic record is read and written with `MagicHeader` at offset 4 and
the array header at offset 0.

- Berkeley DB does this with `DB_DBT_PARTIAL`: `doff=4,
  dlen=sizeof(MagicHeader)` for the magic header and `doff=0,
  dlen=sizeof(ArrayHeader)` for the array header
  (`flexible_ngram_bdb.h:309-326,342-355,379-389,406-418`). The create put
  is `doff=0, dlen=4` (`:118-131`).
- KC and tkrzw load the whole value, overwrite the wanted prefix and store
  it back (`flexible_ngram_kyotodb.h:385-408`,
  `flexible_ngram_tkrzwdb.h:402-430`).

The layer writes the same final bytes on every backend. An array-header
write therefore leaves the items intact, and a missing token becomes a
header-only record, as the pin's 074a2219 Tkrzw `set_array_header` does
(see `kmm-arithmetic-audit`).

### Order

The container's keys have no order contract. The pin's `get_all_items`
appends every four-byte key in the backend's own cursor order and skips
the eight-byte magic key. That order is not a `memcmp` sequence, and the
layer does not sort it:

- Berkeley DB: `DB_NEXT` cursor (`flexible_ngram_bdb.h:245-291`);
  `flexible_ngram.rs` reads it through `RawReadStore::native_keys`, whose
  Berkeley DB arm is a `DB_FIRST`/`DB_NEXT` cursor.
- Kyoto Cabinet: `iterate`, skipped by key size in the visitor
  (`flexible_ngram_kyotodb.h:38-62,262-272`); `native_keys`'s Kyoto
  Cabinet arm is the same `kcdbiterate` key walk.
- tkrzw: `ProcessEach` (`flexible_ngram_tkrzwdb.h:38-60,274-282`);
  `native_keys`'s tkrzw arm calls `tkrzw_dbm_process_each`.

**The tkrzw walk is not its iterator.** `DBM::ProcessEach` visits the hash
buckets in an order that the iterator does not reproduce.
`tkrzw_dbm_process_each` is the pin's call, and the shared `native_keys`
walk uses it. Recorded evidence: on a pin-written three-record file,
`tkrzw_dbm_iter_first`/`_iter_next` gave `[1, 16780275, 16786821]`. The
pin's `export_k_mixture_model` (which calls `get_all_items`) gave
`[16780275, 1, 16786821]`.

Within one single gram the items ascend by **numeric token**, the pin's
`std_lite::lower_bound` with `token_less_than`
(`flexible_single_gram.h:62-66,151-181`), not by key bytes. The layer's
`SingleGram` keeps the same order, and its item operations are the pin's
`lower_bound` loop.

### Tests that pin this

`crates/oxpinyin-store/tests/flexible_bigram.rs` round-trips every
operation on the backend that the build selects. Its last case,
`a_pin_written_file_is_read_in_the_pins_native_order`, reads the committed
`fixtures/w3/{db,kct,tkt}/k_mixture_model.db`. It compares every header, item
and `get_all_items` token with the pin's `export_k_mixture_model` output of
that file, including the token order. No case is ignored, and no case needs a
pin tool directory. The pin file's magic
key sorts before the four-byte token keys under `memcmp`, but nothing uses
that fact. The magic key is recognised by its eight-byte length, as in the
pin's visitors.

## Deferred to Stage 2: the hash/tree split the type system does not carry

This section is recorded evidence from 2026-09-13, from the review of #445 (the Berkeley DB backend,
merged as `33c2e238`). Its citations and line numbers describe `main` at `33c2e238`, not the current tree.

The current tree differs in four ways:

- The framed `ReadStore` tier is removed, so no framed ordered walk can reach a hash handle.
- `BdbStore::first_key_of` and the doc comment on the two Berkeley DB functions are removed.
- `WriteStore::create_hash` and `RawReadStore::open_hash_read_only` still return `Self`, so the types do not carry the hash/tree split.
- `RawReadStore::get_raw` fills a caller's buffer, so item F2 no longer applies.

Stage 2 is the current stage ([roadmap](../../ROADMAP.md)).

The one rule above is a property of the *container*. Two of the store's
constructors open a container that does not have it:

- `RawReadStore::open_hash_read_only` (`crates/oxpinyin-store/src/lib.rs:510`)
- `WriteStore::create_hash` (`crates/oxpinyin-store/src/lib.rs:323`)

Both select a Kyoto Cabinet `HashDB` / Tkrzw `HashDBM` / Berkeley DB
`DB_HASH`. Both return **`Self`**: the same concrete type that
`ReadStore::open_read_only` and `WriteStore::create` return for the
ordered tree container. A handle on an unordered container is therefore
statically indistinguishable from a handle on an ordered one. The whole
framed ordered API (`ReadStore::range`, `for_each`, `is_empty`, `get`) is
in scope on it. Nothing in the type system says that the ordered walk is
unavailable, and nothing checks it at the call.

This is a **trait shape, not a backend defect**. `RawReadStore` imposes
it on every peer that has a hash/tree distinction: Kyoto Cabinet behind
`KcStore`, Tkrzw behind `TkrzwStore`, Berkeley DB behind `BdbStore`.

### What the framed tier would do on a hash handle

The three backends fail differently. That is why this note records the
shape and does not patch each backend. Every line below is cited against
`main` at `33c2e238`.

- **Berkeley DB — a loud runtime error.** `BdbStore::walk`
  (`crates/oxpinyin-store/src/bdb/mod.rs:110`, the body of `range` and
  `for_each`) and `BdbStore::first_key_of` (`:158`, the body of
  `is_empty`) position their cursor with `Seek::AtOrAfter`, which maps to
  `DB_SET_RANGE` (`crates/oxpinyin-store/src/bdb/ffi.rs:499`).
  `DB_SET_RANGE` is documented as Btree-only. On a `DB_HASH` handle libdb
  returns `EINVAL`. `EINVAL` is a positive errno, so this backend maps it
  to `StoreError::Io` (`bdb/ffi.rs:148-150`), not `StoreError::Backend`.
  Neither function branches on the container class.
  `BdbStore::range_raw` (`:276`) *does* branch: `Db::is_hash()`
  (`bdb/ffi.rs:268`) routes hash containers to `range_raw_unordered`
  (`:205`). That function collects every row, sorts by key, then applies
  the bounds. So the raw tier is correct and only the framed tier is not.
- **Kyoto Cabinet — silently wrong rows.** `KcStore::walk`
  (`crates/oxpinyin-store/src/kyotocabinet/mod.rs:138`) and
  `KcStore::is_empty` (`:198`) position with `kccurjumpkey`. The backend's
  own `range_raw` comment (`:224-230`) states why that is not an ordered
  positioning on a hash container: "jump positions at a hash slot, not at
  the first key at or above a lower bound". `range_raw` collects and sorts
  for this reason. The framed walk does not. It would read bucket-ordered
  rows from an arbitrary start. Its stop condition is the first key outside
  the framed prefix. So it would most likely return a truncated or empty
  set, with no error.
- **Tkrzw — outside contract, outcome unobserved.** The framed `scan`
  positions with `tkrzw_dbm_iter_jump`, and the code's own comment
  (`crates/oxpinyin-store/src/tkrzw/mod.rs:624-630`) records that
  HashDBM's `Jump` does not behave as the ordered positioning that the
  walk assumes. The precise result of a framed walk on a hash handle is
  not observed here and is not claimed.

### Why it is unreachable today

Every hash-constructor call site in the workspace uses only the raw tier
(`get_raw`, `range_raw`, `count_raw`), never the framed one:

| Call site | Constructor | Uses |
|---|---|---|
| `crates/oxpinyin-data/src/bigram_table.rs:56` | `open_hash_read_only` | wraps in `RawChewingDbm` |
| `crates/oxpinyin-data/tests/language_model.rs:44` | `create_hash` | `put_raw` |
| `crates/oxpinyin-store/tests/bdb_libpinyin_files.rs:64` | `open_hash_read_only` | `range_raw` |
| `crates/oxpinyin-store/tests/bdb_libpinyin_files.rs:94` | `open_hash_read_only` | `count_raw` |
| `crates/oxpinyin-store/tests/bdb_libpinyin_files.rs:118` | `open_hash_read_only` | `get_raw` |
| `crates/oxpinyin-store/tests/bdb_libpinyin_files.rs:210` | `open_user_bigram` | `range_raw`, `count_raw` |
| `crates/oxpinyin-user/src/persistence.rs:299` (`load_bigram`) | `open_user_bigram` | `range_raw` (`:332`) |
| `crates/oxpinyin-user/src/persistence.rs:1136` (test) | `open_user_bigram` | `get_raw`, `range_raw` |
| `crates/oxpinyin-user/src/persistence.rs:1346` (test) | `open_user_bigram` | `range_raw` |

`RawChewingDbm` (`crates/oxpinyin-data/src/chewing_table.rs:83`) is the
one wrapper that receives a hash handle. Only `RawReadStore` bounds it: its
`get` calls `get_raw` (`:124`) and its `walk` calls `range_raw` (`:142`).
`WriteStore::write_user_bigram`'s trait default
(`crates/oxpinyin-store/src/lib.rs:359`) is `create_hash` + `put_raw` +
`compact` + rename. So no production path reaches a framed walk with a hash
handle. If someone added one, the worst outcome is an `EINVAL` on Berkeley
DB or wrong rows on Kyoto Cabinet. It is never unsafety, and never a silent
corruption of what is written.

### The invariant is unenforced, not absent

"An ordered walk is only ever asked of an ordered container" holds today.
It holds by **call-site convention**, and nothing checks it: not the type,
not a debug assertion, not a test. Suppose a new backend author adds one
framed call on a hash handle, or widens a `RawReadStore` bound to
`ReadStore`. The result is a runtime error on one backend and silent
misordering on another. There is no compile-time signal, and no test fails
until a fixture crosses a hash bucket boundary in the wrong direction. This
note exists to prevent that failure mode.

### Candidate fixes for Stage 2

The fixes are not ranked. Each changes a different amount of code, and
the redesign picks one.

1. **Separate concrete types.** A distinct hash type per backend
   (`KcHashStore` beside `KcStore`, and so on). Only the tree type
   implements `ReadStore`. This gives the strongest guarantee. It changes
   every `RawReadStore` bound in `oxpinyin-data`, `oxpinyin-utils` and
   `oxpinyin-user`, and doubles the per-backend type surface.
2. **A sealed marker type parameter.** One type carries the container
   class in its type (`Store<Tree>` / `Store<Hash>`). The ordered methods
   are in an `impl` block bounded to the ordered marker. This keeps one
   type per backend. The marker must be sealed, or a downstream crate can
   assert an order that the container does not have.
3. **Move the unordered operations onto their own trait.** Split
   `RawReadStore`'s unordered half (`get_raw`, `count_raw`, the hash
   constructors) from its ordered half (`range_raw`). A hash handle is then
   bounded by a trait on which the ordered API is not in scope. This is the
   smallest surface change of the three, and the only one that needs no new
   types. But it fixes the raw tier only. The hash constructors must also
   stop returning `Self`, or return a newtype. Otherwise `ReadStore` stays
   in scope on the value that they return.

### Where this sits

This item joins the store-trait items that the 2026-09-05 ruling kept
out of Stage 1 and deferred to the Stage 2 trait redesign (#340's
"Deferred by ruling"):

- F2: point gets return `Vec<u8>` for 4- and 8-byte values, because the
  trait signature forces it.
- F5: `mask_out` / `remove_user_phrase` materialise whole tables, because
  `&dyn WriteTxn` cannot remove while iterating.

F2 and F5 are recorded in that PR's body only. This section is their home
in the repository. All three items share one precondition: a `ReadStore` /
`WriteStore` / `RawReadStore` / `WriteTxn` signature change. That change is
a STOP until a maintainer opens Stage 2.

`BdbStore::walk` and `BdbStore::first_key_of` carry a doc comment. It says
that they assume a `DB_BTREE` handle, why that holds, and that nothing
enforces it. That is the place where a reader is most likely to reach for
the ordered cursor on a hash container. That comment and this section are
the whole mitigation until the redesign lands.
