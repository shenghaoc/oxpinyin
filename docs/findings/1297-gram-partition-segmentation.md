# 1297-gram-partition-segmentation — phrase segmentation over stored grams

Status: **accepted** · 2026-10-29T13:19:09Z (`date -u`) · Refs #1297.
Policy: the 2026-10-29 UTC “No ported external-library internals” ruling
in [AGENTS.md, Source policy](../../AGENTS.md#source-policy), merged in #1402.
The [inventory](glib-libc-inventory.md#executable-copies-of-external-library-algorithms-2026-10-29-utc) identifies
stored gram searches as C and carried pinyin entry searches as D.
Records use the digest format merged in #1400.

## Contract and derivation

The pin is libpinyin 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`.
The local libpinyin repository was read with `git show 074a2219:<path>`;
its working HEAD is not the pin. `src/storage/ngram.cpp:153–271` searches
stored records before its get/set/insert/remove/range walks. Its writers
normally maintain token order; `merge_single_gram` (`:308–349`) interleaves
the two arrays without sorting, cleaning, or combining later duplicates.
Foreign stored values can therefore violate the search predicate's
partition requirement. `single_gram::lower_bound_records` replaces the
pin's search with `slice::partition_point` on the existing whole-record
view, as required by the ruling. The native row codecs preserve physical
order; `retrieve_all` and `mask_out` do not search and stay unchanged.

Sorted and predicate-partitioned records keep first-not-less semantics,
including duplicates and misses at both ends. Unpartitioned input has
std's unspecified result; the recorded observations name Rust 1.97.1
(`rust-toolchain.toml`), without a cross-toolchain promise. This is a
recorded library implementation difference, outside classes (a)–(c),
not a product ranking change or a change to a persistent format.

## Segmentation difference and evidence

`user-gram-unsorted-phrase-segment` plants a sentence-start gram with total
2³¹ + 2 and stored (token, count) pairs (0x10059f5, 2³¹), (0x10059fc, 1),
(0x10059f4, 1), then segments 今天天气很好.
The pin's stored-order interleave and `get_freq` walk
(`ngram.cpp:231–250,308–349`, `phrase_lookup.cpp:268–290`) miss the boosted
今天 record. The Rust `partition_point` search reaches it. The pin selects
今天天气 as one phrase; Rust selects 今天 and 天气 separately. Both segmenters
return true. All three registered cells (bdb, kc, sound tkrzw) produce the
pair digested below, including the unchanged final 很 and 好 tokens.

Both facades reach gram search through merged system/user grams for
sentence guessing, dynamic adjustment and segmentation. This record
accepts only the phrase-selection difference in the named segmentation
case. The independent training termination/result difference has its own
[record](1297-gram-partition-training.md).

`user-gram-unsorted-guess`, `user-gram-unsorted-dynamic-adjust`,
`user-gram-unsorted-range-boundary` and `system-gram-unsorted-guess` stay
MATCH. Duplicate, zero-count, short-header and partial-tail cases also
keep their existing outcomes. Nothing sorts or cleans a stored gram.
C ABI/layout, real GArray/g_free ownership, lifetime, file formats,
data ownership, process-wide effects and refusal conditions are unchanged.
The facade total remains awaiting ruling.

## Verification

The existing `just runner contract-diff` driver checks the complete stable
observation digests below. `just policy-record` records the per-cell digests
from its `--observations` capture; timings and unrequested file snapshots
remain outside the stable observation. Only
`user-gram-unsorted-phrase-segment` gains this record.
On each registered cell, after sourcing the provisioned environment:

```bash
just heavy -- just runner contract-diff "$CELL" "$PREFIX" "$SUBJECT_P" "$SUBJECT_Z" \
  -- --cases user-gram-unsorted-phrase-segment --observations "obs-$CELL.jsonl"
just policy-record docs/findings/1297-gram-partition-segmentation.md "obs-$CELL.jsonl" \
  --cases user-gram-unsorted-phrase-segment
```

The case-module harness is `tools/bisection/contract_gram_lookup.test.py`.
C's original core tests cover sorted duplicates, empty/short headers,
both-end misses, all partial-tail lengths and stored-order preservation.
The original broader selection remains in `fixtures/w4/1297-search-cases.txt`;
its unchanged scope and the base/PR/pin comparison are documented in the
bench runbook. No raw captures or verification logs are committed.

```contract-diff-policy
{
  "schema": 2,
  "record": "1297-gram-partition-segmentation",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "user-gram-unsorted-phrase-segment": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "ad5ae085147d6c6a4dfe30d4dca32957416a618a519bc1e6f4018649fea5682a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.今天天气很好": "7e7ddc65bbd4049a",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c7c18548ba7f627cbee44ebf7b5ecf89c87497ca45d9bebd2ee0c148b13d56b5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.今天天气很好": "23f15e1108a077a6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.今天天气很好 (list lengths pin 2, subject 2; first change [1]): pin [50356528, 0, 0] … (length 6); subject [16800245, 0, 16810538] … (length 6)",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "ad5ae085147d6c6a4dfe30d4dca32957416a618a519bc1e6f4018649fea5682a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.今天天气很好": "7e7ddc65bbd4049a",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c7c18548ba7f627cbee44ebf7b5ecf89c87497ca45d9bebd2ee0c148b13d56b5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.今天天气很好": "23f15e1108a077a6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.今天天气很好 (list lengths pin 2, subject 2; first change [1]): pin [50356528, 0, 0] … (length 6); subject [16800245, 0, 16810538] … (length 6)",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "ad5ae085147d6c6a4dfe30d4dca32957416a618a519bc1e6f4018649fea5682a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.今天天气很好": "7e7ddc65bbd4049a",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c7c18548ba7f627cbee44ebf7b5ecf89c87497ca45d9bebd2ee0c148b13d56b5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.今天天气很好": "23f15e1108a077a6",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.今天天气很好 (list lengths pin 2, subject 2; first change [1]): pin [50356528, 0, 0] … (length 6); subject [16800245, 0, 16810538] … (length 6)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```
