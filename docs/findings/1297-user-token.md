# #1297 — Exact database marker tokens

**Reverted — 2026-10-28 UTC (external-I/O stage 1).** The `user.conf` database token now goes
through `oxpinyin-input`, which makes the pin's own call (`fscanf("%255s")` and `strcmp`) on
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

`table_info.cpp:122-131,352-354` compares a scanf token with strcmp. A known database name followed by NUL and junk therefore conforms on the pin. `UserTableInfo::parse` compares complete tokens; that hand-edited marker is malformed and the shared persistence path resets the profile. Pin and Rust marker writers emit exact names without NUL. Unknown ordinary names retain their existing class (c) refusal. Both facade readers and all backend names share this rule; no persistent format changes.

**Tests and reproduction:** use the existing `contract-diff.py` scanner selections (`--issue 919 --issue 920`), #1144 punctuation and #929 save selections on bdb, plus the touched-crate gate and #926 tool differentials. The PR reports executed results; source-derived expectations below are specified before the differential run. No captures are submitted.

**Conclusion:** accepted under the merged policy. Valid product grammar, ABI and persistent contracts remain. Runtime is linear in the input; parsing keeps borrowed header/row slices and removes the hex conversion/word-copy machinery.

```contract-diff-policy
{
  "schema": 2,
  "record": "1297-user-token",
  "status": "reverted",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "user-conf-920-nul-database-token": {
      "pin": {
        "sha256": "249a1d365fcb33cd660be5ccb26cc109fc64b9a8465c939ffd0aeba6d5d651d0",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.import-retained": "b5bea41b6c623f7c",
          "result.logs": "4f53cda18c2baa0c",
          "result.reopen": "b5bea41b6c623f7c",
          "result.trained-retained": "b5bea41b6c623f7c",
          "stderr": "12ae32cb1ec02d01"
        }
      },
      "subject": {
        "sha256": "231ad5fe14c54bf5e71458f116064788245c8c8212c22adc407dba126f6418a4",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.import-retained": "fcbcf165908dd18a",
          "result.logs": "4f53cda18c2baa0c",
          "result.reopen": "b5bea41b6c623f7c",
          "result.trained-retained": "fcbcf165908dd18a",
          "stderr": "12ae32cb1ec02d01"
        }
      },
      "divergence": [
        "result.import-retained: pin true; subject false",
        "result.trained-retained: pin true; subject false"
      ]
    },
    "user-conf-920-nul-database-token-zhuyin": {
      "pin": {
        "sha256": "249a1d365fcb33cd660be5ccb26cc109fc64b9a8465c939ffd0aeba6d5d651d0",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.import-retained": "b5bea41b6c623f7c",
          "result.logs": "4f53cda18c2baa0c",
          "result.reopen": "b5bea41b6c623f7c",
          "result.trained-retained": "b5bea41b6c623f7c",
          "stderr": "12ae32cb1ec02d01"
        }
      },
      "subject": {
        "sha256": "231ad5fe14c54bf5e71458f116064788245c8c8212c22adc407dba126f6418a4",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.import-retained": "fcbcf165908dd18a",
          "result.logs": "4f53cda18c2baa0c",
          "result.reopen": "b5bea41b6c623f7c",
          "result.trained-retained": "fcbcf165908dd18a",
          "stderr": "12ae32cb1ec02d01"
        }
      },
      "divergence": [
        "result.import-retained: pin true; subject false",
        "result.trained-retained: pin true; subject false"
      ]
    }
  }
}
```
