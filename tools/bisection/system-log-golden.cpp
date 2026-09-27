// Exact SYSTEM_FILE state from persistence.rs::cross_backend_state.
#include "config.h"
#include "phrase_index.h"
#include <cstring>
using namespace pinyin;
int main(int argc, char **argv) {
    if (argc != 2) return 2;
    ucs4_t phrase[] = {0x4f60};
    guint16 packed = 0x0097; // initial=23, middle=0, final=1, tone=0
    ChewingKey key;
    static_assert(sizeof key == sizeof packed);
    std::memcpy(&key, &packed, sizeof key);
    PhraseItem old_item, new_item;
    old_item.set_phrase_string(1, phrase);
    old_item.add_pronunciation(&key, 100);
    new_item.set_phrase_string(1, phrase);
    new_item.add_pronunciation(&key, 169);
    SubPhraseIndex old_index, new_index;
    if (old_index.add_phrase_item(1, &old_item) != ERROR_OK
        || new_index.add_phrase_item(1, &new_item) != ERROR_OK
        || old_index.add_unigram_frequency(1, 100) != ERROR_OK
        || new_index.add_unigram_frequency(1, 583) != ERROR_OK) return 1;
    PhraseIndexLogger logger;
    MemoryChunk log;
    if (!new_index.diff(&old_index, &logger) || !logger.store(&log)
        || !log.save(argv[1])) return 1;
    return 0;
}
