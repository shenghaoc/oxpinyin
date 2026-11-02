// The semantic adapter of the tkrzw bridge: what cxx cannot express about
// the pin's concrete tkrzw classes, and nothing more.
//
// libpinyin at 074a2219 allocates `TreeDBM`, `HashDBM`, `BabyDBM` and
// `TinyDBM` itself (`new TreeDBM`, `chewing_large_table2_tkrzwdb.cpp:87`;
// `new HashDBM`, `ngram_tkrzwdb.cpp:96`; `new BabyDBM`,
// `chewing_large_table2_tkrzwdb.cpp:57`; `new TinyDBM`,
// `ngram_tkrzwdb.cpp:52`) and calls them through a `DBM*` member. These
// functions give Rust the same shape: a concrete object behind
// `std::unique_ptr<tkrzw::DBM>` (the base class has a virtual destructor,
// `tkrzw_dbm.h:987`), opened — or not — by a separate `Open`, and every
// operation a virtual `DBM` call, as the pin's `m_db->` calls are.
//
// The adapter contains four kinds of code, each a construct cxx has no
// syntax for: constructors (`std::make_unique<Concrete>()`); conversions
// between Rust byte slices and `std::string_view`/`std::string`, and from
// `tkrzw::Status` to the `NativeStatus` the bridge returns; the
// `RecordProcessor` subclass of the key walk (the pin's
// `KeyCollectProcessor`, `ngram_tkrzwdb.cpp:143-159`, whose key length
// check stays in Rust); and the mutation of a database through a shared
// reference, which the classes' own locking permits. It calls the library.
// It copies and emulates none of it: the status codes it returns are
// tkrzw's own, and it catches no exception — the pin catches none either,
// so an exception leaves the bridge through a `noexcept` boundary and ends
// the process, as it ends the pin's.
#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

#include <tkrzw_dbm.h>

#include "rust/cxx.h"

namespace oxpinyin {
namespace tkrzw {

// Defined by the bridge (bridge.rs): a status code and, when the library
// attached one, its message.
struct NativeStatus;

// The pin's `std::unique_ptr<DBM::Iterator>` (`tkrzwdb_utils.h:52`).
using DbmIterator = ::tkrzw::DBM::Iterator;

// `new TreeDBM` / `new HashDBM` / `new BabyDBM` / `new TinyDBM` at the
// class's default tuning, not opened. The two on-memory classes are usable
// as they are; the two file classes answer PRECONDITION_ERROR until `open`
// succeeds (`tkrzw_dbm_hash.cc:400-401`, `tkrzw_dbm_tree.cc:563-564`).
std::unique_ptr<::tkrzw::DBM> new_tree_dbm();
std::unique_ptr<::tkrzw::DBM> new_hash_dbm();
std::unique_ptr<::tkrzw::DBM> new_baby_dbm();
std::unique_ptr<::tkrzw::DBM> new_tiny_dbm();

// `DBM::Open(path, writable, options)`. A failure leaves the object as it
// is; the caller decides whether to keep it, as the pin's owners do.
NativeStatus open(::tkrzw::DBM& dbm, rust::Slice<const std::uint8_t> path, bool writable,
                  std::int32_t options);
// `DBM::Close()`.
NativeStatus close(::tkrzw::DBM& dbm);
// `DBM::IsOpen()`.
bool is_open(const ::tkrzw::DBM& dbm);

// `DBM::Get(key, &value)`: the value is assigned into `value` on SUCCESS.
NativeStatus get(const ::tkrzw::DBM& dbm, rust::Slice<const std::uint8_t> key,
                 std::string& value);
// `DBM::Get(key, nullptr)`: existence without a value copy
// (`phrase_large_table3_tkrzwdb.cpp:166`).
NativeStatus check(const ::tkrzw::DBM& dbm, rust::Slice<const std::uint8_t> key);
// `DBM::Set(key, value)`: overwrite = true, old_value = nullptr, the pin's
// two-argument form.
NativeStatus set(const ::tkrzw::DBM& dbm, rust::Slice<const std::uint8_t> key,
                 rust::Slice<const std::uint8_t> value);
// `DBM::Remove(key)`: old_value = nullptr, the pin's one-argument form.
NativeStatus remove(const ::tkrzw::DBM& dbm, rust::Slice<const std::uint8_t> key);
// `DBM::Synchronize(hard)`: proc = nullptr, the pin's one-argument form.
NativeStatus synchronize(const ::tkrzw::DBM& dbm, bool hard);
// `DBM::Count(&count)`.
NativeStatus count(const ::tkrzw::DBM& dbm, std::int64_t& count);
// `DBM::IsHealthy()`.
bool is_healthy(const ::tkrzw::DBM& dbm);
// `DBM::ProcessEach(&processor, false)` with a processor that copies every
// key into `keys` in the walk's own order (`ngram_tkrzwdb.cpp:143-171`).
NativeStatus collect_keys(const ::tkrzw::DBM& dbm, std::vector<std::string>& keys);

// `DBM::MakeIterator()`. The iterator points back at its database and must
// not outlive it (`tkrzw_dbm_hash.cc:2285-2296`); Rust encodes the borrow.
std::unique_ptr<DbmIterator> make_iterator(const ::tkrzw::DBM& dbm);
// `Iterator::First()`, `Jump(key)`, `Next()`, and `Get(&key, &value)`,
// which assigns the current record into the two strings on SUCCESS.
NativeStatus iter_first(DbmIterator& iter);
NativeStatus iter_jump(DbmIterator& iter, rust::Slice<const std::uint8_t> key);
NativeStatus iter_next(DbmIterator& iter);
NativeStatus iter_get(DbmIterator& iter, std::string& key, std::string& value);

}  // namespace tkrzw
}  // namespace oxpinyin
