# Data formats — pinned oracle table files

Date: 2026-08-12 · Status: recorded; human review required before freeze.
Revision: Tkrzw files converted to redb (formerly via oxpinyin-migrate FFI
bridge, now committed under fixtures/w3/); direct Rust parsing replaced by
redb loader; fixtures updated to .redb.

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
File                        Size          Format family       Loader format
──────────────────────────────────────────────────────────────────────────
bigram.db               20,016,880   Tkrzw Hash DB (v1.3)  → bigram.redb
pinyin_index.bin         5,700,608   Tkrzw Hash DB (v1.7)  → pinyin_index.redb
phrase_index.bin         4,089,856   Tkrzw Hash DB (v1.7)  → phrase_index.redb
addon_pinyin_index.bin   1,254,400   Tkrzw Hash DB (v1.7)  → addon_pinyin_index.redb
addon_phrase_index.bin     963,584   Tkrzw Hash DB (v1.7)  → addon_phrase_index.redb
punct.bin                  534,528   Tkrzw Hash DB (v1.7)  → punct.redb

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

1.  **Tkrzw Hash DB** (6 files) — key-value stores.  Converted to
    portable redb databases (committed under `fixtures/w3/`) before
    loading.
2.  **Custom content files** (16 files) — phrase/character
    dictionaries with a common 28-byte header, an index table, and
    variable-length records.  Parsed directly by `oxpinyin-data`.
3.  **table.conf** — text configuration mapping library indices to
    content-file paths.

---

## 1. Tkrzw Hash DB → redb conversion

All `*_index.bin`, `punct.bin`, and `bigram.db` files are Tkrzw
Hash Database Manager format (HashDBM class, v1.x) in the oracle
installation.  These are **not** parsed directly in Rust.  Instead,
The conversion was originally performed by `oxpinyin-migrate` via FFI to
libtkrzw (Linux-only); the resulting redb databases are committed under
`fixtures/w3/` and `oxpinyin-data` loads them on any platform.

### 1.1 Conversion (historical)

The conversion was originally performed by `oxpinyin-migrate`
(removed from the workspace):

- Read all key-value pairs from the Tkrzw file via the C++ bridge.
- Filtered empty-key tombstone records.
- Sorted entries lexicographically by key for deterministic output.
- Wrote a redb v4 database with a single table `"data"`.

The resulting redb files were **byte-identical** across repeated runs
and are now committed under `fixtures/w3/` (frozen).

### 1.2 redb schema

| Aspect        | Value                                     |
|---------------|-------------------------------------------|
| Table name    | `"data"`                                  |
| Key type      | `&[u8]` (raw bytes from Tkrzw key)        |
| Value type    | `&[u8]` (raw bytes from Tkrzw value)      |
| Key ordering  | Lexicographic (redb B-tree)               |

No additional metadata or indexes.  Key and value interpretation is
the callers responsibility (see §1.3).

### 1.3 Loader (oxpinyin-data)

`oxpinyin_data::LookupTable` wraps a `redb::ReadOnlyDatabase`:

```rust
let table = LookupTable::open(path)?;
let value: Option<Vec<u8>> = table.get(key)?;
let count: u64 = table.len()?;
let all: Vec<(Vec<u8>, Vec<u8>)> = table.iter()?;
```

Portable pure Rust — no glib, no Linux-only deps, no `unsafe`.

### 1.4 Key-value semantics

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

### 1.5 Tkrzw header (reference only)

The oracle's on-disk Tkrzw files use the following header layout
(big-endian multi-byte integers), documented for completeness only.
The converter uses libtkrzw, not direct parsing.

| Offset | Size  | Field                 | Notes                         |
|--------|-------|-----------------------|-------------------------------|
| 0      | 9     | Magic                 | `"TkrzwHDB\n"`                |
| 9      | 1     | fmt_minor             | 7 (3 for bigram.db)           |
| 10     | 1     | fmt_major             | 1                             |
| 11     | 1     | opaque_sz             | varies                        |
| 12     | 1     | offset_width          | 1 (hash fingerprints)         |
| 13     | 1     | align_pow             | 4                             |
| 14–15  | 2     | cyclic_magic          | varies                        |
| 16–19  | 4     | static_flags          | 0                             |
| 20–23  | 4     | num_buckets           | u32 BE                        |
| 24–31  | 8     | num_records           | u64 BE                        |
| 32–39  | 8     | eff_data_size         | u64 BE                        |
| 40–47  | 8     | file_size             | u64 BE                        |
| 48–55  | 8     | mod_time              | u64 BE (µs)                   |
| 56–63  | 8     | db_type + padding     |                               |

---

## 2. Custom content file format

All `*.bin` files that are NOT Tkrzw Hash DB files share a common
28-byte header, a 35-entry primary index, and an optional secondary
index. Two sub-families exist:

