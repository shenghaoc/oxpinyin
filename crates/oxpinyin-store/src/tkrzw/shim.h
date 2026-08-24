// C++ shim over tkrzw's TreeDBM for the oxpinyin store backend.
//
// The interface is deliberately narrow and boring: no templates, no
// overloads, no exceptions, no default arguments.  Every entry point
// takes and returns types cxx can bridge directly, and every failure
// comes back as a ShimStatus carrying tkrzw's own status code and
// message rather than throwing.  All policy — table-name framing, bound
// semantics, write buffering — lives on the Rust side; this file only
// moves bytes.

#pragma once

#include <cstdint>
#include <memory>

#include <tkrzw_dbm.h>
#include <tkrzw_dbm_tree.h>

#include "rust/cxx.h"

namespace oxpinyin_tkrzw {

struct ShimStatus;
struct Mutation;

// An open TreeDBM.  Closed by the destructor, so a dropped Rust handle
// always flushes and releases the file lock.
class Db {
 public:
  Db() = default;
  ~Db();
  Db(const Db&) = delete;
  Db& operator=(const Db&) = delete;

  // `mutable` because tkrzw documents every TreeDBM operation as
  // thread-safe: the database carries its own locking, so a read, a
  // ProcessMulti batch and a Synchronize are all sound through a shared
  // reference.  That is what lets the Rust side keep `get` and `write`
  // on `&self`, as the redb and LMDB backends do.
  mutable tkrzw::TreeDBM dbm;
};

// A cursor over an open Db.  Must not outlive the Db it was made from;
// the Rust side keeps every iterator local to one method call.
class Iter {
 public:
  explicit Iter(std::unique_ptr<tkrzw::DBM::Iterator> iter);
  ~Iter() = default;
  Iter(const Iter&) = delete;
  Iter& operator=(const Iter&) = delete;

  std::unique_ptr<tkrzw::DBM::Iterator> iter;
};

// Opens `path` as a TreeDBM with tkrzw's default tuning, which means the
// default LexicalKeyComparator: plain unsigned byte order, the order the
// redb and LMDB backends already provide.  No custom comparator is set,
// exactly as libpinyin's tkrzwdb_utils.h leaves it.
std::unique_ptr<Db> open_db(rust::Slice<const std::uint8_t> path, bool writable,
                            bool no_create, ShimStatus& status);

ShimStatus db_get(const Db& db, rust::Slice<const std::uint8_t> key,
                  rust::Vec<std::uint8_t>& value, bool& found);

// Applies every mutation in one ProcessMulti call: tkrzw locks all the
// named records for the duration, so the batch lands as a unit against
// any other reader or writer of the same database.
ShimStatus db_apply(const Db& db, rust::Slice<const Mutation> mutations);

ShimStatus db_synchronize(const Db& db, bool hard);

ShimStatus db_rebuild(const Db& db);

std::unique_ptr<Iter> db_iter(const Db& db);

// Positions the cursor at the first record whose key is greater than or
// equal to `key` — TreeDBM's ordered lower-bound jump.
ShimStatus iter_jump(Iter& iter, rust::Slice<const std::uint8_t> key);

// Reads the record under the cursor.  `found` is false once the cursor
// has walked off the end.
ShimStatus iter_get(Iter& iter, rust::Vec<std::uint8_t>& key,
                    rust::Vec<std::uint8_t>& value, bool& found);

ShimStatus iter_next(Iter& iter);

}  // namespace oxpinyin_tkrzw
