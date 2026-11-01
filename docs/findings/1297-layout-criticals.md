# Phrase-library layout faults write no critical

Status: accepted, 2026-10-31 UTC, maintainer decision D1 of #1297 (a
narrow exception to the library stderr ruling, #545). Disposition:
POLICY-ACCEPTED. Rule: [source policy, no hand-written shims](../source-policy.md);
record form: [template](language-runtime-discrepancy-template.md).
Pin: libpinyin 074a2219 (`src/storage/phrase_index.cpp` blob `d58e37e2`,
`src/pinyin.cpp` blob `f27f7cf7`, `src/zhuyin.cpp` blob `d1520c18`), read
from a `git archive` of the pin; GLib 2.84.4 of the reference image.

GLib documents `g_return_val_if_fail` as a precondition assertion on the
caller's arguments, to be used at the top of a public function
(https://docs.gtk.org/glib/warnings.html). Neither site follows that
pattern. `SubPhraseIndex::load` checks file data inside an internal
method. `phrase_segment` checks the result of conversions it has already
made, not its argument on entry. oxpinyin therefore implements the
control flow these checks express, an early return of the pin's value,
and does not reproduce the critical.

`SubPhraseIndex::load` reads `m_total_freq` and the three offsets, then
checks the three separators and the end with `g_return_val_if_fail`
(`phrase_index.cpp:354-356`, `:361`). A chunk whose header and checksum
`MemoryChunk::mmap` accepts but whose layout fails a check (a header-only
chunk, a sparse chunk whose truncated payload is empty, a misplaced
second or third separator) makes the pin log GLib's critical in the
default domain, `bool pinyin::SubPhraseIndex::load(pinyin::MemoryChunk*,
table_offset_t, table_offset_t): assertion '<expr>' failed`, and return
`FALSE`. Every caller ignores that answer (`pinyin.cpp:264`, `:298`,
`:313`; `zhuyin.cpp:208`, `:242`) and goes on with an empty sub-index.
The init, a library load or reload, the save's remap of each system
library (`pinyin.cpp:956`, `zhuyin.cpp:589`) and `mask_out`'s reload
(`pinyin.cpp:1265`, `zhuyin.cpp:800`) reach it.

oxpinyin keeps all of that: the empty sub-index, its local `m_total_freq`
outside the facade total, the save's diff against the empty sub-index and
every return value and file. It makes no log call. The fault queue that
carried each fault from `oxpinyin-data` through the runtime and facade to
the C adapters, the copied `G_STRFUNC` string and the copied expressions
are gone; `LayoutFault` keeps only `total_freq`. Two observable
differences follow, both accepted:

- The critical is missing: a caller's GLib log handler receives nothing,
  and GLib's default handler writes nothing to stderr.
- Under GLib fatal-critical settings the pin aborts at the critical, for
  example in the save after the earlier libraries' files are written.
  oxpinyin does not abort; it completes the operation. **The
  fatal-critical behaviour is lost.** This is an intentional observable
  difference, not exact compatibility.

A layout the pin reaches only through an out-of-chunk read stays the
existing class (b) refusal (policy row 76); it is not affected.

Cases, bound below with `just policy-record` from tkrzw, bdb and kc captures (each
differs from the pin only in the missing critical, or for
`save-fatal-valid-to-invalid*` in the pin's SIGABRT under the fatal mask):
`header-only-library-init`, `header-only-library-init-zhuyin`, `header-only-library-load`, `header-only-library-load-zhuyin`, `header-only-library-save`, `nonzero-total-save`, `nonzero-total-save-zhuyin`, `save-fatal-valid-to-invalid`, `save-fatal-valid-to-invalid-zhuyin`, `save-replacement-invalid-to-valid`, `save-replacement-invalid-to-valid-zhuyin`, `save-replacement-valid-to-invalid`, `save-replacement-valid-to-invalid-zhuyin`, `save-unchanged-layout-failed`, `save-unchanged-layout-failed-zhuyin`, `second-separator-library-init`, `second-separator-library-init-zhuyin`, `short-chunk-layout`, `short-chunk-layout-zhuyin`, `sparse-library-addon`, `sparse-library-addon-reload`, `sparse-library-candidates`, `sparse-library-candidates-zhuyin`, `sparse-library-init`, `sparse-library-init-settled`, `sparse-library-init-settled-zhuyin`, `sparse-library-init-zhuyin`, `sparse-library-load`, `sparse-library-load-zhuyin`, `sparse-library-mask-out`, `sparse-library-mask-out-zhuyin`, `sparse-library-reinit`, `sparse-library-reinit-zhuyin`, `sparse-library-save`, `sparse-library-save-zhuyin`, `sparse-library-tokens`, `sparse-library-tokens-zhuyin`, `third-separator-library-init`, `third-separator-library-init-zhuyin`.
The list is `LAYOUT_CRITICAL_CASES` in `tools/bisection/contract_library_loading.py`.
Since class (c) was retired (2026-10-31 UTC), the
`nonzero-total-candidates` case and its zhuyin twin abort on both sides at
`pinyin.cpp:1859` and `zhuyin.cpp:1261`:
the pin's critical is in the result document the abort discards, so the
cases are compared by the `sigabrt` matcher ([pin-abort-assert-line.md](pin-abort-assert-line.md)),
not this record.
Controls that still MATCH include `crash-oob-layout*`, `save-unchanged-valid*`
and `save-replacement-valid-to-different-valid*`.

```contract-diff-policy
{
  "schema": 2,
  "record": "1297-layout-criticals",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "header-only-library-init": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "c6b29b4ac8311bc99a36b5ffbdfffbaa5fcf22471c11927bd2908c1d32edcdbf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "2018949a96b4826d568db34aee92f7495caec112f19c4c8bffe107af182ae3ea",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "c6b29b4ac8311bc99a36b5ffbdfffbaa5fcf22471c11927bd2908c1d32edcdbf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "2018949a96b4826d568db34aee92f7495caec112f19c4c8bffe107af182ae3ea",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "c6b29b4ac8311bc99a36b5ffbdfffbaa5fcf22471c11927bd2908c1d32edcdbf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "2018949a96b4826d568db34aee92f7495caec112f19c4c8bffe107af182ae3ea",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "header-only-library-init-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "c6b29b4ac8311bc99a36b5ffbdfffbaa5fcf22471c11927bd2908c1d32edcdbf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "2018949a96b4826d568db34aee92f7495caec112f19c4c8bffe107af182ae3ea",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "c6b29b4ac8311bc99a36b5ffbdfffbaa5fcf22471c11927bd2908c1d32edcdbf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "2018949a96b4826d568db34aee92f7495caec112f19c4c8bffe107af182ae3ea",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "c6b29b4ac8311bc99a36b5ffbdfffbaa5fcf22471c11927bd2908c1d32edcdbf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "2018949a96b4826d568db34aee92f7495caec112f19c4c8bffe107af182ae3ea",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "header-only-library-load": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "header-only-library-load-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "header-only-library-save": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "76d7acdeb3ce2a7d176388e93b6be6623aa61f7bbc11cec0114e03c8ed811196",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "4c59ebd47bb36013"
            }
          },
          "subject": {
            "sha256": "4d2fb6693c8351ac1b7335609065343556fcf334dd6eed1cc587d01f965acab6",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "4c59ebd47bb36013"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "d3478cc3cb3cfc5ddd11b3ede9d32f2d75eb7ad5360ecf17c070512a51463a27",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "cb31dbd126e4d1b6"
            }
          },
          "subject": {
            "sha256": "1ddb41de27dd6facbd3ca6090f91461440fcb6f710b7486d8f8de46f30619b89",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "cb31dbd126e4d1b6"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "5d4dbfab755d62e191fca118313c00f6dc82f6748bb2223313d57f5dff86bac8",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "1b265ce952e70252"
            }
          },
          "subject": {
            "sha256": "489868179fa8cdd6fab2bda4197b579af571470bb805d116c4dabef26ddce93f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "2c624232cdd22177",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "1b265ce952e70252"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "nonzero-total-save": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "594c449c95572a203a0725248265b672ec7c20407101d36969c2073b226a9b33",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "6b88d6695b5e8ba701c3622df221c38a9038d536d23b2be51b923b1f1f5a8956",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "594c449c95572a203a0725248265b672ec7c20407101d36969c2073b226a9b33",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "6b88d6695b5e8ba701c3622df221c38a9038d536d23b2be51b923b1f1f5a8956",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "594c449c95572a203a0725248265b672ec7c20407101d36969c2073b226a9b33",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "6b88d6695b5e8ba701c3622df221c38a9038d536d23b2be51b923b1f1f5a8956",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "nonzero-total-save-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "594c449c95572a203a0725248265b672ec7c20407101d36969c2073b226a9b33",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "6b88d6695b5e8ba701c3622df221c38a9038d536d23b2be51b923b1f1f5a8956",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "594c449c95572a203a0725248265b672ec7c20407101d36969c2073b226a9b33",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "6b88d6695b5e8ba701c3622df221c38a9038d536d23b2be51b923b1f1f5a8956",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "594c449c95572a203a0725248265b672ec7c20407101d36969c2073b226a9b33",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "6b88d6695b5e8ba701c3622df221c38a9038d536d23b2be51b923b1f1f5a8956",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.dbin": "f23d08f486106079",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "save-fatal-valid-to-invalid": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "9dccb5ee41c21aa14044d80a2ce0f4efd64a3cb16a3140f946ba088b17ec5466",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "38fda7ac81e31d08",
              "userfiles": "8cee4e15dbd7ce16"
            }
          },
          "subject": {
            "sha256": "44373a81acb593edaf2ddd7208cd3c7e86ea1da9cbb647e89d2fcd6393072e1b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "178d659599bf6084"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"l\" … (length 4): [], \"m\" … (length 6): <list length 3>, \"r\" … (length 7): <dict length 4>} … (length 5)",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.\\n** C\" … (length 205); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "userfiles.addon.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.gb_char.dbin: subject only [26, \"b0624c19cf007ca\" … (length 64)]",
            "userfiles.gb_char.dbin.tmp: pin only [26, \"b0624c19cf007ca\" … (length 64)]",
            "userfiles.gbk_char.dbin: subject only [26, \"f4f18f1863e8a94\" … (length 64)]",
            "userfiles.gbk_char.dbin.tmp: pin only [26, \"f4f18f1863e8a94\" … (length 64)]",
            "userfiles.merged.dbin: subject only [38252, \"59eed2f9ef43edc\" … (length 64)]",
            "userfiles.network.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.opengram.dbin: subject only [26, \"d3b9a15d9498085\" … (length 64)]",
            "userfiles.opengram.dbin.tmp: pin only [26, \"d3b9a15d9498085\" … (length 64)]",
            "userfiles.user.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.user_bigram.db: subject only [4198400]",
            "userfiles.user_phrase_index.bin: subject only [529408]",
            "userfiles.user_pinyin_index.bin: subject only [529408]",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "fba053ede03b4389ea0c7e3609fa7867809d97eb5159cedb4bde77e35ff0f05e",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "38fda7ac81e31d08",
              "userfiles": "a0c9a73f3d638bb3"
            }
          },
          "subject": {
            "sha256": "bac42f76a638cdac966fcb431f8d2638b0c632b970ba8fb6c0c00b0959b5bfde",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "ed5d8d69262c53df"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"l\" … (length 4): [], \"m\" … (length 6): <list length 3>, \"r\" … (length 7): <dict length 4>} … (length 5)",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.\\n** C\" … (length 205); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "userfiles.addon.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.gb_char.dbin: subject only [26, \"b0624c19cf007ca\" … (length 64)]",
            "userfiles.gb_char.dbin.tmp: pin only [26, \"b0624c19cf007ca\" … (length 64)]",
            "userfiles.gbk_char.dbin: subject only [26, \"f4f18f1863e8a94\" … (length 64)]",
            "userfiles.gbk_char.dbin.tmp: pin only [26, \"f4f18f1863e8a94\" … (length 64)]",
            "userfiles.merged.dbin: subject only [38252, \"59eed2f9ef43edc\" … (length 64)]",
            "userfiles.network.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.opengram.dbin: subject only [26, \"d3b9a15d9498085\" … (length 64)]",
            "userfiles.opengram.dbin.tmp: pin only [26, \"d3b9a15d9498085\" … (length 64)]",
            "userfiles.user.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.user_bigram.db: subject only [12288]",
            "userfiles.user_phrase_index.bin: subject only [8192]",
            "userfiles.user_pinyin_index.bin: subject only [8192]",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "2f36ed493bec782f547bb6205f5b8330e1e6bb0d3bf1bb69d1aafb4573e6f793",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "38fda7ac81e31d08",
              "userfiles": "2be5bd8b027249a9"
            }
          },
          "subject": {
            "sha256": "4fd0949448e879b55fb11808a96a38ee08fb8db780bf2af8ddf9cde8d44aed1f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "863067cf3ef15025"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"l\" … (length 4): [], \"m\" … (length 6): <list length 3>, \"r\" … (length 7): <dict length 4>} … (length 5)",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.\\n** C\" … (length 205); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "userfiles.addon.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.gb_char.dbin: subject only [26, \"b0624c19cf007ca\" … (length 64)]",
            "userfiles.gb_char.dbin.tmp: pin only [26, \"b0624c19cf007ca\" … (length 64)]",
            "userfiles.gbk_char.dbin: subject only [26, \"f4f18f1863e8a94\" … (length 64)]",
            "userfiles.gbk_char.dbin.tmp: pin only [26, \"f4f18f1863e8a94\" … (length 64)]",
            "userfiles.merged.dbin: subject only [38252, \"59eed2f9ef43edc\" … (length 64)]",
            "userfiles.network.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.opengram.dbin: subject only [26, \"d3b9a15d9498085\" … (length 64)]",
            "userfiles.opengram.dbin.tmp: pin only [26, \"d3b9a15d9498085\" … (length 64)]",
            "userfiles.user.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.user_bigram.db: subject only [7]",
            "userfiles.user_phrase_index.bin: subject only [7]",
            "userfiles.user_pinyin_index.bin: subject only [7]",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "save-fatal-valid-to-invalid-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "80ce777a594f87f4c386bb38c8748949077ca95f89102abcdddd5e848b4e3ccb",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "38fda7ac81e31d08",
              "userfiles": "7820c8f31835e91d"
            }
          },
          "subject": {
            "sha256": "822d5a86cbb512747dfe444f1c2a394aee8ac098fab452ecbff85d5ab663541e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "e2cce18b3c6ef021"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"l\" … (length 4): [], \"m\" … (length 6): <list length 3>, \"r\" … (length 7): <dict length 4>} … (length 5)",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.\\n** C\" … (length 205); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "userfiles.addon.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.merged.dbin: subject only [38252, \"59eed2f9ef43edc\" … (length 64)]",
            "userfiles.network.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.user.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.user.conf: subject only \"binary format version:7|model data version\" … (length 83)",
            "userfiles.user_bigram.db: subject only [4198400]",
            "userfiles.user_phrase_index.bin: subject only [529408]",
            "userfiles.user_pinyin_index.bin: subject only [529408]",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "80ce777a594f87f4c386bb38c8748949077ca95f89102abcdddd5e848b4e3ccb",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "38fda7ac81e31d08",
              "userfiles": "7820c8f31835e91d"
            }
          },
          "subject": {
            "sha256": "c4faa2aaa935294eace65826284e65c712a9bd0f39200196827676f1576cba47",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "e0975af2d059b50f"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"l\" … (length 4): [], \"m\" … (length 6): <list length 3>, \"r\" … (length 7): <dict length 4>} … (length 5)",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.\\n** C\" … (length 205); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "userfiles.addon.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.merged.dbin: subject only [38252, \"59eed2f9ef43edc\" … (length 64)]",
            "userfiles.network.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.user.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.user.conf: subject only \"binary format version:7|model data version\" … (length 88)",
            "userfiles.user_bigram.db: subject only [12288]",
            "userfiles.user_phrase_index.bin: subject only [8192]",
            "userfiles.user_pinyin_index.bin: subject only [8192]",
            "… further divergence omitted (4096-byte budget)"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "80ce777a594f87f4c386bb38c8748949077ca95f89102abcdddd5e848b4e3ccb",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "38fda7ac81e31d08",
              "userfiles": "7820c8f31835e91d"
            }
          },
          "subject": {
            "sha256": "f6a97b27004db0fc1fa1b521332655d41f3ad10bb47ff0be9dc7e44cedd2251e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "54c680870a6ecb74"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"l\" … (length 4): [], \"m\" … (length 6): <list length 3>, \"r\" … (length 7): <dict length 4>} … (length 5)",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.\\n** C\" … (length 205); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "userfiles.addon.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.merged.dbin: subject only [38252, \"59eed2f9ef43edc\" … (length 64)]",
            "userfiles.network.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.user.bin: subject only [27, \"4aeb3f46357bead\" … (length 64)]",
            "userfiles.user.conf: subject only \"binary format version:7|model data version\" … (length 90)",
            "userfiles.user_bigram.db: subject only [7]",
            "userfiles.user_phrase_index.bin: subject only [7]",
            "userfiles.user_pinyin_index.bin: subject only [7]",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "save-replacement-invalid-to-valid": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "d7692aebd301f7b36f047debbb5f045c9fdb0584436c300bbf1ef11c1068d89b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "c260433e2b09a26a"
            }
          },
          "subject": {
            "sha256": "8c87c2cbf5ef09ce3da27c503227e62220063fb51c7a5d50c19a1279a207589b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "c260433e2b09a26a"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "6e4038af16c2457dfab6dec30cf9a79943683fd1eec6354526f5172bab8d0bd2",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "ca61db4e22026a89"
            }
          },
          "subject": {
            "sha256": "eba2b2efee0a64edf4e3734ca3baf128218c61124d69e1716dfad2ba0340597f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "ca61db4e22026a89"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "a8fe104bb9b1e5430db041fbe833b161dbc7ffde472dc3f7d83b1ade56514a31",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "fb8da819f8dc52da"
            }
          },
          "subject": {
            "sha256": "798bb6521ecb6ec72472121f926946dbc6b323d2bf99b4ff3d55ec2fcbcecee2",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "fb8da819f8dc52da"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "save-replacement-invalid-to-valid-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "f4d7a1696bfa1e54111c197bc205ac4ad70b589d59fa77ede33707792b3955d2",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "6affa84ee84852e1"
            }
          },
          "subject": {
            "sha256": "244f4e67a6cdf5717114e21cc26cf5b902beff53eb1770311438c588ae2fabe0",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "6affa84ee84852e1"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "59ee6921855f4994a15a84bed964e35740432715031ca26d1eb3ad07360f2c76",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "61adfc6fb85927dd"
            }
          },
          "subject": {
            "sha256": "0823e38f7bc63b1343a603629298135f9eef12fefa99bc81acf888189a7aba00",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "61adfc6fb85927dd"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "e81a6442baf740abf9fb2e84625b9a6554b172af7d16097945ec33c9c4852864",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "5dd1be084bce080f"
            }
          },
          "subject": {
            "sha256": "031ec78bf1242c6ba0daf157fc99b6184e6b349929ad2ff72f25d03b38d6c3ae",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "a5362adf1eba9514",
              "result.records": "9d80b63785afde1a",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "5dd1be084bce080f"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "save-replacement-valid-to-invalid": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "8d63fbca3d4599cfe2515c6c2430e411a588c75f102d8f23696729810e5b5a27",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "178d659599bf6084"
            }
          },
          "subject": {
            "sha256": "44373a81acb593edaf2ddd7208cd3c7e86ea1da9cbb647e89d2fcd6393072e1b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "178d659599bf6084"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "2df7e35e154cae3b0ea60f706b6a107a653cd76803b977ad378fd0b47abe3775",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "ed5d8d69262c53df"
            }
          },
          "subject": {
            "sha256": "bac42f76a638cdac966fcb431f8d2638b0c632b970ba8fb6c0c00b0959b5bfde",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "ed5d8d69262c53df"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "7b722f05eda19f0fa15e295c4b82e182b9bdbe9c4c50478353a4e0554ec9fb03",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "863067cf3ef15025"
            }
          },
          "subject": {
            "sha256": "4fd0949448e879b55fb11808a96a38ee08fb8db780bf2af8ddf9cde8d44aed1f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "863067cf3ef15025"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "save-replacement-valid-to-invalid-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "8f91502bc495469e7dac5df78e2d655ab649b5af900823ecd48f8ec2b8d7d88a",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "e2cce18b3c6ef021"
            }
          },
          "subject": {
            "sha256": "822d5a86cbb512747dfe444f1c2a394aee8ac098fab452ecbff85d5ab663541e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "e2cce18b3c6ef021"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "114191dd139db50bee0ab40977a235da990a6ce03193f8279a281562b4b16f06",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "e0975af2d059b50f"
            }
          },
          "subject": {
            "sha256": "c4faa2aaa935294eace65826284e65c712a9bd0f39200196827676f1576cba47",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "e0975af2d059b50f"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "12635fcf34faab834323ab903d4acfeaa3c8c4615c2734fe05691424ca7d5b06",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "54c680870a6ecb74"
            }
          },
          "subject": {
            "sha256": "f6a97b27004db0fc1fa1b521332655d41f3ad10bb47ff0be9dc7e44cedd2251e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "4b40e3785e3de605",
              "result.records": "a710f99eb3cd867f",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "54c680870a6ecb74"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "save-unchanged-layout-failed": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "f9b9e04b140687dad138c79f6cd0b52a975cd8dcc99fa7e068f94e471363daca",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "b8eb79fbd43ead94"
            }
          },
          "subject": {
            "sha256": "b216f10440a1d23265f10ae21d261bf9a11688bd4f21cc47baedd12e6e75ec3c",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "b8eb79fbd43ead94"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "20556b9d973508c8b0bafc88bfb44d70d61da18d1668629f9299e02843f4c064",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "1385670dc32efe71"
            }
          },
          "subject": {
            "sha256": "3df0800b8e58d411ce33486f5951971a964fed9d57e373fdc55fe8033d854511",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "1385670dc32efe71"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "4a3ff60b7da38eb206893739d0a5481c0033916e6b4fad07a9a119be3e26d6be",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "22816b97d61c0be1"
            }
          },
          "subject": {
            "sha256": "2c8203a0c2cb7a9c378892b8233232b2113c2d9bc04b8bf59ad6aabd13155a66",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "22816b97d61c0be1"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "save-unchanged-layout-failed-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "975a4839185ed6d1c195c3e875fc8105606c9baa57eaab9326219559a487f90e",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "0c6fd2e7cf968128"
            }
          },
          "subject": {
            "sha256": "e6b7d7dcf98ca2ff3bfb5feb49efc9fb41bc11baa812ab2daa4a660382db46bf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "0c6fd2e7cf968128"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "7b9a56faeee8401134ca24b52fd1ad0fb2dcfb082620b3c3e2f05fc2d5fc82b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "d47d2de34aabd045"
            }
          },
          "subject": {
            "sha256": "26365128041dcd2a00a3f928726cebed29302879874d7c9060b515a6d5d5363f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "d47d2de34aabd045"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "a070ed74f7f86f84798a942753922f1abc8238558ee9738927ed96a09cb76373",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "56c39f98296362d5"
            }
          },
          "subject": {
            "sha256": "e82606a52e52ae46ade856e058d63771c70278709cfa382348d490feb877fadd",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.merged": "d934aac6c97f61a9",
              "result.records": "9bdf19f5d12e442e",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "56c39f98296362d5"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "second-separator-library-init": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "second-separator-library-init-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "short-chunk-layout": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "f6813b40eab39021ce264b7964ea3155c42d519a420a81cfe1136411afc19af7",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "38c7ac90d9a4e82ccda9e344787782bdea33f0a1b6b793a7d65ba7e384a15f35",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "f6813b40eab39021ce264b7964ea3155c42d519a420a81cfe1136411afc19af7",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "38c7ac90d9a4e82ccda9e344787782bdea33f0a1b6b793a7d65ba7e384a15f35",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "f6813b40eab39021ce264b7964ea3155c42d519a420a81cfe1136411afc19af7",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "38c7ac90d9a4e82ccda9e344787782bdea33f0a1b6b793a7d65ba7e384a15f35",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "short-chunk-layout-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "f6813b40eab39021ce264b7964ea3155c42d519a420a81cfe1136411afc19af7",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "38c7ac90d9a4e82ccda9e344787782bdea33f0a1b6b793a7d65ba7e384a15f35",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "f6813b40eab39021ce264b7964ea3155c42d519a420a81cfe1136411afc19af7",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "38c7ac90d9a4e82ccda9e344787782bdea33f0a1b6b793a7d65ba7e384a15f35",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "f6813b40eab39021ce264b7964ea3155c42d519a420a81cfe1136411afc19af7",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "38c7ac90d9a4e82ccda9e344787782bdea33f0a1b6b793a7d65ba7e384a15f35",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.init": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-addon": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "660cd082f6e4211fd2d7ceafe6fed6d0065ee0369f186873a418141bcf55320f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "afb8c873584ee0e3289c616b358e51f39ac2e3dd1ce3be14d818d3c73410c03b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "660cd082f6e4211fd2d7ceafe6fed6d0065ee0369f186873a418141bcf55320f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "afb8c873584ee0e3289c616b358e51f39ac2e3dd1ce3be14d818d3c73410c03b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "660cd082f6e4211fd2d7ceafe6fed6d0065ee0369f186873a418141bcf55320f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "afb8c873584ee0e3289c616b358e51f39ac2e3dd1ce3be14d818d3c73410c03b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-addon-reload": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "52293c632d29c0743af6fa0b7d124c74c65dc03d51289628b2806324c9327003",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.again": "b5bea41b6c623f7c",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "375527c1f80098c800007dc162055f2eca82e5da386510ae9cd7cce25e174faf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.again": "b5bea41b6c623f7c",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "52293c632d29c0743af6fa0b7d124c74c65dc03d51289628b2806324c9327003",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.again": "b5bea41b6c623f7c",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "375527c1f80098c800007dc162055f2eca82e5da386510ae9cd7cce25e174faf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.again": "b5bea41b6c623f7c",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "52293c632d29c0743af6fa0b7d124c74c65dc03d51289628b2806324c9327003",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.again": "b5bea41b6c623f7c",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "375527c1f80098c800007dc162055f2eca82e5da386510ae9cd7cce25e174faf",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.again": "b5bea41b6c623f7c",
              "result.first": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "sparse-library-candidates": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "63db02152ef9392fac0448b09536d18d5fbb2f13ced3cb986737c6f14cf97c76",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.n": "922c7954216ccfe7",
              "result.parse": "ef2d127de37b942b",
              "result.rows": "29a36b9be934743b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "8286314641fa99b2a8915045403a35e10001999c1934f9927dac415d1ee7890c",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.n": "922c7954216ccfe7",
              "result.parse": "ef2d127de37b942b",
              "result.rows": "29a36b9be934743b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "63db02152ef9392fac0448b09536d18d5fbb2f13ced3cb986737c6f14cf97c76",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.n": "922c7954216ccfe7",
              "result.parse": "ef2d127de37b942b",
              "result.rows": "29a36b9be934743b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "8286314641fa99b2a8915045403a35e10001999c1934f9927dac415d1ee7890c",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.n": "922c7954216ccfe7",
              "result.parse": "ef2d127de37b942b",
              "result.rows": "29a36b9be934743b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "63db02152ef9392fac0448b09536d18d5fbb2f13ced3cb986737c6f14cf97c76",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.n": "922c7954216ccfe7",
              "result.parse": "ef2d127de37b942b",
              "result.rows": "29a36b9be934743b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "8286314641fa99b2a8915045403a35e10001999c1934f9927dac415d1ee7890c",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.n": "922c7954216ccfe7",
              "result.parse": "ef2d127de37b942b",
              "result.rows": "29a36b9be934743b",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-candidates-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "9caa6e1776d5e9cf7fdc59f78c4b26f9591cea48d171fd06784bc4c9e857ce36",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.n": "65a699905c026193",
              "result.parse": "e7f6c011776e8db7",
              "result.rows": "d8ac533de6b21744",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "6942ed59245f48eb2e80cb9026a54fa81a5d9713a9eac29921117f235dd9cbbd",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.n": "65a699905c026193",
              "result.parse": "e7f6c011776e8db7",
              "result.rows": "d8ac533de6b21744",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "9caa6e1776d5e9cf7fdc59f78c4b26f9591cea48d171fd06784bc4c9e857ce36",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.n": "65a699905c026193",
              "result.parse": "e7f6c011776e8db7",
              "result.rows": "d8ac533de6b21744",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "6942ed59245f48eb2e80cb9026a54fa81a5d9713a9eac29921117f235dd9cbbd",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.n": "65a699905c026193",
              "result.parse": "e7f6c011776e8db7",
              "result.rows": "d8ac533de6b21744",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "9caa6e1776d5e9cf7fdc59f78c4b26f9591cea48d171fd06784bc4c9e857ce36",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.n": "65a699905c026193",
              "result.parse": "e7f6c011776e8db7",
              "result.rows": "d8ac533de6b21744",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "6942ed59245f48eb2e80cb9026a54fa81a5d9713a9eac29921117f235dd9cbbd",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.guess_candidates": "b5bea41b6c623f7c",
              "result.guess_sentence": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.n": "65a699905c026193",
              "result.parse": "e7f6c011776e8db7",
              "result.rows": "d8ac533de6b21744",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-init": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-init-settled": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-init-settled-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-init-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "197f5c18c73627a857d738529b6c22d8f0f7f6b988a1095b6744a94df32dc8b9",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "c1b15ab0bcc48bcbec055124e38d0767f44cb2aa30b81d2f0b18564467784c34",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-load": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-load-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "2029f83fede32b8203a66815cfad5ad1f610d26c7d3479c1d20da3e3489e8463",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "96760447093bf5ca5b044d275ed0c7f357a85606e5df5dca89882aab914a7c68",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.load": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "result.unload": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-mask-out": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "94756ddb3817c54c0f387c37f966d8b360f6334a75159dc4ff1a72fd633e9c75",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "dd19ff5920baae75"
            }
          },
          "subject": {
            "sha256": "5db0e8ab0d8f8dcbd8b3a49de9895e805522167bd3d12eb0bd7b9d82efbc4eb3",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "dd19ff5920baae75"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "4e3f811ad3b90a5cb128ed916976c30f1fd6e28fa7e5e91f89469a41cf1dafde",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "50d8d2b377dbe6af"
            }
          },
          "subject": {
            "sha256": "e9ad9e70cd8f6240ac19489db110e4754933a53ca86bab57d94cf6106e209910",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "50d8d2b377dbe6af"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "640c9e7867b9a82fd05061c0eab696a8725b5d0ac0bc74f33573125b0e07ee39",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "5f0faba001a5acbc"
            }
          },
          "subject": {
            "sha256": "a28e44ce546f04ba630dd5bb1356c369bbc2b8dbd73f4b961b36beb41e1f1c44",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "5f0faba001a5acbc"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "sparse-library-mask-out-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "1021558667278d561cb34d537081af89008728a451863dde8b135c86bbd73232",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "44136fa355b3678a"
            }
          },
          "subject": {
            "sha256": "6819159b9cbd331d54cff9777a76922a7421d6dd8f8e573e92d8f83f2abef4d1",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "44136fa355b3678a"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "1021558667278d561cb34d537081af89008728a451863dde8b135c86bbd73232",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "44136fa355b3678a"
            }
          },
          "subject": {
            "sha256": "6819159b9cbd331d54cff9777a76922a7421d6dd8f8e573e92d8f83f2abef4d1",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "44136fa355b3678a"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "1021558667278d561cb34d537081af89008728a451863dde8b135c86bbd73232",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "44136fa355b3678a"
            }
          },
          "subject": {
            "sha256": "6819159b9cbd331d54cff9777a76922a7421d6dd8f8e573e92d8f83f2abef4d1",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.mask_out": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "44136fa355b3678a"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "sparse-library-reinit": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "5c6eed6739ab2e76bf77f0fd368d380abfa64927e963fa22da1907cbd698b660",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "f14b4f3062aceb7eb49fd2ac11383d813c33821a0c3608efb1113d104dfa6a67",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "5c6eed6739ab2e76bf77f0fd368d380abfa64927e963fa22da1907cbd698b660",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "f14b4f3062aceb7eb49fd2ac11383d813c33821a0c3608efb1113d104dfa6a67",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "5c6eed6739ab2e76bf77f0fd368d380abfa64927e963fa22da1907cbd698b660",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "f14b4f3062aceb7eb49fd2ac11383d813c33821a0c3608efb1113d104dfa6a67",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "sparse-library-reinit-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "c9756ad284ae5805836f95558b1f4fdf3c99cb0eed298150502e3498dd11d993",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "stderr": "b3c9553533485fed"
            }
          },
          "subject": {
            "sha256": "6bdc9b5759e16f9c5ed6466f74cb38d08bf6e502007db0c085ef0fecdec40f7f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "b3c9553533485fed"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "c9756ad284ae5805836f95558b1f4fdf3c99cb0eed298150502e3498dd11d993",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "stderr": "b3c9553533485fed"
            }
          },
          "subject": {
            "sha256": "6bdc9b5759e16f9c5ed6466f74cb38d08bf6e502007db0c085ef0fecdec40f7f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "b3c9553533485fed"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "c9756ad284ae5805836f95558b1f4fdf3c99cb0eed298150502e3498dd11d993",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "stderr": "b3c9553533485fed"
            }
          },
          "subject": {
            "sha256": "6bdc9b5759e16f9c5ed6466f74cb38d08bf6e502007db0c085ef0fecdec40f7f",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctxs": "496428035f129afe",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "b3c9553533485fed"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "sparse-library-save": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "9b29f1de64c3388cbb6bbd1c7a519d3799e3e09e6d7e7ccb0b1924975cef5133",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "4c59ebd47bb36013"
            }
          },
          "subject": {
            "sha256": "30048d87fb2801dc04661f03f07acade369bd4e47c3b7ac3431ac1df1413a391",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "4c59ebd47bb36013"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "6131b07edc7487e29557215d91ac022c29d8f6109ced7d99ebfc91ea47932f7b",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "cb31dbd126e4d1b6"
            }
          },
          "subject": {
            "sha256": "00ae0127cba358ae4a5ebe127660cd56ac07b6f154b20cd68c33330a20d91c36",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "cb31dbd126e4d1b6"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "d84fb0cee31e6c3bd0fe6912043874a7dad280ffe38a3de8f15d0e7c33698757",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "1b265ce952e70252"
            }
          },
          "subject": {
            "sha256": "cfa58f6416b2e15df9c43b6f2e4e520aa13893e13df009a93435f9bf5039f478",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "1b265ce952e70252"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "sparse-library-save-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "81a7136d682e48db02b9de797d469bb7c532eb6e6685a49284569cf1cd3a6599",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "0b5114d37d9981d7"
            }
          },
          "subject": {
            "sha256": "ad74b30e485fc837cc812c2bc894604ad2f2bcec1aea9331678f229220ae04af",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "0b5114d37d9981d7"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "2c5f50fec723e5fa1572d0a443b68efe6686ca255fbe953721586ea2086f08cc",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "9b933a0bbf6a3e75"
            }
          },
          "subject": {
            "sha256": "53b35698f5e7bbf20a8ab3306cca05c32b3faa5aee3446a83e3d15e70eed6313",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "9b933a0bbf6a3e75"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "779664d6d7d0329e99a09b1b7af495288017ec400fb2f6c3ecc8913d14046845",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "d7a7ca8ad6ebd085",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "ef0e97e8df6322a1"
            }
          },
          "subject": {
            "sha256": "d42221382730071b1084edf63857af445771656a7512b01ead5142f41c0c8aac",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "8925f7f9e7d8a083",
              "result.logs": "4f53cda18c2baa0c",
              "result.save": "b5bea41b6c623f7c",
              "result.train": "b5bea41b6c623f7c",
              "stderr": "c48a0964ed0c5485",
              "userfiles": "ef0e97e8df6322a1"
            }
          },
          "divergence": [
            "result.logs[0:2]: subject drops [[null, 8], [null, 8]]"
          ]
        }
      }
    },
    "sparse-library-tokens": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "defcd7c2f9afbbfb8e068e8a856e6bd8fa2f771a7d36e7d45eeef3d194df35da",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.has_next": "fcbcf165908dd18a",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "09660625198a5301447ee66b03753696354304f196af62f7ed3a81c282de56ec",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.has_next": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "defcd7c2f9afbbfb8e068e8a856e6bd8fa2f771a7d36e7d45eeef3d194df35da",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.has_next": "fcbcf165908dd18a",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "09660625198a5301447ee66b03753696354304f196af62f7ed3a81c282de56ec",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.has_next": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "defcd7c2f9afbbfb8e068e8a856e6bd8fa2f771a7d36e7d45eeef3d194df35da",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.has_next": "fcbcf165908dd18a",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "09660625198a5301447ee66b03753696354304f196af62f7ed3a81c282de56ec",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.has_next": "fcbcf165908dd18a",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "sparse-library-tokens-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "ea8b0098014dd08674bc93f8d30288a40e0abc52ac2c226a45b93c0b9cb0b127",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "aafa8cdbf4fec3732438ad42a1012d6c190af4baac6f86d09726f309a41f7ff4",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "ea8b0098014dd08674bc93f8d30288a40e0abc52ac2c226a45b93c0b9cb0b127",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "aafa8cdbf4fec3732438ad42a1012d6c190af4baac6f86d09726f309a41f7ff4",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "ea8b0098014dd08674bc93f8d30288a40e0abc52ac2c226a45b93c0b9cb0b127",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "aafa8cdbf4fec3732438ad42a1012d6c190af4baac6f86d09726f309a41f7ff4",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.0x4000001": "ac229148de1f1165",
              "result.0x4000002": "ac229148de1f1165",
              "result.ctx": "b5bea41b6c623f7c",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "third-separator-library-init": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    },
    "third-separator-library-init-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "bdb": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        },
        "kc": {
          "pin": {
            "sha256": "70c629ee3453e909976db34a12d0b318259404fd7e4afcf93f052f2ab345ee92",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "62273f5f9ce1ddac",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "e678d4f713e1c78607a550d324d62de52c660ed937246e6627a85afada144362",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.ctx": "b5bea41b6c623f7c",
              "result.fixture_allocated": "8b926d75599a618e",
              "result.fixture_size": "d59eced1ded07f84",
              "result.logs": "4f53cda18c2baa0c",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]"
          ]
        }
      }
    }
  }
}
```
