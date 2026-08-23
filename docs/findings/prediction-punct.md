# Prediction punctuation — Option A

Date: 2026-08-18 · Status: recorded with #104 · Decision: **Option A**.

W11 Phase 0 made punctuation prediction conditional on `punct.redb`
consumability (`docs/findings/phrase-union.md` §9.3). The W3 file was a
raw Tkrzw convert, so the first prediction PR stubbed the punctuation
prefix empty and registered #104.

## Consumability of the raw convert

The former `oxpinyin-migrate convert` opened every Tkrzw file as HashDBM.
Measured on the pin's `punct.bin` (magic `TkrzwHDB`, 534,528 bytes):

| Open class | `Count` | Key shape | Usable as `m_system_punct_table`? |
|---|---|---|---|
| HashDBM (`convert`) | 1 | one 6-byte key | no |
| TreeDBM (`PunctTable::attach`) | 272 | `phrase_token_t` (4 bytes LE) | yes |

The same file is a HashDBM header that TreeDBM will still iterate as the
punctuation table. The convert therefore copies the wrong record view.
That is why `fixtures/w3/punct.redb` was not public-ABI consumable, and
why a raw reader of that file (Option B as originally framed) cannot
produce oracle-matching punctuation.

A TreeDBM reader in the migrate bridge would work, but it adds a second
Tkrzw class to a HashDBM-only tool, then still needs a UCS-4 value
decoder (`punct_table.cpp` null-terminated `ucs4_t` runs).

## Choice

**Option A** — regenerate from `punct.table` via a dedicated export step
(the same pattern as addon tables).

Rationale:

- Smaller: 370 text rows, no TreeDBM FFI, no UCS-4 decoder.
- Safer: does not depend on Tkrzw opening a HashDBM file as TreeDBM.
- Converges with the W11 addon path: model-archive `.table` text →
  public-ABI redb that `LookupTable` already reads.
- `punct.table` is the source `gen_binary_files --gen-punct-table`
  feeds to `PunctTable::load_text` (`punct_table.cpp:179-202`).

Option B (raw reader of the existing convert) is rejected: the convert
is the HashDBM view, not the token table.

## Public-ABI schema (`punct.redb`)

| Aspect | Value |
|---|---|
| Table name | `"data"` |
| Key | `phrase_token_t` as 4 bytes little-endian |
| Value | UTF-8 punctuation strings, each NUL-terminated, in `punct.table` order (decreasing frequency; first-seen wins on duplicate) |

Lookup is `get(token.to_le_bytes())`. A missing file, or a leftover raw
convert, loads as empty — the same as a failed upstream `attach`.

## Runtime

`pinyin_guess_predicted_candidates_with_punctuations` still runs phrase
prediction, then for each prefix token in `_compute_prefixes` order
(shortest suffix first) appends unseen puncts and prepends that list
(`pinyin.cpp:2465-2492`). The function returns `true` even when the
prefix matched no phrase-table suffix.

`run-union-diff.sh` / `run-predict-diff.sh` still skip type 8: those
drivers compare mini-table capi against the full-table oracle, and a
system suffix such as 测 is only a prefix token on the oracle. The
punctuation list is compared by `run-punct-diff.sh` on prefixes present
in both tables (好 / 中 / 国 / 中国 / 你).
