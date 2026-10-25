// Linux allocation-boundary audit, driven through the public C ABI.
// Expectations: pin 074a2219 storage/chewing_key.cpp:74-88,
// storage/zhuyin_parser2.cpp:301-332, pinyin.cpp:2801-2818 and
// zhuyin.cpp:1793-1810; GLib 2.89.4 glib/gmem.c:95-106,164-177.
// A symbol-sized address range targets the final adapter malloc, not an
// earlier Rust allocation. Stripped/optimized-away symbols are an error.
#include <glib.h>
#include <dlfcn.h>
#include <sys/wait.h>
#include <sys/resource.h>
#include <unistd.h>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>

// Test-only glibc interposition bypass; no shipping library uses these.
extern "C" void *__libc_malloc(size_t);
extern "C" void *__libc_realloc(void *, size_t);
extern "C" void __libc_free(void *);
namespace {
uintptr_t str_lo, str_hi, list_lo, list_hi;
int target, fail_at = 1, seen, fired, channel = -1, diagnostics, warnings;
bool armed, inside;
void *tracked[32];
unsigned tracked_count, freed_count;
bool in(uintptr_t pc, uintptr_t lo, uintptr_t hi) { return pc >= lo && pc < hi; }
bool glib_caller(void *pc, const char *name) {
    Dl_info info{};
    return dladdr(pc, &info) && info.dli_sname && !strcmp(info.dli_sname, name);
}
void marker(char c) { if (channel >= 0) { auto ignored = write(channel, &c, 1); (void)ignored; } }
GLogWriterOutput writer(GLogLevelFlags level, const GLogField *, gsize, gpointer) {
    ++diagnostics;
    if (level & G_LOG_LEVEL_WARNING) ++warnings;
    if (level & G_LOG_LEVEL_ERROR) marker('E');
    return G_LOG_WRITER_HANDLED;
}
void log_handler(const gchar *, GLogLevelFlags level, const gchar *, gpointer) {
    ++diagnostics;
    if (level & G_LOG_LEVEL_WARNING) ++warnings;
    if (level & G_LOG_LEVEL_ERROR) marker('E');
}
}
extern "C" void *malloc(size_t n) noexcept {
    void *caller = __builtin_return_address(0);
    bool relevant = false, fail = false;
    if (armed && !inside) {
        inside = true;
        auto pc = reinterpret_cast<uintptr_t>(caller);
        bool string = in(pc, str_lo, str_hi), list = in(pc, list_lo, list_hi);
        relevant = string || list;
        fail = (target == 1 && string) || (target == 2 && list) ||
               (target == 3 && string) ||
               (target == 4 && glib_caller(caller, "g_malloc"));
        if (fail) { ++seen; fail = seen == fail_at; }
        inside = false;
    }
#ifndef OXPINYIN_ALLOC_FAILURE_DISABLED
    if (fail) { ++fired; armed = false; marker('F'); return nullptr; }
#endif
    void *p = __libc_malloc(n);
    if (relevant && p && tracked_count < 32) tracked[tracked_count++] = p;
    return p;
}
extern "C" void *realloc(void *p, size_t n) noexcept {
    bool fail = false;
    if (armed && !inside && target == 5) {
        inside = true;
        fail = glib_caller(__builtin_return_address(0), "g_realloc");
        if (fail) { ++seen; fail = seen == fail_at; }
        inside = false;
    }
#ifndef OXPINYIN_ALLOC_FAILURE_DISABLED
    if (fail) { ++fired; armed = false; marker('F'); return nullptr; }
#endif
    return __libc_realloc(p, n);
}
extern "C" void free(void *p) noexcept {
    if (p) for (unsigned i = 0; i < tracked_count; ++i)
        if (tracked[i] == p) { tracked[i] = nullptr; ++freed_count; break; }
    __libc_free(p);
}

