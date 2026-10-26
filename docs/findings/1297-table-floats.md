# #1297 — Decimal lambda grammar

- **Status:** accepted · 2026-10-26 UTC · #1297 Phase 2.
- **Disposition:** POLICY-ACCEPTED where the recorded artefact changes output; controls remain MATCH.
- **Policy limb:** language artefact under #1304 (main `ab0b1da7`), not a new product rule or a class (a), (b), or (c) exception.
- **Pin:** libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`; source read from a checkout at 074a2219 whose cited blobs were hashed against the pin. `src/storage/table_info.cpp` blob `07b20e4ee9f061251d62efd2e4c4b09e55119d26`; `utils/utils_helper.h` blob `385415f7244a131dec378ce22436ce32ac45714b`. Reference image glibc 2.41; this native Ubuntu host uses glibc 2.44. Both facades, all backends unless stated otherwise.
- **Contract preserved:** exported symbols, C layouts, ownership, allocators, persistent layouts, data ownership, and the ruled LC_NUMERIC side effect are unchanged. Existing class (b)/(c) diagnostics stay at their boundaries. The input-dependent differences below are the entire admitted scope.
- **Prior rulings:** none reversed; #923 remains in force.

`table_info.cpp:208-242` reads λ with `%f`; the Rust counterpart is `table_info::TableConf::parse`. Header load through either facade reaches the runtime, then the same λ consumers; the training segment model also calls this parser. Stock/model-generated `table.conf` files emit decimal fields. Alternate system directories and hand edits can supply hex, infinity, NaN payloads or incomplete exponents; no writer produces them. Decimal syntax (including signed exponents and decimal overflow/subnormals) still parses directly to f32; hex and named special values are refused before lookup. A language-neutral decimal grammar does not prescribe glibc extensions.

The source-history correction is retained: incomplete exponent rejection arrived in glibc **2.42**, commit `0a8e7ac95c`; the `0x.` rejection arrived in **2.43**, `b52ecff316`. See [source-history record](glib-glibc-version-sensitivity.md), N3/N4. The harness reads the native libc version, without executing a scanner probe: old hosts use the exact accepted pin observation; newer hosts keep the refusing controls as MATCH. There is no remaining host_scanf exemption for these cases.

**Tests and reproduction:** use the existing `contract-diff.py` scanner selections (`--issue 919 --issue 920`), #1144 punctuation and #929 save selections on bdb, plus the touched-crate gate and #926 tool differentials. The PR reports executed results; source-derived expectations below are specified before the differential run. No captures are submitted.

**Conclusion:** accepted under the merged policy. Valid product grammar, ABI and persistent contracts remain. Runtime is linear in the input; parsing keeps borrowed header/row slices and removes the hex conversion/word-copy machinery.

```contract-diff-policy
{
  "schema": 1,
  "record": "1297-table-floats",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "table-conf-lambda-hex": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-hex-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-hex-fraction": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-hex-fraction-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-hex-upper": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-hex-upper-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-inf": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-inf-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-nan": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-nan-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-hex-no-exponent": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-hex-no-exponent-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-infinity": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-infinity-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-nan-payload": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-nan-payload-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-hex-overflow": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-hex-overflow-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-exponent-without-digits": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-exponent-without-digits-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-hex-exponent-without-digits": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-lambda-hex-exponent-without-digits-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-exponent-sign-without-digits": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-exponent-sign-without-digits-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-hex-exponent-sign-without-digits": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-hex-exponent-sign-without-digits-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-hex-exponent-minus-without-digits": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-hex-exponent-minus-without-digits-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-hex-dot-without-digits": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    },
    "table-conf-919-lambda-hex-dot-without-digits-zhuyin": {
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
          "logs": []
        },
        "stderr": "load SCRATCH/sys-X/table.conf failed!\n"
      }
    }
  }
}
```
