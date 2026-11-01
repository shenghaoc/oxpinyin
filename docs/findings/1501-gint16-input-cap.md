# 1501-gint16-input-cap — the full-pinyin parse's 16-bit steps

- **Status:** accepted · 2026-11-01 UTC · issue #1501 · PR #1502
- **Disposition:** POLICY-ACCEPTED (never MATCH)
- **Policy limb:** language artefact (limb 3; the 2026-10-29 UTC "Contract, not implementation" ruling): the stop is a storage width; not (a), (b) or (c). Maintainer decision 2026-11-01 UTC in #1501.
- **Pin:** libpinyin 2.11.92 at 074a2219 (fresh clone; `pinyin_parser2.cpp` blob `47c58761`); GLib/libc: not involved; facade: both; backend: all.
- **Pin derivation:** `parse_value_t` keeps its steps in `gint16` (`pinyin_parser2.cpp:67-70`); `final_step` returns the last step whose sign-extended `m_parsed_len` equals its index (`:353-357`), never above 32767, and `parse` returns it (`:333`).
- **Rust counterpart:** `oxpinyin_core::graph::SegmentGraph`, `oxpinyin_engine::Session` (`session/buffer.rs`), `parse_import_pinyin` (`crates/oxpinyin-capi/src/iterators.rs`).
- **Forward trace:** `*_parse_more_full_pinyins` → `Session::replace_raw` → the graph → the parsed length, keys, candidates; `pinyin_iterator_add_phrase` → `parse_import_pinyin` → `_add_phrase` (`pinyin.cpp:541`).
- **Backward trace:** only `FullPinyinParser2` has 16-bit steps (both `parse_more_full_pinyins` entries, `pinyin.cpp:631-637`, `gen_pinyin_table.cpp:176-181`); the double-pinyin and zhuyin parsers count in `int`.
- **Input partition:** input up to 32767 bytes agrees; only full-pinyin input or import readings longer than 32767 bytes differ.
- **Difference:** past 32767 bytes the pin parses the first 32767 and oxpinyin the whole input; oxpinyin's `MAX_INPUT_BYTES` clamp, import cut and `MAX_GRAPH_INPUT` refusal are removed.
- **Why accepted:** a C++ storage width; oxpinyin keeps graph positions and candidate spans in `u32`, refusing only past `MAX_GRAPH_POSITION` (`u32::MAX - 1` bytes).
- **Contract preserved:** C ABI, layout, ownership and on-disk formats unchanged; an over-long import reading stores its full key list.
- **Affected inputs:** full-pinyin input, pinyin import readings and `gen_pinyin_table` lines longer than 32767 bytes, on both facades and every scheme.
- **Tests:** `contract-diff.py`: POLICY-ACCEPTED `input-length-boundary-32768`, `input-length-boundary-65536`; MATCH control `input-length-boundary-32767`; `combined-library-sixth-add-long-reading-pinyin` unchanged; mutation `tools/bisection/mutations/1501-restore-gint16-cap.patch`.
- **Prior rulings:** none reversed; policy row 59 (#540) is reopened.
- **Evidence:** `just runner contract-diff <cell> "$PINYIN_ORACLE_PREFIX" <capi.so> <zhuyin.so> -- --cases input-length-boundary-32768,input-length-boundary-65536 --observations obs.jsonl`, then `just policy-record docs/findings/1501-gint16-input-cap.md obs.jsonl`, on bdb, kc and tkrzw.
- **Conclusion:** accepted. Maintainer decision 2026-11-01 UTC (#1501).

```contract-diff-policy
{
  "schema": 2,
  "record": "1501-gint16-input-cap",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "input-length-boundary-32768": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "36ff900318f3be39f1d64fc33115665b7afe94dde321c60cbd7a3e8634a1a4ab",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.32768": "8a70afe339e4959c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "ccc5f61101ca1b2a763eae46b3e2d368ecc46854d73a9e924704dd0e55080b12",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.32768": "f53395eaa5f662f0",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.32768[0:2]: pin [32767, 32767]; subject [32768, 32768]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "36ff900318f3be39f1d64fc33115665b7afe94dde321c60cbd7a3e8634a1a4ab",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.32768": "8a70afe339e4959c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "ccc5f61101ca1b2a763eae46b3e2d368ecc46854d73a9e924704dd0e55080b12",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.32768": "f53395eaa5f662f0",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.32768[0:2]: pin [32767, 32767]; subject [32768, 32768]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "36ff900318f3be39f1d64fc33115665b7afe94dde321c60cbd7a3e8634a1a4ab",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.32768": "8a70afe339e4959c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "ccc5f61101ca1b2a763eae46b3e2d368ecc46854d73a9e924704dd0e55080b12",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.32768": "f53395eaa5f662f0",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.32768[0:2]: pin [32767, 32767]; subject [32768, 32768]"
          ]
        }
      }
    },
    "input-length-boundary-65536": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "2af300dc068e9b0d35cd3a2b266899bef09576f24f69863fa4a99ee53a631c47",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.65536": "8a70afe339e4959c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "0f2d99140518d44739cdc206ae6f823038273df2895d7b03ef7b2c09e445b437",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.65536": "250c49096a48b336",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.65536[0:2]: pin [32767, 32767]; subject [65536, 65536]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "2af300dc068e9b0d35cd3a2b266899bef09576f24f69863fa4a99ee53a631c47",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.65536": "8a70afe339e4959c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "0f2d99140518d44739cdc206ae6f823038273df2895d7b03ef7b2c09e445b437",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.65536": "250c49096a48b336",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.65536[0:2]: pin [32767, 32767]; subject [65536, 65536]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "2af300dc068e9b0d35cd3a2b266899bef09576f24f69863fa4a99ee53a631c47",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.65536": "8a70afe339e4959c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "0f2d99140518d44739cdc206ae6f823038273df2895d7b03ef7b2c09e445b437",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.65536": "250c49096a48b336",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.65536[0:2]: pin [32767, 32767]; subject [65536, 65536]"
          ]
        }
      }
    }
  }
}
```
