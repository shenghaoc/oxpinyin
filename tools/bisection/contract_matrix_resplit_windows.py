"""Matrix resplit windows cases (inventory row 54, #1175).
"""
from contract_abi import B, P, S, U, Z
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


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
