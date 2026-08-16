/*
 * legacy-seed.c — W7-T2 legacy-dir seed driver.
 *
 * Builds a *persistent* libpinyin user directory (user.bin phrase index +
 * user_bigram.db bigram database) by driving the pinned libpinyin.so through
 * the exact scripted training + remember sequence that `train-diff.c` uses
 * for the W6-T7 user-store differential. The difference: train-diff exports
 * the triples in-process and discards the dir; this driver *saves* the dir
 * to disk so `pinyin-migrate migrate --legacy-dir <dir>` has a real legacy
 * store to migrate.
 *
 * Sequence (identical to train-diff.c, user-store.md §2/§3/§9):
 *   rounds 1..n — parse "nihao", guess sentence, choose 你 (offset 0),
 *                 re-guess, choose 好, re-guess sentence, train. The
 *                 doubling seeds 69, 138, 414, … on (你 → 好).
 *   remember    — "你好" (-1) after "nihao", then "世界" (-1) and "世界" (7)
 *                 after "shijie": the index-only path and the pronunciation
 *                 merge (5 + 7 = 12 on 世界).
 *   save        — pinyin_save() flushes both files before fini.
 *
 * Usage:
 *   legacy-seed <path-to-so> <systemdir> <userdir> [rounds]
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

typedef void pinyin_context_t;
typedef void pinyin_instance_t;
typedef void lookup_candidate_t;

typedef uint32_t guint;
typedef int32_t gint;
typedef char gchar;

#define DEFAULT_SORT ((guint)0x1e)

typedef pinyin_context_t *(*fn_init)(const char *, const char *);
typedef void (*fn_fini)(pinyin_context_t *);
typedef pinyin_instance_t *(*fn_alloc)(pinyin_context_t *);
typedef void (*fn_free_instance)(pinyin_instance_t *);
typedef size_t (*fn_parse)(pinyin_instance_t *, const char *);
typedef bool (*fn_sentence)(pinyin_instance_t *);
typedef bool (*fn_guess)(pinyin_instance_t *, size_t, guint);
typedef bool (*fn_getn)(pinyin_instance_t *, guint *);
typedef bool (*fn_getc)(pinyin_instance_t *, guint, lookup_candidate_t **);
typedef bool (*fn_getstr)(pinyin_instance_t *, lookup_candidate_t *, const gchar **);
typedef int (*fn_choose)(pinyin_instance_t *, size_t, lookup_candidate_t *);
typedef bool (*fn_train)(pinyin_instance_t *, uint8_t);
typedef bool (*fn_reset)(pinyin_instance_t *);
typedef bool (*fn_remember)(pinyin_instance_t *, const char *, gint);
typedef bool (*fn_save)(pinyin_context_t *);

struct syms {
    fn_init init;
    fn_fini fini;
    fn_alloc alloc;
    fn_free_instance free_instance;
    fn_parse parse;
    fn_sentence sentence;
    fn_guess guess;
    fn_getn getn;
    fn_getc getc;
    fn_getstr getstr;
    fn_choose choose;
    fn_train train;
    fn_reset reset;
    fn_remember remember;
    fn_save save;
};

static void *load(const char *name, void *handle) {
    void *symbol = dlsym(handle, name);
    if (!symbol) {
        fprintf(stderr, "  MISSING: %s\n", name);
        exit(1);
    }
    return symbol;
}

static void resolve_all(void *handle, struct syms *s) {
    s->init = (fn_init)load("pinyin_init", handle);
    s->fini = (fn_fini)load("pinyin_fini", handle);
    s->alloc = (fn_alloc)load("pinyin_alloc_instance", handle);
    s->free_instance = (fn_free_instance)load("pinyin_free_instance", handle);
    s->parse = (fn_parse)load("pinyin_parse_more_full_pinyins", handle);
    s->sentence = (fn_sentence)load("pinyin_guess_sentence", handle);
    s->guess = (fn_guess)load("pinyin_guess_candidates", handle);
    s->getn = (fn_getn)load("pinyin_get_n_candidate", handle);
    s->getc = (fn_getc)load("pinyin_get_candidate", handle);
    s->getstr = (fn_getstr)load("pinyin_get_candidate_string", handle);
    s->choose = (fn_choose)load("pinyin_choose_candidate", handle);
    s->train = (fn_train)load("pinyin_train", handle);
    s->reset = (fn_reset)load("pinyin_reset", handle);
    s->remember = (fn_remember)load("pinyin_remember_user_input", handle);
    s->save = (fn_save)load("pinyin_save", handle);
}

static lookup_candidate_t *find_by_text(const struct syms *s,
                                        pinyin_instance_t *inst,
                                        const char *want) {
    guint count = 0;
    s->getn(inst, &count);
    for (guint i = 0; i < count; i++) {
        lookup_candidate_t *cand = NULL;
        if (!s->getc(inst, i, &cand) || !cand)
            continue;
        const gchar *text = NULL;
        s->getstr(inst, cand, &text);
        if (text && strcmp(text, want) == 0)
            return cand;
    }
    return NULL;
}

int main(int argc, char **argv) {
    if (argc < 4) {
        fprintf(stderr, "Usage: %s <path-to-so> <systemdir> <userdir> [rounds]\n",
                argv[0]);
        return 1;
    }

    int rounds = 2;
    if (argc >= 5)
        rounds = atoi(argv[4]);
    if (rounds < 1 || rounds > 64) {
        fprintf(stderr, "rounds must be in 1..64\n");
        return 1;
    }

    void *handle = dlopen(argv[1], RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    struct syms s;
    memset(&s, 0, sizeof(s));
    resolve_all(handle, &s);

    pinyin_context_t *ctx = s.init(argv[2], argv[3]);
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed\n");
        return 1;
    }
    pinyin_instance_t *inst = s.alloc(ctx);
    if (!inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        return 1;
    }

    /* Phase 1 — the doubling sequence on (你 → 好). */
    for (int round = 1; round <= rounds; round++) {
        s.parse(inst, "nihao");
        s.sentence(inst);
        s.guess(inst, 0, DEFAULT_SORT);

        lookup_candidate_t *ni = find_by_text(&s, inst, "你");
        if (!ni) {
            fprintf(stderr, "round %d: candidate 你 not offered\n", round);
            return 1;
        }
        int offset = s.choose(inst, 0, ni);
        if (offset < 0) {
            fprintf(stderr, "round %d: choose 你 failed\n", round);
            return 1;
        }
        s.sentence(inst);
        s.guess(inst, (size_t)offset, DEFAULT_SORT);

        lookup_candidate_t *hao = find_by_text(&s, inst, "好");
        if (!hao) {
            fprintf(stderr, "round %d: candidate 好 not offered\n", round);
            return 1;
        }
        if (s.choose(inst, (size_t)offset, hao) < 0) {
            fprintf(stderr, "round %d: choose 好 failed\n", round);
            return 1;
        }
        s.sentence(inst);

        if (!s.train(inst, 0)) {
            fprintf(stderr, "round %d: pinyin_train failed\n", round);
            return 1;
        }
        s.reset(inst);
    }

    /* Phase 2 — remember_user_input: index-only path + pronunciation merge. */
    s.parse(inst, "nihao");
    if (!s.remember(inst, "你好", -1)) {
        fprintf(stderr, "remember 你好 failed\n");
        return 1;
    }
    s.parse(inst, "shijie");
    if (!s.remember(inst, "世界", -1)) {
        fprintf(stderr, "remember 世界 failed\n");
        return 1;
    }
    if (!s.remember(inst, "世界", 7)) {
        fprintf(stderr, "remember 世界 7 failed\n");
        return 1;
    }

    if (!s.save(ctx)) {
        fprintf(stderr, "pinyin_save failed\n");
        return 1;
    }

    s.free_instance(inst);
    s.fini(ctx);
    dlclose(handle);
    return 0;
}
