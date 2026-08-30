# BerkeleyDB compat — the open items Phase 2 inherits

> **Status: SHELVED.** The BerkeleyDB compat path was deprioritised in
> favour of the Kyoto Cabinet and tkrzw compat paths (PRs #222, #220, #228).
> This document preserves the Phase 1 research for reference. The
> feat/bdb-backend branch on origin holds the incomplete implementation.

Date: 2026-08-28 · Status: **structured hand-off; one item awaiting a
maintainer decision** · Branch: `claude/pr4-berkeleydb-compat`.

Phase 1's survey (`berkeleydb-compat-phase1.md`) is prose. This is the
same material as a checklist Phase 2 can work from, plus the one
question that must be answered before Phase 2 starts.

## 1. System data lives under `$(libdir)`, and it is BDB too

- **Path:** `$(libdir)/libpinyin/data` — on Debian/Ubuntu
  `/usr/lib/x86_64-linux-gnu/libpinyin/data`. **Not** under `$datadir`.
  Anything that guesses `share` finds nothing.
- **`bigram.db` is a Berkeley DB file**: 25.9 MB, Hash, version 9,
  native byte-order, pagesize 4096 — identified from its header
  (`magic=0x00061561`), matching `ngram_bdb.cpp`'s `DB_HASH` open.

**Consequence for the CI gate.** Phase 1 framed BDB as a user-data
concern; it is not. Every session opens `bigram.db` on the *system*
path, so the shim's ASan/UBSan gate is exercised by every user on
startup, not only by users who have trained. The brief already requires
sanitizing the default backend from day one; this is why that is
load-bearing rather than precautionary.

- `punct.bin` is in the pin's `data/Makefile.am` but **absent** from the
  installed 2.8.1 package (it arrives later). A format detector must not
  require it.

## 2. B-tree key order is `memcmp` over the keys' raw bytes

Neither `phrase_large_table3_bdb.cpp` nor `chewing_large_table2_bdb.cpp`
calls `set_bt_compare`, so both use BDB's default B-tree comparator:
byte-wise `memcmp`, then length, over the key's raw in-memory bytes. The
two tables key on different structs, so the byte layout differs:

- **Phrase table — `ucs4_t[]`** (`phrase_length * sizeof(ucs4_t)`).
  `ucs4_t` is a `guint32`, so on the little-endian targets `memcmp`
  orders it lexicographically over low-byte-first `u32` bytes.
- **Chewing table — `ChewingKey[]`** (`phrase_length *
  sizeof(ChewingKey)`). `ChewingKey` is **not** a `u32`: it is
  libpinyin's 16-bit packed bit-field struct (`chewing_key.h`). Its size
  and byte layout are fixed by the compiler's bit-field packing and the
  storage unit's endianness — ABI-dependent — so `memcmp` runs over those
  raw ABI bytes.

Stated plainly because it is easy to get wrong:

> This is **neither integer order nor big-endian byte order.** For the
> phrase table it is the raw LE bytes of a `u32` array; for the chewing
> table it is the raw ABI bytes of a 16-bit bit-field struct. Neither
> matches integer order — a `ucs4_t` crossing a 256 boundary reorders
> because its low byte compares first.

Both orders are read from the pin's source, **not confirmed against a
real file**, so — like the other ↯ items — they stay an open Phase 2
confirmation task (the phase-1 survey's STOP list); nothing may depend on
them until a real BDB confirms them.

**Action for Phase 2.** The order is read from the pin's source, not
observed, and the `ChewingKey` layout in particular is ABI-specific — so
Phase 2 must *observe it on a real file first*, then codify it. No probe
exists yet; Phase 2 builds it.

1. **Probe (input, run, record).** *Input:* the **user-data** `DB_BTREE`
   tables from the phase-1 survey's §(c) — `phrase_large_table3_bdb` /
   `chewing_large_table2_bdb`, the user's `*.db` written by a
   **BerkeleyDB-built** libpinyin (the Debian/Ubuntu-stable backend). Not
   the system `phrase_index.bin` / `pinyin_index.bin`: those are
   `MemoryChunk` images (§(b)), not BDB B-tree tables, and carry no B-tree
   comparator order — and the only system BDB file, `bigram.db`, is a
   `DB_HASH`, unordered. Train a profile so the table holds keys that
   cross a 256 boundary in both the first and a later element — for the
   phrase key a `ucs4_t` above `0xFF` (any CJK code point already exceeds
   it), for the chewing key a `ChewingKey` whose 16-bit storage unit
   exceeds `0xFF`. *Run:*
   read the keys in storage order — a `DB_BTREE` cursor walk (`->cursor`
   then `->c_get(DB_NEXT)`, the §4 shim surface) or Berkeley DB's
   `db_dump`. *Record:* the raw key bytes and the order they came back in,
   into the fixture the tests below read.
2. **Codify.** Extend the key-ordering contract tests
   (`docs/findings/store-key-ordering.md`) to assert *that observed
   order*, with separate vectors per key — the LE `ucs4_t` bytes for the
   phrase key and the ABI-produced `ChewingKey` bytes for the chewing key
   — asserting the `u32` phrase order and the ABI bit-field chewing order,
   not the store's generic ascending-byte rule, which the other backends
   satisfy by construction and which a small-token suite cannot
   distinguish from these orders.

## 3. The codec simplification is the bigram's half only

- **Confirmed removable:** `UserStore::bigram_successors(prev)`
  (`store.rs:872-891`) range-scans `encode_token_pair(prev, MIN)` to
  `encode_token_pair(prev, MAX)`. That scan is the bigram key's only
  reason for integer-ordered bytes, and the blob-per-prev model turns it
  into a point `get(prev)` plus an item-array decode.
- **Not removable:** `codec.rs`'s big-endian rule also serves
  `PRONUNCIATION`'s prefix scan — `pronunciation_range`
  (`store.rs:188-196`) bounds `[token]` to `[token+1]` over a
  `(token, keys)` composite. The bigram model change does not touch it.

So: drop the bigram half, keep big-endian for the pronunciation key. The
compat backend does not want big-endian anyway — libpinyin writes
native-endian, so the BDB bigram key is raw little-endian `u32`.

## 4. Binding route: write a cxx shim against libdb 5.3.28

No viable Rust binding exists. `libdb` / `libdb-sys` are v0.1.1, last
updated 2020-03-12, and **statically link their own Berkeley DB** —
which is the wrong shape when the entire point is to interoperate with
files the user's own libpinyin wrote through the *system* libdb.
`bdb_parser` is a pure-Rust file parser at v0.1.1; `db185` targets DB
1.85; the rest are application-specific.

- **Target:** libdb **5.3.28**, under the **Sleepycat License** (copyleft,
  not BSD; Oracle relicensed the 6.x line to AGPLv3), which is why the
  distros checked here — the Debian/Ubuntu family — pin to 5.3 (Fedora
  and openSUSE were not reachable from this environment to confirm).
  `libdb5.3-dev` / `libdb5.3t64` across noble, resolute and stonking;
  `libdb-dev` is a metapackage resolving to it, and it is what libpinyin
  build-depends on. The Sleepycat License's
  copyleft/source-availability terms and their interaction with
  oxpinyin's GPL-3.0-or-later through the linked shim are a Phase 2
  licensing item, not settled here.
- **Shim surface (small — libpinyin uses no transactions, no
  environment, no secondary indices; every `open` passes `NULL` for both
  env and txn):** `db_create`, `->open`, `->close`, `->get`, `->put`,
  `->del`, `->cursor` with `->c_get`/`->c_close`, `->sync`.

### `SingleGram` chunk layout — preserve verbatim

The value stored under each bigram key, field for field
(`ngram.cpp:31-74`, `:178-196`):

```text
offset 0 .. 4   guint32  total_freq          native-endian
offset 4 .. N   SingleGramItem[]             (N-4)/8 entries
                struct SingleGramItem {      8 bytes, no padding
                    phrase_token_t m_token;  guint32, native-endian
                    guint32        m_freq;   native-endian
                }
```

- The item array is kept **sorted ascending by `m_token`**:
  `insert_freq` places each item at
  `lower_bound(begin, end, …, token_less_than)`.
- A fresh gram is a 4-byte chunk with `total_freq == 0`, and
  `get_length` asserts that a zero-item gram has zero total.
- The key is `&index` with `size = sizeof(phrase_token_t)` — the raw
  four bytes of a `guint32`, native-endian. Not a string, not
  length-prefixed.

Any write path that does not reproduce this byte-for-byte corrupts a
user's profile silently. The brief's STOP on non-byte-compatible writes
applies here first.

## 5. AWAITING YOUR DECISION — #180 and system data

Phase 2's format detection would open libpinyin's `phrase_index.bin`,
`pinyin_index.bin` and `bigram.db`. Those *are* libpinyin-generated
runtime data consumed by a backend, which is what #180's invariant
addresses:

> The canonical linguistic source is the source of truth. No oxpinyin
> backend may require libpinyin-generated runtime data as its input.

There is **no conflict on user data** — a user's trained bigram is their
own state, not generated runtime data. The tension is on system data,
and it turns on one word:

- **"require" = *cannot function without*** → no conflict. oxpinyin
  still compiles every table from `model20` and runs on its native path;
  the compat path is an added capability, and no build or test step
  depends on it.
- **"require" = *may consume at all*** → the system-data half of Phase 2
  is forbidden as specified.

The first reading matches the invariant's stated purpose — retiring the
`oxpinyin-migrate` producer route so no *producer* depends on an oracle
export — and is what this note recommends. Proposed clarification to add
to `datagen-model20.md`:

> The invariant governs production. No producer may take
> libpinyin-generated runtime data as its input, and oxpinyin must be
> able to build every table it needs from the canonical archive alone.
> It does not govern consumption at runtime: opening a libpinyin-format
> file that is already on the user's system is a feature of the drop-in,
> permitted precisely because nothing in the build or test pipeline
> depends on it.

**Not resolved in code, and Phase 2 is blocked on it** — the one item
awaiting a maintainer *decision*. Everything else above needs no
decision, but the ↯ items still carry their Phase 2 *confirmation* tasks
(notably §2's B-tree order against a real BDB, and the §4 licensing
review); those are work, not open questions for the maintainer.
