/*
 * addon-candidate-diff.c — W11 addon-candidate differential.
 *
 * Load addon library 4 (art) and print ADDON_CANDIDATE rows for "erhuang".
 * Compared exactly between oxpinyin-capi (public-ABI addon_4_*.redb) and
 * the pin (art.bin via pinyin_load_addon_phrase_library).
 *
 * Usage: ./addon-candidate-diff <path-to-so> <systemdir>
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

enum { ADDON_CANDIDATE = 6 };

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef bool (*fn_load_addon)(pinyin_context_t *, uint8_t);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_inst)(pinyin_instance_t *);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_guess)(pinyin_instance_t *, size_t, guint);
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
    fn_load_addon load_addon = (fn_load_addon)must(h, "pinyin_load_addon_phrase_library");
    fn_alloc alloc = (fn_alloc)must(h, "pinyin_alloc_instance");
    fn_free_inst free_inst = (fn_free_inst)must(h, "pinyin_free_instance");
    fn_parse parse = (fn_parse)must(h, "pinyin_parse_more_full_pinyins");
    fn_guess guess = (fn_guess)must(h, "pinyin_guess_candidates");
    fn_n_cand n_cand = (fn_n_cand)must(h, "pinyin_get_n_candidate");
    fn_get_cand get_cand = (fn_get_cand)must(h, "pinyin_get_candidate");
    fn_get_type get_type = (fn_get_type)must(h, "pinyin_get_candidate_type");
    fn_get_str get_str = (fn_get_str)must(h, "pinyin_get_candidate_string");

    char userdir[] = "/tmp/addoncand-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }
    pinyin_context_t *ctx = init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "init failed\n");
        return 1;
    }
    printf("load_addon(4): %s\n", load_addon(ctx, 4) ? "true" : "false");
    printf("load_addon(4) again: %s\n", load_addon(ctx, 4) ? "true" : "false");

    pinyin_instance_t *inst = alloc(ctx);
    parse(inst, "erhuang");
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
        if (type == ADDON_CANDIDATE)
            printf("addon: %s\n", text ? text : "(null)");
    }
    free_inst(inst);
    fini(ctx);
    return 0;
}
