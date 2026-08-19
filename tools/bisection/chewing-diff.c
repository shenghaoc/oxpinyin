/*
 * chewing-diff.c — W13 STANDARD bopomofo differential driver.
 *
 * Drives the chewing entry points over a fixed keyboard input sequence:
 * parse length, parsed-input length, keyboard symbol lookups, candidate
 * list, and auxiliary text at every cursor.  Run against libpinyin.so
 * and libpinyin_capi.so and diff the logs.
 *
 * Usage:
 *   ./chewing-diff <path-to-so> <systemdir> [scheme]
 *
 * The scheme is the ZhuyinScheme header discriminant: 1 (STANDARD),
 * 3 (IBM), 4 (GINYIEH), 5 (ETEN).  Anything else is refused locally —
 * 7 (STANDARD_DVORAK) aborts the oracle and 30 aborts the double-pinyin
 * setter, so neither is ever sent.
 *
 * The corpus names zhuyin symbol sequences plus an optional tone number;
 * the keystroke strings are derived at startup from the selected
 * keyboard's pinned symbol/tone tables below (copied verbatim from
 * zhuyin_table.h at the pin 0c5e80e1) and cross-checked against the
 * library under test through pinyin_in_chewing_keyboard, so no
 * keystroke is hand-authored and drift between the embedded copy and
 * the library's own table fails the run loudly instead of feeding
 * wrong syllables.
 *
 * Scheme 1 runs the full W13 corpus; the other Simple keyboards run the
 * per-scheme corpus: tone-bearing, tone-rejection, shuffle (canonical
 * aux), one recovered live row, one dead row.
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <errno.h>
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

/* ── Pinned keyboard tables (zhuyin_table.h:9-204, marks :12593) ─────── */

struct symbol_row {
    char key;
    const char *symbol;
};

struct tone_row {
    char key;
    int tone;
    const char *mark;
};

static const struct symbol_row STANDARD_SYMBOLS[] = {
    { ',', "ㄝ" },
    { '-', "ㄦ" },
    { '.', "ㄡ" },
    { '/', "ㄥ" },
    { '0', "ㄢ" },
    { '1', "ㄅ" },
    { '2', "ㄉ" },
    { '5', "ㄓ" },
    { '8', "ㄚ" },
    { '9', "ㄞ" },
    { ';', "ㄤ" },
    { 'a', "ㄇ" },
    { 'b', "ㄖ" },
    { 'c', "ㄏ" },
    { 'd', "ㄎ" },
    { 'e', "ㄍ" },
    { 'f', "ㄑ" },
    { 'g', "ㄕ" },
    { 'h', "ㄘ" },
    { 'i', "ㄛ" },
    { 'j', "ㄨ" },
    { 'k', "ㄜ" },
    { 'l', "ㄠ" },
    { 'm', "ㄩ" },
    { 'n', "ㄙ" },
    { 'o', "ㄟ" },
    { 'p', "ㄣ" },
    { 'q', "ㄆ" },
    { 'r', "ㄐ" },
    { 's', "ㄋ" },
    { 't', "ㄔ" },
    { 'u', "ㄧ" },
    { 'v', "ㄒ" },
    { 'w', "ㄊ" },
    { 'x', "ㄌ" },
    { 'y', "ㄗ" },
    { 'z', "ㄈ" },
};

static const struct tone_row STANDARD_TONES[] = {
    { ' ', 1, " " },
    { '6', 2, "ˊ" },
    { '3', 3, "ˇ" },
    { '4', 4, "ˋ" },
    { '7', 5, "˙" },
};

static const struct symbol_row IBM_SYMBOLS[] = {
    { '-', "ㄏ" },
    { '0', "ㄎ" },
    { '1', "ㄅ" },
    { '2', "ㄆ" },
    { '3', "ㄇ" },
    { '4', "ㄈ" },
    { '5', "ㄉ" },
    { '6', "ㄊ" },
    { '7', "ㄋ" },
    { '8', "ㄌ" },
    { '9', "ㄍ" },
    { ';', "ㄠ" },
    { 'a', "ㄧ" },
    { 'b', "ㄥ" },
    { 'c', "ㄣ" },
    { 'd', "ㄩ" },
    { 'e', "ㄒ" },
    { 'f', "ㄚ" },
    { 'g', "ㄛ" },
    { 'h', "ㄜ" },
    { 'i', "ㄗ" },
    { 'j', "ㄝ" },
    { 'k', "ㄞ" },
    { 'l', "ㄟ" },
    { 'n', "ㄦ" },
    { 'o', "ㄘ" },
    { 'p', "ㄙ" },
    { 'q', "ㄐ" },
    { 'r', "ㄓ" },
    { 's', "ㄨ" },
    { 't', "ㄔ" },
    { 'u', "ㄖ" },
    { 'v', "ㄤ" },
    { 'w', "ㄑ" },
    { 'x', "ㄢ" },
    { 'y', "ㄕ" },
    { 'z', "ㄡ" },
};

