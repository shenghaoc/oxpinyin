// plant-oracle-bigram.cc — write raw user-bigram rows into libpinyin's
// user_bigram.db (Tkrzw HashDBM + SingleGram blob).
//
// Public pinyin_train first-seeds 69 (23*3), so counts 9 and 10 cannot
// be produced through the C ABI. This helper plants them so
// run-union-diff.sh can lock `_compute_predicted_bigram_candidates`
// (`pinyin.cpp:2311`, `:2349-2350`) against the pinned oracle.
//
// SingleGram on-disk layout (`src/storage/ngram.cpp:31-34`, `:178-204`):
//   guint32 total_freq;
//   {phrase_token_t token; guint32 freq;} items[], sorted by token.
// Hash key is the predecessor token (`ngram_tkrzwdb.cpp:108`).
//
// Usage: plant-oracle-bigram <userdir> <prev> <cur9> 9 <cur10> 10

#include <tkrzw_dbm_hash.h>

#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <vector>

struct Item {
    uint32_t token;
    uint32_t freq;
};

int main(int argc, char **argv) {
    if (argc != 7) {
        fprintf(stderr, "Usage: %s <userdir> <prev> <cur9> 9 <cur10> 10\n", argv[0]);
        return 1;
    }

    const uint32_t prev = static_cast<uint32_t>(strtoul(argv[2], nullptr, 0));
    const uint32_t cur9 = static_cast<uint32_t>(strtoul(argv[3], nullptr, 0));
    const uint32_t count9 = static_cast<uint32_t>(strtoul(argv[4], nullptr, 0));
    const uint32_t cur10 = static_cast<uint32_t>(strtoul(argv[5], nullptr, 0));
    const uint32_t count10 = static_cast<uint32_t>(strtoul(argv[6], nullptr, 0));
    if (count9 != 9 || count10 != 10) {
        fprintf(stderr, "fatal: this planter only writes the 9/10 filter edge\n");
        return 1;
    }

    std::vector<Item> items;
    items.push_back({cur9, count9});
    items.push_back({cur10, count10});
    if (items[0].token > items[1].token) {
        const Item tmp = items[0];
        items[0] = items[1];
        items[1] = tmp;
    }

    const uint32_t total = count9 + count10;
    std::vector<char> blob(sizeof(uint32_t) + sizeof(Item) * items.size());
    memcpy(blob.data(), &total, sizeof(total));
    memcpy(blob.data() + sizeof(uint32_t), items.data(), sizeof(Item) * items.size());

    const std::string path = std::string(argv[1]) + "/user_bigram.db";
    tkrzw::HashDBM db;
    const tkrzw::Status open_st = db.Open(path, true, tkrzw::File::OPEN_DEFAULT);
    if (!open_st.IsOK()) {
        fprintf(stderr, "open %s: %s\n", path.c_str(), open_st.GetMessage().c_str());
        return 1;
    }
    const std::string_view key(reinterpret_cast<const char *>(&prev), sizeof(prev));
    const std::string_view value(blob.data(), blob.size());
    const tkrzw::Status set_st = db.Set(key, value);
    if (!set_st.IsOK()) {
        fprintf(stderr, "set: %s\n", set_st.GetMessage().c_str());
        return 1;
    }
    db.Synchronize(false);
    db.Close();
    return 0;
}
