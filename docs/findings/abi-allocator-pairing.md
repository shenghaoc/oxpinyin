# ABI allocator pairing — the register and the gate

Status: **contract** (the two `.alloc` registers).

`oxpinyin-capi` and `oxpinyin-zhuyin-capi` ship as drop-in replacements for
`libpinyin.so.15` / `libzhuyin.so.15`. Every pointer the ABI hands a consumer
is owned by exactly one side and released by exactly one function.
`just abi exports` freezes *which* symbols cross the boundary; the
allocator-pairing register and its gate freeze *what crosses with them*.

## 1. What crosses the boundary

Three allocation families, and one non-family that matters as much:

| Family | Producer | Allocator | Released by |
|---|---|---|---|
| Opaque handle | `pinyin_init`, `pinyin_alloc_instance`, the three `*_begin_*` iterators, and the zhuyin equivalents | Rust `Box::into_raw` | its own destructor (`pinyin_fini`, `pinyin_free_instance`, `pinyin_end_*`), which `Box::from_raw`s |
| Caller-owned string | 12 `gchar **` / `char **` out-params in pinyin, 4 in zhuyin | GLib (`g_strdup`, `g_strdup_printf`, `g_strconcat`, `g_strjoinv`: the pin's call per site, #1227), or libc `malloc` via `ffi::malloc_cstr_with` at the `g_ucs4_to_utf8` sites | `g_free` |
| Caller-owned string vector | `pinyin_in_chewing_keyboard`, `zhuyin_in_chewing_keyboard` (`gchar ***`) | GLib: `g_ptr_array_new`/`g_strdup`/`g_ptr_array_add`/`g_ptr_array_free(…, FALSE)` via `ffi::symbol_vector` (#1227) | `g_strfreev` |
| **Borrowed** (not an allocation) | the candidate, its string, the key and key-rest out-params, and `pinyin_get_context` | — | never freed by the caller; released with the instance or context |

The `malloc`/`g_free` crossing at the `malloc_cstr_with` sites is deliberate
and is the pairing assumption the gate exercises: GLib 2.46 removed the memory
vtable, so `g_free` has since been a thin wrapper over libc `free` and the two
names denote one allocator. The gate proves it holds against the GLib the
consumer actually links.

The `GArray`-taking symbols (`pinyin_lookup_tokens`,
`pinyin_token_get_nth_pronunciation`) are outside all four rows: the array is
the caller's, created and freed by the caller with glib's own allocator, and
the library only appends through `g_array_append_vals` (see `dict.rs`'s module
header for why hand-`realloc`ing it was wrong).

## 2. Out-parameter contracts a consumer must know

These are pin behaviour, held by the register's notes:

- **Calls that allocate and return `false`.** The three auxiliary-text
  getters answer `false` with an *allocated empty string* when the matrix is
  empty (`pinyin.cpp:3382-3386`, `3442-3445`). A consumer that frees only on
  `true` leaks one buffer per keystroke, against oxpinyin as against the pin.
  The bigram iterator's terminal row also answers `false` with both strings
  written (`pinyin.cpp:904-910`, compatibility-policy row 36). These slots
  are `false-allocates`.
- **`pinyin_get_pinyin_strings` writes nothing on refusal.** The four
  single-string key getters (`pinyin.cpp:2707-2749`) open with an
  unconditional `*utf8_str = NULL;` before the `0 == key->get_table_index()`
  refusal. `pinyin_get_pinyin_strings` (`:2751-2763`) refuses first (`:2755`)
  and only then writes, each out-param guarded by its own `if`. The caller
  must initialize both out-params. A separator column holds a zero key, so a
  conforming consumer reaches these refusals: `pinyin_get_pinyin_key` /
  `zhuyin_get_zhuyin_key` hand it out with `true` (`xi'` at offset 2 for
  pinyin, `xi1'` at offset 3 for zhuyin).

| Slot | Return on the zero key | Out-param |
|---|---|---|
| `pinyin_get_pinyin_string`, `_zhuyin_string`, `_luoma_pinyin_string`, `_secondary_zhuyin_string` | `false` | NULL |
| `pinyin_get_pinyin_strings` (`shengmu`, `yunmu`) | `false` | both untouched |
| `zhuyin_get_zhuyin_string`, `zhuyin_get_pinyin_string` | `false` | NULL |

- **NULL out-param arguments.** The pin dereferences `utf8_str`
  unconditionally in the four single-string getters; oxpinyin checks it
  first ([compatibility policy, row 41](compatibility-policy.md)). Where the
  pin guards its out-params, oxpinyin's guard matches it exactly.

## 3. The gate

`just abi alloc-pairing` (`tools/dev/abi.py`), run in CI's `test` job beside
the export gates. It stands on a checked-in register per ABI —
`crates/oxpinyin-capi/libpinyin.alloc`,
`crates/oxpinyin-zhuyin-capi/libzhuyin.alloc` — a sibling of the `.ver` files
carrying one line per pointer-shaped slot: its class (`handle:<fn>`, `g_free`,
`g_strfreev`, `borrowed`), what the slot holds on a `false` return
(`false-allocates`, `false-nulls`, `false-untouched`, `false-unreachable`,
`n/a`), and how that note was established:

| `<verified>` | Meaning |
|---|---|
| `pin-probe:<case>` | measured identical at the pin and on oxpinyin, on the reference cell, by the named case (recorded evidence) |
| `source:<file>:<lines>` | argued from the pin's `src/<file>` at 074a2219 |
| `unverified` | executed against oxpinyin only, never compared with the pin |
| `unexecuted` | declared, never executed on either side: the driver does not probe the note and no pin probe has run it |
| `n/a` | the note is `n/a` |

The gate checks that every line has exactly five fields and a fifth of one
of these kinds, and a negative control requires that check to reject a
four-field line and a misspelled kind; it cannot test the evidence a kind
names.

Three checks, then a control:

1. **Coverage of the frozen header.** Every pointer return and every `T **`
   out-parameter `pinyin.h` / `zhuyin.h` declares is classified exactly once,
   and nothing is classified that the header does not declare — 26 slots for
   pinyin, 12 for zhuyin. This is the check that fires when a new allocating
   entry point is added and nobody says who frees it. The header parser is
   trusted because it round-trips: the same extraction recovers exactly the
   79 and 52 symbols the two version scripts freeze.
2. **Class against C type, and destructor reachability.** A non-const
   `gchar **` / `char **` must be `g_free`; a `gchar ***` must be
   `g_strfreev`; a `const T **` must be `borrowed`; a pointer return must be a
   handle or borrowed. Every `handle:<fn>` must name a symbol the version
   script exports, so no handle is declared with a destructor a consumer
   cannot call.
3. **The declaration under LeakSanitizer.** One C++ consumer per ABI
   (`tools/abi/alloc-pairing-*.cc`) drives the real `.so` and releases each
   slot with the register's deallocator, built with `-fsanitize=address`. A
   slot the declared deallocator does not actually release is a leak; a
   borrowed pointer wrongly declared owned is an invalid free. Two properties
   make the run mean something:
   - **Coverage.** The driver reports per-slot whether it reached the slot
     with a non-NULL pointer, and the run is rejected unless every registered
     slot was hit — a gate that never exercises the allocation it checks is
     green for the wrong reason. Reaching all 38 is why the driver trains a
     multi-phrase sentence (the bigram export iterator has no rows otherwise)
     and drives zhuyin through chewing keystrokes rather than full pinyin.
   - **Attribution.** The whole lifecycle runs twice; the first pass runs
     inside `__lsan_disable()` so the one-time statics the backend and glib
     allocate at first use are not the gate's subject. A leak reported in the
     second pass is a *per-call* leak — the kind a consumer accumulates one
     keystroke at a time. No suppression file, and so no risk of one growing
     broad enough to hide a real leak. Coverage is **cleared between the two
     passes**: a slot the warm-up reached had no leak check run against it,
     so carrying its bit forward would report coverage the measured pass
     never earned.
   - **No driver/register drift.** Each driver echoes the register's own
     class and note back with every slot, and the script compares the first
     four fields rather than the symbol and slot name alone. A class
     flipped from `handle:pinyin_fini` to `borrowed`, a renamed destructor,
     or an edited note fails here instead of passing because the key still
     lines up.
   - **The notes are executable.** For every slot whose note is not `n/a` or
     `false-unreachable`, the driver drives a failure path a *conforming*
     consumer can reach — an empty parse, an out-of-range index, an
     exhausted unigram iterator, the bigram iterator's terminal row, an
     unmapped key, `null_token` — and asserts the
     state the register declares. Which assertion runs is read from the slot
     table, never chosen at the call site, so a note that does not match the
     library fails rather than being echoed back unchallenged. NULL-argument
     refusals are deliberately not probed: that is caller misuse, and where
     the pin would simply crash. A slot the driver never probes fails the
     gate as `unprobed false-return contract`; `false-unreachable` is the one
     way out, and it is an explicit, reviewable claim rather than a silent
     omission — the driver fails too if it *does* reach a path the register
     calls unreachable.
4. **A negative control.** Each driver is built a second time with
   `-DOXPINYIN_ALLOC_PAIRING_LEAK`, which drops the frees, and that build is
   *required* to fail with a LeakSanitizer report. A sanitizer that silently
   stopped working would otherwise leave the gate passing forever.

No new dependency, and no Rust rebuild: the `g++` the `test` job installs
carries `libasan`/`liblsan`, the Rust side is not instrumented, and ASan's
`malloc` interposition covers the whole process — GLib's and libc's `malloc`
and Rust's `Box` behind the handles alike. Linux only;
`just abi alloc-pairing --static-only` runs checks 1–2 anywhere.

Run:

```
just abi alloc-pairing                 # 38/38 slots, both controls fire
just abi alloc-pairing --static-only   # checks 1-2, host-portable
```

The gate's failure messages name what failed: an unclassified ABI slot, a
class that disagrees with the header's C type, a handle destructor the `.ver`
does not export, a per-call leak with its Rust source line, an invalid free,
driver/register drift, an out-param state that differs from the note, a
probed path the register calls unreachable, an unprobed false-return
contract, a slot the measured pass never reached, and a malformed or
misspelled register line.


## 4. What this does not cover

- **Consumer-side discipline.** The gate proves the library's half. A consumer
  that frees a handle with `g_free`, or frees a borrowed candidate pointer, is
  still a consumer bug; the register is what such a consumer should be read
  against.
- **The GArray surface.** Caller-owned, and glib's own allocator on both
  sides; the gate creates and frees those arrays but has nothing to prove
  about their pairing.
- **Allocation the C side makes on the library's behalf.** Everything the
  backend library allocates internally is outside the Rust `GlobalAlloc`
  traffic `alloc_count.rs` measures and outside this gate's per-call window by
  construction (it is absorbed by the warm pass). RSS attribution is
  `rss-attribution-2026-09-09`'s subject, not this one's.
- **Non-Linux hosts.** LeakSanitizer's stop-the-world does not run under
  macOS's sandbox. `--static-only` is the portable half; the C ABIs are
  Linux-first regardless (../../CONTRIBUTING.md).
