# Pinyin trellis adjusted-cost comparison

Date: 2026-10-28 UTC (`date -u`). Policy: [witnessed algorithm-precondition
violations, S1/S2](compatibility-policy.md#amendment--witnessed-algorithm-precondition-violations-2026-10-28-utc).
This is the registered product-level ruling, not the language-artefact exception.
The pin is libpinyin 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`;
its named source blobs were read at that revision. Base: `2506b264`.

## Scope and derivation

`src/lookup/phonetic_lookup.h:39,65-91 at 074a2219` provides the adjacent-length
margin. The amendment completes it as `cost_add(cost, 263 * length)`, best first:
negative infinity, finite ascending, positive infinity, NaN. Multiplication
widens the `u32` length to `i64` first; its maximum is 1,129,576,398,585.
Finite addition saturates at `i64::MAX - 1`, remaining finite; infinities and
NaN remain special. Saturation can create additional equivalent costs.

The node heap uses this comparison alone (`src/lookup/phonetic_lookup_heap.h:
25-81 at 074a2219`). Its full-node comparison rejects a tie; an underfilled
node still fills its spare slot. The front-eviction defect remains.
S1 beam selection (`src/lookup/phonetic_lookup.h:770-771 at 074a2219`) and S2
final-tail selection (`:331-335`) use the total key of adjusted cost and earlier
batch position. Batch positions follow node creation order and physical slot
order before any selection (`:272-297`); they never replace the physical slot
copied into a successor's `sub` and read during extraction.

Shipping changes are confined to `nbest.rs`. The literal heap routines, tail
comparison and merge, zhuyin comparison, cost arithmetic, external calls,
C ABI, ownership and persistent formats are unchanged. Time and space
complexities are unchanged. `BeamEntry` adds one batch-position integer per
candidate; node storage does not grow.

## CI expectation audit

Before implementation, `ci.yml`, `store-backends.yml`, `verify-nightly.yml`,
`coverage.yml` and the nextest configuration were inspected. The ordinary
workspace tests include `nbest.rs`, the engine's constrained-session tests,
mini-vocabulary decoding/replay, runtime assembly and C ABI pipeline tests.
The latter expectations remain unchanged. The following changed expectations
are each licensed by the linked S1/S2 amendment:

| Changed expectation | Attribution |
| --- | --- |
| NaN at length 3 loses to finite cost 10 at length 3, formerly a tie | [S1/S2 special-cost completion](compatibility-policy.md#amendment--witnessed-algorithm-precondition-violations-2026-10-28-utc) |
| NaN at length 3 loses to finite cost 10 at length 4, formerly preferred | [S1/S2 special-cost completion](compatibility-policy.md#amendment--witnessed-algorithm-precondition-violations-2026-10-28-utc) |
| Finite cost 10 at length 4 no longer loses to NaN at length 3 | [S1/S2 special-cost completion](compatibility-policy.md#amendment--witnessed-algorithm-precondition-violations-2026-10-28-utc) |
| `(length=3,cost=0)` no longer loses to `(2,263)`; they tie | [S1/S2 threshold equivalence](compatibility-policy.md#amendment--witnessed-algorithm-precondition-violations-2026-10-28-utc) |
| Former cycle's `(6,68730)` no longer loses to `(4,70848)` | [S1/S2 adjusted-cost ordering](compatibility-policy.md#amendment--witnessed-algorithm-precondition-violations-2026-10-28-utc) |

The 500-row and 8,592-input live sentence checks require real-table inputs and
are ignored by ordinary CI. Their active tests validate fixture structure and
the existing six-residual membership, not new decoder outputs. Frozen corpus
generation and capture-replay tests validate their committed inputs/captures.
No frozen corpus or golden is regenerated. No workflow invokes the legacy
`nbest-train-diff` gate; its verdict machinery is not extended here.

The focused BDB ranking selection reuses `contract-diff.py` and its complete
sentence-rank observer, including the captured S1/S2 spellings and table-off
controls. Every intentionally moved case must match one complete pin/subject
pair below and report `POLICY-ACCEPTED`; other observations fail. The broader
behavioral campaign, including corpus-wide movements, is PR4.

## Exact registered expectations

Status: accepted under the S1/S2 ruling. `nbest-user-table-order-pinyin`
registers BDB observations only. The three lambda cases of the original
selection and the four cases added in #1400 (`table-conf-row-removed-gb`,
`table-conf-not-used-gb`, `table-conf-lambda-negative-half`,
`table-conf-lambda-negative-two`) register bdb, kc and tkrzw, the cells their
case registrations cover. No other backend receives an unverified exemption. Each
case has one recorded pin/subject pair per registered cell, including the
unchanged fresh-profile diagnostic: the block keeps the SHA-256 of each
side's complete observation, a digest per field and the divergence as text,
written by `just policy-record` from the differential's `--observations`
capture.

| Moved case | Attribution |
| --- | --- |
| `nbest-user-table-order-pinyin` | S2: equal-count, equal-length final candidates retain earlier candidate-batch position, so 靐靐 precedes 龘龘; the pin heap reverses that tie. |
| `table-conf-lambda-one-and-a-half` | S1/S2 special-cost completion: finite blended paths precede NaN unigram paths in beam and final-tail selection; only sentence/NBEST-prefix observations change. |
| `table-conf-lambda-two` | S1/S2 special-cost completion: finite blended paths precede NaN unigram paths in beam and final-tail selection; only sentence/NBEST-prefix observations change. |
| `table-conf-lambda-million` | S1/S2 special-cost completion: finite blended paths precede NaN unigram paths in beam and final-tail selection; only sentence/NBEST-prefix observations change. |
| `table-conf-row-removed-gb` | S2 tie rule: with the GB dictionary row removed, 蠔, 獆, 皥 and 滈 tie at count 2 among the final tails. The pin breaks the tie by heap pop order (`get_top_results`, `src/lookup/phonetic_lookup.h:142-171 at 074a2219`, over the candidates `get_candidates` lists in node and slot order, `:272-297`, from `get_tails`, `:330-341`) and gets 獆; the ruled key breaks it by earlier batch position and gets 蠔. |
| `table-conf-not-used-gb` | The same S2 tie rule with `gb_char.dbin NOT_USED`: the pin's heap pops 獆, the ruled key keeps 蠔. |
| `table-conf-lambda-negative-half` | S1/S2 special-cost completion: a negative λ leaves `bigram_lambda` negative and `unigram_lambda = 1 − λ` above one (`src/lookup/phonetic_lookup.h:712-713 at 074a2219`), so unigram steps stay finite (`:663`) while a blended step (`:692-693`) takes the log of a negative number, NaN, wherever −λ·b > (1−λ)·u. The ruled order puts NaN last; the pin's comparator does not order NaN at all. |
| `table-conf-lambda-negative-two` | The same special-cost completion at λ = −2. |

For the tie case, the user-table reading order and equal counts are established
by `nbest_user_table_order`; no intermediate beam follows the complete
two-character user phrase before S2. For the three lambda cases, the negative
unigram coefficient produces NaN step costs (`src/lookup/phonetic_lookup.h:
651-663,679-693,712-713 at 074a2219`). `expand_entries` expands every bigram
predecessor and only the beam head for the independent unigram pass, so
S1 ordering propagates into the final range. S2 uses the same explicit
finite-before-NaN rule. These cases jointly cover S1 and S2; this record
does not claim that their effects can be isolated from each other. The
normal-candidate frequency comparator, lambda acceptance, training/save
flags, serialized-file observations and diagnostics remain unchanged.

```contract-diff-policy
{
  "schema": 2,
  "record": "trellis-adjusted-cost-s1-s2",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "nbest-user-table-order-pinyin": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "230618673887b264fd2ab073e6f48bd098c815a713947488a89d2f133bdd2ba4",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.guess": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "4b227777d4dd1fc6",
              "result.sentences": "1422b22e90caaabe",
              "result.tokens": "920d9f0a3cf6e8f6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "5909151a43070ed65f3d9f5fe5e2bc87bbc1ef1150e217d8f66f6bef35dbecc6",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.guess": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.parsed": "4b227777d4dd1fc6",
              "result.sentences": "f0205e082cc82619",
              "result.tokens": "920d9f0a3cf6e8f6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.sentences[0]: subject inserts [[true, \"靐靐\"]]",
            "result.sentences[1:2]: subject drops [[true, \"靐靐\"]]"
          ]
        }
      }
    },
    "table-conf-lambda-one-and-a-half": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "0892a14248fce3a653b275f99ac7dab46e6b7b76bbdd2016802fd34494a6b988",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "2d64dda9ed3c8c54",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "87711d1f62881f51efb6bb51ff9e36ef415bb507af28b7c8c2332c050ef8320d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d84626b538139986",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[0:1]: pin [\"鯓虠\"]; subject [\"你好\"]",
            "result.view.nihao[2:4]: pin [130, [\"鯓虠\", \"鯓号\", \"鯓嚆\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\"]]; subject [128, [\"你好\", \"拟好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[0:1]: pin [\"呭䜹\"]; subject [\"艺术\"]",
            "result.view.yishu[2:4]: pin [608, [\"呭䜹\", \"呭墅\", \"呭疋\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]; subject [606, [\"艺术\", \"一书\", \"亦属\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "ec4b6b6b7fa56451b97b4784972733442c51de9649b097348ae28bc93675a8d9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "2d64dda9ed3c8c54",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "d69e000455f163fc679f5d074339eaa61b47c0b85b43a2c1866fa1338e1fc63e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d84626b538139986",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[0:1]: pin [\"鯓虠\"]; subject [\"你好\"]",
            "result.view.nihao[2:4]: pin [130, [\"鯓虠\", \"鯓号\", \"鯓嚆\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\"]]; subject [128, [\"你好\", \"拟好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[0:1]: pin [\"呭䜹\"]; subject [\"艺术\"]",
            "result.view.yishu[2:4]: pin [608, [\"呭䜹\", \"呭墅\", \"呭疋\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]; subject [606, [\"艺术\", \"一书\", \"亦属\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "004f3021f4e78291d76f51f6796d1bfdb5123f416d1b64fad2b54f2331666be2",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "2d64dda9ed3c8c54",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "209484f9aa4d8fa834705aa2287c66d0f8ed4f6a3f62eec89e0b43bb385bc12a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d84626b538139986",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[0:1]: pin [\"鯓虠\"]; subject [\"你好\"]",
            "result.view.nihao[2:4]: pin [130, [\"鯓虠\", \"鯓号\", \"鯓嚆\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\"]]; subject [128, [\"你好\", \"拟好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[0:1]: pin [\"呭䜹\"]; subject [\"艺术\"]",
            "result.view.yishu[2:4]: pin [608, [\"呭䜹\", \"呭墅\", \"呭疋\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]; subject [606, [\"艺术\", \"一书\", \"亦属\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]"
          ]
        }
      }
    },
    "table-conf-lambda-two": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "0892a14248fce3a653b275f99ac7dab46e6b7b76bbdd2016802fd34494a6b988",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "2d64dda9ed3c8c54",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "50abae6089fcf29ed83759a7346449056b855e9bc402219f23fdd369821bd116",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "9bb3651ca99bff08",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[0:1]: pin [\"鯓虠\"]; subject [\"你好\"]",
            "result.view.nihao[2:4]: pin [130, [\"鯓虠\", \"鯓号\", \"鯓嚆\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\"]]; subject [128, [\"你好\", \"拟好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[0:1]: pin [\"呭䜹\"]; subject [\"一书\"]",
            "result.view.yishu[2:4]: pin [608, [\"呭䜹\", \"呭墅\", \"呭疋\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]; subject [606, [\"一书\", \"亦属\", \"医术\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "ec4b6b6b7fa56451b97b4784972733442c51de9649b097348ae28bc93675a8d9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "2d64dda9ed3c8c54",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "f3f020b317b834b5cb1f9e1e9d278b887683a2a5f70d99efc18c86cbd15d9ecc",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "9bb3651ca99bff08",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[0:1]: pin [\"鯓虠\"]; subject [\"你好\"]",
            "result.view.nihao[2:4]: pin [130, [\"鯓虠\", \"鯓号\", \"鯓嚆\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\"]]; subject [128, [\"你好\", \"拟好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[0:1]: pin [\"呭䜹\"]; subject [\"一书\"]",
            "result.view.yishu[2:4]: pin [608, [\"呭䜹\", \"呭墅\", \"呭疋\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]; subject [606, [\"一书\", \"亦属\", \"医术\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "004f3021f4e78291d76f51f6796d1bfdb5123f416d1b64fad2b54f2331666be2",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "2d64dda9ed3c8c54",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "04aabe0fc2e20e41f3fea49e51c9b97d0a5e37e16f2394b2d163485ff6af7336",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "9bb3651ca99bff08",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[0:1]: pin [\"鯓虠\"]; subject [\"你好\"]",
            "result.view.nihao[2:4]: pin [130, [\"鯓虠\", \"鯓号\", \"鯓嚆\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\"]]; subject [128, [\"你好\", \"拟好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[0:1]: pin [\"呭䜹\"]; subject [\"一书\"]",
            "result.view.yishu[2:4]: pin [608, [\"呭䜹\", \"呭墅\", \"呭疋\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]; subject [606, [\"一书\", \"亦属\", \"医术\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\"]]"
          ]
        }
      }
    },
    "table-conf-lambda-million": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "0892a14248fce3a653b275f99ac7dab46e6b7b76bbdd2016802fd34494a6b988",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "2d64dda9ed3c8c54",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "4845e87768041f239c36a4a0c8fc167941bb75ef2c55a9fac09436291832e9d6",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "77790fd42bca7411",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[0:1]: pin [\"鯓虠\"]; subject [\"拟好\"]",
            "result.view.nihao[2:4]: pin [130, [\"鯓虠\", \"鯓号\", \"鯓嚆\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\"]]; subject [128, [\"拟好\", \"你好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[0:1]: pin [\"呭䜹\"]; subject [\"亦属\"]",
            "result.view.yishu[3][0:3]: pin [\"呭䜹\", \"呭墅\", \"呭疋\"]; subject [\"亦属\", \"亦舒\", \"忆述\"]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "ec4b6b6b7fa56451b97b4784972733442c51de9649b097348ae28bc93675a8d9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "2d64dda9ed3c8c54",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "cafbe1aebb6128835378fa25539fa5ca31fa2428fc0b564b252839ab4ffc61f6",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "77790fd42bca7411",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[0:1]: pin [\"鯓虠\"]; subject [\"拟好\"]",
            "result.view.nihao[2:4]: pin [130, [\"鯓虠\", \"鯓号\", \"鯓嚆\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\"]]; subject [128, [\"拟好\", \"你好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[0:1]: pin [\"呭䜹\"]; subject [\"亦属\"]",
            "result.view.yishu[3][0:3]: pin [\"呭䜹\", \"呭墅\", \"呭疋\"]; subject [\"亦属\", \"亦舒\", \"忆述\"]"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "004f3021f4e78291d76f51f6796d1bfdb5123f416d1b64fad2b54f2331666be2",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "2d64dda9ed3c8c54",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "3470a839687e9f3b54d48e2507dde5153953160cd0214a8022c20a732ae82324",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "77790fd42bca7411",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[0:1]: pin [\"鯓虠\"]; subject [\"拟好\"]",
            "result.view.nihao[2:4]: pin [130, [\"鯓虠\", \"鯓号\", \"鯓嚆\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\"]]; subject [128, [\"拟好\", \"你好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[0:1]: pin [\"呭䜹\"]; subject [\"亦属\"]",
            "result.view.yishu[3][0:3]: pin [\"呭䜹\", \"呭墅\", \"呭疋\"]; subject [\"亦属\", \"亦舒\", \"忆述\"]"
          ]
        }
      }
    },
    "table-conf-row-removed-gb": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "2b1b5d7154019da714e5c168a2d44dc109a0912282eea000a1f4e4d34976123d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "cdb46438f4467f8f",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "25db380361edaa78",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e8cca336d41f2b3292211586d71a34f5366e374b7de19c38b69f721f0eee481b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "cdb46438f4467f8f",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d922208e89b89c77",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[3][2:3]: pin [\"抳獆\"]; subject [\"抳蠔\"]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "8f829b1e05b1f4b22cb9e426a699aa9858884988684df74144741ccd77b242d0",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "ff88306b67ae32da",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "25db380361edaa78",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "d7784e55b56da0960049807d5a1cdafd7aca0253121130e2b1fa3075c175c529",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "ff88306b67ae32da",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d922208e89b89c77",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[3][2:3]: pin [\"抳獆\"]; subject [\"抳蠔\"]"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "a88d14d70e11bd748ccad633e3a183a62b89640eb4b663a2ee0677b07d3bb26d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d8308defa59586f5",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "25db380361edaa78",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "68b10fff1c6d3f59e64b8d1ff9a69337596238548b61124bb4fb202b1ddb6a76",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d8308defa59586f5",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d922208e89b89c77",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[3][2:3]: pin [\"抳獆\"]; subject [\"抳蠔\"]"
          ]
        }
      }
    },
    "table-conf-not-used-gb": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "2b1b5d7154019da714e5c168a2d44dc109a0912282eea000a1f4e4d34976123d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "cdb46438f4467f8f",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "25db380361edaa78",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e8cca336d41f2b3292211586d71a34f5366e374b7de19c38b69f721f0eee481b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "cdb46438f4467f8f",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d922208e89b89c77",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[3][2:3]: pin [\"抳獆\"]; subject [\"抳蠔\"]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "8f829b1e05b1f4b22cb9e426a699aa9858884988684df74144741ccd77b242d0",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "ff88306b67ae32da",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "25db380361edaa78",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "d7784e55b56da0960049807d5a1cdafd7aca0253121130e2b1fa3075c175c529",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "ff88306b67ae32da",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d922208e89b89c77",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[3][2:3]: pin [\"抳獆\"]; subject [\"抳蠔\"]"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "a88d14d70e11bd748ccad633e3a183a62b89640eb4b663a2ee0677b07d3bb26d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d8308defa59586f5",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "25db380361edaa78",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "68b10fff1c6d3f59e64b8d1ff9a69337596238548b61124bb4fb202b1ddb6a76",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d8308defa59586f5",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "d922208e89b89c77",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[3][2:3]: pin [\"抳獆\"]; subject [\"抳蠔\"]"
          ]
        }
      }
    },
    "table-conf-lambda-negative-half": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "7d9eb17bb8d368ecee7ecbc0b483295f1a2d7a7ad578ecf813d3987a6025f919",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "885d6a59869ecff0",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "1ae4eab0b27ecb3c7de10b82bacc0106e6a589f16db81256288ecacaac124c1f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "249bc1dc62b25476",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[2:4]: pin [127, [\"你好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\", \"儗\"]]; subject [128, [\"你好\", \"你号\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[2:4]: pin [606, [\"艺术\", \"一数\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]; subject [605, [\"艺术\", \"一书\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "fb3b25ac23c2b40579fc53538f89c55a41cf684b0484fd9fc09f37af62f9cd35",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "885d6a59869ecff0",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "1209a1ae8019ce3f7ec0af180d7f6745d4e7d802be86c39cff8fd9ceee874e9b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "249bc1dc62b25476",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[2:4]: pin [127, [\"你好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\", \"儗\"]]; subject [128, [\"你好\", \"你号\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[2:4]: pin [606, [\"艺术\", \"一数\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]; subject [605, [\"艺术\", \"一书\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "b93b30473490b74e19d03a9438048ff0a920ed962ec2a4654e71cdc1e7379b5c",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "885d6a59869ecff0",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "7079b954089295d0403842a47afcf35d5e4a22450267b3f41687b0639ebddeb5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "249bc1dc62b25476",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[2:4]: pin [127, [\"你好\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\", \"儗\"]]; subject [128, [\"你好\", \"你号\", \"你好吗\", \"疒\", \"祢\", \"禰\", \"妳\", \"呢\", \"堄\", \"逆\", \"猊\", \"霓\"]]",
            "result.view.yishu[2:4]: pin [606, [\"艺术\", \"一数\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]; subject [605, [\"艺术\", \"一书\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]"
          ]
        }
      }
    },
    "table-conf-lambda-negative-two": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "25760322e89aabe6cca3b07bf14787b6687f2a49602f8d8762a97c5e1a7a4766",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "82a2aa88df6f8bdb",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "1ae4eab0b27ecb3c7de10b82bacc0106e6a589f16db81256288ecacaac124c1f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "721d12a09e791bca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "249bc1dc62b25476",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[3][1:2]: pin [\"你豪\"]; subject [\"你号\"]",
            "result.view.yishu[2:4]: pin [606, [\"艺术\", \"一数\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]; subject [605, [\"艺术\", \"一书\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "81c71054b2d587e22171cb667980098f57cafad623afc907b05bb8e7ad2b013e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "82a2aa88df6f8bdb",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "1209a1ae8019ce3f7ec0af180d7f6745d4e7d802be86c39cff8fd9ceee874e9b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "d7cbcc7be5547102",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "249bc1dc62b25476",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[3][1:2]: pin [\"你豪\"]; subject [\"你号\"]",
            "result.view.yishu[2:4]: pin [606, [\"艺术\", \"一数\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]; subject [605, [\"艺术\", \"一书\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "dbd2cefb0ee06fee02fb269a78817ce7a0c345ccec7b1e867b4097ca362ec8af",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "82a2aa88df6f8bdb",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "7079b954089295d0403842a47afcf35d5e4a22450267b3f41687b0639ebddeb5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.add": "b5bea41b6c623f7c",
              "result.files": "bb1a4a924c7368ca",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "result.view": "249bc1dc62b25476",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.view.nihao[3][1:2]: pin [\"你豪\"]; subject [\"你号\"]",
            "result.view.yishu[2:4]: pin [606, [\"艺术\", \"一数\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]; subject [605, [\"艺术\", \"一书\", \"艺术家\", \"艾\", \"饎\", \"搋\", \"眙\", \"扡\", \"誃\", \"昳\", \"斁\", \"睪\"]]"
          ]
        }
      }
    }
  }
}
```

## Verification

The 20-case BDB selection reports 20 MATCH on base production code and
16 MATCH plus four POLICY-ACCEPTED on the repair. No other selected observation
moves. The four witness/table-off cases and all three zhuyin controls remain
MATCH. This conclusion covers the explicit selection below, not a corpus sweep.
The base run uses the same four additional case registrations, without changing
base production code or adding policy exemptions.

A later full bdb sweep found further cases moved by the same repair:
`table-conf-row-removed-gb`,
`table-conf-not-used-gb`, `table-conf-lambda-negative-half` and
`table-conf-lambda-negative-two` (#1400). They are registered above with
their bdb, kc and tkrzw pairs, and the three lambda cases of the original
selection gained their kc and tkrzw pairs at the same time. The focused
seven-case registration check reports 7 POLICY-ACCEPTED on each of bdb, kc
and tkrzw, with no policy diagnosis. All retained entries are unchanged from
the previous #1400 head. The three `longer-normal-*` cases are not registered
here; the separate #913/#1398 split owns their deduplication fixture and S2
witness.

Original repair verification: 196 unit tests, 39 integration tests and four doctests
passed. The focused contract-driver and verdict harnesses passed 163 and 35
tests respectively. Formatting and engine Clippy passed. No existing session,
replay or frozen-corpus golden changed; the five comparator expectation changes
are listed above.

```sh
just heavy -- cargo fmt --all --check
just clippy --locked -p oxpinyin-engine --all-targets -- -D warnings
just heavy -- cargo test --locked -p oxpinyin-engine
just test-tools tools/bisection/contract-diff.test.py tools/bisection/contract_verdict.test.py
bash tools/oracle/compat.sh --runtime podman --cell bdb --runners contract-diff --cases trellis-adjusted-score/resplit-jianai-on,trellis-adjusted-score/resplit-jianai-off,trellis-adjusted-score/divided-bian-on,trellis-adjusted-score/divided-bian-off,glib-sort-tails-pinyin,glib-sort-tails-zhuyin,nbest-user-table-order-pinyin,nbest-user-table-order-zhuyin,j-choose-nbest-diff-result,final-step-pinyin-ni-20,final-step-pinyin-nihao-20,final-step-pinyin-n-28,table-conf-lambda-one,table-conf-lambda-one-zhuyin,table-conf-lambda-one-and-a-half,table-conf-lambda-two,table-conf-lambda-million,itemless-user-total-guess-above-one,itemless-user-total-guess-above-one-control,import-wrapped-facade-total-guess
uv run --no-project --managed-python python -X lazy_imports=normal tools/bisection/run-all.py --runners contract-diff --cells bdb,kc,tkrzw --cases table-conf-row-removed-gb,table-conf-not-used-gb,table-conf-lambda-negative-half,table-conf-lambda-negative-two,table-conf-lambda-one-and-a-half,table-conf-lambda-two,table-conf-lambda-million
```

The #1400 registration check used the supported cloud setup on native Debian
trixie Linux amd64, repository Rust, debug opt-level 1 libraries, verified
oracle/data manifests for each cell, and the sound tkrzw 1.0.34 runtime.

The compatibility run used canonical Debian trixie Linux amd64, the matching
unpatched BDB reference and verified model20 data. Native tests and shipping
builds used Ubuntu 26.10 x86_64 and repository Rust 1.97.1. The harness wrapper
requires disk-backed TMPDIR, as documented in the setup guide.

Both revisions' shipping files were built in the same run with the BDB
release-stage recipe and equivalent source-path remapping, and verified
stripped. Each regular shipping file is counted once (#864):

| Shipping library | Base `2506b264` | Repair | Increase |
| --- | ---: | ---: | ---: |
| `libpinyin.so.15.0.0` | 2,478,832 bytes | 2,478,832 bytes | 0 |
| `libzhuyin.so.15.0.0` | 2,228,976 bytes | 2,228,976 bytes | 0 |

Run separately at base and repair, using each checkout's own target directory:

```sh
RUSTFLAGS="--remap-path-prefix=$PWD=/oxpinyin" just heavy -- just packaging release-stage bdb --prefix=/usr --libdir=/usr/lib
file target/release-stage/bdb/usr/lib/libpinyin.so.15.0.0 target/release-stage/bdb/usr/lib/libzhuyin.so.15.0.0
stat -c '%n %s' target/release-stage/bdb/usr/lib/libpinyin.so.15.0.0 target/release-stage/bdb/usr/lib/libzhuyin.so.15.0.0
```