static const struct tone_row IBM_TONES[] = {
    { ' ', 1, " " },
    { 'm', 2, "ˊ" },
    { ',', 3, "ˇ" },
    { '.', 4, "ˋ" },
    { '/', 5, "˙" },
};

static const struct symbol_row GINYIEH_SYMBOLS[] = {
    { ',', "ㄚ" },
    { '-', "ㄣ" },
    { '.', "ㄞ" },
    { '/', "ㄢ" },
    { '0', "ㄟ" },
    { '2', "ㄅ" },
    { '3', "ㄉ" },
    { '6', "ㄓ" },
    { '8', "ㄧ" },
    { '9', "ㄛ" },
    { ';', "ㄡ" },
    { '=', "ㄦ" },
    { '[', "ㄤ" },
    { '\'', "ㄥ" },
    { 'b', "ㄒ" },
    { 'c', "ㄌ" },
    { 'd', "ㄋ" },
    { 'e', "ㄊ" },
    { 'f', "ㄎ" },
    { 'g', "ㄑ" },
    { 'h', "ㄕ" },
    { 'i', "ㄨ" },
    { 'j', "ㄘ" },
    { 'k', "ㄩ" },
    { 'l', "ㄝ" },
    { 'm', "ㄙ" },
    { 'n', "ㄖ" },
    { 'o', "ㄜ" },
    { 'p', "ㄠ" },
    { 'r', "ㄍ" },
    { 's', "ㄇ" },
    { 't', "ㄐ" },
    { 'u', "ㄗ" },
    { 'v', "ㄏ" },
    { 'w', "ㄆ" },
    { 'x', "ㄈ" },
    { 'y', "ㄔ" },
};

static const struct tone_row GINYIEH_TONES[] = {
    { ' ', 1, " " },
    { 'q', 2, "ˊ" },
    { 'a', 3, "ˇ" },
    { 'z', 4, "ˋ" },
    { '1', 5, "˙" },
};

static const struct symbol_row ETEN_SYMBOLS[] = {
    { ',', "ㄓ" },
    { '-', "ㄥ" },
    { '.', "ㄔ" },
    { '/', "ㄕ" },
    { '0', "ㄤ" },
    { '7', "ㄑ" },
    { '8', "ㄢ" },
    { '9', "ㄣ" },
    { ';', "ㄗ" },
    { '=', "ㄦ" },
    { '\'', "ㄘ" },
    { 'a', "ㄚ" },
    { 'b', "ㄅ" },
    { 'c', "ㄒ" },
    { 'd', "ㄉ" },
    { 'e', "ㄧ" },
    { 'f', "ㄈ" },
    { 'g', "ㄐ" },
    { 'h', "ㄏ" },
    { 'i', "ㄞ" },
    { 'j', "ㄖ" },
    { 'k', "ㄎ" },
    { 'l', "ㄌ" },
    { 'm', "ㄇ" },
    { 'n', "ㄋ" },
    { 'o', "ㄛ" },
    { 'p', "ㄆ" },
    { 'q', "ㄟ" },
    { 'r', "ㄜ" },
    { 's', "ㄙ" },
    { 't', "ㄊ" },
    { 'u', "ㄩ" },
    { 'v', "ㄍ" },
    { 'w', "ㄝ" },
    { 'x', "ㄨ" },
    { 'y', "ㄡ" },
    { 'z', "ㄠ" },
};

static const struct tone_row ETEN_TONES[] = {
    { ' ', 1, " " },
    { '2', 2, "ˊ" },
    { '3', 3, "ˇ" },
    { '4', 4, "ˋ" },
    { '1', 5, "˙" },
};

