/*
 * tie-order-diff.c — the candidate array order behind comparator ties,
 * against the pin (issue #536).
 *
 * `pinyin_guess_candidates` sorts with a stable sort over a key that
 * truncates the amplified frequency to a `guint32` (`pinyin.cpp:1862-1866`
 * at 074a2219), so many candidates tie and keep the array order the
 * search laid down: `search_matrix` walks the window's key-paths
 * depth-first, each table hit files its records in the index chunk's
 * stored order (`ChewingTableEntry::convert`, `chewing_large_table2.h:
 * 91-133` — the chunk is ordered by stored key, initials then middles and
 * finals then tones, and only then by token, `add_index` `:208-230`), and
 * `_append_items` emits the window library by library in that encounter
 * order (`:1769-1791`). An incomplete syllable matches a whole block of
 * stored keys, so its ties are not token-ascending; a user-library
 * phrase goes through the same chunk law in the user table.
 *
 * Per option word in {0x18a, 0x38a, 0x1fe0039a} (parity, +DYNAMIC_ADJUST,
 * ibus-libpinyin's default with corrections) and sort word in {0x1c, 0x1e}
 * (ibus's, the pin's default), every initial-only key and a few
 * two-syllable incomplete inputs are parsed, guessed and listed in full
 * (not `n'h`: its n-best rows differ on the sentence surface — the
 * frozen class (a) trellis divergence — and every row below them would
 * shift with the count, which this driver is not about): index, type
 * and string per row. (Tokens and unigram frequencies per row made the
 * same walk an order of magnitude slower; the attribution that needed
 * them is in #620.) The user half imports a small dictionary of
 * one-character phrases under `n` readings that differ only in the
 * stored key (the shape ibus-libpinyin's network dictionary has) into
 * USER_DICTIONARY, then lists `n` again.
 *
 * Usage: ./tie-order-diff <path-to-so> <systemdir>
 *
 * The user dir is a fresh temp dir under $TMPDIR, removed afterwards.
 * This file is part of oxpinyin, GPL-3.0-or-later like the rest of it.
 */
#define _POSIX_C_SOURCE 200809L
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dirent.h>
#include <sys/stat.h>
#include <unistd.h>

#include <glib.h>

typedef void context_t;
typedef void instance_t;
typedef void candidate_t;
typedef void iterator_t;
typedef uint32_t phrase_token_t;

#define USER_DICTIONARY 7

static void *lib;

static void *sym(const char *name) {
    void *s = dlsym(lib, name);
    if (!s) {
        fprintf(stderr, "missing symbol: %s\n", name);
        exit(1);
    }
    return s;
}

static const char *INITIALS[] = {
    "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x",
    "zh", "ch", "sh", "r", "z", "c", "s", "y", "w", "a", "e", "o",
    "ni'h", "zh'g", "w'm",
};

static const uint32_t WORDS[] = {0x18a, 0x38a, 0x1fe0039a};
static const uint32_t SORTS[] = {0x1c, 0x1e};

/* The user dictionary: one-character phrases whose readings share the
 * initial `n` and differ in middle, final or both. */
static const struct {
    const char *text, *pinyin;
} USER_PHRASES[] = {
    {"𦰡", "na"}, {"𫐐", "ni"}, {"𫠜", "ni"}, {"𨺙", "ni"}, {"𫔶", "nie"},
    {"𬪩", "nong"}, {"𦰡", "nuo"}, {"𫟅", "nen"}, {"𫵷", "nao"}, {"𬇕", "nu"},
};

static void list(instance_t *inst, const char *tag) {
    bool (*n_candidate)(instance_t *, guint *) = sym("pinyin_get_n_candidate");
    bool (*candidate)(instance_t *, guint, candidate_t **) = sym("pinyin_get_candidate");
    bool (*candidate_type)(instance_t *, candidate_t *, int *) = sym("pinyin_get_candidate_type");
    bool (*candidate_string)(instance_t *, candidate_t *, const gchar **) =
        sym("pinyin_get_candidate_string");

    guint n = 0;
    n_candidate(inst, &n);
    printf("%s n=%u\n", tag, n);
    for (guint i = 0; i < n; ++i) {
        candidate_t *c = NULL;
        int type = 0;
        const gchar *text = NULL;
        if (!candidate(inst, i, &c) || !c || !candidate_type(inst, c, &type) ||
            !candidate_string(inst, c, &text) || !text)
            continue;
        printf("  %u\t%d\t%s\n", i, type, text);
    }
}

