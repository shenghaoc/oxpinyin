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
typedef int (*fn_choose)(zhuyin_instance_t *, size_t, lookup_candidate_t *);
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
    fn_choose choose_candidate;
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
    s->choose_candidate = (fn_choose)resolve_symbol(handle, "zhuyin_choose_candidate", &missing);
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
    "su3",          /* ㄋㄧˇ */
    "su3u3",        /* ㄋㄧˇ ㄧˇ — two-syllable; exercises before-cursor at the
                     * terminal offset (the multi-syllable engine gap: the pin
                     * returns the last syllable's candidates + sentence rows,
                     * oxpinyin the composition-anchored subset) */
    "su3u3u3",      /* ㄋㄧˇ ㄧˇ ㄧˇ — three-syllable; the before-cursor walk
                     * must pool spans ending at each key boundary across
                     * three syllables */
    "zhang",        /* ㄓㄤ (no tone) -- the W13 live row */
    "zhan",         /* ㄓㄢ */
    "n",            /* initial-only ㄋ */
    "u",            /* middle-only ㄧ */
    "wu",           /* ㄨ */
    "yi",           /* ㄧ */
    "nin",          /* ㄋㄧㄣ */
    "hao",          /* ㄏㄠ */
};

static void dump_candidates(const struct symbols *s, zhuyin_instance_t *inst) {
    guint n = 0;
    if (!s->get_n_candidate(inst, &n)) {
        printf("n_candidates: FAILED (zhuyin_get_n_candidate returned false)\n");
        return;
    }
    printf("n_candidates: %u\n", n);
    /* The full list, not a head prefix: the n-best row-count divergence
     * also shifts the phrase tail (a phrase whose text equals a non-first
     * n-best row is dropped by the pinyin-law dedup where the pin's
     * zhuyin display law keeps it), so a 12-row dump hides the damage. */
    for (guint k = 0; k < n; k++) {
        lookup_candidate_t *cand = NULL;
        if (!s->get_candidate(inst, k, &cand) || !cand) {
            printf("  candidate[%u]: FAILED\n", k);
            continue;
        }
        const gchar *text = NULL;
        if (!s->get_candidate_string(inst, cand, &text)) {
            printf("  candidate[%u]: FAILED (zhuyin_get_candidate_string returned false)\n", k);
            continue;
        }
        lookup_candidate_type_t ctype = NORMAL_CANDIDATE_AFTER_CURSOR;
        if (!s->get_candidate_type(inst, cand, &ctype)) {
            printf("  candidate[%u]: FAILED (zhuyin_get_candidate_type returned false)\n", k);
            continue;
        }
        printf("  candidate[%u]: type=%s text=\"%s\"\n",
               k, ctype_name(ctype), text ? text : "(null)");
    }
}

/* The choose battery's corpus — multi-syllable compositions whose
 * before(consumed) window holds phrase rows whose span STARTS after the
 * composition start.  That is the shape the before-cursor choose residual
 * registers (upstream constrains [m_begin, m_end) and answers m_begin as
 * the cursor; oxpinyin records [0, offset) because its engine Candidate
 * carries no span start, and answers the offset): choosing row 1 at
 * before(consumed) on a multi-syllable composition exercises it, and on a
 * single-syllable composition every span also starts at 0, so nothing
 * would be exercised. */
static const char *CHOOSE_CORPUS[] = {
    "su3cl3",   /* ㄋㄧˇㄏㄠˇ (ni3 hao3) — the residual's canonical input:
                 * before(6) row 1 is 好, the SECOND key's span [3,6) */
    "su3u3",
    "su3u3u3",
};

static void dump_sentence(const struct symbols *s, zhuyin_instance_t *inst,
                          const char *label) {
    char *sent = NULL;
    bool ok = s->get_sentence && s->get_sentence(inst, &sent);
    printf("%s: %s text=\"%s\"\n", label, ok ? "true" : "false",
           ok && sent ? sent : "(null)");
    if (sent)
        g_free_fn(sent);
}

