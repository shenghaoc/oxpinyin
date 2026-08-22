# Upstream report drafts — the libpinyin report-back batch

Date: 2026-08-22 · Status: drafts, not filed.

Four finds with fixes and reproductions, consolidated so they can be
filed as a considered set rather than trickling. Filing is the
maintainer's action (and account); nothing here has been posted. The
collection mandate is `AGENTS.md`'s ("collected to report back to
libpinyin once the rewrite is complete") — filing earlier than
rewrite-completion is the maintainer's call to make; the drafts keep
either timing cheap.

Suggested filing shape: four independent issues (no coupling worth
cross-blocking), ordered by severity below, each linking the
memory-safety character where applicable. Reproductions are plain C
against a stock libpinyin build — no harness, no oxpinyin. All four
verified against the pinned libpinyin 2.11.91 (`0c5e80e`); the aux
over-read was additionally surveyed still-present on `main` @
`55e9051` (2026-08-20) with no existing report found on GitHub or
Fedora Bugzilla.

**Shared setup — every reproduction below is a fragment that assumes
this block** (declared once here, not repeated per snippet):

```c
#include <pinyin.h>

/* Build: gcc repro.c -o repro $(pkg-config --cflags --libs libpinyin)
 * (pinyin.h pulls the glib headers through pkg-config). */
pinyin_context_t *ctx = pinyin_init(systemdir, userdir); /* both dirs
    must exist and be writable: the user dir gets the user store */
pinyin_instance_t *inst = pinyin_alloc_instance(ctx);
/* ... the snippet ... */
pinyin_free_instance(inst);
pinyin_fini(ctx);
```

**Build configuration of the verified oracle** (the assert findings
depend on it): built by `tools/oracle/build-oracle.sh` with no caller
CFLAGS/CXXFLAGS, plain `./configure --disable-static --with-dbm=Tkrzw`
— autotools' default `-g -O2`, **no `-DNDEBUG`**, so `assert()` is
live. The aborts in findings 2 and 3 are assertion firings, not
crash-adjacent undefined behaviour, and they degrade silently under
`-DNDEBUG` — finding 2 then answers `true` with an unset out-param,
finding 3 continues past the guard into the search — which is worth
stating in those issues as the NDEBUG-shaped secondary hazard. Finding
4 is different: STANDARD_DVORAK and CUSTOMIZED reach explicit
`abort()` calls, which `-DNDEBUG` does not remove — they stay abortive
in release builds; only the double out-of-enum lie is unconditional.

---

## 1. Heap over-read in `pinyin_get_full_pinyin_auxiliary_text` (info leak)

**Severity: high** — user-visible heap bytes returned to the frontend;
forming the out-of-bounds pointer is UB, so a fault is permitted, not
excluded.

`src/pinyin.cpp:3414-3419`, the mid-key cursor branch: `len = cursor -
begin` is computed in raw-input offsets and then used to index the
canonical pinyin string — `g_strdup(pinyin + len)` reads past the
canonical string whenever `begin < cursor < end` and `len >
strlen(pinyin)`. Not every mid-key cursor is vulnerable; the
SECONDARY_ZHUYIN trigger below is deterministic.

```c
/* Reproduction (assumes the shared setup): prints the auxiliary string
 * with non-deterministic heap bytes past the canonical "tzu" prefix;
 * run under valgrind/ASan to see the over-read directly. */
pinyin_set_full_pinyin_scheme(ctx, FULL_PINYIN_SECONDARY_ZHUYIN); /* 3 */
pinyin_parse_more_full_pinyins(inst, "tzuei");
size_t cursor = 4;   /* the byte immediately before the parsed-key end */
gchar *aux_text = NULL;
pinyin_get_full_pinyin_auxiliary_text(inst, cursor, &aux_text);
printf("aux = \"%s\"\n", aux_text);
g_free(aux_text);
```

LUOMA (scheme 2) reproduces the same shape on 8 of its 18 raw ≥
canonical + 2 rows (e.g. `jhih` → `zh` at cursor 3, `rih` → `r` at
cursor 2).

**Suggested fix** (verified byte-identical where in-bounds on the pin):
clamp before slicing —

```c
const size_t pinyin_len = strlen(pinyin);
const size_t clamped    = len < pinyin_len ? len : pinyin_len;
gchar *left  = g_strndup(pinyin, clamped);
gchar *right = g_strdup(pinyin + clamped);
```

Full writeup with the upstream-status survey and the in-bounds boundary
analysis: `docs/findings/full-pinyin-aux-overread.md`.

