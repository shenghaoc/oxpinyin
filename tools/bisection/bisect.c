/*
 * bisect.c — three-subject ABI bisection fixture.
 *
 * Loads a pinyin shared object (libpinyin.so or libpinyin_capi.so) via
 * dlopen, resolves all 51 W8 fork-bootstrap symbols, drives the full-pinyin
 * keystroke cycle, then probes the remaining symbol groups (double/chewing
 * parse, predicted, user-candidate, key-rest, aux, mask/remember, iterators,
 * scheme setters, addon load) on valid handles.  Run twice (once per .so)
 * and diff the logs to find behavioural divergence.
 *
 * Usage:
 *   ./bisect <path-to-so> <systemdir>
 *   ./bisect --perf <path-to-so> <systemdir>   # perf/RAM JSON line;
 *                                              # see run-perf-baseline.sh
 *
 * The output is a structured text log of return values, candidate strings,
 * and ownership-contract exercises (caller-owned strings are freed after
 * printing).  Deterministic for the same (so, data) pair.
 *
 * Build:
 *   gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o bisect bisect.c -ldl
 */

/* _DEFAULT_SOURCE alongside _POSIX_C_SOURCE: the RSS diagnostic mode calls
 * malloc_trim(3) and mallinfo2(3), glibc extensions that a bare
 * _POSIX_C_SOURCE definition hides. */
#define _DEFAULT_SOURCE
#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <malloc.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
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

/* ── Function pointer types for all 51 live symbols ───────────────────── */

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
typedef size_t   (*fn_pinyin_get_parsed_input_length)(pinyin_instance_t *);
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
typedef export_iterator_t *(*fn_pinyin_begin_get_phrases)(pinyin_context_t *, guint);
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
    fn_pinyin_get_parsed_input_length    get_parsed_input_length;
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
    if (!(table).field) {                                       \
        fprintf(stderr, "  MISSING: %s\n", name);              \
        missing++;                                              \
    }                                                           \
} while (0)

static int resolve_all(void *handle, struct symbols *s) {
    int missing = 0;

    RESOLVE(handle, *s, init,                 "pinyin_init");
    /* oxpinyin W3 fixtures: prefer the non-header constructor. The pinned
     * C++ oracle has no such symbol and keeps pinyin_init. */
    {
        fn_pinyin_init fixture_init =
            (fn_pinyin_init)dlsym(handle, "oxpinyin_init_for_fixtures");
        if (fixture_init)
            s->init = fixture_init;
    }
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
    RESOLVE(handle, *s, get_parsed_input_length, "pinyin_get_parsed_input_length");
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

    printf("resolved: %d/51 symbols\n", 51 - missing);
    return missing;
}

/* ── Caller-owned string release ──────────────────────────────────────── */
/* pinyin.h / T0 freeze: strings returned by pinyin_get_sentence and the
 * *_auxiliary_text getters are caller-owned and must be released with
 * g_free().  GLib is not linked here, so resolve g_free from libglib and
 * fall back to free() (correct on Linux, where GLib uses the system
 * allocator). */

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

/* ── Performance-baseline inputs (same 20-input corpus as the Criterion
 * keystroke-cycle bench: crates/pinyin-oracle/benches/support/mod.rs) ─── */

static const char *PERF_CYCLE_INPUTS[] = {
    "ni",
    "wo",
    "de",
    "nihao",
    "zhongguo",
    "xian",
    "fangan",
    "xi'an",
    "bu'tian",
    "fan'gan",
    "n",
    "zh",
    "chongke",
    "caisho",
    "paolen",
    "waimenggu",
    "lenglan",
    "naoxion",
    "liangniejue",
    "chuaipengdengzaimiu",
};
static const size_t N_PERF_CYCLE_INPUTS =
    sizeof(PERF_CYCLE_INPUTS) / sizeof(PERF_CYCLE_INPUTS[0]);

/* The only observable effect of the perf loops. Function-pointer calls
 * cannot be folded by the compiler anyway, but this keeps the checksum live
 * and makes the intent explicit. */
static volatile uint64_t perf_sink;

static uint64_t now_ns(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0)
        return 0;
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

struct perf_memory {
    long rss_kib;
    long hwm_kib;
    long vm_size_kib;
    long rss_anon_kib;
    long rss_file_kib;
};

