# The preedit key family — Phase 1 explain-back

Date: 2026-08-28 · Status: **Phase 1 only; no code. Awaiting
confirmation before Phase 2.** · Branch: `feat/preedit-key-accessor`.

STOP #1 was: `pinyin_get_pinyin_key` is missing,
`pinyin_get_pinyin_key_rest` and `_positions` are stubs returning
`false`, and together with four siblings they are fcitx-libpinyin's
entire preedit renderer. Under the policy's new E2E I/O rule the two
stubs are **defects**, not gaps. This is the read-back before
implementing.

**None of the three STOP triggers fired.** `ChewingKey` semantics do not
differ in the full-pinyin context; the accessor does not change existing
behaviour; and one accessor does unblock all seven.

## (a) What `pinyin_get_pinyin_key` returns, and what a `ChewingKey` is

`ChewingKey` is **not** a zhuyin-specific type. It is a 16-bit packed
bitfield (`chewing_key.h:41-55`) used by *every* scheme:

```c
struct _ChewingKey {
    guint16 m_initial : 5;
    guint16 m_middle  : 2;
    guint16 m_final   : 5;
    guint16 m_tone    : 3;
    guint16 m_zero_padding : 1;
};
```

Full pinyin, double pinyin and zhuyin all parse into the same struct
with the same values; only the *renderer* differs —
`get_pinyin_string()` vs `get_zhuyin_string()` on one key. The name is
lineage, not semantics. `ChewingKeyRest` is likewise scheme-neutral:
`{ guint16 m_raw_begin; guint16 m_raw_end; }` with
`length() = m_raw_end - m_raw_begin`.

**And both are opaque to consumers.** `libpinyin15-dev` installs
`pinyin.h`, `novel_types.h` and `pinyin_custom2.h` — **not**
`chewing_key.h` — and `pinyin.h` carries only

```c
typedef struct _ChewingKey ChewingKey;
typedef struct _ChewingKeyRest ChewingKeyRest;
```

so a consumer holds an incomplete type. It can pass the pointer back; it
cannot read a field. **oxpinyin's representation is therefore free** —
the E2E I/O rule binds the *observable outputs* (the strings, the two
`guint16`s, the boolean) and not the struct bytes, because no conforming
consumer can observe them.

The pin's function (`pinyin.cpp`):

```c
if (offset >= matrix.size() - 1)        return false;
if (0 == matrix.get_column_size(offset)) return false;
_check_offset(matrix, offset);                       // aborts on a lone zero key before offset
offset = _compute_pinyin_start(matrix, offset);      // skip forward over lone zero-key columns
static ChewingKey key; ChewingKeyRest key_rest;
matrix.get_item(offset, 0, key, key_rest);
*ppkey = &key; return true;
```

Two behaviours to carry: the returned pointer is a **process-wide
`static`**, one slot overwritten on every call (a per-instance slot is
observably identical for correct use and strictly safer), and
`_compute_pinyin_start` **skips forward** over columns holding exactly
one zero key, so an offset pointing into an apostrophe run answers the
key *after* the run.

## (b) The minimal engine change

Smaller than the earlier report implied. `oxpinyin-engine`'s
`cursor.rs::matrix_spans` **already computes exactly this data** on
every D1/D2 call, and throws the key away:

```rust
fn matrix_spans(input, options) -> Result<(Vec<(usize, usize)>, usize)> {
    let graph  = SegmentGraph::build_with_options(input, options)?;
    let matrix = build_scan_matrix(&graph, options, true);
    for column in &matrix { for key in column {
        spans.push((key.syllable_start, key.to));   // ← key.key and key.tone discarded
    }}
}
```

and `ScanKey` (`session.rs:2362-2373`) already carries everything
needed:

```rust
pub(crate) struct ScanKey {
    pub(crate) key: SyllableKey,
    pub(crate) from: usize,
    pub(crate) to: usize,
    pub(crate) syllable_start: usize,
    pub(crate) crosses_separator: bool,
    pub(crate) tone: u8,
}
```

**The change:** make `ScanKey` public (or add a public projection of
it), rename `matrix_spans` to `matrix_keys` returning `Vec<ScanKey>`,
and keep `matrix_spans` as a one-line `.map(|k| (k.syllable_start,
k.to))` over it. Add one `Session` method mirroring `right_word_offset`'s
existing shape:

```rust
pub fn matrix_keys(&self) -> Result<(Vec<ScanKey>, usize), EngineError> {
    crate::cursor::matrix_keys(self.raw.as_bytes(), self.settings.options)
}
```

**No behaviour change.** No new computation is introduced and no
existing caller's values move: D1/D2 keep consuming the identical
`(syllable_start, to)` projection in the identical order. Nothing in the
pins, and no existing test, is touched.

