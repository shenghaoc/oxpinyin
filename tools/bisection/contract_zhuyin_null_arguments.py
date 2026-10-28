"""Zhuyin null arguments cases (inventory row 58, #1175).
"""
import ctypes as C

from contract_abi import B, I, P, S, U, UNTOUCHED, Z


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# Register row 41 (class (b)), the libzhuyin half: the pin dereferences a
# NULL argument and dies of SIGSEGV, the subject's guard answers. One case
# per site so the pin dies at the site under test.
def null_argument_case(call):
    """Adapt a single call result to the registry's ret observation dictionary.

    Exceptions and native process failures propagate to the worker boundary.
    """
    def probe(k):
        """Invoke the supplied case callback and wrap its result under ret."""
        return {'ret': call(k)}
    return probe


_ZY_NULL_FALSE = {'ret': False, 'logs': []}
_ZY_NULL_TRUE = {'ret': True, 'logs': []}


def zy_parse_chewing_null_instance(k):
    """Call the one-key parser with a valid text/output but no instance.

    Return its status if it survives the instance dereference at
    074a2219 zhuyin.cpp:1048 (zhuyin_parse_chewing).
    """
    key = C.c_ushort(0)
    return k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(None, b'su3', C.byref(key))


def zy_parse_chewing_null_text(k):
    """Call the one-key parser with a null text and an initialized key output.

    Return its status if it survives strlen in zhuyin_parse_chewing at
    074a2219 zhuyin.cpp:1052.
    """
    key = C.c_ushort(0)
    return k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, None, C.byref(key))


def zy_parse_chewing_null_key(k):
    """Pass a null key output for su3 and return the parser status if it survives.

    The output is dereferenced by zhuyin_parse_chewing at
    074a2219 zhuyin.cpp:1053-1054.
    """
    return k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, b'su3', None)


def zy_in_chewing_null_instance(k):
    """Query keyboard membership for s with no instance and a real output slot.

    Return the status if the call survives zhuyin_in_chewing_keyboard's instance
    dereference at 074a2219 zhuyin.cpp:1089.
    """
    symbols = P(UNTOUCHED)
    return k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))(None, b's', C.byref(symbols))


def zy_in_chewing_null_symbols(k):
    """Query keyboard membership for s with a null symbol-vector output.

    Return the status if it survives the output dereference in
    zhuyin_in_chewing_keyboard at 074a2219 zhuyin.cpp:1092-1093.
    """
    return k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))(k.inst, b's', None)


