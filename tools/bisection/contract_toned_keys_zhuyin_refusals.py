"""Toned keys and zhuyin refusals cases (inventory row 42, #1175).
"""
import ctypes as C

from contract_abi import B, I, P, S, U, UNTOUCHED, Z, full_inst, zhuyin_chewing
from contract_case_helpers import FACADE_TOTAL


_REGISTRATIONS = {}
_RUN = 'register'


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.setdefault(_RUN, []).append((name, args, kwargs, fn))
        return fn
    return collect


# batch2 group 12e: a tone digit on an initial-only key (PR 12e, #525, row 4)
TONE_INCOMPLETE = (1 << 5) | (1 << 3)


def toned_initial(k, text=b'n4'):
    """Enable tone and incomplete-pinyin options and return a new instance plus consumed bytes.

    Parse text as full pinyin, leaving the context's options set and the
    instance allocated for the caller."""
    k.fn('set_options', B, P, U)(k.ctx, TONE_INCOMPLETE)
    inst = k.alloc()
    return inst, k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)


@case('abort-is-incomplete-toned-initial', abort=False, issues=('525', 'row:4'))
def _(k):
    """Parse n4 as one key and query its incomplete status with tone/incomplete options set.

    Ignore the parse flag and record only the incomplete-query result."""
    k.fn('set_options', B, P, U)(k.ctx, TONE_INCOMPLETE)
    inst = k.alloc()
    key = C.c_uint16(0)
    parsed = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(inst, b'n4', C.byref(key))
    return {'ret': k.fn('get_pinyin_is_incomplete', B, P, C.POINTER(C.c_uint16))(inst, C.byref(key))}


@case('abort-guess-candidates-toned-initial-before-offset', abort=False, issues=('525', 'row:4', '662'))
def _(k):
    """Guess candidates at byte two of n4ni with a toned initial before the lookup offset."""
    # The toned key sits before the lookup offset, outside the window the
    # search walks: the pin dies all the same (review of #662 expected it not
    # to), so the guard looks at the whole matrix.
    inst, _ = toned_initial(k, b'n4ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 2, 0)}


@case('abort-guess-candidates-toned-initial', abort=False, issues=('525', 'row:4'))
def _(k):
    """Guess candidates at byte zero after parsing n4 with tone/incomplete options enabled."""
    inst, _ = toned_initial(k)
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('abort-guess-sentence-toned-initial', abort=False, issues=('525', 'row:4'))
def _(k):
    """Guess a sentence from n4 with tone/incomplete options enabled and record its flag."""
    inst, _ = toned_initial(k)
    return {'ret': k.fn('guess_sentence', B, P)(inst)}


@case('abort-guess-sentence-toned-initial-after-key', abort=False, issues=('525', 'row:4'))
def _(k):
    """Guess a sentence from nihaon4 with a toned initial following the complete keys."""
    inst, _ = toned_initial(k, b'nihaon4')
    return {'ret': k.fn('guess_sentence', B, P)(inst)}


# The keys the guard leaves alone: a toned complete key, an untoned initial.
@case('toned-initial-neighbours', control=True, issues=('525', 'row:4'))
def _(k):
    """Collect parsing, guessing and incomplete-key outputs for neighboring inputs.

    Enable tone/incomplete options. Use fresh multi-key instances per input,
    then a separate instance for single-key queries, returning consumed bytes,
    flags and raw 16-bit keys."""
    out = {}
    k.fn('set_options', B, P, U)(k.ctx, TONE_INCOMPLETE)
    for text in (b'ni4', b'n', b'nin', b'ni4hao'):
        inst = k.alloc()
        n = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
        out[text.decode()] = [n, k.fn('guess_sentence', B, P)(inst),
                              k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)]
    inst = k.alloc()
    for text in (b'n', b'ni4', b'ni'):
        key = C.c_uint16(0)
        parsed = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(inst, text, C.byref(key))
        out['incomplete ' + text.decode()] = [parsed, key.value,
                                              k.fn('get_pinyin_is_incomplete', B, P, C.POINTER(C.c_uint16))(inst, C.byref(key))]
    return out


_RUN = 'register_2'


def offset_call(name, fresh, offset):
    """Capture an offset export, byte position and whether its case starts unparsed."""
    def run(k):
        """Call the captured offset export on a fresh or su3cl3-parsed instance.

        Seed a size_t output with UNTOUCHED but return only the native flag."""
        inst = k.alloc() if fresh else zhuyin_chewing(k)
        out = Z(UNTOUCHED)
        return {'ret': k.fn(name, B, P, Z, C.POINTER(Z))(inst, offset, C.byref(out))}
    return run


