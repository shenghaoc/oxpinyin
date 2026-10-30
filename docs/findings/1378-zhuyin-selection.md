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

## Candidate-sort amendment (#1297 item A)

The selection/sentence results above stay as recorded. The maintainer
decision dated 2026-10-30 preserves the wrapping product comparison,
allowing defined fallback only for cyclic lists. On BDB, KC and sound-tkrzw,
λ=1.5 and λ=2 restore exactly main's previous complete subject observations;
their accepted differences remain the independent selection ruling above.
Only λ=million retains a new candidate-view difference: cyclic groups use
stable numeric fallback while consistent groups retain wrapping order.
Its guard, scope and source proof are in
[1297-candidate-numeric-sort](1297-candidate-numeric-sort.md). All three
existing entries are refreshed with `just policy-record` and revalidated
as POLICY-ACCEPTED on each cell. Pin digests do not move; the changed field
is subject `result.view`. All four
`row58-signed-{score,duplicate}-{off,on}` cases MATCH on every cell; this
record never registers them as differences.

## Conversion amendment (#1416)

#1416 stores the candidate frequency with Rust's `as u32` instead of the
amd64 `cvttss2si` imitation (decision-order limb 3 ([source policy](../source-policy.md#internal-language-artefacts))). For λ > 1 every
positive-unigram candidate score is negative and now stores 0, so the
length-1 rows keep their collection order instead of the wrapped-key
order the amd64 pin produces; the arm64 pin stores 0 too. The three
λ > 1 entries below are re-recorded on bdb, kc and sound tkrzw on
2026-10-30 UTC with that subject `result.view`; their pin digests do
not move. The difference is class (b) in
`upstream-divergences.md` ("Zhuyin candidate frequency: negative
`float`-to-`guint32` store at λ > 1").

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
            "sha256": "423617c1efd0900b7114df1407a1450b151dcdec9b256e8a8b6a9f725f0e972a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "65c7c6855f403860",
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
            "sha256": "0baad65ff301a7e5f30682838f4ba7589c69efc8a39f26302e467e9242b8e680",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "65c7c6855f403860",
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
            "sha256": "5496494dbfa498ce0969f584cf211b3715914d40eb2ced477b6b2a2484960272",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "65c7c6855f403860",
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
            "sha256": "3dce8d6762e04017dae20de11b2f74efce11e620d8de27e829b97d02d739d475",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "7a47c28fd792d262",
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
            "sha256": "bd1412a439e29981946e3f107e0de0071facb5b99e1b1cf5ecbd2423c0f82cb4",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "7a47c28fd792d262",
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
            "sha256": "12b6a86304521d3f3fbda6fb4a7ac50a3140c7f5299695d12ff3fe016bb8bfdf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "7a47c28fd792d262",
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
            "sha256": "75aa49f2a01eee03837d7c4f6882952f92df95df5c6fe323ffe92322ea70697f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "239408984bfd96d3",
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
            "sha256": "6f1b1030f8e7c3f5eab40530ad080b1b8c959aa1230fea4a9f9d42c3f7661a8a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "239408984bfd96d3",
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
            "sha256": "5bce136ffb5d45f5339977e6a194d90f3a5ed6835a6e97118cd69db548b067cc",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "239408984bfd96d3",
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
