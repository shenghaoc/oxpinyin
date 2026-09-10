// alloc-pairing-pinyin.cc — the C consumer half of the allocator-pairing
// gate for libpinyin.so.15.
//
// `crates/oxpinyin-capi/libpinyin.alloc` declares, for every pointer-shaped
// slot `pinyin.h` hands across the boundary, which deallocator releases it.
// This translation unit is the proof that the declaration is true: it drives
// the ABI the way a consumer does, releases each slot with exactly the
// declared deallocator, and is run by tools/abi/check-alloc-pairing.sh under
// LeakSanitizer. A slot whose declared deallocator does not in fact release
// the allocation shows up as a leak; a borrowed pointer wrongly declared
// owned shows up as ASan's invalid-free.
//
// Two properties beyond "it did not crash":
//
//   Coverage. Every slot reports whether it was actually reached with a
//   non-NULL pointer, and whether its false-return contract was probed
//   (`--coverage` prints one SLOT line per slot carrying the register's own
//   class and note, and the script requires the whole entry to match the
//   register and every line to say `hit`). Without that a gate stays green
//   by never exercising the allocation it claims to check — and a register
//   line could drift from what the driver actually does.
//
//   Attribution. The whole lifecycle runs twice. The first pass runs inside
//   __lsan_disable(), so the one-time statics the backend and glib allocate
//   at first use are not this gate's subject; the second pass runs live, and
//   a leak reported there is a PER-CALL leak — the kind a real consumer
//   accumulates one keystroke at a time. Coverage is CLEARED between the two
//   passes: a slot reached only during the warm-up had no leak check run
//   against it, so counting it as covered would overstate what was proven.
//
// Built with -DOXPINYIN_ALLOC_PAIRING_LEAK the TU deliberately drops one
// free. The script builds that variant too and requires it to FAIL: an
// instrument that cannot report a leak it was handed is not evidence.

#include <pinyin.h>

#include <glib.h>

#include <cstdio>
#include <cstring>

#include <sanitizer/lsan_interface.h>

