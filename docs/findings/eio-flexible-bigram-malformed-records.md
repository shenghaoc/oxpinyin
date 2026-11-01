# FlexibleBigram malformed records (class (b), 2026-11-01 UTC)

**Class (b), registered by name.** The pin's three `FlexibleBigram`
implementations (`src/storage/flexible_ngram_bdb.h:165-187`,
`flexible_ngram_kyotodb.h:192-216`, `flexible_ngram_tkrzwdb.h:200-225` at
074a2219) hand a whole stored value to `FlexibleSingleGram`, which treats
the first `sizeof(ArrayHeader)` bytes as a header and every following
`sizeof(ArrayItemWithToken)` chunk as an item. A value shorter than one
header, or a value whose tail is not a whole number of items, is read
past its end when the pin later asks for an item or copies the chunk.
The pin's `get_array_header` asserts only when its backend reports a
shorter partial read (`flexible_ngram_bdb.h:388`,
`flexible_ngram_kyotodb.h:371`, `flexible_ngram_tkrzwdb.h:388`).

oxpinyin answers `StoreError` from `SpecGram::decode` for such a value
(`crates/oxpinyin-store/src/flexible_ngram.rs`), before any caller sees a
header or item. The pin's abort remains for a short array-header read;
the undefined tail read is not reproduced. A tool running on a corrupted
training container therefore reports a store error instead of reading
outside the record.

Only a malformed or corrupted container reaches this path; a container
written by either implementation has whole headers and items. No
tool-diff case plants such a file, so the difference is recorded here
rather than bound to a case.