def guess_call(name, offset):
    """Build a candidate-guess case at the captured byte offset after chewing input."""
    def run(k):
        """Parse su3cl3 on a new instance and return the captured candidate export's flag."""
        return {'ret': k.fn(name, B, P, Z)(zhuyin_chewing(k), offset)}
    return run


def key_call(name, fresh, offset):
    """Build a key-pointer case for an unparsed or chewing-parsed instance and byte offset."""
    def run(k):
        """Call the captured key accessor with a seeded pointer and retain only its flag.

        Allocate an unparsed instance when fresh is true, otherwise parse su3cl3;
        the output pointer is neither dereferenced nor freed."""
        inst = k.alloc() if fresh else zhuyin_chewing(k)
        out = P(UNTOUCHED)
        return {'ret': k.fn(name, B, P, Z, C.POINTER(P))(inst, offset, C.byref(out))}
    return run


def char_offset_call(offset):
    """Build a 你好 character-offset query for the captured byte position."""
    def run(k):
        """Parse su3cl3, query 你好's character offset and return only the native flag."""
        out = Z(UNTOUCHED)
        return {'ret': k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
            zhuyin_chewing(k), '你好'.encode(), offset, C.byref(out))}
    return run


case('abort-zhuyin-get-left-zhuyin-offset-past-matrix', mode='zhuyin', abort=False, issues=('525',))(
    offset_call('get_left_zhuyin_offset', False, 99))
case('abort-zhuyin-get-right-zhuyin-offset-past-matrix', mode='zhuyin', abort=False, issues=('525',))(
    offset_call('get_right_zhuyin_offset', False, 99))
case('abort-zhuyin-get-right-zhuyin-offset-empty-matrix', mode='zhuyin', abort=False, issues=('525',))(
    offset_call('get_right_zhuyin_offset', True, 0))
case('abort-zhuyin-guess-candidates-after-cursor-past-matrix', mode='zhuyin', abort=False, issues=('525',))(
    guess_call('guess_candidates_after_cursor', 99))
case('abort-zhuyin-guess-candidates-before-cursor-past-matrix', mode='zhuyin', abort=False, issues=('525',))(
    guess_call('guess_candidates_before_cursor', 99))
case('abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one', mode='zhuyin', abort=False, issues=('525',))(
    guess_call('guess_candidates_after_cursor', 7))
case('abort-zhuyin-guess-candidates-before-cursor-reserved-slot-plus-one', mode='zhuyin', abort=False, issues=('525',))(
    guess_call('guess_candidates_before_cursor', 7))
case('abort-zhuyin-get-character-offset-past-matrix', mode='zhuyin', abort=False, issues=('525',))(char_offset_call(99))
case('abort-zhuyin-get-zhuyin-key-empty-matrix', mode='zhuyin', abort=False, issues=('525',))(
    key_call('get_zhuyin_key', True, 0))
case('abort-zhuyin-get-zhuyin-key-rest-empty-matrix', mode='zhuyin', abort=False, issues=('525',))(
    key_call('get_zhuyin_key_rest', True, 0))


# The zhuyin zero total aborts the pin only while a searched row is ranked
# (`zhuyin.cpp:1261`): the reserved slot after the cursor and the start before
# it hold none, and the pin answers true with no warning.
for _name, _offset in (('after_cursor', 6), ('before_cursor', 0)):
    def _no_rank_z(name, offset):
        """Capture the before/after-cursor suffix and byte offset for a zero-total case."""
        def run(k):
            """Apply the fixture-derived total-wrap delta and observe a zhuyin candidate window.

            Parse su3cl3 on a new instance, ignore the add flag and return the guess
            flag and row count. The delta assumes the context total is FACADE_TOTAL;
            its update follows wrap_total_to_zero's pin-derived arithmetic."""
            inst = zhuyin_chewing(k)
            k.fn('token_add_unigram_frequency', B, P, U, U)(inst, (1 << 24) | 1, (2 ** 32 - FACADE_TOTAL) % 2 ** 32)
            count = U()
            ret = k.fn('guess_candidates_' + name, B, P, Z)(inst, offset)
            k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            return {'ret': ret, 'rows': count.value}
        return run
    case('zhuyin-total-zero-ranks-nothing-' + _name.replace('_', '-'), mode='zhuyin', control=True, issues=('525',))(
        _no_rank_z(_name, _offset))


@case('abort-zhuyin-parse-full-pinyin-apostrophe', mode='zhuyin', abort=False, issues=('525',))
def _(k):
    """Pass n'i to zhuyin's full-pinyin parser and test whether its 16-bit key stays untouched."""
    key = C.c_uint16(UNTOUCHED & 0xFFFF)
    ret = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(k.inst, b"n'i", C.byref(key))
    return {'ret': ret, 'untouched key': key.value == UNTOUCHED & 0xFFFF}


