/* Session-parse scaling workload (#1501). One pinyin_parse_more_full_pinyins
 * call on a fresh instance, over an input generated in memory, so lengths
 * past argv's practical size need no file. `bisect_parse_unit` is the
 * Callgrind anchor; the clock brackets the same call. Instance allocation,
 * input construction, the observation and cleanup stay outside it.
 *
 * Shapes, each exactly LENGTH bytes:
 *   ni         "ni" repeated, truncated to LENGTH;
 *   apostrophe "ni", then one run of apostrophes, then "ni" (LENGTH >= 4),
 *              the input whose graph build was quadratic before c8f4ed26.
 *
 * Built with -DBISECT_HEAP_COUNTER (and -rdynamic, so the dlopened library
 * and its dependencies bind to these definitions), the harness interposes
 * glibc's allocator and reports the peak of live heap bytes during the
 * timed call, relative to the live bytes at its start. Blocks are counted
 * by malloc_usable_size. The default harness build has no interposer, and
 * no other bisect mode changes. */

#if defined(BISECT_HEAP_COUNTER)
#if !defined(__GLIBC__)
#error "BISECT_HEAP_COUNTER interposes glibc's allocator"
#endif
extern void *__libc_malloc(size_t size);
extern void *__libc_calloc(size_t count, size_t size);
extern void *__libc_realloc(void *pointer, size_t size);
extern void __libc_free(void *pointer);
extern void *__libc_memalign(size_t alignment, size_t size);

static bool heap_counting;
static long long heap_live;
static long long heap_peak;

static void heap_add(void *pointer) {
    if (pointer && heap_counting) {
        heap_live += (long long)malloc_usable_size(pointer);
        if (heap_live > heap_peak)
            heap_peak = heap_live;
    }
}

static void heap_sub(void *pointer) {
    if (pointer && heap_counting)
        heap_live -= (long long)malloc_usable_size(pointer);
}

void *malloc(size_t size) {
    void *pointer = __libc_malloc(size);
    heap_add(pointer);
    return pointer;
}

void *calloc(size_t count, size_t size) {
    void *pointer = __libc_calloc(count, size);
    heap_add(pointer);
    return pointer;
}

void *realloc(void *pointer, size_t size) {
    heap_sub(pointer);
    void *moved = __libc_realloc(pointer, size);
    /* A failed resize keeps the old block; realloc(p, 0) frees it. */
    heap_add(moved ? moved : (size ? pointer : NULL));
    return moved;
}

void free(void *pointer) {
    heap_sub(pointer);
    __libc_free(pointer);
}

void *memalign(size_t alignment, size_t size) {
    void *pointer = __libc_memalign(alignment, size);
    heap_add(pointer);
    return pointer;
}

void *aligned_alloc(size_t alignment, size_t size) {
    return memalign(alignment, size);
}

int posix_memalign(void **out, size_t alignment, size_t size) {
    if (!alignment || alignment % sizeof(void *) || (alignment & (alignment - 1)))
        return EINVAL;
    void *pointer = memalign(alignment, size);
    if (!pointer && size)
        return ENOMEM;
    *out = pointer;
    return 0;
}
#define HEAP_COUNTER_BUILT true
#else
static bool heap_counting;
static long long heap_live;
static long long heap_peak;
#define HEAP_COUNTER_BUILT false
#endif

__attribute__((noinline)) size_t
bisect_parse_unit(const struct symbols *s, pinyin_instance_t *inst,
                  const char *input) {
    return s->parse_full(inst, input);
}

/* Fill TEXT with LENGTH bytes of SHAPE and terminate it; false for an
 * unknown shape or a length the shape cannot hold. */
static bool parse_perf_input(char *text, size_t length, const char *shape) {
    if (strcmp(shape, "ni") == 0) {
        for (size_t i = 0; i < length; ++i)
            text[i] = i % 2 ? 'i' : 'n';
    } else if (strcmp(shape, "apostrophe") == 0) {
        if (length < 4)
            return false;
        memset(text, '\'', length);
        memcpy(text, "ni", 2);
        memcpy(text + length - 2, "ni", 2);
    } else {
        return false;
    }
    text[length] = '\0';
    return true;
}

static int run_parse_perf_mode(int argc, char **argv) {
    if (argc != 6) {
        fprintf(stderr, "Usage: %s --perf-parse SO DATA ni|apostrophe LENGTH\n", argv[0]);
        return 2;
    }
    const char *shape = argv[4];
    if (!*argv[5])
        return 2;
    for (const char *p = argv[5]; *p; ++p)
        if (*p < '0' || *p > '9')
            return 2;
    errno = 0;
    unsigned long long requested = strtoull(argv[5], NULL, 10);
    /* 1 GiB keeps the generated input far below any position width. */
    if (errno || requested < 1 || requested > (1ULL << 30))
        return 2;
    size_t length = (size_t)requested;
    char *text = malloc(length + 1);
    if (!text || !parse_perf_input(text, length, shape)) {
        free(text);
        return 2;
    }
    int status = 1;
    char user_dir[4096];
    void *handle = NULL;
    pinyin_context_t *ctx = NULL;
    pinyin_instance_t *inst = NULL;
    struct symbols s;
    if (!user_dir_template(user_dir, sizeof user_dir, "bisect-parse") || !mkdtemp(user_dir)) {
        free(text);
        return 1;
    }
    handle = dlopen(argv[2], RTLD_NOW);
    if (!handle) {
        fprintf(stderr, "dlopen: %s\n", dlerror());
        goto cleanup;
    }
    int missing = resolve_perf_symbols(handle, &s, false);
    RESOLVE(handle, s, get_parsed_input_length, "pinyin_get_parsed_input_length");
    if (missing)
        goto cleanup;
    ctx = s.init(argv[3], user_dir);
    if (!ctx || !s.set_options(ctx, DEFAULT_FLAGS))
        goto cleanup;
    inst = s.alloc_instance(ctx);
    if (!inst)
        goto cleanup;
    heap_live = heap_peak = 0;
    heap_counting = HEAP_COUNTER_BUILT;
    uint64_t start = now_ns();
    size_t parsed = bisect_parse_unit(&s, inst, text);
    uint64_t end = now_ns();
    heap_counting = false;
    if (!start || end < start)
        goto cleanup;
    size_t stored = s.get_parsed_input_length(inst);
    printf("{\"pid\":%ld,\"shape\":\"%s\",\"length\":%zu,\"parsed\":%zu,\"stored\":%zu,"
           "\"elapsed_ns\":%llu,\"heap_peak_bytes\":",
           (long)getpid(), shape, length, parsed, stored, (unsigned long long)(end - start));
    if (HEAP_COUNTER_BUILT)
        printf("%lld}\n", heap_peak);
    else
        fputs("null}\n", stdout);
    status = ferror(stdout) ? 1 : 0;
cleanup:
    if (inst)
        s.free_instance(inst);
    if (ctx)
        s.fini(ctx);
    if (handle)
        dlclose(handle);
    perf_remove_user_dir(user_dir);
    free(text);
    if (status)
        fputs("parse workload failed\n", stderr);
    return status;
}
