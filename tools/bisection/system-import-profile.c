/* #599 reproducible callgrind/massif workload. No wall-clock timing.
 * Usage: <libpinyin.so> <system-data-dir> <fresh-user-dir> [iterations]
 * Every iteration merges one original item and one appended item in each
 * SYSTEM_FILE library; save writes all four .dbin streams.
 * Use --cold-open instead of iterations for #588's documented empty-input
 * lifecycle: init, set the parity options, allocate/free an instance, fini.
 * This mode performs no import, save or intermediate Callgrind dump.
 * #554 modes: --sentence-fixture <frozen-fixture> (500 comparable rows),
 * --train-fixture <frozen-fixture> (first 200; train_top-equivalent),
 * --phrase-file <lib\tphrase\tpinyin\tcount rows> (10k imports, save, export
 * touched libraries), --export-system (selection only, unmeasured).
 * These modes emit no Callgrind client-request dumps. */
#include <dlfcn.h>
#include <dirent.h>
#include <errno.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <valgrind/callgrind.h>

static void *symbol(void *so, const char *name) {
    void *fn = dlsym(so, name);
    if (!fn) { fprintf(stderr, "missing %s\n", name); exit(1); }
    return fn;
}

/* #554: narrow fixture replay and phrase-file extensions. Output is stdout;
 * callers may compare in memory. No profiler dumps in these modes. */
