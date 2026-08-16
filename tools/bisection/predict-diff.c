/*
 * predict-diff.c — W11 phrase-prediction differential (non-punctuation).
 *
 * Train a user phrase, then guess predicted candidates for a prefix and
 * print type|text. Punctuation is stubbed empty on oxpinyin (#104);
 * this driver compares the non-punctuation prediction list.
 *
 * Usage: ./predict-diff <path-to-so> <systemdir>
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
typedef uint32_t guint;

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_inst)(pinyin_instance_t *);
typedef void import_iterator_t;
typedef import_iterator_t *(*fn_begin_add)(pinyin_context_t *, uint8_t);
typedef bool (*fn_add)(import_iterator_t *, const char *, const char *, int);
typedef void (*fn_end_add)(import_iterator_t *);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_guess)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_get_cand)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_train)(pinyin_instance_t *, uint8_t);
typedef bool (*fn_sentence)(pinyin_instance_t *);
typedef bool (*fn_predict)(pinyin_instance_t *, const char *);
typedef bool (*fn_n_cand)(pinyin_instance_t *, guint *);
typedef bool (*fn_get_type)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_get_str)(pinyin_instance_t *, lookup_candidate_t *, const char **);

static void *must(void *handle, const char *name) {
    void *s = dlsym(handle, name);
    if (!s) {
        fprintf(stderr, "MISSING %s\n", name);
        exit(1);
    }
    return s;
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

    fn_init init = (fn_init)dlsym(h, "oxpinyin_init_for_fixtures");
    if (!init)
        init = (fn_init)must(h, "pinyin_init");
    fn_fini fini = (fn_fini)must(h, "pinyin_fini");
    fn_alloc alloc = (fn_alloc)must(h, "pinyin_alloc_instance");
    fn_free_inst free_inst = (fn_free_inst)must(h, "pinyin_free_instance");
    fn_begin_add begin_add = (fn_begin_add)must(h, "pinyin_begin_add_phrases");
    fn_add add = (fn_add)must(h, "pinyin_iterator_add_phrase");
    fn_end_add end_add = (fn_end_add)must(h, "pinyin_end_add_phrases");
    fn_parse parse = (fn_parse)must(h, "pinyin_parse_more_full_pinyins");
    fn_guess guess = (fn_guess)must(h, "pinyin_guess_candidates");
    fn_get_cand get_cand = (fn_get_cand)must(h, "pinyin_get_candidate");
    fn_choose choose = (fn_choose)must(h, "pinyin_choose_candidate");
    fn_train train = (fn_train)must(h, "pinyin_train");
    fn_sentence sentence = (fn_sentence)must(h, "pinyin_guess_sentence");
    fn_predict predict = (fn_predict)must(h, "pinyin_guess_predicted_candidates_with_punctuations");
    fn_n_cand n_cand = (fn_n_cand)must(h, "pinyin_get_n_candidate");
    fn_get_type get_type = (fn_get_type)must(h, "pinyin_get_candidate_type");
    fn_get_str get_str = (fn_get_str)must(h, "pinyin_get_candidate_string");

    char userdir[] = "/tmp/predictdiff-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }
    pinyin_context_t *ctx = init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "init failed\n");
        return 1;
    }

    import_iterator_t *it = begin_add(ctx, 7);
    add(it, "测测", "cece", 5);
    end_add(it);

    pinyin_instance_t *inst = alloc(ctx);
    /* Train 测测 → 你好 so prediction from "测测" has a user-bigram
     * successor. First-seed 69 already clears the count>=10 filter. */
    parse(inst, "cecenihao");
    sentence(inst);
    guess(inst, 0, 0x1e);
    {
        guint n = 0;
        n_cand(inst, &n);
        lookup_candidate_t *chosen = NULL;
        guint i;
        for (i = 0; i < n; i++) {
            lookup_candidate_t *c = NULL;
            const char *text = NULL;
            get_cand(inst, i, &c);
            get_str(inst, c, &text);
            if (text && strcmp(text, "测测") == 0) {
                chosen = c;
                break;
            }
        }
        if (!chosen) {
            fprintf(stderr, "测测 not offered\n");
            return 1;
        }
        int cursor = choose(inst, 0, chosen);
        if (cursor < 0) {
            fprintf(stderr, "choose 测测 failed\n");
            return 1;
        }
        sentence(inst);
        guess(inst, (size_t)cursor, 0x1e);
        {
            guint n2 = 0;
            n_cand(inst, &n2);
            lookup_candidate_t *next = NULL;
            for (i = 0; i < n2; i++) {
                lookup_candidate_t *c = NULL;
                const char *text = NULL;
                get_cand(inst, i, &c);
                get_str(inst, c, &text);
                if (text && strcmp(text, "你好") == 0) {
                    next = c;
                    break;
                }
            }
            if (!next) {
                fprintf(stderr, "你好 not offered\n");
                return 1;
            }
            if (choose(inst, (size_t)cursor, next) < 0) {
                fprintf(stderr, "choose 你好 failed\n");
                return 1;
            }
        }
        sentence(inst);
    }
    if (!train(inst, 0)) {
        fprintf(stderr, "train failed\n");
        return 1;
    }

    bool ok = predict(inst, "测测");
    printf("predict: %s\n", ok ? "true" : "false");
    guint n = 0;
    n_cand(inst, &n);
    for (guint i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        int type = 0;
        const char *text = NULL;
        get_cand(inst, i, &c);
        get_type(inst, c, &type);
        get_str(inst, c, &text);
        /* Phrase-prediction only: skip punctuation (8) and system prefix
         * suggestions so the comparison is the trained user bigram. */
        if (type != 4)
            continue;
        printf("bigram: %s\n", text ? text : "(null)");
    }
    free_inst(inst);
    fini(ctx);
    return 0;
}
