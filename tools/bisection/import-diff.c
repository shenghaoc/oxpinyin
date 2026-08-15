/*
 * import-diff.c — W7-T1 user-import differential driver.
 *
 * Drives an identical scripted C-ABI import sequence into a pinyin shared
 * object (libpinyin_capi.so or the pinned libpinyin.so), then exports the
 * user phrase library through the W6-T7 export iterator and prints the
 * (phrase, pinyin, count) triples as sortable lines. Run against both
 * libraries and diff the logs; the comparison is exact-integer equality.
 *
 * Scripted sequence (user-store.md §3.3 / §9):
 *   1. a batch of unique phrases;
 *   2. the same phrases again — the raw ABI count is an ADD amount
 *      (pinyin.cpp:574-583, phrase_index.cpp:55-88), so both engines must
 *      accumulate, not replace;
 *   3. a phrase whose pronunciation already exists in the system
 *      dictionary (the user sub-index still gets its own token);
 *   4. an empty batch (_begin -> _end with no add);
 *   5. a phrase whose pinyin parses no complete keys — must return false
 *      and leave no row behind.
 *   save once, then export once.
 *
 * The CLI's idempotency layer (desired-state deltas) lives in
 * pinyin-dictool; this driver deliberately exercises the raw ABI so the
 * two libraries' per-phrase semantics are what is compared.
 *
 * Usage:
 *   ./import-diff <path-to-so> <systemdir>
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

/* ── Opaque handle types (match pinyin.h) ─────────────────────────────── */

typedef void pinyin_context_t;
typedef void import_iterator_t;
typedef void export_iterator_t;

/* ── Scalar types (match pinyin.h / glib) ─────────────────────────────── */

typedef uint32_t guint;
typedef int32_t gint;
typedef char gchar;

/* ── Function pointer types ───────────────────────────────────────────── */

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

struct syms {
    fn_init init;
    fn_fini fini;
    fn_save save;
    fn_begin_add begin_add;
    fn_iterator_add iterator_add;
    fn_end_add end_add;
    fn_begin_get begin_get;
    fn_has_next has_next;
    fn_get_next get_next;
    fn_end_get end_get;
};

static void *load_symbol(const char *name, void *handle) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "  MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

static void resolve_all(void *handle, struct syms *s) {
    s->init = (fn_init)load_symbol("pinyin_init", handle);
    s->fini = (fn_fini)load_symbol("pinyin_fini", handle);
    s->save = (fn_save)load_symbol("pinyin_save", handle);
    s->begin_add = (fn_begin_add)load_symbol("pinyin_begin_add_phrases", handle);
    s->iterator_add = (fn_iterator_add)load_symbol("pinyin_iterator_add_phrase", handle);
    s->end_add = (fn_end_add)load_symbol("pinyin_end_add_phrases", handle);
    s->begin_get = (fn_begin_get)load_symbol("pinyin_begin_get_phrases", handle);
    s->has_next = (fn_has_next)load_symbol("pinyin_iterator_has_next_phrase", handle);
    s->get_next = (fn_get_next)load_symbol("pinyin_iterator_get_next_phrase", handle);
    s->end_get = (fn_end_get)load_symbol("pinyin_end_get_phrases", handle);
}

/* ── Caller-owned string release ──────────────────────────────────────── */

static fn_g_free g_free_fn;

static void resolve_g_free(void) {
    g_free_fn = (fn_g_free)free;
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib) {
        fn_g_free symbol = (fn_g_free)dlsym(glib, "g_free");
        if (symbol)
            g_free_fn = symbol;
    }
}

/* One (phrase, pinyin, count) add. */
static void add_one(const struct syms *s, import_iterator_t *iter,
                    const char *phrase, const char *pinyin, gint count,
                    const char *label) {
    bool ok = s->iterator_add(iter, phrase, pinyin, count);
    printf("add %s: %s\n", label, ok ? "true" : "false");
}

