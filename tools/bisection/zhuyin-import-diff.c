/*
 * zhuyin-import-diff.c — zhuyin_iterator_add_phrase differential driver
 * (issue #575).
 *
 * Opens a libzhuyin shared object (libzhuyin_capi.so or the pinned
 * libzhuyin.so) on <systemdir> with a fresh user directory, imports one
 * battery into library <index> through zhuyin_begin_add_phrases /
 * zhuyin_iterator_add_phrase, and prints every add's return value. When
 * <index> addresses a sub-index slot (< 16) it then prints the stored
 * result — for every battery phrase, each token of library <index> that
 * zhuyin_lookup_tokens returns, with its unigram frequency and its
 * pronunciations as packed ChewingKey words in stored order — saves,
 * re-opens the same user dir and prints it again.
 *
 * The pin parses the reading with ZhuyinDirectParser2 under
 * USE_TONE | FORCE_TONE (zhuyin.cpp:515-523): bopomofo keys separated by
 * spaces or apostrophes, tone 1 when unmarked; a romanized reading parses
 * no key. Battery: toned and unmarked bopomofo, the romanized form, an
 * apostrophe separator, a trailing space, the neutral tone, an incomplete
 * key, a length mismatch, and counts -1, 0, -2, INT_MIN, INT_MAX and a
 * second add onto INT_MAX.
 *
 * Usage:
 *   ./zhuyin-import-diff <path-to-so> <systemdir> <index>
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <glib.h>
#include <limits.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>

typedef void zhuyin_context_t;
typedef void zhuyin_instance_t;
typedef void import_iterator_t;

typedef zhuyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(zhuyin_context_t *);
typedef bool (*fn_save)(zhuyin_context_t *);
typedef import_iterator_t *(*fn_begin_add)(zhuyin_context_t *, guint8);
typedef bool (*fn_iterator_add)(import_iterator_t *, const char *, const char *, gint);
typedef void (*fn_end_add)(import_iterator_t *);
typedef zhuyin_instance_t *(*fn_alloc)(zhuyin_context_t *);
typedef void (*fn_free_instance)(zhuyin_instance_t *);
typedef bool (*fn_lookup_tokens)(zhuyin_instance_t *, const char *, GArray *);
typedef bool (*fn_n_pron)(zhuyin_instance_t *, guint32, guint *);
typedef bool (*fn_nth_pron)(zhuyin_instance_t *, guint32, guint, GArray *);
typedef bool (*fn_unigram)(zhuyin_instance_t *, guint32, guint *);

static fn_init init;
static fn_fini fini;
static fn_alloc alloc_instance;
static fn_free_instance free_instance;
static fn_lookup_tokens lookup_tokens;
static fn_n_pron n_pron;
static fn_nth_pron nth_pron;
static fn_unigram unigram;

struct add {
    const char *phrase;
    const char *reading;
    gint count;
};

static const struct add battery[] = {
    {"测侧", "ㄘㄜˋ ㄘㄜˋ", -1},
    {"测侧", "ce'ce", -1},
    {"测侧", "ㄘㄜ ㄘㄜ", 3},
    {"测侧", "ㄘㄜˋ'ㄘㄜˋ", 2},
    {"零次", "ㄌㄧㄥˊ ㄘˋ", 0},
    {"负二", "ㄈㄨˋ ㄦˋ", -2},
    {"最小", "ㄗㄨㄟˋ ㄒㄧㄠˇ", INT_MIN},
    {"最大", "ㄗㄨㄟˋ ㄉㄚˋ", INT_MAX},
    {"最大", "ㄗㄨㄟˋ ㄉㄚˋ", 1},
    {"你好", "ㄋㄧˇ ㄏㄠˇ ", 5},
    {"轻声", "ㄑㄧㄥ ㄕㄥ˙", 5},
    {"错序", "ㄘㄨㄛˋ ㄒㄩˋ", 5},
    {"残缺", "ㄘ ㄑ", 5},
    {"长度", "ㄔㄤˊ", 5},
    {"罗马", "luo'ma", 5},
    {"", "", 5},
};

static const char *phrases[] = {
    "测侧", "零次", "负二", "最小", "最大", "你好", "轻声", "错序", "残缺", "长度", "罗马",
};

static void *load_symbol(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

static void dump(const char *label, zhuyin_context_t *ctx, guint index) {
    zhuyin_instance_t *instance = alloc_instance(ctx);
    GArray *tokens = g_array_new(FALSE, TRUE, sizeof(guint32));
    GArray *keys = g_array_new(FALSE, TRUE, sizeof(guint16));
    for (size_t i = 0; i < sizeof phrases / sizeof *phrases; ++i) {
        g_array_set_size(tokens, 0);
        lookup_tokens(instance, phrases[i], tokens);
        for (guint t = 0; t < tokens->len; ++t) {
            guint32 token = g_array_index(tokens, guint32, t);
            if ((token >> 24) != index)
                continue;
            guint freq = 0, n = 0;
            unigram(instance, token, &freq);
            n_pron(instance, token, &n);
            printf("%s %s token=%#x unigram=%u prons=%u", label, phrases[i], token, freq, n);
            for (guint k = 0; k < n; ++k) {
                g_array_set_size(keys, 0);
                bool ok = nth_pron(instance, token, k, keys);
                printf(" [");
                for (guint j = 0; ok && j < keys->len; ++j)
                    printf("%s%04x", j ? " " : "", g_array_index(keys, guint16, j));
                printf("%s]", ok ? "" : "get=false");
            }
            printf("\n");
        }
    }
    g_array_free(keys, TRUE);
    g_array_free(tokens, TRUE);
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
    init = (fn_init)load_symbol(handle, "zhuyin_init");
    fini = (fn_fini)load_symbol(handle, "zhuyin_fini");
    fn_save save = (fn_save)load_symbol(handle, "zhuyin_save");
    fn_begin_add begin_add = (fn_begin_add)load_symbol(handle, "zhuyin_begin_add_phrases");
    fn_iterator_add iterator_add = (fn_iterator_add)load_symbol(handle, "zhuyin_iterator_add_phrase");
    fn_end_add end_add = (fn_end_add)load_symbol(handle, "zhuyin_end_add_phrases");
    alloc_instance = (fn_alloc)load_symbol(handle, "zhuyin_alloc_instance");
    free_instance = (fn_free_instance)load_symbol(handle, "zhuyin_free_instance");
    lookup_tokens = (fn_lookup_tokens)load_symbol(handle, "zhuyin_lookup_tokens");
    n_pron = (fn_n_pron)load_symbol(handle, "zhuyin_token_get_n_pronunciation");
    nth_pron = (fn_nth_pron)load_symbol(handle, "zhuyin_token_get_nth_pronunciation");
    unigram = (fn_unigram)load_symbol(handle, "zhuyin_token_get_unigram_frequency");

    mkdir("user", 0700);
    zhuyin_context_t *ctx = init(argv[2], "user");
    if (!ctx) {
        printf("init: NULL\n");
        return 1;
    }

    import_iterator_t *import = begin_add(ctx, (guint8)index);
    for (size_t i = 0; i < sizeof battery / sizeof *battery; ++i) {
        const struct add *a = &battery[i];
        bool ok = iterator_add(import, a->phrase, a->reading, a->count);
        printf("add %zu %s|%s|%d: %s\n", i, a->phrase, a->reading, a->count, ok ? "true" : "false");
        fflush(stdout);
    }
    end_add(import);

    if (index >= 16) {
        fini(ctx);
        return 0;
    }
    dump("live", ctx, index);
    printf("save: %s\n", save(ctx) ? "true" : "false");
    fini(ctx);

    ctx = init(argv[2], "user");
    if (!ctx) {
        printf("reinit: NULL\n");
        return 1;
    }
    dump("reopened", ctx, index);
    fini(ctx);
    return 0;
}