static void read_perf_memory(struct perf_memory *memory) {
    FILE *status = fopen("/proc/self/status", "r");
    char line[256];

    memory->rss_kib = -1;
    memory->hwm_kib = -1;
    memory->vm_size_kib = -1;
    memory->rss_anon_kib = -1;
    memory->rss_file_kib = -1;
    if (!status)
        return;

    while (fgets(line, sizeof line, status)) {
        unsigned long value = 0;
        if (sscanf(line, "VmRSS: %lu kB", &value) == 1)
            memory->rss_kib = (long)value;
        else if (sscanf(line, "VmHWM: %lu kB", &value) == 1)
            memory->hwm_kib = (long)value;
        else if (sscanf(line, "VmSize: %lu kB", &value) == 1)
            memory->vm_size_kib = (long)value;
        else if (sscanf(line, "RssAnon: %lu kB", &value) == 1)
            memory->rss_anon_kib = (long)value;
        else if (sscanf(line, "RssFile: %lu kB", &value) == 1)
            memory->rss_file_kib = (long)value;
    }
    fclose(status);
}

static void perf_json_string(const char *text) {
    putchar('"');
    for (const unsigned char *p = (const unsigned char *)text; *p; p++) {
        switch (*p) {
        case '\\': fputs("\\\\", stdout); break;
        case '"':  fputs("\\\"", stdout); break;
        case '\n': fputs("\\n", stdout); break;
        case '\r': fputs("\\r", stdout); break;
        case '\t': fputs("\\t", stdout); break;
        default:
            if (*p < 0x20) {
                printf("\\u%04x", (unsigned)*p);
            } else {
                putchar(*p);
            }
        }
    }
    putchar('"');
}

static void perf_print_memory_body(const struct perf_memory *m) {
    printf("{\"rss_kib\":%ld,\"hwm_kib\":%ld,\"vm_size_kib\":%ld,"
           "\"rss_anon_kib\":%ld,\"rss_file_kib\":%ld}",
           m->rss_kib, m->hwm_kib, m->vm_size_kib,
           m->rss_anon_kib, m->rss_file_kib);
}

static void perf_print_memory(const char *tag, const struct perf_memory *m) {
    printf("\"%s\":", tag);
    perf_print_memory_body(m);
}

/* ── RSS diagnosis instrument (PERF_MODE=rss-diag) ────────────────────
 *
 * Answers, for one process of either engine, where the resident set
 * actually sits: how much of it is anonymous versus file-backed
 * (smaps_rollup), which mappings carry it (/proc/self/maps), how much of
 * it the allocator is merely retaining (malloc_trim(3) plus mallinfo2(3)),
 * and how the split moves between "just initialized" and "after N
 * keystroke cycles".
 *
 * Every reading is taken identically for libpinyin and for oxpinyin from
 * inside the same driver, so the comparison is symmetric by construction.
 * The /proc text dumps are gated on RSS_DIAG_DIR precisely because
 * writing them allocates: a measurement round that reports the trim delta
 * must leave RSS_DIAG_DIR unset, and a round that captures mappings is a
 * separate round. Nothing in this block runs unless PERF_MODE=rss-diag.
 */

struct smaps_rollup {
    long rss_kib;
    long pss_kib;
    long shared_clean_kib;
    long shared_dirty_kib;
    long private_clean_kib;
    long private_dirty_kib;
    long anonymous_kib;
    long file_kib; /* derived: rss - anonymous, per smaps semantics */
};

static void smaps_rollup_reset(struct smaps_rollup *r) {
    r->rss_kib = -1;
    r->pss_kib = -1;
    r->shared_clean_kib = -1;
    r->shared_dirty_kib = -1;
    r->private_clean_kib = -1;
    r->private_dirty_kib = -1;
    r->anonymous_kib = -1;
    r->file_kib = -1;
}

static void read_smaps_rollup(struct smaps_rollup *r) {
    FILE *f = fopen("/proc/self/smaps_rollup", "r");
    char line[256];

    smaps_rollup_reset(r);
    if (!f)
        return;

    while (fgets(line, sizeof line, f)) {
        unsigned long value = 0;
        if (sscanf(line, "Rss: %lu kB", &value) == 1)
            r->rss_kib = (long)value;
        else if (sscanf(line, "Pss: %lu kB", &value) == 1)
            r->pss_kib = (long)value;
        else if (sscanf(line, "Shared_Clean: %lu kB", &value) == 1)
            r->shared_clean_kib = (long)value;
        else if (sscanf(line, "Shared_Dirty: %lu kB", &value) == 1)
            r->shared_dirty_kib = (long)value;
        else if (sscanf(line, "Private_Clean: %lu kB", &value) == 1)
            r->private_clean_kib = (long)value;
        else if (sscanf(line, "Private_Dirty: %lu kB", &value) == 1)
            r->private_dirty_kib = (long)value;
        else if (sscanf(line, "Anonymous: %lu kB", &value) == 1)
            r->anonymous_kib = (long)value;
    }
    fclose(f);
    if (r->rss_kib >= 0 && r->anonymous_kib >= 0)
        r->file_kib = r->rss_kib - r->anonymous_kib;
}

