#include "shared.h"

static void helper() {
    strlen("same name");
}

extern "C" void zhuyin_entry() {
    helper();
    shared_both();
}
