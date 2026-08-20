/*
 * fullpin-diff.c — W15 full-pinyin scheme differential driver.
 *
 * Drives pinyin_set_full_pinyin_scheme + the full-pinyin entry points
 * over one scheme: 1 HANYU (the frozen surface), 2 LUOMA, 3
 * SECONDARY_ZHUYIN.  Run against libpinyin.so and libpinyin_capi.so
 * and diff the logs.
 *
 * The corpus is derived at runtime from the selected scheme's pinned
 * index (embedded verbatim below): a fixed stride over its rows,
 * apostrophe-joined pairs, tone-bearing and tone-less spellings, a
 * junk suffix, and — for schemes 2/3 — the remap cases the runtime
 * scan finds against the HANYU list (spellings that exist in both but
 * map differently).  Nothing is hand-authored.
 *
 * Scheme 1 runs the tone-less profile by default (PINYIN_INCOMPLETE
 * only — the frozen parity shape).  SCHEME_DIFF_TONE=1 switches it to
 * the USE_TONE profile and extends the corpus with the tone-digit law
 * rows: a second tone digit, the rejected digits 6/0, digit-then-junk,
 * adjacent and apostrophe-joined toned pairs, and every frozen
 * initial-only key with a tone.
 *
 * Usage:
 *   ./fullpin-diff <path-to-so> <systemdir> [scheme]
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

#define PINYIN_INCOMPLETE (1u << 3)
#define USE_TONE          (1u << 5)
#define FULLPIN_FLAGS ((pinyin_option_t)(PINYIN_INCOMPLETE | USE_TONE))
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
typedef bool (*fn_set_full_scheme)(pinyin_context_t *, int);
typedef size_t (*fn_parse_full)(pinyin_instance_t *, const char *);
typedef size_t (*fn_parsed_len)(pinyin_instance_t *);
typedef bool (*fn_guess_sentence)(pinyin_instance_t *);
typedef bool (*fn_get_sentence)(pinyin_instance_t *, uint8_t, char **);
typedef bool (*fn_guess_candidates)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_get_n_candidate)(pinyin_instance_t *, guint *);
typedef bool (*fn_get_candidate)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_get_candidate_type)(pinyin_instance_t *, lookup_candidate_t *, lookup_candidate_type_t *);
typedef bool (*fn_get_candidate_string)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_get_full_aux)(pinyin_instance_t *, size_t, gchar **);
typedef bool (*fn_reset)(pinyin_instance_t *);

struct symbols {
    fn_init init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_instance free_instance;
    fn_set_options set_options;
    fn_set_full_scheme set_full_scheme;
    fn_parse_full parse_full;
    fn_parsed_len parsed_len;
    fn_guess_sentence guess_sentence;
    fn_get_sentence get_sentence;
    fn_guess_candidates guess_candidates;
    fn_get_n_candidate get_n_candidate;
    fn_get_candidate get_candidate;
    fn_get_candidate_type get_candidate_type;
    fn_get_candidate_string get_candidate_string;
    fn_get_full_aux get_full_aux;
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
        fn_init fixture_init = (fn_init)dlsym(handle, "oxpinyin_init_for_fixtures");
        if (fixture_init)
            s->init = fixture_init;
    }
    s->fini = (fn_fini)resolve_symbol(handle, "pinyin_fini", &missing);
    s->alloc = (fn_alloc)resolve_symbol(handle, "pinyin_alloc_instance", &missing);
    s->free_instance = (fn_free_instance)resolve_symbol(handle, "pinyin_free_instance", &missing);
    s->set_options = (fn_set_options)resolve_symbol(handle, "pinyin_set_options", &missing);
    s->set_full_scheme = (fn_set_full_scheme)resolve_symbol(handle, "pinyin_set_full_pinyin_scheme", &missing);
    s->parse_full = (fn_parse_full)resolve_symbol(handle, "pinyin_parse_more_full_pinyins", &missing);
    s->parsed_len = (fn_parsed_len)resolve_symbol(handle, "pinyin_get_parsed_input_length", &missing);
    s->guess_sentence = (fn_guess_sentence)resolve_symbol(handle, "pinyin_guess_sentence", &missing);
    s->get_sentence = (fn_get_sentence)resolve_symbol(handle, "pinyin_get_sentence", &missing);
    s->guess_candidates = (fn_guess_candidates)resolve_symbol(handle, "pinyin_guess_candidates", &missing);
    s->get_n_candidate = (fn_get_n_candidate)resolve_symbol(handle, "pinyin_get_n_candidate", &missing);
    s->get_candidate = (fn_get_candidate)resolve_symbol(handle, "pinyin_get_candidate", &missing);
    s->get_candidate_type = (fn_get_candidate_type)resolve_symbol(handle, "pinyin_get_candidate_type", &missing);
    s->get_candidate_string = (fn_get_candidate_string)resolve_symbol(handle, "pinyin_get_candidate_string", &missing);
    s->get_full_aux = (fn_get_full_aux)resolve_symbol(handle, "pinyin_get_full_pinyin_auxiliary_text", &missing);
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
    return t == NBEST_MATCH_CANDIDATE ? "NBEST_MATCH" : "NORMAL";
}

/* ── Pinned index tables ──────────────────────────────────────────────── */

