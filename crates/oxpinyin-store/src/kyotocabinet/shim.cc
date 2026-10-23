// Kyoto Cabinet shim (#1098): the four classes libpinyin uses, constructed
// directly, behind the C entry points of shim.h.
//
// kclangc.h reaches every class through PolyDB, whose open() parses a
// "#type=" parameter and can construct any of ten classes; libkyotocabinet's
// text holds all of them. libpinyin instead instantiates TreeDB, HashDB,
// ProtoTreeDB and StashDB from the headers in its own text. This shim does
// the same, so only those classes are compiled into the facade.
//
// With no open parameter besides "#type=", PolyDB::open constructs the class
// untuned and calls its open(path, mode); on failure it copies the inner
// error code and message (kcpolydb.h:438-748, Kyoto Cabinet 1.2.80). Here the
// inner object keeps its own error, so the codes and messages are the same.
//
// No C++ exception leaves this file. Every entry point catches all of them
// and returns the failure value of its C signature; that much always holds.
// Where a database exists, fail() also records a SYSTEM error on it, so the
// caller's error code names the cause. That recording is best effort: if it
// throws in turn, the exception is dropped and the code is whatever the
// database last held.

#include "shim.h"

#include <kchashdb.h>
#include <kcplantdb.h>
#include <kcprotodb.h>
#include <kcstashdb.h>

#include <new>

namespace kc = kyotocabinet;

static_assert(int{OXKC_OREADER} == int{kc::BasicDB::OREADER}, "OREADER");
static_assert(int{OXKC_OWRITER} == int{kc::BasicDB::OWRITER}, "OWRITER");
static_assert(int{OXKC_OCREATE} == int{kc::BasicDB::OCREATE}, "OCREATE");

struct oxkc_db {
  kc::BasicDB* db;
  int32_t type;
};

struct oxkc_cur {
  kc::BasicDB::Cursor* cur;
};

namespace {

const char kException[] = "C++ exception in the Kyoto Cabinet shim";

// Best effort: the caller returns its failure value whether or not this
// records anything.
void fail(oxkc_db* db) noexcept {
  try {
    db->db->set_error(_KCCODELINE_, kc::BasicDB::Error::SYSTEM, kException);
  } catch (...) {
  }
}

void fail(oxkc_cur* cur) noexcept {
  try {
    cur->cur->db()->set_error(_KCCODELINE_, kc::BasicDB::Error::SYSTEM, kException);
  } catch (...) {
  }
}

// kclangc's visitor adapter, with a null return read as NOP.
class Visitor : public kc::DB::Visitor {
 public:
  Visitor(oxkc_visit_full full, oxkc_visit_empty empty, void* opq)
      : full_(full), empty_(empty), opq_(opq) {}

 private:
  const char* visit_full(const char* kbuf, size_t ksiz, const char* vbuf, size_t vsiz,
                         size_t* sp) override {
    if (!full_) return NOP;
    const char* result = full_(kbuf, ksiz, vbuf, vsiz, sp, opq_);
    return result ? result : NOP;
  }

  const char* visit_empty(const char* kbuf, size_t ksiz, size_t* sp) override {
    if (!empty_) return NOP;
    const char* result = empty_(kbuf, ksiz, sp, opq_);
    return result ? result : NOP;
  }

  oxkc_visit_full full_;
  oxkc_visit_empty empty_;
  void* opq_;
};

}  // namespace

