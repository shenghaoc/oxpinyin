"""Reviewed pinyin facade body at pin 074a2219; no name-only correspondence."""

from coverage_reviews import review

review("src/pinyin.cpp", [1615],
       [("crates/oxpinyin-capi/src/parse.rs", 'pub extern "C" fn pinyin_in_chewing_keyboard('),
        ("crates/oxpinyin-facade/src/parse.rs", "pub fn in_keyboard("),
        ("crates/oxpinyin-core/src/scheme.rs", "pub fn symbols_for("),
        ("crates/oxpinyin-capi/src/ffi.rs", "pub fn symbol_vector(")],
       "The pin removes Zhuyin correction bits and delegates to the active keyboard's symbol lookup. Rust reads the live scheme and USE_TONE state; symbols_for uses the ported simple/discrete/CP26 tables, preserves initial/middle/final and tone ordering and CP26's display-only i extra. The facade returns false for an empty mapping and otherwise writes the NULL-terminated string vector built with the pin's g_ptr_array_new/g_strdup/g_ptr_array_add/g_ptr_array_free calls and released by g_strfreev. This is the implemented lookup/marshalling role; invalid-pointer behavior is not graded by this structural mapping. No executed MATCH footprint was observed for this export in the requested suites.")
