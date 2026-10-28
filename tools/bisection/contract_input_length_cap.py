"""The input length cap contract cases (#540).

Collect the declarations here and register them with the driver's decorator at
their original position. Importing this module does not register into a driver.
"""
from contract_abi import B, P, S, Z, sentence_out


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# batch2 group 9: the input length cap (PR 9, #540)
@case('input-length-cap', issues=('540',))
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


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