namespace {

// One entry per line of crates/oxpinyin-capi/libpinyin.alloc, carrying all
// four of the register's fields. The script compares the whole entry, so a
// register change the driver does not follow — a class flipped from
// `handle:pinyin_fini` to `borrowed`, a destructor renamed, a false-note
// edited — fails the gate rather than passing unnoticed.
struct Slot {
    const char *symbol;
    const char *name;
    const char *cls;       // handle:<fn> | g_free | g_strfreev | borrowed
    const char *on_false;  // n/a | false-allocates | false-nulls
                           // | false-untouched | false-unreachable
};

// clang-format off
const Slot kSlots[] = {
    {"pinyin_init",                             "return",    "handle:pinyin_fini",                  "n/a"},
    {"pinyin_alloc_instance",                   "return",    "handle:pinyin_free_instance",         "n/a"},
    {"pinyin_begin_add_phrases",                "return",    "handle:pinyin_end_add_phrases",       "n/a"},
    {"pinyin_begin_get_phrases",                "return",    "handle:pinyin_end_get_phrases",       "n/a"},
    {"pinyin_begin_get_bigram_phrases",         "return",    "handle:pinyin_end_get_bigram_phrases", "n/a"},
    {"pinyin_get_context",                      "return",    "borrowed",                            "n/a"},
    {"pinyin_get_candidate",                    "candidate", "borrowed",                            "false-nulls"},
    {"pinyin_get_candidate_string",             "utf8_str",  "borrowed",                            "false-unreachable"},
    {"pinyin_get_pinyin_key",                   "key",       "borrowed",                            "false-nulls"},
    {"pinyin_get_pinyin_key_rest",              "key_rest",  "borrowed",                            "false-nulls"},
    {"pinyin_get_sentence",                     "sentence",  "g_free",                              "false-nulls"},
    {"pinyin_get_full_pinyin_auxiliary_text",   "aux_text",  "g_free",                              "false-allocates"},
    {"pinyin_get_double_pinyin_auxiliary_text", "aux_text",  "g_free",                              "false-allocates"},
    {"pinyin_get_chewing_auxiliary_text",       "aux_text",  "g_free",                              "false-allocates"},
    {"pinyin_get_pinyin_string",                "utf8_str",  "g_free",                              "false-unreachable"},
    {"pinyin_get_zhuyin_string",                "utf8_str",  "g_free",                              "false-unreachable"},
    {"pinyin_get_luoma_pinyin_string",          "utf8_str",  "g_free",                              "false-unreachable"},
    {"pinyin_get_secondary_zhuyin_string",      "utf8_str",  "g_free",                              "false-unreachable"},
    {"pinyin_get_pinyin_strings",               "shengmu",   "g_free",                              "false-unreachable"},
    {"pinyin_get_pinyin_strings",               "yunmu",     "g_free",                              "false-unreachable"},
    {"pinyin_token_get_phrase",                 "utf8_str",  "g_free",                              "false-nulls"},
    {"pinyin_iterator_get_next_phrase",         "phrase",    "g_free",                              "false-untouched"},
    {"pinyin_iterator_get_next_phrase",         "pinyin",    "g_free",                              "false-untouched"},
    {"pinyin_bigram_iterator_get_next_phrase",  "phrase",    "g_free",                              "false-untouched"},
    {"pinyin_bigram_iterator_get_next_phrase",  "pinyin",    "g_free",                              "false-untouched"},
    {"pinyin_in_chewing_keyboard",              "symbols",   "g_strfreev",                          "false-nulls"},
};
// clang-format on

constexpr size_t kSlotCount = sizeof(kSlots) / sizeof(kSlots[0]);
bool g_hit[kSlotCount];
bool g_false_checked[kSlotCount];
bool g_contract_broken = false;

// Cleared between the warm-up and the measured pass: only what the measured
// pass reached had a leak check run against it.
void reset_coverage() {
    for (size_t i = 0; i < kSlotCount; ++i) {
        g_hit[i] = false;
        g_false_checked[i] = false;
    }
}

// A non-heap address a `false-untouched` probe can leave in an out-param and
// recognise afterwards. Never dereferenced and never freed.
char g_sentinel_byte = 0;
gchar *const kSentinel = reinterpret_cast<gchar *>(&g_sentinel_byte);

// `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY`, the
// sort mask every in-tree driver uses.
constexpr guint kDefaultSort = 0x1e;

size_t slot_id(const char *symbol, const char *name) {
    for (size_t i = 0; i < kSlotCount; ++i) {
        if (std::strcmp(kSlots[i].symbol, symbol) == 0 &&
            std::strcmp(kSlots[i].name, name) == 0) {
            return i;
        }
    }
    std::fprintf(stderr, "fatal: no slot %s/%s in the driver table\n", symbol, name);
    std::fflush(stderr);
    return kSlotCount;  // out of range; mark() ignores it and coverage fails
}

void mark(const char *symbol, const char *name, const void *ptr) {
    if (ptr == nullptr) {
        return;
    }
    const size_t id = slot_id(symbol, name);
    if (id < kSlotCount) {
        g_hit[id] = true;
    }
}

// ── False-return contracts ───────────────────────────────────────────
//
// The register's fourth field is the part of the contract a consumer gets
// wrong silently: what an out-param holds when the call answers `false`.
// These probes make it executable. Each drives a failure path a CONFORMING
// consumer can reach — valid handles, in-contract arguments, an empty parse
// or an exhausted iterator — never a NULL-argument refusal, which is caller
// misuse and where the pin would simply crash.

void contract_failed(const char *symbol, const char *name, const char *want,
                     const char *saw) {
    std::fprintf(stderr, "FAIL: %s/%s is declared %s but %s\n", symbol, name, want, saw);
    std::fflush(stderr);
    g_contract_broken = true;
}

// Applies whatever the REGISTER declares for this slot — the note is read
// from the slot table, never chosen at the call site, so editing a note to
// something the library does not do fails here instead of being echoed back
// unchallenged.
//
// `out` is the out-param's value after the probed call; the caller pre-sets
// it to kSentinel so "left alone" is distinguishable from "written NULL".
// Returns the buffer when the declaration is `false-allocates`, so the
// caller can release it through the register's deallocator; nullptr
// otherwise.
void *expect_declared(const char *symbol, const char *name, bool ret, void *out) {
    const size_t id = slot_id(symbol, name);
    if (id >= kSlotCount) {
        return nullptr;
    }
    const char *want = kSlots[id].on_false;

    if (std::strcmp(want, "n/a") == 0 || std::strcmp(want, "false-unreachable") == 0) {
        contract_failed(symbol, name, want,
                        "the driver probed a failure path the register calls unreachable");
        return nullptr;
    }
    if (ret) {
        contract_failed(symbol, name, want, "the probed call returned true");
        return nullptr;
    }

    if (std::strcmp(want, "false-nulls") == 0) {
        if (out == kSentinel) {
            contract_failed(symbol, name, want, "the out-param was left untouched");
        } else if (out != nullptr) {
            contract_failed(symbol, name, want, "the out-param was left non-NULL");
        } else {
            g_false_checked[id] = true;
        }
        return nullptr;
    }
    if (std::strcmp(want, "false-untouched") == 0) {
        if (out != kSentinel) {
            contract_failed(symbol, name, want,
                            out == nullptr ? "the out-param was NULLed"
                                           : "the out-param was written");
        } else {
            g_false_checked[id] = true;
        }
        return nullptr;
    }
    if (std::strcmp(want, "false-allocates") == 0) {
        if (out == kSentinel) {
            contract_failed(symbol, name, want, "the out-param was left untouched");
            return nullptr;
        }
        if (out == nullptr) {
            contract_failed(symbol, name, want, "the out-param was NULL");
            return nullptr;
        }
        g_false_checked[id] = true;
        return out;
    }
    contract_failed(symbol, name, want, "that is not a note this driver understands");
    return nullptr;
}

// A `g_free` slot: record that the pointer was really produced, then release
// it with the register's deallocator. `g_free(NULL)` is a documented no-op,
// which is what the `false-untouched` and `false-nulls` notes rely on.
void release_g_free(const char *symbol, const char *name, gchar *ptr) {
    mark(symbol, name, ptr);
#ifdef OXPINYIN_ALLOC_PAIRING_LEAK
    // The negative control: drop exactly the frees this gate exists to
    // require, so a run that reports nothing proves the instrument is dead.
    (void)ptr;
#else
    g_free(ptr);
#endif
}

void release_g_strfreev(const char *symbol, const char *name, gchar **ptr) {
    mark(symbol, name, ptr);
#ifdef OXPINYIN_ALLOC_PAIRING_LEAK
    (void)ptr;
#else
    g_strfreev(ptr);
#endif
}

// ── The exercises, one per allocation family ─────────────────────────

// Every `gchar **` display getter over one borrowed key, plus the two-slot
// shengmu/yunmu pair. Each out-param is initialized to NULL first, because
// the register marks these `false-nulls` / `false-untouched`: the caller,
// not the library, owns that initialization.
void exercise_key_strings(pinyin_instance_t *instance) {
    ChewingKey *key = nullptr;
    ChewingKeyRest *key_rest = nullptr;
    if (pinyin_get_pinyin_key(instance, 0, &key)) {
        mark("pinyin_get_pinyin_key", "key", key);
    }
    if (pinyin_get_pinyin_key_rest(instance, 0, &key_rest)) {
        mark("pinyin_get_pinyin_key_rest", "key_rest", key_rest);
    }
    if (key == nullptr) {
        return;
    }

    gchar *utf8 = nullptr;
    pinyin_get_pinyin_string(instance, key, &utf8);
    release_g_free("pinyin_get_pinyin_string", "utf8_str", utf8);

    utf8 = nullptr;
    pinyin_get_zhuyin_string(instance, key, &utf8);
    release_g_free("pinyin_get_zhuyin_string", "utf8_str", utf8);

    utf8 = nullptr;
    pinyin_get_luoma_pinyin_string(instance, key, &utf8);
    release_g_free("pinyin_get_luoma_pinyin_string", "utf8_str", utf8);

    utf8 = nullptr;
    pinyin_get_secondary_zhuyin_string(instance, key, &utf8);
    release_g_free("pinyin_get_secondary_zhuyin_string", "utf8_str", utf8);

    gchar *shengmu = nullptr;
    gchar *yunmu = nullptr;
    pinyin_get_pinyin_strings(instance, key, &shengmu, &yunmu);
    release_g_free("pinyin_get_pinyin_strings", "shengmu", shengmu);
    release_g_free("pinyin_get_pinyin_strings", "yunmu", yunmu);
}

// The three auxiliary-text getters over a live parse. Their `false` half is
// a register contract (`false-allocates`) and lives in probe_false_contracts.
void exercise_auxiliary_text(pinyin_instance_t *instance) {
    gchar *aux = nullptr;
    pinyin_get_full_pinyin_auxiliary_text(instance, 2, &aux);
    release_g_free("pinyin_get_full_pinyin_auxiliary_text", "aux_text", aux);

    aux = nullptr;
    pinyin_get_double_pinyin_auxiliary_text(instance, 2, &aux);
    release_g_free("pinyin_get_double_pinyin_auxiliary_text", "aux_text", aux);

    aux = nullptr;
    pinyin_get_chewing_auxiliary_text(instance, 2, &aux);
    release_g_free("pinyin_get_chewing_auxiliary_text", "aux_text", aux);
}

// The candidate surface. Both slots are `borrowed`: the pointers belong to
// the instance and are read, never freed — an ASan invalid-free is what a
// wrong `g_free` here would look like.
void exercise_candidates(pinyin_instance_t *instance) {
    if (!pinyin_guess_candidates(instance, 0, kDefaultSort)) {
        return;
    }
    guint num = 0;
    if (!pinyin_get_n_candidate(instance, &num) || num == 0) {
        return;
    }
    lookup_candidate_t *candidate = nullptr;
    if (!pinyin_get_candidate(instance, 0, &candidate)) {
        return;
    }
    mark("pinyin_get_candidate", "candidate", candidate);

    const gchar *text = nullptr;
    if (pinyin_get_candidate_string(instance, candidate, &text)) {
        mark("pinyin_get_candidate_string", "utf8_str", text);
    }
}

// Find the candidate whose text is `want`, through the ABI itself: the
// borrowed string slot is the only way a consumer can identify a row.
lookup_candidate_t *candidate_for(pinyin_instance_t *instance, const char *want) {
    guint num = 0;
    if (!pinyin_get_n_candidate(instance, &num)) {
        return nullptr;
    }
    for (guint i = 0; i < num; ++i) {
        lookup_candidate_t *candidate = nullptr;
        if (!pinyin_get_candidate(instance, i, &candidate)) {
            continue;
        }
        const gchar *text = nullptr;
        if (pinyin_get_candidate_string(instance, candidate, &text) && text != nullptr &&
            std::strcmp(text, want) == 0) {
            return candidate;
        }
    }
    return nullptr;
}

// Choose 你 then 好 out of `nihao` and train the result, so the user store
// holds a (你 → 好) bigram. Without it `pinyin_begin_get_bigram_phrases`
// opens an iterator with no rows and the two bigram `gchar **` slots are
// never reached — which the coverage check rejects, correctly: an untested
// pairing is not a held pairing.
void train_multiphrase_sentence(pinyin_instance_t *instance) {
    if (!pinyin_guess_candidates(instance, 0, kDefaultSort)) {
        return;
    }
    lookup_candidate_t *ni = candidate_for(instance, "你");
    if (ni == nullptr) {
        return;
    }
    const gint cursor = pinyin_choose_candidate(instance, 0, ni);
    if (cursor <= 0) {
        return;
    }
    // The window is anchored at the caller's offset, so the remaining 好
    // group is offered at the post-choose cursor, never at 0.
    if (!pinyin_guess_candidates(instance, static_cast<size_t>(cursor), kDefaultSort)) {
        return;
    }
    lookup_candidate_t *hao = candidate_for(instance, "好");
    if (hao == nullptr) {
        return;
    }
    pinyin_choose_candidate(instance, static_cast<size_t>(cursor), hao);
    pinyin_train(instance, 0);
}

// The token surface: a real token from pinyin_lookup_tokens, then the
// `gchar **` phrase read over it. The GArray is the caller's, created and
// freed here with glib's own allocator, which is what the ABI expects.
void exercise_tokens(pinyin_instance_t *instance) {
    GArray *tokens = g_array_new(FALSE, FALSE, sizeof(phrase_token_t));
    if (pinyin_lookup_tokens(instance, "你好", tokens) && tokens->len > 0) {
        const phrase_token_t token = g_array_index(tokens, phrase_token_t, 0);
        guint len = 0;
        gchar *utf8 = nullptr;
        pinyin_token_get_phrase(instance, token, &len, &utf8);
        release_g_free("pinyin_token_get_phrase", "utf8_str", utf8);
    }
    g_array_free(tokens, TRUE);
}

// The `gchar ***` slot. Which ASCII keys map is scheme-dependent, so the
// whole printable range is swept and the first hit is enough; the coverage
// check is what fails if none of them allocate.
void exercise_chewing_keyboard(pinyin_instance_t *instance) {
    for (char key = 0x21; key < 0x7f; ++key) {
        gchar **symbols = nullptr;
        if (pinyin_in_chewing_keyboard(instance, key, &symbols)) {
            release_g_strfreev("pinyin_in_chewing_keyboard", "symbols", symbols);
            return;
        }
        // A `false` return nulls the out-param (`false-nulls`); releasing it
        // anyway is the no-op that keeps the consumer loop uniform.
        release_g_strfreev("pinyin_in_chewing_keyboard", "symbols", symbols);
    }
}

// The three iterator handles, each released only by its own `end` function,
// and the four `gchar **` row slots the two export iterators hand out.
void exercise_iterators(pinyin_context_t *context) {
    import_iterator_t *import_iter = pinyin_begin_add_phrases(context, USER_DICTIONARY);
    mark("pinyin_begin_add_phrases", "return", import_iter);
    if (import_iter != nullptr) {
        pinyin_iterator_add_phrase(import_iter, "你好", "ni'hao", 3);
        pinyin_end_add_phrases(import_iter);
    }

    export_iterator_t *export_iter = pinyin_begin_get_phrases(context, USER_DICTIONARY);
    mark("pinyin_begin_get_phrases", "return", export_iter);
    if (export_iter != nullptr) {
        while (pinyin_iterator_has_next_phrase(export_iter)) {
            gchar *phrase = nullptr;
            gchar *pinyin = nullptr;
            gint count = 0;
            if (!pinyin_iterator_get_next_phrase(export_iter, &phrase, &pinyin, &count)) {
                break;
            }
            release_g_free("pinyin_iterator_get_next_phrase", "phrase", phrase);
            release_g_free("pinyin_iterator_get_next_phrase", "pinyin", pinyin);
        }
        // Exhaustion is the reachable failure path: one more call must
        // answer false without writing either out-param, so the consumer
        // loop does not free what it never received.
        gchar *phrase = kSentinel;
        gchar *pinyin = kSentinel;
        gint count = 0;
        const bool more =
            pinyin_iterator_get_next_phrase(export_iter, &phrase, &pinyin, &count);
        expect_declared("pinyin_iterator_get_next_phrase", "phrase", more, phrase);
        expect_declared("pinyin_iterator_get_next_phrase", "pinyin", more, pinyin);
        pinyin_end_get_phrases(export_iter);
    }

    bigram_export_iterator_t *bigram_iter = pinyin_begin_get_bigram_phrases(context);
    mark("pinyin_begin_get_bigram_phrases", "return", bigram_iter);
    if (bigram_iter != nullptr) {
        while (pinyin_bigram_iterator_has_next_phrase(bigram_iter)) {
            gchar *phrase = nullptr;
            gchar *pinyin = nullptr;
            gint count = 0;
            if (!pinyin_bigram_iterator_get_next_phrase(bigram_iter, &phrase, &pinyin,
                                                        &count)) {
                break;
            }
            release_g_free("pinyin_bigram_iterator_get_next_phrase", "phrase", phrase);
            release_g_free("pinyin_bigram_iterator_get_next_phrase", "pinyin", pinyin);
        }
        gchar *phrase = kSentinel;
        gchar *pinyin = kSentinel;
        gint count = 0;
        const bool more =
            pinyin_bigram_iterator_get_next_phrase(bigram_iter, &phrase, &pinyin, &count);
        expect_declared("pinyin_bigram_iterator_get_next_phrase", "phrase", more, phrase);
        expect_declared("pinyin_bigram_iterator_get_next_phrase", "pinyin", more, pinyin);
        pinyin_end_get_bigram_phrases(bigram_iter);
    }
}

// Every reachable false-return contract, driven from an empty parse. Runs
// last in the lifecycle because it resets the instance.
//
// The `false-unreachable` slots are absent by construction: `ChewingKey` is
// an opaque typedef, so a conforming consumer cannot fabricate the unset key
// that is the only non-NULL-argument way into those refusals, and
// pinyin_get_candidate_string answers true for every candidate the ABI
// hands out. Those notes are a claim about reachability, and the script
// requires them to be spelled `false-unreachable` rather than silently
// skipped.
void probe_false_contracts(pinyin_instance_t *instance) {
    pinyin_reset(instance);

    // false-allocates: an empty matrix answers false HAVING allocated an
    // empty string (upstream's shape). Freeing on the false path is the
    // whole point, so each buffer goes back through g_free.
    // The three auxiliary-text getters: an empty matrix answers false
    // HAVING allocated an empty string (upstream's shape). Freeing on the
    // false path is the whole point, so each buffer goes back through
    // g_free — expect_declared hands it back when the register says
    // `false-allocates`.
    gchar *aux = kSentinel;
    bool ok = pinyin_get_full_pinyin_auxiliary_text(instance, 0, &aux);
    release_g_free("pinyin_get_full_pinyin_auxiliary_text", "aux_text",
                   static_cast<gchar *>(expect_declared(
                       "pinyin_get_full_pinyin_auxiliary_text", "aux_text", ok, aux)));

    aux = kSentinel;
    ok = pinyin_get_double_pinyin_auxiliary_text(instance, 0, &aux);
    release_g_free("pinyin_get_double_pinyin_auxiliary_text", "aux_text",
                   static_cast<gchar *>(expect_declared(
                       "pinyin_get_double_pinyin_auxiliary_text", "aux_text", ok, aux)));

    aux = kSentinel;
    ok = pinyin_get_chewing_auxiliary_text(instance, 0, &aux);
    release_g_free("pinyin_get_chewing_auxiliary_text", "aux_text",
                   static_cast<gchar *>(expect_declared("pinyin_get_chewing_auxiliary_text",
                                                        "aux_text", ok, aux)));

    // Nothing to decode, no key at any offset, no candidate at any index,
    // no phrase behind null_token.
    gchar *sentence = kSentinel;
    ok = pinyin_get_sentence(instance, 0, &sentence);
    expect_declared("pinyin_get_sentence", "sentence", ok, sentence);

    lookup_candidate_t *candidate = reinterpret_cast<lookup_candidate_t *>(kSentinel);
    ok = pinyin_get_candidate(instance, 0, &candidate);
    expect_declared("pinyin_get_candidate", "candidate", ok, candidate);

    ChewingKey *key = reinterpret_cast<ChewingKey *>(kSentinel);
    ok = pinyin_get_pinyin_key(instance, 0, &key);
    expect_declared("pinyin_get_pinyin_key", "key", ok, key);

    ChewingKeyRest *key_rest = reinterpret_cast<ChewingKeyRest *>(kSentinel);
    ok = pinyin_get_pinyin_key_rest(instance, 0, &key_rest);
    expect_declared("pinyin_get_pinyin_key_rest", "key_rest", ok, key_rest);

    guint len = 0;
    gchar *utf8 = kSentinel;
    ok = pinyin_token_get_phrase(instance, null_token, &len, &utf8);
    expect_declared("pinyin_token_get_phrase", "utf8_str", ok, utf8);

    // The first ASCII key the active scheme does not map.
    for (char probe = 0x21; probe < 0x7f; ++probe) {
        gchar **symbols = reinterpret_cast<gchar **>(kSentinel);
        if (pinyin_in_chewing_keyboard(instance, probe, &symbols)) {
            release_g_strfreev("pinyin_in_chewing_keyboard", "symbols", symbols);
            continue;
        }
        expect_declared("pinyin_in_chewing_keyboard", "symbols", false, symbols);
        break;
    }
}

// One full consumer lifecycle: the two handles, a parse, a decode, a train
// (so the user store holds a bigram for the export iterator to render), and
// every owned slot allocated and released.
int lifecycle(const char *systemdir, const char *userdir) {
    pinyin_context_t *context = pinyin_init(systemdir, userdir);
    if (context == nullptr) {
        return 1;
    }
    mark("pinyin_init", "return", context);

    pinyin_instance_t *instance = pinyin_alloc_instance(context);
    if (instance == nullptr) {
        pinyin_fini(context);
        return 2;
    }
    mark("pinyin_alloc_instance", "return", instance);
    // Borrowed: the context the instance was allocated from, never a second
    // handle and never passed to pinyin_fini.
    mark("pinyin_get_context", "return", pinyin_get_context(instance));

    pinyin_parse_more_full_pinyins(instance, "nihao");
    pinyin_guess_sentence(instance);

    exercise_candidates(instance);
    exercise_key_strings(instance);
    exercise_tokens(instance);
    exercise_chewing_keyboard(instance);

    // The sentence slot, over the live decode.
    gchar *sentence = nullptr;
    pinyin_get_sentence(instance, 0, &sentence);
    release_g_free("pinyin_get_sentence", "sentence", sentence);

    // Give the user store a unigram and a bigram to export; the two export
    // iterators have rows to hand out only then. Before the auxiliary-text
    // exercise, which ends on pinyin_reset.
    pinyin_remember_user_input(instance, "你好", -1);
    train_multiphrase_sentence(instance);

    exercise_auxiliary_text(instance);
    exercise_iterators(context);
    probe_false_contracts(instance);

    pinyin_free_instance(instance);
    pinyin_fini(context);
    return 0;
}

}  // namespace

