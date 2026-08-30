/*
 * phrase-surface-diff.c — Tier-B ABI differential: the phrase-result
 * surface (`pinyin_phrase_segment` + `pinyin_get_n_phrase` +
 * `pinyin_get_phrase_token`) and the prefix-seeded sentence guess
 * (`pinyin_guess_sentence_with_prefix`).
 *
 * None of these five symbols has a consumer call site in either
 * frontend (ibus-libpinyin at 2d2cdac0, fcitx-libpinyin), so this
 * driver is the only oracle coverage they get: every probe logs the
 * retval plus the observable state — the token array's full shape
 * (token@position, including the null fillers and the failed-match
 * all-null character-length array), and for the prefix guess the
 * retval plus row-0 sentence text at the proved index.
 *
 * Usage: ./phrase-surface-diff <path-to-so> <systemdir>
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
typedef char gchar;

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_inst)(pinyin_instance_t *);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_segment)(pinyin_instance_t *, const char *);
typedef bool (*fn_n_phrase)(pinyin_instance_t *, unsigned int *);
typedef bool (*fn_phrase_token)(pinyin_instance_t *, unsigned int, uint32_t *);
typedef bool (*fn_guess_prefix)(pinyin_instance_t *, const char *);
typedef bool (*fn_sentence)(pinyin_instance_t *, unsigned int, char **);
typedef bool (*fn_reset)(pinyin_instance_t *);

static void *must(void *handle, const char *name) {
    void *s = dlsym(handle, name);
    if (!s) {
        fprintf(stderr, "MISSING %s\n", name);
        exit(1);
    }
    return s;
}

/* Segments one sentence and dumps the full observable: retval, the
 * result length, and every token by position — the failed-match shape
 * (false, char-length, all-null) is exactly what this makes visible. */
static void probe_segment(fn_segment segment, fn_n_phrase n_phrase,
                          fn_phrase_token phrase_token,
                          pinyin_instance_t *inst, const char *sentence) {
    bool ok = segment(inst, sentence);
    unsigned int n = 0;
    bool ngot = n_phrase(inst, &n);
    printf("segment|%s|%d|n=%u|nok=%d|", sentence, ok ? 1 : 0, n, ngot ? 1 : 0);
    for (unsigned int i = 0; i < n; ++i) {
        uint32_t token = 0xFFFFFFFF;
        bool got = phrase_token(inst, i, &token);
        printf("%u:%u:%d ", i, token, got ? 1 : 0);
    }
    printf("\n");
}

static void probe_prefix_guess(fn_parse parse, fn_guess_prefix guess_prefix,
                               fn_sentence sentence, pinyin_instance_t *inst,
                               const char *input, const char *prefix) {
    parse(inst, input);
    bool ok = guess_prefix(inst, prefix);
    printf("prefix|%s|%s|%d|", input, prefix, ok ? 1 : 0);
    if (ok) {
        char *text = NULL;
        /* Row 0 only — a proved index; the pin aborts on past-the-rows
         * asks and that landmine is not this differential's subject.
         * Capture the getter's own retval so a diverging status is
         * visible even when the text happens to match. */
        bool ok_sentence = sentence(inst, 0, &text);
        printf("sok=%d|%s\n", ok_sentence ? 1 : 0, text ? text : "-");
    } else {
        printf("sok=-|-\n");
    }
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
    fn_parse parse = (fn_parse)must(h, "pinyin_parse_more_full_pinyins");
    fn_segment segment = (fn_segment)must(h, "pinyin_phrase_segment");
    fn_n_phrase n_phrase = (fn_n_phrase)must(h, "pinyin_get_n_phrase");
    fn_phrase_token phrase_token =
        (fn_phrase_token)must(h, "pinyin_get_phrase_token");
    fn_guess_prefix guess_prefix =
        (fn_guess_prefix)must(h, "pinyin_guess_sentence_with_prefix");
    fn_sentence get_sentence = (fn_sentence)must(h, "pinyin_get_sentence");
    fn_reset reset = (fn_reset)must(h, "pinyin_reset");

    pinyin_context_t *ctx = init(systemdir, "");
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed\n");
        return 1;
    }
    pinyin_instance_t *inst = alloc(ctx);
    if (!inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        return 1;
    }

    /* Segment probes over the real tables: stored phrases, adjacent
     * phrase pairs, unknown chars, punctuation, ASCII, empty. */
    const char *sentences[] = {
        "你好", "中国", "你好中国", "中国人", "今天", "明天",
        "你好，世界。", "我们", "", "abcd",
    };
    for (unsigned i = 0; i < sizeof(sentences) / sizeof(sentences[0]); ++i) {
        probe_segment(segment, n_phrase, phrase_token, inst, sentences[i]);
    }
    /* Invalid UTF-8: the pin's g_return_val_if_fail gate answers false
     * (a graceful refusal, safe to drive on both engines). */
    {
        const char bad[] = {(char)0xFF, (char)0xFE, 0x00};
        probe_segment(segment, n_phrase, phrase_token, inst, bad);
    }

    /* Fresh-instance reads before any segment: n=0, token false. */
    {
        unsigned int n = 7;
        bool ngot = n_phrase(inst, &n);
        printf("fresh|n=%u|nok=%d\n", n, ngot ? 1 : 0);
    }

    /* reset clears the result (pinyin.cpp:2699). Capture reset's own
     * retval so a diverging status shows up in the differential even
     * when the post-reset count agrees. */
    segment(inst, "你好中国");
    bool ok_reset = reset(inst);
    unsigned int n_after_reset = 0;
    bool ngot_after_reset = n_phrase(inst, &n_after_reset);
    printf("reset|rok=%d|n=%u|nok=%d\n", ok_reset ? 1 : 0,
           n_after_reset, ngot_after_reset ? 1 : 0);

    /* Prefix-seeded sentence guesses over a parsed composition. */
    const char *prefixes[] = {"你好", "中国", "你好中国", "不存在",
                              ""};
    for (unsigned i = 0; i < sizeof(prefixes) / sizeof(prefixes[0]); ++i) {
        probe_prefix_guess(parse, guess_prefix, get_sentence, inst,
                           "nihaoshijie", prefixes[i]);
    }
    /* The plain parse state with no prefix at all. */
    probe_prefix_guess(parse, guess_prefix, get_sentence, inst, "nihao", "");

    free_inst(inst);
    fini(ctx);
    return 0;
}
