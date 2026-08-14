/*
 * bisect.c — three-subject ABI bisection fixture.
 *
 * Loads a pinyin shared object (libpinyin.so or libpinyin_capi.so) via
 * dlopen, resolves all 50 ABI-subset symbols, drives the full-pinyin
 * keystroke cycle through it, and prints a deterministic log.  Run twice
 * (once per .so) and diff the logs to find behavioural divergence.
 *
 * Usage:
 *   ./bisect <path-to-so> <systemdir>
 *
 * The output is a structured text log of return values, candidate strings,
 * and ownership-contract exercises (caller-owned strings are freed after
 * printing).  Deterministic for the same (so, data) pair.
 *
 * Build:
 *   gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o bisect bisect.c -ldl
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
typedef void pinyin_instance_t;
typedef void lookup_candidate_t;
typedef void ChewingKeyRest;
typedef void import_iterator_t;
typedef void export_iterator_t;
typedef void bigram_export_iterator_t;

/* ── Scalar types (match pinyin.h / glib) ─────────────────────────────── */

typedef uint32_t pinyin_option_t;
typedef uint32_t guint;
typedef int32_t  gint;
typedef char     gchar;
typedef uint32_t phrase_token_t;

/* ── Frontend call profile (matches tools/capture/capture.c) ──────────── */

#define IS_PINYIN         (1u << 1)
#define PINYIN_INCOMPLETE (1u << 3)
#define USE_DIVIDED_TABLE (1u << 7)
#define USE_RESPLIT_TABLE (1u << 8)

/* F-A capture profile: IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
 * USE_RESPLIT_TABLE == 0x0000018a. */
#define DEFAULT_FLAGS \
    ((pinyin_option_t)(IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | \
                       USE_RESPLIT_TABLE))

/* SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_AND_FREQUENCY. */
#define DEFAULT_SORT ((guint)0x1e)

/* ── Candidate type enum ──────────────────────────────────────────────── */

typedef enum {
    NBEST_MATCH_CANDIDATE         = 1,
    NORMAL_CANDIDATE              = 2,
    ZOMBIE_CANDIDATE              = 3,
    PREDICTED_BIGRAM_CANDIDATE    = 4,
    PREDICTED_PREFIX_CANDIDATE    = 5,
    ADDON_CANDIDATE               = 6,
    LONGER_CANDIDATE              = 7,
    PREDICTED_PUNCTUATION_CANDIDATE = 8,
} lookup_candidate_type_t;

/* ── Function pointer types for all 50 live symbols ───────────────────── */

/* 1a. Context lifecycle */
typedef pinyin_context_t *  (*fn_pinyin_init)(const char *, const char *);
typedef void                (*fn_pinyin_fini)(pinyin_context_t *);
typedef pinyin_instance_t * (*fn_pinyin_alloc_instance)(pinyin_context_t *);
typedef void                (*fn_pinyin_free_instance)(pinyin_instance_t *);

/* 1b. Configuration */
typedef bool (*fn_pinyin_set_options)(pinyin_context_t *, pinyin_option_t);
typedef bool (*fn_pinyin_set_double_pinyin_scheme)(pinyin_context_t *, int);
typedef bool (*fn_pinyin_set_zhuyin_scheme)(pinyin_context_t *, int);
typedef bool (*fn_pinyin_load_addon_phrase_library)(pinyin_context_t *, uint8_t);
typedef bool (*fn_pinyin_save)(pinyin_context_t *);

/* 1c. Parsing */
typedef size_t   (*fn_pinyin_parse_more_full_pinyins)(pinyin_instance_t *, const char *);
typedef size_t   (*fn_pinyin_parse_more_double_pinyins)(pinyin_instance_t *, const char *);
typedef size_t   (*fn_pinyin_parse_more_chewings)(pinyin_instance_t *, const char *);
typedef bool (*fn_pinyin_in_chewing_keyboard)(pinyin_instance_t *, char, gchar ***);

/* 1d. Sentence / guess */
typedef bool (*fn_pinyin_guess_sentence)(pinyin_instance_t *);
typedef bool (*fn_pinyin_guess_candidates)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_pinyin_guess_predicted)(pinyin_instance_t *, const char *);
typedef bool (*fn_pinyin_reset)(pinyin_instance_t *);

