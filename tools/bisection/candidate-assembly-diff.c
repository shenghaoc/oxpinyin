/*
 * candidate-assembly-diff.c — the candidate-list assembly path of
 * `pinyin_guess_candidates` / `zhuyin_guess_candidates_after_cursor`
 * against the pin (libpinyin 2.11.92 at 074a2219), full list, byte for
 * byte. Diagnosis only: prints every row of every list it asks for
 * (index, type, string) and asserts nothing. The runner diffs two logs.
 *
 * Three defects share this path (issue #582, register rows 34 and 37):
 *
 *   A  (#582)   SORT_WITHOUT_SENTENCE_CANDIDATE (0x1): the pin skips the
 *               n-best prepend (`pinyin.cpp:2295-2296`), so its phrase-
 *               string dedup (`:2298-2300`) never removes a NORMAL row
 *               whose text repeats a sentence.
 *   N  (#34)    n-best lifetime: the pin's parse leaves `m_nbest_results`
 *               alone (`pinyin.cpp:1497-1524`); only `pinyin_reset`
 *               clears it (`:2693-2704`).
 *   C  (#37)    the window after a choose: the pin rebuilds it from
 *               `start = offset` on every call (`pinyin.cpp:2184-2262`)
 *               and keeps no composition offset.
 *
 * Pinyin mode, per option word in {0x0, 0x1, 0x1e, 0x1f}, per import
 * state in {none, 罢跨/ba'kua/5 in USER_DICTIONARY}:
 *   A  li'shi and ba'kua: parse, guess_sentence, guess_candidates(0, W).
 *   N  guess_sentence then a new parse: parse li'shi, guess_sentence,
 *      parse ba'kua (no guess), get_sentence, guess_candidates(0, W);
 *      then guess_sentence, guess_candidates(0, W).
 *   C  a choose then a re-query over lishibakua: guess_sentence,
 *      guess_candidates(0, W), choose the first NORMAL row at offset 0,
 *      re-query at 0 and at the returned cursor, guess_sentence, the same
 *      two re-queries. Words with bit 0x1 clear also run C2: choose row 0
 *      (the 1-best NBEST row) and re-query at 0 after a re-guess.
 *   M  a choose behind an earlier choose — ibus's moveCursorLeft, which
 *      looks up at 0 (`PYPPhoneticEditor.cc:595-604`), at the words with
 *      bit 0x1 clear (under 0x1 ibus commits on the first choose):
 *      M1 chooses the first NORMAL row at 0, re-guesses and looks up at
 *      the cursor, then looks up at 0 and chooses the first one-character
 *      NORMAL row there (the overlapped forcing is replaced); M2 first
 *      chooses a second phrase at the cursor, so a forcing after the
 *      behind-choose's span survives it. Each choose passes the offset
 *      it looked up at, as ibus does; every list, sentence and cursor
 *      after it is printed.
 *   S  sequential chooses at the cursor (#602): choose the first NORMAL
 *      row at 0, re-guess, look up at the returned cursor and choose its
 *      k-th NORMAL row (k = 1..3 — the 1-best there hides a lost forcing),
 *      re-guess and print the sentence; then once more at the new cursor.
 *      Pinyin runs it over lishibakua and li'shi'ba'kua (separators) at
 *      the words with bit 0x1 clear.
 *
 *   T  the C / M / S shapes under the transformed input schemes — double
 *      pinyin (MS), Luoma and secondary zhuyin — at the words 0x0 and 0x1e,
 *      over one four-key spelling per scheme (the caller's offsets are
 *      in the original input's coordinates).
 *   K  lookups strictly inside a key under the same schemes, before and after
 *      a choose: the pin's matrix column there is empty.
 *
 * Zhuyin mode (libzhuyin, standard keyboard, the pin's default option
 * word): the same A / N / C / M / S shape over ㄌㄧˋㄕˇ (`xu4g3`, 歷史) and
 * ㄅㄚˋㄎㄨㄚˋ (`184dj84`) with no import (see the #575 note in main),
 * through zhuyin_guess_candidates_after_cursor.
 *
 * Usage:
 *   ./candidate-assembly-diff pinyin <libpinyin.so> <systemdir>
 *   ./candidate-assembly-diff zhuyin <libzhuyin.so> <systemdir>
 *
 * Both modes set the pin's own default option word explicitly. Each case
 * runs on a fresh context over a fresh user dir under
 * $TMPDIR (removed afterwards). Exit: 0 on a completed walk; 1 on
 * setup failure.
 */

#define _POSIX_C_SOURCE 200809L
#include <dirent.h>
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

