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

The test-only Linux/glibc malloc/free/realloc interposer fails the one libc
allocation whose pointer the call hands to C: the string, the keyboard
vector or one of its strings, or one of the two iterator buffers. It does not
fail a random allocation count in the entire process, and it does not fail
the earlier Rust allocations. A calibration child first runs the call once
without failure and records the caller of every malloc. The call sites that
returned the pointers the call wrote to its outputs are the injection
sites, and the mallocs made at those sites must be exactly those pointers,
in that order, or the audit stops. The injection child then fails the nth
of them. Nothing is read from the library's symbols or line tables, so the
sites are found whether or not the optimizer inlined `ffi::owned_cstr`,
`ffi::owned_cstr_list` or `ffi::malloc_cstr_with*` into their callers. The
registry builds at dev opt-level 1, where that depends on the build: an
incremental build keeps the bigram iterator's `malloc_cstr_with_warning`
instance as a function of its own, a non-incremental one inlines it into
`pinyin_bigram_iterator_get_next_phrase`. The symbol interval that used to
stand for the allocation then held no `malloc`, the injection never fired
and allocation 7 failed on its missing marker (#1308).

Pin malloc and array realloc are identified by their GLib caller symbol.
Injection is armed only around one public API call in a child, and
disabled immediately after the targeted failure. A pipe proves that
injection fired, then either the expected GLib error preceded signal
termination or the expected return/output was observed. No process-wide
memory limit is used. Parent initialization, backend loading, and normal
successful controls precede arming.

The harness requires both normal controls (real nonempty string, exact HSU
`c` mapping to `ㄒ`, `ㄕ`, and successful token lookup) and two builds with
injection disabled that must fail the audit. The fixed bookkeeping array
tracks only final foreign allocations. At the second keyboard-string
failure, it requires both the first string and the vector to have been
freed before the public call returns. This is bounded cleanup evidence,
not a whole-process leak detector; the existing pairing gate still owns
ASan/LeakSanitizer coverage.

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
