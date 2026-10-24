# #918 — a matching raw non-scalar key can return a segment token

- **Status:** source-established difference; proposed exception, not
  admitted or runtime-gated · 2026-10-24 UTC · #918.
- **Disposition:** no POLICY-ACCEPTED case registered by this audit.
- **Policy limb:** eligible for the language/runtime-quirk limb on the
  bounded non-scalar partition below. Not (a): no competing scores or
  last-bit decision is needed. Not (b) or (c): the witness needs no
  out-of-bounds access or assertion. This is separate from #894's text
  output records and requires its own admission before a replacement.
- **Pin:** libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`, GLib
  2.89.4-2/glibc 2.43-3 reference environment; both facades/all backends.
- **Rust:** both facade `phrase.rs::segment_text` and
  `*_phrase_segment`, `SegmentText::Unmatched`, at baseline
  `1c5d96b9605a3e5f1caef3a79fbb653707fd031f`.

## Exact bounded trigger and derivation

Input `ed a0 80 00` encodes U+D800 under GLib's historical converter.
Let a loaded library's phrase table contain raw UCS-4 key `[0x0000d800]`
mapping to one nonzero enabled token T. Its item has declared length one,
bounded storage, positive unigram frequency and a positive library total;
the phrase-table and item lengths agree. Keep predecessor grams absent
or sound, choose a normal lambda with a positive unigram contribution,
and allow allocations/reads to succeed. No other path competes.

Both exports convert one word (`pinyin.cpp:1446-1460`,
`zhuyin.cpp:965-979`; GLib `gutf8.c:665-747,940-997`). Each backend
searches raw word bytes, not Unicode scalar text
(`storage/phrase_large_table3_bdb.cpp:180-207`,
`storage/phrase_large_table3_kyotodb.cpp:164-193`,
`storage/phrase_large_table3_tkrzwdb.cpp:124-153`).
`PhraseTableEntry::search` emits enabled T
(`storage/phrase_large_table3.cpp:29-51`).
`PhraseLookup::unigram_gen_next_step` reads item length/frequency,
not its text, and saves the only final node
(`lookup/phrase_lookup.cpp:305-323,348-377`).
`final_step` returns true with `[T]` (`:382-428`). The getter reports
one token and T. There is no warning or persistent update.

Rust's successful GLib conversion is still invalid as Rust `str`;
`Unmatched(1)` replaces the old result with `[0]` and returns false.
Its getter reports one token and zero. Both paths free the conversion
allocation with real `g_free`; getters do not convert item text.

## Backward trace and preserved contract

The only result producers are allocation, segment lookup and reset,
with getters/free as traced in the [entry audit](918-phrase-segment-source-audit.md).
Raw keys/items can originate in loaded system/addon/user data, journals,
or the pin's GLib-accepting imports; #894 traces those producers but does
not classify this token output. Scalar imports/training do not create
non-scalars. ABI/layout, output ownership and on-disk representations
are unchanged by the current shortcut; it writes no backing data.

This proposed exception cannot change a valid scalar-input result:
the affected branch requires Rust-invalid input that GLib accepted.
Empty/scalar/rejected-byte controls retain their separate lifecycle.
Inconsistent lengths, bad token bounds, short/partial grams, IO errors,
allocation failures and caller lifetime violations are explicitly excluded.
Their defects/exceptions cannot be admitted through this record.

## Verification and conclusion

Source proof only; no fixture was executed. An eventual case belongs in
the existing contract driver, for both facades, with pin `true/[T]` and
subject `false/[0]`, old-result replacement, silent diagnostics, getters,
and scalar/sound-dictionary controls. The case must keep both complete
observations and use POLICY-ACCEPTED only after this record is admitted.
There is no case ID or fabricated successful gate in this checkpoint.

Reproduce the reading with `git show 074a2219:src/lookup/phrase_lookup.cpp`
and the three cited backend search files. Source settles the bounded
path; no binary question remained. Conclusion: **difference established,
separate proposed runtime quirk; keep current code; no removal cleared**.
