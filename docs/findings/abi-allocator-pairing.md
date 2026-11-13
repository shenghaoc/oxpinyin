# ABI allocator pairing — the register and the gate

Status: **contract** (the two `.alloc` registers).

`oxpinyin-capi` and `oxpinyin-zhuyin-capi` ship as drop-in replacements for
`libpinyin.so.15` / `libzhuyin.so.15`. Exactly one side owns every pointer
that the ABI hands a consumer, and exactly one function releases it.
`just abi exports` fixes *which* symbols cross the boundary. The
allocator-pairing register and its gate fix *what crosses with them*.

## 1. What crosses the boundary

The table lists three allocation families and one borrowed class that
matters as much:

| Family | Producer | Allocator | Released by |
|---|---|---|---|
| Opaque handle | `pinyin_init`, `pinyin_alloc_instance`, the three `*_begin_*` iterators, and the zhuyin equivalents | Rust `Box::into_raw` | its own destructor (`pinyin_fini`, `pinyin_free_instance`, `pinyin_end_*`), which `Box::from_raw`s |
| Caller-owned string | 12 `gchar **` / `char **` out-params in pinyin, 4 in zhuyin | GLib (`g_strdup`, `g_strdup_printf`, `g_strconcat`, `g_strjoinv`: the pin's call per site, [§5](#5-caller-owned-strings)), or libc `malloc` via `ffi::malloc_cstr_with` at the `g_ucs4_to_utf8` sites | `g_free` |
| Caller-owned string vector | `pinyin_in_chewing_keyboard`, `zhuyin_in_chewing_keyboard` (`gchar ***`) | GLib: `g_ptr_array_new`/`g_strdup`/`g_ptr_array_add`/`g_ptr_array_free(…, FALSE)` via `ffi::symbol_vector` ([§5](#5-caller-owned-strings)) | `g_strfreev` |
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

1. **Coverage of the pinned header.** Every pointer return and every `T **`
   out-parameter that `pinyin.h` / `zhuyin.h` declares is classified exactly
   once. Nothing is classified that the header does not declare. There are
   26 slots for pinyin and 12 for zhuyin. This check fires when someone adds
   a new allocating entry point without saying who frees it.
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

The two invocations:

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
  pointer, has a consumer bug. Such a consumer is reviewed against the register.
- **The GArray surface.** The caller owns these arrays, and glib's own
  allocator is on both sides. The gate creates and frees those arrays but
  has nothing to prove about their pairing.
- **Allocation the C side makes on the library's behalf.** Everything that
  the backend library allocates internally is, by construction, outside
  this gate's per-call window, because the warm pass absorbs it.
- **Non-Linux hosts.** LeakSanitizer's stop-the-world does not run under
  macOS's sandbox. `--static-only` is the portable half.

## 5. Caller-owned strings

The [compatibility policy](compatibility-policy.md) ("Returned strings") rules how oxpinyin allocates each caller-owned string: with the pin's GLib call where the pin uses one, and with its fallible encoder where the pin converts with `g_ucs4_to_utf8`.
This section lists the sites and the paths that the ruling leaves unchanged.

Pin sources: libpinyin `074a2219`; GLib 2.84.4 `gstrfuncs.c`, `garray.c` and `gutf8.c`; `g_vasprintf` from the disassembly of `libglib-2.0.so.0` 2.90.0, which holds `gprintf.c:351`.

### Sites

| Export | Pin call | oxpinyin call |
| --- | --- | --- |
| `pinyin_get_pinyin_string`, `zhuyin_get_pinyin_string` (HANYU) | `g_strdup` at tone 0, else `g_strdup_printf("%s%d", …)` (`storage/chewing_key.cpp:53-57`) | `ffi::key_text(ChewingKey::pinyin_text())`: the same branch and call |
| `pinyin_get_zhuyin_string`, `zhuyin_get_zhuyin_string` | `g_strdup` at tone 0 or 1, else `g_strdup_printf("%s%s", …, chewing_tone_table[tone])` (`:80-88`) | `ffi::key_text(ChewingKey::zhuyin_text())` |
| `pinyin_get_luoma_pinyin_string`, `zhuyin_get_pinyin_string` (LUOMA) | `g_strdup` / `g_strdup_printf("%s%d", …)` (`:97-101`) | `ffi::key_text(ChewingKey::luoma_pinyin_text())` |
| `pinyin_get_secondary_zhuyin_string`, `zhuyin_get_pinyin_string` (SECONDARY_ZHUYIN) | `g_strdup` / `g_strdup_printf("%s%d", …)` (`:110-114`) | `ffi::key_text(ChewingKey::secondary_zhuyin_text())` |
| `pinyin_get_pinyin_strings` | `g_strdup` per requested half (`:64`, `:71`; `pinyin.cpp:2759-2762`) | `ffi::strdup` per half, written at once, with no deferred cleanup |
| `pinyin_get_{full,double,chewing}_pinyin_auxiliary_text` | `g_strdup("")` for an empty matrix (`pinyin.cpp:3386`, `:3445`, `:3523`); else `g_strconcat(prefix, middle, postfix, NULL)` (`:3431`, `:3509`, `:3569`) | `ffi::strdup("")` / `ffi::strconcat_aux` over the three parts (`aux_matrix::AuxText`) |
| `pinyin_in_chewing_keyboard`, `zhuyin_in_chewing_keyboard` | `g_ptr_array_new`, `g_strdup` + `g_ptr_array_add` per symbol, NULL, `g_ptr_array_free(array, FALSE)`; no symbol: `g_ptr_array_free(array, TRUE)` (`storage/zhuyin_parser2.cpp:301-332`, `:496-544`, `:798-851`) | `ffi::symbol_vector` with the same calls |
| `pinyin_iterator_get_next_phrase` reading | per-key `get_pinyin_string` into a `GPtrArray`, `g_strjoinv("'", …)`, `g_strfreev` (`pinyin.cpp:726-733`) | `ffi::strjoinv` over Rust-built key strings |
| `pinyin_bigram_iterator_get_next_phrase` | `has_next`: two `g_ucs4_to_utf8` (`:822`, `:828`), `g_strconcat(first, second, NULL)` into `m_phrase` (`:830-832`); `get_next`: `g_strdup(m_phrase)`, `g_strdup(pinyin)` (`:904-905`) | `BigramHandle::convert_loaded_pair` at each load (fallible encoder twice, `ffi::strconcat_pair`, `m_phrase` kept on the handle; a NULL result drops the walk's pair, the pin's NULL `m_phrase`), then `ffi::strdup_raw` / `ffi::strdup` per row |
| `pinyin_get_sentence`, `zhuyin_get_sentence` | `convert_to_utf8` (`lookup/lookup.cpp:27-69`) over the n-best result (`pinyin.cpp:1464-1482`, `zhuyin.cpp:983-998`): per non-null token `g_ucs4_to_utf8` (`:58`); while the result is NULL `g_strdup(phrase)` (`:62-63`), else `g_strconcat(result, "", phrase, NULL)` (`:65`); `g_free` of the phrase and the previous result (`:66-67`); returns `true` (`:69`) | `ffi::sentence_text` over `Session::sentence_phrases`: per phrase the fallible encoder, then the same `g_strdup`/`g_strconcat`/`g_free` chain, `true` even when NULL |

The `g_ucs4_to_utf8` sites keep `ffi::malloc_cstr_with`, which is fallible
and silent. These sites are the unigram phrase (`pinyin.cpp:715-716`),
`*_token_get_phrase` (`pinyin.cpp:1644`, `zhuyin.cpp:1112`), the two bigram
conversions and the conversion of each sentence phrase. Those buffers come from libc
`malloc`, and `g_free` releases them ([§1](#1-what-crosses-the-boundary)).

### Unchanged paths and limits

- The phrase of `pinyin_iterator_get_next_phrase` and `*_token_get_phrase`
  follow the ruling without change.
- Some pin values are temporaries freed before return: the per-key
  `GPtrArray` and strings for the reading join, the `m_pinyins` strings and
  the aux key strings. oxpinyin builds them as Rust values. Their
  out-of-memory abort is Rust's (an internal language artefact with a
  different message). Only the returned buffer comes from the pin's GLib call.
- The sentence chain is the pin's, phrase by phrase:
  - The pin's result vector holds one token at the start column of each
    phrase, and `null_token` elsewhere (`extract_result`,
    `lookup/phonetic_lookup.h:370-396`). Every trellis step carries a real
    token (`unigram_gen_next_step` / `bigram_gen_next_step`, `:643-700`).
    Thus the non-null tokens are the phrases of the decoded path in order,
    constraint tokens included.
  - `Session::sentence_phrases` gives the spans of the row, the same path, in
    that order.
  - A NULL conversion of the first phrase leaves the result NULL
    (`g_strdup(NULL)`), so the next phrase starts it. A NULL conversion of a
    later phrase makes `g_strconcat` stop at it and copy the result. Either
    way, that phrase is dropped. A sentence whose every conversion failed is
    NULL with `true`.
  - Where oxpinyin's remaining-input walk prepends the selection record, its
    pieces come first: each chosen phrase or n-best row phrase, and any raw
    gap that the record holds ([below](#raw-gap-text-in-a-prepended-selection)). A row of the
    pre-frequency fallback scorer, which the pin does not have, is one piece.
  - A row with empty text still returns `false` (`NbestRow` rows with empty
    text are dropped).
- The NULL `middle` of the aux concatenation is unreachable in the pin. The
  walk dies first at `assert(matrix.get_column_size(offset) >= 1)`
  (`pinyin.cpp:3410`, `:3467`, `:3545`), which oxpinyin reproduces.
- The pin never frees the bigram iterator's last `m_phrase`
  (`pinyin.cpp:913-920`). oxpinyin frees it when the handle drops. This is
  not observable.
- `g_strdup` of a Rust string with an interior NUL copies up to the NUL, as
  C does. No returned string here carries one.

### Read paths

`Session::sentence_phrases` gives the parts of the row (`SentenceState::parts`),
the saved prefix first: one `SentencePart::Token` for each phrase and one
`SentencePart::Raw` for each raw gap. `Instance::sentence_phrases` resolves
each token through the live phrase index when the caller reads the sentence,
as `convert_to_utf8` does (`lookup/lookup.cpp:37-48`). A token that does not
resolve repeats the last phrase that resolved in the same read, or adds ""
when none has: the pin ignores the result of `get_phrase_item` (`:43`), and
a failed read leaves the item as it was (`storage/phrase_index.cpp:180-206`,
`storage/phrase_index.h:81-84`). The C helpers (`ffi::sentence_text`,
`write_owned_sentence!`) take that iterator. The GLib calls and their order
are unchanged. The saved prefix stays on the session, because the selection
can change while an older sentence result stays readable. The candidate
string stays a snapshot (`zhuyin.cpp:1865-1870`). `BigramExportWalk::get_next` does not
build the phrase of the row. `ExportedBigramRow` carries the pinyin and count.
The adapter copies the `m_phrase` that it converted from
`BigramExportWalk::pair`.

### Raw gap text in a prepended selection

The selection record of the engine keeps raw bytes as text. These are the
bytes typed between the composition offset and a re-anchored selection, and
between forced runs on a rebuild (`SelectionRecord::append_selected`,
`rebuild_from_constraints` in `oxpinyin-engine/src/session/record.rs`).
`Session::guess_over_remaining` prepends that record to every row
(`oxpinyin-engine/src/session/guess.rs`). The raw gap then becomes part of the
sentence text, and `sentence_phrases` gives it as its own `SentencePart::Raw`. The
pin keeps no such record. It decodes the whole matrix under its
constraints (`pinyin.cpp:1372-1387`, `lookup/phonetic_lookup.h:736-845`),
so its result holds a decoded phrase token there, never raw input.

The register entry for the typed-but-unselected gap law of the engine is
upstream divergences, "zhuyin before-cursor candidate window — CLOSED". It
records the law on the `commit()` surface of the engine, and records the C
sentence after a re-guess as the pin's re-decode.

### Allocation-failure gate

`abi-alloc-failure` (`tools/abi/alloc-failure.cc`) fails these allocations,
on the subject and the pin alike:

- the nth `malloc` whose caller is `g_malloc` (renderer, keyboard array and
  strings, unigram reading, bigram phrase, shengmu);
- the `realloc` from `g_realloc` (GArray growth);
- the `malloc` that libc's `vasprintf` makes under `g_strdup_printf` (tone-3
  zhuyin rendering).

It expects GLib's `g_error` then SIGTRAP or SIGABRT. For `g_strdup_printf`, it
expects `gprintf.c:351: failed to allocate memory` on stderr then SIGABRT, with no
`g_log`.

```sh
cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi
just abi alloc-failure "$PINYIN_ORACLE_PREFIX" \
  "$CARGO_TARGET_DIR/debug/libpinyin_capi.so" \
  "$CARGO_TARGET_DIR/debug/libzhuyin_capi.so"
```

## Open

- Whether a C-API call sequence reaches the remaining-input walk with a raw gap in the selection record: the pin source does not settle it.
