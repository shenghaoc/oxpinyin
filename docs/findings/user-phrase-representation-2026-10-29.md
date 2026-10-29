# User-phrase representation — the pin's `SubPhraseIndex`, oxpinyin's row tables, and a phrase-centric design (2026-10-29 UTC)

Status: design (read-only; no code change). Pin: libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99`, read from a depth-1 clone of
`libpinyin/libpinyin` fetched at that commit for this record (`git rev-parse
HEAD` = `074a2219c90feaf962d0d24f034514033ece5f99`); every `path:line` below
is that tree. oxpinyin: `main` at `fcabbbf9`; oxpinyin code is cited by file
and item, never by line. Scope: the `USER_FILE` sub-indexes (nibbles 5, 6, 7)
and the init replay that loads them. On-disk formats and the C ABI are out of
scope for change and unchanged by every proposal here. Std only; no new
dependency.

## 0. Summary

1. **No collapse defect was found.** oxpinyin's pronunciation rows are keyed
   `(token, keys)`, so two byte-identical readings of one item cannot coexist
   (§3). The pin cannot create them either: `PhraseItem::add_pronunciation`
   merges an exact key match into the existing entry and appends only a key
   that differs in initial, middle, final or tone (`phrase_index.cpp:56-93`).
   Every API path that adds a reading goes through it. The collapse is
   therefore reachable only from a profile the pin did not write, and the
   design continues. Two seam losses that do exist are recorded in §3.3; the
   proposed representation removes them without a rule change.
2. **The pin's user phrase index is already phrase-centric** (§1): one
   contiguous item per token (header, text, readings), a dense offset array
   per sub-index, `O(1)` zero-allocation reads, append-on-grow and compaction
   at save. oxpinyin decomposes each item into five ordered-map rows and
   rebuilds owned `UserPhrase` values on every read (§2), at `O(log n)` per
   row and roughly four allocations plus one per reading per materialised
   item, and its open path copies every reading three times.
3. **Proposal** (§4): hold each `USER_FILE` library as the pin's own
   structure, owned — an item-body arena in the pin's item encoding with a
   dense slot-to-offset vector that falls back to an ordered map when the
   token space is sparse — read through the existing `PhraseItemView`, and
   keep the user pinyin index as the separate `(token, packed keys)` set it
   already is. Expected: reads `O(1)` with no allocation, the open of a user
   library proportional to its bytes with three allocations per library
   (the file-read buffer the pin also makes, then the arena and the offset
   vector; the buffer is freed after the copy, so two are retained),
   memory equal to the pin's once compacted and at most twice that plus one
   item between compactions (§4.4), and save by compaction to the bytes the
   pin writes. Against `main` the per-item memory falls by
   more than an order of magnitude (§4.4). The existing `empty`, `modest`,
   `phrases` and `journals` profiles exercise this path barely or not at all
   (§4.5): an implementation PR needs a user-library profile in addition.

## 1. The pin's design

### 1.1 `SubPhraseIndex`

`SubPhraseIndex` (`phrase_index.h:257-430`) holds three fields: a
`guint32 m_total_freq`, a `MemoryChunk m_phrase_index` and a
`MemoryChunk m_phrase_content` (`:259-261`), plus the owning chunk pointer
of a loaded file (`:262`). Inside a sub-index a token is addressed by
`token & PHRASE_MASK` (`:247-249`; `PHRASE_MASK = 0x00FFFFFF`,
`novel_types.h:41`).

- **Offset array.** `m_phrase_index` is a flat array of `table_offset_t`
  (`guint32`, `novel_types.h:128`), one per slot, indexed by
  `(token & PHRASE_MASK) * sizeof(table_offset_t)`; `0` means no item
  (`phrase_index.cpp:185-193`).
- **Content arena.** `m_phrase_content` holds item bodies back to back. The
  first item is placed at offset 8 so `0` stays the empty sentinel
  (`:209-211`). `add_phrase_item` appends the body at the current end and
  writes its offset into the slot (`:208-217`); a slot that already held an
  item keeps the old bytes in the arena as garbage until `compact`.
- **Read.** `get_phrase_item` reads the slot's offset, then the two header
  bytes at that offset, computes the body length from them, and points a
  `PhraseItem` at the bytes with `set_chunk(..., NULL)` — a borrowed view,
  no copy, no allocation (`:180-206`; `memory_chunk.h:251-259`).
- **Remove.** `remove_phrase_item` copies the viewed bytes into a new
  `PhraseItem` (one allocation), zeroes the slot and subtracts the item's
  unigram from `m_total_freq` (`:219-235`). The body stays in the arena.
- **Range.** `get_range` scans the offset array backwards past trailing
  zeros; the range is `[1, index of last non-zero slot + 1)`, and an empty
  array answers `[1, 1)` (`:624-646`).
- **Compact.** `compact` walks the range, copies every live item into a
  fresh `SubPhraseIndex` through `add_phrase_item`, and replaces the old one
  (`:648-675`). It runs at `pinyin_save` (`pinyin.cpp:1139`), at
  `pinyin_end_add_phrases` (`:657`) and at `pinyin_mask_out` (`:1294`).
- **Load.** `load` takes ownership of the file chunk, reads
  `m_total_freq` and the three section offsets, checks the separators, and
  sets the two chunks as views into the file buffer: `O(1)` after the file
  read (`:335-363`). The file read itself is one `MemoryChunk::load` into
  one buffer (`pinyin.cpp:305-313`).
- **Store.** `store` writes the total, the three offsets, the offset array
  and the arena with separator bytes (`:365-390`).
- **Totals.** `add_unigram_frequency` edits the unigram in place at
  `offset + 2` and refuses a `guint32` overflow of the total
  (`:150-178`). `FacadePhraseIndex` keeps its own total across sub-indexes
  (`phrase_index.h:440`, `:628-635`, `:663-671`).
- **Mask out.** `mask_out` removes every item in the range whose token
  matches `(mask, value)` through `remove_phrase_item` (`:677-697`).

`MemoryChunk` grows by doubling (`memory_chunk.h:102-134`), so appends are
amortised `O(body)`; a chunk that still views the file buffer copies on its
first growth (`:106-123`).

### 1.2 `PhraseItem`: inline pronunciations

A `PhraseItem` is one byte string (`phrase_index.h:36-49`, `:64-245`):

| Field | Bytes | Read by |
| --- | --- | --- |
| phrase length `L` | `guint8` at 0 | `get_phrase_length` (`:100-103`) |
| reading count `n` | `guint8` at 1 | `get_n_pronunciation` (`:112-115`) |
| unigram frequency | `guint32` at 2 | `get_unigram_frequency` (`:124-126`) |
| text | `L × ucs4_t` at 6 | `get_phrase_string` (`phrase_index.cpp:103-106`) |
| reading *i* | `L × ChewingKey` (2 bytes each) then `guint32` freq | `get_nth_pronunciation` (`:33-44`) |

`phrase_item_header = 6` (`phrase_index.h:56`). A `ChewingKey` is a 16-bit
bit-field: initial 5, middle 2, final 5, tone 3, padding 1
(`chewing_key.h:43-47`).

- `add_pronunciation(keys, delta)` walks the readings in order, summing
  their frequencies; on the first `pinyin_exact_compare2` match (initial,
  middle, final and tone equal on every key; the padding bit is not
  compared, `pinyin_phrase3.h:33-64`) it adds `delta` in place unless the
  running total would overflow, in which case it returns `false` and
  changes nothing; with no match it increments `n` and appends the key run
  and `delta` at the end (`phrase_index.cpp:56-93`). Readings therefore
  keep insertion order and never repeat a key.
- `increase_pronunciation_possibility(keys, delta)` adds `delta` to **every**
  reading that matches under `pinyin_compare_with_tones` (incomplete
  middle/final and zero tone are wildcards on either side,
  `pinyin_phrase3.h:68-147`), never appends (`phrase_index.cpp:114-143`).
  This is the training path (`phonetic_key_matrix.cpp:603-669`).
- `get_pronunciation_possibility` divides the matched frequency sum by the
  total under the same comparison (`phrase_index.h:136-164`).
- `n` is a `guint8` (`set_n_pronunciation`, `phrase_index.cpp:28-31`), so
  the 256th appended reading wraps the count to 0; every later read sizes
  the item from that count.
- Item equality is size then `memcmp` (`phrase_index.h:235-240`).

### 1.3 Enumeration and export

- `pinyin_begin_get_phrases` probes from `range.m_range_begin` to the first
  token whose item exists and has at least one reading
  (`pinyin.cpp:662-688`).
- `pinyin_iterator_get_next_phrase` re-reads the item under the cursor
  token (`get_phrase_item`, `:706-709`), renders one reading
  (`get_nth_pronunciation`, then `get_pinyin_string` per key and a
  `g_strjoinv("'")`, `:722-734`), and advances to the next reading or
  re-probes the range for the next token (`:741-768`). Nothing is buffered;
  each call is `O(1)` plus the render, with the render's allocations only.
- The token getters read one item each: text (`_token_get_phrase`,
  `:1627-1646`), reading count (`:2786-2799`), nth reading (`:2801-2819`),
  unigram (`:2821-2834`); `pinyin_token_add_unigram_frequency` edits in
  place (`:2836-2843`).

### 1.4 Load and save of user phrases

- **Init.** For each default table, `_load_phrase_library` loads a
  `USER_FILE` library by reading its file into one chunk and calling
  `FacadePhraseIndex::load`, or creates an empty sub-index when the file is
  absent (`pinyin.cpp:303-321`, `:376-392`). The user pinyin index
  (`user_pinyin_index.bin`) and user phrase table (`user_phrase_index.bin`)
  are opened as DBMs before that (`:350-371`).
- **Add.** `_add_phrase` searches the phrase table for the text
  (`:536-542`). An existing token in the same sub-index is removed, gains
  the reading through `add_pronunciation`, and is appended back
  (`:561-583`); **no** pinyin-index entry is added for that reading. A new
  phrase takes `range.m_range_end` (bumped past a zero low part), is added
  to the user phrase table and the user pinyin index, gets the item with its
  one reading, and `count × 3` unigram (`:584-609`). `pinyin_end_add_phrases`
  compacts (`:655-660`).
- **Remember.** `_remember_phrase_recur` walks the matrix and reaches
  `_add_phrase` with the keys it collected (`:3578-3667`); incomplete keys
  are not rejected (the guard is `#if 0`, `:3634-3641`).
