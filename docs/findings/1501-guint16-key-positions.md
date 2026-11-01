# 1501-guint16-key-positions — key positions past byte 65535

- **Status:** accepted · 2026-11-01 UTC · issue #1501 · PR to follow
- **Disposition:** MATCH for the parsed length and the key-rest position
  and length getters; POLICY-ACCEPTED for matrix placement and the
  results that follow from it.
- **Policy limb:** split by the maintainer's ruling of 2026-11-01 UTC
  ("Amendment — key positions past byte 65535" in
  [compatibility-policy.md](compatibility-policy.md)).
  - The getter values are contract (decision-order limb 1).
    `ChewingKeyRest`'s begin and end are `guint16`, and the C getters
    hand out `guint16` positions. C++ defines the conversion from an
    `int` position to `guint16` as reduction mod 2^16, so the wrap is
    part of what the ABI prescribes. oxpinyin reproduces it.
  - Matrix placement is an explicit maintainer-ruling exception, not a
    language artefact. The wrap is contract, and only the pin's use of
    the wrapped begin as a matrix column is waived, for input no IME
    produces. It is not class (a): no arithmetic result is involved. It
    is not class (b): every pin index stays in bounds. It is not class
    (c), which was retired 2026-10-31 UTC, and nothing aborts.
- **Pin:** libpinyin 2.11.92 at 074a2219. Tree read: a fresh clone at
  that commit, with the blobs hashed against the pin:
  `src/storage/chewing_key.h` `7ec4dfb4`,
  `src/storage/pinyin_parser2.cpp` `47c58761`,
  `src/storage/zhuyin_parser2.cpp` `4ef4500b`,
  `src/storage/phonetic_key_matrix.cpp` `ab7e8796`, `src/pinyin.cpp`
  `f27f7cf7`, `src/zhuyin.cpp` `d1520c18`,
  `src/lookup/phonetic_lookup.h` `c092e761`. GLib/libc of the reference
  image: not involved. Facade: both. Backend: all. The derivation is from
  source alone. The narrowing is to an unsigned type, which C++ defines,
  so unlike [1501-gint16-input-cap](1501-gint16-input-cap.md) no object
  code was read.
- **Pin derivation.** N is the parsed length, with N > 65535, and
  `w(x) = x mod 65536`.
  1. `DoublePinyinParser2::parse` (`pinyin_parser2.cpp:532-572`) keeps
     its lengths in `int`s. So do `ZhuyinSimpleParser2`,
     `ZhuyinDiscreteParser2` and `ZhuyinDaChenCP26Parser2`
     (`zhuyin_parser2.cpp:216-267`, `:408-459`, `:718-794`). Each stores
     `m_raw_begin = parsed_len; m_raw_end = parsed_len + i` (`:565`;
     `:259`, `:451`, `:760`) into the `guint16` fields
     (`chewing_key.h:102-103`), so the stored rests are `(w(b), w(e))`.
  2. The facades keep the unwrapped `int`: `pinyin_parse_more_double_pinyins`
     (`pinyin.cpp:1550-1557`), `pinyin_parse_more_chewings`
     (`:1595-1602`) and `zhuyin_parse_more_chewings`
     (`zhuyin.cpp:1068-1074`). The return value and
     `*_get_parsed_input_length` answer N.
  3. `fill_matrix` (`phonetic_key_matrix.cpp:29-83`):
     - It sizes the matrix to N+1 columns (`:47-48`).
     - It appends each key at its wrapped begin `w(b)` (`:51-55`), so the
       matrix is a ring of 65536 columns.
     - It puts the end zero key at the real column N, with the rest
       `(w(N), w(N+1))` (`:59-62`).

     As a result, columns 65536 to N−1 are empty, and each column below
     65536 holds one key per 65536-byte lap, the first lap in row 0. The
     key that crosses byte 65536 has an end below its begin. The `'` gap
     filler compares wrapped ends with wrapped begins (`:67-78`) and adds
     nothing on these gapless inputs. `fuzzy_syllable_step` (`:224-308`),
     the only later step on these paths, copies rests unchanged.
  4. Matrix walks step to `key_rest.m_raw_end` and stop when
     `start > end` (`search_matrix_recur`,
     `compute_pronunciation_possibility_recur`,
     `increase_pronunciation_possibility_recur`, `:341-668`). With
     wrapped ends, a walk can chain keys from different laps, and the
     crossing key's end sends it back towards column 0.
     `get_nbest_match` still iterates every column up to N
     (`phonetic_lookup.h:763-818`).
  5. Getters, `pinyin.cpp:2921-3093`, mirrored at `zhuyin.cpp:1886-2060`:
     - The key and key-rest getters answer `false` at an empty column
       (`pinyin.cpp:2930-2931`), so they answer `false` for every offset
       from 65536 to N−1. Below 65536 they answer row 0, the first lap's
       key.
     - `*_key_rest_positions` copies the two `guint16`s (`:2967-2977`).
     - `*_key_rest_length` is a `guint16` difference
       (`chewing_key.h:111-113`), so the crossing key's length is its
       true length.
     - `*_get_*_offset` for a cursor from 65536 to N−1 scans down past
       the empty columns to a column below 65536 (`:3010-3029`).
     - The right offset of the crossing key is its wrapped end, a column
       to its left (`:3083-3085`).
     - The left offset matches `offset == m_raw_end` against wrapped
       ends, so it never finds a key ending at 65536 or past it
       (`:3043-3053`).
     - Candidates and the sentence run on the matrix of step 3.
