// See adapter.h for what this file is and is not allowed to contain.
#include "oxpinyin-store/src/kyotocabinet/adapter.h"

#include <kchashdb.h>
#include <kcplantdb.h>
#include <kcprotodb.h>
#include <kcstashdb.h>

#include "oxpinyin-store/src/kyotocabinet/bridge.rs.h"

namespace kc = ::kyotocabinet;

// `region` in ffi.rs bounds every key and value by `MEM_MAX_SIZE`, a Rust
// constant, so no slice costs a bridge call; this holds it to the library's.
static_assert(kc::MEMMAXSIZ == 0x3fffffff,
              "ffi.rs MEM_MAX_SIZE must equal kyotocabinet::MEMMAXSIZ");

namespace oxpinyin {
namespace kyotocabinet {

namespace {

// The region a method may read `slice.size()` bytes from. An empty Rust
// slice crosses as address 1 (`rust::Slice`'s dangling pointer, cxx.h), and
// address 1 is `Visitor::REMOVE` (`kcdb.h:46`): `set` returns its value
// pointer from its own visitor (`kcdb.h:1424-1445`), so a zero-length value
// at that address would delete the record instead of storing an empty one.
// Every empty region therefore gets this object's address, which is neither
// `NOP` (0) nor `REMOVE`; a non-empty slice points into a live allocation,
// which is neither either. Keys take the same path: the classes assert a
// non-null key (`kchashdb.h:557`) and hash or copy zero bytes from it.
const char* bytes(rust::Slice<const std::uint8_t> slice) {
  static const char kEmpty = 0;
  if (slice.empty()) return &kEmpty;
  return reinterpret_cast<const char*>(slice.data());
}

rust::Slice<const std::uint8_t> view(const char* buf, std::size_t size) {
  return rust::Slice<const std::uint8_t>(reinterpret_cast<const std::uint8_t*>(buf), size);
}

// The record operations below mutate a database the Rust side holds behind
// a shared reference, as the pin's `const` search methods mutate `m_db`
// through a pointer (`chewing_large_table2_kyotodb.cpp:148-179`). The
// classes permit it: every record operation takes the object's own
// reader-writer lock (`kchashdb.h:556-566`, `kcplantdb.h:1042-1048`,
// `kcprotodb.h:376`, `kcstashdb.h:348`). The objects are never defined
// `const` — `new_*_db` allocate them mutable — so the cast is well defined.
kc::BasicDB& mutable_db(const kc::BasicDB& db) {
  return const_cast<kc::BasicDB&>(db);
}

// The pin's visitors read a record and leave it as it is (`NOP`,
// `chewing_large_table2_kyotodb.cpp:446-511`, `ngram_kyotodb.cpp:163-184`).
// This one copies the record into the Rust sink and returns `NOP` by name.
// The regions are valid for the call only, under the record's lock
// (`kcdb.h:268-271`); the sink copies them before it returns and makes no
// database call. With `NOP` returned, the library reads neither `*sp` nor
// a replacement region (`kchashdb.h:206-216`).
class Forward final : public kc::DB::Visitor {
 public:
  explicit Forward(Sink& sink) : sink_(sink) {}

  const char* visit_full(const char* kbuf, std::size_t ksiz, const char* vbuf,
                         std::size_t vsiz, std::size_t*) override {
    sink_.full(view(kbuf, ksiz), view(vbuf, vsiz));
    return NOP;
  }

  const char* visit_empty(const char*, std::size_t, std::size_t*) override {
    return NOP;
  }

 private:
  Sink& sink_;
};

}  // namespace

rust::String version() {
  return rust::String::lossy(kc::VERSION);
}

std::unique_ptr<kc::BasicDB> new_tree_db() {
  return std::make_unique<kc::TreeDB>();
}

std::unique_ptr<kc::BasicDB> new_hash_db() {
  return std::make_unique<kc::HashDB>();
}

std::unique_ptr<kc::BasicDB> new_stash_db() {
  return std::make_unique<kc::StashDB>();
}

std::unique_ptr<kc::BasicDB> new_proto_tree_db() {
  return std::make_unique<kc::ProtoTreeDB>();
}

ErrorCode error_code(const kc::BasicDB& db) {
  return db.error().code();
}

NativeError last_error(const kc::BasicDB& db) {
  const kc::BasicDB::Error error = db.error();
  NativeError out;
  out.code = error.code();
  out.name = rust::String::lossy(error.name());
  out.message = rust::String::lossy(error.message());
  return out;
}

std::int32_t check(const kc::BasicDB& db, rust::Slice<const std::uint8_t> key) {
  return mutable_db(db).check(bytes(key), key.size());
}

bool get(const kc::BasicDB& db, rust::Slice<const std::uint8_t> key, Sink& sink) {
  Forward visitor(sink);
  return mutable_db(db).accept(bytes(key), key.size(), &visitor, false);
}

bool set(const kc::BasicDB& db, rust::Slice<const std::uint8_t> key,
         rust::Slice<const std::uint8_t> value) {
  return mutable_db(db).set(bytes(key), key.size(), bytes(value), value.size());
}

bool remove(const kc::BasicDB& db, rust::Slice<const std::uint8_t> key) {
  return mutable_db(db).remove(bytes(key), key.size());
}

bool iterate(const kc::BasicDB& db, Sink& sink) {
  Forward visitor(sink);
  return mutable_db(db).iterate(&visitor, false);
}

std::int64_t count(const kc::BasicDB& db) {
  return mutable_db(db).count();
}

bool synchronize(const kc::BasicDB& db, bool hard) {
  return mutable_db(db).synchronize(hard);
}

bool begin_transaction(const kc::BasicDB& db, bool hard) {
  return mutable_db(db).begin_transaction(hard);
}

bool end_transaction(const kc::BasicDB& db, bool commit) {
  return mutable_db(db).end_transaction(commit);
}

bool load_snapshot(const kc::BasicDB& db, const std::string& path) {
  return mutable_db(db).load_snapshot(path);
}

bool dump_snapshot(const kc::BasicDB& db, const std::string& path) {
  return mutable_db(db).dump_snapshot(path);
}

std::unique_ptr<DbCursor> make_cursor(const kc::BasicDB& db) {
  return std::unique_ptr<DbCursor>(mutable_db(db).cursor());
}

bool jump_to(DbCursor& cursor, rust::Slice<const std::uint8_t> key) {
  return cursor.jump(bytes(key), key.size());
}

bool cursor_accept(DbCursor& cursor, Sink& sink, bool step) {
  Forward visitor(sink);
  return cursor.accept(&visitor, false, step);
}

}  // namespace kyotocabinet
}  // namespace oxpinyin
