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
 *
 * Zhuyin mode (libzhuyin, standard keyboard, the pin's default option
 * word): the same A / N / C shape over ㄌㄧˋㄕˇ (`xu4g3`, 歷史) and
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
typedef import_iterator_t *(*fn_begin_add)(context_t *, uint8_t);
typedef bool (*fn_add_phrase)(import_iterator_t *, const char *, const char *, gint);
typedef void (*fn_end_add)(import_iterator_t *);
typedef void (*fn_g_free)(void *);

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
    fn_begin_add begin_add;
    fn_add_phrase add_phrase;
    fn_end_add end_add;
    fn_g_free g_free;
} s;

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
    printf("== A word=0x%x import=%s input=%s\n", word, yesno(import), input);
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
    printf("== C%s word=0x%x import=%s input=%s\n", nbest_choose ? "2" : "1", word,
           yesno(import), input);
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
    s.begin_add = (fn_begin_add)must_prefixed("begin_add_phrases");
    s.add_phrase = (fn_add_phrase)must_prefixed("iterator_add_phrase");
    s.end_add = (fn_end_add)must_prefixed("end_add_phrases");
    if (zhuyin) {
        s.parse = (fn_parse)must("zhuyin_parse_more_chewings");
        s.zhuyin_get_sentence = (fn_zhuyin_get_sentence)must("zhuyin_get_sentence");
        s.zhuyin_guess_cands =
            (fn_zhuyin_guess_cands)must("zhuyin_guess_candidates_after_cursor");
    } else {
        s.parse = (fn_parse)must("pinyin_parse_more_full_pinyins");
        s.pinyin_get_sentence = (fn_pinyin_get_sentence)must("pinyin_get_sentence");
        s.pinyin_guess_cands = (fn_pinyin_guess_cands)must("pinyin_guess_candidates");
    }
    s.g_free = (fn_g_free)dlsym(RTLD_DEFAULT, "g_free");
    if (!s.g_free)
        s.g_free = (fn_g_free)must("g_free");
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
        }
    }
    return 0;
}