- **Remove.** `pinyin_remove_user_candidate` removes the item, its phrase
  table entry and one pinyin-index entry per stored reading, asserting each
  removal succeeds (`:3726-3760`).
- **User pinyin index.** `ChewingLargeTable2::add_index` writes two index
  entries per reading, the incomplete projection and the complete one
  (`chewing_large_table2.cpp:180-198`), each inserted into a chunk kept in
  `phrase_exact_less_than2` then token order (`chewing_large_table2.h:208-231`).
  Index entries are independent of items: a reading merged into an existing
  phrase is never indexed, and a slot that is later reused by an import is
  found under entries the old item left.
- **Save.** `pinyin_save` compacts, then `_write_files` stores each
  `USER_FILE` sub-index into a chunk and writes `<file>.tmp`
  (`:978-993`), stores the two user DBMs and the bigram (`:996-1020`), and
  `_rename_files` renames (`:1132-1147`).

### 1.5 Complexity of the pin's operations

`P` items in the sub-index, `S` slots in the offset array (`S ≥ P`; equal
until removals), `k` readings of an item, `L` its length, `I` entries in the
user pinyin index.

| Operation | Time | Allocations |
| --- | --- | --- |
| `get_phrase_item` | `O(1)` | 0 |
| text / unigram / nth reading getters | `O(1)`, `O(L)` copy for text | 0 (C ABI copies) |
| `add_unigram_frequency` | `O(1)` | 0 |
| `add_pronunciation` | `O(k L)` | 0 or one chunk growth |
| `increase_pronunciation_possibility` | `O(k L)` | 0 |
| `remove_phrase_item` | `O(body)` copy | 1 |
| `add_phrase_item` | amortised `O(body)` | amortised 0 |
| `get_range` | `O(trailing zero slots)` | 0 |
| export walk | `O(S)` probes, `O(1)` per row + render | render only |
| `compact` | `O(S + Σ body)` | 1 new sub-index |
| user library load | `O(file bytes)` read, `O(1)` index | 1 (the file buffer) |
| user library store | `O(S + Σ body)` | chunk growth |
| `add_index` (user pinyin index) | `O(log I)` search + `O(I)` memmove | chunk growth |
| memory per item | `6 + 4L + k(2L + 4)` bytes + 4 per slot | — |

