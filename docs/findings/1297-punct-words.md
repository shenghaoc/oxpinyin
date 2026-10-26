# #1297 — Punctuation values contain whole words

- **Status:** accepted · 2026-10-26 UTC · #1297 Phase 2.
- **Disposition:** POLICY-ACCEPTED where the recorded artefact changes output; controls remain MATCH.
- **Policy limb:** language artefact under #1304 (main `ab0b1da7`), not a new product rule or a class (a), (b), or (c) exception.
- **Pin:** libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`; source read from a checkout at 074a2219 whose cited blobs were hashed against the pin. `src/storage/table_info.cpp` blob `07b20e4ee9f061251d62efd2e4c4b09e55119d26`; `utils/utils_helper.h` blob `385415f7244a131dec378ce22436ce32ac45714b`. Reference image glibc 2.41; this native Ubuntu host uses glibc 2.44. Both facades, all backends unless stated otherwise.
- **Contract preserved:** exported symbols, C layouts, ownership, allocators, persistent layouts, data ownership, and the ruled LC_NUMERIC side effect are unchanged. Existing class (b)/(c) diagnostics stay at their boundaries. The input-dependent differences below are the entire admitted scope.
- **Prior rulings:** none reversed; #923 remains in force.

`punct_table.cpp:56-94` uses pointer differences and byte-end comparison, so 1–3 bytes after a terminated run can add an empty punctuation. `row_format::punct::decode_puncts` walks only `chunks_exact(4)` words. Predictions in both facades read it through the shared data reader; system writers emit complete zero-terminated u32 words, so valid values and their order remain identical. Crafted short/misaligned values lose only the spurious empty punctuation. Stored non-scalars and unterminated whole runs keep their established boundary encoding. The existing #1144 driver still invokes prediction and visits all candidates; accepted witnesses report the raw punctuation bytes, avoiding unrelated ranking fields.

**Tests and reproduction:** use the existing `contract-diff.py` scanner selections (`--issue 919 --issue 920`), #1144 punctuation and #929 save selections on bdb, plus the touched-crate gate and #926 tool differentials. The PR reports executed results; source-derived expectations below are specified before the differential run. No captures are submitted.

**Conclusion:** accepted under the merged policy. Valid product grammar, ABI and persistent contracts remain. Runtime is linear in the input; parsing keeps borrowed header/row slices and removes the hex conversion/word-copy machinery.

```contract-diff-policy
{
  "schema": 1,
  "record": "1297-punct-words",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "punct-decode/misaligned-terminated": {
      "pin": {
        "exit": 0,
        "result": {
          "ret": true,
          "puncts": [
            "efbc8c",
            ""
          ],
          "logs": []
        },
        "stderr": ""
      },
      "subject": {
        "exit": 0,
        "result": {
          "ret": true,
          "puncts": [
            "efbc8c"
          ],
          "logs": []
        },
        "stderr": ""
      }
    },
    "punct-decode/misaligned-short": {
      "pin": {
        "exit": 0,
        "result": {
          "ret": true,
          "puncts": [
            ""
          ],
          "logs": []
        },
        "stderr": ""
      },
      "subject": {
        "exit": 0,
        "result": {
          "ret": true,
          "puncts": [],
          "logs": []
        },
        "stderr": ""
      }
    }
  }
}
```

**Named mutation (2026-10-26 UTC):** `M1297-byte-tail` — restore the byte-length loop condition; `punct-decode/misaligned-terminated` detects it (DIFFER, exit 1). Every mutated source was restored byte-for-byte. Rebuild with `just cell-artifacts --artifacts pinyin-capi,zhuyin-capi` for C-facing witnesses, then run the existing contract driver with the named `--cases`; tool and sink witnesses use `just heavy -- cargo test` with their existing test target. The restored touched-crate gate passes, and the three C-facing mutation witnesses return to their declared MATCH/POLICY-ACCEPTED outcomes.
