"""Null user and library operations cases (inventory row 33, #1175).
"""
import ctypes as C
import os
from pathlib import Path
import tempfile

from contract_abi import B, I, P, S, U, Z, tokens_of, unigram_of
from contract_case_helpers import remember_export, transient_files


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def null_user_session(k):
    """Exercise a literal NULL user directory while checking cwd and TMPDIR for file changes.

    Create two canary directories, change cwd and TMPDIR, then record training,
    imports, lookup, save and mutation outputs. Compare a second NULL-user
    context's tokens before masking the first. Finalize successful contexts on
    the normal path; TMPDIR and cached pointers are not reset. A failed first
    init returns with cwd still changed; successful completion returns to scratch."""
    cwd = tempfile.mkdtemp(prefix='cwd-', dir=k.scratch)
    tmp = tempfile.mkdtemp(prefix='tmp-', dir=k.scratch)
    for directory in (cwd, tmp):
        Path(directory, 'canary').write_bytes(b'unchanged')
    os.chdir(cwd)
    os.environ['TMPDIR'] = tmp
    baseline = [transient_files(d) for d in (cwd, tmp)]
    out = {}

    def checkpoint(label):
        """Record sorted file names whose mode or contents differ from each canary baseline.

        Include creations and deletions in cwd and the configured temporary directory."""
        out['files-' + label] = [sorted(n for n in before.keys() | after.keys()
                                       if before.get(n) != after.get(n))
                                for before, after in zip(baseline, (transient_files(d) for d in (cwd, tmp)))]

    ctx = k.init(k.data, None, literal=True)
    out['init'] = bool(ctx)
    checkpoint('init')
    if not ctx:
        return out
    k._ctx = ctx
    inst = k.inst
    train = (lambda: k.fn('train', B, P, C.c_ubyte)(inst, 0)) if k.mode == 'pinyin' else (lambda: k.fn('train', B, P)(inst))
    out['train-fresh'] = train()
    checkpoint('train-fresh')
    out['save-fresh'] = k.fn('save', B, P)(ctx)
    checkpoint('save-fresh')
    phrase = '你好你好'
    reading = ("ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ')
    reading += ("'" if k.mode == 'pinyin' else ' ') + reading
    added = []
    for library in (7,):
        it = k.fn('begin_add_phrases', P, P, U)(ctx, library)
        added.append(k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading.encode(), 100000))
        k.fn('end_add_phrases', None, P)(it)
    out['added'] = added
    checkpoint('import')
    name = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
    raw = b'ni3hao3' if k.mode == 'pinyin' else b'su3cl3'
    out['parse'] = k.fn(name, Z, P, S)(inst, raw + raw)
    out['guess'] = k.fn('guess_sentence', B, P)(inst)
    out['tokens'] = tokens_of(k, phrase)
    if k.mode == 'pinyin':
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1f)
    else:
        k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    cand, text = P(), S()
    out['candidate0-ok'] = k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand))
    if cand.value:
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        out['candidate0'] = text.value.decode() if text.value else None
        if k.mode == 'pinyin':
            out['candidate0-user'] = k.fn('is_user_candidate', B, P, P)(inst, cand)
    checkpoint('lookup')
    out['train'] = train()
    if k.mode == 'pinyin':
        out['train-invalid-index'] = k.fn('train', B, P, C.c_ubyte)(inst, 255)
    checkpoint('train')
    out['save'] = k.fn('save', B, P)(ctx)
    checkpoint('save')
    token = tokens_of(k, '你')[0]
    out['frequency-before'] = unigram_of(k, token)
    out['frequency-add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(inst, token, 7)
    out['frequency-after'] = unigram_of(k, token)
    if k.mode == 'pinyin':
        out['remember'] = k.fn('remember_user_input', B, P, S, I)(inst, phrase.encode(), 3)
        export = k.fn('begin_get_phrases', P, P, U)(ctx, 7)
        rows = []
        while k.fn('iterator_has_next_phrase', B, P)(export):
            ph, py, count = P(), P(), I()
            ret = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
                export, C.byref(ph), C.byref(py), C.byref(count))
            rows.append([ret, k.text(ph.value), k.text(py.value), count.value])
        k.fn('end_get_phrases', None, P)(export)
        out['export'] = rows
    other = k.init(k.data, None, literal=True)
    own_ctx, own_inst = k._ctx, k._inst
    k._ctx, k._inst = other, None
    out['other-user-tokens'] = [t for t in tokens_of(k, phrase) if t >> 24 == 7]
    k.fn('free_instance', None, P)(k._inst)
    k.fn('fini', None, P)(other)
    k._ctx, k._inst = own_ctx, own_inst
    out['mask'] = k.fn('mask_out', B, P, U, U)(ctx, 0x0f000000, 0x07000000)
    out['tokens-after-mask'] = [t for t in tokens_of(k, phrase) if t >> 24 == 7]
    checkpoint('mutations')
    k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    checkpoint('fini')
    os.chdir(k.scratch)
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('null-user-session-' + _mode, mode=_mode, stderr=True, issues=('642',))(null_user_session)


def transient_library(k,library,null):
    """Import 你好 into the chosen library with either a literal NULL or ordinary user path.

    Switch cwd to scratch and replace the cached context. Return init/import
    flags, matching library tokens and exported rows, then free/finalize without
    clearing cached pointers. Native import flags are not asserted."""
    os.chdir(k.scratch)
    k._ctx=k.init(k.data, None if null else k.user, literal=True)
    out={'init':bool(k._ctx)}
    it=k.fn('begin_add_phrases',P,P,U)(k.ctx,library)
    out['begin']=bool(it)
    out['add']=k.fn('iterator_add_phrase',B,P,S,S,I)(it,'你好'.encode(),b"ni3'hao3" if k.mode=='pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode(),100000)
    k.fn('end_add_phrases',None,P)(it)
    out['tokens']=[t for t in tokens_of(k,'你好') if t>>24==library]
    out['export']=remember_export(k,library,'你好')
    k.fn('free_instance',None,P)(k.inst)
    k.fn('fini',None,P)(k.ctx)
    return out


for _mode in ('pinyin', 'zhuyin'):
    for _library in range(1, 8):
        for _null in (False, True):
            case('user-library-' + str(_library) + ('-null-' if _null else '-ordinary-') + _mode,
                 mode=_mode, control=not _null, stderr=True, issues=('642',))(
                lambda k, library=_library, null=_null: transient_library(k, library, null))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