## 2. oxpinyin's design

### 2.1 The row model

A libpinyin session keeps its values in `SessionValues::Memory`
(`crates/oxpinyin-user/src/session_values.rs`), a
`BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>`: every table is an ordered map
of owned byte keys to owned byte values, and a transaction stages changes in
a second map that commit merges into the first. The tables that carry a user
phrase (`crates/oxpinyin-user/src/store.rs`, constants `PHRASE`,
`PHRASE_BY_TEXT`, `PHRASE_BY_LIB_TEXT`, `PRONUNCIATION`, `UNIGRAM`,
`ALLOC`):

| Table | Key | Value |
| --- | --- | --- |
| `PHRASE` | token, 4 bytes big-endian | text as UCS-4 bytes |
| `PHRASE_BY_LIB_TEXT` | nibble + text bytes | token |
| `PHRASE_BY_TEXT` (nibble 7 only) | text bytes | token |
| `PRONUNCIATION` | token + key tail (`PinyinKey` words, little-endian) | `PronValue`: count `u64`, `seq` `u32`, `indexed` `u8` — 13 bytes |
| `UNIGRAM` | token | `u64` |
| `ALLOC` | nibble or cursor id | next token |

The key tail is not the pin's packed `ChewingKey`: `PinyinKey`
(`crates/oxpinyin-user/src/phrase.rs`) is a dense `SyllableKey` index plus a
tone in the top three bits, converted at the seam by
`packed_to_pinyin_keys` and `pinyin_keys_to_packed`
(`crates/oxpinyin-user/src/store_libpinyin.rs`) through the key's spelling.
Insertion order, which the pin gets from position, is carried by `seq` in
the value and restored by a sort on every read (`sort_by_insertion`). The
`indexed` flag, which the pin keeps in a separate DBM, is also carried in the
value, and the index entries no item backs are held apart in
`Target::kept_index`.

The reader types are owned: `UserPhrase { token, text: Vec<u32>,
pronunciations: Vec<UserPronunciation> }` and
`UserPronunciation { keys: Vec<PinyinKey>, count, indexed }`.

### 2.2 Operations

