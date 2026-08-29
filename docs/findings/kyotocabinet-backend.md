# Kyoto Cabinet compat backend

Date: 2026-08-28 · Status: **backend implemented and measured; three
premises of the brief did not hold and are corrected below** · Branch:
`feat/kyotocabinet-backend`.

Retargeted from `feat/bdb-backend`, which is shelved. What carries over
from the Berkeley DB Phase 1 survey
(`berkeleydb-compat-phase1.md`, `berkeleydb-compat-open-items.md`) is
inherited unchanged: the `SingleGram` layout, the `memcmp`-then-length
key order, the §5 `#180` decision ("require" = *cannot function
without*), and the format-version-field prerequisite for the codec
change.

## Three premises that did not hold

Each was checked before writing code, and each changes the shape of the
work.

### 1. This machine is Ubuntu with a **Berkeley DB** libpinyin

The brief says "RHEL 10.2 is the system you have, and it uses KC", and
names the drop-in test's target as "an existing KC-format user data dir
populated by the real RHEL 10.2 libpinyin".

Measured: `/etc/os-release` is `Ubuntu 24.04.4 LTS (noble)`. The
installed `libpinyin-data` 2.8.1 package's `bigram.db` carries Berkeley
DB's `DB_HASHMAGIC` (`0x00061561` at offset 12), which
`oxpinyin_data::layout` now reports directly:

```text
installed libpinyin data at /usr/lib/x86_64-linux-gnu/libpinyin/data is Berkeley DB (hash)
```

Kyoto Cabinet was not installed at all; `libkyotocabinet-dev` 1.2.80 was
installed from `noble/universe` to do this work. So **there is no
KC-format libpinyin data on this machine**, and the drop-in test as
specified has no target here. What could be done instead is below.

### 2. libpinyin's Kyoto Cabinet files are **not** named `.kch`/`.kct`

The brief says "File extensions: .kch (HashDB) and .kct (TreeDB). Use
these in format detection." Detection on those extensions would never
match anything.

libpinyin's file names are compile-time constants that do not vary with
the DBM backend (`src/pinyin_internal.h:56-58`):

```c
#define SYSTEM_BIGRAM  "bigram.db"
#define USER_BIGRAM    "user_bigram.db"
#define DELETED_BIGRAM "deleted_bigram.db"
```

`--with-dbm=BerkeleyDB`, `--with-dbm=KyotoCabinet` and `--with-dbm=Tkrzw`
all produce a file called `bigram.db`, in three mutually unreadable
formats. `.kch`/`.kct` are Kyoto Cabinet's own convention, which
libpinyin does not use.

**So detection is by magic, and it must discriminate the backends** —
opening a Kyoto Cabinet file with Berkeley DB, or the reverse, is a
failure a user sees as "my input method is broken":

| Library | Bytes | Where |
|---|---|---|
| Berkeley DB Hash | `0x00061561` | offset 12 |
| Kyoto Cabinet `HashDB` | `KC\n\0` then `0x30` | offsets 0 and 8 |
| Kyoto Cabinet `TreeDB` | `KC\n\0` then `0x31` | offsets 0 and 8 |

Both measured. `oxpinyin_data::layout::detect` returns
`DataLayout::Compat(Dbm)` rather than a bare `Compat`, so the caller
learns which backend can read the directory, and an unrecognised
`bigram.db` — a tkrzw build's, say — is refused with its bytes rather
than mis-opened.

### 3. The C API cannot open libpinyin's filename without a `#type=`

This one is load-bearing: without it the C-API binding route does not
work at all.

`kclangc.h`'s `kcdbopen` is `PolyDB`, which picks the database class from
the **path suffix** — `.kch` a file hash database, `.kct` a file tree
database, "otherwise, this function fails" (`kclangc.h:312-320`). Since
libpinyin's file is called `bigram.db`, that is exactly the failing case.
Measured on 1.2.80:

```text
named.kch (correct suffix)         opens
bigram.db (libpinyin's name)       FAILS: invalid operation
bigram.db#type=kch                 opens
```

