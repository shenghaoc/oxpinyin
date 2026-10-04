#!/usr/bin/env python3
"""Case-driven pin-against-subject contract differential (lane C).

Usage: contract-diff.py CELL PREFIX PINYIN_SO [--zhuyin-so PATH]
                        [--cases a,b,...] [--expect-parent]

Each case is a short function that drives one facade through the C ABI and
returns the observations: return values, out-params (an untouched out-param
keeps its sentinel), bytes consumed and the GLib log records. Every case runs
in a fresh worker process per side (pin, subject) with a fresh user dir, so a
crash is a result: an exit code or a signal. The two JSON documents must be
identical, byte for byte. A key that starts with "~" is recorded but left out
of the comparison (an indeterminate value at the pin).

The raw `fprintf(stderr)` text of the pin is not compared; stderr is only
counted. Cases that make the pin abort belong to the class (c) logging
differential, not to this one.

--expect-parent runs the same cases against a parent build: a case marked
`control` must still match, every other case must differ.

Missing inputs and an empty case list fail. Scratch lives under TMPDIR and is
removed even on failure.
"""
import argparse
import ctypes as C
import json
import os
from pathlib import Path
import resource
import shutil
import subprocess
import sys
import tempfile

P, U, I, S, B, Z = C.c_void_p, C.c_uint, C.c_int, C.c_char_p, C.c_bool, C.c_size_t
UNTOUCHED = 0xABCDEF  # out-param sentinel

CASES = {}


def case(name, mode='pinyin', control=False):
    def register(fn):
        CASES[name] = dict(fn=fn, mode=mode, control=control)
        return fn
    return register


class Kit:
    """One library, one fresh user dir; contexts and instances on demand."""

    def __init__(self, mode, so, data, scratch):
        self.mode, self.data, self.scratch = mode, data, scratch
        self.lib = C.CDLL(so, use_errno=True)
        self.glib = C.CDLL('libglib-2.0.so.0')
        self.glib.g_free.argtypes = [P]
        self.glib.g_free.restype = None
        self.logs = []
        cb_type = C.CFUNCTYPE(None, S, U, S, P)

        def handler(domain, level, message, _unused):
            self.logs.append([domain.decode() if domain else None, level])
        self._handler = cb_type(handler)
        self.glib.g_log_set_handler.argtypes = [S, U, cb_type, P]
        self.glib.g_log_set_handler.restype = U
        for domain in (b'libpinyin', None):
            self.glib.g_log_set_handler(domain, 0xFFFFFFFC, self._handler, None)
        self._ctx = None
        self._inst = None
        self.user = tempfile.mkdtemp(prefix='user-', dir=scratch)

    def fn(self, name, res, *args):
        f = getattr(self.lib, self.mode + '_' + name)
        f.restype, f.argtypes = res, list(args)
        return f

    def init(self, system=None, user=None):
        def raw(value):
            return value if isinstance(value, bytes) else value.encode()
        return self.fn('init', P, S, S)(raw(system or self.data), raw(user or self.user))

    @property
    def ctx(self):
        if self._ctx is None:
            self._ctx = self.init()
            assert self._ctx, 'init failed'
        return self._ctx

    def alloc(self):
        inst = self.fn('alloc_instance', P, P)(self.ctx)
        assert inst
        return inst

    @property
    def inst(self):
        if self._inst is None:
            self._inst = self.alloc()
        return self._inst

    def text(self, ptr):
        """Reads and frees a caller-owned string; None for NULL."""
        if not ptr:
            return None
        value = C.string_at(ptr).decode('utf-8', 'replace')
        self.glib.g_free(ptr)
        return value


def worker(mode, so, data, name):
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    scratch = os.environ['TMPDIR']
    kit = Kit(mode, so, data, scratch)
    result = CASES[name]['fn'](kit)
    result['logs'] = kit.logs
    print(json.dumps(result, sort_keys=True, ensure_ascii=False), flush=True)
    os._exit(0)  # no atexit: a case never shares teardown with the next


def observed(result):
    return {k: v for k, v in result.items() if not k.startswith('~')}


# --------------------------------------------------------------------------
# Cases. A control case holds on the parent as well; every other case is the
# regression guard of the fix named in its comment.
# --------------------------------------------------------------------------

def consumed(kit, parse_name, text, fresh=True):
    inst = kit.alloc() if fresh else kit.inst
    return kit.fn(parse_name, Z, P, S)(inst, text)


def key_out(kit, text):
    key = C.c_ushort(UNTOUCHED & 0xFFFF)
    ret = kit.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_ushort))(kit.inst, text, C.byref(key))
    return [ret, key.value]