Costs are for a session store; `P` user items across the three nibbles,
`R` pronunciation rows in the single `PRONUNCIATION` table, `U` rows in
`UNIGRAM` (user items plus every system token with a training delta), `k`
and `L` as in §1.5. "alloc" counts heap allocations beyond the pin's; the
C-boundary copies both sides make are not counted.

- **`UserStore::phrase(token)`** — point read of `PHRASE` (`O(log P)`, the
  value copied out), `decode_words` (a second `Vec<u32>`), then a
  `range` over the token's `PRONUNCIATION` rows (`O(log R + k)`), each row
  decoded into a fresh `Vec<PinyinKey>`, collected and sorted by `seq`
  (`collect_pronunciations_from_store`). Time `O(log P + log R + k log k)`;
  allocations `k + 4`. The pin: `O(1)`, 0.
- **`UserStore::phrases()`** — walks `PHRASE` (two allocations per item:
  `decode_words` then `.to_owned()`), builds a `HashSet` of tokens, then
  `collect_pronunciations_for_tokens`, then clones every token's reading
  vector into the result. Time `O(P + R)` plus `Σ k log k`; allocations
  about `4P + 2R`. The pin's enumeration is `O(S)` with none.
- **`collect_pronunciations_for_tokens`** — one `for_each` over the whole
  `PRONUNCIATION` table regardless of how many tokens are asked for, a
  `HashSet` probe per row, a `Vec<PinyinKey>` per row, a `BTreeMap<Token,
  Vec>` of groups, a sort per group and a second map built from the first.
  Time `O(R + Σ k log k)`; allocations `R + 2P`.
- **`UserStore::export_phrases_in(library)`** — `phrases()` minus the final
  clone, filtered by nibble after the walk, plus per row `render_pinyin`
  (one `String` per key and the join) and a clone of the text. The pin's
  export walk renders the same strings and allocates nothing else.
- **`PhraseExportCursor::probe_user`**
  (`crates/oxpinyin-facade/src/export_rows.rs`) — for each token from the
  probe start to the library's token end it calls `with_user_item`, which
  materialises the whole `UserPhrase` through `UserStore::phrase` to ask
  whether it has a first reading; the item is memoised per token and store
  generation, so `get_next_phrase` reuses it. A walk costs
  `Σ (k_i + 4)` allocations and `O(S (log P + log R))` time against the
  pin's `O(S)` with none. Absent tokens cost one failed `PHRASE` read each.
- **`add_phrase_in`** — the existing-item branch re-reads the token's rows
  (`collect_pronunciations_from_txn`, `k` allocations; a libpinyin session
  memoises the last imported item in `ImportReadings`) and writes one row.
  The new-item branch writes five rows plus the phrase-table record. Each
  `put` copies key and value into the transaction map and again into the
  table on commit. The pin's new-item path is a handful of `set_content`
  calls and two DBM inserts.
- **`remove_user_phrase`** — walks the whole `UNIGRAM` table to recompute
  the total (`O(U)`), where the pin subtracts one field (`O(1)`).
- **`index_phrases` and the lookup rebuild** — `UserLookup::from_store`
  (`crates/oxpinyin-user/src/lookup.rs`) and `user_trellis_index`
  (`crates/oxpinyin-runtime/src/lib.rs`) each call `index_phrases()`, which
  is `phrases()` plus a merge of the kept index entries, and rebuild their
  string-keyed maps from scratch whenever the phrase generation moves:
  two full materialisations and `O(R)` `String` renders per phrase change.
  The pin inserts one entry per new phrase (`O(log I + I)`) and never
  rebuilds.
- **`user_pronunciation_count`** — answered from
  `Target::pronunciation_counts`, a `BTreeMap<Token, u32>` filled at open
  from each item's header (`loaded_pronunciation_counts`) and maintained by
  the write paths, because the row count can differ from the pin's header
  count (a dropped reading, §3.3). The pin reads one byte of the item.

### 2.3 The init replay path

`UserStore::open_libpinyin` → `open_session`
(`crates/oxpinyin-user/src/store_libpinyin.rs`) → `persistence::load_profile`
(`crates/oxpinyin-user/src/persistence.rs`) → `seed_txn_with_replay`. For a
`USER_FILE` library with `P` items, `R` readings:

1. `load_libraries` reads the file (one allocation), validates the chunk
   frame and checksum (`O(bytes)`), and `decode_sub_phrase_index`
   (`crates/oxpinyin-data/src/chunk_write.rs`) decodes every item into an
   owned `ChunkItem { phrase: Vec<u32>, unigram, prons: Vec<(Vec<u16>,
   u32)> }`: `2 + k` allocations per item, then a `BTreeMap<u32, ChunkItem>`
   per library. **First owned copy of every reading.**
2. `load_user_pinyin_index` reads the DBM into `UserState::indexed`, a
   `BTreeSet<(u32, Vec<u16>)>` (one allocation per entry), plus
   `IndexKeys` for the keys that must be written back.
3. `open_session` calls `unbacked_index`, whose `backed_readings` clones
   every reading of every item into a second `BTreeSet<(u32, Vec<u16>)>`
   to subtract from `indexed`. **Second copy.** It then calls
   `loaded_pronunciation_counts` (a map entry per item).
