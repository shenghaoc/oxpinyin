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
 *   user-merged step costs (§9) — the probe blocks below show that
 *   trained user grams move the C-ABI n-best rows like the oracle's;
 *   the runner compares the full logs, probes included.
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
 * Each probe prints up to three surviving NBEST sentence rows, in stored
 * index order, plus the candidate head (type, n-best index, text).
 *
 * Requires a real-unigram system dir (interpolation2.text) on the capi
 * side; the fallback mini fixtures answer no n-best cost data.
 *
 * Usage:
 *   NBESTTRAINDIFF_USER_ROUNDS=<n> NBESTTRAINDIFF_ROUNDS=<n> \
 *     ./nbest-train-diff <path-to-so> <systemdir>
 *   Diagnostic only (no training, candidates or sentence readback):
 *     ./nbest-train-diff <path-to-so> <systemdir> --cases <case-file.tsv>
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "driver-common.h"

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
typedef bool (*fn_save)(pinyin_context_t *);

struct syms {
    fn_init init;
    fn_fini fini;
    fn_save save;
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

static void resolve_all(void *handle, struct syms *s) {
    s->init = (fn_init)dc_require(handle, "pinyin_init");
    s->fini = (fn_fini)dc_require(handle, "pinyin_fini");
    s->save = (fn_save)dc_require(handle, "pinyin_save");
    s->alloc = (fn_alloc)dc_require(handle, "pinyin_alloc_instance");
    s->free_instance = (fn_free_instance)dc_require(handle, "pinyin_free_instance");
    s->parse = (fn_parse)dc_require(handle, "pinyin_parse_more_full_pinyins");
    s->sentence = (fn_sentence)dc_require(handle, "pinyin_guess_sentence");
    s->guess = (fn_guess)dc_require(handle, "pinyin_guess_candidates");
    s->getn = (fn_getn)dc_require(handle, "pinyin_get_n_candidate");
    s->getc = (fn_getc)dc_require(handle, "pinyin_get_candidate");
    s->gettype = (fn_gettype)dc_require(handle, "pinyin_get_candidate_type");
    s->getstr = (fn_getstr)dc_require(handle, "pinyin_get_candidate_string");
    s->getnbest = (fn_getnbest)dc_require(handle, "pinyin_get_candidate_nbest_index");
    s->get_sentence = (fn_get_sentence)dc_require(handle, "pinyin_get_sentence");
    s->choose = (fn_choose)dc_require(handle, "pinyin_choose_candidate");
    s->train = (fn_train)dc_require(handle, "pinyin_train");
    s->reset = (fn_reset)dc_require(handle, "pinyin_reset");
    s->begin_phrases = (fn_begin_phrases)dc_require(handle, "pinyin_begin_get_phrases");
    s->has_next = (fn_has_next)dc_require(handle, "pinyin_iterator_has_next_phrase");
    s->get_next = (fn_get_next)dc_require(handle, "pinyin_iterator_get_next_phrase");
    s->end_phrases = (fn_end_phrases)dc_require(handle, "pinyin_end_get_phrases");
    s->begin_bigram = (fn_begin_bigram)dc_require(handle, "pinyin_begin_get_bigram_phrases");
    s->bigram_has_next =
        (fn_bigram_has_next)dc_require(handle, "pinyin_bigram_iterator_has_next_phrase");
    s->bigram_get_next =
        (fn_bigram_get_next)dc_require(handle, "pinyin_bigram_iterator_get_next_phrase");
    s->end_bigram = (fn_end_bigram)dc_require(handle, "pinyin_end_get_bigram_phrases");
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

/* guess_sentence → guess_candidates → print surviving sentence rows and
 * the candidate head. Duplicate-text stored rows are omitted by the pin's
 * candidate deduplication (src/pinyin.cpp:2103-2112 at 074a2219).
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
    if (!s->guess(inst, 0, DEFAULT_SORT)) {
        fprintf(stderr, "probe %s: guess_candidates failed\n", label);
        return 1;
    }
    guint n = 0;
    if (!s->getn(inst, &n)) {
        fprintf(stderr, "probe %s: get_n_candidate failed\n", label);
        return 1;
    }
    /* DEFAULT_SORT leaves SORT_WITHOUT_SENTENCE_CANDIDATE (0x1) clear.
     * Each NBEST candidate carries a stored result index, even after
     * deduplication; never infer the result count from the candidate count
     * (src/pinyin.cpp:1934-1950,2295-2300,2880-2885 at 074a2219). */
    bool indices[UINT8_MAX + 1] = {false};
    for (guint i = 0; i < n; i++) {
        lookup_candidate_t *cand = NULL;
        int type = 0;
        if (!s->getc(inst, i, &cand) || !cand ||
            !s->gettype(inst, cand, &type)) {
            fprintf(stderr, "probe %s: candidate %u lookup failed\n", label, i);
            return 1;
        }
        if (type == 1) { /* NBEST_MATCH_CANDIDATE */
            guint8 index = 0;
            if (!s->getnbest(inst, cand, &index)) {
                fprintf(stderr, "probe %s: candidate %u nbest index failed\n", label, i);
                return 1;
            }
            indices[index] = true;
        }
    }
    for (guint i = 0, printed = 0; i <= UINT8_MAX && printed < 3; i++) {
        if (!indices[i])
            continue;
        gchar *text = NULL;
        if (!s->get_sentence(inst, (guint8)i, &text) || !text) {
            fprintf(stderr, "probe %s: get_sentence(%u) failed\n", label, i);
            g_free_fn(text);
            return 1;
        }
        printf("probe:%s sentence[%u]=%s\n", label, i, text);
        g_free_fn(text);
        printed++;
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
            printf("probe:%s cand[%u]=%s/%u/%s\n", label, i, dc_type_short_name(type),
                   (unsigned)idx, text ? text : "(null)");
        } else {
            printf("probe:%s cand[%u]=%s/-/%s\n", label, i, dc_type_short_name(type),
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

/* The diagnostic pin captures the ranges inside guess_sentence. This path
 * intentionally needs no candidate enumeration or sentence accessor. */
static int witness_cases(const struct syms *s, void *handle,
                         const char *systemdir, const char *casefile) {
    /* Suppress the older, exhaustive residue dump even if inherited. */
    unsetenv("OXPINYIN_DUMP_TAILS");
    typedef bool (*fn_options)(pinyin_context_t *, guint);
    typedef bool (*fn_scheme)(pinyin_context_t *, int);
    fn_options options = (fn_options)dc_require(handle, "pinyin_set_options");
    fn_scheme full_scheme = (fn_scheme)dc_require(handle, "pinyin_set_full_pinyin_scheme");
    fn_scheme double_scheme = (fn_scheme)dc_require(handle, "pinyin_set_double_pinyin_scheme");
    fn_parse double_parse = (fn_parse)dc_require(handle, "pinyin_parse_more_double_pinyins");
    FILE *file = fopen(casefile, "r");
    if (!file) {
        perror("case file");
        return 1;
    }
    char *line = NULL;
    size_t capacity = 0, completed = 0;
    int result = 0;
    while (getline(&line, &capacity, file) >= 0) {
        line[strcspn(line, "\r\n")] = '\0';
        if (line[0] == '#' || line[0] == '\0')
            continue;
        char *fields[5], *save = NULL;
        char *field = strtok_r(line, "\t", &save);
        size_t count = 0;
        while (field && count < 5) {
            fields[count++] = field;
            field = strtok_r(NULL, "\t", &save);
        }
        if (count != 5 || field) {
            fprintf(stderr, "case file: expected id, parser, scheme, options, input\n");
            result = 1;
            break;
        }
        const bool full = strcmp(fields[1], "full") == 0;
        const bool double_pinyin = strcmp(fields[1], "double") == 0;
        if ((!full && !double_pinyin) ||
            strcmp(fields[2], full ? "1" : "2") != 0 ||
            (strcmp(fields[3], "0x1fe00198") != 0 &&
             strcmp(fields[3], "0x1fe00018") != 0)) {
            fprintf(stderr, "case %s: unsupported parser, scheme or option word\n", fields[0]);
            result = 1;
            break;
        }
        /* Relative to the runner's private scratch directory. */
        char userdir[] = "nbest-witness-user-XXXXXX";
        if (!mkdtemp(userdir)) {
            perror("mkdtemp");
            result = 1;
            break;
        }
        pinyin_context_t *ctx = s->init(systemdir, userdir);
        pinyin_instance_t *inst = NULL;
        size_t parsed = 0;
        bool guessed = false;
        if (ctx && options(ctx, (guint)strtoul(fields[3], NULL, 16)) &&
            (full ? full_scheme(ctx, 1) : double_scheme(ctx, 2))) {
            inst = s->alloc(ctx);
            if (inst && setenv("OXPINYIN_PRECONDITION_CASE", fields[0], 1) == 0) {
                parsed = (full ? s->parse : double_parse)(inst, fields[4]);
                if (parsed == strlen(fields[4]))
                    guessed = s->sentence(inst);
            }
        }
        printf("case\tid=%s\tparser=%s\tscheme=%s\toptions=%s\tinput=%s"
               "\tparsed=%zu\tguess=%d\n", fields[0], fields[1], fields[2],
               fields[3], fields[4], parsed, guessed);
        unsetenv("OXPINYIN_PRECONDITION_CASE");
        if (inst)
            s->free_instance(inst);
        if (ctx)
            s->fini(ctx);
        dc_remove_dir_shallow(userdir);
        if (!guessed) {
            fprintf(stderr, "case %s: setup, complete parse or guess_sentence failed\n", fields[0]);
            result = 1;
            break;
        }
        ++completed;
    }
    if (ferror(file)) {
        perror("read case file");
        result = 1;
    }
    free(line);
    fclose(file);
    printf("cases\tcompleted=%zu\n", completed);
    return completed ? result : 1;
}

/* Read back one selected row from the same TSV inputs as witness_cases.
 * The diagnostic --cases path above deliberately remains decode-only. */
static int surface_case(const struct syms *s, void *handle,
                        const char *systemdir, char **fields) {
    const bool full = strcmp(fields[1], "full") == 0;
    const bool double_pinyin = strcmp(fields[1], "double") == 0;
    if ((!full && !double_pinyin) ||
        strcmp(fields[2], full ? "1" : "2") != 0 ||
        (strcmp(fields[3], "0x1fe00198") != 0 &&
         strcmp(fields[3], "0x1fe00018") != 0)) {
        fprintf(stderr, "case %s: unsupported parser, scheme or option word\n", fields[0]);
        return 1;
    }
    typedef bool (*fn_options)(pinyin_context_t *, guint);
    typedef bool (*fn_scheme)(pinyin_context_t *, int);
    fn_options options = (fn_options)dc_require(handle, "pinyin_set_options");
    fn_scheme scheme = (fn_scheme)dc_require(handle, full ? "pinyin_set_full_pinyin_scheme" :
                                                   "pinyin_set_double_pinyin_scheme");
    fn_parse parse = full ? s->parse :
        (fn_parse)dc_require(handle, "pinyin_parse_more_double_pinyins");
    char userdir[] = "nbest-surface-user-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }
    int result = 1;
    pinyin_context_t *ctx = s->init(systemdir, userdir);
    pinyin_instance_t *inst = NULL;
    if (ctx && options(ctx, (guint)strtoul(fields[3], NULL, 16)) &&
        scheme(ctx, full ? 1 : 2)) {
        inst = s->alloc(ctx);
        if (inst && parse(inst, fields[4]) == strlen(fields[4]))
            result = print_surface(s, inst, fields[0]);
    }
    if (result)
        fprintf(stderr, "case %s: setup, complete parse or surface failed\n", fields[0]);
    if (inst)
        s->free_instance(inst);
    if (ctx)
        s->fini(ctx);
    dc_remove_dir_shallow(userdir);
    return result;
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "Usage: %s <path-to-so> <systemdir>\n", argv[0]);
        return 1;
    }
    if (argc != 3 && (argc != 5 || strcmp(argv[3], "--cases") != 0) &&
        (argc != 9 || strcmp(argv[3], "--surface-case") != 0)) {
        fprintf(stderr, "Usage: %s <path-to-so> <systemdir> [--cases <case-file.tsv> | "
                "--surface-case <id> <parser> <scheme> <options> <input>]\n", argv[0]);
        return 1;
    }

    if (argc != 3) {
        void *handle = dlopen(argv[1], RTLD_NOW);
        if (!handle) {
            fprintf(stderr, "dlopen: %s\n", dlerror());
            return 1;
        }
        struct syms s = {0};
        resolve_all(handle, &s);
        int result;
        if (argc == 5) {
            result = witness_cases(&s, handle, argv[2], argv[4]);
        } else {
            resolve_g_free();
            result = surface_case(&s, handle, argv[2], &argv[4]);
        }
        dlclose(handle);
        return result;
    }

    int rounds = 3;
    if (dc_parse_rounds_env("NBESTTRAINDIFF_ROUNDS", &rounds))
        return 1;
    /* The user-only pair trains once by default. After one seed the pair
     * enters the n-best tails and its own NORMAL candidate is deduped away
     * (NBEST wins), so every later round chooses the pair's ROW — the exact
     * selection-record path fixed in sentence-surface.md §8. Raise the
     * count to stress the row choose under reselection doubling. */
    int user_rounds = 1;
    if (dc_parse_rounds_env("NBESTTRAINDIFF_USER_ROUNDS", &user_rounds))
        return 1;

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

    /* Phase E — persistence: save, tear the context down completely, and
     * reopen the same user dir in a fresh context. The reopened surface
     * and export must equal the in-memory ones — the stored/in-memory
     * interaction the export-only phase cannot see. */
    if (!s.save(ctx)) {
        fprintf(stderr, "reopen: pinyin_save failed\n");
        goto fail_inst;
    }
    s.free_instance(inst);
    s.fini(ctx);
    inst = NULL;
    ctx = s.init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "reopen: pinyin_init failed\n");
        goto fail_no_ctx;
    }
    inst = s.alloc(ctx);
    if (!inst) {
        fprintf(stderr, "reopen: pinyin_alloc_instance failed\n");
        goto fail_ctx;
    }
    if (parse_expect(&s, inst, "nihao", 5) || print_surface(&s, inst, "reopened"))
        goto fail_inst;
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
    dc_remove_dir_shallow(userdir);
    return 0;

    /* Shared cleanup: no exit path leaks the mkdtemp userdir. The reopen
     * phase frees and NULLs the instance before re-init, so a failure
     * after that point must not double-free. */
fail_inst:
    if (inst)
        s.free_instance(inst);
fail_ctx:
    if (ctx)
        s.fini(ctx);
fail_no_ctx:
    dlclose(handle);
fail:
    dc_remove_dir_shallow(userdir);
    return 1;
}
