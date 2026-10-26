# Upstream report drafts — the libpinyin report-back batch

Date: 2026-08-22 · Status: drafts, not filed.

Four finds with fixes and reproductions, consolidated so they can be
filed as a considered set rather than trickling. A fifth draft, item 5,
was added 2026-09-22: Tkrzw has no upstream CI coverage, not an API
defect, and not part of that four-issue filing shape. A sixth, item 6,
was added 2026-09-27 UTC: `pinyin_init` leaves the process
`LC_NUMERIC` at `"C"` (#539), read at the current pin `074a2219`.
A seventh draft, item 7, was added 2026-10-21 UTC: temporary Berkeley
DB handles leak on failed user-database loads (#993). An eighth, item 8,
was added 2026-10-22 UTC: every Berkeley DB `save_db` passes the wrong
handle to `DB->sync`, which has no observable effect. A ninth, item 9,
was added 2026-10-26 UTC: the `SORT_BY_PINYIN_LENGTH` key wraps at 8 bits
(#1275). The file now holds nine drafts; items 7, 8 and 9 are ready to file
as separate reports.
Filing is the
maintainer's action (and account); nothing here has been posted. The
collection mandate is `AGENTS.md`'s ("collected to report back to
libpinyin once the rewrite is complete") — filing earlier than
rewrite-completion is the maintainer's call to make; the drafts keep
either timing cheap.

Suggested filing shape: four independent issues (no coupling worth
cross-blocking), ordered by severity below, each linking the
memory-safety character where applicable. The first four reproductions
are plain C against a stock libpinyin build — no harness, no oxpinyin.
All four verified against the pinned libpinyin 2.11.91 (`0c5e80e`); the aux
over-read was additionally surveyed still-present on `main` @
`55e9051` (2026-08-20) with no existing report found on GitHub or
Fedora Bugzilla.

**Shared setup — the C fragments in items 1–4 and 6 assume
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

## 5. Tkrzw has no upstream CI coverage

**Severity: packaging** — not a crash. Upstream accepts Tkrzw in
autotools, cannot build it with CMake, and never exercises it. Debian
testing and unstable currently ship that configuration.

Read at upstream `main` `074a2219c90feaf962d0d24f034514033ece5f99`
(2026-09-03 09:26:19 UTC, subject "use g_rename to replace rename
since rename fails by replace-existing-file").

`tkrzw` does not appear in `CMakeLists.txt`, under `cmake/`, or under
`.github/`. `configure.ac` does accept it: `--with-dbm=Tkrzw` has its
own `if`, `PKG_CHECK_MODULES`, and `AC_DEFINE([HAVE_TKRZW])`.
`AS_HELP_STRING` for `--with-dbm` still says only "Select BerkeleyDB
or KyotoCabinet", and the comment above the block says the same.

`.github/workflows/` has two jobs, and neither builds Tkrzw:

- `make-check.yml` — `build` on `ubuntu-latest` in `fedora:rawhide`.
  Installs `gcc-c++ libdb-devel glib2-devel make gnome-common wget
  awk`, then `./configure --with-dbm=BerkeleyDB`, `make`, `make
  check`, and `make distcheck`. Berkeley DB is named on the command
  line. This is not a vestigial default: it is the configuration the
  autotools job asks for.
- `cmake.yml` — `build` on the same runner and container. Installs
  `gcc-c++ kyotocabinet-devel glib2-devel cmake ninja wget` and does
  not install `libdb-devel`, so CMake takes the Kyoto Cabinet arm.
  That arm is the only other backend CMake knows.

Upstream therefore tests both build systems and both backends CMake
knows about. It does not test Tkrzw.

CMake also has no way to pin the DBM. The `option()` calls are
`BUILD_SHARED_LIBS`, `BUILD_TESTING`, and `BUILD_UTILS`.
`find_package(BerkeleyDB)` sets `DATABASE_FORMAT` to `BerkeleyDB`
when `DB_FOUND`; otherwise `find_package(KyotoCabinet REQUIRED)` sets
it to `KyotoCabinet`. Whichever of those two is installed wins, and a
packager cannot name the other.

**Reproduction, from the CMake control flow** (not executed as two
configures in this record). Same source tree, `cmake -B build`:

- `libdb-devel` installed: `DB_FOUND` is set, `DATABASE_FORMAT` is
  `BerkeleyDB`, and Kyoto Cabinet is not searched.
- `libdb-devel` absent and Kyoto Cabinet present: `HAVE_BERKELEY_DB`
  stays unset, `find_package(KyotoCabinet REQUIRED)` runs, and
  `DATABASE_FORMAT` is `KyotoCabinet`.
- both absent: configure fails, because Kyoto Cabinet is `REQUIRED`.

`cmake.yml` is the second machine: its package list has
`kyotocabinet-devel` and no `libdb-devel`.

**Debian, checked rather than assumed.** Fetched
`debian/rules` for 2.11.91-1 from
`https://tracker.debian.org/media/packages/libp/libpinyin/rules-2.11.91-1`:
`dh_auto_configure -Sautoconf` with `--with-dbm=Tkrzw`, and the
tracker lists `libtkrzw-dev` as a build dependency. That is the
configuration with no upstream job. Stable and oldstable are
2.8.1-1; the Debian Sources copy of that `debian/rules` passes
`--with-dbm=BerkeleyDB`, which is what `make-check.yml` runs. The
claim that does not survive is "the path Debian ships has no
upstream CI": stable's path does. The claim that does is the
testing/unstable one.

---

## 6. `pinyin_init` leaves the process `LC_NUMERIC` at "C"

**Severity: process state** — not a crash. A library call changes a
process-global locale category and does not put it back.

Read at the pin `074a2219c90feaf962d0d24f034514033ece5f99`
(`src/storage/table_info.cpp`). Unlike items 1–4 this one was not
verified at `0c5e80e`; it is the round-2 audit's D-17 (oxpinyin #539).

`SystemTableInfo2::load` (`:197`, `:291`), `UserTableInfo::load`
(`:328`, `:372`) and `UserTableInfo::save` (`:378`, `:394`) all use the
same shape:

```c++
char * locale = setlocale(LC_NUMERIC, "C");
/* ... fopen, fscanf/fprintf ... */
setlocale(LC_NUMERIC, locale);
return true;
```

Two defects, each enough on its own:

- **The saved value is the new locale, not the old one.** `setlocale`
  with a non-NULL locale argument returns the name of the locale it just
  selected (C11 7.11.1.1p7), so `locale` is `"C"` and the closing call
  re-selects `"C"`. Saving the old value takes a query first —
  `setlocale(LC_NUMERIC, NULL)`, copied with `g_strdup` because the
  next `setlocale` may overwrite the returned buffer — and only then the
  switch.
- **Every early return skips the restore.** A missing file
  (`:199-202`, `:330-333`) or a failed version directive (`:339-348`)
  returns `false` before the closing `setlocale`.

**Reproduction** (the shared setup above, with these lines around its
`pinyin_init` call and `#include <locale.h>` added; the host must have
`zh_CN.UTF-8` generated):

```c
setlocale(LC_ALL, "zh_CN.UTF-8");
printf("before: %s\n", setlocale(LC_NUMERIC, NULL)); /* zh_CN.UTF-8 */
/* pinyin_context_t *ctx = pinyin_init(systemdir, userdir); — as above */
printf("after:  %s\n", setlocale(LC_NUMERIC, NULL)); /* C */
```

The same holds after a failed init (a user dir with no `user.conf`
takes the `:330-333` path) and after `pinyin_fini`, whose
`mark_version` runs `UserTableInfo::save`. An input-method frontend that
prints or parses locale-formatted numbers after init reads them in the
`C` locale from then on.

**Fix:** query, copy, switch, and restore on every exit — or, since the
three functions only need `%d`/`%f` in the C locale, a thread-local
`uselocale(newlocale(LC_NUMERIC_MASK, "C", 0))` that is freed on every
return and leaves the process locale alone.

oxpinyin reproduces the pin's end state bug-for-bug (ruling recorded
2026-09-27 UTC, `compatibility-policy.md` row 39) until upstream fixes
it.

---

## 7. Berkeley DB `load_db` leaks temporary handles on failure

**Severity: resource leak** — repeated context initialization with an
empty user directory loses native allocations, even when every context
is finalized. Both the pinyin and zhuyin facades are affected.

**Affected version:** verified at libpinyin 2.11.92, pin
`074a2219c90feaf962d0d24f034514033ece5f99`, with Berkeley DB 5.3.28
on Linux amd64. Other revisions, current upstream status, and the Kyoto
Cabinet/Tkrzw backends have not been established by this report.

Three `load_db` methods create `tmp_db` with `db_create`, then return
`false` after failed `tmp_db->open` or failed `copy_bdb` without closing
it. Their success paths do close it. The temporary handle is never
transferred to the context, so later `pinyin_fini`/`zhuyin_fini` cannot
release it:

- [`ChewingLargeTable2::load_db`, `src/storage/chewing_large_table2_bdb.cpp:115–130`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_bdb.cpp#L115-L130).
- [`PhraseLargeTable3::load_db`, `src/storage/phrase_large_table3_bdb.cpp:130–145`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_bdb.cpp#L130-L145).
- [`Bigram::load_db`, `src/storage/ngram_bdb.cpp:61–76`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_bdb.cpp#L61-L76).

These are the same pin blobs read in #993's detached `libpinyin-pin`
worktree, checked against the pinned tree before that worktree was
removed. Their Git blob identities are respectively
`469ee7d969e853ff07312b5b72b268dd47433a0e`,
`d54c80a6031c634c91c35d6aebd9ebc2ffac7f46`, and
`8910f9d6c7ba6f12cc234e1e02a7f99d47b6b7f2`.

The [official Berkeley DB C API contract for `DB->open`](https://docs.oracle.com/cd/E17275_01/html/api_reference/C/dbopen.html)
requires `DB->close` even when open fails. The
[`DB->close` contract](https://docs.oracle.com/cd/E17275_01/html/api_reference/C/dbclose.html)
describes release of allocated resources and invalidates the handle
regardless of the close return value.

**Reproduction** (standalone C, with the same `systemdir` prerequisite
as the shared setup). Save this as `repro.c`. It creates a fresh user
directory for each process and loops only over init/fini; no instance,
candidate workload, oxpinyin library or teardown harness is involved.
Build with libpinyin's installed headers and pkg-config files. The
`ZHUYIN` build selects the corresponding libzhuyin calls:

```c
#define _GNU_SOURCE
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#ifdef ZHUYIN
#include <zhuyin.h>
#define INIT zhuyin_init
#define FINI zhuyin_fini
#else
#include <pinyin.h>
#define INIT pinyin_init
#define FINI pinyin_fini
#endif
int main(int argc, char **argv) {
    const char *systemdir = getenv("PINYIN_SYSTEM_DIR");
    int n = argc == 2 ? atoi(argv[1]) : 0, status = 0;
    char userdir[] = "./libpinyin-repro-XXXXXX", marker[128];
    if (!systemdir || n < 1 || !mkdtemp(userdir)) return 2;
    for (int i = 0; i < n; ++i) {
        void *ctx = INIT(systemdir, userdir);
        if (!ctx) { status = 1; break; }
        FINI(ctx);
    }
    snprintf(marker, sizeof marker, "%s/user.conf", userdir);
    unlink(marker);
    if (rmdir(userdir) != 0) status = 1;
    return status;
}
```

Run the block below as a `sh` script from the directory where
`repro.c` was saved; `SRC` records that directory. Set `PREFIX` to the
installation prefix of the pin built with `--with-dbm=BerkeleyDB`; its
`.pc` files must name that same prefix. The block exports
`PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"` and
`LD_LIBRARY_PATH="$PREFIX/lib"`, then builds the pinyin and zhuyin
programs as separate steps. After each `cc` command, check the
`ldd ./repro` output: `libpinyin.so.15` (or `libzhuyin.so.15`) must
resolve under `$PREFIX/lib`, and `libdb` must be the Berkeley DB that
the prefix was built against. `PINYIN_SYSTEM_DIR` must contain the
matching BDB system data (`table.conf`, indexes and phrase/model files).
The pin's libpinyin `.pc` file puts these under `pkgdatadir/data`;
both facades use that same data directory. The block creates a private
temporary working directory, enters it, and removes it on exit,
interrupt or termination, so an interrupted process also loses its
temporary user directory. Each successful process removes its own
`user.conf` marker and directory. The marker can survive between loop
iterations; the three user databases remain absent because this
program does not save.

```sh
PREFIX=/path/to/libpinyin-prefix # the pin, built with --with-dbm=BerkeleyDB
SRC="$PWD"                       # the directory that holds repro.c
workdir="$(mktemp -d)" || exit 1
cleanup() { rm -rf -- "$workdir"; }
trap cleanup 0
trap 'exit 130' INT
trap 'exit 143' TERM
cd "$workdir" || exit 1

export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
export LD_LIBRARY_PATH="$PREFIX/lib"
export PINYIN_SYSTEM_DIR="$(pkg-config --variable=pkgdatadir libpinyin)/data"

# pinyin
cc -std=c11 -g -O0 -Wall -Wextra "$SRC/repro.c" -o repro $(pkg-config --cflags --libs libpinyin)
ldd ./repro
valgrind --leak-check=full --show-leak-kinds=definite,indirect ./repro 1
valgrind --leak-check=full --show-leak-kinds=definite,indirect ./repro 10

# zhuyin
cc -std=c11 -g -O0 -Wall -Wextra -DZHUYIN "$SRC/repro.c" -o repro $(pkg-config --cflags --libs libzhuyin)
ldd ./repro
valgrind --leak-check=full --show-leak-kinds=definite,indirect ./repro 1
valgrind --leak-check=full --show-leak-kinds=definite,indirect ./repro 10
```

**Standalone evidence** (Memcheck 3.27.1, Linux amd64; the table is
from a rerun of the block above on 2026-10-22 UTC, and each N=10 row
held in 5 of 5 further runs per facade on 2026-10-22 UTC): all four
init/fini-only runs completed against the reference pin prefix. Each
process used a fresh empty user directory. The reference libraries were
the pin-built `libpinyin.so.15.0.0` (SHA-256
`754499651c302efd91fe7f72dd6a791978400b268cb6206d2ebe644015721f30`)
and `libzhuyin.so.15.0.0` (SHA-256
`cf4ccbce9f70f6c7e412b92a52a243ba64fdea1ff197cbc44019c6b85650d53b`),
with the pin's existing Berkeley DB system data. On the measurement
host the pin prefix was a relocated copy of `/opt/pinyin-oracle/bdb`;
`PKG_CONFIG_PATH` pointed at a copy of its two `.pc` files with
`prefix=` set to that copy. No header or library content changed.

| Facade | N | Definitely lost | Indirectly lost | Total |
| --- | ---: | ---: | ---: | ---: |
| pinyin | 1 | 4,368 B / 3 blocks | 11,462 B / 30 blocks | 15,830 B / 33 blocks |
| pinyin | 10 | 43,680 B / 30 blocks | 114,620 B / 300 blocks | 158,300 B / 330 blocks |
| zhuyin | 1 | 4,368 B / 3 blocks | 11,462 B / 30 blocks | 15,830 B / 33 blocks |
| zhuyin | 10 | 43,680 B / 30 blocks | 114,620 B / 300 blocks | 158,300 B / 330 blocks |

The three definitely-lost roots are the temporary handles, and the
indirect blocks hang from them. In the N=1 runs the roots are:

- `ChewingLargeTable2::load_db` (`chewing_large_table2_bdb.cpp:115`),
  1,456 direct + 3,816 indirect bytes. The stack runs through
  `load` (`facade_chewing_table2.h:88`) and `pinyin_init`/`zhuyin_init`.
- `PhraseLargeTable3::load_db` (`phrase_large_table3_bdb.cpp:130`),
  1,456 + 3,823 bytes; caller `load` (`facade_phrase_table3.h:87`) and
  `pinyin_init`/`zhuyin_init`.
- `Bigram::load_db` (`ngram_bdb.cpp:61`), 1,456 + 3,823 bytes; caller
  `pinyin_init`/`zhuyin_init`.

The indirect stacks pass through `db_env_create`/`__rep_env_create`
and the failed `tmp_db->open` at lines 121, 136 and 67. N=10 repeats
each root ten times.

The standalone N=1 total is 15,830 B / 33 blocks per facade. #993's
15,854 B / 33 blocks per cycle is 24 bytes higher with the same block
count. The difference is the first-candidate work in #993's cycle:
this loop only calls init/fini and never prepares an instance or a
candidate. The three `load_db` roots are identical. At N=10 the direct
root count and both facades' indirect totals are exactly ten times the
N=1 figures (5 of 5 runs per facade). An earlier run of the previous
command block recorded the zhuyin N=10 indirect loss as 113,884 B / 298
blocks (total 157,564 B / 328 blocks), 736 bytes / 2 blocks below ten
times; the rerun did not reproduce that shortfall, it did not recur in
5 runs, and its cause was not investigated. The
direct 30-block count and every `load_db` root remain.

**Patched-pin check:** applying the suggested diff below to the same
pinned source and rebuilding it in a separate prefix removes the loss.
The patch SHA-256 is
`77e0453a6830255c9fb9ce832864a9ce72e093223d4f7fc6c0214e214067dc4a`.
This host lacks `autoreconf`, so the build used the pin's generated
`configure` and the oracle recipe's `--libs-only` steps
(`--disable-static --with-dbm=BerkeleyDB --enable-libzhuyin`,
`make -C src`, `make -C src install`, `make install-pkgconfigDATA`);
the system data stayed the reference prefix's. The same four valgrind
commands then reported:

| Facade | N | Definitely lost | Indirectly lost |
| --- | ---: | ---: | ---: |
| pinyin | 1 | 0 B / 0 blocks | 0 B / 0 blocks |
| pinyin | 10 | 0 B / 0 blocks | 0 B / 0 blocks |
| zhuyin | 1 | 0 B / 0 blocks | 0 B / 0 blocks |
| zhuyin | 10 | 0 B / 0 blocks | 0 B / 0 blocks |

**Supporting reproduction from #993** (the exact existing attribution
command, retained as historical evidence). It requires the oxpinyin harness at
[`11da1d7adb324f516d1b2cf87db831096e33f486`](https://github.com/shenghaoc/oxpinyin/tree/11da1d7adb324f516d1b2cf87db831096e33f486),
the final head of [#993](https://github.com/shenghaoc/oxpinyin/pull/993);
this documentation PR does not supply that unmerged harness. Run from
`/src` at that ref in the prepared #993 Debian Linux amd64 BDB cell,
with its pin-built `/work/reference`, equivalent system data, and
shipping facade libraries under `/work/shipping`. Preparation and input
identities are recorded in #993. Keep the normal container reservation
and host admission from the [compatibility runbook](../runbooks/compatibility-container.md);
these paths are reproduction inputs, not retained artifact links.

```sh
uv run --no-project --managed-python --python cpython@3.15.0+gil python -B -X lazy_imports=normal -m tools.dev slot run -- uv run --no-project --managed-python --python cpython@3.15.0+gil python -B -X lazy_imports=normal tools/bisection/contract-diff.py bdb /work/reference /work/shipping/lib/libpinyin.so --zhuyin-so /work/shipping/lib/libzhuyin.so --teardown --teardown-stage attribute --teardown-attribution-case journals --teardown-work-dir /work/measure
```

The attribution stage includes the empty-profile Memcheck loop for
both facades and all three subjects. The native child keeps its library
loaded and repeats init, first-candidate preparation, instance free and
fini 100 times; the parent restores the same fresh empty user profile
between cycles. This is the existing lifecycle reproduction, not a
claim that a separate init-only driver was run. Its native Memcheck
worker uses `--leak-check=no --show-leak-kinds=all --num-callers=24
--undef-value-errors=no`, with explicit `VALGRIND_DO_LEAK_CHECK` and
count requests at the lifecycle checkpoints. Thus automatic exit leak
checking is disabled, but post-fini live/lost counts are still measured.
The stage also runs the journals and other attribution controls; it is
not an empty-only command. These remain the original #993 measurements.

**Supporting evidence:** #993's Memcheck 3.27.1 results for the pin,
after fini (not standalone init-only results):

| Completed init/fini cycles | Lost bytes, each facade | Lost blocks, each facade |
| --- | ---: | ---: |
| 1 | 15,854 | 33 |
| 10 | 158,540 | 330 |
| 50 | 792,700 | 1,650 |
| 100 | 1,585,400 | 3,300 |

The allocation stacks identify the three temporary handles above. The
empty directory takes their failed-open paths on each cycle. The
journals profile, with existing user databases, avoids those paths and
has no corresponding post-fini lost-allocation growth. This is live
client-allocation evidence, not an inference from RSS or allocator arena
retention. Raw captures were ephemeral, parsed in memory, and deleted
with #993's private operational files; none is retained or uploaded.

The `copy_bdb` failure branches have the same missing cleanup by source
inspection. They were not fault-injected or measured; the byte/block
figures above apply to failed open only.

**Suggested fix:** close the successfully created temporary handle before
each of the two early returns in all three methods, preserving the
existing `false` result and success-path close. This proposed upstream
diff is not applied to shipping code; it was built and measured in the
separate patched prefix above:

```diff
--- a/src/storage/chewing_large_table2_bdb.cpp
+++ b/src/storage/chewing_large_table2_bdb.cpp
@@ -121,4 +121,6 @@
     ret = tmp_db->open(tmp_db, NULL, filename, NULL,
                        DB_BTREE, DB_RDONLY, 0600);
-    if (ret != 0)
+    if (ret != 0) {
+        tmp_db->close(tmp_db, 0);
         return false;
+    }
@@ -126,2 +128,4 @@
-    if (!copy_bdb(tmp_db, m_db))
+    if (!copy_bdb(tmp_db, m_db)) {
+        tmp_db->close(tmp_db, 0);
         return false;
+    }
--- a/src/storage/phrase_large_table3_bdb.cpp
+++ b/src/storage/phrase_large_table3_bdb.cpp
@@ -136,4 +136,6 @@
     ret = tmp_db->open(tmp_db, NULL, filename, NULL,
                        DB_BTREE, DB_RDONLY, 0600);
-    if (ret != 0)
+    if (ret != 0) {
+        tmp_db->close(tmp_db, 0);
         return false;
+    }
@@ -141,2 +143,4 @@
-    if (!copy_bdb(tmp_db, m_db))
+    if (!copy_bdb(tmp_db, m_db)) {
+        tmp_db->close(tmp_db, 0);
         return false;
+    }
--- a/src/storage/ngram_bdb.cpp
+++ b/src/storage/ngram_bdb.cpp
@@ -67,4 +67,6 @@
     ret = tmp_db->open(tmp_db, NULL, dbfile, NULL,
                        DB_HASH, DB_RDONLY, 0600);
-    if ( ret != 0 )
+    if ( ret != 0 ) {
+        tmp_db->close(tmp_db, 0);
         return false;
+    }
@@ -72,2 +74,4 @@
-    if ( !copy_bdb(tmp_db, m_db) )
+    if ( !copy_bdb(tmp_db, m_db) ) {
+        tmp_db->close(tmp_db, 0);
         return false;
+    }
```

This separate BDB resource-leak report is ready for the maintainer to
file. Nothing has been filed upstream; the
proposed cleanup is not an authorization to change oxpinyin behavior.

---

## 8. Berkeley DB `save_db` syncs the in-memory handle, not the file

**Severity: none observable** — a code defect with no effect on output,
files or durability, because `DB->close` flushes the file anyway. It is
reported so the call can be corrected, not because anything fails.

**Affected version:** read at libpinyin 2.11.92, pin
`074a2219c90feaf962d0d24f034514033ece5f99`, against Berkeley DB 5.3.28
(github.com/berkeleydb/libdb `e66a5e66`, `dist/RELEASE` 5.3.28; Debian's
`5.3.28+dfsg2-11` patch series changes none of the lines cited). Other
revisions and current upstream status have not been established.

Each Berkeley DB `save_db` writes the in-memory table `m_db` into a new
file `tmp_db`, then calls `tmp_db`'s `sync` member with `m_db` as its
handle argument:

- [`Bigram::save_db`, `src/storage/ngram_bdb.cpp:103`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_bdb.cpp#L103)
  (blob `8910f9d6c7ba6f12cc234e1e02a7f99d47b6b7f2`).
- [`ChewingLargeTable2::save_db`, `src/storage/chewing_large_table2_bdb.cpp:157`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_bdb.cpp#L157)
  (blob `469ee7d969e853ff07312b5b72b268dd47433a0e`).
- [`PhraseLargeTable3::save_db`, `src/storage/phrase_large_table3_bdb.cpp:172`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_bdb.cpp#L172)
  (blob `d54c80a6031c634c91c35d6aebd9ebc2ffac7f46`).
- [`PunctTable::save_db`, `src/storage/punct_table_bdb.cpp:133`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_bdb.cpp#L133)
  (blob `bdc7b84022a61c7cb282cd0f319d7a06f3f880ba`).

```c
    if ( tmp_db != NULL ) {
        tmp_db->sync(m_db, 0);
        tmp_db->close(tmp_db, 0);
    }
```

Every `DB` handle's `sync` member is the same function, `__db_sync_pp`
(`src/db/db_method.c:313`), so the call syncs `m_db`. Which database
that is depends on how the table was opened (all lines at 074a2219):

- In libpinyin itself every `save_db` call follows `load_db`, so `m_db`
  is the unnamed in-memory database `load_db` creates, and `__db_sync`
  returns at once for those (`src/db/db_am.c:813-814`). The user bigram
  is loaded at `src/pinyin.cpp:401` and saved at `:1018`, and at
  `src/zhuyin.cpp:343` and `:685`; the user pinyin and phrase tables at
  `src/storage/facade_chewing_table2.h:87` and `:96`, and
  `src/storage/facade_phrase_table3.h:86` and `:95`.
- The library calls `attach` only with `ATTACH_READONLY`, on system
  tables, which are never saved: `src/pinyin.cpp:396` and `:440`,
  `src/zhuyin.cpp:338`, `src/storage/facade_chewing_table2.h:82` and
  `src/storage/facade_phrase_table3.h:81`. No library code calls
  `PunctTable::save_db`.
- When `save_db` follows `attach`, `sync(m_db)` flushes `m_db`'s own
  file, which is still harmless. Two of libpinyin's tests do this:
  `tests/storage/test_ngram.cpp` attaches at `:40` and saves at `:78`,
  and `tests/storage/test_punct_table.cpp` attaches at `:49` and saves
  at `:63`. Nothing in `utils/` does.

In every case the new file is still flushed: `DB->close` without
`DB_NOSYNC` syncs the handle it closes (`src/db/db.c:872-874`). Nothing
reaches the disk later than intended, so there is no reproduction to
give.

**Proposed fix**, the same in each of the four files:

```diff
-        tmp_db->sync(m_db, 0);
+        tmp_db->sync(tmp_db, 0);
```

Nothing has been filed upstream.

---

## 9. `SORT_BY_PINYIN_LENGTH` compares the span in 8 bits

**Severity: low** — a wrong candidate order, no memory or abort hazard.

`compare_item_with_sort_option` (`src/pinyin.cpp:1692-1694` at `074a2219`)
reads the pinyin length as `guint8 len = m_end - m_begin`, but the span is
a `guint16` difference (`lookup_candidate_t`, `:103-104`) with no 255-byte
bound: the parser accepts any run of apostrophes
(`src/storage/pinyin_parser2.cpp:238-251`), each becomes a zero key the
matrix search steps over without counting it toward `MAX_PHRASE_LENGTH`
(`src/storage/phonetic_key_matrix.cpp:69-80,383-393`), and
`pinyin_guess_candidates` records the whole span (`src/pinyin.cpp:2246`).
A span of 256 bytes or more wraps, so the longer candidate sorts as a
short one.

```c
char input[2 + 251 + 3 + 1] = "ni";
memset(input + 2, '\'', 251);
strcpy(input + 253, "hao");            /* 你好 spans 256 bytes */
pinyin_parse_more_full_pinyins(inst, input);
pinyin_guess_candidates(inst, 0, SORT_BY_PINYIN_LENGTH);
/* 你好 (key 0) sorts after 你 (key 2); with 250 apostrophes it is first. */
```

**Suggested fix:** compare the spans as `guint16` (or `gint`) values.

oxpinyin reproduces the wrap (#1275; contract-diff
`pinyin-length-wrap/span-*`). Nothing has been filed upstream.

---

## 10. Question: which order should cyclic trellis selection define?

This is a question about the intended product order, not a defect report:
should sentence selection specify an ordering independently of the heap's
comparison sequence? No issue is authorized or filed for this note.

Source read at 074a2219 in the lane's detached pin checkout:
`src/lookup/phonetic_lookup.h:65-91,141-172` and
`src/lookup/phonetic_lookup_heap.h:56-79`; `include/stl_lite.h:38-44`
aliases `std_lite` to `std`. No issue has been filed.

With nstore > 1, let P = log(1.2f), L be character length and S the log
possibility (larger is better). If L(a)+1=L(b) and S(b)>S(a)+P,
the first clause says a is less than b, while the shorter-sentence
fallback also says b is less than a. The reverse adjacent-length clause
at :75-77 is masked by the fallback at :87-88: every longer left value
is already less regardless of score.

Cycles also occur. For lengths 1/1/2, take scores -3P/-2P/0:
a < b by score, b < c by the adjacent clause, c < a by length.
For lengths 1/2/3, scores -4P/-2P/0 give the same cycle.
These are valid finite possibilities; NaN is unnecessary.

The C++ heap algorithms require their comparator to induce a strict
weak ordering ([alg.sorting.general](https://eel.is/c++draft/alg.sorting.general#3),
covering [heap operations](https://eel.is/c++draft/alg.heap.operations)).
This predicate violates asymmetry and transitivity, so
the selected values depend on the heap implementation's comparison
sequence. oxpinyin preserves the product predicate and counts while
using a compact loser tree of the same directed bracket; the implementation-dependent
difference is recorded in [1297-selection-structure.md](1297-selection-structure.md).

Two independent upstream fix options: exclude adjacent lengths from the
shorter-sentence fallback (the minimal asymmetry fix, which still leaves
the 1/2/3 cycles), or define one adjusted score for all lengths with a
documented total tie order. Neither product-level fix is applied in this
Phase 2 cleanup. The existing margin quantisation (263 Cost units
against gfloat log(1.2f)) remains the earlier class (a) difference.

## Filing notes

- Independence: no fix touches another; file items 1–4 as four issues.
  Item 5 is a separate report; it does not belong in that batch, and
  neither does item 6, which is a process-state defect to file on its
  own. Item 7 is a separate BDB resource-leak report, ready to file.
  Item 8 is a separate BDB report with no observable effect; it can go
  with item 7 or on its own.
  Item 9 is a separate ranking report. Item 10 is a product-order question,
  not a defect report or an instruction to file an issue.
- Order: 1 (info leak) → 2 (API inconsistency) → 3, 4 (abort classes).
- Credit line per `AGENTS.md` attribution rules is the maintainer's to
  place; the reproductions above stand alone.
- After filing, mark each entry in `upstream-divergences.md` and
  `reference/memory-safety-bugs.md` §1.4 with the issue links so the
  ledger stops calling them unreported.
