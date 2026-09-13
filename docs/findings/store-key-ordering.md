# Store key-ordering contract — one place for the whole stack

Date: 2026-08-24 (updated 2026-08-25 for the tkrzw backend; 2026-09-13
for the Stage 2 hash/tree note) · Status:
**audit finding** (verification + tests only; no key encoding changed) ·
Branch: `audit/store-key-ordering`.

`oxpinyin-store` is an ordered byte key–value store, its interface split into
a [`ReadStore`] tier (point get, ranged scan, full scan, emptiness) and a
[`WriteStore`] tier (creation, atomic writes, compaction). This note states,
in one place, the single ordering rule the store guarantees, the encoding each
layer above it chooses, and why those choices are consistent — so a new
backend or a future encode site cannot drift out of contract without a test
going red.

## The one rule

**The store orders keys by ascending byte order — `memcmp` on the raw stored
key bytes — and nothing else.** `ReadStore::range` and `for_each` visit rows in
exactly that order; `range` bounds are compared the same way. This is a pure
property of the stored bytes: the store never decodes a key, so it has no
notion of "integer order". Any meaning a key's bytes carry is imposed by the
layer that encoded them.

All four backends satisfy exactly this rule:

- **redb** (the pure-Rust peer backend; `--no-default-features --features redb`).
  The store uses `TableDefinition<&[u8], &[u8]>`. redb's
  `Key for &[u8]` is `data1.cmp(data2)` — plain lexicographic byte compare
  (`redb-4.1.0/src/types.rs:347`). (redb's *typed* integer keys are a
  different animal: they store `to_le_bytes()` but compare by the *decoded*
  integer, `from_bytes(a).cmp(from_bytes(b))`, `types.rs:645`. oxpinyin does
  not use typed integer keys in the store; it uses the raw `&[u8]` path, which
  is pure memcmp. That distinction is the whole reason encoding choice matters
  below.)
- **LMDB** (the system liblmdb, feature `lmdb`). The environment is opened
  with only `MDB_NOSUBDIR` and `MDB_NOTLS` (plus `MDB_RDONLY` for read-only
  opens, `MDB_WRITEMAP` for writable ones) —
  `crates/oxpinyin-store/src/lmdb/mod.rs`. No `MDB_INTEGERKEY`, no reverse
  or custom comparator is ever set, on the environment or on any database, so
  LMDB uses its **default byte-lexicographic (`memcmp`) comparator**.
  `MDB_INTEGERKEY` would compare in **native** endian order, which disagrees
  with redb's memcmp on big-endian targets and on any multi-byte key that
  crosses a 256 boundary even on little-endian targets. It must never be set.
- **tkrzw** (`cxx` shim over TreeDBM, feature `tkrzw`). `open_db`
  (`crates/oxpinyin-store/src/tkrzw/shim.cc`) calls
  `db->dbm.Open(path, writable, options)` with `options` being only
  `File::OPEN_DEFAULT` or `File::OPEN_NO_CREATE`. **The fourth argument —
  `TreeDBM::TuningParameters` — is omitted**, so it is default-constructed and
  its `key_comparator` stays `nullptr`, which `TreeDBM` resolves to its default
  **`LexicalKeyComparator`: plain unsigned byte order (`memcmp`)**. No
  `Decimal`, `Hexadecimal`, `RealNumber` or any other comparator is installed —
  exactly as libpinyin's `tkrzwdb_utils.h` leaves it. This is stated in both
  `tkrzw/shim.h` and `tkrzw/mod.rs`. It is checked directly by
  `tkrzw_orders_keys_as_unsigned_bytes` (which probes `0x80..=0xff`, the high
  half a *signed*-char comparison would misplace) and, against redb and LMDB,
  by the cross-backend equivalence tests below.
- **Kyoto Cabinet** (feature `kyotocabinet`; the DEFAULT backend). `KcStore`
  opens a `TreeDB` with no `rcomp` tuning parameter
  (`crates/oxpinyin-store/src/kyotocabinet/`), so Kyoto Cabinet's default
  record comparator applies: **`LEXICALCOMP` — byte-wise, shorter key first
  on a shared prefix** — exactly libpinyin's own configuration
  (`phrase_large_table3_kyotodb.cpp` and `chewing_large_table2_kyotodb.cpp`
  install no comparator either). Verified by the same cross-backend
  conformance suite over keys that cross 256 in the first and in a later
  element.

