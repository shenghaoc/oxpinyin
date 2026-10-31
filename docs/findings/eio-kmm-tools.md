# KMM tool asserts and their policy records

Date: 2026-10-31 UTC · Status: **policy record** · Branch: `wip/eio5-kmm`.

The seven k-mixture-model utilities at libpinyin
`074a2219c90feaf962d0d24f034514033ece5f99` reproduce the pin's abort sites
with `std::process::abort()`. The reference build keeps `assert` and
`check_result` active (`utils/training/*_k_mixture_model.cpp`, no `NDEBUG`),
so the pin prints its macro text on stderr and raises `SIGABRT`. Under the
2026-10-31 UTC maintainer ruling in
[eio-tool-check-messages](eio-tool-check-messages.md), oxpinyin does not
imitate the macros' text; stdout, the signal and the file inventory match.

Three registered `tool-diff` cases reach such a site:

- `kmm-merge-no-result-file` — no `--result-file`, so the target is the
  pin's NULL attach and `merge_magic_header`'s
  `check_result(target->set_magic_header(...))` fails
  (`merge_k_mixture_model.cpp:127`).
- `kmm-estimate-zero-total` — the candidate's magic header total frequency
  is zero, and `compute_interpolation` asserts
  `0 != magic_header.m_total_freq` (`estimate_k_mixture_model.cpp:45`).
- `kmm-gen-good-document` — no `--k-mixture-model-file`, so the first
  document trains into a NULL handle and `main`'s
  `check_result(bigram.get_magic_header(...))` fails
  (`gen_k_mixture_model.cpp:380`).

Each case registers `pin_signal: -6` and `reproduces_signal: true`, and the
block below binds the complete observations. These utilities open the
working container, so the entries are per cell, unlike the database-free
marker records.

```contract-diff-policy
{
  "schema": 2,
  "record": "eio-kmm-tools",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "kmm-merge-no-result-file": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "f7aa845823b4150cc01638a0cef8979199284661a3a9b46e7a2c4fdec9c19beb",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "5d85a2e6b56ec429",
              "stderr": "89d24dc9bf85f38a",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "39738abd93902f7ecf49d97922a8173a9ed46d65fe7227b14f82eb3bcb67bf2b",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "5d85a2e6b56ec429",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"merge_k_mixture_model: merge_k_mixture_mod\" … (length 205); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "kmm-estimate-zero-total": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "e1054595723a1676ecd027c864d14c7ef76ce6b9f434d38b716630ff289afcd3",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "5d85a2e6b56ec429",
              "stderr": "87946df291f6e0c9",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "39738abd93902f7ecf49d97922a8173a9ed46d65fe7227b14f82eb3bcb67bf2b",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "5d85a2e6b56ec429",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"estimate_k_mixture_model: estimate_k_mixtu\" … (length 243); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "kmm-gen-good-document": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "002dac0a21c64cc82a50e1b583da3f4c445658563f547770e5170d72daea942d",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "e5258fdb469bfce6",
              "stderr": "0ac8c84bca8ccfac",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "c67b052a2eddf6b0454550c031981f6fe1095fb180db7828b650a91adfaf6e19",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "e5258fdb469bfce6",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"gen_k_mixture_model: gen_k_mixture_model.c\" … (length 206); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```
