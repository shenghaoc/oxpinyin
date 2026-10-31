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
            "sha256": "28dc8597386374f2abc8a8555adc4aee03b0dc25fd68b551576b0715318c50a8",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "f49181c1f1f35f9d",
              "stderr": "89d24dc9bf85f38a",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "32fea30957831116d5bb3a2d4fe0eb310a9eeae531475f33c8e90c6700b6d827",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "f49181c1f1f35f9d",
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
            "sha256": "e6d464bf233b72518cc9341dede6bc8fcb13a1c092da492bb59889f00b996f93",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "f49181c1f1f35f9d",
              "stderr": "87946df291f6e0c9",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "32fea30957831116d5bb3a2d4fe0eb310a9eeae531475f33c8e90c6700b6d827",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "f49181c1f1f35f9d",
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
            "sha256": "6fda64d7b41668f6327781ee631a4e3deb85fef85380acbdcb07f571fa9a247c",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "d160c4f519e8ace9",
              "stderr": "0ac8c84bca8ccfac",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "24d7ef25c5ef9bb806d7e92bd136b3102a85242e08dea60ab6949f3eb8b19f54",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "d160c4f519e8ace9",
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
