// storage_dump.cpp — C-linkage dump shim over libpinyin's internal storage.
//
// Reads a legacy libpinyin user directory (`user.bin` phrase index +
// `user_bigram.db` bigram) through libpinyin's own storage classes
// (`FacadePhraseIndex` / `PhraseItem` over `MemoryChunk`, `Bigram` /
// `SingleGram` over the Tkrzw DBM), and emits a flat, deterministic,
// little-endian byte buffer of verbatim store values for the Rust side to
// parse and import. This is the (B)-refined approach: the legacy format is
// read by the very code that wrote it, so no binary layout is reimplemented
// and no DBM-backend variation is decoded here.
//
// Read-only guarantee: `MemoryChunk::load` opens `O_RDONLY` and verifies the
// stored length + checksum; `Bigram::load_db` opens the DBM with
// `OPEN_NO_CREATE` and copies it into an in-memory DB. Nothing in this shim
// opens a path for writing.

#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

#include <glib.h>

#include <memory_chunk.h>         // pinyin::MemoryChunk
#include <storage/ngram.h>        // pinyin::Bigram, SingleGram
#include <storage/phrase_index.h> // pinyin::FacadePhraseIndex, PhraseItem

namespace {

// ── dump format ────────────────────────────────────────────────────────
//
// All integers are little-endian. The target is the x86-64 oracle host.
//
//   bytes 0..3   "MIGR" magic
//   u32          version (1)
//   u32          tokens_iterated   (m_range_end - m_range_begin)
//   u32          null_slot_count   (get_phrase_item == ERROR_NO_ITEM)
//   u32          corrupt_count     (phrase item present but unreadable)
//   u32          phrase_count
//   u32          bigram_count
//   [phrase records] x phrase_count
//   [bigram records] x bigram_count
//
// Phrase record:
//   u32 token; u32 unigram_freq; u32 utf8_len; bytes phrase_utf8;
//   u32 n_pronunciations; then per pronunciation:
//     u32 n_syllables; then per syllable: u32 pinyin_utf8_len; bytes pinyin;
//     u32 freq
//
// Bigram record (one per predecessor, ascending by predecessor token):
//   u32 prev_token; u32 total_freq; u32 n_successors; then per successor:
//     u32 cur_token; u32 count
//
// The pinyin strings carry libpinyin's tone-digit suffix (`ni3`, zero tone
// plain `ni`); tone stripping and SyllableKey mapping happen on the Rust side.

constexpr uint8_t kMagic[4] = {'M', 'I', 'G', 'R'};
constexpr uint32_t kVersion = 1;

class Writer {
public:
    void u32(uint32_t v) {
        buf_.push_back(static_cast<uint8_t>(v & 0xFF));
        buf_.push_back(static_cast<uint8_t>((v >> 8) & 0xFF));
        buf_.push_back(static_cast<uint8_t>((v >> 16) & 0xFF));
        buf_.push_back(static_cast<uint8_t>((v >> 24) & 0xFF));
    }

    void bytes(const void *p, size_t n) {
        const uint8_t *b = static_cast<const uint8_t *>(p);
        buf_.insert(buf_.end(), b, b + n);
    }

    void str(const std::string &s) {
        u32(static_cast<uint32_t>(s.size()));
        bytes(s.data(), s.size());
    }

    void append(const std::vector<uint8_t> &other) {
        buf_.insert(buf_.end(), other.begin(), other.end());
    }

    const std::vector<uint8_t> &buffer() const { return buf_; }

private:
    std::vector<uint8_t> buf_;
};

void set_error(char *err, size_t err_len, const std::string &msg) {
    if (err != nullptr && err_len > 0) {
        std::snprintf(err, err_len, "%s", msg.c_str());
    }
}

/// Serialises one phrase item into `out`; returns false (and leaves `out`
/// untouched) when any value is unreadable.
bool serialize_phrase(pinyin::PhraseItem &item, phrase_token_t token,
                      Writer *out) {
    const guint8 phrase_length = item.get_phrase_length();
    // Guard against a corrupt length overflowing the stack buffers; a valid
    // user phrase is 1..MAX_PHRASE_LENGTH-1 scalars.
    if (phrase_length == 0 || phrase_length >= MAX_PHRASE_LENGTH) {
        return false;
    }

    ucs4_t phrase[MAX_PHRASE_LENGTH];
    if (!item.get_phrase_string(phrase)) {
        return false;
    }
    gchar *utf8 = g_ucs4_to_utf8(phrase, phrase_length, nullptr, nullptr, nullptr);
    if (utf8 == nullptr) {
        return false;
    }
    std::string phrase_utf8(utf8);
    g_free(utf8);

    const guint8 n_pronunciations = item.get_n_pronunciation();

    // Serialise into a scratch buffer so a mid-phrase failure drops the whole
    // record instead of leaving a truncated one in the dump.
    Writer scratch;
    scratch.u32(token);
    scratch.u32(item.get_unigram_frequency());
    scratch.str(phrase_utf8);
    scratch.u32(n_pronunciations);
    for (guint8 p = 0; p < n_pronunciations; ++p) {
        ChewingKey keys[MAX_PHRASE_LENGTH];
        guint32 freq = 0;
        if (!item.get_nth_pronunciation(p, keys, freq)) {
            return false;
        }
        scratch.u32(phrase_length);
        for (guint8 s = 0; s < phrase_length; ++s) {
            gchar *pinyin = keys[s].get_pinyin_string();
            std::string pinyin_str(pinyin);
            g_free(pinyin);
            scratch.str(pinyin_str);
        }
        scratch.u32(freq);
    }

    out->append(scratch.buffer());
    return true;
}

} // namespace