@case('abort-zhuyin-guess-candidates-total-zero', mode='zhuyin', abort=False, issues=('525',))
def _(k):
    """Guess zhuyin candidates at zero after applying the fixture-derived total-wrap delta.

    Parse su3cl3 first; record only the guess flag, without checking the add result."""
    inst = zhuyin_chewing(k)
    k.fn('token_add_unigram_frequency', B, P, U, U)(inst, (1 << 24) | 1, (2 ** 32 - FACADE_TOTAL) % 2 ** 32)
    return {'ret': k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)}


# The zhuyin sites' graceful neighbours answer without a warning.
@case('zhuyin-cursor-neighbours', mode='zhuyin', control=True, issues=('525',))
def _(k):
    """Collect neighboring zhuyin offset, key and candidate calls on parsed and empty instances.

    Preserve numeric-output sentinels, record key/rest accessor flags, and
    compare candidate guesses at start and end of the six-byte chewing input."""
    out = {}
    inst = zhuyin_chewing(k)
    res = Z(UNTOUCHED)
    out['offset 99'] = [k.fn('get_zhuyin_offset', B, P, Z, C.POINTER(Z))(inst, 99, C.byref(res)),
                        'untouched' if res.value == UNTOUCHED else res.value]
    for label, name, offset in (('left 0', 'get_left_zhuyin_offset', 0), ('left 3', 'get_left_zhuyin_offset', 3),
                                ('right 0', 'get_right_zhuyin_offset', 0), ('right 3', 'get_right_zhuyin_offset', 3)):
        res = Z(UNTOUCHED)
        out[label] = [k.fn(name, B, P, Z, C.POINTER(Z))(inst, offset, C.byref(res)),
                      'untouched' if res.value == UNTOUCHED else res.value]
    for label, name, offset in (('key 0', 'get_zhuyin_key', 0), ('key 99', 'get_zhuyin_key', 99),
                                ('rest 3', 'get_zhuyin_key_rest', 3)):
        ptr = P(UNTOUCHED)
        out[label] = k.fn(name, B, P, Z, C.POINTER(P))(inst, offset, C.byref(ptr))
    out['empty left'] = k.fn('get_left_zhuyin_offset', B, P, Z, C.POINTER(Z))(k.alloc(), 0, C.byref(Z()))
    out['empty guess'] = k.fn('guess_candidates_after_cursor', B, P, Z)(k.alloc(), 0)
    out['guess 0'] = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    out['guess end before'] = k.fn('guess_candidates_before_cursor', B, P, Z)(inst, 6)
    out['guess end after'] = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 6)
    return out


@case('zhuyin-scheme-and-library-neighbours', mode='zhuyin', control=True, issues=('525',))
def _(k):
    """Record zhuyin scheme changes and library boundary operations on one context.

    Apply the listed setters and load/unload calls in order and return their flags."""
    out = {}
    for label, name, arg in (('chewing 1', 'set_chewing_scheme', 1), ('chewing 6', 'set_chewing_scheme', 6),
                             ('chewing 8', 'set_chewing_scheme', 8), ('chewing 9', 'set_chewing_scheme', 9),
                             ('full 1', 'set_full_pinyin_scheme', 1), ('full 3', 'set_full_pinyin_scheme', 3)):
        out[label] = k.fn(name, B, P, I)(k.ctx, arg)
    for label, name, arg in (('load 1', 'load_phrase_library', 1), ('load 7', 'load_phrase_library', 7),
                             ('load 16', 'load_phrase_library', 16), ('load 255', 'load_phrase_library', 255),
                             ('unload 1', 'unload_phrase_library', 1)):
        out[label] = k.fn(name, B, P, C.c_ubyte)(k.ctx, arg)
    return out


# The pinyin sites the offset probes of this group found still silent.
@case('abort-get-right-pinyin-offset-empty-matrix', abort=False, issues=('525',))
def _(k):
    """Query the right pinyin offset at zero on an unparsed instance, recording only the flag."""
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_right_pinyin_offset', B, P, Z, C.POINTER(Z))(k.alloc(), 0, C.byref(out))}


@case('abort-guess-candidates-past-matrix', abort=False, issues=('525',))
def _(k):
    """Guess nihao candidates at byte 99 with sort zero and record the native flag."""
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(full_inst(k, b'nihao'), 99, 0)}


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS.get('register', ()):
        driver_case(name, *args, **kwargs)(fn)


def register_2(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS.get('register_2', ()):
        driver_case(name, *args, **kwargs)(fn)
