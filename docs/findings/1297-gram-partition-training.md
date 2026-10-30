# 1297-gram-partition-training — training over stored grams

Status: **accepted** · 2026-10-29T13:19:09Z (`date -u`) · Refs #1297.
Policy: the 2026-10-29 UTC “No ported external-library internals” ruling
in [Source policy](../source-policy.md), merged in #1402.
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

## Training difference and evidence

`abort-train-unsorted-set-freq-miss-pinyin` and
`abort-train-unsorted-set-freq-miss-zhuyin` train the constrained 你好 guess
against a five-item user sentence-start gram. Relative to 你, its successors
are stored in order -4, -3, -2, +1, -1, each with count one and total five.
`get_freq` misses, `insert_freq` appends, and the pin's final search misses
again (`ngram.cpp:178–205,231–271`). The check of
`user->set_freq(token, freq + seed)` in `phonetic_lookup.h:888–903` therefore
aborts. Under Rust's `partition_point`, insertion and the following search
reach the inserted item: training returns true with no warning. The digests
below retain each facade's exact SIGABRT assertion and successful Rust
result in its complete stable observation. They are the same on all three
registered cells (bdb, kc, sound tkrzw).

Both facades reach these searches through user grams for training and editing.
A search that actually misses after insertion still returns the existing
training refusal; std's successful search is not forced to reproduce another
implementation's miss. This record accepts only the training termination/result
difference in these two facade cases. The independent segmentation difference
has its own [record](1297-gram-partition-segmentation.md).

The existing case IDs, fixtures, captured pin data and pin termination markers
remain. The registry permits this policy record to retain an existing pin abort
using the verifier's exact-SIGABRT support; a changed signal, diagnostic or result
still fails. Other signal/profile/host exception combinations remain rejected.
Duplicate, zero-count, short-header and partial-tail cases keep their existing
outcomes. Nothing sorts or cleans a stored gram. C ABI/layout, real GArray/g_free
ownership, lifetime, file formats, data ownership, process-wide effects and
existing refusal conditions are unchanged. The facade total remains awaiting ruling.

## Verification

The existing `just runner contract-diff` driver checks the complete stable
observation digests below. `just policy-record` records the per-cell digests
from its `--observations` capture; timings and unrequested file snapshots
remain outside the stable observation. Only the two
`abort-train-unsorted-set-freq-miss-*` cases gain this record. On each
registered cell, after sourcing the provisioned environment:

```bash
just heavy -- just runner contract-diff "$CELL" "$PREFIX" "$SUBJECT_P" "$SUBJECT_Z" \
  -- --cases abort-train-unsorted-set-freq-miss-pinyin,abort-train-unsorted-set-freq-miss-zhuyin --observations "obs-$CELL.jsonl"
just policy-record docs/findings/1297-gram-partition-training.md "obs-$CELL.jsonl" \
  --cases abort-train-unsorted-set-freq-miss-pinyin,abort-train-unsorted-set-freq-miss-zhuyin
```

The case-module harness is `tools/bisection/contract_stored_user_gram.test.py`.
C's original user tests check that training preflight and apply agree and that
refusal is atomic, without promising a position for unpartitioned search.
The original broader selection remains in `fixtures/w4/1297-search-cases.txt`;
its unchanged scope and the base/PR/pin comparison are documented in the
bench runbook. No raw captures or verification logs are committed.

```contract-diff-policy
{
  "schema": 2,
  "record": "1297-gram-partition-training",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "abort-train-unsorted-set-freq-miss-pinyin": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "ca7b98643c3f2df795684e8a7f9cc50110af922e990d981733e801706f58f434",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "fa51107d17a7b4a3"
            }
          },
          "subject": {
            "sha256": "8dc42634d77923dbbeb620faec38806dffff88b1c2f5b9dd55f36e930ee5ea4e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.ret": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"logs\": [], \"ret\": true}",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.pytho\" … (length 351); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "ca7b98643c3f2df795684e8a7f9cc50110af922e990d981733e801706f58f434",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "fa51107d17a7b4a3"
            }
          },
          "subject": {
            "sha256": "8dc42634d77923dbbeb620faec38806dffff88b1c2f5b9dd55f36e930ee5ea4e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.ret": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"logs\": [], \"ret\": true}",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.pytho\" … (length 351); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "ca7b98643c3f2df795684e8a7f9cc50110af922e990d981733e801706f58f434",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "fa51107d17a7b4a3"
            }
          },
          "subject": {
            "sha256": "8dc42634d77923dbbeb620faec38806dffff88b1c2f5b9dd55f36e930ee5ea4e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.ret": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"logs\": [], \"ret\": true}",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.pytho\" … (length 351); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "abort-train-unsorted-set-freq-miss-zhuyin": {
      "cells": {
        "bdb": {
          "pin": {
            "sha256": "de71f109a52778531f1c33865d8b8a34f4e2c9f2a668fa717b3719e3d93849db",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "d8e7a3e5cfdd5468"
            }
          },
          "subject": {
            "sha256": "8dc42634d77923dbbeb620faec38806dffff88b1c2f5b9dd55f36e930ee5ea4e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.ret": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"logs\": [], \"ret\": true}",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.pytho\" … (length 351); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "de71f109a52778531f1c33865d8b8a34f4e2c9f2a668fa717b3719e3d93849db",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "d8e7a3e5cfdd5468"
            }
          },
          "subject": {
            "sha256": "8dc42634d77923dbbeb620faec38806dffff88b1c2f5b9dd55f36e930ee5ea4e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.ret": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"logs\": [], \"ret\": true}",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.pytho\" … (length 351); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "tkrzw": {
          "pin": {
            "sha256": "de71f109a52778531f1c33865d8b8a34f4e2c9f2a668fa717b3719e3d93849db",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "d8e7a3e5cfdd5468"
            }
          },
          "subject": {
            "sha256": "8dc42634d77923dbbeb620faec38806dffff88b1c2f5b9dd55f36e930ee5ea4e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.ret": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"logs\": [], \"ret\": true}",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.pytho\" … (length 351); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```
