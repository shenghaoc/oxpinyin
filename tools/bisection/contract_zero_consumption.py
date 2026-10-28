"""Zero-consumption sentence cases."""
import ctypes as C

from contract_abi import B, P, S, U, UNTOUCHED, Z


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #710, row 53 item 2: every zero-consumption parse leaves an empty
# matrix. get_nbest_match returns false before clearing sentence rows
# (074a2219, phonetic_lookup.h:743-748), including the prefix-seeded path.
def zero_consumption_sentence(parser, text, with_prefix):
    """Build a fresh-versus-stale sentence comparison after a zero-consumption parse.

    parser is the parse_more_ suffix; text is bytes. with_prefix chooses a
    sentence guess seeded by 你. The callable asserts zero consumption."""
    def probe(k):
        """Compare fresh instances with ones holding a nihao sentence before the zero-consumption parse.

        Set tone options, record parsing/guessing and sentence outputs, and leave
        both allocated instances live."""
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        out = {}
        for stale in (False, True):
            inst = k.alloc()
            def sentence():
                """Read the active instance's first sentence, preserving the sentinel and freeing owned text."""
                value = P(UNTOUCHED)
                if k.mode == 'pinyin':
                    ret = k.fn('get_sentence', B, P, U, C.POINTER(P))(inst, 0, C.byref(value))
                else:
                    ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(value))
                return [ret, 'untouched' if value.value == UNTOUCHED else k.text(value.value)]
            result = {}
            if stale:
                result['initial_parse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
                result['initial_guess'] = k.fn('guess_sentence', B, P)(inst)
                result['initial_sentence'] = sentence()
            result['parse'] = k.fn('parse_more_' + parser, Z, P, S)(inst, text)
            assert result['parse'] == 0, 'the case must leave an empty matrix'
            if with_prefix:
                result['guess'] = k.fn('guess_sentence_with_prefix', B, P, S)(inst, '你'.encode())
            else:
                result['guess'] = k.fn('guess_sentence', B, P)(inst)
            result['sentence'] = sentence()
            out['stale' if stale else 'fresh'] = result
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _parsers = ('full_pinyins', 'chewings', 'double_pinyins') if _mode == 'pinyin' else ('full_pinyins', 'chewings')
    for _parser in _parsers:
        _texts = (b'n', b'!', b'') if _parser == 'full_pinyins' else (b'!', b'')
        for _text in _texts:
            for _prefix in (False, True):
                _label = _text.decode() or 'empty'
                case(f'zero-consumption-{_mode}-{_parser}-{_label}' + ('-prefix' if _prefix else ''),
                     mode=_mode, control=_mode == 'pinyin', issues=('710', 'row:53'))(
                    zero_consumption_sentence(_parser, _text, _prefix))


def register(driver_case):
    """Register the declarations in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