typedef void context_t;
typedef void instance_t;
typedef void candidate_t;
typedef void import_iterator_t;
typedef uint32_t guint;
typedef int32_t gint;
typedef char gchar;

#define USER_DICTIONARY 7u
#define NBEST_MATCH_CANDIDATE 1
#define NORMAL_CANDIDATE 2
/* libzhuyin's own enum (zhuyin.h): NORMAL_CANDIDATE_AFTER_CURSOR = 2. */
#define ZHUYIN_NORMAL_AFTER_CURSOR 2
/* USE_TONE (pinyin_custom2.h:36), the pin's pinyin default (pinyin.cpp:329). */
#define PINYIN_USE_TONE (1u << 5)
/* The pin's zhuyin default word: USE_TONE | FORCE_TONE (zhuyin.cpp:273). */
#define ZHUYIN_OPTIONS ((1u << 5) | (1u << 6))
#ifndef ZHUYIN_READING
#define ZHUYIN_READING "ㄅㄚˋ ㄎㄨㄚˋ"
#endif

static void *lib;
static bool zhuyin;

typedef context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(context_t *);
typedef bool (*fn_set_options)(context_t *, guint);
typedef instance_t *(*fn_alloc)(context_t *);
typedef void (*fn_free)(instance_t *);
typedef size_t (*fn_parse)(instance_t *, const char *);
typedef bool (*fn_guess_sentence)(instance_t *);
typedef bool (*fn_pinyin_get_sentence)(instance_t *, uint8_t, char **);
typedef bool (*fn_zhuyin_get_sentence)(instance_t *, char **);
typedef bool (*fn_pinyin_guess_cands)(instance_t *, size_t, guint);
typedef bool (*fn_zhuyin_guess_cands)(instance_t *, size_t);
typedef bool (*fn_getn)(instance_t *, guint *);
typedef bool (*fn_getc)(instance_t *, guint, candidate_t **);
typedef bool (*fn_gettype)(instance_t *, candidate_t *, int *);
typedef bool (*fn_getstr)(instance_t *, candidate_t *, const gchar **);
typedef int (*fn_choose)(instance_t *, size_t, candidate_t *);
typedef bool (*fn_save)(context_t *);
typedef bool (*fn_pinyin_train)(instance_t *, uint8_t);
typedef bool (*fn_zhuyin_train)(instance_t *);
typedef bool (*fn_getnbest)(instance_t *, candidate_t *, uint8_t *);
typedef import_iterator_t *(*fn_begin_add)(context_t *, uint8_t);
typedef bool (*fn_add_phrase)(import_iterator_t *, const char *, const char *, gint);
typedef void (*fn_end_add)(import_iterator_t *);
typedef void (*fn_g_free)(void *);
typedef bool (*fn_set_scheme)(context_t *, int);

static struct {
    fn_init init;
    fn_fini fini;
    fn_set_options set_options;
    fn_alloc alloc;
    fn_free free_inst;
    fn_parse parse;
    fn_guess_sentence guess_sentence;
    fn_pinyin_get_sentence pinyin_get_sentence;
    fn_zhuyin_get_sentence zhuyin_get_sentence;
    fn_pinyin_guess_cands pinyin_guess_cands;
    fn_zhuyin_guess_cands zhuyin_guess_cands;
    fn_getn getn;
    fn_getc getc;
    fn_gettype gettype;
    fn_getstr getstr;
    fn_choose choose;
    fn_save save;
    fn_pinyin_train pinyin_train;
    fn_zhuyin_train zhuyin_train;
    fn_getnbest getnbest;
    fn_begin_add begin_add;
    fn_add_phrase add_phrase;
    fn_end_add end_add;
    fn_g_free g_free;
    fn_set_scheme set_full_scheme;
    fn_set_scheme set_double_scheme;
    fn_parse parse_double;
} s;

/* The input scheme a pinyin case runs under (the T cases): 0 plain full
 * pinyin, 1 double pinyin (MS), 2 Luoma, 3 secondary zhuyin. */
static int scheme_kind;

/* Header suffix of a transformed-scheme case; empty for plain full pinyin,
 * so the plain cases keep their names (and the declared table its regexes). */
static const char *scheme_tag(void) {
    static const char *tags[] = {"", " scheme=double", " scheme=luoma", " scheme=secondary-zhuyin"};
    return tags[scheme_kind];
}

static void *must(const char *name) {
    void *sym = dlsym(lib, name);
    if (!sym) {
        fprintf(stderr, "MISSING %s\n", name);
        exit(1);
    }
    return sym;
}

