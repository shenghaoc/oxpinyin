/*
 * scheme-diff.c — W13 scheme differential driver.
 *
 * Loads one pinyin shared object and drives the double-pinyin entry points
 * over a fixed scheme-input sequence: parse length, parsed-input length,
 * candidate list, and auxiliary text at every cursor.  Run the same binary
 * against libpinyin.so and libpinyin_capi.so and diff the logs.
 *
 * Usage:
 *   ./scheme-diff <path-to-so> <systemdir> [scheme-number]
 *
 * scheme-number is the DoublePinyinScheme discriminant (default 2 = MS).
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

#define IS_PINYIN         (1u << 1)
#define PINYIN_INCOMPLETE (1u << 3)
#define USE_DIVIDED_TABLE (1u << 7)
#define USE_RESPLIT_TABLE (1u << 8)
#define DEFAULT_FLAGS                                                   \
    ((pinyin_option_t)(IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | \
                       USE_RESPLIT_TABLE))
#define DEFAULT_SORT ((guint)0x1e)

typedef enum {
    NBEST_MATCH_CANDIDATE = 1,
    NORMAL_CANDIDATE = 2,
    ZOMBIE_CANDIDATE = 3,
    PREDICTED_BIGRAM_CANDIDATE = 4,
    PREDICTED_PREFIX_CANDIDATE = 5,
    ADDON_CANDIDATE = 6,
    LONGER_CANDIDATE = 7,
    PREDICTED_PUNCTUATION_CANDIDATE = 8,
} lookup_candidate_type_t;

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_instance)(pinyin_instance_t *);
typedef bool (*fn_set_options)(pinyin_context_t *, pinyin_option_t);
typedef bool (*fn_set_double_scheme)(pinyin_context_t *, int);
typedef size_t (*fn_parse_double)(pinyin_instance_t *, const char *);
typedef size_t (*fn_parsed_len)(pinyin_instance_t *);
typedef bool (*fn_guess_sentence)(pinyin_instance_t *);
typedef bool (*fn_get_sentence)(pinyin_instance_t *, uint8_t, char **);
typedef bool (*fn_guess_candidates)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_get_n_candidate)(pinyin_instance_t *, guint *);
typedef bool (*fn_get_candidate)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_get_candidate_type)(pinyin_instance_t *, lookup_candidate_t *, lookup_candidate_type_t *);
typedef bool (*fn_get_candidate_string)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_get_double_aux)(pinyin_instance_t *, size_t, gchar **);
typedef bool (*fn_reset)(pinyin_instance_t *);

struct symbols {
    fn_init init;
    fn_init fixture_init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_instance free_instance;
    fn_set_options set_options;
    fn_set_double_scheme set_double_scheme;
    fn_parse_double parse_double;
    fn_parsed_len parsed_len;
    fn_guess_sentence guess_sentence;
    fn_get_sentence get_sentence;
    fn_guess_candidates guess_candidates;
    fn_get_n_candidate get_n_candidate;
    fn_get_candidate get_candidate;
    fn_get_candidate_type get_candidate_type;
    fn_get_candidate_string get_candidate_string;
    fn_get_double_aux get_double_aux;
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

/**
 * Resolves the required pinyin library entry points and an optional fixture initializer.
 *
 * @param handle Handle to the loaded shared library.
 * @param s Structure populated with the resolved function pointers.
 * @returns The number of required symbols that could not be resolved.
 */
static int resolve_all(void *handle, struct symbols *s) {
    int missing = 0;

    s->init = (fn_init)resolve_symbol(handle, "pinyin_init", &missing);
    s->fixture_init = (fn_init)dlsym(handle, "oxpinyin_init_for_fixtures");
    s->fini = (fn_fini)resolve_symbol(handle, "pinyin_fini", &missing);
    s->alloc = (fn_alloc)resolve_symbol(handle, "pinyin_alloc_instance", &missing);
    s->free_instance = (fn_free_instance)resolve_symbol(handle, "pinyin_free_instance", &missing);
    s->set_options = (fn_set_options)resolve_symbol(handle, "pinyin_set_options", &missing);
    s->set_double_scheme = (fn_set_double_scheme)resolve_symbol(handle, "pinyin_set_double_pinyin_scheme", &missing);
    s->parse_double = (fn_parse_double)resolve_symbol(handle, "pinyin_parse_more_double_pinyins", &missing);
    s->parsed_len = (fn_parsed_len)resolve_symbol(handle, "pinyin_get_parsed_input_length", &missing);
    s->guess_sentence = (fn_guess_sentence)resolve_symbol(handle, "pinyin_guess_sentence", &missing);
    s->get_sentence = (fn_get_sentence)resolve_symbol(handle, "pinyin_get_sentence", &missing);
    s->guess_candidates = (fn_guess_candidates)resolve_symbol(handle, "pinyin_guess_candidates", &missing);
    s->get_n_candidate = (fn_get_n_candidate)resolve_symbol(handle, "pinyin_get_n_candidate", &missing);
    s->get_candidate = (fn_get_candidate)resolve_symbol(handle, "pinyin_get_candidate", &missing);
    s->get_candidate_type = (fn_get_candidate_type)resolve_symbol(handle, "pinyin_get_candidate_type", &missing);
    s->get_candidate_string = (fn_get_candidate_string)resolve_symbol(handle, "pinyin_get_candidate_string", &missing);
    s->get_double_aux = (fn_get_double_aux)resolve_symbol(handle, "pinyin_get_double_pinyin_auxiliary_text", &missing);
    s->reset = (fn_reset)resolve_symbol(handle, "pinyin_reset", &missing);

    return missing;
}