/* ── Export dumping ───────────────────────────────────────────────────── */

static void print_phrase_rows(const struct syms *s, pinyin_context_t *ctx) {
    export_iterator_t *iter = s->begin_get(ctx, 7 /* USER_DICTIONARY */);
    if (!iter) {
        printf("phrase: BEGIN-NULL\n");
        return;
    }
    while (s->has_next(iter)) {
        gchar *phrase = NULL;
        gchar *pinyin = NULL;
        gint count = -1;
        if (!s->get_next(iter, &phrase, &pinyin, &count)) {
            printf("phrase: GET-FAILED\n");
            break;
        }
        printf("phrase: %s|%s|%d\n", phrase ? phrase : "(null)",
               pinyin ? pinyin : "(null)", (int)count);
        g_free_fn(phrase);
        g_free_fn(pinyin);
    }
    s->end_get(iter);
}

/* ── Main ─────────────────────────────────────────────────────────────── */

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "Usage: %s <path-to-so> <systemdir>\n", argv[0]);
        return 1;
    }

    void *handle = dlopen(argv[1], RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    struct syms s;
    memset(&s, 0, sizeof(s));
    resolve_all(handle, &s);
    resolve_g_free();

    char userdir[] = "/tmp/importdiff-user-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }

    pinyin_context_t *ctx = s.init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed\n");
        return 1;
    }

    /* 1. Unique phrases. */
    import_iterator_t *batch = s.begin_add(ctx, 7);
    if (!batch) {
        fprintf(stderr, "pinyin_begin_add_phrases returned NULL\n");
        return 1;
    }
    add_one(&s, batch, "你好", "nihao", 1, "unique-nihao");
    /* FullPinyinParser2 takes the longest parsed prefix and ignores the
     * trailing unparsed bytes; both engines must still store ni'hao. */
    add_one(&s, batch, "你好", "nihaoXYZ", 2, "unique-nihao-trailing");
    add_one(&s, batch, "世界", "shi'jie", 2, "unique-shijie");
    add_one(&s, batch, "中国", "zhong'guo", 3, "unique-zhongguo");
    s.end_add(batch);

    /* 2. Re-import the same (phrase, reading) pairs. */
    batch = s.begin_add(ctx, 7);
    if (!batch) {
        fprintf(stderr, "second begin returned NULL\n");
        return 1;
    }
    add_one(&s, batch, "你好", "nihao", 4, "reimport-nihao");
    add_one(&s, batch, "世界", "shi'jie", 5, "reimport-shijie");
    add_one(&s, batch, "中国", "zhong'guo", 6, "reimport-zhongguo");
    s.end_add(batch);

    /* 3. A pronunciation already present in the system dictionary still
     * allocates a USER_DICTIONARY token (pinyin.cpp:563-608). */
    batch = s.begin_add(ctx, 7);
    if (!batch) {
        fprintf(stderr, "third begin returned NULL\n");
        return 1;
    }
    add_one(&s, batch, "美国", "mei'guo", 4, "system-meiguo");
    s.end_add(batch);

    /* 4. Empty batch. */
    batch = s.begin_add(ctx, 7);
    if (!batch) {
        fprintf(stderr, "empty begin returned NULL\n");
        return 1;
    }
    s.end_add(batch);
    printf("empty batch: ok\n");

    /* 5. Pinyin with no complete keys must fail and leave no row. */
    batch = s.begin_add(ctx, 7);
    if (!batch) {
        fprintf(stderr, "invalid begin returned NULL\n");
        return 1;
    }
    add_one(&s, batch, "词", "!!!", 1, "invalid-bang");
    add_one(&s, batch, "词", "n", 1, "invalid-incomplete");
    s.end_add(batch);

    printf("save: %s\n", s.save(ctx) ? "true" : "false");
    print_phrase_rows(&s, ctx);

    s.fini(ctx);
    dlclose(handle);
    rmdir(userdir); /* best-effort: non-empty on oracle success */
    return 0;
}
