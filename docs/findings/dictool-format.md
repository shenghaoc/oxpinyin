# pinyin-dictool and the classic libpinyin user-dictionary interchange (W7-T1)

Date: 2026-08-16 · Status: **pinned for pinyin-dictool import/export** ·
Scope: the text format `pinyin-dictool import --user-dir <path> <file.txt>`
reads and `pinyin-dictool export --user-dir <path> [file.txt]` writes.

pinyin-dictool speaks the **classic ibus-libpinyin interchange format**, not
an invented format. Reference implementation, pinned frontend tag `1.16.5`:

- Import: `LibPinyinBackEnd::importPinyinDictionary`
  (`src/PYLibPinyin.cc:230-277`). Each line is split with
  `g_strsplit_set(line, " \t", 3)` — space **or** TAB, at most three
  pieces. Two pieces mean `count = -1` (the ABI default); three pieces mean
  the third is `count`. The ABI trio is then driven
  (`pinyin_begin_add_phrases` → `pinyin_iterator_add_phrase` →
  `pinyin_end_add_phrases`) and `pinyin_save` is called.
- Export: `LibPinyinBackEnd::exportUserPhrase` /
  `exportBigramPhrase` / `exportPinyinDictionary`
  (`src/PYLibPinyin.cc:280-353`). Export writes the same space-separated
  grammar: phrase rows from `pinyin_begin_get_phrases`, then rendered
  bigram rows from `pinyin_begin_get_bigram_phrases`; a zero count is
  written as the 2-field form, every non-zero count as the 3-field form.

## 1. Grammar

```text
file       := line*
line       := comment | blank | record (LF | EOF)
comment    := [SP / TAB]* '#' [^LF]* LF          # dictool superset
blank      := [SP / TAB]* LF                      # dictool superset
record     := phrase (SP / TAB) pinyin
              [ (SP / TAB) count ] LF
phrase     := (UTF-8 scalar value)*
              with 1 <= count(scalars) < 16,
              none of CR, LF, NUL, SP, TAB
pinyin     := any text the import ABI parses to exactly
              count(phrase scalars) complete keys
count      := digit+
              with numeric value in 0..=2147483647
```

Field splitting matches `g_strsplit_set(line, " \t", 3)` exactly:

- space and TAB are equivalent separators;
- consecutive separators produce empty fields;
- with three or more apparent fields, the third piece keeps the remaining
  separator bytes (`"a b 3 extra"` → `phrase=a`, `pinyin=b`,
  `count-text="3 extra"`).

A 2-field line means **use the ABI default count** (`count = -1` upstream,
which `pinyin_iterator_add_phrase` resolves to 5). The pinyin field is
validated with the same parser the import ABI uses: longest parsed prefix,
complete untuned full-pinyin keys only, trailing unparsed bytes ignored —
so both `nihao` and `nihaoXYZ` parse as `ni'hao` for the 2-character phrase
`你好`, exactly like `FullPinyinParser2` under the frontend path. Tone
digits remain outside the currently supported key schema and are reported
as an unparseable pinyin with a line number.

## 2. Dictool superset extensions

The frontend silently skips any line whose split does not produce 2 or 3
pieces and ignores `pinyin_iterator_add_phrase` failures. pinyin-dictool is
a command-line tool, so it reports instead:

- wrong field count → `line N: expected 2 or 3 space/tab-separated fields`;
- empty phrase/pinyin, unparseable pinyin, phrase/key-count mismatch,
  non-decimal or out-of-range count, duplicate `(phrase, pinyin)` →
  `line N: <reason>`.
- `#` comment lines and whitespace-only blank lines are skipped. The
  frontend has no comment syntax; a comment line would simply fail the
  phrase parse there and be ignored, so accepting it here is a superset,
  not a format fork.

All errors carry 1-based line numbers.

## 3. Count semantics and idempotency

The raw ABI count is an **add amount**, not add-or-update:

- `pinyin_iterator_add_phrase` calls `_add_phrase` (`pinyin.cpp:614-653`);
- an existing same-library phrase has the reading merged with
  `PhraseItem::add_pronunciation(keys, count)`, which adds to the stored
  count (`phrase_index.cpp:55-88`).

pinyin-dictool import is idempotent because the **file count is a desired
absolute pronunciation count**:

- 3-field target `C` raises the stored count to `C` (add `C - current`);
- a 2-field line floors at the ABI default count, 5;
- re-running the same file never doubles;
- a stored count already above the file target is never lowered or
  deleted (the file is a monotonic floor);
- a 3-field `0` against a not-yet-stored pronunciation adds a zero-count
  row once, matching `atoi("0")` on the frontend path; re-runs are no-ops.

The parser rejects duplicate `(phrase, pinyin)` lines, so one input has one
desired value per pronunciation.

## 4. Export

`pinyin-dictool export --user-dir <path> [file.txt]` writes the classic
format through the W6-T7 C ABI export iterators. No `[file]` means stdout.

Ordering rule: **phrase rows first, then rendered bigram rows** — the order
`exportPinyinDictionary` writes when both Export-button flags are on
(`PYLibPinyin.cc:346-350`). Within each block the iterator order is kept:
phrase rows in token order then pronunciation order; bigram rows in the
stored bigram-key order. A row with count 0 is written as 2 fields; every
non-zero count is written as 3 fields, exactly like the frontend's
`-1 == count` skip.

Round-trip contract: `dictool import` → `dictool export` is row-identical
to the imported frontend-style text modulo the ordering rule above (no
trained bigrams means only the phrase block). The reverse contract is the
reason this format exists: a dictool-written file is classic interchange
and imports through the pinned frontend's `importPinyinDictionary`.

## 5. Example

Frontend-style file:

```text
你好 ni'hao 3
世界 shi'jie 7
词 ci
```

Importing it twice into one user directory exports:

```text
你好 ni'hao 3
世界 shi'jie 7
词 ci 5
```

The 2-field `词 ci` floors at default count 5; every count is stable on
re-import.