static const struct symbol_row HSU_INITIALS[] = {
    { 'a', "ㄘ" },
    { 'b', "ㄅ" },
    { 'c', "ㄒ" },
    { 'c', "ㄕ" },
    { 'd', "ㄉ" },
    { 'f', "ㄈ" },
    { 'g', "ㄍ" },
    { 'h', "ㄏ" },
    { 'j', "ㄐ" },
    { 'j', "ㄓ" },
    { 'k', "ㄎ" },
    { 'l', "ㄌ" },
    { 'm', "ㄇ" },
    { 'n', "ㄋ" },
    { 'p', "ㄆ" },
    { 'r', "ㄖ" },
    { 's', "ㄙ" },
    { 't', "ㄊ" },
    { 'v', "ㄑ" },
    { 'v', "ㄔ" },
    { 'z', "ㄗ" },
};

static const struct symbol_row HSU_MIDDLES[] = {
    { 'e', "ㄧ" },
    { 'u', "ㄩ" },
    { 'x', "ㄨ" },
};

static const struct symbol_row HSU_FINALS[] = {
    { 'a', "ㄟ" },
    { 'e', "ㄝ" },
    { 'g', "ㄜ" },
    { 'h', "ㄛ" },
    { 'i', "ㄞ" },
    { 'k', "ㄤ" },
    { 'l', "ㄥ" },
    { 'l', "ㄦ" },
    { 'm', "ㄢ" },
    { 'n', "ㄣ" },
    { 'o', "ㄡ" },
    { 'w', "ㄠ" },
    { 'y', "ㄚ" },
};

static const struct tone_row HSU_TONES[] = {
    { ' ', 1, " " },
    { 'd', 2, "ˊ" },
    { 'f', 3, "ˇ" },
    { 'j', 4, "ˋ" },
    { 's', 5, "˙" },
};

static const struct symbol_row ETEN26_INITIALS[] = {
    { 'b', "ㄅ" },
    { 'c', "ㄒ" },
    { 'c', "ㄕ" },
    { 'd', "ㄉ" },
    { 'f', "ㄈ" },
    { 'g', "ㄐ" },
    { 'g', "ㄓ" },
    { 'h', "ㄏ" },
    { 'j', "ㄖ" },
    { 'k', "ㄎ" },
    { 'l', "ㄌ" },
    { 'm', "ㄇ" },
    { 'n', "ㄋ" },
    { 'p', "ㄆ" },
    { 'q', "ㄗ" },
    { 's', "ㄙ" },
    { 't', "ㄊ" },
    { 'v', "ㄍ" },
    { 'v', "ㄑ" },
    { 'w', "ㄘ" },
    { 'y', "ㄔ" },
};

static const struct symbol_row ETEN26_MIDDLES[] = {
    { 'e', "ㄧ" },
    { 'u', "ㄩ" },
    { 'x', "ㄨ" },
};

static const struct symbol_row ETEN26_FINALS[] = {
    { 'a', "ㄚ" },
    { 'h', "ㄦ" },
    { 'i', "ㄞ" },
    { 'l', "ㄥ" },
    { 'm', "ㄢ" },
    { 'n', "ㄣ" },
    { 'o', "ㄛ" },
    { 'p', "ㄡ" },
    { 'q', "ㄟ" },
    { 'r', "ㄜ" },
    { 't', "ㄤ" },
    { 'w', "ㄝ" },
    { 'z', "ㄠ" },
};

static const struct tone_row ETEN26_TONES[] = {
    { ' ', 1, " " },
    { 'd', 5, "˙" },
    { 'f', 2, "ˊ" },
    { 'j', 3, "ˇ" },
    { 'k', 4, "ˋ" },
};

static const struct symbol_row HSU_DVORAK_INITIALS[] = {
    { 'a', "ㄘ" },
    { 'b', "ㄅ" },
    { 'c', "ㄒ" },
    { 'c', "ㄕ" },
    { 'd', "ㄉ" },
    { 'f', "ㄈ" },
    { 'g', "ㄍ" },
    { 'h', "ㄏ" },
    { 'j', "ㄐ" },
    { 'j', "ㄓ" },
    { 'k', "ㄎ" },
    { 'l', "ㄌ" },
    { 'm', "ㄇ" },
    { 'n', "ㄋ" },
    { 'p', "ㄆ" },
    { 'r', "ㄖ" },
    { 's', "ㄙ" },
    { 't', "ㄊ" },
    { 'v', "ㄑ" },
    { 'v', "ㄔ" },
    { 'z', "ㄗ" },
};