/* The head of the list only: the residual lives in row 1's span start,
 * and the pin's before(consumed) window runs to hundreds of rows (600 on
 * su3u3) — a full dump would bury the choose record.  The full dump is
 * the standard battery's job (dump_candidates above). */
static void dump_candidates_head(const struct symbols *s,
                                 zhuyin_instance_t *inst, guint head) {
    guint n = 0;
    if (!s->get_n_candidate(inst, &n)) {
        printf("n_candidates: FAILED (zhuyin_get_n_candidate returned false)\n");
        return;
    }
    printf("n_candidates: %u\n", n);
    for (guint k = 0; k < n && k < head; k++) {
        lookup_candidate_t *cand = NULL;
        if (!s->get_candidate(inst, k, &cand) || !cand) {
            printf("  candidate[%u]: FAILED\n", k);
            continue;
        }
        const gchar *text = NULL;
        if (!s->get_candidate_string(inst, cand, &text)) {
            printf("  candidate[%u]: FAILED (zhuyin_get_candidate_string returned false)\n", k);
            continue;
        }
        lookup_candidate_type_t ctype = NORMAL_CANDIDATE_AFTER_CURSOR;
        if (!s->get_candidate_type(inst, cand, &ctype)) {
            printf("  candidate[%u]: FAILED (zhuyin_get_candidate_type returned false)\n", k);
            continue;
        }
        printf("  candidate[%u]: type=%s text=\"%s\"\n",
               k, ctype_name(ctype), text ? text : "(null)");
    }
}

/* The before-cursor choose battery: parse, guess (the consumer protocol),
 * then guess_before(consumed), choose row 1, and record what each side
 * answers as the cursor and what its sentence holds right after the
 * choose and after the next re-decode (zhuyin_guess_sentence).  Upstream
 * re-decodes on the next guess — constrain-and-re-decode — so the
 * after-re-guess sentence is where a span-start difference shows up in
 * the conversion, not just the cursor. */
