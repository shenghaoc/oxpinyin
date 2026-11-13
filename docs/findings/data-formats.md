# Data formats: libpinyin system data files

Status: **reference**

This record describes the files in a libpinyin system data directory
(`$PREFIX/lib/libpinyin/data/`) and how oxpinyin reads them.
oxpinyin reads these files directly, with no conversion or import step.
An unmodified libpinyin data directory for the same backend opens as is
([compatibility policy, "Backends and user data"](compatibility-policy.md#backends-and-user-data)).

Pin lines are at libpinyin `074a2219` (`tools/oracle/oracle-pin.txt`);
the tkrzw backend file is
cited where the three backends share the logic. The readers in
`crates/oxpinyin-data` cite the exact upstream lines. §4-§7 give the
DBM files: the pinyin index, the phrase index, the bigram and the
punctuation table.

## 1. File inventory

`data/Makefile.am` runs three programs over the model's text tables:
`gen_binary_files --gen-punct-table`, `import_interpolation` and
`gen_unigram`. `oxpinyin-utils` provides the same three programs under
the same names. `gen_binary_files` writes the default set (four system
libraries) and the addon set (twelve addon libraries).

| File | Container | Content | oxpinyin reader |
|---|---|---|---|
| `pinyin_index.bin`, `addon_pinyin_index.bin` | backend tree DBM (§4) | key: packed `ChewingKey[L]`, two key spaces and empty-value prefix markers; value: `PinyinIndexItem2<L>[]` | `chewing_table.rs` (`ChewingLargeTable2`) |
| `phrase_index.bin`, `addon_phrase_index.bin` | backend tree DBM (§5) | key: UCS-4 phrase text; value: `u32 token[]` | `phrase_table.rs` (`PhraseLargeTable3`) |
| `gb_char.bin`, `gbk_char.bin`, `opengram.bin`, `merged.bin`; addon `art.bin` … `technology.bin` | `MemoryChunk` file (§2) | the phrase index: token → item with text, unigram and pronunciations | `phrase_library.rs` (`SubPhraseIndex`), mmap |
| `bigram.db` | backend hash DBM (§6) | key: previous token; value: `total` + `{next, count}[]` | `bigram_table.rs` (`Bigram`) |
| `punct.bin` | backend tree DBM (§7) | key: token; value: zero-terminated UCS-4 runs | `punct.rs` (`PunctTable`) |
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
`crates/oxpinyin-runtime/tests/libpinyin_data_dir.rs` hold these
values on the pinned data package (`just fetch-contract-data`).

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

---

## 4. Pinyin index (`pinyin_index.bin`, `addon_pinyin_index.bin`)

`gen_binary_files` writes it through
`ChewingLargeTable2::attach(pinyin_index.bin, READWRITE|CREATE)`.
`attach` opens the container handle and reads no record; each lookup is
one `Get`.

### 4.1 ChewingKey: the 16-bit packed syllable

`src/storage/chewing_key.h:41-47`:

```c
struct _ChewingKey {
    guint16 m_initial : 5;   // bits 0..4
    guint16 m_middle  : 2;   // bits 5..6
    guint16 m_final   : 5;   // bits 7..11
    guint16 m_tone    : 3;   // bits 12..14
    guint16 m_zero_padding : 1; // bit 15, always 0
};
```

`sizeof(ChewingKey) == 2`. Packing:
`(initial & 0x1f) | ((middle & 0x3) << 5) | ((final & 0x1f) << 7) | ((tone & 0x7) << 12)`.
The packing uses the compiler's bitfield layout, and the packed `u16` is
stored host-endian. `NUM_INITIALS = 24`, `NUM_MIDDLES = 4`,
`NUM_FINALS = 18` (`chewing_enum.h`). oxpinyin:
`crates/oxpinyin-core/src/chewing_key.rs` (`ChewingKey::to_packed`,
`ChewingKey::from_packed`).

### 4.2 Two key spaces in one DBM

`compute_chewing_index` and `compute_incomplete_chewing_index`
(`pinyin_phrase3.h:160-178`) define two key spaces in one file:

1. **Complete index**: the packed `ChewingKey[L]` with every `m_tone`
   zeroed, 2 bytes per syllable. The value carries all tone variants.
2. **Incomplete index**: the packed `ChewingKey[L]` with only `m_initial`
   set. It serves initial-only abbreviation lookup.

The spaces never collide: an incomplete key has `m_middle == 0 &&
m_final == 0`, and a complete key keeps its middle and final.
`search_internal` (`chewing_large_table2_tkrzwdb.cpp:133-162`) uses the
`2*L` raw bytes of the index array as the DBM key.

### 4.3 Value: `PinyinIndexItem2<L>[]`

`pinyin_phrase3.h:181-196`:

```c
template <int L>
struct PinyinIndexItem2 {
    phrase_token_t m_token;     // u32
    ChewingKey     m_keys[L];  // L × 2 bytes
};
```

The value is a contiguous array of these items. `sizeof` includes tail
padding to the `u32` alignment of `m_token`:

| L | Field sum | sizeof | Padding |
|---|-----------|--------|---------|
| 1 | 6         | **8**  | 2 bytes |
| 2 | 8         | 8      | none    |
| 3 | 10        | **12** | 2 bytes |
| 4 | 12        | 12     | none    |
| 5 | 14        | **16** | 2 bytes |
| 16| 36        | 36     | none    |

`stride = (4 + 2*L + 3) & ~3`. **A reader strides by `sizeof`, not by the
field sum.** `row_format::pinyin_index::item2_stride` holds the formula.

Item layout within the stride: bytes 0..4 are `m_token`, bytes 4..4+2L
are `m_keys[0..L]`, and bytes 4+2L..stride are padding. No reader gives
the padding a meaning. The pin's writer copies a stack object whose
constructor never sets the padding (`pinyin_phrase3.h:191-195`,
`chewing_large_table2.h:208-232`), so the pin's padding bytes are
indeterminate; oxpinyin's writer zero-fills them
([padding record](upstream-divergences.md#the-pinyin-index-dbms-carry-uninitialized-struct-padding)).

The stored `m_keys` keep their tones. The lookup key zeroes them, and the
value keeps them for tone-filtered matching: `ChewingTableEntry::search`
(`chewing_large_table2.h:136-153`) takes the `equal_range` of the items
under `phrase_less_than_with_tones`, in which a zero query tone matches
any stored tone.

The order of items in a value is the insertion order of `gen_binary_files`,
which processes the libraries in sequence (gb_char, gbk_char, opengram,
merged, then the addons). One token can appear more than once with
different tones; the search returns every matching item.

### 4.4 `SEARCH_CONTINUED` prefix markers

For a new key of length L, `add_index_internal`
(`chewing_large_table2_tkrzwdb.cpp:261-313`) also writes an
**empty-value entry** for every shorter prefix (lengths L-1 down to 1)
until it finds one that exists. `search_internal` (`:133-162`) answers:

- key not found: `SEARCH_NONE`;
- key found, empty value: `SEARCH_CONTINUED` (a prefix marker);
- key found, non-empty value: `SEARCH_CONTINUED`, plus `SEARCH_OK` when an
  item matches.

### 4.5 Backend opening and key order

- Kyoto Cabinet: `ChewingLargeTable2` constructs a `TreeDB` (not `PolyDB`)
  with no comparator, so the default `LEXICALCOMP` (byte order) applies.
- tkrzw: `tkrzw_dbm_open` with no comparator, so `LexicalKeyComparator`
  (unsigned byte order) applies.
- Berkeley DB: `DB_BTREE` with no `set_bt_compare`, so byte-wise `memcmp`
  applies.

All three agree with the store's key-ordering contract
([`store-key-ordering.md`](store-key-ordering.md)).

### 4.6 oxpinyin

- `crates/oxpinyin-data/src/row_format.rs` (`pinyin_index`): the key
  encoders, the item decoder and the stride, shared with the
  `gen_binary_files` writer in `oxpinyin-utils`.
- `crates/oxpinyin-data/src/chewing_table.rs`: `ChewingTable`, the
  counterpart of `ChewingLargeTable2`, with the search semantics and the
  prefix probe.
- `crates/oxpinyin-data/src/dict.rs`: `SystemDictionary` and
  `AddonDictionary` implement the `Dictionary` trait over `ChewingTable`.
- `crates/oxpinyin-store/src/lib.rs`: `RawReadStore`, unframed key access
  to libpinyin's DBM files.

---

## 5. Phrase index (`phrase_index.bin`, `addon_phrase_index.bin`)

The same container family as the pinyin index (§4).
`gen_binary_files` writes it through
`PhraseLargeTable3::attach(phrase_index.bin, READWRITE|CREATE)`; opening is
lazy in the same way.

- **Key**: the phrase text as raw `ucs4_t[]` (`g_utf8_to_ucs4`), one
  `guint32` per character, host-endian (`PhraseLargeTable3::search`,
  `phrase_large_table3_tkrzwdb.cpp:124-153`). 你 (U+4F60) is
  `60 4F 00 00`; 你好 is `60 4F 00 00 7D 59 00 00`.
- **Value**: a flat array of `phrase_token_t` (`u32`), with no padding and
  no header (`PhraseTableEntry`, `phrase_large_table3.cpp:29-102`). One
  text has several tokens when it is in several libraries (的 is in
  `gb_char` and `gbk_char`).
- **Prefix markers**: `PhraseLargeTable3::add_index`
  (`phrase_large_table3_tkrzwdb.cpp:193-242`) writes an empty-value entry
  for every shorter prefix of a stored key, as the pinyin index does.

This is the `PhraseLargeTable3` format. The older `PhraseLargeTable2`
value, `{u32 token, ucs4_t phrase[L]}`, repeats the text. oxpinyin
implements `PhraseLargeTable3` only.

Results: key not found, or an empty value, gives no token. A non-empty
value gives its tokens. oxpinyin's decoder returns `DictError::Parse` for
a value whose length is not a multiple of 4
(`row_format::phrase_index::decode_tokens`).

### 5.1 `search_suggestion`: the Jump/Next walk

`PhraseLargeTable3::search_suggestion`
(`phrase_large_table3_tkrzwdb.cpp:155-191`) requires the prefix key to
exist, creates a DBM iterator, `Jump`s to the prefix's UCS-4 key, steps
past it with `Next`, and then decodes each entry while its key continues
the prefix. The walk order is the DBM's key order. oxpinyin walks the same
DBM with the same cursor (`PhraseTable::search_suggestion`, a ranged walk
of the store), so the predicted order is the pin's on the pin's data
([`compatibility-policy.md`](compatibility-policy.md), row 12).

### 5.2 oxpinyin

- `crates/oxpinyin-data/src/row_format.rs` (`phrase_index`): the UCS-4 key
  encoder and the token decoder, shared with the `gen_binary_files` writer.
- `crates/oxpinyin-data/src/phrase_table.rs`: `PhraseTable`, the
  counterpart of `PhraseLargeTable3`, with exact search, the prefix probe
  and `search_suggestion`. It uses the `ChewingDbm` trait and the
  `RawChewingDbm` wrapper from `chewing_table.rs`.
- `crates/oxpinyin-data/src/dict.rs`: `SystemDictionary` answers
  `tokens_for_text` and the suggestion tokens through `PhraseTable`.

---

## 6. Bigram (`bigram.db`)

`bigram.db` is the one **hash** container: Kyoto Cabinet `HashDB`
(`ngram_kyotodb.cpp:117`), tkrzw `HashDBM` (`ngram_tkrzwdb.cpp:96`) or
Berkeley DB `DB_HASH` (`ngram_bdb.cpp`). The pinyin index, the phrase
index and `punct.bin` are tree containers.

- **Key**: the previous `phrase_token_t` as its 4 raw bytes
  (`const char * kbuf = (char *) &index;`, `ngram_kyotodb.cpp:130`).
- **Value**: `total: u32` (the sum of the counts), then
  `{next_token: u32, count: u32}[]` items. This is `SingleGram`'s layout
  (`crates/oxpinyin-data/src/single_gram.rs`).

`RawReadStore::open_hash_read_only` opens the hash container class on each
backend. `BigramTable` reads lazily: each `load_successors(prev_token)` is
one point read, except `sentence_start`, whose value is read once and kept.
The bigram term reaches candidate frequency only under `DYNAMIC_ADJUST`
([`dynamic-adjust.md`](dynamic-adjust.md)).

---

## 7. Punctuation (`punct.bin`)

A tree container, like the pinyin and phrase indexes. The key is the
`phrase_token_t` as 4 bytes, as for the bigram.

The value is a raw UCS-4 stream: each punctuation is its code points and a
`u32` zero terminator, and successive punctuations are concatenated
(`PunctTableEntry::escape`, `punct_table.cpp:41-55`). For example, `，`
then `、` is `[0xFF0C, 0][0x3001, 0]`. The pin reads it back with
`get_all_punctuations` and `unescape` (`punct_table.cpp:57-94`): `unescape`
takes the first word, then words up to the next zero, and the outer loop
compares typed pointers with the chunk's byte end. On a value of whole
words this gives the punctuations in order. oxpinyin walks whole `u32`
words only (`row_format::punct::decode_puncts`); the difference on 1-3
trailing bytes is the [whole-word record](upstream-divergences.md#punctuation-values-contain-whole-words).

`PunctTable` reads `punct.bin` with one point read per lookup and walks
nothing at open. `pinyin_init` ignores a failed `PunctTable::attach`, and
oxpinyin keeps the failed object as the pin does.

oxpinyin (all under `crates/`):

- `oxpinyin-data/src/row_format.rs` (`bigram`, `punct`,
  `encode_token_key`): the key and value layouts, shared with the
  `oxpinyin-utils` writers.
- `oxpinyin-data/src/bigram_table.rs`: `BigramTable`, the lazy bigram
  reader; `oxpinyin-data/src/lm/mod.rs` builds `BigramLanguageModel` on it.
- `oxpinyin-data/src/punct.rs`: `PunctTable`, the lazy punctuation reader.
- `oxpinyin-store`: `RawReadStore::open_hash_read_only` and
  `WriteStore::create_hash` on each backend (Kyoto Cabinet `DbType::Hash`,
  tkrzw `HashDBM`, Berkeley DB `DB_HASH`).
