# ABI allocator pairing — the register and the gate

Status: **contract** (the two `.alloc` registers).

`oxpinyin-capi` and `oxzhuyin-capi` ship as drop-in replacements for
`libpinyin.so.15` / `libzhuyin.so.15`. Exactly one side owns every pointer
that the ABI hands a consumer, and exactly one function releases it.
`just abi` checks the exported symbols against the version scripts and the
ownership of each pointer against the allocation registers.

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
  written (`pinyin.cpp:904-910`). These slots
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

Run `just abi` for the same check as the Linux sanitizer job. The gate reads
`capi/libpinyin.alloc` and `zhuyin-capi/libzhuyin.alloc`. Each register
sits beside its version script. Each row names the symbol and the return
value or output parameter, followed by three fields.

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

The gate compares each header with its register. Every pointer return and
every pointer out-parameter must have exactly one entry. The C type determines
which ownership classes are valid. Each symbol and each named handle
destructor must appear in the corresponding version script.

On Linux the Rust test in `tools/abi` loads each shared library by path and
calls its exported C functions. It generates its slot list from the registers.
The release code checks the operation it actually performs, including the
name of each handle destructor, against that list. The consumer resizes and
writes each owned string before calling `g_free`. It frees and replaces a
vector element, resizes the vector, appends another string and calls
`g_strfreev`. Borrowed pointers stay with their owners.

A warm-up lifecycle runs with leak detection disabled to absorb allocations
made on first use. The measured lifecycle starts with fresh coverage and
leak detection enabled. It must obtain a live pointer for all 26 pinyin
slots and all 12 zhuyin slots. Training a two-phrase sentence supplies
bigram rows. The zhuyin fixture uses chewing keystrokes.

Each reachable refusal starts with a sentinel in the output slot. The test
checks that the call returned false and that it cleared the pointer, left
the sentinel in place, or supplied an allocation, as registered. Inputs
include the empty parse, an out-of-range index on a populated key matrix,
the zero key, an unmapped keyboard key and `null_token`. The terminal
bigram row supplies two strings on false. The test releases both. It stops
the unigram iterator at exhaustion, where another read would abort.

The test runs with the nightly in `fuzz/rust-toolchain.toml`,
`-Zsanitizer=address` and `ASAN_OPTIONS=detect_leaks=1`. Negative controls
run the same test executable in child processes. The parent requires a
nonzero exit and the corresponding LeakSanitizer or AddressSanitizer
diagnostic for a missing free or an invalid free.

## 4. What this does not cover

- **Consumer-side discipline.** The gate proves the library's half. A
  consumer that frees a handle with `g_free`, or frees a borrowed candidate
  pointer, has a consumer bug. Such a consumer is reviewed against the register.
- **The GArray surface.** The caller owns these arrays, and glib's own
  allocator is on both sides. The gate creates and frees those arrays but
  has nothing to prove about their pairing.
- **Warm-up allocations.** LeakSanitizer ignores allocations made on the
  test thread while leak detection is disabled. It can report unreachable
  allocations from the backend as well as the facade during the measured lifecycle.
- **Platform coverage.** The checked allocation lifecycle runs on Linux.
  The macOS gate checks the registers and the shipped libraries' exports.

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
  - A row of the pre-frequency fallback scorer, which the pin does not
    have, is one piece.
  - A row whose text is empty is a row: `pinyin_get_sentence` returns
    `true` with `""`, as `convert_to_utf8` builds it
    (`lookup/lookup.cpp:39-68`). Both sentence getters keep a row whose
    phrases all fail to convert and answer `true` with NULL
    (`pinyin.cpp:1464-1482`, `zhuyin.cpp:983-999`). Candidate dedup skips
    NULL text on its comparison path (policy rows 80 and 106, class (b)).
- The NULL `middle` of the aux concatenation is unreachable in the pin. The
  walk dies first at `assert(matrix.get_column_size(offset) >= 1)`
  (`pinyin.cpp:3410`, `:3467`, `:3545`), which oxpinyin reproduces.
- The pin never frees the bigram iterator's last `m_phrase`
  (`pinyin.cpp:913-920`). oxpinyin frees it when the handle drops. This is
  not observable.
- `g_strdup` of a Rust string with an interior NUL copies up to the NUL, as
  C does. No returned string here carries one.

### Read paths

`Session::sentence_phrases` gives the parts of the row (`SentenceState::parts`):
one `SentencePart::Token` for each phrase, or one `SentencePart::Raw` for a
row with no phrase. `Instance::sentence_phrases` resolves
each token through the live phrase index when the caller reads the sentence,
as `convert_to_utf8` does (`lookup/lookup.cpp:37-48`). A token that does not
resolve repeats the last phrase that resolved in the same read, or adds ""
when none has: the pin ignores the result of `get_phrase_item` (`:43`), and
a failed read leaves the item as it was (`storage/phrase_index.cpp:180-206`,
`storage/phrase_index.h:81-84`). The C helpers (`ffi::sentence_text`,
`write_owned_sentence`) take that iterator. The GLib calls and their order
are unchanged. The candidate
string stays a snapshot (`zhuyin.cpp:1865-1870`). `BigramExportWalk::get_next` does not
build the phrase of the row. `ExportedBigramRow` carries the pinyin and count.
The adapter copies the `m_phrase` that it converted from
`BigramExportWalk::pair`.

### Allocation-failure gate

`just abi alloc-failure` (`tools/abi/alloc-failure.cc`) fails these allocations,
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
cargo build --locked -p oxpinyin-capi -p oxzhuyin-capi
just abi alloc-failure "$PINYIN_ORACLE_PREFIX" \
  "$CARGO_TARGET_DIR/debug/liboxpinyin_capi.so" \
  "$CARGO_TARGET_DIR/debug/liboxzhuyin_capi.so"
```
