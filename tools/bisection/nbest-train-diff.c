/*
 * nbest-train-diff.c — W14 user-aware n-best differential driver.
 *
 * Drives an identical scripted C-ABI train-then-guess sequence into a
 * pinyin shared object (libpinyin_capi.so or the pinned libpinyin.so) and
 * prints the n-best sentence surface at each stage, so the two libraries'
 * logs can be diffed. Two seams share this driver:
 *
 *   selection record (sentence-surface.md §8) — training (你 → 浩) rounds
 *   walks the NBEST-wins dedup: after one seed the pair enters the tails
 *   and only its ROW candidate is choosable, so the export lines prove the
 *   chosen row's own token path landed (你浩 doubling, never a 你好 pile).
 *
 *   user-merged step costs (§9, pending) — the probe blocks below show
 *   whether trained user grams move the C-ABI n-best rows like the
 *   oracle's; compare the full logs once that seam lands.
 *
 * Training shapes:
 *
 *   baseline    — probe "nihao" on an empty user store (rows must agree
 *                 before anything is trained).
 *   user-only   — train (你 → 浩) N rounds (NBESTTRAINDIFF_USER_ROUNDS,
 *                 default 1): the system bigram row for 你 carries no 浩
 *                 successor, so a blended step for the pair exists only
 *                 through the user gram.
 *   augmented   — train (你 → 好) N rounds (NBESTTRAINDIFF_ROUNDS,
 *                 default 3) on top: 你's system row carries 好, so
 *                 training shifts the blended value.
 *   export      — the user-store triple sets, proving both engines landed
 *                 the same trained state.
 *
 * Each probe prints pinyin_get_sentence rows 0..2 plus the candidate head
 * (type, n-best index, text) after guess_sentence → guess_candidates.
 *
 * Requires a real-unigram system dir (interpolation2.text) on the capi
 * side; the fallback mini fixtures answer no n-best cost data.
 *
 * Usage:
 *   NBESTTRAINDIFF_USER_ROUNDS=<n> NBESTTRAINDIFF_ROUNDS=<n> \
 *     ./nbest-train-diff <path-to-so> <systemdir>
 */

#define _POSIX_C_SOURCE 200809L
#include <dirent.h>
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
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
typedef uint8_t guint8;

#define DEFAULT_SORT ((guint)0x1e) /* SORT_BY_PHRASE_LENGTH|PINYIN|FREQUENCY */

/* ── Function pointer types ───────────────────────────────────────────── */

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_instance)(pinyin_instance_t *);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_sentence)(pinyin_instance_t *);
typedef bool (*fn_guess)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_getn)(pinyin_instance_t *, guint *);
typedef bool (*fn_getc)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_gettype)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_getstr)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef bool (*fn_getnbest)(pinyin_instance_t *, lookup_candidate_t *, guint8 *);
typedef bool (*fn_get_sentence)(pinyin_instance_t *, guint8, gchar **);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_train)(pinyin_instance_t *, uint8_t);
typedef bool (*fn_reset)(pinyin_instance_t *);
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
    fn_fini fini;
    fn_alloc alloc;
    fn_free_instance free_instance;
    fn_parse parse;
    fn_sentence sentence;
    fn_guess guess;
    fn_getn getn;
    fn_getc getc;
    fn_gettype gettype;
    fn_getstr getstr;
    fn_getnbest getnbest;
    fn_get_sentence get_sentence;
    fn_choose choose;
    fn_train train;
    fn_reset reset;
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

