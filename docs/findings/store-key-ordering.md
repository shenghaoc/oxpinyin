# Store key-ordering contract — one place for the whole stack

Date: 2026-08-24 (updated 2026-08-25 for the tkrzw backend) · Status:
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

- **redb** (the pure-Rust portability fallback, `--no-default-features`).
  The store uses `TableDefinition<&[u8], &[u8]>`. redb's
  `Key for &[u8]` is `data1.cmp(data2)` — plain lexicographic byte compare
  (`redb-4.1.0/src/types.rs:347`). (redb's *typed* integer keys are a
  different animal: they store `to_le_bytes()` but compare by the *decoded*
  integer, `from_bytes(a).cmp(from_bytes(b))`, `types.rs:645`. oxpinyin does
  not use typed integer keys in the store; it uses the raw `&[u8]` path, which
  is pure memcmp. That distinction is the whole reason encoding choice matters
  below.)
- **LMDB** (`heed`, feature `lmdb`). The environment is opened with only
  `EnvFlags::NO_SUB_DIR` (plus `READ_ONLY` for read-only opens) —
  `crates/oxpinyin-store/src/lmdb.rs`. No `MDB_INTEGERKEY`, no reverse
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
