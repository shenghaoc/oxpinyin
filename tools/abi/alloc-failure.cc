// Linux allocation-boundary audit, driven through the public C ABI.
// Usage: alloc-failure SO DATA pinyin|zhuyin pin|subject USER_DIR [array-only]
// Expectations: pin 074a2219 storage/chewing_key.cpp:47-121,
// storage/zhuyin_parser2.cpp:301-332, pinyin.cpp:715-733, :896-911,
// :2751-2762, :2801-2818 and zhuyin.cpp:1793-1810; GLib glib/gmem.c
// g_malloc/g_realloc (g_error on a failed positive-size allocation) and
// glib/gprintf.c g_vasprintf (on ENOMEM it writes G_STRLOC and ": failed to
// allocate memory" to stderr and calls abort(), with no g_log).
// Both sides allocate a returned string with the pin's GLib call (#1227), so
// both are judged by one rule: the nth malloc whose caller is g_malloc (or
// realloc from g_realloc, or, for the formatted renderer, the malloc libc's
// vasprintf makes) fails, and the process must die as GLib makes it die.
// Nothing is read from the library's symbols or line tables.
#include <glib.h>
#include <dlfcn.h>
#include <sys/wait.h>
#include <sys/resource.h>
#include <unistd.h>
#include <cassert>
#include <cerrno>
#include <csignal>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <initializer_list>

// Test-only glibc interposition bypass; no shipping library uses these.
extern "C" void *__libc_malloc(size_t);
extern "C" void *__libc_realloc(void *, size_t);
namespace {
enum Target { GLIB_MALLOC = 1, GLIB_REALLOC, LIBC_MALLOC };
int target, fail_at = 1, seen, channel = -1;
bool armed, inside;
bool glib_caller(void *pc, const char *name) {
    Dl_info info{};
    return dladdr(pc, &info) && info.dli_sname && !strcmp(info.dli_sname, name);
}
// A malloc made inside libc itself: vasprintf's, under g_strdup_printf.
bool libc_caller(void *pc) {
    Dl_info info{};
    if (!dladdr(pc, &info) || !info.dli_fname) return false;
    const char *base = strrchr(info.dli_fname, '/');
    return !strncmp(base ? base + 1 : info.dli_fname, "libc.so", 7);
}
void marker(char c) { if (channel >= 0) { auto ignored = write(channel, &c, 1); (void)ignored; } }
GLogWriterOutput writer(GLogLevelFlags level, const GLogField *, gsize, gpointer) {
    if (level & G_LOG_LEVEL_ERROR) marker('E');
    return G_LOG_WRITER_HANDLED;
}
void log_handler(const gchar *, GLogLevelFlags level, const gchar *, gpointer) {
    if (level & G_LOG_LEVEL_ERROR) marker('E');
}
}
extern "C" void *malloc(size_t n) noexcept {
    void *caller = __builtin_return_address(0);
    bool fail = false;
    if (armed && !inside) {
        inside = true;
        fail = (target == GLIB_MALLOC && glib_caller(caller, "g_malloc")) ||
               (target == LIBC_MALLOC && libc_caller(caller));
        if (fail) { ++seen; fail = seen == fail_at; }
        inside = false;
    }
#ifndef OXPINYIN_ALLOC_FAILURE_DISABLED
    // A failed malloc sets ENOMEM, which g_vasprintf tests.
    if (fail) { armed = false; marker('F'); errno = ENOMEM; return nullptr; }
#endif
    return __libc_malloc(n);
}
extern "C" void *realloc(void *p, size_t n) noexcept {
    bool fail = false;
    if (armed && !inside && target == GLIB_REALLOC) {
        inside = true;
        fail = glib_caller(__builtin_return_address(0), "g_realloc");
        if (fail) { ++seen; fail = seen == fail_at; }
        inside = false;
    }
#ifndef OXPINYIN_ALLOC_FAILURE_DISABLED
    if (fail) { armed = false; marker('F'); errno = ENOMEM; return nullptr; }
#endif
    return __libc_realloc(p, n);
}

