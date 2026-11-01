"""Key positions past byte 65535, double pinyin and chewing (#1501).

Collect the declarations here and register them with the driver's decorator at
their original position. Importing this module does not register into a driver.
"""
import ctypes as C

from contract_abi import B, I, P, S, U, UNTOUCHED, Z


_REGISTRATIONS = []

# The pin's double-pinyin and chewing parsers count in `int`s and store each
# key's begin and end in `ChewingKeyRest`'s `guint16`s (`chewing_key.h:102-103`,
# `pinyin_parser2.cpp:565`, `zhuyin_parser2.cpp:259`), so past byte 65535 the
# positions wrap mod 2^16; the C getters hand them out wrapped on both sides.
# `fill_matrix` places each key at its wrapped begin
# (`phonetic_key_matrix.cpp:51-55`); oxpinyin keeps real positions, which the
# maintainer's 2026-11-01 UTC ruling accepts.
RECORD = '1501-guint16-key-positions.md'

# `USE_TONE` alone (`pinyin_custom2.h:36`): no fuzzy keys, so each column holds
# one key per 65536-byte lap.
OPTIONS = 1 << 5
# A three-byte toned head, then 32769 two-byte keys: 65541 bytes whose keys
# begin at 0, 3, 5, ..., 65539. The key at 65535..65537 crosses byte 65536;
# the key at 65537..65539 lies wholly past it.
UNITS = 32769
LENGTH = 3 + 2 * UNITS
CROSSING, PAST, LAST, CURSOR = 65535, 65537, 65539, 65538

# label -> (facade, scheme setter, scheme, parser, head, unit). The zhuyin
# facade has no double-pinyin parser.
PARSERS = {
    'double-pinyin': ('pinyin', 'set_double_pinyin_scheme', 2, 'parse_more_double_pinyins', b'ni3', b'ni'),
    'chewing-pinyin': ('pinyin', 'set_zhuyin_scheme', 1, 'parse_more_chewings', b'su3', b'su'),
    'chewing-zhuyin': ('zhuyin', 'set_chewing_scheme', 1, 'parse_more_chewings', b'su3', b'su'),
}


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def parsed(k, label):
    """Configure the context for `label`, parse its 65541-byte input on the
    kit's instance and return the parse's answer and the stored length."""
    _mode, setter, scheme, parser, head, unit = PARSERS[label]
    assert k.fn('set_options', B, P, U)(k.ctx, OPTIONS)
    assert k.fn(setter, B, P, I)(k.ctx, scheme)
    return [k.fn(parser, Z, P, S)(k.inst, head + unit * UNITS),
            k.fn('get_parsed_input_length', Z, P)(k.inst)]


def rest_row(k, offset):
    """Return the key-rest getter's answer at `offset` and, when it hands out
    a rest, the key, the rest's positions and its length."""
    rest = P()
    row = [k.fn('get_%s_key_rest' % k.mode, B, P, Z, C.POINTER(P))(k.inst, offset, C.byref(rest))]
    if row[0] and rest.value:
        key = P()
        k.fn('get_%s_key' % k.mode, B, P, Z, C.POINTER(P))(k.inst, offset, C.byref(key))
        begin, end, length = C.c_ushort(0xABCD), C.c_ushort(0xABCD), C.c_ushort(0xABCD)
        row += [C.string_at(key.value, 2).hex() if key.value else None,
                k.fn('get_%s_key_rest_positions' % k.mode, B, P, P, C.POINTER(C.c_ushort),
                     C.POINTER(C.c_ushort))(k.inst, rest, C.byref(begin), C.byref(end)),
                begin.value, end.value,
                k.fn('get_%s_key_rest_length' % k.mode, B, P, P, C.POINTER(C.c_ushort))(
                    k.inst, rest, C.byref(length)),
                length.value]
    return row


def crossing(label):
    """The parse length and the crossing key's getters: both MATCH."""
    def run(k):
        out = {'parsed': parsed(k, label)}
        out['rest %d' % CROSSING] = rest_row(k, CROSSING)
        return out
    return run


def past(label):
    """The key and key-rest getters at offsets past 65535: the pin's column
    there is empty (POLICY-ACCEPTED)."""
    def run(k):
        parsed(k, label)
        return {'rest %d' % offset: rest_row(k, offset) for offset in (PAST, LAST)}
    return run


def cursor(label):
    """The offset getters around byte 65536: the pin's follow its wrapped
    columns (POLICY-ACCEPTED)."""
    def run(k):
        parsed(k, label)
        out = {}
        for key, name, offset in (('offset', 'get_%s_offset', CURSOR), ('right', 'get_right_%s_offset', CROSSING),
                                  ('left', 'get_left_%s_offset', PAST)):
            result = C.c_size_t(UNTOUCHED)
            out['%s %d' % (key, offset)] = [
                k.fn(name % k.mode, B, P, Z, C.POINTER(Z))(k.inst, offset, C.byref(result)), result.value]
        return out
    return run


for _label, (_mode, *_rest) in PARSERS.items():
    case('key-position-past-65535/%s-crossing-key' % _label, mode=_mode, issues=('1501',))(crossing(_label))
    case('key-position-past-65535/%s-past-key' % _label, mode=_mode, issues=('1501',),
         policy_record=RECORD)(past(_label))
    case('key-position-past-65535/%s-cursor' % _label, mode=_mode, issues=('1501',),
         policy_record=RECORD)(cursor(_label))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
