# 894-nul-phrase-text — embedded zero in stored phrase text

- **Status:** **SUPERSEDED — ordinary MATCH (2026-10-24 UTC)** · issue
  #894 · implementation landed with the limb-3 amendment (#1182).
- **Disposition:** `MATCH`. The former `POLICY-ACCEPTED` marker is
  removed; these are ordinary parity cases.
- **Policy:** decision-order limb 3 as amended 2026-10-24 UTC. The old
  acceptance rested on "ordinary Rust does not reproduce it". That is no
  longer a reason: the pin's outward result is produced in natural,
  efficient Rust.
- **Pin:** libpinyin 2.11.92 at
  `074a2219c90feaf962d0d24f034514033ece5f99`; both facades, all
  backends. GLib **2.89.4-2** source as described in the
  [non-scalar record](894-non-scalar-phrase-text.md). The Debian patch
  series changes no UTF conversion code.

## Pin derivation

The pin stores phrase text as `ucs4_t` words
(`storage/phrase_index.cpp:103-111`, `get_phrase_string`). The token and
candidate paths read those words and pass the declared length to GLib:

- `_token_get_phrase` (`src/pinyin.cpp:1627-1645`) calls
  `g_ucs4_to_utf8(buffer + begin, length - begin, NULL, NULL, NULL)` and
  answers the allocation.
- GLib's `g_ucs4_to_utf8` loop breaks on the first zero word
  (`glib/gutf8.c:1035-1038` at 2.89.4) even though the length is
  explicit. `:1054-1060` encodes only the words before it, with
  `g_unichar_to_utf8` (`:549-607`). The pin therefore exposes the prefix
  before the zero; the words after it are not observable through the
  text.

For `[U+7532, U+0000, U+4E59]` (甲, zero, 乙) the pin's candidate,
sentence and export text is 甲. Sentence assembly converts each selected
token separately and concatenates the prefixes, so a following token 丙
survives: 甲丙.

## Reproduction

`PhraseText` keeps Scalar private and NUL-free, with no cached length or
metadata bytes. Construction scans once for NUL; text containing one becomes
Stored with all characters and the full declared character count.

Core and engine carry the stored UCS-4 words, zero included, instead of
decoding them into a Rust `String` first; only the C adapters encode
them:

- `crates/oxpinyin-core/src/ucs4.rs`, `PhraseText`: an item with a zero
  word keeps its raw stored words, the zero and its suffix included,
  with the declared length in a header word. Its outward text, which
  candidate comparison, dedup, hashing and sentence assembly
  (`PhraseTextConcat`) read, ends at the first zero; ranking reads the
  declared length. Sentence assembly cuts each part at its own zero, so
  a following token survives.
- `crates/oxpinyin-capi-marshal/src/phrase_text.rs`,
  `phrase_text_bytes`: the one C-facing encoder, which stops at the
  first zero.
- `crates/oxpinyin-data/src/phrase_library.rs`,
  `PhraseItemView::phrase_codes` and `phrase_text`: the item's declared
  words, including the zero and its suffix, are read in place.
- `crates/oxpinyin-data/src/phrase_libraries.rs`,
  `phrase_char_count`/`trellis_item`: ranking still counts the stored
  character count, which the zero does not shorten.
- `crates/oxpinyin-data/src/dict.rs`, `resolve_item`: the entry carries
  the stored words and the stored length.
- `crates/oxpinyin-engine` (`Candidate`, `nbest`, `session/lookup.rs`):
  candidate text, dedup and n-best sentence assembly carry `PhraseText`;
  `crates/oxpinyin-facade/src/export_rows.rs` writes export rows through
  `phrase_text_bytes`.
- Both C facades' sentence writers and token-getters write
  `phrase_text_bytes` with the stored length; `CandidateSnapshot::intern`
  receives the prefix because the encoder stops at the zero.

The former Rust counterpart — `PhraseItemView::phrase_chars` rejecting or
retaining the zero and the C marshalling refusing an interior NUL — is
gone from these paths.

The #894 cases below cover system data only. A zero-word item in a user
library (`user.bin`, `addon.bin`) still loads through a Rust `String`
and is converted to stored words, including the NUL and suffix, with the
full declared count. Its outward text ends at that NUL, following the pin's
per-phrase cut by source reading; no #894 case exercises it. That path
is tracked in #1198.

## Cases

`tools/bisection/contract-diff.py --issue 894` selects the five former
marked cases,
`phrase-text/{pinyin,zhuyin}-nul-{candidate,sentence}` and
`phrase-text/pinyin-nul-export`, and the two
`phrase-text/{pinyin,zhuyin}-nul-choose` cases, which choose the stored
phrase, read the sentence and commit it with `train`. They compare both
sides directly and MATCH; the scalar controls and the zhuyin
export-symbol absence checks remain MATCH. The record's old binding observations and
`contract-diff-policy` block are removed; the differential, not a
recorded expectation, is the evidence.

**Verification (2026-10-24 UTC):** the bdb reference cell, 24/24 #894
cases, `MATCH`; the completion gate is recorded in the change's PR.
**Verification (2026-10-25 UTC):** with the `choose` cases, the bdb
reference cell, 32/32 #894 cases, `MATCH`.

**Prior rulings:** none reversed. The valid-input proof stands: valid
supported phrase text has no interior zero, and the change adds no
valid-input difference.
