/*
 * export-diff.c — phrase-export differential driver (issue #533).
 *
 * Opens a pinyin shared object (libpinyin_capi.so or the pinned
 * libpinyin.so) on <systemdir> with a fresh user directory, then walks
 * pinyin_begin_get_phrases(ctx, index) for every library the pin can
 * address safely and prints each yielded row in iteration order:
 *
 *   row <requested-index> <phrase>\t<pinyin>\t<count>
 *   end <requested-index> rows=<n>
 *
 * Order is part of the surface (pinyin.cpp:662-768 walks the sub-index's
 * token range ascending, pronunciations in stored order), so the logs are
 * diffed unsorted.
 *
 * Index set:
 *   - 0..15, every nibble FacadePhraseIndex holds a slot for (0 and 8..15
 *     are empty in the default facade, 1..4 SYSTEM_FILE, 5..7 USER_FILE);
 *   - 257 and 263: export_iterator_t::m_phrase_index is a guint8
 *     (pinyin.cpp:126), so the guint argument truncates to 1 and 7;
 *   - after pinyin_unload_phrase_library(ctx, 2) (phrase_index.cpp:260-268),
 *     library 2 again: an unloaded sub-index has no range.
 * 16..255 are not driven: the pin indexes m_sub_phrase_indices[16] out of
 * bounds (phrase_index.cpp:611), undefined behaviour oxpinyin does not
 * reproduce (class (b)).
 *
 * One user phrase is imported into USER_DICTIONARY first so library 7 has a
 * row on both sides.
 *
 * Usage:
 *   ./export-diff <path-to-so> <systemdir>
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>

typedef void pinyin_context_t;
typedef void import_iterator_t;
typedef void export_iterator_t;
typedef uint32_t guint;
typedef int32_t gint;
typedef char gchar;

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef import_iterator_t *(*fn_begin_add)(pinyin_context_t *, uint8_t);
typedef bool (*fn_iterator_add)(import_iterator_t *, const char *, const char *, gint);
typedef void (*fn_end_add)(import_iterator_t *);
typedef export_iterator_t *(*fn_begin_get)(pinyin_context_t *, guint);
typedef bool (*fn_has_next)(export_iterator_t *);
typedef bool (*fn_get_next)(export_iterator_t *, gchar **, gchar **, gint *);
typedef void (*fn_end_get)(export_iterator_t *);
typedef bool (*fn_unload)(pinyin_context_t *, uint8_t);
typedef void (*fn_g_free)(void *);

static fn_begin_get begin_get;
static fn_has_next has_next;
static fn_get_next get_next;
static fn_end_get end_get;
static fn_g_free g_free_fn;

static void *load_symbol(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

static void dump(pinyin_context_t *ctx, guint index) {
    export_iterator_t *iter = begin_get(ctx, index);
    if (!iter) {
        printf("begin-null %u\n", index);
        return;
    }
    unsigned long n = 0;
    while (has_next(iter)) {
        gchar *phrase = NULL;
        gchar *pinyin = NULL;
        gint count = 0;
        bool ok = get_next(iter, &phrase, &pinyin, &count);
        printf("row %u %s\t%s\t%d%s\n", index, phrase ? phrase : "(null)",
               pinyin ? pinyin : "(null)", count, ok ? "" : "\tget=false");
        g_free_fn(phrase);
        g_free_fn(pinyin);
        ++n;
        if (!ok)
            break;
    }
    end_get(iter);
    printf("end %u rows=%lu\n", index, n);
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s <so> <systemdir>\n", argv[0]);
        return 2;
    }
    void *handle = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    g_free_fn = (fn_g_free)free;
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib && dlsym(glib, "g_free"))
        g_free_fn = (fn_g_free)dlsym(glib, "g_free");

    fn_init init = (fn_init)load_symbol(handle, "pinyin_init");
    fn_fini fini = (fn_fini)load_symbol(handle, "pinyin_fini");
    fn_begin_add begin_add = (fn_begin_add)load_symbol(handle, "pinyin_begin_add_phrases");
    fn_iterator_add iterator_add = (fn_iterator_add)load_symbol(handle, "pinyin_iterator_add_phrase");
    fn_end_add end_add = (fn_end_add)load_symbol(handle, "pinyin_end_add_phrases");
    fn_unload unload = (fn_unload)load_symbol(handle, "pinyin_unload_phrase_library");
    begin_get = (fn_begin_get)load_symbol(handle, "pinyin_begin_get_phrases");
    has_next = (fn_has_next)load_symbol(handle, "pinyin_iterator_has_next_phrase");
    get_next = (fn_get_next)load_symbol(handle, "pinyin_iterator_get_next_phrase");
    end_get = (fn_end_get)load_symbol(handle, "pinyin_end_get_phrases");

    mkdir("user", 0700);
    pinyin_context_t *ctx = init(argv[2], "user");
    if (!ctx) {
        printf("init: NULL\n");
        return 1;
    }

    import_iterator_t *import = begin_add(ctx, 7 /* USER_DICTIONARY */);
    bool added = iterator_add(import, "你好世界", "ni'hao'shi'jie", 5);
    printf("add: %s\n", added ? "true" : "false");
    end_add(import);
    if (!added) {
        fprintf(stderr, "import failed\n");
        fini(ctx);
        return 1;
    }

    for (guint index = 0; index < 16; ++index)
        dump(ctx, index);
    dump(ctx, 257);
    dump(ctx, 263);

    printf("unload 2: %s\n", unload(ctx, 2) ? "true" : "false");
    dump(ctx, 2);

    fini(ctx);
    return 0;
}
