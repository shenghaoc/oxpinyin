# Table-generator invalid UTF-8 — normal refusal

Status: accepted class (b), 2026-10-30 UTC. Disposition: POLICY-ACCEPTED.
Pin: libpinyin 074a2219c90feaf962d0d24f034514033ece5f99, read from
`/workspace/eio-env/libpinyin-src` at that commit before cases ran.

`utils/storage/gen_pinyin_table.cpp:162-186` and
`utils/storage/gen_zhuyin_table.cpp:168-192` count UTF-8 characters and
convert the phrase before checking the parsed key count. A byte `ff`
counts as one character but makes `g_utf8_to_ucs4` return NULL. With a
one-key reading it enters the phrase tree. Equal-length tree comparisons
can dereference that NULL (`gen_pinyin_table.cpp:85-94`,
`gen_zhuyin_table.cpp:90-98`); otherwise output conversion dereferences it
(`gen_pinyin_table.cpp:280-281`, `gen_zhuyin_table.cpp:286-287`). This is
undefined behavior; the reference-cell pin terminates with SIGSEGV.

The Rust generators keep unconverted phrases, read the remaining inputs,
print the node count, open the output, and refuse at the unconverted row
with `<utility>: invalid UTF-8 phrase: <bytes>\n` on stderr and normal
exit 1. C `exit` flushes preceding rows and stdout. Pin crash buffer loss
is not reproduced. Same-length invalid phrases sort before valid phrases;
no null read is reproduced while ordering them. Truncated UTF-8 lead bytes
are backed with zero padding rather than reading outside the allocation.
This extends #1436's existing pinyin policy to the corresponding zhuyin
utility. No shipping library, C API or backend data format changes.

`tool-diff` cases `pinyin-invalid-utf8`, `pinyin-invalid-utf8-after-row`,
`zhuyin-invalid-utf8`, and `zhuyin-invalid-utf8-after-row` bind the complete
exit, stdout, stderr and file observations below through `just policy-record`.
Ordinary input, an empty phrase, and a parse-length mismatch are controls.
An empty phrase with an empty reading is defined: it meets GLib's NULL-array
precondition in `store_one_item` and is omitted with GLib's critical message.

