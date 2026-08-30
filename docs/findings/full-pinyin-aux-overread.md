# Findings — upstream heap over-read in `pinyin_get_full_pinyin_auxiliary_text`

Date: 2026-08-20 · Source tier: post-W15 verification observation.
Status: recorded (upstream defect; oxpinyin unaffected; **still present on
libpinyin `main` @ `55e9051`, no existing report found in surveyed venues**).

## Summary

`pinyin_get_full_pinyin_auxiliary_text`'s mid-key cursor branch
(`src/pinyin.cpp:3414-3419`) computes `len = cursor - begin`, where both
`cursor` and `begin` are **raw-input** byte offsets, then passes `len` to
`g_strdup(pinyin + len)` where `pinyin` is the **canonical pinyin
spelling**. The mid-key branch only fires while `cursor < end`, so the
largest `len` it can produce is `raw_len - 1`; `len > strlen(pinyin)` is
therefore reachable only for a raw key at least **two** bytes longer than
its canonical spelling (`raw_len ≥ canonical_len + 2`, the `raw ≥
canonical + 2` scope used throughout below). Both `SECONDARY_ZHUYIN` and
`LUOMA` have such keys (reproduced below), as does any scheme whose
raw-key length can exceed the pinyin byte length by two or more. For
those keys `len` can exceed `strlen(pinyin)`. Computing `pinyin + len`
is then already undefined behaviour — the result is past the permitted
one-past-the-end pointer — so any outcome, including a fault, is
allowed. What the recorded runs observed is `g_strdup` reading past the
canonical string's `NUL` until the next zero byte in adjacent heap: the
bytes it copies are non-deterministic heap contents — an observed,
user-visible heap information leak into the aux text returned to the
frontend.

## Reproduction

Reproduced on the pinned oracle (libpinyin `2.11.91` @
`0c5e80e1200f84fab185d1c5bde458b770a0636c`) with a throwaway `dlopen`
driver (`/tmp/test-fullpin-aux.c`) that selects `SECONDARY_ZHUYIN`
(`pinyin_set_full_pinyin_scheme(ctx, 3)`) and dumps
`pinyin_get_full_pinyin_auxiliary_text` at every cursor with
non-printable bytes escaped as `\xNN`.

```sh
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o /tmp/test-fullpin-aux \
    /tmp/test-fullpin-aux.c -ldl
TEST_USER_DIR=$(mktemp -d) /tmp/test-fullpin-aux \
    ~/.local/opt/pinyin-oracle/lib/libpinyin.so \
    ~/.local/opt/pinyin-oracle/lib/libpinyin/data
```

Oracle output, one run. The over-read fires at the **mid-key cursor
immediately before the parsed-key boundary** — `aux(4)` for `tzuei`
(boundary `aux(5)`), `aux(5)` for `tzuei4` (boundary `aux(6)`),
`aux(4)` for `tzueiQ` (boundary `aux(5)`); the boundary rows
themselves are clean:

```text
=== input: tzuei (consumed=5) ===
  aux(3): true text="zui| "
  aux(4): true text="zui|<V "   <-- over-read
  aux(5): true text="zui |"

=== input: tzuei4 (consumed=6) ===
  aux(4): true text="zui4| "
  aux(5): true text="zui4|V "   <-- over-read
  aux(6): true text="zui4 |"

=== input: tzueiQ (consumed=5) ===
  aux(3): true text="zui| "
  aux(4): true text="zui|<V "   <-- over-read
  aux(5): true text="zui |"
```

Three consecutive runs, showing the leaked bytes are non-deterministic:

```text
run 1: aux(4): true text="zui|\x9cU "
run 2: aux(4): true text="zui|\xfbU "
run 3: aux(4): true text="zui|\xe6U "
```

None of the three can begin a well-formed UTF-8 sequence here:
`\x9c` is a continuation byte (only valid inside a multi-byte
sequence, invalid as the first byte), `\xfb` is not a valid UTF-8
lead byte at all, and `\xe6` is a valid three-byte lead whose
sequence is completed (or broken) by whatever heap bytes follow. Other
runs land on printable ASCII (`<V`, `sU`, `5V`) that still constitutes
garbage bytes appearing in the aux text. The specific bytes depend on
the process's heap state at the moment of the call.