static const struct symbol_row HSU_DVORAK_MIDDLES[] = {
    { 'e', "ㄧ" },
    { 'u', "ㄩ" },
    { 'x', "ㄨ" },
};

static const struct symbol_row HSU_DVORAK_FINALS[] = {
    { 'a', "ㄟ" },
    { 'e', "ㄝ" },
    { 'g', "ㄜ" },
    { 'h', "ㄛ" },
    { 'i', "ㄞ" },
    { 'k', "ㄤ" },
    { 'l', "ㄥ" },
    { 'l', "ㄦ" },
    { 'm', "ㄢ" },
    { 'n', "ㄣ" },
    { 'o', "ㄡ" },
    { 'w', "ㄠ" },
    { 'y', "ㄚ" },
};

static const struct tone_row HSU_DVORAK_TONES[] = {
    { ' ', 1, " " },
    { 'd', 2, "ˊ" },
    { 'f', 3, "ˇ" },
    { 'j', 4, "ˋ" },
    { 's', 5, "˙" },
};

/* KB_SIMPLE: one symbol table (first-match per key, single symbol).
 * KB_DISCRETE: initial/middle/final slot tables (a key may appear twice
 * in one table — the dual-mapped keys; parsing takes the first row,
 * display collects every row). */
struct keyboard {
    const struct symbol_row *symbols;
    size_t nsymbols;
    const struct symbol_row *initials;
    size_t ninitials;
    const struct symbol_row *middles;
    size_t nmiddles;
    const struct symbol_row *finals;
    size_t nfinals;
    const struct tone_row *tones;
    size_t ntones;
};

#define KB_SIMPLE(symarr, tonearr)                                       \
    {                                                                    \
        symarr, sizeof(symarr) / sizeof(symarr[0]),                      \
        NULL, 0, NULL, 0, NULL, 0,                                       \
        tonearr, sizeof(tonearr) / sizeof(tonearr[0]),                   \
    }

#define KB_DISCRETE(iniarr, midarr, finarr, tonearr)                     \
    {                                                                    \
        NULL, 0,                                                         \
        iniarr, sizeof(iniarr) / sizeof(iniarr[0]),                      \
        midarr, sizeof(midarr) / sizeof(midarr[0]),                      \
        finarr, sizeof(finarr) / sizeof(finarr[0]),                      \
        tonearr, sizeof(tonearr) / sizeof(tonearr[0]),                   \
    }

/* Indexed by scheme slot: 0 -> 1 (STANDARD), 1 -> 3 (IBM),
 * 2 -> 4 (GINYIEH), 3 -> 5 (ETEN), 4 -> 2 (HSU), 5 -> 6 (ETEN26),
 * 6 -> 8 (HSU_DVORAK). */
static const struct keyboard KEYBOARDS[] = {
    KB_SIMPLE(STANDARD_SYMBOLS, STANDARD_TONES),
    KB_SIMPLE(IBM_SYMBOLS, IBM_TONES),
    KB_SIMPLE(GINYIEH_SYMBOLS, GINYIEH_TONES),
    KB_SIMPLE(ETEN_SYMBOLS, ETEN_TONES),
    KB_DISCRETE(HSU_INITIALS, HSU_MIDDLES, HSU_FINALS, HSU_TONES),
    KB_DISCRETE(ETEN26_INITIALS, ETEN26_MIDDLES, ETEN26_FINALS, ETEN26_TONES),
    KB_DISCRETE(HSU_DVORAK_INITIALS, HSU_DVORAK_MIDDLES, HSU_DVORAK_FINALS,
                HSU_DVORAK_TONES),
};

/* The scheme -> slot map; only the implemented keyboards this driver
 * sweeps.  7 (STANDARD_DVORAK) and 30 never reach the oracle. */
static int scheme_slot(int scheme) {
    switch (scheme) {
    case 1: return 0;
    case 3: return 1;
    case 4: return 2;
    case 5: return 3;
    case 2: return 4;
    case 6: return 5;
    case 8: return 6;
    default: return -1;
    }
}

/* ── Corpus: zhuyin symbol sequences + optional tone (0 = none) ───────── */

struct corpus_entry {
    const char *symbols[6]; /* NULL-terminated zhuyin symbols */
    int tone;               /* 1..5, or 0 for tone-less */
};

