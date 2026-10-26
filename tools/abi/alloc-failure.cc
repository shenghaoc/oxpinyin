// Linux allocation-boundary audit, driven through the public C ABI.
// Usage: alloc-failure SO DATA pinyin|zhuyin pin|subject USER_DIR [array-only]
// Expectations: pin 074a2219 storage/chewing_key.cpp:74-88,
// storage/zhuyin_parser2.cpp:301-332, pinyin.cpp:2801-2818 and
// zhuyin.cpp:1793-1810; GLib 2.89.4 glib/gmem.c:95-106,164-177.
// The subject's final foreign allocation is found by where its pointer goes,
// not by symbol: a calibration child runs the call once without failure and
// records the caller of every malloc, and the call sites whose results the
// call hands to C are the injection sites. No symbol size or inlining
// decision is consulted, so a stripped or re-optimised library is targeted
// the same way, and an earlier Rust allocation is never mistaken for the
// final one.
#include <glib.h>
#include <dlfcn.h>
#include <sys/mman.h>
#include <sys/wait.h>
#include <sys/resource.h>
#include <unistd.h>
#include <cassert>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <initializer_list>

// Test-only glibc interposition bypass; no shipping library uses these.
extern "C" void *__libc_malloc(size_t);
extern "C" void *__libc_realloc(void *, size_t);
extern "C" void __libc_free(void *);
namespace {
struct Event { void *ptr; uintptr_t pc; };
// The call sites (malloc return addresses) a calibration found, shared with
// the parent through an anonymous mapping.
struct Calibration { unsigned count; uintptr_t pc[4]; };
enum Target { SITE = 1, GLIB_MALLOC, GLIB_REALLOC };
Event events[1024];
unsigned event_count;
uintptr_t sites[4];
unsigned site_count;
Calibration *calibration;
int target, fail_at = 1, seen, fired, channel = -1, diagnostics, warnings;
bool armed, inside, recording;
const char *warning_domain, *warning_message;
void *tracked[32];
unsigned tracked_count, freed_count;
bool site(uintptr_t pc) {
    for (unsigned i = 0; i < site_count; ++i) if (sites[i] == pc) return true;
    return false;
}
bool glib_caller(void *pc, const char *name) {
    Dl_info info{};
    return dladdr(pc, &info) && info.dli_sname && !strcmp(info.dli_sname, name);
}
void marker(char c) { if (channel >= 0) { auto ignored = write(channel, &c, 1); (void)ignored; } }
GLogWriterOutput writer(GLogLevelFlags level, const GLogField *, gsize, gpointer) {
    ++diagnostics;
    if (level & G_LOG_LEVEL_ERROR) marker('E');
    return G_LOG_WRITER_HANDLED;
}
void log_handler(const gchar *domain, GLogLevelFlags level, const gchar *message, gpointer) {
    ++diagnostics;
    if ((level & G_LOG_LEVEL_WARNING) && domain && message && warning_domain && warning_message &&
        !strcmp(domain, warning_domain) && !strcmp(message, warning_message)) ++warnings;
    if (level & G_LOG_LEVEL_ERROR) marker('E');
}
}
extern "C" void *malloc(size_t n) noexcept {
    void *caller = __builtin_return_address(0);
    auto pc = reinterpret_cast<uintptr_t>(caller);
    bool relevant = false, fail = false;
    if (armed && !inside) {
        inside = true;
        relevant = site(pc);
        fail = (target == SITE && relevant) || (target == GLIB_MALLOC && glib_caller(caller, "g_malloc"));
        if (fail) { ++seen; fail = seen == fail_at; }
        inside = false;
    }
#ifndef OXPINYIN_ALLOC_FAILURE_DISABLED
    if (fail) { ++fired; armed = false; marker('F'); return nullptr; }
#endif
    void *p = __libc_malloc(n);
    if (recording && p && event_count < sizeof events / sizeof *events) events[event_count++] = {p, pc};
    if (relevant && p && tracked_count < 32) tracked[tracked_count++] = p;
    return p;
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
    if (fail) { ++fired; armed = false; marker('F'); return nullptr; }
#endif
    return __libc_realloc(p, n);
}
extern "C" void free(void *p) noexcept {
    if (p) for (unsigned i = 0; i < tracked_count; ++i)
        if (tracked[i] == p) { tracked[i] = nullptr; ++freed_count; break; }
    __libc_free(p);
}