**Obligation discharged, and carried forward.** When this note was first
written, "a new backend must match the default lexicographic comparator" was a
forward-looking promise with two backends in hand. A third backend, tkrzw, has
since arrived and been verified against exactly that requirement (above). **Any
further backend must likewise leave the default lexicographic (memcmp)
comparator in place** — integer comparators, locale collation, reverse order,
or a signed-char compare all break cross-backend parity and are out of
contract.

## What each layer encodes, and why it is consistent

Because the store is pure memcmp, each layer picks an encoding whose byte
order gives the logical order that layer needs.

| Layer | Site | Integer encoding | memcmp of those bytes gives | Why |
|---|---|---|---|---|
| `oxpinyin-data` | `table.rs` (`LeByteKey`) | `to_le_bytes()` | **byte order — deliberately NOT integer order** | matches the frozen exported tables (`data-layer-export.md`: "all multi-byte integers are little-endian; entries written in ascending key order") |
| `oxpinyin-user` | `codec.rs` (`encode_token`, `encode_u64`, `encode_token_pair`, …) | `to_be_bytes()` | **integer order** | so memcmp reproduces libpinyin's integer `phrase_token_t` order (and redb's typed-integer order); successor/range scans are integer-meaningful |
| `oxpinyin-user` | `phrase.rs` (`encode_keys`) | `to_le_bytes()` u16 tail | byte order of the tail | it is a *payload* inside the composite pronunciation **key**, collected by a token-prefix range; its internal order carries no cross-key meaning |

### data layer: byte order is intentional, and load-without-sort depends on it

The exported system tables key tokens as 4-byte little-endian. Under memcmp
that is **byte order, not integer order**: `0x0000_0100` (bytes `00 01 00 00`)
sorts *before* `0x0000_00FF` (bytes `FF 00 00 00`), the reverse of integer
order. `LeByteKey` (`table.rs`) reproduces exactly this via
`self.0.swap_bytes().cmp(&other.0.swap_bytes())`.

The payoff is the **load-without-sort invariant**: the store walks its rows in
memcmp order, which for these LE keys equals ascending `LeByteKey` order, so
the typed loaders (`dict.rs::load_phrase_index`, `dict.rs::load_pinyin_index`,
`lm/mod.rs`) *append* walk rows into a vector that is already sorted for
binary search — no per-row `BTreeMap::insert`, and lookups stay O(log n).
`ensure_sorted_unique` is a self-healing O(n) guard: it re-sorts (and keeps
the last row per key, mirroring `BTreeMap::insert`) only if a walk ever
arrives out of order, so a drift degrades performance rather than silently
returning wrong results — but the invariant is what keeps the fast path taken.
String-keyed loaders (`pinyin_index`) need no wrapper: UTF-8 byte order is the
`str`/`Box<str>` `Ord`, which already equals the walk order. `punct.rs` and
`interp.rs` are not part of this invariant — they slurp into a `BTreeMap`
(punct) or parse a text file and sort by integer (interp), each internally
consistent.

> **Trap — do not "simplify" a data table to a redb typed integer key.**
> The data tables use the raw `&[u8]` path (`TableDefinition<&[u8], &[u8]>`),
> which compares by `memcmp`. redb's *typed* integer keys look like the
> obvious simplification — `TableDefinition<u32, …>` instead of hand-rolled
> LE bytes — but they **store `to_le_bytes()` yet compare by the *decoded*
> integer value** (`redb-4.1.0/src/types.rs:645`). Switching a data table to
> a typed `u32` key would silently change its ordering from byte order to
> integer order, so the store walk would no longer match ascending
> `LeByteKey` order. The `load-without-sort` fast path would stop being taken
> (every load would pay a full re-sort in `ensure_sorted_unique`), and the
> keys the loaders *write* would move — a data-format change, not a refactor.
> Keep data tables on the raw `&[u8]` key with explicit `to_le_bytes`. This
> is the one API choice a future contributor is most likely to get wrong.