/* One slot in `symbols` holds the NULL terminator, so an entry carries at
 * most (len - 1) symbol keys; add one optional tone key and the string NUL. */
#define MAX_SYMBOLS_PER_ENTRY \
    ((sizeof(((struct corpus_entry *)0)->symbols) / sizeof(const char *)) - 1)
#define KEYSTROKE_BUF (MAX_SYMBOLS_PER_ENTRY + 2)
_Static_assert(KEYSTROKE_BUF >= 3, "keystroke buffer too small");

/*
 * Full W13 corpus (scheme 1): the legacy keystroke set re-derived, the
 * rejection class, incomplete keys, the six permutations of ㄅㄧㄝ, a
 * two-symbol swap, shuffle+tone, a multi-key greedy boundary, the twelve
 * recovered rows with mask-valid and mask-invalid tones, and the five
 * dead rows.
 */
static const struct corpus_entry CORPUS[] = {
    { .symbols = { "ㄋ", "ㄧ", NULL }, .tone = 0 },
    { .symbols = { "ㄏ", "ㄠ", NULL }, .tone = 0 },
    { .symbols = { "ㄨ", "ㄛ", NULL }, .tone = 0 },
    { .symbols = { "ㄖ", "ㄣ", NULL }, .tone = 0 },
    { .symbols = { "ㄓ", "ㄨ", "ㄥ", NULL }, .tone = 0 },
    { .symbols = { "ㄕ", NULL }, .tone = 0 },
    { .symbols = { "ㄧ", NULL }, .tone = 0 },
    { .symbols = { "ㄋ", "ㄧ", NULL }, .tone = 2 },
    { .symbols = { "ㄌ", NULL }, .tone = 0 },
    { .symbols = { NULL }, .tone = 0 },
    /* Rejection class: each invalid case beside its valid control. */
    { .symbols = { "ㄋ", "ㄧ", NULL }, .tone = 1 },
    { .symbols = { "ㄋ", "ㄧ", "ㄌ", NULL }, .tone = 2 },
    { .symbols = { "ㄋ", "ㄧ", "ㄏ", "ㄠ", NULL }, .tone = 0 },
    { .symbols = { NULL }, .tone = 2 },
    { .symbols = { NULL }, .tone = 1 },
    /* Incomplete keys: matched under ZHUYIN_INCOMPLETE, then rejected
     * by the all-zero validity masks — consumed 0 both sides. */
    { .symbols = { "ㄅ", NULL }, .tone = 0 },
    /* Shuffle: every permutation of ㄅㄧㄝ (canonical first), a
     * two-symbol swap beside its canonical, shuffle composed with tone,
     * and a multi-key greedy boundary (ㄋㄧ + shuffled ㄅㄝㄧ). */
    { .symbols = { "ㄅ", "ㄧ", "ㄝ", NULL }, .tone = 0 },
    { .symbols = { "ㄅ", "ㄝ", "ㄧ", NULL }, .tone = 0 },
    { .symbols = { "ㄧ", "ㄅ", "ㄝ", NULL }, .tone = 0 },
    { .symbols = { "ㄧ", "ㄝ", "ㄅ", NULL }, .tone = 0 },
    { .symbols = { "ㄝ", "ㄅ", "ㄧ", NULL }, .tone = 0 },
    { .symbols = { "ㄝ", "ㄧ", "ㄅ", NULL }, .tone = 0 },
    { .symbols = { "ㄅ", "ㄧ", NULL }, .tone = 0 },
    { .symbols = { "ㄧ", "ㄅ", NULL }, .tone = 0 },
    { .symbols = { "ㄅ", "ㄝ", "ㄧ", NULL }, .tone = 4 },
    { .symbols = { "ㄋ", "ㄧ", "ㄅ", "ㄝ", "ㄧ", NULL }, .tone = 0 },
    /* Recovered rows (12 zhuyin_index spellings absent from the old
     * 405-row map): a mask-valid and a mask-invalid tone each.  ㄥ
     * (eng) is the rare row whose first tone is VALID — pinned beside
     * ㄋㄧ's illegal first tone above. */
    { .symbols = { "ㄉ", "ㄣ", NULL }, .tone = 4 },
    { .symbols = { "ㄉ", "ㄣ", NULL }, .tone = 1 },
    { .symbols = { "ㄓ", "ㄟ", NULL }, .tone = 4 },
    { .symbols = { "ㄓ", "ㄟ", NULL }, .tone = 1 },
    { .symbols = { "ㄋ", "ㄧ", "ㄚ", NULL }, .tone = 2 },
    { .symbols = { "ㄋ", "ㄧ", "ㄚ", NULL }, .tone = 1 },
    { .symbols = { "ㄧ", "ㄞ", NULL }, .tone = 2 },
    { .symbols = { "ㄧ", "ㄞ", NULL }, .tone = 1 },
    { .symbols = { "ㄔ", "ㄨ", "ㄚ", NULL }, .tone = 3 },
    { .symbols = { "ㄔ", "ㄨ", "ㄚ", NULL }, .tone = 2 },
    { .symbols = { "ㄋ", "ㄨ", "ㄣ", NULL }, .tone = 4 },
    { .symbols = { "ㄋ", "ㄨ", "ㄣ", NULL }, .tone = 1 },
    { .symbols = { "ㄥ", NULL }, .tone = 1 },
    { .symbols = { "ㄥ", NULL }, .tone = 0 },
    /* Dead rows (mask 0): matched, then the post-match validity break
     * consumes nothing. */
    { .symbols = { "ㄈ", "ㄜ", NULL }, .tone = 0 },
    { .symbols = { "ㄎ", "ㄟ", NULL }, .tone = 0 },
    { .symbols = { "ㄉ", "ㄧ", "ㄣ", NULL }, .tone = 0 },
    { .symbols = { "ㄌ", "ㄣ", NULL }, .tone = 0 },
    { .symbols = { "ㄖ", "ㄨ", "ㄚ", NULL }, .tone = 0 },
};

