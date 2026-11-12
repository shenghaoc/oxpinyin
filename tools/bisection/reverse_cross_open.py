#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Local-only cross-open checks for the user profile formats.

    reverse_cross_open.py --cell tkrzw --pin-prefix PREFIX --data DIR \
        target/debug/libpinyin_capi.so target/debug/libzhuyin_capi.so

The four handoff cases run oxpinyin first, then the pin, in fresh workers. The
pin reads the profile oxpinyin wrote and re-saves it; the case asserts the rows
and the saved bytes. No workflow or just recipe calls this module.
"""
import argparse
import ctypes as C
import hashlib
import json
import os
from pathlib import Path
import runpy
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/bisection'))
sys.path.insert(0, str(ROOT))

from contract_abi import Arr, B, I, P, S, U  # noqa: E402
from contract_case_helpers import TOKEN_REUSE_PHRASES, d18_gram  # noqa: E402
import contract_fixtures  # noqa: E402
from contract_fixtures import (  # noqa: E402
    _craft_bdb, _craft_kc, _craft_tkrzw, _write_user_conf,
)
from contract_fixtures import index_file_rows as _index_file_rows  # noqa: E402
from contract_kit import Kit  # noqa: E402

check_tkrzw = runpy.run_path(str(ROOT / 'tools/tkrzw/runtime.py'))['check']

CELL = 'tkrzw'
PIN_LIBRARIES = {'pinyin': 'libpinyin.so', 'zhuyin': 'libzhuyin.so'}
TIMEOUT = 300


def index_file_rows(path, *, kind='tree', allow_empty=False):
    """The container's rows in the selected cell."""
    return _index_file_rows(path, cell=CELL, kind=kind, allow_empty=allow_empty)


def _craft_db(path, rows, dbtype, keep=False, *, system_index=False):
    """A user container of raw rows in the selected cell."""
    if CELL == 'kc':
        assert not (keep or system_index) or dbtype == 1, 'KC system indexes are btrees'
        if system_index:
            return _craft_kc(path, rows, keep=keep, system_index=True)
        return _craft_kc(path, rows, keep=keep)
    if CELL == 'tkrzw':
        return _craft_tkrzw(path, rows, 'HashDBM' if dbtype == 2 else 'TreeDBM', keep=keep)
    return _craft_bdb(path, rows, dbtype, keep=keep)


def _craft_hash(path, rows):
    """A hash container of rows: the pin's user bigram layout."""
    _craft_db(path, rows, 2)


