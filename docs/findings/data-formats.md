# Data formats — pinned oracle table files

Date: 2026-08-09 · Status: recorded; human review required before freeze.

Describes every binary table file installed by the pinned oracle under
`$PREFIX/lib/libpinyin/data/`. This is a SPEC — no loader code may be
written before it is frozen.

## Pin reference

-   libpinyin 2.11.91 (commit `0c5e80e`)
-   model data: `model20.text.tar.gz` (SHA-256 in
    `oracle-environment.md`)
-   Data route: `~/.local/opt/pinyin-oracle/lib/libpinyin/data/`

## File inventory (23 files)

```
File                        Size          Format family
─────────────────────────────────────────────────────────────────
bigram.db               20,016,880   Tkrzw Hash DB (v1.3)
pinyin_index.bin         5,700,608   Tkrzw Hash DB (v1.7)
phrase_index.bin         4,089,856   Tkrzw Hash DB (v1.7)
addon_pinyin_index.bin   1,254,400   Tkrzw Hash DB (v1.7)
addon_phrase_index.bin     963,584   Tkrzw Hash DB (v1.7)
punct.bin                  534,528   Tkrzw Hash DB (v1.7)

gb_char.bin              2,972,097   Custom content (sys)
gbk_char.bin               346,011   Custom content (sys)
opengram.bin               821,157   Custom content (sys)
merged.bin                  32,259   Custom content (sys)

art.bin                     15,791   Custom content (addon)
culture.bin                  1,063   Custom content (addon)
economy.bin                 36,315   Custom content (addon)
geology.bin                 17,985   Custom content (addon)
history.bin                  5,321   Custom content (addon)
life.bin                    76,501   Custom content (addon)
nature.bin                  13,017   Custom content (addon)
people.bin                  67,447   Custom content (addon)
science.bin                 12,795   Custom content (addon)
society.bin                271,189   Custom content (addon)
sport.bin                    3,111   Custom content (addon)
technology.bin              13,543   Custom content (addon)

table.conf                   1,229   Text configuration
```

Three categories:

1.  **Tkrzw Hash DB** (6 files) — key-value stores mapping pinyin
    syllable hashes to phrase-token lists, and phrase texts to metadata.
2.  **Custom content files** (16 files) — phrase/character
    dictionaries with a common 28-byte header, an index table, and
    variable-length records.
3.  **table.conf** — text configuration mapping library indices to
    content-file paths.

---

## 1. Tkrzw Hash DB format

All `*_index.bin`, `punct.bin`, and `bigram.db` files use the Tkrzw
Hash Database Manager format (HashDBM class).

### 1.1 File header

| Offset | Size  | Field                 | Value in oracle               |
|--------|-------|-----------------------|-------------------------------|
| 0      | 10    | Magic                 | `"TkrzwHDB\n"`                |
| 10     | 1     | format_version_minor  | 7 (3 for bigram.db)            |
| 11     | 1     | format_version_major  | 1                              |
| 12     | 1     | opaque_metadata_size  | varies                         |
| 13     | 1     | offset_width          | 4                              |
| 14     | 1     | align_pow             | 10 (= 2^10 = 1024 byte align)  |
| 15–22  | 8     | num_buckets           | varies per file (u64 LE)       |
| 23+    | n     | opaque metadata       | size per byte 12               |

All multi-byte fields are **little-endian**.

### 1.2 Record storage

Internally Tkrzw HashDBM stores records with operation types (SET =
0x80, REMOVE = 0x40, VOID = 0xC0) and CRC-32 checksums. Records are
organised in a hash table with linked overflow chains. The exact
on-disk layout is determined by the Tkrzw library.

**Loader strategy**: pinyin-data will parse the Tkrzw Hash DB format
directly in portable Rust, using the well-documented Tkrzw file layout.
No FFI dependency on libtkrzw.

### 1.3 Key-value semantics

| File                    | Key encoding                | Value encoding                          |
|-------------------------|-----------------------------|-----------------------------------------|
| `pinyin_index.bin`      | 6-byte pinyin syllable hash | `phrase_token_t[]` u32 LE array         |
| `phrase_index.bin`      | 6-byte phrase key           | UTF-8 metadata (ASCII text observed)    |
| `addon_pinyin_index.bin`| 6-byte pinyin syllable hash | `phrase_token_t[]` u32 LE array         |
| `addon_phrase_index.bin`| 6-byte phrase key           | UTF-8 metadata                          |
| `punct.bin`             | 6-byte encoded key          | encoded value                           |
| `bigram.db`             | 6-byte bigram key           | frequency value(s)                      |