`PolyDB` accepts tuning parameters after a `#`, among them `type=`
(`kcpolydb.h:496-515`). So `ffi::Db::open` always appends `#type=kch` or
`#type=kct`, and refuses a path that itself contains `#` rather than
letting it silently change which database is opened.
`a_file_named_bigram_db_opens_despite_having_no_kyoto_suffix` is the test
that notices if this is ever dropped; removing the override makes all six
compat tests fail with `invalid operation: unknown database type`.

## Confirmed from the pin, first-hand

The Ubuntu archive serves libpinyin's source where `codeload.github.com`
does not (403), so these were read rather than taken on trust —
`libpinyin_2.8.1.orig.tar.gz` from `archive.ubuntu.com`. **That route is
worth noting on its own: the oracle may be provisionable this way.**

- **Key encoding** — `ngram_kyotodb.cpp:128`: `const char * kbuf = (char
  *) &index;` with `sizeof(phrase_token_t)`. Byte for byte what the
  Berkeley DB backend does. Native-endian raw `u32`, not a string, not
  length-prefixed.
- **Database classes** — `ngram_kyotodb.cpp:115` is `m_db = new HashDB`;
  `phrase_large_table3_kyotodb.cpp:102` and
  `chewing_large_table2_kyotodb.cpp:87` are `new TreeDB`. (Both also use
  `ProtoTreeDB` opened on the path `"-"`, which is an in-memory working
  copy, not a file format.)
- **`SingleGram` is backend-independent** — `ngram.cpp` is unconditional
  in `src/storage/Makefile.am:72`, while `ngram_bdb.cpp` and
  `ngram_kyotodb.cpp` are added under `if BERKELEYDB` / `if
  KYOTOCABINET`. This is what licenses carrying the Berkeley DB
  measurements over, and what makes the STOP condition "KC and BDB
  produce different logical content for the same key" a statement about
  containers rather than records.
- **No comparator is set** — neither KC table source contains `rcomp`, so
  `TreeDB` uses Kyoto Cabinet's default `LEXICALCOMP`: byte-wise, shorter
  key first on a shared prefix. That is the store's one rule
  (`store-key-ordering.md`) exactly, so this backend satisfies it without
  configuration.

## What was measured

### Real libpinyin records, through the Kyoto Cabinet backend

`tools/kc/run-compat-check.sh`. Because no KC-built libpinyin is
installed here, `tools/kc/bdb-to-kc.c` transcribes the installed
`bigram.db`'s records into a Kyoto Cabinet container — real keys, real
chunks, copied byte for byte with **no Rust in the transcription** — and
the tests read that:

```text
transcribed 56359 records (1849609 successor items)
56359 records, 1849609 successor items, all round-tripped
counts match the model20-derived 56,359 / 1,849,609
```

Every record re-encodes to the bytes on disk. The counts are what
`oxpinyin-datagen` derives independently from `model20`
(`datagen-model20.md`), so two unrelated routes agree.

**How strong this is, stated plainly.** It proves the backend reads
libpinyin's *records* out of a Kyoto Cabinet container. It does **not**
prove that a Kyoto-Cabinet-built libpinyin would have written that exact
container — only a machine with one installed can show that. The runner
prints which of the two cases it took, and the strong case is taken
automatically when a KC-built installation is present.

Non-vacuity confirmed by reversion: dropping the `#type=` override fails
all six compat tests; encoding big-endian instead of native fails the
round-trip on the first record.

### Sanitizers

`tools/kc/run-sanitizers.sh`, clean — 54 tests including the full walk,
under ASan with LeakSanitizer.

This gate carries more weight here than it did for Berkeley DB, because
**the hazard is ownership rather than lifetime**. The brief carried
Berkeley DB's cursor hazard over ("do not hold the returned pointer past
a cursor move"); that is the wrong shape for Kyoto Cabinet, and building
to it would have produced a bug. `kcdbget`, `kccurgetkey`,
`kccurgetvalue` and `kccurget` each return a **caller-owned, freshly
allocated** region — "should be released with the kcfree function"
(`kclangc.h:577-578`, `:923-924`, `:942-943`). Nothing expires when the
cursor moves; the failure modes are a leak, a double free, or a free
through the wrong allocator, and all three are what ASan and LSan see.
The answer in code is ownership, not a lifetime bound: `Buf` owns the
region and its `Drop` calls `kcfree`. `kccurget`'s value pointer is
**interior** to the key's allocation, so `Record` holds one `Buf` and an
offset and frees once.

