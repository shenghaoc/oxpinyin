# #1297 — Configuration line boundaries

**Reverted — 2026-10-28 UTC (external-I/O stage 1).** The `table.conf` header stream now goes
through `oxpinyin-input`, which makes the pin's own call (`fscanf with the pin's ordered formats`) on
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

`table_info.cpp:208-233,254-260` uses scanner whitespace directives and six conversions. `ConfigLines` instead reads nonempty lines and exact directive names; row fields are unlimited complete words and belong to their own line. Incomplete rows are ignored without consuming another row; complete rows validate their target, and comment lines are skipped. Stock/config-generated files preserve their meaning; a one-line header is rejected, and leading indentation is accepted consistently. The pin rejects indentation before its first literal and accepts cross-line/collapsed headers. Scanner width, literal-whitespace matching and stream consumption do not define a language-neutral file grammar. Filename C-boundary cuts and joins remain unchanged; overlong words are no longer split at byte 255.

**Tests and reproduction:** use the existing `contract-diff.py` scanner selections (`--issue 919 --issue 920`), #1144 punctuation and #929 save selections on bdb, plus the touched-crate gate and #926 tool differentials. The PR reports executed results; source-derived expectations below are specified before the differential run. No captures are submitted.

**Conclusion:** accepted under the merged policy. Valid product grammar, ABI and persistent contracts remain. Runtime is linear in the input; parsing keeps borrowed header/row slices and removes the hex conversion/word-copy machinery.

```contract-diff-policy
{
  "schema": 2,
  "record": "1297-table-lines",
  "status": "reverted",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "table-conf-919-header-one-line": {
      "pin": {
        "sha256": "096171703e12a07648d9b811395629efc91d77ae4eb46936e25151db1001429a",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.ctx": "b5bea41b6c623f7c",
          "result.logs": "4f53cda18c2baa0c",
          "stderr": "12ae32cb1ec02d01"
        }
      },
      "subject": {
        "sha256": "c302191f2c479bf4048203961481c53e3c988e4878cc1b9293c31e440782fcb2",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.ctx": "fcbcf165908dd18a",
          "result.logs": "4f53cda18c2baa0c",
          "stderr": "4fb43233d5f48d1b"
        }
      },
      "divergence": [
        "result.ctx: pin true; subject false",
        "stderr: pin \"\"; subject \"load SCRATCH/sys-X/table.conf failed!\\n\""
      ]
    },
    "table-conf-919-header-leading-space": {
      "pin": {
        "sha256": "c302191f2c479bf4048203961481c53e3c988e4878cc1b9293c31e440782fcb2",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.ctx": "fcbcf165908dd18a",
          "result.logs": "4f53cda18c2baa0c",
          "stderr": "4fb43233d5f48d1b"
        }
      },
      "subject": {
        "sha256": "096171703e12a07648d9b811395629efc91d77ae4eb46936e25151db1001429a",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.ctx": "b5bea41b6c623f7c",
          "result.logs": "4f53cda18c2baa0c",
          "stderr": "12ae32cb1ec02d01"
        }
      },
      "divergence": [
        "result.ctx: pin false; subject true",
        "stderr: pin \"load SCRATCH/sys-X/table.conf failed!\\n\"; subject \"\""
      ]
    },
    "table-conf-919-header-one-line-zhuyin": {
      "pin": {
        "sha256": "096171703e12a07648d9b811395629efc91d77ae4eb46936e25151db1001429a",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.ctx": "b5bea41b6c623f7c",
          "result.logs": "4f53cda18c2baa0c",
          "stderr": "12ae32cb1ec02d01"
        }
      },
      "subject": {
        "sha256": "c302191f2c479bf4048203961481c53e3c988e4878cc1b9293c31e440782fcb2",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.ctx": "fcbcf165908dd18a",
          "result.logs": "4f53cda18c2baa0c",
          "stderr": "4fb43233d5f48d1b"
        }
      },
      "divergence": [
        "result.ctx: pin true; subject false",
        "stderr: pin \"\"; subject \"load SCRATCH/sys-X/table.conf failed!\\n\""
      ]
    },
    "table-conf-919-header-leading-space-zhuyin": {
      "pin": {
        "sha256": "c302191f2c479bf4048203961481c53e3c988e4878cc1b9293c31e440782fcb2",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.ctx": "fcbcf165908dd18a",
          "result.logs": "4f53cda18c2baa0c",
          "stderr": "4fb43233d5f48d1b"
        }
      },
      "subject": {
        "sha256": "096171703e12a07648d9b811395629efc91d77ae4eb46936e25151db1001429a",
        "fields": {
          "exit": "5feceb66ffc86f38",
          "result.ctx": "b5bea41b6c623f7c",
          "result.logs": "4f53cda18c2baa0c",
          "stderr": "12ae32cb1ec02d01"
        }
      },
      "divergence": [
        "result.ctx: pin false; subject true",
        "stderr: pin \"load SCRATCH/sys-X/table.conf failed!\\n\"; subject \"\""
      ]
    }
  }
}
```