static void sweep(context_t *ctx, instance_t *inst, const char *phase) {
    bool (*set_options)(context_t *, uint32_t) = sym("pinyin_set_options");
    size_t (*parse)(instance_t *, const char *) = sym("pinyin_parse_more_full_pinyins");
    bool (*guess)(instance_t *) = sym("pinyin_guess_sentence");
    bool (*candidates)(instance_t *, size_t, guint) = sym("pinyin_guess_candidates");
    bool (*reset)(instance_t *) = sym("pinyin_reset");

    for (size_t w = 0; w < sizeof(WORDS) / sizeof(WORDS[0]); ++w) {
        printf("options %#x: %d\n", WORDS[w], set_options(ctx, WORDS[w]));
        for (size_t s = 0; s < sizeof(SORTS) / sizeof(SORTS[0]); ++s) {
            for (size_t i = 0; i < sizeof(INITIALS) / sizeof(INITIALS[0]); ++i) {
                char tag[96];
                reset(inst);
                size_t parsed = parse(inst, INITIALS[i]);
                bool guessed = guess(inst);
                bool listed = candidates(inst, 0, SORTS[s]);
                snprintf(tag, sizeof tag, "%s|%#x|%#x|%s parsed=%zu guess=%d list=%d", phase,
                         WORDS[w], SORTS[s], INITIALS[i], parsed, guessed, listed);
                list(inst, tag);
                reset(inst);
            }
        }
    }
}

static void remove_tree(const char *dir) {
    DIR *d = opendir(dir);
    if (!d)
        return;
    struct dirent *e;
    while ((e = readdir(d))) {
        if (!strcmp(e->d_name, ".") || !strcmp(e->d_name, ".."))
            continue;
        char path[4096];
        snprintf(path, sizeof path, "%s/%s", dir, e->d_name);
        unlink(path);
    }
    closedir(d);
    rmdir(dir);
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "Usage: %s <so> <systemdir>\n", argv[0]);
        return 1;
    }
    lib = dlopen(argv[1], RTLD_NOW);
    if (!lib) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        return 1;
    }
    context_t *(*init)(const char *, const char *) = sym("pinyin_init");
    void (*fini)(context_t *) = sym("pinyin_fini");
    instance_t *(*alloc)(context_t *) = sym("pinyin_alloc_instance");
    void (*free_instance)(instance_t *) = sym("pinyin_free_instance");
    iterator_t *(*begin)(context_t *, guint8) = sym("pinyin_begin_add_phrases");
    bool (*add)(iterator_t *, const char *, const char *, gint) = sym("pinyin_iterator_add_phrase");
    void (*end)(iterator_t *) = sym("pinyin_end_add_phrases");

    char user_dir[4096];
    const char *tmp = getenv("TMPDIR");
    snprintf(user_dir, sizeof user_dir, "%s/tie-order-diff-%ld", tmp ? tmp : "/tmp", (long)getpid());
    remove_tree(user_dir);
    if (mkdir(user_dir, 0700) != 0) {
        perror("mkdir");
        return 1;
    }

    context_t *ctx = init(argv[2], user_dir);
    if (!ctx) {
        fprintf(stderr, "pinyin_init failed\n");
        return 1;
    }
    instance_t *inst = alloc(ctx);
    if (!inst) {
        fprintf(stderr, "pinyin_alloc_instance failed\n");
        return 1;
    }

    sweep(ctx, inst, "system");

    iterator_t *iter = begin(ctx, USER_DICTIONARY);
    for (size_t i = 0; iter && i < sizeof(USER_PHRASES) / sizeof(USER_PHRASES[0]); ++i)
        printf("import %s %s: %d\n", USER_PHRASES[i].text, USER_PHRASES[i].pinyin,
               add(iter, USER_PHRASES[i].text, USER_PHRASES[i].pinyin, 1));
    if (iter)
        end(iter);

    sweep(ctx, inst, "user");

    free_instance(inst);
    fini(ctx);
    remove_tree(user_dir);
    return 0;
}
