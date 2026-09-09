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
//   non-NULL pointer (`--coverage` prints one SLOT line per slot, and the
//   script requires the set to equal the register's and every line to say
//   `hit`). Without that a gate stays green by never exercising the
//   allocation it claims to check.
//
//   Attribution. The whole lifecycle runs twice. The first pass runs inside
//   __lsan_disable(), so the one-time statics the backend and glib allocate
//   at first use are not this gate's subject; the second pass runs live, and
//   a leak reported there is a PER-CALL leak — the kind a real consumer
//   accumulates one keystroke at a time.
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

// One entry per line of crates/oxpinyin-capi/libpinyin.alloc. The names are
// the register's two key fields; the script compares this set against the
// file, so a register line with no slot here (or the reverse) fails the gate.
struct Slot {
    const char *symbol;
    const char *name;
};

// clang-format off
const Slot kSlots[] = {
    {"pinyin_init",                             "return"},
    {"pinyin_alloc_instance",                   "return"},
    {"pinyin_begin_add_phrases",                "return"},
    {"pinyin_begin_get_phrases",                "return"},
    {"pinyin_begin_get_bigram_phrases",         "return"},
    {"pinyin_get_context",                      "return"},
    {"pinyin_get_candidate",                    "candidate"},
    {"pinyin_get_candidate_string",             "utf8_str"},
    {"pinyin_get_pinyin_key",                   "key"},
    {"pinyin_get_pinyin_key_rest",              "key_rest"},
    {"pinyin_get_sentence",                     "sentence"},
    {"pinyin_get_full_pinyin_auxiliary_text",   "aux_text"},
    {"pinyin_get_double_pinyin_auxiliary_text", "aux_text"},
    {"pinyin_get_chewing_auxiliary_text",       "aux_text"},
    {"pinyin_get_pinyin_string",                "utf8_str"},
    {"pinyin_get_zhuyin_string",                "utf8_str"},
    {"pinyin_get_luoma_pinyin_string",          "utf8_str"},
    {"pinyin_get_secondary_zhuyin_string",      "utf8_str"},
    {"pinyin_get_pinyin_strings",               "shengmu"},
    {"pinyin_get_pinyin_strings",               "yunmu"},
    {"pinyin_token_get_phrase",                 "utf8_str"},
    {"pinyin_iterator_get_next_phrase",         "phrase"},
    {"pinyin_iterator_get_next_phrase",         "pinyin"},
    {"pinyin_bigram_iterator_get_next_phrase",  "phrase"},
    {"pinyin_bigram_iterator_get_next_phrase",  "pinyin"},
    {"pinyin_in_chewing_keyboard",              "symbols"},
};
// clang-format on

constexpr size_t kSlotCount = sizeof(kSlots) / sizeof(kSlots[0]);
bool g_hit[kSlotCount];

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

// The three auxiliary-text getters, each called TWICE: once over a live
// parse, and once after pinyin_reset. The second call is the point of the
// exercise — the register marks these `false-allocates`, so the reset call
// returns false having allocated an empty string, and a consumer that frees
// only on `true` leaks one buffer per keystroke.
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

    pinyin_reset(instance);

    aux = nullptr;
    pinyin_get_full_pinyin_auxiliary_text(instance, 0, &aux);
    release_g_free("pinyin_get_full_pinyin_auxiliary_text", "aux_text", aux);

    aux = nullptr;
    pinyin_get_double_pinyin_auxiliary_text(instance, 0, &aux);
    release_g_free("pinyin_get_double_pinyin_auxiliary_text", "aux_text", aux);

    aux = nullptr;
    pinyin_get_chewing_auxiliary_text(instance, 0, &aux);
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
        pinyin_end_get_bigram_phrases(bigram_iter);
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

    // Pass 2: the same lifecycle, live. Anything still held here was
    // allocated and not released by the declared deallocator.
    const int rc = lifecycle(argv[1], argv[2]);
    if (rc != 0) {
        std::fprintf(stderr, "fatal: measured lifecycle failed (%d)\n", rc);
        return rc;
    }

    if (coverage) {
        for (size_t i = 0; i < kSlotCount; ++i) {
            std::printf("SLOT %s %s %s\n", kSlots[i].symbol, kSlots[i].name,
                        g_hit[i] ? "hit" : "miss");
        }
        std::fflush(stdout);
    }

    if (__lsan_do_recoverable_leak_check() != 0) {
        std::fprintf(stderr, "FAIL: LeakSanitizer reported a per-call leak\n");
        return 10;
    }
    return 0;
}