template<class T> T symbol(void *lib, const char *mode, const char *name) {
    char full[128]; snprintf(full, sizeof full, "%s_%s", mode, name);
    void *p = dlsym(lib, full); assert(p); return reinterpret_cast<T>(p);
}
int main(int argc, char **argv) {
    assert(argc == 9 || (argc == 10 && !strcmp(argv[9], "array-only")));
    // so, data, facade, pin|subject, str offset/size, list offset/size
    const bool array_only = argc == 10;
    const bool pin = !strcmp(argv[4], "pin"), pinyin = !strcmp(argv[3], "pinyin");
    void *lib = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL); assert(lib);
    auto init = symbol<void *(*)(const char *, const char *)>(lib, argv[3], "init");
    Dl_info info{}; assert(dladdr(reinterpret_cast<void *>(init), &info));
    auto base = reinterpret_cast<uintptr_t>(info.dli_fbase);
    str_lo = base + strtoull(argv[5], nullptr, 16); str_hi = str_lo + strtoull(argv[6], nullptr, 16);
    list_lo = base + strtoull(argv[7], nullptr, 16); list_hi = list_lo + strtoull(argv[8], nullptr, 16);
    auto alloc = symbol<void *(*)(void *)>(lib, argv[3], "alloc_instance");
    auto parse = symbol<size_t (*)(void *, const char *)>(lib, argv[3], "parse_more_full_pinyins");
    auto key_get = symbol<bool (*)(void *, unsigned, void **)>(lib, argv[3], pinyin ? "get_pinyin_key" : "get_zhuyin_key");
    auto render = symbol<bool (*)(void *, void *, char **)>(lib, argv[3], "get_zhuyin_string");
    auto keyboard = symbol<bool (*)(void *, char, char ***)>(lib, argv[3], "in_chewing_keyboard");
    auto lookup = symbol<bool (*)(void *, const char *, GArray *)>(lib, argv[3], "lookup_tokens");
    auto nth = symbol<bool (*)(void *, unsigned, unsigned, GArray *)>(lib, argv[3], "token_get_nth_pronunciation");
    auto free_inst = symbol<void (*)(void *)>(lib, argv[3], "free_instance");
    auto fini = symbol<void (*)(void *)>(lib, argv[3], "fini");
    void *ctx = init(argv[2], nullptr); assert(ctx);
    void *inst = alloc(ctx); assert(inst); assert(parse(inst, "ni1hao3") > 0);
    void *key = nullptr; assert(key_get(inst, 0, &key) && key);
    char *text = nullptr; assert(render(inst, key, &text) && text); g_free(text);
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
    int failures = 0;
    for (int operation = array_only ? 5 : 1; operation <= 5; ++operation) {
        // pin vector and element probes share its fatal strdup allocation;
        // subject distinguishes vector malloc from first-element malloc.
        int fd[2]; assert(pipe(fd) == 0); pid_t child = fork(); assert(child >= 0);
        if (!child) {
            close(fd[0]); channel = fd[1];
            rlimit core{0, 0}; setrlimit(RLIMIT_CORE, &core);
            g_log_set_writer_func(writer, nullptr, nullptr);
            g_log_set_handler("GLib", static_cast<GLogLevelFlags>(0xFFFFFFFC), log_handler, nullptr);
            g_log_set_handler(nullptr, static_cast<GLogLevelFlags>(0xFFFFFFFC), log_handler, nullptr);
            g_log_set_handler(pinyin ? "libpinyin" : "libzhuyin", static_cast<GLogLevelFlags>(0xFFFFFFFC), log_handler, nullptr);
            GArray *keys = g_array_new(FALSE, FALSE, sizeof(uint16_t));
            target = operation == 5 ? 5 : pin ? 4 : operation == 4 ? 3 : operation;
            fail_at = operation == 4 ? 2 : 1;
            seen = fired = diagnostics = warnings = 0; tracked_count = freed_count = 0; armed = true;
            bool ret; char *out = reinterpret_cast<char *>(1); char **list = reinterpret_cast<char **>(1);
            if (operation == 1) ret = render(inst, key, &out);
            else if (operation < 5) ret = keyboard(inst, keyboard_key, &list);
            else ret = nth(inst, token, 0, keys);
            armed = false;
            // Class (c): exactly one warning and failure, while retaining
            // the partial-vector cleanup checks and fatal GArray control.
            bool valid = !pin && operation < 5 && fired == 1 && seen == fail_at && diagnostics == 1 && warnings == 1;
            if (operation == 1) valid = valid && out == nullptr && !ret;
            else if (operation < 5) valid = valid && list == nullptr && !ret && tracked_count == freed_count;
            if (operation == 4) valid = valid && tracked_count == 2 && freed_count == 2;
            if (valid) marker('R');
            _exit(valid ? 0 : 1);
        }
        close(fd[1]); char evidence[32]{}; ssize_t count = read(fd[0], evidence, sizeof evidence);
        // Read until EOF: markers may arrive in separate writes before fatal exit.
        while (count >= 0 && count < 32) { ssize_t n = read(fd[0], evidence + count, sizeof evidence - count); if (n <= 0) break; count += n; }
        close(fd[0]); int status; assert(waitpid(child, &status, 0) == child);
        const bool fatal = pin || operation == 5;
        bool ok = count == 2 && evidence[0] == 'F' && evidence[1] == (fatal ? 'E' : 'R');
        ok = ok && (fatal ? WIFSIGNALED(status) && (WTERMSIG(status) == SIGTRAP || WTERMSIG(status) == SIGABRT)
                         : WIFEXITED(status) && WEXITSTATUS(status) == 0);
        printf("%s %s allocation-%d: %s (%s)\n", argv[3], argv[4], operation,
               ok ? "PASS" : "FAIL", fatal ? "fatal GLib error" : "logged class-(c) refusal");
        if (!ok) ++failures;
    }
    free_inst(inst); fini(ctx); dlclose(lib);
    return failures ? 1 : 0;
}
