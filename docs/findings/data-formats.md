# Data formats — libpinyin system data files

This record describes the files in a libpinyin system data directory
(`$PREFIX/lib/libpinyin/data/`) and how oxpinyin reads them.
oxpinyin reads these files directly, with no conversion or import step.
An unmodified libpinyin data directory for the same backend opens as is
([compatibility policy, "Backends and user data"](compatibility-policy.md#backends-and-user-data)).

Source: libpinyin at the pin (`074a2219`; `tools/oracle/oracle-pin.txt`
is authoritative). The readers in `crates/oxpinyin-data` cite the exact
upstream lines. The per-file DBM records are:

- [`pinyin-dbm-format-2026-09-01.md`](pinyin-dbm-format-2026-09-01.md) — `pinyin_index.bin`;
- [`phrase-dbm-format-2026-09-01.md`](phrase-dbm-format-2026-09-01.md) — `phrase_index.bin`;
- [`bigram-punct-format-2026-09-01.md`](bigram-punct-format-2026-09-01.md) — `bigram.db` and `punct.bin`.

## 1. File inventory

`data/Makefile.am` runs three programs over the model's text tables:
`gen_binary_files --gen-punct-table`, `import_interpolation` and
`gen_unigram`. `oxpinyin-utils` provides the same three programs under
the same names. `gen_binary_files` writes the default set (four system
libraries) and the addon set (twelve addon libraries).

| File | Container | Content | oxpinyin reader |
|---|---|---|---|
| `pinyin_index.bin`, `addon_pinyin_index.bin` | backend tree DBM | key: packed `ChewingKey[L]`, two key spaces and empty-value prefix markers; value: `PinyinIndexItem2<L>[]` | `chewing_table.rs` (`ChewingLargeTable2`) |
| `phrase_index.bin`, `addon_phrase_index.bin` | backend tree DBM | key: UCS-4 phrase text; value: `u32 token[]` | `phrase_table.rs` (`PhraseLargeTable3`) |
| `gb_char.bin`, `gbk_char.bin`, `opengram.bin`, `merged.bin`; addon `art.bin` … `technology.bin` | `MemoryChunk` file (§2) | the phrase index: token → item with text, unigram and pronunciations | `phrase_library.rs` (`SubPhraseIndex`), mmap |
| `bigram.db` | backend hash DBM | key: previous token; value: `total` + `{next, count}[]` | `bigram_table.rs` (`Bigram`) |
| `punct.bin` | backend tree DBM | key: token; value: zero-terminated UCS-4 runs | `punct.rs` (`PunctTable`) |
| `table.conf` | text (§3) | versions, λ, database format, library map | `table_info.rs` (`SystemTableInfo2`) |

The backend tree DBM is Kyoto Cabinet `TreeDB`, tkrzw `TreeDBM` or
Berkeley DB `DB_BTREE`. The hash DBM is `HashDB`, `HashDBM` or
`DB_HASH`. libpinyin selects the backend with `--with-dbm`; oxpinyin
selects it with a cargo feature. The DBM files are therefore
backend-specific. The chunk files are backend-independent: the per-backend
`fixtures/w3` directories hold byte-identical copies.

Every reader is lazy. Opening a directory opens the DBM handles and maps
the chunk files. Each lookup is a point read. All `u32` and `ucs4_t`
fields are host-endian upstream; every supported target is
little-endian, and the readers refuse to compile for big-endian targets.

The DBM files and the chunk files share the `.bin` extension but are
different families. File magic separates them:

```text
Kyoto Cabinet pinyin_index.bin: 4b 43 0a 00 10 0e 06 bc 31 08   TreeDB
Kyoto Cabinet bigram.db:        4b 43 0a 00 10 0e 06 bc 30 03   HashDB
tkrzw         pinyin_index.bin: 54 6b 72 7a 77 48 44 42 0a 07   "TkrzwHDB" header
any backend   gb_char.bin:      {length u32}{checksum u32} then the payload
```

---

## 2. MemoryChunk file format (per-library phrase index)

Upstream: `src/include/memory_chunk.h` (`MemoryChunk::save`, `mmap`,
`get_check_sum`) and `src/storage/phrase_index.cpp`
(`SubPhraseIndex::load`, `store`, `get_phrase_item`, `get_range`).
oxpinyin: `crates/oxpinyin-data/src/chunk_format.rs` holds the shared
constants and checksum; `phrase_library.rs` reads and `chunk_write.rs`
writes.

```text
u32 length      == file size − 8 (as guint32); mmap refuses a mismatch
u32 checksum    XOR of the payload's u32 words, tail bytes shifted in;
                mmap recomputes it, which pages the whole file in
payload:
  u32  total_freq                       Σ item unigram frequencies
  u32  index_one, index_two, index_three  section offsets
  '#'  separator at [16], [index_two − 1] and [index_three − 1]
  @ index_one: u32 offsets[token & PHRASE_MASK]   0 = no item
  @ index_two: items
      { u8 phrase_length, u8 n_pronunciations, u32 unigram_freq,
        ucs4_t phrase[phrase_length],
        { ChewingKey keys[phrase_length], u32 freq } × n_pronunciations }
  index_three == payload length
```

Facts the readers rely on:

- `index_one` is always 17: four `u32` words plus the separator. It has
  no alignment, so the loader reads `u32`s at unaligned offsets.
- Item offsets are relative to `index_two`. The first item is at 8:
  `SubPhraseIndex::add_phrase_item` reserves bytes 0..8 so that 0 stays
  the "no item" value. A library with no items has an empty item area.
- One item is `6 + 4·L + n·(2·L + 4)` bytes, with `sizeof(ChewingKey) == 2`.
- `get_range` answers `1 .. range_end`, where `range_end` is the offset
  array length with trailing zero slots removed. An empty library answers `1..1`.
- `PHRASE_MASK` is `0x00FFFFFF`; the library index is the token's bits 24–27 (§3.3).

On the pinned model the four system libraries hold 138,096 items.
的 (`0x010005DB`) carries two readings, `de 2213855` and `di 11000`.
`crates/oxpinyin-data/tests/package_libraries.rs` and
`crates/oxpinyin-runtime/tests/libpinyin_data_dir.rs` assert these
values against the pinned data package (`just fetch-contract-data`).

A reading of the first 28 bytes as a "header" maps onto this layout as
follows (the retired W3 content-fixture tooling used that reading):

| Offset | Field |
|---|---|
| 0 | `length` |
| 4 | `checksum` |
| 8 | `total_freq` |
| 12 | `index_one` (17) |
| 16 | `index_two` |
| 20 | `index_three` (equals `length`) |
| 24 | the `'#'` separator (0x23) and the zero low bytes of slot 0, so the `u32` reads 35 |

---

## 3. Table configuration (table.conf)

A plain-text configuration file. It gives the format versions, λ, the
source format, the database format and the library rows. The pinned model's file:

```
binary format version:7
model data version:14
lambda parameter:0.312699

source table format:pinyin
database format:Tkrzw
```

The `database format:` token is the backend's: `BerkeleyDB`,
`KyotoCabinet` or `Tkrzw`. `crates/oxpinyin-data/src/table_info.rs`
scans the file with the pin's own `fscanf` sequence, and
`table_conf.rs` holds λ.

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

The three file columns are the model source table, the system chunk and
the user file. A `SYSTEM_FILE` library's user file (`.dbin`) is its diff
log in the user directory. A `USER_FILE` library lives in the user
directory only.

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
from bits 24–27 of a `phrase_token_t`. Two separate index spaces exist:

-   **Default space**: indices 0–7, loaded by `pinyin_load_phrase_library`.
-   **Addon space**: indices 4–15, loaded by `pinyin_load_addon_phrase_library`.

Index 4 is overloaded: it maps to `MERGED_DICTIONARY` (merged.bin) in
the default space and `art` in the addon space. The facade that holds
the token decides its meaning.

oxpinyin keeps the two spaces apart as upstream does: `SystemDictionary`
is the default facade and `AddonDictionary` the addon facade
(`crates/oxpinyin-data/src/dict.rs`). `system_files.rs` lists both
library sets.