/* 1e. Candidate access */
typedef bool (*fn_pinyin_get_n_candidate)(pinyin_instance_t *, guint *);
typedef bool (*fn_pinyin_get_candidate)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_pinyin_get_candidate_type)(pinyin_instance_t *, lookup_candidate_t *, lookup_candidate_type_t *);
typedef bool (*fn_pinyin_get_candidate_string)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_pinyin_get_candidate_nbest_index)(pinyin_instance_t *, lookup_candidate_t *, uint8_t *);
typedef bool (*fn_pinyin_is_user_candidate)(pinyin_instance_t *, lookup_candidate_t *);
typedef bool (*fn_pinyin_remove_user_candidate)(pinyin_instance_t *, lookup_candidate_t *);

/* 1f. Selection / training */
typedef gint     (*fn_pinyin_choose_candidate)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_pinyin_choose_predicted_candidate)(pinyin_instance_t *, lookup_candidate_t *);
typedef bool (*fn_pinyin_train)(pinyin_instance_t *, uint8_t);

/* 1g. Sentence retrieval */
typedef bool (*fn_pinyin_get_sentence)(pinyin_instance_t *, uint8_t, char **);
typedef bool (*fn_pinyin_get_character_offset)(pinyin_instance_t *, const char *, size_t, size_t *);

/* 1h. Cursor / key rest */
typedef bool (*fn_pinyin_get_pinyin_key_rest)(pinyin_instance_t *, size_t, ChewingKeyRest **);
typedef bool (*fn_pinyin_get_pinyin_key_rest_positions)(pinyin_instance_t *, ChewingKeyRest *, uint16_t *, uint16_t *);
typedef bool (*fn_pinyin_get_pinyin_offset)(pinyin_instance_t *, size_t, size_t *);
typedef bool (*fn_pinyin_get_left_pinyin_offset)(pinyin_instance_t *, size_t, size_t *);
typedef bool (*fn_pinyin_get_right_pinyin_offset)(pinyin_instance_t *, size_t, size_t *);

/* 1i. Auxiliary text */
typedef bool (*fn_pinyin_get_full_pinyin_auxiliary_text)(pinyin_instance_t *, size_t, gchar **);
typedef bool (*fn_pinyin_get_double_pinyin_auxiliary_text)(pinyin_instance_t *, size_t, gchar **);
typedef bool (*fn_pinyin_get_chewing_auxiliary_text)(pinyin_instance_t *, size_t, gchar **);

/* 1j. User data */
typedef bool           (*fn_pinyin_mask_out)(pinyin_context_t *, phrase_token_t, phrase_token_t);
typedef bool           (*fn_pinyin_remember_user_input)(pinyin_instance_t *, const char *, gint);
typedef import_iterator_t *(*fn_pinyin_begin_add_phrases)(pinyin_context_t *, uint8_t);
typedef bool           (*fn_pinyin_iterator_add_phrase)(import_iterator_t *, const char *, const char *, gint);
typedef void               (*fn_pinyin_end_add_phrases)(import_iterator_t *);

/* 1k. Phrase / bigram export */
typedef export_iterator_t *(*fn_pinyin_begin_get_phrases)(pinyin_context_t *, uint8_t);
typedef bool           (*fn_pinyin_iterator_has_next_phrase)(export_iterator_t *);
typedef bool           (*fn_pinyin_iterator_get_next_phrase)(export_iterator_t *, gchar **, gchar **, gint *);
typedef void               (*fn_pinyin_end_get_phrases)(export_iterator_t *);
typedef bigram_export_iterator_t *(*fn_pinyin_begin_get_bigram_phrases)(pinyin_context_t *);
typedef bool           (*fn_pinyin_bigram_iterator_has_next_phrase)(bigram_export_iterator_t *);
typedef bool           (*fn_pinyin_bigram_iterator_get_next_phrase)(bigram_export_iterator_t *, gchar **, gchar **, gint *);
typedef void               (*fn_pinyin_end_get_bigram_phrases)(bigram_export_iterator_t *);

/* ── Symbol table ─────────────────────────────────────────────────────── */

