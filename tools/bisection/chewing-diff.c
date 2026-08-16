/*
 * chewing-diff.c — W13 STANDARD bopomofo differential driver.
 *
 * Drives the chewing entry points over a fixed STANDARD-keyboard input
 * sequence: parse length, parsed-input length, keyboard symbol lookups,
 * candidate list, and auxiliary text at every cursor.  Run against
 * libpinyin.so and libpinyin_capi.so and diff the logs.
 *
 * Usage:
 *   ./chewing-diff <path-to-so> <systemdir>
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef void pinyin_context_t;
typedef void pinyin_instance_t;
typedef void lookup_candidate_t;
typedef char gchar;

typedef uint32_t pinyin_option_t;
typedef uint32_t guint;
typedef int32_t gint;

#define IS_ZHUYIN         (1u << 2)
#define ZHUYIN_INCOMPLETE (1u << 4)
#define USE_TONE          (1u << 5)
#define CHEWING_FLAGS                                                   \
    ((pinyin_option_t)(IS_ZHUYIN | ZHUYIN_INCOMPLETE | USE_TONE))
#define DEFAULT_SORT ((guint)0x1e)

typedef enum {
    NBEST_MATCH_CANDIDATE = 1,
    NORMAL_CANDIDATE = 2,
} lookup_candidate_type_t;

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_instance)(pinyin_instance_t *);
typedef bool (*fn_set_options)(pinyin_context_t *, pinyin_option_t);
typedef bool (*fn_set_zhuyin_scheme)(pinyin_context_t *, int);
typedef size_t (*fn_parse_chewing)(pinyin_instance_t *, const char *);
typedef size_t (*fn_parsed_len)(pinyin_instance_t *);
typedef bool (*fn_in_chewing_keyboard)(pinyin_instance_t *, char, gchar ***);
typedef bool (*fn_guess_sentence)(pinyin_instance_t *);
typedef bool (*fn_get_sentence)(pinyin_instance_t *, uint8_t, char **);
typedef bool (*fn_guess_candidates)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_get_n_candidate)(pinyin_instance_t *, guint *);
typedef bool (*fn_get_candidate)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_get_candidate_type)(pinyin_instance_t *, lookup_candidate_t *, lookup_candidate_type_t *);
typedef bool (*fn_get_candidate_string)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_get_chewing_aux)(pinyin_instance_t *, size_t, gchar **);
typedef bool (*fn_reset)(pinyin_instance_t *);

struct symbols {
    fn_init init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_instance free_instance;
    fn_set_options set_options;
    fn_set_zhuyin_scheme set_zhuyin_scheme;
    fn_parse_chewing parse_chewing;
    fn_parsed_len parsed_len;
    fn_in_chewing_keyboard in_chewing_keyboard;
    fn_guess_sentence guess_sentence;
    fn_get_sentence get_sentence;
    fn_guess_candidates guess_candidates;
    fn_get_n_candidate get_n_candidate;
    fn_get_candidate get_candidate;
    fn_get_candidate_type get_candidate_type;
    fn_get_candidate_string get_candidate_string;
    fn_get_chewing_aux get_chewing_aux;
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
    s->init = (fn_init)resolve_symbol(handle, "pinyin_init", &missing);
    {
        fn_init fixture_init =
            (fn_init)dlsym(handle, "oxpinyin_init_for_fixtures");
        if (fixture_init)
            s->init = fixture_init;
    }
    s->fini = (fn_fini)resolve_symbol(handle, "pinyin_fini", &missing);
    s->alloc = (fn_alloc)resolve_symbol(handle, "pinyin_alloc_instance", &missing);
    s->free_instance = (fn_free_instance)resolve_symbol(handle, "pinyin_free_instance", &missing);
    s->set_options = (fn_set_options)resolve_symbol(handle, "pinyin_set_options", &missing);
    s->set_zhuyin_scheme = (fn_set_zhuyin_scheme)resolve_symbol(handle, "pinyin_set_zhuyin_scheme", &missing);
    s->parse_chewing = (fn_parse_chewing)resolve_symbol(handle, "pinyin_parse_more_chewings", &missing);
    s->parsed_len = (fn_parsed_len)resolve_symbol(handle, "pinyin_get_parsed_input_length", &missing);
    s->in_chewing_keyboard = (fn_in_chewing_keyboard)resolve_symbol(handle, "pinyin_in_chewing_keyboard", &missing);
    s->guess_sentence = (fn_guess_sentence)resolve_symbol(handle, "pinyin_guess_sentence", &missing);
    s->get_sentence = (fn_get_sentence)resolve_symbol(handle, "pinyin_get_sentence", &missing);
    s->guess_candidates = (fn_guess_candidates)resolve_symbol(handle, "pinyin_guess_candidates", &missing);
    s->get_n_candidate = (fn_get_n_candidate)resolve_symbol(handle, "pinyin_get_n_candidate", &missing);
    s->get_candidate = (fn_get_candidate)resolve_symbol(handle, "pinyin_get_candidate", &missing);
    s->get_candidate_type = (fn_get_candidate_type)resolve_symbol(handle, "pinyin_get_candidate_type", &missing);
    s->get_candidate_string = (fn_get_candidate_string)resolve_symbol(handle, "pinyin_get_candidate_string", &missing);
    s->get_chewing_aux = (fn_get_chewing_aux)resolve_symbol(handle, "pinyin_get_chewing_auxiliary_text", &missing);
    s->reset = (fn_reset)resolve_symbol(handle, "pinyin_reset", &missing);
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
    return t == NBEST_MATCH_CANDIDATE ? "NBEST_MATCH" : "NORMAL";
}

static const char *TEST_INPUTS[] = {
    "su",
    "cl",
    "ji",
    "bp",
    "5j/",
    "g",
    "u",
    "su6",
    "x",
    "",
};
static const size_t N_INPUTS = sizeof(TEST_INPUTS) / sizeof(TEST_INPUTS[0]);

static void drive_input(const struct symbols *s, pinyin_instance_t *inst,
                        const char *input) {
    printf("=== input: \"%s\" ===\n", input);

    for (const unsigned char *p = (const unsigned char *)input; *p; p++) {
        gchar **symbols = NULL;
        bool ok = s->in_chewing_keyboard(inst, (char)*p, &symbols);
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

    size_t consumed = s->parse_chewing(inst, input);
    printf("parse_chewing: consumed=%zu\n", consumed);
    printf("parsed_input_length: %zu\n", s->parsed_len(inst));

    bool gs = s->guess_sentence(inst);
    printf("guess_sentence: %s\n", gs ? "true" : "false");

    if (s->get_sentence) {
        char *sentence = NULL;
        bool ok = s->get_sentence(inst, 0, &sentence);
        printf("get_sentence: %s text=\"%s\"\n",
               ok ? "true" : "false", sentence ? sentence : "(null)");
        if (sentence)
            g_free_fn(sentence);
    }

    bool gc = s->guess_candidates(inst, 0, DEFAULT_SORT);
    printf("guess_candidates: %s\n", gc ? "true" : "false");

    guint n = 0;
    if (s->get_n_candidate)
        s->get_n_candidate(inst, &n);
    printf("n_candidates: %u\n", n);

    guint limit = n < 12 ? n : 12;
    for (guint i = 0; i < limit; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->get_candidate(inst, i, &cand) || !cand) {
            printf("  candidate[%u]: FAILED\n", i);
            continue;
        }
        const gchar *text = NULL;
        s->get_candidate_string(inst, cand, &text);
        lookup_candidate_type_t ctype = NORMAL_CANDIDATE;
        s->get_candidate_type(inst, cand, &ctype);
        printf("  candidate[%u]: type=%s text=\"%s\"\n",
               i, ctype_name(ctype), text ? text : "(null)");
    }

    for (size_t cursor = 0; cursor <= consumed; cursor++) {
        gchar *aux = NULL;
        bool ok = s->get_chewing_aux(inst, cursor, &aux);
        printf("chewing_aux(%zu): %s text=\"%s\"\n",
               cursor, ok ? "true" : "false", aux ? aux : "(null)");
        if (aux)
            g_free_fn(aux);
    }

    s->reset(inst);
    printf("reset_parsed_len: %zu\n", s->parsed_len(inst));
    printf("\n");
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: %s <so> <systemdir>\n", argv[0]);
        return 1;
    }

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

    const char *user_dir = getenv("SCHEME_DIFF_USER_DIR");
    pinyin_context_t *ctx = s.init(argv[2], user_dir ? user_dir : "");
    if (!ctx) {
        dlclose(handle);
        return 1;
    }
    s.set_options(ctx, CHEWING_FLAGS);
    printf("set_zhuyin_scheme(1): %s\n",
           s.set_zhuyin_scheme(ctx, 1) ? "true" : "false");

    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        s.fini(ctx);
        dlclose(handle);
        return 1;
    }

    for (size_t i = 0; i < N_INPUTS; i++)
        drive_input(&s, inst, TEST_INPUTS[i]);

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    return 0;
}
