"""Matrix resplit windows cases (inventory row 54, #1175).
"""
import ctypes as C

from contract_abi import B, P, S, U, UNTOUCHED, Z
from contract_case_helpers import guess_rows, zhuyin_guess_rows


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #682/#683, pin 074a2219: forward resplit mutation, then divided, then
# fuzzy (phonetic_key_matrix.cpp:87-228; pinyin.cpp:1497-1524). Every
# offset reads the whole parse's matrix, including newly created columns.
# libzhuyin's full-pinyin seam omits both split passes (zhuyin.cpp:1017-
# 1040); cover that facade's distinct order too. Compare every ordered
# type/text row.
MATRIX_RESPLIT_INPUTS = (
    'banangang', 'baguanangang', 'chenanengang', 'anangang', 'fanangang',
    'lianai', 'xiane', 'liane', 'xianai', 'qiane', 'lian', 'xian',
)


def matrix_resplit_windows(text, options):
    """Build a full-input candidate-window comparison under the supplied option word.

    text is intended to be ASCII pinyin: the callable compares consumed bytes
    with len(text) and visits offsets through that length, inclusive."""
    def run(k):
        """Assert a complete parse and collect ordered candidates at every input offset.

        Use pinyin sort word 0x1e or zhuyin after-cursor guesses without a preceding
        sentence guess. Return consumed length and the list of window observations."""
        assert k.fn('set_options', B, P, U)(k.ctx, options)
        inst = k.inst
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        assert parsed == len(text), 'characterisation requires a complete parse'
        windows = []
        for offset in range(len(text) + 1):
            if k.mode == 'pinyin':
                windows.append(guess_rows(k, inst, offset, 0x1e))
            else:
                windows.append(zhuyin_guess_rows(k, inst, offset))
        return {'parsed': parsed, 'windows': windows}
    return run


for _mode in ('pinyin', 'zhuyin'):
    for _tables in (False, True):
        for _fuzzy in (False, True):
            # PINYIN_AMB_AN_ANG: expand the halves produced by the tables.
            _options = (0x18a if _tables else 0x0a) | ((1 << 17) if _fuzzy else 0)
            for _text in MATRIX_RESPLIT_INPUTS:
                case('matrix-resplit-%s-%s-%s-%s' % (
                    _mode, _text, 'tables' if _tables else 'plain',
                    'fuzzy' if _fuzzy else 'exact'), mode=_mode,
                    control=not _tables or _text in ('baguanangang', 'chenanengang')
                    or (_mode == 'pinyin' and _text in ('lian', 'xian')), issues=('682', '683'))(
                        matrix_resplit_windows(_text, _options))


# #1031: every reader of a facade's one retained matrix. zhuyin.cpp:1017-1040
# fills then fuzzes, and zhuyin_get_zhuyin_key (:1886-1927), guess, sentence,
# character offset and training all read instance->m_matrix, so a divided
# (xian = xi + an) or resplit (fangan = fang + an) pair adds no key on the
# zhuyin facade; pinyin.cpp:1514-1522 adds both and every pinyin reader sees
# them.
SURFACE_INPUTS = (('xian', '西安'), ('fangan', '方案'))


def key_bytes(pointer, length):
    """The key or rest bytes a getter handed out, None for a NULL pointer."""
    return C.string_at(pointer.value, length).hex() if pointer.value else None


def matrix_resplit_surfaces(text, phrase, options=0x18a):
    """Observe each surface that reads the parsed matrix, over `text` under `options`.

    Use the complete full-pinyin parse, the key and key-rest getters at every
    offset (with the rest's positions and length), candidate guesses at
    offsets 0 and 2, a sentence guess and its list, and the character offset
    of `phrase` at every offset. Report each return with its untouched
    out-parameter flag."""
    def run(k):
        assert k.fn('set_options', B, P, U)(k.ctx, options)
        inst = k.inst
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode()) == len(text)
        out = {'keys': [], 'rests': [], 'characters': []}
        for offset in range(len(text) + 1):
            key, rest = P(), P()
            out['keys'].append([k.fn('get_%s_key' % k.mode, B, P, Z, C.POINTER(P))(
                inst, offset, C.byref(key)), key_bytes(key, 2)])
            row = [k.fn('get_%s_key_rest' % k.mode, B, P, Z, C.POINTER(P))(
                inst, offset, C.byref(rest)), key_bytes(rest, 4)]
            if rest.value:
                begin, end, length = C.c_ushort(0xABCD), C.c_ushort(0xABCD), C.c_ushort(0xABCD)
                row += [k.fn('get_%s_key_rest_positions' % k.mode, B, P, P, C.POINTER(C.c_ushort),
                             C.POINTER(C.c_ushort))(inst, rest, C.byref(begin), C.byref(end)),
                        begin.value, end.value,
                        k.fn('get_%s_key_rest_length' % k.mode, B, P, P, C.POINTER(C.c_ushort))(
                            inst, rest, C.byref(length)),
                        length.value]
            out['rests'].append(row)
            characters = Z(UNTOUCHED)
            ret = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
                inst, phrase.encode(), offset, C.byref(characters))
            out['characters'].append([ret, 'untouched' if characters.value == UNTOUCHED
                                      else characters.value])
        guess = (lambda offset: guess_rows(k, inst, offset, 0x1e)) if k.mode == 'pinyin' \
            else (lambda offset: zhuyin_guess_rows(k, inst, offset))
        out['guess 0'] = guess(0)
        out['guess 2'] = guess(2)
        out['sentence'] = k.fn('guess_sentence', B, P)(inst)
        out['after sentence 0'] = guess(0)
        return out
    return run


for _mode in ('pinyin', 'zhuyin'):
    for _text, _phrase in SURFACE_INPUTS:
        # The pinyin facade already reads its divided matrix everywhere.
        case('matrix-resplit-surfaces-%s-%s' % (_mode, _text), mode=_mode,
             control=_mode == 'pinyin', issues=('1031',))(
                 matrix_resplit_surfaces(_text, _phrase))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
