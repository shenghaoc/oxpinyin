"""Cursor, sentence and auxiliary-text refusal cases."""
import ctypes as C

from contract_abi import B, P, S, UNTOUCHED, Z, full_inst, sentence_out


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# batch2 group 12b: cursor, offset, sentence-index and aux-text refusals (PR 12b, #525)


@case('abort-get-sentence-past-rows', abort=False, issues=('525',))
def _(k):
    """Guess nihao, then record only the sentence-accessor flag for index five."""
    inst = full_inst(k, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    return {'ret': sentence_out(k, inst, 5)[0]}


@case('abort-get-left-pinyin-offset-after-separator', abort=False, issues=('525',))
def _(k):
    """Query the left offset at byte three after parsing ni'hao, recording only the flag."""
    inst = full_inst(k, b"ni'hao")
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_left_pinyin_offset', B, P, Z, C.POINTER(Z))(inst, 3, C.byref(out))}


@case('abort-get-right-pinyin-offset-after-separator', abort=False, issues=('525',))
def _(k):
    """Query the right offset at byte three after parsing ni'hao, recording only the flag."""
    inst = full_inst(k, b"ni'hao")
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_right_pinyin_offset', B, P, Z, C.POINTER(Z))(inst, 3, C.byref(out))}


@case('abort-get-right-pinyin-offset-nihao-5', abort=False, issues=('525',))
def _(k):
    """Query the right offset at byte five of nihao, discarding the seeded output offset."""
    inst = full_inst(k, b'nihao')
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_right_pinyin_offset', B, P, Z, C.POINTER(Z))(inst, 5, C.byref(out))}


@case('abort-get-character-offset-past-matrix', abort=False, issues=('525',))
def _(k):
    """Query 你好's character offset at byte 99 of nihao and return only the native flag."""
    inst = full_inst(k, b'nihao')
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
        inst, '你好'.encode(), 99, C.byref(out))}


@case('abort-get-character-offset-after-separator', abort=False, issues=('525',))
def _(k):
    """Query 你好's character offset at byte three of ni'hao and return only the flag."""
    inst = full_inst(k, b"ni'hao")
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
        inst, '你好'.encode(), 3, C.byref(out))}


@case('abort-parse-full-pinyin-apostrophe', abort=False, issues=('525',))
def _(k):
    """Pass n'i to the single-key parser and record its flag without reading the output key."""
    key = C.c_uint16(UNTOUCHED & 0xFFFF)
    return {'ret': k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(
        k.inst, b"n'i", C.byref(key))}


@case('abort-get-pinyin-key-empty-matrix', abort=False, issues=('525',))
def _(k):
    """Request key zero on an unparsed instance and record only the native flag."""
    out = P(UNTOUCHED)
    return {'ret': k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(k.alloc(), 0, C.byref(out))}


@case('abort-get-pinyin-key-rest-empty-matrix', abort=False, issues=('525',))
def _(k):
    """Request key-rest zero on an unparsed instance and record only the native flag."""
    out = P(UNTOUCHED)
    return {'ret': k.fn('get_pinyin_key_rest', B, P, Z, C.POINTER(P))(k.alloc(), 0, C.byref(out))}


@case('abort-double-auxiliary-text-cut-three-bytes-in', abort=False, issues=('525',))
def _(k):
    """Render double-pinyin auxiliary text three bytes into zhong and retain only the flag.

    Seed the output pointer with UNTOUCHED but do not read or free its value."""
    inst = full_inst(k, b'zhong')
    aux = P(UNTOUCHED)
    return {'ret': k.fn('get_double_pinyin_auxiliary_text', B, P, Z, C.POINTER(P))(inst, 3, C.byref(aux))}


# A leading `'` leaves column 0 empty at the pin and the aux walk asserts
# `get_column_size(offset) >= 1` (`pinyin.cpp:3311`) on every cursor.
def leading_apostrophe_aux(fname, text=b"'nihao"):
    """Capture a renderer suffix and full-pinyin bytes for a cursor-two auxiliary case."""
    def run(k):
        """Parse the captured bytes on a new instance and record the renderer's flag.

        Call get_<suffix>_auxiliary_text at byte cursor two with a seeded pointer;
        the pointer's value is neither compared nor freed."""
        inst = full_inst(k, text)
        aux = P(UNTOUCHED)
        return {'ret': k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, 2, C.byref(aux))}
    return run


for _fname in ('full_pinyin', 'double_pinyin', 'chewing'):
    case('abort-%s-auxiliary-text-leading-apostrophe' % _fname.replace('_', '-'), abort=False, issues=('525',))(
        leading_apostrophe_aux(_fname))
case('abort-full-pinyin-auxiliary-text-doubled-leading-apostrophe', abort=False, issues=('525',))(
    leading_apostrophe_aux('full_pinyin', b"''ni"))


# The graceful neighbours of the sites above answer without a warning.
@case('cursor-and-sentence-neighbours', control=True, issues=('525',))
def _(k):
    """Collect sentence, cursor, key and auxiliary outputs for neighboring valid calls.

    Use guessed nihao and a separate unparsed instance. Preserve numeric-output
    sentinels, report key-pointer nullness and decode/free returned auxiliary text."""
    out = {}
    inst = full_inst(k, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    out['sentence 0'] = sentence_out(k, inst, 0)
    fresh = k.alloc()
    out['sentence of fresh'] = sentence_out(k, fresh, 3)
    for label, name, offset in (('left 2', 'get_left_pinyin_offset', 2), ('right 2', 'get_right_pinyin_offset', 2),
                                ('right 4', 'get_right_pinyin_offset', 4)):
        res = Z(UNTOUCHED)
        out[label] = [k.fn(name, B, P, Z, C.POINTER(Z))(inst, offset, C.byref(res)),
                      'untouched' if res.value == UNTOUCHED else res.value]
    key = C.c_uint16(UNTOUCHED & 0xFFFF)
    out['parse ni'] = [k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(inst, b'ni', C.byref(key)), key.value]
    for offset in (0, 1):
        ptr = P(UNTOUCHED)
        ret = k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(inst, offset, C.byref(ptr))
        out['key %d' % offset] = [ret, ptr.value is None]
    for cursor in (1, 2, 5):
        aux = P(UNTOUCHED)
        ret = k.fn('get_double_pinyin_auxiliary_text', B, P, Z, C.POINTER(P))(inst, cursor, C.byref(aux))
        out['double aux %d' % cursor] = [ret, k.text(aux.value) if aux.value != UNTOUCHED else 'untouched']
    return out


def register(driver_case):
    """Register the declarations in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