### user layer: big-endian so memcmp == integer order

Every **token and bigram key field** is `to_be_bytes()`: bare tokens
(`encode_token` — the unigram, phrase, and bigram-total keys) and the
`(prev, cur)` pairs of the bigram key (`encode_token_pair`). memcmp on
big-endian bytes equals integer order, so the user store's logical order over
those fields *is* integer order — which is what libpinyin's token space and
the bigram successor scan assume.

The one field this does **not** cover is the pinyin-key tail of the
pronunciation key: `encode_token_bytes(token, phrase::encode_keys(keys))` is a
big-endian token prefix followed by the key sequence packed **little-endian
`u16`** (`phrase.rs`). That tail is a within-token payload — pronunciation
rows are collected by the token-prefix range (see the data-layer section
above), so the tail's byte order carries no cross-key integer meaning and does
not affect the successor or count scans. `decode_keys` reads it back
little-endian, so encode and decode agree.

`bigram_successors(prev)` ranges
`[encode_token_pair(prev, MIN) ..= encode_token_pair(prev, MAX)]`: the
fixed-width big-endian `prev` prefix brackets exactly the successors of
`prev`, and they come back in ascending integer `cur` order. `codec.rs`'s
`order` proptest module already checks this encoding against redb's typed
`(u32,u32)` compare order; the tests added by this audit extend it to the
cross-backend and 256-boundary cases across the backends those suites
compile in (the store suite: all four; the user suite: redb, LMDB and
tkrzw — see the inventory below).

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

No site was found whose encode and decode disagree. **No inconsistency; no
key encoding was changed.**

## Tests that pin this (added by the audit)

- `crates/oxpinyin-store/src/lib.rs` — the `tests::key_ordering` module (folded
  into the store suite alongside the per-tier groups, not a separate file):
  every compiled backend yields byte-identical `for_each` and `range`
  sequences on key sets that cross 256 — under the default features that is
  **redb == Kyoto Cabinet**, and
  `cargo test -p oxpinyin-store --features "kyotocabinet,tkrzw,lmdb"` (the
  store-backends CI gate's conformance pass) is the full four-way
  **redb == Kyoto Cabinet == tkrzw == LMDB** check; plus, redb-only, that
  swapping an encode site's endianness changes the observed walk order
  (non-vacuity).
- `crates/oxpinyin-data/src/table.rs` tests — the load-without-sort invariant:
  the store walk of 256-crossing LE keys is already `LeByteKey`-sorted (fast
  path taken) while the same walk is *not* integer-sorted (a loader assuming
  integer order would break).
- `crates/oxpinyin-user/src/store.rs` tests — the bigram successor scan
  returns the complete, correctly ordered successor set across 256 under the
  `user_store_tests!` macro's arms (**redb, LMDB and tkrzw**; the Kyoto
  Cabinet backend is covered by the store crate's four-way conformance suite
  above, not by this macro), and the raw bigram walk is identical across every
  backend the user crate's cross-backend test compiles in, each backend's own
  walk asserted to be in integer order. Flipping `encode_token_pair`'s
  endianness reddens the check on every one of those backends, not just
  redb.

## 256-boundary blind spot

All of the above ordering distinctions vanish for keys below 256: a single
non-zero byte sorts the same under byte order and integer order. Only a key
set that spans below **and** above 256 (in the byte position under test) makes
byte order and integer order diverge, so only such a set can tell a correct
encoding from a broken one. Every ordering test here crosses 256 deliberately;
a small-id-only fixture is the blind spot that would let an ordering defect
pass unseen.

## Deferred to Stage 2: the hash/tree split the type system does not carry

Recorded 2026-09-13, noticed while reviewing #445 (the Berkeley DB
backend). **No code changes with this note** — it queues a trait
redesign, it does not perform one.

The one rule above is a property of the *container*. Two of the store's
constructors open a container that does not have it:

- `RawReadStore::open_hash_read_only` (`crates/oxpinyin-store/src/lib.rs:503`)
- `WriteStore::create_hash` (`crates/oxpinyin-store/src/lib.rs:316`)

Both select a Kyoto Cabinet `HashDB` / Tkrzw `HashDBM` / Berkeley DB
`DB_HASH`, and both return **`Self`** — the same concrete type
`ReadStore::open_read_only` and `WriteStore::create` return for the
ordered tree container. A handle on an unordered container is therefore
statically indistinguishable from a handle on an ordered one, and the
whole framed ordered API — `ReadStore::range`, `for_each`, `is_empty`,
`get` — is in scope on it. Nothing in the type system says the ordered
walk is unavailable; nothing checks it at the call.

This is a **trait shape, not a backend defect**. `RawReadStore` imposes
it on every peer that has a hash/tree distinction at all: Kyoto Cabinet
behind `KcStore`, Tkrzw behind `TkrzwStore`, Berkeley DB behind
`BdbStore`. redb and LMDB have one container class, so their
`create_hash` / `open_hash_read_only` defaults delegate to
`create` / `open_read_only` and the question does not arise for them.

### What the framed tier would do on a hash handle

The three backends fail differently, which is why the shape is worth
recording rather than patching per backend. The Berkeley DB lines below
are cited against `feat/berkeleydb-backend` at `689b6940` (#445, not yet
landed); every other line is against `main` at `4104a960`.

- **Berkeley DB — a loud runtime error.** `BdbStore::walk`
  (`crates/oxpinyin-store/src/bdb/mod.rs:100`, the body of `range` and
  `for_each`) and `BdbStore::first_key_of` (`:145`, the body of
  `is_empty`) position their cursor with `Seek::AtOrAfter`, which maps to
  `DB_SET_RANGE` (`crates/oxpinyin-store/src/bdb/ffi.rs:499`).
  `DB_SET_RANGE` is documented Btree-only; on a `DB_HASH` handle libdb
  returns `EINVAL`, which — being a positive errno — this backend maps
  to `StoreError::Io` (`bdb/ffi.rs:148-150`), not `StoreError::Backend`.
  Neither function branches on the container class.
  `BdbStore::range_raw` (`:263`) *does* — `Db::is_hash()`
  (`bdb/ffi.rs:268`) routes hash containers to `range_raw_unordered`
  (`:192`), which collects every row, sorts by key, then applies the
  bounds — so the raw tier is correct and only the framed tier is not.
- **Kyoto Cabinet — silently wrong rows.** `KcStore::walk`
  (`crates/oxpinyin-store/src/kyotocabinet/mod.rs:138`) and
  `KcStore::is_empty` (`:198`) position with `kccurjumpkey`. The backend's
  own `range_raw` comment (`:224-230`) already states why that is not an
  ordered positioning on a hash container: "jump positions at a hash
  slot, not at the first key at or above a lower bound". `range_raw`
  collects-and-sorts for exactly this reason; the framed walk does not,
  so it would read bucket-ordered rows from an arbitrary start and — its
  stop condition being the first key outside the framed prefix — most
  likely return a truncated or empty set, with no error at all.
- **Tkrzw — outside contract, outcome unobserved.** The framed `scan`
  positions with `tkrzw_dbm_iter_jump`, and the code's own comment
  (`crates/oxpinyin-store/src/tkrzw/mod.rs:624-630`) records that
  HashDBM's `Jump` does not behave as the ordered positioning the walk
  assumes. The precise result of a framed walk on a hash handle has not
  been observed here and is not claimed.

### Why it is unreachable today

Every hash-constructor call site in the workspace uses only the raw tier
— `get_raw`, `range_raw`, `count_raw` — and never the framed one:

