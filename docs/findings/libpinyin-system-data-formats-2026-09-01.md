# libpinyin system-data formats vs the #269 sysimage — source-level comparison

Date: 2026-09-01 · Status: **investigation / architecture decision input** ·
Answers the maintainer's challenge to PR #269 ("why a new oxpinyin-specific
storage format at all?").

Everything below was verified against the pinned source tree
(`libpinyin-2.11.91`, `0c5e80e1`) **and** against the actual bytes of the
data files generated inside the two pinned builds (`/opt/libpinyin-{kc,
tkrzw}/lib/libpinyin/data` in the perf-matrix container — hexdumps quoted
where they settle a classification).

---

## 1. What libpinyin actually generates and consumes

`data/Makefile.am` runs three tools over the same model20 text tables:

```text
gen_binary_files --gen-punct-table   → the DBMs + the per-library chunk files
import_interpolation                 → bigram.db (+ λ in table.conf)
gen_unigram                          → +1 unigram sweep over the chunk files
```

`utils/storage/gen_binary_files.cpp` (152 lines, read in full) drives, for
the four system libraries and again for the twelve addon libraries:

- `ChewingLargeTable2::attach(pinyin_index.bin, READWRITE|CREATE)` — the
  **pinyin index**;
- `PhraseLargeTable3::attach(phrase_index.bin, …)` — the **phrase table**;
- `FacadePhraseIndex::load_text` per library, then `save_phrase_index` /
  `save_dictionary` — the **per-library phrase-index chunk files**
  (`gb_char.bin`, `gbk_char.bin`, `opengram.bin`, `merged.bin`, and
  `art.bin` … `technology.bin`);
- optionally `PunctTable::attach(punct.bin)`.

### 1.1 File-by-file classification (bytes confirm the source)

| File | Container (verified magic) | Content schema | Runtime consumer | Init access |
|---|---|---|---|---|
| `pinyin_index.bin` | **KC TreeDB** (`4b 43 0a … 31 08`) or **Tkrzw TreeDBM** (`TkrzwHDB…`), per `--with-dbm` | key = packed `ChewingKey[L]` (2 B/key), **two key spaces** + empty-value prefix markers; value = `PinyinIndexItem2[L][]` = `{u32 token, ChewingKey keys[L]}[]` (4+2L B/record) | `ChewingLargeTable2` (`chewing_large_table2_{kyotodb,tkrzwdb}.cpp`) | lazy `open()`; per-lookup `Get` + in-value binary search |
| `phrase_index.bin` | same DBM family | key = raw **UCS-4** phrase bytes; value = `u32 token[]` | `PhraseLargeTable3` | lazy `open()`; per-lookup `Get` |
| `gb_char.bin` … `merged.bin`, addon `*.bin` | **`MemoryChunk` file**: `{u32 length, u32 checksum}` then payload (§1.2) | the phrase **index**: token → item with text, unigram, pronunciations | `SubPhraseIndex` via `MemoryChunk::mmap` (`_load_phrase_library`, `pinyin.cpp:234-318`) | **mmap** + one-pass checksum |
| `bigram.db` | KC **HashDB** (`30 03`) / Tkrzw **HashDBM** | key = prev token LE; value = `total:u32` + `{next,count}[]` — the schema oxpinyin already froze | `Bigram` (`ngram_tkrzwdb.cpp:96` `new HashDBM`) | lazy attach |
| `punct.bin` | KC TreeDB / Tkrzw TreeDBM | token → NUL-separated puncts | `PunctTable` | lazy attach |
| `table.conf` | text | library map, λ, format version | `SystemTableInfo2` | read |

Ground-truth hexdumps (this changes the reading of "`.bin`" in the
earlier docs — the DBMs and the chunk files are different families that
happen to share an extension):

```text
KC cell   pinyin_index.bin: 4b 43 0a 00 10 0e 06 bc 31 08   → KC TreeDB
KC cell   bigram.db:        4b 43 0a 00 10 0e 06 bc 30 03   → KC HashDB
Tkrzw cell pinyin_index.bin: 54 6b 72 7a 77 48 44 42 0a 07 → Tkrzw DBM
both      gb_char.bin:      b9 59 2d 00 | e2 41 f3 50 | payload…
                            length = 0x2D59B9 = filesize − 8  ✔
                            payload: [total_freq u32][index_one u32 = 0x11]
                                     [index_two u32][index_three u32 = payload_len]  ✔
```

