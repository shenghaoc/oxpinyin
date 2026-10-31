# Allocation-failure audit harness

2026-10-25 UTC. Refs #924; follow-up to merged #1222. Subject baseline
`27c785c07d30ae4332647d76b106c1bafaf71958`; pin
`074a2219c90feaf962d0d24f034514033ece5f99`; GLib tag `2.89.4`, commit
`fa41d356ee4936264c45cf11fa6c2640a89fbdda`. Pin citations below start at
`src/`; GLib citations start at its repository root. These expectations
were derived from source before the fault-injected pin executions.

This is the C consumer pattern used by `alloc-pairing-*.cc`, with precise
allocation failure injection. It does not create another contract-diff
framework or change production ownership. Run it against the debug
libraries the registry builds; the harness reads no symbols, so a stripped
library works as well.

```sh
just heavy -- cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi
just heavy -- just abi alloc-failure "$PINYIN_ORACLE_PREFIX" \
  "$CARGO_TARGET_DIR/debug/libpinyin_capi.so" \
  "$CARGO_TARGET_DIR/debug/libzhuyin_capi.so"
```

2026-10-31 UTC (#1227, class (c) retired). Both sides now allocate the
returned strings and the keyboard vector with the pin's GLib calls
([record](../../docs/findings/returned-string-allocation.md)), so the subject
is judged by the pin's rule and the earlier calibration, warning, tracked
and outputs-unchanged checks of the retired refusal are gone. The
test-only Linux/glibc malloc/realloc interposer, armed only around one
public API call in a forked child, fails:

| Operation | Call | Failed allocation |
| --- | --- | --- |
| 1 | `*_get_zhuyin_string`, tone 1 | 1st `malloc` from `g_malloc` (`g_strdup`) |
| 2, 3, 4 | `*_in_chewing_keyboard`, HSU `c` | 1st, 2nd, 3rd `malloc` from `g_malloc`: the array, `ㄒ`, `ㄕ` |
| 5 | `*_token_get_nth_pronunciation` | 1st `realloc` from `g_realloc` (GArray growth) |
| 6 | `pinyin_iterator_get_next_phrase` | 1st `malloc` from `g_malloc` (`g_strjoinv`) |
| 7 | `pinyin_bigram_iterator_get_next_phrase` (subject only: the seeded user bigram) | 1st `malloc` from `g_malloc` (`g_strdup`) |
| 8 | `pinyin_get_pinyin_strings` | 1st `malloc` from `g_malloc` (`g_strdup`) |
| 9 | `*_get_zhuyin_string`, tone 3 | 1st `malloc` made inside libc (`vasprintf` under `g_strdup_printf`) |

A failed allocation sets `ENOMEM`, as glibc's does. Operations 1-8 must log
GLib's `g_error` and die of SIGTRAP or SIGABRT. Operation 9 must write
`gprintf.c:<line>: failed to allocate memory` to stderr and die of SIGABRT
with no `g_log`: `g_vasprintf` tests `errno == ENOMEM` after
`__vasprintf_chk`, writes `G_STRLOC` and the message with `fwrite` and calls
`abort()` (read from the host `libglib-2.0.so.0` 2.90.0 disassembly; its
`.rodata` holds `../../../glib/gprintf.c:351`). The g_ucs4_to_utf8
conversions allocate through `g_try_malloc_n` (pin) or the subject's
fallible encoder (libc `malloc` from Rust), neither of which is counted, so
they are not injected. A pipe proves that injection fired. Parent
initialization, backend loading and the normal controls (`ㄏㄠˇ`, `ㄋㄧ`,
the HSU `c` mapping, a token lookup) precede arming. A build with injection
disabled must fail the audit on both facades.

## Source matrix and results

The per-exported-caller allocation matrix, classification and remaining
partitions are recorded in the [#924 follow-through ledger](../../docs/findings/glib-libc-verification.md#924-allocation-failure-injection-follow-through--2026-10-25-utc).
The new existing-driver IDs are `glib-924-oom-array-growth-pinyin` and
`glib-924-oom-array-growth-zhuyin`. They reuse this harness in array-only
mode, which finds no allocation site: the real GLib growth check is
identified by its GLib caller. The full ABI harness is a source-derived
defect audit, not a blanket parity gate: it labels silent NULL returns as
confirmed defects. No policy exception or expected contract result is
weakened.