- **Rust counterpart.**
  - The segment graph and session keep real `u32` positions
    (`oxpinyin_core::graph::SegmentGraph`, `oxpinyin_engine::Session`).
  - The facade's ABI columns sit at real offsets
    (`InstanceCore::abi_key_at`, `crates/oxpinyin-facade/src/cursor.rs`).
  - The C adapters narrow a rest with `as u16`, which wraps
    (`pinyin_get_pinyin_key_rest`, `crates/oxpinyin-capi/src/cursor.rs`;
    `zhuyin_get_zhuyin_key_rest`, `crates/oxpinyin-zhuyin-capi/src/cursor.rs`).
    Before this record they saturated at 65535.
  - The length getters compute `end.wrapping_sub(begin)` over the slot.
- **Forward trace:** `pinyin_parse_more_double_pinyins`,
  `pinyin_parse_more_chewings` and `zhuyin_parse_more_chewings` →
  `InstanceCore` parse → the session's key spans at real positions → the
  key, key-rest, position and length getters, the offset getters, the
  auxiliary text, candidates and sentences.
- **Backward trace:** the only producers of a position past 65535 at the
  pin are the double-pinyin parser and the three keyboard chewing
  parsers above. The full-pinyin parse stops at 32767 bytes
  ([1501-gint16-input-cap](1501-gint16-input-cap.md)).
  `ZhuyinDirectParser2` feeds only the import iterator (`zhuyin.cpp:516`)
  and `gen_zhuyin_table` (`utils/storage/gen_zhuyin_table.cpp:182`),
  which build no matrix.
- **Input partition:**
  - Valid supported input: compositions up to 65535 bytes. No position
    wraps, and both sides agree.
  - Malformed, non-scalar or host-dependent input: not involved.
  - Other (over-long): a double-pinyin or chewing composition longer than
    65535 bytes, on either facade. Only this partition differs.
- **Difference.** The inputs are N = 65541 bytes: `ni3` + 32769 × `ni`
  (MS double pinyin) or `su3` + 32769 × `su` (standard keyboard), with
  `USE_TONE`. Keys begin at 0, 3, 5, … 65539. The key at 65535..65537
  crosses byte 65536, and the key at 65537..65539 lies wholly past it.

  | Result | Pin | oxpinyin | Disposition |
  | --- | --- | --- | --- |
  | parse return, `*_get_parsed_input_length` | 65541 | 65541 | MATCH |
  | key, key-rest positions and length at 65535 (the crossing key) | `(65535, 1)`, length 2 | `(65535, 1)`, length 2 | MATCH |
  | key and key-rest at 65537 and 65539 | `false` (empty column) | `true`, `(1, 3)` and `(3, 5)`, length 2 | POLICY-ACCEPTED |
  | `*_get_*_offset(65538)` | 65535 (the highest occupied column below 65536) | the real key start | POLICY-ACCEPTED |
  | right offset at 65535 | 1 (the wrapped end) | 65537 | POLICY-ACCEPTED |
  | left offset at 65537 | 0 (no wrapped end equals 65537) | 65535 | POLICY-ACCEPTED |
  | candidates and sentence past 65535 | from the ring matrix of step 3 | from real positions | POLICY-ACCEPTED, not cased |

  The pin's answers above come from the derivation. The cases capture
  both sides.
