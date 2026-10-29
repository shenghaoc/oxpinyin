# #1378 — zhuyin selection order

- **Status:** accepted under the maintainer's 2026-10-29 UTC ruling
  “No ported external-library internals”; recorded 2026-10-29T08:41:25Z (`date -u`).
- **Disposition:** POLICY-ACCEPTED, never MATCH.
- **Policy limb:** internal language/library artefact. The choice of equal
  elements made by libstdc++'s heap schedule is not a product ranking key,
  a transcendental precision difference, a memory-safety refusal or an abort.
- **Pin:** libpinyin 2.11.92 at `074a2219`; read from the pinned git tree,
  `phonetic_lookup.h` blob `c092e7610cb0`. Facade: zhuyin. BDB, KC and
  sound-tkrzw observations below were each verified on that cell.
- **Pin derivation:** `src/lookup/phonetic_lookup.h:66-91,141-172,331-335`
  compares equal-length raw possibilities without an identity tie key,
  then runs the standard heap algorithms. `src/zhuyin.cpp:50` chooses
  the one-store, one-result shape. User reading insertion order is the
  tone/key order of `src/storage/chewing_large_table2.h:208-231` with
  `pinyin_phrase3.h:33-63`; the fixture adds tokens in the opposite order.
- **Rust counterpart:** `nbest.rs`, `selection_loses_to`: length ascending,
  raw f32 possibility descending, NaN last, signed zeros equal, then earlier
  batch position. Single-slot admission still uses the pin's raw `<`.
- **Forward trace:** `zhuyin_begin_add_phrases` / iterator → stored user
  readings → toneless `dada` parse → `zhuyin_guess_sentence` → beam and
  final selection → `zhuyin_get_sentence`. No sort or cleanup of stored
  records occurs. The selected sentence is the observable.
- **Backward trace:** every zhuyin beam and final-tail selection uses this
  same comparator. Pinyin uses adjusted S and its existing batch tie key;
  phrase segmentation uses raw `<` / `>` scans. Candidate-frequency and
  predicted sorting, storage writes, and the C ownership paths are unchanged.
- **Input partition:** ordinary distinct scores keep the length/raw-score
  keys; equal scores use batch position. NaNs rank last at equal length;
  signed zeros tie. The case adds equally counted `龘龘` (tone 3) then
  `靐靐` (tone 2), parses toneless `dada`, and reads the one sentence.
- **Difference:** pin `龘龘`; subject `靐靐`. Both parse 4 bytes, guess
  successfully, retain tokens `0x7000001` and `0x7000002`, and emit the
  same fresh-user diagnostic. Tone-2 `靐靐` is earlier in the stored reading
  walk and therefore the candidate batch. No arithmetic difference is
  needed to explain this equal-count, equal-length tie.
- **Contract preserved:** C ABI/layout, real GLib ownership, exported symbols,
  persistent bytes/formats, data ownership, diagnostics and process effects
  are unchanged. Only the ruled sentence tie differs.
- **Tests:** `nbest-user-table-order-zhuyin` is POLICY-ACCEPTED; the pinyin
  counterpart retains its existing S2 record. `glib-sort-tails-zhuyin` and
  `table-conf-lambda-one-zhuyin` are controls. Unit tests cover NaN, signed
  zero and earlier batch position; the full BDB contract run covers the
  other call routes. No new driver or dependency.
- **Prior rulings:** the 2026-10-29 ruling supplies these exact keys. It
  preserves single-slot admission and pinyin front eviction.
- **Reproduction:** provision with `tools/cloud/setup.sh`; build debug
  artifacts with `just heavy -- just cell-artifacts`; run `just heavy --
  just runner contract-diff bdb "$PINYIN_ORACLE_PREFIX" "$OXPINYIN_CAPI_SO"
  "$OXPINYIN_ZHUYIN_SO" -- --cases nbest-user-table-order-zhuyin,glib-sort-tails-zhuyin,table-conf-lambda-one-zhuyin`.
  Verification output stays local and disposable.

The same key completes ordering when λ exceeds one. The unigram coefficient
is negative (`phonetic_lookup.h:651-663,712-713`), so unigram paths take
`log(negative)` and carry NaN; blended paths can remain finite
(`:679-693`). The pin's `<` predicate leaves NaN unordered. The ruled
selection key puts NaN last while preserving the single-slot admission
rule. The three additional cases are POLICY-ACCEPTED:

