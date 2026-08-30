/*
 * zhuyin-diff.c — libzhuyin differential driver.
 *
 * Drives the zhuyin facade's core entry points over a fixed STANDARD-keyboard
 * input corpus: parse length, parsed-input length, keyboard symbol lookups,
 * candidate list with the zhuyin 4-value candidate-type tag, and the
 * key-rest span getters.  Run against the pinned libzhuyin.so (oracle) and
 * the oxpinyin libzhuyin.so.15 (the Rust facade) and diff the logs.
 *
 * Usage:
 *   ./zhuyin-diff <path-to-libzhuyin.so> <systemdir> [userdir]
 *
 * The zhuyin facade exports `zhuyin_init` / `zhuyin_parse_more_chewings` /
 * `zhuyin_in_chewing_keyboard` / `zhuyin_get_candidate_type` — the last of
 * which reads the 4-value enum (BEST_MATCH_CANDIDATE=1,
 * NORMAL_CANDIDATE_AFTER_CURSOR=2, NORMAL_CANDIDATE_BEFORE_CURSOR=3,
 * ZOMBIE_CANDIDATE=4), the zhuyin-local type whose discriminants collide with
 * the pinyin eight.  The corpus is small but deterministically derived from
 * the pinned STANDARD keyboard; every input exercises a distinct observable.
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef void zhuyin_context_t;
typedef void zhuyin_instance_t;
typedef void lookup_candidate_t;
typedef char gchar;

typedef uint32_t pinyin_option_t;
typedef uint32_t guint;
typedef int32_t gint;

#define ZHUYIN_INCOMPLETE (1u << 4)
#define USE_TONE          (1u << 5)
#define FORCE_TONE        (1u << 6)
/* The pin's default option word is USE_TONE | FORCE_TONE with no
 * ZHUYIN_INCOMPLETE (zhuyin.cpp:273). Test that default so the two sides are
 * compared like-for-like; ZHUYIN_INCOMPLETE is remasked separately. */
#define ZHUYIN_FLAGS \
    ((pinyin_option_t)(USE_TONE | FORCE_TONE))

typedef enum {
    BEST_MATCH_CANDIDATE = 1,
    NORMAL_CANDIDATE_AFTER_CURSOR = 2,
    NORMAL_CANDIDATE_BEFORE_CURSOR = 3,
    ZOMBIE_CANDIDATE = 4,
} lookup_candidate_type_t;

typedef zhuyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(zhuyin_context_t *);
typedef zhuyin_instance_t *(*fn_alloc)(zhuyin_context_t *);
typedef void (*fn_free_instance)(zhuyin_instance_t *);
typedef bool (*fn_set_options)(zhuyin_context_t *, pinyin_option_t);
typedef bool (*fn_set_chewing_scheme)(zhuyin_context_t *, int);
typedef size_t (*fn_parse_chewing)(zhuyin_instance_t *, const char *);
typedef size_t (*fn_parsed_len)(zhuyin_instance_t *);
typedef bool (*fn_in_chewing_keyboard)(zhuyin_instance_t *, char, gchar ***);
typedef bool (*fn_guess_sentence)(zhuyin_instance_t *);
typedef bool (*fn_get_sentence)(zhuyin_instance_t *, char **);
typedef bool (*fn_guess_after)(zhuyin_instance_t *, size_t);
typedef bool (*fn_guess_before)(zhuyin_instance_t *, size_t);
typedef bool (*fn_get_n_candidate)(zhuyin_instance_t *, guint *);
typedef bool (*fn_get_candidate)(zhuyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_get_candidate_type)(zhuyin_instance_t *, lookup_candidate_t *, lookup_candidate_type_t *);
typedef bool (*fn_get_candidate_string)(zhuyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_reset)(zhuyin_instance_t *);

struct symbols {
    fn_init init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_instance free_instance;
    fn_set_options set_options;
    fn_set_chewing_scheme set_chewing_scheme;
    fn_parse_chewing parse_chewing;
    fn_parsed_len parsed_len;
    fn_in_chewing_keyboard in_chewing_keyboard;
    fn_guess_sentence guess_sentence;
    fn_get_sentence get_sentence;
    fn_guess_after guess_after;
    fn_guess_before guess_before;
    fn_get_n_candidate get_n_candidate;
    fn_get_candidate get_candidate;
    fn_get_candidate_type get_candidate_type;
    fn_get_candidate_string get_candidate_string;
    fn_reset reset;
};