rustc has no `undefined` sanitizer (`-Zsanitizer=` accepts address, cfi,
dataflow, hwaddress, kcfi, kernel-address, kernel-hwaddress, leak,
memory, memtag, safestack, shadow-call-stack, thread, realtime), so UBSan
cannot apply to Rust. libkyotocabinet's own internals are not
instrumented either.

## Two places this backend is better than the Berkeley DB one

- **Real transactions.** Kyoto Cabinet gives a standalone handle
  `kcdbbegintran`/`kcdbendtran`, so `WriteStore::write` is the library's
  own transaction: writes go straight through, reads see them, and an
  `Err` rolls back. The Berkeley DB backend has to buffer and replay
  precisely because libpinyin's environment-less opens leave it no
  transaction to use.
- **`kcdbcount`** is tracked rather than counted, so `BigramDb::len` is
  O(1).

## Bindings: generated fresh

A weaker argument than the Berkeley DB backend's, and worth being honest
about. `KCDB` and `KCCUR` are **opaque** one-pointer wrappers
(`kclangc.h:48-58`), so unlike `DB`/`DBT`/`DBC` there is no exposed
struct layout to get wrong and a checked-in binding could not silently
misread a field. What is still baked from the header are the open-mode
`enum` constants — stable across Kyoto Cabinet's life, but values a
checked-in binding would carry from one machine to another.

Generated fresh anyway, for one concrete reason rather than symmetry: it
keeps the declarations and the linked library in lockstep, which is what
makes the `KCVERSION` gate meaningful. The cost is a build-time libclang,
small here because linking already requires the development package that
carries the header — only libclang is added, and only for an opt-in
feature.

## Not done, and why

- **Item 6, the default backend.** Already correct as briefed: redb stays
  the default and Kyoto Cabinet is `--features kyotocabinet`. The
  Berkeley DB portability lesson applies unchanged — `registry.rs` puts
  `DefaultStore` in a `static Mutex`, so it must be `Send`, and
  `ci.yml`'s `test-portable` job runs `oxpinyin-data` and `oxpinyin-user`
  on macOS and Windows, where neither library exists.
- **Item 7's MemoryChunk half.** `phrase_index.bin` (7.8 M) and
  `pinyin_index.bin` (10.6 M) are libpinyin `MemoryChunk` images, not
  Kyoto Cabinet files, and no reader for that format exists in this tree.
  A follow-up PR; `oxpinyin_data::layout` is the seam it plugs into, and
  it now reports which DBM the directory uses, which that reader will
  need.
- **The codec simplification.** Unchanged from the Berkeley DB finding:
  correct as analysis, but the user store has no format-version field, so
  changing the native bigram key encoding would misread every existing
  profile silently.

## Not verified

- **The drop-in test did not run**, and could not: there is no
  Kyoto-Cabinet-built libpinyin on this machine (premise 1), and
  ibus-libpinyin is not installed either. The gate stands unmet.
- **The frozen candidate and sentence pins were not re-measured.** They
  are measured against the pin-built oracle, which
  `tools/oracle/build-oracle.sh` cannot fetch here (`codeload.github.com`
  answers 403). This change adds a backend behind an off-by-default
  feature and touches no decode path, so it cannot move them — an
  argument, not a measurement. **The Ubuntu archive route above may make
  the oracle provisionable**; that is worth trying before the next
  measurement is needed.
- **`--features tkrzw` could not be built** — no tkrzw `pkg-config` on
  this machine, and the Ubuntu package is the broken one
  (`tkrzw-distro-compat.md`). Default, `kyotocabinet` and `lmdb` were all
  built and tested.
