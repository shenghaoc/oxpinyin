# libpinyin pinyin-index DBM format — P2 source-level findings

Date: 2026-09-01 · Status: **verified against pinned source and real bytes**

Findings for the P2 implementation: direct consumption of libpinyin's
`pinyin_index.bin` DBM. All facts verified against the pinned source tree
(`libpinyin-2.11.91`, `0c5e80e1`) and/or the real data files from the
perf-matrix container.

---

## 1. DBM container

`pinyin_index.bin` is a **backend DBM** — KC TreeDB or Tkrzw TreeDBM,
selected by `--with-dbm` at libpinyin's build time. It is NOT a
MemoryChunk/mmap file (those are the per-library `.bin` files: P1 scope).

Source: `gen_binary_files.cpp` — `ChewingLargeTable2::attach(pinyin_index.bin,
READWRITE|CREATE)`.

Hexdump confirmation:
- KC cell: `4b 43 0a 00 10 0e 06 bc 31 08` → KC TreeDB magic
- Tkrzw cell: `54 6b 72 7a 77 48 44 42 0a 07` → Tkrzw DBM

Opening is lazy: `attach` opens the container handle without scanning.
Per-lookup `Get` is the runtime access pattern.

## 2. ChewingKey — the 16-bit packed syllable

`src/storage/chewing_key.h:41-48`:

```c
struct _ChewingKey {
    guint16 m_initial : 5;   // bits 0..4
    guint16 m_middle  : 2;   // bits 5..6
    guint16 m_final   : 5;   // bits 7..11
    guint16 m_tone    : 3;   // bits 12..14
    guint16 m_zero_padding : 1; // bit 15, always 0
};
```

`static_assert(sizeof(ChewingKey) == 2)` verified by
`tools/bisection/key-surface-diff.c:49`.

Packing: `((initial & 0x1f) | ((middle & 0x3) << 5) | ((final & 0x1f) << 7) | ((tone & 0x7) << 12))`.

Element counts: `NUM_INITIALS = 24`, `NUM_MIDDLES = 4`, `NUM_FINALS = 18`
(`chewing_enum.h`).

Rust implementation: `oxpinyin-chewing/src/chewing_key.rs` —
`ChewingKey::to_packed()` / `ChewingKey::from_packed()`.

## 3. Two key spaces in one DBM

`pinyin_phrase3.h:160-177` defines two key spaces sharing one file:

1. **Complete index:** key = packed `ChewingKey[L]` with every `m_tone`
   zeroed, 2 bytes per syllable, concatenated. Tone-free lookup: the
   value carries all tone variants.

2. **Incomplete (initial) index:** key = packed `ChewingKey[L]` with
   only `m_initial` set (middle, final, tone all zero). Used for
   initial-only abbreviation lookup.

The two spaces coexist because their packed representations never
collide: an incomplete key has `m_middle == 0 && m_final == 0`, while
a complete tone-zeroed key preserves middle and final.

## 4. DBM key construction

`chewing_large_table2_tkrzwdb.cpp:221-232` (`search`):

1. Copy the input `ChewingKey[L]` array.
2. Zero every `m_tone`.
3. Encode the array as `2*L` raw bytes (LE on LE platforms).
4. Call `m_db->Get(encoded_key)`.

For the incomplete key space, step 2 is replaced by keeping only
`m_initial` (zero middle, final, tone).

## 5. DBM value layout — PinyinIndexItem2\<L\>

`pinyin_phrase3.h:181` (verified against source):

```c
template <int L>
struct PinyinIndexItem2 {
    phrase_token_t m_token;     // u32
    ChewingKey     m_keys[L];  // L × 2 bytes
};
```

### Critical ABI fact: C++ struct padding

`sizeof(PinyinIndexItem2<L>)` includes tail padding to the `u32`
alignment of `m_token`:

| L | Field sum | sizeof | Padding |
|---|-----------|--------|---------|
| 1 | 6         | **8**  | 2 bytes |
| 2 | 8         | 8      | none    |
| 3 | 10        | **12** | 2 bytes |
| 4 | 12        | 12     | none    |
| 5 | 14        | **16** | 2 bytes |
| 16| 36        | 36     | none    |