-   **Addon dictionaries** (art, culture, …, technology): 35 pinyin
    consonant groups, each containing variable-length phrase records.
    Formula-driven record parsing covers 11/12 files completely; the
    12th (geology.bin) has records with nested sub-records (see §2.5).
-   **System dictionaries** (gb_char, gbk_char, opengram, merged):
    same header, but data-section layout not yet ported.
    Out of scope for initial loader implementation per §5.

### 2.1 Common header (28 bytes)

All fields are **u32 little-endian** unless noted.

| Offset | Field        | Description                                   |
|--------|--------------|-----------------------------------------------|
| 0      | data_size    | Total file size minus 8 bytes                 |
| 4      | magic        | Per-file checksum/identifier (varies)         |
| 8      | num_items    | Number of entries (`nitems`) in the file      |
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

-   `magic`: A per-file u32 that varies. Appears to be derived from
    the file contents (possibly a checksum) and is NOT a type
    identifier. Cannot be relied upon for format detection.
-   `num_items` (`nitems`): For addon dictionaries, this is the exact
    phrase count. For system dictionaries, may use a different encoding.
-   `capacity`: The maximum `phrase_index` value + 1 in the token
    address space. Phrase indices range from 0 to `capacity - 1`.
-   `num_groups`: Always 35, corresponding to the 35 pinyin initial
    consonant groups (upstream: `PinyinCustomSettings::NUMBER_OF_INITIALS = 35`).

### 2.2 Primary index (35 entries)

Immediately after the 28-byte header, at file offset 28:

```
Offset 28–167:  35 × u32 LE  (140 bytes)
```

Each entry is an index into a **virtual address space** — these are NOT
file offsets. For small files (e.g., culture.bin, data section < 1 KB)
the index values exceed the file size. They represent positions in a
memory-mapped structure the oracle constructs at load time.

For the Rust loader, these virtual addresses are unused; the loader
parses the data section sequentially.

### 2.3 Secondary index

When `nitems` exceeds `ngroups` (35), a **secondary index** follows the
primary index at offset 168. It contains `secondary_entries = nitems - ngroups`
u32 LE values, each monotonically increasing.

```
secondary_index_offset = 168
secondary_index_size   = secondary_entries × 4
```

