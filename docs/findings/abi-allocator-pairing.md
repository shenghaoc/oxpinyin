# ABI allocator pairing — the audit and the gate

Date: 2026-09-09 · Status: contract (the register) + audit (§2)

`oxpinyin-capi` and `oxpinyin-zhuyin-capi` ship as drop-in replacements for
`libpinyin.so.15` / `libzhuyin.so.15`. Every pointer the ABI hands a consumer
is owned by exactly one side and released by exactly one function. Until this
change nothing held the library to that: `tools/abi/check-exports.sh` froze
*which* symbols cross the boundary, and nothing froze *what crosses with
them*. `docs/safety/oxpinyin-safety-profile.md` said so at Layer 8, listing
"the `g_free`/malloc pairing assumption" among the things that remain
judgment rather than a gate.

This document records what the audit of the two crates found, and what the
gate that replaced the judgment item now enforces.

## 1. What crosses the boundary

Three allocation families, and one non-family that matters as much:

| Family | Producer | Allocator | Released by |
|---|---|---|---|
| Opaque handle | `pinyin_init`, `pinyin_alloc_instance`, the three `*_begin_*` iterators, and the zhuyin equivalents | Rust `Box::into_raw` | its own destructor (`pinyin_fini`, `pinyin_free_instance`, `pinyin_end_*`), which `Box::from_raw`s |
| Caller-owned string | 12 `gchar **` / `char **` out-params in pinyin, 4 in zhuyin | libc `malloc`, via `ffi::owned_cstr` | `g_free` |
| Caller-owned string vector | `pinyin_in_chewing_keyboard`, `zhuyin_in_chewing_keyboard` (`gchar ***`) | libc `malloc` for both the array and every string, via `ffi::owned_cstr_list` | `g_strfreev` |
| **Borrowed** (not an allocation) | the candidate, its string, the key and key-rest out-params, and `pinyin_get_context` | — | never freed by the caller; released with the instance or context |

The `malloc`/`g_free` crossing in the middle two rows is deliberate and is the
pairing assumption the gate exercises: GLib 2.46 removed the memory vtable, so
`g_free` has since been a thin wrapper over libc `free` and the two names
denote one allocator. `ffi.rs` states this at the definition of `owned_cstr`;
the gate is what proves it holds against the GLib the consumer actually links.

The `GArray`-taking symbols (`pinyin_lookup_tokens`,
`pinyin_token_get_nth_pronunciation`) are outside all four rows: the array is
the caller's, created and freed by the caller with glib's own allocator, and
the library only appends through `g_array_append_vals` (see `dict.rs`'s module
header for why hand-`realloc`ing it was wrong).

## 2. What the audit found

**No mismatch.** Every caller-owned buffer in both crates is produced by
`owned_cstr` / `owned_cstr_list` — libc `malloc` — and documented `g_free` /
`g_strfreev`; every handle is `Box::into_raw` and reaches C only through a
destructor that `Box::from_raw`s it, never through `g_free`. No allocation
crosses families, and no allocation is released twice or on an error path.
There is therefore no bug fix to land ahead of the gate.

Two things the audit did surface, both now pinned by the register rather than
changed:

**(a) Entry points that allocate and return `false`.** The three
auxiliary-text getters answer `false` with an *allocated empty string* when
the matrix is empty — upstream's shape (`pinyin.cpp:3382-3386`, `3442-3445`),
reproduced deliberately. A consumer written to the obvious rule ("check the
return, then free") leaks one buffer per keystroke against oxpinyin exactly as
it does against the pin. Nothing recorded this before; the register's
`false-allocates` note does, and the gate's driver exercises precisely that
path.

**(b) One out-param initialization asymmetry — parity, confirmed against the
pin.** `pinyin_get_pinyin_strings` returns `false` for a key whose table index
is 0 *without writing either out-param*, where the neighbouring single-string
renderers (`render_key` in `cursor.rs`, `display_string_getter` in `keys.rs`)
NULL theirs first. The caller must therefore initialize to NULL itself. This is
not an allocator mismatch, and it is upstream's own asymmetry rather than a
divergence.

Read from a fresh clone of libpinyin at the pinned commit `074a2219`
(`2.11.92`), the cited blob hashed against the pin's tree (`src/pinyin.cpp` =
`f27f7cf7`): the five getters sit consecutively, and the four single-string
ones — `pinyin_get_zhuyin_string` `pinyin.cpp:2707-2716`,
`pinyin_get_pinyin_string` `:2718-2727`, `pinyin_get_luoma_pinyin_string`
`:2729-2738`, `pinyin_get_secondary_zhuyin_string` `:2740-2749` — each open
with an unconditional `*utf8_str = NULL;` (`:2710`, `:2721`, `:2732`, `:2743`)
*before* the `0 == key->get_table_index()` refusal. `pinyin_get_pinyin_strings`
`:2751-2763`, immediately after them, refuses first (`:2755`) and only then
writes, each out-param guarded by its own `if (shengmu)` / `if (yunmu)`
(`:2758`, `:2760`). NULLing the out-params here would therefore be the
divergence, not the fix; the audit's original caution was right, and oxpinyin
needs no change.

The one difference on this path is the NULL-argument guard: upstream
dereferences `utf8_str` unconditionally in the four neighbours, while oxpinyin
checks it first — the established memory-safety property of every entry point
(`docs/safety/oxpinyin-safety-profile.md`), not a behaviour change on any
non-NULL call. Where the pin guards its out-params, as it does here, oxpinyin's
guard matches it exactly.