def zy_pinyin_string_null_instance(k):
    """Parse a real key, then request its pinyin spelling without an instance.

    Return the getter status if it survives the context access in
    zhuyin_get_pinyin_string at 074a2219 zhuyin.cpp:1746-1747.
    """
    key = C.c_ushort(0)
    k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, b'su3', C.byref(key))
    string = P(UNTOUCHED)
    return k.fn('get_pinyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(None, C.byref(key), C.byref(string))


def zy_zhuyin_string_null_key(k):
    """Request a zhuyin spelling for a null key using a real output slot.

    Return the status if it survives the key access in zhuyin_get_zhuyin_string
    at 074a2219 zhuyin.cpp:1735-1736.
    """
    string = P(UNTOUCHED)
    return k.fn('get_zhuyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(k.inst, None, C.byref(string))


def zy_zhuyin_string_null_out(k):
    """Parse a real key, then request its zhuyin spelling with no output slot.

    Return the status if it survives zhuyin_get_zhuyin_string's output write
    at 074a2219 zhuyin.cpp:1735.
    """
    key = C.c_ushort(0)
    k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, b'su3', C.byref(key))
    return k.fn('get_zhuyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(k.inst, C.byref(key), None)


def zy_key_rest_positions_null_rest(k):
    """Request both positions from a null rest pointer using real numeric outputs.

    Return the status if it survives zhuyin_get_zhuyin_key_rest_positions at
    074a2219 zhuyin.cpp:1933-1937.
    """
    begin, end = C.c_ushort(0), C.c_ushort(0)
    return k.fn('get_zhuyin_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))(
        k.inst, None, C.byref(begin), C.byref(end))


def zy_n_phrase_null_instance(k):
    """Read the phrase count without an instance, retaining only the call status.

    zhuyin_get_n_phrase reads the instance at 074a2219 zhuyin.cpp:2229.
    """
    count = U(0)
    return k.fn('get_n_phrase', B, P, C.POINTER(U))(None, C.byref(count))


def zy_n_phrase_null_out(k):
    """Read the cached instance's phrase count with a null output pointer.

    Return the status if it survives zhuyin_get_n_phrase's output write at
    074a2219 zhuyin.cpp:2229.
    """
    return k.fn('get_n_phrase', B, P, C.POINTER(U))(k.inst, None)


def zy_phrase_token_null_instance(k):
    """Read token zero without an instance, using a real output slot.

    Return the status if it survives the instance access in
    zhuyin_get_phrase_token at 074a2219 zhuyin.cpp:2236.
    """
    slot = U(0)
    return k.fn('get_phrase_token', B, P, U, C.POINTER(U))(None, 0, C.byref(slot))


def zy_phrase_token_null_out(k):
    """Request token zero with a null output pointer and retain only the call status.

    zhuyin_get_phrase_token writes the null token before checking range at
    074a2219 zhuyin.cpp:2238-2241.
    """
    return k.fn('get_phrase_token', B, P, U, C.POINTER(U))(k.inst, 0, None)


def zy_first_rest(k):
    """Parse su3cl3 and return the instance with its first borrowed key-rest pointer.

    Require the rest lookup to succeed and produce a non-null pointer.
    """
    inst = k.inst
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    rest = P(UNTOUCHED)
    assert k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))(inst, 0, C.byref(rest)) and rest.value
    return inst, rest


def zy_first_candidate(k):
    """Parse and guess su3cl3, returning the instance and first borrowed candidate.

    Require guessing and candidate lookup to succeed with a non-null pointer.
    """
    inst = k.inst
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    cand = P(UNTOUCHED)
    assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand)) and cand.value
    return inst, cand


def zy_key_rest_length_null_rest(k):
    """Prepare a parsed instance, then request length from a null key rest.

    Return the status if it survives zhuyin_get_zhuyin_key_rest_length at
    074a2219 zhuyin.cpp:1945.
    """
    inst, _ = zy_first_rest(k)
    length = C.c_ushort(0)
    return k.fn('get_zhuyin_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(inst, None, C.byref(length))


def zy_key_rest_length_null_length(k):
    """Request a real key rest's length with no output slot.

    Return the status if it survives zhuyin_get_zhuyin_key_rest_length's write
    at 074a2219 zhuyin.cpp:1945.
    """
    inst, rest = zy_first_rest(k)
    return k.fn('get_zhuyin_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(inst, rest, None)


def zy_candidate_type_null_candidate(k):
    """Prepare candidate state, then read a null candidate's type into a real slot.

    Return the status if it survives zhuyin_get_candidate_type at
    074a2219 zhuyin.cpp:1861.
    """
    inst, _ = zy_first_candidate(k)
    kind = I(0)
    return k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, None, C.byref(kind))


def zy_candidate_type_null_out(k):
    """Request the first candidate's type with a null output pointer.

    Return the status if it survives zhuyin_get_candidate_type's write at
    074a2219 zhuyin.cpp:1861.
    """
    inst, cand = zy_first_candidate(k)
    return k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, None)


def zy_candidate_string_null_candidate(k):
    """Prepare candidate state, then request a null candidate's text with a real slot.

    Return the status if it survives zhuyin_get_candidate_string at
    074a2219 zhuyin.cpp:1868.
    """
    inst, _ = zy_first_candidate(k)
    text = P(UNTOUCHED)
    return k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, None, C.byref(text))


def zy_candidate_string_null_out(k):
    """Request the first candidate's text with a null output pointer.

    Return the status if it survives zhuyin_get_candidate_string's write at
    074a2219 zhuyin.cpp:1868.
    """
    inst, cand = zy_first_candidate(k)
    return k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, cand, None)


