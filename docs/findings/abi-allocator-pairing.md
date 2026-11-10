# ABI allocator pairing — the register and the gate

Status: **contract** (the two `.alloc` registers).

`oxpinyin-capi` and `oxpinyin-zhuyin-capi` ship as drop-in replacements for
`libpinyin.so.15` / `libzhuyin.so.15`. Exactly one side owns every pointer
that the ABI hands a consumer, and exactly one function releases it.
`just abi exports` freezes *which* symbols cross the boundary. The
allocator-pairing register and its gate freeze *what crosses with them*.

## 1. What crosses the boundary

The table lists three allocation families and one borrowed class that
matters as much:

| Family | Producer | Allocator | Released by |
|---|---|---|---|
| Opaque handle | `pinyin_init`, `pinyin_alloc_instance`, the three `*_begin_*` iterators, and the zhuyin equivalents | Rust `Box::into_raw` | its own destructor (`pinyin_fini`, `pinyin_free_instance`, `pinyin_end_*`), which `Box::from_raw`s |
| Caller-owned string | 12 `gchar **` / `char **` out-params in pinyin, 4 in zhuyin | GLib (`g_strdup`, `g_strdup_printf`, `g_strconcat`, `g_strjoinv`: the pin's call per site, #1227), or libc `malloc` via `ffi::malloc_cstr_with` at the `g_ucs4_to_utf8` sites | `g_free` |
| Caller-owned string vector | `pinyin_in_chewing_keyboard`, `zhuyin_in_chewing_keyboard` (`gchar ***`) | GLib: `g_ptr_array_new`/`g_strdup`/`g_ptr_array_add`/`g_ptr_array_free(…, FALSE)` via `ffi::symbol_vector` (#1227) | `g_strfreev` |
| **Borrowed** (not an allocation) | the candidate, its string, the key and key-rest out-params, and `pinyin_get_context` | — | never freed by the caller; released with the instance or context |

The `malloc`/`g_free` crossing at the `malloc_cstr_with` sites is
deliberate. It is the pairing assumption that the gate exercises. GLib 2.46
removed the memory vtable, so `g_free` is a thin wrapper over libc `free`,
and the two names denote one allocator. The gate proves that this holds
against the GLib that the consumer actually links.

The `GArray`-taking symbols (`pinyin_lookup_tokens`,
`pinyin_token_get_nth_pronunciation`) are outside all four rows. The array
belongs to the caller, which creates and frees it with glib's own
allocator. The library only appends through `g_array_append_vals`.
`dict.rs`'s module header explains why a hand `realloc` of the array is
wrong.

## 2. Out-parameter contracts a consumer must know

These are pin behaviour, held by the register's notes:

- **Calls that allocate and return `false`.** The three auxiliary-text
  getters answer `false` with an *allocated empty string* when the matrix is
  empty (`pinyin.cpp:3382-3386`, `3442-3445`). A consumer that frees only on
  `true` leaks one buffer per keystroke, on oxpinyin and on the pin.
  The bigram iterator's terminal row also answers `false` with both strings
  written (`pinyin.cpp:904-910`, compatibility-policy row 36). These slots
  are `false-allocates`.
- **`pinyin_get_pinyin_strings` writes nothing on refusal.** The four
  single-string key getters (`pinyin.cpp:2707-2749`) open with an
  unconditional `*utf8_str = NULL;` before the `0 == key->get_table_index()`
  refusal. `pinyin_get_pinyin_strings` (`:2751-2763`) refuses first (`:2755`)
  and only then writes, with each out-param guarded by its own `if`. The
  caller must initialize both out-params. A separator column holds a zero
  key, so a conforming consumer reaches these refusals.
  `pinyin_get_pinyin_key` / `zhuyin_get_zhuyin_key` hand out that key with
  `true` (`xi'` at offset 2 for pinyin, `xi1'` at offset 3 for zhuyin).

| Slot | Return on the zero key | Out-param |
|---|---|---|
| `pinyin_get_pinyin_string`, `_zhuyin_string`, `_luoma_pinyin_string`, `_secondary_zhuyin_string` | `false` | NULL |
| `pinyin_get_pinyin_strings` (`shengmu`, `yunmu`) | `false` | both untouched |
| `zhuyin_get_zhuyin_string`, `zhuyin_get_pinyin_string` | `false` | NULL |

- **NULL out-param arguments.** The pin dereferences `utf8_str`
  unconditionally in the four single-string getters. oxpinyin checks it
  first ([compatibility policy, row 41](compatibility-policy.md)). Where the
  pin guards its out-params, oxpinyin's guard matches it exactly.

## 3. The gate

The gate is `just abi alloc-pairing` (`tools/dev/abi.py`). CI's `test` job
runs it beside the export gates. It reads one checked-in register per ABI:
`crates/oxpinyin-capi/libpinyin.alloc` and
`crates/oxpinyin-zhuyin-capi/libzhuyin.alloc`. Each register sits beside
the `.ver` files and has one line per pointer-shaped slot. The line holds:

- the slot's class (`handle:<fn>`, `g_free`, `g_strfreev`, `borrowed`);
- what the slot holds on a `false` return (`false-allocates`,
  `false-nulls`, `false-untouched`, `false-unreachable`, `n/a`);
- how that note was established, as one of these `<verified>` kinds:

| `<verified>` | Meaning |
|---|---|
| `pin-probe:<case>` | measured identical at the pin and on oxpinyin, on the reference cell, by the named case (recorded evidence) |
| `source:<file>:<lines>` | argued from the pin's `src/<file>` at 074a2219 |
| `unverified` | executed against oxpinyin only, never compared with the pin |
| `unexecuted` | declared, never executed on either side: the driver does not probe the note and no pin probe has run it |
| `n/a` | the note is `n/a` |

The gate checks that every line has exactly five fields, with a fifth
field of one of these kinds. A negative control requires that check to
reject a four-field line and a misspelled kind. The check cannot test the
evidence that a kind names.

Three checks, then a control:

1. **Coverage of the frozen header.** Every pointer return and every `T **`
   out-parameter that `pinyin.h` / `zhuyin.h` declares is classified exactly
   once. Nothing is classified that the header does not declare. There are
   26 slots for pinyin and 12 for zhuyin. This check fires when someone adds
   a new allocating entry point without saying who frees it. The header
   parser is trusted because it round-trips: the same extraction recovers
   exactly the 79 and 52 symbols that the two version scripts freeze.
2. **Class against C type, and destructor reachability.** A non-const
   `gchar **` / `char **` must be `g_free`; a `gchar ***` must be
   `g_strfreev`; a `const T **` must be `borrowed`; a pointer return must be a
   handle or borrowed. Every `handle:<fn>` must name a symbol that the
   version script exports. So no handle has a declared destructor that a
   consumer cannot call.
3. **The declaration under LeakSanitizer.** One C++ consumer per ABI
   (`tools/abi/alloc-pairing-*.cc`) drives the real `.so` and releases each
   slot with the register's deallocator. It is built with
   `-fsanitize=address`. A slot that the declared deallocator does not
   actually release is a leak. A borrowed pointer wrongly declared owned is
   an invalid free. These properties make the run meaningful:
   - **Coverage.** The driver reports for each slot whether it reached the
     slot with a non-NULL pointer. The run is rejected unless every
     registered slot was hit. A gate that never exercises the allocation it
     checks passes for the wrong reason. To reach all 38 slots, the driver
     trains a multi-phrase sentence (otherwise the bigram export iterator
     has no rows). It also drives zhuyin through chewing keystrokes, not
     full pinyin.
   - **Attribution.** The whole lifecycle runs twice. The first pass runs
     inside `__lsan_disable()`, so the one-time statics that the backend and
     glib allocate at first use are not the gate's subject. A leak reported
     in the second pass is a *per-call* leak: the kind that a consumer
     accumulates one keystroke at a time. There is no suppression file, so
     no suppression can grow broad enough to hide a real leak. Coverage is
     **cleared between the two passes**. No leak check ran against a slot
     that the warm-up reached, so keeping its bit would report coverage that
     the measured pass did not earn.
   - **No driver/register drift.** Each driver echoes the register's own
     class and note back with every slot. The script compares the first
     four fields, not only the symbol and slot name. A class changed from
     `handle:pinyin_fini` to `borrowed`, a renamed destructor, or an edited
     note fails here. It does not pass merely because the key still matches.
   - **The notes are executable.** For every slot whose note is not `n/a` or
     `false-unreachable`, the driver drives a failure path that a
     *conforming* consumer can reach. The paths are an empty parse, an
     out-of-range index, an exhausted unigram iterator, the bigram
     iterator's terminal row, an unmapped key and `null_token`. The driver
     asserts the state that the register declares. The slot table selects
     the assertion; the call site never chooses it. So a note that does not
     match the library fails and is not echoed back unchallenged.
     NULL-argument refusals are deliberately not probed: they are caller
     misuse, where the pin would simply crash. A slot that the driver never
     probes fails the gate as `unprobed false-return contract`.
     `false-unreachable` is the one exception. It is an explicit, reviewable
     claim, not a silent omission. The driver also fails if it *does* reach
     a path that the register calls unreachable.
4. **A negative control.** Each driver is built a second time with
   `-DOXPINYIN_ALLOC_PAIRING_LEAK`, which drops the frees, and that build is
   *required* to fail with a LeakSanitizer report. Without this control, a
   sanitizer that silently stopped working would leave the gate passing.

The gate needs no new dependency and no Rust rebuild:

- The `g++` that the `test` job installs carries `libasan`/`liblsan`.
- The Rust side is not instrumented.
- ASan's `malloc` interposition covers the whole process: GLib's and libc's
  `malloc`, and Rust's `Box` behind the handles.

The full gate runs on Linux only. `just abi alloc-pairing --static-only`
runs checks 1–2 on any host.

Run:

```
just abi alloc-pairing                 # 38/38 slots, both controls fire
just abi alloc-pairing --static-only   # checks 1-2, host-portable
```

The gate's failure messages name what failed:

- an unclassified ABI slot;
- a class that disagrees with the header's C type;
- a handle destructor that the `.ver` does not export;
- a per-call leak, with its Rust source line;
- an invalid free;
- driver/register drift;
- an out-param state that differs from the note;
- a probed path that the register calls unreachable;
- an unprobed false-return contract;
- a slot that the measured pass never reached;
- a malformed or misspelled register line.

## 4. What this does not cover

- **Consumer-side discipline.** The gate proves the library's half. A
  consumer that frees a handle with `g_free`, or frees a borrowed candidate
  pointer, has a consumer bug. Review such a consumer against the register.
- **The GArray surface.** The caller owns these arrays, and glib's own
  allocator is on both sides. The gate creates and frees those arrays but
  has nothing to prove about their pairing.
- **Allocation the C side makes on the library's behalf.** Everything that
  the backend library allocates internally is, by construction, outside
  this gate's per-call window, because the warm pass absorbs it.
  RSS attribution is the subject of
  `rss-attribution-2026-09-09`, not of this gate.
- **Non-Linux hosts.** LeakSanitizer's stop-the-world does not run under
  macOS's sandbox. `--static-only` is the portable half. The C ABIs are
  Linux-first in any case ([CONTRIBUTING.md](../../CONTRIBUTING.md)).
