/*
 * open-counter-diff.c — one launch of an IME against a user dir, for the
 * user.conf open-counter differential (#523).
 *
 * The open counter only moves across process boundaries: the pin raises it
 * in pinyin_init's check_format (pinyin.cpp:185-187), lowers it in
 * pinyin_fini (:1194-1200), and wipes the profile when an init reads a
 * value above OPEN_COUNTER_LIMIT (table_info.cpp:32, :409-410). libzhuyin
 * never moves it: zhuyin.cpp's check_format only reads user.conf
 * (:126-162) and zhuyin_save's mark_version writes a fresh
 * UserTableInfo, counter 0 (:164-176, :695). So one process of this
 * driver is one launch, and run-open-counter-diff.sh runs it once per
 * launch, reading user.conf and the user dir's inventory in between.
 *
 * Usage:
 *   open-counter-diff <pinyin|zhuyin> <lib.so> <systemdir> <userdir> <launch> <cycle|crash>
 *
 *   cycle  init -> learned state -> learn (train launch k's target;
 *          pinyin also imports launch k's user phrase) -> save ->
 *          learned state -> fini.
 *   crash  init -> learned state -> SIGKILL: the process dies between init
 *          and fini, with no save and no fini, the way a crashed frontend
 *          leaves the profile.
 *
 * The learned state is read in-process, never by an extra launch: a
 * separate reader process would be one more init, and on the pinyin side
 * one more counter step, which would perturb the thing being measured.
 * The readings, each deterministic on both sides:
 *
 *   target rows  (pinyin) every training target of the table, looked up
 *                by text, with its tokens and unigram frequency. Training
 *                raises a target's frequency through the user files; a
 *                wipe drops it back to the system value.
 *   rank rows    (zhuyin) every training input of the table, parsed and
 *                listed: the sentence row, then the two-character
 *                candidates after the cursor, in order. Training moves the
 *                target up that list; a wipe moves it back. zhuyin reads
 *                learning this way because its frequency getter is not a
 *                shared surface: oxpinyin's zhuyin_token_get_unigram_
 *                frequency answers one above the pin's and leaves the
 *                user's trained delta out — a divergence of its own, not
 *                this differential's subject.
 *   phrase rows  (pinyin) the user dictionary through the §9 export: the
 *                user phrases the imports added. The bigram export runs
 *                once per context, after the save: repeating it in one
 *                context is the pin's registered class-(b) use-after-free
 *                (pinyin.cpp:842-872).
 *
 * What is learned is fixed per launch, not left to decoding. Each target
 * is a system word that both libraries list below their sentence rows
 * (ibus's candidate order for pinyin), so choosing it installs a
 * constraint and training writes it; a decode-driven choice would let the
 * registered class-(a) decode divergences into a differential that is
 * about persistence. The zhuyin targets are the lowest-frequency
 * two-character candidates of their inputs, so one training visibly moves
 * each. The imported phrases are spelled in tone-less pinyin, which both
 * pinyin importers accept; libzhuyin imports nothing here, because the
 * two zhuyin importers accept no common spelling (the pin parses the
 * argument as bopomofo, oxpinyin as full pinyin).
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
/* ibus's candidate order (PYPConfig.cc:151), as tools/oracle/user_driver.c:
 * SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY. */
#define SORT_OPTION (0x4 | 0x8 | 0x10)
/* The import count, explicit on both sides (-1 would ask for 5). */
#define IMPORT_COUNT 5

/* One training target per launch: the input typed and the word chosen
 * from its candidates at offset 0 — the first two-character listed
 * candidate below the sentence rows, identical on both sides for this
 * table. */
struct target {
    const char *typed;
    const char *word;
};

static const struct target PINYIN_TARGETS[] = {
    {"li'shi", "历时"},    {"yu'yan", "寓言"},  {"di'zhi", "抵制"},
    {"shi'jian", "实践"},  {"shui'guo", "睡过"}, {"zhong'yao", "锺繇"},
    {"shi'jie", "时节"},   {"shi'chang", "失常"}, {"zheng'fu", "正负"},
    {"yi'yuan", "议员"},   {"ji'shu", "基数"},  {"zhi'du", "只读"},
    {"shi'yan", "试验"},   {"gong'shi", "攻势"},
};

