# P5 datagen compatibility — format conversion findings

Date: 2026-09-01 · Status: **implemented and tested**

## Pipeline audit

The existing oxpinyin datagen compiles from `model20.text.tar.gz` directly
(the canonical-source invariant). The compiled entries use an oxpinyin-
specific key encoding that differs from libpinyin's on-disk format for
two of the four table types.

### Format comparison

| Table | oxpinyin datagen key | libpinyin on-disk key | Compatible? |
|---|---|---|---|
| `pinyin_index` | apostrophe-joined UTF-8 | packed ChewingKey[L] (tone-zeroed) | **no** |
| `phrase_index` | token u32 LE → UTF-8 text | UCS-4 text → u32 token[] | **no** (reversed) |
| `bigram` | prev token u32 LE | prev token u32 LE | **yes** |
| `punct` | token u32 LE | token u32 LE | **yes** |

The bigram and punctuation formats are already byte-compatible. P5 adds
conversion functions for the two incompatible tables.

## Conversion module (`compat.rs`)

### `convert_pinyin_index`

Converts apostrophe-joined UTF-8 pinyin keys to packed ChewingKey format:

1. Parse syllables from apostrophe-separated key
2. Resolve each syllable via `ChewingKey::from_pinyin`
3. Encode complete key: tone-zeroed packed ChewingKey[L], 2 bytes per key
4. Encode value as `PinyinIndexItem2<L>[]` with correct C++ ABI stride
5. Write empty-value prefix markers for every shorter prefix (SEARCH_CONTINUED)
6. Write incomplete (initial-only) key entries

Syllables that don't resolve to a ChewingKey (e.g., syllable fragments)
are skipped — matching upstream's behavior where only valid pinyin
spellings are indexed.

### `convert_phrase_index`

Reverses the direction from token→text to text→tokens:

1. Parse each (token u32 LE, UTF-8 text) entry
2. Encode text as UCS-4 key (each char as u32 LE)
3. Group tokens by text (same phrase text → multiple tokens from
   different libraries)
4. Encode tokens as flat u32[] value

### Backend semantics

The conversion produces `Entries` (the universal intermediate) that can
be written through any backend. The backend-specific behavior:

| Backend | Format identity with libpinyin? |
|---|---|
| KC | Yes — same TreeDB/HashDB container, same key/value bytes |
| Tkrzw | Yes — same TreeDBM/HashDBM container, same key/value bytes |
| redb | Own native format — not libpinyin-readable, but same logical content |
| LMDB | Own native format — not libpinyin-readable, but same logical content |

KC/Tkrzw output is directly consumable by both libpinyin and oxpinyin.
redb/LMDB output is consumable only by oxpinyin (they are oxpinyin-
specific backends with no libpinyin equivalent).

### Canonical-source invariant

Preserved. The conversion operates on the in-memory `Entries` compiled
from model20 text — it never reads a libpinyin-generated database.

```text
model20 → datagen compile → Entries → compat convert → Entries → WriteStore
```

The conversion is a pure function of the compiled entries: no runtime
data, no oracle, no import step.

## Test coverage

- 7 `compat` unit tests: single/multi-syllable ChewingKey encoding,
  prefix markers, incomplete keys, phrase direction reversal, multiple
  tokens for same text, unknown syllable handling, stride validation
- 4 `compat_round_trip` integration tests: write through store, read
  back through `RawReadStore::get_raw`, verify key/value encoding for
  pinyin, phrase, bigram (already compatible), punct (already compatible)