extern "C" {

/// Dumps `user_bin` + `user_bigram_db` into a malloc'd buffer.
///
/// Returns 0 on success (`*out_buf`/`*out_len` describe the buffer, freed by
/// `legacy_dump_free`), -1 on failure with a NUL-terminated message in `err`.
int legacy_dump(const char *user_bin, const char *user_bigram_db,
                uint8_t **out_buf, size_t *out_len, char *err, size_t err_len) {
    *out_buf = nullptr;
    *out_len = 0;

    // 1. Load the user phrase index (`user.bin`), read-only.
    //
    // `FacadePhraseIndex::load` transfers ownership of the chunk to the
    // `SubPhraseIndex` it installs (`SubPhraseIndex::load` stores the pointer
    // and its destructor deletes it), so `chunk` must be heap-allocated and
    // must not be deleted here — on success or failure alike.
    pinyin::MemoryChunk *chunk = new pinyin::MemoryChunk();
    if (!chunk->load(user_bin)) {
        delete chunk;
        set_error(err, err_len, "cannot load user phrase index (read-only open failed)");
        return -1;
    }
    pinyin::FacadePhraseIndex phrase_index;
    if (!phrase_index.load(USER_DICTIONARY, chunk)) {
        set_error(err, err_len, "cannot load user phrase index (facade load failed)");
        return -1;
    }

    // 2. Load the user bigram (`user_bigram.db`), read-only.
    pinyin::Bigram bigram;
    if (!bigram.load_db(user_bigram_db)) {
        set_error(err, err_len, "cannot load user bigram database");
        return -1;
    }

    // 3. Iterate the user phrase range.
    PhraseIndexRange range;
    if (phrase_index.get_range(USER_DICTIONARY, range) != ERROR_OK) {
        set_error(err, err_len, "cannot get user phrase index range");
        return -1;
    }

    const uint32_t tokens_iterated = range.m_range_end - range.m_range_begin;
    uint32_t null_slots = 0;
    uint32_t corrupt = 0;
    Writer phrases;
    uint32_t phrase_count = 0;

    for (phrase_token_t token = range.m_range_begin; token < range.m_range_end; ++token) {
        pinyin::PhraseItem item;
        int status = phrase_index.get_phrase_item(token, item);
        if (status == ERROR_NO_ITEM) {
            ++null_slots;
            continue;
        }
        if (status != ERROR_OK) {
            ++corrupt;
            continue;
        }
        if (!serialize_phrase(item, token, &phrases)) {
            ++corrupt;
            continue;
        }
        ++phrase_count;
    }

    // 4. Iterate the bigram predecessors.
    GArray *items = g_array_new(FALSE, FALSE, sizeof(phrase_token_t));
    if (!bigram.get_all_items(items)) {
        g_array_free(items, TRUE);
        set_error(err, err_len, "cannot enumerate user bigram predecessors");
        return -1;
    }

    // Deterministic order: the DBM iteration order is unspecified, so sort
    // the predecessor tokens ascending.
    phrase_token_t *predecessors =
        reinterpret_cast<phrase_token_t *>(items->data);
    std::sort(predecessors, predecessors + items->len);

    Writer bigrams;
    uint32_t bigram_count = 0;
    for (guint i = 0; i < items->len; ++i) {
        const phrase_token_t prev = g_array_index(items, phrase_token_t, i);
        pinyin::SingleGram *single_gram = nullptr;
        if (!bigram.load(prev, single_gram, /*copy=*/true)) {
            ++corrupt;
            continue;
        }
        guint32 total_freq = 0;
        single_gram->get_total_freq(total_freq);

        BigramPhraseWithCountArray successors =
            g_array_new(FALSE, FALSE, sizeof(BigramPhraseItemWithCount));
        single_gram->retrieve_all(successors);

        bigrams.u32(prev);
        bigrams.u32(total_freq);
        bigrams.u32(successors->len);
        for (guint s = 0; s < successors->len; ++s) {
            const BigramPhraseItemWithCount item =
                g_array_index(successors, BigramPhraseItemWithCount, s);
            bigrams.u32(item.m_token);
            bigrams.u32(item.m_count);
        }
        ++bigram_count;

        g_array_free(successors, TRUE);
        delete single_gram;
    }
    g_array_free(items, TRUE);

    // 5. Assemble the final buffer: header + phrase records + bigram records.
    Writer out;
    out.bytes(kMagic, sizeof(kMagic));
    out.u32(kVersion);
    out.u32(tokens_iterated);
    out.u32(null_slots);
    out.u32(corrupt);
    out.u32(phrase_count);
    out.u32(bigram_count);
    out.append(phrases.buffer());
    out.append(bigrams.buffer());

    const std::vector<uint8_t> &final = out.buffer();
    uint8_t *buf = static_cast<uint8_t *>(std::malloc(final.size()));
    if (buf == nullptr && !final.empty()) {
        set_error(err, err_len, "out of memory");
        return -1;
    }
    std::memcpy(buf, final.data(), final.size());
    *out_buf = buf;
    *out_len = final.size();
    return 0;
}

void legacy_dump_free(uint8_t *buf) { std::free(buf); }

} // extern "C"
