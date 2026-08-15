# pinyin-dictool user-vocabulary text format (W7-T1)

Date: 2026-08-16 · Status: **pinned for pinyin-dictool import mode** ·
Scope: the text format `pinyin-dictool import --user-dir <path> <file.txt>`
reads, and the idempotency contract around it.

This is a pinyin-rs format. libpinyin has no CLI for user-dictionary text
import; its UI import path (`PYLibPinyin.cc:230-276`) reads a UI-private
whitespace-split file and `import_interpolation` is a training-time system
model tool. There is therefore no upstream format this file could conflict
with or must reproduce byte-for-byte.

## 1. Why TSV

`phrase<TAB>pinyin<TAB>count`, one record per line:

- The phrase and pinyin fields are both arbitrary text with no spaces in the
  pinyin and no structural punctuation in the phrase. A single-character TAB
  delimiter is therefore unambiguous, unlike the upstream UI's
  `g_strsplit_set(line, " \t", 3)`.
- Users can create and edit the file with a spreadsheet, `cut`, `sort`, or
  plain `printf`; no JSON/quoting rules are required.
- The W6-T7 phrase export is already a `(phrase, pinyin, count)` triple, so
  export → save as TSV → import is a direct round-trip.
- The pinyin field is spelled the same way the export iterator spells it:
  `'`-joined syllables (`ni'hao`, `zhong'guo`).

JSON Lines was considered and rejected: every field would need quoting and a
JSON dependency or a hand parser, with no benefit for a three-column integer
table.

## 2. Grammar

```text
file       := line*
line       := comment | blank | record (LF | EOF)
comment    := [SP / TAB]* '#' [^LF]* LF
blank      := [SP / TAB]* LF
record     := phrase TAB pinyin TAB count LF
phrase     := (UTF-8 scalar value)*
             with 1 <= count(scalars) < 16,
             none of TAB, CR, LF, NUL
pinyin     := syllable (''' syllable)*
syllable   := a complete untuned full-pinyin syllable from the frozen
             pinyin-core inventory (405 entries)
             with count(syllables) == count(phrase scalars)
count      := digit+
digit     := '0'..'9'
             with numeric value in 1..=2147483647
```

Deterministic notes:

- The file is split on `LF`; a `CR` immediately before the `LF` is removed
  (CRLF input works). A lone CR is an ordinary forbidden field byte.
- Comment lines and blank lines are recognized after leading ASCII space or
  TAB only, and are skipped. `#` inside a data field is data, not a comment.
- Data lines have exactly three TAB-separated fields. Extra tabs are a parse
  error, never a truncated count.
- Every field must be valid UTF-8. Invalid UTF-8 is reported at the 1-based
  line where decoding stopped.
- The phrase is 1..=15 Unicode scalar values — the same validity window the
  import ABI enforces (`MAX_PHRASE_LENGTH = 16`, `pinyin.cpp:642-643`).
- Pinyin is intentionally stricter than the raw ABI. The ABI's
  `FullPinyinParser2` accepts unseparated strings (`nihao`) and ignores an
  unparsed trailing suffix; this file format requires the canonical
  `'`-joined complete syllables so parsing is deterministic, a malformed
  line is caught before any write, and export round-trips are literal.
  Tone digits, correction aliases, and initial-only keys (`n`) are not part
  of the frozen pinyin-core inventory and are rejected here; the raw ABI's
  tone path stores keys the current user schema cannot render.
- `count` is decimal, no sign, no `-1` default marker. The default marker is
  omitted deliberately: idempotency below needs an absolute target.

All format errors carry the 1-based line number:
`pinyin-dictool: line N: <reason>`.

## 3. Count semantics and idempotency

The raw ABI's `count` is an **add amount**, not an add-or-update:

- `pinyin_iterator_add_phrase` calls `_add_phrase` (`pinyin.cpp:614-653`).
- For a phrase already in `USER_DICTIONARY`, `_add_phrase` removes the item,
  calls `PhraseItem::add_pronunciation(keys, count)`, and re-adds it
  (`pinyin.cpp:574-583`).
- `PhraseItem::add_pronunciation` finds the exact key sequence and **adds**
  `delta` to its stored count (`phrase_index.cpp:55-88`).

So two raw ABI calls `add(你好, nihao, 3)` store 6, not 3. pinyin-rs
reproduces that at the C ABI, and the W7 differential verifies both engines
accumulate identically.

`pinyin-dictool import` is still idempotent because **this format's count is
a desired absolute pronunciation count**, not an add amount. The tool
materializes the existing phrase export before the import batch and passes
the raw ABI `max(0, desired - current)` as its add amount:

- Re-running the same file leaves every count at its desired value — no
  doubling.
- A store with a lower count is raised to the file's value.
- A store with a higher count (for example from later training) is never
  lowered or deleted; the file is a monotonic floor, not a wipe-and-replace.
- New readings for an existing phrase merge onto that phrase's token, exactly
  as the raw ABI does.

The file parser rejects duplicate `(phrase, pinyin)` lines, so one input has
one desired value per pronunciation and no line-order ambiguity.

## 4. Batch and save semantics referenced by the format

The import trio is a per-phrase sequence, not one atomic batch. Each
successful `pinyin_iterator_add_phrase` mutates the phrase/pinyin tables and
the unigram immediately; `pinyin_end_add_phrases` only compacts, arms
`m_modified`, and frees the iterator (`pinyin.cpp:506-512, 614-658`). The
dictool therefore:

1. parses and validates the whole file first (no partial write on format
   errors),
2. performs ABI adds one by one (each a durable redb commit), then
3. calls `pinyin_end_add_phrases` and `pinyin_save`.

## 5. Example

```text
# My phrases
你好ni'hao3
世界shi'jie7
行xing4
行hang2
```

Importing it twice into one user directory exports:

```text
你好|ni'hao|3
世界|shi'jie|7
行|xing|4
行|hang|2
```

exactly once per pronunciation, with the same counts both times.