/* STANDARD keyboard keys: ㄉㄧㄢˋ ㄒㄧㄢˋ is "2u04vu04" and so on. Every
 * syllable carries a tone key, as FORCE_TONE (zhuyin.cpp:273) demands. */
static const struct target ZHUYIN_TARGETS[] = {
    {"2u04vu04", "癫痫"}, {"xu4g3", "砾石"},  {"m3u06", "预演"},   {"2u453", "低脂"},
    {"g4ru,4", "师姐"},   {"g4t;3", "时长"},  {"ru4gj4", "奇数"},  {"g6u04", "十堰"},
    {"u4ru04", "一间"},   {"g4g6", "适时"},   {"542j4", "之都"},   {"y4ru3", "字集"},
    {"xu4u4", "李毅"},    {"gj4y4", "熟字"},
};

#define N_TARGETS (sizeof(PINYIN_TARGETS) / sizeof(PINYIN_TARGETS[0]))
_Static_assert(sizeof(ZHUYIN_TARGETS) == sizeof(PINYIN_TARGETS), "one target per launch on both");

/* The pinyin user phrases, one import per clean launch. The pairs name no
 * system phrase, so each import adds a USER_DICTIONARY item. */
static const struct target PINYIN_IMPORTS[] = {
    {"ni'hao", "泥壕"},  {"ba'kua", "罢跨"},  {"ma'gu", "马股"},  {"na'gui", "拿柜"},
    {"ta'ye", "踏爷"},   {"sa'pei", "洒赔"},  {"la'hun", "辣魂"}, {"ka'nao", "卡恼"},
    {"za'mo", "杂抹"},   {"na'di", "那底"},   {"ta'dai", "塔戴"}, {"za'shai", "砸晒"},
    {"na'dao", "纳稻"},  {"ba'rui", "拔蕊"},
};
_Static_assert(sizeof(PINYIN_IMPORTS) == sizeof(PINYIN_TARGETS), "one import per launch");

static void *lib;
static const char *prefix;

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

/* A launch that dies between init and fini. SIGKILL runs no atexit
 * handler and no destructor on either side, so neither library gets a
 * chance to write anything after this point. */
static void die_before_fini(void) {
    printf("crash: SIGKILL before fini\n");
    fflush(stdout);
    fflush(stderr);
    raise(SIGKILL);
    exit(1); /* unreachable: SIGKILL cannot be caught or ignored */
}

static void target_rows(instance_t *inst, const struct target *table, const char *when) {
    bool (*lookup)(instance_t *, const char *, GArray *) = sym("lookup_tokens");
    bool (*unigram)(instance_t *, phrase_token_t, guint *) = sym("token_get_unigram_frequency");

    int n = 0;
    GString *rows = g_string_new(NULL);
    for (size_t i = 0; i < N_TARGETS; ++i) {
        GArray *tokens = g_array_new(FALSE, FALSE, sizeof(phrase_token_t));
        lookup(inst, table[i].word, tokens);
        for (guint t = 0; t < tokens->len; ++t) {
            phrase_token_t token = g_array_index(tokens, phrase_token_t, t);
            guint freq = 0;
            bool known = unigram(inst, token, &freq);
            g_string_append_printf(rows, "row@%s\tT\t%s\t%#010x\t%s%u\n", when, table[i].word,
                                   token, known ? "" : "unknown:", freq);
            n++;
        }
        g_array_free(tokens, TRUE);
    }
    printf("targets@%s: %d\n%s", when, n, rows->str);
    g_string_free(rows, TRUE);
}