// Attribute the pointers one call handed to C, in allocation order, to the
// malloc call sites that returned them. The mallocs made at those sites during
// the call must be exactly those pointers, in that order: a temporary at the
// same site, an output the Rust allocator made, or a pointer no recorded malloc
// returned would make "the nth allocation" mean something else, so it is refused.
bool find_sites(void *const *outputs, unsigned count) {
    site_count = 0;
    for (unsigned i = 0; i < count; ++i) {
        unsigned e = event_count;
        for (unsigned j = 0; j < event_count; ++j) if (events[j].ptr == outputs[i]) e = j;
        if (e == event_count) { fprintf(stderr, "calibration: output %u was not returned by a recorded malloc\n", i); return false; }
        if (!site(events[e].pc)) {
            if (site_count == sizeof sites / sizeof *sites) { fprintf(stderr, "calibration: too many allocation sites\n"); return false; }
            sites[site_count++] = events[e].pc;
        }
    }
    unsigned matched = 0;
    for (unsigned j = 0; j < event_count; ++j) {
        if (!site(events[j].pc)) continue;
        if (matched == count || events[j].ptr != outputs[matched]) {
            fprintf(stderr, "calibration: malloc %u at an output site is not output %u\n", j, matched);
            return false;
        }
        ++matched;
    }
    if (matched != count) { fprintf(stderr, "calibration: %u of %u outputs allocated\n", matched, count); return false; }
    return true;
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
    calibration = static_cast<Calibration *>(mmap(nullptr, sizeof *calibration, PROT_READ | PROT_WRITE,
                                                  MAP_SHARED | MAP_ANONYMOUS, -1, 0));
    assert(calibration != MAP_FAILED);
    // One operation in this (forked) process: its setup, then the call under
    // test, either recorded without failure to find the allocation sites or
    // with the targeted allocation failed. Returns the child's exit status.
    auto run = [&](int operation, bool calibrate) {
        rlimit core{0, 0}; setrlimit(RLIMIT_CORE, &core);
        g_log_set_writer_func(writer, nullptr, nullptr);
        g_log_set_handler("GLib", static_cast<GLogLevelFlags>(0xFFFFFFFC), log_handler, nullptr);
        g_log_set_handler(nullptr, static_cast<GLogLevelFlags>(0xFFFFFFFC), log_handler, nullptr);
        g_log_set_handler(pinyin ? "libpinyin" : "libzhuyin", static_cast<GLogLevelFlags>(0xFFFFFFFC), log_handler, nullptr);
        GArray *keys = g_array_new(FALSE, FALSE, sizeof(uint16_t));
        void *iterator = nullptr;
        auto next = static_cast<bool (*)(void *, char **, char **, int *)>(nullptr);
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
        // The subject's failing allocation is the fail_at-th final foreign
        // allocation of the call (list or string, in allocation order); the
        // pin's is the fail_at-th g_malloc, and operation 5 is GArray growth.
        target = operation == 5 ? GLIB_REALLOC : pin ? GLIB_MALLOC : SITE;
        fail_at = pin ? (operation == 4 ? 2 : 1) : (operation == 4 ? 3 : operation == 3 || operation >= 6 ? 2 : 1);
        warning_domain = pinyin ? "libpinyin" : "libzhuyin";
        warning_message = operation == 2 ? "owned_cstr_list: allocation failed" : "owned_cstr: allocation failed";
        if (operation == 6) warning_message = "malloc_cstr_with: allocation failed";
        seen = fired = diagnostics = warnings = 0; tracked_count = freed_count = 0;
        event_count = 0;
        if (calibrate) recording = true; else armed = true;
        bool ret; char *out = reinterpret_cast<char *>(1); char **list = reinterpret_cast<char **>(1);
        char *reading = reinterpret_cast<char *>(2); int frequency = 73;
        if (operation == 1) ret = render(inst, key, &out);
        else if (operation < 5) ret = keyboard(inst, keyboard_key, &list);
        else if (operation == 5) ret = nth(inst, token, 0, keys);
        else if (operation < 8) ret = next(iterator, &out, &reading, &frequency);
        else ret = symbol<bool (*)(void *, void *, char **, char **)>(lib, argv[3], "get_pinyin_strings")(inst, key, &out, &reading);
        armed = recording = false;
        if (calibrate) {
            // The pointers handed to C, in the order the call allocated them: the
            // vector before its strings. The iterators return whether more rows
            // follow, so they are judged by what they wrote.
            void *outputs[4]{}; unsigned count = 0;
            bool wrote = ret;
            if (operation == 1) outputs[count++] = out;
            else if (operation < 5) {
                if (wrote) {
                    outputs[count++] = list;
                    for (char **element = list; *element && count < 4; ++element) outputs[count++] = *element;
                }
            } else {
                wrote = out != reinterpret_cast<char *>(1) && reading != reinterpret_cast<char *>(2);
                outputs[count++] = out; outputs[count++] = reading;
            }
            const unsigned expected = operation == 1 ? 1 : operation < 5 ? 3 : 2;
            if (!wrote || count != expected || !find_sites(outputs, count)) return 1;
            calibration->count = site_count;
            memcpy(calibration->pc, sites, sizeof sites);
            return 0;
        }
        // Class (c): exactly one warning and failure, while retaining
        // the partial-vector cleanup checks and fatal GArray control.
        bool valid = !pin && operation < 5 && fired == 1 && seen == fail_at && diagnostics == 1 && warnings == 1;
        if (operation == 1) valid = valid && out == nullptr && !ret;
        else if (operation < 5) valid = valid && list == nullptr && !ret && tracked_count == freed_count;
        if (operation == 4) valid = valid && tracked_count == 2 && freed_count == 2;
        if (operation >= 6) {
            valid = !ret && fired == 1 && seen == fail_at && diagnostics == 1 && warnings == 1 &&
                out == reinterpret_cast<char *>(1) && reading == reinterpret_cast<char *>(2) &&
                frequency == 73 && tracked_count == 1 && tracked_count == freed_count;
            dprintf(STDERR_FILENO, "allocation-%d: return=%d warnings=%d outputs-unchanged=%d tracked=%u freed=%u\n",
                    operation, ret, warnings, out == reinterpret_cast<char *>(1) && reading == reinterpret_cast<char *>(2) && frequency == 73,
                    tracked_count, freed_count);
        }
        if (valid) marker('R');
        return valid ? 0 : 1;
    };
    int failures = 0;
    for (int operation = array_only ? 5 : 1; operation <= (!pin && pinyin && !array_only ? 8 : 5); ++operation) {
        // pin vector and element probes share its fatal strdup allocation;
        // subject distinguishes vector malloc from first-element malloc.
        site_count = 0;
        if (!pin && operation != 5) {
            pid_t probe = fork(); assert(probe >= 0);
            if (!probe) _exit(run(operation, true));
            int status; assert(waitpid(probe, &status, 0) == probe);
            if (!WIFEXITED(status) || WEXITSTATUS(status)) {
                printf("%s %s allocation-%d: FAIL (allocation sites not found)\n", argv[3], argv[4], operation);
                fprintf(stderr, "allocation-%d: calibration child-status=%d\n", operation, status);
                ++failures;
                continue;
            }
            site_count = calibration->count;
            memcpy(sites, calibration->pc, sizeof sites);
        }
        int fd[2]; assert(pipe(fd) == 0); pid_t child = fork(); assert(child >= 0);
        if (!child) { close(fd[0]); channel = fd[1]; _exit(run(operation, false)); }
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
        if (!ok) fprintf(stderr, "allocation-%d: child-status=%d markers=%.*s\n", operation, status, static_cast<int>(count), evidence);
    }
    free_inst(inst); fini(ctx); dlclose(lib);
    return failures ? 1 : 0;
}
