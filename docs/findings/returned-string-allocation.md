# Returned-string allocation (#1227)

2026-10-31 UTC. Pin: libpinyin `074a2219c90feaf962d0d24f034514033ece5f99`,
read from a `git archive` of the pin. GLib: 2.84.4 sources for
`gstrfuncs.c`, `garray.c` and `gutf8.c`; `g_vasprintf` read from the
disassembly of the host's `libglib-2.0.so.0` (2.90.0), which holds
`gprintf.c:351`.

Maintainer ruling 2026-10-31 UTC (compatibility policy, "Returned strings
(#1227)"): where the pin allocates a returned string with `g_strdup`,
`g_strdup_printf`, `g_strconcat`, `g_strjoinv`, `g_malloc` or the
`g_ptr_array_*` calls, oxpinyin calls that function with the pin's
arguments, so allocation failure aborts as it does in the pin. Where the pin
converts with `g_ucs4_to_utf8`, oxpinyin keeps its encoder (#894/#1224) and
makes its allocation fallible: NULL, the pin's return value, no warning, no
abort. The class (c) refusal (`malloc_cstr_with_warning`, the warning
variants of `owned_cstr`/`owned_cstr_list`) is retired.

## Sites

| Export | Pin call | oxpinyin call |
| --- | --- | --- |
| `pinyin_get_pinyin_string`, `zhuyin_get_pinyin_string` (HANYU) | `g_strdup` at tone 0, else `g_strdup_printf("%s%d", …)` (`storage/chewing_key.cpp:53-57`) | `ffi::key_text(ChewingKey::pinyin_text())`: the same branch and call |
| `pinyin_get_zhuyin_string`, `zhuyin_get_zhuyin_string` | `g_strdup` at tone 0 or 1, else `g_strdup_printf("%s%s", …, chewing_tone_table[tone])` (`:80-88`) | `ffi::key_text(ChewingKey::zhuyin_text())` |
| `pinyin_get_luoma_pinyin_string`, `zhuyin_get_pinyin_string` (LUOMA) | `g_strdup` / `g_strdup_printf("%s%d", …)` (`:97-101`) | `ffi::key_text(ChewingKey::luoma_pinyin_text())` |
| `pinyin_get_secondary_zhuyin_string`, `zhuyin_get_pinyin_string` (SECONDARY_ZHUYIN) | `g_strdup` / `g_strdup_printf("%s%d", …)` (`:110-114`) | `ffi::key_text(ChewingKey::secondary_zhuyin_text())` |
| `pinyin_get_pinyin_strings` | `g_strdup` per requested half (`:64`, `:71`; `pinyin.cpp:2759-2762`) | `ffi::strdup` per half, written at once; the deferred cleanup is gone |
| `pinyin_get_{full,double,chewing}_pinyin_auxiliary_text` | `g_strdup("")` for an empty matrix (`pinyin.cpp:3386`, `:3445`, `:3523`); else `g_strconcat(prefix, middle, postfix, NULL)` (`:3431`, `:3509`, `:3569`) | `ffi::strdup("")` / `ffi::strconcat_aux` over the three parts (`aux_matrix::AuxText`) |
| `pinyin_in_chewing_keyboard`, `zhuyin_in_chewing_keyboard` | `g_ptr_array_new`, `g_strdup` + `g_ptr_array_add` per symbol, NULL, `g_ptr_array_free(array, FALSE)`; no symbol: `g_ptr_array_free(array, TRUE)` (`storage/zhuyin_parser2.cpp:301-332`, `:496-544`, `:798-851`) | `ffi::symbol_vector` with the same calls |
| `pinyin_iterator_get_next_phrase` reading | per-key `get_pinyin_string` into a `GPtrArray`, `g_strjoinv("'", …)`, `g_strfreev` (`pinyin.cpp:726-733`) | `ffi::strjoinv` over Rust-built key strings |
| `pinyin_bigram_iterator_get_next_phrase` | `has_next`: two `g_ucs4_to_utf8` (`:822`, `:828`), `g_strconcat(first, second, NULL)` into `m_phrase` (`:830-832`); `get_next`: `g_strdup(m_phrase)`, `g_strdup(pinyin)` (`:904-905`) | `BigramHandle::convert_loaded_pair` at each load (fallible encoder twice, `ffi::strconcat_pair`, `m_phrase` kept on the handle; a NULL result drops the walk's pair, the pin's NULL `m_phrase`), then `ffi::strdup_raw` / `ffi::strdup` per row |
| `pinyin_get_sentence`, `zhuyin_get_sentence` | `convert_to_utf8` (`lookup/lookup.cpp:27-69`) over the n-best result (`pinyin.cpp:1464-1482`, `zhuyin.cpp:983-998`): per non-null token `g_ucs4_to_utf8` (`:58`); while the result is NULL `g_strdup(phrase)` (`:62-63`), else `g_strconcat(result, "", phrase, NULL)` (`:65`); `g_free` of the phrase and the previous result (`:66-67`); returns `true` (`:69`) | `ffi::sentence_text` over `Session::sentence_phrases`: per phrase the fallible encoder, then the same `g_strdup`/`g_strconcat`/`g_free` chain, `true` even when NULL |

The `g_ucs4_to_utf8` sites keep `ffi::malloc_cstr_with`, already fallible
and silent before this change: the unigram phrase (`pinyin.cpp:715-716`),
`*_token_get_phrase` (`pinyin.cpp:1644`, `zhuyin.cpp:1112`), the two bigram
conversions and each sentence phrase's conversion. Those buffers come from libc
`malloc`; `g_free` releases them.

## Left as it was, and limits

- `pinyin_iterator_get_next_phrase`'s phrase and `*_token_get_phrase`
  already behaved per the ruling; unchanged.
- The pin's per-key `GPtrArray` and strings for the reading join, its
  `m_pinyins` strings and the aux key strings are temporaries freed before
  return. oxpinyin builds them as Rust values; their out-of-memory abort is
  Rust's (an internal language artefact with a different message), and only
  the returned buffer comes from the pin's GLib call.
- The sentence chain is the pin's, phrase by phrase. The pin's result
  vector holds one token at each phrase's start column and `null_token`
  elsewhere (`extract_result`, `lookup/phonetic_lookup.h:370-396`); every
  trellis step carries a real token (`unigram_gen_next_step` /
  `bigram_gen_next_step`, `:643-700`), so the non-null tokens are the
  decoded path's phrases in order, constraint tokens included.
  `Session::sentence_phrases` hands out the row's spans, the same path, in
  that order. A NULL conversion of the first phrase leaves the result NULL
  (`g_strdup(NULL)`), so the next phrase starts it; a later one makes
  `g_strconcat` stop at it and copy the result: either way that phrase is
  dropped, and a sentence whose every conversion failed is NULL with `true`.
  Where oxpinyin's remaining-input walk prepends the selection record, its
  pieces come first: each chosen phrase or n-best row phrase, and any raw
  gap the record holds (next section). A row of the pre-frequency fallback
  scorer, which the pin does not have, is one piece. A row with empty text
  still answers `false`, as before (`NbestRow` rows with empty text are
  dropped).
- The aux concatenation's NULL `middle` is unreachable in the pin: the walk
  dies first at `assert(matrix.get_column_size(offset) >= 1)`
  (`pinyin.cpp:3410`, `:3467`, `:3545`), which oxpinyin reproduces.
- The pin never frees the bigram iterator's last `m_phrase`
  (`pinyin.cpp:913-920`); oxpinyin frees it when the handle drops, which is
  not observable.
- `g_strdup` of a Rust string with an interior NUL copies up to the NUL, as
  C does; no returned string here carries one.

## Pre-existing difference: raw gap text in a prepended selection

Not introduced by #1227 or by this stack; recorded here because the
per-phrase sentence chain names it. The engine's selection record keeps
the raw bytes typed between the composition offset and a re-anchored
selection, and between forced runs on a rebuild, as text
(`SelectionRecord::append_selected`, `rebuild_from_constraints` in
`oxpinyin-engine/src/session/record.rs`). When
`Session::guess_over_remaining` prepends that record to every row
(`oxpinyin-engine/src/session/guess.rs`), the raw gap becomes part of the
sentence text, and `sentence_phrases` hands it out as its own piece. The
pin keeps no such record: it decodes the whole matrix under its
constraints (`pinyin.cpp:1372-1387`, `lookup/phonetic_lookup.h:736-845`),
so its result holds a decoded phrase token there, never raw input. The
existing register entry for the engine's typed-but-unselected gap law is
upstream divergences, "zhuyin before-cursor candidate window — CLOSED",
amendment 2026-09-06, which records it on the engine's `commit()` surface
and the C sentence after a re-guess as the pin's re-decode. Whether a
C-API call sequence reaches the remaining-input walk with a gap in the
record was not settled from source here; no probe was run.

## Gate

`abi-alloc-failure` (`tools/abi/alloc-failure.cc`) fails, on the subject and
the pin alike, the nth `malloc` whose caller is `g_malloc` (renderer,
keyboard array and strings, unigram reading, bigram phrase, shengmu), the
`realloc` from `g_realloc` (GArray growth), and the `malloc` libc's
`vasprintf` makes under `g_strdup_printf` (tone-3 zhuyin rendering). It
expects GLib's `g_error` then SIGTRAP or SIGABRT, and for `g_strdup_printf`
`gprintf.c:351: failed to allocate memory` on stderr then SIGABRT, with no
`g_log`.

```sh
just heavy -- cargo build --locked -p oxpinyin-capi -p oxpinyin-zhuyin-capi
just heavy -- just abi alloc-failure "$PINYIN_ORACLE_PREFIX" \
  "$CARGO_TARGET_DIR/debug/libpinyin_capi.so" \
  "$CARGO_TARGET_DIR/debug/libzhuyin_capi.so"
```
