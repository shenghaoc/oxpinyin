/*
 * dict-surface-diff.c — Tier-C ABI differential: the dictionary-
 * introspection surface (`pinyin_lookup_tokens`, `pinyin_token_get_*`,
 * `pinyin_token_add_unigram_frequency`) and the phrase-library
 * load/unload pair.
 *
 * None of these eight symbols has a consumer call site in either
 * frontend, so this driver is their only oracle coverage: token sweeps
 * from phrase lookups feed the per-token reads; add-then-read sequences
 * pin the overlay semantics including the absent-token false whose
 * facade-total bump still shifts the prediction denominator; the
 * load/unload retval table pins the already-loaded / GBK-only /
 * already-unloaded laws; and a prediction dump before/after the adds
 * makes the denominator shift observable through candidate order.
 *
 * Usage: ./dict-surface-diff <path-to-so> <systemdir>
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
typedef struct GArrayView { char *data; unsigned int len; } GArrayView;
typedef GArrayView GArray;
typedef char gchar;

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_inst)(pinyin_instance_t *);
typedef bool (*fn_lookup_tokens)(pinyin_instance_t *, const char *, GArray *);
typedef bool (*fn_token_phrase)(pinyin_instance_t *, uint32_t, unsigned int *, gchar **);
typedef bool (*fn_token_n_pron)(pinyin_instance_t *, uint32_t, unsigned int *);
typedef bool (*fn_token_nth_pron)(pinyin_instance_t *, uint32_t, unsigned int, GArray *);
typedef bool (*fn_token_unigram)(pinyin_instance_t *, uint32_t, unsigned int *);
typedef bool (*fn_token_add)(pinyin_instance_t *, uint32_t, unsigned int);
typedef bool (*fn_load)(pinyin_context_t *, uint8_t);
typedef bool (*fn_unload)(pinyin_context_t *, uint8_t);
typedef bool (*fn_predict)(pinyin_instance_t *, const char *);
typedef bool (*fn_n_cand)(pinyin_instance_t *, unsigned int *);
typedef bool (*fn_get_cand)(pinyin_instance_t *, unsigned int, lookup_candidate_t **);
typedef bool (*fn_get_type)(pinyin_instance_t *, lookup_candidate_t *, int *);
typedef bool (*fn_get_str)(pinyin_instance_t *, lookup_candidate_t *, const char **);
typedef void (*fn_reset)(pinyin_instance_t *);

/* glib: the caller-side array is a REAL glib GArray (created through
 * g_array_new, torn down through g_array_free) — the pin's
 * g_array_append_vals reads the array's private element-size fields,
 * so a mirror struct crashes it (SIGFPE in g_array_maybe_expand,
 * observed first-hand). The public data/len prefix is what both
 * engines' appends and the driver's reads go through. */
extern GArray * g_array_new(int zero_terminated, int clear, unsigned int element_size);
extern void g_array_free(GArray * array, int free_segment);
/* pinyin_token_get_phrase's `utf8_str` is a caller-owned `gchar *`
 * released with `g_free` per the pin's contract (see pinyin.h). */
extern void g_free(void *ptr);

static GArray *tokenarray_new(void) {
    return g_array_new(0, 0, 4);
}

