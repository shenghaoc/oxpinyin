/* #599 reproducible callgrind/massif workload. No wall-clock timing.
 * Usage: <libpinyin.so> <system-data-dir> <fresh-user-dir> [iterations]
 * Every iteration merges one original item and one appended item in each
 * SYSTEM_FILE library; save writes all four .dbin streams. */
#include <dlfcn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <valgrind/callgrind.h>

static void *symbol(void *so, const char *name) {
    void *fn = dlsym(so, name);
    if (!fn) { fprintf(stderr, "missing %s\n", name); exit(1); }
    return fn;
}
int main(int argc, char **argv) {
    if (argc < 4 || argc > 5) return 2;
    void *so = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!so) { fprintf(stderr, "%s\n", dlerror()); return 1; }
    void *(*init)(const char *, const char *) = symbol(so, "pinyin_init");
    void (*fini)(void *) = symbol(so, "pinyin_fini");
    void *(*begin)(void *, uint8_t) = symbol(so, "pinyin_begin_add_phrases");
    bool (*add)(void *, const char *, const char *, int32_t) = symbol(so, "pinyin_iterator_add_phrase");
    void (*end)(void *) = symbol(so, "pinyin_end_add_phrases");
    bool (*save)(void *) = symbol(so, "pinyin_save");
    unsigned iterations = argc == 5 ? (unsigned)strtoul(argv[4], NULL, 10) : 1000;
    mkdir(argv[3], 0700);
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
