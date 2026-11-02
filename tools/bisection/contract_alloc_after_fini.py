"""The pinyin allocation-after-finalization contract case (#528).

Collect the original decorated function here and register it with the
driver's decorator at its original position. Importing this module does not
register into a driver.
"""
from contract_abi import B, P, S, Z


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# batch2 group 10: pinyin_alloc_instance on a finalised context (PR 10, #528)
@case('alloc-instance-after-fini', issues=('528',))
def _(k):
    """Observe a native allocation attempt after freeing an instance and finalizing its context.

    Create an independent context and record whether its initial instance was
    non-NULL. Keep the post-finalization pointer's truth value only under a
    tilde-prefixed diagnostic key, excluded by observed. Do not free that
    second pointer. This is an isolated-worker case: the invalid lifetime
    access can terminate the process before an observation is returned.
    The pin path is pinyin_alloc_instance after pinyin_fini in src/pinyin.cpp
    at 074a2219."""
    # The pin reads the freed context (pinyin.cpp:1322) and survives by
    # chance; the ruled answer here is NULL (class (b)), so the answer itself
    # is left out of the comparison and the case holds the exit status: the
    # parent build crashes where the pin does not.
    context = k.init()
    live = k.fn('alloc_instance', P, P)(context)
    k.fn('free_instance', None, P)(live)
    k.fn('fini', None, P)(context)
    after = k.fn('alloc_instance', P, P)(context)
    return {'live instance': bool(live), '~instance after fini': bool(after)}



def instance_outlives_context(k):
    """Replay audit D-06's row-42 witness: allocate, fini, then guess.

    The pin dereferences its freed context (pinyin.cpp:1373-1380 at
    074a2219); the ruled safe answer is true. Use the existing driver's
    isolated worker and class-(b) observation, including the zhuyin twin.
    """
    context = k.init()
    instance = k.fn('alloc_instance', P, P)(context)
    # A populated matrix reaches the freed lookup; an empty composition
    # returns false before consulting it (phonetic_lookup.h get_nbest_match).
    parse, text = (('parse_more_chewings', b'su3cl3') if k.mode == 'zhuyin'
                   else ('parse_more_full_pinyins', b'nihao'))
    k.fn(parse, Z, P, S)(instance, text)
    k.fn('fini', None, P)(context)
    answer = k.fn('guess_sentence', B, P)(instance)
    k.fn('free_instance', None, P)(instance)
    return {'guess': answer}


case('instance_outlives_context', crash={'guess': True}, stderr_varies=True,
     issues=('528',))(instance_outlives_context)
case('zhuyin-instance_outlives_context', mode='zhuyin', crash={'guess': True},
     stderr_varies=True, issues=('528',))(instance_outlives_context)


def register(driver_case):
    """Register the declaration with the driver's current decorator."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