4. `seed_txn_with_replay` first clones the whole `state.libraries` map
   (**third copy**), then per item writes `PHRASE`, `PHRASE_BY_LIB_TEXT` and
   (nibble 7) `PHRASE_BY_TEXT`, per reading converts the packed words to
   `PinyinKey`s (`packed_to_pinyin_keys`: a spelling lookup and a `Vec`),
   encodes the key tail (a `Vec`), builds the row key (a `Vec`) and `put`s
   (two more copies into the change map), then writes `UNIGRAM`; commit
   moves the rows into the table maps. Roughly `8 + 5k` allocations per
   item, each landing in a B-tree node.
5. The system `.dbin` journals are replayed by `replay::Merge`
   (`crates/oxpinyin-user/src/replay.rs`) over an owned copy of the pin's
   own layout — a `Content` arena and slot offsets — and the touched items
   are retained as lazy byte views (`ReplayedLibrary`), decoded to
   `ChunkItem` on first use in `SystemItems`
   (`crates/oxpinyin-user/src/system_items.rs`). ADD records whose slot the
   shipped library does not hold are folded into `libraries` in step 4 and
   take the `USER_FILE` row path.

So a user library costs about `13 + 8k` allocations per item to open, three
owned copies of its readings and five B-tree rows per item, where the pin
costs one `read` into one buffer and two pointer assignments.

### 2.4 Memory per item, `main` against the pin

Derived from the type sizes and the row layout above, not measured: a
two-character item with one reading (`L = 2`, `k = 1`). `Vec<u8>` is a
24-byte header plus a heap block; the glibc minimum block is 32 bytes.
B-tree nodes hold up to eleven key-value pairs of 48 bytes, so an entry
costs between 48 and about 100 bytes of node depending on fill.

| Representation | Bytes per item | Composition |
| --- | --- | --- |
| pin `SubPhraseIndex` | 26 | body `6 + 8 + 8` in the arena, one 4-byte slot |
| `main` session rows | about 800–1,000 | 5 rows = 10 `Vec<u8>` (240 header + 320 heap) + 5 node entries (≈250–500) |
| `main` side structures | about 100–200 more | `indexed` and `backed` set entries, `pronunciation_counts` entry, `ChunkItem` until `loaded` drops |

The ratio is more than an order of magnitude and grows with `k`, since each
reading adds a row (`48 + 13 + 24 + 32`-byte order) where the pin adds
`2L + 4` bytes.

## 3. The possible defect: identical pronunciation keys

### 3.1 What oxpinyin does

A reading is the row `(token, key_bytes)` in `PRONUNCIATION`; `add_pronunciation_to_rows`
compares `row.key_bytes == key_bytes` and either edits that row or inserts
a new key, and `seed_txn_with_replay` writes one row per reading with the
same key, so the second of two byte-identical readings of one item
overwrites the first (its count and `seq` win, the first entry's count is
lost). `index_reading` on `UserPhrase` likewise finds by `reading.keys ==
keys`. Collapse exists in the structure.

### 3.2 Whether the pin keeps separate entries

Every path that adds a reading to an item goes through
`PhraseItem::add_pronunciation` (`phrase_index.cpp:56-93`): `_add_phrase`
for both its branches (`pinyin.cpp:578`, `:602`), hence
`pinyin_iterator_add_phrase` (`:614-653`) and `pinyin_remember_user_input`
(`:3669-3710`) through `_remember_phrase_recur`, and `load_text` for the
generated system tables (`phrase_index.cpp:578-579`). `add_pronunciation`
returns after the first `pinyin_exact_compare2` match, which compares
initial, middle, final and tone of every key (`pinyin_phrase3.h:33-64`), so
a key run equal in those fields is merged, never appended. The training
path (`increase_pronunciation_possibility`, `phrase_index.cpp:114-143`)
appends nothing. The journal merge (`:439-520`) copies whole items. The
wrap at 256 readings (`:88`) makes later reads see zero readings, and the
next `add_pronunciation` on the re-read item appends one key to an item with
none, so repetition cannot accumulate there either.

Two keys that differ only in the padding bit compare equal and are merged by
the pin; oxpinyin's `ChewingKey::from_packed`
(`crates/oxpinyin-chewing/src/chewing_key.rs`) drops the bit, so both sides
agree. `content_table` has 441 rows with 441 distinct pinyin spellings
(`pinyin_parser_table.h:3976` onward, counted for this record), so the
spelling round trip through `SyllableKey` maps distinct table keys to
distinct ids.

**Conclusion.** No call sequence on the pin's C API produces an item with
two identical key runs, so there is no case on which oxpinyin collapses what
the pin keeps separate. The remaining exposure is a `user.bin` the pin did
not write through its API; a profile the pin's own `check_format` accepts
can carry one, and oxpinyin then reports one reading where the pin reports
two. That is a robustness difference on foreign data, recorded here, not a
defect that stops this design.

### 3.3 Seam losses that do exist