| Case | Rank-0 `ni3hao3`, pin → subject | Rank-0 `yi4shu4`, pin → subject |
| --- | --- | --- |
| `table-conf-lambda-one-and-a-half-zhuyin` | 儞䝥 → 你好 | 苅襩 → 艺术 |
| `table-conf-lambda-two-zhuyin` | 儞䝥 → 你好 | 苅襩 → 一书 |
| `table-conf-lambda-million-zhuyin` | 儞䝥 → 你好 | 苅襩 → 亦舒 |

Candidate-prefix/count changes follow the chosen NBEST sentence and the
pin's existing sentence-over-phrase dedup; candidate-frequency sorting
itself is unchanged. The complete observations below bind every field.

After #1400 merged, PR B rebased onto `9789880a` and migrated this record
with `just policy-record --migrate`. Schema 2 binds the digest of every complete
pin and subject observation, with field digests and bounded divergence text
for diagnosis. The four cases above are re-recorded on BDB, KC and sound-tkrzw;
the earlier broad selections are historical evidence, not new rebase runs.

```contract-diff-policy
{
  "schema": 2,
  "record": "1378-zhuyin-selection",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "nbest-user-table-order-zhuyin": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "9844a108aaaf34868b41aac183bbabb7184bca2b7e9e1c8fff6fccaac40ace7a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.guess": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "4b227777d4dd1fc6",
              "result.sentence": "4b33290598007444",
              "result.tokens": "920d9f0a3cf6e8f6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "cebf91dc0761ae93d0dccf425f71cd378b713529dfe2564792c8062d74a00728",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.guess": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "4b227777d4dd1fc6",
              "result.sentence": "d142796d081f9247",
              "result.tokens": "920d9f0a3cf6e8f6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.sentence[1:2]: pin [\"龘龘\"]; subject [\"靐靐\"]"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "9844a108aaaf34868b41aac183bbabb7184bca2b7e9e1c8fff6fccaac40ace7a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.guess": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "4b227777d4dd1fc6",
              "result.sentence": "4b33290598007444",
              "result.tokens": "920d9f0a3cf6e8f6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "cebf91dc0761ae93d0dccf425f71cd378b713529dfe2564792c8062d74a00728",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.guess": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "4b227777d4dd1fc6",
              "result.sentence": "d142796d081f9247",
              "result.tokens": "920d9f0a3cf6e8f6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.sentence[1:2]: pin [\"龘龘\"]; subject [\"靐靐\"]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "9844a108aaaf34868b41aac183bbabb7184bca2b7e9e1c8fff6fccaac40ace7a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.guess": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "4b227777d4dd1fc6",
              "result.sentence": "4b33290598007444",
              "result.tokens": "920d9f0a3cf6e8f6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "cebf91dc0761ae93d0dccf425f71cd378b713529dfe2564792c8062d74a00728",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.guess": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "4b227777d4dd1fc6",
              "result.sentence": "d142796d081f9247",
              "result.tokens": "920d9f0a3cf6e8f6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.sentence[1:2]: pin [\"龘龘\"]; subject [\"靐靐\"]"
          ]
        }
      }
    },
    "table-conf-lambda-million-zhuyin": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "45d552b8a451de9c0a4c7791f551dbd529e44dd2654c7fbcd5f9ec5561e6ce2f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "f49b4d07786a8d7f",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "59d936b00e99ae188df32547680e0714d034193d9437a02cdc6bef86a5b90394",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d4dc15fd11d86bf4",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.ni3hao3 (list lengths pin 4, subject 4; first change [0]): pin \"儞䝥\"; subject \"你好\"",
            "result.view.yi4shu4 (list lengths pin 4, subject 4; first change [0]): pin \"苅襩\"; subject \"亦舒\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "17c0d86aacfc4447fdad551e260acf8b533de9c9086c20004edd61d8856887c8",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "f49b4d07786a8d7f",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "86e58c5a47d1824e8442a41714165722eada3ee5356aed7f83f4190e8b237a29",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d4dc15fd11d86bf4",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.ni3hao3 (list lengths pin 4, subject 4; first change [0]): pin \"儞䝥\"; subject \"你好\"",
            "result.view.yi4shu4 (list lengths pin 4, subject 4; first change [0]): pin \"苅襩\"; subject \"亦舒\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "91bb86e4150a82642a1f83d69a73249906603c5dad58a1986303327813f721c6",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "f49b4d07786a8d7f",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "b5b11a71c778e24c89bb98fe22ec2762d8f670199c2b2e14adfc9aafa2fca0b1",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d4dc15fd11d86bf4",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.ni3hao3 (list lengths pin 4, subject 4; first change [0]): pin \"儞䝥\"; subject \"你好\"",
            "result.view.yi4shu4 (list lengths pin 4, subject 4; first change [0]): pin \"苅襩\"; subject \"亦舒\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "table-conf-lambda-one-and-a-half-zhuyin": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "090652e010946f72df455467152ca36a0b78e9e60d435dd77b6204dbc2e67fad",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "27578a4e0311df6d",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "fd4658f58e56a110c7bbc7b5396fd38222a6fe37b0ea095909d8fabf3da35d0f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "303c3751d89c9df9",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.ni3hao3 (list lengths pin 4, subject 4; first change [0]): pin \"儞䝥\"; subject \"你好\"",
            "result.view.yi4shu4 (list lengths pin 4, subject 4; first change [0]): pin \"苅襩\"; subject \"艺术\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "6cf9bfb1f4435a8f530858adf4adb2b8009b75de9339177d17476d366c5658fa",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "27578a4e0311df6d",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "3cde87366437e29cf44c3043c204d8972d6b29fb10bfb025d3c44dea2481264e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "303c3751d89c9df9",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.ni3hao3 (list lengths pin 4, subject 4; first change [0]): pin \"儞䝥\"; subject \"你好\"",
            "result.view.yi4shu4 (list lengths pin 4, subject 4; first change [0]): pin \"苅襩\"; subject \"艺术\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "bfcf189b05600ba62638302477488f36f7cd8b89f273587435f46a9b4b09eaca",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "27578a4e0311df6d",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "52245a761f447ee883689fc7974068182439fe910b722e6ce4a70d3ad48f14fa",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "303c3751d89c9df9",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.ni3hao3 (list lengths pin 4, subject 4; first change [0]): pin \"儞䝥\"; subject \"你好\"",
            "result.view.yi4shu4 (list lengths pin 4, subject 4; first change [0]): pin \"苅襩\"; subject \"艺术\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "table-conf-lambda-two-zhuyin": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "90224689d44d1c31b21744c8e554e9be6b2cf269da9485736a2918fe2930e6b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "91b4931463964506",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "d9b91b5d4731eff38343b99b286da9f3e78123334e388650f07ff86fd50bd7d2",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "9c0e3f0e0e33d57b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.ni3hao3 (list lengths pin 4, subject 4; first change [0]): pin \"儞䝥\"; subject \"你好\"",
            "result.view.yi4shu4 (list lengths pin 4, subject 4; first change [0]): pin \"苅襩\"; subject \"一书\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "4e7d7a24d4da2b23b115038a0f9158da7a28a18e162aec58cc1bb7828b6d13fb",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "91b4931463964506",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "942caee812bd631dee926bb8caa8128b47aa10162aa3f3c1af9e423cd292c545",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "9c0e3f0e0e33d57b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.ni3hao3 (list lengths pin 4, subject 4; first change [0]): pin \"儞䝥\"; subject \"你好\"",
            "result.view.yi4shu4 (list lengths pin 4, subject 4; first change [0]): pin \"苅襩\"; subject \"一书\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "cae7a44ba5bd287c2614709e8268449979fc12abc9dd3966537c0faa5959dca0",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "91b4931463964506",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "8fff335fa3b11da0b34898787a8022f4164281bbf01a33248baeb98547b14690",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "9c0e3f0e0e33d57b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.ni3hao3 (list lengths pin 4, subject 4; first change [0]): pin \"儞䝥\"; subject \"你好\"",
            "result.view.yi4shu4 (list lengths pin 4, subject 4; first change [0]): pin \"苅襩\"; subject \"一书\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```
