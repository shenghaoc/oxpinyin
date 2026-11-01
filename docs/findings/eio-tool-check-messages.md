# Utility check macros write no message

Status: accepted language artefact, 2026-10-31 UTC. Disposition:
POLICY-ACCEPTED. Rule: [source policy, external I/O](../source-policy.md#external-io);
record form: [template](language-runtime-discrepancy-template.md).
Pin: libpinyin 074a2219 (`utils/training/k_mixture_model_to_interpolation.cpp`
blob `957765f4`, `src/storage/tag_utility.cpp` blob `232a07dc`).

A section line that `taglib_read` refuses fails `check_result`, which is
`assert` in the reference build (`src/include/pinyin_utils.h:26-31`). The
pin writes glibc's assertion line, and before it GLib's
`g_return_val_if_fail` critical where a value is missing
(`tag_utility.cpp:202`, `:246`), then aborts:

- `kti-unknown-line-assert`: an unknown line in `\1-gram` (`:119`).
- `kti-missing-value-assert`: `\item` without its phrase (`:202`, then
  `:119`).
- `kti-bigram-assert`: a `\2-gram` item without `count` (`:256` warning,
  then `:158`).
- `kti-last-line-reread-assert`: a last line without a newline, which the
  body re-reads (`:88`).

oxpinyin aborts on the same condition and writes neither message; GLib
warnings from the pin's own code stay. Stdout, files and SIGABRT match
(`reproduces_signal`).

```contract-diff-policy
{
  "schema": 2,
  "record": "eio-tool-check-messages",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "kti-unknown-line-assert": {
      "database_free": {
        "pin": {
          "sha256": "2776c117f8a7ed86f59c0ecc533af1c502e47945f6b8ca3a48a126ae45349280",
          "fields": {
            "exit": "03b26944890929ff",
            "files": "44136fa355b3678a",
            "stderr": "e22c424286aa25bb",
            "stdout": "b46f64cadd1061ee"
          }
        },
        "subject": {
          "sha256": "7a93fc23a4fe41ca94da362df2125f6961c53a31285952651aa4ff7257040914",
          "fields": {
            "exit": "03b26944890929ff",
            "files": "44136fa355b3678a",
            "stderr": "12ae32cb1ec02d01",
            "stdout": "b46f64cadd1061ee"
          }
        },
        "divergence": [
          "stderr: pin \"k_mixture_model_to_interpolation: k_mixtur\" … (length 188); subject \"\"",
          "… further divergence omitted (4096-byte budget)"
        ]
      }
    },
    "kti-missing-value-assert": {
      "database_free": {
        "pin": {
          "sha256": "82ea0e345ff7baffd73634d5887f361aa49a1de019401d9fa53b591ba77fe6c0",
          "fields": {
            "exit": "03b26944890929ff",
            "files": "44136fa355b3678a",
            "stderr": "8dc6da6c1d1bf065",
            "stdout": "370365b8cae99a18"
          }
        },
        "subject": {
          "sha256": "ebf2efa87164d821e752bd7ce977ce0776e4a304b751b79b3eb5a0ca0ae7e6c5",
          "fields": {
            "exit": "03b26944890929ff",
            "files": "44136fa355b3678a",
            "stderr": "12ae32cb1ec02d01",
            "stdout": "370365b8cae99a18"
          }
        },
        "divergence": [
          "stderr: pin \"\\n** (process:PID): CRITICAL **: TIME: bool\" … (length 333); subject \"\"",
          "… further divergence omitted (4096-byte budget)"
        ]
      }
    },
    "kti-bigram-assert": {
      "database_free": {
        "pin": {
          "sha256": "ed3e87c819312ac73c44b68aebdb33c91eb69775640f5de8154849f4c1e6ae3a",
          "fields": {
            "exit": "03b26944890929ff",
            "files": "44136fa355b3678a",
            "stderr": "abb1bee1af0b7ca5",
            "stdout": "a7f211389c8e66a6"
          }
        },
        "subject": {
          "sha256": "d460f9951af05bfb2b209d6d9d36c29f03bd487c747b0a41744944b71dee34d4",
          "fields": {
            "exit": "03b26944890929ff",
            "files": "44136fa355b3678a",
            "stderr": "05e60d3e0163b771",
            "stdout": "a7f211389c8e66a6"
          }
        },
        "divergence": [
          "stderr: pin \"\\n** (process:PID): WARNING **: TIME: misse\" … (length 254); subject \"\\n** (process:PID): WARNING **: TIME: misse\" … (length 67)",
          "… further divergence omitted (4096-byte budget)"
        ]
      }
    },
    "kti-last-line-reread-assert": {
      "database_free": {
        "pin": {
          "sha256": "a98f0448b3270ea9e83bee22e1c93980e60758eabdb3bcb3043139cfc6a926cd",
          "fields": {
            "exit": "03b26944890929ff",
            "files": "44136fa355b3678a",
            "stderr": "b49468b1051cdbb5",
            "stdout": "370365b8cae99a18"
          }
        },
        "subject": {
          "sha256": "ebf2efa87164d821e752bd7ce977ce0776e4a304b751b79b3eb5a0ca0ae7e6c5",
          "fields": {
            "exit": "03b26944890929ff",
            "files": "44136fa355b3678a",
            "stderr": "12ae32cb1ec02d01",
            "stdout": "370365b8cae99a18"
          }
        },
        "divergence": [
          "stderr: pin \"k_mixture_model_to_interpolation: k_mixtur\" … (length 184); subject \"\"",
          "… further divergence omitted (4096-byte budget)"
        ]
      }
    },
    "table-info-save-fails": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "2c2ecb2976b1308e06fd0777bb35303cec7670deba1803e08acb31c4a95ba16e",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "ab786b23734061c6",
              "stderr": "1b9fe7bdd5e9d0e0",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "c9729ee057b8b0d87a667031d8deec93c11c7abc45d920cf52f551cfa0b6425f",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "ab786b23734061c6",
              "stderr": "f15cf91b4786a459",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"write /tmp/user.conf failed.\\ntest_table_in\" … (length 152); subject \"write /tmp/user.conf failed.\\n\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```

## Config test driver (2026-11-01 UTC)

`table-info-save-fails` makes `/tmp/user.conf` a directory. The pin's
`UserTableInfo::save` writes `write /tmp/user.conf failed.\n` and returns
false (`src/storage/table_info.cpp:377-385` at `074a2219`);
`check_result` in `tests/storage/test_table_info.cpp:93` then aborts.
oxpinyin preserves that diagnostic and SIGABRT without imitating the
assertion text. The complete streams and files are bound in this
record's schema-2 entry.
