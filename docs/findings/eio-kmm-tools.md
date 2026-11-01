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
        },
        "bdb": {
          "pin": {
            "sha256": "ad6225b68403761520a268ec2b346e5f0d503361f1fbdd6a9f0e49de47c2a67e",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "d86c069c65882e81",
              "stderr": "89d24dc9bf85f38a",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "71f7edba1b1cb6c20f1be1563988ff5ec8c40562d917bd6932ffe7c5474a1828",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "d86c069c65882e81",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"merge_k_mixture_model: merge_k_mixture_mod\" … (length 205); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "c9d0224189228501582f0937813055ae5c9755f5cf41bb5aae85cdb514612005",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "bdcbe4f8512d2844",
              "stderr": "89d24dc9bf85f38a",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "5f36c352d845932b7be884400e6e962accaa08dec3abf6ec1c3ecccef36e7346",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "bdcbe4f8512d2844",
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
        },
        "bdb": {
          "pin": {
            "sha256": "9d7ad4ccd2a3eeab6ef70995d26114ff47ce43d0ffeaaee2c6618a357163a7b7",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "d86c069c65882e81",
              "stderr": "87946df291f6e0c9",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "71f7edba1b1cb6c20f1be1563988ff5ec8c40562d917bd6932ffe7c5474a1828",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "d86c069c65882e81",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"estimate_k_mixture_model: estimate_k_mixtu\" … (length 243); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "bc498a982967dc5fc7df810effd0fb085ccc4b5ce6d74277b266436155003d25",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "bdcbe4f8512d2844",
              "stderr": "87946df291f6e0c9",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "5f36c352d845932b7be884400e6e962accaa08dec3abf6ec1c3ecccef36e7346",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "bdcbe4f8512d2844",
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
        },
        "bdb": {
          "pin": {
            "sha256": "0b14bfd3fc44df23b4d543a2fa4f3bfebf22e43056bb9a414d5de9be8d680fe1",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "a193f9cd07b74bc9",
              "stderr": "0ac8c84bca8ccfac",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "41c84ec728db5024cd375f0ff7be86c7ded676852baf3eb1b7758bbf67c493a2",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "a193f9cd07b74bc9",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"gen_k_mixture_model: gen_k_mixture_model.c\" … (length 206); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "f40f152d5031826b88d38e9940aecf505f064d589444e69316cefbc58b6849fc",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "72e3c5e58e635963",
              "stderr": "0ac8c84bca8ccfac",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "fae38738a7d1def82a118d43244d5981ae72c93306886460933a4875708511f9",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "72e3c5e58e635963",
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