static void tokenarray_free(GArray *a) {
    g_array_free(a, 1);
}

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
    const char *systemdir = argv[2];

    fn_init init = (fn_init)dlsym(h, "oxpinyin_init_for_fixtures");
    if (!init)
        init = (fn_init)must(h, "pinyin_init");
    fn_fini fini = (fn_fini)must(h, "pinyin_fini");
    fn_alloc alloc = (fn_alloc)must(h, "pinyin_alloc_instance");
    fn_free_inst free_inst = (fn_free_inst)must(h, "pinyin_free_instance");
    fn_lookup_tokens lookup_tokens = (fn_lookup_tokens)must(h, "pinyin_lookup_tokens");
    fn_token_phrase token_phrase = (fn_token_phrase)must(h, "pinyin_token_get_phrase");
    fn_token_n_pron token_n_pron =
        (fn_token_n_pron)must(h, "pinyin_token_get_n_pronunciation");
    fn_token_nth_pron token_nth_pron =
        (fn_token_nth_pron)must(h, "pinyin_token_get_nth_pronunciation");
    fn_token_unigram token_unigram =
        (fn_token_unigram)must(h, "pinyin_token_get_unigram_frequency");
    fn_token_add token_add = (fn_token_add)must(h, "pinyin_token_add_unigram_frequency");
    fn_load load = (fn_load)must(h, "pinyin_load_phrase_library");
    fn_unload unload = (fn_unload)must(h, "pinyin_unload_phrase_library");
    fn_predict predict = (fn_predict)must(h, "pinyin_guess_predicted_candidates_with_punctuations");
    fn_n_cand n_cand = (fn_n_cand)must(h, "pinyin_get_n_candidate");
    fn_get_cand get_cand = (fn_get_cand)must(h, "pinyin_get_candidate");
    fn_get_type get_type = (fn_get_type)must(h, "pinyin_get_candidate_type");
    fn_get_str get_str = (fn_get_str)must(h, "pinyin_get_candidate_string");
    fn_reset reset = (fn_reset)must(h, "pinyin_reset");

    fprintf(stderr, "marker: dlopen done\n");
    setvbuf(stdout, NULL, _IONBF, 0);
    pinyin_context_t *ctx = init(systemdir, "");
    fprintf(stderr, "marker: init done\n");
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed\n");
        return 1;
    }
    pinyin_instance_t *inst = alloc(ctx);
    if (!inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        return 1;
    }

    /* Token lookups: stored phrases, unknown phrases, reused array. */
    /* No empty string: the pin SIGFPEs in the phrase-table search on
     * a zero-length lookup — a theirs-bug shape the no-abort policy
     * cannot reproduce; excluded and recorded. */
    const char *phrases[] = {"你好", "中国", "你好中国", "不存在词", "abcd"};
    for (unsigned i = 0; i < sizeof(phrases) / sizeof(phrases[0]); ++i) {
        GArray *a = tokenarray_new();
        bool ok = lookup_tokens(inst, phrases[i], a);
        printf("lookup|%s|%d|n=%u|", phrases[i], ok ? 1 : 0, a->len);
        for (unsigned k = 0; k < a->len; ++k) {
            uint32_t token;
            memcpy(&token, a->data + (size_t)k * 4, 4);
            printf("%u,", token);
        }
        printf("\n");
        tokenarray_free(a);
    }

    /* Collect the tokens of 你好 and 中国 for the per-token probes. */
    uint32_t nihao_token = 0, zhongguo_token = 0;
    {
        GArray *a = tokenarray_new();
        lookup_tokens(inst, "你好", a);
        if (a->len)
            memcpy(&nihao_token, a->data, 4);
        tokenarray_free(a);
        a = tokenarray_new();
        lookup_tokens(inst, "中国", a);
        if (a->len)
            memcpy(&zhongguo_token, a->data, 4);
        tokenarray_free(a);
    }

    /* Per-token reads: text, pronunciation count, unigram frequency. */
    const uint32_t tokens[] = {nihao_token, zhongguo_token};
    const char *token_names[] = {"你好", "中国"};
    for (unsigned i = 0; i < sizeof(tokens) / sizeof(tokens[0]); ++i) {
        fprintf(stderr, "probe token %u\n", i);
        unsigned int len = 0, num = 0, freq = 0;
        bool ok_phrase = token_phrase(inst, tokens[i], &len, NULL);
        gchar *text = NULL;
        bool ok_text = token_phrase(inst, tokens[i], &len, &text);
        bool ok_num = token_n_pron(inst, tokens[i], &num);
        bool ok_freq = token_unigram(inst, tokens[i], &freq);
        printf("token|%s|phrase=%d|len=%u|text_ok=%d|text=%s|npron=%d|pron=%u|freq_ok=%d|freq=%u\n",
               token_names[i], ok_phrase ? 1 : 0, len,
               ok_text ? 1 : 0, text ? text : "-",
               ok_num ? 1 : 0, num, ok_freq ? 1 : 0, freq);
        /* Release the pin-allocated string through the ABI's own
         * allocator; skipping this leaks per token, and using the wrong
         * allocator (free) would crash on glibc's aligned buckets. */
        if (text) {
            g_free(text);
            text = NULL;
        }

        /* Cover pinyin_token_get_nth_pronunciation for every pronunciation:
         * both retval and the packed two-byte key sequence go into the diff.
         * element_size=2 matches the packed ChewingKey word the surface
         * writes; g_array_free releases the buffer between rounds. */
        for (unsigned int nth = 0; nth < num; ++nth) {
            GArray *keys = g_array_new(0, 0, 2);
            bool ok_nth = token_nth_pron(inst, tokens[i], nth, keys);
            printf("nth-pron|%s|nth=%u|ok=%d|nkeys=%u|", token_names[i], nth,
                   ok_nth ? 1 : 0, keys->len);
            for (unsigned int k = 0; k < keys->len; ++k) {
                uint16_t packed;
                memcpy(&packed, keys->data + (size_t)k * 2, 2);
                printf("%04x,", packed);
            }
            printf("\n");
            g_array_free(keys, 1);
        }

        /* The add-then-read overlay: +11 visible on the next read. */
        bool add_ok = token_add(inst, tokens[i], 11);
        unsigned int freq2 = 0;
        token_unigram(inst, tokens[i], &freq2);
        printf("token|%s|add11=%d|freq=%u|shift=%d\n", token_names[i],
               add_ok ? 1 : 0, freq2, (freq2 >= freq + 11) ? 1 : 0);
    }

    /* The absent-token add: false, but the facade total still moves —
     * the differential makes that observable through the prediction
     * order shift at the end. */
    bool absent_add = token_add(inst, 0x09FFFFFE, 500);
    printf("absent-add=%d\n", absent_add ? 1 : 0);

    /* Prediction before/after the adds: the denominator shift is
     * observable through the candidate list shape. */
    reset(inst);
    fprintf(stderr, "probe predict before\n");
    bool pred_before = predict(inst, "你");
    printf("pred-before=%d|", pred_before ? 1 : 0);
    {
        unsigned int n = 0;
        n_cand(inst, &n);
        printf("n=%u\n", n);
        for (unsigned int i = 0; i < n; ++i) {
            lookup_candidate_t *cand = NULL;
            int type = 0;
            const char *text = NULL;
            bool ok_get = get_cand(inst, i, &cand);
            bool ok_type = get_type(inst, cand, &type);
            bool ok_str = get_str(inst, cand, &text);
            printf("pred-before-cand|%u|get=%d|type=%d|type_ok=%d|text=%s|str_ok=%d\n",
                   i, ok_get ? 1 : 0, type, ok_type ? 1 : 0,
                   text ? text : "-", ok_str ? 1 : 0);
        }
    }
    token_add(inst, tokens[0], 25);
    reset(inst);
    bool pred_after = predict(inst, "你");
    printf("pred-after=%d|", pred_after ? 1 : 0);
    {
        unsigned int n = 0;
        n_cand(inst, &n);
        printf("n=%u\n", n);
        for (unsigned int i = 0; i < n; ++i) {
            lookup_candidate_t *cand = NULL;
            int type = 0;
            const char *text = NULL;
            bool ok_get = get_cand(inst, i, &cand);
            bool ok_type = get_type(inst, cand, &type);
            bool ok_str = get_str(inst, cand, &text);
            printf("pred-after-cand|%u|get=%d|type=%d|type_ok=%d|text=%s|str_ok=%d\n",
                   i, ok_get ? 1 : 0, type, ok_type ? 1 : 0,
                   text ? text : "-", ok_str ? 1 : 0);
        }
    }

    /* Load/unload retval table: GBK (2) is loaded at init; every other
     * index refuses both directions. */
    /* In-range indexes only: the pin ASSERTS on 16+ (pinyin.cpp:466) —
     * the no-abort refusals are pinned by the Rust ABI suite, same
     * exclusion as the addon-unload driver. */
    const uint8_t indexes[] = {0, 1, 2, 3, 4, 5, 6, 7};
    for (unsigned i = 0; i < sizeof(indexes) / sizeof(indexes[0]); ++i) {
        printf("unload|%u|%d\n", indexes[i], unload(ctx, indexes[i]) ? 1 : 0);
    }
    /* Load probes only on the file types the pin accepts — RESERVED /
     * OPENGRAM / ADDON / NETWORK hit the SYSTEM_FILE||USER_FILE assert
     * at pinyin.cpp:457 (measured abort; the refusals are pinned by the
     * Rust ABI suite). */
    const uint8_t load_indexes[] = {1, 2, 4, 7};
    for (unsigned i = 0; i < sizeof(load_indexes) / sizeof(load_indexes[0]); ++i) {
        printf("load|%u|%d\n", load_indexes[i], load(ctx, load_indexes[i]) ? 1 : 0);
    }

    free_inst(inst);
    fini(ctx);
    return 0;
}