static void *must_prefixed(const char *suffix) {
    char name[128];
    snprintf(name, sizeof(name), "%s_%s", zhuyin ? "zhuyin" : "pinyin", suffix);
    return must(name);
}

static const char *yesno(bool v) { return v ? "true" : "false"; }

static void rm_rf(const char *dir) {
    DIR *d = opendir(dir);
    if (d) {
        struct dirent *entry;
        while ((entry = readdir(d)) != NULL) {
            if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0)
                continue;
            char path[4096];
            if (snprintf(path, sizeof(path), "%s/%s", dir, entry->d_name) >=
                (int)sizeof(path))
                continue;
            unlink(path);
        }
        closedir(d);
    }
    rmdir(dir);
}

struct session {
    char userdir[256];
    context_t *ctx;
    instance_t *inst;
};

/* A fresh context on its own user dir, optionally with the one import. */
static bool open_session(struct session *ss, const char *systemdir, guint options,
                         bool import) {
    const char *tmp = getenv("TMPDIR");
    snprintf(ss->userdir, sizeof(ss->userdir), "%s/cand-assembly-XXXXXX",
             tmp && *tmp ? tmp : "/tmp");
    if (!mkdtemp(ss->userdir)) {
        perror("mkdtemp");
        return false;
    }
    ss->ctx = s.init(systemdir, ss->userdir);
    if (!ss->ctx) {
        printf("init=NULL\n");
        rm_rf(ss->userdir);
        return false;
    }
    printf("set_options(0x%x)=%s\n", options, yesno(s.set_options(ss->ctx, options)));
    if (scheme_kind == 1)
        printf("set_double_pinyin_scheme(MS)=%s\n", yesno(s.set_double_scheme(ss->ctx, 2)));
    else if (scheme_kind > 1)
        printf("set_full_pinyin_scheme(%d)=%s\n", scheme_kind,
               yesno(s.set_full_scheme(ss->ctx, scheme_kind)));
    if (import) {
        import_iterator_t *it = s.begin_add(ss->ctx, USER_DICTIONARY);
        /* libzhuyin parses the import's reading with ZhuyinDirectParser2
         * (`zhuyin.cpp:515-526`): bopomofo, not pinyin. */
        const char *phrase = zhuyin ? "罷跨" : "罢跨";
        const char *reading = zhuyin ? ZHUYIN_READING : "ba'kua";
        bool added = it && s.add_phrase(it, phrase, reading, 5);
        printf("import %s/%s/5=%s\n", phrase, reading, yesno(added));
        if (it)
            s.end_add(it);
        if (!added) {
            /* A requested import that cannot be installed leaves an empty
             * user store under an `import=true` header: if it fails the
             * same way on both sides the logs still match, and the case
             * would pass without running the path it claims to cover. */
            fprintf(stderr, "import %s/%s failed\n", phrase, reading);
            s.fini(ss->ctx);
            rm_rf(ss->userdir);
            exit(1);
        }
    }
    ss->inst = s.alloc(ss->ctx);
    if (!ss->inst) {
        printf("alloc=NULL\n");
        s.fini(ss->ctx);
        rm_rf(ss->userdir);
        return false;
    }
    return true;
}

static void close_session(struct session *ss) {
    s.free_inst(ss->inst);
    s.fini(ss->ctx);
    rm_rf(ss->userdir);
}

static bool guess_cands(instance_t *inst, size_t offset, guint word) {
    return zhuyin ? s.zhuyin_guess_cands(inst, offset)
                  : s.pinyin_guess_cands(inst, offset, word);
}

/* The whole list: count, then every row as `index type string`. */
static guint dump_list(instance_t *inst, const char *label, size_t offset, guint word) {
    bool ok = guess_cands(inst, offset, word);
    guint n = 0;
    bool got_n = s.getn(inst, &n);
    printf("LIST %s offset=%zu ret=%s n=%u%s\n", label, offset, yesno(ok), n,
           got_n ? "" : " (get_n=false)");
    for (guint i = 0; i < n; ++i) {
        candidate_t *cand = NULL;
        int type = -1;
        const gchar *str = NULL;
        if (!s.getc(inst, i, &cand) || !cand) {
            printf("  %u get_candidate=false\n", i);
            continue;
        }
        bool t = s.gettype(inst, cand, &type);
        bool c = s.getstr(inst, cand, &str);
        printf("  %u %d %s\n", i, t ? type : -1, c && str ? str : "(null)");
    }
    return n;
}