static void print_smaps_rollup(const char *tag, const struct smaps_rollup *r) {
    printf("\"%s\":{\"rss_kib\":%ld,\"pss_kib\":%ld,"
           "\"shared_clean_kib\":%ld,\"shared_dirty_kib\":%ld,"
           "\"private_clean_kib\":%ld,\"private_dirty_kib\":%ld,"
           "\"anonymous_kib\":%ld,\"file_kib\":%ld}",
           tag, r->rss_kib, r->pss_kib, r->shared_clean_kib,
           r->shared_dirty_kib, r->private_clean_kib, r->private_dirty_kib,
           r->anonymous_kib, r->file_kib);
}

struct arena_state {
    long arena;      /* bytes obtained from sbrk (non-mmap heap)      */
    long hblks;      /* count of mmap'd regions                        */
    long hblkhd;     /* bytes in mmap'd regions                        */
    long uordblks;   /* bytes in in-use chunks                         */
    long fordblks;   /* bytes in free chunks retained by the allocator */
    long keepcost;   /* releasable top-of-heap bytes                   */
};

static void read_arena_state(struct arena_state *a) {
    struct mallinfo2 mi = mallinfo2();

    a->arena = (long)mi.arena;
    a->hblks = (long)mi.hblks;
    a->hblkhd = (long)mi.hblkhd;
    a->uordblks = (long)mi.uordblks;
    a->fordblks = (long)mi.fordblks;
    a->keepcost = (long)mi.keepcost;
}

static void print_arena_state(const char *tag, const struct arena_state *a) {
    printf("\"%s\":{\"arena\":%ld,\"hblks\":%ld,\"hblkhd\":%ld,"
           "\"uordblks\":%ld,\"fordblks\":%ld,\"keepcost\":%ld}",
           tag, a->arena, a->hblks, a->hblkhd,
           a->uordblks, a->fordblks, a->keepcost);
}

/* Copy a /proc text file verbatim. Returns 0 on success. The destination
 * is <dir>/<tag>-<what>.txt; the caller owns naming. */
static int dump_proc_text(const char *dir, const char *tag, const char *what,
                          const char *src) {
    char path[512];
    FILE *in, *out;
    char buffer[4096];
    size_t n;

    if (snprintf(path, sizeof path, "%s/%s-%s.txt", dir, tag, what)
        >= (int)sizeof path)
        return -1;
    in = fopen(src, "r");
    if (!in)
        return -1;
    out = fopen(path, "w");
    if (!out) {
        fclose(in);
        return -1;
    }
    while ((n = fread(buffer, 1, sizeof buffer, in)) > 0) {
        if (fwrite(buffer, 1, n, out) != n) {
            fclose(in);
            fclose(out);
            return -1;
        }
    }
    fclose(in);
    return fclose(out) == 0 ? 0 : -1;
}

/* glibc's own arena XML, which mallinfo2 cannot express: per-arena free
 * totals, the mmap threshold, and the system-bytes high-water. */
static int dump_malloc_info(const char *dir, const char *tag) {
    char path[512];
    FILE *out;

    if (snprintf(path, sizeof path, "%s/%s-mallocinfo.xml", dir, tag)
        >= (int)sizeof path)
        return -1;
    out = fopen(path, "w");
    if (!out)
        return -1;
    if (malloc_info(0, out) != 0) {
        fclose(out);
        return -1;
    }
    return fclose(out) == 0 ? 0 : -1;
}

/* The gated Rust counting allocator, when the artifact under test carries
 * it (`--features alloc-count`, never in a shipped build). Absent from
 * libpinyin and from any default oxpinyin artifact, so every field stays
 * -1 there and the JSON shape does not change between cells. Live and
 * peak-live are the RSS-relevant pair: cumulative count and cumulative
 * bytes say nothing about resident memory. */
struct alloc_counters {
    long long count;
    long long bytes;
    long long live_bytes;
    long long peak_live_bytes;
};

typedef uint64_t (*fn_alloc_u64)(void);

typedef void (*fn_alloc_void)(void);

struct alloc_counter_syms {
    fn_alloc_u64 count;
    fn_alloc_u64 bytes;
    fn_alloc_u64 live;
    fn_alloc_u64 peak;
    fn_alloc_void reset_peak;
};

static void resolve_alloc_counters(void *handle, struct alloc_counter_syms *a) {
    a->count = (fn_alloc_u64)dlsym(handle, "oxpinyin_alloc_count");
    a->bytes = (fn_alloc_u64)dlsym(handle, "oxpinyin_alloc_bytes");
    a->live = (fn_alloc_u64)dlsym(handle, "oxpinyin_alloc_live_bytes");
    a->peak = (fn_alloc_u64)dlsym(handle, "oxpinyin_alloc_peak_live_bytes");
    a->reset_peak = (fn_alloc_void)dlsym(handle, "oxpinyin_alloc_reset_peak");
}

