# Upstream report drafts — the libpinyin report-back batch

Status: **open**. Drafts, not filed.

This file holds thirteen draft upstream reports. Each states the defect with
its pin cites and, where one exists, a reproduction and a fix:

- Items 1–4: four API defects, to file as one considered set.
- Item 5: Tkrzw has no upstream CI coverage. It is not an API defect and not part of the four-issue set.
- Item 6: `pinyin_init` leaves the process `LC_NUMERIC` at `"C"`.
- Item 7: temporary Berkeley DB handles leak on failed user-database loads.
- Item 8: every Berkeley DB `save_db` passes the wrong handle to `DB->sync`, with no observable effect.
- Item 9: the `SORT_BY_PINYIN_LENGTH` key wraps at 8 bits.
- Items 10–12: reachable heap-comparator and tail-sort precondition violations, and the source-established node front-eviction inversion.
- Item 13: a reloaded system library's total counts logged ADD and growing MODIFY records twice.

Items 5–13 are separate reports. Filing is the maintainer's action (and
account). Nothing here has been filed upstream. The contributor rules give the
collection mandate ("collected to report back to libpinyin once the rewrite
is complete"). Filing before the rewrite is complete is the maintainer's
decision. The drafts keep either timing cheap.

Suggested filing shape for items 1–4: four independent issues (no coupling
worth cross-blocking), ordered by severity below. Each links the
memory-safety character where applicable. The first four reproductions
are plain C against a stock libpinyin build, with no harness and no oxpinyin.
Every line number in this file is at the pin, libpinyin 2.11.92
`074a2219c90feaf962d0d24f034514033ece5f99`, where each defect is present.

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

**Build configuration** (the assert findings depend on it): plain
`./configure --disable-static --with-dbm=Tkrzw` with no caller
CFLAGS/CXXFLAGS. This gives autotools' default `-g -O2`, with **no
`-DNDEBUG`**, so `assert()` is live. The aborts in findings 2 and 3 are assertion firings, not
crash-adjacent undefined behaviour. They degrade silently under
`-DNDEBUG`: finding 2 then returns `true` with an unset out-param, and
finding 3 continues past the guard into the search. State this in those
issues as the NDEBUG-shaped secondary hazard. Finding 4 is different:
STANDARD_DVORAK and CUSTOMIZED reach explicit `abort()` calls, which
`-DNDEBUG` does not remove. They stay abortive in release builds. Only the
double out-of-enum lie is unconditional.

---

## 1. Heap over-read in `pinyin_get_full_pinyin_auxiliary_text` (info leak)

**Severity: high.** User-visible heap bytes are returned to the frontend.
Forming the out-of-bounds pointer is UB, so a fault is permitted, not
excluded.

`src/pinyin.cpp:3414-3419`, the mid-key cursor branch: `len = cursor -
begin` is computed in raw-input offsets and then used to index the
canonical pinyin string. `g_strdup(pinyin + len)` reads past the
canonical string whenever `begin < cursor < end` and `len >
strlen(pinyin)`. Not every mid-key cursor is vulnerable. The
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

LUOMA (scheme 2) reproduces the same shape where the raw key is at
least two bytes longer than the canonical one (e.g. `jhih` → `zh` at
cursor 3, `rih` → `r` at cursor 2).

**Suggested fix:** clamp before slicing. Where the slice is in bounds,
the result does not change:

```c
const size_t pinyin_len = strlen(pinyin);
const size_t clamped    = len < pinyin_len ? len : pinyin_len;
gchar *left  = g_strndup(pinyin, clamped);
gchar *right = g_strdup(pinyin + clamped);
```

---

## 2. `pinyin_get_sentence` is inconsistent on an out-of-range index

**Severity: medium.** This is a caller-misuse class for which the API
itself already returns `false` in one case. The same error gets `false`
on an empty result set, and `assert(index < results.size())` (SIGABRT)
on a non-empty one. This is an internal inconsistency, not a policy question.

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
rows that the candidate list carries. A row-count or race mistake is a crash.

**Suggested fix:** return `false` past the row count. This is the empty-set
branch's own behaviour, applied uniformly:

```c
if (index >= results.size())
    return false;
```

---

## 3. `contains_incomplete_pinyin` aborts on a toned initial-only key

**Severity: medium.** Public API inputs reach an assert: the
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
shape (search tonelessly, as the key semantics imply), or reject the
toned initial-only key at parse. The defect is the current split: the
parser permits the key, and the search aborts on it.

---

## 4. Scheme setters: a valid scheme aborts, invalid inputs lie or abort

**Severity: medium.** Three related contract holes in the setter
family. The first is a VALID input
(STANDARD_DVORAK is a real scheme). The other two are invalid-input
handling that aborts or lies:

- **zhuyin STANDARD_DVORAK (7)**: `pinyin_set_zhuyin_scheme` routes 7
  into `ZhuyinSimpleParser2::set_scheme`, whose dvorak arm assigns both
  tables and falls through into `default: abort()`
  (`zhuyin_parser2.cpp:291-295`). The wrapper also `delete`s the old
  parser before the switch (`pinyin.cpp:1163-1164`), so the context is
  broken even if the abort were caught.

```c
pinyin_set_zhuyin_scheme(ctx, 7);     /* SIGABRT after the tables were
                                         assigned */
```

- **double CUSTOMIZED (30)**: aborts mid-call inside
  `DoublePinyinParser2::set_scheme` (`pinyin_parser2.cpp:611-612`)
  after the unconditional fallback clear already ran; the wrapper never
  returns.

- **double out-of-enum (0, 7–29, 31+)**: the parser clears
  `m_fallback_table` first (`pinyin_parser2.cpp:582`) and returns `false`.
  The wrapper ignores the result and returns `true`. A live
  fallback-bearing scheme (ZRM/PYJJ/XHE) silently loses its fallback,
  while the caller is told that the call succeeded. This is a half-mutation.

**Suggested fix:**

- The dvorak arm needs `return true;` after its assignments. A bare `break`
  is not enough, because `set_scheme` returns `false` after the switch.
- The wrappers should propagate the parser's `false` instead of returning `true`.
- CUSTOMIZED should validate before any mutation runs.

---

## 5. Tkrzw has no upstream CI coverage

**Severity: packaging.** Not a crash. Upstream accepts Tkrzw in
autotools, cannot build it with CMake, and never exercises it. Debian
testing and unstable currently ship that configuration.

`tkrzw` does not appear in `CMakeLists.txt`, under `cmake/`, or under
`.github/`. `configure.ac` does accept it: `--with-dbm=Tkrzw` has its
own `if`, `PKG_CHECK_MODULES`, and `AC_DEFINE([HAVE_TKRZW])`.
`AS_HELP_STRING` for `--with-dbm` still says only "Select BerkeleyDB
or KyotoCabinet", and the comment above the block says the same.

`.github/workflows/` has two jobs, and neither builds Tkrzw:

- `make-check.yml`: `build` on `ubuntu-latest` in `fedora:rawhide`.
  Installs `gcc-c++ libdb-devel glib2-devel make gnome-common wget
  awk`, then `./configure --with-dbm=BerkeleyDB`, `make`, `make
  check`, and `make distcheck`. Berkeley DB is named on the command
  line. This is not a vestigial default: it is the configuration the
  autotools job asks for.
- `cmake.yml`: `build` on the same runner and container. Installs
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

**Reproduction.** Same source tree, `cmake -B build`:

- `libdb-devel` installed: `DB_FOUND` is set, `DATABASE_FORMAT` is
  `BerkeleyDB`, and Kyoto Cabinet is not searched.
- `libdb-devel` absent and Kyoto Cabinet present: `HAVE_BERKELEY_DB`
  stays unset, `find_package(KyotoCabinet REQUIRED)` runs, and
  `DATABASE_FORMAT` is `KyotoCabinet`.
- both absent: configure fails, because Kyoto Cabinet is `REQUIRED`.

`cmake.yml` is the second machine: its package list has
`kyotocabinet-devel` and no `libdb-devel`.

**Debian.** The `debian/rules` for 2.11.91-1
(`https://tracker.debian.org/media/packages/libp/libpinyin/rules-2.11.91-1`)
runs `dh_auto_configure -Sautoconf` with `--with-dbm=Tkrzw`, and
`libtkrzw-dev` is a build dependency. That is the configuration with no
upstream job. Stable and oldstable ship 2.8.1-1, whose `debian/rules`
passes `--with-dbm=BerkeleyDB`, which `make-check.yml` runs. Thus the
configuration that Debian testing and unstable ship has no upstream CI,
and the one that stable ships has.

---

## 6. `pinyin_init` leaves the process `LC_NUMERIC` at "C"

**Severity: process state.** Not a crash. A library call changes a
process-global locale category and does not put it back.

In `src/storage/table_info.cpp`, `SystemTableInfo2::load` (`:197`, `:291`), `UserTableInfo::load`
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
  selected (C11 7.11.1.1p7). Thus `locale` is `"C"`, and the closing call
  selects `"C"` again. To save the old value, query it first with
  `setlocale(LC_NUMERIC, NULL)`, and copy it with `g_strdup`, because the
  next `setlocale` may overwrite the returned buffer. Only then switch.
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

**Fix:** query, copy, switch, and restore on every exit. Alternatively,
the three functions only need `%d`/`%f` in the C locale, so they can use a
thread-local `uselocale(newlocale(LC_NUMERIC_MASK, "C", 0))` that is freed
on every return and leaves the process locale alone.

oxpinyin reproduces the pin's end state bug-for-bug
([ruling](compatibility-policy.md#lc_numeric-after-init-row-39)) until upstream fixes
it.

---

## 7. Berkeley DB `load_db` leaks temporary handles on failure

**Severity: resource leak.** Repeated context initialization with an
empty user directory loses native allocations, even when every context
is finalized. Both the pinyin and zhuyin facades are affected.

**Affected version:** libpinyin 2.11.92, pin
`074a2219c90feaf962d0d24f034514033ece5f99`, with Berkeley DB 5.3.28
on Linux amd64. Other revisions and the Kyoto Cabinet and Tkrzw
backends are not established.

Three `load_db` methods create `tmp_db` with `db_create`, then return
`false` after failed `tmp_db->open` or failed `copy_bdb` without closing
it. Their success paths do close it. The temporary handle is never
transferred to the context, so later `pinyin_fini`/`zhuyin_fini` cannot
release it:

- [`ChewingLargeTable2::load_db`, `src/storage/chewing_large_table2_bdb.cpp:115–130`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_bdb.cpp#L115-L130).
- [`PhraseLargeTable3::load_db`, `src/storage/phrase_large_table3_bdb.cpp:130–145`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_bdb.cpp#L130-L145).
- [`Bigram::load_db`, `src/storage/ngram_bdb.cpp:61–76`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_bdb.cpp#L61-L76).

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

**Expected result:** Memcheck reports three definitely-lost roots per
init/fini cycle, one for each temporary handle, with the indirect blocks
hanging from them:

- `ChewingLargeTable2::load_db` (`chewing_large_table2_bdb.cpp:115`),
  1,456 direct + 3,816 indirect bytes.
- `PhraseLargeTable3::load_db` (`phrase_large_table3_bdb.cpp:130`),
  1,456 + 3,823 bytes.
- `Bigram::load_db` (`ngram_bdb.cpp:61`), 1,456 + 3,823 bytes.

The indirect stacks pass through `db_env_create`/`__rep_env_create`
and the failed `tmp_db->open` at lines 121, 136 and 67. Both facades
give the same figures: 4,368 B definitely and 11,462 B indirectly lost
at N=1, and exactly ten times that at N=10. The loss grows linearly with
init/fini cycles, because an empty user directory takes the failed-open
paths on each cycle. A user directory with existing user databases
avoids those paths. With the suggested diff below applied, the same four
commands report no definite or indirect loss.

The `copy_bdb` failure branches have the same missing cleanup in the
source. The figures above apply to failed open only.

**Suggested fix:** close the successfully created temporary handle before
each of the two early returns in all three methods, preserving the
existing `false` result and success-path close. This proposed upstream
diff is not applied to shipping code:

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

The proposed cleanup is not an authorization to change oxpinyin behavior.

---

## 8. Berkeley DB `save_db` syncs the in-memory handle, not the file

**Severity: none observable.** This is a code defect with no effect on
output, files or durability, because `DB->close` flushes the file anyway.
The report is so that the call can be corrected, not because anything fails.

**Affected version:** libpinyin 2.11.92, pin
`074a2219c90feaf962d0d24f034514033ece5f99`, against Berkeley DB 5.3.28
(github.com/berkeleydb/libdb `e66a5e66`, `dist/RELEASE` 5.3.28). Other
revisions are not established.

Each Berkeley DB `save_db` writes the in-memory table `m_db` into a new
file `tmp_db`, then calls `tmp_db`'s `sync` member with `m_db` as its
handle argument:

- [`Bigram::save_db`, `src/storage/ngram_bdb.cpp:103`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/ngram_bdb.cpp#L103).
- [`ChewingLargeTable2::save_db`, `src/storage/chewing_large_table2_bdb.cpp:157`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/chewing_large_table2_bdb.cpp#L157).
- [`PhraseLargeTable3::save_db`, `src/storage/phrase_large_table3_bdb.cpp:172`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/phrase_large_table3_bdb.cpp#L172).
- [`PunctTable::save_db`, `src/storage/punct_table_bdb.cpp:133`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/storage/punct_table_bdb.cpp#L133).

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

---

## 9. `SORT_BY_PINYIN_LENGTH` compares the span in 8 bits

**Severity: low.** A wrong candidate order, with no memory or abort hazard.

`compare_item_with_sort_option` (`src/pinyin.cpp:1692-1694` at `074a2219`)
reads the pinyin length as `guint8 len = m_end - m_begin`. But the span is
a `guint16` difference (`lookup_candidate_t`, `:103-104`) with no 255-byte
bound:

- The parser accepts any run of apostrophes
  (`src/storage/pinyin_parser2.cpp:238-251`).
- Each apostrophe becomes a zero key that the matrix search steps over
  without counting it toward `MAX_PHRASE_LENGTH`
  (`src/storage/phonetic_key_matrix.cpp:69-80,383-393`).
- `pinyin_guess_candidates` records the whole span (`src/pinyin.cpp:2246`).

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

oxpinyin reproduces the wrap (unit test
`session::tests::the_pinyin_length_key_wraps_at_eight_bits`).

---

## 10. Beam and final-tail heap selection receive a non-strict comparator

`trellis_value_less_than<2>` can return true in both directions for two
candidates from the same live selection range. The enabled
"allow longer sentence" branch lets an adjacent longer sentence beat
the shorter one by `log(1.2f)`, while the unconditional shorter-sentence
fallback returns true in the reverse direction
(`src/lookup/phonetic_lookup.h:39,65–90 at 074a2219`).

**Reproduction:** reference BDB data, full-pinyin scheme 1, options
`0x1fe00198`, a fresh user directory, parse then `guess_sentence`. The
predicate on these live candidates, before selection, gives:

| Site and input | Step | Candidate A: position, length, `%a m_poss` | Candidate B: position, length, `%a m_poss` | Predicate results |
| --- | ---: | --- | --- | --- |
| Beam, `jianai` | 4 | 0, 1, `-0x1.b84bb8p+3` | 124, 2, `-0x1.85afap+3` | `less(A,B)=true`, `less(B,A)=true` |
| Final tails, `bian` | 4 | 1, 1, `-0x1.99254p+3` | 50, 2, `-0x1.9235c4p+3` | `less(A,B)=true`, `less(B,A)=true` |

`get_top_results` passes this comparator to `std::make_heap` and
`std::pop_heap` (`src/lookup/phonetic_lookup.h:143–171 at 074a2219`),
called at `src/lookup/phonetic_lookup.h:770–771,331–335 at 074a2219`.
The two-way result violates asymmetry, hence C++
[`[alg.sorting.general]`](https://eel.is/c++draft/alg.sorting.general)'s
strict weak ordering requirement. No particular downstream heap order
is claimed to be portable.

**Suggested repair:** rank by `poss − length · ln(1.2)`, with an explicit
tie order for batch selection. This preserves the adjacent-length margin
intent; for length gaps d≥2 it replaces unconditional shorter preference
with a probability-ratio threshold `1.2^d`, a deliberate semantic choice.
Keep the node admission tie rule separate from the selection tie key.

Full candidate identities are in
`docs/findings/upstream-divergences.md#pinyin-trellis-adjusted-cost--s1s2-repair`.

---

## 11. Tail sorting truncates score differences into non-transitive equality

`trellis_value_compare` returns `-(lhs->m_poss - rhs->m_poss)` as `gint`
(`src/lookup/phonetic_lookup.h:174–178 at 074a2219`). Two scores less
than one log unit apart therefore compare equal. This is incompatible
with the consistent total ordering required by the qsort-style contract
of `g_ptr_array_sort`, called at
`src/lookup/phonetic_lookup.h:337 at 074a2219`.

**Reproduction:** for `qian`, full-pinyin scheme 1, options `0x1fe00198`,
reference BDB data and a fresh user directory, parse then
`guess_sentence` reaches this three-element pre-sort array at step 4:

| Position | Length | `%a m_poss` | Current token | Slot |
| ---: | ---: | --- | --- | ---: |
| 0 (A) | 1 | `-0x1.c9dc3cp+2` | `0x010017b3` | 0 |
| 1 (B) | 1 | `-0x1.dda91p+2` | `0x010017b3` | 1 |
| 2 (C) | 1 | `-0x1.0a68dp+3` | `0x010017af` | 0 |

The pin comparator returns zero for AB, BA, BC and CB, −1 for AC, and
+1 for CA. Thus A~B and B~C but A≁C. All three previous tokens are
`0x00000001`.

**Suggested repair:** keep the source's "in descending order" criterion
and use a stable descending `m_poss` comparison without truncation. Do
not substitute the adjusted selection score. Policy for NaN from a
λ outside `[0, 1]` is a separate question.

Full results are in
`docs/findings/upstream-divergences.md#pinyin-trellis-adjusted-cost--s1s2-repair`. The
[GLib 2.84.4 sort contract](https://github.com/GNOME/glib/blob/2.84.4/glib/garray.c#L2240-L2295)
and [C11 §7.22.5 paragraph 4](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1570.pdf)
state the comparator requirements.

---

## 12. The two-slot trellis node evicts its best resident

The defect follows from the source. The comments
"min heap here", "invariant: min heap", and "find minium item" describe
a worst-at-front bounded queue, but `trellis_value_comp` uses the ordinary
less-than direction with `std::push_heap` and `std::pop_heap`
(`src/lookup/phonetic_lookup_heap.h:25–36,56–81 at 074a2219`).
`src/include/stl_lite.h:38–44 at 074a2219` imports those standard algorithms.
For equal-length finite candidates, less-than orders by possibility,
so the heap front is the best resident. A full node admits only a
newcomer better than that front and evicts the front, retaining the
weaker resident.

For example, with equal lengths and resident scores −10 and −20, the
source admits −5 and retains {−5, −20}, discarding −10. A newcomer −15
would instead be rejected despite beating the −20 resident. These
numbers illustrate the source-derived algorithm; they are not runtime
captures. The comparator on these same-length scores satisfies the heap
contract, so this is an algorithm defect independent of item 10.

**Suggested upstream repair:** put the worst resident at the front with
the inverse valid heap order, preserving the separate admission test and
rejecting tying newcomers. oxpinyin retains the pin's front eviction
bug-for-bug; changing it requires a separate maintainer ruling or Stage 3.

---

## 13. A reloaded library's total applies logged item changes twice

**Severity: low.** Wrong unigram probabilities for the session, with no memory or abort hazard.

The defect follows from the source; no reproduction was run.

`pinyin_save` compacts the phrase index, so each sub-index total is the sum of its items (`src/pinyin.cpp:1139`, `src/storage/phrase_index.cpp:648-675` at `074a2219`).
`SubPhraseIndex::diff` then logs that total as the new value of a `LOG_MODIFY_HEADER` record, before the item records (`phrase_index.cpp:393-399`).
The total already holds the effect of every addition, removal and item change.

On the next load, `SubPhraseIndex::merge` replays the log over the system chunk:

- `LOG_MODIFY_HEADER` sets `m_total_freq` to the logged total (`phrase_index.cpp:500-512`).
- `LOG_ADD_RECORD` calls `add_phrase_item`, which adds the item's unigram frequency to `m_total_freq` again (`:453-457`, `:215`).
- `LOG_REMOVE_RECORD` calls `remove_phrase_item`, which subtracts the item's unigram frequency from `m_total_freq` again (`:460-463`, `:219-234`, subtraction at `:233`).
- A `LOG_MODIFY_RECORD` whose new item is larger removes the old item and adds the new one, which subtracts the old unigram frequency and adds the new one (`:486-491`, `:233`, `:215`).
- An in-place `LOG_MODIFY_RECORD` leaves the total alone (`:492-499`).

The reloaded total is the saved total, plus each logged ADD item's unigram frequency, minus each logged REMOVE item's unigram frequency, plus the frequency change of each MODIFY that enlarged its item. The change is the new frequency minus the old frequency and can be negative.
`FacadePhraseIndex::merge` carries the total error into the facade total (`:291-296`), which divides every unigram probability (`src/pinyin.cpp:1814-1815`).

For example, a library whose items sum to 2162 at save, with one MODIFY that enlarged an item and changed its frequency from 100 to 1086, one ADD of frequency 516 and no REMOVE, reloads with a total of 2162 + 516 − 0 + (1086 − 100) = 3664.
These numbers are derived from the source; they are not a runtime capture.

**Suggested fix:** keep the header record first and check the current total against its logged old total (`phrase_index.cpp:503-512`). Replaying an ADD first can change the current total and fail that check. After replaying the last item record, set `m_total_freq` to the header's new total.

oxpinyin reproduces the reloaded total (unit test `a_mixed_profile_keeps_shipped_appended_and_user_state_apart`).

---

## Filing notes

- Independence: no fix touches another. Items 1–4 are filed as four issues.
  - Item 5 is a separate report. It does not belong in that batch.
  - Item 6 does not belong in that batch either. It is a process-state
    defect to file on its own.
  - Item 7 is a separate BDB resource-leak report.
  - Item 8 is a separate BDB report with no observable effect. It can go
    with item 7 or on its own.
  - Item 9 is a separate ranking report.
  - Items 10 and 11 report distinct comparator contracts. Item 12 reports
    a contract-valid algorithm defect and must not be conflated with them.
  - Item 13 is a separate phrase-index report.
- Order: 1 (info leak) → 2 (API inconsistency) → 3, 4 (abort classes).
- Filing follows a search of GitHub and Fedora Bugzilla for an existing report.
- Credit line under the historical attribution convention is the maintainer's to
  place; the reproductions above stand alone.
- After filing, each entry in `upstream-divergences.md` and
  `reference/memory-safety-bugs.md` §1.4 is marked with the issue links, so
  that the ledger no longer calls them unreported.
