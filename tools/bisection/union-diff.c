/*
 * union-diff.c — W11 union differential driver.
 *
 * One scripted C-ABI sequence against libpinyin_capi.so or the pin-built
 * libpinyin.so. Printed lines are compared exactly. Four sections:
 *
 *   1. Import a unique user phrase via the add/iterator/end trio, re-parse,
 *      report the index of that phrase in the candidate list.
 *   2. Load addon library 4 (art); print ADDON_CANDIDATE texts for "erhuang".
 *   3. Choose the first NBEST_MATCH row for "cecenihao" and print the
 *      returned cursor: upstream answers matrix.size() - 1
 *      unconditionally (pinyin.cpp:2513-2519) — 9 here — whatever span
 *      the row's own path covered; the capi's single-phrase row ends at 4
 *      under the old row-own-end answer, which is the divergence this
 *      section pins shut. Then the corrected post-NBEST flow:
 *      pinyin_guess_sentence (the lookup at the tail slot starts no span),
 *      then pinyin_train. This section MUST run before 4: section 4's
 *      train seeds user unigram deltas that let the mini decode cover the
 *      whole input, the row stops being single-phrase, and the cursor
 *      agrees trivially on both engines.
 *   4. Train 测测 → 你好, then guess predicted candidates for "测测".
 *      Phrase-prediction only: PREDICTED_PUNCTUATION (8) is omitted.
 *      Mini vs full prefix tokens (测) would diverge on punctuation;
 *      that list is compared by punct-diff.c. The lookup between the two
 *      chooses anchors at the imported phrase's own extent ("cece", 4
 *      bytes) — NOT at choose's returned cursor: on the capi the 测测
 *      candidate is an NBEST row (the typing gap below), so the cursor is
 *      the whole parse end — the reserved tail slot, where no word
 *      candidates start by design and the frontend re-runs
 *      pinyin_guess_sentence instead. An earlier draft guessed at the
 *      cursor and only worked because the old row-own-end return happened
 *      to equal the phrase extent there.
 *
 * Only data-insensitive shapes are printed: indices, the cursor value,
 * and call bools. Row texts differ between the mini and full model data
 * by construction (the same 测测 text is an NBEST row on the mini fixture
 * and a phrase candidate on the full model — the typing gap below).
 *
 * Sort profile (not the empty-store parity profile, which never trains):
 *   guess:  SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY
 *           (0x1e), the same mask as tools/bisection/import-diff.c uses
 *           for the live list.
 *   predict: SORT_BY_PHRASE_LENGTH | SORT_BY_FREQUENCY (upstream
 *            pinyin.cpp:2439), no pinyin-span key.
 *
 * NBEST_MATCH (1) vs NORMAL (2) on the same text is a pre-existing
 * full-pinyin typing gap, not W11's. This driver never prints candidate
 * type for the user-position line, so that gap cannot fail the union
 * assertions — same exclusion shape as tie-order.
 *
 * Usage: ./union-diff <path-to-so> <systemdir>
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef void pinyin_context_t;
typedef void pinyin_instance_t;
typedef void lookup_candidate_t;
typedef void import_iterator_t;
typedef uint32_t guint;

enum {
    NBEST_MATCH_CANDIDATE = 1,
    NORMAL_CANDIDATE = 2,
    PREDICTED_BIGRAM_CANDIDATE = 4,
    PREDICTED_PREFIX_CANDIDATE = 5,
    ADDON_CANDIDATE = 6,
    PREDICTED_PUNCTUATION_CANDIDATE = 8,
    GUESS_SORT = 0x1e
};

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_inst)(pinyin_instance_t *);
typedef bool (*fn_reset)(pinyin_instance_t *);
typedef bool (*fn_load_addon)(pinyin_context_t *, uint8_t);
typedef import_iterator_t *(*fn_begin_add)(pinyin_context_t *, uint8_t);
typedef bool (*fn_add)(import_iterator_t *, const char *, const char *, int);
typedef void (*fn_end_add)(import_iterator_t *);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_guess)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_n_cand)(pinyin_instance_t *, guint *);
typedef bool (*fn_get_cand)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_get_type)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_get_str)(pinyin_instance_t *, lookup_candidate_t *, const char **);
typedef bool (*fn_is_user)(pinyin_instance_t *, lookup_candidate_t *);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_train)(pinyin_instance_t *, uint8_t);
typedef bool (*fn_sentence)(pinyin_instance_t *);
typedef bool (*fn_predict)(pinyin_instance_t *, const char *);

struct syms {
    fn_init init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_inst free_inst;
    fn_reset reset;
    fn_load_addon load_addon;
    fn_begin_add begin_add;
    fn_add add;
    fn_end_add end_add;
    fn_parse parse;
    fn_guess guess;
    fn_n_cand n_cand;
    fn_get_cand get_cand;
    fn_get_type get_type;
    fn_get_str get_str;
    fn_is_user is_user;
    fn_choose choose;
    fn_train train;
    fn_sentence sentence;
    fn_predict predict;
};

static void *must(void *handle, const char *name) {
    void *s = dlsym(handle, name);
    if (!s) {
        fprintf(stderr, "MISSING %s\n", name);
        exit(1);
    }
    return s;
}

static lookup_candidate_t *find_by_text(const struct syms *s, pinyin_instance_t *inst,
                                        const char *want) {
    guint n = 0;
    guint i;
    s->n_cand(inst, &n);
    for (i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        const char *text = NULL;
        s->get_cand(inst, i, &c);
        s->get_str(inst, c, &text);
        if (text && strcmp(text, want) == 0)
            return c;
    }
    return NULL;
}

static int index_of_text(const struct syms *s, pinyin_instance_t *inst, const char *want) {
    guint n = 0;
    guint i;
    s->n_cand(inst, &n);
    for (i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        const char *text = NULL;
        s->get_cand(inst, i, &c);
        s->get_str(inst, c, &text);
        if (text && strcmp(text, want) == 0)
            return (int)i;
    }
    return -1;
}

static lookup_candidate_t *find_first_by_type(const struct syms *s,
                                              pinyin_instance_t *inst,
                                              int type) {
    guint n = 0;
    guint i;
    s->n_cand(inst, &n);
    for (i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        int t = 0;
        s->get_cand(inst, i, &c);
        s->get_type(inst, c, &t);
        if (t == type)
            return c;
    }
    return NULL;
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "Usage: %s <so> <systemdir>\n", argv[0]);
        return 1;
    }
    void *h = dlopen(argv[1], RTLD_NOW);
    if (!h) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }

    struct syms s;
    memset(&s, 0, sizeof(s));
    s.init = (fn_init)dlsym(h, "oxpinyin_init_for_fixtures");
    if (!s.init)
        s.init = (fn_init)must(h, "pinyin_init");
    s.fini = (fn_fini)must(h, "pinyin_fini");
    s.alloc = (fn_alloc)must(h, "pinyin_alloc_instance");
    s.free_inst = (fn_free_inst)must(h, "pinyin_free_instance");
    s.reset = (fn_reset)must(h, "pinyin_reset");
    s.load_addon = (fn_load_addon)must(h, "pinyin_load_addon_phrase_library");
    s.begin_add = (fn_begin_add)must(h, "pinyin_begin_add_phrases");
    s.add = (fn_add)must(h, "pinyin_iterator_add_phrase");
    s.end_add = (fn_end_add)must(h, "pinyin_end_add_phrases");
    s.parse = (fn_parse)must(h, "pinyin_parse_more_full_pinyins");
    s.guess = (fn_guess)must(h, "pinyin_guess_candidates");
    s.n_cand = (fn_n_cand)must(h, "pinyin_get_n_candidate");
    s.get_cand = (fn_get_cand)must(h, "pinyin_get_candidate");
    s.get_type = (fn_get_type)must(h, "pinyin_get_candidate_type");
    s.get_str = (fn_get_str)must(h, "pinyin_get_candidate_string");
    s.is_user = (fn_is_user)must(h, "pinyin_is_user_candidate");
    s.choose = (fn_choose)must(h, "pinyin_choose_candidate");
    s.train = (fn_train)must(h, "pinyin_train");
    s.sentence = (fn_sentence)must(h, "pinyin_guess_sentence");
    s.predict = (fn_predict)must(h, "pinyin_guess_predicted_candidates_with_punctuations");

    char userdir[] = "/tmp/uniondiff-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }
    pinyin_context_t *ctx = s.init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "init failed\n");
        return 1;
    }
    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        fprintf(stderr, "alloc failed\n");
        return 1;
    }

    printf("profile: guess=0x1e length|pinyin|freq; predict=length|freq; "
           "nbest-vs-normal excluded\n");

    /* ── 1. user phrase surface ─────────────────────────────────────── */
    {
        import_iterator_t *it = s.begin_add(ctx, 7);
        s.add(it, "测测", "cece", 5);
        s.end_add(it);
    }
    s.parse(inst, "cece");
    s.guess(inst, 0, GUESS_SORT);
    {
        int idx = index_of_text(&s, inst, "测测");
        lookup_candidate_t *c = find_by_text(&s, inst, "测测");
        printf("user-pos: %d 测测 user=%s\n", idx,
               (c && s.is_user(inst, c)) ? "yes" : "no");
    }

    /* ── 2. addon library ───────────────────────────────────────────── */
    s.reset(inst);
    printf("load_addon(4): %s\n", s.load_addon(ctx, 4) ? "true" : "false");
    printf("load_addon(4) again: %s\n", s.load_addon(ctx, 4) ? "true" : "false");
    s.parse(inst, "erhuang");
    s.guess(inst, 0, GUESS_SORT);
    {
        guint n = 0;
        guint i;
        s.n_cand(inst, &n);
        for (i = 0; i < n; i++) {
            lookup_candidate_t *c = NULL;
            int type = 0;
            const char *text = NULL;
            s.get_cand(inst, i, &c);
            s.get_type(inst, c, &type);
            s.get_str(inst, c, &text);
            if (type == ADDON_CANDIDATE)
                printf("addon: %s\n", text ? text : "(null)");
        }
    }

    /* ── 3. NBEST row choose: the cursor is the whole parse end ─────── */
    s.reset(inst);
    s.parse(inst, "cecenihao");
    s.sentence(inst);
    s.guess(inst, 0, GUESS_SORT);
    {
        lookup_candidate_t *row = find_first_by_type(&s, inst, NBEST_MATCH_CANDIDATE);
        if (!row) {
            fprintf(stderr, "nbest row not offered\n");
            return 1;
        }
        int cursor = s.choose(inst, 0, row);
        if (cursor < 0) {
            fprintf(stderr, "choose nbest row failed\n");
            return 1;
        }
        /* matrix.size() - 1 = parsed_len here (pinyin.cpp:2513-2519),
         * whatever span the row's own path covered. Pin the absolute
         * value: the log diff below cannot catch both engines answering
         * the same wrong cursor. */
        if (cursor != (int)strlen("cecenihao")) {
            fprintf(stderr, "nbest section: cursor %d is not the parse end %zu\n",
                    cursor, strlen("cecenihao"));
            return 1;
        }
        printf("nbest-cursor: %d\n", cursor);
        /* The corrected post-NBEST flow: the tail slot starts no span, so
         * re-run the sentence lookup under the diff_result forcings, then
         * train. Both must succeed — a matched pair of failures would
         * otherwise read IDENTICAL with the flow broken. */
        int resentenced = s.sentence(inst);
        printf("nbest-resentence: %s\n", resentenced ? "true" : "false");
        if (!resentenced) {
            fprintf(stderr, "nbest section: re-sentence failed\n");
            return 1;
        }
        int trained = s.train(inst, 0);
        printf("nbest-train: %s\n", trained ? "true" : "false");
        if (!trained) {
            fprintf(stderr, "nbest section: train failed\n");
            return 1;
        }
    }

    /* ── 4. train then predict ──────────────────────────────────────── */
    s.reset(inst);
    s.parse(inst, "cecenihao");
    s.sentence(inst);
    s.guess(inst, 0, GUESS_SORT);
    {
        lookup_candidate_t *first = find_by_text(&s, inst, "测测");
        if (!first) {
            fprintf(stderr, "测测 not offered for train\n");
            return 1;
        }
        /* The returned cursor is deliberately unused. On the capi this
         * candidate is an NBEST row (the typing gap above), so the cursor
         * is the whole parse end — the reserved tail slot, where no word
         * candidates start by design; on the pin it is a phrase candidate
         * and the cursor is the span end. Either way the next lookup
         * anchors at the chosen phrase's own extent — the "cece" this
         * driver imported in section 1. */
        if (s.choose(inst, 0, first) < 0) {
            fprintf(stderr, "choose 测测 failed\n");
            return 1;
        }
        s.sentence(inst);
        s.guess(inst, strlen("cece"), GUESS_SORT);
        {
            lookup_candidate_t *next = find_by_text(&s, inst, "你好");
            if (!next) {
                fprintf(stderr, "你好 not offered for train\n");
                return 1;
            }
            if (s.choose(inst, strlen("cece"), next) < 0) {
                fprintf(stderr, "choose 你好 failed\n");
                return 1;
            }
        }
        s.sentence(inst);
    }
    if (!s.train(inst, 0)) {
        fprintf(stderr, "train failed\n");
        return 1;
    }
    printf("predict: %s\n", s.predict(inst, "测测") ? "true" : "false");
    {
        guint n = 0;
        guint i;
        s.n_cand(inst, &n);
        for (i = 0; i < n; i++) {
            lookup_candidate_t *c = NULL;
            int type = 0;
            const char *text = NULL;
            s.get_cand(inst, i, &c);
            s.get_type(inst, c, &type);
            s.get_str(inst, c, &text);
            if (type == PREDICTED_PUNCTUATION_CANDIDATE)
                continue;
            if (type != PREDICTED_BIGRAM_CANDIDATE && type != PREDICTED_PREFIX_CANDIDATE)
                continue;
            printf("pred: type=%d text=%s\n", type, text ? text : "(null)");
        }
    }

    s.free_inst(inst);
    s.fini(ctx);
    return 0;
}