static void print_sentence(instance_t *inst, const char *label) {
    char *sentence = NULL;
    bool ok = zhuyin ? s.zhuyin_get_sentence(inst, &sentence)
                     : s.pinyin_get_sentence(inst, 0, &sentence);
    printf("SENTENCE %s ret=%s %s\n", label, yesno(ok), ok && sentence ? sentence : "(none)");
    if (sentence)
        s.g_free(sentence);
}

/* Index of the first row of `type` in the current list, or -1. */
static int first_of_type(instance_t *inst, int type) {
    guint n = 0;
    if (!s.getn(inst, &n))
        return -1;
    for (guint i = 0; i < n; ++i) {
        candidate_t *cand = NULL;
        int t = -1;
        if (s.getc(inst, i, &cand) && cand && s.gettype(inst, cand, &t) && t == type)
            return (int)i;
    }
    return -1;
}

/* Index of the k-th (1-based) row of `type`, or -1. */
static int nth_of_type(instance_t *inst, int type, int k) {
    guint n = 0;
    if (!s.getn(inst, &n))
        return -1;
    for (guint i = 0; i < n; ++i) {
        candidate_t *cand = NULL;
        int t = -1;
        if (s.getc(inst, i, &cand) && cand && s.gettype(inst, cand, &t) && t == type &&
            --k == 0)
            return (int)i;
    }
    return -1;
}

/* Index of the first row of `type` whose string is one CJK character
 * (three UTF-8 bytes), or -1. */
static int first_single_of_type(instance_t *inst, int type) {
    guint n = 0;
    if (!s.getn(inst, &n))
        return -1;
    for (guint i = 0; i < n; ++i) {
        candidate_t *cand = NULL;
        int t = -1;
        const gchar *str = NULL;
        if (s.getc(inst, i, &cand) && cand && s.gettype(inst, cand, &t) && t == type &&
            s.getstr(inst, cand, &str) && str && strlen(str) == 3)
            return (int)i;
    }
    return -1;
}

static int choose_row_at(instance_t *inst, const char *label, int index, size_t offset) {
    candidate_t *cand = NULL;
    const gchar *str = NULL;
    if (index < 0 || !s.getc(inst, (guint)index, &cand) || !cand) {
        printf("CHOOSE %s row=%d unavailable\n", label, index);
        return -1;
    }
    s.getstr(inst, cand, &str);
    int cursor = s.choose(inst, offset, cand);
    printf("CHOOSE %s offset=%zu row=%d %s cursor=%d\n", label, offset, index,
           str ? str : "(null)", cursor);
    return cursor;
}

static int choose_row(instance_t *inst, const char *label, int index) {
    candidate_t *cand = NULL;
    const gchar *str = NULL;
    if (index < 0 || !s.getc(inst, (guint)index, &cand) || !cand) {
        printf("CHOOSE %s row=%d unavailable\n", label, index);
        return -1;
    }
    s.getstr(inst, cand, &str);
    int cursor = s.choose(inst, 0, cand);
    printf("CHOOSE %s row=%d %s cursor=%d\n", label, index, str ? str : "(null)", cursor);
    return cursor;
}

static void case_a(const char *systemdir, guint options, guint word, bool import,
                   const char *input) {
    struct session ss;
    printf("== A word=0x%x import=%s input=%s%s\n", word, yesno(import), input, scheme_tag());
    if (!open_session(&ss, systemdir, options, import))
        return;
    printf("parse=%zu\n", s.parse(ss.inst, input));
    printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
    print_sentence(ss.inst, "after-guess");
    dump_list(ss.inst, "after-guess", 0, word);
    close_session(&ss);
}

static void case_n(const char *systemdir, guint options, guint word, bool import,
                   const char *first, const char *second) {
    struct session ss;
    printf("== N word=0x%x import=%s input=%s->%s\n", word, yesno(import), first, second);
    if (!open_session(&ss, systemdir, options, import))
        return;
    printf("parse=%zu\n", s.parse(ss.inst, first));
    printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
    printf("reparse=%zu\n", s.parse(ss.inst, second));
    print_sentence(ss.inst, "after-reparse");
    dump_list(ss.inst, "after-reparse", 0, word);
    printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
    print_sentence(ss.inst, "after-reguess");
    dump_list(ss.inst, "after-reguess", 0, word);
    close_session(&ss);
}

static void requery(instance_t *inst, const char *label, int cursor, guint word) {
    char buf[64];
    snprintf(buf, sizeof(buf), "%s@0", label);
    dump_list(inst, buf, 0, word);
    if (cursor > 0) {
        snprintf(buf, sizeof(buf), "%s@cursor", label);
        dump_list(inst, buf, (size_t)cursor, word);
    }
}

