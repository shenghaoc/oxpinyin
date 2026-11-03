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

The family C tkrzw `ngram-snapshot-directory` and
`punct-table-snapshot-directory` records bind only this assertion-text
exception. Their files compare exactly after the approved
[HashDBM clock timestamp normalization](eio-test-dbm-abort-timestamp.md).
The timestamp normalization itself is not a POLICY-ACCEPTED divergence.

The KC snapshot-abort and K-mixture existing-empty witnesses also bind
only assertion text after the 2026-11-02 UTC native-write and bounded-read
corrections. Their exit, stdout and complete files match without a KC
mask; the previous WAL/file and missing-abort differences are corrected,
not accepted. See [the shared attach finding](eio-test-flexible-attach-empty.md).
For `kmm-prune-existing-empty` on BDB, this same case entry instead binds
the separately accepted failed-open-handle diagnostic under
[register row 113](compatibility-policy.md#bdb-failed-handles); its other
output and files match.

The family E1 `memory-chunk-save-directory` case reaches
`tests/include/test_memory_chunk.cpp:61` (074a2219): `check_result(save(...))`
asserts after opening `/tmp/test.bin` fails. Both sides abort, retain the
input directory, and lose unflushed C stdout; only the assertion text differs.

The family E2 `phrase-index-logger-refused-layout` case uses a valid
27-byte frame whose payload separator is `!`, not `#`.
`SubPhraseIndex::load` rejects it with `g_return_val_if_fail`
(`phrase_index.cpp:354` at 074a2219). Both programs complete normally,
printing cached totals 0 and -7; only that macro's critical messages differ.
The source's separate stored local total stays 7 through the refused load.

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
    },
    "ngram-snapshot-directory": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "be3ee8a417a696af8f6dbdb9021f2d7308d301f2ec80016c988131c38c20f3d9",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "34265cfe62ed28f0",
              "stderr": "81cfa7db57792fac",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "231b5ad3b3064d10ba452bdbb4f0b5252f552c0c5c1602f5deee5f118ed14366",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "34265cfe62ed28f0",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_ngram: test_ngram.cpp:78: int main(in\" … (length 109); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "cf8551aeadb84c52c1e0384668f398a002c4ed10e36b4584d4b2323ecf632f36",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "8194ea42b68e7d2b",
              "stderr": "81cfa7db57792fac",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "bdbbd5c30f45df880947d323be8e3c1bf64813050abd318d56ab0f739d403deb",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "8194ea42b68e7d2b",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_ngram: test_ngram.cpp:78: int main(in\" … (length 109); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "d364b2f01ce65ebe5ba0c51dd10b5686f297c2750db81c888b18bf8dbd2257b7",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "380b662aaa739bb9",
              "stderr": "81cfa7db57792fac",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "6565bbbd068c349fd05b24132ef66cc9e834d4950af0b012a901cc3bebaf15b0",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "380b662aaa739bb9",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_ngram: test_ngram.cpp:78: int main(in\" … (length 109); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "punct-table-snapshot-directory": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "82da5cb6ea90f59d6ea5e6c8ba991843aa6d4e4bd62d10f6ad6eae0d51b860d4",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "062600d61e562187",
              "stderr": "8bf4341617ca3866",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "e36b17e199019769d7920967f9c95ca6b3c5da7d22ece527fefac0bed0a60a5d",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "062600d61e562187",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_punct_table: test_punct_table.cpp:63:\" … (length 120); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "5291faabea7350e5bdd72a8d0f8b1843621f2801bf4ea309c3f15f955ef3db46",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "888823687cff71ed",
              "stderr": "8bf4341617ca3866",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "593a6e5cb8fab94098ca836919d111062056dd99e217f720f440e3b5e9c88a0c",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "888823687cff71ed",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_punct_table: test_punct_table.cpp:63:\" … (length 120); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "046ad48fa7256f3ebca4a710d91e383b3ce82dd9bac89056abb9b4d21a16f726",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "d9104db243a951df",
              "stderr": "8bf4341617ca3866",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "95e30ecd02a446744a6a38f1d5aa187b008c4c9f708eea4616064debf605dbf3",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "d9104db243a951df",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_punct_table: test_punct_table.cpp:63:\" … (length 120); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "ngram-attach-directory": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "6bade2db6e167fc480abd23d6ff1a2e8a8b4555e352b499191fe2ef4b04dcf28",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "2ec7eebe79fbbdaf",
              "stderr": "662882698e52976c",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "747e81cd2742b493c9dfc6d826a377f64625d18b8860f7ca5b39254d27b9c95f",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "2ec7eebe79fbbdaf",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_ngram: test_ngram.cpp:40: int main(in\" … (length 136); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "6bade2db6e167fc480abd23d6ff1a2e8a8b4555e352b499191fe2ef4b04dcf28",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "2ec7eebe79fbbdaf",
              "stderr": "662882698e52976c",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "747e81cd2742b493c9dfc6d826a377f64625d18b8860f7ca5b39254d27b9c95f",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "2ec7eebe79fbbdaf",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_ngram: test_ngram.cpp:40: int main(in\" … (length 136); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "6bade2db6e167fc480abd23d6ff1a2e8a8b4555e352b499191fe2ef4b04dcf28",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "2ec7eebe79fbbdaf",
              "stderr": "662882698e52976c",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "747e81cd2742b493c9dfc6d826a377f64625d18b8860f7ca5b39254d27b9c95f",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "2ec7eebe79fbbdaf",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_ngram: test_ngram.cpp:40: int main(in\" … (length 136); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "flexible-ngram-empty-file": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "fb6fb38cbccdce4e0d71777fc9b1e8be6fae1a170093eafcec1fe20c497476d1",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "04e6165d746ca302",
              "stderr": "409ea8d67dc7a752",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "65ec0c219775b17aa386292df050e6476f37c0274725f9ae489d09e4d24be26c",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "04e6165d746ca302",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_flexible_ngram: test_flexible_ngram.c\" … (length 158); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "18a3305e465e0557dd7d7057a6dc3078b1f9fb7734278fc84bb39c1823017764",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "f912d9b5cf1e2256",
              "stderr": "409ea8d67dc7a752",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "b45643ce88ff44cfba9e8f5eb033a909a76910abb1ce64fdfb149feb3b10256e",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "f912d9b5cf1e2256",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_flexible_ngram: test_flexible_ngram.c\" … (length 158); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "flexible-ngram-attach-directory": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "31fc4cb48284ffef163dfc80093ec880b7b8b9c8f254748e60132582c0adebdc",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "4145c8d961d854f4",
              "stderr": "409ea8d67dc7a752",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "0ca40063de9f7b61e729b493395bc011ec91264b8f68833c158e338cae91ade9",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "4145c8d961d854f4",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_flexible_ngram: test_flexible_ngram.c\" … (length 158); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "31fc4cb48284ffef163dfc80093ec880b7b8b9c8f254748e60132582c0adebdc",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "4145c8d961d854f4",
              "stderr": "409ea8d67dc7a752",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "0ca40063de9f7b61e729b493395bc011ec91264b8f68833c158e338cae91ade9",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "4145c8d961d854f4",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_flexible_ngram: test_flexible_ngram.c\" … (length 158); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "31fc4cb48284ffef163dfc80093ec880b7b8b9c8f254748e60132582c0adebdc",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "4145c8d961d854f4",
              "stderr": "409ea8d67dc7a752",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "0ca40063de9f7b61e729b493395bc011ec91264b8f68833c158e338cae91ade9",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "4145c8d961d854f4",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_flexible_ngram: test_flexible_ngram.c\" … (length 158); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "punct-table-attach-directory": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "893022412353b9022cc5933b717f8feb0bc04c7abc3ef91c753c5761f980a388",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "ffdb2f934edaef70",
              "stderr": "0e69c96d7ec2c9de",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "10e7eec5f777df79b6fcff7eec261604074d3d2f769de2782d54190c9589a145",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "ffdb2f934edaef70",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_punct_table: test_punct_table.cpp:49:\" … (length 149); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "893022412353b9022cc5933b717f8feb0bc04c7abc3ef91c753c5761f980a388",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "ffdb2f934edaef70",
              "stderr": "0e69c96d7ec2c9de",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "10e7eec5f777df79b6fcff7eec261604074d3d2f769de2782d54190c9589a145",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "ffdb2f934edaef70",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_punct_table: test_punct_table.cpp:49:\" … (length 149); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "893022412353b9022cc5933b717f8feb0bc04c7abc3ef91c753c5761f980a388",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "ffdb2f934edaef70",
              "stderr": "0e69c96d7ec2c9de",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "10e7eec5f777df79b6fcff7eec261604074d3d2f769de2782d54190c9589a145",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "ffdb2f934edaef70",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_punct_table: test_punct_table.cpp:49:\" … (length 149); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "kmm-gen-existing-empty": {
      "cells": {
        "kc": {
          "pin": {
            "sha256": "e8cfa4aa65e8bdba547a23de18a18fc0f9425b1e20d7d5f044694f8329685ccd",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "8717e778b8513204",
              "stderr": "7d19d0ee36e5a5f1",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "1e35d7f5b78b54f298d0865e45bec03b55e9643aa246db209b95f4756505b0a1",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "8717e778b8513204",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"gen_k_mixture_model: ../../src/storage/fle\" … (length 367); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "kmm-prune-existing-empty": {
      "cells": {
        "kc": {
          "pin": {
            "sha256": "98e2573d1e818918d792dc2b2a413b96d5096491b5cadc2b1bfe425c3c33dded",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "fbd85468fd271bc9",
              "stderr": "0c7ffe9508482ab0",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "b169846bf27c9f72e742d89e792a4375151ea7d8e297b06382d815337bd393ac",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "fbd85468fd271bc9",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"prune_k_mixture_model: ../../src/storage/f\" … (length 369); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "cf08351d93c3d2ef069d688a100ff4329d9ca5823fa1e3126ac99cca607d0228",
            "fields": {
              "exit": "d029fa3a95e174a1",
              "files": "0e66d39c1373af0c",
              "stderr": "7f27de0e0ff45af2",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "e0a4549d7a5f034b548811da8b4e92218640ae622d2e8f2d65a05b3edc6b6e8e",
            "fields": {
              "exit": "d029fa3a95e174a1",
              "files": "0e66d39c1373af0c",
              "stderr": "3b6f0eec09b49a6a",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB0004 fop_read_meta: empty.db: unexpecte\" … (length 232); subject \"BDB0004 fop_read_meta: empty.db: unexpecte\" … (length 100)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "memory-chunk-save-directory": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "6a968f8385c7dde3aaa99be38a6697135259ae2f4543aff65549d078cfa00a3b",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "fd78820024278f7d",
              "stderr": "72baaa962c0645db",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "4762e9f653df46d1e48b75fb8a6cc08c71d022e5dafe50eddd19722e2cf1d815",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "fd78820024278f7d",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"test_memory_chunk: test_memory_chunk.cpp:6\" … (length 117); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "phrase-index-wrong-long-length": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "e2b483e6df12ed1df5b9c7c47d4ef83d83be1f7878590328c95bf5cea08ae627",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "c0c1c3bb999b449a",
              "stderr": "045de632bd357968",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "0ef832ee4b5d39a21703de24d264bb9a492bc674de0e48ad346fc60525b70e59",
            "fields": {
              "exit": "03b26944890929ff",
              "files": "c0c1c3bb999b449a",
              "stderr": "6df573a4135e5947",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"FacadePhraseIndex::load_text:yi1'yi1'yi1'y\" … (length 220); subject \"FacadePhraseIndex::load_text:yi1'yi1'yi1'y\" … (length 99)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "phrase-index-logger-refused-layout": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "5eca72baccff282f190a2d665a1769dc67ddc210b655babd7bbb72f8a4eaf23e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "781c7817252ebbe2",
              "stderr": "7e7d37f83866a912",
              "stdout": "6fbbc3bfabedc351"
            }
          },
          "subject": {
            "sha256": "4253120477fd392bdb6ecca2a45ff9be2f0c6e37e698c78a02d6eeaf4d696947",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "781c7817252ebbe2",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "6fbbc3bfabedc351"
            }
          },
          "divergence": [
            "stderr: pin \"\\n** (process:PID): CRITICAL **: TIME: bool\" … (length 546); subject \"\"",
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
