/*
 * import-semantics-diff.c — pinyin_iterator_add_phrase semantics
 * differential driver (issue #534).
 *
 * Opens a pinyin shared object (libpinyin_capi.so or the pinned
 * libpinyin.so) on <systemdir> with a fresh user directory, imports one
 * battery into library <index> through pinyin_begin_add_phrases /
 * pinyin_iterator_add_phrase, and prints every add's return value. When
 * <index> addresses a sub-index slot (< 16) it then exports that library
 * (only rows whose phrase belongs to the battery, in iteration order),
 * saves, re-opens the same user dir and exports again: the stored result
 * is the surface, not the return value alone.
 *
 * Battery (pinyin.cpp:515-640): counts -1 (default 5), 0, 1, 5, -2,
 * INT_MIN, INT_MAX and a second add onto INT_MAX; toned
 * (PINYIN_CORRECT_ALL | USE_TONE parses "ce4'shi4") and toneless
 * readings; second readings of an existing phrase (row order); a repeat
 * of an existing reading (accumulation); corrections; a length mismatch.
 *
 * Lookup probe: a reading merged into an existing phrase is never added
 * to the user pinyin index (pinyin.cpp:569-582), so "测验" must not be a
 * candidate for "ceshi" although it carries the reading ce'shi. For
 * "ceshi" and "ceyan" the driver prints, per battery phrase, whether the
 * candidate list offers it — presence, not rank — live and after reopen.
 *
 * Usage:
 *   ./import-semantics-diff <path-to-so> <systemdir> <index>
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <limits.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

typedef void pinyin_context_t;
typedef void import_iterator_t;
typedef void export_iterator_t;
typedef uint32_t guint;
typedef int32_t gint;
typedef char gchar;

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef bool (*fn_save)(pinyin_context_t *);
typedef import_iterator_t *(*fn_begin_add)(pinyin_context_t *, uint8_t);
typedef bool (*fn_iterator_add)(import_iterator_t *, const char *, const char *, gint);
typedef void (*fn_end_add)(import_iterator_t *);
typedef export_iterator_t *(*fn_begin_get)(pinyin_context_t *, guint);
typedef bool (*fn_has_next)(export_iterator_t *);
typedef bool (*fn_get_next)(export_iterator_t *, gchar **, gchar **, gint *);
typedef void (*fn_end_get)(export_iterator_t *);
typedef void (*fn_g_free)(void *);
typedef void *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_instance)(void *);
typedef size_t (*fn_parse)(void *, const char *);
typedef bool (*fn_guess)(void *, size_t, guint);
typedef bool (*fn_getn)(void *, guint *);
typedef bool (*fn_getc)(void *, guint, void **);
typedef bool (*fn_getstr)(void *, void *, const gchar **);

static fn_init init;
static fn_fini fini;
static fn_save save;
static fn_begin_get begin_get;
static fn_has_next has_next;
static fn_get_next get_next;
static fn_end_get end_get;
static fn_g_free g_free_fn;
static fn_alloc alloc_instance;
static fn_free_instance free_instance;
static fn_parse parse_full;
static fn_guess guess_candidates;
static fn_getn get_n;
static fn_getc get_c;
static fn_getstr get_str;

struct add {
    const char *phrase;
    const char *pinyin;
    gint count;
};

static const struct add battery[] = {
    {"测试", "ce4'shi4", 5},
    {"测验", "ce'yan", 5},
    {"测验", "ce4'yan4", 7},
    {"测验", "ce'yan", 3},
    {"测验", "ce'shi", 2},
    {"默认", "mo'ren", -1},
    {"零次", "ling'ci", 0},
    {"一次", "yi'ci", 1},
    {"负二", "fu'er", -2},
    {"最小", "zui'xiao", INT_MIN},
    {"最大", "zui'da", INT_MAX},
    {"最大", "zui'da", 1},
    {"泥壕", "ni2'hao2", 5},
    {"绝句", "jv'ju", 5},
    {"女儿", "nv'er", 5},
    {"亲爱", "qin'ai", 5},
    {"西安", "xi'an", 5},
    {"长度", "chang", 5},
    {"无效", "xyz'abc", 5},
    {"", "", 5},
};

static const char *battery_phrases[] = {
    "测试", "测验", "默认", "零次", "一次", "负二", "最小", "最大",
    "泥壕", "绝句", "女儿", "亲爱", "西安", "长度", "无效",
};

static bool in_battery(const char *phrase) {
    for (size_t i = 0; i < sizeof battery_phrases / sizeof *battery_phrases; ++i)
        if (0 == strcmp(phrase, battery_phrases[i]))
            return true;
    return false;
}

static void *load_symbol(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

static void dump(const char *label, pinyin_context_t *ctx, guint index) {
    export_iterator_t *iter = begin_get(ctx, index);
    unsigned long n = 0;
    while (has_next(iter)) {
        gchar *phrase = NULL;
        gchar *pinyin = NULL;
        gint count = 0;
        bool ok = get_next(iter, &phrase, &pinyin, &count);
        if (phrase && in_battery(phrase))
            printf("%s %s\t%s\t%d\n", label, phrase, pinyin ? pinyin : "(null)", count);
        g_free_fn(phrase);
        g_free_fn(pinyin);
        ++n;
        if (!ok)
            break;
    }
    end_get(iter);
    printf("%s rows=%lu\n", label, n);
}

/* Presence of every battery phrase among the candidates for `pinyin`. */
static void probe(const char *label, pinyin_context_t *ctx, const char *pinyin) {
    void *instance = alloc_instance(ctx);
    parse_full(instance, pinyin);
    guess_candidates(instance, 0, 0x1e /* SORT_BY_PHRASE_LENGTH|PINYIN|FREQUENCY */);
    guint n = 0;
    get_n(instance, &n);
    for (size_t i = 0; i < sizeof battery_phrases / sizeof *battery_phrases; ++i) {
        bool present = false;
        for (guint k = 0; k < n && !present; ++k) {
            void *candidate = NULL;
            const gchar *text = NULL;
            if (get_c(instance, k, &candidate) && get_str(instance, candidate, &text) && text)
                present = 0 == strcmp(text, battery_phrases[i]);
        }
        printf("%s probe %s %s: %s\n", label, pinyin, battery_phrases[i],
               present ? "present" : "absent");
    }
    free_instance(instance);
}

