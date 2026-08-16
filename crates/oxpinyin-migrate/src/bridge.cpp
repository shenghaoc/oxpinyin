/// bridge.cpp — C linkage wrapper around libtkrzw HashDBM.
///
/// Exposes a minimal set of functions so oxpinyin-migrate can read
/// oracle Tkrzw tables via FFI without linking C++ directly.

#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>

#include <tkrzw_dbm_hash.h>

// ── opaque types ──────────────────────────────────────────────────────

struct TkrzwDB {
    tkrzw::HashDBM dbm;
    std::string last_error;
};

struct TkrzwIter {
    std::unique_ptr<tkrzw::DBM::Iterator> iter;
    std::string key_buf;
    std::string val_buf;
};

// ── database ──────────────────────────────────────────────────────────

extern "C" {

TkrzwDB *tkrzw_open(const char *path) {
    auto *db = new TkrzwDB();
    tkrzw::Status s = db->dbm.Open(path, false);
    if (s != tkrzw::Status::SUCCESS) {
        db->last_error = s.GetMessage();
    }
    return db;
}

void tkrzw_close(TkrzwDB *db) { delete db; }

const char *tkrzw_error(const TkrzwDB *db) { return db->last_error.c_str(); }

int64_t tkrzw_count(TkrzwDB *db) {
    int64_t count = -1;
    db->dbm.Count(&count);
    return count;
}

// ── iterator ──────────────────────────────────────────────────────────

TkrzwIter *tkrzw_iter_make(TkrzwDB *db) {
    auto *it = new TkrzwIter();
    it->iter = db->dbm.MakeIterator();
    return it;
}

void tkrzw_iter_free(TkrzwIter *it) { delete it; }

int tkrzw_iter_first(TkrzwIter *it) {
    auto s = it->iter->First();
    if (s != tkrzw::Status::SUCCESS) return 0;
    auto key_sv = it->iter->GetKey();
    auto val_sv = it->iter->GetValue();
    it->key_buf.assign(key_sv.data(), key_sv.size());
    it->val_buf.assign(val_sv.data(), val_sv.size());
    return 1;
}

int tkrzw_iter_next(TkrzwIter *it) {
    auto s = it->iter->Next();
    if (s != tkrzw::Status::SUCCESS) return 0;
    auto key_sv = it->iter->GetKey();
    auto val_sv = it->iter->GetValue();
    it->key_buf.assign(key_sv.data(), key_sv.size());
    it->val_buf.assign(val_sv.data(), val_sv.size());
    return 1;
}

const uint8_t *tkrzw_iter_key(const TkrzwIter *it, size_t *len) {
    *len = it->key_buf.size();
    return reinterpret_cast<const uint8_t *>(it->key_buf.data());
}

const uint8_t *tkrzw_iter_value(const TkrzwIter *it, size_t *len) {
    *len = it->val_buf.size();
    return reinterpret_cast<const uint8_t *>(it->val_buf.data());
}

} // extern "C"