static void resolve_all(void *handle, struct syms *s) {
    s->init = (fn_init)load("pinyin_init", handle);
    s->fini = (fn_fini)load("pinyin_fini", handle);
    s->alloc = (fn_alloc)load("pinyin_alloc_instance", handle);
    s->free_instance = (fn_free_instance)load("pinyin_free_instance", handle);
    s->parse = (fn_parse)load("pinyin_parse_more_full_pinyins", handle);
    s->sentence = (fn_sentence)load("pinyin_guess_sentence", handle);
    s->guess = (fn_guess)load("pinyin_guess_candidates", handle);
    s->getn = (fn_getn)load("pinyin_get_n_candidate", handle);
    s->getc = (fn_getc)load("pinyin_get_candidate", handle);
    s->gettype = (fn_gettype)load("pinyin_get_candidate_type", handle);
    s->getstr = (fn_getstr)load("pinyin_get_candidate_string", handle);
    s->getnbest = (fn_getnbest)load("pinyin_get_candidate_nbest_index", handle);
    s->get_sentence = (fn_get_sentence)load("pinyin_get_sentence", handle);
    s->choose = (fn_choose)load("pinyin_choose_candidate", handle);
    s->train = (fn_train)load("pinyin_train", handle);
    s->reset = (fn_reset)load("pinyin_reset", handle);
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

/* ── Helpers ──────────────────────────────────────────────────────────── */

/* Removes `dir` and everything under it, so no exit path can leak the
 * mkdtemp userdir even when the engine already wrote user data into it. */
static void rm_rf(const char *dir) {
    DIR *d = opendir(dir);
    if (d) {
        struct dirent *entry;
        while ((entry = readdir(d)) != NULL) {
            if (strcmp(entry->d_name, ".") == 0 ||
                strcmp(entry->d_name, "..") == 0)
                continue;
            char path[4096];
            if (snprintf(path, sizeof(path), "%s/%s", dir, entry->d_name) >=
                (int)sizeof(path))
                continue;
            unlink(path); /* the store writes regular files only */
        }
        closedir(d);
    }
    rmdir(dir);
}

/* Parses `input` and requires it to consume exactly `expect` bytes — a
 * partial parse would silently shrink every later surface. */
static int parse_expect(const struct syms *s, pinyin_instance_t *inst,
                        const char *input, size_t expect) {
    size_t parsed = s->parse(inst, input);
    if (parsed != expect) {
        fprintf(stderr, "parse(%s) consumed %zu bytes, expected %zu\n",
                input, parsed, expect);
        return 1;
    }
    return 0;
}

static lookup_candidate_t *find_by_text(const struct syms *s,
                                        pinyin_instance_t *inst,
                                        const char *want) {
    guint count = 0;
    if (!s->getn(inst, &count)) {
        fprintf(stderr, "find_by_text: pinyin_get_n_candidate failed\n");
        return NULL;
    }
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

static const char *type_name(int type) {
    switch (type) {
    case 1: return "NBEST";
    case 2: return "NORMAL";
    case 3: return "ZOMBIE";
    case 4: return "PRED_BIGRAM";
    case 5: return "PRED_PREFIX";
    case 6: return "ADDON";
    case 7: return "LONGER";
    case 8: return "PRED_PUNCT";
    default: return "?";
    }
}

/* guess_sentence → print sentence rows 0..2 and the candidate head.
 * Returns 1 on any C-ABI failure so the caller can bail through the
 * shared cleanup; every accessor is checked before the instance state
 * is reused. */
static int print_surface(const struct syms *s, pinyin_instance_t *inst,
                         const char *label) {
    bool guessed = s->sentence(inst);
    printf("probe:%s guess=%d\n", label, (int)guessed);
    if (!guessed) {
        fprintf(stderr, "probe %s: guess_sentence failed\n", label);
        return 1;
    }
    for (guint8 i = 0; i < 3; i++) {
        gchar *text = NULL;
        bool got = s->get_sentence(inst, i, &text);
        if (got && !text) {
            fprintf(stderr, "probe %s: get_sentence(%u) true with no text\n",
                    label, i);
            return 1;
        }
        /* `false` is the expected empty row (no n-best result at that
         * index), not a failure — printed as "-". */
        printf("probe:%s sentence[%u]=%s\n", label, i, got ? text : "-");
        /* Caller-owned per the C ABI contract; g_free(NULL) is a no-op. */
        g_free_fn(text);
    }
    if (!s->guess(inst, 0, DEFAULT_SORT)) {
        fprintf(stderr, "probe %s: guess_candidates failed\n", label);
        return 1;
    }
    guint n = 0;
    if (!s->getn(inst, &n)) {
        fprintf(stderr, "probe %s: get_n_candidate failed\n", label);
        return 1;
    }
    printf("probe:%s n=%u\n", label, n);
    guint limit = n < 8 ? n : 8;
    for (guint i = 0; i < limit; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->getc(inst, i, &cand) || !cand) {
            fprintf(stderr, "probe %s: get_candidate(%u) failed\n", label, i);
            return 1;
        }
        int type = 0;
        if (!s->gettype(inst, cand, &type)) {
            fprintf(stderr, "probe %s: get_candidate_type(%u) failed\n", label, i);
            return 1;
        }
        const gchar *text = NULL;
        if (!s->getstr(inst, cand, &text)) {
            fprintf(stderr, "probe %s: get_candidate_string(%u) failed\n", label, i);
            return 1;
        }
        if (type == 1) { /* NBEST: the oracle asserts nbest_index is only
                          asked of NBEST rows */
            guint8 idx = 255;
            if (!s->getnbest(inst, cand, &idx)) {
                fprintf(stderr, "probe %s: get_candidate_nbest_index(%u) failed\n",
                        label, i);
                return 1;
            }
            printf("probe:%s cand[%u]=%s/%u/%s\n", label, i, type_name(type),
                   (unsigned)idx, text ? text : "(null)");
        } else {
            printf("probe:%s cand[%u]=%s/-/%s\n", label, i, type_name(type),
                   text ? text : "(null)");
        }
    }
    if (!s->reset(inst)) {
        fprintf(stderr, "probe %s: reset failed\n", label);
        return 1;
    }
    return 0;
}

/* One scripted training round on (first → second) via the real
 * self-learning path: parse, sentence, guess, choose first, re-decode,
 * guess, choose second, re-decode, train. Returns 1 on any failure; the
 * caller routes it through the shared cleanup. */
static int train_pair(const struct syms *s, pinyin_instance_t *inst, int round,
                      const char *first, const char *second) {
    if (parse_expect(s, inst, "nihao", 5))
        return 1;
    if (!s->sentence(inst) || !s->guess(inst, 0, DEFAULT_SORT)) {
        fprintf(stderr, "round %d: pre-choose decode failed\n", round);
        return 1;
    }

    lookup_candidate_t *ni = find_by_text(s, inst, first);
    if (!ni) {
        fprintf(stderr, "round %d: candidate %s not offered\n", round, first);
        return 1;
    }
    int offset = s->choose(inst, 0, ni);
    if (offset < 0) {
        fprintf(stderr, "round %d: choose %s failed\n", round, first);
        return 1;
    }
    if (!s->sentence(inst) || !s->guess(inst, (size_t)offset, DEFAULT_SORT)) {
        fprintf(stderr, "round %d: mid-choose decode failed\n", round);
        return 1;
    }

    lookup_candidate_t *hao = find_by_text(s, inst, second);
    if (!hao) {
        fprintf(stderr, "round %d: candidate %s not offered\n", round, second);
        return 1;
    }
    if (s->choose(inst, (size_t)offset, hao) < 0) {
        fprintf(stderr, "round %d: choose %s failed\n", round, second);
        return 1;
    }
    /* Kept for upstream sequence fidelity, unchecked: with the input fully
     * consumed the capi's guess_sentence answers false (Session's
     * empty-remaining contract) while the oracle answers true — a known
     * edge divergence, and the return value is not part of the compared
     * surface (train walks the recorded selection, not this decode). */
    s->sentence(inst);

    if (!s->train(inst, 0)) {
        fprintf(stderr, "round %d: pinyin_train failed\n", round);
        return 1;
    }
    if (!s->reset(inst)) {
        fprintf(stderr, "round %d: reset failed\n", round);
        return 1;
    }
    return 0;
}

/* ── Main ─────────────────────────────────────────────────────────────── */

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "Usage: %s <path-to-so> <systemdir>\n", argv[0]);
        return 1;
    }

    int rounds = 3;
    if (getenv("NBESTTRAINDIFF_ROUNDS"))
        rounds = atoi(getenv("NBESTTRAINDIFF_ROUNDS"));
    if (rounds < 1 || rounds > 8) {
        fprintf(stderr, "NBESTTRAINDIFF_ROUNDS must be in 1..8\n");
        return 1;
    }
    /* The user-only pair trains once by default. After one seed the pair
     * enters the n-best tails and its own NORMAL candidate is deduped away
     * (NBEST wins), so every later round chooses the pair's ROW — the exact
     * selection-record path fixed in sentence-surface.md §8. Raise the
     * count to stress the row choose under reselection doubling. */
    int user_rounds = 1;
    if (getenv("NBESTTRAINDIFF_USER_ROUNDS"))
        user_rounds = atoi(getenv("NBESTTRAINDIFF_USER_ROUNDS"));
    if (user_rounds < 1 || user_rounds > 8) {
        fprintf(stderr, "NBESTTRAINDIFF_USER_ROUNDS must be in 1..8\n");
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

    char userdir[] = "/tmp/nbesttraindiff-user-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }

    pinyin_context_t *ctx = s.init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed (real-unigram systemdir required)\n");
        goto fail;
    }
    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        goto fail_ctx;
    }

    /* Phase A — baseline: empty user store. */
    if (parse_expect(&s, inst, "nihao", 5) ||
        print_surface(&s, inst, "baseline"))
        goto fail_inst;

    /* Phase B — user-only pair: 你 → 浩 (no system successor). */
    for (int round = 1; round <= user_rounds; round++) {
        if (train_pair(&s, inst, round, "你", "浩"))
            goto fail_inst;
    }
    if (parse_expect(&s, inst, "nihao", 5) ||
        print_surface(&s, inst, "user-only"))
        goto fail_inst;

    /* Phase C — augmented pair: 你 → 好 (system successor, count 30). */
    for (int round = 1; round <= rounds; round++) {
        if (train_pair(&s, inst, round, "你", "好"))
            goto fail_inst;
    }
    if (parse_expect(&s, inst, "nihao", 5) ||
        print_surface(&s, inst, "augmented"))
        goto fail_inst;

    /* Phase D — the landed user state, one export per process. */
    {
        export_iterator_t *iter = s.begin_phrases(ctx, 7 /* USER_DICTIONARY */);
        if (iter) {
            while (s.has_next(iter)) {
                gchar *phrase = NULL;
                gchar *pinyin = NULL;
                gint count = -1;
                if (!s.get_next(iter, &phrase, &pinyin, &count))
                    break;
                printf("phrase: %s|%s|%d\n", phrase ? phrase : "(null)",
                       pinyin ? pinyin : "(null)", (int)count);
                g_free_fn(phrase);
                g_free_fn(pinyin);
            }
            s.end_phrases(iter);
        }
        bigram_export_iterator_t *biter = s.begin_bigram(ctx);
        if (biter) {
            while (s.bigram_has_next(biter)) {
                gchar *phrase = NULL;
                gchar *pinyin = NULL;
                gint count = -1;
                /* Upstream's bigram get_next fills the out-params and
                 * returns whether MORE rows follow, not whether the call
                 * succeeded (user-store.md §9, mirrored from train-diff):
                 * print the filled row either way, and stop the loop when
                 * it reports no more. */
                bool more = s.bigram_get_next(biter, &phrase, &pinyin, &count);
                printf("bigram: %s|%s|%d\n", phrase ? phrase : "(null)",
                       pinyin ? pinyin : "(null)", (int)count);
                g_free_fn(phrase);
                g_free_fn(pinyin);
                if (!more)
                    break;
            }
            s.end_bigram(biter);
        }
    }

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    rm_rf(userdir);
    return 0;

    /* Shared cleanup: no exit path leaks the mkdtemp userdir. */
fail_inst:
    s.free_instance(inst);
fail_ctx:
    s.fini(ctx);
    dlclose(handle);
fail:
    rm_rf(userdir);
    return 1;
}
