/*
 * bigram-export-diff.c — user-bigram export differential driver
 * (issue #541, register row 36).
 *
 * Opens a pinyin shared object (libpinyin_capi.so or the pinned
 * libpinyin.so) on <systemdir> with a fresh user directory, trains one
 * scenario through the public ABI (parse, choose each named candidate in
 * turn, train), then runs ONE bigram export — the pin's iterator keeps
 * state a second export in the same context trips over — printing, in
 * iteration order:
 *
 *   has_next0 <bool>
 *   row <phrase>\t<pinyin>\t<count>\tget=<bool>
 *   rows=<n>
 *   has_next_after_false <bool>
 *
 * get= is pinyin_bigram_iterator_get_next_phrase's return, which the pin
 * defines as has_next after the row (pinyin.cpp:896-911, register row 36).
 * The last line calls has_next once more after it answered false: the pin's
 * iterator then walks the gram it loaded but never scanned
 * (pinyin.cpp:875-892).
 *
 * The export walks the pin's own user-bigram container in its DB order
 * (Bigram::get_all_items, pinyin.cpp:776-787), so run it against a prefix
 * built with tools/bisection/patches/bigram-export-strjoinv: the unpatched
 * pin reads past its join arrays (pinyin.cpp:843-849) and crashes on most
 * heaps before a second row.
 *
 * Scenarios (argv[3]): empty, one, two, many, repeat — see scenario().
 * Training chooses each named phrase as a normal candidate, never the
 * n-best sentence candidate: choosing the top n-best result trains nothing
 * on the pin and a sentence_start gram on oxpinyin (#603), which is not
 * this driver's subject.
 *
 * Usage:
 *   ./bigram-export-diff <path-to-so> <systemdir> <scenario>
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

typedef void ctx_t;
typedef void inst_t;
typedef void iter_t;

static void *load_symbol(void *handle, const char *name) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

/* Each entry: the full-pinyin input, then the candidates to choose in
 * order, comma-separated. A sentence's first phrase follows sentence_start,
 * so every scenario also stores sentence_start grams. */
static const char *const *scenario(const char *name) {
    static const char *const empty[] = {NULL};
    static const char *const one[] = {"nihao:你,好", NULL};
    static const char *const two[] = {"nihao:你,好", "woai:我,爱", NULL};
    static const char *const many[] = {
        "nihao:你,好", "woai:我,爱", "tahen:他,很", "nihaoshijie:你好,世界",
        "zhongguorenmin:中国,人民", "woaibeijing:我,爱,北京", "jintian:今天",
        "wohenhao:我,很,好", NULL,
    };
    static const char *const repeat[] = {
        "nihao:你,好", "nihao:你,好", "nihao:你,好", "woai:我,爱",
        "nihaoshijie:你好,世界", "nihaoshijie:你好,世界", NULL,
    };
    if (0 == strcmp(name, "empty"))
        return empty;
    if (0 == strcmp(name, "one"))
        return one;
    if (0 == strcmp(name, "two"))
        return two;
    if (0 == strcmp(name, "many"))
        return many;
    if (0 == strcmp(name, "repeat"))
        return repeat;
    return NULL;
}

