"""The remember_user_input signed count contract cases (#540).

Collect the declarations here and register them with the driver's decorator at
their original position. Importing this module does not register into a driver.
"""
import ctypes as C

from contract_abi import B, I, P, S, U, Z


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def remember_negative(count):
    """Build a phrase-remembering case with the supplied signed count, including controls.

    Pass count as a C signed int; negative and positive controls share the
    same sequence."""
    def run(k):
        """Remember 你好世界 after parsing its input and enumerate library 7.

        Return the remember flag and phrase/reading/count rows after the attempt.
        Decode and free exported strings; leave the get iterator allocated and
        the cached instance's parse and user-library changes in place."""
        inst = k.inst
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihaoshijie')
        out = {'remember': k.fn('remember_user_input', B, P, S, I)(inst, '你好世界'.encode(), count)}
        it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
        has = k.fn('iterator_has_next_phrase', B, P)
        nxt = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))
        rows = []
        while has(it):
            ph, py, n = P(), P(), I()
            nxt(it, C.byref(ph), C.byref(py), C.byref(n))
            rows.append([k.text(ph.value), k.text(py.value), n.value])
        out['rows'] = rows
        return out
    return run


for _count in (-2, -5, -2147483648):
    case('remember-count-%d' % -_count if _count > -100 else 'remember-count-min')(remember_negative(_count))
case('remember-count-default', control=True)(remember_negative(-1))
case('remember-count-seven', control=True)(remember_negative(7))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
