# C string inputs at the exported boundary (#1279)

Source audit at `fd8f1775`, recorded 2026-10-26 UTC. This is an inventory
checkpoint, not a claim that the differences below are fixed or verified.

Read trees: libpinyin checkout at
`074a2219c90feaf962d0d24f034514033ece5f99`; GLib checkout at tag `2.84.4`.
Pin citations below are relative to libpinyin `src/`. GLib citations are
relative to its `glib/`. No exploratory pin execution was used.

The public headers `pinyin.h` and `zhuyin.h`, checked against
`crates/oxpinyin-capi/libpinyin.ver` and
`crates/oxpinyin-zhuyin-capi/libzhuyin.ver`, contain **25 exported functions
with 29 input string parameters**. Each parameter has a row below.
`gchar **` and `const gchar **` parameters in the remaining exports are
outputs, not input strings. `in_chewing_keyboard` takes a single byte.
The non-shipping fixture hooks in `oxpinyin-capi/src/context.rs`
(`oxpinyin_init_for_fixtures` and `oxpinyin_test_set_user_bigram`)
are not pin exports, but their conversion callers
must also move when the helpers move.

## Conversion rules

**U — `g_utf8_to_ucs4(input, -1, NULL, &length, NULL)`.**
GLib `gutf8.c:938-993`, with `g_utf8_get_char_extended` at `:661-743`,
accepts canonical one-to-six-byte encodings through `0x7fffffff`, including
surrogates and values above Unicode's maximum. It rejects overlong forms,
bad continuations, truncated sequences and `fe`/`ff` lead bytes. On
rejection it returns NULL without writing `length`. Every entry below
initializes its local length to zero, except pinyin `_compute_prefixes`,
which uses the instance field and guards the returned pointer before
using its length. NULL input is dereferenced at `gutf8.c:950`, unless the
entry guards it first. Allocation failure also leaves the length unwritten.
Accepted words must remain `u32` values in `PhraseText`, not be converted
through `char`, a lossy string or an empty-string fallback.

**L — `g_utf8_strlen(input, -1)` before U.**
GLib `gutf8.c:239-278` counts by the skip table, without validating.
NULL logs its `g_return_val_if_fail` critical and returns zero (`:245`),
after which these entries still call U and dereference NULL. A truncated
lead can skip the terminating NUL and read past the allocation: unlike U
alone, this is not a defined rejected conversion. Canonical non-scalars
have the same L and U lengths. Defined malformed scans with nonzero L
and failed U reach the entry's length-mismatch critical, returning false
without changing the phrase result. Do not describe all malformed scans
as that defined case.

**R — bytes plus `strlen`, without Unicode conversion.**
The full/double and keyboard parsers consume their byte alphabets. Batch
parsers keep keys parsed before a rejected byte. Whole-key parsers reject
a spelling outside their index, subject to their checks before rejection.
Full-pinyin `parse_one_key` duplicates the bytes, asserts no apostrophe,
then zeroes the output key (`storage/pinyin_parser2.cpp:164-175`). Thus
an apostrophe anywhere, including after invalid UTF-8, reaches the assert.
Double-pinyin and keyboard failures generally leave the output key alone
(`storage/pinyin_parser2.cpp:405-530`;
`storage/zhuyin_parser2.cpp:162-214,335-414,573-724`). NULL reaches
`strlen` before any parser mutation. Empty full-pinyin and Simple keyboard
inputs with USE_TONE have a separate pre-buffer read (`:180` and `:174`);
they are not suitable non-empty controls.

**D — direct zhuyin reading bytes.**
`storage/zhuyin_parser2.cpp:914-959` splits on ASCII space/apostrophe,
parses a complete segment, and retains earlier keys when a later segment
fails. `parse_one_key` (`:859-912`) walks character starts and copies the
last character to test its tone before comparing the complete segment to
the index. Canonical non-scalars cannot match that index. Malformed bytes
in a later segment do not invalidate earlier keys. Cutting an invalid
segment to its valid UTF-8 prefix is wrong: `ㄉㄚ` followed immediately by
`ff` is one failed segment, whereas `ㄉㄚ ff` retains the first key.
Truncated final character handling can advance a pointer beyond the string
allocation in the character-start walk and in `g_utf8_strncpy` (GLib
`gutf8.c:451-464`). Its `strncpy` stops reading at NUL and pads; it is not
evidence of an inevitable read fault. Keep that pointer-arithmetic UB
separate from a verified crash, and do not invent a pin return value.