Read while deriving §3.2; both vanish under §4 because the item keeps the
pin's packed words.

- `packed_to_pinyin_keys` answers `None`, and `seed_txn_with_replay` then
  **drops the reading**, when a stored key's spelling is outside
  `SyllableKey::from_text` (the option-only and zhuyin-only tiers are only
  in `from_canonical_text`) or its tone is above `MAX_KEY_TONE`. A reading
  that reaches the file can therefore disappear from the item at the next
  open, with `pronunciation_counts` still reporting the header count. Which
  spellings the pin's parsers can actually store in a user item is a
  separate reading task; this record only notes that the seam is lossy
  where the pin's item is not.
- `pinyin_keys_to_packed` rebuilds the packed word from the spelling, so
  the saved bytes are the canonical key for that spelling. The pin writes
  back the bytes it loaded.

## 4. Proposed representation

### 4.1 Types

One owned library per `USER_FILE` nibble, in the pin's own shape, in
`oxpinyin-user`:

```rust
/// A USER_FILE sub-index held in memory: the pin's SubPhraseIndex, owned.
pub(crate) struct UserLibrary {
    /// m_total_freq — the stored total, moved by the same operations as the pin's.
    total: u32,
    /// slot (token & PHRASE_MASK) → offset into `content`; 0 is "no item".
    slots: Slots,
    /// Item bodies in the pin's item encoding (header, UCS-4 text, readings),
    /// starting at offset 8; a replaced body stays as garbage until `compact`.
    content: Vec<u8>,
    /// Bytes of live bodies, for the compaction trigger.
    live: usize,
}

enum Slots {
    /// Direct indexing: the pin's offset array. Chosen while the array is
    /// at most a small multiple of the item count.
    Dense(Vec<u32>),
    /// Ordered fallback for a sparse token space (after removals, mask-out
    /// or a profile with high tokens): slot → offset.
    Sparse(BTreeMap<u32, u32>),
}
```

An item is read as `PhraseItemView<'_>`
(`crates/oxpinyin-data/src/phrase_library.rs`), the view the mapped system
chunks already use: phrase length, reading count, unigram, text words, and
`pronunciation(i)` as packed `ChewingKey` words plus frequency. Text,
frequency and ordered readings are one contiguous record, as the pin's.
`UserPhrase` and `UserPronunciation` stay as the owned C-boundary and
export values, built from a view only where the pin also copies (the token
getters fill a caller `GArray`; the export renders strings).

The user pinyin index stays separate: the `(token, packed keys)` set the
profile load already produces (`UserState::indexed`, `IndexKeys`), owned
by the session, with `indexed()` answered by membership. The kept entries
no item backs need no special set: they are simply members whose token has
no matching reading, which is the pin's situation.

The dense/sparse switch is a space rule only. Proposed threshold: dense
while `slots.len() <= 4 * items + 256`, re-evaluated at compaction; the pin
is always dense, so dense is never worse than the pin and sparse is better
only where the pin wastes slots.

### 4.2 Operations

| Operation | Proposed | Pin (§1.5) | `main` (§2.2) |
| --- | --- | --- | --- |
| read item (`phrase`, getters, probe) | `O(1)` slot + header, 0 alloc; sparse `O(log P)` | `O(1)`, 0 | `O(log P + log R + k log k)`, `k + 4` |
| reading count | header byte | header byte | side map |
| `add_unigram_frequency` | in place | in place | row rewrite |
| exact-reading increment (import on an existing item) | `O(kL)` in place | same | row rewrite after `k`-row re-read |
| append a reading | copy body + new entry to the arena end, `O(body)`, amortised 0 alloc | same | one row |
| remove | zero the slot, `O(1)`; body garbage until compact | same (+1 copy) | 5 row removals + `O(U)` total walk |
| enumeration / export walk | `O(S)`, 0 alloc, render only | same | `O(P + R)` materialisation + render |
| open a user library | one read, frame check, one `memcpy` of the arena and one of the offsets; `O(bytes)`, 3 allocations (buffer, arena, offsets), 2 retained | one read, `O(1)`, 1 allocation (the buffer, retained) | `O(P + R)`, `13 + 8k` allocations per item, three reading copies |
| save | `compact()` then the pin's `store` layout; bytes identical to the pin's output by construction | same | `build_chunk` from `ChunkItem`s re-read from rows |
| lookup index rebuild | one pass over views per generation change, shared by both consumers | incremental insert | two `phrases()` materialisations |
| memory per item | `6 + 4L + k(2L + 4)` + 4 per slot; plus garbage ≤ live between writes and ≤ live + one body inside a write (§4.4) | same, plus unbounded garbage until `compact` | §2.4 |

Allocation counts in this table are every heap allocation the operation
makes, including the file-read buffer the pin and the proposal both fill,
whether or not it outlives the call. The per-item counts in the `main`
column exclude the per-library buffer. The pin's open makes one allocation
and keeps it. The proposal makes three and frees the buffer once the arena
and the offset vector are copied, so two are retained.