def crafted_bigram_context(k, rows):
    """A context whose user directory carries the crafted user bigram rows."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    _craft_hash(os.path.join(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    return ctx


def tkrzw_saved_bigram_bytes(raw):
    """Saved HashDBM bytes with only the timestamp bytes zeroed."""
    assert len(raw) >= 128, 'saved HashDBM is shorter than its metadata'
    masked = bytearray(raw)
    masked[48:56] = b'\0' * 8
    return bytes(masked)


def issue1114_collision_saves(k):
    """Load or save three keys in TinyDBM bucket 74121 and compare saved bytes."""
    rows = []
    for token, degree in ((0x01000d46, 1), (0x01004404, 6), (0x01004dd8, 100)):
        value = (69 * degree).to_bytes(4, 'little') + b''.join(
            item.to_bytes(4, 'little') + (69).to_bytes(4, 'little')
            for item in range(0x01000001, 0x01000001 + degree))
        rows.append((token.to_bytes(4, 'little'), value))
    handoff = os.environ.get('CONTRACT_1114_PROFILE')
    if handoff:
        k.user = handoff
    reloading = os.environ.get('CONTRACT_1114_RELOAD') == '1'
    ctx = k.init() if reloading else crafted_bigram_context(k, rows)
    assert ctx
    path = Path(k.user, 'user_bigram.db')
    original = path.read_bytes() if reloading else None
    expected_rows = sorted([[key.hex(), value.hex()] for key, value in rows])
    saved = []
    for text, pinyin, zhuyin in (('达达', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'),
                                ('你你', "ni3'ni3", 'ㄋㄧˇ ㄋㄧˇ')):
        iterator = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        assert iterator
        reading = pinyin if k.mode == 'pinyin' else zhuyin
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(
            iterator, text.encode(), reading.encode(), 100)
        k.fn('end_add_phrases', None, P)(iterator)
        assert k.fn('save', B, P)(ctx)
        raw = path.read_bytes()
        if original is not None:
            assert tkrzw_saved_bigram_bytes(raw) == tkrzw_saved_bigram_bytes(original), (
                'cross-reload changed saved bytes')
        actual_rows = index_file_rows(path, kind='hash')
        assert actual_rows == expected_rows, 'cross-reload changed a key or value'
        saved.append(dict(bytes=len(raw),
                          sha256=hashlib.sha256(tkrzw_saved_bigram_bytes(raw)).hexdigest(),
                          rows=actual_rows))
        k.fn('fini', None, P)(ctx)
        ctx = k.init()
        assert ctx
    k.fn('fini', None, P)(ctx)
    return {'saved_bigram': saved}


def _save929_import(k, ctx, number, count=100):
    entry = TOKEN_REUSE_PHRASES[number]
    iterator = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    assert iterator
    assert k.fn('iterator_add_phrase', B, P, S, S, I)(
        iterator, entry[0].encode(), entry[1 if k.mode == 'pinyin' else 2].encode(), count)
    k.fn('end_add_phrases', None, P)(iterator)


def _save929_tokens(k, ctx):
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert inst
    k.glib.g_array_new.restype, k.glib.g_array_new.argtypes = P, [I, I, U]
    k.glib.g_array_free.restype, k.glib.g_array_free.argtypes = P, [P, I]
    result = []
    try:
        for text, _, _ in TOKEN_REUSE_PHRASES[:3]:
            arr = k.glib.g_array_new(0, 0, 4)
            try:
                k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
                view = C.cast(arr, C.POINTER(Arr)).contents
                result.append(list(C.cast(view.data, C.POINTER(U))[:view.len]))
            finally:
                k.glib.g_array_free(arr, 1)
    finally:
        k.fn('free_instance', None, P)(inst)
    return result


def save_929_logical_roundtrip(k):
    """Dump exact key/value records and re-save through the receiving facade."""
    handoff = os.environ.get('CONTRACT_929_PROFILE')
    if handoff:
        k.user = handoff
    bigram_key = (0x07000001).to_bytes(4, 'little')
    bigram_value = d18_gram(37, 0x07000002, 37)
    if os.environ.get('CONTRACT_929_RELOAD') != '1':
        k._ctx = crafted_bigram_context(k, [(bigram_key, bigram_value)])
        ctx = k.ctx
        _save929_import(k, ctx, 0)
        _save929_import(k, ctx, 1)
        assert k.fn('save', B, P)(ctx)
        _save929_import(k, ctx, 2)
        k.fn('fini', None, P)(ctx)
        k._ctx = None

    def rows():
        return {name: index_file_rows(Path(k.user, name),
                                      kind='hash' if name == 'user_bigram.db' else 'tree',
                                      allow_empty=True)
                for name in ('user_pinyin_index.bin', 'user_phrase_index.bin', 'user_bigram.db')}

    before = rows()
    assert before['user_bigram.db'] == [[bigram_key.hex(), bigram_value.hex()]]
    ctx = k.ctx
    tokens = _save929_tokens(k, ctx)
    assert tokens == [[0x07000001], [0x07000002], []], tokens
    _save929_import(k, ctx, 0, count=0)
    assert k.fn('save', B, P)(ctx)
    after = rows()
    assert before == after, 'facade reopen/save changed logical records'
    k.fn('fini', None, P)(ctx)
    k._ctx = None
    return dict(rows=after, tokens=tokens)


CASES = {
    'issue1114-collision-load-save-pinyin': ('pinyin', issue1114_collision_saves),
    'issue1114-collision-load-save-zhuyin': ('zhuyin', issue1114_collision_saves),
    'save-929-logical-cross-open-pinyin': ('pinyin', save_929_logical_roundtrip),
    'save-929-logical-cross-open-zhuyin': ('zhuyin', save_929_logical_roundtrip),
}


def run_worker(name, mode, library, data, scratch, extra_env=None):
    """Run one case in a fresh worker and return its printed observation."""
    scratch.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ)
    environment.update(extra_env or {})
    environment.update(CONTRACT_CELL=CELL, TMPDIR=str(scratch))
    command = [sys.executable, '-X', 'utf8', str(Path(__file__).resolve()),
               '--worker', name, '--mode', mode, '--library', str(library),
               '--data', str(data), '--scratch', str(scratch)]
    result = subprocess.run(command, capture_output=True, timeout=TIMEOUT, env=environment)
    if result.returncode != 0:
        raise RuntimeError(result.stderr.decode('utf-8', 'replace'))
    observations = [json.loads(line) for line in result.stdout.decode().splitlines()
                    if line.startswith('{')]
    if not observations:
        raise RuntimeError('the worker printed no observation')
    return observations[-1]


def cross_open(case, subject, pin, data, root, *, environment_case):
    """Run the subject, then let the pin read the profile the subject wrote."""
    subject_out = run_worker(case, CASES[case][0], subject, data, root / 'subject')
    donor = root / 'donor'
    shutil.copytree(subject_out['profile'], donor)
    profile_key, reload_key = environment_case
    pin_out = run_worker(case, CASES[case][0], pin, data, root / 'pin',
                         {profile_key: str(donor), reload_key: '1'})
    return subject_out, pin_out


def handoff_cases(cell):
    """The handoff cases that run on CELL.

    The issue1114 cases compare tkrzw HashDBM saved bytes, so they run only on
    the tkrzw cell. The save-929 cases run on every supported cell.
    """
    return [case for case in CASES
            if cell == 'tkrzw' or not case.startswith('issue1114-')]


def require_pin(prefix, mode):
    library = prefix / 'lib' / PIN_LIBRARIES[mode]
    if not library.is_file():
        raise SystemExit(f'missing input: {library}')
    return library


def main(argv):
    parser = argparse.ArgumentParser(prog='reverse_cross_open.py', description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--cell', choices=['bdb', 'kc', 'tkrzw'], default='tkrzw')
    parser.add_argument('--pin-prefix', type=Path)
    parser.add_argument('--data', type=Path)
    parser.add_argument('--worker', help=argparse.SUPPRESS)
    parser.add_argument('--mode', choices=['pinyin', 'zhuyin'], help=argparse.SUPPRESS)
    parser.add_argument('--library', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('--scratch', type=Path, help=argparse.SUPPRESS)
    parser.add_argument('pinyin_so', nargs='?', type=Path)
    parser.add_argument('zhuyin_so', nargs='?', type=Path)
    args = parser.parse_args(argv)

    global CELL
    if args.worker:
        CELL = os.environ.get('CONTRACT_CELL', 'tkrzw')
        contract_fixtures.FACADE = str(args.library)
        kit = Kit(args.mode, str(args.library), args.data, str(args.scratch),
                  cell=CELL, check_tkrzw=check_tkrzw)
        result = CASES[args.worker][1](kit)
        result['logs'] = kit.logs
        result['profile'] = kit.user
        print(json.dumps(result, sort_keys=True, ensure_ascii=False), flush=True)
        os._exit(0)

    CELL = args.cell
    if args.cell == 'tkrzw' and args.data is None:
        parser.error('the tkrzw cell reads the package data: pass --data DIR (just fetch-contract-data)')
    data = args.data or Path('/nonexistent-contract-data')
    if args.pin_prefix is None:
        parser.error('the reverse cross-open check runs the pin: pass --pin-prefix PREFIX')
    if args.pinyin_so is None:
        parser.error('missing input: PINYIN_SO')
    if args.zhuyin_so is None:
        parser.error('missing input: --zhuyin-so')
    failures = 0
    shapes = {
        'issue1114': ('CONTRACT_1114_PROFILE', 'CONTRACT_1114_RELOAD'),
        'save-929': ('CONTRACT_929_PROFILE', 'CONTRACT_929_RELOAD'),
    }
    with tempfile.TemporaryDirectory(prefix='reverse-cross-open-') as root:
        for case in handoff_cases(CELL):
            mode = CASES[case][0]
            subject = args.zhuyin_so if mode == 'zhuyin' else args.pinyin_so
            if subject is None:
                parser.error(f'{case} needs --zhuyin-so' if mode == 'zhuyin' else f'{case} needs PINYIN_SO')
            pin = require_pin(args.pin_prefix, mode)
            shape = shapes['issue1114' if case.startswith('issue1114') else 'save-929']
            try:
                subject_out, pin_out = cross_open(case, subject, pin, data, Path(root) / case, environment_case=shape)
                ok = True
            except (AssertionError, RuntimeError, subprocess.TimeoutExpired) as error:
                failures += 1
                print(json.dumps(dict(case=case, cell=CELL, ok=False, reason=str(error)[:300])))
                continue
            print(json.dumps(dict(case=case, cell=CELL, ok=ok, direction='oxpinyin-to-pin',
                                  saved=subject_out.get('saved_bigram') or subject_out.get('rows'))))
    return 1 if failures else 0


if __name__ == '__main__':
    raise SystemExit(main(sys.argv[1:]))
