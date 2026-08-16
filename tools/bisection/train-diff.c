/*
 * train-diff.c — W6-T7 user-store differential driver.
 *
 * Drives an identical scripted C-ABI training sequence into a pinyin
 * shared object (libpinyin_capi.so or the pinned libpinyin.so), then
 * exports the user data through the §9 iterators and prints the
 * (phrase, pinyin, count) triples as sortable lines. Run against both
 * libraries and diff the logs; the comparison is exact-integer equality.
 *
 * Scripted sequence (user-store.md §2/§3/§9):
 *   rounds 1..TRAINDIFF_ROUNDS (default 8) — parse "nihao", choose 你
 *                 (offset 0), re-guess, choose 好, re-guess sentence,
 *                 train. Each round re-trains the same (你 → 好) pair:
 *                 seeds 69, 138, 414, 1242, 3726, 11178, 22080 (clamp),
 *                 22080. Running the driver once per round count lets the
 *                 runner observe the whole sequence from a single export
 *                 per context: the pinned oracle's own bigram export
 *                 segfaults (stale join buffer) when the export cycle is
 *                 repeated inside one context, so the runner keeps one
 *                 export per process.
 *   remember    — "你好" (count -1) after "nihao", then "世界" (-1) and
 *                 "世界" (7) after "shijie": the index-only path and the
 *                 pronunciation-merge path.
 *   export      — the full phrase and bigram triple sets, once.
 *
 * The predicted-candidate path is deliberately absent: the capi's
 * pinyin_guess_predicted_candidates_with_punctuations is a stub returning
 * false, so no predicted candidate can be driven on that side, and the
 * pinned library asserts on a non-predicted candidate — the sequence
 * cannot be made identical across both engines.
 *
 * Usage:
 *   TRAINDIFF_ROUNDS=<n> ./train-diff <path-to-so> <systemdir>
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
typedef void export_iterator_t;
typedef void bigram_export_iterator_t;

/* ── Scalar types (match pinyin.h / glib) ─────────────────────────────── */

typedef uint32_t guint;
typedef int32_t gint;
typedef char gchar;

#define DEFAULT_SORT ((guint)0x1e) /* SORT_BY_PHRASE_LENGTH|PINYIN|FREQUENCY */

/* ── Function pointer types ───────────────────────────────────────────── */

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_instance)(pinyin_instance_t *);
typedef bool (*fn_set_options)(pinyin_context_t *, uint32_t);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_sentence)(pinyin_instance_t *);
typedef bool (*fn_guess)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_getn)(pinyin_instance_t *, guint *);
typedef bool (*fn_getc)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_getstr)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_train)(pinyin_instance_t *, uint8_t);
typedef bool (*fn_reset)(pinyin_instance_t *);
typedef bool (*fn_remember)(pinyin_instance_t *, const char *, gint);
typedef bool (*fn_mask_out)(pinyin_context_t *, uint32_t, uint32_t);
typedef export_iterator_t *(*fn_begin_phrases)(pinyin_context_t *, guint);
typedef bool (*fn_has_next)(export_iterator_t *);
typedef bool (*fn_get_next)(export_iterator_t *, gchar **, gchar **, gint *);
typedef void (*fn_end_phrases)(export_iterator_t *);
typedef bigram_export_iterator_t *(*fn_begin_bigram)(pinyin_context_t *);
typedef bool (*fn_bigram_has_next)(bigram_export_iterator_t *);
typedef bool (*fn_bigram_get_next)(bigram_export_iterator_t *, gchar **, gchar **, gint *);
typedef void (*fn_end_bigram)(bigram_export_iterator_t *);

struct syms {
    fn_init init;
    fn_init fixture_init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_instance free_instance;
    fn_set_options set_options;
    fn_parse parse;
    fn_sentence sentence;
    fn_guess guess;
    fn_getn getn;
    fn_getc getc;
    fn_getstr getstr;
    fn_choose choose;
    fn_train train;
    fn_reset reset;
    fn_remember remember;
    fn_mask_out mask_out;
    fn_begin_phrases begin_phrases;
    fn_has_next has_next;
    fn_get_next get_next;
    fn_end_phrases end_phrases;
    fn_begin_bigram begin_bigram;
    fn_bigram_has_next bigram_has_next;
    fn_bigram_get_next bigram_get_next;
    fn_end_bigram end_bigram;
};

static void *load(const char *name, void *handle) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "  MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

static int file_exists(const char *dir, const char *name) {
    char path[4096];
    struct stat st;
    if (snprintf(path, sizeof(path), "%s/%s", dir, name) >= (int)sizeof(path))
        return 0;
    return stat(path, &st) == 0 && S_ISREG(st.st_mode);
}

