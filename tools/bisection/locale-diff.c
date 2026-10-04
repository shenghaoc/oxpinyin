/*
 * locale-diff.c — the process-locale differential (#539, register row 39),
 * over the forms of the user-dir argument (#619, register row 46).
 *
 * The pin's table.conf and user.conf codecs each run
 * `char * locale = setlocale(LC_NUMERIC, "C"); ... setlocale(LC_NUMERIC,
 * locale);` (storage/table_info.cpp:197,291; :328,372; :378,394). setlocale
 * answers the name of the locale it installs, so the "restore" installs "C"
 * again, and every early return skips it with the same result: each call
 * leaves the process's LC_NUMERIC at "C", on success and on failure. The
 * entry points that reach a codec inherit that: pinyin_init/zhuyin_init
 * (pinyin.cpp:337, :178, :187; zhuyin.cpp:281, :132), pinyin_save and
 * zhuyin_save past their user-dir and m_modified guards (pinyin.cpp:1133-
 * 1143; zhuyin.cpp:548-552, :695), and pinyin_fini unconditionally
 * (pinyin.cpp:1200). zhuyin_fini writes nothing (zhuyin.cpp:741-757).
 *
 * Which of those a context reaches turns on its user dir, and the user-dir
 * guards test the pointer (pinyin.cpp:1133, :2671; zhuyin.cpp:548, :1697).
 * The inits keep `g_strdup(userdir)` (pinyin.cpp:332, zhuyin.cpp:276) and
 * build every user file's path as `g_build_filename(m_user_dir, <name>,
 * NULL)` when the file is touched; g_build_filename drops an empty element
 * and stops at a NULL one. So:
 *
 *   ""    is the working directory: every path is a bare file name, resolved
 *         against the directory that is current when the file is opened, not
 *         the one that was current at init. The context trains, and a dirty
 *         save writes the profile there and resets LC_NUMERIC.
 *   NULL  is no user dir: train and save answer false, every path is the
 *         empty string and nothing is written.
 *   "."   and an absolute directory are ordinary user dirs.
 *
 * This driver plays the consumer that adopted its environment's locale
 * (the runner exports LC_ALL=zh_CN.UTF-8) and reads LC_NUMERIC and the
 * LC_ALL composite back after every entry point, re-adopting the
 * environment before each step so each site is measured on its own. With
 * them it prints what the step left on disk — each watched directory's
 * inventory (name:size:mode) and its user.conf text — since where the
 * profile lands is what the user-dir forms differ in:
 *
 *   before            setlocale(LC_ALL, "") alone
 *   init-ok           a good system dir and the user dir under test
 *   chdir             (only with <chdir-to>) the consumer moves there
 *   save-unmodified   save straight after init (both guards: false, no write)
 *   train             choose the first listed candidate + train, which sets
 *                     m_modified; the watched word's unigram frequency is
 *                     printed first — the system value on a fresh profile,
 *                     the trained one once a saved profile was read back
 *   save-modified     save past both guards
 *   fini              teardown
 *   init-missing      a system dir that does not exist
 *   init-empty        a system dir with no table.conf
 *   init-garbage      a table.conf whose first line is not the format line
 *   init-truncated    a table.conf cut after its two version lines
 *   init-empty-str    systemdir ""
 *   init-null         systemdir NULL
 *   init-null-userdir a good system dir with userdir NULL, then fini
 *
 * Usage:
 *   locale-diff <pinyin|zhuyin> <lib.so> <systemdir> <userdir> <scratchdir>
 *               [<chdir-to>]
 *
 * <userdir> is passed to init as given, so "" and "." are the working
 * directory forms; the word NULL passes a NULL pointer. <scratchdir> holds
 * the failure fixtures (missing/, empty/, garbage/, truncated/), which the
 * runner prepares. The watched directories are the working directory and,
 * beside it, an absolute <userdir> or — with <chdir-to> — the directory the
 * process started in. No path is printed, so each side may run in
 * directories of its own; run-locale-diff.sh runs every form twice in the
 * same directories and diffs the two sides' logs byte for byte.
 *
 * This file is part of oxpinyin, GPL-3.0-or-later like the rest of it.
 */

#define _POSIX_C_SOURCE 200809L
#include <dirent.h>
#include <dlfcn.h>
#include <errno.h>
#include <limits.h>
#include <locale.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#include <glib.h>

