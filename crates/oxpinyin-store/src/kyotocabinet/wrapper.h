/* bindgen entry point for the Kyoto Cabinet backend.
 *
 * The system kclangc.h and nothing else. Kyoto Cabinet is C++ internally,
 * but this is its complete C API, so bindgen reads a C header and no cxx
 * bridge is involved. See build.rs for why the declarations are generated
 * rather than checked in. */
#include <kclangc.h>