# Register row 43 (class (b)): `zhuyin_begin_add_phrases` stores any `guint8`
# (`zhuyin.cpp:392-398`), so a following `zhuyin_iterator_add_phrase` on
# library 16 reaches `_add_phrase`'s `phrase_index->get_range(index, range)`
# (`zhuyin.cpp:475`), which reads `m_sub_phrase_indices[16]`
# (`storage/phrase_index.cpp:611`) one past the 16-entry array
# (`storage/phrase_index.h:441`) and dies of SIGSEGV. The subject refuses
# every library that is neither a user file nor a loaded system library.
# The pinyin twin's index is not part of the row (the audit recorded library
# 255 accepted there, so that side has no stable crash to hold).
def zy_import_index_out_of_range(k):
    """Attempt an import through library index 16 and return its add result.

    Return 'no-iterator' if begin refuses; otherwise close the iterator after
    adding. The pin's zhuyin_begin_add_phrases stores that index unchecked at
    074a2219 zhuyin.cpp:392-398, and _add_phrase passes it to get_range at
    zhuyin.cpp:475.
    """
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 16)
    if not it:
        return 'no-iterator'
    out = k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你好'.encode(), 'ㄋㄧˇ ㄏㄠˇ'.encode(), 1)
    k.fn('end_add_phrases', None, P)(it)
    return out


case('zhuyin-add-phrase-library-16', mode='zhuyin', control=True, crash={'ret': False}, issues=('563', 'row:41'))(
    null_argument_case(zy_import_index_out_of_range))


for _zyname, _zycall, _zyanswer in (
        ('null-instance-parse-chewing', zy_parse_chewing_null_instance, _ZY_NULL_FALSE),
        ('null-text-parse-chewing', zy_parse_chewing_null_text, _ZY_NULL_FALSE),
        ('null-key-parse-chewing', zy_parse_chewing_null_key, _ZY_NULL_FALSE),
        ('null-instance-in-chewing-keyboard', zy_in_chewing_null_instance, _ZY_NULL_FALSE),
        ('null-symbols-in-chewing-keyboard', zy_in_chewing_null_symbols, _ZY_NULL_TRUE),
        ('null-instance-get-pinyin-string', zy_pinyin_string_null_instance, _ZY_NULL_FALSE),
        ('null-key-get-zhuyin-string', zy_zhuyin_string_null_key, _ZY_NULL_FALSE),
        ('null-out-get-zhuyin-string', zy_zhuyin_string_null_out, _ZY_NULL_FALSE),
        ('null-rest-get-zhuyin-key-rest-positions', zy_key_rest_positions_null_rest, _ZY_NULL_FALSE),
        ('null-rest-get-zhuyin-key-rest-length', zy_key_rest_length_null_rest, _ZY_NULL_FALSE),
        ('null-length-get-zhuyin-key-rest-length', zy_key_rest_length_null_length, _ZY_NULL_TRUE),
        ('null-candidate-get-candidate-type', zy_candidate_type_null_candidate, _ZY_NULL_FALSE),
        ('null-out-get-candidate-type', zy_candidate_type_null_out, _ZY_NULL_TRUE),
        ('null-candidate-get-candidate-string', zy_candidate_string_null_candidate, _ZY_NULL_FALSE),
        ('null-out-get-candidate-string', zy_candidate_string_null_out, _ZY_NULL_TRUE),
        ('null-instance-get-n-phrase', zy_n_phrase_null_instance, _ZY_NULL_FALSE),
        ('null-out-get-n-phrase', zy_n_phrase_null_out, _ZY_NULL_TRUE),
        ('null-instance-get-phrase-token', zy_phrase_token_null_instance, _ZY_NULL_FALSE),
        ('null-out-get-phrase-token', zy_phrase_token_null_out, _ZY_NULL_FALSE)):
    case('zhuyin-' + _zyname, mode='zhuyin', control=True, crash=_zyanswer, issues=('563', 'row:41'))(
        null_argument_case(_zycall))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
