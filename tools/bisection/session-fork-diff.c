/*
 * session-fork-diff.c — one libpinyin context across a fork, for the
 * session-store differential (#531, #546).
 *
 * The pin holds a context's working state in process memory
 * (pinyin.cpp:326-444 at 074a2219 loads the profile into the context; the
 * user bigram is an in-memory container, ngram_bdb.cpp:47-77) and opens no
 * temp file and reads no temp variable anywhere under src. So a forked
 * child that learns and saves touches only the user dir's files, and the
 * parent's context is untouched by it; a process that dies before
 * pinyin_fini leaves nothing outside the user dir. oxpinyin's session
 * store must behave the same: a file-backed session shared across the
 * fork was truncated by the child's save, after which the parent's
 * import and save answered false on Kyoto Cabinet (and the audited build
 * spun, #531), and it read TMPDIR and left a scratch behind a SIGKILL
 * (#546).
 *
 * Usage:
 *   session-fork-diff <lib.so> <systemdir> <userdir> <mode>
 *
 *   fork-_exit   init -> fork; child: learn, save, _exit(0); parent: wait,
 *                learn, save, learned state, fini.
 *   fork-exit    the same, the child leaving through exit(0) — atexit
 *                handlers and static destructors run in the child too.
 *   fork-nosave  the child learns but does not save, and _exit(0)s: the
 *                parent's profile must not carry the child's learning.
 *   crash        init -> learned state -> SIGKILL before fini: whatever
 *                either library created outside the user dir is what the
 *                runner then finds in TMPDIR.
 *
 * Every step prints a line; the runner diffs the two libraries' logs
 * byte for byte, together with the user dir's inventory and TMPDIR's
 * entry count after the process is gone. The learned state is read
 * in-process, as open-counter-diff.c reads it: the training targets'
 * tokens with their unigram frequencies, and the user dictionary
 * through begin_get_phrases.
 *
 * This file is part of oxpinyin, GPL-3.0-or-later like the rest of it.
 */

#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

#include <glib.h>

typedef void context_t;
typedef void instance_t;
typedef void candidate_t;
typedef void iterator_t;
typedef uint32_t phrase_token_t;

/* novel_types.h:161 */
#define USER_DICTIONARY 7
/* pinyin.h:45 NORMAL_CANDIDATE */
#define LISTED_CANDIDATE 2
/* ibus's candidate order (PYPConfig.cc:151). */
#define SORT_OPTION (0x4 | 0x8 | 0x10)
#define IMPORT_COUNT 5

struct target {
    const char *typed;
    const char *word;
};

/* The child's and the parent's learning: a system word each, listed
 * below the sentence rows on both sides for this table, and a user
 * phrase each that names no system phrase. */
static const struct target CHILD_TARGET = {"li'shi", "历时"};
static const struct target PARENT_TARGET = {"yu'yan", "寓言"};
static const struct target CHILD_IMPORT = {"ni'hao", "泥壕"};
static const struct target PARENT_IMPORT = {"ba'kua", "罢跨"};

static void *lib;

static void *sym(const char *name) {
    void *symbol = dlsym(lib, name);
    if (!symbol) {
        fprintf(stderr, "missing symbol: %s\n", name);
        exit(1);
    }
    return symbol;
}

static bool list_candidates(instance_t *inst, const char *typed) {
    size_t (*parse)(instance_t *, const char *) = sym("pinyin_parse_more_full_pinyins");
    bool (*guess_sentence)(instance_t *) = sym("pinyin_guess_sentence");
    bool (*guess)(instance_t *, size_t, guint) = sym("pinyin_guess_candidates");

    if (strlen(typed) != parse(inst, typed) || !guess_sentence(inst))
        return false;
    return guess(inst, 0, SORT_OPTION);
}

/* open-counter-diff.c's train_target, pinyin only: choose the target at
 * offset 0, re-guess, train n-best row 0, reset. */
