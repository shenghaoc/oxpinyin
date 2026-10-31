//! Output allocation and diagnostics for the exposed C boundary.
//! Input conversion is shared by `oxpinyin_capi_marshal::input`.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

use oxpinyin_chewing::KeyText;

// `void *malloc(size_t)` and `void free(void *)` from the host's libc.
unsafe extern "C" {
    fn malloc(size: usize) -> *mut c_void;
    fn free(ptr: *mut c_void);
}

/// `text` with a terminating NUL, the C string a GLib call reads. C reads
/// it up to its first NUL, as it reads the pin's argument.
fn c_arg(text: &[u8]) -> Vec<u8> {
    let mut arg = Vec::with_capacity(text.len() + 1);
    arg.extend_from_slice(text);
    arg.push(0);
    arg
}

/// `g_strdup(text)`: the pin's duplication of a returned string. GLib
/// allocates with `g_malloc`, so a failed allocation aborts as the pin's
/// does. The caller owns the result and frees it with `g_free`.
pub fn strdup(text: &str) -> *mut c_char {
    let arg = c_arg(text.as_bytes());
    // SAFETY: `arg` is NUL-terminated and outlives the call.
    unsafe { glib_sys::g_strdup(arg.as_ptr().cast()) }
}

/// `g_strdup(text)` for a C string that may be NULL: GLib answers NULL for
/// NULL, the pin's `g_strdup` of a failed `g_ucs4_to_utf8` result
/// (`lookup.cpp:63`).
///
/// # Safety
///
/// `text` is NULL or a NUL-terminated string.
pub unsafe fn strdup_raw(text: *const c_char) -> *mut c_char {
    // SAFETY: The caller guarantees `text` is NULL or NUL-terminated;
    // `g_strdup` accepts NULL.
    unsafe { glib_sys::g_strdup(text) }
}

/// A key rendering made with the call `_ChewingKey::get_*_string` makes
/// (`chewing_key.cpp:47-121`): `g_strdup` of the spelling, or
/// `g_strdup_printf` with the pin's format and arguments. GLib formats and
/// allocates, so a failed allocation fails as the pin's does.
pub fn key_text(text: KeyText) -> *mut c_char {
    match text {
        KeyText::Plain(base) => strdup(base),
        KeyText::Digit(base, tone) => {
            let base = c_arg(base.as_bytes());
            // SAFETY: `%s%d` consumes one NUL-terminated `const char *`,
            // which outlives the call, and one `int`.
            unsafe {
                glib_sys::g_strdup_printf(
                    c"%s%d".as_ptr(),
                    base.as_ptr().cast::<c_char>(),
                    c_int::from(tone),
                )
            }
        }
        KeyText::Mark(base, mark) => {
            let (base, mark) = (c_arg(base.as_bytes()), c_arg(mark.as_bytes()));
            // SAFETY: `%s%s` consumes two NUL-terminated `const char *`
            // arguments, both of which outlive the call.
            unsafe {
                glib_sys::g_strdup_printf(
                    c"%s%s".as_ptr(),
                    base.as_ptr().cast::<c_char>(),
                    mark.as_ptr().cast::<c_char>(),
                )
            }
        }
    }
}

/// The keyboard symbol vector as the pin's `in_chewing_scheme` builds it
/// (`zhuyin_parser2.cpp:301-332`, `:496-544`, `:798-851`): a
/// `g_ptr_array_new`, one `g_strdup` and `g_ptr_array_add` per symbol, then
/// the terminating NULL and `g_ptr_array_free(array, FALSE)`, whose
/// segment the caller frees with `g_strfreev`. No symbol answers NULL after
/// `g_ptr_array_free(array, TRUE)`, as the pin's `false` does.
pub fn symbol_vector(items: &[impl AsRef<str>]) -> *mut *mut c_char {
    // SAFETY: `g_ptr_array_new` takes no arguments and aborts rather than
    // answering NULL.
    let array = unsafe { glib_sys::g_ptr_array_new() };
    for item in items {
        let symbol = strdup(item.as_ref());
        // SAFETY: `array` is the live array made above; it takes ownership
        // of the GLib-allocated `symbol`.
        unsafe { glib_sys::g_ptr_array_add(array, symbol.cast()) };
    }
    if items.is_empty() {
        // SAFETY: `array` is live and holds no elements; freeing the segment
        // frees nothing else.
        unsafe { glib_sys::g_ptr_array_free(array, glib_sys::GTRUE) };
        return ptr::null_mut();
    }
    // SAFETY: `array` is live. Adding NULL terminates the vector, and
    // `g_ptr_array_free(…, FALSE)` releases the array and hands its
    // segment, now owned by the caller, back.
    unsafe {
        glib_sys::g_ptr_array_add(array, ptr::null_mut());
        glib_sys::g_ptr_array_free(array, glib_sys::GFALSE).cast()
    }
}

