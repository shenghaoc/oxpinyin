use super::*;
use oxpinyin::vocab::PhraseText;

#[test]
fn ffi_output_helpers_preserve_ownership_and_bytes() {
    let duplicated = strdup("abc");
    // SAFETY: The helper returned a NUL-terminated GLib string.
    assert_eq!(unsafe { CStr::from_ptr(duplicated) }.to_bytes(), b"abc");
    // SAFETY: `g_free` owns the returned `g_strdup` allocation.
    unsafe { glib_sys::g_free(duplicated.cast()) };
    // SAFETY: GLib accepts a NULL string and returns NULL.
    assert!(unsafe { strdup_raw(ptr::null()) }.is_null());

    let key = key_text(KeyText::Digit("a", 3));
    // SAFETY: The helper returned a NUL-terminated GLib string.
    assert_eq!(unsafe { CStr::from_ptr(key) }.to_bytes(), b"a3");
    // SAFETY: `g_free` owns the returned key string.
    unsafe { glib_sys::g_free(key.cast()) };

    let vector = symbol_vector(&["one", "two"]);
    // SAFETY: `symbol_vector` returns a NULL-terminated GLib vector.
    assert_eq!(unsafe { CStr::from_ptr(*vector.add(1)) }.to_bytes(), b"two");
    // SAFETY: The vector and elements belong to `g_strfreev`.
    unsafe { glib_sys::g_strfreev(vector) };
    assert!(symbol_vector(&[] as &[&str]).is_null());

    let sentence = sentence_text([Some(PhraseText::from("你")), Some(PhraseText::from("好"))]);
    // SAFETY: The helper returned a NUL-terminated GLib string.
    let sentence_bytes = unsafe { CStr::from_ptr(sentence) }.to_bytes();
    assert_eq!(sentence_bytes, "你好".as_bytes());
    // SAFETY: `g_free` releases the final GLib allocation.
    unsafe { glib_sys::g_free(sentence.cast()) };

    let filled = malloc_cstr_with(3, |out| {
        out.copy_from_slice(b"abc");
        3
    });
    // SAFETY: The helper returned a NUL-terminated malloc string.
    assert_eq!(unsafe { CStr::from_ptr(filled) }.to_bytes(), b"abc");
    // SAFETY: `g_free` releases the returned allocation, as the C contract
    // tells callers.
    unsafe { glib_sys::g_free(filled.cast()) };
    assert!(
        malloc_cstr_with(1, |out| {
            out[0] = 0;
            1
        })
        .is_null()
    );
    log_warning("a\0b"); // Interior NUL is dropped before GLib sees it.
}