/* List an input's candidates at offset 0 the way the train step does. */
static bool list_candidates(instance_t *inst, bool zhuyin, const char *typed) {
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

static void rank_rows(instance_t *inst, const struct target *table, const char *when) {
    bool (*n_candidate)(instance_t *, guint *) = sym("get_n_candidate");
    bool (*candidate)(instance_t *, guint, candidate_t **) = sym("get_candidate");
    bool (*candidate_type)(instance_t *, candidate_t *, int *) = sym("get_candidate_type");
    bool (*candidate_string)(instance_t *, candidate_t *, const gchar **) =
        sym("get_candidate_string");
    bool (*reset)(instance_t *) = sym("reset");

    GString *rows = g_string_new(NULL);
    for (size_t i = 0; i < N_TARGETS; ++i) {
        guint n = 0;
        if (list_candidates(inst, true, table[i].typed))
            n_candidate(inst, &n);
        GString *sentence = g_string_new(NULL), *listed = g_string_new(NULL);
        int shown = 0;
        for (guint j = 0; j < n && shown < 5; ++j) {
            candidate_t *c = NULL;
            int type = 0;
            const gchar *text = NULL;
            if (!candidate(inst, j, &c) || !c || !candidate_type(inst, c, &type) ||
                !candidate_string(inst, c, &text) || !text)
                continue;
            if (type != LISTED_CANDIDATE)
                g_string_append_printf(sentence, "%s%s", sentence->len ? " " : "", text);
            else if (g_utf8_strlen(text, -1) == 2) {
                g_string_append_printf(listed, "%s%s", listed->len ? " " : "", text);
                shown++;
            }
        }
        g_string_append_printf(rows, "row@%s\tR\t%s\t%s\t%s\n", when, table[i].typed,
                               sentence->str, listed->str);
        g_string_free(sentence, TRUE);
        g_string_free(listed, TRUE);
        reset(inst);
    }
    printf("ranks@%s: %zu\n%s", when, N_TARGETS, rows->str);
    g_string_free(rows, TRUE);
}

static void phrase_rows(context_t *ctx, const char *when) {
    iterator_t *(*begin)(context_t *, guint) = sym("begin_get_phrases");
    bool (*has_next)(iterator_t *) = sym("iterator_has_next_phrase");
    bool (*next)(iterator_t *, gchar **, gchar **, gint *) = sym("iterator_get_next_phrase");
    void (*end)(iterator_t *) = sym("end_get_phrases");

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

static void bigram_rows(context_t *ctx, const char *when) {
    iterator_t *(*begin)(context_t *) = sym("begin_get_bigram_phrases");
    bool (*has_next)(iterator_t *) = sym("bigram_iterator_has_next_phrase");
    bool (*next)(iterator_t *, gchar **, gchar **, gint *) = sym("bigram_iterator_get_next_phrase");
    void (*end)(iterator_t *) = sym("end_get_bigram_phrases");

    iterator_t *iter = begin(ctx);
    int n = 0;
    GString *rows = g_string_new(NULL);
    while (iter && has_next(iter)) {
        gchar *phrase = NULL, *pinyin = NULL;
        gint count = -1;
        if (!next(iter, &phrase, &pinyin, &count))
            break;
        g_string_append_printf(rows, "row@%s\tB\t%s\t%s\t%d\n", when, phrase, pinyin, count);
        g_free(phrase);
        g_free(pinyin);
        n++;
    }
    if (iter)
        end(iter);
    printf("bigrams@%s: %d%s\n%s", when, n, iter ? "" : " (no iterator)", rows->str);
    g_string_free(rows, TRUE);
}

static bool import_phrase(context_t *ctx, const struct target *p) {
    iterator_t *(*begin)(context_t *, guint8) = sym("begin_add_phrases");
    bool (*add)(iterator_t *, const char *, const char *, gint) = sym("iterator_add_phrase");
    void (*end)(iterator_t *) = sym("end_add_phrases");

    iterator_t *iter = begin(ctx, USER_DICTIONARY);
    if (!iter)
        return false;
    bool ok = add(iter, p->word, p->typed, IMPORT_COUNT);
    end(iter);
    return ok;
}

/* Choose the target at offset 0, then train the ibus way: re-guess after
 * the choose (the pin asserts in train_result3 when a train follows a
 * choose without one), train, reset. pinyin_train trains n-best row 0;
 * zhuyin_train takes no index. */
static const char *train_target(instance_t *inst, bool zhuyin, const struct target *t) {
    bool (*guess_sentence)(instance_t *) = sym("guess_sentence");
    bool (*n_candidate)(instance_t *, guint *) = sym("get_n_candidate");
    bool (*candidate)(instance_t *, guint, candidate_t **) = sym("get_candidate");
    bool (*candidate_type)(instance_t *, candidate_t *, int *) = sym("get_candidate_type");
    bool (*candidate_string)(instance_t *, candidate_t *, const gchar **) =
        sym("get_candidate_string");
    int (*choose)(instance_t *, size_t, candidate_t *) = sym("choose_candidate");
    bool (*reset)(instance_t *) = sym("reset");

    const char *result = "ok";
    guint n = 0;
    candidate_t *chosen = NULL;
    if (!list_candidates(inst, zhuyin, t->typed))
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
    }
    else if (choose(inst, 0, chosen) <= 0)
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

static int run(const char *system_dir, const char *user_dir, long launch, bool zhuyin,
               bool crash) {
    context_t *(*init)(const char *, const char *) = sym("init");
    void (*fini)(context_t *) = sym("fini");
    bool (*save)(context_t *) = sym("save");
    instance_t *(*alloc)(context_t *) = sym("alloc_instance");
    void (*free_instance)(instance_t *) = sym("free_instance");

    const struct target *table = zhuyin ? ZHUYIN_TARGETS : PINYIN_TARGETS;
    size_t k = (size_t)(launch - 1) % N_TARGETS;

    context_t *ctx = init(system_dir, user_dir);
    printf("init: %s\n", ctx ? "ok" : "NULL");
    if (!ctx)
        return 1;
    instance_t *inst = alloc(ctx);
    if (!inst) {
        printf("alloc: NULL\n");
        return 1;
    }
    if (zhuyin)
        rank_rows(inst, table, "init");
    else {
        target_rows(inst, table, "init");
        phrase_rows(ctx, "init");
    }
    if (crash)
        die_before_fini();

    if (!zhuyin)
        printf("import: %s %d\n", PINYIN_IMPORTS[k].word, import_phrase(ctx, &PINYIN_IMPORTS[k]));
    printf("train: %s %s\n", table[k].word, train_target(inst, zhuyin, &table[k]));
    printf("save: %d\n", save(ctx));
    if (zhuyin)
        rank_rows(inst, table, "saved");
    else {
        target_rows(inst, table, "saved");
        phrase_rows(ctx, "saved");
        bigram_rows(ctx, "saved");
    }
    free_instance(inst);
    fini(ctx);
    printf("fini\n");
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 7) {
        fprintf(stderr,
                "usage: %s <pinyin|zhuyin> <lib.so> <systemdir> <userdir> <launch> <cycle|crash>\n",
                argv[0]);
        return 2;
    }
    bool zhuyin = strcmp(argv[1], "zhuyin") == 0;
    if (!zhuyin && strcmp(argv[1], "pinyin") != 0) {
        fprintf(stderr, "kind must be pinyin or zhuyin: %s\n", argv[1]);
        return 2;
    }
    char *end = NULL;
    long launch = strtol(argv[5], &end, 10);
    if (!end || *end || launch < 1) {
        fprintf(stderr, "launch must be a positive integer: %s\n", argv[5]);
        return 2;
    }
    bool crash = strcmp(argv[6], "crash") == 0;
    if (!crash && strcmp(argv[6], "cycle") != 0) {
        fprintf(stderr, "mode must be cycle or crash: %s\n", argv[6]);
        return 2;
    }

    prefix = argv[1];
    lib = dlopen(argv[2], RTLD_NOW | RTLD_LOCAL);
    if (!lib) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    printf("launch: %ld %s\n", launch, argv[6]);
    return run(argv[3], argv[4], launch, zhuyin, crash);
}