struct symbols {
    /* 1a */
    fn_pinyin_init                       init;
    fn_pinyin_fini                       fini;
    fn_pinyin_alloc_instance             alloc_instance;
    fn_pinyin_free_instance              free_instance;
    /* 1b */
    fn_pinyin_set_options                set_options;
    fn_pinyin_set_double_pinyin_scheme   set_double_pinyin_scheme;
    fn_pinyin_set_zhuyin_scheme          set_zhuyin_scheme;
    fn_pinyin_load_addon_phrase_library  load_addon_phrase_library;
    fn_pinyin_save                       save;
    /* 1c */
    fn_pinyin_parse_more_full_pinyins    parse_full;
    fn_pinyin_parse_more_double_pinyins  parse_double;
    fn_pinyin_parse_more_chewings        parse_chewing;
    fn_pinyin_in_chewing_keyboard        in_chewing_keyboard;
    /* 1d */
    fn_pinyin_guess_sentence             guess_sentence;
    fn_pinyin_guess_candidates           guess_candidates;
    fn_pinyin_guess_predicted            guess_predicted;
    fn_pinyin_reset                      reset;
    /* 1e */
    fn_pinyin_get_n_candidate            get_n_candidate;
    fn_pinyin_get_candidate              get_candidate;
    fn_pinyin_get_candidate_type         get_candidate_type;
    fn_pinyin_get_candidate_string       get_candidate_string;
    fn_pinyin_get_candidate_nbest_index  get_candidate_nbest_index;
    fn_pinyin_is_user_candidate          is_user_candidate;
    fn_pinyin_remove_user_candidate      remove_user_candidate;
    /* 1f */
    fn_pinyin_choose_candidate           choose_candidate;
    fn_pinyin_choose_predicted_candidate choose_predicted_candidate;
    fn_pinyin_train                      train;
    /* 1g */
    fn_pinyin_get_sentence               get_sentence;
    fn_pinyin_get_character_offset       get_character_offset;
    /* 1h */
    fn_pinyin_get_pinyin_key_rest            get_pinyin_key_rest;
    fn_pinyin_get_pinyin_key_rest_positions  get_pinyin_key_rest_positions;
    fn_pinyin_get_pinyin_offset              get_pinyin_offset;
    fn_pinyin_get_left_pinyin_offset         get_left_pinyin_offset;
    fn_pinyin_get_right_pinyin_offset        get_right_pinyin_offset;
    /* 1i */
    fn_pinyin_get_full_pinyin_auxiliary_text    get_full_aux;
    fn_pinyin_get_double_pinyin_auxiliary_text  get_double_aux;
    fn_pinyin_get_chewing_auxiliary_text        get_chewing_aux;
    /* 1j */
    fn_pinyin_mask_out                  mask_out;
    fn_pinyin_remember_user_input       remember_user_input;
    fn_pinyin_begin_add_phrases         begin_add_phrases;
    fn_pinyin_iterator_add_phrase       iterator_add_phrase;
    fn_pinyin_end_add_phrases           end_add_phrases;
    /* 1k */
    fn_pinyin_begin_get_phrases              begin_get_phrases;
    fn_pinyin_iterator_has_next_phrase        iterator_has_next;
    fn_pinyin_iterator_get_next_phrase        iterator_get_next;
    fn_pinyin_end_get_phrases                end_get_phrases;
    fn_pinyin_begin_get_bigram_phrases       begin_get_bigram;
    fn_pinyin_bigram_iterator_has_next_phrase bigram_has_next;
    fn_pinyin_bigram_iterator_get_next_phrase bigram_get_next;
    fn_pinyin_end_get_bigram_phrases         end_get_bigram;
};

/* ── Symbol resolution ────────────────────────────────────────────────── */

#define RESOLVE(handle, table, field, name) do {                \
    (table).field = (typeof((table).field))dlsym(handle, name); \
    if (!(table).field)                                         \
        fprintf(stderr, "  MISSING: %s\n", name);              \
} while (0)

