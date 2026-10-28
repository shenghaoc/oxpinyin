"""The default option word contract cases (#532).

Collect the declarations here and register them with the driver's decorator at
their original position. Importing this module does not register into a driver.
"""
from contract_abi import B, P, U, consumed, key_out


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# lane C, #532: the option word `pinyin_init` seeds is USE_TONE.
@case('default-option-parse', issues=('532',))
def _(k):
    """Record consumed bytes for complete, incomplete and invalid default-option inputs.

    Parse each spelling on a fresh instance without changing context options."""
    return {t.decode(): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'su3cl3', b'ni2hao', b'n', b'zzz', b'nihao', b'ni3')}


@case('default-option-single-key', issues=('532',))
def _(k):
    """Record single-key return flags and raw keys under the initial context options.

    Reuse the cached instance for the tone, incomplete and invalid spellings."""
    return {t.decode(): key_out(k, t) for t in (b'n', b'ni3', b'ni', b'a1', b'zhang', b'zzz')}


@case('explicit-use-tone-parse', control=True, issues=('532',))
def _(k):
    """Set only USE_TONE and record consumed bytes for each spelling on a fresh instance."""
    k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    return {t.decode(): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'su3cl3', b'ni2hao', b'n', b'zzz', b'nihao')}


@case('explicit-incomplete-parse', control=True, issues=('532',))
def _(k):
    """Set only PINYIN_INCOMPLETE and record consumed bytes on fresh instances."""
    k.fn('set_options', B, P, U)(k.ctx, 1 << 3)
    return {t.decode(): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'su3cl3', b'ni2hao', b'n', b'zzz', b'nihao')}


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
