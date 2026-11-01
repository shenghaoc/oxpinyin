"""The input length cap contract cases (#540, #1501).

Collect the declarations here and register them with the driver's decorator at
their original position. Importing this module does not register into a driver.
"""
from contract_abi import B, P, S, Z, sentence_out


_REGISTRATIONS = []

# The pin's full-pinyin parse keeps its steps in `gint16`s
# (`pinyin_parser2.cpp:67-70`, `:248`, `:292`), so it stops at byte 32767;
# oxpinyin parses the whole input. A language artefact (#1501).
RECORD = '1501-gint16-input-cap.md'


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# batch2 group 9: the input length cap (PR 9, #540)
@case('input-length-cap', issues=('540', '1501'), policy_record=RECORD)
def _(k):
    """Record parser and stored lengths for six ASCII inputs around size boundaries.

    Create a fresh instance for each byte length from 4,096 through 65,537
    listed in the fixture. Repeat ni and truncate to that byte length, then
    return both native byte counts under its decimal label."""
    out = {}
    for length in (4096, 4097, 32766, 32767, 32768, 65537):
        inst = k.alloc()
        text = (b'ni' * length)[:length]
        out[str(length)] = [k.fn('parse_more_full_pinyins', Z, P, S)(inst, text),
                            k.fn('get_parsed_input_length', Z, P)(inst)]
    return out


@case('long-input-sentence', issues=('540',))
def _(k):
    """Parse 4,800 bytes of repeated ni and record guessing and sentence-length outputs.

    Use a fresh instance and sentence index zero. Return parsed bytes, the
    guess flag, and the sentence-accessor flag plus Python string length or
    None. Any string returned by sentence_out, including its untouched marker,
    is reduced to a length; decoded lengths count code points, not UTF-8 bytes."""
    # A decode past the old cap: the sentence is as long as the pin's.
    inst = k.alloc()
    text = b'ni' * 2400
    out = {'parsed': k.fn('parse_more_full_pinyins', Z, P, S)(inst, text),
           'guess': k.fn('guess_sentence', B, P)(inst)}
    row = sentence_out(k, inst, 0)
    out['sentence'] = [row[0], len(row[1]) if isinstance(row[1], str) else row[1]]
    return out


def boundary(length):
    """Parse `length` bytes of repeated ni on a fresh instance; return the
    parse's answer and the stored parsed length under its decimal label."""
    def run(k):
        inst = k.alloc()
        text = (b'ni' * ((length + 1) // 2))[:length]
        return {str(length): [k.fn('parse_more_full_pinyins', Z, P, S)(inst, text),
                              k.fn('get_parsed_input_length', Z, P)(inst)]}
    return run


# Around the pin's `gint16` bound and the old 65535-byte graph refusal. At
# 32767 bytes both sides parse the whole input; past it the pin stops at
# 32767 and oxpinyin does not.
for _length in (32767, 32768, 65535, 65536):
    case('input-length-boundary-%d' % _length, issues=('1501',),
         **(dict(control=True) if _length == 32767 else dict(policy_record=RECORD)))(boundary(_length))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