/*
 * Per-scheme corpus for the added Simple keyboards (IBM, GINYIEH,
 * ETEN): tone-bearing, tone-rejection, shuffle with its canonical aux,
 * one recovered live row (both tones), one dead row, and the empty
 * input.
 */
static const struct corpus_entry SIMPLE_CORPUS[] = {
    { .symbols = { "ㄋ", "ㄧ", NULL }, .tone = 0 },
    { .symbols = { "ㄋ", "ㄧ", NULL }, .tone = 2 },
    { .symbols = { "ㄋ", "ㄧ", NULL }, .tone = 1 },
    { .symbols = { "ㄅ", "ㄧ", "ㄝ", NULL }, .tone = 0 },
    { .symbols = { "ㄅ", "ㄝ", "ㄧ", NULL }, .tone = 0 },
    { .symbols = { "ㄅ", "ㄝ", "ㄧ", NULL }, .tone = 4 },
    { .symbols = { "ㄉ", "ㄣ", NULL }, .tone = 4 },
    { .symbols = { "ㄉ", "ㄣ", NULL }, .tone = 1 },
    { .symbols = { "ㄈ", "ㄜ", NULL }, .tone = 0 },
    { .symbols = { NULL }, .tone = 0 },
};

/*
 * Per-scheme corpus for the Discrete keyboards (HSU, ETEN26,
 * HSU_DVORAK), shared because all three bind the same 37 symbols:
 * canonical control, tone-bearing, tone-rejection, the dual-key
 * ambiguity (ㄒ typed alone is the keyboard's shi remap; with a middle
 * it is the plain xi), and the keyboard-specific correction rows — the
 * same ㄍㄧ keystrokes are ji on the HSU keyboards and qi on ETEN26,
 * and ㄆ is ETEN26's ou stand-in but an incomplete row on HSU.
 */
static const struct corpus_entry DISCRETE_CORPUS[] = {
    { .symbols = { "ㄋ", "ㄧ", NULL }, .tone = 0 },
    { .symbols = { "ㄋ", "ㄧ", NULL }, .tone = 2 },
    { .symbols = { "ㄋ", "ㄧ", NULL }, .tone = 1 },
    { .symbols = { "ㄒ", NULL }, .tone = 0 },
    { .symbols = { "ㄒ", "ㄧ", NULL }, .tone = 0 },
    { .symbols = { "ㄒ", "ㄨ", NULL }, .tone = 0 },
    { .symbols = { "ㄍ", "ㄧ", NULL }, .tone = 0 },
    { .symbols = { "ㄐ", "ㄨ", "ㄥ", NULL }, .tone = 0 },
    { .symbols = { "ㄇ", NULL }, .tone = 0 },
    { .symbols = { "ㄆ", NULL }, .tone = 0 },
    { .symbols = { "ㄅ", NULL }, .tone = 0 },
    { .symbols = { NULL }, .tone = 0 },
};

