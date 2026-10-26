# #1297 — Native nonfinite tool formatting

- **Status:** accepted · 2026-10-26 UTC · #1297 Phase 2.
- **Disposition:** POLICY-ACCEPTED where the recorded artefact changes output; controls remain MATCH.
- **Policy limb:** language artefact under #1304 (main `ab0b1da7`), not a new product rule or a class (a), (b), or (c) exception.
- **Pin:** libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`; source read from a checkout at 074a2219 whose cited blobs were hashed against the pin. `src/storage/table_info.cpp` blob `07b20e4ee9f061251d62efd2e4c4b09e55119d26`; `utils/utils_helper.h` blob `385415f7244a131dec378ce22436ce32ac45714b`. Reference image glibc 2.41; this native Ubuntu host uses glibc 2.44. Both facades, all backends unless stated otherwise.
- **Contract preserved:** exported symbols, C layouts, ownership, allocators, persistent layouts, data ownership, and the ruled LC_NUMERIC side effect are unchanged. Existing class (b)/(c) diagnostics stay at their boundaries. The input-dependent differences below are the entire admitted scope.
- **Prior rulings:** none reversed; #923 remains in force.

`estimate_interpolation.cpp:116-117,139` and `eval_correction_rate.cpp:201-203` print an empty mean/rate with printf %f. `format_six_decimals` uses Rust formatting; `mean_of_no_contexts` returns f64::NAN without forcing a hardware division. Finite values still have six decimals, infinity still has its native spelling, and empty contexts print NaN rather than glibc nan/-nan. The training and KMM binaries share this formatter; no shipping C export calls it. The existing empty-input differential keeps the pin nonfinite observation and asserts the Rust spelling separately. A language-neutral output rule does not prescribe a libc NaN sign/spelling.

**Tests and reproduction:** use the existing `contract-diff.py` scanner selections (`--issue 919 --issue 920`), #1144 punctuation and #929 save selections on bdb, plus the touched-crate gate and #926 tool differentials. The PR reports executed results; source-derived expectations below are specified before the differential run. No captures are submitted.

**Conclusion:** accepted under the merged policy. Valid product grammar, ABI and persistent contracts remain. Runtime is linear in the input; parsing keeps borrowed header/row slices and removes the hex conversion/word-copy machinery.

**Named mutation (2026-10-26 UTC):** `M1297-lowercase-nan` — lowercase `format_six_decimals` output; the existing live `empty_inputs_record_native_nan_spelling` differential detects it (the exact `NaN` spelling assertion fails, exit 101). Every mutated source was restored byte-for-byte. Rebuild with `just cell-artifacts --artifacts pinyin-capi,zhuyin-capi` for C-facing witnesses, then run the existing contract driver with the named `--cases`; tool and sink witnesses use `just heavy -- cargo test` with their existing test target. The restored touched-crate gate passes, and the three C-facing mutation witnesses return to their declared MATCH/POLICY-ACCEPTED outcomes.
