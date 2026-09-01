# libpinyin phrase-index DBM format — P3 source-level findings

Date: 2026-09-01 · Status: **verified against pinned source and real bytes**

Findings for the P3 implementation: direct consumption of libpinyin's
`phrase_index.bin` DBM. All facts verified against the pinned source tree
(`libpinyin-2.11.91`, `0c5e80e1`).

---

## 1. DBM container

`phrase_index.bin` is a **backend DBM** — KC TreeDB or Tkrzw TreeDBM,
selected by `--with-dbm` at libpinyin's build time. Same container
family as `pinyin_index.bin` (P2).

Source: `gen_binary_files.cpp` — `PhraseLargeTable3::attach(phrase_index.bin,
READWRITE|CREATE)`.

Opening is lazy: `attach` opens the container handle without scanning.
Per-lookup `Get` is the runtime access pattern.

## 2. Key encoding — UCS-4 phrase text

`phrase_large_table3_tkrzwdb.cpp:28-52`:

The key is the phrase text encoded as raw `ucs4_t[]` — each character
as a `guint32` (4 bytes, LE on LE platforms), concatenated.

Examples:
- 你 (U+4F60): key = `60 4F 00 00` (4 bytes)
- 你好 (U+4F60 U+597D): key = `60 4F 00 00 7D 59 00 00` (8 bytes)
- Key length = `char_count * 4`

The encoding uses `g_utf8_to_ucs4` in libpinyin. In Rust:
`text.chars().flat_map(|ch| (ch as u32).to_le_bytes())`.

## 3. Value encoding — bare token array

`phrase_large_table3.cpp:28-52`:

The value is a flat array of `phrase_token_t` values (each `u32` LE),
concatenated. No struct padding, no header.

- Value length = `token_count * 4`
- Each token occupies bytes `[i*4..(i+1)*4]`

Multiple tokens for the same text can occur when a phrase appears in
multiple libraries (e.g. 的 appears in both `gb_char` and `gbk_char`).

This is simpler than the pinyin index's `PinyinIndexItem2` — no
ChewingKey arrays, no struct padding.

## 4. PhraseLargeTable3 vs PhraseLargeTable2

The v3 format (used since libpinyin 2.x) is simpler than v2:

- **v2** (`phrase_large_table2.cpp:62`): `PhraseIndexItem2 = {u32 token,
  ucs4_t phrase[L]}` — each record contains both the token AND the
  phrase text (redundant with the key).
- **v3** (`phrase_large_table3.cpp:28-52`): bare `u32 token[]` — the
  phrase text is already the key, so the value is just tokens.

P3 implements v3 only (the pinned libpinyin 2.11.91 version).

## 5. Continuation semantics — prefix markers

Like the pinyin index, `PhraseLargeTable3::add_index` writes
empty-value entries for every shorter prefix of a stored key
(`phrase_large_table3_tkrzwdb.cpp:120-140`). This enables
`search_suggestion`'s cursor walk.

## 6. search_suggestion — the Jump/Next walk

`phrase_large_table3_tkrzwdb.cpp:150-190`:

The upstream `search_suggestion` creates a DBM iterator, `Jump`s to the
UCS-4 encoding of the prefix, then `Next`s through entries whose key
starts with the prefix and is strictly longer. Each matching entry's
token array is decoded and returned.

The walk order is the DBM's physical order — deterministic for a given
file but backend-dependent and not expressible as a sort key. oxpinyin
deliberately uses a text-ascending `BTreeMap` order instead (documented
in `upstream-divergences.md`, "Predicted-candidate tie order").

For P3, `search_suggestion` is stubbed — the cursor walk requires
`range_raw` on the store (a larger extension). The existing
`SystemDictionary::suggest_after` continues to work via its reverse map.

## 7. Host-endianness

Same as the pinyin index: all `u32` values (UCS-4 characters, tokens)
are stored in native byte order — LE on all supported targets.

## 8. Error/no-result semantics

- Key not found → empty (no tokens)
- Key found, empty value → empty (prefix marker, not a phrase)
- Key found, value length not multiple of 4 → parse error
- Key found, non-empty value → decode tokens

## 9. Rust implementation

- `crates/oxpinyin-data/src/phrase_table.rs` — PhraseTable: the Rust
  equivalent of `PhraseLargeTable3`. UCS-4 key encoding, token array
  decoding, exact-match search, prefix probe.
- `crates/oxpinyin-data/src/chewing_dict.rs` — ChewingDictionary now
  supports an optional `PhraseTable` for `tokens_for_text` lookups via
  `open_with_phrase_dbm()`. Falls back to scanning the legacy phrase
  index when no phrase DBM is provided.
- Reuses the P2 `ChewingDbm` trait and `RawChewingDbm` wrapper — no
  new storage abstraction.