/* ── Keystroke derivation ─────────────────────────────────────────────── */

static char key_for_symbol(const struct keyboard *kb, const char *symbol) {
    for (size_t i = 0; i < kb->nsymbols; i++)
        if (strcmp(kb->symbols[i].symbol, symbol) == 0)
            return kb->symbols[i].key;
    fprintf(stderr, "no key on this keyboard spells %s\n", symbol);
    exit(1);
}

/* The key that makes a Discrete positional probe read `symbol` at
 * `slot`: only each table's first row per key can be parsed (a dual
 * key's second row is display-only), and the slot's probe order is
 * initials -> middles -> finals starting at `slot`. */
static char key_for_discrete_slot(const struct keyboard *kb, size_t slot,
                                  const char *symbol) {
    const struct symbol_row *tables[3];
    size_t counts[3];
    size_t n = 0;
    if (slot == 0 && kb->ninitials) {
        tables[n] = kb->initials;
        counts[n] = kb->ninitials;
        n++;
    }
    if (slot <= 1 && kb->nmiddles) {
        tables[n] = kb->middles;
        counts[n] = kb->nmiddles;
        n++;
    }
    if (kb->nfinals) {
        tables[n] = kb->finals;
        counts[n] = kb->nfinals;
        n++;
    }
    for (size_t t = 0; t < n; t++) {
        char last = 0;
        bool have_last = false;
        for (size_t i = 0; i < counts[t]; i++) {
            char key = tables[t][i].key;
            if (have_last && last == key)
                continue;
            last = key;
            have_last = true;
            if (strcmp(tables[t][i].symbol, symbol) == 0)
                return key;
        }
    }
    fprintf(stderr, "no key on this keyboard derives %s at slot %zu\n",
            symbol, slot);
    exit(1);
}

static char key_for_tone(const struct keyboard *kb, int tone) {
    for (size_t i = 0; i < kb->ntones; i++)
        if (kb->tones[i].tone == tone)
            return kb->tones[i].key;
    fprintf(stderr, "no key on this keyboard spells tone %d\n", tone);
    exit(1);
}

/* Derives the keystroke string for one corpus entry into `out`, which must
 * hold at least KEYSTROKE_BUF bytes. */
static void derive_keystrokes(const struct keyboard *kb,
                              const struct corpus_entry *entry, char *out) {
    size_t len = 0;
    size_t slot = 0;
    for (const char *const *symbol = entry->symbols; symbol && *symbol; symbol++) {
        if (kb->symbols)
            out[len++] = key_for_symbol(kb, *symbol);
        else
            out[len++] = key_for_discrete_slot(kb, slot, *symbol);
        slot++;
    }
    if (entry->tone != 0)
        out[len++] = key_for_tone(kb, entry->tone);
    out[len] = '\0';
}

/* Cross-checks the embedded tables of the selected keyboard against the
 * library under test: every embedded symbol key must report exactly its
 * symbol, every tone key its mark, and non-keys must report false.
 * Returns 0 when the library's keyboard matches the pinned copy. */