**P — paths.** `g_strdup` copies bytes and preserves NULL (GLib
`gstrfuncs.c:315-330`). `g_build_filename` uses the path builder
(`gfileutils.c:1904-1999,2202-2243`): NULL ends the argument list;
empty components are skipped. On the reference Unix host invalid UTF-8
and non-scalar encodings are ordinary filename bytes. NULL systemdir
builds an empty filename; empty systemdir builds `table.conf` in cwd.
NULL userdir means no user directory; empty userdir means cwd.

## Per-parameter inventory

Paths in the oxpinyin column start at `crates/`; each names the file and
item, not a line. `capi` abbreviates `oxpinyin-capi`, `zhuyin-capi`
abbreviates `oxpinyin-zhuyin-capi`. U/L/R/D/P include the invalid UTF-8 and
non-scalar rules above. State columns concern live handles and writable
out-parameters; invalid handles remain outside this string audit.

| Export / parameter | Pin conversion and cite | NULL and state before conversion | Current oxpinyin helper/result at fd8f1775 | Classification |
|---|---|---|---|---|
| `pinyin_init` / systemdir | P, `pinyin.cpp:326-341` | Allocates context, sets options, duplicates paths; later load resets LC_NUMERIC and emits `open  failed.\nload  failed!\n` for NULL systemdir (`storage/table_info.cpp:194-203`). | `capi/src/context.rs` `init_context`: `ffi::cstr_to_path`, locale reset, exact NULL failure; Unix bytes preserved. | Matching Unix path boundary. |
| `pinyin_init` / userdir | P, `pinyin.cpp:332-345` | NULL is allowed; user-dependent operations see no user directory. Context initialization precedes file I/O. | Same item/helper: `Option<PathBuf>` distinguishes NULL and empty. | Matching Unix path boundary. |
| `zhuyin_init` / systemdir | P, `zhuyin.cpp:270-285` | Same path rules and locale side effect; initial options differ. | `zhuyin-capi/src/context.rs` `zhuyin_init`: `ffi::cstr_to_path`; preserves bytes. | Matching Unix path boundary. |
| `zhuyin_init` / userdir | P, `zhuyin.cpp:276-289` | NULL allowed; no user directory. | Same item/helper: `Option<PathBuf>`. | Matching Unix path boundary. |
| `pinyin_iterator_add_phrase` / phrase | U, `pinyin.cpp:614-646` | Reads iterator context/index; guards either NULL string and returns false. No persistent mutation before conversion. Failed U leaves length zero and is refused. | `capi/src/iterators.rs` `pinyin_iterator_add_phrase`: `cstr_to_owned_lossy` actually does strict UTF-8-or-empty; `chars().count()` and `&str` store calls discard non-scalars. | Defect: preserve accepted non-scalars through table lookup, item comparison and storage. Malformed/NULL refusal matches. |
| `pinyin_iterator_add_phrase` / pinyin | R, `pinyin.cpp:623-646`, `storage/pinyin_parser2.cpp:217-333` | NULL guarded; phrase conversion precedes parsing; no persistent mutation until length checks pass. | Same item: `cstr_to_parsed_prefix` and `parse_import_pinyin`; keeps keys before invalid bytes. | Matching raw-parser outcome; retain the distinction from D. |
| `zhuyin_iterator_add_phrase` / phrase | U, `zhuyin.cpp:500-532` | Same guards/zero length as pinyin; no persistent mutation before conversion. | `zhuyin-capi/src/iterators.rs` `zhuyin_iterator_add_phrase`: `cstr_to_owned_lossy`, `chars().count()`, `&str` store calls. | Defect: accepted non-scalar words lost. |
| `zhuyin_iterator_add_phrase` / pinyin | D, `zhuyin.cpp:508-532`, `storage/zhuyin_parser2.cpp:859-959` | NULL guarded; converts phrase first; earlier complete segments survive a later failed segment. | Same item: `cstr_to_owned_lossy` rejects the entire reading on any invalid UTF-8. | Defect: loses valid earlier segments. Must not substitute R's arbitrary valid-byte-prefix cut. Truncated pointer-walk UB requires a separately justified disposition. |
| `pinyin_guess_sentence_with_prefix` / prefix | U, `pinyin.cpp:1389-1434` | Resets prefixes to sentence_start and frees old prefix words before conversion. NULL faults; no-input ruling retains the reset and guesses without added tokens. | `capi/src/sentence.rs` `pinyin_guess_sentence_with_prefix`: `ucs4::prefix_text(cstr_opt(...))`, `Option<PhraseText>`. | Matching non-NULL; NULL class (b), already registered under “NULL prediction prefix”. |
| `pinyin_guess_predicted_candidates` / prefix | U, `pinyin.cpp:2413-2427,1389-1398` | Frees candidates, empties prefixes, frees old prefix words. NULL faults; no-input result is false with emptied candidates/prefixes. | `capi/src/predict.rs` `pinyin_guess_predicted_candidates`: `prefix_text`; `clear_borrowed_prefix` handles the alias before reading. | Matching; NULL and freed candidate alias class (b), already registered by #1272/#1277. |
| `pinyin_guess_predicted_candidates_with_punctuations` / prefix | U via preceding entry, `pinyin.cpp:2456-2498` | Same resets; ignores prediction false. No prefixes means true with empty candidates. | `capi/src/sentence.rs` same-named item: shared `prefix_text` and alias handling. | Matching; NULL/alias existing class (b). |
| `zhuyin_guess_sentence_with_prefix` / prefix | U, `zhuyin.cpp:912-953` | Resets prefixes to sentence_start; NULL faults. Invalid conversion adds no prefix. | `zhuyin-capi/src/sentence.rs` same-named item: `prefix_text`, `Option<PhraseText>`. | Matching non-NULL; existing NULL class (b). |
| `pinyin_phrase_segment` / sentence | L then U, `pinyin.cpp:1446-1462` | No phrase-result mutation before conversion. NULL: L critical, then U fault. Defined conversion-length mismatch: entry critical, false, previous result retained. Accepted non-scalars search raw words. | `capi/src/phrase.rs` `segment_text` / `pinyin_phrase_segment`: local `SegmentText`, shared `ucs4::decode`; NULL returns false silently; malformed bytes log entry critical. | Non-scalar/defined malformed gate matches #918. NULL loses a defined prior critical; truncated over-read needs separate class (b) treatment. |
| `zhuyin_phrase_segment` / sentence | L then U, `zhuyin.cpp:965-981` | Same as pinyin. | `zhuyin-capi/src/phrase.rs` same items and behavior. | Same classification as pinyin. |
| `pinyin_parse_full_pinyin` / onepinyin | R, `pinyin.cpp:1484-1495` | No state mutation before strlen. NULL faults before key write. Non-NULL apostrophe asserts before zeroing key. | `capi/src/keys.rs` same-named item: `cstr_to_string`, then apostrophe check, then zeroes key. | Defect: invalid UTF-8 plus apostrophe suppresses class (c) warning and overwrites key. NULL refusal must be registered class (b). |
| `pinyin_parse_more_full_pinyins` / pinyins | R, `pinyin.cpp:1497-1525` | Allocates temporary arrays before strlen; instance lengths/matrix change only after parse. NULL faults. | `capi/src/parse.rs` same-named item / `parse_c_string`: `cstr_to_parsed_prefix`; NULL parses empty and clears parse state. | Matching non-NULL parser input. NULL no-input outcome needs class (b) registration. |
| `pinyin_parse_double_pinyin` / onepinyin | R, `pinyin.cpp:1527-1538` | No mutation before strlen; NULL faults. Parser failure preserves key. | `capi/src/keys.rs` same-named item: `cstr_to_string`, false for invalid, key unchanged. | Matching invalid/non-scalar rejection; NULL class (b), register. |
| `pinyin_parse_more_double_pinyins` / pinyins | R, `pinyin.cpp:1540-1564` | Temporary arrays before strlen; matrix/length changes after parse. NULL faults. | `capi/src/parse.rs` same-named item: `cstr_to_parsed_prefix`. | Matching non-NULL; NULL no-input state needs class (b) registration. |
| `pinyin_parse_chewing` / onechewing | R, `pinyin.cpp:1566-1580` | Masks local correction options before strlen, no stored options mutation. NULL faults. | `capi/src/keys.rs` same-named item: `cstr_to_string`; invalid becomes empty, false without key write. | Matching non-alphabet rejection; NULL class (b), register; empty Simple-parser UB is distinct. |
| `pinyin_parse_more_chewings` / chewings | R, `pinyin.cpp:1582-1612` | Local option mask and temporary arrays before strlen; matrix/length changes after parse. NULL faults. | `capi/src/parse.rs` same-named item: `cstr_to_parsed_prefix`. | Matching non-NULL; NULL no-input state class (b), register. |
| `zhuyin_parse_full_pinyin` / onepinyin | R, `zhuyin.cpp:1001-1015` | Masks local correction bits; otherwise full-pinyin ordering above. | `zhuyin-capi/src/keys.rs` same-named item: `cstr_to_string` before apostrophe check/key zero. | Same invalid-plus-apostrophe defect; NULL class (b). |
| `zhuyin_parse_more_full_pinyins` / pinyins | R, `zhuyin.cpp:1017-1043` | Local option mask/temporary arrays before strlen; lengths/matrix after parse. | `zhuyin-capi/src/parse.rs` same-named item: `cstr_to_parsed_prefix`. | Matching non-NULL; NULL class (b), register. |
| `zhuyin_parse_chewing` / onechewing | R, `zhuyin.cpp:1045-1056` | No mutation before strlen; NULL faults. | `zhuyin-capi/src/keys.rs` same-named item: `cstr_to_string`; false, key unchanged on rejected input. | Matching non-alphabet rejection; NULL class (b), register; distinguish empty Simple-parser UB. |
| `zhuyin_parse_more_chewings` / chewings | R, `zhuyin.cpp:1058-1082` | Temporary arrays before strlen, matrix/length after parse. NULL faults. | `zhuyin-capi/src/parse.rs` same-named item: `cstr_to_parsed_prefix`. | Matching non-NULL; NULL class (b), register. |
| `pinyin_lookup_tokens` / phrase | U, `pinyin.cpp:2652-2667` | No output mutation before conversion. Afterwards searches words, then clears/fills output array in `reduce_tokens` (`storage/phrase_large_table3.h:77-97`). NULL faults first. | `capi/src/dict.rs` `pinyin_lookup_tokens`: `cstr_to_string`, `tokens_for_text`; NULL returns before clearing array. | Defect: non-scalars become empty lookup. Invalid conversion currently searches empty as pin does; NULL needs registered no-input array outcome. |
| `zhuyin_lookup_tokens` / phrase | U, `zhuyin.cpp:1678-1693` | Same ordering and NULL fault. | `zhuyin-capi/src/dict.rs` same-named item: `cstr_to_string`, `tokens_for_text`. | Same defects as pinyin. |
| `pinyin_get_character_offset` / phrase | U, `pinyin.cpp:3193-3239` | Empty matrix returns first; offset and `_check_offset` assertions precede NULL guard/conversion. NULL then returns false with length untouched; failed U length zero returns false. | `capi/src/sentence.rs` same-named item: `cstr_to_strict` before `InstanceCore::character_offset`; rejects non-scalars and bypasses prior offset validation for NULL/invalid UTF-8. | Defects: raw-word walk and prior-check ordering. Needs coordinated `Session::character_offset` type change in excluded session directory. |
| `zhuyin_get_character_offset` / phrase | U, `zhuyin.cpp:2148-2194` | Empty matrix and range assert first; `_check_offset` is called without the outer pinyin assert. NULL returns false with length untouched; failed U returns false. | `zhuyin-capi/src/sentence.rs` same-named item: `cstr_to_strict` before core offset walk. | Same conversion/order defects; preserve the pin's facade-specific assertion distinction. |
| `pinyin_remember_user_input` / phrase | U, `pinyin.cpp:3669-3707` | NULL explicitly returns false; converts before length checks and per-word token lookup. Persistent mutation only in later recursive add. | `capi/src/user_data.rs` same-named item: `cstr_to_string`, `chars`, scalar per-character lookup, `UserStore::add_phrase(&str, ...)`. | Defect: non-scalar input rejected even when corresponding raw-word dictionary tokens exist. Malformed/NULL refusal matches. |

