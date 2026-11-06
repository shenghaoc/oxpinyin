#include "shared.h"

static void helper() {
    strcmp("same name", "other library");
}

extern "C" void pinyin_entry(gchar *pointer) {
    helper();
    shared_both();
    only_pinyin();
    Holder<2> holder;
    holder.used();
    G_N_ELEMENTS(holder.m_bytes);
    pointer = g_utf8_next_char(pointer);
    CHECK_POINTER(pointer)
#if 0
    g_free(pointer);
#endif
}