static void case_c(const char *systemdir, guint options, guint word, bool import,
                   const char *input, bool nbest_choose) {
    struct session ss;
    printf("== C%s word=0x%x import=%s input=%s%s\n", nbest_choose ? "2" : "1", word,
           yesno(import), input, scheme_tag());
    if (!open_session(&ss, systemdir, options, import))
        return;
    printf("parse=%zu\n", s.parse(ss.inst, input));
    printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
    dump_list(ss.inst, "before-choose", 0, word);
    int normal = zhuyin ? ZHUYIN_NORMAL_AFTER_CURSOR : NORMAL_CANDIDATE;
    int row = nbest_choose ? first_of_type(ss.inst, NBEST_MATCH_CANDIDATE)
                           : first_of_type(ss.inst, normal);
    int cursor = choose_row(ss.inst, nbest_choose ? "nbest" : "normal", row);
    if (row >= 0) {
        requery(ss.inst, "after-choose", cursor, word);
        printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
        print_sentence(ss.inst, "after-choose-reguess");
        requery(ss.inst, "after-choose-reguess", cursor, word);
    }
    close_session(&ss);
}

/* A printed lookup at a returned cursor; false when the choose returned
 * no usable cursor (nothing is looked up then). */
static bool lookup(instance_t *inst, const char *label, int offset, guint word) {
    if (offset <= 0)
        return false;
    dump_list(inst, label, (size_t)offset, word);
    return true;
}

static void case_m(const char *systemdir, guint options, guint word, bool import,
                   const char *input, bool later_forcing) {
    struct session ss;
    printf("== M%s word=0x%x import=%s input=%s%s\n", later_forcing ? "2" : "1", word,
           yesno(import), input, scheme_tag());
    if (!open_session(&ss, systemdir, options, import))
        return;
    int normal = zhuyin ? ZHUYIN_NORMAL_AFTER_CURSOR : NORMAL_CANDIDATE;
    printf("parse=%zu\n", s.parse(ss.inst, input));
    printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
    dump_list(ss.inst, "start", 0, word);
    int cursor = choose_row_at(ss.inst, "first", first_of_type(ss.inst, normal), 0);
    printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
    if (!lookup(ss.inst, "after-first", cursor, word)) {
        close_session(&ss);
        return;
    }
    if (later_forcing) {
        int next = choose_row_at(ss.inst, "second", first_of_type(ss.inst, normal),
                                 (size_t)cursor);
        printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
        print_sentence(ss.inst, "after-second");
        if (next > 0 && next < (int)strlen(input))
            dump_list(ss.inst, "after-second", (size_t)next, word);
    }
    /* moveCursorLeft: the lookup cursor goes to 0, behind the choose. */
    dump_list(ss.inst, "behind", 0, word);
    int back = choose_row_at(ss.inst, "behind", first_single_of_type(ss.inst, normal), 0);
    printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
    print_sentence(ss.inst, "after-behind");
    dump_list(ss.inst, "after-behind", 0, word);
    lookup(ss.inst, "after-behind", back, word);
    close_session(&ss);
}

static void case_s(const char *systemdir, guint options, guint word, const char *input,
                   int k) {
    struct session ss;
    printf("== S%d word=0x%x input=%s%s\n", k, word, input, scheme_tag());
    if (!open_session(&ss, systemdir, options, false))
        return;
    int normal = zhuyin ? ZHUYIN_NORMAL_AFTER_CURSOR : NORMAL_CANDIDATE;
    printf("parse=%zu\n", s.parse(ss.inst, input));
    printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
    dump_list(ss.inst, "start", 0, word);
    int cursor = choose_row_at(ss.inst, "first", first_of_type(ss.inst, normal), 0);
    for (int step = 0; step < 2; ++step) {
        printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
        print_sentence(ss.inst, "step");
        if (cursor <= 0 || cursor >= (int)strlen(input))
            break;
        dump_list(ss.inst, "at-cursor", (size_t)cursor, word);
        cursor = choose_row_at(ss.inst, "at-cursor", nth_of_type(ss.inst, normal, k),
                               (size_t)cursor);
    }
    printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
    print_sentence(ss.inst, "end");
    close_session(&ss);
}

