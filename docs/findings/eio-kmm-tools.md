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

Refreshed 2026-11-03 UTC: the entries were first bound at `494766c3`,
when each cell's `fixtures/w3` directory still held `datagen-manifest.txt`.
`4d60c774` (#1500) deleted that file. The tool runner copies the whole
cell directory into the case's working directory, so both sides' `files`
listing lost one name. That is the only change: exit, stdout and stderr
digests are as first recorded.

```contract-diff-policy
{
  "schema": 3,
  "record": "eio-kmm-tools",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "kmm-merge-no-result-file": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "39f4e388726e4802f1e136e6305bac3bd9fd5cd1b7d2e1d5e3f7bf293340eb95",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "29f52aace84acfc2",
              "stderr": "89d24dc9bf85f38a",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "0c97adadd2952ac3cb53ba36767b6d89695b4eac96da1ca5f06d4002cae8d610",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "29f52aace84acfc2",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"merge_k_mixture_model: merge_k_mixture_mod\" … (length 205); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ],
          "accepted": [
            "stderr"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "8159fd605ab614f5e3e7ee8f7c83fcf7e88cbe6dd683cd726b8305bc16219a7d",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "273a1168b384b60a",
              "stderr": "89d24dc9bf85f38a",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "1b40257eb586b36d616a5b029f348bfa36ff474e81e46181ab47bbeef9fdafc5",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "273a1168b384b60a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"merge_k_mixture_model: merge_k_mixture_mod\" … (length 205); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ],
          "accepted": [
            "stderr"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "4a9d7400a90b917efc8d4db707826a3327877ddb7ceac57372b718fefed60895",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "6444af0cb2c07e59",
              "stderr": "89d24dc9bf85f38a",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "0d1353d6377dfec9e21255f146fd57f562785abd6ca29742572c45680d8a1210",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "6444af0cb2c07e59",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"merge_k_mixture_model: merge_k_mixture_mod\" … (length 205); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ],
          "accepted": [
            "stderr"
          ]
        }
      }
    },
    "kmm-estimate-zero-total": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "e955c0f9a429d4936ce623776b750008106b346edec1e03d7e4eed632f542888",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "29f52aace84acfc2",
              "stderr": "87946df291f6e0c9",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "0c97adadd2952ac3cb53ba36767b6d89695b4eac96da1ca5f06d4002cae8d610",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "29f52aace84acfc2",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"estimate_k_mixture_model: estimate_k_mixtu\" … (length 243); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ],
          "accepted": [
            "stderr"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "da78c956f2418915ca4d475e2e57d74105a95fe860f09e27f1729f77134b9dd8",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "273a1168b384b60a",
              "stderr": "87946df291f6e0c9",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "1b40257eb586b36d616a5b029f348bfa36ff474e81e46181ab47bbeef9fdafc5",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "273a1168b384b60a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"estimate_k_mixture_model: estimate_k_mixtu\" … (length 243); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ],
          "accepted": [
            "stderr"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "65403bb1ba59326fe27d6e104466adacff3ca8cf18d515d990d74764c6480d8f",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "6444af0cb2c07e59",
              "stderr": "87946df291f6e0c9",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "0d1353d6377dfec9e21255f146fd57f562785abd6ca29742572c45680d8a1210",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "6444af0cb2c07e59",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"estimate_k_mixture_model: estimate_k_mixtu\" … (length 243); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ],
          "accepted": [
            "stderr"
          ]
        }
      }
    },
    "kmm-gen-good-document": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "d4b425a1c694603ec0410303f675c709b654e4d1b18c92c8728c6e1c46458c26",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "dd97f3b63ba556e1",
              "stderr": "0ac8c84bca8ccfac",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "384057dc9a7c1b83d2ac2dce5bf001dac315b559881f7cc51dd101f4684f95ae",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "dd97f3b63ba556e1",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"gen_k_mixture_model: gen_k_mixture_model.c\" … (length 206); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ],
          "accepted": [
            "stderr"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "590b9859d1526bea185562feb9a1454fd85c191f04d4c4ffee72f08c72ef6ef0",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "af63951d78f8b129",
              "stderr": "0ac8c84bca8ccfac",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "ca83ee76630db293eb34056ef05a065172429468a16f52b7e7d0b201ddc61776",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "af63951d78f8b129",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"gen_k_mixture_model: gen_k_mixture_model.c\" … (length 206); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ],
          "accepted": [
            "stderr"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "d623bdf659efe685623b2e6a0b0a32ca153e3ec80e480749d9d4d3c723f3a9b0",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "34f72ab6f1a37313",
              "stderr": "0ac8c84bca8ccfac",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "07f89ae84cd5a9c9fbb958b26b3e130a63cc386a7af8f72eb8aa95bd7da37eac",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "34f72ab6f1a37313",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"gen_k_mixture_model: gen_k_mixture_model.c\" … (length 206); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ],
          "accepted": [
            "stderr"
          ]
        }
      }
    }
  }
}
```
