# #1297 — Decimal lambda grammar

**Reverted — 2026-10-28 UTC.** This record is one of the revert targets in
[external-io-boundary.md](external-io-boundary.md): external behaviour must
match libpinyin on the same system, and refusing hexadecimal λ, infinity and
NaN was a defect. That change was an interim fix for the lambda field
only. Stage 1 of that plan (2026-10-28 UTC) moved the call into
`oxpinyin-input`: `TableConf::load` scans the whole `table.conf` stream with
the pin's `fscanf` calls, `"lambda parameter:%f\n"` among them, for both
facades and the tools alike, and the interim `sscanf` reader is gone. The lambda cases below no longer have active
`POLICY-ACCEPTED` registrations. Incomplete exponents and `0x.` also require
MATCH on every host; no libc-version branch selects an exemption. The older
record and observations below remain historical.

Source derivation for this ruling:

- Pin `074a2219c90feaf962d0d24f034514033ece5f99`, read with `git show` from
  `/home/sheng/repos/libpinyin`: `src/storage/table_info.cpp:197,220-229,291`
  installs `LC_NUMERIC="C"`, scans `"lambda parameter:%f\n"` into a `gfloat`,
  accepts exactly one assignment, and then reads the source-format header.
  The read blob hashes to `07b20e4ee9f061251d62efd2e4c4b09e55119d26`, equal to
  `git rev-parse 074a2219:src/storage/table_info.cpp`; `utils/utils_helper.h`
  likewise matches `385415f7244a131dec378ce22436ce32ac45714b`.
- At glibc **glibc-2.41**, [`stdio-common/sscanf.c:29-42`](https://github.com/bminor/glibc/blob/glibc-2.41/stdio-common/sscanf.c#L29)
  wraps a string FILE and calls `__vfscanf_internal`;
  [`stdio-common/isoc99_fscanf.c:24-33`](https://github.com/bminor/glibc/blob/glibc-2.41/stdio-common/isoc99_fscanf.c#L24)
  calls that same scanner on the real FILE. The ISO mode flags only change
  allocation and integer conversions, not `%f` or `%n`.
- [`stdio-common/vfscanf-internal.c:112-121,600-605,665-683,698-711,2579-2589,3137-3144`](https://github.com/bminor/glibc/blob/glibc-2.41/stdio-common/vfscanf-internal.c#L665)
  shows read-ahead pushback decrementing the consumed-byte count, whitespace
  consuming through the first non-space byte and pushing it back, `%n`
  writing that count without adding an assignment, and `%f` writing a
  `float`. The final `\n` does not require a physical newline and cannot
  revoke a successful assignment. glibc **glibc-2.44**, read from
  [sourceware's tagged source](https://sourceware.org/git/?p=glibc.git;a=blob;f=stdio-common/vfscanf-internal.c;hb=glibc-2.44),
  has the same rules at `:112-121,680-698,713-726,2652-2663,3214-3221`.
- [`libio/strfile.h:81-91`](https://github.com/bminor/glibc/blob/glibc-2.41/libio/strfile.h#L81)
  and [`libio/strops.c:36-60,144-157`](https://github.com/bminor/glibc/blob/glibc-2.41/libio/strops.c#L36)
  show the string stream ending at NUL. The reader therefore supplies the
  actual remaining field bytes, including whitespace and the next header,
  terminated at the first NUL. `%f\n%tn` preserves the pin's float conversion
  and next-header position; `%tn` writes a `ptrdiff_t` count (Rust `isize`)
  without changing the required return of 1 or limiting input to `INT_MAX`.
  A NUL stays in the Rust input for the next header, as it does in the FILE.

The parser retains borrowed header and row slices. The C reader copies the
remaining bytes once for NUL termination: O(n) time and O(n) total input
space, as the existing whole-file read; no scan is added to the ranking path.
Training explicitly chooses the portable decimal reader. Current main's
datagen only emits `table.conf`; it has no `TableConf::parse` or `read` call.

## Historical record


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

```json
{
  "schema": 1,
  "record": "1297-table-floats",
  "status": "reverted",
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
