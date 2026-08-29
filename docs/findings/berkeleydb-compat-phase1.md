# BerkeleyDB compatibility — Phase 1 survey

Date: 2026-08-28 · Status: **survey; STOP for confirmation before Phase 2**
· Branch: `claude/pr4-berkeleydb-compat`.

Everything below is read first-hand from the pin (`0c5e80e`, libpinyin
2.11.91) and from the real packages installed on an Ubuntu system, not
from the register. Four things came out differently from the brief; they
are marked **↯**.

## (a) Binding route

**No viable Rust binding exists. The cxx shim is the route.**

| Crate | Latest | Updated | Verdict |
| --- | --- | --- | --- |
| `libdb` / `libdb-sys` | 0.1.1 | 2020-03-12 | "statically linked" bindings, unmaintained six years |
| `bdb` | 0.0.1 | 2018-10-28 | abandoned at 0.0.1 |
| `bdb_parser` | 0.1.1 | 2024-07-02 | pure-Rust *file* parser, not libdb |
| `db185` | 0.3.0 | 2026-05-12 | DB 1.85 format, not DB5 |
| `bitcoin-bdb`, `gnucobol-rs-bdb-format` | — | — | application-specific readers |

`libdb`/`libdb-sys` are additionally the wrong *shape*: they statically
link their own Berkeley DB. The whole point here is to interoperate with
files the user's own libpinyin wrote through the system libdb, so the
shim must link the same shared library the distro ships, not a vendored
copy.

**Version to target: libdb 5.3.** Confirmed across the Ubuntu series
(`libdb5.3-dev` and `libdb5.3t64` in noble, resolute and stonking;
`libdb-dev` is a metapackage resolving to it), and it is what libpinyin
build-depends on. 5.3.28 is under the **Sleepycat License** (a copyleft
license, not BSD); Oracle relicensed the 6.x line to AGPLv3, which is why
every distro pins to 5.3. Debian's is the same source Ubuntu syncs.
Fedora and openSUSE could not be checked — their metadata is unreachable
from this environment.

The shim surface needed is small, because libpinyin's own use of BDB is
small: `db_create`, `->open`, `->close`, `->get`, `->put`, `->del`,
`->cursor` + `->c_get`/`->c_close`, `->sync`. No transactions, no
environment, no secondary indices — every `open` below passes
`NULL` for both the environment and the transaction.

## (b) System data — what `pinyin_init(systemdir, …)` opens

**↯ The system dir is not under `$datadir`.** `data/Makefile.am` sets
`libpinyin_dbdir = $(libdir)/libpinyin/data`, so on Debian/Ubuntu it is
`/usr/lib/x86_64-linux-gnu/libpinyin/data` — a multiarch *library* path,
not `/usr/share`. Anything that guesses `share` will not find it.

Verified against the installed `libpinyin-data` package rather than the
Makefile alone:

| File | Size | Format |
| --- | --- | --- |
| `phrase_index.bin` | 7.8 M | libpinyin `MemoryChunk` image |
| `pinyin_index.bin` | 10.6 M | libpinyin `MemoryChunk` image |
| `addon_phrase_index.bin` | 970 K | same, addon libraries |
| `addon_pinyin_index.bin` | 1.5 M | same |
| **`bigram.db`** | **25.9 M** | **Berkeley DB Hash, version 9, native byte-order, pagesize 4096** |
| 16 × `*.bin` table files | 1 K – 3.0 M | per-library phrase tables |
| `table.conf` | 1.2 K | table manifest |

`bigram.db` identified from its header (`magic=0x00061561`, `version=9`),
matching `ngram_bdb.cpp`'s `DB_HASH` open. **↯ Note the system bigram is
a BDB file too** — the brief frames BDB as a user-data concern, but the
compat reader needs it for system data as well.

`punct.bin` is in the pin's `Makefile.am` list but absent from the
installed 2.8.1 package: it arrives in a later release. A format
detector must not require it.

## (c) User data layouts, from the pin

### Bigram — `ngram_bdb.cpp`, `DB_HASH`

Open: `m_db->open(m_db, NULL, dbfile, NULL, DB_HASH, db_flags, 0644)`.

- **Key** — `db_key.data = &index; db_key.size = sizeof(phrase_token_t)`:
  the raw four bytes of a `guint32`, **native-endian**. Not a string, not
  length-prefixed.
- **Value** — `db_data.data = single_gram->m_chunk.begin(); .size =
  m_chunk.size()`: the entire `SingleGram` blob, every successor of
  `prev` in one value.

The chunk, field by field (`ngram.cpp:31-74`, `:178-196`):

```text
offset 0 .. 4   guint32  total_freq          native-endian
offset 4 .. N   SingleGramItem[]             (N-4)/8 entries
                struct SingleGramItem {      8 bytes, no padding
                    phrase_token_t m_token;  guint32, native-endian
                    guint32        m_freq;   native-endian
                }
```

The item array is kept **sorted ascending by `m_token`**: `insert_freq`
places each new item at `lower_bound(begin, end, …, token_less_than)`.
A fresh gram is a 4-byte chunk whose `total_freq` is 0, and `get_length`
asserts that a zero-item gram has zero total.

No ordering requirement on the *key*: every access is a point
`get`/`put`/`del` by `prev`.

### Phrase and chewing tables — `DB_BTREE`

