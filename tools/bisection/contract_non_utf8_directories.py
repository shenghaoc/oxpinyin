"""The non-UTF-8 directory name contract cases (#587).

Collect the declarations here and register them with the driver's decorator at
their original position. Importing this module does not register into a driver.
"""
import ctypes as C
import os

from contract_abi import B, P, S, Z


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# batch2 group 13: directory names that are not UTF-8 (PR 13, #587)
def non_utf8_dirs(k, bad_system, bad_user):
    """Opens a context whose system and/or user directory name holds a 0xFF byte.

    Create the optional system symlink and new user directory under scratch.
    On successful pinyin initialization, parse, guess, train and save, then
    list non-hidden user files using filesystem decoding. Leave the native
    context and instance live."""
    tag = str(os.getpid()).encode()
    scratch = os.fsencode(k.scratch)
    system = os.path.join(scratch, b'system-\xff' + tag) if bad_system else os.fsencode(k.data)
    if bad_system:
        os.symlink(os.fsencode(k.data), system)
    user = os.path.join(scratch, b'user-\xff' + tag) if bad_user else os.fsencode(k.user)
    if bad_user:
        os.mkdir(user)
    ctx = k.init(system=system, user=user)
    out = {'init': bool(ctx)}
    if ctx:
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        out['guess'] = k.fn('guess_sentence', B, P)(inst)
        out['train'] = k.fn('train', B, P, C.c_ubyte)(inst, 0)
        out['save'] = k.fn('save', B, P)(ctx)
        out['user files'] = sorted(os.fsdecode(n) for n in os.listdir(user) if not n.startswith(b'.'))
    return out


for _label, _system, _user in (('system', True, False), ('user', False, True), ('both', True, True)):
    case('non-utf8-directory-' + _label, issues=('587',))(
        lambda k, system=_system, user=_user: non_utf8_dirs(k, system, user))


@case('non-utf8-directory-zhuyin-both', mode='zhuyin', issues=('587',))
def _(k):
    """Exercise zhuyin training and saving with 0xFF in both directory names.

    Create a system symlink and user directory under scratch, returning init
    and successful-init operation flags plus non-hidden filenames. Leave the
    context and instance live."""
    tag = str(os.getpid()).encode()
    scratch = os.fsencode(k.scratch)
    system = os.path.join(scratch, b'system-\xff' + tag)
    os.symlink(os.fsencode(k.data), system)
    user = os.path.join(scratch, b'user-\xff' + tag)
    os.mkdir(user)
    ctx = k.init(system=system, user=user)
    out = {'init': bool(ctx)}
    if ctx:
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
        out['guess'] = k.fn('guess_sentence', B, P)(inst)
        out['train'] = k.fn('train', B, P)(inst)
        out['save'] = k.fn('save', B, P)(ctx)
        out['user files'] = sorted(os.fsdecode(n) for n in os.listdir(user) if not n.startswith(b'.'))
    return out


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