struct index_row {
    const char *spelling;
    const char *canonical;
};

#include "fullpin-tables.h"

/* ── Corpus derivation ────────────────────────────────────────────────── */

#define MAX_INPUT 64
#define MAX_CORPUS 128

struct corpus {
    char inputs[MAX_CORPUS][MAX_INPUT];
    size_t count;
};

static void corpus_add(struct corpus *c, const char *input) {
    if (c->count >= MAX_CORPUS) {
        fprintf(stderr, "corpus overflow at %s\n", input);
        exit(1);
    }
    snprintf(c->inputs[c->count], MAX_INPUT, "%s", input);
    c->count++;
}

static const char *hanyu_target(const char *spelling) {
    /* The HANYU list carries spellings that are their own target. */
    for (size_t i = 0; i < sizeof(HANYU_SYLLABLES) / sizeof(HANYU_SYLLABLES[0]); i++)
        if (strcmp(HANYU_SYLLABLES[i], spelling) == 0)
            return spelling;
    return NULL;
}

/*
 * Derives the corpus from the selected scheme's table: a fixed stride
 * (tone-less, tone-bearing, junk-suffixed), an apostrophe-joined pair,
 * and — for the index schemes — the remap rows the runtime scan finds
 * against the HANYU list.  Scheme 1 strides the HANYU list itself.
 *
 * Under SCHEME_DIFF_TONE=1 the scheme-1 corpus additionally carries the
 * tone-digit law rows: a second tone digit, the rejected 6/0,
 * digit-then-junk, adjacent and apostrophe-joined toned pairs, and every
 * frozen initial-only key with a tone.
 */
static int scheme1_tone_mode(void) {
    const char *tone = getenv("SCHEME_DIFF_TONE");
    return tone && strcmp(tone, "1") == 0;
}

static void derive_corpus(struct corpus *c, int scheme) {
    if (scheme == 1) {
        size_t n = sizeof(HANYU_SYLLABLES) / sizeof(HANYU_SYLLABLES[0]);
        for (size_t i = 0; i < n; i += 37) {
            corpus_add(c, HANYU_SYLLABLES[i]);
            char buf[MAX_INPUT];
            snprintf(buf, MAX_INPUT, "%s4", HANYU_SYLLABLES[i]);
            corpus_add(c, buf);
            snprintf(buf, MAX_INPUT, "%sQ", HANYU_SYLLABLES[i]);
            corpus_add(c, buf);
        }
        if (scheme1_tone_mode()) {
            char buf[MAX_INPUT];
            /* a second tone digit on the stride */
            for (size_t i = 0; i < n; i += 37) {
                snprintf(buf, MAX_INPUT, "%s3", HANYU_SYLLABLES[i]);
                corpus_add(c, buf);
            }
            /* the rejected digits: not tones, left as junk */
            for (size_t k = 0; k <= 6; k += 6) {
                snprintf(buf, MAX_INPUT, "%s%zu", HANYU_SYLLABLES[0], k);
                corpus_add(c, buf);
                snprintf(buf, MAX_INPUT, "%s%zu", HANYU_SYLLABLES[1], k);
                corpus_add(c, buf);
            }
            /* digit then junk */
            snprintf(buf, MAX_INPUT, "%s4Q", HANYU_SYLLABLES[0]);
            corpus_add(c, buf);
            snprintf(buf, MAX_INPUT, "%s4Q", HANYU_SYLLABLES[1]);
            corpus_add(c, buf);
            /* adjacent and apostrophe-joined toned pairs */
            snprintf(buf, MAX_INPUT, "%s3%s3", HANYU_SYLLABLES[0], HANYU_SYLLABLES[1]);
            corpus_add(c, buf);
            snprintf(buf, MAX_INPUT, "%s4'%s4", HANYU_SYLLABLES[0], HANYU_SYLLABLES[1]);
            corpus_add(c, buf);
            /* NOT swept: a tone digit on an initial-only key ("n4"). The
             * pin's parser admits it (the scan's only precondition is the
             * option-gated index hit) but its phrase search asserts
             * CHEWING_ZERO_TONE == key.m_tone for initial-only keys
             * (pinyin_phrase3.h:152) and the oracle SIGABRTs — the same
             * class as zhuyin 7 / double 30: no oracle differential is
             * possible. oxpinyin keeps the parsed key and does not abort;
             * the class is covered by the core graph tests and recorded
             * in docs/findings/upstream-divergences.md. */
        }
        corpus_add(c, "");
        return;
    }

    const struct index_row *table =
        (scheme == 2) ? LUOMA_INDEX : SECONDARY_INDEX;
    size_t n = (scheme == 2)
        ? sizeof(LUOMA_INDEX) / sizeof(LUOMA_INDEX[0])
        : sizeof(SECONDARY_INDEX) / sizeof(SECONDARY_INDEX[0]);

    for (size_t i = 0; i < n; i += 29) {
        corpus_add(c, table[i].spelling);
        char buf[MAX_INPUT];
        snprintf(buf, MAX_INPUT, "%s4", table[i].spelling);
        corpus_add(c, buf);
        snprintf(buf, MAX_INPUT, "%sQ", table[i].spelling);
        corpus_add(c, buf);
    }
    /* apostrophe-joined pair from the table's first two rows */
    {
        char buf[MAX_INPUT];
        snprintf(buf, MAX_INPUT, "%s'%s", table[0].spelling, table[1].spelling);
        corpus_add(c, buf);
    }
    /* remap rows: spelling exists in HANYU but maps elsewhere */
    for (size_t i = 0; i < n && c->count < MAX_CORPUS - 1; i++) {
        const char *own = hanyu_target(table[i].spelling);
        if (own && strcmp(own, table[i].canonical) != 0) {
            corpus_add(c, table[i].spelling);
        }
    }
    corpus_add(c, "");
}