template<class T> T symbol(void *lib, const char *mode, const char *name) {
    char full[128]; snprintf(full, sizeof full, "%s_%s", mode, name);
    void *p = dlsym(lib, full); assert(p); return reinterpret_cast<T>(p);
}
int main(int argc, char **argv) {
    assert(argc == 6 || (argc == 7 && !strcmp(argv[6], "array-only")));
    // so, data, facade, pin|subject, user directory (subject only)
    const bool array_only = argc == 7;
    const bool pin = !strcmp(argv[4], "pin"), pinyin = !strcmp(argv[3], "pinyin");
    void *lib = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL); assert(lib);
    auto init = symbol<void *(*)(const char *, const char *)>(lib, argv[3], "init");
    auto alloc = symbol<void *(*)(void *)>(lib, argv[3], "alloc_instance");
    auto parse = symbol<size_t (*)(void *, const char *)>(lib, argv[3], "parse_more_full_pinyins");
    auto key_get = symbol<bool (*)(void *, unsigned, void **)>(lib, argv[3], pinyin ? "get_pinyin_key" : "get_zhuyin_key");
    auto render = symbol<bool (*)(void *, void *, char **)>(lib, argv[3], "get_zhuyin_string");
    auto keyboard = symbol<bool (*)(void *, char, char ***)>(lib, argv[3], "in_chewing_keyboard");
    auto lookup = symbol<bool (*)(void *, const char *, GArray *)>(lib, argv[3], "lookup_tokens");
    auto nth = symbol<bool (*)(void *, unsigned, unsigned, GArray *)>(lib, argv[3], "token_get_nth_pronunciation");
    auto free_inst = symbol<void (*)(void *)>(lib, argv[3], "free_instance");
    auto fini = symbol<void (*)(void *)>(lib, argv[3], "fini");
    void *ctx = init(argv[2], pin ? nullptr : argv[5]); assert(ctx);
    void *inst = alloc(ctx); assert(inst); assert(parse(inst, "ni1hao3") > 0);
    // Normal controls. The key getters hand out one static slot, so an
    // operation that renders another key fetches it in its own child.
    void *key = nullptr; assert(key_get(inst, 3, &key) && key);
    char *text = nullptr; assert(render(inst, key, &text) && text);
    assert(!strcmp(text, "ㄏㄠˇ")); g_free(text); // tone 3: g_strdup_printf("%s%s", ...)
    assert(key_get(inst, 0, &key) && key);
    assert(render(inst, key, &text) && text);
    assert(!strcmp(text, "ㄋㄧ")); g_free(text); // tone 1: g_strdup
    auto scheme = symbol<bool (*)(void *, int)>(lib, argv[3], pinyin ? "set_zhuyin_scheme" : "set_chewing_scheme");
    assert(scheme(ctx, 2)); // ZHUYIN_HSU, table.h:209-232: c -> ㄒ, ㄕ.
    const char keyboard_key = 'c';
    char **control = nullptr;
    assert(keyboard(inst, keyboard_key, &control));
    assert(control && control[0] && control[1] && !control[2]);
    assert(!strcmp(control[0], "ㄒ") && !strcmp(control[1], "ㄕ"));
    g_strfreev(control);
    GArray *tokens = g_array_new(FALSE, FALSE, sizeof(unsigned));
    assert(lookup(inst, "你好", tokens) && tokens->len);
    unsigned token = g_array_index(tokens, unsigned, 0); g_array_free(tokens, TRUE);
    // One operation in this (forked) process: its setup, then the call under
    // test with the targeted allocation failed. It returns only if the
    // process survived, which fails the operation.
    auto run = [&](int operation) {
        rlimit core{0, 0}; setrlimit(RLIMIT_CORE, &core);
        g_log_set_writer_func(writer, nullptr, nullptr);
        g_log_set_handler("GLib", static_cast<GLogLevelFlags>(0xFFFFFFFC), log_handler, nullptr);
        g_log_set_handler(nullptr, static_cast<GLogLevelFlags>(0xFFFFFFFC), log_handler, nullptr);
        g_log_set_handler(pinyin ? "libpinyin" : "libzhuyin", static_cast<GLogLevelFlags>(0xFFFFFFFC), log_handler, nullptr);
        GArray *keys = g_array_new(FALSE, FALSE, sizeof(uint16_t));
        void *iterator = nullptr;
        auto next = static_cast<bool (*)(void *, char **, char **, int *)>(nullptr);
        if (operation == 9) assert(key_get(inst, 3, &key) && key);
        if (operation == 6) {
            auto begin = symbol<void *(*)(void *, unsigned)>(lib, argv[3], "begin_get_phrases");
            iterator = begin(ctx, token >> 24);
            auto has = symbol<bool (*)(void *)>(lib, argv[3], "iterator_has_next_phrase");
            assert(iterator && has(iterator));
            next = symbol<bool (*)(void *, char **, char **, int *)>(lib, argv[3], "iterator_get_next_phrase");
        } else if (operation == 7) {
            // Seed a subject-only user bigram through the public API.
            auto guess = symbol<bool (*)(void *)>(lib, argv[3], "guess_sentence");
            auto candidates = symbol<bool (*)(void *, size_t, unsigned)>(lib, argv[3], "guess_candidates");
            auto number = symbol<bool (*)(void *, unsigned *)>(lib, argv[3], "get_n_candidate");
            auto candidate = symbol<bool (*)(void *, unsigned, void **)>(lib, argv[3], "get_candidate");
            auto string = symbol<bool (*)(void *, void *, const char **)>(lib, argv[3], "get_candidate_string");
            auto choose = symbol<int (*)(void *, size_t, void *)>(lib, argv[3], "choose_candidate");
            assert(parse(inst, "nihao") > 0);
            assert(guess(inst));
            size_t cursor = 0;
            for (const char *wanted : {"你", "好"}) {
                assert(candidates(inst, cursor, 0x1e));
                unsigned n = 0; assert(number(inst, &n));
                bool found = false;
                for (unsigned i = 0; i < n; ++i) {
                    void *item = nullptr; const char *value = nullptr;
                    assert(candidate(inst, i, &item) && string(inst, item, &value));
                    if (!strcmp(value, wanted)) { cursor = choose(inst, cursor, item); found = true; break; }
                }
                assert(found);
            }
            // Refresh the decoded sentence after both forcing choices.
            assert(guess(inst));
            auto train = symbol<bool (*)(void *, unsigned)>(lib, argv[3], "train");
            assert(train(inst, 0));
            auto begin = symbol<void *(*)(void *)>(lib, argv[3], "begin_get_bigram_phrases");
            iterator = begin(ctx);
            auto has = symbol<bool (*)(void *)>(lib, argv[3], "bigram_iterator_has_next_phrase");
            assert(iterator && has(iterator));
            next = symbol<bool (*)(void *, char **, char **, int *)>(lib, argv[3], "bigram_iterator_get_next_phrase");
        }
        // Operation 5 fails GArray growth (g_realloc) and 9 the formatted
        // renderer's vasprintf malloc; the rest fail the nth g_malloc of the
        // call: for the keyboard the array (2), first string (3) and second
        // string (4); for the iterators the joined reading (6) and the
        // duplicated phrase (7); the shengmu (8).
        target = operation == 5 ? GLIB_REALLOC : operation == 9 ? LIBC_MALLOC : GLIB_MALLOC;
        fail_at = operation == 3 ? 2 : operation == 4 ? 3 : 1;
        seen = 0;
        armed = true;
        char *out = nullptr; char **list = nullptr; char *reading = nullptr; int frequency = 0;
        if (operation == 1 || operation == 9) render(inst, key, &out);
        else if (operation < 5) keyboard(inst, keyboard_key, &list);
        else if (operation == 5) nth(inst, token, 0, keys);
        else if (operation < 8) next(iterator, &out, &reading, &frequency);
        else symbol<bool (*)(void *, void *, char **, char **)>(lib, argv[3], "get_pinyin_strings")(inst, key, &out, &reading);
        armed = false;
        marker('R');
        return 1;
    };
    // Operations 6-8 reach the export iterators and the shengmu/yunmu
    // renderer of the pinyin facade; 7 seeds a user bigram that only the
    // subject's user directory keeps.
    auto applies = [&](int operation) {
        if (array_only) return operation == 5;
        if (operation >= 6 && operation <= 8) return pinyin && (!pin || operation != 7);
        return true;
    };
    int failures = 0;
    for (int operation = 1; operation <= 9; ++operation) {
        if (!applies(operation)) continue;
        int fd[2]; assert(pipe(fd) == 0);
        int err[2]; assert(pipe(err) == 0);
        pid_t child = fork(); assert(child >= 0);
        if (!child) {
            close(fd[0]); close(err[0]); channel = fd[1];
            if (operation == 9) dup2(err[1], STDERR_FILENO);
            close(err[1]);
            _exit(run(operation));
        }
        close(fd[1]); close(err[1]);
        // Read until EOF: markers may arrive in separate writes before fatal exit.
        char evidence[32]{}; ssize_t count = 0;
        for (ssize_t n; count < 32 && (n = read(fd[0], evidence + count, sizeof evidence - count)) > 0;) count += n;
        char diagnostic[512]{}; ssize_t length = 0;
        for (ssize_t n; length < 511 && (n = read(err[0], diagnostic + length, 511 - length)) > 0;) length += n;
        close(fd[0]); close(err[0]); int status; assert(waitpid(child, &status, 0) == child);
        bool ok;
        if (operation == 9) {
            // g_vasprintf writes its own message and calls abort(); no g_log.
            ok = count == 1 && evidence[0] == 'F' && strstr(diagnostic, "gprintf.c:") &&
                 strstr(diagnostic, ": failed to allocate memory\n") &&
                 WIFSIGNALED(status) && WTERMSIG(status) == SIGABRT;
        } else {
            ok = count == 2 && evidence[0] == 'F' && evidence[1] == 'E' && WIFSIGNALED(status) &&
                 (WTERMSIG(status) == SIGTRAP || WTERMSIG(status) == SIGABRT);
        }
        printf("%s %s allocation-%d: %s (%s)\n", argv[3], argv[4], operation,
               ok ? "PASS" : "FAIL", operation == 9 ? "fatal GLib printf failure" : "fatal GLib error");
        if (!ok) ++failures;
        if (!ok) fprintf(stderr, "allocation-%d: child-status=%d markers=%.*s stderr=%.*s\n", operation, status,
                         static_cast<int>(count), evidence, static_cast<int>(length), diagnostic);
    }
    free_inst(inst); fini(ctx); dlclose(lib);
    return failures ? 1 : 0;
}
