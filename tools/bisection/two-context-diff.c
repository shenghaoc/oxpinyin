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
 * The two contexts may also be one of each library (#578's review): each
 * keeps its own library's user.conf law whichever opened first. libpinyin
 * raises the counter at init, writes it back at save and lowers it at fini;
 * libzhuyin's init only reads it, its save writes a fresh 0 and its fini
 * writes nothing (zhuyin.cpp:126-162, :164-176, :741-757).
 *
 * Usage:
 *   two-context-diff <pinyin|zhuyin> <lib.so> <systemdir> <userdir> <scenario>
 *   two-context-diff cross <libpinyin.so> <libzhuyin.so> <systemdir> <userdir>
 *                    <scenario>
 *
 *   one-learns      init A, init B; A learns item 1; each context's view
 *                   of it; save B, save A; fini A, fini B.
 *   fini-reversed   as one-learns, with fini B before fini A.
 *   both-learn      init A, init B; A learns item 1, B learns item 2;
 *                   save A, save B; fini A, fini B; then a third context
 *                   shows which learning the profile kept.
 *   late-save       as both-learn, but fini A before B saves.
 *
 *   cross, one libzhuyin context Z and one libpinyin context P:
 *   zhuyin-first    init Z, init P; each learns its library's item 1 and
 *                   shows its view; save Z, save P; fini Z, fini P; then a
 *                   fresh P2 and a fresh Z2 show what the profile kept.
 *   pinyin-first    the same with P opened, learning, saving and finishing
 *                   first.
 *   zhuyin-first-fini-reversed, pinyin-first-fini-reversed
 *                   the same with the two finis in the reverse order.
 *   zhuyin-first-late-save, pinyin-first-late-save
 *                   the first context finishes before the second saves.
 *
 * "Learns" is what run-open-counter-diff.sh's driver does: pinyin imports
 * a user phrase and trains a system word chosen below the sentence rows;
 * zhuyin trains a system word. A view is read the same way too: pinyin's
 * user-dictionary rows and the words' unigram frequencies, zhuyin's
 * candidate order. In the cross scenarios a libpinyin view also reads the
 * frequencies of the words the libzhuyin context trains. user.conf's
 * counter line is printed after every step.
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

/* One library: its symbols' prefix and its handle. */
struct facade {
    const char *prefix;
    void *lib;
    bool zhuyin;
};

static struct facade pinyin_facade = {"pinyin", NULL, false};
static struct facade zhuyin_facade = {"zhuyin", NULL, true};
static bool cross;
static const char *system_dir;
static const char *user_dir;

static void *sym(const struct facade *f, const char *name) {
    char full[96];
    snprintf(full, sizeof full, "%s_%s", f->prefix, name);
    void *symbol = dlsym(f->lib, full);
    if (!symbol) {
        fprintf(stderr, "missing symbol: %s\n", full);
        exit(1);
    }
    return symbol;
}

static bool open_lib(struct facade *f, const char *path) {
    f->lib = dlopen(path, RTLD_NOW | RTLD_LOCAL);
    if (!f->lib)
        fprintf(stderr, "dlopen %s: %s\n", path, dlerror());
    return f->lib != NULL;
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
    const struct facade *f;
    context_t *context;
    instance_t *instance;
};

static struct ctx init_ctx(const char *name, const struct facade *f) {
    context_t *(*init)(const char *, const char *) = sym(f, "init");
    instance_t *(*alloc)(context_t *) = sym(f, "alloc_instance");
    struct ctx c = {name, f, init(system_dir, user_dir), NULL};
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
    printf("alloc %s: ok\n", name);
    char step[32];
    snprintf(step, sizeof step, "init %s", name);
    counter(step);
    return c;
}

static void fini_ctx(struct ctx *c) {
    void (*free_instance)(instance_t *) = sym(c->f, "free_instance");
    void (*fini)(context_t *) = sym(c->f, "fini");
    free_instance(c->instance);
    fini(c->context);
    printf("fini %s: ok\n", c->name);
    char step[32];
    snprintf(step, sizeof step, "fini %s", c->name);
    counter(step);
}

static void save_ctx(struct ctx *c, bool expect_written) {
    bool (*save)(context_t *) = sym(c->f, "save");
    bool written = save(c->context);
    printf("save %s: %s\n", c->name, written ? "ok" : "unchanged");
    if (written != expect_written)
        exit(1);
    char step[32];
    snprintf(step, sizeof step, "save %s", c->name);
    counter(step);
}

static bool list_candidates(struct ctx *c, const char *typed) {
    size_t (*parse)(instance_t *, const char *) =
        sym(c->f, c->f->zhuyin ? "parse_more_chewings" : "parse_more_full_pinyins");
    bool (*guess_sentence)(instance_t *) = sym(c->f, "guess_sentence");
    if (strlen(typed) != parse(c->instance, typed) || !guess_sentence(c->instance))
        return false;
    if (c->f->zhuyin) {
        bool (*guess_after)(instance_t *, size_t) = sym(c->f, "guess_candidates_after_cursor");
        return guess_after(c->instance, 0);
    }
    bool (*guess)(instance_t *, size_t, guint) = sym(c->f, "guess_candidates");
    return guess(c->instance, 0, SORT_OPTION);
}

/* A libpinyin context's unigram frequency for each token of `word`. */
static void unigram_view(struct ctx *c, const char *word) {
    bool (*lookup)(instance_t *, const char *, GArray *) = sym(c->f, "lookup_tokens");
    bool (*unigram)(instance_t *, phrase_token_t, guint *) =
        sym(c->f, "token_get_unigram_frequency");
    GArray *tokens = g_array_new(FALSE, FALSE, sizeof(phrase_token_t));
    lookup(c->instance, word, tokens);
    for (guint t = 0; t < tokens->len; ++t) {
        guint freq = 0;
        unigram(c->instance, g_array_index(tokens, phrase_token_t, t), &freq);
        printf("view %s: T %s %u\n", c->name, word, freq);
    }
    g_array_free(tokens, TRUE);
}

/* What context c sees of items 1..N: pinyin's user-dictionary rows and
 * the targets' unigram frequencies; zhuyin's candidate order for each
 * target's input. */
static void view(struct ctx *c) {
    bool (*n_candidate)(instance_t *, guint *) = sym(c->f, "get_n_candidate");
    bool (*candidate)(instance_t *, guint, candidate_t **) = sym(c->f, "get_candidate");
    bool (*candidate_type)(instance_t *, candidate_t *, int *) = sym(c->f, "get_candidate_type");
    bool (*candidate_string)(instance_t *, candidate_t *, const gchar **) =
        sym(c->f, "get_candidate_string");
    bool (*reset)(instance_t *) = sym(c->f, "reset");

    if (!c->f->zhuyin) {
        iterator_t *(*begin)(context_t *, guint) = sym(c->f, "begin_get_phrases");
        bool (*has_next)(iterator_t *) = sym(c->f, "iterator_has_next_phrase");
        bool (*next)(iterator_t *, gchar **, gchar **, gint *) =
            sym(c->f, "iterator_get_next_phrase");
        void (*end)(iterator_t *) = sym(c->f, "end_get_phrases");

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
        for (size_t i = 0; i < N_ITEMS; ++i)
            unigram_view(c, PINYIN_TARGETS[i].word);
        /* The libzhuyin context's words, as this context reads them. */
        if (cross)
            for (size_t i = 0; i < N_ITEMS; ++i)
                unigram_view(c, ZHUYIN_TARGETS[i].word);
        return;
    }
    for (size_t i = 0; i < N_ITEMS; ++i) {
        guint n = 0;
        if (list_candidates(c, ZHUYIN_TARGETS[i].typed))
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
    const struct facade *f = c->f;
    bool (*guess_sentence)(instance_t *) = sym(f, "guess_sentence");
    bool (*n_candidate)(instance_t *, guint *) = sym(f, "get_n_candidate");
    bool (*candidate)(instance_t *, guint, candidate_t **) = sym(f, "get_candidate");
    bool (*candidate_type)(instance_t *, candidate_t *, int *) = sym(f, "get_candidate_type");
    bool (*candidate_string)(instance_t *, candidate_t *, const gchar **) =
        sym(f, "get_candidate_string");
    int (*choose)(instance_t *, size_t, candidate_t *) = sym(f, "choose_candidate");
    bool (*reset)(instance_t *) = sym(f, "reset");

    if (!f->zhuyin) {
        iterator_t *(*begin)(context_t *, guint8) = sym(f, "begin_add_phrases");
        bool (*add)(iterator_t *, const char *, const char *, gint) =
            sym(f, "iterator_add_phrase");
        void (*end)(iterator_t *) = sym(f, "end_add_phrases");
        iterator_t *iter = begin(c->context, USER_DICTIONARY);
        bool ok = iter && add(iter, PINYIN_IMPORTS[k].word, PINYIN_IMPORTS[k].typed, IMPORT_COUNT);
        if (iter)
            end(iter);
        printf("import %s: %s %s\n", c->name, PINYIN_IMPORTS[k].word,
               ok ? "ok" : "failed");
        if (!ok)
            exit(1);
    }

    const struct target *t = f->zhuyin ? &ZHUYIN_TARGETS[k] : &PINYIN_TARGETS[k];
    const char *result = "ok";
    guint n = 0;
    candidate_t *chosen = NULL;
    if (!list_candidates(c, t->typed))
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
    else if (f->zhuyin) {
        bool (*train)(instance_t *) = sym(f, "train");
        if (!train(c->instance))
            result = "train-false";
    } else {
        bool (*train)(instance_t *, guint8) = sym(f, "train");
        if (!train(c->instance, 0))
            result = "train-false";
    }
    if (!reset(c->instance) && strcmp(result, "ok") == 0)
        result = "reset-failed";
    printf("train %s: %s %s\n", c->name, t->word, result);
    if (strcmp(result, "ok") != 0)
        exit(1);
}

/* Two contexts of one library. */
static int run_same(const struct facade *f, const char *scenario) {
    struct ctx a = init_ctx("A", f);
    struct ctx b = init_ctx("B", f);
    if (strcmp(scenario, "one-learns") == 0 || strcmp(scenario, "fini-reversed") == 0) {
        learn(&a, 0);
        view(&a);
        view(&b);
        save_ctx(&b, false);
        save_ctx(&a, true);
        if (strcmp(scenario, "one-learns") == 0) {
            fini_ctx(&a);
            fini_ctx(&b);
        } else {
            fini_ctx(&b);
            fini_ctx(&a);
        }
    } else if (strcmp(scenario, "both-learn") == 0 || strcmp(scenario, "late-save") == 0) {
        bool late_save = strcmp(scenario, "late-save") == 0;
        learn(&a, 0);
        learn(&b, 1);
        view(&a);
        view(&b);
        save_ctx(&a, true);
        if (late_save)
            fini_ctx(&a);
        save_ctx(&b, true);
        if (!late_save)
            fini_ctx(&a);
        fini_ctx(&b);
        struct ctx c = init_ctx("C", f);
        view(&c);
        fini_ctx(&c);
    } else {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }
    return 0;
}

/* One context of each library: the first opens, learns and saves first
 * and, unless reversed, finishes first. */
static int run_cross(const char *scenario) {
    static const struct {
        const char *name;
        bool zhuyin_first;
        bool fini_reversed;
        bool late_save;
    } SCENARIOS[] = {
        {"zhuyin-first", true, false, false},
        {"zhuyin-first-fini-reversed", true, true, false},
        {"pinyin-first", false, false, false},
        {"pinyin-first-fini-reversed", false, true, false},
        {"zhuyin-first-late-save", true, false, true},
        {"pinyin-first-late-save", false, false, true},
    };
    size_t s = 0;
    while (s < sizeof SCENARIOS / sizeof *SCENARIOS && strcmp(SCENARIOS[s].name, scenario) != 0)
        ++s;
    if (s == sizeof SCENARIOS / sizeof *SCENARIOS) {
        fprintf(stderr, "unknown scenario: %s\n", scenario);
        return 2;
    }
    const struct facade *f1 = SCENARIOS[s].zhuyin_first ? &zhuyin_facade : &pinyin_facade;
    const struct facade *f2 = SCENARIOS[s].zhuyin_first ? &pinyin_facade : &zhuyin_facade;
    struct ctx first = init_ctx(f1->zhuyin ? "Z" : "P", f1);
    struct ctx second = init_ctx(f2->zhuyin ? "Z" : "P", f2);
    learn(&first, 0);
    learn(&second, 0);
    view(&first);
    view(&second);
    save_ctx(&first, true);
    if (SCENARIOS[s].late_save)
        fini_ctx(&first);
    save_ctx(&second, true);
    if (SCENARIOS[s].late_save) {
        fini_ctx(&second);
    } else if (SCENARIOS[s].fini_reversed) {
        fini_ctx(&second);
        fini_ctx(&first);
    } else {
        fini_ctx(&first);
        fini_ctx(&second);
    }
    /* What the profile kept, as a fresh context of each library reads it. */
    struct ctx p = init_ctx("P2", &pinyin_facade);
    view(&p);
    fini_ctx(&p);
    struct ctx z = init_ctx("Z2", &zhuyin_facade);
    view(&z);
    fini_ctx(&z);
    return 0;
}

int main(int argc, char **argv) {
    cross = argc > 1 && strcmp(argv[1], "cross") == 0;
    if (argc != (cross ? 7 : 6)) {
        fprintf(stderr,
                "usage: %s <pinyin|zhuyin> <lib.so> <systemdir> <userdir> "
                "<one-learns|fini-reversed|both-learn|late-save>\n"
                "       %s cross <libpinyin.so> <libzhuyin.so> <systemdir> <userdir> "
                "<zhuyin-first|pinyin-first>[-fini-reversed|-late-save]\n",
                argv[0], argv[0]);
        return 2;
    }
    struct facade *only = NULL;
    if (!cross) {
        if (strcmp(argv[1], "pinyin") == 0)
            only = &pinyin_facade;
        else if (strcmp(argv[1], "zhuyin") == 0)
            only = &zhuyin_facade;
        else {
            fprintf(stderr, "kind must be pinyin, zhuyin or cross: %s\n", argv[1]);
            return 2;
        }
    }
    /* argv index of <systemdir>: after one library path, or after two. */
    int at = cross ? 4 : 3;
    system_dir = argv[at];
    user_dir = argv[at + 1];
    const char *scenario = argv[at + 2];
    if (cross ? !open_lib(&pinyin_facade, argv[2]) || !open_lib(&zhuyin_facade, argv[3])
              : !open_lib(only, argv[2]))
        return 1;
    printf("scenario: %s\n", scenario);
    return cross ? run_cross(scenario) : run_same(only, scenario);
}
