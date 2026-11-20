"""ctypes aliases and the GArray view shared by the cross-open cases."""
import ctypes as C

P, U, I, S, B, Z = C.c_void_p, C.c_uint, C.c_int, C.c_char_p, C.c_bool, C.c_size_t


class Arr(C.Structure):
    """View GLib's public GArray data pointer and unsigned element count via ctypes."""
    _fields_ = [('data', P), ('len', U)]
