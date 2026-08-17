/*
 * punct-diff.c — punctuation-prediction differential (#104).
 *
 * Guess predicted candidates for system prefixes that exist in both the
 * W3 mini phrase index and the pin's full tables, and print only
 * PREDICTED_PUNCTUATION (8) texts. Phrase suggestions stay off the log
 * so mini vs full prefix-suggestion lists cannot fail the comparison.
 *
 * Usage: ./punct-diff <path-to-so> <systemdir>
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
typedef bool (*fn_predict)(pinyin_instance_t *, const char *);
typedef bool (*fn_n_cand)(pinyin_instance_t *, guint *);
typedef bool (*fn_get_cand)(pinyin_instance_t *, guint, lookup_candidate_t **);
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

static void dump_puncts(fn_predict predict, fn_n_cand n_cand, fn_get_cand get_cand,
                        fn_get_type get_type, fn_get_str get_str,
                        pinyin_instance_t *inst, const char *prefix, const char *tag) {
    bool ok = predict(inst, prefix);
    printf("%s-predict: %s\n", tag, ok ? "true" : "false");
    guint n = 0;
    n_cand(inst, &n);
    unsigned seen = 0;
    for (guint i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        int type = 0;
        const char *text = NULL;
        get_cand(inst, i, &c);
        get_type(inst, c, &type);
        get_str(inst, c, &text);
        if (type != 8)
            continue;
        printf("%s-punct: %s\n", tag, text ? text : "(null)");
        seen++;
    }
    printf("%s-n: %u\n", tag, seen);
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
    fn_predict predict = (fn_predict)must(h, "pinyin_guess_predicted_candidates_with_punctuations");
    fn_n_cand n_cand = (fn_n_cand)must(h, "pinyin_get_n_candidate");
    fn_get_cand get_cand = (fn_get_cand)must(h, "pinyin_get_candidate");
    fn_get_type get_type = (fn_get_type)must(h, "pinyin_get_candidate_type");
    fn_get_str get_str = (fn_get_str)must(h, "pinyin_get_candidate_string");

    char userdir[] = "/tmp/punctdiff-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }
    pinyin_context_t *ctx = init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "init failed\n");
        return 1;
    }
    pinyin_instance_t *inst = alloc(ctx);
    if (!inst) {
        fprintf(stderr, "alloc failed\n");
        return 1;
    }

    /* Mini-table phrases that also exist in the pin's punct.table. */
    dump_puncts(predict, n_cand, get_cand, get_type, get_str, inst, "好", "hao");
    dump_puncts(predict, n_cand, get_cand, get_type, get_str, inst, "中", "zhong");
    dump_puncts(predict, n_cand, get_cand, get_type, get_str, inst, "国", "guo");
    dump_puncts(predict, n_cand, get_cand, get_type, get_str, inst, "中国", "zhongguo");
    dump_puncts(predict, n_cand, get_cand, get_type, get_str, inst, "你", "ni");

    free_inst(inst);
    fini(ctx);
    return 0;
}
