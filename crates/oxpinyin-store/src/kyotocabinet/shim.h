// The C++ side of the Kyoto Cabinet bridge (`ffi.rs`).
//
// A narrow, owned shim over Kyoto Cabinet's C++ classes. It constructs the
// concrete class libpinyin constructs for each container (TreeDB, HashDB,
// StashDB, ProtoTreeDB) and exposes exactly the operations `ffi.rs` binds:
// byte slices in, status codes out, and records handed to a Rust
// collector from inside a visitor. Kyoto Cabinet's engine is header code,
// so these classes are compiled into this object from the build-time
// headers, as they are into libpinyin's; the system libkyotocabinet
// supplies the rest (files, threads, locks, utilities).
//
// No function here throws on purpose. An exception Kyoto Cabinet raises
// (`std::bad_alloc` from `new`) reaches cxx's noexcept boundary and ends
// the process with std::terminate.
#pragma once

#include <kccommon.h>
#include <kcdb.h>

#include <cstdint>
#include <memory>

#include "rust/cxx.h"

namespace oxpinyin {
namespace kc {

// Rust collectors (`ffi.rs`); a visitor copies a record into one.
struct PointRead;
struct RecordSink;
struct Records;
struct KeySink;

class Cursor;

// One database object of a concrete class, closed and deleted on drop.
class Db {
 public:
  explicit Db(kyotocabinet::BasicDB* db);
  ~Db();
  Db(const Db&) = delete;
  Db& operator=(const Db&) = delete;

  bool open(rust::Slice<const uint8_t> path, uint32_t mode);
  bool tune_buckets(int64_t bnum);
  int32_t ecode() const;
  rust::Slice<const uint8_t> emsg() const;

  bool accept_get(rust::Slice<const uint8_t> key, PointRead& out) const;
  bool set(rust::Slice<const uint8_t> key, rust::Slice<const uint8_t> value) const;
  bool remove(rust::Slice<const uint8_t> key) const;
  bool iterate_records(Records& out) const;
  bool iterate_keys(KeySink& out) const;
  int64_t count() const;
  bool sync(bool hard) const;
  bool begin_transaction(bool hard) const;
  bool end_transaction(bool commit) const;
  bool load_snapshot(rust::Slice<const uint8_t> path) const;
  bool dump_snapshot(rust::Slice<const uint8_t> path) const;
  std::unique_ptr<Cursor> cursor() const;

 private:
  kyotocabinet::BasicDB* db_;
};

// A cursor over one Db, deleted on drop; Rust keeps it inside the Db's
// lifetime.
class Cursor {
 public:
  explicit Cursor(kyotocabinet::BasicDB::Cursor* cursor);
  ~Cursor();
  Cursor(const Cursor&) = delete;
  Cursor& operator=(const Cursor&) = delete;

  bool jump();
  bool jump_key(rust::Slice<const uint8_t> key);
  bool accept_record(RecordSink& out, bool step);
  int32_t ecode() const;

 private:
  kyotocabinet::BasicDB::Cursor* cursor_;
};

// `which`: 0 TreeDB, 1 HashDB, 2 StashDB, 3 ProtoTreeDB (`DbType`).
std::unique_ptr<Db> db_new(uint8_t which);
rust::Slice<const uint8_t> codename(int32_t code);
rust::Slice<const uint8_t> version();

}  // namespace kc
}  // namespace oxpinyin