Compaction runs where the pin's does (save, import end, mask-out). It also
runs after any mutating write that leaves `garbage > live`: the append of a
reading, the replacement of a body, a removal. Garbage is the arena length
less `live`. The check is two counter reads at the end of each write,
outside the write; a mask-out compacts once at its end, as the pin's does
(`pinyin.cpp:1294`), so its removals do not accumulate. Because the check
follows every write, `garbage ≤ live` holds between writes. Inside one
write, before its check, the arena holds at most `live + one body` of
garbage, where `live` is the live bytes before the write and one body is
the item the write orphans.

A compaction costs `O(S + Σ body)` and fires only after more than `live`
bytes were orphaned, so the check adds amortised `O(1)` per orphaned byte.
These extra compactions are the one place the proposal spends time the pin
does not, in return for a bound on garbage that the pin lacks: the pin's
arena grows until its next `compact` (save, import end or mask-out).
§4.7 reports both axes.

The oxpinyin-specific reverse maps `PHRASE_BY_TEXT` and `PHRASE_BY_LIB_TEXT`
exist only for `token_for_phrase` (one caller, `crates/oxpinyin-capi/src/context.rs`);
the pin answers that question from the user phrase table
(`pinyin.cpp:536-542`), which oxpinyin already seeds verbatim
(`phrase_table::TABLE`). They are retired with the rows. A user item's
unigram lives in the item, as the pin's; the facade total and the library
offset bookkeeping (`UNIGRAM_TOTAL`, `SYSTEM_BASE`, `LIBRARY_OFFSET`) are
unchanged in meaning, and the count memo keeps serving the bigram rows.

### 4.3 What the arena buys over a typed item

A typed alternative — `Box<[u32]>` text, `u32` unigram and a boxed slice of
`{ keys: Box<[u16]>, count: u32 }` readings — is three allocations per
one-reading item and about 120 bytes for the `L = 2, k = 1` case against
the pin's 26. It is simpler to mutate but strictly worse than the pin in
memory and in open time (one decode per item). The arena is the pin's
structure and so cannot be worse on the pin's workloads; it reuses the view
type oxpinyin already maintains for the shipped chunks and the replay
`Content`, and it saves the loaded bytes verbatim, which closes §3.3.

### 4.4 Expected memory

Per two-character, one-reading item: 26 live bytes in the proposal, as in
the pin, and of the order of 900 for `main` (§2.4).

Between compactions the arena also holds garbage. The check of §4.2 runs
after every mutating write, so `garbage ≤ live` between writes and, inside
a write, `garbage ≤ live + one body`, with `live` taken before the write.
One body is at most 8,736 bytes, an item of 15 characters with 255
readings: `6 + 4·15 + 255·(2·15 + 4)`. The arena's length is therefore at most `2·live + 8,736` bytes, which is at most
52 bytes per such item plus that constant.

While a compaction runs it builds the new arena beside the old one, so the
peak for the duration of that call is at most `3·live + 8,736` bytes. The
pin's `compact` also holds old and new together (`phrase_index.cpp:659-672`),
but the pin's old arena has no bound. Vector capacity slack is left out of
these figures on both sides: the pin's `MemoryChunk` also doubles its
capacity on growth (`memory_chunk.h:127`).

For a library of ten thousand such items: about 260 kB live against about
9 MB for `main`, at most about 520 kB between compactions. These are derived
figures; the implementation PR measures them (§4.7).

### 4.5 Expected effect on the named profiles and the export

The four profiles are built by `tools/bisection/teardown.py`
(`modest_rows`, `select_rows`) through the worker's `td_populate`
(`tools/bisection/teardown.h`), which calls `pinyin_begin_add_phrases` for
libraries 1 to 4 only and rejects any other library. They therefore
populate the **system** sub-indexes and leave `user.bin` empty:

| Profile | Content | What the user-phrase path sees at init |
| --- | --- | --- |
| `empty` | no rows | no `USER_FILE` items; the proposal must allocate nothing for an absent library (an empty `UserLibrary` is two empty vectors) |
| `modest` | `测试` / `测侧`, ten times each, libraries 1–4, count 5 | at most a few items: a phrase present in a library is a MODIFY of a system item; one absent from it becomes an ADD record, an appended system item that today takes the `USER_FILE` row path |
| `phrases` | 10,000 distinct exported phrases, count 20,000, in export order | 10,000 MODIFY records: the exported reading is the item's own, so `add_pronunciation` increments in place; no user items |
| `journals` | the same, balanced 3,000/3,000/3,000/1,000 across libraries 1–4 | the same across four journals |

Expectation, stated plainly:

- `empty`: no change in time or memory; the gate is "no regression".
- `modest`: a bounded improvement proportional to the handful of appended
  items; below the measurement's noise unless the ADD count is non-zero.
- `phrases`, `journals`: no change from this proposal. Their init cost is
  the journal replay, which the 2026-10-27 attribution measured at about
  86% of the zhuyin facade's init instructions
  (`init-attribution-1090-2026-10-27.md`, `load_logs_with_mask` and
  `Merge::finish_lazy`). That path already uses
  the arena shape this design proposes for user libraries; unifying the two
  (§4.6 phase 3) is where those profiles could move, and it is not claimed
  here.
