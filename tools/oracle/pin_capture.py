#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Capture the pin's observations of the apostrophe inputs into a staging directory.

    pin_capture.py capture RUN --prefix PREFIX --data DATA [--staging DIR]

`capture` runs in the capture image (tools/oracle/capture-pin.sh). It first records what it
runs, capture.json: the pin commit of the prefix's build record, the SHA-256 of the
libpinyin.so it loads and the identity of the system data. It refuses the capture unless all
three are this repository's pins: `libpinyin_commit` and this platform's
`libpinyin_sha256_tkrzw` in tools/oracle/oracle-pin.txt, and `content_sha256` in
tools/oracle/libpinyin-data.json. Only then does it make the C API calls of each observation
below, each in a fresh process with a fresh user directory, and write DIR/RUN/capture.json
and DIR/RUN/observations.json (DIR defaults to target/pin-observations). An observation that
ends in the pin's abort keeps the results of the calls before it.

These are proposed observations. Nothing here writes outside the staging directory or reads
back into a test: a person reviews two runs and writes the values into the Rust tests.
docs/runbooks/pin-captures.md is the procedure. Standard library only.
"""
import argparse
import ctypes as C
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/bisection'))
sys.path.insert(0, str(ROOT))
import contract_reference as reference  # noqa: E402
from tools.oracle import image  # noqa: E402

SCHEMA = 'pin-observations-v1'
FAMILY = 'apostrophe'
BACKEND = 'tkrzw'
DEFAULT_STAGING = ROOT / 'target/pin-observations'
PLATFORMS = {'x86_64': 'linux/amd64', 'aarch64': 'linux/arm64'}
UNTOUCHED_U16 = 0xCDEF  # contract_abi.UNTOUCHED & 0xffff
WORKER_TIMEOUT = 120  # seconds for one observation's worker, as the Rust death tests allow a child


class CaptureError(Exception):
    """A capture cannot proceed; the message says why."""


# ── the observations: plain C API calls ──────────────────────────────────

def parse(facade, text):
    """Parse TEXT as full pinyin; answer the parsed length."""
    facade.call('pinyin_parse_more_full_pinyins', text)
    return facade.call('pinyin_get_parsed_input_length')


def key_abort(facade, text):
    """F-E-14: a key query at offset 0 of an apostrophe-only parse."""
    parse(facade, text)
    facade.call('pinyin_get_pinyin_key', 0)


def key_walk(facade, text):
    """The key, key rest, positions and pinyin string at every offset up to the parsed length.

    The pin dereferences the key rest and the key without a null check, so positions and string
    are asked only of a non-null pointer."""
    for offset in range(parse(facade, text) + 1):
        key = facade.call('pinyin_get_pinyin_key', offset)
        rest = facade.call('pinyin_get_pinyin_key_rest', offset)
        if rest[1]:
            facade.call('pinyin_get_pinyin_key_rest_positions')
        if key[1] is not None:
            facade.call('pinyin_get_pinyin_string')


# Observation ID -> (function, input).
OBSERVATIONS = {
    "key-abort/'": (key_abort, "'"),
    "key-abort/''": (key_abort, "''"),
    "key-abort/'''": (key_abort, "'''"),
    "key-walk/ni'": (key_walk, "ni'"),
    "key-walk/ni'!": (key_walk, "ni'!"),
    "key-walk/ni'i": (key_walk, "ni'i"),
}


# ── the calls on the pin ──────────────────────────────────────────────────

P, U16, B, S, Z = C.c_void_p, C.c_uint16, C.c_bool, C.c_char_p, C.c_size_t


class Pin:
    """One context and instance on the pin. Each call prints its name before it runs and its
    result after, so the parent sees which call an abort ended."""

    def __init__(self, library, data, user):
        self.lib = C.CDLL(library)
        self.lib.g_free.argtypes, self.lib.g_free.restype = [P], None
        self.key, self.rest = P(), P()
        context = self.fn('pinyin_init', P, S, S)(os.fsencode(data), os.fsencode(user))
        if not context:
            raise CaptureError('pinyin_init failed')
        self.instance = self.fn('pinyin_alloc_instance', P, P)(context)
        if not self.instance:
            raise CaptureError('pinyin_alloc_instance failed')

    def fn(self, name, result, *args):
        function = getattr(self.lib, name)
        function.restype, function.argtypes = result, list(args)
        return function

    def call(self, name, *args):
        print(json.dumps({'call': [name, list(args)]}), flush=True)
        result = self.run(name, *args)
        print(json.dumps({'result': result}), flush=True)
        return result

    def run(self, name, *args):
        if name == 'pinyin_parse_more_full_pinyins':
            return int(self.fn(name, Z, P, S)(self.instance, args[0].encode()))
        if name == 'pinyin_get_parsed_input_length':
            return int(self.fn(name, Z, P)(self.instance))
        if name == 'pinyin_get_pinyin_key':
            self.key = P()
            ok = bool(self.fn(name, B, P, Z, C.POINTER(P))(self.instance, args[0], C.byref(self.key)))
            return [ok, C.cast(self.key, C.POINTER(U16)).contents.value if self.key else None]
        if name == 'pinyin_get_pinyin_key_rest':
            self.rest = P()
            ok = bool(self.fn(name, B, P, Z, C.POINTER(P))(self.instance, args[0], C.byref(self.rest)))
            return [ok, bool(self.rest)]
        if name == 'pinyin_get_pinyin_key_rest_positions':
            begin, end = U16(UNTOUCHED_U16), U16(UNTOUCHED_U16)
            ok = bool(self.fn(name, B, P, P, C.POINTER(U16), C.POINTER(U16))(
                self.instance, self.rest, C.byref(begin), C.byref(end)))
            return [ok, begin.value, end.value]
        if name == 'pinyin_get_pinyin_string':
            text = P()
            ok = bool(self.fn(name, B, P, P, C.POINTER(P))(self.instance, self.key, C.byref(text)))
            value = None
            if text.value:
                value = C.string_at(text.value).decode()
                self.lib.g_free(text.value)
            return [ok, value]
        raise CaptureError(f'no call {name}')


def worker(name, library, data):
    function, text = OBSERVATIONS[name]
    with tempfile.TemporaryDirectory(prefix='pin-observation-') as user:
        function(Pin(library, data, user), text)
    return 0


def observe(name, library, data):
    """Run observation NAME in a fresh worker process; answer its calls, results and end."""
    with tempfile.TemporaryDirectory(prefix='pin-capture-') as scratch:
        try:
            result = subprocess.run([sys.executable, __file__, 'worker', name, str(library), str(data)],
                                    capture_output=True, env=dict(os.environ, TMPDIR=scratch),
                                    timeout=WORKER_TIMEOUT)
        except subprocess.TimeoutExpired as error:
            raise CaptureError(f'{name}: the worker did not end within {WORKER_TIMEOUT} s') from error
    calls, results = [], []
    for line in result.stdout.decode().splitlines():
        record = json.loads(line)
        if 'call' in record:
            calls.append(record['call'])
        else:
            results.append(record['result'])
    if result.returncode > 0:
        raise CaptureError(f'{name}: the worker failed ({result.returncode}): '
                           + result.stderr.decode(errors='replace')[-600:])
    end = {'exit': 0} if result.returncode == 0 else {'signal': -result.returncode}
    _, text = OBSERVATIONS[name]
    return {'input': text, 'calls': calls, 'results': results, 'end': end}


# ── capture.json and its check ───────────────────────────────────────────

def identify(prefix, data):
    """capture.json for the pin at PREFIX on the system data DATA."""
    record = image.fields(Path(prefix) / 'oracle-pin.txt')
    return {'schema': SCHEMA, 'family': FAMILY, 'backend': BACKEND,
            'platform': PLATFORMS.get(platform.machine(), platform.machine()),
            'image': os.environ.get('OXPINYIN_CAPTURE_IMAGE'),
            'pin_commit': record.get('libpinyin_commit'),
            'library_sha256': image.digest_file(Path(prefix) / 'lib/libpinyin.so'),
            'data': {'kind': reference.PACKAGE, 'digest': reference.data_identity(data)}}


def check_capture(capture, root=ROOT):
    """Refuse a capture of another pin, another library or other data."""
    pins = Path(root) / 'tools/oracle/oracle-pin.txt'
    data = json.loads((Path(root) / 'tools/oracle/libpinyin-data.json').read_text())
    library = image.platform_field(pins, capture.get('platform'), f'libpinyin_sha256_{BACKEND}')
    for label, got, wanted in (
        ('pin commit', capture.get('pin_commit'), image.fields(pins).get('libpinyin_commit')),
        ('library SHA-256', capture.get('library_sha256'), library),
        ('data identity', capture.get('data'), {'kind': reference.PACKAGE, 'digest': data['content_sha256']}),
    ):
        if not wanted or got != wanted:
            raise CaptureError(f'capture.json: the {label} is {got!r}, the repository pins {wanted!r}')


def capture(run, identity, staging, take, root=ROOT):
    """Check IDENTITY, take each observation with TAKE, and stage both under STAGING/RUN."""
    separators = {'/', os.sep, os.altsep} - {None}
    if run in ('', '.', '..') or any(separator in run for separator in separators):
        raise CaptureError(f'the run name {run!r} is not a single path component')
    check_capture(identity, root)
    directory = Path(staging) / run
    if directory.exists():
        raise CaptureError(f'{directory} exists: stage each run once')
    observations = {name: take(name) for name in OBSERVATIONS}
    directory.mkdir(parents=True)
    for name, value in (('capture.json', identity), ('observations.json', observations)):
        (directory / name).write_text(json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False) + '\n')
    print(f'{directory}: {len(observations)} proposed observations', flush=True)
    return 0


def main(argv):
    if argv[:1] == ['worker']:
        return worker(*argv[1:])
    parser = argparse.ArgumentParser(prog='pin_capture.py', description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest='command', required=True)
    run = commands.add_parser('capture', help='capture one run on the pin, in the capture image')
    run.add_argument('run', help='the run name, such as 1 or 2')
    run.add_argument('--prefix', required=True, help="the pin's tkrzw install prefix")
    run.add_argument('--data', required=True, help='the pinned libpinyin-data directory')
    run.add_argument('--staging', default=str(DEFAULT_STAGING), help='the staging directory')
    args = parser.parse_args(argv)
    try:
        reference.require_capture_environment()
        library = Path(args.prefix) / 'lib/libpinyin.so'
        return capture(args.run, identify(args.prefix, args.data), args.staging,
                       lambda name: observe(name, library, args.data))
    except (CaptureError, reference.ReferenceError, OSError) as error:
        print(f'FAIL: {error}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