static void resolve_all(void *handle, struct syms *s) {
    s->init = (fn_init)load("pinyin_init", handle);
    s->fixture_init = (fn_init)dlsym(handle, "oxpinyin_init_for_fixtures");
    s->fini = (fn_fini)load("pinyin_fini", handle);
    s->alloc = (fn_alloc)load("pinyin_alloc_instance", handle);
    s->free_instance = (fn_free_instance)load("pinyin_free_instance", handle);
    s->set_options = (fn_set_options)load("pinyin_set_options", handle);
    s->parse = (fn_parse)load("pinyin_parse_more_full_pinyins", handle);
    s->sentence = (fn_sentence)load("pinyin_guess_sentence", handle);
    s->guess = (fn_guess)load("pinyin_guess_candidates", handle);
    s->getn = (fn_getn)load("pinyin_get_n_candidate", handle);
    s->getc = (fn_getc)load("pinyin_get_candidate", handle);
    s->getstr = (fn_getstr)load("pinyin_get_candidate_string", handle);
    s->choose = (fn_choose)load("pinyin_choose_candidate", handle);
    s->train = (fn_train)load("pinyin_train", handle);
    s->reset = (fn_reset)load("pinyin_reset", handle);
    s->remember = (fn_remember)load("pinyin_remember_user_input", handle);
    s->mask_out = (fn_mask_out)load("pinyin_mask_out", handle);
    s->begin_phrases = (fn_begin_phrases)load("pinyin_begin_get_phrases", handle);
    s->has_next = (fn_has_next)load("pinyin_iterator_has_next_phrase", handle);
    s->get_next = (fn_get_next)load("pinyin_iterator_get_next_phrase", handle);
    s->end_phrases = (fn_end_phrases)load("pinyin_end_get_phrases", handle);
    s->begin_bigram = (fn_begin_bigram)load("pinyin_begin_get_bigram_phrases", handle);
    s->bigram_has_next =
        (fn_bigram_has_next)load("pinyin_bigram_iterator_has_next_phrase", handle);
    s->bigram_get_next =
        (fn_bigram_get_next)load("pinyin_bigram_iterator_get_next_phrase", handle);
    s->end_bigram = (fn_end_bigram)load("pinyin_end_get_bigram_phrases", handle);
}

/* ── Caller-owned string release ──────────────────────────────────────── */

typedef void (*fn_g_free)(void *);
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

/* ── Candidate search by text ─────────────────────────────────────────── */

#define CANDIDATE_DEPTH 10u

static void print_top10(const struct syms *s, pinyin_instance_t *inst,
                        const char *label) {
    guint n = 0;
    s->getn(inst, &n);
    printf("cand:%s n=%u\n", label, n);
    guint limit = n < CANDIDATE_DEPTH ? n : CANDIDATE_DEPTH;
    for (guint i = 0; i < limit; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->getc(inst, i, &cand) || !cand) {
            printf("cand:%s[%u]=FAILED\n", label, i);
            continue;
        }
        const gchar *text = NULL;
        s->getstr(inst, cand, &text);
        printf("cand:%s[%u]=%s\n", label, i, text ? text : "(null)");
    }
}

static lookup_candidate_t *find_by_text(const struct syms *s,
                                        pinyin_instance_t *inst,
                                        const char *want) {
    guint count = 0;
    s->getn(inst, &count);
    for (guint i = 0; i < count; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->getc(inst, i, &cand) || !cand)
            continue;
        const gchar *text = NULL;
        s->getstr(inst, cand, &text);
        if (text && strcmp(text, want) == 0)
            return cand;
    }
    return NULL;
}

/* ── Export dumping ───────────────────────────────────────────────────── */

static void print_phrase_rows(const struct syms *s, pinyin_context_t *ctx) {
    export_iterator_t *iter = s->begin_phrases(ctx, 7 /* USER_DICTIONARY */);
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
    s->end_phrases(iter);
}

static void print_bigram_rows(const struct syms *s, pinyin_context_t *ctx) {
    bigram_export_iterator_t *iter = s->begin_bigram(ctx);
    if (!iter) {
        printf("bigram: BEGIN-NULL\n");
        return;
    }
    /* Upstream's get_next fills the out-params and returns whether MORE rows
     * follow — not whether this call succeeded. Print every row the
     * has_next/get_next pair yields and stop when has_next goes false. */
    while (s->bigram_has_next(iter)) {
        gchar *phrase = NULL;
        gchar *pinyin = NULL;
        gint count = -1;
        s->bigram_get_next(iter, &phrase, &pinyin, &count);
        printf("bigram: %s|%s|%d\n", phrase ? phrase : "(null)",
               pinyin ? pinyin : "(null)", (int)count);
        g_free_fn(phrase);
        g_free_fn(pinyin);
    }
    s->end_bigram(iter);
}