static void load_syms(void) {
    s.init = (fn_init)must_prefixed("init");
    s.fini = (fn_fini)must_prefixed("fini");
    s.set_options = (fn_set_options)must_prefixed("set_options");
    s.alloc = (fn_alloc)must_prefixed("alloc_instance");
    s.free_inst = (fn_free)must_prefixed("free_instance");
    s.guess_sentence = (fn_guess_sentence)must_prefixed("guess_sentence");
    s.getn = (fn_getn)must_prefixed("get_n_candidate");
    s.getc = (fn_getc)must_prefixed("get_candidate");
    s.gettype = (fn_gettype)must_prefixed("get_candidate_type");
    s.getstr = (fn_getstr)must_prefixed("get_candidate_string");
    s.choose = (fn_choose)must_prefixed("choose_candidate");
    s.save = (fn_save)must_prefixed("save");
    s.begin_add = (fn_begin_add)must_prefixed("begin_add_phrases");
    s.add_phrase = (fn_add_phrase)must_prefixed("iterator_add_phrase");
    s.end_add = (fn_end_add)must_prefixed("end_add_phrases");
    if (zhuyin) {
        s.zhuyin_train = (fn_zhuyin_train)must("zhuyin_train");
        s.parse = (fn_parse)must("zhuyin_parse_more_chewings");
        s.zhuyin_get_sentence = (fn_zhuyin_get_sentence)must("zhuyin_get_sentence");
        s.zhuyin_guess_cands =
            (fn_zhuyin_guess_cands)must("zhuyin_guess_candidates_after_cursor");
    } else {
        s.pinyin_train = (fn_pinyin_train)must("pinyin_train");
        s.getnbest = (fn_getnbest)must("pinyin_get_candidate_nbest_index");
        s.parse = (fn_parse)must("pinyin_parse_more_full_pinyins");
        s.pinyin_get_sentence = (fn_pinyin_get_sentence)must("pinyin_get_sentence");
        s.pinyin_guess_cands = (fn_pinyin_guess_cands)must("pinyin_guess_candidates");
        s.set_full_scheme = (fn_set_scheme)must("pinyin_set_full_pinyin_scheme");
        s.set_double_scheme = (fn_set_scheme)must("pinyin_set_double_pinyin_scheme");
        s.parse_double = (fn_parse)must("pinyin_parse_more_double_pinyins");
    }
    s.g_free = (fn_g_free)dlsym(RTLD_DEFAULT, "g_free");
    if (!s.g_free)
        s.g_free = (fn_g_free)must("g_free");
}

/* Lane I (#603 / #527): every offered n-best row is chosen on its own
 * fresh context, re-guessed, trained and saved. Pinyin diffs against row
 * zero (074a2219 pinyin.cpp:2515-2520); zhuyin BEST_MATCH adds no
 * constraint at all (zhuyin.cpp:1643-1644). The runner dumps the saved
 * bigram DB and compares the binary unigram logs, avoiding the export
 * iterator's independently audited traversal defects. */
static int train_rows(const char *systemdir, const char *root, const char *input,
                      int fixture) {
    for (int ordinal = 1; ; ++ordinal) {
        struct session ss;
        guint options = zhuyin ? ZHUYIN_OPTIONS : PINYIN_USE_TONE;
        if (!open_session(&ss, systemdir, options, false))
            return 1;
        if (fixture == 3) {
            import_iterator_t *it = s.begin_add(ss.ctx, USER_DICTIONARY);
            bool added = it && s.add_phrase(it, "测测", "cece", 5);
            if (it)
                s.end_add(it);
            if (!added) {
                close_session(&ss);
                return 1;
            }
        }
        if (s.parse(ss.inst, input) != strlen(input) ||
            !s.guess_sentence(ss.inst) || !guess_cands(ss.inst, 0, 0x1e)) {
            close_session(&ss);
            return 1;
        }
        int row = nth_of_type(ss.inst, NBEST_MATCH_CANDIDATE, ordinal);
        if (row < 0) {
            close_session(&ss);
            if (ordinal == 1) {
                fprintf(stderr, "no n-best candidate for %s\n", input);
                return 1;
            }
            break;
        }
        printf("== T fixture=%d ordinal=%d input=%s\n", fixture, ordinal, input);
        if (!zhuyin) {
            candidate_t *candidate = NULL;
            uint8_t rank = 255;
            if (!s.getc(ss.inst, (guint)row, &candidate) || !candidate ||
                !s.getnbest(ss.inst, candidate, &rank)) {
                close_session(&ss);
                return 1;
            }
            /* Coverage metadata, separate from the training-state log:
             * #594 can give equal displayed rows different tail ranks.
             * Keep each side's actual rank rather than assuming the list
             * position is the n-best index. lishibakua covers 0,1,2. */
            fprintf(stderr, "coverage fixture=%d ordinal=%d nbest-index=%u\n",
                    fixture, ordinal, (unsigned)rank);
        }
        int cursor = choose_row(ss.inst, "nbest", row);
        bool guessed = s.guess_sentence(ss.inst);
        bool trained = zhuyin ? s.zhuyin_train(ss.inst) : s.pinyin_train(ss.inst, 0);
        bool saved = s.save(ss.ctx);
        printf("reguess=%s train=%s save=%s\n", yesno(guessed), yesno(trained),
               yesno(saved));
        s.free_inst(ss.inst);
        s.fini(ss.ctx);
        char dest[4096];
        if (cursor < 0 || !guessed || !trained || !saved ||
            snprintf(dest, sizeof(dest), "%s/%d-%d", root, fixture, ordinal) >=
                (int)sizeof(dest) || rename(ss.userdir, dest) != 0) {
            rm_rf(ss.userdir);
            fprintf(stderr, "choose/train/save/snapshot failed\n");
            return 1;
        }
    }
    return 0;
}