The secondary index also uses virtual addresses (not file offsets). Its
purpose (in the oracle's memory-mapped layout) is per-record lookup from
the 35×N group table.

### 2.4 Data section

The data section starts after all index tables:

```
data_start = 168 + secondary_entries × 4 + preamble_size

preamble_size:
  - 6 bytes  when nitems ≤ 35  (zero bytes in observed files)
  - 10 bytes when nitems > 35  (observed as `0x00 0x00 0x64 0x00 0x00 0x00 0x64 0x00 0x00 0x00`)
```

Records are **back-to-back** with NO separator between them. Each record
is self-describing via its `n_gram` and `flags` bytes.

#### Record structure

Every record has a 6-byte header:

| Offset | Size | Field              | Description                        |
|--------|------|--------------------|------------------------------------|
| 0      | 1    | n_gram             | Number of tokens in the phrase     |
| 1      | 1    | flags              | Record type / layout selector      |
| 2      | 4    | phrase_frequency   | u32 LE, overall phrase frequency   |

The `flags` byte determines the number of token pairs and their encoding:

| flags | # token pairs | Token encoding                                | Size formula (ng = n_gram)         |
|-------|--------------|-----------------------------------------------|------------------------------------|
| 0     | ng           | Standard: all `[u32 token][u32 freq]`          | `6 + ng × 8`                       |
| 1     | ng           | Hybrid: first `[u32][u32]`, rest `[u32][u16]` | `6 + 8 + (ng−1)×6 + 2` (ng ≥ 2)   |
| 2     | ng + 1       | Standard: all `[u32 token][u32 freq]`          | `6 + (ng+1) × 8`                  |
| 3     | ng + 2       | Standard, plus padding (see §2.4.1)            | See below                          |
| 4     | ng + 3       | Standard, plus padding (see §2.4.1)            | See below                          |

##### Flags=0: Standard format

All `ng` token pairs use full-width encoding: `[token: u32 LE][freq: u32 LE]`.

Size: `6 + n_gram × 8` bytes.

##### Flags=1: Compact format (hybrid)

The **first** token pair uses full-width encoding:
`[token0: u32 LE][freq0: u32 LE]`.

Each **subsequent** pair (pairs 1 … ng−1) uses a compact 6-byte layout:
`[token: u32 LE][freq: u16 LE][pad: 2 bytes]`. The pad is `0x00 0x00`.

A 2-byte zero pad follows the last compact pair.

Total size for ng ≥ 2: `6 + 8 + (ng−1) × 6 + 2 = 14 + (ng−1) × 6 + 2`.
For ng = 1: size = 14 (header 6 + one standard pair 8, no compact pairs, no pad).

**The `0x64 00 00 00` (frequency 100) misconception**: Earlier analysis
mistook the frequent occurrence of `0x64` (= 100) at record boundaries
for a separator. In the compact format, the **last token's frequency is
often exactly 100**, and the 2-byte zero pad follows. This creates a
`64 00 00 00` byte sequence that looks like a separator when inspected
at alignment boundaries, but it is just coincidental data.

##### Flags=2: Standard format with 1 extra token pair

Size: `6 + (n_gram + 1) × 8`.

##### Flags=3, 4: Standard format with extra tokens and padding

For flags ≥ 3, the number of token pairs is `n_gram + flags − 1`.

The base size is `6 + (ng + flags − 1) × 8`, plus padding:

| Condition       | Extra padding bytes           |
|-----------------|-------------------------------|
| ng ≥ 3 (fl=3)   | `(ng − 2) × 2`                |
| ng ≥ 5 (fl=4)   | `(ng − 2) × 2 + 2`            |

These formulas are verified for all ng ≤ 10 across 16,829 records in 11 files.

##### Flags=3 with ng ≥ 13: Nested sub-records

For flags=3 records with ng ≥ 13 (observed only in geology.bin),
the payload after the 6-byte header contains **nested fl=1 sub-records**
instead of flat token pairs. Each sub-record uses the compact (fl=1)
format.

The outer record size is `6 + Σ(sub_record_sizes)`. The termination
condition for sub-record scanning is **not yet fully determined**;
records with this structure are handled by a fallback parser that reads
sub-records iteratively until the accumulated structure becomes invalid.

40 of 534 geology.bin records use this nested layout; they require
further analysis.

#### Record size summary

| Flags | Condition          | Size in bytes                                        |
|-------|--------------------|------------------------------------------------------|
| 0     | any ng             | `6 + ng × 8`                                         |
| 1     | ng = 1             | `14`                                                 |
| 1     | ng ≥ 2             | `14 + (ng−1) × 6 + 2`                                |
| 2     | any ng             | `6 + (ng+1) × 8`                                     |
| 3     | ng < 13            | `6 + (ng+2) × 8 + (ng−2)×2` (ng ≥ 3); 0 pad for ng<3|
| 3     | ng ≥ 13            | `6 + Σ(sub_record_sizes)` — nested                  |
| 4     | ng < 5             | `6 + (ng+3) × 8`                                    |
| 4     | ng ≥ 5             | `6 + (ng+3) × 8 + (ng−2)×2 + 2`                     |

#### Example: compact record (culture.bin, offset 174)

```
02 01 01 00 00 00   header: ng=2, fl=1, freq=1
89 5b 00 00         token[0] = 0x5B89
7b 51 00 00         freq[0]  = 0x517B = 20859
80 01 00 00         token[1] = 0x0180
35 02               freq[1] u16 = 0x0235 = 565
00 00               pad
```

Size: `6 + 8 + (2−1)×6 + 2 = 22`. ✓

**Phrase index**: Within a content file, records are addressed by their
**0-based sequential index** in the data section. The `phrase_index`
field of a `phrase_token_t` is this index. The `capacity` header field
equals the maximum valid index + 1.

#### Token encoding

Each token pair encodes a `phrase_token_t` = `(library_index << 16) | phrase_index`.
The `library_index` identifies which dictionary file (0–15 per §3.3);
the `phrase_index` is the record index within that file.

### 2.5 Parsing validation (addon dictionaries)

| File          | nitems | Status       | Notes                              |
|---------------|--------|--------------|------------------------------------|
| culture.bin   | 34     | ✓ 34/34      | All compact (fl=1)                 |
| art.bin       | 488    | ✓ 488/488    |                                    |
| economy.bin   | 1,005  | ✓ 1,005/1,005|                                    |
| history.bin   | 178    | ✓ 178/178    |                                    |
| nature.bin    | 411    | ✓ 411/411    |                                    |
| science.bin   | 381    | ✓ 381/381    |                                    |
| sport.bin     | 93     | ✓ 93/93      |                                    |
| technology.bin| 391    | ✓ 391/391    |                                    |
| life.bin      | 2,331  | ✓ 2,331/2,331| Includes fl=0,3,4                 |
| people.bin    | 1,955  | ✓ 1,955/1,955| Includes fl=0,3,4                 |
| society.bin   | 8,454  | ✓ 8,454/8,454| All flag types                    |
| geology.bin   | 534    | ~492/534     | 40 records use nested sub-records |

11 of 12 addon files parse completely with the formulas above. The
remaining 40 geology.bin records (fl=3, ng ≥ 13) need the nested
sub-record parser, which will be finalised during loader implementation.

### 2.6 System dictionaries

System dictionaries (gb_char, gbk_char, opengram, merged) use the same
28-byte header and index tables. Their data-section format has not been
ported.

**Out of scope** for initial loader implementation per §5.

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

For oxpinyin loaders, we maintain separate loader instances for the
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
