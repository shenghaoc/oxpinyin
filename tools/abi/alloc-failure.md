# Allocation-failure audit harness

2026-10-25 UTC. Refs #924; follow-up to merged #1222. Subject baseline
`27c785c07d30ae4332647d76b106c1bafaf71958`; pin
`074a2219c90feaf962d0d24f034514033ece5f99`; GLib tag `2.89.4`, commit
`fa41d356ee4936264c45cf11fa6c2640a89fbdda`. Pin citations below start at
`src/`; GLib citations start at its repository root. These expectations
were derived from source before the fault-injected pin executions.

This is the C consumer pattern used by `alloc-pairing-*.cc`, with precise
allocation failure injection. It does not create another contract-diff
framework or change production ownership. Run it against unstripped debug
libraries; the script refuses missing/ambiguous adapter symbols.

```sh
just heavy -- cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi
just heavy -- tools/abi/check-alloc-failure.sh "$PINYIN_ORACLE_PREFIX" \
  "$CARGO_TARGET_DIR/debug/libpinyin_capi.so" \
  "$CARGO_TARGET_DIR/debug/libzhuyin_capi.so"
```

The test-only Linux/glibc malloc/free/realloc interposer uses the debug
symbol's exact address interval to fail the direct libc allocation inside
`ffi::owned_cstr` or `ffi::owned_cstr_list`. It does not fail a random
allocation count in the entire process. Pin malloc and array realloc are
identified by their GLib caller symbol. Injection is armed only around one
public API call in a child, and disabled immediately after the targeted
failure. A pipe proves that injection fired, then either the expected GLib
error preceded signal termination or the expected return/output was
observed. No process-wide memory limit is used. Parent initialization,
backend loading, and normal successful controls precede arming.

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
mode; no debug-symbol lookup is needed for the real GLib growth check.
The full ABI harness is a source-derived defect audit, not a blanket parity
gate: it labels silent NULL returns as confirmed defects. No policy
exception or expected contract result is weakened.
