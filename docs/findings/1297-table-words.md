# #1297 — Complete table words and exact enum tokens

**Reverted — 2026-10-28 UTC (external-I/O stage 1).** The `table.conf` words now goes
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

`table_info.cpp:228-233,255-286` reads `%255s` words and maps them with C-string comparisons. `TableConf::parse` uses complete unbounded byte fields and exact enum tokens; only filename bytes retain their C-boundary NUL cut and path joining. Both facade initializers and the training model loader share the parser. System generators emit short canonical tokens and filenames, while hand edits can carry NUL suffixes or words over 255 bytes. A known source/database/target/index/file-type token followed by NUL and suffix used to alias the known token; it now fails exact token validation. Long filenames stay one field instead of being split into subsequent columns. No generated file needs the C buffer width or strcmp alias. C ABI ownership and persistent encodings remain; the existing driver keeps the header-token and overlong-word fixtures. Exact enum failures retain their boundary warning. Unit `a_long_filename_stays_one_field` checks the width removal independently of filesystem component limits.

**Forward trace:** custom system path -> both facade initializers -> shared TableConf -> enum/filename fields -> runtime library loads. Training's model loader reads the same TableConf. **Backward trace:** all token/filename producers are the system data generator or a caller-provided table.conf; the marker writer independently emits canonical backend names and is covered by the user-token record. No backend parser bypasses these fields.

**Affected inputs and tests:** canonical tokens remain MATCH; the source/database NUL fixtures below are POLICY-ACCEPTED. C-string filename semantics remain the path-boundary contract. The two `table-conf-abort-919-word-width-overlong` cases (pinyin and `-zhuyin`) append one 300-byte word and four other words. The pin cuts that word into 255 and 45 bytes, creating six fields and aborting on the first target. Rust reads all 300 bytes as one word: the resulting five-field row is incomplete and ignored, so initialization succeeds without a GLib warning. The original empty-profile fixture then prints its ordinary missing-user.conf diagnostic; pinyin writes the canonical version/backend marker with counter 1, while zhuyin leaves the directory empty. The pin aborts before that profile step. The correction checks the complete row’s field count before validating its target; it does not restore a width cut. Both differences are POLICY-ACCEPTED under this record. The width unit test independently proves a 300-byte filename stays intact.

**Sensitivity:** `M1297-short-row-target` restored target refusal before the field-count check. Both unchanged overlong fixtures became DIFFER (pin SIGABRT, subject false with one warning), exit 1. Restoring the source byte-for-byte returns the whole-word path. The record checks the complete exit, result, stderr and per-cell profile listing; a signal other than the registered pin SIGABRT is not accepted.

**Conclusion:** accepted language artefact; parsing remains O(input length), with borrowed row fields. Reproduce with the existing contract driver `--issue 919` and the touched-crate tests. The PR reports executed gates; no captures are submitted.

```contract-diff-policy
{
  "schema": 1,
  "record": "1297-table-words",
  "status": "reverted",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "table-conf-919-source-token-nul": {
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
    "table-conf-919-source-token-nul-zhuyin": {
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
              "libzhuyin",
              16
            ]
          ]
        },
        "stderr": ""
      }
    },
    "table-conf-919-database-token-nul": {
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
    "table-conf-919-database-token-nul-zhuyin": {
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
              "libzhuyin",
              16
            ]
          ]
        },
        "stderr": ""
      }
    },
    "table-conf-abort-919-word-width-overlong": {
      "cells": {
        "bdb": {
          "pin": {
            "exit": -6,
            "result": null,
            "stderr": "",
            "userfiles": {}
          },
          "subject": {
            "exit": 0,
            "result": {
              "ret": true,
              "logs": []
            },
            "stderr": "open SCRATCH/user-X/user.conf failed.",
            "userfiles": {
              "user.conf": "binary format version:7|model data version:14|database format:BerkeleyDB|open counter:1|"
            }
          }
        },
        "kc": {
          "pin": {
            "exit": -6,
            "result": null,
            "stderr": "",
            "userfiles": {}
          },
          "subject": {
            "exit": 0,
            "result": {
              "ret": true,
              "logs": []
            },
            "stderr": "open SCRATCH/user-X/user.conf failed.",
            "userfiles": {
              "user.conf": "binary format version:7|model data version:14|database format:KyotoCabinet|open counter:1|"
            }
          }
        },
        "tkrzw": {
          "pin": {
            "exit": -6,
            "result": null,
            "stderr": "",
            "userfiles": {}
          },
          "subject": {
            "exit": 0,
            "result": {
              "ret": true,
              "logs": []
            },
            "stderr": "open SCRATCH/user-X/user.conf failed.",
            "userfiles": {
              "user.conf": "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|"
            }
          }
        }
      }
    },
    "table-conf-abort-919-word-width-overlong-zhuyin": {
      "pin": {
        "exit": -6,
        "result": null,
        "stderr": "",
        "userfiles": {}
      },
      "subject": {
        "exit": 0,
        "result": {
          "ret": true,
          "logs": []
        },
        "stderr": "open SCRATCH/user-X/user.conf failed.",
        "userfiles": {}
      }
    }
  }
}
```