/* T: the same C / M / S shapes under the transformed input schemes. The
 * caller's offsets live in the ORIGINAL input's coordinates (the pin's
 * matrix keeps each key at its key rest's `m_raw_begin`,
 * `phonetic_key_matrix.cpp:52-56`), and the window is built from
 * `start = offset` for every scheme (`pinyin.cpp:2224-2262`), so a lookup
 * behind a choose answers the window there and a choose from it moves the
 * record back (register row 37). One four-key spelling per scheme, the
 * ones the cases actually run:
 *   double pinyin (MS)  `liuibakw`     li|ui|ba|kw   历史把跨
 *   Luoma               `nihaobakua`   ni|hao|ba|kua 你好把跨 — not the
 *                       历史 spelling, which needs Luoma's `shih`: the pin
 *                       decodes that incomplete key under USE_TONE and
 *                       oxpinyin does not (#626), so it is kept out of
 *                       this differential
 *   secondary zhuyin    `lishibakua`   li|shi|ba|kua 历史把跨
 * The K cases probe the offsets strictly inside a key, where the pin's
 * matrix column is empty. */
static void case_t(const char *systemdir, guint options, guint word, int kind,
                   const char *input) {
    scheme_kind = kind;
    fn_parse saved = s.parse;
    if (kind == 1)
        s.parse = s.parse_double;
    case_a(systemdir, options, word, false, input);
    case_c(systemdir, options, word, false, input, false);
    case_m(systemdir, options, word, false, input, false);
    case_m(systemdir, options, word, false, input, true);
    for (int k = 1; k <= 3; ++k)
        case_s(systemdir, options, word, input, k);
    s.parse = saved;
    scheme_kind = 0;
}

static void case_k(const char *systemdir, guint options, guint word, int kind,
                   const char *input, const int *offsets, int n, bool after_choose) {
    scheme_kind = kind;
    fn_parse saved = s.parse;
    if (kind == 1)
        s.parse = s.parse_double;
    struct session ss;
    printf("== K%d word=0x%x input=%s%s\n", after_choose ? 2 : 1, word, input, scheme_tag());
    if (open_session(&ss, systemdir, options, false)) {
        printf("parse=%zu\n", s.parse(ss.inst, input));
        if (after_choose) {
            printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
            dump_list(ss.inst, "start", 0, word);
            choose_row_at(ss.inst, "first", first_of_type(ss.inst, NORMAL_CANDIDATE), 0);
        }
        printf("guess_sentence=%s\n", yesno(s.guess_sentence(ss.inst)));
        print_sentence(ss.inst, "before-mid");
        for (int j = 0; j < n; ++j) {
            char label[32];
            snprintf(label, sizeof label, "mid(%d)", offsets[j]);
            dump_list(ss.inst, label, (size_t)offsets[j], word);
        }
        close_session(&ss);
    }
    s.parse = saved;
    scheme_kind = 0;
}

