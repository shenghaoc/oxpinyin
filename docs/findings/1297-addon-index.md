# #1297 — Addon indexes in the library range

**Reverted — 2026-10-28 UTC (external-I/O stage 1).** The addon slot now goes
through `oxpinyin-input`, which makes the pin's own call (`atoi`, cut to a `guint8`) on
`SystemTableInfo2::load`'s or `UserTableInfo::load`'s stream
([external-io-boundary.md](external-io-boundary.md), stage 1). The cases
below run their original probes and are gated as MATCH; no case registers
this record. The record and its observations below remain historical.

- **Status:** accepted · 2026-10-26 UTC · #1297 Phase 2.
- **Disposition:** POLICY-ACCEPTED where the recorded artefact changes output; controls remain MATCH.
- **Policy limb:** language artefact under #1304 (main `ab0b1da7`), not a new product rule or a class (a), (b), or (c) exception.
- **Pin:** libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`; source read from a checkout at 074a2219 whose cited blobs were hashed against the pin. `src/storage/table_info.cpp` blob `07b20e4ee9f061251d62efd2e4c4b09e55119d26`; `utils/utils_helper.h` blob `385415f7244a131dec378ce22436ce32ac45714b`. Reference image glibc 2.41; this native Ubuntu host uses glibc 2.44. Both facades, all backends unless stated otherwise.
- **Contract preserved:** exported symbols, C layouts, ownership, allocators, persistent layouts, data ownership, and the ruled LC_NUMERIC side effect are unchanged. Existing class (b)/(c) diagnostics stay at their boundaries. The input-dependent differences below are the entire admitted scope.
- **Prior rulings:** none reversed; #923 remains in force.

`table_info.cpp:159-161,271-280` uses atoi and narrows to guint8 before the 16-slot assertion. `TableConf::parse` instead parses a complete usize and checks the library range. Default enum names and valid addon indexes 0–15 keep their slots and duplicate-row replacement. Both facades open the same file; only the pinyin API can load addon dictionaries. `256` and `abc` previously reached slot 0; now initialization refuses with the existing typed addon-index warning. A slot requirement prescribes the range, not atoi or guint8 wrap.

**Tests and reproduction:** use the existing `contract-diff.py` scanner selections (`--issue 919 --issue 920`), #1144 punctuation and #929 save selections on bdb, plus the touched-crate gate and #926 tool differentials. The PR reports executed results; source-derived expectations below are specified before the differential run. No captures are submitted.

**Conclusion:** accepted under the merged policy. Valid product grammar, ABI and persistent contracts remain. Runtime is linear in the input; parsing keeps borrowed header/row slices and removes the hex conversion/word-copy machinery.

```contract-diff-policy
{
  "schema": 1,
  "record": "1297-addon-index",
  "status": "reverted",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "table-conf-addon-index-wraps": {
      "pin": {
        "exit": 0,
        "result": {
          "ctx": true,
          "logs": []
        },
        "stderr": ""
      },
      "subject": {
        "exit": 0,
        "result": {
          "ctx": false,
          "logs": [
            [
              "libpinyin",
              16
            ]
          ]
        },
        "stderr": ""
      }
    },
    "table-conf-919-addon-atoi-nondigit": {
      "pin": {
        "exit": 0,
        "result": {
          "ctx": true,
          "logs": []
        },
        "stderr": ""
      },
      "subject": {
        "exit": 0,
        "result": {
          "ctx": false,
          "logs": [
            [
              "libpinyin",
              16
            ]
          ]
        },
        "stderr": ""
      }
    }
  }
}
```
