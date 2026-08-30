/*
 * pred-order-diff.c — predicted-prefix row-order differential (the B1
 * moving-number gate).
 *
 * Dumps every PREDICTED_PREFIX (type 5) candidate text, in list order,
 * for the eight prefixes of the uncovered-surface differential's punct
 * phase, one label per row:
 *
 *   pred-<tag>:[i]=<text>
 *
 * The runner diffs the two dumps and reports per-prefix position
 * mismatches — a number that moves (baseline, 2026-08-25: 好 174/178;
 * the others recorded in the findings doc) rather than a binary
 * pass/fail. This is the instrument for the B1 fix PR: the prefix
 * subtraction must drive the count to the recorded store-layout
 * divergence residual, not to zero.
 *
 * Usage: ./pred-order-diff <path-to-so> <systemdir>
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

typedef void pinyin_context_t;
typedef void pinyin_instance_t;
typedef void lookup_candidate_t;
typedef uint32_t guint;

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free)(pinyin_instance_t *);
typedef bool (*fn_predict)(pinyin_instance_t *, const char *);
typedef bool (*fn_n)(pinyin_instance_t *, guint *);
typedef bool (*fn_getc)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_gettype)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_getstr)(pinyin_instance_t *, lookup_candidate_t *, const char **);

/* Removes `dir` and everything under it, so no exit path leaks the
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

static void *must(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

/* One prefix: predict, then print every PREDICTED_PREFIX row in list
 * order. Returns 1 on accessor failure. */
static int dump_prefix(fn_predict predict, fn_n n_cand, fn_getc get_cand,
                       fn_gettype get_type, fn_getstr get_str,
                       pinyin_instance_t *inst, const char *prefix,
                       const char *tag) {
    if (!predict(inst, prefix)) {
        fprintf(stderr, "predict(%s) failed\n", tag);
        return 1;
    }
    guint n = 0;
    if (!n_cand(inst, &n)) {
        fprintf(stderr, "get_n_candidate failed after predict(%s)\n", tag);
        return 1;
    }
    for (guint i = 0; i < n; i++) {
        lookup_candidate_t *cand = NULL;
        if (!get_cand(inst, i, &cand) || !cand) {
            fprintf(stderr, "get_candidate(%u) failed for %s\n", i, tag);
            return 1;
        }
        int type = 0;
        if (!get_type(inst, cand, &type)) {
            fprintf(stderr, "get_candidate_type(%u) failed for %s\n", i, tag);
            return 1;
        }
        if (type != 5) /* PREDICTED_PREFIX only */
            continue;
        const char *text = NULL;
        if (!get_str(inst, cand, &text)) {
            fprintf(stderr, "get_candidate_string(%u) failed for %s\n", i, tag);
            return 1;
        }
        printf("pred-%s:[%u]=%s\n", tag, i, text ? text : "(null)");
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "Usage: %s <path-to-so> <systemdir>\n", argv[0]);
        return 1;
    }

    void *handle = dlopen(argv[1], RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    fn_init init = (fn_init)must(handle, "pinyin_init");
    fn_fini fini = (fn_fini)must(handle, "pinyin_fini");
    fn_alloc alloc = (fn_alloc)must(handle, "pinyin_alloc_instance");
    fn_free free_i = (fn_free)must(handle, "pinyin_free_instance");
    /* libpinyin < 2.11 (RHEL/Fedora ship 2.8.1) has no
     * _with_punctuations variant; the plain call emits the same
     * PREDICTED_PREFIX rows (the punctuations variant only prepends
     * punctuation rows of a different type, which the type filter below
     * drops), so falling back keeps the compared row set identical. */
    fn_predict predict = (fn_predict)dlsym(
        handle, "pinyin_guess_predicted_candidates_with_punctuations");
    if (!predict)
        predict = (fn_predict)must(handle, "pinyin_guess_predicted_candidates");
    fn_n n_cand = (fn_n)must(handle, "pinyin_get_n_candidate");
    fn_getc get_cand = (fn_getc)must(handle, "pinyin_get_candidate");
    fn_gettype get_type = (fn_gettype)must(handle, "pinyin_get_candidate_type");
    fn_getstr get_str = (fn_getstr)must(handle, "pinyin_get_candidate_string");

    char userdir[] = "/tmp/predorderdiff-user-XXXXXX";
    if (!mkdtemp(userdir)) {
        perror("mkdtemp");
        return 1;
    }
    pinyin_context_t *ctx = init(argv[2], userdir);
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed\n");
        rm_rf(userdir);
        return 1;
    }
    pinyin_instance_t *inst = alloc(ctx);
    if (!inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        fini(ctx);
        rm_rf(userdir);
        return 1;
    }

    /* The same eight prefixes as the punct phase of
     * uncovered-surface-diff.c, so the two differentials share the
     * subject set. */
    static const struct {
        const char *prefix; /* UTF-8 */
        const char *tag;
    } prefixes[] = {
        {"\xe5\xa5\xbd", "hao"},                      /* 好 */
        {"\xe7\x9a\x84", "de"},                       /* 的 */
        {"\xe4\xb8\x80", "yi"},                       /* 一 */
        {"\xe4\xbd\xa0", "ni"},                       /* 你 */
        {"\xe4\xb8\xad\xe5\x9b\xbd", "zhongguo"},     /* 中国 */
        {"\xe6\x88\x91", "wo"},                       /* 我 */
        {"\xe6\x98\xaf", "shi"},                      /* 是 */
        {"\xe4\xba\x86", "le"},                       /* 了 */
    };

    int failed = 0;
    for (size_t i = 0; i < sizeof(prefixes) / sizeof(prefixes[0]); i++) {
        if (dump_prefix(predict, n_cand, get_cand, get_type, get_str, inst,
                        prefixes[i].prefix, prefixes[i].tag)) {
            failed = 1;
            break;
        }
    }

    free_i(inst);
    fini(ctx);
    dlclose(handle);
    rm_rf(userdir);
    return failed;
}