### 1.2 The chunk-file (MemoryChunk) layout — the mmap'd format

`src/include/memory_chunk.h` (`save`/`mmap`) and
`src/storage/phrase_index.cpp` (`SubPhraseIndex::{load,store,get_phrase_item}`),
verified against `gb_char.bin` bytes:

```text
u32 length      == filesize − 8        (mmap checks this)
u32 checksum    = XOR-fold over payload (mmap recomputes → pages it all in)
payload:
  u32  total_freq                     Σ item unigram freqs (post gen_unigram)
  u32  index_one, index_two, index_three   section offsets; index_three == payload_len
  byte separator at [16] and at [index_two−1], [index_three−1]
  @ index_one: u32 offsets[token & PHRASE_MASK]   (0 = no item; dense per library)
  @ index_two: entries:
      { u8 phrase_length, u8 n_pronunciations, u32 unigram_freq,
        ucs4_t phrase[phrase_length],
        { ChewingKey keys[phrase_length], u32 freq } × n_pronunciations }
```

`get_phrase_item` is one offset read plus a view over the mapped entry —
the architecture PR #269 was asked to recreate. **libpinyin has it, and it
covers exactly the phrase index.**

### 1.3 The pinyin index is *not* a mmap format — including in libpinyin

`pinyin_index.bin` is a backend DBM. libpinyin's init is sub-millisecond
with it because `attach` only opens the container (no walk), and lookups
are per-keystroke `Get`s (`chewing_large_table2_tkrzwdb.cpp:133-162`:
encode key → `m_db->Get` → copy value into a `MemoryChunk` → binary search
`PinyinIndexItem2` records). Two key spaces live in the one DBM
(`pinyin_phrase3.h:160-177`):

- complete index: keys with `m_tone` zeroed;
- incomplete index: keys reduced to `m_initial` only;

and `add_index_internal` writes **empty-value entries for every shorter
prefix** (`chewing_large_table2_tkrzwdb.cpp:284-296`) — that is
libpinyin's `SEARCH_CONTINUED` mechanism. There is no separate
initial-key list; oxpinyin's `initial_keys` derivation is a re-expression
of this second key space.

---

## 2. The comparison, and the verdict on the central question

> **Why does PR #269 need a new oxpinyin-specific storage format at all?**

**It does not.** There is no technical necessity, and the project's own
frozen policy forbids the direction:

- `docs/findings/compatibility-policy.md` (2026-08-28, maintainer-decided):
  "the same binary interface, **the same file formats**, the same
  observable behaviour" — a new runtime data format is not one of the four
  divergence classes; outside them, "everything … is a defect to be
  reverted."
- The performance goal does not require it: libpinyin demonstrates
  ~0.9 ms init and 12–18 MiB RSS **on these exact files**
  (`perf-backend-matrix-2026-08-31.md`) — the #260 baseline itself says so.

How the sysformat happened, honestly:

1. **It inherited a pre-policy divergence.** oxpinyin's data layer is the
   frozen export schema (`data-layer-export.md`, 2026-08-10): apostrophe-
   joined spelling keys and `{token, freq}` records — a re-keyed
   projection of the same logical content, not libpinyin's formats.
2. **That layer was created under a policy that no longer exists.** The
   2026-08-10 route explicitly "sidesteps the undocumented format
   entirely" because at the time reading upstream source was restricted
   ("Deriving the real bucket function would require reading upstream
   C++"). The source policy has since been replaced ("Reading and
   copying upstream C++ source is expected and encouraged"), which
   removes the reason the export re-keying existed.
3. **#269's scope was "repack the current tables, change nothing
   behaviorally."** Given that framing, a new image format was the
   mechanically safest packaging — but the right response to the
   representation question, under the current policy, is to converge on
   libpinyin's formats, not to crystallize the divergence into new
   on-disk artifacts and a second writer/reader pipeline.

So: #269's *mechanics* (mmap reader, offset views, precompute-at-datagen,
the measurement harness) are the right machinery pointed at the wrong
format.

### Format-by-format