## Implementation and verification boundary

Use one safe conversion module in `oxpinyin-capi-marshal` over
`Option<&CStr>` or borrowed bytes. Keep only the nullable pointer borrow
at each C adapter (or stamp that edge once with the existing macro pattern).
Return `Option<PhraseText>` for U, a Rust representation of the complete
raw-parser decision for R/D, and `Option<PathBuf>` for P. No internal
function needs `c_char`. `ucs4::decode` and `prefix_text` already supply U's
word decoder; do not add another decoder or emulate GLib objects.

Raw-word input must continue through `UserStore::add_phrase_in` and
`add_system_phrase_in` in `oxpinyin-user/src/store.rs`, exact table keys,
stored item comparison, and the remember/offset per-word token lookups.
Do not implement the C conversion and then turn its result back into a
strict scalar string. `Dictionary::tokens_for_ucs4` and the user store's
`phrase_table_tokens_for_ucs4` already provide raw-word lookup.

The character-offset forwarding path currently requires `&str` in
`oxpinyin-facade/src/cursor.rs` `InstanceCore::character_offset`,
`oxpinyin-engine/src/session/offsets.rs` `Session::character_offset`, and
`oxpinyin-engine/src/char_offset.rs` `character_offset_over_keys`. The
middle file is explicitly excluded from Lane B. Changing only the outer
adapter cannot carry non-scalars through that path. Duplicating the
session's graph/scan-matrix logic in the adapter would defeat the shared
implementation requirement. Coordination is pending; no excluded file
has been changed.

