# libpinyin bigram and punctuation formats

Date: 2026-09-01 · Status: **verified against pinned source and real bytes**

The formats of libpinyin's `bigram.db` and `punct.bin`, which oxpinyin
reads directly. The directory overview is
[`data-formats.md`](data-formats.md).

## 1. Bigram (`bigram.db`)

### Container

**KC HashDB** / **Tkrzw HashDBM** / **Berkeley DB `DB_HASH`** — not a
tree container. This is the critical distinction from the other DBM files:

| File | Container |
|---|---|
| `pinyin_index.bin` | KC TreeDB / Tkrzw TreeDBM / BDB `DB_BTREE` |
| `phrase_index.bin` | KC TreeDB / Tkrzw TreeDBM / BDB `DB_BTREE` |
| `punct.bin` | KC TreeDB / Tkrzw TreeDBM / BDB `DB_BTREE` |
| **`bigram.db`** | **KC HashDB / Tkrzw HashDBM / BDB `DB_HASH`** |

Source: `ngram_kyotodb.cpp:115` (`new HashDB`), `ngram_tkrzwdb.cpp:96`
(`new HashDBM`), `ngram_bdb.cpp` (`DB_HASH`).

Opening is lazy: `attach` opens the container handle without scanning.

### Key encoding

`ngram_kyotodb.cpp:128`: `const char * kbuf = (char *) &index;` with
`sizeof(phrase_token_t)`.

Key = previous `phrase_token_t` as 4 bytes, native-endian (LE on all
supported targets).

### Value encoding

`total:u32` followed by `{next_token:u32, count:u32}[]` records.

- Bytes 0..4: total (u32 LE) = sum of all counts
- Bytes 4..4+8n: records, each `{next_token: u32 LE, count: u32 LE}`

This is `SingleGram`'s layout (`crates/oxpinyin-data/src/single_gram.rs`).

### Hash open path

The store's `RawReadStore::open_read_only` opens the tree container.
For `bigram.db`, `RawReadStore::open_hash_read_only` selects the hash
container class on each backend: Kyoto Cabinet `HashDB`, tkrzw
`HashDBM`, Berkeley DB `DB_HASH`.

### Lazy access

`BigramTable` reads the bigram lazily. Each
`load_successors(prev_token)` is a single DBM point read. The one
exception is `sentence_start`, whose value is read once and kept.

### DYNAMIC_ADJUST compatibility

`DYNAMIC_ADJUST` controls whether bigram counts are folded into the
candidate frequency:

```text
freq = λ · P_bigram(w_n | w_n-1) + (1 − λ) · P_unigram(w_n)
```

When `DYNAMIC_ADJUST` is clear, `bigram_poss` is `0.0`, and the term
drops out by IEEE-754 construction. The bigram lookup happens at
candidate-generation time (per keystroke), not at init time
([`dynamic-adjust.md`](dynamic-adjust.md)).

## 2. Punctuation (`punct.bin`)

### Container

KC TreeDB / Tkrzw TreeDBM / Berkeley DB `DB_BTREE` — the same
container class as the pinyin and phrase indexes.

### Key encoding

`phrase_token_t` as 4 bytes LE — same as the bigram key.

### Value encoding

A raw UCS-4 stream (`PunctTableEntry::escape`, `punct_table.cpp:40-54`):
each punctuation is its UCS-4 codepoints followed by a u32 zero
terminator, successive punctuations concatenated. Example: `，` then
`、` stores `[0xFF0C, 0][0x3001, 0]` as little-endian u32s. Read back
by scanning u32s to each zero (`unescape` / `get_all_punctuations`,
`punct_table.cpp:56-94`).

### Lazy access

`PunctTable` reads `punct.bin` with one point read per lookup. Nothing
is walked at open. `pinyin_init` ignores a failed
`PunctTable::attach`, and oxpinyin keeps the failed object as the pin does.

## 3. Rust implementation

All paths are under `crates/`.

- `oxpinyin-data/src/row_format.rs` (`bigram`, `punct`,
  `encode_token_key`): the key and value layouts, shared with the
  `oxpinyin-utils` writers.
- `oxpinyin-data/src/bigram_table.rs`: `BigramTable`, the lazy bigram
  reader. `oxpinyin-data/src/lm/mod.rs` builds `BigramLanguageModel` on it.
- `oxpinyin-data/src/punct.rs`: `PunctTable`, the lazy punctuation reader.
- `oxpinyin-store`: `RawReadStore::open_hash_read_only` and
  `WriteStore::create_hash` on each backend (Kyoto Cabinet
  `DbType::Hash`, tkrzw `HashDBM`, Berkeley DB `DB_HASH`).
