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

## Exact BDB expectations

Status: accepted under the S1/S2 ruling. Only BDB observations are registered.
No other backend receives an unverified exemption. Each case has one complete
pin/subject pair, including the unchanged fresh-profile diagnostic.

| Moved case | Attribution |
| --- | --- |
| `nbest-user-table-order-pinyin` | S2: equal-count, equal-length final candidates retain earlier candidate-batch position, so 靐靐 precedes 龘龘; the pin heap reverses that tie. |
| `table-conf-lambda-one-and-a-half` | S1/S2 special-cost completion: finite blended paths precede NaN unigram paths in beam and final-tail selection; only sentence/NBEST-prefix observations change. |
| `table-conf-lambda-two` | S1/S2 special-cost completion: finite blended paths precede NaN unigram paths in beam and final-tail selection; only sentence/NBEST-prefix observations change. |
| `table-conf-lambda-million` | S1/S2 special-cost completion: finite blended paths precede NaN unigram paths in beam and final-tail selection; only sentence/NBEST-prefix observations change. |

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
  "schema": 1,
  "record": "trellis-adjusted-cost-s1-s2",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "nbest-user-table-order-pinyin": {
      "cells": {
        "bdb": {
          "pin": {
            "exit": 0,
            "result": {
              "guess": true,
              "logs": [],
              "parsed": 4,
              "sentences": [
                [
                  true,
                  "龘龘"
                ],
                [
                  true,
                  "靐靐"
                ],
                [
                  true,
                  "大大"
                ]
              ],
              "tokens": [
                "0x7000001",
                "0x7000002"
              ]
            },
            "stderr": "open SCRATCH/user-X/user.conf failed."
          },
          "subject": {
            "exit": 0,
            "result": {
              "guess": true,
              "logs": [],
              "parsed": 4,
              "sentences": [
                [
                  true,
                  "靐靐"
                ],
                [
                  true,
                  "龘龘"
                ],
                [
                  true,
                  "大大"
                ]
              ],
              "tokens": [
                "0x7000001",
                "0x7000002"
              ]
            },
            "stderr": "open SCRATCH/user-X/user.conf failed."
          }
        }
      }
    },
    "table-conf-lambda-one-and-a-half": {
      "cells": {
        "bdb": {
          "pin": {
            "exit": 0,
            "result": {
              "add": true,
              "files": {
                "addon.bin": 27,
                "gb_char.dbin": 26,
                "gbk_char.dbin": 26,
                "merged.dbin": 26,
                "network.bin": 27,
                "opengram.dbin": 26,
                "user.bin": 71,
                "user.conf": "binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|",
                "user_bigram.db": null,
                "user_phrase_index.bin": "dbm",
                "user_pinyin_index.bin": "dbm"
              },
              "logs": [],
              "save": true,
              "train": true,
              "view": {
                "nihao": [
                  "鯓虠",
                  true,
                  130,
                  [
                    "鯓虠",
                    "鯓号",
                    "鯓嚆",
                    "你好吗",
                    "疒",
                    "祢",
                    "禰",
                    "妳",
                    "呢",
                    "堄",
                    "逆",
                    "猊"
                  ]
                ],
                "yishu": [
                  "呭䜹",
                  true,
                  608,
                  [
                    "呭䜹",
                    "呭墅",
                    "呭疋",
                    "艺术家",
                    "艾",
                    "饎",
                    "搋",
                    "眙",
                    "扡",
                    "誃",
                    "昳",
                    "斁"
                  ]
                ]
              }
            },
            "stderr": "open SCRATCH/user-X/user.conf failed."
          },
          "subject": {
            "exit": 0,
            "result": {
              "add": true,
              "files": {
                "addon.bin": 27,
                "gb_char.dbin": 26,
                "gbk_char.dbin": 26,
                "merged.dbin": 26,
                "network.bin": 27,
                "opengram.dbin": 26,
                "user.bin": 71,
                "user.conf": "binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|",
                "user_bigram.db": null,
                "user_phrase_index.bin": "dbm",
                "user_pinyin_index.bin": "dbm"
              },
              "logs": [],
              "save": true,
              "train": true,
              "view": {
                "nihao": [
                  "你好",
                  true,
                  128,
                  [
                    "你好",
                    "拟好",
                    "你好吗",
                    "疒",
                    "祢",
                    "禰",
                    "妳",
                    "呢",
                    "堄",
                    "逆",
                    "猊",
                    "霓"
                  ]
                ],
                "yishu": [
                  "艺术",
                  true,
                  606,
                  [
                    "艺术",
                    "一书",
                    "亦属",
                    "艺术家",
                    "艾",
                    "饎",
                    "搋",
                    "眙",
                    "扡",
                    "誃",
                    "昳",
                    "斁"
                  ]
                ]
              }
            },
            "stderr": "open SCRATCH/user-X/user.conf failed."
          }
        }
      }
    },
    "table-conf-lambda-two": {
      "cells": {
        "bdb": {
          "pin": {
            "exit": 0,
            "result": {
              "add": true,
              "files": {
                "addon.bin": 27,
                "gb_char.dbin": 26,
                "gbk_char.dbin": 26,
                "merged.dbin": 26,
                "network.bin": 27,
                "opengram.dbin": 26,
                "user.bin": 71,
                "user.conf": "binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|",
                "user_bigram.db": null,
                "user_phrase_index.bin": "dbm",
                "user_pinyin_index.bin": "dbm"
              },
              "logs": [],
              "save": true,
              "train": true,
              "view": {
                "nihao": [
                  "鯓虠",
                  true,
                  130,
                  [
                    "鯓虠",
                    "鯓号",
                    "鯓嚆",
                    "你好吗",
                    "疒",
                    "祢",
                    "禰",
                    "妳",
                    "呢",
                    "堄",
                    "逆",
                    "猊"
                  ]
                ],
                "yishu": [
                  "呭䜹",
                  true,
                  608,
                  [
                    "呭䜹",
                    "呭墅",
                    "呭疋",
                    "艺术家",
                    "艾",
                    "饎",
                    "搋",
                    "眙",
                    "扡",
                    "誃",
                    "昳",
                    "斁"
                  ]
                ]
              }
            },
            "stderr": "open SCRATCH/user-X/user.conf failed."
          },
          "subject": {
            "exit": 0,
            "result": {
              "add": true,
              "files": {
                "addon.bin": 27,
                "gb_char.dbin": 26,
                "gbk_char.dbin": 26,
                "merged.dbin": 26,
                "network.bin": 27,
                "opengram.dbin": 26,
                "user.bin": 71,
                "user.conf": "binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|",
                "user_bigram.db": null,
                "user_phrase_index.bin": "dbm",
                "user_pinyin_index.bin": "dbm"
              },
              "logs": [],
              "save": true,
              "train": true,
              "view": {
                "nihao": [
                  "你好",
                  true,
                  128,
                  [
                    "你好",
                    "拟好",
                    "你好吗",
                    "疒",
                    "祢",
                    "禰",
                    "妳",
                    "呢",
                    "堄",
                    "逆",
                    "猊",
                    "霓"
                  ]
                ],
                "yishu": [
                  "一书",
                  true,
                  606,
                  [
                    "一书",
                    "亦属",
                    "医术",
                    "艺术家",
                    "艾",
                    "饎",
                    "搋",
                    "眙",
                    "扡",
                    "誃",
                    "昳",
                    "斁"
                  ]
                ]
              }
            },
            "stderr": "open SCRATCH/user-X/user.conf failed."
          }
        }
      }
    },
    "table-conf-lambda-million": {
      "cells": {
        "bdb": {
          "pin": {
            "exit": 0,
            "result": {
              "add": true,
              "files": {
                "addon.bin": 27,
                "gb_char.dbin": 26,
                "gbk_char.dbin": 26,
                "merged.dbin": 26,
                "network.bin": 27,
                "opengram.dbin": 26,
                "user.bin": 71,
                "user.conf": "binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|",
                "user_bigram.db": null,
                "user_phrase_index.bin": "dbm",
                "user_pinyin_index.bin": "dbm"
              },
              "logs": [],
              "save": true,
              "train": true,
              "view": {
                "nihao": [
                  "鯓虠",
                  true,
                  130,
                  [
                    "鯓虠",
                    "鯓号",
                    "鯓嚆",
                    "你好吗",
                    "疒",
                    "祢",
                    "禰",
                    "妳",
                    "呢",
                    "堄",
                    "逆",
                    "猊"
                  ]
                ],
                "yishu": [
                  "呭䜹",
                  true,
                  608,
                  [
                    "呭䜹",
                    "呭墅",
                    "呭疋",
                    "艺术家",
                    "艾",
                    "饎",
                    "搋",
                    "眙",
                    "扡",
                    "誃",
                    "昳",
                    "斁"
                  ]
                ]
              }
            },
            "stderr": "open SCRATCH/user-X/user.conf failed."
          },
          "subject": {
            "exit": 0,
            "result": {
              "add": true,
              "files": {
                "addon.bin": 27,
                "gb_char.dbin": 26,
                "gbk_char.dbin": 26,
                "merged.dbin": 26,
                "network.bin": 27,
                "opengram.dbin": 26,
                "user.bin": 71,
                "user.conf": "binary format version:7|model data version:14|database format:BerkeleyDB|open counter:0|",
                "user_bigram.db": null,
                "user_phrase_index.bin": "dbm",
                "user_pinyin_index.bin": "dbm"
              },
              "logs": [],
              "save": true,
              "train": true,
              "view": {
                "nihao": [
                  "拟好",
                  true,
                  128,
                  [
                    "拟好",
                    "你好",
                    "你好吗",
                    "疒",
                    "祢",
                    "禰",
                    "妳",
                    "呢",
                    "堄",
                    "逆",
                    "猊",
                    "霓"
                  ]
                ],
                "yishu": [
                  "亦属",
                  true,
                  608,
                  [
                    "亦属",
                    "亦舒",
                    "忆述",
                    "艺术家",
                    "艾",
                    "饎",
                    "搋",
                    "眙",
                    "扡",
                    "誃",
                    "昳",
                    "斁"
                  ]
                ]
              }
            },
            "stderr": "open SCRATCH/user-X/user.conf failed."
          }
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

Engine verification: 196 unit tests, 39 integration tests and four doctests
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
```

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
