"""C ABI vocabulary shared by the cross-open cases.

The ctypes aliases the cases write their signatures with, the GArray view and
tokens_of, which drives one facade lookup through the case's Kit and uses only
its `fn`, `inst` and `glib`.
"""
import ctypes as C

P, U, I, S, B, Z = C.c_void_p, C.c_uint, C.c_int, C.c_char_p, C.c_bool, C.c_size_t


class Arr(C.Structure):
    """View GLib's public GArray data pointer and unsigned element count via ctypes."""
    _fields_ = [('data', P), ('len', U)]


def tokens_of(k, phrase):
    """Copy phrase-token IDs from a newly allocated four-byte-element GArray.

    UTF-8 encode phrase and perform lookup on the cached instance. Ignore the
    lookup return flag and copy the array's reported contents into a Python
    list; the native array remains allocated."""
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    arr = new(0, 0, 4)
    k.fn('lookup_tokens', B, P, S, P)(k.inst, phrase.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    return C.cast(view.data, C.POINTER(U))[:view.len]