static int verify_keyboard_tables(const struct symbols *s,
                                  pinyin_instance_t *inst,
                                  const struct keyboard *kb) {
    int failures = 0;

    for (size_t i = 0; i < kb->nsymbols; i++) {
        gchar **symbols = NULL;
        bool ok = s->in_chewing_keyboard(inst, kb->symbols[i].key, &symbols);
        if (!ok || !symbols || !symbols[0] ||
            strcmp(symbols[0], kb->symbols[i].symbol) != 0 || symbols[1]) {
            printf("table-check: symbol key '%c' mismatch\n",
                   kb->symbols[i].key);
            failures++;
        }
        free_strv(symbols);
    }
    /* Discrete slot tables: every row's symbol (both rows of a dual
     * key) must appear in the key's reported vector, and tone keys must
     * report their mark. */
    for (int slot = 0; slot < 3; slot++) {
        const struct symbol_row *table;
        size_t count;
        switch (slot) {
        case 0: table = kb->initials; count = kb->ninitials; break;
        case 1: table = kb->middles; count = kb->nmiddles; break;
        default: table = kb->finals; count = kb->nfinals; break;
        }
        for (size_t i = 0; i < count; i++) {
            gchar **symbols = NULL;
            bool ok = s->in_chewing_keyboard(inst, table[i].key, &symbols);
            bool found = false;
            if (ok && symbols) {
                for (gchar **q = symbols; *q; q++)
                    if (strcmp(*q, table[i].symbol) == 0)
                        found = true;
            }
            if (!found) {
                printf("table-check: slot-%d key '%c' missing symbol %s\n",
                       slot, table[i].key, table[i].symbol);
                failures++;
            }
            free_strv(symbols);
        }
    }
    for (size_t i = 0; i < kb->ntones; i++) {
        gchar **symbols = NULL;
        bool ok = s->in_chewing_keyboard(inst, kb->tones[i].key, &symbols);
        bool found = false;
        if (ok && symbols) {
            for (gchar **q = symbols; *q; q++)
                if (strcmp(*q, kb->tones[i].mark) == 0)
                    found = true;
        }
        if (!found) {
            printf("table-check: tone key '%c' missing mark\n",
                   kb->tones[i].key);
            failures++;
        }
        free_strv(symbols);
    }
    for (const char *nonkey = "!EQ^"; *nonkey; nonkey++) {
        gchar **symbols = NULL;
        if (s->in_chewing_keyboard(inst, *nonkey, &symbols)) {
            printf("table-check: non-key '%c' accepted\n", *nonkey);
            failures++;
        }
        free_strv(symbols);
    }
    return failures;
}

/* ── Drive ────────────────────────────────────────────────────────────── */

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
    int scheme = 1;
    if (argc > 3) {
        /* Reject everything atoi would silently accept: trailing text
         * ("2junk"), empty values, conversion errors, overflow — a
         * typoed CLI arg must not quietly run the wrong scheme (the
         * same rule nbest-train-diff's parse_rounds_env applies). */
        errno = 0;
        char *end = NULL;
        long value = strtol(argv[3], &end, 10);
        if (errno != 0 || end == argv[3] || *end != '\0') {
            scheme = -1;
        } else {
            scheme = (int)value;
        }
    }
    int slot = scheme_slot(scheme);
    if (argc < 3 || slot < 0) {
        fprintf(stderr,
                "usage: %s <so> <systemdir> [scheme]\n"
                "  scheme: 1 STANDARD, 2 HSU, 3 IBM, 4 GINYIEH, 5 ETEN,\n"
                "          6 ETEN26, 8 HSU_DVORAK (7 and 30 are refused)\n",
                argv[0]);
        return 1;
    }
    const struct keyboard *kb = &KEYBOARDS[slot];

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
    bool scheme_selected = s.set_zhuyin_scheme(ctx, scheme);
    printf("set_zhuyin_scheme(%d): %s\n", scheme,
           scheme_selected ? "true" : "false");
    if (!scheme_selected) {
        fprintf(stderr, "set_zhuyin_scheme(%d) rejected\n", scheme);
        s.fini(ctx);
        dlclose(handle);
        return 1;
    }

    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        s.fini(ctx);
        dlclose(handle);
        return 1;
    }

    if (verify_keyboard_tables(&s, inst, kb) != 0) {
        fprintf(stderr, "embedded keyboard tables disagree with the library\n");
        s.free_instance(inst);
        s.fini(ctx);
        dlclose(handle);
        return 1;
    }
    printf("table-check: ok\n");

    const struct corpus_entry *corpus;
    size_t ncorpus;
    if (scheme == 1) {
        corpus = CORPUS;
        ncorpus = sizeof(CORPUS) / sizeof(CORPUS[0]);
    } else if (scheme == 2 || scheme == 6 || scheme == 8) {
        corpus = DISCRETE_CORPUS;
        ncorpus = sizeof(DISCRETE_CORPUS) / sizeof(DISCRETE_CORPUS[0]);
    } else {
        corpus = SIMPLE_CORPUS;
        ncorpus = sizeof(SIMPLE_CORPUS) / sizeof(SIMPLE_CORPUS[0]);
    }

    for (size_t i = 0; i < ncorpus; i++) {
        char keystrokes[KEYSTROKE_BUF];
        derive_keystrokes(kb, &corpus[i], keystrokes);
        drive_input(&s, inst, keystrokes);
    }

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    return 0;
}