static int resolve_all(void *handle, struct symbols *s) {
    int missing = 0;

    RESOLVE(handle, *s, init,                 "pinyin_init");
    RESOLVE(handle, *s, fini,                 "pinyin_fini");
    RESOLVE(handle, *s, alloc_instance,       "pinyin_alloc_instance");
    RESOLVE(handle, *s, free_instance,        "pinyin_free_instance");
    RESOLVE(handle, *s, set_options,          "pinyin_set_options");
    RESOLVE(handle, *s, set_double_pinyin_scheme, "pinyin_set_double_pinyin_scheme");
    RESOLVE(handle, *s, set_zhuyin_scheme,    "pinyin_set_zhuyin_scheme");
    RESOLVE(handle, *s, load_addon_phrase_library, "pinyin_load_addon_phrase_library");
    RESOLVE(handle, *s, save,                 "pinyin_save");
    RESOLVE(handle, *s, parse_full,           "pinyin_parse_more_full_pinyins");
    RESOLVE(handle, *s, parse_double,         "pinyin_parse_more_double_pinyins");
    RESOLVE(handle, *s, parse_chewing,        "pinyin_parse_more_chewings");
    RESOLVE(handle, *s, in_chewing_keyboard,  "pinyin_in_chewing_keyboard");
    RESOLVE(handle, *s, guess_sentence,       "pinyin_guess_sentence");
    RESOLVE(handle, *s, guess_candidates,     "pinyin_guess_candidates");
    RESOLVE(handle, *s, guess_predicted,      "pinyin_guess_predicted_candidates_with_punctuations");
    RESOLVE(handle, *s, reset,                "pinyin_reset");
    RESOLVE(handle, *s, get_n_candidate,      "pinyin_get_n_candidate");
    RESOLVE(handle, *s, get_candidate,        "pinyin_get_candidate");
    RESOLVE(handle, *s, get_candidate_type,   "pinyin_get_candidate_type");
    RESOLVE(handle, *s, get_candidate_string,  "pinyin_get_candidate_string");
    RESOLVE(handle, *s, get_candidate_nbest_index, "pinyin_get_candidate_nbest_index");
    RESOLVE(handle, *s, is_user_candidate,    "pinyin_is_user_candidate");
    RESOLVE(handle, *s, remove_user_candidate, "pinyin_remove_user_candidate");
    RESOLVE(handle, *s, choose_candidate,     "pinyin_choose_candidate");
    RESOLVE(handle, *s, choose_predicted_candidate, "pinyin_choose_predicted_candidate");
    RESOLVE(handle, *s, train,                "pinyin_train");
    RESOLVE(handle, *s, get_sentence,         "pinyin_get_sentence");
    RESOLVE(handle, *s, get_character_offset,  "pinyin_get_character_offset");
    RESOLVE(handle, *s, get_pinyin_key_rest,   "pinyin_get_pinyin_key_rest");
    RESOLVE(handle, *s, get_pinyin_key_rest_positions, "pinyin_get_pinyin_key_rest_positions");
    RESOLVE(handle, *s, get_pinyin_offset,     "pinyin_get_pinyin_offset");
    RESOLVE(handle, *s, get_left_pinyin_offset, "pinyin_get_left_pinyin_offset");
    RESOLVE(handle, *s, get_right_pinyin_offset, "pinyin_get_right_pinyin_offset");
    RESOLVE(handle, *s, get_full_aux,          "pinyin_get_full_pinyin_auxiliary_text");
    RESOLVE(handle, *s, get_double_aux,        "pinyin_get_double_pinyin_auxiliary_text");
    RESOLVE(handle, *s, get_chewing_aux,       "pinyin_get_chewing_auxiliary_text");
    RESOLVE(handle, *s, mask_out,              "pinyin_mask_out");
    RESOLVE(handle, *s, remember_user_input,   "pinyin_remember_user_input");
    RESOLVE(handle, *s, begin_add_phrases,     "pinyin_begin_add_phrases");
    RESOLVE(handle, *s, iterator_add_phrase,   "pinyin_iterator_add_phrase");
    RESOLVE(handle, *s, end_add_phrases,       "pinyin_end_add_phrases");
    RESOLVE(handle, *s, begin_get_phrases,     "pinyin_begin_get_phrases");
    RESOLVE(handle, *s, iterator_has_next,     "pinyin_iterator_has_next_phrase");
    RESOLVE(handle, *s, iterator_get_next,     "pinyin_iterator_get_next_phrase");
    RESOLVE(handle, *s, end_get_phrases,       "pinyin_end_get_phrases");
    RESOLVE(handle, *s, begin_get_bigram,      "pinyin_begin_get_bigram_phrases");
    RESOLVE(handle, *s, bigram_has_next,       "pinyin_bigram_iterator_has_next_phrase");
    RESOLVE(handle, *s, bigram_get_next,       "pinyin_bigram_iterator_get_next_phrase");
    RESOLVE(handle, *s, end_get_bigram,        "pinyin_end_get_bigram_phrases");

    /* Count missing critical-path symbols. */
    if (!s->init)           missing++;
    if (!s->fini)           missing++;
    if (!s->alloc_instance) missing++;
    if (!s->free_instance)  missing++;
    if (!s->parse_full)     missing++;
    if (!s->guess_sentence) missing++;
    if (!s->guess_candidates) missing++;
    if (!s->get_n_candidate)  missing++;
    if (!s->get_candidate)    missing++;
    if (!s->get_candidate_string) missing++;
    if (!s->reset)          missing++;
    if (!s->save)           missing++;

    return missing;
}

