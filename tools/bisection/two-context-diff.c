/*
 * two-context-diff.c — two contexts on one user dir in one process, for
 * the context-independence differential (#538).
 *
 * The pin keeps no process-wide state for a user dir: every pinyin_init /
 * zhuyin_init reads the profile into its own context (pinyin.cpp:326-444,
 * zhuyin.cpp:270-), learns into its own memory, and every save writes that
 * context's whole state (pinyin.cpp:1132-1147, zhuyin.cpp:547-699). So a
 * second context does not see what the first learned and has not saved;
 * each context's save answers for its own modifications; libpinyin's open
 * counter is raised once per init and lowered once per fini, each context
 * on its own copy (pinyin.cpp:185-187, :1194-1200); and when both contexts
 * save, the later save's files are the profile.
 *
 * Usage:
 *   two-context-diff <pinyin|zhuyin> <lib.so> <systemdir> <userdir> <scenario>
 *
 *   one-learns      init A, init B; A learns item 1; each context's view
 *                   of it; save B, save A; fini A, fini B.
 *   fini-reversed   as one-learns, with fini B before fini A.
 *   both-learn      init A, init B; A learns item 1, B learns item 2;
 *                   save A, save B; fini A, fini B; then a third context
 *                   shows which learning the profile kept.
 *
 * "Learns" is what run-open-counter-diff.sh's driver does: pinyin imports
 * a user phrase and trains a system word chosen below the sentence rows;
 * zhuyin trains a system word. A view is read the same way too: pinyin's
 * user-dictionary rows and the words' unigram frequencies, zhuyin's
 * candidate order. user.conf's counter line is printed after every step.
 *
 * This file is part of oxpinyin, GPL-3.0-or-later like the rest of it.
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include <glib.h>

typedef void context_t;
typedef void instance_t;
typedef void candidate_t;
typedef void iterator_t;
typedef uint32_t phrase_token_t;

/* novel_types.h:161 */
#define USER_DICTIONARY 7
/* pinyin.h:45 NORMAL_CANDIDATE; zhuyin.h:43 NORMAL_CANDIDATE_AFTER_CURSOR */
#define LISTED_CANDIDATE 2
/* ibus's candidate order (PYPConfig.cc:151). */
#define SORT_OPTION (0x4 | 0x8 | 0x10)
#define IMPORT_COUNT 5

struct target {
    const char *typed;
    const char *word;
};

/* Items 1 and 2: the first two rows of open-counter-diff.c's tables,
 * chosen there so that each is listed below the sentence rows on both
 * sides and one training visibly moves it. */
static const struct target PINYIN_TARGETS[] = {{"li'shi", "历时"}, {"yu'yan", "寓言"}};
static const struct target PINYIN_IMPORTS[] = {{"ni'hao", "泥壕"}, {"ba'kua", "罢跨"}};
static const struct target ZHUYIN_TARGETS[] = {{"2u04vu04", "癫痫"}, {"xu4g3", "砾石"}};
#define N_ITEMS 2

static void *lib;
static const char *prefix;
static bool zhuyin;
static const char *system_dir;
static const char *user_dir;

static void *sym(const char *name) {
    char full[96];
    snprintf(full, sizeof full, "%s_%s", prefix, name);
    void *symbol = dlsym(lib, full);
    if (!symbol) {
        fprintf(stderr, "missing symbol: %s\n", full);
        exit(1);
    }
    return symbol;
}

/* user.conf's counter line as the file holds it now. */
static void counter(const char *step) {
    char path[4096];
    snprintf(path, sizeof path, "%s/user.conf", user_dir);
    FILE *f = fopen(path, "r");
    char line[256], found[256] = "absent";
    if (f) {
        strcpy(found, "no counter line");
        while (fgets(line, sizeof line, f))
            if (strncmp(line, "open counter:", 13) == 0) {
                line[strcspn(line, "\n")] = 0;
                snprintf(found, sizeof found, "%s", line + 13);
            }
        fclose(f);
    }
    printf("counter after %s: %s\n", step, found);
}

struct ctx {
    const char *name;
    context_t *context;
    instance_t *instance;
};

static struct ctx init_ctx(const char *name) {
    context_t *(*init)(const char *, const char *) = sym("init");
    instance_t *(*alloc)(context_t *) = sym("alloc_instance");
    struct ctx c = {name, init(system_dir, user_dir), NULL};
    if (!c.context) {
        printf("init %s: NULL\n", name);
        exit(1);
    }
    c.instance = alloc(c.context);
    if (!c.instance) {
        printf("alloc %s: NULL\n", name);
        exit(1);
    }
    printf("init %s: ok\n", name);
    char step[32];
    snprintf(step, sizeof step, "init %s", name);
    counter(step);
    return c;
}