- **Why accepted:** the maintainer's ruling of 2026-11-01 UTC. A ring of
  65536 columns places syllables that are 65536 bytes apart in one
  column, which no IME composition reaches. oxpinyin keeps one column
  per input byte.
- **Contract preserved:** the C ABI and layout are unchanged, including
  `ChewingKeyRest`'s two `guint16`s. Positions past 65535 reach the
  caller wrapped, as the pin's do. Ownership, lifetime and the
  process-wide key and key-rest slots are unchanged, every export is
  unchanged, and on-disk files are untouched. Diagnostics: none
  involved. No pin assert is reached on these paths: the zhuyin
  `_check_offset` assert (reached from `zhuyin_get_zhuyin_key` at `zhuyin.cpp:1898`)
  needs a zero key in the previous column, and the only zero key is at
  column N.
- **Affected inputs:** double-pinyin and chewing compositions longer than
  65535 bytes, on both facades (the zhuyin facade has no double-pinyin
  parser).
- **Tests:** `contract-diff.py` cases in
  `tools/bisection/contract_key_positions_past_65535.py`, for each of
  `double-pinyin` and `chewing-pinyin` (pinyin facade) and
  `chewing-zhuyin` (zhuyin facade):
  - MATCH: `key-position-past-65535/<parser>-crossing-key`, which covers
    the parse return, the parsed length, and the crossing key's key,
    positions and length.
  - POLICY-ACCEPTED: `key-position-past-65535/<parser>-past-key` (the key
    getters at 65537 and 65539) and
    `key-position-past-65535/<parser>-cursor` (`*_get_*_offset(65538)`,
    right at 65535, left at 65537).
  - Named mutation `tools/bisection/mutations/1501-saturate-key-rest-positions.patch`
    restores the saturation. Under it the three `-crossing-key` cases
    must DIFFER.
  - Unit tests: `key_rest_positions_past_byte_65535_wrap` in
    `crates/oxpinyin-capi/src/cursor.rs` and
    `crates/oxpinyin-zhuyin-capi/src/e2e_tests.rs`.
  - Candidates and the sentence past 65535 bytes are not cased: the pin's
    cost on the ring matrix (step 4) is not derived, and a case that does
    not finish would be INVALID on every cell.
- **Prior rulings:** the 2026-11-01 UTC #1501 decision on the `gint16`
  steps stands. This record corrects that record's claim that
  double-pinyin and chewing past 65535 bytes match the pin.
- **Evidence:** the digests below come from `just policy-record` over an
  `--observations` capture of the cases above on tkrzw, the only cell
  `cells-for-diff.py` selects:

  ```sh
  just runner contract-diff <cell> "$PINYIN_ORACLE_PREFIX" <capi.so> <zhuyin.so> -- --family key-position-past-65535 --observations obs.jsonl
  just policy-record docs/findings/1501-guint16-key-positions.md obs.jsonl
  ```