typedef void (*fn_g_free)(void *);

static fn_g_free g_free_fn;

static void resolve_g_free(void) {
    g_free_fn = (fn_g_free)free;
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib) {
        fn_g_free sym = (fn_g_free)dlsym(glib, "g_free");
        if (sym)
            g_free_fn = sym;
    }
}

static const char *ctype_name(lookup_candidate_type_t t) {
    switch (t) {
    case NBEST_MATCH_CANDIDATE: return "NBEST_MATCH";
    case NORMAL_CANDIDATE: return "NORMAL";
    case ZOMBIE_CANDIDATE: return "ZOMBIE";
    case PREDICTED_BIGRAM_CANDIDATE: return "PREDICTED_BIGRAM";
    case PREDICTED_PREFIX_CANDIDATE: return "PREDICTED_PREFIX";
    case ADDON_CANDIDATE: return "ADDON";
    case LONGER_CANDIDATE: return "LONGER";
    case PREDICTED_PUNCTUATION_CANDIDATE: return "PREDICTED_PUNCT";
    default: return "UNKNOWN";
    }
}

static const char *TEST_INPUTS[] = {
    "ni",
    "nihk",
    "wom",
    "zhrgguor",
    "bj",
    "a",
    "o",
    "ru",
    "lv",
    "nv",
    "er",
    ";",
    "aa",
    "niha",
    "z",
};
static const size_t N_INPUTS = sizeof(TEST_INPUTS) / sizeof(TEST_INPUTS[0]);

/**
 * Drives a pinyin instance with an input string and reports parsing, sentence,
 * candidate, auxiliary-text, and reset results.
 *
 * @param s Resolved pinyin library symbols used to operate the instance.
 * @param inst Pinyin instance to exercise.
 * @param input Input string to process.
 */
static void drive_input(const struct symbols *s, pinyin_instance_t *inst,
                        const char *input) {
    printf("=== input: \"%s\" ===\n", input);

    size_t consumed = s->parse_double(inst, input);
    printf("parse_double: consumed=%zu\n", consumed);
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
        bool ok = s->get_double_aux(inst, cursor, &aux);
        printf("double_aux(%zu): %s text=\"%s\"\n",
               cursor, ok ? "true" : "false", aux ? aux : "(null)");
        if (aux)
            g_free_fn(aux);
    }

    s->reset(inst);
    printf("reset_parsed_len: %zu\n", s->parsed_len(inst));
    printf("\n");
}

/**
 * Determines whether a named file exists in a directory.
 * @param dir Directory containing the file.
 * @param name File name to check.
 * @return 1 if the file exists and can be opened for reading, 0 otherwise.
 */
static int file_exists(const char *dir, const char *name) {
    char path[4096];
    snprintf(path, sizeof(path), "%s/%s", dir, name);
    FILE *file = fopen(path, "rb");
    if (file) {
        fclose(file);
        return 1;
    }
    return 0;
}

/**
 * Runs the differential driver against the specified pinyin shared library.
 *
 * @param argc Number of command-line arguments.
 * @param argv Command-line arguments containing the shared-object path, system directory, and optional scheme number.
 * @return 0 on successful completion, or 1 if argument validation, library loading, symbol resolution, initialization, or instance allocation fails.
 */
int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: %s <so> <systemdir> [scheme]\n", argv[0]);
        return 1;
    }

    int scheme = argc >= 4 ? atoi(argv[3]) : 2;
    resolve_g_free();

    void *handle = dlopen(argv[1], RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }

    struct symbols s;
    memset(&s, 0, sizeof s);
    int missing = resolve_all(handle, &s);
    if (missing) {
        fprintf(stderr, "missing %d symbols\n", missing);
        dlclose(handle);
        return 1;
    }

    const char *user_dir = getenv("SCHEME_DIFF_USER_DIR");
    fn_init init = (s.fixture_init && !file_exists(argv[2], "interpolation2.text"))
        ? s.fixture_init
        : s.init;
    pinyin_context_t *ctx = init(argv[2], user_dir ? user_dir : "");
    if (!ctx) {
        fprintf(stderr, "init failed\n");
        dlclose(handle);
        return 1;
    }
    s.set_options(ctx, DEFAULT_FLAGS);
    bool scheme_ok = s.set_double_scheme(ctx, scheme);
    printf("set_double_pinyin_scheme(%d): %s\n", scheme,
           scheme_ok ? "true" : "false");

    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        fprintf(stderr, "alloc failed\n");
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