int main(int argc, char **argv) {
    if (argc < 3) {
        std::fprintf(stderr, "usage: %s <systemdir> <userdir> [--coverage]\n", argv[0]);
        return 64;
    }
    const bool coverage = argc > 3 && std::strcmp(argv[3], "--coverage") == 0;

    // Pass 1: absorb the one-time allocations the backend, glib and the
    // lazily-built tables make at first use. They are process-lifetime state,
    // not a per-call pairing failure, and __lsan_disable() keeps them out of
    // the report for good — including the check at exit.
    __lsan_disable();
    const int warm = lifecycle(argv[1], argv[2]);
    __lsan_enable();
    if (warm != 0) {
        std::fprintf(stderr, "fatal: warm-up lifecycle failed (%d)\n", warm);
        return warm;
    }

    // Whatever the warm-up reached, it reached with the leak check disabled;
    // carrying those bits forward would report coverage the measured pass
    // never earned. The contract probes are cleared with them, and a
    // contract broken during the warm-up still stands — it is a property of
    // the library, not of which pass observed it.
    reset_coverage();

    // Pass 2: the same lifecycle, live. Anything still held here was
    // allocated and not released by the declared deallocator.
    const int rc = lifecycle(argv[1], argv[2]);
    if (rc != 0) {
        std::fprintf(stderr, "fatal: measured lifecycle failed (%d)\n", rc);
        return rc;
    }

    if (coverage) {
        for (size_t i = 0; i < kSlotCount; ++i) {
            const char *probed = "n-a";
            if (std::strcmp(kSlots[i].on_false, "false-unreachable") == 0) {
                probed = "unreachable";
            } else if (std::strcmp(kSlots[i].on_false, "n/a") != 0) {
                probed = g_false_checked[i] ? "checked" : "unchecked";
            }
            std::printf("SLOT %s %s %s %s %s %s\n", kSlots[i].symbol, kSlots[i].name,
                        kSlots[i].cls, kSlots[i].on_false, g_hit[i] ? "hit" : "miss", probed);
        }
        std::fflush(stdout);
    }

    if (g_contract_broken) {
        std::fprintf(stderr, "FAIL: a declared false-return contract does not hold\n");
        return 11;
    }
    if (__lsan_do_recoverable_leak_check() != 0) {
        std::fprintf(stderr, "FAIL: LeakSanitizer reported a per-call leak\n");
        return 10;
    }
    return 0;
}
