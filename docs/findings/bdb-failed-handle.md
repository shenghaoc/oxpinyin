# bdb-failed-handle — Berkeley DB handles whose open failed

- **Status:** accepted · 2026-11-02 UTC · issue #1516
- **Disposition:** POLICY-ACCEPTED on the bdb cell, under the external API misuse ruling.
- **Explanation:** [compatibility register, row 113](compatibility-policy.md#bdb-failed-handles). This file holds only the digests of the accepted observations.
- **Evidence:** `just runner contract-diff bdb "$PINYIN_ORACLE_PREFIX" <libpinyin_capi.so> <libzhuyin_capi.so> -- --cases-prefix missing-system-dbm/bdb- --observations <capture>`, and `just runner tool-diff --pin-build <bdb pin tree> --subject-dir <bdb cell>/debug --observations <capture>`; then `just policy-record docs/findings/bdb-failed-handle.md <capture> --cases <IDs>`. The captures are of commit tree `54ff07ec` (code) on the host, Berkeley DB 5.3.28.

```contract-diff-policy
{
  "schema": 2,
  "record": "bdb-failed-handle",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "missing-system-dbm/bdb-pinyin-addon-phrase-lifecycle": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7f63a2056ed0c3750b094a30c9ee0a08e49da14828b0ffd81f4514a94d8406c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "625ad7ebfd188d9daa1b219583eb810ef9e3d36efd352ed39d5e86a50828f2c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-addon-phrase-operations": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "e9ced834affa52bcebf3edcdedfd3645e9a79294162fb3631efea33a90b882f0",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "subject": {
            "sha256": "07437d139b92392fbc6aa39bbd24bfa02179a1a24bfa312af417268b63a1b62d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-addon-phrase-overlay": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "5236dda379d0c82bd7556ee7d62e8df560bab21f1cb9a6e4cace05bfa5f1e060",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "subject": {
            "sha256": "d918b58dbd9152d852a33cfcdf8a26b81a2010331d6810ff5a0436d382204f15",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-addon-pinyin-lifecycle": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7f63a2056ed0c3750b094a30c9ee0a08e49da14828b0ffd81f4514a94d8406c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "625ad7ebfd188d9daa1b219583eb810ef9e3d36efd352ed39d5e86a50828f2c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-addon-pinyin-operations": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "89dbea7c117c3fd221603b3af2f4c4cdd4b3a6d571fd3be55d7118dd6eea372b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "7f4e92694edc4b46",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "subject": {
            "sha256": "07437d139b92392fbc6aa39bbd24bfa02179a1a24bfa312af417268b63a1b62d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 199); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-addon-pinyin-overlay": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "9fe350a7fb2dcd963f9083ee1f782e697b664069d18065c4d758adde73a74df3",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "7f4e92694edc4b46",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "subject": {
            "sha256": "d918b58dbd9152d852a33cfcdf8a26b81a2010331d6810ff5a0436d382204f15",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 199); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-bigram-lifecycle": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7f63a2056ed0c3750b094a30c9ee0a08e49da14828b0ffd81f4514a94d8406c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "625ad7ebfd188d9daa1b219583eb810ef9e3d36efd352ed39d5e86a50828f2c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-bigram-operations": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "1d93192d0befdc399e68d56497e2a5f1c25cddf46b4e1938680e414b6d70a2ce",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "969ccb426b69b950",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "cd0ecbe4c7f9dd5e",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "subject": {
            "sha256": "5414b5e53bf52cec4e991a053a0d9e84dc71045c926f5e55e8a1a788391633e1",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "969ccb426b69b950",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 7327); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-bigram-overlay": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "2f065877c3da018a6f291e525889aaa5373ed4e117202dfc4a77bb56080592f3",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "7dd5a01482661bc0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "cf4f456a81fcbee6",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "subject": {
            "sha256": "7eb6e07880c60b615d85d3c33adc451be1017817ee3c8e8dfa81e46fb6e405b2",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "7dd5a01482661bc0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 7459); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-phrase-lifecycle": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7f63a2056ed0c3750b094a30c9ee0a08e49da14828b0ffd81f4514a94d8406c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "625ad7ebfd188d9daa1b219583eb810ef9e3d36efd352ed39d5e86a50828f2c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-phrase-operations": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "72bee9c1b6d8f7631991493b0b7f7985a7c316d2eb72e3a3b8e150dd8a10c431",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "3e3ff34a3cecfef1",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "fcbcf165908dd18a",
              "result.predicted_rows": "4f53cda18c2baa0c",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "4f53cda18c2baa0c",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "fcbcf165908dd18a",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "4f53cda18c2baa0c",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "7b34eaa5bb3ad021",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "subject": {
            "sha256": "d74f9e2a79ffbe3bffe3592154003acd52d7842ad60e430590691fdcdbc1a67f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "3e3ff34a3cecfef1",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "fcbcf165908dd18a",
              "result.predicted_rows": "4f53cda18c2baa0c",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "4f53cda18c2baa0c",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "fcbcf165908dd18a",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "4f53cda18c2baa0c",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 727); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-phrase-overlay": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "d74bce643c2c1b243f34b14ab9600f5c053fae4adfca5483b69baa67a57e8d09",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "3e3ff34a3cecfef1",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "fcbcf165908dd18a",
              "result.predicted_rows": "4f53cda18c2baa0c",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "4f53cda18c2baa0c",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "17c75ee4c363725b",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "c4dc0db24d9e07aa",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "subject": {
            "sha256": "4529346d3d730471e14b270432eeef7bf735bb8647a81259c2744d121e2f109c",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "3e3ff34a3cecfef1",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "fcbcf165908dd18a",
              "result.predicted_rows": "4f53cda18c2baa0c",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "4f53cda18c2baa0c",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "17c75ee4c363725b",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 859); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-pinyin-lifecycle": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7f63a2056ed0c3750b094a30c9ee0a08e49da14828b0ffd81f4514a94d8406c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "625ad7ebfd188d9daa1b219583eb810ef9e3d36efd352ed39d5e86a50828f2c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-pinyin-operations": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "ba41b1b6c8d50fceedef245c873ac0ca467e4c8ccf8510998cfe7efd91fd47f7",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "4f53cda18c2baa0c",
              "result.saved": "fcbcf165908dd18a",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "f67169e1a4109036",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "fcbcf165908dd18a",
              "stderr": "6bd6bc134a590dfb",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "03bc638b393d3b6dbcafa0284220e2c57636ca68fd6dbeb9d9cdc6494bce019f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "4f53cda18c2baa0c",
              "result.saved": "fcbcf165908dd18a",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "f67169e1a4109036",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "fcbcf165908dd18a",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 466); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-pinyin-overlay": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "519fc528a8825060f913f1c7e8636671de8d17091cead4a5b6013daacd0ebb53",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "c6000f0e9afc18ac",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "565e33553ada44d8",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "subject": {
            "sha256": "44d3183545fe2dfe13ec6b50a3bb7c0a3f6707ccc424ce89421bcf9ad6cd722d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "c6000f0e9afc18ac",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 664); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-pinyin-unloaded": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "46a400d34035ceac47ff6bcf482047ee77953f39ed3bcf5ae7d294a23a71d64a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "4f53cda18c2baa0c",
              "result.saved": "fcbcf165908dd18a",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "f67169e1a4109036",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "fcbcf165908dd18a",
              "result.unloaded": "b5bea41b6c623f7c",
              "stderr": "6bd6bc134a590dfb",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "cc52ebbf904946f6aeed75cfe5274037da802ab1bd4d5a7f7c47cd5200f3fc30",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "4f53cda18c2baa0c",
              "result.saved": "fcbcf165908dd18a",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "f67169e1a4109036",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "fcbcf165908dd18a",
              "result.unloaded": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 466); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-punctuation-lifecycle": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7f63a2056ed0c3750b094a30c9ee0a08e49da14828b0ffd81f4514a94d8406c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "625ad7ebfd188d9daa1b219583eb810ef9e3d36efd352ed39d5e86a50828f2c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-punctuation-operations": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "89dbea7c117c3fd221603b3af2f4c4cdd4b3a6d571fd3be55d7118dd6eea372b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "7f4e92694edc4b46",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "subject": {
            "sha256": "07437d139b92392fbc6aa39bbd24bfa02179a1a24bfa312af417268b63a1b62d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 199); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-pinyin-punctuation-overlay": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "9fe350a7fb2dcd963f9083ee1f782e697b664069d18065c4d758adde73a74df3",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "7f4e92694edc4b46",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "subject": {
            "sha256": "d918b58dbd9152d852a33cfcdf8a26b81a2010331d6810ff5a0436d382204f15",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.predicted": "b5bea41b6c623f7c",
              "result.predicted_rows": "5da92f8091dc931f",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.punctuation": "b5bea41b6c623f7c",
              "result.punctuation_rows": "5da92f8091dc931f",
              "result.rows": "e5e76f7bb31348b0",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 199); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-zhuyin-bigram-lifecycle": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7f63a2056ed0c3750b094a30c9ee0a08e49da14828b0ffd81f4514a94d8406c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "625ad7ebfd188d9daa1b219583eb810ef9e3d36efd352ed39d5e86a50828f2c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-zhuyin-bigram-operations": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "273fc468d3e7e624761e97b24f7e64e0c1089ea7938d8629d1bad708e55519ad",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "5366a54b0a438a4b",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "cd0ecbe4c7f9dd5e",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "subject": {
            "sha256": "0438d8088c3063e2d0990af10f311fdcccb3463b6a64c61fb8f082363e46b743",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "5366a54b0a438a4b",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 7327); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-zhuyin-bigram-overlay": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "385b43db377f4bbf1252da80d61ff2309656f095e91f8b7d78c1416fcceb7e46",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "2892d162e73f1337",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "cf4f456a81fcbee6",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "subject": {
            "sha256": "4a075f32d4c41182f415dbe900d7ae0338fac17fa2e1e0ca85d306274238ad0b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "2892d162e73f1337",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 7459); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-zhuyin-phrase-lifecycle": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7f63a2056ed0c3750b094a30c9ee0a08e49da14828b0ffd81f4514a94d8406c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "625ad7ebfd188d9daa1b219583eb810ef9e3d36efd352ed39d5e86a50828f2c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-zhuyin-phrase-operations": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "53de94a994057a480e7c760bccdd912352745a2fbdeb3e151793b733ab1b0bdc",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "3e3ff34a3cecfef1",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "dee9b49b967291d1",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "fcbcf165908dd18a",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "4f53cda18c2baa0c",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "d45955db11bd4793",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "subject": {
            "sha256": "52daab6e01ca13089d520a5ec1d91137251cd50b04c030e79972cf274de5e84c",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "3e3ff34a3cecfef1",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "dee9b49b967291d1",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "fcbcf165908dd18a",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "4f53cda18c2baa0c",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "3fb3d3734dc9cb8e"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 463); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-zhuyin-phrase-overlay": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "bda4567bb5d3677496ab570f057c32e43a0c424e968d2f7baec55ae38d290efa",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "3e3ff34a3cecfef1",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "dee9b49b967291d1",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "17c75ee4c363725b",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "881793ba162e793c",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "subject": {
            "sha256": "52b3e106d82cf64d39f1955e624fe139e3114a0cc163460fdb636048597f59bf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "3e3ff34a3cecfef1",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "dee9b49b967291d1",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "17c75ee4c363725b",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 595); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-zhuyin-pinyin-lifecycle": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7f63a2056ed0c3750b094a30c9ee0a08e49da14828b0ffd81f4514a94d8406c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "7f6de7009526db9c",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "625ad7ebfd188d9daa1b219583eb810ef9e3d36efd352ed39d5e86a50828f2c5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.fini": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-zhuyin-pinyin-operations": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "1c595e370ce4c155f9883f7b27b27016b41500ada839428d219ea4c69bfdab49",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "4f53cda18c2baa0c",
              "result.saved": "fcbcf165908dd18a",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "f67169e1a4109036",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "fcbcf165908dd18a",
              "stderr": "8db4f4a8066ecc5d",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "3264460d7b213d812d872e5d907a221f6e6e937402353f96aed74c59b65df6a2",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "4f53cda18c2baa0c",
              "result.saved": "fcbcf165908dd18a",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "f67169e1a4109036",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "fcbcf165908dd18a",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 397); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-zhuyin-pinyin-overlay": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "02c6e43b97488a86f94f5f611017ce23e47270c678cb4ca0360733b767979cf5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "c6000f0e9afc18ac",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "881793ba162e793c",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "subject": {
            "sha256": "c9099fa502a592f063965ebcdba91f5766e5a2b3582fe952b08935ca2e2af610",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.added": "b5bea41b6c623f7c",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "c6000f0e9afc18ac",
              "result.saved": "b5bea41b6c623f7c",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "7f0b42ee85da1ae1",
              "result.tokens": "0f3afcdca65c26df",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "e437077c25d7a0c2"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 595); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "missing-system-dbm/bdb-zhuyin-pinyin-unloaded": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "352d37131ea6f7535a73fe953aba1eb7d7c9d5535467f4a44ad01b21e13c51be",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "4f53cda18c2baa0c",
              "result.saved": "fcbcf165908dd18a",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "f67169e1a4109036",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "fcbcf165908dd18a",
              "result.unloaded": "b5bea41b6c623f7c",
              "stderr": "8db4f4a8066ecc5d",
              "userfiles": "d4d32be4be801586"
            }
          },
          "subject": {
            "sha256": "b1727a446a7a9403f097b18e24b3857ac7b0c948cfb525977b489adb15b5a5da",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.candidates": "b5bea41b6c623f7c",
              "result.character_offset": "2705bd6ffb3d9453",
              "result.fini": "b5bea41b6c623f7c",
              "result.guessed": "b5bea41b6c623f7c",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "7902699be42c8a8e",
              "result.prefix_guess": "b5bea41b6c623f7c",
              "result.rows": "4f53cda18c2baa0c",
              "result.saved": "fcbcf165908dd18a",
              "result.segmented": "b5bea41b6c623f7c",
              "result.sentence": "f67169e1a4109036",
              "result.tokens": "8fa7f7afbba4805a",
              "result.train_guess": "b5bea41b6c623f7c",
              "result.trained": "fcbcf165908dd18a",
              "result.unloaded": "b5bea41b6c623f7c",
              "stderr": "12ae32cb1ec02d01",
              "userfiles": "d4d32be4be801586"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 397); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "estimate_interpolation-missing-bigram-file": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "6d063456864e07d733924fb4560e9e2286c20d313cfa9dd77bf82d9efb15992d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "4d4382d14a0e59cb",
              "stderr": "ef11af4ca295d10f",
              "stdout": "fa425b043b704d34"
            }
          },
          "subject": {
            "sha256": "45c6427874460e2f9ccbe6106d6ee2797372317362140ed387e97e9561c41195",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "4d4382d14a0e59cb",
              "stderr": "d780402df3c38e3f",
              "stdout": "fa425b043b704d34"
            }
          },
          "divergence": [
            "stderr: pin \"BDB0004 fop_read_meta: bigram.db: unexpect\" … (length 264); subject \"BDB0004 fop_read_meta: bigram.db: unexpect\" … (length 65)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "estimate_interpolation-missing-deleted-bigram": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "baf05542e2e01b9905abde778319330dbde7bb842f0f4126b917ed7a94d6dace",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "c65e36a0db206439",
              "stderr": "3204e40bc561ea76",
              "stdout": "46b16e7ce12817cf"
            }
          },
          "subject": {
            "sha256": "30f529fc140bfabe4c6151aaaeaa762a2db61b0ff0304d7d059dcceb774dc7fd",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "c65e36a0db206439",
              "stderr": "3a453ff5b2a516bb",
              "stdout": "46b16e7ce12817cf"
            }
          },
          "divergence": [
            "stderr: pin \"BDB0004 fop_read_meta: deleted_bigram.db: \" … (length 209); subject \"BDB0004 fop_read_meta: deleted_bigram.db: \" … (length 73)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "kmm-estimate-no-models": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "2997c67e23b4d6f1b0bbdd97fd64f04c6421e9b317a98689bebf092f89c64c7a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "44136fa355b3678a",
              "stderr": "2c57ad5c83129373",
              "stdout": "46b16e7ce12817cf"
            }
          },
          "subject": {
            "sha256": "f29ec3658b57d63c818bbee575e7798165b3d0bc6b35081afaadffb6cd18a52d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "44136fa355b3678a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "46b16e7ce12817cf"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 335); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "eval_correction_rate-missing-bigram-file": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "435ccc44c3bfaa44f7664247b56c4d81efc0c6995fa046dec107ef0f16a526a1",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "9593103e45571bf4",
              "stderr": "b71a626117b795ad",
              "stdout": "fc654780ad04aaed"
            }
          },
          "subject": {
            "sha256": "91ed78b4a3297aa201c1d7e165c5be7e167e80b4ba12ade82211b7b01361b9ee",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "9593103e45571bf4",
              "stderr": "d780402df3c38e3f",
              "stdout": "fc654780ad04aaed"
            }
          },
          "divergence": [
            "stderr: pin \"BDB0004 fop_read_meta: bigram.db: unexpect\" … (length 198); subject \"BDB0004 fop_read_meta: bigram.db: unexpect\" … (length 65)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "exp-missing-bigram": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7ce3bd58a47d12f2f4d0b33dce35eb0ab9266686058423e944e87d22fc554b52",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "d1929b2cff147642",
              "stderr": "44a7cb07bc975778",
              "stdout": "bc120aac787b0360"
            }
          },
          "subject": {
            "sha256": "fb906d1c0860b0eca37e72ef18a4bb79a16fda43e0179afb4dad981120355833",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "d1929b2cff147642",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "bc120aac787b0360"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->cursor: method not permitted b\" … (length 136); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "kmm-export-absent-file": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "d0b56f409d90a88f07dba9e35175f723374e9278154134b7f52bbc351dc69824",
            "fields": {
              "exit": "d4735e3a265e16ee",
              "files": "273a1168b384b60a",
              "stderr": "075c80cc037e3a9d",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "f24cf9c64cbad68f359bed31686bced8620bb5fcf2f3f5c7f381cd5e73f6eeda",
            "fields": {
              "exit": "d4735e3a265e16ee",
              "files": "273a1168b384b60a",
              "stderr": "34ae8847cb4eb355",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 89); subject \"open absent.db failed.\\n\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "gen_deleted_ngram-too-many-arguments": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "ffc2461b9a11dd980fc8ea703239aecfa280c6c1064c408226926ea0469dfd86",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "7f6de7009526db9c",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "717d6b6f3f185d5d433a6010a8a4a01877c7a7488eb3c1dee1b9f9f4faf7450f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "gen_deleted_ngram-missing-bigram-file": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "f6e9c9e00f5f4c053c5b78e965186ecca54fb5994568daa7de44aac441fd5dfd",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "a41d41bee4189825",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "717d6b6f3f185d5d433a6010a8a4a01877c7a7488eb3c1dee1b9f9f4faf7450f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 199); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "gen_deleted_ngram-empty-stdin": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "ffc2461b9a11dd980fc8ea703239aecfa280c6c1064c408226926ea0469dfd86",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "7f6de7009526db9c",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "717d6b6f3f185d5d433a6010a8a4a01877c7a7488eb3c1dee1b9f9f4faf7450f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "gen_deleted_ngram-ordinary": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "6a90333c237da681e851d6187010bd259dffcdb043fe38fd28f0e38cec7e5052",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "446aa92985177d6e",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "717d6b6f3f185d5d433a6010a8a4a01877c7a7488eb3c1dee1b9f9f4faf7450f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 463); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "gen_ngram-too-many-arguments": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "ffc2461b9a11dd980fc8ea703239aecfa280c6c1064c408226926ea0469dfd86",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "7f6de7009526db9c",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "717d6b6f3f185d5d433a6010a8a4a01877c7a7488eb3c1dee1b9f9f4faf7450f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "gen_ngram-missing-bigram-file": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "62b1bb80272de5cd3ad76e784d6007a74709a2f56845f32361ab05027dda648a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "086f30a10bc18287",
              "stderr": "a41d41bee4189825",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "9da092723b66a7ffc0141260c735f45d94405cd0469114e7fbe1abb0d4fd0067",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "086f30a10bc18287",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 199); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "gen_ngram-empty-stdin": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "ffc2461b9a11dd980fc8ea703239aecfa280c6c1064c408226926ea0469dfd86",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "7f6de7009526db9c",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "717d6b6f3f185d5d433a6010a8a4a01877c7a7488eb3c1dee1b9f9f4faf7450f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "273a1168b384b60a",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->sync: method not permitted bef\" … (length 67); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "gen_ngram-ordinary": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "129b657c97c450b242659e06babed72172da570d2c478d344c0fe9b2dd681835",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "2810df7766ae7408",
              "stderr": "446aa92985177d6e",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "277a91af8c0d8013f1dfee4d35dee5339b79dfb8721b6e114c39e1aa4ce14bae",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "2810df7766ae7408",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 463); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "gen_ngram-skip-pi-gram": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "82b9acdc0fde8de1790bb627bd044e5b8efe3e91e79ab59dce1d45250fe2309f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "b2b9fb37f00902c0",
              "stderr": "a41d41bee4189825",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "4ba25dac8dc1cb7ba5f860e6115c6137d934db5c2afe2acf61a420bb513b5149",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "files": "b2b9fb37f00902c0",
              "stderr": "12ae32cb1ec02d01",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 199); subject \"\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "kmm-validate-missing-file": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "262677261906b7c71410186cc3304194efb174ec9c98f5a35aa9c6ed558174e6",
            "fields": {
              "exit": "d029fa3a95e174a1",
              "files": "44136fa355b3678a",
              "stderr": "8f62558b73034349",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "subject": {
            "sha256": "e5833fb4bee9a4b2e637ac1444cb22e15fc365f7c05a55eb99e6b0d3537bd113",
            "fields": {
              "exit": "d029fa3a95e174a1",
              "files": "44136fa355b3678a",
              "stderr": "6341d2d2d909c3ae",
              "stdout": "12ae32cb1ec02d01"
            }
          },
          "divergence": [
            "stderr: pin \"BDB1565 DB->get: method not permitted befo\" … (length 203); subject \"no magic header in k mixture model.\\nk mixt\" … (length 71)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```