int main(int argc, char **argv) {
    if (argc != 4 || (strcmp(argv[1], "pinyin") != 0 && strcmp(argv[1], "zhuyin") != 0)) {
        fprintf(stderr, "usage: %s pinyin|zhuyin <lib.so> <systemdir>\n", argv[0]);
        return 1;
    }
    zhuyin = strcmp(argv[1], "zhuyin") == 0;
    lib = dlopen(argv[2], RTLD_NOW | RTLD_GLOBAL);
    if (!lib) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    load_syms();
    const char *systemdir = argv[3];
    setvbuf(stdout, NULL, _IOLBF, 0);

    const char *train_root = getenv("CAND_ASSEMBLY_TRAIN_DIR");
    if (train_root) {
        const char *inputs[] = {"jintian", "nihao", "lishibakua", "cecenihao"};
        const char *chewings[] = {"xu4g3", "su3cl3", "xu4g3184dj84"};
        for (int i = 0; i < (zhuyin ? 3 : 4); ++i)
            if (train_rows(systemdir, train_root, zhuyin ? chewings[i] : inputs[i], i))
                return 1;
        return 0;
    }

    if (zhuyin) {
        /* Import state off only: libzhuyin's import reading parser
         * diverges on its own (issue #575 — the pin takes bopomofo, the
         * subject full pinyin), which would mask this path. The helper
         * keeps the bopomofo reading for when #575 closes. */
        for (int imp = 0; imp < 1; ++imp) {
            case_a(systemdir, ZHUYIN_OPTIONS, 0, imp, "xu4g3");
            case_a(systemdir, ZHUYIN_OPTIONS, 0, imp, "184dj84");
            case_n(systemdir, ZHUYIN_OPTIONS, 0, imp, "xu4g3", "184dj84");
            case_n(systemdir, ZHUYIN_OPTIONS, 0, imp, "xu4g3", "xu4g3");
            case_c(systemdir, ZHUYIN_OPTIONS, 0, imp, "xu4g3184dj84", false);
            case_c(systemdir, ZHUYIN_OPTIONS, 0, imp, "xu4g3184dj84", true);
            case_m(systemdir, ZHUYIN_OPTIONS, 0, imp, "xu4g3184dj84", false);
            case_m(systemdir, ZHUYIN_OPTIONS, 0, imp, "xu4g3184dj84", true);
            for (int k = 1; k <= 3; ++k)
                case_s(systemdir, ZHUYIN_OPTIONS, 0, "xu4g3184dj84", k);
        }
        return 0;
    }

    /* Pinyin: `pinyin_init`'s own option word (`USE_TONE`,
     * `pinyin.cpp:329`), set explicitly so the defaults of the two sides
     * (issue #532) do not enter the comparison. No DYNAMIC_ADJUST: the
     * lists are a function of the parse, the import and the sort word. */
    const guint options = PINYIN_USE_TONE;
    const guint words[] = {0x0, 0x1, 0x1e, 0x1f};
    for (size_t w = 0; w < sizeof(words) / sizeof(words[0]); ++w) {
        guint word = words[w];
        for (int imp = 0; imp < 2; ++imp) {
            case_a(systemdir, options, word, imp, "li'shi");
            case_a(systemdir, options, word, imp, "ba'kua");
            case_n(systemdir, options, word, imp, "li'shi", "ba'kua");
            case_n(systemdir, options, word, imp, "li'shi", "li'shi");
            case_c(systemdir, options, word, imp, "lishibakua", false);
            if (!(word & 0x1))
                case_c(systemdir, options, word, imp, "lishibakua", true);
            if (!(word & 0x1)) {
                case_m(systemdir, options, word, imp, "lishibakua", false);
                case_m(systemdir, options, word, imp, "lishibakua", true);
                if (!imp) {
                    for (int k = 1; k <= 3; ++k) {
                        case_s(systemdir, options, word, "lishibakua", k);
                        case_s(systemdir, options, word, "li'shi'ba'kua", k);
                    }
                }
            }
        }
    }
    static const struct {
        int kind;
        const char *input;
    } schemes[] = {{1, "liuibakw"}, {2, "nihaobakua"}, {3, "lishibakua"}};
    for (size_t t = 0; t < sizeof(schemes) / sizeof(schemes[0]); ++t) {
        case_t(systemdir, options, 0x0, schemes[t].kind, schemes[t].input);
        case_t(systemdir, options, 0x1e, schemes[t].kind, schemes[t].input);
    }
    /* K: lookups strictly inside a key, original coordinates. */
    static const int mid_double[] = {1, 3, 5, 7};
    static const int mid_long[] = {1, 3, 4, 6, 8, 9};
    for (size_t t = 0; t < sizeof(schemes) / sizeof(schemes[0]); ++t) {
        const int *offs = schemes[t].kind == 1 ? mid_double : mid_long;
        int n = schemes[t].kind == 1 ? 4 : 6;
        for (int after = 0; after < 2; ++after) {
            case_k(systemdir, options, 0x0, schemes[t].kind, schemes[t].input, offs, n, after);
            case_k(systemdir, options, 0x1e, schemes[t].kind, schemes[t].input, offs, n, after);
        }
    }
    return 0;
}