static void fini_ctx(struct ctx *c) {
    void (*free_instance)(instance_t *) = sym("free_instance");
    void (*fini)(context_t *) = sym("fini");
    free_instance(c->instance);
    fini(c->context);
    printf("fini %s\n", c->name);
    char step[32];
    snprintf(step, sizeof step, "fini %s", c->name);
    counter(step);
}

static void save_ctx(struct ctx *c) {
    bool (*save)(context_t *) = sym("save");
    printf("save %s: %d\n", c->name, save(c->context));
    char step[32];
    snprintf(step, sizeof step, "save %s", c->name);
    counter(step);
}

static bool list_candidates(instance_t *inst, const char *typed) {
    size_t (*parse)(instance_t *, const char *) =
        sym(zhuyin ? "parse_more_chewings" : "parse_more_full_pinyins");
    bool (*guess_sentence)(instance_t *) = sym("guess_sentence");
    if (strlen(typed) != parse(inst, typed) || !guess_sentence(inst))
        return false;
    if (zhuyin) {
        bool (*guess_after)(instance_t *, size_t) = sym("guess_candidates_after_cursor");
        return guess_after(inst, 0);
    }
    bool (*guess)(instance_t *, size_t, guint) = sym("guess_candidates");
    return guess(inst, 0, SORT_OPTION);
}

/* What context c sees of items 1..N: pinyin's user-dictionary rows and
 * the targets' unigram frequencies; zhuyin's candidate order for each
 * target's input. */
static void view(struct ctx *c) {
    bool (*n_candidate)(instance_t *, guint *) = sym("get_n_candidate");
    bool (*candidate)(instance_t *, guint, candidate_t **) = sym("get_candidate");
    bool (*candidate_type)(instance_t *, candidate_t *, int *) = sym("get_candidate_type");
    bool (*candidate_string)(instance_t *, candidate_t *, const gchar **) =
        sym("get_candidate_string");
    bool (*reset)(instance_t *) = sym("reset");

    if (!zhuyin) {
        iterator_t *(*begin)(context_t *, guint) = sym("begin_get_phrases");
        bool (*has_next)(iterator_t *) = sym("iterator_has_next_phrase");
        bool (*next)(iterator_t *, gchar **, gchar **, gint *) = sym("iterator_get_next_phrase");
        void (*end)(iterator_t *) = sym("end_get_phrases");
        bool (*lookup)(instance_t *, const char *, GArray *) = sym("lookup_tokens");
        bool (*unigram)(instance_t *, phrase_token_t, guint *) =
            sym("token_get_unigram_frequency");

        iterator_t *iter = begin(c->context, USER_DICTIONARY);
        while (iter && has_next(iter)) {
            gchar *phrase = NULL, *pinyin = NULL;
            gint count = -1;
            if (!next(iter, &phrase, &pinyin, &count))
                break;
            printf("view %s: P %s %s %d\n", c->name, phrase, pinyin, count);
            g_free(phrase);
            g_free(pinyin);
        }
        if (iter)
            end(iter);
        for (size_t i = 0; i < N_ITEMS; ++i) {
            GArray *tokens = g_array_new(FALSE, FALSE, sizeof(phrase_token_t));
            lookup(c->instance, PINYIN_TARGETS[i].word, tokens);
            for (guint t = 0; t < tokens->len; ++t) {
                guint freq = 0;
                unigram(c->instance, g_array_index(tokens, phrase_token_t, t), &freq);
                printf("view %s: T %s %u\n", c->name, PINYIN_TARGETS[i].word, freq);
            }
            g_array_free(tokens, TRUE);
        }
        return;
    }
    for (size_t i = 0; i < N_ITEMS; ++i) {
        guint n = 0;
        if (list_candidates(c->instance, ZHUYIN_TARGETS[i].typed))
            n_candidate(c->instance, &n);
        GString *row = g_string_new(NULL);
        int shown = 0;
        for (guint j = 0; j < n && shown < 5; ++j) {
            candidate_t *cand = NULL;
            int type = 0;
            const gchar *text = NULL;
            if (!candidate(c->instance, j, &cand) || !cand ||
                !candidate_type(c->instance, cand, &type) ||
                !candidate_string(c->instance, cand, &text) || !text)
                continue;
            if (type != LISTED_CANDIDATE || g_utf8_strlen(text, -1) == 2) {
                g_string_append_printf(row, " %s%s", type == LISTED_CANDIDATE ? "" : "*", text);
                shown++;
            }
        }
        printf("view %s: R %s%s\n", c->name, ZHUYIN_TARGETS[i].typed, row->str);
        g_string_free(row, TRUE);
        reset(c->instance);
    }
}

