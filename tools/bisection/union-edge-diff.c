/*
 * union-edge-diff.c — prediction filter-edge differential (count 9 vs 10).
 *
 * Public pinyin_train first-seeds 69 (23*3), so the copied upstream
 * threshold (`const guint32 filter = 10` at pinyin.cpp:2311,
 * `m_count < filter` at :2349-2350) is not reachable through the C ABI.
 * This driver imports three user phrases, then either plants the 9/10
 * rows (capi test symbol) or leaves a saved user dir for
 * plant-oracle-bigram.cc. A later predict pass prints whether each
 * successor survived as PREDICTED_BIGRAM.
 *
 * Usage:
 *   ./union-edge-diff <so> <systemdir> setup   <userdir>
 *   ./union-edge-diff <so> <systemdir> plant   <userdir>
 *   ./union-edge-diff <so> <systemdir> predict <userdir>
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
typedef void import_iterator_t;
typedef uint32_t guint;

enum { PREDICTED_BIGRAM_CANDIDATE = 4 };

#define EDGE_PREFIX "测测"
#define EDGE_BELOW "甲甲"
#define EDGE_AT "乙乙"
#define EDGE_PREFIX_PY "cece"
#define EDGE_BELOW_PY "jiajia"
#define EDGE_AT_PY "yiyi"
#define EDGE_BELOW_COUNT 9
#define EDGE_AT_COUNT 10

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_inst)(pinyin_instance_t *);
typedef import_iterator_t *(*fn_begin_add)(pinyin_context_t *, uint8_t);
typedef bool (*fn_add)(import_iterator_t *, const char *, const char *, int);
typedef void (*fn_end_add)(import_iterator_t *);
typedef bool (*fn_n_cand)(pinyin_instance_t *, guint *);
typedef bool (*fn_get_cand)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_get_type)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_get_str)(pinyin_instance_t *, lookup_candidate_t *, const char **);
typedef bool (*fn_predict)(pinyin_instance_t *, const char *);
typedef bool (*fn_save)(pinyin_context_t *);
typedef bool (*fn_plant)(pinyin_context_t *, const char *, const char *, uint64_t);

struct syms {
    fn_init init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_inst free_inst;
    fn_begin_add begin_add;
    fn_add add;
    fn_end_add end_add;
    fn_n_cand n_cand;
    fn_get_cand get_cand;
    fn_get_type get_type;
    fn_get_str get_str;
    fn_predict predict;
    fn_save save;
    fn_plant plant;
};

static void *must(void *handle, const char *name) {
    void *s = dlsym(handle, name);
    if (!s) {
        fprintf(stderr, "MISSING %s\n", name);
        exit(1);
    }
    return s;
}

static int import_edge_phrases(const struct syms *s, pinyin_context_t *ctx) {
    import_iterator_t *it = s->begin_add(ctx, 7);
    if (!it)
        return 0;
    int ok = s->add(it, EDGE_PREFIX, EDGE_PREFIX_PY, 5) &&
             s->add(it, EDGE_BELOW, EDGE_BELOW_PY, 5) &&
             s->add(it, EDGE_AT, EDGE_AT_PY, 5);
    s->end_add(it);
    return ok;
}

static void print_edge_hit(const struct syms *s, pinyin_instance_t *inst,
                           int want_count, const char *want) {
    guint n = 0;
    guint i;
    s->n_cand(inst, &n);
    for (i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        int type = 0;
        const char *text = NULL;
        s->get_cand(inst, i, &c);
        s->get_type(inst, c, &type);
        s->get_str(inst, c, &text);
        if (type == PREDICTED_BIGRAM_CANDIDATE && text && strcmp(text, want) == 0) {
            printf("pred-edge-%d: %s type=%d\n", want_count, want, type);
            return;
        }
    }
    printf("pred-edge-%d: %s absent\n", want_count, want);
}

int main(int argc, char **argv) {
    if (argc != 5) {
        fprintf(stderr, "Usage: %s <so> <systemdir> setup|plant|predict <userdir>\n",
                argv[0]);
        return 1;
    }
    const char *cmd = argv[3];
    const char *userdir = argv[4];
    int do_setup = strcmp(cmd, "setup") == 0;
    int do_plant = strcmp(cmd, "plant") == 0;
    int do_predict = strcmp(cmd, "predict") == 0;
    if (!do_setup && !do_plant && !do_predict) {
        fprintf(stderr, "Usage: %s <so> <systemdir> setup|plant|predict <userdir>\n",
                argv[0]);
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
    s.begin_add = (fn_begin_add)must(h, "pinyin_begin_add_phrases");
    s.add = (fn_add)must(h, "pinyin_iterator_add_phrase");
    s.end_add = (fn_end_add)must(h, "pinyin_end_add_phrases");
    s.n_cand = (fn_n_cand)must(h, "pinyin_get_n_candidate");
    s.get_cand = (fn_get_cand)must(h, "pinyin_get_candidate");
    s.get_type = (fn_get_type)must(h, "pinyin_get_candidate_type");
    s.get_str = (fn_get_str)must(h, "pinyin_get_candidate_string");
    s.predict = (fn_predict)must(h, "pinyin_guess_predicted_candidates_with_punctuations");
    s.save = (fn_save)must(h, "pinyin_save");
    s.plant = (fn_plant)dlsym(h, "oxpinyin_test_set_user_bigram");

    pinyin_context_t *ctx = s.init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "init failed\n");
        return 1;
    }

    if (do_setup) {
        if (!import_edge_phrases(&s, ctx)) {
            fprintf(stderr, "edge import failed\n");
            s.fini(ctx);
            return 1;
        }
        if (!s.save(ctx)) {
            fprintf(stderr, "edge save failed\n");
            s.fini(ctx);
            return 1;
        }
        printf("edge-setup: %s %s %s\n", EDGE_PREFIX, EDGE_BELOW, EDGE_AT);
        s.fini(ctx);
        return 0;
    }

    if (do_plant) {
        if (!s.plant) {
            fprintf(stderr, "oxpinyin_test_set_user_bigram not in this .so\n");
            s.fini(ctx);
            return 3;
        }
        if (!s.plant(ctx, EDGE_PREFIX, EDGE_BELOW, EDGE_BELOW_COUNT) ||
            !s.plant(ctx, EDGE_PREFIX, EDGE_AT, EDGE_AT_COUNT)) {
            fprintf(stderr, "plant 9/10 failed\n");
            s.fini(ctx);
            return 1;
        }
        printf("edge-plant: %s->%s=%d %s->%s=%d\n", EDGE_PREFIX, EDGE_BELOW,
               EDGE_BELOW_COUNT, EDGE_PREFIX, EDGE_AT, EDGE_AT_COUNT);
        s.fini(ctx);
        return 0;
    }

    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        fprintf(stderr, "alloc failed\n");
        s.fini(ctx);
        return 1;
    }
    printf("predict-edge: %s\n", s.predict(inst, EDGE_PREFIX) ? "true" : "false");
    print_edge_hit(&s, inst, EDGE_BELOW_COUNT, EDGE_BELOW);
    print_edge_hit(&s, inst, EDGE_AT_COUNT, EDGE_AT);
    s.free_inst(inst);
    s.fini(ctx);
    return 0;
}