/// The pin's `convert_to_utf8` over the sentence row's phrases
/// (`lookup/lookup.cpp:27-69`), call for call. Each phrase is converted by
/// this facade's fallible encoder, which stands for `g_ucs4_to_utf8`
/// (`:58`) and answers NULL when its allocation fails. While the result is
/// still NULL it becomes `g_strdup(phrase)` (`:62-63`), which is NULL again
/// for a NULL phrase; otherwise `g_strconcat(result, "", phrase, NULL)`
/// (`:65`, the NULL delimiter made `""` at `:33-34`), which stops at a NULL
/// phrase and so copies the result unchanged. The phrase and the previous
/// result are then freed (`:66-67`). GLib aborts if its own allocation
/// fails. NULL when every conversion failed.
pub fn sentence_text(phrases: &[oxpinyin_core::PhraseText]) -> *mut c_char {
    let mut result: *mut c_char = ptr::null_mut();
    for phrase in phrases {
        let bytes = oxpinyin_capi_marshal::phrase_text::phrase_text_bytes(phrase);
        let converted = malloc_cstr_with(bytes.len(), |out| {
            out.copy_from_slice(&bytes);
            bytes.len()
        });
        let previous = result;
        result = if previous.is_null() {
            // SAFETY: `converted` is NULL or this call's NUL-terminated
            // buffer; `g_strdup` answers NULL for NULL.
            unsafe { strdup_raw(converted) }
        } else {
            // SAFETY: `previous` is the NUL-terminated GLib string this loop
            // made, `""` is a static C string, `converted` is NULL (which
            // ends the list) or this call's NUL-terminated buffer, and the
            // list ends with NULL.
            unsafe {
                glib_sys::g_strconcat(
                    previous.cast_const(),
                    c"".as_ptr(),
                    converted.cast_const(),
                    ptr::null::<c_char>(),
                )
            }
        };
        // SAFETY: `converted` is NULL or this call's encoder buffer, and
        // `previous` NULL or the GLib string this loop owns; nothing else
        // holds either, and `g_free` releases both.
        unsafe {
            glib_sys::g_free(converted.cast());
            glib_sys::g_free(previous.cast());
        }
    }
    result
}

/// Hands C one `malloc`-allocated, NUL-terminated string of `len` bytes
/// that `fill` writes into the buffer it is given, returning how many it
/// wrote: the encoder that stands for the pin's `g_ucs4_to_utf8`
/// conversions (`zhuyin.cpp:1112`, `lookup.cpp:54`, `:58`). Like that function it answers NULL when its allocation fails,
/// with no warning and no abort; it also answers NULL when `fill` wrote
/// another length or the bytes carry a NUL. The buffer is zeroed before
/// `fill` sees it, so the slice it writes through is initialized memory.
/// The caller frees the result with `g_free`.
pub fn malloc_cstr_with(len: usize, fill: impl FnOnce(&mut [u8]) -> usize) -> *mut c_char {
    let Some(size) = len.checked_add(1) else {
        return ptr::null_mut();
    };
    // SAFETY: `malloc` returns `size` writable bytes or null.
    let dst = unsafe { malloc(size) }.cast::<u8>();
    if dst.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: `dst` points to `size` writable bytes that nothing else
    // references; zeroing them makes every byte an initialized `u8`.
    unsafe {
        ptr::write_bytes(dst, 0, size);
    }
    // SAFETY: `dst` points to `size` initialized bytes this function alone
    // owns until it returns, so the exclusive slice is valid for its scope.
    let buffer = unsafe { std::slice::from_raw_parts_mut(dst, size) };
    let written = fill(&mut buffer[..len]);
    if written != len || buffer[..len].contains(&0) {
        // SAFETY: `dst` came from `malloc` above and was handed out nowhere.
        unsafe { free(dst.cast()) };
        return ptr::null_mut();
    }
    buffer[len] = 0;
    dst.cast::<c_char>()
}

/// Logs `message` through `GLib` at warning level under the `libzhuyin`
/// domain. The library's only diagnostic channel: the C ABI's frozen
/// return shapes (`false` / NULL) carry no reason, and glib is already
/// linked for the ABI's `GArray`s. A message with an interior NUL is
/// dropped rather than truncated.
pub fn log_warning(message: &str) {
    let Ok(message) = CString::new(message) else {
        return;
    };
    // SAFETY: `g_log` is variadic; the `%s` format consumes exactly one
    // `const char *`, and both the domain and the message are
    // NUL-terminated buffers that outlive the call.
    unsafe {
        glib_sys::g_log(
            c"libzhuyin".as_ptr(),
            glib_sys::G_LOG_LEVEL_WARNING,
            c"%s".as_ptr(),
            message.as_ptr(),
        );
    }
}