The same probe under `LUOMA` (`pinyin_set_full_pinyin_scheme(ctx, 2)`)
confirms that scheme is affected in practice, not just implicated by
construction. Its index has eighteen rows whose raw spelling is at
least two bytes longer than the canonical pinyin; sweeping all of
them, eight leaked visibly in three consecutive runs —
`chih`/`jhih`/`shih` → `ch`/`zh`/`sh` and `chyu` → `qu` at cursor 3,
`rih`/`sih`/`zih` → `r`/`s`/`z` at cursor 2, `tsih` → `c` at cursors
2 and 3:

```text
=== input: jhih (consumed=4) ===
  aux(2): true text="zh| "
  aux(3): true text="zh|6 "   <-- over-read
  aux(4): true text="zh |"
```

`rih`'s cursor-2 leak varied across the three runs (`\xc26`, `w\x1a`,
`\x0f$`). The remaining ten candidate rows printed clean text at their
over-read cursors in these runs — when the byte after the canonical
string's `NUL` happens to be zero the copied suffix is empty and the
leak is invisible, but the out-of-bounds read is the same.

## The bug

Cite: `src/pinyin.cpp:3411-3424` at pin `0c5e80e`:

```cpp
/* at the middle of pinyin key */
const size_t begin = key_rest.m_raw_begin;
const size_t end = key_rest.m_raw_end;
const size_t len = cursor - begin;
if (begin < cursor && cursor < end) {
    gchar * pinyin = key.get_pinyin_string();
    gchar * left = g_strndup(pinyin, len);
    gchar * right = g_strdup(pinyin + len);
    middle = g_strconcat(left, "|", right, " ", NULL);
    g_free(left);
    g_free(right);
    g_free(pinyin);
    break;
}
```

`begin`/`end`/`cursor` are raw-input positions
(`key_rest.m_raw_begin`/`m_raw_end`), so `len = cursor - begin` is the
raw-key delta. But `pinyin` is the canonical pinyin string
(`key.get_pinyin_string()`), whose byte length depends on the scheme, not
on raw keystrokes. `g_strndup(pinyin, len)` clamps at `len` bytes
(safe even when `len > strlen(pinyin)`): by its API contract it copies
at most `n` source bytes, always NUL-terminates, and NUL-pads when the
source is shorter — so the source read is bounded by the contract, not
by an implementation detail. But `g_strdup(pinyin + len)` does **not**
clamp — it reads from
`pinyin + len` until the next zero byte. When `len > strlen(pinyin)`,
that pointer is past the canonical string's terminator and the read runs
into whatever heap sits after.

For `SECONDARY_ZHUYIN`, `tzuei` (5 raw bytes) canonicalises to `zui`
(3 bytes). At `cursor = 4` (before the last raw byte `i`), `len = 4`,
but `strlen("zui") = 3` — `pinyin + 4` reads past the terminator.

The fix this doc proposes upstream (verified byte-identical to
oxpinyin's output at the over-read positions, see the local
verification below) repairs the length **invariant** rather than only
the failing pointer add:

```diff
--- a/src/pinyin.cpp
+++ b/src/pinyin.cpp
@@ -3415,9 +3415,11 @@ bool pinyin_get_full_pinyin_auxiliary_text(pinyin_instance_t * instance,
         if (begin < cursor && cursor < end) {
             gchar * pinyin = key.get_pinyin_string();
-            gchar * left = g_strndup(pinyin, len);
-            gchar * right = g_strdup(pinyin + len);
+            const size_t pinyin_len = strlen(pinyin);
+            const size_t clamped    = len < pinyin_len ? len : pinyin_len;
+            gchar * left  = g_strndup(pinyin, clamped);
+            gchar * right = g_strdup(pinyin + clamped);
             middle = g_strconcat(left, "|", right, " ", NULL);
             g_free(left);
             g_free(right);
```