static unsigned export_walk(void *so, void *ctx, unsigned lib, bool rows) {
    void *(*begin)(void *, uint32_t) = symbol(so, "pinyin_begin_get_phrases");
    bool (*next)(void *) = symbol(so, "pinyin_iterator_has_next_phrase");
    bool (*get)(void *, char **, char **, int32_t *) = symbol(so, "pinyin_iterator_get_next_phrase");
    void (*end)(void *) = symbol(so, "pinyin_end_get_phrases");
    void (*release)(void *) = symbol(so, "g_free");
    void *it = begin(ctx, lib);
    if (!it) { fprintf(stderr, "export begin failed\n"); exit(1); }
    unsigned count = 0;
    while (next(it)) {
        char *phrase = NULL, *pinyin = NULL;
        int32_t frequency = -1;
        if (!get(it, &phrase, &pinyin, &frequency) || !phrase || !pinyin) {
            fprintf(stderr, "export get failed\n"); exit(1);
        }
        if (rows) printf("%u\t%s\t%s\t%d\n", lib, phrase, pinyin, frequency);
        release(phrase); release(pinyin); ++count;
    }
    end(it);
    return count;
}
static int sentence_fixture(void *so, void *ctx, const char *path, bool training) {
    FILE *f = fopen(path, "r");
    if (!f) return 1;
    bool (*options)(void *, uint32_t) = symbol(so, "pinyin_set_options");
    void *(*alloc)(void *) = symbol(so, "pinyin_alloc_instance");
    void (*free_instance)(void *) = symbol(so, "pinyin_free_instance");
    bool (*reset)(void *) = symbol(so, "pinyin_reset");
    size_t (*parse)(void *, const char *) = symbol(so, "pinyin_parse_more_full_pinyins");
    size_t (*parsed)(void *) = symbol(so, "pinyin_get_parsed_input_length");
    bool (*sentence)(void *) = symbol(so, "pinyin_guess_sentence");
    bool (*guess)(void *, size_t, uint32_t) = symbol(so, "pinyin_guess_candidates");
    bool (*getn)(void *, uint32_t *) = symbol(so, "pinyin_get_n_candidate");
    bool (*getc)(void *, uint32_t, void **) = symbol(so, "pinyin_get_candidate");
    bool (*type)(void *, void *, int *) = symbol(so, "pinyin_get_candidate_type");
    bool (*nbest)(void *, void *, uint8_t *) = symbol(so, "pinyin_get_candidate_nbest_index");
    bool (*text)(void *, void *, const char **) = symbol(so, "pinyin_get_candidate_string");
    bool (*get_sentence)(void *, uint8_t, char **) = symbol(so, "pinyin_get_sentence");
    bool (*train)(void *, uint8_t) = symbol(so, "pinyin_train");
    bool (*save)(void *) = symbol(so, "pinyin_save");
    void (*release)(void *) = symbol(so, "g_free");
    if (!options(ctx, 0x18a)) return 1;
    void *inst = alloc(ctx);
    if (!inst) return 1;
    char *line = NULL; size_t capacity = 0; unsigned count = 0, trained = 0;
    while (getline(&line, &capacity, f) >= 0) {
        if (line[0] == '#' || line[0] == '\n') continue;
        char *cursor = line;
        char *input = strsep(&cursor, "\t");
        char *expected = strsep(&cursor, "\t");
        char *surface = strsep(&cursor, "\t");
        if (!expected || !surface) return 1;
        if (!surface[0]) continue; /* sentence_tail.rs::measure filter */
        if (training && count == 200) break;
        if (!reset(inst)) return 1;
        size_t parse_return = parse(inst, input);
        size_t length = parsed(inst);
        /* Session::train_top checks full consumption before guessing. */
        if (training && length != strlen(input)) {
            fprintf(stderr, "train_top parse precondition failed row=%u input=%s parsed=%zu expected=%zu\n",
                    count + 1, input, length, strlen(input));
            return 1;
        }
        if (!training && length == 0) return 1;
        bool guessed = sentence(inst);
        /* Session::train_top's reset/parse/full-consumption/guess/train(0)
         * sequence; no candidate selection or unguessed training. */
        if (training) {
            if (!guessed || !train(inst, 0)) {
                fprintf(stderr, "train_top guess/train failed row=%u input=%s guess=%d\n",
                        count + 1, input, guessed);
                return 1;
            }
            char *top = NULL;
            if (!get_sentence(inst, 0, &top) || !top) return 1;
            ++trained;
            printf("train\t%s\t%zu\t%zu\t1\t%s\n", input, parse_return, length, top);
            release(top);
        } else {
            if (!guess(inst, 0, 0x1e)) return 1;
            uint32_t n = 0;
            if (!getn(inst, &n)) return 1;
            uint8_t proven = 0;
            for (uint32_t i = 0; i < n; ++i) {
                void *cand = NULL; int kind = 0; uint8_t rank = 0;
                if (!getc(inst, i, &cand) || !type(inst, cand, &kind)) return 1;
                if (kind == 1) {
                    if (!nbest(inst, cand, &rank)) return 1;
                    if (rank > proven) proven = rank;
                }
            }
            if (proven > 2) proven = 2;
            printf("%s\t%s\t", input, guessed ? "true" : "false");
            for (uint8_t i = 0; i <= proven; ++i) {
                char *value = NULL;
                bool ok = get_sentence(inst, i, &value);
                if (i) putchar(1);
                printf("%s", ok && value ? value : "-"); release(value);
            }
            putchar('\t');
            for (uint32_t i = 0; i < n && i < 6; ++i) {
                void *cand = NULL; int kind = 0; uint8_t rank = 0; const char *value = NULL;
                if (!getc(inst, i, &cand) || !type(inst, cand, &kind) || !text(inst, cand, &value)) return 1;
                if (i) putchar(1);
                if (kind == 1) {
                    if (!nbest(inst, cand, &rank)) return 1;
                    printf("n/%u/%s", rank, value);
                } else printf("%c/-/%s", kind == 2 ? 'N' : kind == 6 ? 'a' : '?', value);
            }
            putchar('\n');
        }
        ++count;
    }
    free(line); fclose(f);
    if (count != (training ? 200u : 500u) || (training && !save(ctx))) return 1;
    printf("operations\t%u\t%u\n", count, trained);
    free_instance(inst);
    return 0;
}
static int phrase_file(void *so, void *ctx, const char *path) {
    FILE *f = fopen(path, "r");
    if (!f) return 1;
    void *(*begin)(void *, uint8_t) = symbol(so, "pinyin_begin_add_phrases");
    bool (*add)(void *, const char *, const char *, int32_t) = symbol(so, "pinyin_iterator_add_phrase");
    void (*end)(void *) = symbol(so, "pinyin_end_add_phrases");
    bool (*save)(void *) = symbol(so, "pinyin_save");
    char *line = NULL; size_t capacity = 0; unsigned imported = 0, active = 0;
    void *it = NULL; bool touched[5] = {false};
    bool (*load)(void *, uint8_t) = symbol(so, "pinyin_load_phrase_library");
    ssize_t line_length;
    while ((line_length = getline(&line, &capacity, f)) >= 0) {
        if (strnlen(line, (size_t)line_length) != (size_t)line_length) return 1;
        char *cursor = line;
        char *lib_s = strsep(&cursor, "\t"), *phrase = strsep(&cursor, "\t"), *pinyin = strsep(&cursor, "\t");
        if (!lib_s || !phrase || !pinyin || !cursor) return 1;
        if (lib_s[0] < '0' || lib_s[0] > '9') return 1;
        char *lib_end = NULL;
        errno = 0;
        unsigned long parsed_lib = strtoul(lib_s, &lib_end, 10);
        if (errno == ERANGE || *lib_end || parsed_lib < 1 || parsed_lib > 4) return 1;
        unsigned lib = (unsigned)parsed_lib;
        char *count_end = NULL;
        errno = 0;
        long parsed_count = strtol(cursor, &count_end, 10);
        if (cursor == count_end || errno == ERANGE ||
            parsed_count < INT32_MIN || parsed_count > INT32_MAX ||
            !(*count_end == '\0' ||
              (*count_end == '\n' && count_end[1] == '\0') ||
              (*count_end == '\r' && count_end[1] == '\n' && count_end[2] == '\0'))) return 1;
        if (active != lib) {
            if (it) end(it);
            (void)load(ctx, (uint8_t)lib);
            it = begin(ctx, (uint8_t)lib); active = lib; touched[lib] = true;
            if (!it) return 1;
        }
        if (!add(it, phrase, pinyin, (int32_t)parsed_count)) return 1;
        ++imported;
    }
    if (it) end(it);
    free(line); fclose(f);
    if (imported != 10000 || !save(ctx)) return 1;
    unsigned exported = 0;
    for (unsigned lib = 1; lib <= 4; ++lib) if (touched[lib]) exported += export_walk(so, ctx, lib, true);
    printf("operations\t%u\t%u\n", imported, exported);
    return 0;
}
int main(int argc, char **argv) {
    if (argc < 4 || argc > 6) return 2;
    if (mkdir(argv[3], 0700) != 0 && errno != EEXIST) {
        perror("create user directory");
        return 1;
    }
    DIR *user = opendir(argv[3]);
    if (!user) { perror("open user directory"); return 1; }
    errno = 0;
    struct dirent *entry;
    while ((entry = readdir(user))) {
        if (strcmp(entry->d_name, ".") && strcmp(entry->d_name, "..")) {
            fprintf(stderr, "user directory must be fresh and empty\n");
            closedir(user);
            return 1;
        }
    }
    int read_error = errno;
    if (closedir(user) != 0 || read_error || access(argv[3], W_OK | X_OK) != 0) {
        fprintf(stderr, "user directory must be readable and writable\n");
        return 1;
    }
    void *so = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!so) { fprintf(stderr, "%s\n", dlerror()); return 1; }
    void *(*init)(const char *, const char *) = symbol(so, "pinyin_init");
    void (*fini)(void *) = symbol(so, "pinyin_fini");
    if (argc == 6 || (argc == 5 && strcmp(argv[4], "--export-system") == 0)) {
        void *ctx = init(argv[2], argv[3]);
        if (!ctx) return 1;
        int result = 2;
        if (argc == 5) {
            bool (*load)(void *, uint8_t) = symbol(so, "pinyin_load_phrase_library");
            for (unsigned lib = 1; lib <= 4; ++lib) {
                (void)load(ctx, (uint8_t)lib);
                export_walk(so, ctx, lib, true);
            }
            result = 0;
        } else if (!strcmp(argv[4], "--sentence-fixture") || !strcmp(argv[4], "--train-fixture")) {
            result = sentence_fixture(so, ctx, argv[5], !strcmp(argv[4], "--train-fixture"));
        } else if (!strcmp(argv[4], "--phrase-file")) {
            result = phrase_file(so, ctx, argv[5]);
        }
        fini(ctx); dlclose(so); return result;
    }
    if (argc == 5 && strcmp(argv[4], "--cold-open") == 0) {
        bool (*options)(void *, uint32_t) = symbol(so, "pinyin_set_options");
        void *(*alloc)(void *) = symbol(so, "pinyin_alloc_instance");
        void (*free_instance)(void *) = symbol(so, "pinyin_free_instance");
        void *ctx = init(argv[2], argv[3]);
        if (!ctx) return 1;
        if (!options(ctx, 0x18a)) { fini(ctx); return 1; }
        void *instance = alloc(ctx);
        if (!instance) { fini(ctx); return 1; }
        free_instance(instance);
        fini(ctx);
        dlclose(so);
        return 0;
    }
    void *(*begin)(void *, uint8_t) = symbol(so, "pinyin_begin_add_phrases");
    bool (*add)(void *, const char *, const char *, int32_t) = symbol(so, "pinyin_iterator_add_phrase");
    void (*end)(void *) = symbol(so, "pinyin_end_add_phrases");
    bool (*save)(void *) = symbol(so, "pinyin_save");
    unsigned iterations = argc == 5 ? (unsigned)strtoul(argv[4], NULL, 10) : 1000;
    void *ctx = init(argv[2], argv[3]);
    if (!ctx) return 1;
    CALLGRIND_DUMP_STATS_AT("opened");
    for (unsigned lib = 1; lib <= 4; ++lib) {
        void *iter = begin(ctx, (uint8_t)lib);
        for (unsigned i = 0; i < iterations; ++i) {
            if (!add(iter, "测试", "ce4'shi4", 5)
                || !add(iter, "测侧", "ce4'ce4", 5)) return 1;
        }
        end(iter);
    }
    CALLGRIND_DUMP_STATS_AT("imported");
    if (!save(ctx)) return 1;
    CALLGRIND_DUMP_STATS_AT("saved");
    fini(ctx);
    dlclose(so);
    return 0;
}