typedef void context_t;
typedef void instance_t;
typedef void candidate_t;

/* pinyin.h:45 NORMAL_CANDIDATE; zhuyin.h:43 NORMAL_CANDIDATE_AFTER_CURSOR */
#define LISTED_CANDIDATE 2
/* ibus's candidate order (PYPConfig.cc:151):
 * SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY. */
#define SORT_OPTION (0x4 | 0x8 | 0x10)

/* The typed input and the watched word: the first listed candidate below
 * the sentence rows on a fresh profile, identical on both sides, which the
 * first process therefore trains (the same pair
 * tools/bisection/open-counter-diff.c trains first). The zhuyin keys are
 * STANDARD keyboard keys with a tone on every syllable (FORCE_TONE). */
static const char *PINYIN_TYPED = "li'shi", *PINYIN_WORD = "\xe5\x8e\x86\xe6\x97\xb6"; /* 历时 */
static const char *ZHUYIN_TYPED = "2u04vu04", *ZHUYIN_WORD = "\xe7\x99\xab\xe7\x97\xab"; /* 癫痫 */

static void *lib;
static const char *prefix;

/* The directory printed beside the working directory after every step: an
 * absolute user dir, or the directory the process started in. */
static const char *watched_label;
static const char *watched_dir;

/* `<prefix>_<name>`: the two libraries name every entry point alike. */
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

/* Re-adopt the environment's locale, so every step starts from the state
 * the runner's LC_ALL names and a site that resets LC_NUMERIC shows on its
 * own line. A locale the host has not generated is a provisioning gap, not
 * a divergence: fail loudly. */
static void adopt_environment(void) {
    if (!setlocale(LC_ALL, "")) {
        fprintf(stderr, "fatal: setlocale(LC_ALL, \"\") failed; is the locale named by LC_ALL generated?\n");
        exit(1);
    }
}

static void print_escaped(const char *data, size_t len) {
    for (size_t i = 0; i < len; ++i) {
        unsigned char c = (unsigned char)data[i];
        if (c == '\n')
            fputs("\\n", stdout);
        else if (c == '\\')
            fputs("\\\\", stdout);
        else if (c < 0x20 || c == 0x7f)
            printf("\\x%02x", c);
        else
            fputc(c, stdout);
    }
}

static int by_name(const struct dirent **a, const struct dirent **b) {
    return strcmp((*a)->d_name, (*b)->d_name);
}

static int not_dots(const struct dirent *entry) {
    return strcmp(entry->d_name, ".") != 0 && strcmp(entry->d_name, "..") != 0;
}

/* One directory's inventory as `name:size:mode`, byte-ordered, and its
 * user.conf text (the open counter lives there). */
static void show_dir(const char *step, const char *label, const char *dir) {
    struct dirent **names = NULL;
    int n = scandir(dir, &names, not_dots, by_name);
    if (n < 0) {
        fprintf(stderr, "fatal: scandir(%s): %s\n", dir, strerror(errno));
        exit(1);
    }
    printf("%s\t%s:", step, label);
    if (n == 0)
        fputs(" (empty)", stdout);
    for (int i = 0; i < n; ++i) {
        char path[PATH_MAX];
        struct stat st;
        snprintf(path, sizeof path, "%s/%s", dir, names[i]->d_name);
        fputc(' ', stdout);
        print_escaped(names[i]->d_name, strlen(names[i]->d_name));
        if (stat(path, &st) == 0)
            printf(":%lld:%o", (long long)st.st_size, (unsigned)(st.st_mode & 07777));
        else
            fputs(":?:?", stdout);
        free(names[i]);
    }
    free(names);
    fputc('\n', stdout);

    char conf[PATH_MAX];
    snprintf(conf, sizeof conf, "%s/user.conf", dir);
    printf("%s\t%s/user.conf: ", step, label);
    FILE *file = fopen(conf, "r");
    if (!file) {
        fputs("(absent)\n", stdout);
        return;
    }
    char text[1024];
    size_t got = fread(text, 1, sizeof text, file);
    fclose(file);
    print_escaped(text, got);
    fputc('\n', stdout);
}

static void probe(const char *step) {
    const char *numeric = setlocale(LC_NUMERIC, NULL);
    const char *all = setlocale(LC_ALL, NULL);
    printf("%s\tLC_NUMERIC=%s\tLC_ALL=%s\n", step, numeric ? numeric : "(null)", all ? all : "(null)");
    show_dir(step, "cwd", ".");
    if (watched_dir)
        show_dir(step, watched_label, watched_dir);
}

