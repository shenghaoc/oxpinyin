/* bindgen entry point for the Berkeley DB backend.
 *
 * The system db.h and nothing else: this backend links the libdb the
 * distro already ships, which is the same library that wrote the files
 * it has to read. See build.rs for why the declarations are generated
 * from this header rather than checked in. */
#include <db.h>