static void *resolve_symbol(void *handle, const char *name, int *missing) {
    void *sym = dlsym(handle, name);
    if (!sym) {
        fprintf(stderr, "  MISSING: %s\n", name);
        (*missing)++;
    }
    return sym;
}

static int resolve_all(void *handle, struct symbols *s) {
    int missing = 0;
    s->init = (fn_init)resolve_symbol(handle, "zhuyin_init", &missing);
    s->fini = (fn_fini)resolve_symbol(handle, "zhuyin_fini", &missing);
    s->alloc = (fn_alloc)resolve_symbol(handle, "zhuyin_alloc_instance", &missing);
    s->free_instance = (fn_free_instance)resolve_symbol(handle, "zhuyin_free_instance", &missing);
    s->set_options = (fn_set_options)resolve_symbol(handle, "zhuyin_set_options", &missing);
    s->set_chewing_scheme = (fn_set_chewing_scheme)resolve_symbol(handle, "zhuyin_set_chewing_scheme", &missing);
    s->parse_chewing = (fn_parse_chewing)resolve_symbol(handle, "zhuyin_parse_more_chewings", &missing);
    s->parsed_len = (fn_parsed_len)resolve_symbol(handle, "zhuyin_get_parsed_input_length", &missing);
    s->in_chewing_keyboard = (fn_in_chewing_keyboard)resolve_symbol(handle, "zhuyin_in_chewing_keyboard", &missing);
    s->guess_sentence = (fn_guess_sentence)resolve_symbol(handle, "zhuyin_guess_sentence", &missing);
    s->get_sentence = (fn_get_sentence)resolve_symbol(handle, "zhuyin_get_sentence", &missing);
    s->guess_after = (fn_guess_after)resolve_symbol(handle, "zhuyin_guess_candidates_after_cursor", &missing);
    s->guess_before = (fn_guess_before)resolve_symbol(handle, "zhuyin_guess_candidates_before_cursor", &missing);
    s->get_n_candidate = (fn_get_n_candidate)resolve_symbol(handle, "zhuyin_get_n_candidate", &missing);
    s->get_candidate = (fn_get_candidate)resolve_symbol(handle, "zhuyin_get_candidate", &missing);
    s->get_candidate_type = (fn_get_candidate_type)resolve_symbol(handle, "zhuyin_get_candidate_type", &missing);
    s->get_candidate_string = (fn_get_candidate_string)resolve_symbol(handle, "zhuyin_get_candidate_string", &missing);
    s->reset = (fn_reset)resolve_symbol(handle, "zhuyin_reset", &missing);
    return missing;
}

typedef void (*fn_g_free)(void *);
typedef void (*fn_g_strfreev)(gchar **);
static fn_g_free g_free_fn;
static fn_g_strfreev g_strfreev_fn;

static void resolve_g_free(void) {
    g_free_fn = (fn_g_free)free;
    g_strfreev_fn = NULL;
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib) {
        fn_g_free sym = (fn_g_free)dlsym(glib, "g_free");
        if (sym)
            g_free_fn = sym;
        g_strfreev_fn = (fn_g_strfreev)dlsym(glib, "g_strfreev");
    }
}

static void free_strv(gchar **v) {
    if (!v)
        return;
    if (g_strfreev_fn) {
        g_strfreev_fn(v);
        return;
    }
    for (gchar **p = v; *p; p++)
        g_free_fn(*p);
    g_free_fn(v);
}

static const char *ctype_name(lookup_candidate_type_t t) {
    switch (t) {
    case BEST_MATCH_CANDIDATE: return "BEST_MATCH";
    case NORMAL_CANDIDATE_AFTER_CURSOR: return "AFTER";
    case NORMAL_CANDIDATE_BEFORE_CURSOR: return "BEFORE";
    case ZOMBIE_CANDIDATE: return "ZOMBIE";
    default: return "UNKNOWN";
    }
}

/* A small deterministic corpus of keystroke inputs.  Each exercises a
 * distinct observable; the parsed length and candidate rows are what get
 * diffed. */
