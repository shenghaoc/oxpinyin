/* bindgen entry point for the Kyoto Cabinet backend.
 *
 * The C entry points of the C++ shim (shim.h), which constructs the four
 * classes libpinyin uses directly instead of through kclangc.h's PolyDB.
 * See build.rs for why the declarations are generated rather than checked
 * in. */
#include "shim.h"
