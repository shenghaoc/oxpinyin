/* V3 probe: one (lib, api, input, offset) call per process, so an abort is
 * a datum rather than a lost run. Opaque out-structs get a generous aligned
 * buffer; only the boolean return and first bytes are reported. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <dlfcn.h>
#include <stdbool.h>
#include "pinyin.h"

int main(int argc, char **argv) {
    setvbuf(stdout, NULL, _IONBF, 0);
    if (argc != 6) { fprintf(stderr, "usage: drv LIB API INPUT OFFSET SYSDIR\n"); return 2; }
    const char *lib = argv[1], *api = argv[2], *input = argv[3], *sysdir = argv[5];
    size_t offset = (size_t)strtoul(argv[4], NULL, 10);

    void *h = dlopen(lib, RTLD_NOW);
    if (!h) { fprintf(stderr, "dlopen: %s\n", dlerror()); return 3; }
    pinyin_context_t *(*init)(const char *, const char *) = dlsym(h, "pinyin_init");
    pinyin_instance_t *(*alloc)(pinyin_context_t *) = dlsym(h, "pinyin_alloc_instance");
    size_t (*parse)(pinyin_instance_t *, const char *) = dlsym(h, "pinyin_parse_more_full_pinyins");
    bool (*keyf)(pinyin_instance_t *, size_t, ChewingKey **) = dlsym(h, "pinyin_get_pinyin_key");
    bool (*keyrestf)(pinyin_instance_t *, size_t, ChewingKeyRest **) = dlsym(h, "pinyin_get_pinyin_key_rest");
    bool (*charoff)(pinyin_instance_t *, const char *, size_t, size_t *) = dlsym(h, "pinyin_get_character_offset");
    bool (*rightoff)(pinyin_instance_t *, size_t, size_t *) = dlsym(h, "pinyin_get_right_pinyin_offset");
    bool (*guess)(pinyin_instance_t *, size_t, guint) = dlsym(h, "pinyin_guess_candidates");
    guint (*candnum)(pinyin_instance_t *) = dlsym(h, "pinyin_get_candidate_number");
    void (*freeinst)(pinyin_instance_t *) = dlsym(h, "pinyin_free_instance");
    int (*exit_)(pinyin_context_t *) = dlsym(h, "pinyin_exit");
    if (!init || !alloc || !parse) { fprintf(stderr, "core syms missing\n"); return 3; }
    if (!keyf || !keyrestf || !charoff || !rightoff || !guess) { fprintf(stderr, "api syms missing\n"); return 3; }

    char userdir[] = "/tmp/drv-user-XXXXXX";
    if (!mkdtemp(userdir)) { perror("mkdtemp"); return 3; }
    pinyin_context_t *ctx = init(sysdir, userdir);
    if (!ctx) { printf("init=FAIL\n"); return 0; } fprintf(stderr, "[dbg] init ok\n");
    pinyin_instance_t *inst = alloc(ctx); fprintf(stderr, "[dbg] alloc ok\n");
    if (!inst) { printf("alloc=FAIL\n"); return 0; }
    size_t parsed = parse(inst, input); fprintf(stderr, "[dbg] parse ok %zu\n", parsed);

    _Alignas(16) unsigned char buf[64];
    size_t out = (size_t)-1;
    bool r = false;
    ChewingKey *kout = NULL; ChewingKeyRest *rout = NULL;
    if (!strcmp(api, "key")) {
        r = keyf(inst, offset, &kout);
        if (r && kout) printf("parsed=%zu ret=%d key=%02x%02x%02x\n", parsed, r, ((unsigned char*)kout)[0], ((unsigned char*)kout)[1], ((unsigned char*)kout)[2]);
        else printf("parsed=%zu ret=%d key=NULL\n", parsed, r);
    } else if (!strcmp(api, "keyrest")) {
        r = keyrestf(inst, offset, &rout);
        if (r && rout) printf("parsed=%zu ret=%d rest=%02x%02x%02x%02x\n", parsed, r, ((unsigned char*)rout)[0], ((unsigned char*)rout)[1], ((unsigned char*)rout)[2], ((unsigned char*)rout)[3]);
        else printf("parsed=%zu ret=%d rest=NULL\n", parsed, r);
    } else if (!strcmp(api, "charoff")) {
        r = charoff(inst, input, offset, &out);
        printf("parsed=%zu ret=%d out=%zu\n", parsed, r, out);
    } else if (!strcmp(api, "rightoff")) {
        r = rightoff(inst, offset, &out);
        printf("parsed=%zu ret=%d out=%zu\n", parsed, r, out);
    } else if (!strcmp(api, "guess")) {
        r = guess(inst, offset, 0);
        guint n = candnum ? candnum(inst) : 0;
        printf("parsed=%zu ret=%d cands=%u\n", parsed, r, n);
    } else { fprintf(stderr, "unknown api %s\n", api); return 2; }

    if (freeinst) freeinst(inst);
    if (exit_) exit_(ctx);
    fflush(NULL);
    return 0;
}