static void read_alloc_counters(const struct alloc_counter_syms *a,
                                struct alloc_counters *c) {
    c->count = a->count ? (long long)a->count() : -1;
    c->bytes = a->bytes ? (long long)a->bytes() : -1;
    c->live_bytes = a->live ? (long long)a->live() : -1;
    c->peak_live_bytes = a->peak ? (long long)a->peak() : -1;
}

static void print_alloc_counters(const char *tag, const struct alloc_counters *c) {
    printf("\"%s\":{\"count\":%lld,\"bytes\":%lld,"
           "\"live_bytes\":%lld,\"peak_live_bytes\":%lld}",
           tag, c->count, c->bytes, c->live_bytes, c->peak_live_bytes);
}

/* One 20-input cycle. For every input the instance is reset, then each
 * accumulated ASCII prefix is parsed and decoded the way the C++ frontend
 * drives the keystroke path: parse, guess candidates, read the count. */
static uint64_t run_perf_cycle(const struct symbols *s, pinyin_instance_t *inst) {
    uint64_t checksum = 1469598103934665603ULL;

    for (size_t i = 0; i < N_PERF_CYCLE_INPUTS; i++) {
        const char *input = PERF_CYCLE_INPUTS[i];
        size_t length = strlen(input);
        char prefix[256];

        if (length + 1 > sizeof prefix)
            return checksum;
        if (!s->reset(inst))
            return checksum;

        for (size_t j = 0; j < length; j++) {
            prefix[j] = input[j];
            prefix[j + 1] = '\0';

            size_t consumed = s->parse_full(inst, prefix);
            bool guessed = s->guess_candidates(inst, 0, DEFAULT_SORT);
            guint candidate_count = 0;
            bool counted = s->get_n_candidate(inst, &candidate_count);

            checksum ^= (uint64_t)consumed;
            checksum ^= guessed ? 0x9e3779b97f4a7c15ULL : 0xbf58476d1ce4e5b9ULL;
            checksum ^= counted ? (uint64_t)candidate_count : 0x94d049bb133111ebULL;
            checksum = (checksum << 7) | (checksum >> 57);
        }
    }
    return checksum;
}

/* One timed unit: `repeats` back-to-back corpus passes over the frozen
 * 20-input cycle above. PERF_REPEATS scales the amount of work inside the
 * timed region without touching the corpus, so input structure — every
 * string, every prefix length, every reset — is identical at every size.
 * At the default of 1 this returns run_perf_cycle's own checksum
 * unchanged, so the default workload is the pre-knob workload exactly. */
static uint64_t run_perf_unit(const struct symbols *s, pinyin_instance_t *inst,
                              int repeats) {
    uint64_t checksum = 0;

    for (int r = 0; r < repeats; r++)
        checksum ^= run_perf_cycle(s, inst);
    return checksum;
}

static int perf_env_count(const char *name, int fallback) {
    const char *value = getenv(name);
    char *end = NULL;
    long parsed;

    if (!value || !*value)
        return fallback;
    parsed = strtol(value, &end, 10);
    if (!end || *end || parsed < 1 || parsed > 100000)
        return fallback;
    return (int)parsed;
}

static int resolve_perf_symbols(void *handle, struct symbols *s) {
    int missing = 0;

    memset(s, 0, sizeof *s);
    RESOLVE(handle, *s, init,            "pinyin_init");
    RESOLVE(handle, *s, fini,            "pinyin_fini");
    RESOLVE(handle, *s, alloc_instance,  "pinyin_alloc_instance");
    RESOLVE(handle, *s, free_instance,   "pinyin_free_instance");
    RESOLVE(handle, *s, set_options,     "pinyin_set_options");
    RESOLVE(handle, *s, parse_full,      "pinyin_parse_more_full_pinyins");
    RESOLVE(handle, *s, guess_candidates,"pinyin_guess_candidates");
    RESOLVE(handle, *s, get_n_candidate, "pinyin_get_n_candidate");
    RESOLVE(handle, *s, reset,           "pinyin_reset");
    return missing;
}

static void perf_remove_user_dir(char *user_dir) {
    char command[640];

    if (strlen(user_dir) + 32 >= sizeof command)
        return;
    snprintf(command, sizeof command, "rm -rf -- '%s'", user_dir);
    /* Best-effort cleanup of our own mkdtemp directory; a failure is not
     * worth aborting a measurement over. The status is bound rather than
     * cast away because system(3) is warn_unused_result under the
     * _FORTIFY_SOURCE level Ubuntu's gcc enables by default, and a plain
     * (void) cast does not satisfy it -- the harness would not build at
     * -Werror outside the Debian container. */
    int status = system(command);
    (void)status;
}