/* ── Caller-owned string release ──────────────────────────────────────── */
/* pinyin.h / T0 freeze: strings returned by pinyin_get_sentence and the
 * *_auxiliary_text getters are caller-owned and must be released with
 * g_free().  GLib is not linked here, so resolve g_free from libglib and
 * fall back to free() (correct on Linux, where GLib uses the system
 * allocator). */

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

/* ── Candidate-type name ──────────────────────────────────────────────── */

static const char *ctype_name(lookup_candidate_type_t t) {
    switch (t) {
    case NBEST_MATCH_CANDIDATE:          return "NBEST_MATCH";
    case NORMAL_CANDIDATE:               return "NORMAL";
    case ZOMBIE_CANDIDATE:               return "ZOMBIE";
    case PREDICTED_BIGRAM_CANDIDATE:     return "PREDICTED_BIGRAM";
    case PREDICTED_PREFIX_CANDIDATE:     return "PREDICTED_PREFIX";
    case ADDON_CANDIDATE:                return "ADDON";
    case LONGER_CANDIDATE:               return "LONGER";
    case PREDICTED_PUNCTUATION_CANDIDATE: return "PREDICTED_PUNCT";
    default:                             return "UNKNOWN";
    }
}

/* ── Test inputs ──────────────────────────────────────────────────────── */

static const char *TEST_INPUTS[] = {
    "nihao",
    "xian",
    "fangan",
    "zhongguoren",
    "beijing",
    "a",
    "b",
    "",
};
static const size_t N_INPUTS = sizeof(TEST_INPUTS) / sizeof(TEST_INPUTS[0]);

/* ── Drive one input through the keystroke cycle ──────────────────────── */

static void drive_input(const struct symbols *s, pinyin_instance_t *inst,
                         const char *input) {
    printf("=== input: \"%s\" ===\n", input);

    /* Parse */
    size_t consumed = s->parse_full(inst, input);
    printf("parse_full: consumed=%zu\n", consumed);

    /* Guess sentence */
    bool gs = s->guess_sentence(inst);
    printf("guess_sentence: %s\n", gs ? "true" : "false");

    /* Get sentence (caller-owned; release with g_free) */
    if (s->get_sentence) {
        char *sentence = NULL;
        bool ok = s->get_sentence(inst, 0, &sentence);
        printf("get_sentence: %s text=\"%s\"\n",
               ok ? "true" : "false",
               sentence ? sentence : "(null)");
        if (sentence) {
            g_free_fn(sentence);
            printf("get_sentence: free OK\n");
        }
    }

    /* Auxiliary text (caller-owned; release with g_free) */
    if (s->get_full_aux) {
        gchar *aux = NULL;
        bool ok = s->get_full_aux(inst, 0, &aux);
        printf("get_full_aux: %s text=\"%s\"\n",
               ok ? "true" : "false",
               aux ? aux : "(null)");
        if (aux) {
            g_free_fn(aux);
            printf("get_full_aux: free OK\n");
        }
    }

    /* Guess candidates */
    bool gc = s->guess_candidates(inst, 0, DEFAULT_SORT);
    printf("guess_candidates: %s\n", gc ? "true" : "false");

    /* Enumerate candidates */
    guint n = 0;
    if (s->get_n_candidate)
        s->get_n_candidate(inst, &n);
    printf("n_candidates: %u\n", n);

    guint limit = n < 10 ? n : 10;
    for (guint i = 0; i < limit; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->get_candidate(inst, i, &cand) || !cand) {
            printf("  candidate[%u]: FAILED\n", i);
            continue;
        }

        const gchar *text = NULL;
        s->get_candidate_string(inst, cand, &text);

        lookup_candidate_type_t ctype = NORMAL_CANDIDATE;
        if (s->get_candidate_type)
            s->get_candidate_type(inst, cand, &ctype);

        uint8_t nbest = 0;
        if (s->get_candidate_nbest_index)
            s->get_candidate_nbest_index(inst, cand, &nbest);

        printf("  candidate[%u]: type=%s nbest=%u text=\"%s\"\n",
               i, ctype_name(ctype), (unsigned)nbest,
               text ? text : "(null)");
    }

    /* Character offset (if sentence was produced) */
    if (s->get_character_offset && consumed > 0) {
        size_t char_off = 0;
        s->get_character_offset(inst, input, consumed, &char_off);
        printf("character_offset(%zu): %zu\n", consumed, char_off);
    }

    /* Cursor positioning */
    if (s->get_pinyin_offset) {
        size_t off = 0;
        s->get_pinyin_offset(inst, consumed, &off);
        printf("pinyin_offset(%zu): %zu\n", consumed, off);
    }
    if (s->get_left_pinyin_offset) {
        size_t left = 0;
        s->get_left_pinyin_offset(inst, consumed, &left);
        printf("left_offset(%zu): %zu\n", consumed, left);
    }
    if (s->get_right_pinyin_offset) {
        size_t right = 0;
        s->get_right_pinyin_offset(inst, consumed, &right);
        printf("right_offset(%zu): %zu\n", consumed, right);
    }

    /* Choose the first candidate if available (exercises selection) */
    if (n > 0 && s->choose_candidate) {
        lookup_candidate_t *cand = NULL;
        if (s->get_candidate(inst, 0, &cand) && cand) {
            gint new_offset = s->choose_candidate(inst, 0, cand);
            printf("choose_candidate[0]: offset=%d\n", new_offset);

            /* Train after selection */
            if (s->train) {
                bool trained = s->train(inst, 0);
                printf("train: %s\n", trained ? "true" : "false");
            }
        }
    }

    /* Reset for next input */
    bool r = s->reset(inst);
    printf("reset: %s\n", r ? "true" : "false");
    printf("\n");
}

