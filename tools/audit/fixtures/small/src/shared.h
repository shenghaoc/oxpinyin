#ifndef SHARED_H
#define SHARED_H
#include <glib.h>
#include <string.h>

inline void shared_both() {
    g_free(g_strdup("both"));
}

template <int N> struct Holder {
    char m_bytes[N];
    void used() { memset(m_bytes, 0, N); }
    void uninstantiated() { memcpy(m_bytes, m_bytes, N); }
};

inline void never_called() {
    g_strdup("nobody");
}

#define CHECK_POINTER(pointer) g_free(pointer);

void only_pinyin();
void only_tool();
void kept_by_nothing();
#endif