- `phrase_large_table3_bdb.cpp`: key = the phrase as raw `ucs4_t[]`,
  `size = phrase_length * sizeof(ucs4_t)`; value = the entry chunk.
- `chewing_large_table2_bdb.cpp`: key = raw `ChewingKey[]`,
  `size = phrase_length * sizeof(ChewingKey)`; value = the entry chunk.

**↯ Neither calls `set_bt_compare`**, so both use BDB's default B-tree
comparator: a byte-wise `memcmp`, then length, over the key's raw
in-memory bytes. The two tables key on different structs, so the byte
layout — and therefore the order — differs:

- **Phrase table — `ucs4_t[]`.** `ucs4_t` is a `guint32`, so the key is a
  `u32` array; on the little-endian targets, `memcmp` orders it
  lexicographically over low-byte-first `u32` bytes — neither integer
  order nor big-endian order. A code point crossing a 256 boundary
  reorders relative to integer order because its low byte compares first.
- **Chewing table — `ChewingKey[]`.** `ChewingKey` is **not** a `u32`: it
  is libpinyin's 16-bit packed bit-field struct (`chewing_key.h` —
  `m_initial` / `m_middle` / `m_final` / …). Its `sizeof` and its
  in-memory byte layout are fixed by the compiler's bit-field packing and
  the storage unit's endianness — ABI-dependent — so the `memcmp` runs
  over those raw ABI bytes, which match no simple integer order of the
  phonetic fields.

Both orders are read from the pin's source, not from a real file; each is
the blind spot the brief flags for the ordering tests, and **must be
confirmed experimentally against a real BDB before anything depends on
it** (still an open Phase 2 ↯ item, below).

## (d) Codec simplification — ↯ partly, not wholesale

The brief's premise is right for the bigram and wrong for the codec as a
whole.

**Right:** `UserStore::bigram_successors(prev)` (`store.rs:872-891`) is a
range scan from `encode_token_pair(prev, Token::MIN)` to
`encode_token_pair(prev, Token::MAX)` over a `(prev, cur)` composite key.
That scan is the only reason the *bigram* key needs integer-ordered
bytes. Under the blob-per-prev model it becomes a single point `get(prev)`
followed by decoding the item array, and the requirement disappears
entirely.

**Wrong:** `codec.rs`'s big-endian rule is not the bigram's alone. The
`PRONUNCIATION` table prefix-scans by token — `pronunciation_range`
(`store.rs:188-196`) bounds `[token]` to `[token+1]` over a
`(token, keys)` composite — and that scan is untouched by the bigram
model change. Dropping big-endian wholesale would break it.

So: the bigram key's ordering requirement can be dropped; `codec.rs`
keeps big-endian for the pronunciation key. And the compat backend does
not want big-endian anyway — libpinyin writes native-endian, so the BDB
bigram key is raw little-endian `u32` on every machine we target.

## (e) Does #180 forbid reading the user's existing BDB files?

The invariant (`datagen-model20.md`): *"The canonical linguistic source
is the source of truth. No oxpinyin backend may require
libpinyin-generated runtime data as its input."*

**On user data: no conflict, and it is not close.** The invariant is
about *production* — every runtime-data producer consumes the pinned
`model20.text.tar.gz` directly, and the ROADMAP note says what it was for:
retiring the `oxpinyin-migrate` route so no producer depends on an oracle
export. A user's trained bigram is not libpinyin-generated *runtime data*;
it is the user's own state, and reading it is a product feature of the
drop-in.

**On system data: there is a genuine tension, and it needs the written
answer the brief asks for.** Phase 2's format detection would open
libpinyin's `phrase_index.bin`, `pinyin_index.bin` and `bigram.db` — and
those *are* libpinyin-generated runtime data, consumed by a backend.
Whether that violates #180 turns on one word:

- If **"require"** means *cannot function without*, there is no conflict.
  oxpinyin still compiles every table it needs from model20 and runs on
  its native path; the compat path is an added capability, and no build or
  test step depends on it.
- If **"require"** means *may consume at all*, the system-data half of
  Phase 2 is forbidden as specified.

The first reading is the one that matches the invariant's stated purpose,
and it is the one this survey recommends. Proposed clarification to add to
`datagen-model20.md`:

> The invariant governs production. No producer may take
> libpinyin-generated runtime data as its input, and oxpinyin must be able
> to build every table it needs from the canonical archive alone. It does
> not govern consumption at runtime: opening a libpinyin-format file that
> is already on the user's system is a feature of the drop-in, permitted
> precisely because nothing in the build or test pipeline depends on it.

## STOP — what Phase 2 needs to decide

1. **The #180 clarification above, in writing.** Phase 2's format
   detection is blocked on it.
2. **Confirmation of the four ↯ items**, since each changes Phase 2's
   shape: the system dir is under `$(libdir)`, the system bigram is
   itself BDB, the B-tree order is `memcmp` over the keys' raw bytes (LE
   `u32` for the `ucs4_t` phrase key, an ABI-dependent bit-field layout
   for the `ChewingKey` chewing key), and big-endian stays in `codec.rs`
   for the pronunciation key.

Not blocking, but worth deciding with them: `bigram.db` is 25.9 MB of
system data in a format we would then read through the shim on the
default path, which makes the shim's ASan/UBSan gate (already required by
the brief) load-bearing for every session rather than only for trained
users.