static void choose_battery(const struct symbols *s, zhuyin_instance_t *inst,
                           const char *input) {
    printf("=== input: \"%s\" ===\n", input);
    size_t consumed = s->parse_chewing(inst, input);
    printf("parse_chewing: consumed=%zu\n", consumed);

    bool gs = s->guess_sentence(inst);
    printf("guess_sentence: %s\n", gs ? "true" : "false");
    dump_sentence(s, inst, "get_sentence (baseline)");

    if (!s->guess_before(inst, consumed)) {
        printf("guess_before(%zu): false\n", consumed);
        s->reset(inst);
        printf("\n");
        return;
    }
    printf("guess_before(%zu): true\n", consumed);
    dump_candidates_head(s, inst, 8);

    lookup_candidate_t *cand = NULL;
    if (!s->get_candidate(inst, 1, &cand) || !cand) {
        printf("choose row 1: FAILED (zhuyin_get_candidate returned no row)\n");
        s->reset(inst);
        printf("\n");
        return;
    }
    const gchar *text = NULL;
    if (s->get_candidate_string(inst, cand, &text))
        printf("  chosen row string: \"%s\"\n", text ? text : "(null)");
    lookup_candidate_type_t ctype = NORMAL_CANDIDATE_AFTER_CURSOR;
    if (s->get_candidate_type(inst, cand, &ctype))
        printf("  chosen row type: %s\n", ctype_name(ctype));

    int cursor = s->choose_candidate(inst, consumed, cand);
    printf("zhuyin_choose_candidate(%zu, row 1) -> cursor: %d\n", consumed,
           cursor);
    dump_sentence(s, inst, "get_sentence (after choose)");

    bool reguessed = s->guess_sentence(inst);
    printf("guess_sentence (re-decode): %s\n", reguessed ? "true" : "false");
    dump_sentence(s, inst, "get_sentence (after re-guess)");

    s->reset(inst);
    printf("\n");
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: %s <libzhuyin.so> <systemdir> [userdir] [noguess] [choose]\n", argv[0]);
        fprintf(stderr, "       'noguess'/'choose' also work as the only optional argument\n");
        return 1;
    }
    /* "noguess"/"choose" as the first optional argument select a protocol
     * without requiring a user-directory placeholder. */
    const char *user_dir =
        (argc > 3 && strcmp(argv[3], "noguess") != 0 && strcmp(argv[3], "choose") != 0)
            ? argv[3] : "";
    bool no_guess = false;
    bool do_choose = false;
    for (int a = 3; a < argc; ++a) {
        if (strcmp(argv[a], "noguess") == 0)
            no_guess = true;
        else if (strcmp(argv[a], "choose") == 0)
            do_choose = true;
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
    zhuyin_context_t *ctx = s.init(argv[2], user_dir);
    if (!ctx) {
        fprintf(stderr, "zhuyin_init failed\n");
        dlclose(handle);
        return 1;
    }
    bool opts_ok = s.set_options(ctx, ZHUYIN_FLAGS);
    printf("set_options(0x%x): %s\n", ZHUYIN_FLAGS, opts_ok ? "true" : "false");
    if (!opts_ok) {
        fprintf(stderr, "set_options(0x%x) rejected\n", ZHUYIN_FLAGS);
        s.fini(ctx);
        dlclose(handle);
        return 1;
    }
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

    /* Protocol flags were parsed up top ("noguess", "choose").  A
     * "noguess" run reproduces the register's original before(3)
     * measurement shape — parse, then the lookup battery, with NO
     * zhuyin_guess_sentence in between (the pin's m_nbest_results stays
     * empty, so nothing is prepended). The default protocol guesses a
     * sentence first, which is what a real consumer does and what the
     * standing differential pins. Both protocols are part of the record:
     * docs/findings/upstream-divergences.md publishes them side by side
     * for the before(3) boundary. A "choose" run replaces the standard
     * battery with the before-cursor choose battery over CHOOSE_CORPUS
     * (always guess-first — that is the consumer protocol the residual
     * was registered under). */

    if (do_choose) {
        for (size_t i = 0; i < sizeof(CHOOSE_CORPUS) / sizeof(CHOOSE_CORPUS[0]); i++)
            choose_battery(&s, inst, CHOOSE_CORPUS[i]);
    } else
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

        if (no_guess) {
            printf("guess_sentence: (skipped — noguess protocol)\n");
        } else {
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
        }

        /* The five lookup surfaces, each rebuilding the list and dumping it
         * in full: after(0), after(consumed), before(0), before(3) (a key
         * boundary mid-composition; skipped when consumed < 3 so the
         * oracle's out-of-range matrix read is never poked), and
         * before(consumed). A guess rebuilds the instance's list wholesale
         * on both sides (upstream too), so the shared up-front
         * guess_sentence keeps the sequence symmetric. */
        const struct lookup_spec {
            bool before;
            size_t offset;
            const char *label;
        } lookups[] = {
            {false, 0, "after(0)"},
            {false, consumed, "after(consumed)"},
            {true, 0, "before(0)"},
            {true, 3, "before(3)"},
            {true, consumed, "before(consumed)"},
        };
        for (size_t l = 0; l < sizeof(lookups) / sizeof(lookups[0]); l++) {
            if (lookups[l].offset > consumed)
                continue;
            bool ok = lookups[l].before
                          ? s.guess_before(inst, lookups[l].offset)
                          : s.guess_after(inst, lookups[l].offset);
            printf("%s: %s\n", lookups[l].label, ok ? "true" : "false");
            dump_candidates(&s, inst);
        }

        s.reset(inst);
        printf("reset_parsed_len: %zu\n", s.parsed_len(inst));
        printf("\n");
    }

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    return 0;
}
