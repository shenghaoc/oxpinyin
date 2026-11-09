# #1297 — Punctuation values contain whole words

- **Status:** accepted · 2026-10-26 UTC · #1297. The external-I/O ruling keeps this record.
- **Disposition:** POLICY-ACCEPTED where the recorded artefact changes output; controls remain MATCH.
- **Policy limb:** [internal language artefact](compatibility-policy.md#internal-language-artefacts) (#1304, main `ab0b1da7`). It is not a product rule and not a class (a) or (b) exception. The read past an unterminated run is a separate class (b) entry, [Unterminated punctuation value over-read](upstream-divergences.md#unterminated-punctuation-value-over-read).
- **Pin:** libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`; source read from a checkout at 074a2219 whose cited blobs were hashed against the pin. `src/storage/table_info.cpp` blob `07b20e4ee9f061251d62efd2e4c4b09e55119d26`; `utils/utils_helper.h` blob `385415f7244a131dec378ce22436ce32ac45714b`. Reference image glibc 2.41; this native Ubuntu host uses glibc 2.44. Both facades, all backends unless stated otherwise.
- **Contract preserved:** exported symbols, C layouts, ownership, allocators, persistent layouts, data ownership, and the ruled LC_NUMERIC side effect are unchanged. Existing class (b) diagnostics stay at their boundaries. The input-dependent differences below are the entire admitted scope.
- **Prior rulings:** none reversed; #923 remains in force.

`punct_table.cpp:56-94` uses pointer differences and byte-end comparison, so 1–3 bytes after a terminated run can add an empty punctuation. `row_format::punct::decode_puncts` walks only `chunks_exact(4)` words. Predictions in both facades read it through the shared data reader; system writers emit complete zero-terminated u32 words, so valid values and their order remain identical. Crafted short/misaligned values lose only the spurious empty punctuation. Stored non-scalars and unterminated whole runs keep their established boundary encoding. The existing #1144 driver still invokes prediction and visits all candidates; accepted witnesses report the raw punctuation bytes, avoiding unrelated ranking fields.

**Tests:** `row_format` unit test `punct_stream_ignores_partial_words` holds the whole-word walk; `punct_stream_ends_an_unterminated_run` and `punct_stream_keeps_non_scalar_words` hold the neighbouring partitions. The `punct-decode/*` cases are recorded evidence; the accepted cases are listed below.

**Conclusion:** accepted under the merged policy. Valid product grammar, ABI and persistent contracts remain. Runtime is linear in the input; parsing keeps borrowed header/row slices and removes the hex conversion/word-copy machinery.

Accepted cases, on every cell: `punct-decode/misaligned-short` and `punct-decode/misaligned-terminated`. In each, the pin returns one more empty punctuation from the 1–3 trailing bytes (`punct_table.cpp:56-94`) and oxpinyin does not (`row_format::punct::decode_puncts`); every other punctuation is identical.

**Named mutation:** `M1297-byte-tail` restores the byte-length loop condition; `punct-decode/misaligned-terminated` detected it (DIFFER) in the recorded evidence.
