"""GLib path construction cases (inventory row 53, #1175).
"""
import os
from pathlib import Path
import shutil
import tempfile

from contract_abi import B, P, S, Z
from contract_case_helpers import LIBRARY_FACADES, train_once
from contract_fixtures import private_system


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #643: g_build_filename drops empty elements, but stops at NULL.
def init_system_argument(k, form):
    """Initialize with a literal empty, NULL, dot or absolute system argument in a temporary cwd.

    form='empty-missing' omits system links; other forms link regular data files.
    On successful init, cache the context, parse and guess, then free/finalize
    without clearing caches. Return user filenames and switch cwd to scratch
    on the normal path; exceptions do not restore cwd."""
    with tempfile.TemporaryDirectory(prefix='sys-', dir=k.scratch) as cwd:
        if form != 'empty-missing':
            # Read-only system files; only the separate user directory is writable.
            for path in Path(k.data).iterdir():
                if path.is_file():
                    os.symlink(path, Path(cwd) / path.name)
        os.chdir(cwd)
        system = {'empty': b'', 'empty-missing': b'', 'null': None,
                  'dot': b'.', 'absolute': os.fsencode(cwd)}[form]
        ctx = k.init(system, k.user, literal=True)
        out = {'init': bool(ctx)}
        if ctx:
            k._ctx = ctx
            inst = k.inst
            name = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
            out['parse'] = k.fn(name, Z, P, S)(inst, b'ni3hao3' if k.mode == 'pinyin' else b'su3cl3')
            out['guess'] = k.fn('guess_sentence', B, P)(inst)
            k.fn('free_instance', None, P)(inst)
            k.fn('fini', None, P)(ctx)
        out['files'] = sorted(os.listdir(k.user))
        os.chdir(k.scratch)
        return out


for _mode in ('pinyin', 'zhuyin'):
    for _form in ('empty', 'empty-missing', 'null', 'dot', 'absolute'):
        case('init-system-' + _form + '-' + _mode, mode=_mode,
             control=_form in ('empty-missing', 'dot', 'absolute'), stderr=True, issues=('643',))(
                 lambda k, form=_form: init_system_argument(k, form))


# #922: `g_build_filename(dir, name, NULL)` as the outward result of the
# pin's path joins (074a2219 `pinyin.cpp:252-310`, `:335-368`, `:952-1119`,
# `zhuyin.cpp:196-252`, `:279-300`; GLib 2.89.4 `g_build_path_va`): the file
# each spelling opens, the user directory's file set and the bytes of the
# lines that name a joined path. Separators are repeated or trailing in the
# directory arguments and leading or nested in the `table.conf` file names.
DIR_SPELLINGS = {
    'dup-trailing': lambda path: path + b'///',
    'leading-dup': lambda path: b'/' + path,
    'dot-segments': lambda path: path + b'/./',
    'dotdot': lambda path: path + b'/../' + os.path.basename(path),
}


def spell_dir(form, path):
    """The bytes of `path` spelled as `form` names it; `None` leaves them as they are."""
    path = os.fsencode(path)
    return DIR_SPELLINGS[form](path) if form else path


def dir_spelling_session(k, system_form, user_form):
    """Open, train, save and reopen with the system and user directories spelled as asked.

    Return the flags and the sorted user directory file names. A spelling that
    reached another file would change them or the diagnostics."""
    name, index, parse, text, train_args = LIBRARY_FACADES[k.mode]
    system, user = spell_dir(system_form, k.data), spell_dir(user_form, k.user)
    ctx = k.init(system, user, literal=True)
    out = {'init': bool(ctx)}
    if ctx:
        out['train'] = train_once(k, ctx, parse, text, train_args)
        out['save'] = k.fn('save', B, P)(ctx)
        k.fn('fini', None, P)(ctx)
        again = k.init(system, user, literal=True)
        out['reopen'] = bool(again)
        if again:
            k.fn('fini', None, P)(again)
    out['files'] = sorted(os.listdir(k.user))
    return out


for _mode in ('pinyin', 'zhuyin'):
    for _label, _system, _user in (
            ('system-dup-trailing', 'dup-trailing', None), ('both-leading-dup', 'leading-dup', 'leading-dup'),
            ('both-dot-segments', 'dot-segments', 'dot-segments'), ('both-dotdot', 'dotdot', 'dotdot')):
        case(f'glib-922-dir-{_label}-{_mode}', mode=_mode, control=True, stderr=True, issues=('922',))(
            lambda k, system=_system, user=_user: dir_spelling_session(k, system, user))
    # The user directory's repeated trailing separators reach the `open` line
    # of `user.conf`; the parent build spells them differently, so this is not
    # a control.
    case(f'glib-922-dir-user-dup-trailing-{_mode}', mode=_mode, stderr=True, issues=('922',))(
        lambda k: dir_spelling_session(k, None, 'dup-trailing'))


def fini_write_failed(k):
    """Settle a profile, reopen it with the user directory ending in `///`, remove it, then finalize.

    The marker exists at the second open, so the only line is fini's
    `write <dir>/user.conf failed.`."""
    first = k.init()
    assert first, 'init failed'
    k.fn('fini', None, P)(first)
    ctx = k.init(user=spell_dir('dup-trailing', k.user))
    out = {'init': bool(ctx)}
    shutil.rmtree(k.user)
    k.fn('fini', None, P)(ctx)
    return out


