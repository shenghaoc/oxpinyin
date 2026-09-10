/* Translation unit bindgen reads for the LMDB backend.
 *
 * <lmdb.h> is the system LMDB header — the whole point of this backend
 * is that it is the distribution's, not a vendored copy.
 *
 * <unistd.h> is here for `sysconf`/`_SC_PAGESIZE` alone. LMDB rounds a
 * map size up to a page multiple silently; oxpinyin refuses one that is
 * not already a multiple, so callers get StoreError::InvalidInput
 * instead of a ceiling quietly different from the one they asked for.
 * Answering that needs the system page size, and reading it here keeps
 * the backend free of a `libc` dependency and free of hardcoded
 * `_SC_*` numbers, which differ per platform.
 */
#include <lmdb.h>
#include <unistd.h>
