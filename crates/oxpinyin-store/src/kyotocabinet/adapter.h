// The semantic adapter of the Kyoto Cabinet bridge: what cxx cannot express
// about the pin's concrete Kyoto Cabinet classes, and nothing more.
//
// libpinyin at 074a2219 allocates `TreeDB`, `HashDB`, `StashDB` and
// `ProtoTreeDB` itself (`new TreeDB`, `chewing_large_table2_kyotodb.cpp:87`;
// `new HashDB`, `ngram_kyotodb.cpp:117`; `new StashDB`,
// `ngram_kyotodb.cpp:58`; `new ProtoTreeDB`,
// `chewing_large_table2_kyotodb.cpp:100`) and calls them through a
// `BasicDB*` member. These functions give Rust the same shape: a concrete
// object behind `std::unique_ptr<kyotocabinet::BasicDB>` (the base class has
// a virtual destructor, `kcdb.h:1172`), opened — or not — by a separate
// `open`, and every operation a `BasicDB` call, as the pin's `m_db->` calls
// are.
//
// The adapter holds the three constructs the ruling names — the `Visitor`
// subclass, the empty-region pointer, and the read of `BasicDB::error()` —
// and the glue cxx has no syntax for: the constructors, the library's
// version string, the views of a Rust slice as the `(const char*, size_t)` pairs
// the methods take, and the forwarding of a database held behind a shared
// reference, which the classes' own locking permits. It calls the library.
// It copies and emulates none of it, and it catches no exception: the pin
// catches none either, so an exception leaves the bridge through a
// `noexcept` boundary and ends the process, as it ends the pin's.
#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>
#include <string>

#include <kcdb.h>

#include "rust/cxx.h"

namespace oxpinyin {
namespace kyotocabinet {

// Defined by the bridge (bridge.rs).
struct NativeError;
struct Sink;

// The pin's `BasicDB::Cursor*` (`chewing_large_table2_kyotodb.cpp:546`).
using DbCursor = ::kyotocabinet::BasicDB::Cursor;
// `BasicDB::Error::Code` and `BasicDB::OpenMode` under names the bridge can
// bind; cxx checks each variant against them.
using ErrorCode = ::kyotocabinet::BasicDB::Error::Code;
using OpenMode = ::kyotocabinet::BasicDB::OpenMode;

// `kyotocabinet::VERSION`.
rust::String version();

// `new TreeDB` / `new HashDB` / `new StashDB` / `new ProtoTreeDB`, untuned
// and not opened.
std::unique_ptr<::kyotocabinet::BasicDB> new_tree_db();
std::unique_ptr<::kyotocabinet::BasicDB> new_hash_db();
std::unique_ptr<::kyotocabinet::BasicDB> new_stash_db();
std::unique_ptr<::kyotocabinet::BasicDB> new_proto_tree_db();

// `BasicDB::error().code()`, the calling thread's last error code on `db`;
// it copies no text.
ErrorCode error_code(const ::kyotocabinet::BasicDB& db);
// `BasicDB::error()`, the calling thread's last error on `db`, with its
// name and message copied.
NativeError last_error(const ::kyotocabinet::BasicDB& db);

// `check(kbuf, ksiz)`.
std::int32_t check(const ::kyotocabinet::BasicDB& db, rust::Slice<const std::uint8_t> key);
// `accept(kbuf, ksiz, visitor, false)` with a visitor that copies the
// record into `sink`.
bool get(const ::kyotocabinet::BasicDB& db, rust::Slice<const std::uint8_t> key, Sink& sink);
// `set(kbuf, ksiz, vbuf, vsiz)`.
bool set(const ::kyotocabinet::BasicDB& db, rust::Slice<const std::uint8_t> key,
         rust::Slice<const std::uint8_t> value);
// `remove(kbuf, ksiz)`.
bool remove(const ::kyotocabinet::BasicDB& db, rust::Slice<const std::uint8_t> key);
// `iterate(visitor, false)` with a visitor that copies every record into
// `sink`, in the walk's own order.
bool iterate(const ::kyotocabinet::BasicDB& db, Sink& sink);
// `count()`.
std::int64_t count(const ::kyotocabinet::BasicDB& db);
// `synchronize(hard)`: proc = NULL, checker = NULL, the pin's form.
bool synchronize(const ::kyotocabinet::BasicDB& db, bool hard);
// `begin_transaction(hard)` and `end_transaction(commit)`.
bool begin_transaction(const ::kyotocabinet::BasicDB& db, bool hard);
bool end_transaction(const ::kyotocabinet::BasicDB& db, bool commit);
// `load_snapshot(path)` and `dump_snapshot(path)`: checker = NULL, the
// pin's form (`chewing_large_table2_kyotodb.cpp:105,129`).
bool load_snapshot(const ::kyotocabinet::BasicDB& db, const std::string& path);
bool dump_snapshot(const ::kyotocabinet::BasicDB& db, const std::string& path);

// `cursor()`. The cursor dereferences its database on every call
// (`kcplantdb.h:222-228`) and must not outlive it; the bridge encodes the
// borrow.
std::unique_ptr<DbCursor> make_cursor(const ::kyotocabinet::BasicDB& db);
// `Cursor::jump(kbuf, ksiz)`.
bool jump_to(DbCursor& cursor, rust::Slice<const std::uint8_t> key);
// `Cursor::accept(visitor, false, step)` with a visitor that copies the
// record at the cursor into `sink`.
bool cursor_accept(DbCursor& cursor, Sink& sink, bool step);

}  // namespace kyotocabinet
}  // namespace oxpinyin