Formula: `stride = (4 + 2*L + 3) & ~3`.

Verified by a size probe compiled against the pinned headers
(`docs/findings/libpinyin-system-data-formats-2026-09-01.md` §5):
`sizeof(PinyinIndexItem2<1>) == 8`, not 6.

**Any Rust reader must stride by `sizeof`, not by the field sum.**

### Value format

The value is a contiguous array of `PinyinIndexItem2<L>` records:
`value_len / sizeof(PinyinIndexItem2<L>)` records, each at offset
`i * sizeof(PinyinIndexItem2<L>)`.

Record layout within the stride:
- Bytes 0..4: `m_token` (u32 LE)
- Bytes 4..4+2L: `m_keys[0]..m_keys[L-1]` (ChewingKey LE, 2 bytes each)
- Bytes 4+2L..stride: padding (zero-filled)

The stored `m_keys` preserve the original tones — the lookup key zeroes
them, but the value keeps them for tone-filtered matching.

## 6. SEARCH_CONTINUED — prefix markers

`chewing_large_table2_tkrzwdb.cpp:284-296` (`add_index_internal`):

For every stored key of length L, `add_index_internal` writes
**empty-value entries** for every shorter prefix (lengths 1..L-1).
These are the `SEARCH_CONTINUED` markers.

The search logic (`search_internal`, lines 133-162):

- Key not found → `SEARCH_NONE`
- Key found, empty value → `SEARCH_CONTINUED` (prefix marker)
- Key found, non-empty value, tone matches → `SEARCH_OK | SEARCH_CONTINUED`
- Key found, non-empty value, no tone match → `SEARCH_CONTINUED`

`SEARCH_CONTINUED` is set whenever the key exists (empty or not).

## 7. Tone matching within the value

When the input has non-zero tones, `search_internal` performs a
binary search within the decoded value to find records whose stored
`m_keys[i].m_tone` matches `input_keys[i].m_tone` for all `i`. When
all input tones are zero (tone-free lookup), all records match.

## 8. Host-endianness assumptions

ChewingKey packing uses the C compiler's bitfield layout. On LE
platforms (x86, ARM LE — the targets libpinyin builds for), the packed
u16 is stored LE. All supported oxpinyin targets are LE.

`m_token` (u32) is stored in native endianness — LE on LE platforms.

## 9. Duplicate/ordering semantics

The DBM value is a flat array of `PinyinIndexItem2<L>` records. The
order within a value is the insertion order from `gen_binary_files`,
which processes libraries sequentially (gb_char, gbk_char, opengram,
merged, then addons). Duplicates can occur when the same token appears
in multiple libraries with different tones; the search returns all
matching records.

## 10. Backend opening

- KC: `ChewingLargeTable2` constructs a `TreeDB` (not `PolyDB`) and
  opens the file. No comparator parameter → KC default `LEXICALCOMP`
  (byte order).
- Tkrzw: `tkrzw_dbm_open` with no comparator → default
  `LexicalKeyComparator` (unsigned byte order).

Both are consistent with oxpinyin-store's existing key-ordering
contract.

## 11. The DBM is opened lazily

`ChewingLargeTable2::attach` only opens the container handle. No
records are read until a lookup calls `Get`. This is why libpinyin's
init is sub-millisecond for the pinyin index — the cost is deferred
to per-keystroke lookups.

## 12. Rust implementation

- `crates/oxpinyin-data/src/chewing_table.rs` — ChewingTable: the Rust
  equivalent of `ChewingLargeTable2`. Key encoding, value decoding with
  correct ABI stride, search semantics matching the upstream.
- `crates/oxpinyin-data/src/chewing_dict.rs` — ChewingDictionary:
  bridges `ChewingTable` to the `Dictionary` trait via `SyllableKey` →
  `ChewingKey` conversion.
- `crates/oxpinyin-store/src/lib.rs` — `RawReadStore` trait: unframed
  key access for consuming libpinyin's flat-keyspace DBM files directly.