/* ── Main ─────────────────────────────────────────────────────────────── */

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "Usage: %s <path-to-so> <systemdir>\n", argv[0]);
        return 1;
    }

    int rounds = 8;
    if (getenv("TRAINDIFF_ROUNDS"))
        rounds = atoi(getenv("TRAINDIFF_ROUNDS"));
    if (rounds < 1 || rounds > 64) {
        fprintf(stderr, "TRAINDIFF_ROUNDS must be in 1..64\n");
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

    char userdir[] = "/tmp/traindiff-user-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }

    fn_init init = (s.fixture_init && !file_exists(argv[2], "interpolation2.text"))
        ? s.fixture_init
        : s.init;
    pinyin_context_t *ctx = init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed\n");
        return 1;
    }
    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        return 1;
    }
    if (getenv("TRAINDIFF_OPTIONS")) {
        uint32_t options = (uint32_t)strtoul(getenv("TRAINDIFF_OPTIONS"), NULL, 16);
        if (!s.set_options(ctx, options)) {
            fprintf(stderr, "pinyin_set_options failed\n");
            return 1;
        }
    }

    /* Phase 1 — the doubling sequence on (你 → 好). Each round: parse,
     * guess sentence, choose 你 at offset 0, re-guess sentence (the
     * frontend's constraint-respecting re-decode), guess candidates at the
     * new offset, choose 好, re-guess sentence, train. */
    for (int round = 1; round <= rounds; round++) {
        s.parse(inst, "nihao");
        s.sentence(inst);
        s.guess(inst, 0, DEFAULT_SORT);

        lookup_candidate_t *ni = find_by_text(&s, inst, "你");
        if (!ni) {
            fprintf(stderr, "round %d: candidate 你 not offered\n", round);
            return 1;
        }
        int offset = s.choose(inst, 0, ni);
        if (offset < 0) {
            fprintf(stderr, "round %d: choose 你 failed\n", round);
            return 1;
        }
        s.sentence(inst);
        s.guess(inst, (size_t)offset, DEFAULT_SORT);

        lookup_candidate_t *hao = find_by_text(&s, inst, "好");
        if (!hao) {
            fprintf(stderr, "round %d: candidate 好 not offered\n", round);
            return 1;
        }
        if (s.choose(inst, (size_t)offset, hao) < 0) {
            fprintf(stderr, "round %d: choose 好 failed\n", round);
            return 1;
        }
        s.sentence(inst);

        if (!s.train(inst, 0)) {
            fprintf(stderr, "round %d: pinyin_train failed\n", round);
            return 1;
        }
        s.reset(inst);
    }

    /* Phase 2 — remember_user_input: index-only path and the pronunciation
     * merge (5 + 7 = 12 on 世界). */
    s.parse(inst, "nihao");
    if (!s.remember(inst, "你好", -1)) {
        fprintf(stderr, "remember 你好 failed\n");
        return 1;
    }
    s.parse(inst, "shijie");
    if (!s.remember(inst, "世界", -1)) {
        fprintf(stderr, "remember 世界 failed\n");
        return 1;
    }
    if (!s.remember(inst, "世界", 7)) {
        fprintf(stderr, "remember 世界 7 failed\n");
        return 1;
    }

    /* Phase 2.5 — masking (T6): TRAINDIFF_MASK selects the frontend's
     * "user" clear (library mask against USER_DICTIONARY) or its "all"
     * clear (0x0 against 0x0). One mask per process, before the single
     * export. */
    if (getenv("TRAINDIFF_MASK")) {
        uint32_t mask = 0;
        uint32_t value = 0;
        if (strcmp(getenv("TRAINDIFF_MASK"), "user") == 0) {
            mask = 0x0F000000u;
            value = 0x07000000u;
        } else if (strcmp(getenv("TRAINDIFF_MASK"), "all") == 0) {
            mask = 0x0u;
            value = 0x0u;
        } else {
            fprintf(stderr, "TRAINDIFF_MASK must be user or all\n");
            return 1;
        }
        if (!s.mask_out(ctx, mask, value)) {
            fprintf(stderr, "pinyin_mask_out failed\n");
            return 1;
        }
    }

    /* Phase 2.75 — populated-store candidate dump (W10 DYNAMIC_ADJUST
     * read-side). After the same training both engines just wrote, guess
     * at offset 0 and again after choosing 你. The cited sites
     * (pinyin.cpp:2201-2212, :1845-1851) add the bigram term of m_freq
     * only when DYNAMIC_ADJUST is set; bit-clear (this path) must not let
     * trained user bigrams change candidate frequency. */
    if (getenv("TRAINDIFF_DUMP_CANDIDATES")) {
        s.reset(inst);
        s.parse(inst, "nihao");
        s.guess(inst, 0, DEFAULT_SORT);
        print_top10(&s, inst, "nihao@0");

        lookup_candidate_t *ni = find_by_text(&s, inst, "你");
        if (!ni) {
            fprintf(stderr, "dump: candidate 你 not offered after training\n");
            return 1;
        }
        int offset = s.choose(inst, 0, ni);
        if (offset < 0) {
            fprintf(stderr, "dump: choose 你 failed\n");
            return 1;
        }
        s.guess(inst, (size_t)offset, DEFAULT_SORT);
        print_top10(&s, inst, "after-ni");
        s.reset(inst);
    }

    /* Phase 3 — the full triple sets, one export per context. */
    print_phrase_rows(&s, ctx);
    print_bigram_rows(&s, ctx);

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    rmdir(userdir); /* best-effort: non-empty on success */
    return 0;
}
