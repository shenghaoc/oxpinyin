/*
 * user-candidate-diff.c — W11 user-phrase surface differential.
 *
 * Import a unique user phrase through the add/iterator/end trio, re-parse,
 * and print every candidate as type|text|user. Compared exactly between
 * oxpinyin-capi and the pin-built libpinyin.
 *
 * Usage: ./user-candidate-diff <path-to-so> <systemdir>
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

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_inst)(pinyin_instance_t *);
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
    fn_n_cand n_cand = (fn_n_cand)must(h, "pinyin_get_n_candidate");
    fn_get_cand get_cand = (fn_get_cand)must(h, "pinyin_get_candidate");
    fn_get_type get_type = (fn_get_type)must(h, "pinyin_get_candidate_type");
    fn_get_str get_str = (fn_get_str)must(h, "pinyin_get_candidate_string");
    fn_is_user is_user = (fn_is_user)must(h, "pinyin_is_user_candidate");

    char userdir[] = "/tmp/usercand-XXXXXX";
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
    parse(inst, "cece");
    guess(inst, 0, 0x1e);
    guint n = 0;
    n_cand(inst, &n);
    for (guint i = 0; i < n; i++) {
        lookup_candidate_t *c = NULL;
        int type = 0;
        const char *text = NULL;
        get_cand(inst, i, &c);
        get_type(inst, c, &type);
        get_str(inst, c, &text);
        if (is_user(inst, c))
            printf("user %u: type=%d text=%s\n", i, type, text ? text : "(null)");
    }
    free_inst(inst);
    fini(ctx);
    return 0;
}