static const char *train_target(instance_t *inst, const struct target *t) {
    bool (*guess_sentence)(instance_t *) = sym("pinyin_guess_sentence");
    bool (*n_candidate)(instance_t *, guint *) = sym("pinyin_get_n_candidate");
    bool (*candidate)(instance_t *, guint, candidate_t **) = sym("pinyin_get_candidate");
    bool (*candidate_type)(instance_t *, candidate_t *, int *) = sym("pinyin_get_candidate_type");
    bool (*candidate_string)(instance_t *, candidate_t *, const gchar **) =
        sym("pinyin_get_candidate_string");
    int (*choose)(instance_t *, size_t, candidate_t *) = sym("pinyin_choose_candidate");
    bool (*train)(instance_t *, guint8) = sym("pinyin_train");
    bool (*reset)(instance_t *) = sym("pinyin_reset");

    const char *result = "ok";
    guint n = 0;
    candidate_t *chosen = NULL;
    if (!list_candidates(inst, t->typed))
        result = "no-list";
    else
        n_candidate(inst, &n);
    for (guint i = 0; i < n && !chosen; ++i) {
        candidate_t *c = NULL;
        int type = 0;
        const gchar *text = NULL;
        if (candidate(inst, i, &c) && c && candidate_type(inst, c, &type) &&
            type == LISTED_CANDIDATE && candidate_string(inst, c, &text) && text &&
            strcmp(text, t->word) == 0)
            chosen = c;
    }
    if (!chosen) {
        if (strcmp(result, "ok") == 0)
            result = "no-candidate";
    } else if (choose(inst, 0, chosen) <= 0)
        result = "choose-failed";
    else if (!guess_sentence(inst))
        result = "reguess-failed";
    else if (!train(inst, 0))
        result = "train-false";
    reset(inst);
    return result;
}

static bool import_phrase(context_t *ctx, const struct target *p) {
    iterator_t *(*begin)(context_t *, guint8) = sym("pinyin_begin_add_phrases");
    bool (*add)(iterator_t *, const char *, const char *, gint) = sym("pinyin_iterator_add_phrase");
    void (*end)(iterator_t *) = sym("pinyin_end_add_phrases");

    iterator_t *iter = begin(ctx, USER_DICTIONARY);
    if (!iter)
        return false;
    bool ok = add(iter, p->word, p->typed, IMPORT_COUNT);
    end(iter);
    return ok;
}

static void target_rows(instance_t *inst, const char *when) {
    bool (*lookup)(instance_t *, const char *, GArray *) = sym("pinyin_lookup_tokens");
    bool (*unigram)(instance_t *, phrase_token_t, guint *) =
        sym("pinyin_token_get_unigram_frequency");
    const struct target *targets[] = {&CHILD_TARGET, &PARENT_TARGET};

    for (size_t i = 0; i < 2; ++i) {
        GArray *tokens = g_array_new(FALSE, FALSE, sizeof(phrase_token_t));
        lookup(inst, targets[i]->word, tokens);
        for (guint t = 0; t < tokens->len; ++t) {
            phrase_token_t token = g_array_index(tokens, phrase_token_t, t);
            guint freq = 0;
            bool known = unigram(inst, token, &freq);
            printf("row@%s\tT\t%s\t%#010x\t%s%u\n", when, targets[i]->word, token,
                   known ? "" : "unknown:", freq);
        }
        g_array_free(tokens, TRUE);
    }
}

static void phrase_rows(context_t *ctx, const char *when) {
    iterator_t *(*begin)(context_t *, guint) = sym("pinyin_begin_get_phrases");
    bool (*has_next)(iterator_t *) = sym("pinyin_iterator_has_next_phrase");
    bool (*next)(iterator_t *, gchar **, gchar **, gint *) = sym("pinyin_iterator_get_next_phrase");
    void (*end)(iterator_t *) = sym("pinyin_end_get_phrases");

    iterator_t *iter = begin(ctx, USER_DICTIONARY);
    int n = 0;
    GString *rows = g_string_new(NULL);
    while (iter && has_next(iter)) {
        gchar *phrase = NULL, *pinyin = NULL;
        gint count = -1;
        if (!next(iter, &phrase, &pinyin, &count))
            break;
        g_string_append_printf(rows, "row@%s\tP\t%s\t%s\t%d\n", when, phrase, pinyin, count);
        g_free(phrase);
        g_free(pinyin);
        n++;
    }
    if (iter)
        end(iter);
    printf("phrases@%s: %d%s\n%s", when, n, iter ? "" : " (no iterator)", rows->str);
    g_string_free(rows, TRUE);
}

