// See adapter.h for what this file is and is not allowed to contain.
#include "oxpinyin-store/src/tkrzw/adapter.h"

#include <string_view>
#include <utility>

#include <tkrzw_dbm_baby.h>
#include <tkrzw_dbm_hash.h>
#include <tkrzw_dbm_tiny.h>
#include <tkrzw_dbm_tree.h>
#include <tkrzw_file.h>
#include <tkrzw_lib_common.h>

#include "oxpinyin-store/src/tkrzw/bridge.rs.h"

namespace oxpinyin {
namespace tkrzw {

namespace {

// The numbers the Rust side matches on are tkrzw's own; an upstream
// renumbering fails the build instead of misclassifying a status.
static_assert(::tkrzw::Status::SUCCESS == 0);
static_assert(::tkrzw::Status::SYSTEM_ERROR == 2);
static_assert(::tkrzw::Status::PRECONDITION_ERROR == 4);
static_assert(::tkrzw::Status::NOT_FOUND_ERROR == 7);
static_assert(::tkrzw::File::OPEN_DEFAULT == 0);
static_assert(::tkrzw::File::OPEN_NO_CREATE == (1 << 1));

// A Rust byte slice as the `std::string_view` the DBM methods take. The
// view borrows the slice's bytes for the duration of one call; none of the
// methods below retains it (`Set` copies the key and value into the record,
// `tkrzw_dbm.h:1123-1131`).
std::string_view view(rust::Slice<const std::uint8_t> bytes) {
  return std::string_view(reinterpret_cast<const char*>(bytes.data()), bytes.size());
}

// `tkrzw::Status` as the bridge's plain struct: the code, and the message
// only when the library attached one, so a success allocates nothing.
NativeStatus wrap(const ::tkrzw::Status& status) {
  NativeStatus out;
  out.code = static_cast<std::int32_t>(status.GetCode());
  if (status.HasMessage()) {
    out.message = std::make_unique<std::string>(status.GetMessage());
  }
  return out;
}

// The record operations below mutate a database the Rust side holds behind
// a shared reference, as the pin's `const` search methods mutate `m_db`
// through a pointer (`chewing_large_table2_tkrzwdb.cpp:133-146`). The
// classes permit it: "All operations are thread-safe; Multiple threads can
// access the same database concurrently" (`tkrzw_dbm_tree.h:36-40`,
// `tkrzw_dbm_hash.h:35-39`, `tkrzw_dbm_baby.h:35-39`,
// `tkrzw_dbm_tiny.h:34-38`); each implementation takes its own mutex
// (`tkrzw_dbm_hash.cc:399`, `tkrzw_dbm_tree.cc:562`). The objects are
// never defined `const` — `new_*_dbm` allocate them mutable — so the cast
// is well defined.
::tkrzw::DBM& mutable_dbm(const ::tkrzw::DBM& dbm) {
  return const_cast<::tkrzw::DBM&>(dbm);
}

// The pin's `KeyCollectProcessor` (`ngram_tkrzwdb.cpp:143-159`) and
// `FlexibleKeyCollectProcessor` (`flexible_ngram_tkrzwdb.h:39-60`) without
// their length checks, which stay in Rust: every key the walk visits is
// copied, in the walk's order, and every record is left as it is (`NOOP`).
// `ProcessEach` also calls `ProcessEmpty` once before and once after the
// walk (`tkrzw_dbm.h:1535-1544`); those calls carry no record.
class KeyCollector final : public ::tkrzw::DBM::RecordProcessor {
 public:
  explicit KeyCollector(std::vector<std::string>& keys) : keys_(keys) {}

  std::string_view ProcessFull(std::string_view key, std::string_view) override {
    keys_.emplace_back(key);
    return NOOP;
  }

  std::string_view ProcessEmpty(std::string_view) override {
    return NOOP;
  }

 private:
  std::vector<std::string>& keys_;
};

}  // namespace

std::unique_ptr<::tkrzw::DBM> new_tree_dbm() {
  return std::make_unique<::tkrzw::TreeDBM>();
}

std::unique_ptr<::tkrzw::DBM> new_hash_dbm() {
  return std::make_unique<::tkrzw::HashDBM>();
}

std::unique_ptr<::tkrzw::DBM> new_baby_dbm() {
  return std::make_unique<::tkrzw::BabyDBM>();
}

std::unique_ptr<::tkrzw::DBM> new_tiny_dbm() {
  return std::make_unique<::tkrzw::TinyDBM>();
}

NativeStatus open(::tkrzw::DBM& dbm, rust::Slice<const std::uint8_t> path, bool writable,
                  std::int32_t options) {
  return wrap(dbm.Open(std::string(view(path)), writable, options));
}

NativeStatus close(::tkrzw::DBM& dbm) {
  return wrap(dbm.Close());
}

bool is_open(const ::tkrzw::DBM& dbm) {
  return dbm.IsOpen();
}

NativeStatus get(const ::tkrzw::DBM& dbm, rust::Slice<const std::uint8_t> key,
                 std::string& value) {
  return wrap(mutable_dbm(dbm).Get(view(key), &value));
}

NativeStatus check(const ::tkrzw::DBM& dbm, rust::Slice<const std::uint8_t> key) {
  return wrap(mutable_dbm(dbm).Get(view(key), nullptr));
}

NativeStatus set(const ::tkrzw::DBM& dbm, rust::Slice<const std::uint8_t> key,
                 rust::Slice<const std::uint8_t> value) {
  return wrap(mutable_dbm(dbm).Set(view(key), view(value)));
}

NativeStatus remove(const ::tkrzw::DBM& dbm, rust::Slice<const std::uint8_t> key) {
  return wrap(mutable_dbm(dbm).Remove(view(key)));
}

NativeStatus synchronize(const ::tkrzw::DBM& dbm, bool hard) {
  return wrap(mutable_dbm(dbm).Synchronize(hard));
}

NativeStatus count(const ::tkrzw::DBM& dbm, std::int64_t& count) {
  return wrap(mutable_dbm(dbm).Count(&count));
}

bool is_healthy(const ::tkrzw::DBM& dbm) {
  return dbm.IsHealthy();
}

NativeStatus collect_keys(const ::tkrzw::DBM& dbm, std::vector<std::string>& keys) {
  KeyCollector collector(keys);
  return wrap(mutable_dbm(dbm).ProcessEach(&collector, false));
}

std::unique_ptr<DbmIterator> make_iterator(const ::tkrzw::DBM& dbm) {
  return mutable_dbm(dbm).MakeIterator();
}

NativeStatus iter_first(DbmIterator& iter) {
  return wrap(iter.First());
}

NativeStatus iter_jump(DbmIterator& iter, rust::Slice<const std::uint8_t> key) {
  return wrap(iter.Jump(view(key)));
}

NativeStatus iter_next(DbmIterator& iter) {
  return wrap(iter.Next());
}

NativeStatus iter_get(DbmIterator& iter, std::string& key, std::string& value) {
  return wrap(iter.Get(&key, &value));
}

}  // namespace tkrzw
}  // namespace oxpinyin