- Export: on the four profiles the export walks system libraries with
  overrides and is unchanged. On a profile with `P` user items the walk
  drops from `Σ (k_i + 4)` allocations and `O(S log)` reads to `O(S)`
  zero-allocation probes plus the render both sides pay.

Because the existing profiles do not exercise the user library, the
implementation PR adds one that does (§4.7), built with the same worker by
importing into library 7.

### 4.6 Migration plan

Each phase is a separate PR, green on its own, with the §4.7 measurements
where it changes shipping code.

1. **Library type and open/save.** Add `UserLibrary` and `Slots`; load a
   `USER_FILE` file into it in `load_libraries` (frame check, then the
   offsets and the arena copied once) instead of decoding to `ChunkItem`s;
   save by `compact` plus the pin's `store` layout and assert byte equality
   with `build_chunk` in the existing round-trip tests. `UserState::libraries`
   becomes the libraries themselves. ADD-record items of system libraries
   keep taking this path, now as views.
2. **Readers over views.** `phrase`, `phrases`, `export_phrases_in`,
   `item_pronunciations`, `user_pronunciation_count`, `phrase_text`,
   `user_library_token_end` and the export cursor read the library; the
   `PHRASE`, `PHRASE_BY_TEXT`, `PHRASE_BY_LIB_TEXT` and `PRONUNCIATION` rows
   and `pronunciation_counts`, `kept_index`, `backed_readings` and
   `ImportReadings` go. `indexed()` becomes set membership. The lookup
   rebuild and the trellis index take one shared pass over views.
3. **Writers.** `add_phrase_in`, `promote_addon_phrase`,
   `train_pronunciations`, `remove_user_phrase`, `mask_out` and the library
   load/unload paths edit the arena; the `UNIGRAM` rows of user tokens fold
   into the item. Optionally unify with `replay::Content` so that MODIFY
   items are the same type; measured separately on `phrases` and
   `journals`.
4. **Standalone store.** `UserStore::open` and `create_standalone` (tests
   and benches only, no shipping caller) persist each library as one DBM row
   holding the chunk bytes, so one code path serves both; needs a maintainer
   decision because it changes the plain store's file content.

Phase 1 keeps every observable behaviour: the same bytes are read and
written, and the row model still serves reads until phase 2. The
differentials that gate the lane are the existing import, export and
user-store contract cases on the reference cell (`import-diff`,
`export-diff`, the `save_*` and `user_store_*` crate tests); phases 2 and 3
add nothing to the C surface.

### 4.7 What an implementation PR must show

Measured in one run on one host, base / PR / pin, with the commands in
`docs/runbooks/benches.md`:

1. **Init on the four existing profiles** (`just measure startup`, both
   facades, warm): wall and instructions for `init`, with `empty` and
   `phrases`/`journals` shown as unchanged within the control's interval,
   so a change to the user path is proven not to regress the shipped
   profiles.
2. **Init and export on a user-library profile**: 10,000 rows imported
   into library 7 through the same worker (`td_populate` extended to accept
   `7`, which is the only tooling change the lane needs), one reading per
   phrase and a second variant with eight readings per phrase; report init
   wall, init instructions, peak heap (Massif), the open's allocation count
   (§4.2: three, two retained, against the pin's one) and the export's wall
   and allocation count for base, PR and pin, with the PR at or below the
   pin on every axis.
3. **Steady state**: the keystroke cycle with that profile loaded
   (`just measure steady-comparison`), to show the lookup rebuild and the
   per-token reads did not regress.
4. **Memory**: the session's resident set after init on the user-library
   profile, base / PR / pin, with the per-item figure of §4.4 confirmed or
   corrected. Add a write-heavy session on the same profile, with repeated
   reading appends and removals on the same items and the mid-session
   compactions counted, and report the arena's peak length and the
   session's wall against the `2·live + 8,736` and `3·live + 8,736` bounds
   of §4.4 and the amortised cost of §4.2.
5. **Bytes**: the saved `user.bin` of the PR byte-identical to the pin's on
   the import, train, remove and mask-out sequences the contract cases
   already run, and the stripped shipping-library sizes (Constitution §2).
6. **Crate benches**: `export_phrases` and `phrase_read`
   (`crates/oxpinyin-user/benches`) before and after, as supporting numbers
   only; the profile measurements above are the gate.

## 5. Open questions for the maintainer

1. Whether a foreign `user.bin` with two identical key runs in one item is
   in scope at all (§3.2). The arena keeps such an item verbatim, so the
   proposal removes the difference without a ruling, but a ruling settles
   what the row model must do until then.
2. Whether the readings `packed_to_pinyin_keys` drops today (§3.3) are
   reachable from the pin's parsers; if they are, the drop is a current
   defect and phase 1 fixes it.
3. The standalone store's persistence (§4.6 phase 4).
4. Whether phase 3's unification with the journal replay arena is wanted
   in this lane or left to #1090.
