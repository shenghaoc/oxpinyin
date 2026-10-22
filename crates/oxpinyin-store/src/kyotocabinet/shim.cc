// See shim.h.
#include "oxpinyin-store/src/kyotocabinet/shim.h"

#include <kchashdb.h>
#include <kcprotodb.h>
#include <kcstashdb.h>

#include <cstring>
#include <string>

#include "oxpinyin-store/src/kyotocabinet/ffi.rs.h"

namespace oxpinyin {
namespace kc {

namespace {

using kyotocabinet::BasicDB;
using kyotocabinet::DB;

// The constants `ffi.rs` names by value.
static_assert(BasicDB::Error::NOREC == 7, "ffi.rs NO_RECORD");
static_assert(BasicDB::OREADER == 1 << 0 && BasicDB::OWRITER == 1 << 1 &&
                  BasicDB::OCREATE == 1 << 2,
              "ffi.rs open modes");

// The pointer Kyoto Cabinet may read `bytes.size()` bytes through. An
// empty slice's data pointer is a dangling non-null address, and a value
// pointer equal to `Visitor::REMOVE` (address 1) deletes the record
// instead of storing an empty one; `ffi.rs::c_ptr` documents the hazard
// for the C API. Every byte pointer crossing into Kyoto Cabinet goes
// through here.
const char* bytes(rust::Slice<const uint8_t> slice) {
  static const char empty = 0;
  return slice.empty() ? &empty : reinterpret_cast<const char*>(slice.data());
}

rust::Slice<const uint8_t> view(const char* buf, size_t size) {
  static const uint8_t empty = 0;
  if (size == 0) return rust::Slice<const uint8_t>(&empty, 0);
  return rust::Slice<const uint8_t>(reinterpret_cast<const uint8_t*>(buf), size);
}

rust::Slice<const uint8_t> text(const char* str) {
  return view(str, std::strlen(str));
}

std::string path_of(rust::Slice<const uint8_t> path) {
  return std::string(reinterpret_cast<const char*>(path.data()), path.size());
}

// A visitor that copies each record into a Rust collector and changes
// nothing. It runs under Kyoto Cabinet's lock and never calls back into
// the database.
template <typename Sink, void (*Visit)(Sink&, rust::Slice<const uint8_t>,
                                       rust::Slice<const uint8_t>)>
class Collect : public DB::Visitor {
 public:
  explicit Collect(Sink& sink) : sink_(sink) {}

 private:
  const char* visit_full(const char* kbuf, size_t ksiz, const char* vbuf,
                         size_t vsiz, size_t*) override {
    Visit(sink_, view(kbuf, ksiz), view(vbuf, vsiz));
    return NOP;
  }
  const char* visit_empty(const char*, size_t, size_t*) override {
    return NOP;
  }

  Sink& sink_;
};

}  // namespace

Db::Db(BasicDB* db) : db_(db) {}

Db::~Db() {
  // `kcdbclose` then `kcdbdel`, as the C binding's handle does.
  db_->close();
  delete db_;
}

bool Db::open(rust::Slice<const uint8_t> path, uint32_t mode) {
  return db_->open(path_of(path), mode);
}

bool Db::tune_buckets(int64_t bnum) {
  if (auto* hash = dynamic_cast<kyotocabinet::HashDB*>(db_)) return hash->tune_buckets(bnum);
  if (auto* tree = dynamic_cast<kyotocabinet::TreeDB*>(db_)) return tree->tune_buckets(bnum);
  return false;
}

int32_t Db::ecode() const { return db_->error().code(); }

rust::Slice<const uint8_t> Db::emsg() const { return text(db_->error().message()); }

bool Db::accept_get(rust::Slice<const uint8_t> key, PointRead& out) const {
  Collect<PointRead, visit_point> visitor(out);
  return db_->accept(bytes(key), key.size(), &visitor, false);
}

bool Db::set(rust::Slice<const uint8_t> key, rust::Slice<const uint8_t> value) const {
  return db_->set(bytes(key), key.size(), bytes(value), value.size());
}

bool Db::remove(rust::Slice<const uint8_t> key) const {
  return db_->remove(bytes(key), key.size());
}

bool Db::iterate_records(Records& out) const {
  Collect<Records, visit_records> visitor(out);
  return db_->iterate(&visitor, false);
}

bool Db::iterate_keys(KeySink& out) const {
  Collect<KeySink, visit_keys> visitor(out);
  return db_->iterate(&visitor, false);
}

int64_t Db::count() const { return db_->count(); }

bool Db::sync(bool hard) const { return db_->synchronize(hard, NULL, NULL); }

bool Db::begin_transaction(bool hard) const { return db_->begin_transaction(hard); }

bool Db::end_transaction(bool commit) const { return db_->end_transaction(commit); }

bool Db::load_snapshot(rust::Slice<const uint8_t> path) const {
  return db_->load_snapshot(path_of(path), NULL);
}

bool Db::dump_snapshot(rust::Slice<const uint8_t> path) const {
  return db_->dump_snapshot(path_of(path), NULL);
}

std::unique_ptr<Cursor> Db::cursor() const {
  return std::unique_ptr<Cursor>(new Cursor(db_->cursor()));
}

Cursor::Cursor(BasicDB::Cursor* cursor) : cursor_(cursor) {}

Cursor::~Cursor() { delete cursor_; }

bool Cursor::jump() { return cursor_->jump(); }

bool Cursor::jump_key(rust::Slice<const uint8_t> key) {
  return cursor_->jump(bytes(key), key.size());
}

bool Cursor::accept_record(RecordSink& out, bool step) {
  Collect<RecordSink, visit_record> visitor(out);
  return cursor_->accept(&visitor, false, step);
}

int32_t Cursor::ecode() const { return cursor_->error().code(); }

std::unique_ptr<Db> db_new(uint8_t which) {
  BasicDB* db = NULL;
  switch (which) {
    case 0: db = new kyotocabinet::TreeDB; break;
    case 1: db = new kyotocabinet::HashDB; break;
    case 2: db = new kyotocabinet::StashDB; break;
    default: db = new kyotocabinet::ProtoTreeDB; break;
  }
  return std::unique_ptr<Db>(new Db(db));
}

rust::Slice<const uint8_t> codename(int32_t code) {
  return text(BasicDB::Error::codename(static_cast<BasicDB::Error::Code>(code)));
}

rust::Slice<const uint8_t> version() { return text(kyotocabinet::VERSION); }

}  // namespace kc
}  // namespace oxpinyin
