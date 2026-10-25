# 894-non-scalar-phrase-text — non-scalar stored phrase text

- **Status:** **SUPERSEDED — ordinary MATCH (2026-10-24 UTC)** · issue
  #894 · implementation landed with the limb-3 amendment (#1182).
- **Disposition:** `MATCH`. The former `POLICY-ACCEPTED` marker is
  removed; these are ordinary parity cases.
- **Policy:** decision-order limb 3 as amended 2026-10-24 UTC. Rust's
  `String` restriction is not a reason to accept a difference: the
  stored words are carried as bytes and encoded the way GLib encodes
  them.
- **Pin:** libpinyin 2.11.92,
  `074a2219c90feaf962d0d24f034514033ece5f99`, read from a clean tree at
  that commit. Both facades; all DBM backends. Reference image GLib
  **2.89.4-2**, not the issue's 2.80.0. The snapshot
  `20260831T000000Z` Debian source package was read, including its patch
  series: none changes `gutf8.c`. Its upstream `gutf8.c` equals the
  GNOME 2.89.4 tag at `fa41d356ee4936264c45cf11fa6c2640a89fbdda`.

## Pin derivation

Phrase text is stored as `ucs4_t` words
(`storage/phrase_index.cpp:103-111`). `_token_get_phrase`
(`src/pinyin.cpp:1627-1645`) calls `g_ucs4_to_utf8` with the declared
length. At GLib 2.89.4:

- `glib/gutf8.c:1035-1047` stops on zero and rejects only values at or
  above `0x80000000`;
- `UTF8_LENGTH` (`:81-87`) and `g_unichar_to_utf8` (`:549-607`) encode
  U+D800 as `ed a0 80` and U+110000 as `f4 90 80 80`. Neither is valid
  UTF-8; both are non-NULL results.

For `[U+7532, U+D800, U+4E59]` the pin's candidate, sentence and export
text is the byte string `e7 94 b2 ed a0 80 e4 b9 99`. Sentence assembly
converts each selected token separately, so a following token 丙
survives.

## Reproduction

Core and engine carry the pin's raw stored words, without a `char`/`str`
round trip; only the C adapters encode them:

- `crates/oxpinyin-core/src/ucs4.rs`, `PhraseText`: ordinary text stays
  in a `CompactString`. An item with a non-scalar or zero word keeps its
  raw stored words, with the declared length in a header word.
  `from_stored_words` range-checks every word before the first zero, as
  `g_ucs4_to_utf8` does. Candidate comparison, dedup, hashing and
  sentence assembly (`PhraseTextConcat`) read the outward words before
  the first zero; ranking reads the declared length. Core has no byte
  encoder.
- `crates/oxpinyin-capi-marshal/src/phrase_text.rs`,
  `phrase_text_bytes`: the one C-facing encoder. It borrows ordinary text
  up to its first NUL and encodes stored words up to the first zero with
  `encode_word`, which follows `g_unichar_to_utf8`'s widths and lead
  bytes.
- `crates/oxpinyin-data/src/phrase_library.rs`,
  `PhraseItemView::phrase_codes` and `phrase_text`: the item's raw words
  are read in place.
- `crates/oxpinyin-data/src/phrase_libraries.rs`,
  `phrase_char_count`/`trellis_item`: the stored character count prices
  the item even when its text is not scalar.
- `crates/oxpinyin-data/src/dict.rs`, `resolve_item`: candidates keep
  the stored words and the stored length.
- `crates/oxpinyin-engine` (`Candidate`, `nbest`, `session/lookup.rs`):
  candidate text, dedup and n-best sentence assembly carry `PhraseText`;
  `crates/oxpinyin-facade/src/export_rows.rs` writes export rows through
  `phrase_text_bytes`.
- Both C facades' sentence writers and token getters write
  `phrase_text_bytes` through `malloc_cstr_with`, and candidates intern
  the same bytes; the token getters answer the pin's stored length.
- `crates/oxpinyin-user/src/persistence.rs`, `SystemLibrary::item`: the
  user store reads a system library's original item with
  `phrase_codes`, so a malformed system item keeps its raw words.

The former Rust counterpart — `char::from_u32` refusing the item and the
C marshalling answering NULL/false — is gone from these paths.

The #894 cases below cover system data only. A malformed item in a user
library (`user.bin`, `addon.bin`) still loads through a Rust `String`: a
non-scalar item is dropped, and a zero-word item becomes stored words
with the full declared count and outward text ending at the NUL. Those items
are tracked in #1198.

## Cases

`tools/bisection/contract-diff.py --issue 894` selects
`phrase-text/{pinyin,zhuyin}-{surrogate,above-unicode}-{candidate,sentence,choose}`
and `phrase-text/pinyin-{surrogate,above-unicode}-export`. They now
compare both sides directly and MATCH; the scalar controls and the
zhuyin export-symbol absence checks remain MATCH. A `choose` case
chooses the stored phrase, reads the sentence and commits it as a client
does, with `train`; it observes the cursor, the sentence bytes, the
trained unigrams and, for pinyin, the exported pronunciation counts. The record's old
binding observations and `contract-diff-policy` block are removed; the
differential, not a recorded expectation, is the evidence.

**Verification (2026-10-24 UTC):** the bdb reference cell, 24/24 #894
cases, `MATCH`; the completion gate is recorded in the change's PR.
**Verification (2026-10-25 UTC):** with the `choose` cases, the bdb
reference cell, 32/32 #894 cases, `MATCH`.

**Prior rulings:** none reversed. The valid-input proof stands: valid
supported phrase tables contain no non-scalar word, and the change adds
no valid-input difference.