int main(int argc, char **argv) {
    if (argc != 4 || !scenario(argv[3])) {
        fprintf(stderr, "usage: %s <so> <systemdir> empty|one|two|many|repeat\n", argv[0]);
        return 2;
    }
    void *h = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!h) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    ctx_t *(*init)(const char *, const char *) = load_symbol(h, "pinyin_init");
    void (*fini)(ctx_t *) = load_symbol(h, "pinyin_fini");
    inst_t *(*alloc)(ctx_t *) = load_symbol(h, "pinyin_alloc_instance");
    void (*free_instance)(inst_t *) = load_symbol(h, "pinyin_free_instance");
    size_t (*parse)(inst_t *, const char *) = load_symbol(h, "pinyin_parse_more_full_pinyins");
    bool (*guess_sentence)(inst_t *) = load_symbol(h, "pinyin_guess_sentence");
    bool (*guess_candidates)(inst_t *, size_t, uint32_t) = load_symbol(h, "pinyin_guess_candidates");
    bool (*get_n)(inst_t *, uint32_t *) = load_symbol(h, "pinyin_get_n_candidate");
    bool (*get_candidate)(inst_t *, uint32_t, void **) = load_symbol(h, "pinyin_get_candidate");
    bool (*get_string)(inst_t *, void *, const char **) = load_symbol(h, "pinyin_get_candidate_string");
    bool (*get_type)(inst_t *, void *, int *) = load_symbol(h, "pinyin_get_candidate_type");
    int (*choose)(inst_t *, size_t, void *) = load_symbol(h, "pinyin_choose_candidate");
    bool (*train)(inst_t *, uint8_t) = load_symbol(h, "pinyin_train");
    bool (*reset)(inst_t *) = load_symbol(h, "pinyin_reset");
    iter_t *(*begin)(ctx_t *) = load_symbol(h, "pinyin_begin_get_bigram_phrases");
    bool (*has_next)(iter_t *) = load_symbol(h, "pinyin_bigram_iterator_has_next_phrase");
    bool (*get_next)(iter_t *, char **, char **, int32_t *) =
        load_symbol(h, "pinyin_bigram_iterator_get_next_phrase");
    void (*end)(iter_t *) = load_symbol(h, "pinyin_end_get_bigram_phrases");
    void (*g_free_fn)(void *) = free;
    void *glib = dlopen("libglib-2.0.so.0", RTLD_NOW);
    if (glib && dlsym(glib, "g_free"))
        g_free_fn = (void (*)(void *))dlsym(glib, "g_free");

    setvbuf(stdout, NULL, _IONBF, 0);
    mkdir("user", 0700);
    ctx_t *ctx = init(argv[2], "user");
    if (!ctx) {
        printf("init: NULL\n");
        return 1;
    }
    inst_t *instance = alloc(ctx);

    for (const char *const *entry = scenario(argv[3]); *entry; ++entry) {
        char buffer[256];
        snprintf(buffer, sizeof buffer, "%s", *entry);
        char *words = strchr(buffer, ':');
        *words++ = '\0';
        reset(instance);
        parse(instance, buffer);
        guess_sentence(instance);
        size_t offset = 0;
        bool ok = true;
        for (char *word = strtok(words, ","); word && ok; word = strtok(NULL, ",")) {
            guess_candidates(instance, offset, 0x1e /* SORT_BY_PHRASE_LENGTH|PINYIN|FREQUENCY */);
            uint32_t n = 0;
            get_n(instance, &n);
            void *hit = NULL;
            for (uint32_t k = 0; k < n && !hit; ++k) {
                void *candidate = NULL;
                const char *text = NULL;
                int type = 0;
                if (get_candidate(instance, k, &candidate) && get_string(instance, candidate, &text) &&
                    text && 0 == strcmp(text, word) && get_type(instance, candidate, &type) &&
                    1 /* NBEST_MATCH_CANDIDATE */ != type)
                    hit = candidate;
            }
            int next = hit ? choose(instance, offset, hit) : -1;
            ok = next >= 0;
            offset = ok ? (size_t)next : offset;
            guess_sentence(instance);
        }
        printf("train %s: %s\n", *entry, ok && train(instance, 0) ? "true" : "false");
    }

    iter_t *iter = begin(ctx);
    if (!iter) {
        printf("begin: NULL\n");
    } else {
        bool more = has_next(iter);
        printf("has_next0 %s\n", more ? "true" : "false");
        unsigned rows = 0;
        while (more && rows < 10000) {
            char *phrase = NULL, *pinyin = NULL;
            int32_t count = 0;
            more = get_next(iter, &phrase, &pinyin, &count);
            printf("row %s\t%s\t%d\tget=%s\n", phrase ? phrase : "(null)",
                   pinyin ? pinyin : "(null)", count, more ? "true" : "false");
            g_free_fn(phrase);
            g_free_fn(pinyin);
            ++rows;
        }
        printf("rows=%u\n", rows);
        printf("has_next_after_false %s\n", has_next(iter) ? "true" : "false");
        end(iter);
    }
    free_instance(instance);
    fini(ctx);
    return 0;
}