---

## 2. `pinyin_get_sentence` is inconsistent on an out-of-range index

**Severity: medium** — a caller-misuse class that the API itself
already answers `false` for in one case: the same error gets `false`
on an empty result set and `assert(index < results.size())` — SIGABRT
— on a non-empty one. Internal inconsistency, not a policy question.

`src/pinyin.cpp:1463-1482`:

```c
if (0 == results.size())
    return false;                      /* defined refusal */
MatchResult result = NULL;
assert(index < results.size());        /* the same misuse aborts here */
```

```c
/* Reproduction: parse an input whose lookup yields rows, then ask one
 * past whatever the lookup produced. */
pinyin_parse_more_full_pinyins(inst, "nihao");
pinyin_guess_sentence(inst);
pinyin_guess_candidates(inst, 0, 0x1e);
char *sentence = NULL;
pinyin_get_sentence(inst, 3, &sentence);   /* rows.size() == 3 here;
                                             any index >= it aborts */
```

Frontends are safe today only because they render exactly the NBEST
rows the candidate list carries; a row-count/race mistake is a crash.

**Suggested fix:** answer `false` past the row count — the empty-set
branch's own behaviour, applied uniformly:

```c
if (index >= results.size())
    return false;
```

---

## 3. `contains_incomplete_pinyin` aborts on a toned initial-only key

**Severity: medium** — a reachable assert from public API inputs: the
parser produces exactly what the search asserts against.

`src/storage/pinyin_phrase3.h:146-156` asserts
`CHEWING_ZERO_TONE == key.m_tone` for any zero-middle/zero-final key,
and every `chewing_large_table2` search path dispatches through it.
Under `PINYIN_INCOMPLETE | USE_TONE` the parser accepts `n4` (an
initial-only key with a tone digit); the first phrase search
containing that key trips the assert.

```c
pinyin_set_options(ctx, PINYIN_INCOMPLETE | USE_TONE);   /* 0x8 | 0x20 */
pinyin_parse_more_full_pinyins(inst, "n4");
pinyin_guess_sentence(inst);          /* assert fires in the search */
```

**Suggested fix:** either relax the assert for the incomplete-key
shape (search tonelessly, as the key semantics imply) or reject the
toned initial-only key at parse; the current split — parser permits,
search aborts — is the defect.

---

## 4. Scheme setters: a valid scheme aborts, invalid inputs lie or abort

**Severity: medium** — three related contract holes in the setter
family, all verified at `0c5e80e`. The first is a VALID input
(STANDARD_DVORAK is a real scheme); the other two are invalid-input
handling that aborts or lies:

- **zhuyin STANDARD_DVORAK (7)** — `pinyin_set_zhuyin_scheme` routes 7
  into `ZhuyinSimpleParser2::set_scheme`, whose dvorak arm assigns both
  tables and falls through into `default: abort()`
  (`zhuyin_parser2.cpp:291-295`). The wrapper also `delete`s the old
  parser before the switch (`pinyin.cpp:1163-1164`), so the context is
  broken even if the abort were caught. Still present at libpinyin tip
  `95e3af7`.

```c
pinyin_set_zhuyin_scheme(ctx, 7);     /* SIGABRT after the tables were
                                         assigned */
```

- **double CUSTOMIZED (30)** — aborts mid-call inside
  `DoublePinyinParser2::set_scheme` (`pinyin_parser2.cpp:611-612`)
  after the unconditional fallback clear already ran; the wrapper never
  returns.

- **double out-of-enum (0, 7–29, 31+)** — the parser clears
  `m_fallback_table` first (`pinyin_parser2.cpp:582`), returns `false`;
  the wrapper ignores the result and answers `true`. A live
  fallback-bearing scheme (ZRM/PYJJ/XHE) silently loses its fallback
  while the caller is told the call succeeded — a half-mutation.

**Suggested fix:** the dvorak arm needs `return true;` after its
assignments — a bare `break` is not enough, `set_scheme` returns
`false` after the switch; the wrappers should propagate the parser's
`false` instead of answering `true`; CUSTOMIZED should validate before
any mutation runs.

---

## Filing notes

- Independence: no fix touches another; file as four issues.
- Order: 1 (info leak) → 2 (API inconsistency) → 3, 4 (abort classes).
- Credit line per `AGENTS.md` attribution rules is the maintainer's to
  place; the reproductions above stand alone.
- After filing, mark each entry in `upstream-divergences.md` and
  `reference/memory-safety-bugs.md` §1.4 with the issue links so the
  ledger stops calling them unreported.