def save_write_failed(k):
    """Open with the user directory ending in `///`, train, remove it, then save and finalize.

    The save's `rename <dir>/<file>.tmp to <dir>/<file> failed.` lines and the
    `write <dir>/user.conf failed.` line name paths joined to that directory."""
    name, index, parse, text, train_args = LIBRARY_FACADES[k.mode]
    ctx = k.init(user=spell_dir('dup-trailing', k.user))
    out = {'init': bool(ctx)}
    out['train'] = train_once(k, ctx, parse, text, train_args)
    shutil.rmtree(k.user)
    out['save'] = k.fn('save', B, P)(ctx)
    k.fn('fini', None, P)(ctx)
    return out


for _mode in ('pinyin', 'zhuyin'):
    case(f'glib-922-save-write-failed-dup-trailing-{_mode}', mode=_mode, stderr=True,
         issues=('922',))(save_write_failed)


# Only pinyin's fini writes the marker (`pinyin.cpp:1194-1200`); zhuyin's has
# no such line.
case('glib-922-fini-write-failed-dup-trailing-pinyin', stderr=True, issues=('922',))(fini_write_failed)


# A `table.conf` file name is a path beneath the system directory however it
# begins. `sub` holds the library for the nested spelling and is empty for the
# `..` one, which resolves back to the directory itself.
ROW_SPELLINGS = {'leading-slash': '/{}', 'double-leading-slash': '//{}',
                 'nested': 'sub//{}', 'dotdot': 'sub/../{}'}


def spelled_system(k, form, name):
    """A private system directory whose `table.conf` names library `name` as `form` spells it.

    The first word that is exactly `name` is replaced. `nested` also links the
    library under `sub`."""
    system = private_system(k)
    conf = Path(system, 'table.conf')
    words = conf.read_text().split(' ')
    spelled = ROW_SPELLINGS[form].format(name)
    for at, word in enumerate(words):
        if word == name:
            words[at] = spelled
            break
    else:
        raise AssertionError(name)
    os.mkdir(Path(system, 'sub'))
    if form == 'nested':
        os.symlink(os.path.join(k.data, name), Path(system, 'sub', name))
    conf.unlink()
    conf.write_text(' '.join(words))
    return system


def row_spelling_session(k, form):
    """Initialize against a library named as `form` spells it and parse one sentence."""
    name, index, parse, text, train_args = LIBRARY_FACADES[k.mode]
    ctx = k.init(system=spelled_system(k, form, 'gb_char.bin'))
    out = {'init': bool(ctx)}
    if ctx:
        inst = k.fn('alloc_instance', P, P)(ctx)
        out['parse'] = k.fn(parse, Z, P, S)(inst, text)
        out['guess'] = k.fn('guess_sentence', B, P)(inst)
        k.fn('free_instance', None, P)(inst)
        k.fn('fini', None, P)(ctx)
    return out


def row_spelling_missing(form, system_form):
    """Build a case whose library `table.conf` spells as `form` is missing from the directory.

    The pin writes `mmap <path> failed!` and dies in the empty chunk (row 68,
    class (b)); `system_form` spells the directory it is named in."""
    def probe(k):
        """Initialize, expecting the library's line and the subject's survival."""
        name = LIBRARY_FACADES[k.mode][0]
        system = spelled_system(k, form, name)
        os.unlink(os.path.join(system, ROW_SPELLINGS[form].format(name).lstrip('/')))
        return {'ctx': bool(k.init(spell_dir(system_form, system), k.user, literal=True))}
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _form in ROW_SPELLINGS:
        case(f'glib-922-row-{_form}-{_mode}', mode=_mode, control=True, stderr=True, issues=('922',))(
            lambda k, form=_form: row_spelling_session(k, form))
    for _label, _form, _system in (('leading-slash', 'leading-slash', None),
                                   ('double-leading-slash', 'double-leading-slash', 'dup-trailing'),
                                   ('nested', 'nested', 'dup-trailing'),
                                   ('dup-trailing-dir', 'leading-slash', 'dup-trailing')):
        case(f'glib-922-row-missing-{_label}-{_mode}', mode=_mode, control=_system is None,
             crash={'ctx': True, 'logs': []}, issues=('922',))(row_spelling_missing(_form, _system))


# The lines that name `table.conf` come from `g_build_filename(system_dir,
# "table.conf", NULL)`: a directory made only of separators keeps them all.
def table_conf_line(k, system, rejected=False):
    """Initialize with `system` (bytes, or a scratch directory name spelled by a function)."""
    if rejected:
        directory = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
        Path(directory, 'table.conf').write_text('not a table.conf\n')
        system = system(os.fsencode(directory))
    return {'ctx': bool(k.init(system, k.user, literal=True))}


for _mode in ('pinyin', 'zhuyin'):
    for _label, _system in (('slash', b'/'), ('slashes', b'//'), ('many-slashes', b'////'),
                            ('missing-dup-trailing', b'/nonexistent-922-system-dir///')):
        case(f'glib-922-table-conf-missing-{_label}-{_mode}', mode=_mode, control=_system != b'//' and _system != b'////',
             stderr=True, issues=('922',))(lambda k, system=_system: table_conf_line(k, system))
    case(f'glib-922-table-conf-rejected-dup-trailing-{_mode}', mode=_mode, control=True, stderr=True, issues=('922',))(
        lambda k: table_conf_line(k, DIR_SPELLINGS['dup-trailing'], rejected=True))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