static int run_perf_mode(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr,
                "Usage: %s --perf <path-to-so> <systemdir>\n"
                "  Environment: PERF_BACKEND (label), PERF_MODE\n"
                "    (speed|ram-init|ram-cycle|rss-diag), PERF_CYCLES\n"
                "    (default 8), PERF_REPEATS (corpus passes per cycle,\n"
                "    default 1). rss-diag additionally reports\n"
                "    smaps_rollup and mallinfo2 at init and after the\n"
                "    cycles, calls malloc_trim(0) and re-reads both;\n"
                "    RSS_DIAG_DIR (with RSS_DIAG_TAG) also writes the\n"
                "    /proc/self/maps, /proc/self/smaps and malloc_info\n"
                "    dumps, which\n"
                "    allocate and so belong in their own round.\n",
                argv[0]);
        return 1;
    }

    const char *so_path    = argv[2];
    const char *system_dir = argv[3];

    const char *backend    = getenv("PERF_BACKEND");
    const char *mode       = getenv("PERF_MODE");
    int cycles             = perf_env_count("PERF_CYCLES", 8);
    int repeats            = perf_env_count("PERF_REPEATS", 1);

    if (!backend)
        backend = so_path;
    if (!mode)
        mode = "speed";

    /* RSS diagnosis. The /proc text dumps are opt-in on RSS_DIAG_DIR
     * because writing them allocates: a round that reports the
     * malloc_trim delta must run with RSS_DIAG_DIR unset. */
    const bool rss_diag = strcmp(mode, "rss-diag") == 0;
    const char *diag_dir = getenv("RSS_DIAG_DIR");
    const char *diag_tag = getenv("RSS_DIAG_TAG");

    if (diag_tag == NULL || *diag_tag == '\0')
        diag_tag = "run";
    if (!rss_diag)
        diag_dir = NULL;

    char user_dir[] = "/tmp/bisect-perf-XXXXXX";
    if (!mkdtemp(user_dir)) {
        perror("mkdtemp");
        return 1;
    }

    void *handle = dlopen(so_path, RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        perf_remove_user_dir(user_dir);
        return 1;
    }

    struct symbols sym;
    int missing = resolve_perf_symbols(handle, &sym);
    if (missing > 0) {
        fprintf(stderr, "fatal: %d perf symbols missing\n", missing);
        dlclose(handle);
        perf_remove_user_dir(user_dir);
        return 1;
    }

    uint64_t init_start = now_ns();
    pinyin_context_t *ctx = sym.init(system_dir, user_dir);
    uint64_t init_end = now_ns();
    if (!ctx) {
        fprintf(stderr, "fatal: pinyin_init returned NULL\n");
        dlclose(handle);
        perf_remove_user_dir(user_dir);
        return 1;
    }

    (void)sym.set_options(ctx, DEFAULT_FLAGS);
    uint64_t alloc_start = now_ns();
    pinyin_instance_t *inst = sym.alloc_instance(ctx);
    uint64_t alloc_end = now_ns();
    if (!inst) {
        fprintf(stderr, "fatal: alloc_instance returned NULL\n");
        sym.fini(ctx);
        dlclose(handle);
        perf_remove_user_dir(user_dir);
        return 1;
    }

    struct perf_memory after_init;
    struct smaps_rollup rollup_init;
    struct arena_state arena_init;

    read_perf_memory(&after_init);
    struct alloc_counter_syms alloc_syms;
    struct alloc_counters alloc_init;
    struct alloc_counters alloc_cycle;

    resolve_alloc_counters(handle, &alloc_syms);
    if (rss_diag) {
        read_smaps_rollup(&rollup_init);
        read_arena_state(&arena_init);
        read_alloc_counters(&alloc_syms, &alloc_init);
        /* Bound the peak to the cycle region. Read initialization's
         * counters first, then drop the peak to the current live total,
         * so `alloc_cycle.peak_live_bytes` is the cycles' own high-water
         * mark and not the larger of the two regions. Absent from
         * libpinyin and from any default oxpinyin artifact, where the
         * symbol does not resolve and this is a no-op. */
        if (alloc_syms.reset_peak)
            alloc_syms.reset_peak();
        if (diag_dir) {
            (void)dump_proc_text(diag_dir, diag_tag, "maps-init",
                                 "/proc/self/maps");
            (void)dump_proc_text(diag_dir, diag_tag, "smaps-init",
                                 "/proc/self/smaps");
            (void)dump_proc_text(diag_dir, diag_tag, "smaps-rollup-init",
                                 "/proc/self/smaps_rollup");
            (void)dump_malloc_info(diag_dir, diag_tag);
        }
    }

    printf("{\"backend\":");
    perf_json_string(backend);
    printf(",\"so\":");
    perf_json_string(so_path);
    printf(",\"systemdir\":");
    perf_json_string(system_dir);
    printf(",\"mode\":");
    perf_json_string(mode);
    printf(",\"repeats\":%d", repeats);
    printf(",\"init_ns\":%llu,\"alloc_ns\":%llu,",
           (unsigned long long)(init_end - init_start),
           (unsigned long long)(alloc_end - alloc_start));
    perf_print_memory("after_init", &after_init);
    if (rss_diag) {
        putchar(',');
        print_smaps_rollup("rollup_init", &rollup_init);
        putchar(',');
        print_arena_state("arena_init", &arena_init);
        putchar(',');
        print_alloc_counters("alloc_init", &alloc_init);
    }

    if (strcmp(mode, "ram-init") != 0) {
        uint64_t *cycle_ns = calloc((size_t)cycles, sizeof *cycle_ns);
        struct perf_memory after_first;
        struct perf_memory after_last;
        if (!cycle_ns) {
            fprintf(stderr, "fatal: calloc cycle_ns failed\n");
            sym.free_instance(inst);
            sym.fini(ctx);
            dlclose(handle);
            perf_remove_user_dir(user_dir);
            return 1;
        }

        for (int i = 0; i < cycles; i++) {
            uint64_t start = now_ns();
            perf_sink ^= run_perf_unit(&sym, inst, repeats);
            uint64_t end = now_ns();
            cycle_ns[i] = end - start;
            if (i == 0)
                read_perf_memory(&after_first);
        }
        read_perf_memory(&after_last);

        /* The decisive experiment. Read the post-cycle state, then ask
         * glibc to return everything it is merely retaining, then read
         * the same state again. A gap that collapses here was allocator
         * retention driven by transient churn; a gap that survives is
         * held in live structures or in file-backed pages. */
        struct smaps_rollup rollup_cycle;
        struct arena_state arena_cycle;
        struct perf_memory after_trim;
        struct smaps_rollup rollup_trim;
        struct arena_state arena_trim;
        int trim_released = -1;

        if (rss_diag) {
            read_smaps_rollup(&rollup_cycle);
            read_arena_state(&arena_cycle);
            read_alloc_counters(&alloc_syms, &alloc_cycle);
            if (diag_dir) {
                (void)dump_proc_text(diag_dir, diag_tag, "maps-cycle",
                                     "/proc/self/maps");
                (void)dump_proc_text(diag_dir, diag_tag, "smaps-cycle",
                                     "/proc/self/smaps");
                (void)dump_proc_text(diag_dir, diag_tag,
                                     "smaps-rollup-cycle",
                                     "/proc/self/smaps_rollup");
            }
            trim_released = malloc_trim(0);
            read_perf_memory(&after_trim);
            read_smaps_rollup(&rollup_trim);
            read_arena_state(&arena_trim);
            if (diag_dir) {
                (void)dump_proc_text(diag_dir, diag_tag, "maps-trim",
                                     "/proc/self/maps");
                (void)dump_proc_text(diag_dir, diag_tag, "smaps-trim",
                                     "/proc/self/smaps");
                (void)dump_proc_text(diag_dir, diag_tag,
                                     "smaps-rollup-trim",
                                     "/proc/self/smaps_rollup");
            }
        }

        printf(",\"cycles_ns\":[");
        for (int i = 0; i < cycles; i++)
            printf("%s%llu", i ? "," : "", (unsigned long long)cycle_ns[i]);
        printf("],\"after_first\":");
        perf_print_memory_body(&after_first);
        printf(",\"after_last\":");
        perf_print_memory_body(&after_last);
        if (rss_diag) {
            putchar(',');
            print_smaps_rollup("rollup_cycle", &rollup_cycle);
            putchar(',');
            print_arena_state("arena_cycle", &arena_cycle);
            putchar(',');
            print_alloc_counters("alloc_cycle", &alloc_cycle);
            printf(",\"trim_released\":%d,\"after_trim\":", trim_released);
            perf_print_memory_body(&after_trim);
            putchar(',');
            print_smaps_rollup("rollup_trim", &rollup_trim);
            putchar(',');
            print_arena_state("arena_trim", &arena_trim);
        }
        free(cycle_ns);
    }

    printf("}\n");

    sym.free_instance(inst);
    sym.fini(ctx);
    dlclose(handle);
    perf_remove_user_dir(user_dir);
    return 0;
}