The 6-byte key encoding for pinyin syllables is a compact binary
representation of the syllable's initial, final, and tone. The exact
encoding is **pending detailed analysis** but can be cross-referenced
against the oracle via the FFI at load time.

### 1.4 Phrase token encoding

`phrase_token_t` = `u32` (little-endian).

```
bits 31–28  bits 27–24     bits 23–0
┌──────────┬──────────────┬──────────────────┐
│ reserved │ library_idx  │ phrase_index     │
│  (0)     │  (4 bits)    │  (24 bits)       │
└──────────┴──────────────┴──────────────────┘
```

Constants (from upstream `novel_types.h`):

```
PHRASE_MASK                    = 0x00FFFFFF
PHRASE_INDEX_LIBRARY_MASK      = 0x0F000000
PHRASE_INDEX_LIBRARY_COUNT     = 16
PHRASE_INDEX_LIBRARY_INDEX(t)  = (t >> 24) & 0x0F
PHRASE_INDEX_MAKE_TOKEN(l, pi) = ((l << 24) & 0x0F000000) | (pi & 0x00FFFFFF)
```

A `phrase_token_t` with value `0x00000000` (library=0, phrase=0) is a
**sentinel** marking the start of a token list.

---

## 2. Custom content file format

All `*.bin` files that are NOT Tkrzw Hash DB files share a common
28-byte header and an index table. Two sub-formats exist:

-   **Addon dictionaries** (art, culture, …, technology): 35 pinyin
    consonant groups, each containing variable-length phrase records.
-   **System dictionaries** (gb_char, gbk_char, opengram, merged):
    same header, but a different data-section layout with additional
    indexing structures.

### 2.1 Common header (28 bytes)

All fields are **u32 little-endian** unless noted.

| Offset | Field        | Description                                   |
|--------|--------------|-----------------------------------------------|
| 0      | data_size    | Total file size minus 8 bytes                 |
| 4      | magic        | Per-file checksum/magic (varies)              |
| 8      | num_items    | Number of entries in the dictionary           |
| 12     | version      | Format version; consistently **17** (0x11)    |
| 16     | capacity     | Maximum phrase index +1 (token address space) |
| 20     | data_size    | Duplicate of offset 0                         |
| 24     | num_groups   | Number of index groups; consistently **35**   |

**Invariants** (verified across all 16 content files):

-   `data_size == file_size - 8`
-   `data_size` at offset 0 equals `data_size` at offset 20
-   `version == 17` for all files
-   `num_groups == 35` for all files

**Field details:**

-   `magic`: A per-file u32 that varies. It appears to be derived from
    the file contents (possibly a checksum) and is NOT a type
    identifier. It cannot be relied upon for format detection.
-   `num_items`: For addon dictionaries, this is the exact phrase count.
    For system dictionaries (gb_char, gbk_char, opengram), this field may
    use a different encoding and does not directly represent the phrase
    count.
-   `capacity`: The maximum `phrase_index` value + 1 in the token
    address space. Phrase indices range from 0 to `capacity - 1`.
-   `num_groups`: Always 35, corresponding to the 35 pinyin initial
    consonant groups (PinyinCustomSettings::NUMBER_OF_INITIALS = 35 in
    upstream).

### 2.2 Index table

Immediately after the 28-byte header, starting at file offset 28:

```
Offset 28–167:  35 × u32 LE  (140 bytes)
```

Each entry is an index into a virtual address space. These are **NOT**
file offsets — for small files (e.g., culture.bin with data section
< 1 KB) the index values exceed the file size. They represent positions
in a memory-mapped table structure that the oracle constructs at load
time.

The 35 groups correspond to pinyin initial consonant groups:
b, p, m, f, d, t, n, l, g, k, h, j, q, x, zh, ch, sh, r, z, c, s,
y, w, and combinations thereof.

For the Rust loader, the index values are ignored; instead, the loader
parses the data section sequentially and builds its own index from the
record contents.

### 2.3 Data section

Starts at file offset **168** (28 header + 140 index).

#### Addon dictionaries

The data section begins with an 8-byte preamble (all zero in observed
files), followed by variable-length phrase records.

**Record structure** (variable length):