§2(c)'s sweep then reclassified both slots `false-unreachable`, and that is the
sharper answer: reaching this `false` at all needs a `ChewingKey` whose table
index is 0, and the opaque typedef denies a conforming consumer one. The gate
therefore does not probe this contract and does not pretend to — which is
exactly why the source read above is what settles it. Where the driver cannot
reach a path, the pin is the only evidence there is. The drivers still
initialize every out-param to NULL regardless, which is the consumer-side
discipline that makes the asymmetry invisible either way.

**(c) Three of the audit's own out-param notes were wrong.** Recorded here
because it is the point of the mechanism, not an aside: §3's fourth field
began as prose derived from *reading* the code. When the gate was extended to
*execute* those notes — drive each reachable failure path and assert the
stated out-param state — three of them failed immediately.
`pinyin_get_candidate`, `pinyin_get_pinyin_key` and
`pinyin_get_pinyin_key_rest` (and their zhuyin twins) all NULL their
out-param on the reachable refusal, where the register had said they left it
untouched. A consumer trusting the prose would have initialized to NULL and
been fine; one relying on "untouched" to keep a previous value would not.
Six further slots were reclassified `false-unreachable`: their only
non-NULL-argument `false` needs an unset `ChewingKey`, and `ChewingKey` is an
opaque typedef, so a conforming consumer cannot fabricate one.

The general lesson is the one this document exists to make mechanical: a
register that is only read is a register that drifts. The notes are now
executable, and the gate rejects any entry whose contract was never probed.

## 3. The gate

`tools/abi/check-alloc-pairing.sh`, run in CI's `test` job beside the two
export gates. It stands on a checked-in register per ABI —
`crates/oxpinyin-capi/libpinyin.alloc`,
`crates/oxpinyin-zhuyin-capi/libzhuyin.alloc` — a sibling of the `.ver` files
carrying one line per pointer-shaped slot: its class (`handle:<fn>`, `g_free`,
`g_strfreev`, `borrowed`) and what the slot holds on a `false` return.

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
     class and note back with every slot, and the script compares the whole
     four-field entry rather than the symbol and slot name alone. A class
     flipped from `handle:pinyin_fini` to `borrowed`, a renamed destructor,
     or an edited note fails here instead of passing because the key still
     lines up.
   - **The notes are executable.** For every slot whose note is not `n/a` or
     `false-unreachable`, the driver drives a failure path a *conforming*
     consumer can reach — an empty parse, an out-of-range index, an
     exhausted iterator, an unmapped key, `null_token` — and asserts the
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

No new dependency, and no Rust rebuild: the `g++` the `test` job already
installs carries `libasan`/`liblsan` on debian:testing, the Rust side is not
instrumented, and ASan's `malloc` interposition covers the whole process —
the library's libc `malloc` in `owned_cstr` and Rust's `Box` behind the
handles alike. Linux only; `--static-only` runs checks 1–2 anywhere.

## 4. Evidence

Run in a `debian:testing` container provisioned with the `test` job's apt set
(`docs/runbooks/`-style replay; the workflow's own package list, nothing
added). Commands, not captures:

```
tools/abi/check-alloc-pairing.sh                 # 38/38 slots, both controls fire
tools/abi/check-alloc-pairing.sh --static-only   # checks 1-2, host-portable
```

Four fault injections were used to establish that the gate can fail, each
reverted afterwards; each is reproducible from the description alone:

| Injection | Expected | Observed |
|---|---|---|
| drop one register line | check 1 names the unclassified slot | `FAIL: unclassified ABI slot` |
| flip a `g_free` class to `borrowed` | check 2 names the disagreement | `FAIL: … is 'char-pp' in the header but 'borrowed' in the register` |
| point a handle at a destructor the `.ver` does not export | check 3's static half | `FAIL: … which libpinyin.ver does not export` |
| a stray `owned_cstr` in `pinyin_get_full_pinyin_auxiliary_text` | a per-call leak, attributed to Rust source | `Direct leak of 27 byte(s) … in owned_cstr crates/oxpinyin-capi/src/ffi.rs:97` |
| `g_free` on the borrowed candidate string | an invalid free | `AddressSanitizer: attempting double-free` |
| flip a class in the register only | driver/register drift | `FAIL: driver/register drift`, naming the whole entry |
| restore §2(c)'s original `false-untouched` note on `pinyin_get_candidate`, in the register *and* the driver | the probe rejects it | `is declared false-untouched but the out-param was NULLed` |
| mark a probed slot `false-unreachable` | the reachability claim is contradicted | `the driver probed a failure path the register calls unreachable` |
| delete one probe call | its note is prose again | `FAIL: unprobed false-return contract`, naming the slot |
| run one exercise during the warm-up only | the measured pass earns no coverage for it | `slots the driver never reached with a live pointer: pinyin_in_chewing_keyboard symbols` |

## 5. What this does not cover

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
  `docs/findings/rss-attribution-2026-09-09.md`'s subject, not this one's.
- **Non-Linux hosts.** LeakSanitizer's stop-the-world does not run under
  macOS's sandbox. `--static-only` is the portable half; the C ABIs are
  Linux-first regardless (AGENTS.md).