/* ── Drive one input through the keystroke cycle ──────────────────────── */

static void drive_input(const struct symbols *s, pinyin_instance_t *inst,
                         const char *input) {
    printf("=== input: \"%s\" ===\n", input);

    /* Parse */
    size_t consumed = s->parse_full(inst, input);
    printf("parse_full: consumed=%zu\n", consumed);
    printf("parsed_input_length: %zu\n", s->get_parsed_input_length(inst));

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
        bool have_nbest = false;
        if (ctype == NBEST_MATCH_CANDIDATE && s->get_candidate_nbest_index)
            have_nbest = s->get_candidate_nbest_index(inst, cand, &nbest);

        char nbest_buf[8];
        const char *nbest_text = "-";
        if (have_nbest) {
            snprintf(nbest_buf, sizeof nbest_buf, "%u", (unsigned)nbest);
            nbest_text = nbest_buf;
        }

        printf("  candidate[%u]: type=%s nbest=%s text=\"%s\"\n",
               i, ctype_name(ctype), nbest_text,
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
    printf("parsed_input_length_after_reset: %zu\n",
           s->get_parsed_input_length(inst));
    printf("\n");
}

/* ── Remaining 27 symbols (valid handles only; no NULL-handle calls) ─── */

static void probe_aux(pinyin_instance_t *inst,
                      bool (*getter)(pinyin_instance_t *, size_t, gchar **),
                      const char *name) {
    if (!getter)
        return;
    gchar *aux = NULL;
    bool ok = getter(inst, 0, &aux);
    printf("%s: %s text=\"%s\"\n", name, ok ? "true" : "false",
           aux ? aux : "(null)");
    if (aux) {
        g_free_fn(aux);
        printf("%s: free OK\n", name);
    }
}

static void probe_remaining(const struct symbols *s, pinyin_context_t *ctx,
                            pinyin_instance_t *inst) {
    printf("=== extra_symbols ===\n");

    /* Scheme setters + addon load (after the full-pinyin cycle). */
    if (s->set_double_pinyin_scheme) {
        bool ok = s->set_double_pinyin_scheme(ctx, 2); /* DOUBLE_PINYIN_MS */
        printf("set_double_pinyin_scheme(MS): %s\n", ok ? "true" : "false");
    }
    if (s->set_zhuyin_scheme) {
        bool ok = s->set_zhuyin_scheme(ctx, 1); /* ZHUYIN_STANDARD */
        printf("set_zhuyin_scheme(STANDARD): %s\n", ok ? "true" : "false");
    }
    if (s->load_addon_phrase_library) {
        bool ok = s->load_addon_phrase_library(ctx, 0);
        printf("load_addon_phrase_library(0): %s\n", ok ? "true" : "false");
    }
    if (s->mask_out) {
        bool ok = s->mask_out(ctx, 0, 0);
        printf("mask_out(0,0): %s\n", ok ? "true" : "false");
    }

    /* Double / chewing parse + chewing keyboard (g_strfreev). */
    if (s->parse_double) {
        size_t n = s->parse_double(inst, "nihao");
        printf("parse_double: consumed=%zu\n", n);
        if (s->reset)
            s->reset(inst);
    }
    if (s->parse_chewing) {
        size_t n = s->parse_chewing(inst, "nihao");
        printf("parse_chewing: consumed=%zu\n", n);
        if (s->reset)
            s->reset(inst);
    }
    if (s->in_chewing_keyboard) {
        gchar **symbols = NULL;
        bool ok = s->in_chewing_keyboard(inst, 'a', &symbols);
        printf("in_chewing_keyboard('a'): %s symbols=%s\n",
               ok ? "true" : "false", symbols ? "set" : "(null)");
        if (symbols)
            free_strv(symbols);
    }

    /* Key-rest + double/chewing aux + remember, after a real parse. */
    if (s->parse_full)
        s->parse_full(inst, "nihao");
    if (s->get_pinyin_key_rest) {
        ChewingKeyRest *rest = NULL;
        bool ok = s->get_pinyin_key_rest(inst, 0, &rest);
        printf("get_pinyin_key_rest(0): %s rest=%s\n",
               ok ? "true" : "false", rest ? "set" : "(null)");
        if (ok && rest && s->get_pinyin_key_rest_positions) {
            uint16_t begin = 0, end = 0;
            bool pos = s->get_pinyin_key_rest_positions(inst, rest, &begin, &end);
            printf("get_pinyin_key_rest_positions: %s begin=%u end=%u\n",
                   pos ? "true" : "false", (unsigned)begin, (unsigned)end);
        }
    }
    probe_aux(inst, s->get_double_aux, "get_double_aux");
    probe_aux(inst, s->get_chewing_aux, "get_chewing_aux");
    if (s->remember_user_input) {
        bool ok = s->remember_user_input(inst, "你好", 1);
        printf("remember_user_input: %s\n", ok ? "true" : "false");
    }

    /* User-candidate + predicted guess/choose. */
    if (s->guess_candidates)
        s->guess_candidates(inst, 0, DEFAULT_SORT);
    if (s->get_candidate && s->is_user_candidate) {
        lookup_candidate_t *cand = NULL;
        if (s->get_candidate(inst, 0, &cand) && cand) {
            bool user = s->is_user_candidate(inst, cand);
            printf("is_user_candidate[0]: %s\n", user ? "true" : "false");
            if (s->remove_user_candidate) {
                bool rm = s->remove_user_candidate(inst, cand);
                printf("remove_user_candidate[0]: %s\n", rm ? "true" : "false");
            }
        } else {
            printf("is_user_candidate[0]: no candidate\n");
        }
    }
    if (s->guess_predicted) {
        bool ok = s->guess_predicted(inst, "你");
        printf("guess_predicted: %s\n", ok ? "true" : "false");
        if (ok && s->get_candidate && s->choose_predicted_candidate) {
            lookup_candidate_t *cand = NULL;
            if (s->get_candidate(inst, 0, &cand) && cand) {
                bool ch = s->choose_predicted_candidate(inst, cand);
                printf("choose_predicted_candidate[0]: %s\n",
                       ch ? "true" : "false");
            }
        }
    }

    /* Import / unigram export / bigram export. Drive the import trio on
     * USER_DICTIONARY (7) so every symbol has a live handle; the phrase
     * export below reads that same user sub-index. */
    if (s->begin_add_phrases) {
        import_iterator_t *it = s->begin_add_phrases(ctx, 7);
        printf("begin_add_phrases: %s\n", it ? "ok" : "NULL");
        if (it) {
            if (s->iterator_add_phrase) {
                bool add = s->iterator_add_phrase(it, "你好", "nihao", 1);
                printf("iterator_add_phrase: %s\n", add ? "true" : "false");
            }
            if (s->end_add_phrases)
                s->end_add_phrases(it);
            printf("end_add_phrases: ok\n");
        }
    }
    if (s->begin_get_phrases) {
        export_iterator_t *it = s->begin_get_phrases(ctx, 7);
        printf("begin_get_phrases: %s\n", it ? "ok" : "NULL");
        if (it) {
            bool has = s->iterator_has_next ? s->iterator_has_next(it) : false;
            printf("iterator_has_next: %s\n", has ? "true" : "false");
            if (has && s->iterator_get_next) {
                gchar *phrase = NULL, *pinyin = NULL;
                gint count = 0;
                bool next = s->iterator_get_next(it, &phrase, &pinyin, &count);
                printf("iterator_get_next: %s count=%d\n",
                       next ? "true" : "false", (int)count);
                if (phrase)
                    g_free_fn(phrase);
                if (pinyin)
                    g_free_fn(pinyin);
            }
            if (s->end_get_phrases)
                s->end_get_phrases(it);
            printf("end_get_phrases: ok\n");
        }
    }
    if (s->begin_get_bigram) {
        bigram_export_iterator_t *it = s->begin_get_bigram(ctx);
        printf("begin_get_bigram: %s\n", it ? "ok" : "NULL");
        if (it) {
            bool has = s->bigram_has_next ? s->bigram_has_next(it) : false;
            printf("bigram_has_next: %s\n", has ? "true" : "false");
            if (has && s->bigram_get_next) {
                gchar *phrase = NULL, *pinyin = NULL;
                gint count = 0;
                bool next = s->bigram_get_next(it, &phrase, &pinyin, &count);
                printf("bigram_get_next: %s count=%d\n",
                       next ? "true" : "false", (int)count);
                if (phrase)
                    g_free_fn(phrase);
                if (pinyin)
                    g_free_fn(pinyin);
            }
            if (s->end_get_bigram)
                s->end_get_bigram(it);
            printf("end_get_bigram: ok\n");
        }
    }

    if (s->reset)
        s->reset(inst);
    printf("\n");
}

/* ── Main ─────────────────────────────────────────────────────────────── */

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--perf") == 0)
        return run_perf_mode(argc, argv);

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

    /* Resolve all 51 symbols. */
    printf("=== resolve ===\n");
    struct symbols sym;
    memset(&sym, 0, sizeof(sym));
    int missing = resolve_all(handle, &sym);

    if (missing > 0) {
        fprintf(stderr, "fatal: %d of 51 symbols missing\n", missing);
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

    /* Phase 2b — Remaining 27 symbols, after the full-pinyin cycle. */
    probe_remaining(&sym, ctx, inst);

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