```contract-diff-policy
{
  "schema": 2,
  "record": "eio-table-generator-invalid-utf8",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "pinyin-invalid-utf8": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "035b3bf802d19bb56fb7b8a5fc741fa264d2253e8bebb9f64bae8d49b231e287",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "1e55aa5c7e5a7c45",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "886f46342e943e713b20f491ca87c6411b4ceb13d8630a7e17130e3a5a32292d",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "1e55aa5c7e5a7c45",
              "stderr": "13858d9c31ff566b",
              "stdout": "e0e780df9a45d101"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"gen_pinyin_table: invalid UTF-8 phrase: \\udcff\\n\"",
            "stdout: pin \"\"; subject \"nnodes: 1\\n\""
          ]
        },
        "kc": {
          "pin": {
            "sha256": "035b3bf802d19bb56fb7b8a5fc741fa264d2253e8bebb9f64bae8d49b231e287",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "1e55aa5c7e5a7c45",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "886f46342e943e713b20f491ca87c6411b4ceb13d8630a7e17130e3a5a32292d",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "1e55aa5c7e5a7c45",
              "stderr": "13858d9c31ff566b",
              "stdout": "e0e780df9a45d101"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"gen_pinyin_table: invalid UTF-8 phrase: \\udcff\\n\"",
            "stdout: pin \"\"; subject \"nnodes: 1\\n\""
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "035b3bf802d19bb56fb7b8a5fc741fa264d2253e8bebb9f64bae8d49b231e287",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "1e55aa5c7e5a7c45",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "886f46342e943e713b20f491ca87c6411b4ceb13d8630a7e17130e3a5a32292d",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "1e55aa5c7e5a7c45",
              "stderr": "13858d9c31ff566b",
              "stdout": "e0e780df9a45d101"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"gen_pinyin_table: invalid UTF-8 phrase: \\udcff\\n\"",
            "stdout: pin \"\"; subject \"nnodes: 1\\n\""
          ]
        }
      }
    },
    "pinyin-invalid-utf8-after-row": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "f56fa53597a8da0ad562eb47f6541b4a103ea4964ad9c2e3882c7472e53bf821",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "a3acf6f4877566df",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "7a310073a274d9c1168f6b837eb8778903c2b5d6a5042c424809ad97c82a922a",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "88a85319db45cbd2",
              "stderr": "bde73b26dd854bd2",
              "stdout": "445030a9caae1e78"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "files.temp.out: pin \"hex:\"; subject \"hex:6e6909e4bda0093109350a\"",
            "stderr: pin \"\"; subject \"gen_pinyin_table: invalid UTF-8 phrase: \\udcff\\udcff\" … (length 43)",
            "stdout: pin \"\"; subject \"nnodes: 2\\n\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "f56fa53597a8da0ad562eb47f6541b4a103ea4964ad9c2e3882c7472e53bf821",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "a3acf6f4877566df",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "7a310073a274d9c1168f6b837eb8778903c2b5d6a5042c424809ad97c82a922a",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "88a85319db45cbd2",
              "stderr": "bde73b26dd854bd2",
              "stdout": "445030a9caae1e78"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "files.temp.out: pin \"hex:\"; subject \"hex:6e6909e4bda0093109350a\"",
            "stderr: pin \"\"; subject \"gen_pinyin_table: invalid UTF-8 phrase: \\udcff\\udcff\" … (length 43)",
            "stdout: pin \"\"; subject \"nnodes: 2\\n\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "f56fa53597a8da0ad562eb47f6541b4a103ea4964ad9c2e3882c7472e53bf821",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "a3acf6f4877566df",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "7a310073a274d9c1168f6b837eb8778903c2b5d6a5042c424809ad97c82a922a",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "88a85319db45cbd2",
              "stderr": "bde73b26dd854bd2",
              "stdout": "445030a9caae1e78"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "files.temp.out: pin \"hex:\"; subject \"hex:6e6909e4bda0093109350a\"",
            "stderr: pin \"\"; subject \"gen_pinyin_table: invalid UTF-8 phrase: \\udcff\\udcff\" … (length 43)",
            "stdout: pin \"\"; subject \"nnodes: 2\\n\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "zhuyin-invalid-utf8": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "52680b7f294d70187d60b4c6674613dec3a1e3723e377530291b6fc06ea4c3e3",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "dbd06a6b5ce7ade3",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "72b573ddb19cec2b3c5dc02ca3e2b7549ea2cf51338d6ed2bf0352d389a55dea",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "dbd06a6b5ce7ade3",
              "stderr": "76d64a05d1b6a556",
              "stdout": "e0e780df9a45d101"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"gen_zhuyin_table: invalid UTF-8 phrase: \\udcff\\n\"",
            "stdout: pin \"\"; subject \"nnodes: 1\\n\""
          ]
        },
        "kc": {
          "pin": {
            "sha256": "52680b7f294d70187d60b4c6674613dec3a1e3723e377530291b6fc06ea4c3e3",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "dbd06a6b5ce7ade3",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "72b573ddb19cec2b3c5dc02ca3e2b7549ea2cf51338d6ed2bf0352d389a55dea",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "dbd06a6b5ce7ade3",
              "stderr": "76d64a05d1b6a556",
              "stdout": "e0e780df9a45d101"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"gen_zhuyin_table: invalid UTF-8 phrase: \\udcff\\n\"",
            "stdout: pin \"\"; subject \"nnodes: 1\\n\""
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "52680b7f294d70187d60b4c6674613dec3a1e3723e377530291b6fc06ea4c3e3",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "dbd06a6b5ce7ade3",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "72b573ddb19cec2b3c5dc02ca3e2b7549ea2cf51338d6ed2bf0352d389a55dea",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "dbd06a6b5ce7ade3",
              "stderr": "76d64a05d1b6a556",
              "stdout": "e0e780df9a45d101"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "stderr: pin \"\"; subject \"gen_zhuyin_table: invalid UTF-8 phrase: \\udcff\\n\"",
            "stdout: pin \"\"; subject \"nnodes: 1\\n\""
          ]
        }
      }
    },
    "zhuyin-invalid-utf8-after-row": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "4a3213dbc47c6b62082b73f4f6f1508810ab6343afea2ece5e611a46a3187e6f",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "655afc21f75faba0",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "a0d8bbca5c67e0b0719d37401ee55aacc21602a8031cc817aa01333fe5ca0cbb",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "53dc96f0cb25d5f8",
              "stderr": "692bbb0d35b207b0",
              "stdout": "445030a9caae1e78"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "files.temp.out: pin \"hex:\"; subject \"hex:e3848be384a709e4bda0093109350a\"",
            "stderr: pin \"\"; subject \"gen_zhuyin_table: invalid UTF-8 phrase: \\udcff\\udcff\" … (length 43)",
            "stdout: pin \"\"; subject \"nnodes: 2\\n\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "4a3213dbc47c6b62082b73f4f6f1508810ab6343afea2ece5e611a46a3187e6f",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "655afc21f75faba0",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "a0d8bbca5c67e0b0719d37401ee55aacc21602a8031cc817aa01333fe5ca0cbb",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "53dc96f0cb25d5f8",
              "stderr": "692bbb0d35b207b0",
              "stdout": "445030a9caae1e78"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "files.temp.out: pin \"hex:\"; subject \"hex:e3848be384a709e4bda0093109350a\"",
            "stderr: pin \"\"; subject \"gen_zhuyin_table: invalid UTF-8 phrase: \\udcff\\udcff\" … (length 43)",
            "stdout: pin \"\"; subject \"nnodes: 2\\n\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "4a3213dbc47c6b62082b73f4f6f1508810ab6343afea2ece5e611a46a3187e6f",
            "fields": {
              "exit": "2e42a91d2375f9a3",
              "files": "655afc21f75faba0",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "a0d8bbca5c67e0b0719d37401ee55aacc21602a8031cc817aa01333fe5ca0cbb",
            "fields": {
              "exit": "6b86b273ff34fce1",
              "files": "53dc96f0cb25d5f8",
              "stderr": "692bbb0d35b207b0",
              "stdout": "445030a9caae1e78"
            }
          },
          "divergence": [
            "exit: pin -11; subject 1",
            "files.temp.out: pin \"hex:\"; subject \"hex:e3848be384a709e4bda0093109350a\"",
            "stderr: pin \"\"; subject \"gen_zhuyin_table: invalid UTF-8 phrase: \\udcff\\udcff\" … (length 43)",
            "stdout: pin \"\"; subject \"nnodes: 2\\n\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```
