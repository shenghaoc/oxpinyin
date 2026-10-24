# #918 — a matching raw non-scalar key returns a segment token

- **Status:** identical, runtime-gated · 2026-10-24 UTC · #918.
- **Disposition:** MATCH; no exception and no POLICY-ACCEPTED case.
- **Policy:** the C-facing contract is preserved. The pin's observable
  result is reproduced in natural Rust: the input is decoded to raw `u32`
  words and compared with the stored raw key. This is not a
  language/runtime quirk, a class (a)–(c) case, or a Stage 3 departure.
- **Pin:** libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`, GLib
  2.89.4-2/glibc 2.43-3 reference environment; both facades.
- **Rust:** both facade `phrase.rs::segment_text` and
  `*_phrase_segment`, `oxpinyin_core::ucs4::decode`,
  `Dictionary::tokens_for_ucs4`, the system and user phrase-table raw
  reads, and `engine/src/phrase.rs::phrase_segment_ucs4`.

## Exact bounded trigger and derivation

Input `ed a0 80 00` encodes U+D800 under the pin's historical converter.
Let a loaded library's phrase table contain raw UCS-4 key `[0x0000d800]`
mapping to one nonzero enabled token T. Its item has declared length one,
bounded storage, positive unigram frequency and a positive library total;
the phrase-table and item lengths agree. Predecessor grams are absent or
sound, λ is normal, and allocations/reads succeed. No other path competes.

Both exports convert one word (`pinyin.cpp:1446-1460`,
`zhuyin.cpp:965-979`). Each backend searches the stored raw word bytes,
not Unicode scalar text (`storage/phrase_large_table3_bdb.cpp:180-207`,
`storage/phrase_large_table3_kyotodb.cpp:164-193`,
`storage/phrase_large_table3_tkrzwdb.cpp:124-153`).
`PhraseTableEntry::search` emits enabled T
(`storage/phrase_large_table3.cpp:29-51`).
`PhraseLookup::unigram_gen_next_step` reads item length/frequency, not its
text, and saves the only final node (`lookup/phrase_lookup.cpp:305-323,348-377`).
`final_step` returns true with `[T]` (`:382-428`). The getter reports one
token and T. There is no warning or persistent update.

The Rust facade decodes a Rust-invalid but historically valid byte
sequence with `oxpinyin_core::ucs4::decode`. That plain decoder accepts
canonical 1–6 byte forms, including surrogates, values above U+10FFFF and
the five/six-byte encodings through `0x7fff_ffff`, and rejects overlong,
truncated or otherwise malformed forms. It builds only `u32` words: no
`char` or `str` is created, and no GLib converter is called on this
branch. `Dictionary::tokens_for_ucs4` reads the system and user phrase
tables with the raw key; the segmenter then reads the item's declared
length and unigram frequency, which are header fields, not text. The
result is the pin's true/`[T]`, with the same getter outputs and silence.

## Backward trace and preserved contract

The only result producers are allocation, segment lookup and reset, with
getters/free as traced in the [entry audit](918-phrase-segment-source-audit.md).
Raw keys/items can originate in loaded system/addon/user data, journals,
or the pin's GLib-accepting imports; #894 traces those producers. The raw
path changes no producer. ABI/layout, output ownership and on-disk
representations are unchanged; it writes no backing data.

Valid scalar input still takes the ordinary `&str` path. Empty input,
GLib-rejected bytes, short/partial grams, inconsistent lengths, bad token
bounds, IO errors, allocation failures and caller lifetime violations
keep their separate records. Nothing here admits them.

## Verification

`tools/bisection/contract-diff.py` registers
`phrase-segment-nonscalar-raw-key-{pinyin,zhuyin}`. Each builds a private
system `gb_char.bin` item with text `[0xD800]`, a phrase key `[0xD800]`
mapped to token `0x01000001`, and calls `phrase_segment` with
`ed a0 80 00`; both sides answer true with that token, no warning, and
the old result replaced. The bdb reference gate and the kc and tkrzw
cells each ran 10/10 MATCH. The
named raw-lookup mutation made both short-gram cases and both raw-key
cases DIFFER and the six controls MATCH; restored, the same ten cases
returned 10/10 MATCH.

Source settles the bounded path; the runtime case is the gate. Conclusion:
**identical; no exception, no quirk admission.** The separate
`*_token_get_phrase` text conversion for a stored value
`>= 0x80000000` is still a malformed-item difference and is not admitted
by this record.
