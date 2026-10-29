# #1297 — Nonconforming decimal markers reset profiles

**Reverted — 2026-10-28 UTC (external-I/O stage 1).** The `user.conf` decimal fields now goes
through `oxpinyin-input`, which makes the pin's own call (`fscanf("%d")`) on
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

`table_info.cpp:338-359,399-412`, `pinyin.cpp:172-218` and `zhuyin.cpp:126-163` load the marker, compare versions, then keep or clean the profile. `user_files::UserTableInfo::parse` uses complete i32 fields, and `persistence::parse_user_conf` treats a malformed field as an absent/nonconforming marker. The same save and clean paths remain. Pin-generated markers are always in range; hand-edited versions increased by 2^32 can wrap back to conform on the pin; out-of-range counters can wrap to zero. Rust resets the profile instead. Missing counters still default to zero; valid negative counters remain signed. All marker producers (`conform_to`, save, fini, both facades) retain the signed persistent grammar. No other caller bypasses the shared parser.

**Tests and reproduction:** use the existing `contract-diff.py` scanner selections (`--issue 919 --issue 920`), #1144 punctuation and #929 save selections on bdb, plus the touched-crate gate and #926 tool differentials. The PR reports executed results; source-derived expectations below are specified before the differential run. No captures are submitted.

**Conclusion:** accepted under the merged policy. Valid product grammar, ABI and persistent contracts remain. Runtime is linear in the input; parsing keeps borrowed header/row slices and removes the hex conversion/word-copy machinery.

```contract-diff-policy
{
  "schema": 2,
  "record": "1297-user-decimals",
  "status": "reverted",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "user-conf-920-version-overflow-conforms": {
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
    "user-conf-920-version-overflow-conforms-zhuyin": {
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
    "user-conf-920-counter-overflow-conforms": {
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
    "user-conf-920-counter-overflow-conforms-zhuyin": {
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

**Named mutation (2026-10-26 UTC):** `M1297-counter-default` — replace the invalid-counter error with `.unwrap_or(0)`; `user-conf-920-counter-overflow-conforms` detects it (DIFFER, exit 1). Every mutated source was restored byte-for-byte. Rebuild with `just cell-artifacts --artifacts pinyin-capi,zhuyin-capi` for C-facing witnesses, then run the existing contract driver with the named `--cases`; tool and sink witnesses use `just heavy -- cargo test` with their existing test target. The restored touched-crate gate passes, and the three C-facing mutation witnesses return to their declared MATCH/POLICY-ACCEPTED outcomes.