/* learn <who>: import the side's phrase, train its target, and save
 * unless told not to. The import comes first so the trained target's
 * state is read on both sides after the same sequence. */
static void learn(context_t *ctx, instance_t *inst, const char *who, const struct target *t,
                  const struct target *p, bool do_save) {
    bool (*save)(context_t *) = sym("pinyin_save");
    printf("import %s: %s %d\n", who, p->word, import_phrase(ctx, p));
    printf("train %s: %s %s\n", who, t->word, train_target(inst, t));
    if (do_save)
        printf("save %s: %d\n", who, save(ctx));
    fflush(stdout);
}

int main(int argc, char **argv) {
    if (argc != 5) {
        fprintf(stderr, "usage: session-fork-diff <lib.so> <systemdir> <userdir> "
                        "<fork-_exit|fork-exit|fork-nosave|crash>\n");
        return 2;
    }
    const char *mode = argv[4];
    bool crash = strcmp(mode, "crash") == 0;
    bool child_saves = strcmp(mode, "fork-nosave") != 0;
    bool child_exit = strcmp(mode, "fork-exit") == 0;
    if (!crash && strcmp(mode, "fork-_exit") != 0 && strcmp(mode, "fork-exit") != 0 &&
        strcmp(mode, "fork-nosave") != 0) {
        fprintf(stderr, "unknown mode: %s\n", mode);
        return 2;
    }

    lib = dlopen(argv[1], RTLD_NOW);
    if (!lib) {
        fprintf(stderr, "%s\n", dlerror());
        return 1;
    }
    context_t *(*init)(const char *, const char *) = sym("pinyin_init");
    void (*fini)(context_t *) = sym("pinyin_fini");
    instance_t *(*alloc)(context_t *) = sym("pinyin_alloc_instance");
    void (*free_instance)(instance_t *) = sym("pinyin_free_instance");

    /* Line-buffered on both sides of the fork: a full buffer would be
     * duplicated into the child and flushed twice. */
    setvbuf(stdout, NULL, _IOLBF, 0);

    printf("mode: %s\n", mode);
    context_t *ctx = init(argv[2], argv[3]);
    printf("init: %s\n", ctx ? "ok" : "NULL");
    if (!ctx)
        return 1;
    instance_t *inst = alloc(ctx);
    printf("alloc: %s\n", inst ? "ok" : "NULL");
    if (!inst)
        return 1;
    target_rows(inst, "init");
    phrase_rows(ctx, "init");

    if (crash) {
        printf("crash: SIGKILL before fini\n");
        fflush(stdout);
        fflush(stderr);
        raise(SIGKILL);
        return 1; /* unreachable */
    }

    fflush(stdout);
    fflush(stderr);
    pid_t pid = fork();
    if (pid < 0) {
        perror("fork");
        return 1;
    }
    if (pid == 0) {
        learn(ctx, inst, "child", &CHILD_TARGET, &CHILD_IMPORT, child_saves);
        if (child_exit)
            exit(0);
        _exit(0);
    }
    int status = 0;
    if (waitpid(pid, &status, 0) != pid) {
        perror("waitpid");
        return 1;
    }
    if (WIFEXITED(status))
        printf("child: exited %d\n", WEXITSTATUS(status));
    else if (WIFSIGNALED(status))
        printf("child: signal %d\n", WTERMSIG(status));
    else
        printf("child: status %d\n", status);

    learn(ctx, inst, "parent", &PARENT_TARGET, &PARENT_IMPORT, true);
    target_rows(inst, "parent");
    phrase_rows(ctx, "parent");
    free_instance(inst);
    fini(ctx);
    printf("fini: ok\n");
    return 0;
}
