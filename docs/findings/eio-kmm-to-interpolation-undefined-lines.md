# k_mixture_model_to_interpolation undefined lines — normal refusal

Status: accepted class (b), 2026-10-31 UTC. Disposition: POLICY-ACCEPTED.
Pin: libpinyin 074a2219 (`utils/training/k_mixture_model_to_interpolation.cpp`
blob `957765f4`, `src/storage/tag_utility.cpp` blob `232a07dc`).

`taglib_read` takes `tokens[0]` from `split_line` as the line tag
(`tag_utility.cpp:179-190`). The reference pin dies with SIGSEGV in:

- `kti-blank-line`: an empty line in a section; `strcmp` reads the NULL
  `tokens[0]` (`:183`, `:190`).
- `kti-end-missing`: input ending in a newline without `\end`; the final
  `std::getline` empties `linebuf` and the body re-reads it
  (`k_mixture_model_to_interpolation.cpp:86-88`), the same `strcmp`.
- `kti-trailing-escape`: a backslash ending a quoted token at the end of
  the line; `split_line` answers NULL (`:137`), `g_strv_length` logs GLib's
  critical (`:181`), and `tokens[0]` reads through the NULL.

oxpinyin makes the pin's calls up to that read, then writes
`k_mixture_model_to_interpolation: <reason>: <line>\n` on stderr and exits
1. Stdout and files match. `split_line`'s own critical is not written
([eio-tool-check-messages](eio-tool-check-messages.md)).

```contract-diff-policy
{
  "schema": 2,
  "record": "eio-kmm-to-interpolation-undefined-lines",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "kti-blank-line": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "b81fe1069575c2175eabd695dc24ff5ffae0cf558992b7beea3dbba19a2a0b04",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "44136fa355b3678a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "subject": {
            "sha256": "41070e5f5263ec40d5f81d525be1d1a3069aefd62864d371586bef6e32d75bd3",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "44136fa355b3678a",
              "stderr": "4c3c408173258476",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"k_mixture_model_to_interpolation: line wit\" … (length 57)",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "b81fe1069575c2175eabd695dc24ff5ffae0cf558992b7beea3dbba19a2a0b04",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "44136fa355b3678a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "subject": {
            "sha256": "41070e5f5263ec40d5f81d525be1d1a3069aefd62864d371586bef6e32d75bd3",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "44136fa355b3678a",
              "stderr": "4c3c408173258476",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"k_mixture_model_to_interpolation: line wit\" … (length 57)",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "b81fe1069575c2175eabd695dc24ff5ffae0cf558992b7beea3dbba19a2a0b04",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "44136fa355b3678a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "subject": {
            "sha256": "41070e5f5263ec40d5f81d525be1d1a3069aefd62864d371586bef6e32d75bd3",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "44136fa355b3678a",
              "stderr": "4c3c408173258476",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"k_mixture_model_to_interpolation: line wit\" … (length 57)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "kti-end-missing": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "b81fe1069575c2175eabd695dc24ff5ffae0cf558992b7beea3dbba19a2a0b04",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "44136fa355b3678a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "subject": {
            "sha256": "41070e5f5263ec40d5f81d525be1d1a3069aefd62864d371586bef6e32d75bd3",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "44136fa355b3678a",
              "stderr": "4c3c408173258476",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"k_mixture_model_to_interpolation: line wit\" … (length 57)",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "b81fe1069575c2175eabd695dc24ff5ffae0cf558992b7beea3dbba19a2a0b04",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "44136fa355b3678a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "subject": {
            "sha256": "41070e5f5263ec40d5f81d525be1d1a3069aefd62864d371586bef6e32d75bd3",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "44136fa355b3678a",
              "stderr": "4c3c408173258476",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"k_mixture_model_to_interpolation: line wit\" … (length 57)",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "b81fe1069575c2175eabd695dc24ff5ffae0cf558992b7beea3dbba19a2a0b04",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "44136fa355b3678a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "subject": {
            "sha256": "41070e5f5263ec40d5f81d525be1d1a3069aefd62864d371586bef6e32d75bd3",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "44136fa355b3678a",
              "stderr": "4c3c408173258476",
              "stdout": "b46f64cadd1061ee"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"k_mixture_model_to_interpolation: line wit\" … (length 57)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "kti-trailing-escape": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "c9054a545f2f692b5948028d494ea89f4ddf137d1f8f5d596039a8dbb4027f9b",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "44136fa355b3678a",
              "stderr": "54fe9558d1ba87a3",
              "stdout": "370365b8cae99a18"
            }
          },
          "subject": {
            "sha256": "78c4160277f912cb162cf7d77a72417471c4fc972e34ee1c9c46720658cb4392",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "44136fa355b3678a",
              "stderr": "182655569db3a22d",
              "stdout": "370365b8cae99a18"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\\n** (process:PID): CRITICAL **: TIME: gcha\" … (length 196); subject \"\\n(process:PID): GLib-CRITICAL **: TIME: g_\" … (length 170)",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "c9054a545f2f692b5948028d494ea89f4ddf137d1f8f5d596039a8dbb4027f9b",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "44136fa355b3678a",
              "stderr": "54fe9558d1ba87a3",
              "stdout": "370365b8cae99a18"
            }
          },
          "subject": {
            "sha256": "78c4160277f912cb162cf7d77a72417471c4fc972e34ee1c9c46720658cb4392",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "44136fa355b3678a",
              "stderr": "182655569db3a22d",
              "stdout": "370365b8cae99a18"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\\n** (process:PID): CRITICAL **: TIME: gcha\" … (length 196); subject \"\\n(process:PID): GLib-CRITICAL **: TIME: g_\" … (length 170)",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "c9054a545f2f692b5948028d494ea89f4ddf137d1f8f5d596039a8dbb4027f9b",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "44136fa355b3678a",
              "stderr": "54fe9558d1ba87a3",
              "stdout": "370365b8cae99a18"
            }
          },
          "subject": {
            "sha256": "78c4160277f912cb162cf7d77a72417471c4fc972e34ee1c9c46720658cb4392",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "44136fa355b3678a",
              "stderr": "182655569db3a22d",
              "stdout": "370365b8cae99a18"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\\n** (process:PID): CRITICAL **: TIME: gcha\" … (length 196); subject \"\\n(process:PID): GLib-CRITICAL **: TIME: g_\" … (length 170)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```