Verification additions belong in `tools/bisection/contract-diff.py`:
non-scalar import/lookup/remember/offset fixtures with non-empty scalar
controls, direct-zhuyin invalid later segment versus invalid same segment,
invalid-plus-apostrophe whole-key inputs with output-key sentinels, and
NULL/state sequences. `phrase-text_system` and the raw-word fixture
builders already support the required words. Keep defined malformed and
truncated-over-read cases separate. Use `pin_not_run` for deliberately
avoided UB; use `crash=` only where the corresponding source-derived
fault is verified by the existing driver. Do not label policy differences
MATCH. Parent revert-and-check, selected-cell neighboring families,
touched-crate gates and stripped sizes remain required and unrun at this
inventory checkpoint.

## Implementation progress after the inventory checkpoint

The shared `oxpinyin-capi-marshal/src/input.rs` module now serves both
facades. `utf8_to_ucs4` retains historical non-scalars as `PhraseText`;
`direct_zhuyin_reading` retains only complete preceding segments; paths
remain native filename bytes. The adapter-only `borrow_c_string` macro
contains the nullable pointer edge. Both duplicated conversion sets and
both `SegmentText` decoders are removed.

Import, lookup and remember now carry accepted words through table search,
item comparison and user storage. `oxpinyin-user/src/store.rs`
`add_phrase_in` and `add_system_phrase_in` accept native `PhraseText`
conversions and write the full word keys; `phrase_table.rs`
`add_index_words` shares the existing index algorithm. Whole-key full
pinyin checks raw apostrophe bytes before scalar spelling conversion.
NULL lookup clears the output array; NULL segmentation preserves the
prior GLib critical and then clears the phrase result by segmenting no
input. These state-sensitive exceptions are registered in
`upstream-divergences.md`.

The `c-string-input` family of the existing contract driver covers these
changes, including non-empty scalar controls, output-key sentinels, raw-word
fixtures, NULL state checks and non-scalar character-offset words. The
character-offset raw words reach `Session::character_offset` as the stored
`PhraseText` (maintainer ruling); no part of the audit remains blocked.
