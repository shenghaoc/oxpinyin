# libpinyin bigram and punctuation formats — P4 source-level findings

Date: 2026-09-01 · Status: **verified against pinned source and real bytes**

## 1. Bigram (`bigram.db`)

### Container

**KC HashDB** / **Tkrzw HashDBM** — NOT TreeDB/TreeDBM. This is the
critical distinction from the other DBM files:

| File | Container |
|---|---|
| `pinyin_index.bin` | KC TreeDB / Tkrzw TreeDBM |
| `phrase_index.bin` | KC TreeDB / Tkrzw TreeDBM |
| `punct.bin` | KC TreeDB / Tkrzw TreeDBM |
| **`bigram.db`** | **KC HashDB / Tkrzw HashDBM** |

Source: `ngram_kyotodb.cpp:115` (`new HashDB`), `ngram_tkrzwdb.cpp:96`
(`new HashDBM`).

Opening is lazy: `attach` opens the container handle without scanning.

### Key encoding

`ngram_kyotodb.cpp:128`: `const char * kbuf = (char *) &index;` with
`sizeof(phrase_token_t)`.

Key = previous `phrase_token_t` as 4 bytes, native-endian (LE on all
supported targets). Same encoding as the bigram's current oxpinyin
format — byte-identical.

### Value encoding

`total:u32` followed by `{next_token:u32, count:u32}[]` records.

- Bytes 0..4: total (u32 LE) = sum of all counts
- Bytes 4..4+8n: records, each `{next_token: u32 LE, count: u32 LE}`

This schema is byte-identical to what `BigramLanguageModel::open`
already parses — the value format is already frozen in oxpinyin.

### P4 change: HashDB open path

The store's `RawReadStore::open_read_only` opens TreeDB/TreeDBM. For
`bigram.db`, the new `RawReadStore::open_hash_read_only` selects the
hash container class:

- KC: `#type=kch` (HashDB)
- Tkrzw: `dbm=hash` (HashDBM)
- redb/LMDB: delegates to default (no hash/tree distinction)

### P4 change: lazy access

`BigramLanguageModel::open` eagerly slurps every row into a sorted Vec
at init time. `BigramTable` replaces this with lazy per-key `get_raw`
lookups — each `load_successors(prev_token)` is a single DBM point
read.

### DYNAMIC_ADJUST compatibility

`DYNAMIC_ADJUST` controls whether bigram counts are folded into the
candidate frequency:

```text
freq = λ · P_bigram(w_n | w_n-1) + (1 − λ) · P_unigram(w_n)
```

When `DYNAMIC_ADJUST` is clear, `bigram_poss` is `0.0`, and the term
drops out by IEEE-754 construction. The lazy access pattern does not
change this: the bigram lookup happens at candidate-generation time
(per-keystroke), not at init time. P4 preserves this conditional,
per-keystroke access pattern.

## 2. Punctuation (`punct.bin`)

### Container

KC TreeDB / Tkrzw TreeDBM — same container class as the pinyin and
phrase indexes.

### Key encoding

`phrase_token_t` as 4 bytes LE — same as the bigram key.

### Value encoding

A raw UCS-4 stream (`PunctTableEntry::escape`, `punct_table.cpp:40-54`):
each punctuation is its UCS-4 codepoints followed by a u32 zero
terminator, successive punctuations concatenated. Example: `，` then
`、` stores `[0xFF0C, 0][0x3001, 0]` as little-endian u32s. Read back
by scanning u32s to each zero (`unescape` / `get_all_punctuations`,
`punct_table.cpp:56-94`).

This differs from the eager `PunctTable`'s NUL-separated UTF-8 redb
schema — the redb-native format is oxpinyin's own, while this reader
consumes libpinyin's physical layout directly.

### P4 change: lazy access

`PunctTable::open` eagerly walks every row into a `BTreeMap`.
`LazyPunctTable` replaces this with per-key `get_raw` lookups.

## 3. Rust implementation

- `bigram_table.rs`: `BigramTable` — lazy bigram reader, reuses
  `ChewingDbm` trait from P2, same value schema as `BigramLanguageModel`
- `lazy_punct.rs`: `LazyPunctTable` — lazy punct reader, same value
  schema as `PunctTable`
- Store extension: `RawReadStore::open_hash_read_only` for KC HashDB /
  Tkrzw HashDBM
- KC: `DbType::Hash` variant with `#type=kch` tuning
- Tkrzw: `open_hash` with `dbm=hash` parameter