/* Learn item k in context c: pinyin imports a user phrase and trains a
 * target; zhuyin trains a target. As open-counter-diff.c: choose the
 * target below the sentence rows, re-guess, train, reset. */
static void learn(struct ctx *c, size_t k) {
    bool (*guess_sentence)(instance_t *) = sym("guess_sentence");
    bool (*n_candidate)(instance_t *, guint *) = sym("get_n_candidate");
    bool (*candidate)(instance_t *, guint, candidate_t **) = sym("get_candidate");
    bool (*candidate_type)(instance_t *, candidate_t *, int *) = sym("get_candidate_type");
    bool (*candidate_string)(instance_t *, candidate_t *, const gchar **) =
        sym("get_candidate_string");
    int (*choose)(instance_t *, size_t, candidate_t *) = sym("choose_candidate");
    bool (*reset)(instance_t *) = sym("reset");

    if (!zhuyin) {
        iterator_t *(*begin)(context_t *, guint8) = sym("begin_add_phrases");
        bool (*add)(iterator_t *, const char *, const char *, gint) = sym("iterator_add_phrase");
        void (*end)(iterator_t *) = sym("end_add_phrases");
        iterator_t *iter = begin(c->context, USER_DICTIONARY);
        bool ok = iter && add(iter, PINYIN_IMPORTS[k].word, PINYIN_IMPORTS[k].typed, IMPORT_COUNT);
        if (iter)
            end(iter);
        printf("import %s: %s %d\n", c->name, PINYIN_IMPORTS[k].word, ok);
    }

    const struct target *t = zhuyin ? &ZHUYIN_TARGETS[k] : &PINYIN_TARGETS[k];
    const char *result = "ok";
    guint n = 0;
    candidate_t *chosen = NULL;
    if (!list_candidates(c->instance, t->typed))
        result = "no-list";
    else
        n_candidate(c->instance, &n);
    for (guint i = 0; i < n && !chosen; ++i) {
        candidate_t *cand = NULL;
        int type = 0;
        const gchar *text = NULL;
        if (candidate(c->instance, i, &cand) && cand &&
            candidate_type(c->instance, cand, &type) && type == LISTED_CANDIDATE &&
            candidate_string(c->instance, cand, &text) && text && strcmp(text, t->word) == 0)
            chosen = cand;
    }
    if (!chosen) {
        if (strcmp(result, "ok") == 0)
            result = "no-candidate";
    } else if (choose(c->instance, 0, chosen) <= 0)
        result = "choose-failed";
    else if (!guess_sentence(c->instance))
        result = "reguess-failed";
    else if (zhuyin) {
        bool (*train)(instance_t *) = sym("train");
        if (!train(c->instance))
            result = "train-false";
    } else {
        bool (*train)(instance_t *, guint8) = sym("train");
        if (!train(c->instance, 0))
            result = "train-false";
    }
    reset(c->instance);
    printf("train %s: %s %s\n", c->name, t->word, result);
}

int main(int argc, char **argv) {
    if (argc != 6) {
        fprintf(stderr,
                "usage: %s <pinyin|zhuyin> <lib.so> <systemdir> <userdir> "
                "<one-learns|fini-reversed|both-learn>\n",
                argv[0]);
        return 2;
    }
    zhuyin = strcmp(argv[1], "zhuyin") == 0;
    if (!zhuyin && strcmp(argv[1], "pinyin") != 0) {
        fprintf(stderr, "kind must be pinyin or zhuyin: %s\n", argv[1]);
        return 2;
    }
    const char *scenario = argv[5];
    prefix = argv[1];
    system_dir = argv[3];
    user_dir = argv[4];
    lib = dlopen(argv[2], RTLD_NOW | RTLD_LOCAL);
    if (!lib) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    printf("scenario: %s\n", scenario);

    struct ctx a = init_ctx("A");
    struct ctx b = init_ctx("B");
    if (strcmp(scenario, "one-learns") == 0 || strcmp(scenario, "fini-reversed") == 0) {
        learn(&a, 0);
        view(&a);
        view(&b);
        save_ctx(&b);
        save_ctx(&a);
        if (strcmp(scenario, "one-learns") == 0) {
            fini_ctx(&a);
            fini_ctx(&b);
        } else {
            fini_ctx(&b);
            fini_ctx(&a);
        }
    } else if (strcmp(scenario, "both-learn") == 0) {
        learn(&a, 0);
        learn(&b, 1);
        view(&a);
        view(&b);
        save_ctx(&a);
        save_ctx(&b);
        fini_ctx(&a);
        fini_ctx(&b);
        struct ctx c = init_ctx("C");
        view(&c);
        fini_ctx(&c);
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }
    return 0;
}
