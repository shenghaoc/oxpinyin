# 894-nul-phrase-text — embedded zero in stored phrase text

- **Status:** accepted under the existing category; verification pending · 2026-10-24 UTC · issue #894.
- **Disposition:** POLICY-ACCEPTED (never MATCH).
- **Policy limb:** language/runtime quirk, limb 3. No transcendental math,
  invalid memory access or pin assertion makes this class (a), (b) or (c).
- **Pin:** libpinyin 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`,
  clean `/workspace/libpinyin-pin`; both facades, all backends. GLib
  **2.89.4-2**, source package and patches verified as described in the
  [non-scalar record](894-non-scalar-phrase-text.md). The Debian patch
  series changes no UTF conversion code.
- **Pin derivation:** `glib/gutf8.c:1035-1038` stops at a zero even with
  an explicit positive length. `:1054-1060` encodes only that prefix.
  This is unchanged from the issue's 2.80.0 description, with corrected
  source citations.
- **Rust counterpart:** `oxpinyin-data/src/phrase_library.rs`,
  `PhraseItemView::phrase_chars`, accepts zero as a Rust `char` and
  continues through the declared item length. Collection produces the
  full string, including zero and its suffix.

## Forward trace and consumers

The loaders and `PhraseItem::get_phrase_string`
(`src/storage/phrase_index.cpp:103-111`) copy the stored words. Phonetic
parsing and the separate index reach the item without checking text.
For `[U+7532, U+0000, U+4E59]` (甲, zero, 乙):

| Consumer | Pin | Current oxpinyin |
| --- | --- | --- |
| Token text and length | `_token_get_phrase`, `pinyin.cpp:1627-1645` / `zhuyin.cpp:1096-1113`, reports stored length 3 and allocated text 甲 | `dict.rs` token introspection holds all 3 chars; `*_token_get_phrase` writes length 3, then `owned_cstr` refuses interior NUL, writes NULL and returns false. A length-only call succeeds. |
| Candidate length/ranking | `_compute_phrase_length`, `pinyin.cpp:1953-1992` / `zhuyin.cpp:1294-1317`, uses stored length 3, independently of truncated display text. Count/reading-based ranking still prices the full item. | `PhraseLibraries::phrase_char_count` / `trellis_item`, and `session/lookup.rs` rank keys count all 3 chars, including zero and suffix. The initial length rank agrees; it is not computed from the visible prefix. |
| Candidate dedup | `pinyin.cpp:2041-2138` / `zhuyin.cpp:1348-1416` compares converted C strings, so 甲, 甲-zero-乙 and 甲-zero-丙 can collide; frequency/NBEST rules choose the survivor | `session/mod.rs`, `dedup_ranked_by_text` and `dedup_by_text_keep_first`, compare full Rust strings. Those texts are distinct, so retained membership can differ before C marshalling. |
| Borrowed candidate string | `_compute_phrase_strings_of_items`, `pinyin.cpp:1995-2038` / `zhuyin.cpp:1320-1344`, exposes 甲, with the normal borrowed lifetime | Both facade candidate builders call `CandidateSnapshot::intern` (`oxpinyin-facade/src/snapshot.rs`), which refuses any interior NUL and drops that row. It does **not** hand out a longer allocation merely truncated by the C caller. |
| Sentence selection/assembly | `phonetic_lookup.h:643-695` prices length/counts, then `lookup.cpp:27-69` converts each selected token separately and concatenates its prefix. The next token survives: `[甲,0,乙]` followed by 丙 renders 甲丙. | `nbest.rs`, `token_text`/`text`, concatenates the complete Rust text: 甲-zero-乙丙. N-best dedup and later selection see that full text. Both `sentence.rs` get functions use `write_owned_sentence!`, whose `owned_cstr` refuses NUL, writes NULL, returns false. |
| Phrase export | `pinyin.cpp:662-768` probes the item, returns prefix 甲 and all three pronunciation syllables/count, then advances | `ExportRow::text_len` and writers retain all chars; `capi/iterators.rs` -> `malloc_cstr_with` refuses interior NUL, so true/NULL phrase/intact full reading/count. Cursor still advances. |
| Zhuyin export | No symbol in `zhuyin.h` or `libzhuyin.ver` | No symbol. Cases explicitly check absence, MATCH. |

The candidate false/unreadable warning from the non-scalar record is not
used for this case: zero decodes successfully. An isolated zero-containing
candidate is dropped during snapshot construction while guess succeeds.
For sentence get, the difference requires a non-NULL out pointer; a NULL
output does not allocate and can still return true. The source record
does not treat allocation failure as evidence for this quirk.

## Backward trace and valid-input proof

Candidate strings come from normal/system, addon, longer and pinyin
predicted items through the two `_token_get_phrase` helpers, or from the
sentence path for NBEST. Prefix prediction can begin beyond a stored
zero (`pinyin.cpp:2018-2022`); that slice may itself be NUL-free. A slice
starting before zero is subject to this record. Punctuation is separate
(#1144). Candidate choice keeps token/readings/constraints, not a repaired
copy of the visible prefix; addon promotion can copy the malformed item.

Sentence getters consume the stored n-best result only (`pinyin.cpp:1464-1481`,
`zhuyin.cpp:983-998`). Parsing and prefix/phrase lookup select tokens;
they do not manufacture a zero inside valid phrase text. `convert_to_utf8`
is shared by both facades. Rust's trellis and sentence cache retain the
full scalar text until C marshalling. A changed dedup survivor, sentence
or trained token remains a consequence of the original malformed table.

Export probes system items, overridden system items and user items. The
common `PhraseExportCursor`/`ExportRow` covers these producers.
`ContextCore::export_phrases` can retain the full Rust string; the standalone
dictionary tool is not a supported Rust library interface. Bigram export
converts predecessor and successor separately (`pinyin.cpp:822-830`) and
joins their prefixes; its independent vector-overread exception (row 1)
is not admitted here. Native save preserves phrase words and count changes;
this work does not rewrite data to prefix text.

The text producers are system/addon chunks, user chunks and their journals;
C import, pinyin remember, and table generation. The latter three all
convert NUL-terminated input with `g_utf8_to_ucs4(...,-1,...)`:
`pinyin.cpp:614-652,3669-3710`, `zhuyin.cpp:500-537`,
`phrase_index.cpp:522-555`. GLib `gutf8.c:952-974` stops before zero and
`:987-990` writes a terminator excluded from `items_written`.
`set_phrase_string` copies exactly the supplied phrase length
(`phrase_index.cpp:108-111`), excluding that terminator. Rust C imports
start from `CStr`, so cannot introduce an interior zero either. Training
changes counts/readings, and selection and save propagate existing items;
none manufactures an interior zero from a valid item. All three backends
store the same phrase chunk encoding; their KV implementation does not
change this argument.

**Input partition:** valid supported phrase data has complete, bounded
items with nonzero scalar text and matching readings. Zero is a Unicode
scalar, but an interior zero in the declared phrase body cannot be
produced by the valid C/text-table producers above. A crafted/corrupted
chunk, user item or journal containing it is malformed phrase data.
A C string with bytes after its terminator only supplies its prefix;
it cannot supply those bytes as additional phrase characters. Empty,
overlong, truncated or invalid-reading items keep their separate rules.

**Why accepted:** GLib applies a C-terminator rule to a length-delimited
UCS-4 buffer; Rust decodes its explicit length and applies normal C-string
validation later. Every difference above requires that malformed table
state; no valid supported input changes. **Contract preserved:** no C
symbol/layout, ownership or lifetime changes; borrowed candidates stay
borrowed, returned strings remain `g_free`-compatible, real `GArray`
ownership is preserved. Native system/user formats, data ownership,
process-wide effects and diagnostics stay unchanged. The observed NULL,
false and omitted rows are explicit record contents, not hidden parity.

## Tests and methodology

`tools/bisection/contract-diff.py --issue 894` selects
`phrase-text/{pinyin,zhuyin}-nul-{candidate,sentence,export}` and the
ordinary `*-valid-*` controls. The malformed candidate/sentence paths
and pinyin export are POLICY-ACCEPTED; zhuyin export absence is MATCH.
The sentence fixture includes a following token to expose per-phrase
truncation versus whole-string refusal. Raw bytes, returns, NULL versus
untouched outputs, readings/counts and logs are compared with the current
record's exact observations on **both** sides. No result is learned by
running the oracle. Marker negative tests require missing/stale records
and unexpected observations to fail. The named subject mutation and
reference-cell results are reported after verification; raw output stays
ephemeral. **Prior rulings:** none reversed. **Conclusion:** retain normal
Rust behavior under limb 3; source classification complete, verification
pending (2026-10-24 UTC).