`len` is a **raw-input** offset (`cursor - begin`) being used as a
**string** length against the canonical pinyin; `clamped` restores
that invariant for both halves of the split. The un-clamped
`g_strndup(pinyin, len)` on the `left` side is *not* correct by
construction — it stays in bounds only because `g_strndup`'s API
contract bounds the source read (at most `n` bytes, stopping at the
source `NUL`; shorter sources are NUL-padded). That bound is a
property of choosing `g_strndup`, not of the code around it: swap the
copy for anything that honours the requested byte count literally
(`memcpy` or a hand loop — neither promises to stop at the source
`NUL`) and the over-read silently revives. A one-line right-side-only clamp
(`g_strdup(pinyin + std_lite::min((size_t)len, strlen(pinyin)))`)
produces byte-identical output on every input — the two candidates
diverge on nothing (`clamped == len` whenever `len < strlen`) — but it
leaves `left` on the unclamped raw offset and keeps the NUL-stopping
coupling, so it is the weaker form to hand a reviewer.

The variant actually applied to our pin is that minimal-diff one-liner
(placed at the `g_strdup` call, since `pinyin` is declared inside the
enclosing `if` block and a smaller hunk is what we want carrying the
oracle): `tools/oracle/patches/fullpin-aux-overread.patch`, landed on
main with the `full-pinyin-schemes` branch (PR #126). Refer to that
file for the applied form; the three-line diff above is the
upstream-submittable one.

## Upstream status

- **Pin `0c5e80e`** — over-read present, reproduced above.
- **libpinyin `main` @ `55e9051`** — the same code is unchanged; the fix
  is **not upstream yet** (verified by reading
  `src/pinyin.cpp:3411-3424` at that tip; the `len`/`right` computation
  is byte-identical to the pin).
- **Impact through the shipped frontend: latent.** ibus-libpinyin
  1.16.5 does reach the function —
  `FullPinyinEditor::updateAuxiliaryText` calls
  `pinyin_get_full_pinyin_auxiliary_text` (`src/PYPFullPinyinEditor.cc:128`)
  with the editor cursor — but the context never leaves the HANYU
  default. **Search scope (reproducible negative):** the ibus-libpinyin
  source tree at tag `1.16.5-22-g612004e` contains no direct call
  site of `pinyin_set_full_pinyin_scheme` —
  `grep -rn "pinyin_set_full_pinyin_scheme" src/` over that tree
  returns nothing (the tag's tarball and the clone at that commit
  agree). Under HANYU, raw and canonical lengths are equal, so `len`
  can never exceed `strlen(pinyin)` and the mid-key branch cannot
  over-read. **Scope of the claim:** latent *through ibus-libpinyin
  1.16.5 as shipped* — not unreachable in general. Selecting the
  scheme and parsing only prepare the instance; the over-read fires
  when `pinyin_get_full_pinyin_auxiliary_text` is then asked for a
  mid-key cursor whose raw delta `cursor - begin` exceeds the
  canonical pinyin length. Any consumer that calls
  `pinyin_set_full_pinyin_scheme` with a romanised scheme
  (SECONDARY_ZHUYIN or LUOMA), or drives the raw `pinyin_parse_*`
  single-syllable paths under such a scheme, can reach the over-read
  under those conditions; the W15 differential here is exactly such a
  consumer. **Venue:** the
  bug is in libpinyin (report candidate — Peng Wu); ibus-libpinyin is
  a consumer that happens not to trigger it as shipped and inherits
  the fix.
- **Fix verified locally** — built a patched pinned oracle at
  `$HOME/.local/opt/pinyin-oracle-patched` from the same
  `2.11.91`/`0c5e80e` source archive with the diff above applied, and
  swept it against the unpatched pin: byte-identical on full 1
  (HANYU) and full 2 (LUOMA); differs on full 3 (SECONDARY_ZHUYIN)
  only at the three known over-read positions
  (`tzuei` cursor 4, `tzuei4` cursor 5, `tzueiQ` cursor 4). At those
  three positions the patched oracle emits `zui|` + space, `zui4|` +
  space, and `zui|` + space — byte-identical to oxpinyin. No other
  output differences were observed in this sweep. The sweep's strided
  corpus happens to contain no LUOMA over-read candidate row, which
  is why full 2 shows no difference there; the direct LUOMA probe
  (Reproduction above) closes that gap — at every leaking cursor the
  patched pin emits the clamped empty right suffix (`zh|` + space,
  `r|` + space, …) and is byte-identical to the unpatched pin
  everywhere else.
- **Existing reports** — searched the venues below on 2026-08-20; **no
  existing report** matches this bug:
  - libpinyin GitHub issues: no hit for the function name, "auxiliary
    text", "over-read"/"overflow", or "secondary zhuyin" (surveyed via
    the GitHub search API, all matches unrelated).
  - ibus-libpinyin GitHub issues: no hit for the function name or those
    terms. The nearest match on tone/parse territory is
    [#570](https://github.com/libpinyin/ibus-libpinyin/issues/570) —
    unrelated (assert in `_check_offset`, cross-indexed in
    `docs/testing/oracle-bisect-differential-abort.md`).
  - Fedora Bugzilla (`bugzilla.redhat.com`, components `libpinyin` and
    `ibus-libpinyin`): no hit. Existing crashes filed against those
    components are the ABRT ones already catalogued in
    `reference/memory-safety-bugs.md` §1 (`pinyin_mask_out`,
    `contains_incomplete_pinyin`, `SubPhraseIndex::load`); none touches
    the auxiliary-text formatter.
  - Broader web index: nothing on the function name plus the
    over-read/overflow class.

Recording it here as a fresh upstream memory-safety bug (heap
information leak into user-visible aux text) — an upstream-report
candidate **with the fix attached** (the three-line diff above).

## Scope

- **oracle-side only.** The over-read fires inside pin-built
  `libpinyin.so`; nothing in the oxpinyin tree runs on that stack.
- **Not a CI gate.** The canonical CI form of the parity harness routes
  through `pinyin-oracle` under `ORACLE_LOCK`, which does not exercise
  `pinyin_get_full_pinyin_auxiliary_text` on `SECONDARY_ZHUYIN` or
  `LUOMA` inputs.
- **Parity coverage is unaffected.** oxpinyin's own aux text is
  well-formed at these positions by construction (see below); the
  PARSE_AUX differential surfaced this precisely because oxpinyin's
  output is the clamped, de-UB'd version.

## oxpinyin

oxpinyin's full-pinyin aux formatter routes through
`crates/oxpinyin-core`'s `SyllableKey`/canonical-spelling machinery and
Rust slice bounds. There is no equivalent of `g_strdup(pinyin + len)`:
constructing a `&str` past its length panics with a bounds error, so the
"read until next zero byte" pattern is not expressible in safe Rust.
Instead the aux formatter's mid-key branch produces an empty right
suffix, which is well-formed UTF-8 and matches the intent of the
canonical spelling.

Reproduced against `oxpinyin-capi` built from the pending
`full-pinyin-schemes` branch (which exposes
`pinyin_set_full_pinyin_scheme`), same inputs:

```text
=== input: tzuei (consumed=5) ===
  aux(3): true text="zui| "
  aux(4): true text="zui| "
  aux(5): true text="zui |"

=== input: tzuei4 (consumed=6) ===
  aux(4): true text="zui4| "
  aux(5): true text="zui4| "
  aux(6): true text="zui4 |"

=== input: tzueiQ (consumed=5) ===
  aux(3): true text="zui| "
  aux(4): true text="zui| "
  aux(5): true text="zui |"
```

Every position emits well-formed UTF-8. The empty right suffix is the
clamped, memory-safe answer to the same question `pinyin_get_full_
pinyin_auxiliary_text` asks — "what canonical bytes sit to the right of
this raw cursor" — under the correct clamp `min(len, strlen(pinyin))`.

## Action

None on this side. No code, test, or pin change. Recorded here so future
work on the SECONDARY_ZHUYIN or LUOMA full-pinyin paths (both reproduced
above) has a pointer to the known upstream cause rather than being
re-diagnosed. Cross-indexed in
`reference/memory-safety-bugs.md` (§1 — invalid access). The
`full-pinyin-schemes` PR's upstream-divergences register entry
(`docs/findings/upstream-divergences.md`, added by that branch) tracks
oxpinyin's deliberate clamped behaviour; this document tracks the
upstream defect itself.