static const char *SYLLABLE_CORPUS[] = {
    "su3",          /* ㄙㄨˇ */
    "zhang",        /* ㄓㄤ (no tone) -- the W13 live row */
    "zhan",         /* ㄓㄢ */
    "n",            /* initial-only ㄋ */
    "u",            /* middle-only ㄧ */
    "wu",           /* ㄨ */
    "yi",           /* ㄧ */
    "nin",          /* ㄋㄧㄣ */
    "hao",          /* ㄏㄠ */
};

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: %s <libzhuyin.so> <systemdir> [userdir]\n", argv[0]);
        return 1;
    }
    const char *user_dir = argc > 3 ? argv[3] : "";
    resolve_g_free();
    void *handle = dlopen(argv[1], RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    struct symbols s;
    memset(&s, 0, sizeof s);
    if (resolve_all(handle, &s)) {
        dlclose(handle);
        return 1;
    }
    zhuyin_context_t *ctx = s.init(argv[2], user_dir);
    if (!ctx) {
        fprintf(stderr, "zhuyin_init failed\n");
        dlclose(handle);
        return 1;
    }
    s.set_options(ctx, ZHUYIN_FLAGS);
    bool selected = s.set_chewing_scheme(ctx, 1); /* STANDARD */
    printf("set_chewing_scheme(1): %s\n", selected ? "true" : "false");
    if (!selected) {
        fprintf(stderr, "set_chewing_scheme(1) rejected\n");
        s.fini(ctx);
        dlclose(handle);
        return 1;
    }
    zhuyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        s.fini(ctx);
        dlclose(handle);
        return 1;
    }

    for (size_t i = 0; i < sizeof(SYLLABLE_CORPUS) / sizeof(SYLLABLE_CORPUS[0]); i++) {
        const char *input = SYLLABLE_CORPUS[i];
        printf("=== input: \"%s\" ===\n", input);
        for (const unsigned char *p = (const unsigned char *)input; *p; p++) {
            gchar **symbols = NULL;
            bool ok = s.in_chewing_keyboard(inst, (char)*p, &symbols);
            printf("in_chewing_keyboard('%c'): %s symbols=", *p,
                   ok ? "true" : "false");
            if (symbols) {
                printf("[");
                for (gchar **q = symbols; *q; q++) {
                    if (q != symbols)
                        printf(",");
                    printf("%s", *q);
                }
                printf("]");
                free_strv(symbols);
            } else {
                printf("(null)");
            }
            printf("\n");
        }
        size_t consumed = s.parse_chewing(inst, input);
        printf("parse_chewing: consumed=%zu\n", consumed);
        printf("parsed_input_length: %zu\n", s.parsed_len(inst));

        bool gs = s.guess_sentence(inst);
        printf("guess_sentence: %s\n", gs ? "true" : "false");
        if (s.get_sentence) {
            char *sent = NULL;
            bool ok = s.get_sentence(inst, &sent);
            printf("get_sentence: %s text=\"%s\"\n",
                   ok ? "true" : "false", sent ? sent : "(null)");
            if (sent)
                g_free_fn(sent);
        }

        bool ga = s.guess_after(inst, 0);
        printf("guess_after_cursor: %s\n", ga ? "true" : "false");
        guint n = 0;
        if (s.get_n_candidate)
            s.get_n_candidate(inst, &n);
        printf("n_candidates: %u\n", n);
        guint limit = n < 12 ? n : 12;
        for (guint k = 0; k < limit; k++) {
            lookup_candidate_t *cand = NULL;
            if (!s.get_candidate(inst, k, &cand) || !cand) {
                printf("  candidate[%u]: FAILED\n", k);
                continue;
            }
            const gchar *text = NULL;
            s.get_candidate_string(inst, cand, &text);
            lookup_candidate_type_t ctype = NORMAL_CANDIDATE_AFTER_CURSOR;
            s.get_candidate_type(inst, cand, &ctype);
            printf("  candidate[%u]: type=%s text=\"%s\"\n",
                   k, ctype_name(ctype), text ? text : "(null)");
        }

        /* Exercise the before-cursor family on the same parse. */
        bool gb = s.guess_before(inst, consumed != 0 ? consumed : 0);
        printf("guess_before_cursor: %s\n", gb ? "true" : "false");
        s.reset(inst);
        printf("reset_parsed_len: %zu\n", s.parsed_len(inst));
        printf("\n");
    }

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    return 0;
}