| Call site | Constructor | Uses |
|---|---|---|
| `crates/oxpinyin-data/src/bigram_table.rs:56` | `open_hash_read_only` | wraps in `RawChewingDbm` |
| `crates/oxpinyin-data/tests/language_model.rs:44` | `create_hash` | `put_raw` |
| `crates/oxpinyin-datagen/src/write.rs:279` (`get_hash`) | `open_hash_read_only` | `get_raw` |
| `crates/oxpinyin-datagen/src/write.rs:304` (`count_hash`) | `open_hash_read_only` | `count_raw` |
| `crates/oxpinyin-datagen/src/write.rs:424` (`write_hash_with`) | `create_hash` | `put_raw` |
| `crates/oxpinyin-datagen/src/write.rs:443` (`verify_hash`) | `open_hash_read_only` | `get_raw` |
| `crates/oxpinyin-datagen/tests/libpinyin_parity.rs:146` | `open_hash_read_only` | `get_raw` |
| `crates/oxpinyin-store/benches/support/mod.rs:88` | `open_hash_read_only` | open cost only |
| `crates/oxpinyin-user/src/persistence.rs:299` (`load_bigram`) | `open_user_bigram` | `range_raw` (`:332`) |

`RawChewingDbm` (`crates/oxpinyin-data/src/chewing_table.rs:83`) is the
one wrapper a hash handle is handed to, and it is bounded by
`RawReadStore` alone: its `get` calls `get_raw` (`:124`) and its `walk`
calls `range_raw` (`:142`). `WriteStore::write_user_bigram`'s trait
default (`crates/oxpinyin-store/src/lib.rs:352`) is `create_hash` +
`put_raw` + `compact` + rename. So no production path reaches a framed
walk with a hash handle, and the worst outcome if one were added is an
`EINVAL` on Berkeley DB or wrong rows on Kyoto Cabinet — never unsafety,
and never a silent corruption of what is written.

### The invariant is unenforced, not absent

"An ordered walk is only ever asked of an ordered container" holds today.
It holds by **call-site convention**, checked by nothing: not the type,
not a debug assertion, not a test. A new backend author who adds one
framed call on a hash handle — or a `RawReadStore` bound that widens to
`ReadStore` — gets a runtime error on one backend and silent misordering
on another, with no compile-time signal and no red test until a fixture
happens to cross a hash bucket boundary in the wrong direction. That is
the failure mode this note exists to pre-empt.

### Candidate fixes for Stage 2

Not ranked; each has a different blast radius and the redesign picks one.

1. **Separate concrete types.** A distinct hash type per backend
   (`KcHashStore` beside `KcStore`, and so on), with only the tree type
   implementing `ReadStore`. Strongest guarantee; changes every
   `RawReadStore` bound in `oxpinyin-data`, `oxpinyin-datagen` and
   `oxpinyin-user`, and doubles the per-backend type surface.
2. **A sealed marker type parameter.** One type carrying the container
   class in its type — `Store<Tree>` / `Store<Hash>` — with the ordered
   methods in an `impl` block bounded to the ordered marker. Keeps one
   type per backend; the marker must be sealed or a downstream crate can
   assert order the container does not have.
3. **Move the unordered operations onto their own trait.** Split
   `RawReadStore`'s unordered half (`get_raw`, `count_raw`, the hash
   constructors) from its ordered half (`range_raw`), so a hash handle is
   bounded by a trait on which the ordered API is simply not in scope.
   Smallest surface change of the three and the only one that needs no
   new types, but it fixes the raw tier only: the hash constructors must
   also stop returning `Self` — or return a newtype — or `ReadStore`
   stays in scope on the value they hand back.

### Where this sits

It joins the store-trait items ruled out of Stage 1 on 2026-09-05 and
deferred to the Stage 2 trait redesign — #340's "Deferred by ruling": F2
(point gets return `Vec<u8>` for 4- and 8-byte values because the trait
signature forces it) and F5 (`mask_out` / `remove_user_phrase`
materialise whole tables because `&dyn WriteTxn` cannot remove while
iterating). Those two were recorded in that PR's body and never written
into a document; this section is the repo-side home for the list, and
the three of them share one precondition — a `ReadStore` / `WriteStore` /
`RawReadStore` / `WriteTxn` signature change, which is a STOP until a
maintainer opens Stage 2.

Not done here, and cheap when #445 lands: a doc comment on
`BdbStore::walk` and `BdbStore::first_key_of` saying they assume a
`DB_BTREE` handle and pointing at this section for why that holds.