int main(int argc, char **argv) {
    if (argc != 4) {
        fprintf(stderr, "usage: %s <so> <systemdir> <index>\n", argv[0]);
        return 2;
    }
    unsigned index = (unsigned)strtoul(argv[3], NULL, 10);
    void *handle = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    g_free_fn = (fn_g_free)free;
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib && dlsym(glib, "g_free"))
        g_free_fn = (fn_g_free)dlsym(glib, "g_free");

    init = (fn_init)load_symbol(handle, "pinyin_init");
    fini = (fn_fini)load_symbol(handle, "pinyin_fini");
    save = (fn_save)load_symbol(handle, "pinyin_save");
    fn_begin_add begin_add = (fn_begin_add)load_symbol(handle, "pinyin_begin_add_phrases");
    fn_iterator_add iterator_add = (fn_iterator_add)load_symbol(handle, "pinyin_iterator_add_phrase");
    fn_end_add end_add = (fn_end_add)load_symbol(handle, "pinyin_end_add_phrases");
    begin_get = (fn_begin_get)load_symbol(handle, "pinyin_begin_get_phrases");
    has_next = (fn_has_next)load_symbol(handle, "pinyin_iterator_has_next_phrase");
    get_next = (fn_get_next)load_symbol(handle, "pinyin_iterator_get_next_phrase");
    end_get = (fn_end_get)load_symbol(handle, "pinyin_end_get_phrases");
    alloc_instance = (fn_alloc)load_symbol(handle, "pinyin_alloc_instance");
    free_instance = (fn_free_instance)load_symbol(handle, "pinyin_free_instance");
    parse_full = (fn_parse)load_symbol(handle, "pinyin_parse_more_full_pinyins");
    guess_candidates = (fn_guess)load_symbol(handle, "pinyin_guess_candidates");
    get_n = (fn_getn)load_symbol(handle, "pinyin_get_n_candidate");
    get_c = (fn_getc)load_symbol(handle, "pinyin_get_candidate");
    get_str = (fn_getstr)load_symbol(handle, "pinyin_get_candidate_string");

    mkdir("user", 0700);
    pinyin_context_t *ctx = init(argv[2], "user");
    if (!ctx) {
        printf("init: NULL\n");
        return 1;
    }

    import_iterator_t *import = begin_add(ctx, (uint8_t)index);
    for (size_t i = 0; i < sizeof battery / sizeof *battery; ++i) {
        const struct add *a = &battery[i];
        bool ok = iterator_add(import, a->phrase, a->pinyin, a->count);
        printf("add %zu %s %s %d: %s\n", i, a->phrase, a->pinyin, a->count, ok ? "true" : "false");
        fflush(stdout);
    }
    end_add(import);

    if (index >= 16) {
        fini(ctx);
        return 0;
    }
    dump("live", ctx, index);
    probe("live", ctx, "ceshi");
    probe("live", ctx, "ceyan");
    printf("save: %s\n", save(ctx) ? "true" : "false");
    fini(ctx);

    ctx = init(argv[2], "user");
    if (!ctx) {
        printf("reinit: NULL\n");
        return 1;
    }
    dump("reopened", ctx, index);
    probe("reopened", ctx, "ceshi");
    probe("reopened", ctx, "ceyan");
    fini(ctx);
    return 0;
}
