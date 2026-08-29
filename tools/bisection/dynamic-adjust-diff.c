/*
 * dynamic-adjust-diff.c — DYNAMIC_ADJUST candidate-ranking differential.
 *
 * The frozen corpus cannot exercise this bit. It is single-shot: one parse,
 * one guess at offset 0, and the frozen option words all leave
 * DYNAMIC_ADJUST (1<<9) clear. Under those conditions the bigram term is
 * absent by construction, so a corpus run proves nothing about it.
 *
 * This drives the shape that does exercise it: parse, guess a sentence so a
 * 1-best result exists, choose the first candidate, then guess again at the
 * offset the choose advanced to. At that offset upstream's
 * _get_previous_token reads the 1-best result and returns the chosen token,
 * Gate 2 merges its gram, and Gate 3 folds a bigram term into every
 * candidate's frequency — reordering the list.
 *
 * Prints one line per candidate as index|type|text. Compared exactly
 * between oxpinyin-capi and the pin-built libpinyin.
 *
 * NON-VACUITY: run twice, once with DYNAMIC_ADJUST set and once clear. The
 * two outputs must DIFFER on at least one input; if they are identical the
 * probe is not reaching the feature and a comparison that passes proves
 * nothing. `--mode` selects which word is used; run-dynamic-adjust-diff.sh
 * drives both and enforces the difference.
 *
 * Usage: ./dynamic-adjust-diff <path-to-so> <systemdir> <on|off> <userdir>
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
typedef uint32_t guint;

/* IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE | USE_RESPLIT_TABLE —
 * the parity word (tools/bisection/live-typing-diff.c). Bit 9 clear. */
#define PARITY_OPTIONS ((guint)0x18a)
#define DYNAMIC_ADJUST ((guint)0x200)
/* SORT_BY_PHRASE_LENGTH | PINYIN | FREQUENCY. */
#define DEFAULT_SORT ((guint)0x1e)

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef bool (*fn_set_options)(pinyin_context_t *, guint);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_inst)(pinyin_instance_t *);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_guess_sentence)(pinyin_instance_t *);
typedef bool (*fn_guess)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_n_cand)(pinyin_instance_t *, guint *);
typedef bool (*fn_get_cand)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_get_type)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_get_str)(pinyin_instance_t *, lookup_candidate_t *, const char **);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);

static void *must(void *handle, const char *name) {
    void *s = dlsym(handle, name);
    if (!s) {
        fprintf(stderr, "MISSING %s\n", name);
        exit(1);
    }
    return s;
}

/* Inputs long enough that a choose leaves a non-zero offset with more to
 * decode — the only shape where prev_token is non-null. */
static const char *const INPUTS[] = {
    "nihao", "beijing", "zhongguo", "womenshi", "xiexieni", "shijie", "pinyinshurufa",
};

int main(int argc, char **argv) {
    if (argc != 5) {
        fprintf(stderr, "Usage: %s <so> <systemdir> <on|off> <userdir>\n", argv[0]);
        return 1;
    }
    if (strcmp(argv[3], "on") != 0 && strcmp(argv[3], "off") != 0) {
        fprintf(stderr, "Usage: %s <so> <systemdir> <on|off> <userdir>\n", argv[0]);
        return 1;
    }
    const bool bit_on = strcmp(argv[3], "on") == 0;
    void *h = dlopen(argv[1], RTLD_NOW);
    if (!h) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    fn_init init = (fn_init)must(h, "pinyin_init");
    fn_fini fini = (fn_fini)must(h, "pinyin_fini");
    fn_set_options set_options = (fn_set_options)must(h, "pinyin_set_options");
    fn_alloc alloc = (fn_alloc)must(h, "pinyin_alloc_instance");
    fn_free_inst free_inst = (fn_free_inst)must(h, "pinyin_free_instance");
    fn_parse parse = (fn_parse)must(h, "pinyin_parse_more_full_pinyins");
    fn_guess_sentence guess_sentence = (fn_guess_sentence)must(h, "pinyin_guess_sentence");
    fn_guess guess = (fn_guess)must(h, "pinyin_guess_candidates");
    fn_n_cand n_cand = (fn_n_cand)must(h, "pinyin_get_n_candidate");
    fn_get_cand get_cand = (fn_get_cand)must(h, "pinyin_get_candidate");
    fn_get_type get_type = (fn_get_type)must(h, "pinyin_get_candidate_type");
    fn_get_str get_str = (fn_get_str)must(h, "pinyin_get_candidate_string");
    fn_choose choose = (fn_choose)must(h, "pinyin_choose_candidate");

    pinyin_context_t *ctx = init(argv[2], argv[4]);
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed\n");
        return 1;
    }
    const guint options = bit_on ? (PARITY_OPTIONS | DYNAMIC_ADJUST) : PARITY_OPTIONS;
    set_options(ctx, options);

    for (size_t i = 0; i < sizeof(INPUTS) / sizeof(INPUTS[0]); ++i) {
        pinyin_instance_t *inst = alloc(ctx);
        if (!inst) {
            fprintf(stderr, "alloc failed\n");
            return 1;
        }
        parse(inst, INPUTS[i]);
        /* A 1-best result must exist before _get_previous_token can read it. */
        guess_sentence(inst);
        if (!guess(inst, 0, DEFAULT_SORT)) {
            printf("%s|no-first-guess\n", INPUTS[i]);
            free_inst(inst);
            continue;
        }
        lookup_candidate_t *first = NULL;
        if (!get_cand(inst, 0, &first)) {
            printf("%s|no-first-candidate\n", INPUTS[i]);
            free_inst(inst);
            continue;
        }
        const int offset = choose(inst, 0, first);
        if (offset <= 0) {
            printf("%s|choose-did-not-advance|%d\n", INPUTS[i], offset);
            free_inst(inst);
            continue;
        }
        /* The offset the choose advanced to: prev_token is the chosen token
         * here, so the bit is live. */
        guess_sentence(inst);
        if (!guess(inst, (size_t)offset, DEFAULT_SORT)) {
            printf("%s|%d|no-second-guess\n", INPUTS[i], offset);
            free_inst(inst);
            continue;
        }
        guint n = 0;
        n_cand(inst, &n);
        for (guint k = 0; k < n; ++k) {
            lookup_candidate_t *cand = NULL;
            if (!get_cand(inst, k, &cand)) {
                continue;
            }
            int type = -1;
            const char *text = NULL;
            get_type(inst, cand, &type);
            get_str(inst, cand, &text);
            printf("%s|%d|%u|%d|%s\n", INPUTS[i], offset, k, type, text ? text : "");
        }
        free_inst(inst);
    }
    fini(ctx);
    dlclose(h);
    return 0;
}
