// plant-oracle-bigram.cc — write raw user-bigram rows into libpinyin's
// user_bigram.db (selected native backend + SingleGram blob).
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

#if defined(ORACLE_BDB)
#include <db.h>
#elif defined(ORACLE_KC)
#include <kcstashdb.h>
#else
#include <tkrzw_dbm_hash.h>
#endif

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
#if defined(ORACLE_BDB)
    // Pin 074a2219, storage/ngram_bdb.cpp:110-120, read in the
    // work-bdb/src/libpinyin-074a2219c90feaf962d0d24f034514033ece5f99 tree: user bigrams are DB_HASH.
    DB *db = nullptr;
    int status = db_create(&db, nullptr, 0);
    if (status == 0)
        status = db->open(db, nullptr, path.c_str(), nullptr, DB_HASH, 0, 0);
    DBT key = {}, value = {};
    uint32_t key_token = prev;
    key.data = &key_token;
    key.size = sizeof(key_token);
    value.data = blob.data();
    value.size = static_cast<uint32_t>(blob.size());
    if (status == 0)
        status = db->put(db, nullptr, &key, &value, 0);
    if (db) {
        const int close_status = db->close(db, 0);
        if (status == 0) status = close_status;
    }
    if (status != 0) {
        fprintf(stderr, "plant %s: %s\n", path.c_str(), db_strerror(status));
        return 1;
    }
#elif defined(ORACLE_KC)
    // Pin 074a2219, src/storage/ngram_kyotodb.cpp:53-103, read in the
    // same work-bdb pin tree: user bigrams are a StashDB snapshot.
    kyotocabinet::StashDB db;
    bool ok = db.open("-", kyotocabinet::BasicDB::OREADER |
                           kyotocabinet::BasicDB::OWRITER |
                           kyotocabinet::BasicDB::OCREATE);
    if (ok) ok = db.load_snapshot(path);
    if (ok) ok = db.set(reinterpret_cast<const char *>(&prev), sizeof(prev),
                        blob.data(), blob.size());
    if (ok) {
        // Match the pin's save_db: replace the snapshot, not a file HashDB.
        if (std::remove(path.c_str()) != 0) {
            std::perror(path.c_str());
            return 1;
        }
        ok = db.dump_snapshot(path);
    }
    if (!ok) fprintf(stderr, "plant %s: %s\n", path.c_str(), db.error().message());
    const bool closed = db.close();
    if (!closed) fprintf(stderr, "close %s: %s\n", path.c_str(), db.error().message());
    if (!ok || !closed) return 1;
#else
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
#endif
    return 0;
}