**The seven, confirmed:**

| # | Symbol | Needs |
| --- | --- | --- |
| 1 | `pinyin_get_pinyin_key` | the accessor |
| 2 | `pinyin_get_pinyin_key_rest` *(stub today)* | the accessor |
| 3 | `pinyin_get_pinyin_key_rest_positions` *(stub today)* | the accessor |
| 4 | `pinyin_get_pinyin_key_rest_length` | `end - begin` from 2 |
| 5 | `pinyin_get_pinyin_string` | the key from 1; `SyllableKey::text()` |
| 6 | `pinyin_get_pinyin_strings` | the key from 1; `text.rs::split_pinyin_key` |
| 7 | `pinyin_get_zhuyin_string` | the key from 1; the chewing renderer |

4–7 are pure capi wrapping once 1–3 have data. One accessor, all seven.

## (c) Interaction with D1/D2 — they share a root, in both directions

**In the engine:** `pinyin_get_pinyin_offset`, `_left_` and `_right_`
already reach `matrix_spans` for plain full pinyin
(`cursor.rs:266-292`). This PR does not add a parallel derivation; it
widens theirs. `build_columns` — which already models the pin's matrix,
including `lone_zero()` columns — stays the single model of the matrix.

**In the consumer, which matters more:** fcitx's preedit loop is
*driven* by a D1/D2 symbol. `eim.cpp` walks
`for (i = pinyinOffset(); i < m_parsedLen; )`, and advances with

```c
size_t nexti;
if (pinyin_get_right_pinyin_offset(m_inst, i, &nexti)) { i = nexti; } else { … }
```

So `pinyin_get_pinyin_key` is only ever asked about offsets
`pinyin_get_right_pinyin_offset` produced — i.e. **syllable-aligned
ones**. The empty-mid-syllable-column `false` path is real and must be
implemented, but it is not on fcitx's hot path; what fcitx actually
depends on is that the aligned offsets agree with D1/D2's walk. Sharing
`matrix_spans` is what guarantees that by construction.

## (d) E2E I/O expectation for `nihao`

**Derived from source, not measured** — the pinned oracle cannot be
provisioned here (`build-oracle.sh`'s SHA-pinned tarballs come from
`codeload.github.com`, 403 under this egress policy). Phase 2 must
confirm these against a built oracle before claiming compliance; that is
what the E2E rule's "a symbol with no probe is unverified" means.

`nihao` is 5 bytes parsing as `ni|hao`, so the matrix has
`consumed + 1 = 6` columns: a key at column 0 (`ni`, raw 0–2), a key at
column 2 (`hao`, raw 2–5), columns 1/3/4 empty (mid-syllable), column 5
the reserved end slot.

| offset | `_get_pinyin_key` | key | `_key_rest_positions` | `_length` | `_pinyin_string` | `_pinyin_strings` | `_zhuyin_string` |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 0 | `true` | `ni` | `(0, 2)` | 2 | `ni` | `n` / `i` | `ㄋㄧ` |
| 1 | `false` | — | — | — | — | — | — |
| 2 | `true` | `hao` | `(2, 5)` | 3 | `hao` | `h` / `ao` | `ㄏㄠ` |
| 3 | `false` | — | — | — | — | — | — |
| 4 | `false` | — | — | — | — | — | — |
| 5 | `false` (`offset >= size-1`) | — | — | — | — | — | — |

fcitx's own walk visits 0 → 2 → 5, where 5 ends the loop against
`m_parsedLen`, so it never sees a `false`.

**One subtlety that would otherwise differ silently.** The pin sets
`rest.m_raw_begin = m` over the *one-pinyin substring*
(`pinyin_parser2.cpp:282`), so `m_raw_begin` is the syllable's own start
— **not** including a preceding apostrophe, which occupies its own
zero-key column. That is exactly the `(syllable_start, to)` projection
`matrix_spans` already collects, and D1/D2 matching the pin corroborates
it. oxpinyin folds a consumed apostrophe onto the *following* edge
(`Edge::from` may be one byte before `syllable_start`), so Phase 2 must
report `syllable_start` and not `from`, and must reproduce
`_compute_pinyin_start`'s skip-forward over lone-zero columns.

## What Phase 2 owes

- The engine accessor above, plus capi wrappers for all seven.
- A differential probe driving each of the seven at every offset of a
  parsed input, asserting byte-identical output against the oracle —
  the E2E rule's verification clause. The table in (d) is the expected
  result to confirm, not to assume.
- Frozen pins bit-identical, and `cpp-smoke` still asserting drop-in
  identity (`DT_NEEDED: libpinyin.so.15` resolving to the build under
  test, not a system libpinyin).