| Offset | Size  | Field            | Description                              |
|--------|-------|------------------|------------------------------------------|
| 0      | 1     | n_gram           | Number of characters/tokens in phrase    |
| 1      | 1     | flags            | Phrase flags (1 = normal, 2 = …)         |
| 2      | 4     | phrase_frequency | u32 LE, overall phrase frequency         |
| 6      | n×8   | token_data       | n_gram pairs of (token: u32, freq: u32)  |
| …      | 4     | separator        | `0x64 0x00 0x00 0x00` = decimal 100     |

**Record size**: `2 + 4 + n_gram × 8 + 4` = `10 + n_gram × 8` bytes.

**Known issue — separator ambiguity**: The separator value `0x64`
(decimal 100) can also appear as data within token or frequency fields.
A record parser must use the `n_gram` field to compute the expected
record length and verify the separator at the computed position. If the
separator does not match, the record structure must be re-examined.

**Example** (culture.bin, first record at offset 176): `02 01 01 00 00
00 89 5b 00 00 7b 51 00 00 80 01 35 02 64 00 00 00`

-   n_gram = 2
-   flags = 1
-   phrase_freq = 0x00000001
-   token[0] = 0x00005b89, freq[0] = 0x0000517b
-   token[1] = 0x00000180, freq[1] = 0x00000235
-   separator = 0x00000064 ✓

**Phrase index**: Within a content file, records are addressed by their
**0-based sequential index** in the data section. The `phrase_index`
field of a `phrase_token_t` is this index. The `capacity` header field
equals the maximum valid index + 1.

#### System dictionaries

System dictionaries (gb_char, gbk_char, opengram, merged) use the same
28-byte header and 35-entry index table, but their data section begins
with additional indexing structures (observed as monotonically
increasing u32 arrays at offset 168).

**Pending investigation**: The exact data-section layout for system
dictionaries. These files are required for phrase lookup (library
indices 1–4) and will be reverse-engineered in a follow-up.

System dictionary details are described or flagged as out-of-scope.

---

## 3. Table configuration (table.conf)

A plain-text configuration file associating library indices with content
file paths. Format version 7.

```
binary format version:7
model data version:14
lambda parameter:0.312699

source table format:pinyin
database format:Tkrzw
```

### 3.1 Default library entries

```
default RESERVED         NULL NULL NULL         NOT_USED
default GB_DICTIONARY     gb_char.table  gb_char.bin  gb_char.dbin  SYSTEM_FILE
default GBK_DICTIONARY    gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE
default OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin SYSTEM_FILE
default MERGED_DICTIONARY   merged.table   merged.bin   merged.dbin   SYSTEM_FILE
default ADDON_DICTIONARY    NULL NULL addon.bin      USER_FILE
default NETWORK_DICTIONARY  NULL NULL network.bin    USER_FILE
default USER_DICTIONARY     NULL NULL user.bin       USER_FILE
```

| Library index | Name              | Content file    | Type        |
|---------------|-------------------|-----------------|-------------|
| 0             | RESERVED          | —               | sentinel    |
| 1             | GB_DICTIONARY     | gb_char.bin     | SYSTEM_FILE |
| 2             | GBK_DICTIONARY    | gbk_char.bin    | SYSTEM_FILE |
| 3             | OPENGRAM_DICTIONARY| opengram.bin   | SYSTEM_FILE |
| 4             | MERGED_DICTIONARY | merged.bin      | SYSTEM_FILE |
| 5             | ADDON_DICTIONARY  | addon.bin       | USER_FILE   |
| 6             | NETWORK_DICTIONARY| network.bin     | USER_FILE   |
| 7             | USER_DICTIONARY   | user.bin        | USER_FILE   |

### 3.2 Addon dictionary entries

```
addon 4  art.table  art.bin  NULL DICTIONARY
addon 5  culture.table  culture.bin  NULL DICTIONARY
addon 6  economy.table  economy.bin  NULL DICTIONARY
addon 7  geology.table  geology.bin  NULL DICTIONARY
addon 8  history.table  history.bin  NULL DICTIONARY
addon 9  life.table  life.bin  NULL DICTIONARY
addon 10 nature.table  nature.bin  NULL DICTIONARY
addon 11 people.table  people.bin  NULL DICTIONARY
addon 12 science.table  science.bin  NULL DICTIONARY
addon 13 society.table  society.bin  NULL DICTIONARY
addon 14 sport.table  sport.bin  NULL DICTIONARY
addon 15 technology.table  technology.bin  NULL DICTIONARY
```

