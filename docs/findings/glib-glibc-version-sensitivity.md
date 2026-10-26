# GLib and glibc version sensitivity of the reimplemented behaviours (#1298)

Status: **record** (read-only investigation; no code change) · 2026-10-26
UTC (`date -u`: 2026-10-26T11:36:16Z) · Parent: #915 · Inventory:
[glib-libc-inventory.md](glib-libc-inventory.md) (#917), read at main
`3b481075`.
Pin: libpinyin 2.11.92 at `074a2219c90feaf962d0d24f034514033ece5f99`, read
from a shallow fetch of that commit (github.com/libpinyin/libpinyin).
Reference image: `debian:trixie`, GLib 2.84.4-3~deb13u5, glibc
2.41-12+deb13u4 (upstream 2.84.4 and 2.41).

Trees read:

- GLib: the github.com/GNOME/glib mirror, a blobless clone at
  `50b938a3e8f5154a25b5af3dd8b9dd8b2ccba7c3` (2026-10-25); tags 2.72.4
  (`6ed6303e`), 2.80.0 (`763cc3b2`), 2.84.4 (`41eca608`), 2.86.5
  (`3033f786`), 2.88.0 (`7a03e2ef`), 2.88.4 (`802d1893`), 2.90.0
  (`436d47a7`) and 2.90.1 (`e05063cc`).
- glibc: the github.com/bminor/glibc mirror (`04e750e7`, 2026-01-20, which
  ends before 2.43) and sourceware.org/git/glibc.git for `glibc-2.43`
  (`f762ccf8`), `glibc-2.44` (`c3a3a980`), `release/2.44/master` at
  `1848099f` (the commit Arch builds) and `master` at `d259884e`
  (2026-10-24); tags 2.35 (`f94f6d8a`), 2.39 (`ef321e23`), 2.41
  (`74f59e92`) and 2.42 (`d2097651`).
- Apple `Libc-1752.120.2`, `stdio/FreeBSD/vfscanf.c`
  (github.com/apple-oss-distributions/Libc), one function, for the macOS
  row.

Nothing was run: no build, no probe, no differential. GLib and glibc line
numbers are those of the named tag; pin line numbers are 074a2219's;
oxpinyin code is cited by file and item.

## Result

Across GLib 2.72.4 to 2.90.1 and glibc 2.35 to 2.44, no behaviour that the
shipped libraries reimplement changes its outward result on valid input,
for caller text shorter than 2³¹ characters. The claim covers the shipped
libraries only. The tools have one valid-input exception, row G: their
taglib tokenizer splits CJK text by the host's Unicode tables (item 2).
Caller text of 2³¹ characters or more is outside the claim too: the C entry
points convert it with no length limit, and GLib up to 2.86.5 counts it in
a `gint` (U, below).

The decoder and encoder behind `g_utf8_to_ucs4` and `g_ucs4_to_utf8`, the
`g_sort_array` merge, `g_strconcat`, `g_build_filename` and
`g_strsplit_set` are textually identical at every shipped tag, apart from
integer-width changes and overflow checks. Of those, only the
`g_utf8_to_ucs4` counters are reachable, and only by that long caller
text. Two things do change:

1. **glibc's `%f` lexer.** 2.41 consumes a `nan(…)` payload, 2.42 rejects
   an exponent mark without digits, 2.43 rejects a significand without
   digits, and 2.44 pushes back the byte that breaks `nan` or `inf`; 2.41
   also fixes `strtof`'s subnormal rounding. Each changes what a real
   libpinyin does with a malformed `lambda parameter:` line and nothing
   else: `%d`, `%255s`, `strtol` and `atoi` are unchanged on every path
   the pin uses. The subject's fixed scanner follows the newest grammar
   (2.43's, which 2.44 leaves observably intact). It agrees on every rule
   with the glibc 2.43 and 2.44 hosts: Fedora 44 and newer, Debian testing,
   Ubuntu 26.04 and 26.10, and Arch. With Fedora 43's glibc 2.42 it agrees
   on N1 to N3 and differs on N4: Fedora 43 loads `0x.` as λ = 0, and the
   subject refuses it. Against the reference image's 2.41 it differs on N3
   and N4, which the `host_scanf` markers report as HOST-DEPENDENT (the
   `0x.` marker by the 2026-10-25 ruling).
2. **GLib's Unicode tables** behind `g_unichar_isgraph`: Unicode 14 in
   2.72, 15.1 in 2.80, 16.0 in 2.84 and 2.86, 17.0 in 2.88 and 2.90. The
   function reaches only the tools' taglib tokenizer, which reads
   `interpolation2.text` and the k-mixture model text.

Every change classified here is host-dependent and recordable under
#1303's platform clause; none makes oxpinyin's platform-independent logic
differ between hosts, and none forces a new decision on a specific
behaviour. The `g_utf8_to_ucs4` counters at 2³¹ characters or more are not
classified: on the older GLibs the result there is undefined behaviour,
which this record does not derive. One attribution is corrected below:
the rejection of `1e`-type spellings that the G11 records credit to glibc
2.43-3 is glibc 2.42's commit; only the `0x.` rule is 2.43's.

## Versions examined

Package versions as the distribution package pages showed them on
2026-10-26 (tracker.debian.org, launchpad.net, packages.fedoraproject.org,
archlinux.org, formulae.brew.sh). Distribution patch sets were not read;
the comparison is between upstream tags.

| Distribution (state on 2026-10-26) | GLib | glibc |
| --- | --- | --- |
| Debian 13 trixie, stable; the reference image | 2.84.4 (`2.84.4-3~deb13u3`; the image carries `~deb13u5`) | 2.41 (`2.41-12+deb13u3`; the image carries `+deb13u4`) |
| Debian testing (forky) | 2.90.1 (`2.90.1-2`) | 2.43 (`2.43-7`; experimental has `2.44-3`) |
| Ubuntu 22.04 LTS jammy, supported | 2.72.4 (`2.72.4-0ubuntu2.10`) | 2.35 (`2.35-0ubuntu3.15`) |
| Ubuntu 24.04 LTS noble, supported | 2.80.0 (`2.80.0-6ubuntu3.10`) | 2.39 (`2.39-0ubuntu8.9`) |
| Ubuntu 26.04 LTS resolute, supported | 2.88.0 (`2.88.0-1ubuntu0.1`) | 2.43 (`2.43-2ubuntu2.4`) |
| Ubuntu 26.10 stonking, current stable release | 2.90.0 (`2.90.0-1`) | 2.44 (`2.44-1ubuntu2`) |
| Fedora 43 | 2.86.5 (`2.86.5-1.fc43`) | 2.42 (`2.42-17.fc43`) |
| Fedora 44 | 2.88.4 (`2.88.4-1.fc44`) | 2.43 (`2.43-9.fc44`) |
| Fedora 45, branched; final release still pending on 2026-10-26 | 2.90.0 (`2.90.0-1.fc45`; 2.90.1 in testing) | 2.44 (`2.44-1.fc45`) |
| Fedora Rawhide | 2.90.1 (`2.90.1-1.fc46`) | master snapshot (`2.44.9000-5.fc46`) |
| Arch, rolling | 2.90.1 (`2.90.1-1`, 2026-10-07) | 2.44 release branch at `1848099f` (`2.44+r50+g1848099f063e-1`) |
| Homebrew on macOS | 2.90.1 (`glib` 2.90.1) | not glibc: Apple's libSystem (`Libc-1752.120.2` read) |

Ubuntu 27.04 (tenacious, in pre-release freeze) carries the same 2.90.0
and 2.44 as 26.10. The oldest version in the range is set by jammy, the
oldest LTS still in standard support.

Upstream versions compared: GLib 2.72.4, 2.80.0, 2.84.4, 2.86.5, 2.88.0,
2.88.4, 2.90.0 and 2.90.1; glibc 2.35, 2.39, 2.41, 2.42, 2.43, 2.44, the
2.44 release branch at `1848099f` and master at `d259884e`. None of the 50
commits between `glibc-2.44` and `1848099f` touches a file read here.

## Method

For GLib, every function below was cut out of its file at each of the
eight tags (from its definition line to the closing brace at column 0) and
compared byte for byte with the 2.84.4 text; where a tag differed, the
difference was diffed, the commit that introduced it found with
`git log -S`, and the first release tag containing that commit with
`git tag --contains`. The `UTF8_COMPUTE`, `UTF8_LENGTH`, `UTF8_GET` and
`UNICODE_VALID` macro block (2.84.4 `glib/gutf8.c:45-114`) and the
`utf8_skip_data` table after it (`:115-124`) were compared the same way,
and the Unicode data version was read from `glib/gunichartables.h` line 7
at each tag.

For glibc, the diffs of `stdio-common/vfscanf-internal.c`,
`stdlib/strtod_l.c` and `stdlib/strtol_l.c` between each pair of
consecutive shipped releases (2.35 to 2.39, 2.39 to 2.41, 2.41 to 2.42,
2.42 to 2.43, 2.43 to 2.44, 2.44 to `1848099f`, 2.44 to master at
`d259884e`) were read in full, together with the commit logs of
`stdlib/strtol.c`, `stdlib/atoi.c` and `locale/C-ctype.c` over the same
range. Each commit named below is dated with the first release tag that
contains it.

"Changed" below means the function's text changed between two shipped
versions; the outward column says whether the pin's call site can observe
it.

## GLib

| Behaviour | oxpinyin counterpart | GLib function (2.84.4) | Text across 2.72.4 to 2.90.1 | Outward |
| --- | --- | --- | --- | --- |
| U1 UTF-8 to UCS-4 decoding | `oxpinyin-capi-marshal/src/ucs4.rs` `decode`, `prefix_text`; `input.rs` `utf8_to_ucs4` | `g_utf8_get_char_extended` (`gutf8.c:662-744`), `g_utf8_to_ucs4` (`:938-995`), `g_utf8_get_char` (`:333-345`), the macro block (`:45-114`), `utf8_skip_data` | identical at all eight tags, except that `g_utf8_to_ucs4` counts in `size_t` instead of `gint` from 2.87.3 (commit `0bb99fa46`, 2026-01-20; shipped from 2.88.0) | unchanged below 2³¹ characters |
| U2 UCS-4 to UTF-8 encoding | `oxpinyin-capi-marshal/src/phrase_text.rs` `phrase_text_bytes`, `encode_word`; `oxpinyin-core/src/ucs4.rs` `PhraseText::from_words` (the refusal at `0x80000000`) | `g_ucs4_to_utf8` (`:1022-1069`), `g_unichar_to_utf8` (`:546-596`), `g_string_insert_unichar` (`gstring.c:816-888`, the punctuation path) | identical, except `size_t` counters in `g_ucs4_to_utf8` (`0bb99fa46`) and `size_t len; char first; size_t i` in `g_unichar_to_utf8` (commit `5d5cebdbf`, 2025-11-25, first in 2.87.1): `first` is or-ed into the lead byte, whose low eight bits are the same; `g_string_insert_unichar` at 2.72.4 and 2.80.0 differs only in the signedness of its position argument | unchanged |
| U3 the length gate | `oxpinyin-capi/src/phrase.rs` `segment_text` and the zhuyin twin | `g_utf8_strlen` (`:235-271`), `g_utf8_next_char` (`gunicode.h`) | identical; the macro lost a `(char *)` cast in 2.81.0 (`c583162cc`), same value | unchanged |
| S merge order | `oxpinyin-engine/src/nbest.rs` `glib_merge_sort` (with `tail_compare`, `c_float_to_gint`); `oxpinyin-engine/src/session/mod.rs` `glib_rank_sort`; `oxpinyin-capi/src/predict.rs` `sort_predicted` | `msort_with_tmp` (`gqsort.c:70-193`), `msort_r` (`:197-282`); `g_array_sort`, `g_array_sort_with_data`, `g_ptr_array_sort` (`garray.c`) | the split at `n / 2`, the `cmp <= 0` left preference and the right remainder left in place are identical at all eight tags. 2.72.4 differs only in the copy type of the pointer-sized case (`unsigned long`, made `guintptr` by `d2bbd69fd`, 2022-12-14, first in 2.75.3) and in alignment macro spelling. `g_sort_array` was added in 2.81.1 (`b32e1b63e`, 2024-06-28; 2.82) and the three `garray.c` entry points call it instead of `g_qsort_with_data`; both call `msort_r` | unchanged |
| J1 `g_strconcat` | `oxpinyin-capi/src/aux_matrix.rs` `AuxMatrix::full`, `double`, `chewing` (plain concatenation; the NULL stop is unreachable, #925) | `g_strconcat` (`gstrfuncs.c:564-599`) | identical, except that from 2.87.0 (`3719ac8db`, 2025-09-17; shipped from 2.88.0) the length sum is overflow-checked and aborts through `g_error` where it wrapped before | unchanged (a wrap needs a combined length above `SIZE_MAX`) |
| J2 `g_build_filename` | `oxpinyin-data/src/table_info.rs` `pin_join`, `join_bytes`, `FileName::under`; `oxpinyin-data/src/pin_stderr.rs` `build_filename`; `oxpinyin-user/src/persistence.rs` `user_file`, `user_conf_path` | `g_build_filename` (`gfileutils.c:2298-2309`), `g_build_filename_va` (`:2204-2217`), `g_build_path_va` (`:1904-2004`); Windows `g_build_pathname_va` (`:2090-2199`) | identical, except that from 2.85.2 (`23b70b0d3`, 2025-07-18; shipped from 2.86.5) the separator length and the element index are `size_t`, closing an out-of-bounds read with more than `INT_MAX` elements | unchanged (the pin passes two elements) |
| T `g_strsplit_set` | `oxpinyin-dictool/src/format.rs` `split_classic`; `oxpinyin-training/src/counter/parse.rs` `parse_segmented_line` | `g_strsplit_set` (`gstrfuncs.c:2470-2533`) | identical at all eight tags | unchanged |
| G `g_unichar_isgraph` | `oxpinyin-data/src/interp_grammar.rs` `is_graph` (tool scope) | `g_unichar_isgraph` (`guniprop.c:185-194`), `g_unichar_isspace` (`:278-300`), `g_unichar_type` (`:724-727`), the tables in `gunichartables.h` | code identical at all eight tags; the data move: Unicode 14 (2.72.4), 15.0 (`65092de98`, first in 2.74.1), 15.1 (`17145fab3`, 2.79.0; 2.80.0), 16.0 (`e2b96c867`, 2.83.0; 2.84.4 and 2.86.5), 17.0 (`4dace3e02`, 2.87.0; 2.88.0 to 2.90.1) | **changed**: a code point assigned after the host's table is `G_UNICODE_UNASSIGNED` there |

Also checked and identical at every tag: `g_strv_contains`
(`gstrfuncs.c:3234-3247`, the punctuation dedup walk of #928).

### U — UTF-8 and UCS-4

The pin converts caller text with `g_utf8_to_ucs4` (`pinyin.cpp:628,1397,
1452,2658,3210,3678` and the zhuyin twins) and stored words with
`g_ucs4_to_utf8` (`pinyin.cpp:1627-1645`, `lookup.cpp:27-60`,
`pinyin.cpp:698-716`); the punctuation reader encodes with
`g_string_append_unichar` (`punct_table.cpp:65`). At every shipped GLib the
decoder accepts the same set (canonical one- to six-byte forms through
`0x7fffffff`, surrogates included; overlong, truncated and `fe`/`ff` forms
rejected) and the encoder writes the same bytes (three- to six-byte
historical forms above U+FFFF, a stop at the first zero word, a refusal at
`0x80000000`). The only textual changes widen loop counters from `gint` to
`size_t` (2.88.0 and later).

Stored words cannot reach the widening: every `g_ucs4_to_utf8` call passes
a stored item's phrase length, which the item keeps in a `guint8`
(`phrase_index.h:100-103`). Caller text can. Each `g_utf8_to_ucs4` call
above passes length `-1` with no limit before the conversion, and the
checks that follow act on its result, such as `MAX_PHRASE_LENGTH` in
`pinyin_remember_user_input` (`pinyin.cpp:3680`). The subject's C entry
points that call `utf8_to_ucs4` (`oxpinyin-capi-marshal/src/input.rs`) or
`prefix_text` (`oxpinyin-capi-marshal/src/ucs4.rs`) add no limit before the
conversion either. For caller text of 2³¹ characters or more, so at least
2³¹ bytes:

- GLib up to 2.86.5 counts characters in a `gint` (2.84.4
  `gutf8.c:945,970`). Counting past `INT_MAX` is signed overflow, which is
  undefined behaviour in C. The compiled result was not read.
- GLib 2.88.0 and later count in `size_t` (2.88.0 `gutf8.c:946`) and
  convert the whole string, or return NULL when `try_malloc_n` cannot
  allocate the result.
- The subject converts the whole string with `usize` counts and has no 2³¹
  boundary. Valid Unicode UTF-8 is copied once as text; other accepted
  input goes through `decode`, which reserves one word per input byte.

So the subject's `decode`, `encode_word` and `PhraseText::from_words`
match every GLib in the range on stored words and on caller text shorter
than 2³¹ characters. The claim excludes two cases this record did not
trace: caller text of 2³¹ characters or more, and a conversion whose
allocation fails. Within those bounds the #894 and #918 records are
version-independent.

### S — the merge sort

`g_ptr_array_sort(tails, trellis_value_compare)` (`phonetic_lookup.h:337`),
`g_array_sort_with_data(compare_item_with_sort_option)`
(`pinyin.cpp:2286,2443`) and `g_array_sort` (`zhuyin.cpp:1529,1610`) all
reach `msort_r` at every tag: through `g_qsort_with_data` up to 2.80, through
`g_sort_array` from 2.82. `msort_with_tmp` makes the same comparisons in the
same order at all eight tags, so the orders the
[921-927 record](921-927-glib-sorts.md) derives for non-transitive tail
scores and for λ outside `[0, 1]` hold on every listed system. The 2.75.3
change to the pointer-sized copy loop and the 2.82 `g_sort_array` wrapper
alter copying and the entry symbol, not a comparison.

### J — `g_strconcat` and `g_build_filename`

`g_strconcat` stops at the first NULL argument and concatenates the rest at
every tag; the 2.87.0 overflow check only replaces an unreachable `size_t`
wrap with an abort. The pin's three auxiliary-text joins
(`pinyin.cpp:3431,3509,3569`) and the export joins (`pinyin.cpp:830,865`)
see the same bytes everywhere.

`g_build_filename(dir, name, NULL)` (`pinyin.cpp:395,400`;
`table_info.cpp:201,332`; `zhuyin.cpp:337,342`) is `g_build_path_va` with
`"/"` on Unix at every tag, and the join law in the
[G3 table](glib-libc-verification.md#g3--path-joining) (trailing
separators of `dir` dropped, leading separators of `name` dropped, one
separator between the bodies, empty parts skipped) is unchanged. The 2.85.2
change widens two counters. `pin_join`, `build_filename` and the user-file
joins therefore produce every listed GLib's path.

### T — `g_strsplit_set`

The two tool splits the subject reproduces, `g_strsplit_set(line, " \t", 2)`
(`utils_helper.h:57`) and ibus-libpinyin's classic three-field split, see
the same function text at every tag: an empty string gives no fields,
adjacent delimiters give empty fields, and the last field keeps the rest of
the line including delimiters. `split_classic` and `parse_segmented_line`
are version-independent.

### G — `g_unichar_isgraph`

`split_line` (`tag_utility.cpp:118-166`) skips `g_unichar_isspace`
characters (`:127`) and ends an unquoted token at the first character that
is not `g_unichar_isgraph` (`:152`); the outer loop then steps over that
character, so a token containing it is cut in two and the record gains a
field. `g_unichar_isgraph` is `TYPE(c)` outside Cc, Cf, Cn, Cs and Zs, and
`TYPE` is a lookup in `gunichartables.h`, so the answer for a code point
depends on the Unicode version of the host's GLib:

| Host GLib | Unicode data | Assigned in later versions and therefore not graph on this host |
| --- | --- | --- |
| 2.72.4 (jammy) | 14 | everything below plus Unicode 15.0's 4,489 characters, among them CJK Unified Ideographs Extension H (U+31350 to U+323AF) |
| 2.80.0 (noble) | 15.1.0 | everything below; it has Extension I (U+2EBF0 to U+2EE5D, Unicode 15.1) |
| 2.84.4 (trixie, the reference image), 2.86.5 (Fedora 43) | 16.0.0 | Unicode 17.0's 4,803 characters, among them CJK Unified Ideographs Extension J (U+323B0 to U+3347F) |
| 2.88.0, 2.88.4, 2.90.0, 2.90.1 (resolute, forky, stonking, Fedora 44 and 45, Arch, Homebrew) | 17.0.0 | none at the time of writing |

Unicode 16.0 adds no CJK unified ideographs (its 5,185 characters are
other scripts, Egyptian hieroglyphs, legacy-computing symbols and emoji),
so for CJK text the four table generations collapse to three: jammy, then
noble to Fedora 43, then the Unicode 17 hosts. A real libpinyin's
`import_interpolation`, `import_k_mixture_model` or
`k_mixture_model_to_interpolation` (the three callers of `taglib_read`)
on the reference image cuts an unquoted token at an Extension J
ideograph; on Arch it keeps the token whole; on jammy it also cuts at
Extension H and I.
The reference model's export contains only long-assigned characters
([interpolation2-grammar.md](interpolation2-grammar.md)), so no shipped
input reaches the difference.

The subject's `is_graph` is the whitespace-and-control test recorded in
that document. It keeps every assigned printable character of every table
generation, and it also keeps Cf characters and unassigned code points,
which every GLib excludes. So it differs from every GLib, the newest
included, on assigned Cf characters such as U+00AD SOFT HYPHEN, U+200B ZERO
WIDTH SPACE and U+FEFF ZERO WIDTH NO-BREAK SPACE, and on code points still
unassigned in Unicode 17. That is the bounded divergence recorded in
[interpolation2-grammar.md](interpolation2-grammar.md) and in #926
([926-tool-tokenizers.md](926-tool-tokenizers.md)). #926 also records that
GLib counts U+2028 and U+2029 as graph inside a token; the subject's
`is_graph` does not, because Rust's `char::is_whitespace` includes both.
Against an older GLib the subject differs in addition on the characters
assigned after that GLib's table. Nothing in the shipped library reaches
`g_unichar_isgraph`: the function's only callers are the tool-only
`tag_utility.cpp` (inventory row G16).

## glibc

The pin reads `table.conf` with `fscanf` directives `%d`, `%f` and `%255s`
(`table_info.cpp:208,214,220,228,232,255`) and `user.conf` with `%d` and
`%255s` (`:338,344,352,357`); an addon index goes through `atoi`
(`:160`). The subject's counterparts are `oxpinyin-data/src/table_info.rs`
`Scan` (`literal`, `int`, `float`, `exponent`, `word`), `strtol_digits`,
`atoi_guint8`, `c_float`, `hex_float` and `round_to_f32`,
`oxpinyin-data/src/user_files.rs` `Scan::int`, and in the tools
`oxpinyin-kmm/src/cnum.rs` `atoi` and `atol`. The recorded reference for
them is the G11 walk of glibc 2.41 and 2.43-3 in
[glib-libc-verification.md](glib-libc-verification.md#g11--numeric-parsing-and-locale).

| Change | First release (commit) | What changed | Hosts on the old side | Through the pin |
| --- | --- | --- | --- | --- |
| N1 `nan(n-char-sequence)` | 2.41 (`04e8698fcc`, 2024-10-25, BZ #30647) | up to 2.39 the `%f` lexer converts as soon as it has read `nan` (2.39 `vfscanf-internal.c:2031-2032`), leaving `(…)` in the stream; from 2.41 it consumes `(`, `[A-Za-z0-9_]*` and `)`, and fails the conversion on any other byte or EOF (2.41 `:2031-2077`) | jammy, noble | `lambda parameter:nan(1)`: old side assigns NaN, then `source table format:` meets `(`, converts nothing and `to_table_phonetic_type` reads the uninitialised `str` (the class (b) path of a missing source line); new side loads with NaN (policy row 83). The subject follows the new side |
| N2 `strtof` subnormal rounding | 2.41 (`457622c2fa`, 2024-08-27, bug 30220; `strtod_l.c:225,296`) | a value whose correctly rounded `float` is subnormal, and whose dropped bits are exactly the least significant kept bit and possibly the half bit, rounded one unit low | jammy, noble | λ below 2⁻¹²⁶ enters `m_freq` as `λ·bigram·2²⁴` (`pinyin.cpp:1810-1866`), below the integer truncation, so no frequency, order or cost changes. The subject rounds to nearest even |
| N3 exponent mark without digits | 2.42 (`0a8e7ac95c`, 2025-03-28, BZ #12701): `got_digit = 0` at the mark (2.42 `:2207`) and the `got_e && !got_digit` failure (`:2563`) | up to 2.41 `1e`, `1e+`, `1e-`, `0x1p`, `0x1p+` and `0x1p-` convert (`:2191`, `:2536-2539`; the buffer is `1e`, `strtof` stops at `e`, the mark is consumed); from 2.42 the conversion fails | jammy, noble, trixie | old side loads λ = 1.0; new side returns `false` from `SystemTableInfo2::load`, printing `load … failed!`. The subject refuses; registered `host_scanf`, HOST-DEPENDENT on the reference cell |
| N4 significand without digits | 2.43 (`b52ecff316`, 2025-08-11, BZ #12701): the check becomes `!got_digit` (2.43 `:2576`) | up to 2.42 `0x.` reaches `strtof("0x.")`, which consumes `0` and returns 0 (2.41 `strtod_l.c:690-700,732-768,1044-1048`); from 2.43 the conversion fails | jammy, noble, trixie, Fedora 43 | old side loads λ = 0; new side refuses. The subject refuses; registered `host_scanf=('0x.',)` (ruling 2026-10-25) |
| N5 push-back on a failed `nan` or `inf` | 2.44 (`21103aef12`, 2026-06-25, BZ #34208; `:2048-2195`) | the byte that breaks `nan`, `inf` or `infinity` is `ungetc`'d before the conversion fails; before, it was consumed | everything up to 2.43 | not observable: a failed `%f` returns from `load` (`table_info.cpp:220-224`) and nothing reads the stream again |
| N6 the C23 entry points | 2.38 (`dee2bea048`, `64924422a9`) | `fscanf` binds `__isoc23_fscanf` in a translation unit with C23 enabled (2.41 `libio/stdio.h:442-444`; `include/features.h:262-267`, and `_GNU_SOURCE`, which g++ defines, sets `_ISOC23_SOURCE` at `:219-220`), which passes `SCANF_ISOC23_BIN_CST` (`isoc23_fscanf.c:30-31`); `strtol` binds `__isoc23_strtol` the same way. The flag admits `0b` for `%i` and base 0 only (`vfscanf-internal.c:1483-1493`; `strtol_l.c:327-331`) | 2.35 binds `__isoc99_fscanf` (`libio/stdio.h:434-436`) | not observable: `%d` is base 10, `%f` and `%255s` ignore the flag, `atoi` is base 10 |
| N7 `%d`, `%255s`, `strtol` | 2.42 (`e78cb4f6bd`), 2.43 (`b692181703`, `2b16c76609`), 2.44 (`8398987772`, `8ed4c70915`), master (`6467136459`, `b2d944913a`) | the 2.42 integer-prefix change only moves where the leading `0` is buffered; for base 10 the buffer is `0` and the `x` is pushed back as before (2.42 `:1463-1507`). 2.43's `inchar_in_field` guards act on explicit widths and on the `I` flag's locale digits; 2.43's `%c` input error and the 2.44 and master allocation changes act on `%c` and `%m`. The `%s` conversion has no other hunk between 2.35 and master. `strtol_l.c` changes are the C23 `0b` path (base 0 and 2) and a macro rename | none | unchanged |

The 2.35 to 2.39 changes (`%b`, `%wN`, binary prefixes for `%i`, heap
buffers for locale digits, 2.38 and 2.39) and the 2.42 to 2.43 `strtod_l.c`
changes (`stdc_leading_zeros` for `count_leading_zeros`, alias plumbing)
have no path to the pin's directives. The λ value the hosts agree on is the
same value: where N3 or N4 loads, every old-side host loads 1.0 or 0, so the
host decides only whether `pinyin_init` succeeds, not which λ the engine
ranks with.

### macOS (Homebrew)

The pin on macOS links Apple's libSystem, not glibc. Its `%f` conversion
(`Libc-1752.120.2`, `stdio/FreeBSD/vfscanf.c`, `parsefloat`,
lines 1025-1217) is a commit-and-push-back state machine: it reads ahead,
then pushes the input back to the last position that formed a complete
number (`parsedone`, lines 1209-1214). So `1e` converts to 1 with `e`
pushed back, `0x.` to 0 with `x.` pushed back, `infin` to `inf` with `in`
pushed back, and an unclosed `nan(` to `nan` with `(` pushed back. In the
pin the pushed-back bytes then meet the literal of the next directive,
which fails without a conversion, and `to_table_phonetic_type` reads the
uninitialised `str`: the class (b) path. On glibc the same spellings either
load (old side) or fail cleanly (new side). Apple's `%d` and `%s`
conversions were not traced, and the Windows C runtime was not read.

## Classification under #1303's platform clause

The proposed clause reads: native hosts give native results where the
pin's result is host- or libc-dependent, recorded as a platform-dependent
difference; platform-independent logic must not differ between hosts.

| Change | Reaches | Classification |
| --- | --- | --- |
| U1 (the `size_t` counters of `g_utf8_to_ucs4`, 2.88.0) | caller text of 2³¹ characters or more, through every C entry point that converts caller text | not classified: on GLib up to 2.86.5 the pin's result is undefined behaviour, which this record does not derive |
| U2, U3, S, J1, J2, T (`size_t` counters, overflow checks, the copy type, the `g_sort_array` wrapper) | nothing the pin's inputs can reach | unchanged; no classification needed |
| G (Unicode 15.0, 15.1, 16.0, 17.0 tables) | the tool-only `split_line`; never the shipped library | host-dependent, record (tool scope; the deliberate `is_graph` divergence in [interpolation2-grammar.md](interpolation2-grammar.md) now has a version dimension: "unassigned" is per host) |
| N1 (2.41) | whether a malformed `nan(…)` λ loads or reaches the uninitialised `str` | host-dependent, record. No `host_scanf` entry covers a glibc 2.39 host: a cell built natively on noble, the cloud host, would report `table-conf-919-lambda-nan-payload` and `nan-unclosed` as DIFFER (pin on the class (b) path, subject loading or refusing) |
| N2 (2.41) | one subnormal unit of a malformed λ, below the frequency quantum | host-dependent, record; unobservable |
| N3 (2.42) | whether a malformed λ such as `1e` loads; it loads on jammy, noble and trixie | host-dependent, record; already registered as `host_scanf` |
| N4 (2.43) | whether `0x.` loads; it loads on jammy, noble, trixie and Fedora 43 | host-dependent, record; already registered as `host_scanf=('0x.',)` (ruling 2026-10-25). The marker's check calls the running host's `sscanf` (`tools/bisection/contract-diff.py`, `host_scanf_accepts`), so it covers a Fedora 43 host as well as the reference image |
| N5 (2.44), N6 (2.38), N7 | nothing observable through the pin | unchanged |

Among the classified changes, nothing reaches platform-independent logic
with a host-dependent value: where the hosts differ, they differ on whether
`pinyin_init` succeeds or on tool tokenization, and the λ the engine ranks
with is the same on every host that loads the file. No classified change
forces a decision on a specific behaviour. The U1 row is outside both
statements until the older GLibs' compiled result is read.

The clause's final text settles how a behaviour oxpinyin reimplements is
read. #1304 (`docs/findings/compatibility-policy.md`, "Native Rust
implementation", maintainer ruling 2026-10-26 UTC, at `ab0b1da7`) says
that where the pin's result depends on the host or its libc, oxpinyin
"uses Rust or the host's native facilities and need not reproduce the
reference image's libc", with the difference recorded as
platform-dependent, and that platform-independent logic must not differ.
One Rust-modelled grammar on every host is therefore the policy's reading,
not a departure from it. The subject's scanner is fixed at 2.43's grammar
(the N3 and N4 rules, and N1's payload rule); on trixie, noble and jammy,
and on Fedora 43 for the N4 rule, that is not the host libc's result, and
the `host_scanf` markers, `0x.`'s by the ruling of 2026-10-25, record the
reference-cell difference as HOST-DEPENDENT. The modelled version is named
here so the G11 records can carry it when they are next touched.

## Correction to earlier records

[glib-libc-inventory.md](glib-libc-inventory.md) (G11, "trixie reference")
and [glib-libc-verification.md](glib-libc-verification.md) (G11, the `%f`
rows) attribute both the exponent rule and the significand rule to glibc
2.43-3, because they compared 2.41 against Debian's 2.43-3. The exponent
rule (`got_digit = 0` at the mark and the `got_e && !got_digit` failure) is
commit `0a8e7ac95c`, first released in **2.42**; only the significand rule
(`!got_digit` alone, the `0x.` case) is `b52ecff316` in 2.43. The cited
2.43-3 line numbers (`vfscanf-internal.c:2218-2219`, `:2571-2576`) are
correct for that tree; only the release attribution changes. The practical
consequence is Fedora 43 (glibc 2.42): its libpinyin refuses `1e` and
loads `0x.`. The two records are not edited by this change; the amendment
is owed when G11 is next touched.

## Limits

- Distribution patches to GLib and glibc were not read; the comparison is
  between upstream release tags. The reference image's GLib and glibc
  revisions have been read before as Debian source
  ([glib-libc-verification.md](glib-libc-verification.md)).
- The Unicode table comparison is by data version and the Unicode release
  notes' block lists; no per-code-point enumeration was run.
- Apple's libc was read for one function; Windows was not read.
- Caller text of 2³¹ characters or more was not traced past GLib's
  counter: the compiled result of the `gint` overflow in GLib up to 2.86.5
  was not read, nor was the pin's handling of a NULL conversion followed
  through each entry point. Allocation failure is not covered on either
  side.
- Fedora 45's versions are its branched repository's on 2026-10-26; the
  release was still pending.
- The pin is 074a2219 on every host considered; the libpinyin versions the
  distributions package are out of scope.

## Reproduction

The commands below repeat every comparison in Method: each GLib function in
the table and `g_strv_contains` at the eight tags, the `gutf8.c` macro block
through `utf8_skip_data`, `g_utf8_next_char`, the Unicode data version, the
seven glibc diff pairs, the glibc logs, the Arch commit check, and the date
and first release of every named commit. They end with the two sources that
were read but not compared, the pin and Apple's scanner. glibc is cloned
from sourceware.org, which holds every tag read here; the bminor mirror
stops before 2.43. The two branch commits are fetched by full hash and
asserted, and every comparison names them by hash. Save the block as a file
and run it with `bash` from an empty directory; it only reads.

```bash
set -eu
git clone -q --filter=blob:none --no-checkout https://github.com/GNOME/glib.git
git clone -q --filter=blob:none --no-checkout --single-branch --branch master \
  https://sourceware.org/git/glibc.git
ARCH=1848099f063e99d4ffecbd7667766d54862398b9    # release/2.44/master, the commit Arch builds
MASTER=d259884eebd9c09a6c45eb7d5b2ada3970242bbe  # master on 2026-10-24
git -C glibc fetch -q --filter=blob:none https://sourceware.org/git/glibc.git "$ARCH" "$MASTER"
for c in "$ARCH" "$MASTER"; do test "$(git -C glibc rev-parse --verify "${c}^{commit}")" = "$c"; done

# GLib, at each tag against 2.84.4: every function in the table and g_strv_contains,
# from the definition line to the closing brace at column 0; the gutf8.c block from
# UTF8_COMPUTE through the closing brace of utf8_skip_data; g_utf8_next_char; and
# the Unicode data version.
fn() { git -C glib show "${2}:glib/${1}" | awk -v n="$3" \
  'BEGIN { s = "^" n " \\(" } { if (!p && $0 ~ s) p = 1; if (p) print; if (p && $0 == "}") exit }'; }
blk() { git -C glib show "${1}:glib/gutf8.c" | awk '/^#define UTF8_COMPUTE/ { p = 1 } p { print } p && /^};/ { exit }'; }
nxt() { git -C glib show "${1}:glib/gunicode.h" | grep '^#define g_utf8_next_char'; }
FNS='gutf8.c:g_utf8_get_char_extended gutf8.c:g_utf8_to_ucs4 gutf8.c:g_utf8_get_char
  gutf8.c:g_ucs4_to_utf8 gutf8.c:g_unichar_to_utf8 gutf8.c:g_utf8_strlen
  gstring.c:g_string_insert_unichar gqsort.c:msort_with_tmp gqsort.c:msort_r
  garray.c:g_array_sort garray.c:g_array_sort_with_data garray.c:g_ptr_array_sort
  gstrfuncs.c:g_strconcat gstrfuncs.c:g_strsplit_set gstrfuncs.c:g_strv_contains
  gfileutils.c:g_build_filename gfileutils.c:g_build_filename_va gfileutils.c:g_build_path_va
  gfileutils.c:g_build_pathname_va guniprop.c:g_unichar_isgraph guniprop.c:g_unichar_isspace
  guniprop.c:g_unichar_type'
for t in 2.72.4 2.80.0 2.84.4 2.86.5 2.88.0 2.88.4 2.90.0 2.90.1; do
  for f in $FNS; do
    a=$(fn "${f%%:*}" 2.84.4 "${f##*:}"); b=$(fn "${f%%:*}" "$t" "${f##*:}")
    if [ -z "$a" ] || [ -z "$b" ]; then echo "$t ${f##*:} not found"
    elif [ "$a" != "$b" ]; then echo "$t ${f##*:} differs"; fi
  done
  [ "$(blk 2.84.4)" = "$(blk "$t")" ] || echo "$t gutf8.c macro block or utf8_skip_data differs"
  [ "$(nxt 2.84.4)" = "$(nxt "$t")" ] || echo "$t g_utf8_next_char differs"
  echo "$t $(git -C glib show "${t}:glib/gunichartables.h" | grep G_UNICODE_DATA_VERSION)"
done
# The GLib commits named in the record: author date and first tag.
for c in d2bbd69fd c583162cc b32e1b63e 23b70b0d3 3719ac8db 5d5cebdbf 0bb99fa46 \
         65092de98 17145fab3 e2b96c867 4dace3e02; do
  echo "$c $(git -C glib show -s --format=%as "$c") $(git -C glib tag --contains "$c" | sort -V | head -1)"
done

# glibc: the scanner diffs between consecutive shipped versions, read in full.
for p in glibc-2.35:glibc-2.39 glibc-2.39:glibc-2.41 glibc-2.41:glibc-2.42 \
         glibc-2.42:glibc-2.43 glibc-2.43:glibc-2.44 "glibc-2.44:${ARCH}" "glibc-2.44:${MASTER}"; do
  echo "== ${p}"
  git -C glibc diff "${p%%:*}" "${p##*:}" -- stdio-common/vfscanf-internal.c \
    stdlib/strtod_l.c stdlib/strtol_l.c
done
# The integer and ctype sources over the same range.
git -C glibc log --oneline "glibc-2.35..${MASTER}" -- stdlib/strtol.c stdlib/atoi.c locale/C-ctype.c
git -C glibc log --oneline "glibc-2.44..${ARCH}" -- stdlib/strtol.c stdlib/atoi.c locale/C-ctype.c
# The Arch commit: 50 commits after glibc-2.44, none touching a file read here.
git -C glibc rev-list --count "glibc-2.44..${ARCH}"
git -C glibc log --oneline "glibc-2.44..${ARCH}" -- stdio-common/vfscanf-internal.c \
  stdio-common/isoc23_fscanf.c stdlib/strtod_l.c stdlib/strtol_l.c stdlib/strtol.c \
  stdlib/atoi.c locale/C-ctype.c libio/stdio.h include/features.h
# The glibc commits named in the record: author date and first release.
for c in 04e8698fcc 457622c2fa 0a8e7ac95c b52ecff316 21103aef12 dee2bea048 64924422a9 \
         e78cb4f6bd b692181703 2b16c76609 8398987772 8ed4c70915 6467136459 b2d944913a; do
  echo "$c $(git -C glibc show -s --format=%as "$c") $(git -C glibc tag --contains "$c" | grep -E '^glibc-2\.[0-9]+$' | sort -V | head -1)"
done

# Read, not compared: the pin and Apple's scanner.
git init -q libpinyin
git -C libpinyin fetch -q --depth 1 https://github.com/libpinyin/libpinyin.git \
  074a2219c90feaf962d0d24f034514033ece5f99
git -C libpinyin show FETCH_HEAD:src/pinyin.cpp | sed -n 3678,3681p
curl -fsSL https://raw.githubusercontent.com/apple-oss-distributions/Libc/Libc-1752.120.2/stdio/FreeBSD/vfscanf.c \
  | sed -n 1209,1214p
```

In one run on 2026-10-26, the GLib loop printed no `not found` line and
exactly these differences from 2.84.4, which are the changes in the GLib
table:

| Tags | Differs from 2.84.4 |
| --- | --- |
| 2.72.4 | `g_string_insert_unichar`, `msort_with_tmp`, `msort_r`, `g_array_sort`, `g_array_sort_with_data`, `g_ptr_array_sort`, `g_utf8_next_char` |
| 2.80.0 | `g_string_insert_unichar`, `g_array_sort`, `g_array_sort_with_data`, `g_ptr_array_sort`, `g_utf8_next_char` |
| 2.86.5 | `g_build_path_va`, `g_build_pathname_va` |
| 2.88.0, 2.88.4, 2.90.0, 2.90.1 | `g_utf8_to_ucs4`, `g_ucs4_to_utf8`, `g_unichar_to_utf8`, `g_strconcat`, `g_build_path_va`, `g_build_pathname_va` |

The data versions read `14` at 2.72.4, `15.1.0` at 2.80.0, `16.0.0` at
2.84.4 and 2.86.5, and `17.0.0` from 2.88.0 on. The commit loops printed the
dates and first releases cited above, with no release for the two
master-only commits. The pair from `glibc-2.44` to `1848099f` printed no
diff, the Arch count was 50, and the Arch file log was empty. The first
glibc log listed nine commits: four copyright updates, a clang weak-alias
fix, the C2X-to-C23 rename, `64924422a9` (N6), and two commits that change
only the C locale's cached-data pointer; the second printed nothing.

Package versions: `https://tracker.debian.org/pkg/glib2.0`,
`https://tracker.debian.org/pkg/glibc`,
`https://launchpad.net/ubuntu/+source/glib2.0`,
`https://launchpad.net/ubuntu/+source/glibc`,
`https://packages.fedoraproject.org/pkgs/glib2/glib2/`,
`https://packages.fedoraproject.org/pkgs/glibc/glibc/`,
`https://archlinux.org/packages/core/x86_64/glib2/`,
`https://archlinux.org/packages/core/x86_64/glibc/`,
`https://formulae.brew.sh/api/formula/glib.json`. Apple's scanner:
`https://github.com/apple-oss-distributions/Libc/blob/Libc-1752.120.2/stdio/FreeBSD/vfscanf.c`.