/* The watched word's unigram frequency as the context holds it now. */
static void show_watched(instance_t *inst, bool zhuyin) {
    bool (*lookup_tokens)(instance_t *, const char *, GArray *) = sym("lookup_tokens");
    bool (*unigram)(instance_t *, guint32, guint *) = sym("token_get_unigram_frequency");
    const char *word = zhuyin ? ZHUYIN_WORD : PINYIN_WORD;
    GArray *tokens = g_array_new(FALSE, FALSE, sizeof(guint32));

    printf("train: watched %s unigram", word);
    if (!lookup_tokens(inst, word, tokens) || tokens->len == 0)
        fputs(" (no token)", stdout);
    for (guint i = 0; i < tokens->len; ++i) {
        guint32 token = g_array_index(tokens, guint32, i);
        guint freq = 0;
        if (unigram(inst, token, &freq))
            printf(" 0x%08x=%u", token, freq);
        else
            printf(" 0x%08x=(none)", token);
    }
    fputc('\n', stdout);
    g_array_free(tokens, TRUE);
}

/* Choose the first listed candidate at offset 0, then train the ibus way:
 * re-guess after the choose, train, reset. pinyin_train trains n-best row 0;
 * zhuyin_train takes no index. The first listed candidate, not a fixed
 * word: a second process on the same profile finds the word it trained
 * promoted to a sentence row, and still has to train something. */
static const char *train_target(instance_t *inst, bool zhuyin) {
    size_t (*parse)(instance_t *, const char *) =
        sym(zhuyin ? "parse_more_chewings" : "parse_more_full_pinyins");
    bool (*guess_sentence)(instance_t *) = sym("guess_sentence");
    bool (*n_candidate)(instance_t *, guint *) = sym("get_n_candidate");
    bool (*candidate)(instance_t *, guint, candidate_t **) = sym("get_candidate");
    bool (*candidate_type)(instance_t *, candidate_t *, int *) = sym("get_candidate_type");
    bool (*candidate_string)(instance_t *, candidate_t *, const gchar **) =
        sym("get_candidate_string");
    int (*choose)(instance_t *, size_t, candidate_t *) = sym("choose_candidate");
    bool (*reset)(instance_t *) = sym("reset");

    const char *typed = zhuyin ? ZHUYIN_TYPED : PINYIN_TYPED;
    const char *result = "ok";
    guint n = 0;
    candidate_t *chosen = NULL;
    const gchar *chosen_text = NULL;

    bool listed = strlen(typed) == parse(inst, typed) && guess_sentence(inst);
    if (listed) {
        if (zhuyin) {
            bool (*guess_after)(instance_t *, size_t) = sym("guess_candidates_after_cursor");
            listed = guess_after(inst, 0);
        } else {
            bool (*guess)(instance_t *, size_t, guint) = sym("guess_candidates");
            listed = guess(inst, 0, SORT_OPTION);
        }
    }
    if (!listed)
        result = "no-list";
    else
        n_candidate(inst, &n);
    for (guint i = 0; i < n && !chosen; ++i) {
        candidate_t *c = NULL;
        int type = 0;
        const gchar *text = NULL;
        if (candidate(inst, i, &c) && c && candidate_type(inst, c, &type) &&
            type == LISTED_CANDIDATE && candidate_string(inst, c, &text) && text) {
            chosen = c;
            chosen_text = text;
        }
    }
    printf("train: target %s\n", chosen ? chosen_text : "(none)");
    if (!chosen) {
        if (strcmp(result, "ok") == 0)
            result = "no-candidate";
    } else if (choose(inst, 0, chosen) <= 0)
        result = "choose-failed";
    else if (!guess_sentence(inst))
        result = "reguess-failed";
    else if (zhuyin) {
        bool (*train)(instance_t *) = sym("train");
        if (!train(inst))
            result = "train-false";
    } else {
        bool (*train)(instance_t *, guint8) = sym("train");
        if (!train(inst, 0))
            result = "train-false";
    }
    reset(inst);
    return result;
}

/* One init that is expected to fail; the pin leaks the half-built context
 * on this path (pinyin.cpp:337-340), so there is nothing to free. */
