"""The double finalization contract cases (#573).

Collect the declarations here and register them with the driver's decorator at
their original position. Importing this module does not register into a driver.
"""
from contract_abi import P


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #573 (B battery): a second pinyin_fini / zhuyin_fini on one context
def fini_twice(k):
    """Finalize one independently initialized context twice and report only if both calls return.

    Allocate and free a live instance before finalization. This deliberately
    reuses the context address and can terminate the isolated worker. The
    lifetime violation targets pinyin_fini in src/pinyin.cpp and zhuyin_fini
    in src/zhuyin.cpp at pin 074a2219, both of which delete the context."""
    # The pin reads and deletes the freed context again (pinyin.cpp:1194-1222,
    # zhuyin.cpp:741-757) and dies of SIGSEGV, with stderr bytes read from
    # freed memory; the ruled answer is a silent return (class (b), row 105).
    context = k.init()
    live = k.fn('alloc_instance', P, P)(context)
    k.fn('free_instance', None, P)(live)
    k.fn('fini', None, P)(context)
    k.fn('fini', None, P)(context)
    return {'second fini returned': True}


case('fini-twice', crash={'second fini returned': True}, stderr_varies=True, issues=('573',))(fini_twice)
case('zhuyin-fini-twice', mode='zhuyin', crash={'second fini returned': True}, stderr_varies=True, issues=('573',))(fini_twice)


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
