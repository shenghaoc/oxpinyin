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

Cases, bound below with `just policy-record` from a tkrzw capture (each
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
        }
      }
    }
  }
}
```