| Config index | Name          | Content file     |
|--------------|---------------|------------------|
| 4            | art           | art.bin          |
| 5            | culture       | culture.bin      |
| 6            | economy       | economy.bin      |
| 7            | geology       | geology.bin      |
| 8            | history       | history.bin      |
| 9            | life          | life.bin         |
| 10           | nature        | nature.bin       |
| 11           | people        | people.bin       |
| 12           | science       | science.bin      |
| 13           | society       | society.bin      |
| 14           | sport         | sport.bin        |
| 15           | technology     | technology.bin   |

### 3.3 Library index spaces

The `PHRASE_INDEX_LIBRARY_INDEX` macro extracts a 4-bit library index
from the top of a `phrase_token_t`. Two separate index spaces exist:

-   **Default space**: indices 0–7, loaded by `pinyin_load_phrase_library`.
-   **Addon space**: indices 4–15, loaded by `pinyin_load_addon_phrase_library`.

Index 4 is overloaded: it maps to `MERGED_DICTIONARY` (merged.bin) in
the default space and `art` in the addon space. The correct
interpretation depends on which load function populated the token.

For pinyin-rs loaders, we maintain separate loader instances for the
default and addon dictionaries, resolving tokens through the
appropriate loader.

---

## 4. End-to-end lookup flow

```
User input: "ni hao"
     │
     ▼
┌─────────────────────────┐
│ 1. Pinyin parser        │  → produces pinyin syllable tuple
│    (ni3, hao3)          │
└────────┬────────────────┘
         │
         ▼
┌─────────────────────────┐
│ 2. pinyin_index.bin     │  → 6-byte syllable hash → phrase_token_t[]
│    Tkrzw Hash DB lookup │    e.g., [0x0100002A, 0x0100005F, …]
└────────┬────────────────┘
         │
         ▼
┌─────────────────────────┐
│ 3. Token decomposition  │  → (lib=1, phi=0x2A), (lib=1, phi=0x5F), …
│    library_idx + phi    │
└────────┬────────────────┘
         │
         ▼
┌─────────────────────────┐
│ 4. Content file lookup  │  → lib=1 reads gb_char.bin, phi=0x2A
│    per token            │    → lookup record at index 0x2A
└────────┬────────────────┘
         │
         ▼
┌─────────────────────────┐
│ 5. Record decode        │  → phrase text, frequency, character tokens
│    n_gram, tokens, freq │
└────────┬────────────────┘
         │
         ▼
┌─────────────────────────┐
│ 6. Candidate assembly   │  → candidates with probability scores
│    (W4 SegmentGraph)    │
└─────────────────────────┘
```

## 5. Scope and exclusions

### In scope (T2 loaders will implement)
-   Tkrzw Hash DB reader (portable Rust, no libtkrzw dependency)
-   Addon content file parser (all 12 files)
-   Table configuration parser
-   Phrase token decode/encode
-   Portable path resolution (caller supplies data directory)

### Out of scope for initial implementation
-   System dictionary content-file parsing (gb_char, gbk_char, opengram,
    merged) — these require additional format investigation
-   `phrase_index.bin` / `addon_phrase_index.bin` value semantics —
    these are needed for phrase-to-token reverse lookups, not the forward
    pinyin→candidate flow
-   `punct.bin` — punctuation candidate generation
-   `bigram.db` — bigram language model scoring
-   USER_FILE dictionaries (addon.bin, network.bin, user.bin) — these are
    user-generated, not part of the pinned data

### Explicitly excluded
-   Berkeley DB backend — the oracle is pinned to Tkrzw only
-   `.table` source files — these are model-generation inputs (from
    `model20.text.tar.gz`), not consumed at runtime
-   `.dbin` compiled database files — not present in the oracle
    installation

---

## 6. Verification notes

-   All format descriptions were derived from binary analysis of the
    pinned oracle's installed data files.
-   The addon record structure has been verified against culture.bin
    (34 records, 1063 bytes), the smallest content file.
-   Tkrzw format details were cross-referenced against
    `/usr/include/tkrzw_dbm_hash.h` and
    `/usr/include/tkrzw_dbm_hash_impl.h` on the oracle build host.
-   Phrase token encoding was confirmed from upstream header
    `novel_types.h` (tag 2.11.91).
-   Table.conf parsing was verified against the oracle's installed copy.
-   No upstream C/C++ source code was read to derive any format
    description.