| oxpinyin need | libpinyin's file | Direct consumption? | Gap in oxpinyin today |
|---|---|---|---|
| phrase text per token, pronunciations, per-item freqs | `gb_char.bin`-family chunk files | **yes — mmap, layout verified** | reader for the chunk layout (the #269 `MappedFile` + views transfer directly); UCS-4→UTF-8 at the boundary |
| pinyin-key → candidates | `pinyin_index.bin` DBM | **yes — via the store backends oxpinyin already links** (KC TreeDB / Tkrzw TreeDBM) | `ChewingKey` encoder (`SyllableKey` ↔ packed 16-bit key) — explicit tables in `chewing_key.cpp`/`chewing_enum.h`; incomplete key space replaces `initial_keys`; prefix markers replace the boundary probe |
| phrase text → tokens (predict/import) | `phrase_index.bin` DBM | **yes** | UCS-4 key encoding; suggestion = the `Jump`/`Next` continuation walk (`phrase_large_table3_tkrzwdb.cpp:150-190`) — exactly `suggest_after` |
| bigram | `bigram.db` | **yes — value schema already frozen in oxpinyin** | KC HashDB / Tkrzw **HashDBM** open paths (the store opens TreeDB/TreeDBM only) |
| punct | `punct.bin` | **yes** | container already TreeDB; schema already frozen |
| λ, library map | `table.conf` | **yes** | already read |

## 3. The seven questions

1. **What files/formats does libpinyin generate?** §1.1's table — two DBM
   indexes (plus addon aggregates), per-library MemoryChunk phrase-index
   files, a HashDB(M) bigram, a TreeDB(M) punct, `table.conf`.
2. **Exact layouts?** §1.2 (chunk file), §1.3 + `pinyin_phrase3.h:181`
   (`PinyinIndexItem2 = {u32 token, ChewingKey[L]}`, `ChewingKey` =
   16-bit bitfield `initial:5|middle:2|final:5|tone:3|pad:1`,
   `chewing_key.h:41`), `phrase_large_table2.cpp:62`
   (`PhraseIndexItem2 = {u32 token, ucs4_t phrase[L]}`; the v3 DBM value
   is the bare `u32 token[]`, `phrase_large_table3.cpp:28-52`).
3. **Which runtime structures consume them?** `FacadeChewingTable2` →
   `ChewingLargeTable2{,_kyotodb,_tkrzwdb}`; `FacadePhraseTable3` →
   `PhraseLargeTable3{,…}`; `FacadePhraseIndex` → `SubPhraseIndex` over
   `MemoryChunk::mmap`; `Bigram`; `PunctTable`.
4. **Does libpinyin already provide the mmap/offset representation #269
   recreates?** For the phrase index — yes, exactly (`SubPhraseIndex`).
   For the pinyin index — no; upstream serves it from a lazily-opened DBM
   with per-lookup gets. PR #269's premise that the pinyin index is an
   mmap-able libpinyin artifact was wrong; upstream's own design is
   lazy-attach + point reads.
5. **Can oxpinyin directly mmap and consume the existing files?** The
   chunk files: yes, byte-for-byte (verified). The DBMs: yes, through the
   KC/Tkrzw backends oxpinyin already links — the drop-in model is
   compile-time backend coupling, the same choice a distribution makes
   with `--with-dbm` (a KC-built oxpinyin reads a KC-built distro's data;
   this is libpinyin's own constraint, not a new one).
6. **Can the datagen pipeline emit the same format?** Yes —
   `gen_binary_files` is 152 lines over the `load_text` parsing datagen
   already reproduces natively from model20. Emitting libpinyin's
   key/value schemas through the existing per-backend writers satisfies
   both this policy *and* the W15 canonical-source invariant (which
   constrains **provenance** — no producer may consume libpinyin-generated
   runtime data — not format; a native emitter preserves it).
7. **Legitimate reasons Rust cannot consume the representation?** None
   fundamental. Real constraints, all addressable: (a) DBM files are
   backend-coupled — inherent to the drop-in model; (b) `ucs4_t`/`u32`
   fields are host-endian — fine across the supported LE targets, same
   constraint libpinyin itself has; (c) `MemoryChunk::mmap`'s checksum
   pass pages the whole file in at open — matching it reproduces
   libpinyin's ~10 MiB RssFile profile, and is the faithful default;
   (d) the engine seam changes: `Dictionary` keys become packed
   `ChewingKey` sequences instead of apostrophe strings — an internal
   API change, not an observable one.

## 4. Consequences for PR #269

Recommended disposition (matches the maintainer's stated preference):

1. **Do not merge as-is** — mark draft.
2. **Retain** the parts that are format-independent and load-bearing:
   `MappedFile` (the mmap machinery, the crate's documented exception),
   the offset-view reader technique, the measurement harness
   (`load_profile` modes, the matrix rerun protocol), and the findings.
3. **Remove** `sysformat.rs`, the oxpinyin image writer/reader, the
   `.bin` artifacts, and the fixture migration.
4. **Rebuild the data layer on libpinyin's formats**, staged:
   - **P1 — chunk reader:** `MemoryChunk`/`SubPhraseIndex` reader
     (mmap, checksum, offset array, item views). Serves phrase text,
     pronunciations, per-item freqs. Replaces the phrase-index half;
     parity suite stays green because the logical content is the
     proven-identical export content (`datagen-model20.md`'s
     entry-for-entry equivalence).
   - **P2 — ChewingKey encoder + chewing DBM reader:** the 16-bit key
     packing from `chewing_key.cpp`, the two key spaces, the
     empty-value prefix markers; `Dictionary` lookup key becomes the
     packed sequence. Replaces the pinyin-index half and deletes the
     `initial_keys` derivation (upstream has no such list).
   - **P3 — phrase table + bigram + punct:** `phrase_index.bin` reads
     (incl. the suggestion continuation walk), lazy `bigram.db` attach
     (HashDB(M) open paths in the store), `punct.bin`.
   - **P4 — datagen emits the formats:** a native `gen_binary_files`
     equivalent so test dirs, fixtures, and differentials are
     self-contained and byte-schema-compatible with distro data; the
     runtime accepts a directory regardless of which producer wrote it.
5. **Expected end state:** init and RSS approach libpinyin's own profile
   (nothing eager anywhere — strictly better than #269's 21.7 ms, which
   still slurps the bigram), and the data dir is the distro's:

```text
                 ┌── libpinyin runtime
libpinyin data ──┤
                 └── oxpinyin runtime        (the drop-in promise)
model20 ── oxpinyin-datagen ── same-format files   (the W15 invariant)
```

The one planning note: P2 is the substantial item (the syllable encoder
interacts with `SyllableKey`'s frozen ids and the parser seams), and it
is precisely the work that was withdrawn in August under the retired
clean-room rule — it is now both permitted and, per this investigation,
required by the compatibility policy.

---

## 5. P1 verification addendum (2026-09-01, reader implemented)

Implementing the chunk-file reader (`crates/oxpinyin-data/src/phrase_library.rs`)
pinned down three facts the source read had left open, all now asserted
against the pinned build's real files:

- **Entry offsets are 0-based, first item at 8.**
  `SubPhraseIndex::add_phrase_item`: `offset = m_phrase_content.size();
  if (0 == offset) offset = 8;` — bytes 0..8 of the entry area are
  reserved so `0` stays the "no item" sentinel.
- **`index_one` is 17 and carries no alignment requirement** — 4 words +
  separator; the loader reads u32s at unaligned offsets. Only the
  offset *array length* is u32-sized.
- **The chunk files are backend-independent**: byte-identical SHA-256s
  from the KC and Tkrzw cells (only the DBMs differ with `--with-dbm`).

Real-data validation (`OXPINYIN_LIBPINYIN_DATA`, strict-gated): the four
system libraries open under mmap + upstream checksum; **138,096 items —
exactly the frozen export's phrase-token count**; 的 (0x010005DB) carries
its two model20 readings verbatim (`de 2213855`, `di 11000`), 锕
(0x01000001) its single `a 7`; the addon `art.bin` rides the same format.

One P2-critical ABI fact surfaced by a size probe compiled against the
pinned headers: `sizeof(PinyinIndexItem2<1>) == 8`, not 6 — the C++
struct carries tail padding to its 4-byte alignment (L=1: 4+2 padded to
8; L=3: padded to 12; even L unpadded). Any Rust reader of the chewing
DBM's value arrays must stride by `sizeof(PinyinIndexItem2<L>)`, not by
the field sum.
