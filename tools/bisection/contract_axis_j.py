"""The axis J reachability control cases.

Collect the declarations here and register them with the driver's decorator at
their original position. Importing this module does not register into a driver.
"""
import ctypes as C

from contract_abi import B, I, P, S, Z, guessed, row_of


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# Axis J targeted reachability controls, at libpinyin 074a2219:
# pinyin.cpp:2515-2520 calls ForwardPhoneticConstraints::diff_result when
# choosing a displayed NBEST row. No forged candidate or context is needed.
@case('j-choose-nbest-diff-result', control=True)
def _(k):
    """Choose a displayed NBEST row and record its rank output and signed selection result.

    Assert a row exists and initialize rank to 0xff without asserting getter success."""
    inst = guessed(k)
    candidate = row_of(k, inst, 1)
    assert candidate, 'no displayed NBEST candidate'
    rank = C.c_ubyte(0xff)
    ranked = k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
        inst, candidate, C.byref(rank))
    chosen = k.fn('choose_candidate', I, P, Z, P)(inst, 0, candidate)
    return {'ranked': ranked, 'rank': rank.value, 'chosen': chosen}


# Scheme HSU (2) constructs ZhuyinDiscreteParser2; Dachen CP26 (9)
# constructs ZhuyinDaChenCP26Parser2. Batch parse goes through their parse
# virtuals (pinyin.cpp:1595 / zhuyin.cpp:1068), rather than one-key parsing.
def j_multi_chewing(scheme):
    """Build a multi-key chewing parse case for the selected facade scheme number."""
    def probe(k):
        """Select the facade's scheme, parse abc and record consumed and reported input lengths.

        Record selection failure rather than asserting it; lengths are byte counts."""
        setter = 'set_zhuyin_scheme' if k.mode == 'pinyin' else 'set_chewing_scheme'
        selected = k.fn(setter, B, P, I)(k.ctx, scheme)
        parsed = k.fn('parse_more_chewings', Z, P, S)(k.inst, b'abc')
        length = k.fn('get_parsed_input_length', Z, P)(k.inst)
        return {'scheme': scheme, 'selected': selected, 'parsed': parsed, 'length': length}
    return probe


for _j_mode in ('pinyin', 'zhuyin'):
    for _j_scheme in (2, 9):
        case(f'j-chewing-multi-key-{_j_mode}-{_j_scheme}', mode=_j_mode,
             control=True)(j_multi_chewing(_j_scheme))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
