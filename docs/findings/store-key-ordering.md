# Store key-ordering contract

Status: **contract** (the store's key-ordering rule) · Backends: tkrzw
(default), Kyoto Cabinet, Berkeley DB.

This contract states the one ordering rule that the store guarantees and
the obligation that it puts on every backend. The key encodings that the
layers above the store choose are not part of it.

`oxpinyin-store` is an ordered byte key–value store. Each store file is one
raw keyspace, as each pin DBM file is one database opened with no
subdatabase name. The interface has two tiers:

- `RawReadStore`: open, point get, ranged scan, full scan.
- `WriteStore`, which extends `RawReadStore`: creation, atomic writes,
  compaction.

## The one rule

**The store orders keys by ascending byte order — `memcmp` on the raw stored
key bytes — and nothing else.** The raw walks (`scan_raw`, `range_raw`) visit
rows in exactly that order, and range bounds are compared the same way.
This is a property of the stored bytes only. The store never decodes a key,
so it has no concept of "integer order". The layer that encoded a key gives
its bytes any meaning they carry.

Every backend satisfies exactly this rule.

- **tkrzw** (`cxx` adapter over TreeDBM, feature `tkrzw`; the default
  backend). The adapter (`crates/oxpinyin-store/src/tkrzw/adapter.cc`) calls
  `dbm.Open(path, writable, options)`. `options` is only
  `File::OPEN_DEFAULT` or `File::OPEN_NO_CREATE`. **The fourth argument,
  `TreeDBM::TuningParameters`, is omitted.** It is therefore
  default-constructed, and its `key_comparator` stays `nullptr`. `TreeDBM`
  resolves `nullptr` to its default **`LexicalKeyComparator`: plain unsigned
  byte order (`memcmp`)**. No `Decimal`, `Hexadecimal`, `RealNumber` or other
  comparator is installed, exactly as libpinyin's `tkrzwdb_utils.h` leaves
  it.
- **Kyoto Cabinet** (feature `kyotocabinet`). `KcStore` opens a `TreeDB`
  with no `rcomp` tuning parameter
  (`crates/oxpinyin-store/src/kyotocabinet/`). Kyoto Cabinet's default
  record comparator therefore applies: **`LEXICALCOMP` — byte-wise, shorter
  key first on a shared prefix**. This is libpinyin's own configuration:
  `phrase_large_table3_kyotodb.cpp` and `chewing_large_table2_kyotodb.cpp`
  install no comparator either.
- **Berkeley DB** (feature `bdb`; libpinyin's original DBM). `BdbStore`
  opens a `DB_BTREE` with **no `set_bt_compare`**
  (`crates/oxpinyin-store/src/bdb/ffi.rs`). libdb's default B-tree
  comparator therefore applies: **byte-wise `memcmp`, shorter key first on a
  shared prefix**. This is the configuration that libpinyin uses:
  `phrase_large_table3_bdb.cpp` and `chewing_large_table2_bdb.cpp` install
  no comparator either. `tools/bdb/btree-order.c` confirms it by
  experiment ([Berkeley DB backend](berkeleydb-backend.md#key-ordering)).
  Setting a comparator here would silently reorder files that libpinyin
  wrote.

**Obligation.** Every backend above is verified against this requirement.
**Any further backend must likewise leave the default lexicographic (memcmp)
comparator in place.** Integer comparators, locale collation, reverse order
and a signed-char compare all break cross-backend parity and are out of
contract.

## Tests that pin the rule

- `crates/oxpinyin-store/src/lib.rs`, the `tests::key_ordering` module:
  each build proves that *its* `DefaultStore` walks and ranges equal the
  mathematically byte-ordered sequence on key sets that cross 256. A binary
  holds exactly one backend (the two `compile_error!` guards in `lib.rs`
  refuse every pairwise combination and the zero-backend build), so no
  process can compare peers. The store-backends CI matrix runs every peer
  build, and so turns those independent results into **Kyoto Cabinet ==
  tkrzw == Berkeley DB**.
- `tkrzw_orders_keys_as_unsigned_bytes` probes `0x80..=0xff`, the high
  half that a *signed*-char comparison would misplace.

## 256-boundary blind spot

The ordering distinctions disappear for keys below 256: a single non-zero
byte sorts the same under byte order and integer order. Byte order and
integer order differ only for a key set that spans below **and** above 256
in the byte position under test. Only such a set can tell a correct
encoding from a broken one. Every ordering test crosses 256 on purpose. A
fixture with only small ids is the blind spot that would let an ordering
defect pass unseen.

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
`reset()` order (`:51-57`), not after every record.

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
header-only record, as the pin's 074a2219 Tkrzw `set_array_header` does.

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
  `native_keys`'s tkrzw arm calls `DBM::ProcessEach`.

**The tkrzw walk is not its iterator.** `DBM::ProcessEach` visits the hash
buckets in an order that the iterator does not reproduce.
`ProcessEach` is the pin's call, and the shared `native_keys` walk uses
it. On a pin-written three-record file the iterator gives
`[1, 16780275, 16786821]`, and the pin's `export_k_mixture_model` (which
calls `get_all_items`) gives `[16780275, 1, 16786821]`.

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

## Deferred: the hash/tree split that the type system does not carry

The one rule is a property of the *container*. Two constructors open a
container that does not have it: `WriteStore::create_hash` and
`RawReadStore::open_hash_read_only` select a Kyoto Cabinet `HashDB`, a
tkrzw `HashDBM` or a Berkeley DB `DB_HASH`. Both still return **`Self`**,
the same concrete type as the ordered tree constructors, so a handle on an
unordered container is statically indistinguishable from an ordered one.
This is a trait shape, not a backend defect.

"An ordered walk is only ever asked of an ordered container" holds today by
call-site convention: every hash handle is used only through unordered
operations, and the raw tier sorts where it must (Berkeley DB's `range_raw`
routes a hash container to `range_raw_unordered`). Nothing enforces it: not
the type, not a debug assertion, not a test.

Candidate fixes, unranked; each needs a store-trait signature change, which
is a STOP until a maintainer opens it:

1. **Separate concrete types**: a hash type per backend beside the tree
   type, with only the tree type offering the ordered walks.
2. **A sealed marker type parameter** (`Store<Tree>` / `Store<Hash>`), with
   the ordered methods bounded to the ordered marker.
3. **Split the unordered operations onto their own trait**, and make the
   hash constructors stop returning `Self`.