/* ── Main ─────────────────────────────────────────────────────────────── */

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "Usage: %s <path-to-so> <systemdir>\n", argv[0]);
        return 1;
    }

    const char *so_path    = argv[1];
    const char *system_dir = argv[2];

    /* Create a temporary user directory. */
    char user_dir[] = "/tmp/bisect-user-XXXXXX";
    if (!mkdtemp(user_dir)) {
        perror("mkdtemp");
        return 1;
    }

    printf("# bisection fixture\n");
    printf("# so: %s\n", so_path);
    printf("# systemdir: %s\n", system_dir);
    printf("# userdir: %s\n", user_dir);
    printf("# flags: 0x%08x\n", (unsigned)DEFAULT_FLAGS);
    printf("# sort: 0x%08x\n", (unsigned)DEFAULT_SORT);
    printf("\n");

    /* Load the shared object. */
    printf("=== dlopen ===\n");
    void *handle = dlopen(so_path, RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        rmdir(user_dir);
        return 1;
    }
    printf("dlopen: ok\n\n");

    /* Resolve all 50 symbols. */
    printf("=== resolve ===\n");
    struct symbols sym;
    memset(&sym, 0, sizeof(sym));
    int missing = resolve_all(handle, &sym);
    printf("resolved: missing_critical=%d\n\n", missing);

    if (missing > 0) {
        fprintf(stderr, "fatal: %d critical symbols missing\n", missing);
        dlclose(handle);
        rmdir(user_dir);
        return 1;
    }

    /* Resolve g_free for caller-owned strings before the first drive. */
    resolve_g_free();

    /* Phase 1 — Initialization. */
    printf("=== init ===\n");
    pinyin_context_t *ctx = sym.init(system_dir, user_dir);
    printf("pinyin_init: %s\n", ctx ? "ok" : "NULL");
    if (!ctx) {
        fprintf(stderr, "fatal: pinyin_init returned NULL\n");
        dlclose(handle);
        rmdir(user_dir);
        return 1;
    }

    bool opt = sym.set_options(ctx, DEFAULT_FLAGS);
    printf("set_options(0x%08x): %s\n", (unsigned)DEFAULT_FLAGS,
           opt ? "true" : "false");

    pinyin_instance_t *inst = sym.alloc_instance(ctx);
    printf("alloc_instance: %s\n", inst ? "ok" : "NULL");
    if (!inst) {
        fprintf(stderr, "fatal: alloc_instance returned NULL\n");
        sym.fini(ctx);
        dlclose(handle);
        rmdir(user_dir);
        return 1;
    }
    printf("\n");

    /* Phase 2 — Drive test inputs. */
    for (size_t i = 0; i < N_INPUTS; i++) {
        drive_input(&sym, inst, TEST_INPUTS[i]);
    }

    /* Phase 3 — Teardown. */
    printf("=== teardown ===\n");
    sym.free_instance(inst);
    printf("free_instance: ok\n");

    bool saved = sym.save(ctx);
    printf("save: %s\n", saved ? "true" : "false");

    sym.fini(ctx);
    printf("fini: ok\n");

    dlclose(handle);
    printf("dlclose: ok\n");

    rmdir(user_dir);
    return 0;
}