# lane C, #532: the option word `pinyin_init` seeds is USE_TONE.
@case('default-option-parse')
def _(k):
    return {t.decode(): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'su3cl3', b'ni2hao', b'n', b'zzz', b'nihao', b'ni3')}


@case('default-option-single-key')
def _(k):
    return {t.decode(): key_out(k, t) for t in (b'n', b'ni3', b'ni', b'a1', b'zhang', b'zzz')}


@case('explicit-use-tone-parse', control=True)
def _(k):
    k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    return {t.decode(): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'su3cl3', b'ni2hao', b'n', b'zzz', b'nihao')}


@case('explicit-incomplete-parse', control=True)
def _(k):
    k.fn('set_options', B, P, U)(k.ctx, 1 << 3)
    return {t.decode(): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'su3cl3', b'ni2hao', b'n', b'zzz', b'nihao')}


# --------------------------------------------------------------------------

def run_worker(mode, so, data, name, scratch):
    env = dict(os.environ, TMPDIR=str(scratch))
    proc = subprocess.run([sys.executable, __file__, '--worker', mode, str(so), str(data), name],
                          capture_output=True, text=True, env=env)
    lines = [json.loads(line) for line in proc.stdout.splitlines() if line.startswith('{')]
    return dict(exit=proc.returncode, result=lines[-1] if lines else None,
                stderr_lines=len(proc.stderr.splitlines()))


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('cell', choices=['bdb', 'kc', 'tkrzw'])
    parser.add_argument('prefix', type=Path)
    parser.add_argument('pinyin_so', type=Path)
    parser.add_argument('--zhuyin-so', type=Path)
    parser.add_argument('--cases', default='')
    parser.add_argument('--expect-parent', action='store_true')
    args = parser.parse_args()
    lib = args.prefix / 'lib'
    data = lib / 'libpinyin/data'
    inputs = [args.prefix / 'oracle-pin.txt', lib / 'libpinyin.so', args.pinyin_so, data / 'bigram.db']
    for path in inputs:
        assert path.is_file(), 'missing input: %s' % path
    manifest = (args.prefix / 'oracle-pin.txt').read_text()
    assert 'libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' in manifest
    assert '+dbm-' + args.cell + '\n' in manifest
    # The whole set only when --cases is omitted: an explicit list that
    # names nothing (`--cases ,`) is an error, not a request for everything.
    names = [n for n in args.cases.split(',') if n] if args.cases else sorted(CASES)
    assert names, 'no cases selected'
    for n in names:
        assert n in CASES, 'unknown case: %s' % n
    failures = 0
    with tempfile.TemporaryDirectory(prefix='contract-diff-') as scratch:
        for n in names:
            spec = CASES[n]
            if spec['mode'] == 'zhuyin':
                assert args.zhuyin_so and args.zhuyin_so.is_file() and (lib / 'libzhuyin.so').is_file(), \
                    'zhuyin case %s needs --zhuyin-so and the pin libzhuyin' % n
                pin_so, subject_so = lib / 'libzhuyin.so', args.zhuyin_so
            else:
                pin_so, subject_so = lib / 'libpinyin.so', args.pinyin_so
            pin = run_worker(spec['mode'], pin_so.resolve(), data, n, scratch)
            subject = run_worker(spec['mode'], subject_so.resolve(), data, n, scratch)
            # A side that died or printed nothing observed nothing: two
            # identical failures must not read as a match (nor satisfy
            # --expect-parent as a difference).
            same = all(r['exit'] == 0 and r['result'] is not None for r in (pin, subject)) and \
                observed(pin['result']) == observed(subject['result'])
            expected = same if (not args.expect_parent or spec['control']) else not same
            verdict = 'MATCH' if same else 'DIFFER'
            print(json.dumps(dict(cell=args.cell, case=n, verdict=verdict, expected_ok=expected,
                                  exit=[pin['exit'], subject['exit']],
                                  stderr_lines=[pin['stderr_lines'], subject['stderr_lines']]),
                             sort_keys=True), flush=True)
            if not expected or (not same and not args.expect_parent):
                failures += 1
                if not same:
                    for side, r in (('pin', pin), ('subject', subject)):
                        print('  %s exit=%s %s' % (side, r['exit'], json.dumps(
                            r['result'] and observed(r['result']), sort_keys=True, ensure_ascii=False)))
    return 1 if failures else 0


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--worker':
        worker(*sys.argv[2:])
    else:
        sys.exit(main())