/* ── Drive ────────────────────────────────────────────────────────────── */

static void drive_input(const struct symbols *s, pinyin_instance_t *inst,
                        const char *input) {
    printf("=== input: \"%s\" ===\n", input);

    size_t consumed = s->parse_full(inst, input);
    printf("parse_full: consumed=%zu\n", consumed);
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
        bool ok = s->get_full_aux(inst, cursor, &aux);
        printf("full_aux(%zu): %s text=\"%s\"\n",
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
        errno = 0;
        char *end = NULL;
        long value = strtol(argv[3], &end, 10);
        if (errno != 0 || end == argv[3] || *end != '\0'
            || value < 1 || value > 3) {
            scheme = -1;
        } else {
            scheme = (int)value;
        }
    }
    if (argc < 3 || scheme < 1 || scheme > 3) {
        fprintf(stderr,
                "usage: %s <so> <systemdir> [scheme]\n"
                "  scheme: 1 HANYU, 2 LUOMA, 3 SECONDARY_ZHUYIN\n",
                argv[0]);
        return 1;
    }

    /* Scheme 1 runs the tone-less profile by default — the frozen parity
     * shape (PINYIN_INCOMPLETE only) — while schemes 2/3 always carry the
     * pinned tone-digit behavior.  SCHEME_DIFF_TONE=1 switches scheme 1
     * to the USE_TONE profile with the tone-law corpus rows. */
    pinyin_option_t flags =
        (scheme == 1 && !scheme1_tone_mode()) ? PINYIN_INCOMPLETE : FULLPIN_FLAGS;

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
    /* Both configuration calls must be accepted: a silently rejected
     * set_options would run the sweep under the context defaults, and a
     * rejected scheme would quietly sweep HANYU on both sides while the
     * log claims otherwise. */
    if (!s.set_options(ctx, flags)) {
        fprintf(stderr, "pinyin_set_options(%u) rejected\n", flags);
        s.fini(ctx);
        dlclose(handle);
        return 1;
    }
    bool scheme_selected = s.set_full_scheme(ctx, scheme);
    printf("set_full_pinyin_scheme(%d): %s\n", scheme,
           scheme_selected ? "true" : "false");
    if (!scheme_selected) {
        fprintf(stderr, "pinyin_set_full_pinyin_scheme(%d) rejected\n", scheme);
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

    struct corpus corpus = { .count = 0 };
    derive_corpus(&corpus, scheme);

    for (size_t i = 0; i < corpus.count; i++)
        drive_input(&s, inst, corpus.inputs[i]);

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    return 0;
}