static void failing_init(const char *step, const char *system_dir, const char *user_dir) {
    context_t *(*init)(const char *, const char *) = sym("init");
    adopt_environment();
    context_t *ctx = init(system_dir, user_dir);
    printf("%s: init %s\n", step, ctx ? "ok" : "NULL");
    probe(step);
}

int main(int argc, char **argv) {
    if (argc != 6 && argc != 7) {
        fprintf(stderr,
                "usage: %s <pinyin|zhuyin> <lib.so> <systemdir> <userdir> <scratchdir> [<chdir-to>]\n",
                argv[0]);
        return 2;
    }
    prefix = argv[1];
    bool zhuyin = strcmp(prefix, "zhuyin") == 0;
    if (!zhuyin && strcmp(prefix, "pinyin") != 0) {
        fprintf(stderr, "first argument must be pinyin or zhuyin\n");
        return 2;
    }
    const char *system_dir = argv[3], *scratch = argv[5];
    const char *user_dir = strcmp(argv[4], "NULL") == 0 ? NULL : argv[4];
    const char *chdir_to = argc == 7 ? argv[6] : NULL;
    static char first_cwd[PATH_MAX];
    if (chdir_to) {
        if (!getcwd(first_cwd, sizeof first_cwd)) {
            fprintf(stderr, "fatal: getcwd: %s\n", strerror(errno));
            return 1;
        }
        watched_label = "first-cwd";
        watched_dir = first_cwd;
    } else if (user_dir && user_dir[0] == '/') {
        watched_label = "userdir";
        watched_dir = user_dir;
    }
    lib = dlopen(argv[2], RTLD_NOW | RTLD_LOCAL);
    if (!lib) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    setvbuf(stdout, NULL, _IOLBF, 0);

    context_t *(*init)(const char *, const char *) = sym("init");
    void (*fini)(context_t *) = sym("fini");
    bool (*save)(context_t *) = sym("save");
    instance_t *(*alloc)(context_t *) = sym("alloc_instance");
    void (*free_instance)(instance_t *) = sym("free_instance");

    adopt_environment();
    probe("before");

    adopt_environment();
    context_t *ctx = init(system_dir, user_dir);
    printf("init-ok: init %s\n", ctx ? "ok" : "NULL");
    probe("init-ok");
    if (!ctx)
        return 1;

    if (chdir_to) {
        if (chdir(chdir_to) != 0) {
            fprintf(stderr, "fatal: chdir(%s): %s\n", chdir_to, strerror(errno));
            return 1;
        }
        printf("chdir: done\n");
        probe("chdir");
    }

    adopt_environment();
    printf("save-unmodified: save %d\n", save(ctx));
    probe("save-unmodified");

    adopt_environment();
    instance_t *inst = alloc(ctx);
    if (!inst) {
        printf("train: alloc NULL\n");
        return 1;
    }
    show_watched(inst, zhuyin);
    /* Whether this train had to succeed depends on the form — a NULL user
     * dir refuses it on the pin (pinyin.cpp:2671, zhuyin.cpp:1697) — so the
     * runner, which knows the form, is the one that fails a run whose oracle
     * never reached the dirty save. The driver carries on either way: a
     * train the pin makes and the other side refuses is a divergence, and
     * the steps after it are part of it. */
    printf("train: %s\n", train_target(inst, zhuyin));
    probe("train");

    adopt_environment();
    printf("save-modified: save %d\n", save(ctx));
    probe("save-modified");

    adopt_environment();
    free_instance(inst);
    fini(ctx);
    printf("fini: done\n");
    probe("fini");

    char path[4096];
    snprintf(path, sizeof path, "%s/missing", scratch);
    failing_init("init-missing", path, user_dir);
    snprintf(path, sizeof path, "%s/empty", scratch);
    failing_init("init-empty", path, user_dir);
    snprintf(path, sizeof path, "%s/garbage", scratch);
    failing_init("init-garbage", path, user_dir);
    snprintf(path, sizeof path, "%s/truncated", scratch);
    failing_init("init-truncated", path, user_dir);
    failing_init("init-empty-str", "", user_dir);
    failing_init("init-null", NULL, user_dir);

    adopt_environment();
    ctx = init(system_dir, NULL);
    printf("init-null-userdir: init %s\n", ctx ? "ok" : "NULL");
    probe("init-null-userdir");
    if (ctx) {
        adopt_environment();
        fini(ctx);
        printf("fini-null-userdir: done\n");
        probe("fini-null-userdir");
    }
    return 0;
}