- **Conclusion:** accepted. Maintainer ruling 2026-11-01 UTC (#1501).

```contract-diff-policy
{
  "schema": 2,
  "record": "1501-guint16-key-positions",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "key-position-past-65535/chewing-pinyin-cursor": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "f82ba4bda9b417dee0b41d34f6a416362512dde2bf377f4d4840bb4b155422b1",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.left 65537": "2705bd6ffb3d9453",
              "result.logs": "4f53cda18c2baa0c",
              "result.offset 65538": "475ad3c42b626ab9",
              "result.right 65535": "2bf55f259687bc08",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e92737eea14cea9cc70453c690890b36b0aef6efc691d9ab8f18e01e6192573d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.left 65537": "475ad3c42b626ab9",
              "result.logs": "4f53cda18c2baa0c",
              "result.offset 65538": "2a50ec2c37ec307e",
              "result.right 65535": "2a50ec2c37ec307e",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.left 65537[1:2]: pin [0]; subject [65535]",
            "result.offset 65538[1:2]: pin [65535]; subject [65537]",
            "result.right 65535[1:2]: pin [1]; subject [65537]"
          ]
        }
      }
    },
    "key-position-past-65535/chewing-pinyin-past-key": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "27666531445ac8b8cb76d8e4a90aada85956d48cde515ee7351ec9d1eab7172d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.rest 65537": "456e2e3fa05ee1e2",
              "result.rest 65539": "456e2e3fa05ee1e2",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "78be0e2a2c01c7683b6f53d7d322e2d227fbad748e7d907c27e854479111ad87",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.rest 65537": "5c53850023b08bdc",
              "result.rest 65539": "9c44033e4d8b71e3",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.rest 65537[0:1]: pin [false]; subject [true, \"2b00\", true] … (length 7)",
            "result.rest 65539[0:1]: pin [false]; subject [true, \"2b00\", true] … (length 7)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "key-position-past-65535/chewing-zhuyin-cursor": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "f82ba4bda9b417dee0b41d34f6a416362512dde2bf377f4d4840bb4b155422b1",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.left 65537": "2705bd6ffb3d9453",
              "result.logs": "4f53cda18c2baa0c",
              "result.offset 65538": "475ad3c42b626ab9",
              "result.right 65535": "2bf55f259687bc08",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e92737eea14cea9cc70453c690890b36b0aef6efc691d9ab8f18e01e6192573d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.left 65537": "475ad3c42b626ab9",
              "result.logs": "4f53cda18c2baa0c",
              "result.offset 65538": "2a50ec2c37ec307e",
              "result.right 65535": "2a50ec2c37ec307e",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.left 65537[1:2]: pin [0]; subject [65535]",
            "result.offset 65538[1:2]: pin [65535]; subject [65537]",
            "result.right 65535[1:2]: pin [1]; subject [65537]"
          ]
        }
      }
    },
    "key-position-past-65535/chewing-zhuyin-past-key": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "27666531445ac8b8cb76d8e4a90aada85956d48cde515ee7351ec9d1eab7172d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.rest 65537": "456e2e3fa05ee1e2",
              "result.rest 65539": "456e2e3fa05ee1e2",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "78be0e2a2c01c7683b6f53d7d322e2d227fbad748e7d907c27e854479111ad87",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.rest 65537": "5c53850023b08bdc",
              "result.rest 65539": "9c44033e4d8b71e3",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.rest 65537[0:1]: pin [false]; subject [true, \"2b00\", true] … (length 7)",
            "result.rest 65539[0:1]: pin [false]; subject [true, \"2b00\", true] … (length 7)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "key-position-past-65535/double-pinyin-cursor": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "f82ba4bda9b417dee0b41d34f6a416362512dde2bf377f4d4840bb4b155422b1",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.left 65537": "2705bd6ffb3d9453",
              "result.logs": "4f53cda18c2baa0c",
              "result.offset 65538": "475ad3c42b626ab9",
              "result.right 65535": "2bf55f259687bc08",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e92737eea14cea9cc70453c690890b36b0aef6efc691d9ab8f18e01e6192573d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.left 65537": "475ad3c42b626ab9",
              "result.logs": "4f53cda18c2baa0c",
              "result.offset 65538": "2a50ec2c37ec307e",
              "result.right 65535": "2a50ec2c37ec307e",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.left 65537[1:2]: pin [0]; subject [65535]",
            "result.offset 65538[1:2]: pin [65535]; subject [65537]",
            "result.right 65535[1:2]: pin [1]; subject [65537]"
          ]
        }
      }
    },
    "key-position-past-65535/double-pinyin-past-key": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "27666531445ac8b8cb76d8e4a90aada85956d48cde515ee7351ec9d1eab7172d",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.rest 65537": "456e2e3fa05ee1e2",
              "result.rest 65539": "456e2e3fa05ee1e2",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "78be0e2a2c01c7683b6f53d7d322e2d227fbad748e7d907c27e854479111ad87",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.rest 65537": "5c53850023b08bdc",
              "result.rest 65539": "9c44033e4d8b71e3",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.rest 65537[0:1]: pin [false]; subject [true, \"2b00\", true] … (length 7)",
            "result.rest 65539[0:1]: pin [false]; subject [true, \"2b00\", true] … (length 7)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```