extern "C" {

const char* oxkc_version(void) noexcept { return kc::VERSION; }

int32_t oxkc_enorec(void) noexcept { return kc::BasicDB::Error::NOREC; }

const char* oxkc_ecodename(int32_t code) noexcept {
  return kc::BasicDB::Error::codename(static_cast<kc::BasicDB::Error::Code>(code));
}

oxkc_db* oxkc_db_new(int32_t type) noexcept {
  oxkc_db* db = new (std::nothrow) oxkc_db{nullptr, type};
  if (!db) return nullptr;
  try {
    switch (type) {
      case OXKC_TREE: db->db = new kc::TreeDB; break;
      case OXKC_HASH: db->db = new kc::HashDB; break;
      case OXKC_STASH: db->db = new kc::StashDB; break;
      case OXKC_PROTO_TREE: db->db = new kc::ProtoTreeDB; break;
      default: break;
    }
  } catch (...) {
  }
  if (!db->db) {
    delete db;
    return nullptr;
  }
  return db;
}

int32_t oxkc_db_tune_buckets(oxkc_db* db, int64_t bnum) noexcept {
  try {
    switch (db->type) {
      case OXKC_TREE: return static_cast<kc::TreeDB*>(db->db)->tune_buckets(bnum);
      case OXKC_HASH: return static_cast<kc::HashDB*>(db->db)->tune_buckets(bnum);
      case OXKC_STASH: return static_cast<kc::StashDB*>(db->db)->tune_buckets(bnum);
      default:
        db->db->set_error(_KCCODELINE_, kc::BasicDB::Error::INVALID, "no bucket tuning");
        return 0;
    }
  } catch (...) {
    fail(db);
    return 0;
  }
}

int32_t oxkc_db_open(oxkc_db* db, const char* path, uint32_t mode) noexcept {
  try {
    return db->db->open(path, mode);
  } catch (...) {
    fail(db);
    return 0;
  }
}

int32_t oxkc_db_close(oxkc_db* db) noexcept {
  try {
    return db->db->close();
  } catch (...) {
    fail(db);
    return 0;
  }
}

void oxkc_db_del(oxkc_db* db) noexcept {
  try {
    delete db->db;
  } catch (...) {
  }
  delete db;
}

int32_t oxkc_db_ecode(oxkc_db* db) noexcept {
  try {
    return db->db->error().code();
  } catch (...) {
    return kc::BasicDB::Error::SYSTEM;
  }
}

const char* oxkc_db_emsg(oxkc_db* db) noexcept {
  try {
    return db->db->error().message();
  } catch (...) {
    return kException;
  }
}

int32_t oxkc_db_accept(oxkc_db* db, const char* kbuf, size_t ksiz, oxkc_visit_full full,
                       oxkc_visit_empty empty, void* opq, int32_t writable) noexcept {
  try {
    Visitor visitor(full, empty, opq);
    return db->db->accept(kbuf, ksiz, &visitor, writable);
  } catch (...) {
    fail(db);
    return 0;
  }
}

int32_t oxkc_db_check(oxkc_db* db, const char* kbuf, size_t ksiz) noexcept {
  try {
    return db->db->check(kbuf, ksiz);
  } catch (...) {
    fail(db);
    return -1;
  }
}

int32_t oxkc_db_iterate(oxkc_db* db, oxkc_visit_full full, void* opq, int32_t writable) noexcept {
  try {
    Visitor visitor(full, nullptr, opq);
    return db->db->iterate(&visitor, writable);
  } catch (...) {
    fail(db);
    return 0;
  }
}

int32_t oxkc_db_set(oxkc_db* db, const char* kbuf, size_t ksiz, const char* vbuf,
                    size_t vsiz) noexcept {
  try {
    return db->db->set(kbuf, ksiz, vbuf, vsiz);
  } catch (...) {
    fail(db);
    return 0;
  }
}

int32_t oxkc_db_remove(oxkc_db* db, const char* kbuf, size_t ksiz) noexcept {
  try {
    return db->db->remove(kbuf, ksiz);
  } catch (...) {
    fail(db);
    return 0;
  }
}

int32_t oxkc_db_sync(oxkc_db* db, int32_t hard) noexcept {
  try {
    return db->db->synchronize(hard);
  } catch (...) {
    fail(db);
    return 0;
  }
}

int32_t oxkc_db_load_snapshot(oxkc_db* db, const char* src) noexcept {
  try {
    return db->db->load_snapshot(src);
  } catch (...) {
    fail(db);
    return 0;
  }
}

int32_t oxkc_db_dump_snapshot(oxkc_db* db, const char* dest) noexcept {
  try {
    return db->db->dump_snapshot(dest);
  } catch (...) {
    fail(db);
    return 0;
  }
}

int64_t oxkc_db_count(oxkc_db* db) noexcept {
  try {
    return db->db->count();
  } catch (...) {
    fail(db);
    return -1;
  }
}

int32_t oxkc_db_begin_transaction(oxkc_db* db, int32_t hard) noexcept {
  try {
    return db->db->begin_transaction(hard);
  } catch (...) {
    fail(db);
    return 0;
  }
}

int32_t oxkc_db_end_transaction(oxkc_db* db, int32_t commit) noexcept {
  try {
    return db->db->end_transaction(commit);
  } catch (...) {
    fail(db);
    return 0;
  }
}

oxkc_cur* oxkc_db_cursor(oxkc_db* db) noexcept {
  oxkc_cur* cur = new (std::nothrow) oxkc_cur{nullptr};
  if (!cur) return nullptr;
  try {
    cur->cur = db->db->cursor();
  } catch (...) {
    fail(db);
  }
  if (!cur->cur) {
    delete cur;
    return nullptr;
  }
  return cur;
}

void oxkc_cur_del(oxkc_cur* cur) noexcept {
  try {
    delete cur->cur;
  } catch (...) {
  }
  delete cur;
}

int32_t oxkc_cur_jump(oxkc_cur* cur) noexcept {
  try {
    return cur->cur->jump();
  } catch (...) {
    fail(cur);
    return 0;
  }
}

int32_t oxkc_cur_jump_key(oxkc_cur* cur, const char* kbuf, size_t ksiz) noexcept {
  try {
    return cur->cur->jump(kbuf, ksiz);
  } catch (...) {
    fail(cur);
    return 0;
  }
}

int32_t oxkc_cur_accept(oxkc_cur* cur, oxkc_visit_full full, void* opq, int32_t writable,
                        int32_t step) noexcept {
  try {
    Visitor visitor(full, nullptr, opq);
    return cur->cur->accept(&visitor, writable, step);
  } catch (...) {
    fail(cur);
    return 0;
  }
}

int32_t oxkc_cur_ecode(oxkc_cur* cur) noexcept {
  try {
    return cur->cur->error().code();
  } catch (...) {
    return kc::BasicDB::Error::SYSTEM;
  }
}

}  // extern "C"
