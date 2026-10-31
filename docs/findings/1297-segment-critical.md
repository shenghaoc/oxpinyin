# Phrase segmentation writes no invalid-UTF-8 critical

Status: accepted, 2026-10-31 UTC, maintainer decision D1 of #1297 (a
narrow exception to the library stderr ruling, #545). Disposition:
POLICY-ACCEPTED. Rule: [source policy, no hand-written shims](../source-policy.md);
record form: [template](language-runtime-discrepancy-template.md).
Pin: libpinyin 074a2219 (`src/pinyin.cpp` blob `f27f7cf7`,
`src/zhuyin.cpp` blob `d1520c18`), read from a `git archive` of the pin;
GLib 2.84.4 of the reference image.

GLib documents `g_return_val_if_fail` as a precondition assertion on the
caller's arguments, to be used at the top of a public function
(https://docs.gtk.org/glib/warnings.html). Neither site follows that
pattern. `SubPhraseIndex::load` checks file data inside an internal
method. `phrase_segment` checks the result of conversions it has already
made, not its argument on entry. oxpinyin therefore implements the
control flow these checks express, an early return of the pin's value,
and does not reproduce the critical.

`pinyin_phrase_segment` and `zhuyin_phrase_segment` count the sentence's
characters with `g_utf8_strlen`, convert it with `g_utf8_to_ucs4`, and
check `g_return_val_if_fail(num_of_chars == ucs4_len, FALSE)`
(`pinyin.cpp:1450-1454`, `zhuyin.cpp:969-973`). Invalid UTF-8 makes the
conversion fail, so the check fails: the pin logs GLib's critical, in the
default domain, `bool pinyin_phrase_segment(pinyin_instance_t*, const
char*): assertion 'num_of_chars == ucs4_len' failed` (the zhuyin twin
with its own `G_STRFUNC`), and returns `FALSE` with the earlier phrase
result unchanged.

oxpinyin (`oxpinyin-capi` and `oxpinyin-zhuyin-capi`, `phrase.rs`) returns
`false` and keeps the earlier result, and makes no log call. Two
observable differences follow, both accepted:

- The critical is missing: a caller's GLib log handler receives nothing,
  and GLib's default handler writes nothing to stderr.
- Under GLib fatal-critical settings (`G_DEBUG=fatal-criticals`,
  `g_log_set_always_fatal`) the pin aborts at the critical. oxpinyin does
  not abort; it returns `false`. **The fatal-critical behaviour is lost.**
  This is an intentional observable difference, not exact compatibility.

Return values, the phrase result and every other state are the pin's.
The NULL-sentence path is not covered by this record: it makes the pin's
own `g_utf8_strlen` call, whose critical is GLib's.

Cases, bound below with `just policy-record` from a tkrzw capture:
`glib-924-segment-critical-{pinyin,zhuyin}`,
`glib-924-segment-fatal-critical-{pinyin,zhuyin}` and
`issue542-phrase-segment-invalid-utf8-{pinyin,zhuyin}`. Controls that
still MATCH: `c-string-input/{pinyin,zhuyin}-segment-state-{null,valid}`.
Unit tests: `surrogate_segment_matches_pin_length_and_no_critical` in both
facades.

```contract-diff-policy
{
  "schema": 2,
  "record": "1297-segment-critical",
  "status": "current",
  "disposition": "POLICY-ACCEPTED",
  "cases": {
    "glib-924-segment-critical-pinyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "a60195321be6e341ff1e6bfd37d3ffebf84766927b08495566509ba214a66cb5",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.messages": "98f4eca89c3f5b4f",
              "result.ret": "fcbcf165908dd18a",
              "result.retained": "e7be14d533932cdd",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "fde3c2a1f80efb71edeb5ea8bf55205dee062a5b5bec8075d96111ae131b8f31",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.messages": "4f53cda18c2baa0c",
              "result.ret": "fcbcf165908dd18a",
              "result.retained": "e7be14d533932cdd",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]",
            "result.messages (list lengths pin 1, subject 0; first change [0]): subject drops \"bool pinyin_phrase_segment(pinyin_instance\" … (length 104)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "glib-924-segment-critical-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "2ee76e4105aee72918ce77f2827aed8f01c983e548cfefd586b4b217953229d1",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.messages": "f60e3f18b53744ac",
              "result.ret": "fcbcf165908dd18a",
              "result.retained": "e7be14d533932cdd",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "fde3c2a1f80efb71edeb5ea8bf55205dee062a5b5bec8075d96111ae131b8f31",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.messages": "4f53cda18c2baa0c",
              "result.ret": "fcbcf165908dd18a",
              "result.retained": "e7be14d533932cdd",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]",
            "result.messages (list lengths pin 1, subject 0; first change [0]): subject drops \"bool zhuyin_phrase_segment(zhuyin_instance\" … (length 104)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "glib-924-segment-fatal-critical-pinyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "bb22edc102df71ad84667b70f4353771d274253ea5c1a9a7098fa6dc35716407",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "75193a3d247458df"
            }
          },
          "subject": {
            "sha256": "fde3c2a1f80efb71edeb5ea8bf55205dee062a5b5bec8075d96111ae131b8f31",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.messages": "4f53cda18c2baa0c",
              "result.ret": "fcbcf165908dd18a",
              "result.retained": "e7be14d533932cdd",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"l\" … (length 4): [], \"m\" … (length 8): [], \"r\" … (length 3): false} … (length 4)",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.\\n** C\" … (length 159); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "glib-924-segment-fatal-critical-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "02c9337daac910a6e34e544bef32428ed384866b073b15425ff233f0da989cc9",
            "fields": {
              "exit": "03b26944890929ff",
              "result": "74234e98afe7498f",
              "stderr": "cc029d284750d7dd"
            }
          },
          "subject": {
            "sha256": "fde3c2a1f80efb71edeb5ea8bf55205dee062a5b5bec8075d96111ae131b8f31",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.messages": "4f53cda18c2baa0c",
              "result.ret": "fcbcf165908dd18a",
              "result.retained": "e7be14d533932cdd",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "exit: pin -6; subject 0",
            "result: pin null; subject {\"l\" … (length 4): [], \"m\" … (length 8): [], \"r\" … (length 3): false} … (length 4)",
            "stderr: pin \"open SCRATCH/user-X/user.conf failed.\\n** C\" … (length 159); subject \"open SCRATCH/user-X/user.conf failed.\"",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "issue542-phrase-segment-invalid-utf8-pinyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "249bc9c83fa47a25b527cb707287e519f9c64f953869a32d79ee7ad9bfce4eb4",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.messages": "98f4eca89c3f5b4f",
              "result.ret": "fcbcf165908dd18a",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "017aff2547ba6dab37f3a74b2ede184000d2b8155f191449255acd4cea7a71f4",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.messages": "4f53cda18c2baa0c",
              "result.ret": "fcbcf165908dd18a",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]",
            "result.messages (list lengths pin 1, subject 0; first change [0]): subject drops \"bool pinyin_phrase_segment(pinyin_instance\" … (length 104)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    },
    "issue542-phrase-segment-invalid-utf8-zhuyin": {
      "cells": {
        "tkrzw": {
          "pin": {
            "sha256": "df4d30997c388d56e7c56a5ee96bdc230d6d2c84cbcb659c3010d5530b611f51",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "62273f5f9ce1ddac",
              "result.messages": "f60e3f18b53744ac",
              "result.ret": "fcbcf165908dd18a",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "subject": {
            "sha256": "017aff2547ba6dab37f3a74b2ede184000d2b8155f191449255acd4cea7a71f4",
            "fields": {
              "exit": "5feceb66ffc86f38",
              "result.logs": "4f53cda18c2baa0c",
              "result.messages": "4f53cda18c2baa0c",
              "result.ret": "fcbcf165908dd18a",
              "stderr": "c48a0964ed0c5485"
            }
          },
          "divergence": [
            "result.logs[0:1]: subject drops [[null, 8]]",
            "result.messages (list lengths pin 1, subject 0; first change [0]): subject drops \"bool zhuyin_phrase_segment(zhuyin_instance\" … (length 104)",
            "… further divergence omitted (4096-byte budget)"
          ]
        }
      }
    }
  }
}
```
