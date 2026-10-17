#!/usr/bin/env python3
"""Case-driven pin-against-subject contract differential (lane C).

Usage: contract-diff.py CELL PREFIX PINYIN_SO [--zhuyin-so PATH]
                        [--cases a,b,...] [--expect-parent]
       contract-diff.py bdb PREFIX PREFIX/lib/libpinyin.so
                        --regenerate-sentence-ties FIXTURE

Each case is a short function that drives one facade through the C ABI and
returns the observations: return values, out-params (an untouched out-param
keeps its sentinel), bytes consumed and the GLib log records. Every case runs
in a fresh worker process per side (pin, subject) with a fresh user dir, so a
crash is a result: an exit code or a signal. The two JSON documents must be
identical, byte for byte. A key that starts with "~" is recorded but left out
of the comparison (an indeterminate value at the pin).

The raw `fprintf(stderr)` text of the pin is compared only by the cases
that ask for it (`stderr=True`, below); every other case counts the lines.
Cases that make the pin abort belong to the class (c) logging differential,
not to this one.

A class (c) case (`abort=`) holds when the pin dies of SIGABRT and the subject
answers the declared value with exactly one warning in its own library's domain
(`libpinyin`, or `libzhuyin` for the zhuyin facade).

A class (b) case (`crash=`) holds when the pin dies of SIGSEGV after its raw
stderr line (an empty chunk read through a NULL base) and the subject, which
does not crash, wrote the same stderr bytes and answers the declared fields.
The pin's bytes are compared up to the crash; the invalid read is never
reproduced.

--expect-parent runs the same cases against a parent build: a case marked
`control` must still match, every other case must differ.

Missing inputs and an empty case list fail. Scratch lives under TMPDIR and is
removed even on failure.
"""
import argparse
import copy
import ctypes as C
import hashlib
import json
import os
from pathlib import Path
import re
import resource
import shutil
import signal
import subprocess
import sys
import tempfile
import time

P, U, I, S, B, Z = C.c_void_p, C.c_uint, C.c_int, C.c_char_p, C.c_bool, C.c_size_t
UNTOUCHED = 0xABCDEF  # out-param sentinel
# The DBM cell the run is for (set by `main`, inherited by every worker):
# fixtures that craft a user table write it in that backend's own format.
CELL = os.environ.get('CONTRACT_DIFF_CELL', 'bdb')

CASES = {}


NO_ABORT = object()

# Each facade logs under its own library's domain.
WARNING_DOMAIN = {'pinyin': 'libpinyin', 'zhuyin': 'libzhuyin'}


def case(name, mode='pinyin', control=False, abort=NO_ABORT, stderr=False, crash=None, userdir=False,
         cells=('bdb', 'kc', 'tkrzw'), either=False, host_scanf=(), stderr_varies=False):
    """Registers a case. `stderr=True` also compares what the library wrote
    to stderr (raw `fprintf`s of the pin; GLib logs are the `logs` field),
    with the scratch directory names normalised. `abort=<value>` marks a
    class (c) site: the pin
    must die of SIGABRT, and the subject must return `<value>` in its `ret`
    field with exactly one GLib warning in the facade's domain (`libpinyin`,
    or `libzhuyin` for the zhuyin facade; level 16). `crash=<fields>` marks a
    class (b) site: the pin must die of SIGSEGV (after the stderr it wrote
    up to then), and the subject must exit normally, write the same stderr
    bytes and answer `<fields>` (a dict of result fields). `userdir=True`
    also holds the user directory a side leaves behind (file names, the
    `user.conf` text, the size of each library chunk) to the pin's, for an
    abort that comes after the pin has written some of its profile. `cells`
    names the DBM cells whose storage format the case's fixture reproduces;
    a case skipped on the others prints `SKIP` instead of a verdict.
    `either=True` (with both `abort=` and `crash=`) is for a fixture whose
    pin site depends on the file system: the pin's own signal picks the
    matcher, SIGABRT the class (c) one and anything else the class (b) one.
    `stderr_varies=True` (with `crash=`) is for a site where the pin's raw
    stderr is read from freed memory and so changes from run to run: the pin
    must still die of SIGSEGV and the subject answer `<fields>`, but the
    stderr bytes are not compared.
    `host_scanf=<spellings>` names `%f` inputs the case expects the host
    libc's scanner to reject, as the oracle image's glibc does. The pin
    reads them with the host libc, so on a host whose scanner accepts one
    the pin loads what the subject refuses. Only that difference (the pin
    created its context; the subject refused `table.conf`) prints
    `HOST-DEPENDENT` instead of `DIFFER` and is not a failure; any other
    difference stays a DIFFER. A MATCH is reported as such on any host."""
    def register(fn):
        assert not either or (abort is not NO_ABORT and crash is not None), name
        CASES[name] = dict(fn=fn, mode=mode, control=control, abort=abort, stderr=stderr or crash is not None,
                           crash=crash, userdir=userdir, cells=cells, either=either, host_scanf=tuple(host_scanf),
                           stderr_varies=stderr_varies)
        return fn
    return register


def host_scanf_accepts(spelling):
    """Whether this host's libc converts `spelling` with `%f`, as the pin's
    `fscanf` does. The pin's workers run on this libc."""
    scanned = C.c_float()
    return C.CDLL(None).sscanf(spelling.encode(), b'%f', C.byref(scanned)) == 1


def pin_loaded_subject_refused_table_conf(pin, subject):
    """The one difference a host-libc `%f` explains: the pin created its
    context and the subject refused `table.conf` with its single message.
    The pin's stderr must be exactly its fresh-profile line,
    `open <user dir>/user.conf failed.` (`UserTableInfo::load` writes it
    with no trailing newline when the empty user directory has no
    `user.conf`: `table_info.cpp:330-333` at 074a2219), so any other pin
    diagnostic, and an empty one, stays a DIFFER."""
    return (pin['exit'] == 0 and subject['exit'] == 0
            and (pin['result'] or {}).get('ctx') is True
            and (subject['result'] or {}).get('ctx') is False
            and re.fullmatch(r'open \S*/user\.conf failed\.', pin['stderr']) is not None
            and re.fullmatch(r'load \S*table\.conf failed!\n', subject['stderr']) is not None)


class Kit:
    """One library, one fresh user dir; contexts and instances on demand."""

    def __init__(self, mode, so, data, scratch):
        self.mode, self.data, self.scratch = mode, data, scratch
        self.lib = C.CDLL(so, use_errno=True)
        self.glib = C.CDLL('libglib-2.0.so.0')
        self.glib.g_free.argtypes = [P]
        self.glib.g_free.restype = None
        self.logs = []
        self.log_messages = []
        cb_type = C.CFUNCTYPE(None, S, U, S, P)

        def handler(domain, level, message, _unused):
            self.logs.append([domain.decode() if domain else None, level])
            self.log_messages.append(message.decode() if message else None)
        self._handler = cb_type(handler)
        self.glib.g_log_set_handler.argtypes = [S, U, cb_type, P]
        self.glib.g_log_set_handler.restype = U
        for domain in (b'libpinyin', b'libzhuyin', None):
            self.glib.g_log_set_handler(domain, 0xFFFFFFFC, self._handler, None)
        self._ctx = None
        self._inst = None
        self.user = tempfile.mkdtemp(prefix='user-', dir=scratch)

    def fn(self, name, res, *args):
        f = getattr(self.lib, self.mode + '_' + name)
        f.restype, f.argtypes = res, list(args)
        return f

    def init(self, system=None, user=None, *, literal=False):
        """literal=True passes None/empty bytes unchanged instead of defaults."""
        def raw(value):
            return value if value is None or isinstance(value, bytes) else value.encode()
        if not literal:
            system, user = system or self.data, user or self.user
        return self.fn('init', P, S, S)(raw(system), raw(user))

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
    if name.startswith('axis-c-'):
        # Fedora's pipe core_pattern can invoke systemd-coredump even when
        # RLIMIT_CORE is zero. This guard prevents routing these deliberate
        # pin faults into a host capture service; descendants inherit it.
        libc = C.CDLL(None, use_errno=True)
        libc.prctl.argtypes, libc.prctl.restype = [I, C.c_ulong, C.c_ulong, C.c_ulong, C.c_ulong], I
        assert libc.prctl(4, 0, 0, 0, 0) == 0, ('PR_SET_DUMPABLE failed', C.get_errno())
    scratch = os.environ['TMPDIR']
    kit = Kit(mode, so, data, scratch)
    result = CASES[name]['fn'](kit)
    result['logs'] = kit.logs
    if name.startswith('axis-c-'):
        result['log_messages'] = kit.log_messages
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


# lane C, #587: valid pinyin followed by invalid UTF-8 keeps the prefix.
@case('bytes-parse-more-full')
def _(k):
    return {repr(t): consumed(k, 'parse_more_full_pinyins', t)
            for t in (b'ni\xffhao', b'nihao\xff', b'\xffnihao', b'ni\xe4\xbd', b'ni\xc0\x80hao')}


@case('bytes-parse-more-double')
def _(k):
    return {repr(t): consumed(k, 'parse_more_double_pinyins', t)
            for t in (b'ni\xffhk', b'nihk\xff', b'\xffnihk')}


@case('bytes-parse-more-chewing')
def _(k):
    return {repr(t): consumed(k, 'parse_more_chewings', t)
            for t in (b'su\xff3', b'su3\xff', b'\xffsu3')}


@case('bytes-add-phrase-pinyin')
def _(k):
    rows = []
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    ret = [add(it, '你好'.encode(), b"ni'ha\xffo", 1), add(it, '世界'.encode(), b"shi'jie\xff", 1)]
    k.fn('end_add_phrases', None, P)(it)
    out = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
    has = k.fn('iterator_has_next_phrase', B, P)
    nxt = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))
    while has(out):
        ph, py, n = P(), P(), I()
        nxt(out, C.byref(ph), C.byref(py), C.byref(n))
        rows.append([k.text(ph.value), k.text(py.value), n.value])
    return dict(added=ret, rows=rows)


@case('bytes-whole-argument', control=True)
def _(k):
    # Entries that hand the whole argument to the pin keep refusing it.
    out = {'parse_full_pinyin': key_out(k, b'ni\xff')}
    arr = k.glib.g_array_new
    arr.restype, arr.argtypes = P, [I, I, U]
    tokens = arr(0, 0, 4)
    out['lookup_tokens'] = k.fn('lookup_tokens', B, P, S, P)(k.inst, '你'.encode() + b'\xff', tokens)
    parse_n = consumed(k, 'parse_more_full_pinyins', b'nihaoshijie', fresh=False)
    out['remember'] = k.fn('remember_user_input', B, P, S, I)(k.inst, '你好世界'.encode() + b'\xff', 1)
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    out['add_bad_phrase'] = k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你好'.encode() + b'\xff', b"ni'hao", 1)
    k.fn('end_add_phrases', None, P)(it)
    out['parsed'] = parse_n
    return out


@case('bytes-zhuyin-parse-more', mode='zhuyin')
def _(k):
    out = {}
    for name, texts in (('parse_more_full_pinyins', (b'ni3\xffhao3', b'ni3hao3\xff', b'\xffni3')),
                        ('parse_more_chewings', (b'su\xff3', b'su3\xff', b'\xffsu3'))):
        for t in texts:
            out[name + ' ' + repr(t)] = consumed(k, name, t)
    return out


# The zhuyin direct parser splits on spaces and apostrophes, so a reading
# with an invalid byte in its last token is refused whole: unchanged.
@case('bytes-zhuyin-add-phrase', mode='zhuyin', control=True)
def _(k):
    out = {}
    for label, reading in (('whole', 'ㄋㄧˇ ㄏㄠˇ'.encode()), ('tail-invalid', 'ㄋㄧˇ ㄏㄠˇ'.encode() + b'\xff'),
                           ('mid-invalid', 'ㄋㄧˇ'.encode() + b'\xff' + 'ㄏㄠˇ'.encode())):
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
        out[label] = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
        k.fn('end_add_phrases', None, P)(it)
    return out


def sentence_out(k, inst, index):
    out = P(UNTOUCHED)
    ret = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(inst, index, C.byref(out))
    if out.value == UNTOUCHED:
        return [ret, 'untouched']
    return [ret, k.text(out.value)]


# lane C, #542 rows 2, 6, 7, 20: out-params the pin leaves alone, or writes.
@case('empty-parse-sentence')
def _(k):
    out = {}
    for t in (b'', b'!'):
        inst = k.alloc()
        n = k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
        guessed = k.fn('guess_sentence', B, P)(inst)
        out[repr(t)] = [n, guessed, sentence_out(k, inst, 0)]
    inst = k.alloc()
    out['fresh'] = [k.fn('guess_sentence', B, P)(inst), sentence_out(k, inst, 0)]
    return out


@case('character-offset-out')
def _(k):
    fn = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))
    out = {}
    full, double = 'parse_more_full_pinyins', 'parse_more_double_pinyins'
    for label, parse, text, phrase, offset in (
            ('recursion-fails-1', full, b'nihao', '啊', 3), ('recursion-fails-2', full, b'nihao', '你', 5),
            ('recursion-fails-3', full, b'nihao', '好', 5), ('hit', full, b'nihao', '你好', 5),
            ('no-token', full, b'nihao', 'x', 2), ('empty-phrase', full, b'nihao', '', 2),
            ('no-parse', full, b'', '你好', 0), ('keyless', full, b"'", '你好', 0),
            ('double-fails', double, b'nihk', '啊', 2), ('double-hit', double, b'nihk', '你好', 4)):
        inst = k.alloc()
        k.fn(parse, Z, P, S)(inst, text)
        length = Z(UNTOUCHED)
        ret = fn(inst, phrase.encode(), offset, C.byref(length))
        out[label] = [ret, 'untouched' if length.value == UNTOUCHED else length.value]
    return out


@case('token-get-phrase-out')
def _(k):
    out = {}
    for token in (0xFFFFFFFF, 0x0DEADBEE, 0x01000000):
        length, text = U(UNTOUCHED), P(UNTOUCHED)
        ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
            k.inst, token, C.byref(length), C.byref(text))
        out[hex(token)] = [ret, 'untouched' if length.value == UNTOUCHED else length.value,
                           'untouched' if text.value == UNTOUCHED else k.text(text.value)]
    return out


class Arr(C.Structure):
    _fields_ = [('data', P), ('len', U)]


def tokens_of(k, phrase):
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    arr = new(0, 0, 4)
    k.fn('lookup_tokens', B, P, S, P)(k.inst, phrase.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    return C.cast(view.data, C.POINTER(U))[:view.len]


@case('nth-pronunciation-range')
def _(k):
    token = tokens_of(k, '你好')[0]
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    count = U(UNTOUCHED)
    k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
    out = {'n': count.value}
    for nth in (0, count.value - 1, count.value, count.value + 1, 0xFFFFFFFF):
        keys = new(0, 0, 2)
        ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
        view = C.cast(keys, C.POINTER(Arr)).contents
        content = C.string_at(view.data, view.len * 2).hex() if view.len else ''
        in_range = nth < count.value
        out['nth %d' % nth] = [ret, view.len] + ([content] if in_range else [])
        # Past the last reading the pin returns whatever its stack held; the
        # ruled answer is zeroed keys (class (b) for the content).
        out['~content nth %d' % nth] = content
    # An unknown token still empties the caller's array first.
    append = k.glib.g_array_append_vals
    append.restype, append.argtypes = P, [P, P, U]
    keys = new(0, 0, 2)
    seed = C.c_ushort(0x1234)
    append(keys, C.byref(seed), 1)
    ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, 0xFFFFFFFF, 0, keys)
    out['unknown token'] = [ret, C.cast(keys, C.POINTER(Arr)).contents.len]
    return out


# lane C, #542 rows 14 and 19: the pin's function-static slots and u16 length.
@case('static-key-slots')
def _(k):
    first, second = k.alloc(), k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(first, b'ni')
    k.fn('parse_more_full_pinyins', Z, P, S)(second, b'hao')
    key = k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))
    rest = k.fn('get_pinyin_key_rest', B, P, Z, C.POINTER(P))
    a, b, ra, rb = P(), P(), P(), P()
    out = {'ret': [key(first, 0, C.byref(a)), key(second, 0, C.byref(b)),
                   rest(first, 0, C.byref(ra)), rest(second, 0, C.byref(rb))]}
    out['key same pointer'] = a.value == b.value
    out['rest same pointer'] = ra.value == rb.value
    out['first key now'] = C.string_at(a.value, 2).hex()
    out['first rest now'] = C.string_at(ra.value, 4).hex()
    k.fn('free_instance', None, P)(first)
    out['key after free'] = C.string_at(a.value, 2).hex()
    out['rest after free'] = C.string_at(ra.value, 4).hex()
    return out


class Rest(C.Structure):
    _fields_ = [('begin', C.c_ushort), ('end', C.c_ushort)]


@case('key-rest-length')
def _(k):
    fn = k.fn('get_pinyin_key_rest_length', B, P, C.POINTER(Rest), C.POINTER(C.c_ushort))
    out = {}
    for begin, end in ((0, 2), (5, 2), (2, 2), (0, 65535), (65535, 0), (1, 65535)):
        length = C.c_ushort(0xBEEF)
        rest = Rest(begin, end)
        out['%d..%d' % (begin, end)] = [fn(k.inst, C.byref(rest), C.byref(length)), length.value]
    return out


# lane C, #542 rows 1, 3, 10, 21: return values of sentence and unload.
@case('sentence-before-guess')
def _(k):
    out = {}
    inst = k.alloc()
    out['fresh'] = [sentence_out(k, inst, 0), sentence_out(k, inst, 255)]
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    out['parsed'] = [sentence_out(k, inst, 0), sentence_out(k, inst, 255)]
    k.fn('guess_sentence', B, P)(inst)
    out['guessed'] = sentence_out(k, inst, 0)
    k.fn('reset', B, P)(inst)
    out['reset'] = sentence_out(k, inst, 0)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    out['reparsed'] = sentence_out(k, inst, 0)
    # A parse alone keeps the rows of the last guess.
    k.fn('guess_sentence', B, P)(inst)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'xian')
    out['guessed then reparsed'] = sentence_out(k, inst, 0)
    return out


@case('sentence-before-guess-schemes')
def _(k):
    out = {}
    inst = k.alloc()
    k.fn('parse_more_double_pinyins', Z, P, S)(inst, b'nihk')
    out['double'] = sentence_out(k, inst, 0)
    inst = k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    out['chewing'] = sentence_out(k, inst, 0)
    return out


@case('guess-sentence-keyless')
def _(k):
    out = {}
    for t in (b"'", b"''", b'', b'!', b"nihao'"):
        inst = k.alloc()
        n = k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
        ret = k.fn('guess_sentence', B, P)(inst)
        out[repr(t)] = [n, ret, sentence_out(k, inst, 0)]
    # A keyless guess answers false before it clears anything: the rows of
    # the last guess stay.
    for t in (b"'", b'!'):
        inst = k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        first = k.fn('guess_sentence', B, P)(inst)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
        out['stale after ' + repr(t)] = [first, k.fn('guess_sentence', B, P)(inst), sentence_out(k, inst, 0)]
        inst = k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
        out['with-prefix ' + repr(t)] = [k.fn('guess_sentence_with_prefix', B, P, S)(inst, '你'.encode()),
                                         sentence_out(k, inst, 0)]
    return out


@case('guess-candidates-keyless')
def _(k):
    # `0 == matrix.size()`: the candidates are freed and the answer is
    # false, whatever sentence rows an earlier guess left.
    out = {}
    for t in (b"'", b"'''", b'!', b'', b"ni'"):
        for stale in (False, True):
            inst = k.alloc()
            if stale:
                k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
                k.fn('guess_sentence', B, P)(inst)
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
            ret = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1E)
            count = U(UNTOUCHED)
            k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            out['%r stale=%s' % (t, stale)] = [ret, count.value > 0]
    return out


@case('unload-phrase-library-repeat')
def _(k):
    unload = k.fn('unload_phrase_library', B, P, U)
    return {'unload': [unload(k.ctx, i) for i in (2, 2, 2, 1, 3, 15, 2)]}


@case('sentence-with-prefix-bytes')
def _(k):
    out = {}
    for label, prefix in (('invalid', b'\xff\xfe'), ('valid', '你'.encode()), ('empty', b''),
                          ('tail-invalid', '你'.encode() + b'\xff')):
        inst = k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        ret = k.fn('guess_sentence_with_prefix', B, P, S)(inst, prefix)
        out[label] = [ret, sentence_out(k, inst, 0), sentence_out(k, inst, 1)]
    return out


# lane C, #542 rows 8 and 9: auxiliary text renders from the shared parse.
def aux_matrix(k, parse_name, text, cursors):
    out = {}
    for fname in ('full_pinyin', 'double_pinyin', 'chewing'):
        for cursor in cursors:
            inst = k.alloc()
            k.fn(parse_name, Z, P, S)(inst, text)
            aux = P(UNTOUCHED)
            ret = k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, cursor, C.byref(aux))
            out['%s@%d' % (fname, cursor)] = [ret, 'untouched' if aux.value == UNTOUCHED else k.text(aux.value)]
    return out


@case('aux-text-full-parse')
def _(k):
    # Mid-key cursors stay at one or two bytes into a key: the pin aborts on
    # a longer mid-key cursor of the double-pinyin renderer.
    return aux_matrix(k, 'parse_more_full_pinyins', b'nihao', (0, 1, 2, 3, 4, 5, 9))


@case('aux-text-full-parse-apostrophe')
def _(k):
    return aux_matrix(k, 'parse_more_full_pinyins', b"xi'an", (0, 1, 2, 4, 5))


@case('aux-text-double-parse')
def _(k):
    return aux_matrix(k, 'parse_more_double_pinyins', b'nihk', (0, 1, 2, 3, 4, 9))


@case('aux-text-chewing-parse')
def _(k):
    return aux_matrix(k, 'parse_more_chewings', b'su3cl3', (0, 1, 2, 3, 4, 5, 6, 9))


@case('aux-text-tones')
def _(k):
    # Tone digits ride the keys: the pinyin spelling appends them, the double
    # renderer appends them after a cut key, the zhuyin spelling marks them.
    out = {}
    for parse_name, text, cursors in (('parse_more_full_pinyins', b'ni3hao4', (0, 1, 2, 3, 4, 5, 7)),
                                      ('parse_more_double_pinyins', b'ni3hk4', (0, 1, 2, 3, 4, 5, 6))):
        for label, value in aux_matrix(k, parse_name, text, cursors).items():
            out[parse_name + ' ' + label] = value
    return out


def aux_cursors(k, text, cursors, functions=('full_pinyin', 'double_pinyin', 'chewing')):
    """The listed aux functions at the listed cursors, after a full parse."""
    out = {}
    for fname in functions:
        for cursor in cursors:
            inst = k.alloc()
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
            aux = P(UNTOUCHED)
            ret = k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, cursor, C.byref(aux))
            out['%s %s@%d' % (text.decode(), fname, cursor)] = [
                ret, 'untouched' if aux.value == UNTOUCHED else k.text(aux.value)]
    return out


def aux_options(k, word, texts):
    """The three aux functions under one option word. The double renderer
    aborts the pin three bytes into a key (`pinyin.cpp:3488`), so it is read
    at the cursors that stay within two."""
    k.fn('set_options', B, P, U)(k.ctx, word)
    out = {}
    for text in texts:
        every = range(0, len(text) + 2)
        out.update(aux_cursors(k, text, every, ('full_pinyin', 'chewing')))
        out.update(aux_cursors(k, text, (0, 1, 2, len(text), len(text) + 1), ('double_pinyin',)))
    return out


# Several keys can begin at one byte under the divided, resplit and fuzzy
# options; the pin's renderers read the first (`get_item(column, 0)`).
@case('aux-text-divided-table')
def _(k):
    return aux_options(k, (1 << 5) | (1 << 7), (b'xian', b'jiangnan', b'xianan', b'tiananmen'))


@case('aux-text-resplit-table')
def _(k):
    return aux_options(k, (1 << 5) | (1 << 8), (b'zhengfu', b'xianan', b'dianxin', b'changan'))


@case('aux-text-divided-and-resplit')
def _(k):
    return aux_options(k, (1 << 5) | (1 << 7) | (1 << 8), (b'xian', b'zhengfu', b'xianan'))


@case('aux-text-fuzzy-keys')
def _(k):
    return aux_options(k, (1 << 5) | (1 << 12) | (1 << 15) | (1 << 17), (b'zongguo', b'nan', b'lan', b'zi', b'banggan'))


@case('aux-text-no-parse', control=True)
def _(k):
    out = {}
    for fname in ('full_pinyin', 'double_pinyin', 'chewing'):
        inst = k.alloc()
        aux = P(UNTOUCHED)
        ret = k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, 0, C.byref(aux))
        out[fname] = [ret, 'untouched' if aux.value == UNTOUCHED else k.text(aux.value)]
    return out


@case('aux-text-mixed-parses')
def _(k):
    # The matrix is the last parse, whatever mode made it.
    out = {}
    inst = k.alloc()
    k.fn('parse_more_double_pinyins', Z, P, S)(inst, b'nihk')
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'xian')
    for fname in ('full_pinyin', 'double_pinyin', 'chewing'):
        aux = P(UNTOUCHED)
        ret = k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, 2, C.byref(aux))
        out[fname] = [ret, 'untouched' if aux.value == UNTOUCHED else k.text(aux.value)]
    return out


# batch2 group 7: the sub-index unigram total guard (PR 7, #540)
def choose_text(k, inst, offset, wanted):
    k.fn('guess_candidates', B, P, Z, U)(inst, offset, 0)
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, text = P(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        if text.value == wanted.encode():
            return k.fn('choose_candidate', Z, P, Z, P)(inst, offset, cand)
    return None


def unigram_of(k, token):
    freq = U(UNTOUCHED)
    ret = k.fn('token_get_unigram_frequency', B, P, U, C.POINTER(U))(k.inst, token, C.byref(freq))
    return [ret, freq.value]


# #576: zhuyin.cpp:1813-1826 at 074a2219 reads the phrase item's
# stored unigram, including generation-time +1 and the in-memory overlay.
# e2853126 already ported the getter; keep the pinyin twin as a control.
def token_unigram_system(k):
    token = 0x01001225  # 你; the stored system field is 52887, not 52888.
    out = {'token': hex(token), 'before': unigram_of(k, token)}
    out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 7)
    out['after'] = unigram_of(k, token)
    out['absent-item'] = unigram_of(k, 0x01FFFFFF)
    out['absent-library'] = unigram_of(k, 0x0FFFFFFF)
    return out


def token_unigram_user(k):
    phrase = '你好你好'
    reading = "ni3'hao3'ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ'
    iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert iterator
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(
        iterator, phrase.encode(), reading.encode(), 9)
    k.fn('end_add_phrases', None, P)(iterator)
    assert added
    tokens = [token for token in tokens_of(k, phrase) if token >> 24 == 7]
    assert len(tokens) == 1, tokens
    token = tokens[0]
    out = {'imported': added, 'token': hex(token), 'before': unigram_of(k, token)}
    out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 7)
    out['after'] = unigram_of(k, token)
    return out


# #809: the ten zhuyin-only syllables (content_table keys that pinyin_index
# lacks) are stored as key words the pin hands back unfiltered
# (074a2219 zhuyin.cpp:1778-1811, phrase_index.cpp:33-44). Libraries 1-4 read
# the replaced item through the spelling round trip, which dropped them;
# libraries 5-7 read the user store and are the controls.
ZHUYIN_ONLY = {
    'chua': 'ㄔㄨㄚ', 'den': 'ㄉㄣ', 'din': 'ㄉㄧㄣ', 'fe': 'ㄈㄜ', 'kei': 'ㄎㄟ',
    'len': 'ㄌㄣ', 'nia': 'ㄋㄧㄚ', 'rua': 'ㄖㄨㄚ', 'yai': 'ㄧㄞ', 'zhei': 'ㄓㄟ',
}
ZHUYIN_ONLY_WORDS = ['龘鱻', '麤靐', '驫齉', '爨馕', '鬻厵', '龘麤', '鱻靐', '驫爨', '齉鬻', '馕厵']


def zhuyin_only_readings(k, libraries):
    out = {}
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    for library in libraries:
        for (pinyin, zhuyin), word in zip(ZHUYIN_ONLY.items(), ZHUYIN_ONLY_WORDS):
            if k.mode == 'pinyin':
                readings = ["%s'%s" % (pinyin, pinyin), "%s3'%s3" % (pinyin, pinyin)]
            else:
                readings = ['%s %s' % (zhuyin, zhuyin), '%sˇ %sˇ' % (zhuyin, zhuyin)]
            iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
            added = [k.fn('iterator_add_phrase', B, P, S, S, I)(iterator, word.encode(), r.encode(), 5)
                     for r in readings]
            k.fn('end_add_phrases', None, P)(iterator)
            row = {'added': added}
            tokens = [token for token in tokens_of(k, word) if token >> 24 == library]
            row['tokens'] = [hex(token) for token in tokens]
            for token in tokens:
                count = U(UNTOUCHED)
                row['n_pron'] = [k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(
                    k.inst, token, C.byref(count)), count.value]
                for nth in range(min(count.value, 4)):
                    keys = new(0, 0, 2)
                    ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
                    view = C.cast(keys, C.POINTER(Arr)).contents
                    row['nth %d' % nth] = [ret, C.string_at(view.data, view.len * 2).hex() if view.len else '']
            out['lib %d %s' % (library, pinyin)] = row
    return out


for _mode in ('pinyin', 'zhuyin'):
    _prefix = 'zhuyin-' if _mode == 'zhuyin' else ''
    case(_prefix + 'system-zhuyin-only-readings', mode=_mode)(
        lambda k: zhuyin_only_readings(k, (1, 2, 3, 4)))
    case(_prefix + 'user-zhuyin-only-readings', mode=_mode, control=True)(
        lambda k: zhuyin_only_readings(k, (5, 6, 7)))


# #808: the default facade's USER_FILE libraries 5 (ADDON), 6 (NETWORK) and 7
# (USER) are read from m_phrase_index (074a2219 pinyin.cpp:2821-2834 via
# phrase_index.h:646-652): true with the stored field for a real token, false
# with 0 for an id that owns no item. The pinyin getter used to read the addon
# facade for 5 and 6 and never checked the item for 7.
def token_unigram_user_libraries(k):
    out = {}
    plants = {5: '你好你好', 6: '再见再见', 7: '你好你好呀'}
    readings = {
        'pinyin': {5: "ni3'hao3'ni3'hao3", 6: "zai4'jian4'zai4'jian4", 7: "ni3'hao3'ni3'hao3'ya1"},
        'zhuyin': {5: 'ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ', 6: 'ㄗㄞˋ ㄐㄧㄢˋ ㄗㄞˋ ㄐㄧㄢˋ',
                   7: 'ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ ㄧㄚ'},
    }[k.mode]
    for library, phrase in plants.items():
        iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
        assert iterator
        added = k.fn('iterator_add_phrase', B, P, S, S, I)(
            iterator, phrase.encode(), readings[library].encode(), 9)
        k.fn('end_add_phrases', None, P)(iterator)
        assert added
        tokens = [token for token in tokens_of(k, phrase) if token >> 24 == library]
        assert len(tokens) == 1, tokens
        token = tokens[0]
        out['lib %d token' % library] = hex(token)
        out['lib %d item' % library] = unigram_of(k, token)
        for delta in (0, 2, 100):
            absent = (library << 24) | ((token & 0xFFFFFF) + delta) if delta else token - 1
            out['lib %d absent %#x' % (library, absent)] = unigram_of(k, absent)
        out['lib %d add' % library] = k.fn(
            'token_add_unigram_frequency', B, P, U, U)(k.inst, token, 7)
        out['lib %d after' % library] = unigram_of(k, token)
    return out


def train_selected_top(k, wanted, token, guess_before_choose=False):
    """Choose a normal phrase, guess n-best, then train result 0 without reset.

    This test-only sequence preserves the runbook's train_top precondition:
    a candidate list alone cannot be trained. The normal selection also
    installs the OneStep constraint that train_result3 needs to write counts.
    """
    inst = k.inst
    if k.mode == 'pinyin':
        # SORT_WITHOUT_SENTENCE (0x1) would train during choose instead
        # of installing a constraint (074a2219 pinyin.cpp:2566-2576).
        assert k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
    else:
        assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    count = U()
    assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for index in range(count.value):
        candidate, text, kind = P(), S(), I()
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(
            inst, index, C.byref(candidate))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(
            inst, candidate, C.byref(text))
        assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(
            inst, candidate, C.byref(kind))
        # NORMAL_CANDIDATE / NORMAL_CANDIDATE_AFTER_CURSOR are both 2.
        # The zhuyin BEST_MATCH row installs no constraint (074a2219
        # zhuyin.cpp:1643-1654), so selecting it would make this a no-op.
        if kind.value == 2 and text.value == wanted.encode():
            before = {}
            if guess_before_choose:
                # Retain this NORMAL row before guessing promotes matching
                # text to NBEST in a newly generated candidate list. Guess
                # leaves m_candidates intact (074a2219 pinyin.cpp:1372-1386).
                assert k.fn('guess_sentence', B, P)(inst)
                before['before-choose'] = final_step_sentences(k, inst)
            cursor = k.fn('choose_candidate', I, P, Z, P)(inst, 0, candidate)
            assert cursor > 0, 'normal phrase selection failed'
            after_choose = unigram_of(k, token)
            guessed = k.fn('guess_sentence', B, P)(inst)
            assert guessed, 'training needs a guessed n-best result'
            if k.mode == 'pinyin':
                trained = k.fn('train', B, P, C.c_ubyte)(inst, 0)
            else:
                trained = k.fn('train', B, P)(inst)
            assert trained, 'training n-best result 0 failed'
            return {'chosen': wanted, 'kind': kind.value, 'cursor': cursor,
                    'after-choose': after_choose, 'guess': guessed, 'train': trained, **before}
    raise AssertionError('missing normal candidate: ' + wanted)


def token_unigram_trained(k):
    token = 0x01003e57  # 癫痫: one constrained train adds 69 * 7 = 483.
    assert tokens_of(k, '癫痫') == [token]
    parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
    text = b'dian4xian4' if k.mode == 'pinyin' else b'2u04vu04'
    parsed = k.fn(parse, Z, P, S)(k.inst, text)
    assert parsed == len(text), 'the whole training input must parse'
    out = {'token': hex(token), 'parsed': parsed, 'before': unigram_of(k, token)}
    out['training'] = train_selected_top(k, '癫痫', token)
    assert out['training']['after-choose'] == out['before'], 'selection must not train'
    out['after'] = unigram_of(k, token)
    return out


for _mode in ('pinyin', 'zhuyin'):
    _prefix = 'zhuyin-' if _mode == 'zhuyin' else ''
    case(_prefix + 'token-unigram-system', mode=_mode, control=_mode == 'pinyin')(
        token_unigram_system)
    case(_prefix + 'token-unigram-user', mode=_mode, control=_mode == 'pinyin')(
        token_unigram_user)
    case(_prefix + 'token-unigram-trained', mode=_mode, control=_mode == 'pinyin')(
        token_unigram_trained)
    case(_prefix + 'token-unigram-user-libraries', mode=_mode, control=_mode == 'zhuyin')(
        token_unigram_user_libraries)


@case('train-unigram-total')
def _(k):
    # Once the library's guint32 total would overflow the item stops growing:
    # at the pin 你 freezes at 2122897296 after about 20000 trainings.
    inst = k.inst
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    ni, hao = tokens_of(k, '你')[0], tokens_of(k, '好')[0]
    first = choose_text(k, inst, 0, '你')
    second = choose_text(k, inst, first, '好') if first else None
    k.fn('guess_sentence', B, P)(inst)
    out = {'chosen': [first, second], 'before': [unigram_of(k, ni), unigram_of(k, hao)]}
    train = k.fn('train', B, P, C.c_ubyte)
    for step, count in ((1, 1), (2, 9999), (3, 10000), (4, 10000)):
        rets = {train(inst, 0) for _ in range(count)}
        out['after %d' % sum(c for s, c in ((1, 1), (2, 9999), (3, 10000), (4, 10000)) if s <= step)] = [
            sorted(rets), unigram_of(k, ni), unigram_of(k, hao)]
    return out


@case('predicted-unigram-overflow')
def _(k):
    # Training 你 and 好 fills library 1 to within one seed of its guint32
    # total; accepted predicted candidates (483 each) then run it out, and
    # the pin answers false from the call that no longer fits.
    inst = k.inst
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    first = choose_text(k, inst, 0, '你')
    second = choose_text(k, inst, first, '好') if first else None
    k.fn('guess_sentence', B, P)(inst)
    train = k.fn('train', B, P, C.c_ubyte)
    for _ in range(20000):
        train(inst, 0)
    k.fn('guess_predicted_candidates', B, P, S)(inst, '你'.encode())
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    pick = None
    for i in range(count.value):
        cand, kind, text = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        if kind.value == 5 and (tokens_of(k, text.value.decode()) or [0])[0] >> 24 == 1:
            pick = cand
            break
    out = {'chosen': [first, second], 'found': pick is not None}
    if pick is None:
        return out
    accepted = 0
    choose = k.fn('choose_predicted_candidate', B, P, P)
    while accepted < 1000 and choose(inst, pick):
        accepted += 1
    out['accepted before the first refusal'] = accepted
    return out


# batch2 group 8: choose_predicted_candidate's return table (PR 8, #542, #540)
def predicted_rows(k, inst, prefix, limit=12):
    """The first rows of a predicted list: (type, text)."""
    k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, prefix.encode())
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    for i in range(min(count.value, limit)):
        cand, kind, text = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        rows.append([kind.value, text.value.decode()])
    return rows, count.value


def choose_predicted_of(k, inst, want_kind, skip=0):
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, kind = P(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        if kind.value == want_kind:
            if skip:
                skip -= 1
                continue
            return k.fn('choose_predicted_candidate', B, P, P)(inst, cand)
    return None


@case('choose-predicted-types')
def _(k):
    # The return table: a punctuation row has no frequency and answers true,
    # a prefix row answers true. (The prefix row's bigram is not compared:
    # the pin trains its unigram only.)
    out = {}
    inst = k.inst
    out['before'] = predicted_rows(k, inst, '我')
    out['punctuation'] = choose_predicted_of(k, inst, 8)
    out['punctuation again'] = choose_predicted_of(k, inst, 8)
    out['prefix'] = choose_predicted_of(k, inst, 5)
    return out


def remember_negative(count):
    def run(k):
        inst = k.inst
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihaoshijie')
        out = {'remember': k.fn('remember_user_input', B, P, S, I)(inst, '你好世界'.encode(), count)}
        it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
        has = k.fn('iterator_has_next_phrase', B, P)
        nxt = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))
        rows = []
        while has(it):
            ph, py, n = P(), P(), I()
            nxt(it, C.byref(ph), C.byref(py), C.byref(n))
            rows.append([k.text(ph.value), k.text(py.value), n.value])
        out['rows'] = rows
        return out
    return run


for _count in (-2, -5, -2147483648):
    case('remember-count-%d' % -_count if _count > -100 else 'remember-count-min')(remember_negative(_count))
case('remember-count-default', control=True)(remember_negative(-1))
case('remember-count-seven', control=True)(remember_negative(7))


# batch2 group 9: the input length cap (PR 9, #540)
@case('input-length-cap')
def _(k):
    out = {}
    for length in (4096, 4097, 32766, 32767, 32768, 65537):
        inst = k.alloc()
        text = (b'ni' * length)[:length]
        out[str(length)] = [k.fn('parse_more_full_pinyins', Z, P, S)(inst, text),
                            k.fn('get_parsed_input_length', Z, P)(inst)]
    return out


@case('long-input-sentence')
def _(k):
    # A decode past the old cap: the sentence is as long as the pin's.
    inst = k.alloc()
    text = b'ni' * 2400
    out = {'parsed': k.fn('parse_more_full_pinyins', Z, P, S)(inst, text),
           'guess': k.fn('guess_sentence', B, P)(inst)}
    row = sentence_out(k, inst, 0)
    out['sentence'] = [row[0], len(row[1]) if isinstance(row[1], str) else row[1]]
    return out


# batch2 group 10: pinyin_alloc_instance on a finalised context (PR 10, #528)
@case('alloc-instance-after-fini')
def _(k):
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


# #573 (B battery): a second pinyin_fini / zhuyin_fini on one context
def fini_twice(k):
    # The pin reads and deletes the freed context again (pinyin.cpp:1194-1222,
    # zhuyin.cpp:741-757) and dies of SIGSEGV, with stderr bytes read from
    # freed memory; the ruled answer is a silent return (class (b), row 105).
    context = k.init()
    live = k.fn('alloc_instance', P, P)(context)
    k.fn('free_instance', None, P)(live)
    k.fn('fini', None, P)(context)
    k.fn('fini', None, P)(context)
    return {'second fini returned': True}


case('fini-twice', crash={'second fini returned': True}, stderr_varies=True)(fini_twice)
case('zhuyin-fini-twice', mode='zhuyin', crash={'second fini returned': True}, stderr_varies=True)(fini_twice)


# batch2 group 11: the facade unigram total wraps like the pin's (PR 11, #540)
FACADE_TOTAL = 51051831   # the oracle data's loaded libraries, summed


def wrap_total_to_zero(k, inst):
    """Adds the delta that wraps the facade `guint32` total to zero."""
    add = k.fn('token_add_unigram_frequency', B, P, U, U)
    return add(inst, (1 << 24) | 1, (2 ** 32 - FACADE_TOTAL) % 2 ** 32)


# The zero total aborts the pin only while a searched row is ranked
# (`_compute_frequency_of_items`, `pinyin.cpp:1859`): a window with no phrase
# row — the reserved slot, a mid-key offset — answers true with no warning.
# One case each, since the wrapped total is the context's.
for _label, _offset, _sort in (('end-sort-3', 5, 3), ('mid-key-sort-3', 3, 3), ('end-sort-2', 5, 2)):
    def _no_rank(offset, sort):
        def run(k):
            inst = k.alloc()
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
            wrap_total_to_zero(k, inst)
            count = U()
            ret = k.fn('guess_candidates', B, P, Z, U)(inst, offset, sort)
            k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            return {'ret': ret, 'rows': count.value}
        return run
    case('facade-total-zero-ranks-nothing-' + _label, control=True)(_no_rank(_offset, _sort))


@case('facade-total-zero', abort=False)
def _(k):
    # `assert(0 < total_freq)` while ranking a candidate (pinyin.cpp:1859).
    inst = k.inst
    added = wrap_total_to_zero(k, inst)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    return {'add': added, 'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1E)}


# batch2 group 12a: library-index, scheme and iterator refusals (PR 12a, #525)
def ctx_call(name, res, *args):
    def run(k):
        return {'ret': k.fn(name, res, P, *[t for t, _ in args])(k.ctx, *[v for _, v in args])}
    return run


for _name, _call in (
        ('set-full-pinyin-scheme-0', ctx_call('set_full_pinyin_scheme', B, (I, 0))),
        ('set-full-pinyin-scheme-4', ctx_call('set_full_pinyin_scheme', B, (I, 4))),
        ('set-double-pinyin-scheme-30', ctx_call('set_double_pinyin_scheme', B, (I, 30))),
        ('set-zhuyin-scheme-0', ctx_call('set_zhuyin_scheme', B, (I, 0))),
        ('set-zhuyin-scheme-10', ctx_call('set_zhuyin_scheme', B, (I, 10))),
        ('set-zhuyin-scheme-7', ctx_call('set_zhuyin_scheme', B, (I, 7))),
        ('load-phrase-library-0', ctx_call('load_phrase_library', B, (C.c_ubyte, 0))),
        ('load-phrase-library-8', ctx_call('load_phrase_library', B, (C.c_ubyte, 8))),
        ('unload-phrase-library-16', ctx_call('unload_phrase_library', B, (C.c_ubyte, 16))),
        ('unload-addon-phrase-library-16', ctx_call('unload_addon_phrase_library', B, (C.c_ubyte, 16)))):
    case('abort-' + _name, abort=False)(_call)


@case('abort-iterator-get-next-phrase', abort=False)
def _(k):
    # An empty user dictionary: the begin probes no next token.
    it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
    phrase, pinyin, count = P(), P(), I()
    return {'has next': k.fn('iterator_has_next_phrase', B, P)(it),
            'ret': k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
                it, C.byref(phrase), C.byref(pinyin), C.byref(count))}


# #681, row 62: the bigram twin. A fresh user dir has an empty user bigram,
# so the iterator's predecessor is still null_token and the pin asserts
# (`pinyin.cpp:902`) before it writes anything. A control: the parent build
# already answers this way; the case registers behaviour, it changes none.
@case('abort-bigram-iterator-get-next-phrase', control=True, abort=False)
def _(k):
    it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
    phrase, pinyin, count = P(UNTOUCHED), P(UNTOUCHED), I(UNTOUCHED)
    has_next = k.fn('bigram_iterator_has_next_phrase', B, P)(it)
    ret = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
        it, C.byref(phrase), C.byref(pinyin), C.byref(count))
    return {'has next': has_next, 'ret': ret,
            'untouched phrase': phrase.value == UNTOUCHED,
            'untouched pinyin': pinyin.value == UNTOUCHED,
            'untouched count': count.value == UNTOUCHED}


# The in-range neighbours answer without a warning, as at the pin.
@case('scheme-and-library-neighbours', control=True)
def _(k):
    out = {}
    for label, name, res, arg in (
            ('full 1', 'set_full_pinyin_scheme', B, (I, 1)), ('full 3', 'set_full_pinyin_scheme', B, (I, 3)),
            ('double 6', 'set_double_pinyin_scheme', B, (I, 6)), ('double 99', 'set_double_pinyin_scheme', B, (I, 99)),
            ('zhuyin 1', 'set_zhuyin_scheme', B, (I, 1)), ('zhuyin 9', 'set_zhuyin_scheme', B, (I, 9)),
            ('load 1', 'load_phrase_library', B, (C.c_ubyte, 1)), ('load 7', 'load_phrase_library', B, (C.c_ubyte, 7)),
            ('load 16', 'load_phrase_library', B, (C.c_ubyte, 16)), ('load 255', 'load_phrase_library', B, (C.c_ubyte, 255)),
            ('unload 1', 'unload_phrase_library', B, (C.c_ubyte, 1)), ('unload 15', 'unload_phrase_library', B, (C.c_ubyte, 15)),
            ('unload addon 15', 'unload_addon_phrase_library', B, (C.c_ubyte, 15)),
            ('load addon 16', 'load_addon_phrase_library', B, (C.c_ubyte, 16))):
        out[label] = k.fn(name, res, P, arg[0])(k.ctx, arg[1])
    return out


# batch2 group 12b: cursor, offset, sentence-index and aux-text refusals (PR 12b, #525)
def full_inst(k, text):
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    return inst


@case('abort-get-sentence-past-rows', abort=False)
def _(k):
    inst = full_inst(k, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    return {'ret': sentence_out(k, inst, 5)[0]}


@case('abort-get-left-pinyin-offset-after-separator', abort=False)
def _(k):
    inst = full_inst(k, b"ni'hao")
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_left_pinyin_offset', B, P, Z, C.POINTER(Z))(inst, 3, C.byref(out))}


@case('abort-get-right-pinyin-offset-after-separator', abort=False)
def _(k):
    inst = full_inst(k, b"ni'hao")
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_right_pinyin_offset', B, P, Z, C.POINTER(Z))(inst, 3, C.byref(out))}


@case('abort-get-right-pinyin-offset-nihao-5', abort=False)
def _(k):
    inst = full_inst(k, b'nihao')
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_right_pinyin_offset', B, P, Z, C.POINTER(Z))(inst, 5, C.byref(out))}


@case('abort-get-character-offset-past-matrix', abort=False)
def _(k):
    inst = full_inst(k, b'nihao')
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
        inst, '你好'.encode(), 99, C.byref(out))}


@case('abort-get-character-offset-after-separator', abort=False)
def _(k):
    inst = full_inst(k, b"ni'hao")
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
        inst, '你好'.encode(), 3, C.byref(out))}


@case('abort-parse-full-pinyin-apostrophe', abort=False)
def _(k):
    key = C.c_uint16(UNTOUCHED & 0xFFFF)
    return {'ret': k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(
        k.inst, b"n'i", C.byref(key))}


@case('abort-get-pinyin-key-empty-matrix', abort=False)
def _(k):
    out = P(UNTOUCHED)
    return {'ret': k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(k.alloc(), 0, C.byref(out))}


@case('abort-get-pinyin-key-rest-empty-matrix', abort=False)
def _(k):
    out = P(UNTOUCHED)
    return {'ret': k.fn('get_pinyin_key_rest', B, P, Z, C.POINTER(P))(k.alloc(), 0, C.byref(out))}


@case('abort-double-auxiliary-text-cut-three-bytes-in', abort=False)
def _(k):
    inst = full_inst(k, b'zhong')
    aux = P(UNTOUCHED)
    return {'ret': k.fn('get_double_pinyin_auxiliary_text', B, P, Z, C.POINTER(P))(inst, 3, C.byref(aux))}


# A leading `'` leaves column 0 empty at the pin and the aux walk asserts
# `get_column_size(offset) >= 1` (`pinyin.cpp:3311`) on every cursor.
def leading_apostrophe_aux(fname, text=b"'nihao"):
    def run(k):
        inst = full_inst(k, text)
        aux = P(UNTOUCHED)
        return {'ret': k.fn('get_%s_auxiliary_text' % fname, B, P, Z, C.POINTER(P))(inst, 2, C.byref(aux))}
    return run


for _fname in ('full_pinyin', 'double_pinyin', 'chewing'):
    case('abort-%s-auxiliary-text-leading-apostrophe' % _fname.replace('_', '-'), abort=False)(
        leading_apostrophe_aux(_fname))
case('abort-full-pinyin-auxiliary-text-doubled-leading-apostrophe', abort=False)(
    leading_apostrophe_aux('full_pinyin', b"''ni"))


# The graceful neighbours of the sites above answer without a warning.
@case('cursor-and-sentence-neighbours', control=True)
def _(k):
    out = {}
    inst = full_inst(k, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    out['sentence 0'] = sentence_out(k, inst, 0)
    fresh = k.alloc()
    out['sentence of fresh'] = sentence_out(k, fresh, 3)
    for label, name, offset in (('left 2', 'get_left_pinyin_offset', 2), ('right 2', 'get_right_pinyin_offset', 2),
                                ('right 4', 'get_right_pinyin_offset', 4)):
        res = Z(UNTOUCHED)
        out[label] = [k.fn(name, B, P, Z, C.POINTER(Z))(inst, offset, C.byref(res)),
                      'untouched' if res.value == UNTOUCHED else res.value]
    key = C.c_uint16(UNTOUCHED & 0xFFFF)
    out['parse ni'] = [k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(inst, b'ni', C.byref(key)), key.value]
    for offset in (0, 1):
        ptr = P(UNTOUCHED)
        ret = k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(inst, offset, C.byref(ptr))
        out['key %d' % offset] = [ret, ptr.value is None]
    for cursor in (1, 2, 5):
        aux = P(UNTOUCHED)
        ret = k.fn('get_double_pinyin_auxiliary_text', B, P, Z, C.POINTER(P))(inst, cursor, C.byref(aux))
        out['double aux %d' % cursor] = [ret, k.text(aux.value) if aux.value != UNTOUCHED else 'untouched']
    return out


# batch2 group 12c: candidate-type refusals (PR 12c, #525)
def row_of(k, inst, want_kind):
    """The first candidate of a type in the instance's list, or None."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, kind = P(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        if kind.value == want_kind:
            return cand
    return None


def guessed(k, text=b'nihao'):
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    return inst


def predicted(k, prefix='我'):
    inst = k.alloc()
    k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, prefix.encode())
    return inst


@case('abort-get-candidate-nbest-index-normal-row', abort=False)
def _(k):
    inst = guessed(k)
    out = C.c_ubyte(0xAB)
    ret = k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
        inst, row_of(k, inst, 2), C.byref(out))
    # The pin dies before it writes `*index`; an `untouched*` field must stay true.
    return {'ret': ret, 'untouched index': out.value == 0xAB}


@case('abort-remove-user-candidate-normal-system-token', abort=False)
def _(k):
    inst = guessed(k)
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, row_of(k, inst, 2))}


@case('abort-remove-user-candidate-sentence-row', abort=False)
def _(k):
    inst = guessed(k)
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, row_of(k, inst, 1))}


# lane #525 batch C: the pin asserts when pinyin_remove_user_candidate cannot
# remove a user phrase (class (c): pinyin.cpp:3743/3750/3759). The subject
# fails the call with false and one warning in the libpinyin domain.
def _add_user_phrase(k, text, readings, count=5):
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert it
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    added = [add(it, text.encode(), reading, count) for reading in readings]
    k.fn('end_add_phrases', None, P)(it)
    assert all(added)
    return added


def _user_candidate_rows(k, inst):
    """The NORMAL user-dictionary candidates of the current guess list."""
    rows = []
    n = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
    for i in range(n.value):
        cand, kind = P(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        if kind.value == 2 and k.fn('is_user_candidate', B, P, P)(inst, cand):
            rows.append(cand)
    return rows


def _guessed_user_phrase(k, text, readings, parse=b'dada'):
    _add_user_phrase(k, text, readings)
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    rows = _user_candidate_rows(k, inst)
    assert rows
    return inst, rows


@case('abort-remove-user-candidate-twice', abort=False)
def _(k):
    inst, rows = _guessed_user_phrase(k, '龘龘', [b"da2'da2"])
    first = k.fn('remove_user_candidate', B, P, P)(inst, rows[0])
    # The first removal must complete, or the second call would assert for
    # the wrong reason (the phrase still present).
    assert first, 'the first removal did not complete'
    # The phrase is gone from the user dictionary: the pin asserts at :3743.
    return {'first': first, 'ret': k.fn('remove_user_candidate', B, P, P)(inst, rows[0])}


@case('abort-remove-user-candidate-missing-phrase-table', abort=False)
def _(k):
    _add_user_phrase(k, '龘龘', [b"da2'da2"])
    assert k.fn('save', B, P)(k.ctx)
    k.fn('fini', None, P)(k.ctx)
    # user_phrase_index.bin holds the phrase table; user.bin still holds the
    # phrase index, so :3743 passes and :3750 fires.
    index = os.path.join(k.user, 'user_phrase_index.bin')
    os.replace(index, index + '.gone')
    ctx = k.init()
    assert ctx
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'dada')
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    rows = _user_candidate_rows(k, inst)
    assert rows
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, rows[0])}


@case('abort-remove-user-candidate-multi-pron', abort=False)
def _(k):
    # Two readings for one phrase: the second is merged without a pinyin
    # index entry, so :3759 fires when the phrase is removed.
    inst, rows = _guessed_user_phrase(k, '龘龘', [b"da2'da2", b"da1'da1"])
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, rows[0])}


# batch 525-D: `pinyin_remove_user_candidate` also drives `Bigram::mask_out`
# (`pinyin.cpp:3766`, mask `PHRASE_INDEX_LIBRARY_MASK|PHRASE_MASK`), so all
# three corrupt user-bigram shapes reach it. The pin's phrase-index removals
# are in memory only and die with the abort, so a failed call leaves the store
# unchanged; the subject refuses before it removes anything.
def _removable_user_bigram_context(k, rows_for):
    """A context with one persisted user phrase and a crafted `user_bigram.db`
    carrying `rows_for(token)`. The phrase makes the candidate row acceptable
    (the batch-C checks pass), so the pin reaches `user_bigram->mask_out`
    (`pinyin.cpp:3766`). `rows_for` gets the phrase's user token, which the
    residual shape's item must equal — the pin's mask selects nothing
    otherwise."""
    _add_user_phrase(k, '龘龘', [b"da2'da2"])
    assert k.fn('save', B, P)(k.ctx)
    probe = k.alloc()
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    arr = new(0, 0, 4)
    assert k.fn('lookup_tokens', B, P, S, P)(probe, '龘龘'.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    token = list(C.cast(view.data, C.POINTER(U))[:view.len])[0]
    k.fn('free_instance', None, P)(probe)
    k.fn('fini', None, P)(k.ctx)
    _craft_hash(os.path.join(k.user, 'user_bigram.db'), rows_for(token))
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'dada')
    assert k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    candidates = _user_candidate_rows(k, inst)
    assert candidates, 'no user candidate'
    return inst, candidates[0]


@case('abort-remove-user-candidate-short-bigram-value', abort=False)
def _(k):
    inst, cand = _removable_user_bigram_context(
        k, lambda _token: [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, cand)}


@case('abort-remove-user-candidate-non-token-bigram-key', abort=False)
def _(k):
    inst, cand = _removable_user_bigram_context(
        k, lambda _token: [(b'\x00\x01', b'\x07\x00\x00\x00')])
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, cand)}


@case('abort-remove-user-candidate-residual-bigram-gram', abort=False)
def _(k):
    # `total_freq` 7 with one item of the removed token and frequency 3:
    # `SingleGram::mask_out` drops the item and leaves 4, so the pin's
    # `get_length` assert dies (`ngram.cpp:70`, `ngram_bdb.cpp:243`).
    def rows(token):
        value = b'\x07\x00\x00\x00' + token.to_bytes(4, 'little') + (3).to_bytes(4, 'little')
        return [(b'\x01\x00\x00\x00', value)]

    inst, cand = _removable_user_bigram_context(k, rows)
    return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, cand)}


@case('abort-choose-candidate-predicted-prefix-row', abort=0)
def _(k):
    inst = predicted(k)
    return {'ret': k.fn('choose_candidate', I, P, Z, P)(inst, 0, row_of(k, inst, 5))}


@case('abort-choose-predicted-candidate-normal-row', abort=False)
def _(k):
    inst = guessed(k)
    return {'ret': k.fn('choose_predicted_candidate', B, P, P)(inst, row_of(k, inst, 2))}


@case('abort-choose-predicted-candidate-sentence-row', abort=False)
def _(k):
    inst = guessed(k)
    return {'ret': k.fn('choose_predicted_candidate', B, P, P)(inst, row_of(k, inst, 1))}


# `pinyin_choose_candidate` under SORT_WITHOUT_SENTENCE_CANDIDATE asserts
# `0 == offset` on the NORMAL leg (`pinyin.cpp:2566`, register row 45).
def choose_without_sentence(k, offset):
    inst = full_inst(k, b'nihao')
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1)
    row = row_of(k, inst, 2)
    assert row, 'no NORMAL row'
    return {'ret': k.fn('choose_candidate', I, P, Z, P)(inst, offset, row)}


@case('abort-choose-candidate-without-sentence-offset-1', abort=0)
def _(k):
    return choose_without_sentence(k, 1)


# At offset 0 the leg trains the unigram and answers 1 on both sides.
@case('choose-candidate-without-sentence-offset-0', control=True)
def _(k):
    return choose_without_sentence(k, 0)


# The rows the guards leave alone answer without a warning.
@case('candidate-type-neighbours', control=True)
def _(k):
    out = {}
    inst = guessed(k)
    nbest = C.c_ubyte(0xAB)
    out['nbest of sentence row'] = [k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
        inst, row_of(k, inst, 1), C.byref(nbest)), nbest.value]
    out['is user'] = [k.fn('is_user_candidate', B, P, P)(inst, row_of(k, inst, kind)) for kind in (1, 2)]
    out['choose normal'] = k.fn('choose_candidate', I, P, Z, P)(inst, 0, row_of(k, inst, 2))
    pred = predicted(k)
    out['choose predicted prefix'] = k.fn('choose_predicted_candidate', B, P, P)(pred, row_of(k, pred, 5))
    out['choose predicted punctuation'] = k.fn('choose_predicted_candidate', B, P, P)(pred, row_of(k, pred, 8))
    return out


# batch2 group 12d: training a stale forcing (PR 12d, #525, row 6)
def row_with_text(k, inst, kind, text):
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, ctype, string = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(ctype))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(string))
        if ctype.value == kind and string.value == text.encode():
            return cand
    return None


def forced_sentence(k, relook):
    """nihao decoded, 泥 forced over 'ni' afterwards; optionally looked up again."""
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    chosen = k.fn('choose_candidate', I, P, Z, P)(inst, 0, row_with_text(k, inst, 2, '泥'))
    if relook:
        k.fn('guess_sentence', B, P)(inst)
    return inst, chosen


@case('abort-zhuyin-train-stale-forcing', mode='zhuyin', abort=False)
def _(k):
    # The same walk through libzhuyin: a forcing added after the last
    # sentence lookup (`phonetic_lookup.h:868`).
    inst = k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    chosen = None
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, text = P(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        if text.value == '泥'.encode():
            chosen = cand
            break
    k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
    return {'ret': k.fn('train', B, P)(inst)}


@case('abort-train-stale-forcing', abort=False)
def _(k):
    inst, _chosen = forced_sentence(k, False)
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# An index past a NONEMPTY result set, on a real user directory: the user-dir
# gate passes and `assert(index < results.size())` fails (`pinyin.cpp:2684`).
# `null-user-session-pinyin` trains with a NULL user directory and so stops
# at the earlier gate.
@case('abort-train-index-past-results', abort=False, userdir=True)
def _(k):
    inst = full_inst(k, b'nihao')
    assert k.fn('guess_sentence', B, P)(inst), 'no sentence to train'
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 255)}


@case('train-forcing-after-lookup', control=True)
def _(k):
    inst, chosen = forced_sentence(k, True)
    return {'chosen': chosen, 'train': k.fn('train', B, P, C.c_ubyte)(inst, 0),
            'again': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# #825: a forced phrase over an abbreviation whose incomplete keys expand to
# more than SCAN_EXPANSION_LIMIT (4,096) complete-key combinations. The pin's
# `validate_constraint` (`phonetic_lookup.cpp:120-165` at 074a2219) searches
# no table: `compute_pronunciation_possibility` walks the matrix columns
# against the forced item's stored pronunciations
# (`phonetic_key_matrix.cpp:534` onward), where an incomplete key matches any
# final (`pinyin_compare_middle_and_final3`, `pinyin_phrase3.h:80-86`), so the
# constraint survives however many completions the abbreviation has.
# Option word 0x18a (incomplete pinyin), sort word 0x1e; two user phrases of
# the same initials, the first (A) counted 50 and the second (B) counted 1.
CONSTRAINT_OPTIONS, CONSTRAINT_SORT = 0x18A, 0x1E


def constraint_phrases(k, phrases):
    """Imports (text, reading, count) rows into USER_DICTIONARY; the returns."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert it
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    added = [add(it, text.encode(), reading, count) for text, reading, count in phrases]
    k.fn('end_add_phrases', None, P)(it)
    return added


def candidate_heads(k, inst, limit=6):
    """(kind, text) of the first `limit` rows of the candidate list."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    heads = []
    for i in range(min(count.value, limit)):
        cand, kind, text = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        heads.append([kind.value, text.value.decode() if text.value else None])
    return heads


def constraint_instance(k, phrases, parse, options=CONSTRAINT_OPTIONS):
    """The shared head of the sequence: options, imports, parse, control guess,
    candidate list, and the second phrase's candidate chosen. On the full data
    the n-best usually holds the first phrase alone, so the second reaches the
    list as its NORMAL row, else as an NBEST row; choosing either installs
    the same OneStep forcing over the whole input (`pinyin.cpp:2515-2520` and
    `:2566-2576`)."""
    out = {'options': k.fn('set_options', B, P, U)(k.ctx, options),
           'import': constraint_phrases(k, phrases)}
    inst = k.alloc()
    out['parse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
    out['control guess'] = k.fn('guess_sentence', B, P)(inst)
    out['control row 0'] = sentence_out(k, inst, 0)
    out['candidates'] = k.fn('guess_candidates', B, P, Z, U)(inst, 0, CONSTRAINT_SORT)
    out['head'] = candidate_heads(k, inst)
    kind = 2 if row_with_text(k, inst, 2, phrases[1][0]) else 1
    out['chosen kind'] = kind
    row = row_with_text(k, inst, kind, phrases[1][0])
    assert row, 'no row for ' + phrases[1][0]
    out['choose'] = k.fn('choose_candidate', I, P, Z, P)(inst, 0, row)
    return inst, out


def constraint_survives(k, phrases, parse):
    inst, out = constraint_instance(k, phrases, parse)
    out['guess 5'] = k.fn('guess_sentence', B, P)(inst)
    out['row 0 at 5'] = sentence_out(k, inst, 0)
    out['reparse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
    out['guess 6'] = k.fn('guess_sentence', B, P)(inst)
    out['row 0 at 6'] = sentence_out(k, inst, 0)
    return out


CONSTRAINT_PHRASES = [
    ('中华人民', b"zhong1'hua2'ren2'min2", 50), ('种花人民', b"zhong3'hua1'ren2'min2", 1)]
CONSTRAINT_PHRASES_LONG = [
    ('中华人民共和国', b"zhong1'hua2'ren2'min2'gong4'he2'guo2", 50),
    ('中华人民公会国', b"zhong1'hua2'ren2'min2'gong1'hui4'guo2", 1)]

# (input, incomplete-key product on main's `expand_keys`): under the cap the
# subject already agreed with the pin, so those rows are the controls.
for _parse, _product, _phrases in (
        (b'zhonghuarenmin', 'none', CONSTRAINT_PHRASES),
        (b'zhonghuarm', '266', CONSTRAINT_PHRASES),
        (b'zhhrenmin', '361', CONSTRAINT_PHRASES),
        (b'zhonghrm', '5054', CONSTRAINT_PHRASES),
        (b'zhhrm', '96026', CONSTRAINT_PHRASES),
        (b'zhhrmghg', 'over 96026', CONSTRAINT_PHRASES_LONG)):
    case('constraint-survives-' + _parse.decode(), control=_product in ('none', '266', '361'))(
        lambda k, _p=_parse, _ph=_phrases: constraint_survives(k, _ph, _p))


# The same forcing met by `reconcile_replaced_selection` instead of by the
# next guess: with `PINYIN_CORRECT_V_U` on, `jve` and `jue` are one key, so
# re-parsing `jvezhhrm` over `juezhhrm` changes a byte inside the covered span
# without moving a key boundary. The replacement does not extend the covered
# bytes, so the engine revalidates the retained forcing (96,026 completions of
# `zh h r m`) before the next lookup; nothing is guessed in between.
CONSTRAINT_PHRASES_V_U = [
    ('决中华人民', b"jue2'zhong1'hua2'ren2'min2", 50),
    ('绝种花人民', b"jue2'zhong3'hua1'ren2'min2", 1)]


def constraint_survives_reconcile(parse, replacement):
    def run(k):
        inst, out = constraint_instance(k, CONSTRAINT_PHRASES_V_U, parse, options=CONSTRAINT_OPTIONS | 1 << 27)
        out['reparse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, replacement)
        out['candidates after'] = k.fn('guess_candidates', B, P, Z, U)(inst, 0, CONSTRAINT_SORT)
        out['head after'] = candidate_heads(k, inst)
        out['guess'] = k.fn('guess_sentence', B, P)(inst)
        out['row 0'] = sentence_out(k, inst, 0)
        return out
    return run


for _parse, _replacement, _product in (
        (b'juezhonghuarm', b'jvezhonghuarm', 266), (b'juezhhrm', b'jvezhhrm', 96026)):
    case('constraint-survives-reconcile-' + _parse.decode(), control=_product < 4096)(
        constraint_survives_reconcile(_parse, _replacement))


# #825 review: `validate_constraint` declares `PhraseItem item;` once, before
# its loop (`phonetic_lookup.cpp:141`), and `get_phrase_item` leaves it alone
# when it fails (`phrase_index.h:649-650`, `phrase_index.cpp:189-193`). A
# forced token whose library was unloaded is therefore checked against the
# previous forcing's item. Two phrases forced, the later one only in the GBK
# library (2), which is then unloaded: the stale item keeps the later forcing
# when it spells the later span (`nini`: both read `ni`) and drops it when it
# does not (`nihao`).
def constraint_stale_item(first, second, parse):
    def run(k):
        out = {'options': k.fn('set_options', B, P, U)(k.ctx, CONSTRAINT_OPTIONS)}
        inst = k.alloc()
        out['parse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
        out['guess'] = k.fn('guess_sentence', B, P)(inst)
        out['first'] = choose_text(k, inst, 0, first)
        out['second'] = choose_text(k, inst, out['first'], second)
        out['tokens'] = [hex(t) for t in tokens_of(k, second)]
        out['guess loaded'] = k.fn('guess_sentence', B, P)(inst)
        out['row 0 loaded'] = sentence_out(k, inst, 0)
        out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 2)
        out['guess unloaded'] = k.fn('guess_sentence', B, P)(inst)
        out['row 0 unloaded'] = sentence_out(k, inst, 0)
        return out
    return run


case('constraint-stale-item-keeps-nini')(constraint_stale_item('你', '抳', b'nini'))
case('constraint-stale-item-drops-nihao', control=True)(constraint_stale_item('你', '晧', b'nihao'))


# batch G: a stored forcing outlives a re-parse that shortened the input
# (PR G, #525, row 82). The walk tails the LAST span at
# `constraints->length() - 1` (phonetic_lookup.h:921), not the current
# matrix's last column, and the store survives the parse unvalidated
# (pinyin.cpp:1497-1525). `increase_pronunciation_possibility` then asserts
# `end < matrix->size()` (`storage/phonetic_key_matrix.cpp:661`) on a matrix
# the re-parse shrank; the pin aborts, the subject refuses before observing
# anything and warns once in the `libpinyin` domain.
def reparse_after_choose(k, shrink):
    """nihao decoded, a phrase chosen past row 1, looked up again, re-parsed."""
    inst = k.alloc()
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    chosen = P()
    k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 2, C.byref(chosen))
    k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
    k.fn('guess_sentence', B, P)(inst)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, shrink)
    return inst


@case('abort-train-after-reparse-past-matrix-end', abort=False)
def _(k):
    inst = reparse_after_choose(k, b'ni')
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# The `'ni` twin walks the same clamp into a matrix whose column 0 the
# leading apostrophe left empty (`:663`): reproduced, SIGABRT at
# `phonetic_key_matrix.cpp:663` under the `__assert_fail` wrapper.
@case('abort-train-after-reparse-empty-start-column', abort=False)
def _(k):
    inst = reparse_after_choose(k, b"'ni")
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# batch G review fix: the separator between keys is a zero-key column, not
# an empty one. `fill_matrix` fills it (`phonetic_key_matrix.cpp:52-78`), so
# the pin's `get_column_size(start) > 0` (`:663`) passes and training
# completes when the stale span's end lands on a key column. After a
# re-parse to `ni'ha`, the stale second span starts on the apostrophe
# column and the pin trains; the engine accepts the zero-key column too.
@case('train-after-reparse-internal-zero-key', control=True)
def _(k):
    inst = reparse_after_choose(k, b"ni'ha")
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


# The stale end can still land on an empty column: after a re-parse to
# `ni'hao`, the pin passes `:663` (the apostrophe column holds a zero key)
# and aborts at `:664` (`get_column_size(end) > 0`). The engine refuses with
# the same fault.
@case('abort-train-after-reparse-empty-end-column', abort=False)
def _(k):
    inst = reparse_after_choose(k, b"ni'hao")
    return {'ret': k.fn('train', B, P, C.c_ubyte)(inst, 0)}






# The keys the :661/:663 guard leaves alone: the same choose-and-look-up
# with no shortening re-parse, and a re-parse that repeats the input.
@case('train-after-choose-lookup-control', control=True)
def _(k):
    def choose_and_relook(inst):
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        k.fn('guess_sentence', B, P)(inst)
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        chosen = P()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 2, C.byref(chosen))
        k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
        k.fn('guess_sentence', B, P)(inst)

    inst = k.alloc()
    choose_and_relook(inst)
    out = {'no-reparse': k.fn('train', B, P, C.c_ubyte)(inst, 0)}

    inst = k.alloc()
    choose_and_relook(inst)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    out['same-length-reparse'] = k.fn('train', B, P, C.c_ubyte)(inst, 0)
    return out


# batch E: over-long user chewing keys (#525, row 84). The pin's user
# `ChewingLargeTable2` instantiates its entries only for lengths 1..=16, so a
# `user_pinyin_index.bin` key past `MAX_PHRASE_LENGTH` syllables drives both
# `mask_out` (`storage/chewing_large_table2_bdb.cpp:529`) and the
# longer-candidate walk (`:282`) into their `switch`'s `default: abort()`. The
# key cannot be written through the API (`_add_phrase` refuses
# `phrase_length >= MAX_PHRASE_LENGTH`, `pinyin.cpp:643`), so the case crafts
# the user index directly, in the running cell's own format (`_craft_db`): a
# Berkeley DB btree on bdb (the pin's own layout,
# `chewing_large_table2_bdb.cpp:58`), a Kyoto Cabinet snapshot on kc and a
# tkrzw `TreeDBM` file on tkrzw. The kc and tkrzw chewing tables carry the
# same switch (`chewing_large_table2_kyotodb.cpp:498`, `:268`;
# `chewing_large_table2_tkrzwdb.cpp:465`, `:251`), so each cell's pin dies at
# its own line.
def _craft_db(path, rows, dbtype, keep=False):
    """A user-table container of raw `key -> value` records in the running
    cell's own format: a Berkeley DB `dbtype` container on bdb, the Kyoto
    Cabinet snapshot the pin's `load_db` reads on kc (every user table
    there is loaded with `load_snapshot`, `chewing_large_table2_kyotodb.cpp:106`,
    `ngram_kyotodb.cpp:54`, `phrase_large_table3_kyotodb.cpp:109`), or the
    tkrzw `TreeDBM` file the pin's `load_db` opens on tkrzw
    (`chewing_large_table2_tkrzwdb.cpp:83`, `ngram_tkrzwdb.cpp:48`,
    `phrase_large_table3_tkrzwdb.cpp:83`); the bigram is a `HashDBM` there
    (`ngram_tkrzwdb.cpp:50`). `dbtype` picks the container kind (1 btree/tree,
    2 hash); the rows are the same records in every cell. `keep=True` is
    the fixture that adds rows to the copied system index: a native attached
    TreeDB on kc (`chewing_large_table2_kyotodb.cpp:87-89`), a TreeDBM on
    tkrzw (`chewing_large_table2_tkrzwdb.cpp:87-88`), and a btree on bdb."""
    if CELL == 'kc':
        assert not keep or dbtype == 1, 'retained KC fixtures are system btrees'
        return _craft_kc(path, rows, keep=keep)
    if CELL == 'tkrzw':
        return _craft_tkrzw(path, rows, 'HashDBM' if dbtype == 2 else 'TreeDBM', keep=keep)
    return _craft_bdb(path, rows, dbtype, keep=keep)


def _craft_kc(path, rows, keep=False):
    """A Kyoto Cabinet snapshot of the records (`BasicDB::dump_snapshot`),
    written through the C API from an in-memory database. `keep=True`
    instead adds to the native TreeDB the pin attaches as a system index."""
    kc = C.CDLL('libkyotocabinet.so.16')
    kc.kcdbnew.restype = C.c_void_p
    kc.kcdbdel.argtypes = [C.c_void_p]
    kc.kcdbopen.argtypes = [C.c_void_p, C.c_char_p, C.c_uint32]
    kc.kcdbset.argtypes = [C.c_void_p, C.c_char_p, C.c_size_t, C.c_char_p, C.c_size_t]
    kc.kcdbdumpsnap.argtypes = [C.c_void_p, C.c_char_p]
    kc.kcdbclose.argtypes = [C.c_void_p]
    if os.path.exists(path) and not keep:
        os.unlink(path)
    db = kc.kcdbnew()
    assert db
    # Existing system TreeDB: KCOWRITER, explicit type because its suffix
    # is .bin. User snapshot: KCOWRITER | KCOCREATE on the memory database.
    target = (path + '#type=kct').encode() if keep else b'-'
    mode = (1 << 1) if keep else (1 << 1) | (1 << 2)
    assert kc.kcdbopen(db, target, mode)
    for key, value in rows:
        assert kc.kcdbset(db, key, len(key), value, len(value))
    if not keep:
        assert kc.kcdbdumpsnap(db, path.encode())
    assert kc.kcdbclose(db)
    kc.kcdbdel(db)


def _craft_tkrzw(path, rows, kind, keep=False):
    """A tkrzw `kind` file (`TreeDBM` for the chewing and phrase tables,
    `HashDBM` for the bigram, as each pin `load_db` opens it) of the records,
    written through the C API with the default tuning the pin's `save_db`
    uses. `keep=True` adds to an existing system index without unlinking it."""
    tk = C.CDLL('libtkrzw.so.1')
    tk.tkrzw_dbm_open.restype = C.c_void_p
    tk.tkrzw_dbm_open.argtypes = [C.c_char_p, C.c_bool, C.c_char_p]
    tk.tkrzw_dbm_set.restype = C.c_bool
    tk.tkrzw_dbm_set.argtypes = [C.c_void_p, C.c_char_p, C.c_int32, C.c_char_p, C.c_int32, C.c_bool]
    tk.tkrzw_dbm_close.restype = C.c_bool
    tk.tkrzw_dbm_close.argtypes = [C.c_void_p]
    if os.path.exists(path) and not keep:
        os.unlink(path)
    db = tk.tkrzw_dbm_open(path.encode(), True, ('dbm=' + kind).encode())
    assert db
    for key, value in rows:
        assert tk.tkrzw_dbm_set(db, key, len(key), value, len(value), True)
    assert tk.tkrzw_dbm_close(db)


def _craft_bdb(path, rows, dbtype, keep=False, read_keys=(), allow_missing=False):
    """A Berkeley DB `dbtype` container; `keep=True` adds to existing rows."""
    import ctypes.util

    class DBT(C.Structure):
        # 40 bytes on x86_64: the `app_data` pointer between `doff` and
        # `flags` (omitting it misaligns `flags`, and `__db_put_pp` then
        # rejects the flag argument).
        _fields_ = [
            ('data', C.c_void_p),
            ('size', C.c_uint),
            ('ulen', C.c_uint),
            ('dlen', C.c_uint),
            ('doff', C.c_uint),
            ('app_data', C.c_void_p),
            ('flags', C.c_uint),
        ]

    db = C.CDLL(ctypes.util.find_library('db'))
    db.db_create.argtypes = [C.POINTER(C.c_void_p), C.c_void_p, C.c_uint]
    db.db_create.restype = C.c_int
    db.__db_open_pp.argtypes = [C.c_void_p, C.c_void_p, C.c_char_p, C.c_char_p,
                                C.c_int, C.c_uint, C.c_int]
    db.__db_open_pp.restype = C.c_int
    db.__db_put_pp.argtypes = [C.c_void_p, C.c_void_p, C.POINTER(DBT), C.POINTER(DBT), C.c_uint]
    db.__db_put_pp.restype = C.c_int
    db.__db_close_pp.argtypes = [C.c_void_p, C.c_uint]
    db.__db_close_pp.restype = C.c_int
    db.__db_get_pp.argtypes = [P, P, C.POINTER(DBT), C.POINTER(DBT), U]
    db.__db_get_pp.restype = I
    if rows is not None and os.path.exists(path) and not keep:
        os.unlink(path)
    handle = C.c_void_p()
    assert db.db_create(C.byref(handle), None, 0) == 0
    # DB_CREATE == 0x1; the pin opens its tables the same way
    # (`chewing_large_table2_bdb.cpp:90`, `ngram_bdb.cpp:56`).
    flags = 0x1 if rows is not None else 0x400  # DB_CREATE / DB_RDONLY
    assert db.__db_open_pp(handle, None, path.encode(), None, dbtype, flags, 0o600) == 0
    live = []
    for key, value in rows or ():
        kbuf = C.create_string_buffer(key)
        vbuf = C.create_string_buffer(value)
        live += [kbuf, vbuf]
        kd = DBT(C.cast(kbuf, C.c_void_p), len(key), 0, 0, 0, None, 0)
        vd = DBT(C.cast(vbuf, C.c_void_p), len(value), 0, 0, 0, None, 0)
        assert db.__db_put_pp(handle, None, C.byref(kd), C.byref(vd), 0) == 0
    values = []
    for key in read_keys:
        kbuf = C.create_string_buffer(key)
        kd = DBT(C.cast(kbuf, P), len(key), 0, 0, 0, None, 0)
        vd = DBT()
        status = db.__db_get_pp(handle, None, C.byref(kd), C.byref(vd), 0)
        if status != 0 and allow_missing:
            values.append(None)
            continue
        assert status == 0
        values.append(C.string_at(vd.data, vd.size))
    assert db.__db_close_pp(handle, 0) == 0
    del live
    return values


def _craft_btree(path, keys):
    """A Berkeley DB btree of `key -> empty` records."""
    _craft_db(path, [(key, b'') for key in keys], 1)  # DB_BTREE == 1


def _craft_hash(path, rows):
    """A Berkeley DB hash of `key -> value` records (the pin's user bigram
    layout, `ngram_bdb.cpp:56`)."""
    _craft_db(path, rows, 2)  # DB_HASH == 2


def _write_user_conf(k):
    """A conforming `user.conf` for the cell's system tables.

    The pin writes one at `pinyin_save` / `zhuyin_save` only, but
    `check_format` (`pinyin.cpp:172`, `zhuyin.cpp:126`) wipes the user
    tables when it is missing or stale. `pinyin_init`'s check_format
    rewrites it on every open (so the pinyin cases survive without this);
    `zhuyin_init`'s does not, so a crafted zhuyin index needs the marker
    present before the reopen or it is unlinked."""
    keys = ('binary format version:', 'model data version:', 'database format:')
    values = {}
    with open(os.path.join(k.data, 'table.conf')) as table_conf:
        for line in table_conf:
            for key in keys:
                if line.startswith(key):
                    values[key] = line[len(key):].strip()
    with open(os.path.join(k.user, 'user.conf'), 'w') as user_conf:
        for key in keys:
            user_conf.write(key + values[key] + '\n')
        user_conf.write('open counter:0\n')


def overlong_index_context(k, parse=None, rows=None, options=None):
    """A context whose user dir carries crafted `user_pinyin_index.bin` rows,
    by default the packed first syllable of `parse` and a 17-syllable key that
    extends it. The first init writes the conforming `user.conf`; the crafted
    keys are then added and the profile reopened, so the longer-candidate walk
    reaches them. `rows(first)` overrides the crafted key list — e.g. the exact
    prefix row can be dropped, or the over-long row's trailing byte made odd.
    `options` sets the option word of both the build context and the reopened
    one (e.g. `PINYIN_INCOMPLETE = 8`, so a partial first syllable projects to
    a zero-initial word)."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    if options is not None:
        k.fn('set_options', B, P, U)(ctx, options)
    if parse is None:
        first = b'\x00\x01'
    else:
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
        key = C.c_void_p()
        assert k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(inst, 0, C.byref(key))
        first = C.string_at(key.value, 2)
        k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    if rows is None:
        rows = [first, first * 17]
    else:
        rows = rows(first)
    _craft_btree(os.path.join(k.user, 'user_pinyin_index.bin'), rows)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    if options is not None:
        k.fn('set_options', B, P, U)(ctx, options)
    return ctx


@case('abort-mask-out-overlong-index-key', abort=False)
def _(k):
    ctx = overlong_index_context(k)
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-zhuyin-mask-out-overlong-index-key', mode='zhuyin', abort=False)
def _(k):
    # libzhuyin masks `m_pinyin_table` too (`zhuyin.cpp:763`), the same user
    # chewing table, so the same :529 walk aborts there.
    ctx = overlong_index_context(k)
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-guess-candidates-overlong-index-key', abort=False)
def _(k):
    ctx = overlong_index_context(k, parse=b'ni')
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    # sort 0 clears SORT_WITHOUT_LONGER_CANDIDATE, so the walk runs and
    # reaches the crafted extension (`pinyin.cpp:2292-2293`).
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('abort-guess-candidates-overlong-odd-index-key', abort=False)
def _(k):
    # A 35-byte key is not one upstream writes, but `phrase_length =
    # db_key.size / sizeof(ChewingKey)` is integer division
    # (`chewing_large_table2_bdb.cpp:470`), so the pin reads it as 17 words
    # and its `default: abort()` dies. The subject classifies the complete-word
    # prefix the same way and drops the trailing byte only after.
    ctx = overlong_index_context(
        k, parse=b'ni', rows=lambda first: [first, first * 17 + b'\x07'])
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('abort-mask-out-overlong-odd-index-key', abort=False)
def _(k):
    ctx = overlong_index_context(
        k, rows=lambda first: [first, first * 17 + b'\x07'])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


# A key of no whole word (empty, or one byte) reads as `size / 2 == 0` words
# and `mask_out`'s `switch` has no such case: `default: abort()`
# (`chewing_large_table2_bdb.cpp:529`, the KC twin :499, tkrzw :466).
@case('abort-mask-out-empty-index-key', abort=False, userdir=True)
def _(k):
    ctx = overlong_index_context(k, rows=lambda first: [b'', first])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-mask-out-one-byte-index-key', abort=False, userdir=True)
def _(k):
    ctx = overlong_index_context(k, rows=lambda first: [first, b'\x07'])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-zhuyin-mask-out-empty-index-key', mode='zhuyin', abort=False, userdir=True)
def _(k):
    ctx = overlong_index_context(k, rows=lambda first: [b'', first])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


# The system `pinyin_index.bin` is the same `ChewingLargeTable2` btree, and
# `FacadeChewingTable2::search_suggestion` walks it too
# (`chewing_large_table2_bdb.cpp:282`): a key past 16 words that extends the
# query dies at the same `default: abort()`. The stock index has none, so the
# fixture is a private copy (never a link into the oracle) with one row added
# in its native attached format (KC aborts at :269, tkrzw at :252).
def system_overlong_index_context(k, trailing=b''):
    ctx = k.init()
    assert ctx, 'the first init failed'
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    key = C.c_void_p()
    assert k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(inst, 0, C.byref(key))
    first = C.string_at(key.value, 2)
    k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    system = private_system(k)
    path = os.path.join(system, 'pinyin_index.bin')
    os.unlink(path)  # the link into the oracle's data
    shutil.copyfile(os.path.join(k.data, 'pinyin_index.bin'), path)
    _craft_db(path, [(first * 17 + trailing, b'')], 1, keep=True)
    ctx = k.init(system=system)
    assert ctx, 'the open on the private system dir failed'
    return ctx


@case('abort-guess-candidates-overlong-system-index-key', abort=False, userdir=True)
def _(k):
    ctx = system_overlong_index_context(k)
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('abort-guess-candidates-overlong-odd-system-index-key', abort=False, userdir=True)
def _(k):
    ctx = system_overlong_index_context(k, trailing=b'\x07')
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('guess-after-overlong-index-key-without-prefix-answers-true')
def _(k):
    # The pin's `search_suggestion` probes the exact query key first
    # (`cursorp->c_get(..., DB_SET)`, `chewing_large_table2_bdb.cpp:576`) and
    # answers SEARCH_NONE when it is absent, so an over-long key whose prefix
    # row was never written walks nothing and the abort is unreachable.
    ctx = overlong_index_context(k, parse=b'ni', rows=lambda first: [first * 17])
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('reparse-after-guess-ignores-overlong-index-key')
def _(k):
    # `_prepend_longer_candidates` runs only from `pinyin_guess_candidates`
    # (`pinyin.cpp:2292-2293`); a parse refreshes the cached list without it,
    # so the pin cannot reach the abort from the re-parse.
    ctx = overlong_index_context(k, parse=b'ni')
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'hao')
    guessed = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    reparsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    return {'guess_hao': guessed, 'reparse_ni': reparsed}


@case('abort-guess-candidates-overlong-incomplete-index-key', abort=False)
def _(k):
    # `compute_incomplete_chewing_index` sets only `m_initial`, so the partial
    # `n` projects to its initial and the vowel-initial `an` to the zero
    # initial. The 17-word key extends that two-word projection, so the pin's
    # `default: abort()` dies at the guess.
    ctx = overlong_index_context(
        k,
        parse=b'n',
        options=8,  # PINYIN_INCOMPLETE
        rows=lambda first: [
            first + b'\x00\x00',
            first + b'\x00\x00' + b'\x01\x00' * 15,
        ],
    )
    inst = k.fn('alloc_instance', P, P)(ctx)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b"n'an")
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}
# batch 525-D: crafted `user_bigram.db` rows the pin's bigram walk aborts on.
# A value shorter than a `guint32` dies at `MemoryChunk::get_content<guint32>`
# (`memory_chunk.h:390`, reached by `get_total_freq`); a key that is not four
# bytes dies at `Bigram::get_all_items` (`ngram_bdb.cpp:199`); an item-less
# gram with a residual total dies at `SingleGram::get_length`
# (`ngram.cpp:70`, reached by `_compute_predicted_bigram_candidates`); and a
# gram whose total its items do not cover dies at `get_length` after the mask
# removes every item (`ngram.cpp:70`, reached by `Bigram::mask_out`,
# `ngram_bdb.cpp:243`). The container is written in the running cell's own
# format (`_craft_db`): a Berkeley DB hash on bdb (the pin's own layout,
# `ngram_bdb.cpp:56`), a Kyoto Cabinet snapshot on kc (`ngram_kyotodb.cpp:54`)
# and a tkrzw `HashDBM` file on tkrzw (`ngram_tkrzwdb.cpp:48`), each read by
# that cell's pin through its own `Bigram` twin.
def crafted_bigram_context(k, rows):
    """A context whose user dir carries crafted `user_bigram.db` rows
    (`(key_bytes, value_bytes)` records). The first init writes the
    conforming `user.conf`; the rows are then written and the profile
    reopened."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    _craft_hash(os.path.join(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    return ctx


def predicted_bigram_context(k, phrase, value):
    """A context whose user bigram carries `value` under every
    phrase-table token of `phrase`, so `_compute_prefixes` reaches it. The
    token is read from this side's own table, so the fixture matches the
    pin and the subject alike."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    inst = k.fn('alloc_instance', P, P)(ctx)
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    arr = new(0, 0, 4)
    assert k.fn('lookup_tokens', B, P, S, P)(inst, phrase.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    tokens = list(C.cast(view.data, C.POINTER(U))[:view.len])
    k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    _craft_hash(os.path.join(k.user, 'user_bigram.db'),
                [(token.to_bytes(4, 'little'), value) for token in tokens])
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    return ctx


def predicted_bigram_row_context(k, phrase, successor, count=20):
    """A context whose user bigram lists `successor` after `phrase` with
    `count` (the pin lists a predicted-bigram row from a count of 10 and a
    one- or two-word successor, `pinyin.cpp:2331-2368`). Both tokens are read
    from this side's own table."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    inst = k.fn('alloc_instance', P, P)(ctx)
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    found = []
    for text in (phrase, successor):
        arr = new(0, 0, 4)
        assert k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
        view = C.cast(arr, C.POINTER(Arr)).contents
        found.append(list(C.cast(view.data, C.POINTER(U))[:view.len]))
    k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    value = count.to_bytes(4, 'little') + found[1][0].to_bytes(4, 'little') + count.to_bytes(4, 'little')
    _craft_hash(os.path.join(k.user, 'user_bigram.db'), [(token.to_bytes(4, 'little'), value) for token in found[0]])
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    return ctx


# `pinyin_choose_candidate` takes no predicted row of either kind
# (`pinyin.cpp:2507`); the prefix arm is `abort-choose-candidate-predicted-prefix-row`.
# The user bigram is a Berkeley DB hash here, so only the bdb cell opens it.
@case('abort-choose-candidate-predicted-bigram-row', abort=0, cells=('bdb',))
def _(k):
    ctx = predicted_bigram_row_context(k, '我', '你好')
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, '我'.encode())
    row = row_of(k, inst, 4)
    assert row, 'the planted gram listed no predicted-bigram row'
    return {'ret': k.fn('choose_candidate', I, P, Z, P)(inst, 0, row)}


@case('abort-mask-out-short-bigram-value', abort=False)
def _(k):
    # The pin's `mask_out` loads the gram and `get_total_freq` asserts on
    # the three-byte value (`memory_chunk.h:390`, `ngram.cpp:80`).
    ctx = crafted_bigram_context(k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-zhuyin-mask-out-short-bigram-value', mode='zhuyin', abort=False)
def _(k):
    ctx = crafted_bigram_context(k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('mask-out-short-bigram-value-masked-key', control=True)
def _(k):
    # A short value whose key the mask erases wholesale is never loaded, so
    # the pin erases it and completes (`ngram_bdb.cpp:231-238`); the
    # `get_total_freq` assert is reached only for a key the mask leaves
    # alone. A control: the guard must not over-refuse here, so pin and
    # subject both complete and the parent build (no bigram checks) matches.
    ctx = crafted_bigram_context(k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 1)}


@case('abort-mask-out-non-token-bigram-key', abort=False)
def _(k):
    # The pin's `mask_out` walks the container with `get_all_items`, whose
    # `key.size == sizeof(phrase_token_t)` assert dies on the two-byte key
    # (`ngram_bdb.cpp:199`).
    ctx = crafted_bigram_context(k, [(b'\x00\x01', b'\x07\x00\x00\x00')])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-zhuyin-mask-out-non-token-bigram-key', mode='zhuyin', abort=False)
def _(k):
    ctx = crafted_bigram_context(k, [(b'\x00\x01', b'\x07\x00\x00\x00')])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


# A gram whose `total_freq` is not covered by its items (total 7, one item of
# frequency 0): the mask removes the item (token 0 == value 0) but the key is
# left alone, so `SingleGram::mask_out` leaves a residual total and
# `Bigram::mask_out`'s `get_length` assert dies (`ngram.cpp:70`,
# `ngram_bdb.cpp:243`).
@case('abort-mask-out-residual-bigram-gram', abort=False)
def _(k):
    ctx = crafted_bigram_context(
        k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00\x00' + b'\x00' * 8)])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-zhuyin-mask-out-residual-bigram-gram', mode='zhuyin', abort=False)
def _(k):
    ctx = crafted_bigram_context(
        k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00\x00' + b'\x00' * 8)])
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


@case('abort-begin-bigram-phrases-non-token-key', abort=False)
def _(k):
    # `pinyin_begin_get_bigram_phrases` walks the same container through
    # `get_all_items` (`pinyin.cpp:776-787`), so the two-byte key aborts at
    # `ngram_bdb.cpp:199` before any row is exported.
    ctx = crafted_bigram_context(k, [(b'\x00\x01', b'\x07\x00\x00\x00')])
    return {'ret': bool(k.fn('begin_get_bigram_phrases', P, P)(ctx))}


def issue677_sequence(k):
    """The #677 sequence over a valid bigram row that the mask removes."""
    ctx = crafted_bigram_context(
        k, [(b'\x01\x00\x00\x07', d18_gram(100, 0x01000001, 100))])

    def add(text, reading):
        it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        assert it, 'the import iterator'
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, text.encode(), reading.encode(), 100)
        k.fn('end_add_phrases', None, P)(it)

    def save():
        return bool(k.fn('save', B, P)(ctx))

    reading = (lambda text: text) if k.mode == 'pinyin' else (lambda text: text)
    add('达达', "da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ')
    ret1 = save()
    mask = bool(k.fn('mask_out', B, P, U, U)(ctx, 0x0f000000, 0x07000000))
    ret2 = save()
    path = Path(k.user, 'user_bigram.db')
    key = b'\x01\x00\x00\x07'
    after_mask = d18_bigram_db(k, path, read_keys=[key], allow_missing=True)[0]
    add('你你', "ni3'ni3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄋㄧˇ')
    ret3 = save()
    ret4 = save()
    add('好好', "hao3'hao3" if k.mode == 'pinyin' else 'ㄏㄠˇ ㄏㄠˇ')
    ret5 = save()
    result = {
        'ret1': ret1, 'mask': mask, 'ret2': ret2, 'ret3': ret3,
        'ret4': ret4, 'ret5': ret5,
        'files': sorted(os.listdir(k.user)),
        'removed_key_after_mask': after_mask.hex() if after_mask is not None else None,
    }
    # The raw DBM contract is the backend's own `Get` result. On a
    # library that retains a removed row, its value remains observable;
    # on a library that deletes it, the result is absent. Compare that
    # value, the return sequence and the user-file inventory in every cell.
    final = d18_bigram_db(k, path, read_keys=[key], allow_missing=True)[0]
    result['removed_key_final'] = final.hex() if final is not None else None
    return result


for _issue677_mode in ('pinyin', 'zhuyin'):
    case('issue677-mask-save-sequence-' + _issue677_mode, mode=_issue677_mode)(issue677_sequence)


# #762: the pin hands a new user phrase `range.m_range_end` of the user
# sub-index (`pinyin.cpp:589-592`), and `SubPhraseIndex::get_range` trims
# trailing zero offsets (`phrase_index.cpp:624-650`), so a mask that empties
# the top of the range frees those tokens for the next import.
TOKEN_REUSE_PHRASES = (
    ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'), ('龖龖', "ta4'ta4", 'ㄊㄚˋ ㄊㄚˋ'),
    ('靐靐', "bing3'bing3", 'ㄅㄧㄥˇ ㄅㄧㄥˇ'), ('鱻鱻', "xian1'xian1", 'ㄒㄧㄢ ㄒㄧㄢ'),
    ('麤麤', "cu1'cu1", 'ㄘㄨ ㄘㄨ'), ('厵厵', "yuan2'yuan2", 'ㄩㄢˊ ㄩㄢˊ'),
)


def token_reuse_sequence(k, steps, bigram_rows=None, library=7):
    """Runs `steps` over the user library and reports what a caller can see:
    each step's return, the user token of every phrase imported so far, and
    the user files left behind. `('add', n)` imports phrase n, `('save',)`,
    `('mask', mask, value)` and `('reopen',)` are the facade calls.
    `bigram_rows` seeds `user_bigram.db` first, with rows that name the token
    the first import will take."""
    ctx = crafted_bigram_context(k, bigram_rows) if bigram_rows else k.ctx
    column = 1 if k.mode == 'pinyin' else 2
    out, seen = [], []

    def tokens():
        inst = k.fn('alloc_instance', P, P)(ctx)
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        found = {}
        for n in seen:
            arr = new(0, 0, 4)
            k.fn('lookup_tokens', B, P, S, P)(inst, TOKEN_REUSE_PHRASES[n][0].encode(), arr)
            view = C.cast(arr, C.POINTER(Arr)).contents
            found[TOKEN_REUSE_PHRASES[n][0]] = [hex(t) for t in C.cast(view.data, C.POINTER(U))[:view.len]]
        k.fn('free_instance', None, P)(inst)
        return found

    for step in steps:
        if step[0] == 'add':
            it = k.fn('begin_add_phrases', P, P, U)(ctx, library)
            assert it, 'the import iterator'
            text, reading = TOKEN_REUSE_PHRASES[step[1]][0], TOKEN_REUSE_PHRASES[step[1]][column]
            out.append(['add', bool(k.fn('iterator_add_phrase', B, P, S, S, I)(it, text.encode(), reading.encode(), 100))])
            k.fn('end_add_phrases', None, P)(it)
            seen.append(step[1])
        elif step[0] == 'save':
            out.append(['save', bool(k.fn('save', B, P)(ctx))])
        elif step[0] == 'mask':
            out.append(['mask', bool(k.fn('mask_out', B, P, U, U)(ctx, step[1], step[2]))])
        elif step[0] == 'reopen':
            k.fn('fini', None, P)(ctx)
            ctx = k.init()
            assert ctx, 'the reopen failed'
        out.append(tokens())
    return {'steps': out, 'files': token_reuse_files(k)}


USER_INDEX_FILES = ('user_phrase_index.bin', 'user_pinyin_index.bin')


def index_file_rows(path):
    """Every record of a user index container, key by key, in the running
    cell's own format: `[key hex, value hex]` in ascending key order, so a
    key whose value is empty is a row of its own. A Berkeley DB btree is read
    through `db_dump` (its page headers carry a random file id and an LSN),
    a Kyoto Cabinet snapshot (`chewing_large_table2_kyotodb.cpp:106`,
    `phrase_large_table3_kyotodb.cpp:109`) is loaded into an on-memory tree
    and walked with a cursor, and a tkrzw `TreeDBM`
    (`chewing_large_table2_tkrzwdb.cpp:83`, `phrase_large_table3_tkrzwdb.cpp:83`)
    is opened read-only and walked with an iterator."""
    rows = []
    if CELL == 'kc':
        kc = C.CDLL('libkyotocabinet.so.16')
        kc.kcdbnew.restype = P
        kc.kcdbdel.argtypes = [P]
        kc.kcdbopen.argtypes = [P, S, C.c_uint32]
        kc.kcdbloadsnap.argtypes = [P, S]
        kc.kcdbcursor.restype, kc.kcdbcursor.argtypes = P, [P]
        kc.kccurjump.argtypes = [P]
        kc.kccurget.restype = P
        kc.kccurget.argtypes = [P, C.POINTER(Z), C.POINTER(P), C.POINTER(Z), I]
        kc.kccurdel.argtypes = [P]
        kc.kcfree.argtypes = [P]
        kc.kcdbclose.argtypes = [P]
        db = kc.kcdbnew()
        assert db and kc.kcdbopen(db, b'+', (1 << 1) | (1 << 2))  # KCOWRITER | KCOCREATE
        assert kc.kcdbloadsnap(db, str(path).encode())
        cur = kc.kcdbcursor(db)
        if kc.kccurjump(cur):
            while True:
                ksiz, vbuf, vsiz = Z(), P(), Z()
                kbuf = kc.kccurget(cur, C.byref(ksiz), C.byref(vbuf), C.byref(vsiz), 1)
                if not kbuf:
                    break
                rows.append([C.string_at(kbuf, ksiz.value).hex(), C.string_at(vbuf, vsiz.value).hex()])
                kc.kcfree(kbuf)
        kc.kccurdel(cur)
        assert kc.kcdbclose(db)
        kc.kcdbdel(db)
    elif CELL == 'tkrzw':
        tk = C.CDLL('libtkrzw.so.1')
        libc = C.CDLL(None)
        libc.free.argtypes = [P]
        tk.tkrzw_dbm_open.restype, tk.tkrzw_dbm_open.argtypes = P, [S, B, S]
        tk.tkrzw_dbm_make_iterator.restype, tk.tkrzw_dbm_make_iterator.argtypes = P, [P]
        tk.tkrzw_dbm_iter_first.restype, tk.tkrzw_dbm_iter_first.argtypes = B, [P]
        tk.tkrzw_dbm_iter_get.restype = B
        tk.tkrzw_dbm_iter_get.argtypes = [P, C.POINTER(P), C.POINTER(C.c_int32), C.POINTER(P), C.POINTER(C.c_int32)]
        tk.tkrzw_dbm_iter_next.restype, tk.tkrzw_dbm_iter_next.argtypes = B, [P]
        tk.tkrzw_dbm_iter_free.argtypes = [P]
        tk.tkrzw_dbm_close.restype, tk.tkrzw_dbm_close.argtypes = B, [P]
        db = tk.tkrzw_dbm_open(str(path).encode(), False, b'dbm=TreeDBM')
        assert db
        it = tk.tkrzw_dbm_make_iterator(db)
        assert tk.tkrzw_dbm_iter_first(it)
        while True:
            kptr, ksiz, vptr, vsiz = P(), C.c_int32(), P(), C.c_int32()
            if not tk.tkrzw_dbm_iter_get(it, C.byref(kptr), C.byref(ksiz), C.byref(vptr), C.byref(vsiz)):
                break
            rows.append([C.string_at(kptr, ksiz.value).hex(), C.string_at(vptr, vsiz.value).hex()])
            libc.free(kptr)
            libc.free(vptr)
            tk.tkrzw_dbm_iter_next(it)
        tk.tkrzw_dbm_iter_free(it)
        assert tk.tkrzw_dbm_close(db)
    else:
        dump = subprocess.run(['db5.3_dump', str(path)], check=True, capture_output=True).stdout.decode()
        body = dump.split('DATA=END')[0].split('HEADER=END\n')[1].split('\n')[:-1]
        rows = [[body[i].strip(), body[i + 1].strip()] for i in range(0, len(body), 2)]
    return sorted(rows)


def zero_item_padding(key_hex, value_hex):
    """A pinyin index value with the padding of every item zeroed. An odd key
    length leaves two uninitialised bytes after the keys of each `4 + 2L` byte
    item, which upstream copies off the stack (upstream-divergences.md, "The
    pinyin index DBMs carry uninitialized struct padding", class (b)); the
    tails a mask walk carries between records copy them along."""
    length = len(key_hex) // 4
    if length % 2 == 0 or not value_hex:
        return value_hex
    stride = 4 + 2 * length + 2
    raw = bytearray(bytes.fromhex(value_hex))
    for start in range(0, len(raw) - stride + 1, stride):
        raw[start + stride - 2:start + stride] = b'\0\0'
    return raw.hex()


def token_reuse_files(k):
    """The user files a save leaves, by content. The two user index
    containers are compared record by record, a key whose value is empty
    included ([`index_file_rows`]); a Berkeley DB user bigram is read back
    through `db_dump` and only its non-empty rows are compared (on the other
    backends only that the bigram container exists), `user.bin` is compared
    whole except for the 4 timestamp bytes of its header, and every other file
    is compared as written."""
    files = {}
    for name in sorted(os.listdir(k.user)):
        path = Path(k.user, name)
        if not path.is_file() or name.endswith('.tmp'):
            continue
        raw = path.read_bytes()
        if name in USER_INDEX_FILES:
            rows = index_file_rows(path)
            if name == 'user_pinyin_index.bin':
                rows = [[key, zero_item_padding(key, value)] for key, value in rows]
            files[name] = rows
        elif CELL != 'bdb' and name == 'user_bigram.db':
            files[name] = len(raw) > 0  # another backend's container: only its presence is compared
        elif raw[12:16] in (b'\x62\x31\x05\x00', b'\x61\x15\x06\x00'):
            dump = subprocess.run(['db5.3_dump', str(path)], check=True, capture_output=True).stdout.decode()
            body = dump.split('DATA=END')[0].split('HEADER=END\n')[1].split('\n')[:-1]
            files[name] = [[body[i].strip(), body[i + 1].strip()] for i in range(0, len(body), 2) if body[i + 1].strip()]
        elif name in ('user.bin', 'addon.bin', 'network.bin'):
            files[name] = (raw[:4] + b'\0\0\0\0' + raw[8:]).hex()
        else:
            files[name] = raw.hex()
    return files


USER_ALL = (0x0f000000, 0x07000000)
TOKEN_REUSE_SEQUENCES = {
    # name: (control, steps). A control holds on the parent too: it has no
    # mask, or reopens first (the load seeds the cursor from the saved items,
    # which already reproduced the pin). A mask below the top of the range
    # hands out the token the parent did, but the save after it writes the
    # masked phrase's index keys back empty (#784).
    'no-mask': (True, [('add', 0), ('add', 1), ('save',), ('add', 2), ('save',)]),
    'mask-all': (False, [('add', 0), ('save',), ('mask',) + USER_ALL, ('save',), ('add', 1), ('save',)]),
    'mask-highest': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000003),
                             ('save',), ('add', 3), ('save',)]),
    'mask-middle': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000002),
                            ('save',), ('add', 3), ('save',)]),
    'mask-twice': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000003),
                           ('mask', 0xffffffff, 0x07000002), ('add', 3), ('add', 4), ('save',)]),
    'mask-import-mask': (False, [('add', 0), ('add', 1), ('save',), ('mask', 0xffffffff, 0x07000002), ('add', 2),
                                 ('mask', 0xffffffff, 0x07000002), ('add', 3), ('save',)]),
    'mask-reopen': (True, [('add', 0), ('add', 1), ('save',), ('mask', 0xffffffff, 0x07000002), ('save',),
                            ('reopen',), ('add', 2), ('save',)]),
    'mask-unsaved-reopen': (True, [('add', 0), ('add', 1), ('save',), ('mask', 0xffffffff, 0x07000002),
                                    ('reopen',), ('add', 2), ('save',)]),
    'mask-reopen-middle': (True, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000002),
                                  ('save',), ('reopen',), ('add', 3), ('save',)]),
}
# A user bigram row under the token and another that lists it as a successor:
# the pin's `mask_out` drops both, so the reused token starts clean.
TOKEN_REUSE_BIGRAM_ROWS = [
    (b'\x01\x00\x00\x07', b''.join(n.to_bytes(4, 'little') for n in (100, 0x01000001, 100))),
    (b'\x01\x00\x00\x01', b''.join(n.to_bytes(4, 'little') for n in (100, 0x07000001, 100))),
]
for _mode in ('pinyin', 'zhuyin'):
    for _name, (_control, _steps) in TOKEN_REUSE_SEQUENCES.items():
        case('issue762-token-reuse-%s-%s' % (_name, _mode), mode=_mode, control=_control)(
            lambda k, steps=_steps: token_reuse_sequence(k, steps))
    case('issue762-token-reuse-stale-bigram-%s' % _mode, mode=_mode, cells=('bdb',))(
        lambda k: token_reuse_sequence(k, TOKEN_REUSE_SEQUENCES['mask-all'][1], TOKEN_REUSE_BIGRAM_ROWS))
    case('issue762-token-reuse-stale-bigram-no-mask-%s' % _mode, mode=_mode, control=True, cells=('bdb',))(
        lambda k: token_reuse_sequence(k, TOKEN_REUSE_SEQUENCES['no-mask'][1], TOKEN_REUSE_BIGRAM_ROWS))


# The pin takes the next token from the trimmed range end for every index
# (`_add_phrase`, `pinyin.cpp:586-592`) and for addon promotion
# (`pinyin.cpp:2541-2544`, which has no `token++` over a zero low word), so the
# ADDON_DICTIONARY sub-index (5) frees its top the same way the user one does.
ADDON_ALL = (0x0f000000, 0x05000000)
ADDON_IMPORT_SEQUENCES = {
    'no-mask': (True, [('add', 0), ('add', 1), ('add', 2), ('save',), ('add', 3), ('save',)]),
    'mask-highest': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x05000003),
                             ('add', 3), ('save',)]),
    'mask-middle': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x05000002),
                            ('add', 3), ('save',)]),
}
for _mode in ('pinyin', 'zhuyin'):
    for _name, (_control, _steps) in ADDON_IMPORT_SEQUENCES.items():
        case('issue762-addon-import-%s-%s' % (_name, _mode), mode=_mode, control=_control)(
            lambda k, steps=_steps: token_reuse_sequence(k, steps, library=5))

ADDON_PROMOTE_INPUTS = (('shuxue', '书学'), ('wuji', '武技'), ('wusheng', '武生'), ('wuxi', '武戏'))


def addon_promote_sequence(k, mask_value):
    """Promotes the addon candidates in turn (`pinyin_choose_candidate`); with
    `mask_value`, masks that token after the third and before the fourth, with
    no reopen between."""
    ctx = k.ctx
    for index in (4, 5):
        assert k.fn('load_addon_phrase_library', B, P, C.c_ubyte)(ctx, index)
    out, texts = [], []

    def promote(pinyin):
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, pinyin.encode())
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        row = row_of(k, inst, 6)
        assert row, 'no addon candidate for ' + pinyin
        out.append(['promote', pinyin, k.fn('choose_candidate', I, P, Z, P)(inst, 0, row)])
        k.fn('free_instance', None, P)(inst)

    def tokens():
        inst = k.fn('alloc_instance', P, P)(ctx)
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        found = {}
        for text in texts:
            arr = new(0, 0, 4)
            k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
            view = C.cast(arr, C.POINTER(Arr)).contents
            found[text] = [hex(t) for t in C.cast(view.data, C.POINTER(U))[:view.len]]
        k.fn('free_instance', None, P)(inst)
        return found

    for pinyin, text in ADDON_PROMOTE_INPUTS[:3]:
        promote(pinyin)
        texts.append(text)
        out.append(tokens())
    out.append(['save', bool(k.fn('save', B, P)(ctx))])
    if mask_value is not None:
        out.append(['mask', bool(k.fn('mask_out', B, P, U, U)(ctx, 0xffffffff, mask_value))])
        out.append(tokens())
    promote(ADDON_PROMOTE_INPUTS[3][0])
    texts.append(ADDON_PROMOTE_INPUTS[3][1])
    out.append(tokens())
    out.append(['save', bool(k.fn('save', B, P)(ctx))])
    return {'steps': out, 'files': token_reuse_files(k)}


case('issue762-addon-promote-no-mask', control=True)(lambda k: addon_promote_sequence(k, None))
case('issue762-addon-promote-mask-highest')(lambda k: addon_promote_sequence(k, 0x05000003))
case('issue762-addon-promote-mask-middle', control=True)(lambda k: addon_promote_sequence(k, 0x05000002))


# #784: the pin never deletes a key from its user index tables. `mask_out`
# rewrites every record in place (`phrase_large_table3_bdb.cpp:391-405` and
# `chewing_large_table2_bdb.cpp:488-537` put with `DB_CURRENT`; the kc visitors
# return the masked value, `phrase_large_table3_kyotodb.cpp:361-370` and
# `chewing_large_table2_kyotodb.cpp:459-506`; the tkrzw processors return it,
# `phrase_large_table3_tkrzwdb.cpp:288-296` and
# `chewing_large_table2_tkrzwdb.cpp:429-473`), and `pinyin_remove_user_candidate`'s
# `remove_index` calls store the emptied entry back
# (`phrase_large_table3_bdb.cpp:353-368`, `chewing_large_table2_bdb.cpp:417-432`,
# `phrase_large_table3_kyotodb.cpp:334-345`, `phrase_large_table3_tkrzwdb.cpp:265-274`).
# A key whose last record left stays with an empty value, and every later save
# copies it. Each step that saves records both index files key by key.
INDEX_KEY_READINGS = {
    # The facade's own parse input for each TOKEN_REUSE_PHRASES reading.
    'pinyin': ('dada', 'tata', 'bingbing', 'xianxian', 'cucu', 'yuanyuan'),
    'zhuyin': ('286286', 'w84w84', '1u/31u/3', 'vu0 vu0 ', 'hj hj ', 'm06m06'),
}


def index_key_sequence(k, steps, phrases=None, readings=None):
    """Runs `steps` over the user library and records each save's index files.
    `('add', n)`, `('save',)`, `('mask', mask, value)`, `('reopen',)` as in
    `token_reuse_sequence`; `('remove', n)` removes phrase n through its user
    candidate (`pinyin_remove_user_candidate`); `('lookup', n)` records what a
    fresh instance shows for phrase n: the candidates of its reading, the
    tokens of its text and of its first character, and (pinyin) the predicted
    rows after its first character. `('guess', pinyin, zhuyin)` parses and
    guesses that input. `phrases` and `readings` default to
    `TOKEN_REUSE_PHRASES` and `INDEX_KEY_READINGS`."""
    ctx = k.ctx
    phrases = TOKEN_REUSE_PHRASES if phrases is None else phrases
    readings = INDEX_KEY_READINGS if readings is None else readings
    column = 1 if k.mode == 'pinyin' else 2
    parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
    out = []

    def tokens(inst, text):
        arr = k.glib.g_array_new(0, 0, 4)
        ret = k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
        view = C.cast(arr, C.POINTER(Arr)).contents
        return [ret, [hex(t) for t in C.cast(view.data, C.POINTER(U))[:view.len]]]

    def candidates(inst, limit=12):
        count = U()
        k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
        rows = []
        for i in range(min(count.value, limit)):
            cand, kind, text = P(), I(), S()
            k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
            rows.append([kind.value, text.value.decode()])
        return [count.value, rows]

    def guess(inst, n):
        reading = (n if isinstance(n, str) else readings[k.mode][n]).encode()
        k.fn(parse, Z, P, S)(inst, reading)
        if k.mode == 'pinyin':
            k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        else:
            k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)

    k.glib.g_array_new.restype, k.glib.g_array_new.argtypes = P, [I, I, U]
    for step in steps:
        if step[0] == 'add':
            it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
            assert it, 'the import iterator'
            text, reading = phrases[step[1]][0], phrases[step[1]][column]
            out.append(['add', bool(k.fn('iterator_add_phrase', B, P, S, S, I)(it, text.encode(), reading.encode(), 100))])
            k.fn('end_add_phrases', None, P)(it)
        elif step[0] == 'save':
            out.append(['save', bool(k.fn('save', B, P)(ctx)), token_reuse_files(k)])
        elif step[0] == 'mask':
            out.append(['mask', bool(k.fn('mask_out', B, P, U, U)(ctx, step[1], step[2]))])
        elif step[0] == 'reopen':
            k.fn('fini', None, P)(ctx)
            ctx = k.init()
            assert ctx, 'the reopen failed'
        elif step[0] in ('unload', 'load'):
            out.append([step[0], bool(k.fn(step[0] + '_phrase_library', B, P, U)(ctx, 7))])
        elif step[0] == 'remove':
            inst = k.fn('alloc_instance', P, P)(ctx)
            guess(inst, step[1])
            rows = _user_candidate_rows(k, inst)
            assert rows, 'no user candidate to remove'
            out.append(['remove', bool(k.fn('remove_user_candidate', B, P, P)(inst, rows[0]))])
            k.fn('free_instance', None, P)(inst)
        elif step[0] == 'guess':
            inst = k.fn('alloc_instance', P, P)(ctx)
            guess(inst, step[1] if k.mode == 'pinyin' else step[2])
            out.append(['guess', step[1], candidates(inst)])
            k.fn('free_instance', None, P)(inst)
        elif step[0] == 'lookup':
            text = phrases[step[1]][0]
            inst = k.fn('alloc_instance', P, P)(ctx)
            guess(inst, step[1])
            seen = {'candidates': candidates(inst), 'tokens': tokens(inst, text)}
            # #785: on kc and tkrzw a partial mask leaves the previous record's
            # post-mask tail in the first character's phrase-table key, and this
            # lookup returns it; bdb is the control that carries nothing.
            seen['prefix tokens'] = tokens(inst, text[0])
            if k.mode == 'pinyin':
                seen['predicted'] = predicted_rows(k, inst, text[0])
            out.append(['lookup', text, seen])
            k.fn('free_instance', None, P)(inst)
    return {'steps': out}


MIDDLE, HIGHEST = (0xffffffff, 0x07000002), (0xffffffff, 0x07000003)
SAVED = [('add', 0), ('add', 1), ('add', 2), ('save',)]
INDEX_KEY_SEQUENCES = {
    # name: (control, steps). A control holds on the parent too: no key is
    # emptied, or no save writes the files after one is. Neither `mask_out`
    # nor `remove_user_candidate` marks the context modified, so a save right
    # after one answers false and writes nothing, at the pin and here alike;
    # the import after it is what makes the next save write the tables.
    'no-mask': (True, SAVED + [('add', 3), ('save',)]),
    'mask-save': (True, SAVED + [('mask',) + MIDDLE, ('save',)]),
    'mask-middle': (False, SAVED + [('mask',) + MIDDLE, ('add', 3), ('save',)]),
    # The highest token is handed out again (#762), under another text.
    'mask-highest': (False, SAVED + [('mask',) + HIGHEST, ('add', 3), ('save',)]),
    'mask-middle-highest': (False, SAVED + [('mask',) + MIDDLE, ('mask',) + HIGHEST, ('add', 3), ('save',)]),
    'mask-unsaved': (False, [('add', 0), ('add', 1), ('add', 2), ('mask',) + MIDDLE, ('save',)]),
    'mask-all': (False, [('add', 0), ('add', 1), ('save',), ('mask',) + USER_ALL, ('add', 2), ('save',)]),
    # The emptied keys come back from the file: the next session's save keeps
    # them, and the masked phrase imported again fills its own keys.
    'mask-reopen': (False, SAVED + [('mask',) + MIDDLE, ('add', 3), ('save',), ('reopen',), ('add', 4),
                                    ('save',), ('reopen',), ('add', 1), ('save',)]),
    # What a fresh instance shows after the mask: in the masking session, and
    # after a reopen that loaded the emptied keys from the file.
    'mask-lookup': (True, SAVED + [('mask',) + MIDDLE, ('lookup', 1), ('lookup', 0)]),
    'mask-reopen-lookup': (False, SAVED + [('mask',) + MIDDLE, ('add', 3), ('save',), ('reopen',),
                                           ('lookup', 1), ('lookup', 0)]),
}
for _mode in ('pinyin', 'zhuyin'):
    for _name, (_control, _steps) in INDEX_KEY_SEQUENCES.items():
        case('issue784-index-keys-%s-%s' % (_name, _mode), mode=_mode, control=_control)(
            lambda k, steps=_steps: index_key_sequence(k, steps))
# `pinyin_remove_user_candidate` (the zhuyin facade has none).
REMOVE_SEQUENCES = {
    'remove-save': (True, SAVED + [('remove', 1), ('save',)]),
    'remove-middle': (False, SAVED + [('remove', 1), ('add', 3), ('save',)]),
    'remove-unsaved': (False, [('add', 0), ('add', 1), ('add', 2), ('remove', 2), ('save',)]),
    'remove-reopen': (False, [('add', 0), ('add', 1), ('save',), ('remove', 0), ('add', 2), ('save',),
                              ('reopen',), ('add', 3), ('save',)]),
    'remove-lookup': (True, [('add', 0), ('add', 1), ('save',), ('remove', 1), ('lookup', 1), ('lookup', 0)]),
    'remove-reopen-lookup': (False, [('add', 0), ('add', 1), ('save',), ('remove', 1), ('add', 2), ('save',),
                                     ('reopen',), ('lookup', 1), ('lookup', 0)]),
}
for _name, (_control, _steps) in REMOVE_SEQUENCES.items():
    case('issue784-index-keys-%s' % _name, control=_control)(
        lambda k, steps=_steps: index_key_sequence(k, steps))


# #785: on kc and tkrzw the user pinyin table's mask walk starts every key
# length from the table entry the last operation of that length left. Two
# phrases share the initials `d d`, so that record holds two items where the
# others hold one; a search hit or an `add_index` on it leaves both in the
# entry, and the first record the walk meets in that length picks up the
# second as its tail. The single-syllable phrase gives length 1 a record
# that the empty prefix markers after it inherit whole.
TAIL_PHRASES = (
    ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'), ('龖龖', "ta4'ta4", 'ㄊㄚˋ ㄊㄚˋ'),
    ('靐靐', "bing3'bing3", 'ㄅㄧㄥˇ ㄅㄧㄥˇ'), ('鬥鬥', "dou4'dou4", 'ㄉㄡˋ ㄉㄡˋ'),
    ('龘', 'da2', 'ㄉㄚˊ'), ('鱻鱻', "xian1'xian1", 'ㄒㄧㄢ ㄒㄧㄢ'),
    ('龗龗', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'),
)
TAIL_READINGS = {
    'pinyin': ('dada', 'tata', 'bingbing', 'doudou', 'da', 'xianxian', 'dada'),
    'zhuyin': ('286286', 'w84w84', '1u/31u/3', '2.42.4', '286', 'vu0 vu0 ', '286286'),
}
DD, D = ('guess', 'dada', '286286'), ('guess', 'da', '286')
SHARED = [('add', 0), ('add', 6), ('add', 3), ('add', 1), ('add', 2)]
# A mask that leaves no item behind, and a lookup that reads no tail, hold on
# the parent as well.
TAIL_CONTROLS = ('mask-all', 'lookup')
MASK_TA, MASK_DA2 = ('mask', 0xffffffff, 0x07000004), ('mask', 0xffffffff, 0x07000002)
TAIL_SEQUENCES = {
    # The add of the second `da da` item leaves that record in the length-2 entry.
    'add': SHARED + [MASK_TA, ('save',)],
    # A search hit on the record does the same.
    'hit': SHARED + [('save',), DD, MASK_TA, ('add', 5), ('save',)],
    'hit-shrink': SHARED + [('save',), DD, MASK_DA2, ('add', 5), ('save',)],
    # A suggestion walk empties the entries of the longer lengths.
    'suggestion': SHARED + [DD, D, MASK_TA, ('save',)],
    # The entry the first walk leaves is where the second starts.
    'mask-twice': SHARED + [DD, MASK_TA, MASK_DA2, ('save',)],
    # A hit on a record that carries a tail.
    'hit-tailed': SHARED + [DD, MASK_DA2, DD, MASK_TA, ('save',)],
    # One guess hits two different length-2 records, so the last hit the
    # engine makes is the one the walk starts from.
    'hit-order': SHARED + [('save',), ('guess', 'dadatata', '286286w84w84'), MASK_TA, ('add', 5),
                           ('save',)],
    'hit-order-reverse': SHARED + [('save',), ('guess', 'tatadada', 'w84w84286286'), MASK_TA, ('add', 5),
                                   ('save',)],
    # A removal leaves the edited record in the entry.
    'remove': SHARED + [('remove', 3), MASK_TA, ('save',)],
    # Length 1: the prefix markers after the non-empty record.
    'markers': [('add', 4), ('add', 0), ('add', 1), ('add', 2), MASK_TA, ('save',)],
    'markers-hit': [('add', 4), ('add', 0), ('add', 1), ('add', 2), ('save',),
                    ('guess', 'da', '286'), MASK_TA, ('add', 5), ('save',)],
    # The tails come back from the file.
    'reopen': SHARED + [DD, MASK_TA, ('save',), ('reopen',), ('add', 5), ('save',)],
    'reopen-mask': SHARED + [DD, MASK_TA, ('save',), ('reopen',), MASK_DA2, ('add', 5), ('save',)],
    # What a fresh instance shows over the tailed records.
    'lookup': SHARED + [DD, MASK_DA2, ('lookup', 0), ('lookup', 3)],
    # A mask that removes nothing still rewrites every record.
    'mask-none': SHARED + [DD, ('mask', 0xffffffff, 0x07000009), ('save',)],
    'mask-all': SHARED + [DD, ('mask',) + USER_ALL, ('add', 5), ('save',)],
}
for _mode in ('pinyin', 'zhuyin'):
    for _name, _steps in TAIL_SEQUENCES.items():
        if _mode == 'zhuyin' and _name.startswith('remove'):
            continue  # the zhuyin facade has no `remove_user_candidate`
        case('issue785-pinyin-tails-%s-%s' % (_name, _mode), mode=_mode, control=_name in TAIL_CONTROLS)(
            lambda k, steps=_steps: index_key_sequence(k, steps, TAIL_PHRASES, TAIL_READINGS))


# #814 meets #785: unsaved imports leave their index entries behind a library
# 7 unload and reload; the item that takes the first token again sits among
# them in the `da da` record (two items, a tail in the mask walk), and the
# mask removes an entry no item reading backs. The zhuyin facade only: the
# pinyin facade never unloads library 7.
case('issue785-pinyin-tails-kept-entry-zhuyin', mode='zhuyin', cells=('kc', 'tkrzw'))(
    lambda k: index_key_sequence(
        k, SHARED + [DD, ('unload',), ('load',), ('add', 5), MASK_DA2, ('save',)],
        TAIL_PHRASES, TAIL_READINGS))


@case('abort-guess-predicted-short-bigram-value', abort=False)
def _(k):
    # `_compute_predicted_bigram_candidates` merges the three-byte gram and
    # `get_total_freq` asserts (`memory_chunk.h:390`, `pinyin.cpp:2330`).
    ctx = predicted_bigram_context(k, '我', b'\x07\x00\x00')
    inst = k.fn('alloc_instance', P, P)(ctx)
    return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


@case('abort-guess-predicted-empty-bigram-gram', abort=False)
def _(k):
    # An item-less gram with a residual total merges to `total_freq != 0`
    # and `SingleGram::get_length` asserts (`ngram.cpp:70`,
    # `pinyin.cpp:2332`).
    ctx = predicted_bigram_context(k, '我', b'\x07\x00\x00\x00')
    inst = k.fn('alloc_instance', P, P)(ctx)
    return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


# `_compute_predicted_bigram_candidates` loads only the resolved prefixes'
# own grams (`pinyin.cpp:2322-2330`), so a faulty row under a token the
# prediction never resolves to is never read: the call completes. The
# `我` prefixes do not include `你`'s token, so these are controls for the
# two faults above — the prefix-scoped guard must not over-refuse.
@case('guess-predicted-unrelated-short-bigram-value', control=True)
def _(k):
    ctx = predicted_bigram_context(k, '你', b'\x07\x00\x00')
    inst = k.fn('alloc_instance', P, P)(ctx)
    return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


@case('guess-predicted-unrelated-empty-bigram-gram', control=True)
def _(k):
    ctx = predicted_bigram_context(k, '你', b'\x07\x00\x00\x00')
    inst = k.fn('alloc_instance', P, P)(ctx)
    return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


# batch2 group 12e: a tone digit on an initial-only key (PR 12e, #525, row 4)
TONE_INCOMPLETE = (1 << 5) | (1 << 3)


def toned_initial(k, text=b'n4'):
    k.fn('set_options', B, P, U)(k.ctx, TONE_INCOMPLETE)
    inst = k.alloc()
    return inst, k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)


@case('abort-is-incomplete-toned-initial', abort=False)
def _(k):
    k.fn('set_options', B, P, U)(k.ctx, TONE_INCOMPLETE)
    inst = k.alloc()
    key = C.c_uint16(0)
    parsed = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(inst, b'n4', C.byref(key))
    return {'ret': k.fn('get_pinyin_is_incomplete', B, P, C.POINTER(C.c_uint16))(inst, C.byref(key))}


@case('abort-guess-candidates-toned-initial-before-offset', abort=False)
def _(k):
    # The toned key sits before the lookup offset, outside the window the
    # search walks: the pin dies all the same (review of #662 expected it not
    # to), so the guard looks at the whole matrix.
    inst, _ = toned_initial(k, b'n4ni')
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 2, 0)}


@case('abort-guess-candidates-toned-initial', abort=False)
def _(k):
    inst, _ = toned_initial(k)
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


@case('abort-guess-sentence-toned-initial', abort=False)
def _(k):
    inst, _ = toned_initial(k)
    return {'ret': k.fn('guess_sentence', B, P)(inst)}


@case('abort-guess-sentence-toned-initial-after-key', abort=False)
def _(k):
    inst, _ = toned_initial(k, b'nihaon4')
    return {'ret': k.fn('guess_sentence', B, P)(inst)}


# The keys the guard leaves alone: a toned complete key, an untoned initial.
@case('toned-initial-neighbours', control=True)
def _(k):
    out = {}
    k.fn('set_options', B, P, U)(k.ctx, TONE_INCOMPLETE)
    for text in (b'ni4', b'n', b'nin', b'ni4hao'):
        inst = k.alloc()
        n = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
        out[text.decode()] = [n, k.fn('guess_sentence', B, P)(inst),
                              k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)]
    inst = k.alloc()
    for text in (b'n', b'ni4', b'ni'):
        key = C.c_uint16(0)
        parsed = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(inst, text, C.byref(key))
        out['incomplete ' + text.decode()] = [parsed, key.value,
                                              k.fn('get_pinyin_is_incomplete', B, P, C.POINTER(C.c_uint16))(inst, C.byref(key))]
    return out


# batch2 group 12f: the zhuyin twins (PR 12f, #525)
for _name, _call in (
        ('chewing-scheme-0', ctx_call('set_chewing_scheme', B, (I, 0))),
        ('chewing-scheme-7', ctx_call('set_chewing_scheme', B, (I, 7))),
        ('chewing-scheme-10', ctx_call('set_chewing_scheme', B, (I, 10))),
        ('chewing-scheme-minus-1', ctx_call('set_chewing_scheme', B, (I, -1))),
        ('full-pinyin-scheme-0', ctx_call('set_full_pinyin_scheme', B, (I, 0))),
        ('full-pinyin-scheme-4', ctx_call('set_full_pinyin_scheme', B, (I, 4))),
        ('load-phrase-library-0', ctx_call('load_phrase_library', B, (C.c_ubyte, 0))),
        ('load-phrase-library-8', ctx_call('load_phrase_library', B, (C.c_ubyte, 8))),
        ('unload-phrase-library-16', ctx_call('unload_phrase_library', B, (C.c_ubyte, 16))),
        ('unload-phrase-library-255', ctx_call('unload_phrase_library', B, (C.c_ubyte, 255)))):
    case('abort-zhuyin-' + _name, mode='zhuyin', abort=False)(_call)


def zhuyin_chewing(k, text=b'su3cl3'):
    inst = k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(inst, text)
    return inst


def offset_call(name, fresh, offset):
    def run(k):
        inst = k.alloc() if fresh else zhuyin_chewing(k)
        out = Z(UNTOUCHED)
        return {'ret': k.fn(name, B, P, Z, C.POINTER(Z))(inst, offset, C.byref(out))}
    return run


def guess_call(name, offset):
    def run(k):
        return {'ret': k.fn(name, B, P, Z)(zhuyin_chewing(k), offset)}
    return run


def key_call(name, fresh, offset):
    def run(k):
        inst = k.alloc() if fresh else zhuyin_chewing(k)
        out = P(UNTOUCHED)
        return {'ret': k.fn(name, B, P, Z, C.POINTER(P))(inst, offset, C.byref(out))}
    return run


def char_offset_call(offset):
    def run(k):
        out = Z(UNTOUCHED)
        return {'ret': k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
            zhuyin_chewing(k), '你好'.encode(), offset, C.byref(out))}
    return run


case('abort-zhuyin-get-left-zhuyin-offset-past-matrix', mode='zhuyin', abort=False)(
    offset_call('get_left_zhuyin_offset', False, 99))
case('abort-zhuyin-get-right-zhuyin-offset-past-matrix', mode='zhuyin', abort=False)(
    offset_call('get_right_zhuyin_offset', False, 99))
case('abort-zhuyin-get-right-zhuyin-offset-empty-matrix', mode='zhuyin', abort=False)(
    offset_call('get_right_zhuyin_offset', True, 0))
case('abort-zhuyin-guess-candidates-after-cursor-past-matrix', mode='zhuyin', abort=False)(
    guess_call('guess_candidates_after_cursor', 99))
case('abort-zhuyin-guess-candidates-before-cursor-past-matrix', mode='zhuyin', abort=False)(
    guess_call('guess_candidates_before_cursor', 99))
case('abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one', mode='zhuyin', abort=False)(
    guess_call('guess_candidates_after_cursor', 7))
case('abort-zhuyin-guess-candidates-before-cursor-reserved-slot-plus-one', mode='zhuyin', abort=False)(
    guess_call('guess_candidates_before_cursor', 7))
case('abort-zhuyin-get-character-offset-past-matrix', mode='zhuyin', abort=False)(char_offset_call(99))
case('abort-zhuyin-get-zhuyin-key-empty-matrix', mode='zhuyin', abort=False)(
    key_call('get_zhuyin_key', True, 0))
case('abort-zhuyin-get-zhuyin-key-rest-empty-matrix', mode='zhuyin', abort=False)(
    key_call('get_zhuyin_key_rest', True, 0))


# The zhuyin zero total aborts the pin only while a searched row is ranked
# (`zhuyin.cpp:1261`): the reserved slot after the cursor and the start before
# it hold none, and the pin answers true with no warning.
for _name, _offset in (('after_cursor', 6), ('before_cursor', 0)):
    def _no_rank_z(name, offset):
        def run(k):
            inst = zhuyin_chewing(k)
            k.fn('token_add_unigram_frequency', B, P, U, U)(inst, (1 << 24) | 1, (2 ** 32 - FACADE_TOTAL) % 2 ** 32)
            count = U()
            ret = k.fn('guess_candidates_' + name, B, P, Z)(inst, offset)
            k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            return {'ret': ret, 'rows': count.value}
        return run
    case('zhuyin-total-zero-ranks-nothing-' + _name.replace('_', '-'), mode='zhuyin', control=True)(
        _no_rank_z(_name, _offset))


@case('abort-zhuyin-parse-full-pinyin-apostrophe', mode='zhuyin', abort=False)
def _(k):
    key = C.c_uint16(UNTOUCHED & 0xFFFF)
    ret = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(k.inst, b"n'i", C.byref(key))
    return {'ret': ret, 'untouched key': key.value == UNTOUCHED & 0xFFFF}


@case('abort-zhuyin-guess-candidates-total-zero', mode='zhuyin', abort=False)
def _(k):
    inst = zhuyin_chewing(k)
    k.fn('token_add_unigram_frequency', B, P, U, U)(inst, (1 << 24) | 1, (2 ** 32 - FACADE_TOTAL) % 2 ** 32)
    return {'ret': k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)}


# The zhuyin sites' graceful neighbours answer without a warning.
@case('zhuyin-cursor-neighbours', mode='zhuyin', control=True)
def _(k):
    out = {}
    inst = zhuyin_chewing(k)
    res = Z(UNTOUCHED)
    out['offset 99'] = [k.fn('get_zhuyin_offset', B, P, Z, C.POINTER(Z))(inst, 99, C.byref(res)),
                        'untouched' if res.value == UNTOUCHED else res.value]
    for label, name, offset in (('left 0', 'get_left_zhuyin_offset', 0), ('left 3', 'get_left_zhuyin_offset', 3),
                                ('right 0', 'get_right_zhuyin_offset', 0), ('right 3', 'get_right_zhuyin_offset', 3)):
        res = Z(UNTOUCHED)
        out[label] = [k.fn(name, B, P, Z, C.POINTER(Z))(inst, offset, C.byref(res)),
                      'untouched' if res.value == UNTOUCHED else res.value]
    for label, name, offset in (('key 0', 'get_zhuyin_key', 0), ('key 99', 'get_zhuyin_key', 99),
                                ('rest 3', 'get_zhuyin_key_rest', 3)):
        ptr = P(UNTOUCHED)
        out[label] = k.fn(name, B, P, Z, C.POINTER(P))(inst, offset, C.byref(ptr))
    out['empty left'] = k.fn('get_left_zhuyin_offset', B, P, Z, C.POINTER(Z))(k.alloc(), 0, C.byref(Z()))
    out['empty guess'] = k.fn('guess_candidates_after_cursor', B, P, Z)(k.alloc(), 0)
    out['guess 0'] = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    out['guess end before'] = k.fn('guess_candidates_before_cursor', B, P, Z)(inst, 6)
    out['guess end after'] = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 6)
    return out


@case('zhuyin-scheme-and-library-neighbours', mode='zhuyin', control=True)
def _(k):
    out = {}
    for label, name, arg in (('chewing 1', 'set_chewing_scheme', 1), ('chewing 6', 'set_chewing_scheme', 6),
                             ('chewing 8', 'set_chewing_scheme', 8), ('chewing 9', 'set_chewing_scheme', 9),
                             ('full 1', 'set_full_pinyin_scheme', 1), ('full 3', 'set_full_pinyin_scheme', 3)):
        out[label] = k.fn(name, B, P, I)(k.ctx, arg)
    for label, name, arg in (('load 1', 'load_phrase_library', 1), ('load 7', 'load_phrase_library', 7),
                             ('load 16', 'load_phrase_library', 16), ('load 255', 'load_phrase_library', 255),
                             ('unload 1', 'unload_phrase_library', 1)):
        out[label] = k.fn(name, B, P, C.c_ubyte)(k.ctx, arg)
    return out


# The pinyin sites the offset probes of this group found still silent.
@case('abort-get-right-pinyin-offset-empty-matrix', abort=False)
def _(k):
    out = Z(UNTOUCHED)
    return {'ret': k.fn('get_right_pinyin_offset', B, P, Z, C.POINTER(Z))(k.alloc(), 0, C.byref(out))}


@case('abort-guess-candidates-past-matrix', abort=False)
def _(k):
    return {'ret': k.fn('guess_candidates', B, P, Z, U)(full_inst(k, b'nihao'), 99, 0)}


# Row 65 / #696. The pin's LONGER row is a property of the whole parse,
# not the lookup offset: `_prepend_longer_candidates` (`pinyin.cpp:1870-
# 1933`) searches the whole matrix with `prefix_len = m_parsed_key_len`
# (`:1876`, `:1883`) and is called for every offset whose sort word leaves
# `SORT_WITHOUT_LONGER_CANDIDATE` clear (`:2292-2293`). Only the main span
# search starts at `offset` (`:2229`), so an offset no span starts on — and
# the reserved slot past the parse — answers the LONGER row alone.
def candidate_rows(k, inst):
    """The instance's full candidate list as ordered [type, string] rows."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    for i in range(count.value):
        cand, kind, text = P(), I(), S()
        if k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand)):
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
            rows.append([kind.value, text.value.decode('utf-8', 'replace')])
        else:
            rows.append(None)
    return rows


def guess_rows(k, inst, offset, sort):
    """One `pinyin_guess_candidates` call: the return and the full list."""
    ret = k.fn('guess_candidates', B, P, Z, U)(inst, offset, sort)
    return {'ret': ret, 'rows': candidate_rows(k, inst)}


def zhuyin_guess_rows(k, inst, offset):
    """One `zhuyin_guess_candidates_after_cursor` call: return and list."""
    ret = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, offset)
    return {'ret': ret, 'rows': candidate_rows(k, inst)}


# The characterisation set: single and multi-syllable, a bare partial tail,
# apostrophes, and inputs whose longer phrases exist (nihao -> 你好吗,
# women -> 我们…) and do not (woshi, xian). Every matrix offset of each input
# — 0 through the reserved slot — three sort words — 0 (LONGER kept,
# unsorted) plus the two sorted words with and without the LONGER bit — and
# with and without a prior sentence lookup, which is what stores the n-best
# rows. Full lists, not returns.
#
# The pin's matrix holds `parsed + 1` columns for the pinyin facade; an
# offset past that reads through `_check_offset`'s `get_column_size` assert
# (`pinyin.cpp:2163-2182`, `:2226`) and aborts — the class (c) sites
# `abort-guess-candidates-past-matrix` and its zhuyin twin hold. The walk
# therefore stops at `parsed` (the reserved slot) and `parsed + 1`.
GUESS_CANDIDATE_INPUTS = (b'ni', b'n', b'nihao', b'nih', b"ni'hao",
                          b'hao', b'women', b'zhongguo', b'woshi', b'xian')
GUESS_CANDIDATE_SORTS = (0, 0x1c, 0x1e)


@case('guess-candidates-lookup-offset')
def _(k):
    out = {}
    for text in GUESS_CANDIDATE_INPUTS:
        for previous in (0, 1):
            inst = k.alloc()
            parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
            if previous:
                k.fn('guess_sentence', B, P)(inst)
            for sort in GUESS_CANDIDATE_SORTS:
                for offset in range(parsed + 2):
                    label = '%s prev=%d sort=%#x off=%d' % (
                        text.decode(), previous, sort, offset)
                    out[label] = guess_rows(k, inst, offset, sort)
    return out


# The zhuyin facade has no LONGER prepend (`zhuyin.cpp:1460-1539` prepends
# only the sentence rows at `:1533`), so the same full-list walk must already
# match at every offset; it is the control that bounds the rule to the pinyin
# facade. Its reserved slot is the LAST valid column (`parsed`); one past it
# aborts (row 66,
# `abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one`).
GUESS_CANDIDATE_ZHUYIN_INPUTS = (b'su3', b'su', b'su3cl3', b'su3cl',
                                 b"su3'cl3", b'cl3', b'ji3', b'ji3cl3',
                                 b'zhong1', b'xian1')


@case('guess-candidates-lookup-offset-zhuyin', mode='zhuyin', control=True)
def _(k):
    out = {}
    for text in GUESS_CANDIDATE_ZHUYIN_INPUTS:
        for previous in (0, 1):
            inst = k.alloc()
            parsed = k.fn('parse_more_chewings', Z, P, S)(inst, text)
            if previous:
                k.fn('guess_sentence', B, P)(inst)
            for offset in range(parsed + 1):
                label = '%s prev=%d off=%d' % (text.decode(), previous, offset)
                out[label] = zhuyin_guess_rows(k, inst, offset)
    return out


# One past the reserved slot is an ordinary lookup that finds nothing —
# except the LONGER row, which the pin still lists there (row 65). The full
# list is compared, not the return alone.
@case('guess-candidates-past-the-reserved-slot')
def _(k):
    out = {}
    inst = full_inst(k, b'nihao')
    for offset in (5, 6):
        out['offset %d' % offset] = guess_rows(k, inst, offset, 0)
    return out


# batch2 group 13: directory names that are not UTF-8 (PR 13, #587)
def non_utf8_dirs(k, bad_system, bad_user):
    """Opens a context whose system and/or user directory name holds a 0xFF byte."""
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
    case('non-utf8-directory-' + _label)(
        lambda k, system=_system, user=_user: non_utf8_dirs(k, system, user))


@case('non-utf8-directory-zhuyin-both', mode='zhuyin')
def _(k):
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


# batch2 group 14: the zhuyin twins of the batch-1 contracts (PR 14, #542)
# A populated USER_FILE token follows the facade's unload contract. Pinyin
# refuses index 7; zhuyin unloads it and its phrase-item reads fail until reload
# (074a2219 pinyin.cpp:466-474, zhuyin.cpp:378-388, phrase_index.h:646-657).
def user_library_token_unload(k):
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    tokens = [token for token in tokens_of(k, '你好') if token >> 24 == 7]
    assert added and len(tokens) == 1, 'a populated index-7 token is required'
    token = tokens[0]
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]

    def reads():
        keys = new(0, 0, 2)
        ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, 0, keys)
        view = C.cast(keys, C.POINTER(Arr)).contents
        content = C.string_at(view.data, view.len * 2).hex() if view.len else ''
        length, phrase = U(UNTOUCHED), P(UNTOUCHED)
        phrase_ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
            k.inst, token, C.byref(length), C.byref(phrase))
        count = U(UNTOUCHED)
        count_ret = k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(
            k.inst, token, C.byref(count))
        return dict(nth=[ret, view.len, content],
                    phrase=[phrase_ret, length.value,
                            'untouched' if phrase.value == UNTOUCHED else k.text(phrase.value)],
                    n=[count_ret, count.value])

    out = {'before': reads(), 'save': k.fn('save', B, P)(k.ctx)}
    out['unload'] = k.fn('unload_phrase_library', B, P, C.c_ubyte)(k.ctx, 7)
    out['unloaded'] = reads()
    out['load'] = k.fn('load_phrase_library', B, P, C.c_ubyte)(k.ctx, 7)
    out['reloaded'] = reads()
    return out


case('user-library-token-unload-pinyin', control=True)(user_library_token_unload)
case('user-library-token-unload-zhuyin', mode='zhuyin')(user_library_token_unload)


def zhuyin_sentence_out(k, inst):
    out = P(UNTOUCHED)
    ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(out))
    if out.value == UNTOUCHED:
        return [ret, 'untouched']
    return [ret, k.text(out.value)]


@case('zhuyin-sentence-before-guess', mode='zhuyin')
def _(k):
    out = {}
    inst = k.alloc()
    out['fresh'] = zhuyin_sentence_out(k, inst)
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    out['parsed'] = zhuyin_sentence_out(k, inst)
    k.fn('guess_sentence', B, P)(inst)
    out['guessed'] = zhuyin_sentence_out(k, inst)
    k.fn('reset', B, P)(inst)
    out['reset'] = zhuyin_sentence_out(k, inst)
    inst = k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(inst, b"'")
    out['keyless'] = [k.fn('guess_sentence', B, P)(inst), zhuyin_sentence_out(k, inst)]
    return out


@case('zhuyin-character-offset-out', mode='zhuyin')
def _(k):
    fn = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))
    out = {}
    for label, text, phrase, offset in (
            ('recursion-fails-1', b'su3cl3', '啊', 3), ('recursion-fails-2', b'su3cl3', '你', 6),
            ('hit', b'su3cl3', '你好', 5), ('reserved-slot', b'su3cl3', '你好', 6), ('reserved-slot-1', b'su3cl3', '你', 6), ('no-token', b'su3cl3', 'x', 2), ('empty-phrase', b'su3cl3', '', 2),
            ('no-parse', b'', '你好', 0), ('keyless', b"'", '你好', 0)):
        inst = k.alloc()
        k.fn('parse_more_chewings', Z, P, S)(inst, text)
        length = Z(UNTOUCHED)
        ret = fn(inst, phrase.encode(), offset, C.byref(length))
        out[label] = [ret, 'untouched' if length.value == UNTOUCHED else length.value]
    return out


@case('zhuyin-token-get-phrase-out', mode='zhuyin')
def _(k):
    out = {}
    for token in (0xFFFFFFFF, 0x0DEADBEE, 0x01000000):
        length, text = U(UNTOUCHED), P(UNTOUCHED)
        ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
            k.inst, token, C.byref(length), C.byref(text))
        out[hex(token)] = [ret, 'untouched' if length.value == UNTOUCHED else length.value,
                           'untouched' if text.value == UNTOUCHED else k.text(text.value)]
    return out


@case('zhuyin-nth-pronunciation-range', mode='zhuyin')
def _(k):
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    arr = new(0, 0, 4)
    k.fn('lookup_tokens', B, P, S, P)(k.inst, '你好'.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    token = C.cast(view.data, C.POINTER(U))[:view.len][0]
    count = U(UNTOUCHED)
    k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
    out = {'n': count.value}
    for nth in (0, count.value - 1, count.value, count.value + 1, 0xFFFFFFFF):
        keys = new(0, 0, 2)
        ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
        kview = C.cast(keys, C.POINTER(Arr)).contents
        content = C.string_at(kview.data, kview.len * 2).hex() if kview.len else ''
        in_range = nth < count.value
        out['nth %d' % nth] = [ret, kview.len] + ([content] if in_range else [])
        out['~content nth %d' % nth] = content
    append = k.glib.g_array_append_vals
    append.restype, append.argtypes = P, [P, P, U]
    keys = new(0, 0, 2)
    seed = C.c_ushort(0x1234)
    append(keys, C.byref(seed), 1)
    ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, 0xFFFFFFFF, 0, keys)
    out['unknown token'] = [ret, C.cast(keys, C.POINTER(Arr)).contents.len]
    return out


@case('zhuyin-static-key-slots', mode='zhuyin')
def _(k):
    first, second = k.alloc(), k.alloc()
    k.fn('parse_more_chewings', Z, P, S)(first, b'su3')
    k.fn('parse_more_chewings', Z, P, S)(second, b'cl3')
    key = k.fn('get_zhuyin_key', B, P, Z, C.POINTER(P))
    rest = k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))
    a, b, ra, rb = P(), P(), P(), P()
    out = {'ret': [key(first, 0, C.byref(a)), key(second, 0, C.byref(b)),
                   rest(first, 0, C.byref(ra)), rest(second, 0, C.byref(rb))]}
    out['key same pointer'] = a.value == b.value
    out['rest same pointer'] = ra.value == rb.value
    out['first key now'] = C.string_at(a.value, 2).hex()
    out['first rest now'] = C.string_at(ra.value, 4).hex()
    k.fn('free_instance', None, P)(first)
    out['key after free'] = C.string_at(a.value, 2).hex()
    out['rest after free'] = C.string_at(ra.value, 4).hex()
    return out


@case('zhuyin-key-rest-length', mode='zhuyin')
def _(k):
    fn = k.fn('get_zhuyin_key_rest_length', B, P, C.POINTER(Rest), C.POINTER(C.c_ushort))
    out = {}
    for begin, end in ((0, 2), (5, 2), (2, 2), (0, 65535), (65535, 0), (1, 65535)):
        length = C.c_ushort(0xBEEF)
        rest = Rest(begin, end)
        out['%d..%d' % (begin, end)] = [fn(k.inst, C.byref(rest), C.byref(length)), length.value]
    return out


@case('zhuyin-unload-phrase-library-repeat', mode='zhuyin')
def _(k):
    unload = k.fn('unload_phrase_library', B, P, U)
    return {'unload': [unload(k.ctx, i) for i in range(0, 16)] + [unload(k.ctx, i) for i in (2, 2)]}


def zhuyin_candidates_at_start(k):
    inst = zhuyin_chewing(k)
    k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    return count.value


@case('zhuyin-unload-phrase-library-effect', mode='zhuyin')
def _(k):
    unload = k.fn('unload_phrase_library', B, P, C.c_ubyte)
    load = k.fn('load_phrase_library', B, P, C.c_ubyte)
    out = {'before': zhuyin_candidates_at_start(k)}
    for index in (3, 2, 4):
        out['unload %d' % index] = [unload(k.ctx, index), zhuyin_candidates_at_start(k)]
        out['load %d' % index] = [load(k.ctx, index), zhuyin_candidates_at_start(k)]
        out['load %d again' % index] = load(k.ctx, index)
    return out


@case('zhuyin-alloc-instance-after-fini', mode='zhuyin')
def _(k):
    # As the pinyin case: the pin reads the freed context and survives by
    # chance, the ruled answer is NULL (class (b)); the case holds the exit
    # status and leaves the answer out.
    context = k.init()
    live = k.fn('alloc_instance', P, P)(context)
    k.fn('free_instance', None, P)(live)
    k.fn('fini', None, P)(context)
    after = k.fn('alloc_instance', P, P)(context)
    return {'live instance': bool(live), '~instance after fini': bool(after)}


# batch2 group 15: the pin's stderr and the save report (PR 15, #545)
def save_after_training(k, remove_dir, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
    ctx = k.ctx
    inst = k.alloc()
    k.fn(parse, Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    out = {'train': k.fn('train', B, P, *train_args)(inst, *((0,) if train_args else ()))}
    if remove_dir:
        shutil.rmtree(k.user)
    out['save'] = k.fn('save', B, P)(ctx)
    return out


def blocked_save(k, blocked, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
    """A second, modified save with one path of the file set blocked by a
    non-empty directory: what each side prints and which files it updates."""
    import hashlib
    ctx = k.ctx
    inst = k.alloc()
    k.fn(parse, Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    train = k.fn('train', B, P, *train_args)
    train(inst, *((0,) if train_args else ()))
    out = {'first': k.fn('save', B, P)(ctx)}

    def snapshot():
        files = {}
        for name in sorted(os.listdir(k.user)):
            path = os.path.join(k.user, name)
            files[name] = ('dir:' + ','.join(sorted(os.listdir(path)))) if os.path.isdir(path) \
                else hashlib.md5(open(path, 'rb').read()).hexdigest()[:8]
        return files
    before = snapshot()
    train(inst, *((0,) if train_args else ()))
    path = os.path.join(k.user, blocked)
    if os.path.isdir(path) or os.path.exists(path):
        os.replace(path, path + '.moved')
    os.mkdir(path)
    open(os.path.join(path, 'keep'), 'w').write('x')
    out['second'] = k.fn('save', B, P)(ctx)
    after = snapshot()
    out['files'] = {n: ('new' if n not in before else 'changed' if before[n] != after[n] else 'same')
                    for n in after if not n.endswith('.moved')}
    return out


# The pin writes and renames file by file (`pinyin.cpp:940-1130`): with one
# path blocked every other file is updated, one line is printed and the
# profile mixes two saves.
@case('stderr-save-one-tmp-blocked', stderr=True)
def _(k):
    return blocked_save(k, 'user_pinyin_index.bin.tmp')


@case('stderr-save-one-final-blocked', stderr=True)
def _(k):
    return blocked_save(k, 'user_phrase_index.bin')


@case('stderr-save-one-tmp-blocked-zhuyin', mode='zhuyin', stderr=True)
def _(k):
    return blocked_save(k, 'user_pinyin_index.bin.tmp', 'parse_more_chewings', b'su3cl3', ())


# Batch A (#525): `MemoryChunk::save`'s header writes (`memory_chunk.h:543`,
# `:547`). The pin writes the header as two `guint32`s and `assert`s each, so
# a filesystem that refuses a header word kills it mid-save. The save is the
# one failure the pin does not carry past: oxpinyin fails the call and logs
# one warning in the facade's own domain.
def _dirty_for_save(k, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
    ctx = k.ctx
    inst = k.alloc()
    k.fn(parse, Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    train = k.fn('train', B, P, *train_args)
    train(inst, *((0,) if train_args else ()))
    return ctx


def save_header_dev_full(k, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
    """`user.bin.tmp` symlinked to `/dev/full`: its every write fails, so the
    very first header word does (`memory_chunk.h:543`,
    `ret_len == sizeof(length)`)."""
    ctx = _dirty_for_save(k, parse, text, train_args)
    Path(k.user, 'user.bin.tmp').symlink_to('/dev/full')
    return {'ret': k.fn('save', B, P)(ctx)}


def save_header_rlimit(k, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
    """`RLIMIT_FSIZE` of one header word: the length word fits (file size 4)
    and the checksum word past it is refused (`memory_chunk.h:547`,
    `ret_len == sizeof(checksum)`)."""
    ctx = _dirty_for_save(k, parse, text, train_args)
    signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
    resource.setrlimit(resource.RLIMIT_FSIZE, (4, 4))
    return {'ret': k.fn('save', B, P)(ctx)}


@case('abort-save-chunk-header-length', abort=False)
def _(k):
    return save_header_dev_full(k)


@case('abort-save-chunk-header-checksum', abort=False)
def _(k):
    return save_header_rlimit(k)


@case('abort-zhuyin-save-chunk-header-length', mode='zhuyin', abort=False)
def _(k):
    return save_header_dev_full(k, 'parse_more_chewings', b'su3cl3', ())


@case('abort-zhuyin-save-chunk-header-checksum', mode='zhuyin', abort=False)
def _(k):
    return save_header_rlimit(k, 'parse_more_chewings', b'su3cl3', ())


@case('stderr-fresh-user-dir', stderr=True)
def _(k):
    ctx = k.ctx
    k.fn('fini', None, P)(ctx)
    return {'init': bool(ctx)}


@case('stderr-fresh-user-dir-zhuyin', mode='zhuyin', stderr=True)
def _(k):
    ctx = k.ctx
    k.fn('fini', None, P)(ctx)
    return {'init': bool(ctx)}


@case('stderr-init-missing-system-dir', stderr=True)
def _(k):
    return {'ctx': bool(k.init(system='/nonexistent-system-dir'))}


@case('stderr-init-missing-system-dir-zhuyin', mode='zhuyin', stderr=True)
def _(k):
    return {'ctx': bool(k.init(system='/nonexistent-system-dir'))}


@case('stderr-init-system-dir-without-table-conf', stderr=True)
def _(k):
    empty = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
    return {'ctx': bool(k.init(system=empty + '//'))}


@case('stderr-save-dir-removed', stderr=True)
def _(k):
    return save_after_training(k, True)


@case('stderr-save-dir-removed-zhuyin', mode='zhuyin', stderr=True)
def _(k):
    return save_after_training(k, True, 'parse_more_chewings', b'su3cl3', ())


@case('stderr-fini-dir-removed', stderr=True)
def _(k):
    ctx = k.ctx
    shutil.rmtree(k.user)
    k.fn('fini', None, P)(ctx)
    return {}


@case('stderr-fini-dir-removed-zhuyin', mode='zhuyin', stderr=True)
def _(k):
    ctx = k.ctx
    shutil.rmtree(k.user)
    k.fn('fini', None, P)(ctx)
    return {}


# The paths that stay quiet at the pin: a save that has nothing to save, a
# save that works, a reopen of the profile it wrote.
# Paths in the pin's lines are the caller's bytes, not text.
@case('stderr-non-utf8-user-dir', stderr=True)
def _(k):
    user = os.path.join(os.fsencode(k.scratch), b'user-\xff' + os.path.basename(k.user.encode())[5:])
    os.mkdir(user)
    ctx = k.init(user=user)
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    out = {'train': k.fn('train', B, P, C.c_ubyte)(inst, 0)}
    shutil.rmtree(user)
    out['save'] = k.fn('save', B, P)(ctx)
    k.fn('fini', None, P)(ctx)
    return out


@case('stderr-quiet-saves', stderr=True)
def _(k):
    ctx = k.ctx
    out = {'unmodified save': k.fn('save', B, P)(ctx)}
    out.update(save_after_training(k, False))
    k.fn('fini', None, P)(ctx)
    ctx2 = k.init()
    out['reopen'] = bool(ctx2)
    out['second save'] = k.fn('save', B, P)(ctx2)
    k.fn('fini', None, P)(ctx2)
    return out


@case('stderr-unmodified-save-dir-removed', stderr=True)
def _(k):
    ctx = k.ctx
    shutil.rmtree(k.user)
    return {'save': k.fn('save', B, P)(ctx)}


# The raw `mmap %s failed!` lines of the pin's library loaders (#545;
# `pinyin.cpp:256`, `:290`, `:956`, `:1265`, `zhuyin.cpp:200`, `:589`,
# `:800`). Each site hands the chunk it failed to map, empty, on: the
# library load reads `NULL + 16` and the save's `diff` does the same on the
# old chunk, so the pin dies of SIGSEGV right after the line (register
# row 68, class (b)). The system directory is a private copy of the oracle's
# (every file a link back to it) with one library broken; the oracle's own
# files are never touched.
def private_system(k):
    system = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
    for entry in os.listdir(k.data):
        os.symlink(os.path.join(k.data, entry), os.path.join(system, entry))
    return system


def break_library(k, system, name, how):
    """Replaces the link to library `name` with a file `MemoryChunk::mmap`
    refuses: none (`missing`), shorter than the 8-byte header (`short`), or a
    payload whose checksum does not verify (`checksum`)."""
    path = os.path.join(system, name)
    os.unlink(path)
    if how == 'short':
        Path(path).write_bytes(b'\0\0\0\0')
    elif how == 'checksum':
        data = bytearray(Path(k.data, name).read_bytes())
        data[-1] ^= 0xff
        Path(path).write_bytes(bytes(data))
    elif how in ('dir-long', 'dir-short'):
        # A non-empty directory in the library's place. The size the pin
        # reads from `lseek(SEEK_END)` is the file system's: ext4 answers
        # the htree end of file (huge) for any directory, btrfs the summed
        # length of the entry names twice over (2 for `x`, 50 for 25
        # characters), so which `MemoryChunk::mmap` site is reached follows
        # the size (`memory_chunk.h:479-483` against `:493`).
        os.makedirs(path)
        Path(path, 'e' * 25 if how == 'dir-long' else 'x').write_bytes(b'x')
    else:
        assert how == 'missing'


def train_once(k, ctx, parse, text, train_args):
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn(parse, Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    return k.fn('train', B, P, *train_args)(inst, *((0,) if train_args else ()))


LIBRARY_BREAKS = ('missing', 'short', 'checksum')
# mode -> (library file, index, parse call, text, train args)
LIBRARY_FACADES = {
    'pinyin': ('merged.bin', 4, 'parse_more_full_pinyins', b'nihao', (C.c_ubyte,)),
    'zhuyin': ('merged.bin', 4, 'parse_more_chewings', b'su3cl3', ()),
}


def library_probe(site, how):
    def probe(k):
        name, index, parse, text, train_args = LIBRARY_FACADES[k.mode]
        system = private_system(k)
        if site in ('init', 'init-fresh', 'init-nonconform'):
            # The pin judges the user profile before it loads a library, so
            # the failing init writes the profile's line first when the
            # profile is fresh or non-conforming and nothing of it when the
            # profile conforms: the three user dirs below.
            if site != 'init-fresh':
                settled = k.init()
                assert settled, 'init failed'
                train_once(k, settled, parse, text, train_args)
                assert k.fn('save', B, P)(settled), 'save failed'
                k.fn('fini', None, P)(settled)
            if site == 'init-nonconform':
                conf = Path(k.user, 'user.conf')
                conf.write_text(re.sub(r'(model data version:)\d+', r'\g<1>999', conf.read_text()))
            break_library(k, system, name, how)
            return {'ctx': bool(k.init(system=system))}
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        load = k.fn('load_phrase_library', B, P, C.c_ubyte)
        if site == 'load':
            # GBK is the one library `pinyin_unload_phrase_library` lets go
            # (TSI_DICTIONARY, 1, is the one `zhuyin_unload_phrase_library`
            # keeps); index 2 is unloadable on both facades.
            out = {'unload': k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2)}
            break_library(k, system, 'gbk_char.bin', how)
            out['load'] = load(ctx, 2)
            return out
        if site == 'addon':
            addon = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
            out = {'unload': k.fn('unload_addon_phrase_library', B, P, C.c_ubyte)(ctx, 4)}
            break_library(k, system, 'art.bin', how)
            out['load'] = addon(ctx, 4)
            return out
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        break_library(k, system, name, how)
        if site == 'save':
            out['save'] = k.fn('save', B, P)(ctx)
        else:
            assert site == 'mask-out'
            out['mask_out'] = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        return out
    return probe


def library_expected(site, how, mode):
    """What the subject answers at a site where the pin dies: its answers
    from before the lines were added, held unchanged. An init that fails on a
    library logs one warning in its own domain and answers NULL."""
    if site.startswith('init'):
        failed = how != 'missing'
        return {'ctx': not failed, 'logs': [[WARNING_DOMAIN[mode], 16]] if failed else []}
    return dict(logs=[], **{
        'load': {'unload': True, 'load': True},
        'addon': {'unload': True, 'load': False},
        'save': {'train': True, 'save': True},
        'mask-out': {'train': True, 'mask_out': True},
    }[site])


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    for _how in LIBRARY_BREAKS:
        for _site in ('init', 'load', 'save', 'mask-out') + (('addon',) if _mode == 'pinyin' else ()):
            case(f'stderr-library-{_site}-{_how}' + _suffix, mode=_mode,
                 crash=library_expected(_site, _how, _mode))(library_probe(_site, _how))
        # Both lines in the pin's order: the profile's, then the library's.
        for _site, _label in (('init-fresh', 'fresh-user-dir'), ('init-nonconform', 'nonconforming-user-dir')):
            case(f'stderr-library-init-{_label}-{_how}' + _suffix, mode=_mode,
                 crash=library_expected(_site, _how, _mode))(library_probe(_site, _how))


# A directory where a system library should be (register row 96). A size of
# 8 or more reads as a header-sized file, `read` then fails with EISDIR and
# `assert(ret_len == sizeof(length))` kills the pin (`memory_chunk.h:493`,
# class (c)); a smaller size is `MemoryChunk::mmap`'s ordinary `false` and
# the empty chunk the pin dies on later (row 76, class (b)). The long entry
# name reaches `:493` on ext4 and on btrfs; the one-character entry reaches
# it on ext4 and row 76 on btrfs, so both shapes match whichever site the
# pin's own signal names.
LIBRARY_FINAL = {'init': 'ctx', 'load': 'load', 'save': 'save', 'mask-out': 'mask_out', 'addon': 'load'}


def library_directory_probe(site, how):
    inner = library_probe(site, how)

    def probe(k):
        out = inner(k)
        out['ret'] = out[LIBRARY_FINAL[site]]
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    # The zhuyin facade has no addon entry point.
    for _site in (s for s in LIBRARY_FINAL if s != 'addon' or _mode == 'pinyin'):
        case(f'abort-library-directory-long-entry-{_site}' + _suffix, mode=_mode,
             abort=False, crash=library_expected(_site, 'short', _mode), either=True)(library_directory_probe(_site, 'dir-long'))
        case(f'library-directory-one-entry-{_site}' + _suffix, mode=_mode, abort=False,
             crash=library_expected(_site, 'short', _mode), either=True)(library_directory_probe(_site, 'dir-short'))


def library_directory_guard(site, guard):
    def probe(k):
        system = private_system(k)
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        name = 'gbk_char.bin' if guard == 'unloaded' else 'gb_char.bin'
        if guard == 'reserved':
            text = text.replace('default RESERVED NULL NULL NULL NOT_USED',
                                'default RESERVED gb_char.table reserved.bin reserved.dbin SYSTEM_FILE')
            name = 'reserved.bin'
            os.symlink(os.path.join(k.data, 'gb_char.bin'), os.path.join(system, name))
        conf.unlink()
        conf.write_text(text)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, phrase, train_args = LIBRARY_FACADES[k.mode]
        assert train_once(k, ctx, parse, phrase, train_args), 'train failed'
        if guard == 'unloaded':
            assert k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2), 'unload failed'
        break_library(k, system, name, 'dir-long')
        if site == 'save':
            ret = k.fn('save', B, P)(ctx)
        else:
            ret = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        return {'ret': ret}
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _guard in ('reserved', 'unloaded'):
        for _site in ('save', 'mask-out'):
            case(f'library-directory-guard-{_guard}-{_site}' + ('-zhuyin' if _mode == 'zhuyin' else ''),
                 mode=_mode, control=True)(library_directory_guard(_site, _guard))


# The guards that keep the pin silent with the same broken library: the
# library is already loaded (a load asks for nothing), it is unloaded (a
# save and a mask-out skip it), nothing is modified (a save writes nothing),
# or it is whole (a reload maps it).
def library_control(site):
    def probe(k):
        name, index, parse, text, train_args = LIBRARY_FACADES[k.mode]
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        unload = k.fn('unload_phrase_library', B, P, C.c_ubyte)
        load = k.fn('load_phrase_library', B, P, C.c_ubyte)
        out = {}
        if site == 'load-already-loaded':
            break_library(k, system, 'gbk_char.bin', 'checksum')
            out['load'] = load(ctx, 2)
        elif site == 'load-whole':
            out['unload'] = unload(ctx, 2)
            out['load'] = load(ctx, 2)
        elif site == 'addon-already-loaded':
            addon = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
            out['first'] = addon(ctx, 4)
            break_library(k, system, 'art.bin', 'checksum')
            out['again'] = addon(ctx, 4)
        elif site == 'save-unmodified':
            break_library(k, system, name, 'checksum')
            out['save'] = k.fn('save', B, P)(ctx)
        else:
            assert site in ('save-unloaded', 'mask-out-unloaded')
            out['train'] = train_once(k, ctx, parse, text, train_args)
            out['unload'] = unload(ctx, 2)
            break_library(k, system, 'gbk_char.bin', 'checksum')
            if site == 'save-unloaded':
                out['save'] = k.fn('save', B, P)(ctx)
            else:
                out['mask_out'] = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        k.fn('fini', None, P)(ctx)
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _site in ('load-already-loaded', 'load-whole', 'save-unmodified', 'save-unloaded', 'mask-out-unloaded') + \
            (('addon-already-loaded',) if _mode == 'pinyin' else ()):
        case(f'stderr-library-control-{_site}' + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode,
             control=True, stderr=True)(library_control(_site))


# --------------------------------------------------------------------------

def remember_export(k, library, phrase):
    if k.mode == 'zhuyin':
        return 'NO EXPORT API'
    it=k.fn('begin_get_phrases',P,P,U)(k.ctx,library)
    rows=[]
    while k.fn('iterator_has_next_phrase',B,P)(it):
        ph,py,n=P(),P(),I()
        ret=k.fn('iterator_get_next_phrase',B,P,C.POINTER(P),C.POINTER(P),C.POINTER(I))(it,C.byref(ph),C.byref(py),C.byref(n))
        text,reading=k.text(ph.value),k.text(py.value)
        if text==phrase: rows.append([ret,text,reading,n.value])
    k.fn('end_get_phrases',None,P)(it)
    return rows


def remember_tones(k):
    out={'remember_api':hasattr(k.lib,k.mode+'_remember_user_input'),
         'export_api':hasattr(k.lib,k.mode+'_begin_get_phrases')}
    if k.mode=='zhuyin':return out
    it=k.fn('begin_add_phrases',P,P,U)(k.ctx,7)
    out['add']=k.fn('iterator_add_phrase',B,P,S,S,I)(it,'你好你好'.encode(),b"ni3'hao3'ni3'hao3",100000)
    k.fn('end_add_phrases',None,P)(it)
    out['parse']=k.fn('parse_more_full_pinyins',Z,P,S)(k.inst,b'ni3hao3ni3hao3')
    out['guess']=k.fn('guess_sentence',B,P)(k.inst)
    out['remember']=k.fn('remember_user_input',B,P,S,I)(k.inst,'你好你好'.encode(),3)
    out['export']=remember_export(k,7,'你好你好')
    k.fn('free_instance',None,P)(k.inst)
    k.fn('fini',None,P)(k.ctx)
    return out


case('remember-tones-pinyin')(remember_tones)
case('remember-api-absent-zhuyin', mode='zhuyin', control=True)(remember_tones)

# #643: g_build_filename drops empty elements, but stops at NULL.
def init_system_argument(k, form):
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
             control=_form in ('empty-missing', 'dot', 'absolute'), stderr=True)(
                 lambda k, form=_form: init_system_argument(k, form))


def remember_review(k, double=False):
    k.fn('set_options', B, P, U)(k.ctx, 0x20)
    parse = 'parse_more_double_pinyins' if double else 'parse_more_full_pinyins'
    phrase = '你' if double else '你好'
    out = {'parse': k.fn(parse, Z, P, S)(k.inst, b'ni3' if double else b'ni3hao3')}
    out['guess'] = k.fn('guess_sentence', B, P)(k.inst)
    if not double:
        out['clear-tone'] = k.fn('set_options', B, P, U)(k.ctx, 0)
    out['remember'] = k.fn('remember_user_input', B, P, S, I)(k.inst, phrase.encode(), 3)
    out['export'] = remember_export(k, 7, phrase)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    return out

case('remember-cleared-tone-pinyin')(remember_review)
case('remember-double-tone-pinyin')(lambda k: remember_review(k, True))
# #642: NULL is transient state, not an absent user index. Snapshot both
# filesystem destinations at every step, including a pre-existing canary.
def transient_files(root):
    return {str(p.relative_to(root)): (p.stat().st_mode, p.read_bytes())
            for p in Path(root).rglob('*') if p.is_file()}


def null_user_session(k):
    cwd = tempfile.mkdtemp(prefix='cwd-', dir=k.scratch)
    tmp = tempfile.mkdtemp(prefix='tmp-', dir=k.scratch)
    for directory in (cwd, tmp):
        Path(directory, 'canary').write_bytes(b'unchanged')
    os.chdir(cwd)
    os.environ['TMPDIR'] = tmp
    baseline = [transient_files(d) for d in (cwd, tmp)]
    out = {}

    def checkpoint(label):
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
    case('null-user-session-' + _mode, mode=_mode, stderr=True)(null_user_session)


def transient_library(k,library,null):
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
                 mode=_mode, control=not _null, stderr=True)(
                lambda k, library=_library, null=_null: transient_library(k, library, null))


# #759: the export iterator is the pin's cursor (pinyin.cpp:662-768 at
# 074a2219). Every get_next re-reads the item it is at and re-probes the
# range (:748-767), so an import that lands between two calls is exported:
# a reading merged into the item under the cursor, and a token appended
# past it. The parent buffered each probed item's rows at the probe.
def export_rows(k, it, limit=None):
    rows = []
    while k.fn('iterator_has_next_phrase', B, P)(it) and (limit is None or len(rows) < limit):
        ph, py, n = P(), P(), I(UNTOUCHED)
        ret = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
            it, C.byref(ph), C.byref(py), C.byref(n))
        rows.append([ret, k.text(ph.value), k.text(py.value), n.value])
    return rows


def import_rows(k, library, rows):
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
    added = [k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading.encode(), count)
             for phrase, reading, count in rows]
    k.fn('end_add_phrases', None, P)(it)
    return added


def export_open_across_import_system(k):
    # Library 1's first two tokens: their reading counts, and the second's
    # text and first reading through the token getters.
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    counts = []
    for token in (0x01000001, 0x01000002):
        count = U(UNTOUCHED)
        assert k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
        counts.append(count.value)
    length, text = U(UNTOUCHED), P(UNTOUCHED)
    assert k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
        k.inst, 0x01000002, C.byref(length), C.byref(text))
    phrase = k.text(text.value)
    keys = new(0, 0, 2)
    assert k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, 0x01000002, 0, keys)
    view = C.cast(keys, C.POINTER(Arr)).contents
    syllables = []
    for i in range(view.len):
        key = C.c_ushort(C.cast(view.data, C.POINTER(C.c_ushort))[i])
        rendered = P(UNTOUCHED)
        assert k.fn('get_pinyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(
            k.inst, C.byref(key), C.byref(rendered))
        syllables.append(k.text(rendered.value))
    # The same syllables with tone 1: a reading the item does not hold.
    reading = "'".join(syllable + '1' for syllable in syllables)
    it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 1)
    out = {'counts': counts, 'phrase': phrase, 'reading': reading}
    # Every row of the first token: the cursor has probed the second.
    out['first'] = export_rows(k, it, counts[0])
    out['added'] = import_rows(k, 1, [(phrase, reading, 5)])
    out['second'] = export_rows(k, it, counts[1] + 1)
    k.fn('end_get_phrases', None, P)(it)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    return out


def export_open_across_import_user(k):
    out = {'seed': import_rows(k, 7, [('你好', "ni'hao", 5)])}
    it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
    out['has_next'] = k.fn('iterator_has_next_phrase', B, P)(it)
    out['added'] = import_rows(k, 7, [('好你', "hao'ni", 7), ('你好', "ni2'hao2", 3)])
    out['rows'] = export_rows(k, it)
    out['has_next_after'] = k.fn('iterator_has_next_phrase', B, P)(it)
    k.fn('end_get_phrases', None, P)(it)
    k.fn('fini', None, P)(k.ctx)
    return out


# #790: train_result3 changes the live PhraseItem (074a2219
# phonetic_lookup.h:925-929; phrase_index.cpp:114-143). Export reads it on
# each call (pinyin.cpp:707-739), before save or a library reload.
def issue790_export(k, action='train'):
    phrase, token = '你', tokens_of(k, '你')[0]
    out = {'before': remember_export(k, 1, phrase)}
    iterator = (k.fn('begin_get_phrases', P, P, U)(k.ctx, 1)
                if action == 'open-before' else None)
    inst = k.inst
    out['parsed'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    if action in ('unconstrained', 'no-train'):
        out['guessed'] = k.fn('guess_sentence', B, P)(inst)
        if action == 'unconstrained':
            out['train'] = k.fn('train', B, P, C.c_ubyte)(inst, 0)
    else:
        out['training'] = train_selected_top(k, phrase, token, guess_before_choose=True)
    if action in ('save-reopen', 'replayed-train'):
        out['save'] = k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        if action == 'replayed-train':
            out['replayed'] = remember_export(k, 1, phrase)
            assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'ni')
            out['retraining'] = train_selected_top(k, phrase, token, guess_before_choose=True)
    if iterator:
        out['after'] = [row for row in export_rows(k, iterator) if row[1] == phrase]
        k.fn('end_get_phrases', None, P)(iterator)
    else:
        out['after'] = remember_export(k, 1, phrase)
    return out


case('issue790-export-after-train-system')(issue790_export)
for _action in ('no-train', 'unconstrained', 'open-before', 'save-reopen', 'replayed-train'):
    case('issue790-export-' + _action + '-system',
         control=_action in ('no-train', 'unconstrained', 'save-reopen'))(
             lambda k, action=_action: issue790_export(k, action))


def issue790_readings(k, phrase, readings, unigram=None):
    """Replay a system-item MODIFY using the existing logger fixture
    helpers; only the named item's readings and optional unigram change."""
    token = tokens_of(k, phrase)[0]
    assert token >> 24 == 1
    packed = [(key_out(k, text.encode())[1], count) for text, count in readings]
    payload = Path(k.data, 'gb_char.bin').read_bytes()[8:]
    word = lambda offset: int.from_bytes(payload[offset:offset + 4], 'little')
    total, index, content = word(0), word(4), word(8)
    offset = content + word(index + (token & 0x00FFFFFF) * 4)
    length, nprons = payload[offset:offset + 2]
    assert length == 1
    old = payload[offset:offset + 10 + nprons * 6]
    previous = int.from_bytes(old[2:6], 'little')
    unigram = previous if unigram is None else unigram
    new = (bytes((1, len(packed))) + unigram.to_bytes(4, 'little') + old[6:10] +
           b''.join(key.to_bytes(2, 'little') + count.to_bytes(4, 'little')
                    for key, count in packed))
    record = ((3).to_bytes(4, 'little') + (token & 0x00FFFFFF).to_bytes(4, 'little') +
              len(old).to_bytes(2, 'little') + len(new).to_bytes(2, 'little') + old + new)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    k._ctx = k._inst = None
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, (total + unigram - previous) & 0xFFFFFFFF) + record)
    return token


def issue790_bounded_counts(k, count):
    token = issue790_readings(k, '你', [('ni3', count), ('ni2', 0), ('na4', 0)])
    out = {'before': remember_export(k, 1, '你')}
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'ni')
    out['training'] = train_selected_top(k, '你', token, guess_before_choose=True)
    out['after'] = remember_export(k, 1, '你')
    return out


case('issue790-export-zero-and-signed-system')(
    lambda k: issue790_bounded_counts(k, (1 << 31) - 69))
case('issue790-export-prefix-overflow-system')(
    lambda k: issue790_bounded_counts(k, (1 << 32) - 76))


def issue790_imported(k, appended=False):
    phrase, reading = ('你你你你', "ni'ni'ni'ni") if appended else ('你', 'ni2')
    out = {'imported': import_rows(k, 1, [(phrase, reading, 5)]),
           'before': remember_export(k, 1, phrase)}
    token = tokens_of(k, phrase)[0]
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, reading.encode())
    out['training'] = train_selected_top(k, phrase, token, guess_before_choose=True)
    out['after'] = remember_export(k, 1, phrase)
    return out


case('issue790-export-override-only-system')(issue790_imported)
case('issue790-export-appended-system')(lambda k: issue790_imported(k, appended=True))


@case('issue790-guess-after-train-system')
def _(k):
    # The first ni reading has 1/100 of 尼's pronunciation mass. A large
    # unrelated start-row total makes the new bigram too small to explain
    # the ranking change: only reading the trained pronunciation can do it.
    token = issue790_readings(k, '尼', [('ni2', 1), ('na2', 99)], 200000)
    _replace_system_start_row(k, _gram_bytes(1, [(_PAST_RANGES, 1)]))
    _replace_user_bigram(k, [(1, _gram_bytes(1 << 28, [(_PAST_RANGES, 1 << 28)]))])
    out = {'parsed': k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'ni')}
    out['training'] = train_selected_top(k, '尼', token, guess_before_choose=True)
    out['before'] = out['training'].pop('before-choose')
    out['clear'] = k.fn('clear_constraint', B, P, Z)(k.inst, 0)
    out['after'] = _guess_out(k, b'ni')
    return out


@case('issue790-character-offset-after-train-system')
def _(k):
    # Both readings have shipped index entries. An incomplete n path
    # trains na (initially zero) and nei; the public offset call then tests
    # na without saving/reopening. An unindexed positive reading would
    # instead exercise incomplete-key expansion's membership semantics.
    token = issue790_readings(k, '哪', [('na3', 0), ('nei3', 100)])
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a)
    get = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))
    def offset():
        assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'na')
        value = Z(UNTOUCHED)
        return [get(k.inst, '哪'.encode(), 1, C.byref(value)), value.value]
    out = {'before': offset()}
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'n')
    out['training'] = train_selected_top(k, '哪', token, guess_before_choose=True)
    out['clear'] = k.fn('clear_constraint', B, P, Z)(k.inst, 0)
    out['after'] = offset()
    return out


case('issue759-export-open-across-import-system')(export_open_across_import_system)
case('issue759-export-open-across-import-user')(export_open_across_import_user)


# #525: reduce_tokens asserts on the sixth add, with five existing tokens.
def combined_import_bound(k, guard=True):
    phrase = '你好'
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    added = []
    for library in range(1, 6):
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
        added.append(k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading, 100000))
        k.fn('end_add_phrases', None, P)(it)
    assert added == [True] * 5
    if not guard:
        return {'added': added}
    assert k.fn('save', B, P)(k.ctx)
    files = transient_files(k.user)
    tokens = [16802309, 33570045, 50359794, 67109866, 83886081]
    frequencies = [unigram_of(k, token) for token in tokens]
    exports = [remember_export(k, library, phrase) for library in range(1, 8)]
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 6)
    ret = k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading, 100000)
    # The pin aborts above. These fields are checked by the existing class(c)
    # driver and prove the subject refused before changing prior state.
    out = {'ret': ret,
           'untouched-tokens': tokens_of(k, phrase) == tokens,
           'untouched-frequencies': [unigram_of(k, token) for token in tokens] == frequencies,
           'untouched-readings': [remember_export(k, library, phrase) for library in range(1, 8)] == exports,
           'untouched-dirty': not k.fn('save', B, P)(k.ctx),
           'untouched-files': transient_files(k.user) == files}
    k.fn('end_add_phrases', None, P)(it)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    return out

for _mode in ('pinyin', 'zhuyin'):
    case('combined-library-sixth-add-' + _mode, mode=_mode, abort=False)(combined_import_bound)
    case('combined-library-fifth-add-' + _mode, mode=_mode, control=True)(
        lambda k: combined_import_bound(k, guard=False))


def import_review(k, invalid=False):
    out = combined_import_bound(k, guard=False)
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 6 if invalid else 8)
    reading = b'ni3' if invalid else b"ni3'hao3"
    if k.mode == 'zhuyin':
        reading = ('ㄋㄧˇ' if invalid else 'ㄋㄧˇ ㄏㄠˇ').encode()
    out['ret'] = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 3)
    k.fn('end_add_phrases', None, P)(it)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    return out

for _mode in ('pinyin', 'zhuyin'):
    case('combined-library-invalid-reading-' + _mode, mode=_mode, control=True)(lambda k: import_review(k, True))
    case('combined-library-unused-index-' + _mode, mode=_mode, abort=False)(import_review)


# #757 PR 0: two user phrases with equal counts whose user-table order
# differs from token order. add_index keeps the user table ordered by
# pinyin_exact_compare2 keys, then token (074a2219 chewing_large_table2.h:
# 208-231, pinyin_phrase3.h:33-63), so 靐靐 (da2'da2, added second) comes
# before 龘龘 (da3'da3, added first). A toneless parse matches both with
# equal unigram and pronunciation possibility: the n-best tie goes to the
# phrase the trellis walk meets first (phonetic_lookup.h:560-572).
def nbest_user_table_order(k):
    phrases = (('龘龘', "da3'da3", 'ㄉㄚˇ ㄉㄚˇ'), ('靐靐', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'))
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert it
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    added = [add(it, text.encode(), (pinyin if k.mode == 'pinyin' else zhuyin).encode(), 100000)
             for text, pinyin, zhuyin in phrases]
    k.fn('end_add_phrases', None, P)(it)
    assert all(added), added
    tokens = [[token for token in tokens_of(k, text) if token >> 24 == 7] for text, _, _ in phrases]
    assert all(len(found) == 1 for found in tokens), tokens
    assert tokens[0][0] < tokens[1][0], 'the first phrase added holds the lower token'
    out = {'tokens': [hex(found[0]) for found in tokens]}
    if k.mode == 'zhuyin':
        # USE_TONE without FORCE_TONE, so the zhuyin facade takes a toneless key.
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    inst = k.alloc()
    out['parsed'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'dada')
    out['guess'] = k.fn('guess_sentence', B, P)(inst)
    if k.mode == 'pinyin':
        out['sentences'] = [sentence_out(k, inst, index) for index in range(3)]
    else:
        out['sentence'] = zhuyin_sentence_out(k, inst)
    return out


# Controls: the parent build already met the user items in this order, so
# PR 0 preserves the output here.
for _mode in ('pinyin', 'zhuyin'):
    case('nbest-user-table-order-' + _mode, mode=_mode, control=True)(nbest_user_table_order)


def user_dir_listing(scratch, before):
    """What the run left in the user directories it created under `scratch`:
    every file by name, `user.conf` by text, library chunks by size. The
    directories are removed."""
    listing = {}
    for entry in sorted(set(os.listdir(scratch)) - before):
        path = os.path.join(scratch, entry)
        if not entry.startswith('user-') or not os.path.isdir(path):
            continue
        for name in sorted(os.listdir(path)):
            file = os.path.join(path, name)
            if name == 'user.conf':
                listing[name] = Path(file).read_text().replace('\n', '|')
            elif os.path.isfile(file) and not name.startswith('user_') and name.endswith(('.bin', '.dbin')):
                listing[name] = os.path.getsize(file)
            else:
                listing[name] = None
        shutil.rmtree(path, ignore_errors=True)
    return listing


# Axis J targeted reachability controls, at libpinyin 074a2219:
# pinyin.cpp:2515-2520 calls ForwardPhoneticConstraints::diff_result when
# choosing a displayed NBEST row. No forged candidate or context is needed.
@case('j-choose-nbest-diff-result', control=True)
def _(k):
    inst = guessed(k)
    candidate = row_of(k, inst, 1)
    assert candidate, 'no displayed NBEST candidate'
    rank = C.c_ubyte(0xff)
    ranked = k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
        inst, candidate, C.byref(rank))
    chosen = k.fn('choose_candidate', I, P, Z, P)(inst, 0, candidate)
    return {'ranked': ranked, 'rank': rank.value, 'chosen': chosen}


# Scheme HSU (2) constructs ZhuyinDiscreteParser2; Dachen CP26 (9)
# constructs ZhuyinDaChenCP26Parser2. Batch parse goes through their parse
# virtuals (pinyin.cpp:1595 / zhuyin.cpp:1068), rather than one-key parsing.
def j_multi_chewing(scheme):
    def probe(k):
        setter = 'set_zhuyin_scheme' if k.mode == 'pinyin' else 'set_chewing_scheme'
        selected = k.fn(setter, B, P, I)(k.ctx, scheme)
        parsed = k.fn('parse_more_chewings', Z, P, S)(k.inst, b'abc')
        length = k.fn('get_parsed_input_length', Z, P)(k.inst)
        return {'scheme': scheme, 'selected': selected, 'parsed': parsed, 'length': length}
    return probe


for _j_mode in ('pinyin', 'zhuyin'):
    for _j_scheme in (2, 9):
        case(f'j-chewing-multi-key-{_j_mode}-{_j_scheme}', mode=_j_mode,
             control=True)(j_multi_chewing(_j_scheme))


def run_worker(mode, so, data, name, scratch, extra_env=None, timeout=None):
    env = dict(os.environ, TMPDIR=str(scratch))
    env.update(extra_env or {})
    before = set(os.listdir(scratch))
    proc = subprocess.run([sys.executable, __file__, '--worker', mode, str(so), str(data), name],
                          capture_output=True, env=env, timeout=timeout)
    stdout = proc.stdout.decode('utf-8', 'replace')
    # stderr keeps invalid UTF-8 apart from a real U+FFFD (`surrogateescape`),
    # since the pin writes paths as their bytes.
    stderr = proc.stderr.decode('utf-8', 'surrogateescape')
    lines = [json.loads(line) for line in stdout.splitlines() if line.startswith('{')]
    return dict(exit=proc.returncode, result=lines[-1] if lines else None,
                userfiles=user_dir_listing(scratch, before),
                measurement_roots=[line.removeprefix('J-ROOT ') for line in stdout.splitlines() if line.startswith('J-ROOT ')],
                measurement_abort_flush=('J-GCOV-ABORT-BEGIN' in stdout.splitlines()
                                         and 'J-GCOV-ABORT-END' in stdout.splitlines()),
                stderr_lines=len(stderr.splitlines()),
                stderr=re.sub(r'(user|sys)-(\udcff)?[A-Za-z0-9_]+', r'\1-\2X', stderr))


# #640 leading separators, #698 retained before-cursor windows, #681 zero keys.
# Calls reuse the existing isolated worker and distinguish NULL from untouched.
def borrowed_text(pointer):
    if pointer.value==UNTOUCHED:
        return 'untouched'
    return C.string_at(pointer.value).decode() if pointer.value else None


def owned_text(k,pointer):
    return 'untouched' if pointer.value==UNTOUCHED else k.text(pointer.value)


def separator_case(text, op, off, options=0x18a):

    def probe(k):
        inst = k.inst
        k.fn('set_options', B, P, U)(k.ctx, options)
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        out = {'parse_return_consumed': parsed, 'get_parsed_input_length': k.fn('get_parsed_input_length', Z, P)(inst)}
        if op == 'parse':
            return out
        guessed = k.fn('guess_sentence', B, P)(inst)
        out['guess_sentence'] = guessed
        if op == 'sentence':
            ptr = P(UNTOUCHED)
            if k.mode == 'pinyin':
                ret = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(inst, 0, C.byref(ptr))
            else:
                ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(ptr))
            out.update(ret=ret, sentence='untouched' if ptr.value == UNTOUCHED else k.text(ptr.value))
            return out
        if op in ['candidates', 'before']:
            if k.mode == 'pinyin':
                ret = k.fn('guess_candidates', B, P, Z, U)(inst, off, 0x1e)
            else:
                ret = k.fn('guess_candidates_before_cursor' if op == 'before' else 'guess_candidates_after_cursor', B, P, Z)(inst, off)
            n = U(UNTOUCHED)
            got = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
            out.update(ret=ret, get_n=got, n=n.value)
            rows = []
            if got:
                for index in range(n.value):
                    candidate, kind, string = (P(UNTOUCHED), I(UNTOUCHED), P(UNTOUCHED))
                    fetched = k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(candidate))
                    typed = k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, candidate, C.byref(kind))
                    rendered = k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, candidate, C.byref(string))
                    row = [fetched, typed, kind.value, rendered, borrowed_text(string)]
                    if k.mode == 'pinyin' and kind.value == 1:
                        nbest = C.c_ubyte(0xab)
                        row.extend([k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(inst, candidate, C.byref(nbest)), nbest.value])
                    rows.append(row)
            out['rows'] = rows
            return out
        outval = Z(UNTOUCHED)
        if op in ['left', 'right', 'offset']:
            name = 'get_' + ('' + k.mode + '_offset' if op == 'offset' else op + '_' + k.mode + '_offset')
            ret = k.fn(name, B, P, Z, C.POINTER(Z))(inst, off, C.byref(outval))
            out.update(ret=ret, out=outval.value, untouched_output=outval.value == UNTOUCHED if not ret else True)
            return out
        if op == 'character':
            phrase = '阿' if text == "'a" else '你好' if 'hao' in text else '你'
            ret = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(inst, phrase.encode(), off, C.byref(outval))
            out.update(ret=ret, out=outval.value, phrase=phrase, untouched_output=outval.value == UNTOUCHED if not ret else True)
            return out
        ptr = P(UNTOUCHED)
        suffix = 'key_rest' if op == 'rest' else 'key'
        ret = k.fn('get_' + k.mode + '_' + suffix, B, P, Z, C.POINTER(P))(inst, off, C.byref(ptr))
        out.update(ret=ret, pointer='untouched' if ptr.value == UNTOUCHED else 'null' if not ptr.value else 'key')
        if not ret:
            out['untouched_or_null_pointer'] = ptr.value in (None, UNTOUCHED)
        if ret and ptr.value:
            out['bytes'] = C.string_at(ptr.value, 4 if op == 'rest' else 2).hex()
            if op == 'rest':
                begin, end, length = (C.c_ushort(0xbeef), C.c_ushort(0xbeef), C.c_ushort(0xbeef))
                out['positions_ret'] = k.fn('get_' + k.mode + '_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))(inst, ptr, C.byref(begin), C.byref(end))
                out['positions'] = [begin.value, end.value]
                out['length_ret'] = k.fn('get_' + k.mode + '_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(inst, ptr, C.byref(length))
                out['length'] = length.value
            if op == 'render':
                for name in ['pinyin', 'zhuyin', 'luoma_pinyin', 'secondary_zhuyin'] if k.mode == 'pinyin' else ['zhuyin', 'pinyin']:
                    string = P(UNTOUCHED)
                    rendered = k.fn('get_' + name + '_string', B, P, P, C.POINTER(P))(inst, ptr, C.byref(string))
                    out[name + '_string'] = [rendered, owned_text(k, string)]
                if k.mode == 'pinyin':
                    initial, final = (P(UNTOUCHED), P(UNTOUCHED))
                    rendered = k.fn('get_pinyin_strings', B, P, P, C.POINTER(P), C.POINTER(P))(inst, ptr, C.byref(initial), C.byref(final))
                    out['pinyin_strings'] = [rendered, owned_text(k, initial), owned_text(k, final)]
        return out
    return probe

SEPARATOR_INPUTS=("'ni","''ni","'nihao","'ni'hao","'ni'","'a")
SEPARATOR_ABORTS = {
    'zhuyin/3/before': (4,),
    'zhuyin/4/before': (4,),
    'pinyin/0/right': (3,),
    'pinyin/0/character': (0, 1, 2, 3),
    'pinyin/1/right': (4,),
    'pinyin/1/character': (0, 1, 2, 3, 4),
    'pinyin/2/right': (6,),
    'pinyin/2/character': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/3/left': (4,),
    'pinyin/3/right': (4, 7),
    'pinyin/3/character': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/4/left': (4,),
    'pinyin/4/right': (3, 4),
    'pinyin/4/character': (0, 1, 2, 3, 4),
    'pinyin/5/right': (2,),
    'pinyin/5/character': (0, 1, 2),
    'zhuyin/0/right': (3,),
    'zhuyin/0/character': (0, 1, 2, 3),
    'zhuyin/1/right': (4,),
    'zhuyin/1/character': (0, 1, 2, 3, 4),
    'zhuyin/2/right': (6,),
    'zhuyin/2/character': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/3/candidates': (4,),
    'zhuyin/3/left': (4, 7),
    'zhuyin/3/right': (4, 7),
    'zhuyin/3/key': (4,),
    'zhuyin/3/rest': (4,),
    'zhuyin/3/character': (0, 1, 2, 3, 4, 5, 6, 7),
    'zhuyin/4/candidates': (4,),
    'zhuyin/4/left': (4,),
    'zhuyin/4/right': (3, 4),
    'zhuyin/4/character': (0, 1, 2, 3, 4),
    'zhuyin/5/right': (2,),
    'zhuyin/5/character': (0, 1, 2),
}
SEPARATOR_CONTROLS = {
    'pinyin/0/parse': (0,),
    'pinyin/0/offset': (0, 1, 2, 3),
    'pinyin/0/left': (0, 1, 2, 3),
    'pinyin/0/right': (0, 1, 2, 3),
    'pinyin/0/key': (1, 2, 3),
    'pinyin/0/rest': (1, 2, 3),
    'pinyin/0/character': (0, 1, 2, 3),
    'pinyin/1/parse': (0,),
    'pinyin/1/offset': (0, 1, 2, 3, 4),
    'pinyin/1/left': (0, 1, 2, 3, 4),
    'pinyin/1/right': (0, 1, 2, 3, 4),
    'pinyin/1/key': (2, 3, 4),
    'pinyin/1/rest': (2, 3, 4),
    'pinyin/1/character': (0, 1, 2, 3, 4),
    'pinyin/2/parse': (0,),
    'pinyin/2/offset': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/2/left': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/2/right': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/2/key': (1, 2, 3, 4, 5, 6),
    'pinyin/2/rest': (1, 2, 3, 4, 5, 6),
    'pinyin/2/character': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/3/parse': (0,),
    'pinyin/3/offset': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/left': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/right': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/key': (1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/rest': (1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/character': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/4/parse': (0,),
    'pinyin/4/offset': (0, 1, 2, 3, 4),
    'pinyin/4/left': (0, 1, 2, 3, 4),
    'pinyin/4/right': (0, 1, 2, 3, 4),
    'pinyin/4/key': (1, 2, 4),
    'pinyin/4/rest': (1, 2, 4),
    'pinyin/4/character': (0, 1, 2, 3, 4),
    'pinyin/5/parse': (0,),
    'pinyin/5/offset': (0, 1, 2),
    'pinyin/5/left': (0, 1, 2),
    'pinyin/5/right': (0, 1, 2),
    'pinyin/5/key': (1, 2),
    'pinyin/5/rest': (1, 2),
    'pinyin/5/character': (0, 1, 2),
    'zhuyin/0/parse': (0,),
    'zhuyin/0/offset': (0, 1, 2, 3),
    'zhuyin/0/left': (0, 1, 2, 3),
    'zhuyin/0/right': (0, 1, 2, 3),
    'zhuyin/0/key': (1, 2, 3),
    'zhuyin/0/rest': (1, 2, 3),
    'zhuyin/0/character': (0, 1, 2),
    'zhuyin/1/parse': (0,),
    'zhuyin/1/offset': (0, 1, 2, 3, 4),
    'zhuyin/1/left': (0, 1, 2, 3, 4),
    'zhuyin/1/right': (0, 1, 2, 3, 4),
    'zhuyin/1/key': (2, 3, 4),
    'zhuyin/1/rest': (2, 3, 4),
    'zhuyin/1/character': (0, 1, 2, 3),
    'zhuyin/2/parse': (0,),
    'zhuyin/2/offset': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/2/left': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/2/right': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/2/key': (1, 2, 3, 4, 5, 6),
    'zhuyin/2/rest': (1, 2, 3, 4, 5, 6),
    'zhuyin/2/character': (0, 1, 2, 3, 4, 5),
    'zhuyin/3/parse': (0,),
    'zhuyin/3/offset': (0, 1, 2, 3, 4, 5, 6, 7),
    'zhuyin/3/left': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/3/right': (0, 1, 2, 3, 4, 5, 6, 7),
    'zhuyin/3/key': (1, 2, 5, 6, 7),
    'zhuyin/3/rest': (1, 2, 5, 6, 7),
    'zhuyin/3/character': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/4/parse': (0,),
    'zhuyin/4/offset': (0, 1, 2, 3, 4),
    'zhuyin/4/left': (0, 1, 2, 3, 4),
    'zhuyin/4/right': (0, 1, 2, 3, 4),
    'zhuyin/4/key': (1, 2, 4),
    'zhuyin/4/rest': (1, 2, 4),
    'zhuyin/4/character': (0, 1, 2, 3),
    'zhuyin/5/parse': (0,),
    'zhuyin/5/offset': (0, 1, 2),
    'zhuyin/5/left': (0, 1, 2),
    'zhuyin/5/right': (0, 1, 2),
    'zhuyin/5/key': (1, 2),
    'zhuyin/5/rest': (1, 2),
    'zhuyin/5/character': (0, 1),
}
for mode in ('pinyin','zhuyin'):
    for index,text in enumerate(SEPARATOR_INPUTS):
        operations=['parse','sentence','candidates','offset','left','right','key','rest','character']
        if mode=='zhuyin':
            operations.append('before')
        for op in operations:
            for offset in ([0] if op in ('parse','sentence') else range(len(text)+1)):
                identity=f'{mode}/{index}/{op}/{offset}'
                kwargs={'mode':mode,'control':offset in SEPARATOR_CONTROLS.get(f'{mode}/{index}/{op}', ())}
                if offset in SEPARATOR_ABORTS.get(f"{mode}/{index}/{op}", ()):
                    kwargs['abort']=False
                case('separator/'+identity,**kwargs)(separator_case(text,op,offset))
BEFORE_CURSOR_INPUTS=('ni','hao','wo','shi','zhong','guo','xian','tian',
    'nihao','zhongguo','beijing','shanghai','renmin','nihaoshijie',
    'zhonghuarenmin','beijingdaxue','shijieheping','nih','zhongg',
    'beijin','nihaoshij','ni3','ni3hao3','zhong1guo2')
BEFORE_CURSOR_CONTROLS=(('ni', 0), ('ni', 2), ('hao', 0), ('hao', 3), ('wo', 0), ('wo', 2), ('shi', 0), ('shi', 3), ('zhong', 0), ('zhong', 5), ('guo', 0), ('guo', 3), ('xian', 0), ('tian', 0), ('nihao', 0), ('nihao', 2), ('nihao', 5), ('zhongguo', 0), ('zhongguo', 5), ('zhongguo', 8), ('beijing', 0), ('beijing', 3), ('beijing', 7), ('shanghai', 0), ('shanghai', 5), ('shanghai', 8), ('renmin', 0), ('renmin', 3), ('renmin', 6), ('nihaoshijie', 0), ('nihaoshijie', 2), ('nihaoshijie', 5), ('nihaoshijie', 8), ('zhonghuarenmin', 0), ('zhonghuarenmin', 5), ('zhonghuarenmin', 8), ('zhonghuarenmin', 11), ('zhonghuarenmin', 14), ('beijingdaxue', 0), ('beijingdaxue', 3), ('beijingdaxue', 7), ('beijingdaxue', 9), ('beijingdaxue', 12), ('shijieheping', 0), ('shijieheping', 3), ('shijieheping', 8), ('shijieheping', 12), ('nih', 0), ('nih', 2), ('nih', 3), ('zhongg', 0), ('zhongg', 5), ('zhongg', 6), ('beijin', 0), ('beijin', 3), ('beijin', 6), ('nihaoshij', 0), ('nihaoshij', 2), ('nihaoshij', 5), ('nihaoshij', 8), ('nihaoshij', 9), ('ni3', 0), ('ni3', 3), ('ni3hao3', 0), ('ni3hao3', 3), ('ni3hao3', 7), ('zhong1guo2', 0), ('zhong1guo2', 6), ('zhong1guo2', 10))
for text in BEFORE_CURSOR_INPUTS:
    options=0x1aa if any(char.isdigit() for char in text) else 0x18a
    for offset in range(len(text)+1):
        case(f'before-cursor/{text}/{offset}',mode='zhuyin',
             control=(text,offset) in BEFORE_CURSOR_CONTROLS)(separator_case(text,'before',offset,options))
        for op in ('candidates','key','rest'):
            case(f'separator-free-control/{text}/{op}/{offset}',control=True)(
                separator_case(text,op,offset,options))
    case(f'separator-free-control/{text}/sentence',control=True)(
        separator_case(text,'sentence',0,options))
for mode in ('pinyin','zhuyin'):
    for text in ("ni'","ni'hao","ni''hao","ni'hao'"):
        for op in ('key','rest'):
            for offset in range(len(text)+1):
                previous_zero=offset>0 and text[offset-1]=="'"
                is_zero=offset<len(text) and text[offset]=="'"
                tail_zero=is_zero and all(char=="'" for char in text[offset:])
                kwargs={'mode':mode,'control':not (tail_zero if mode=='pinyin' else is_zero) and not (mode=='zhuyin' and previous_zero and offset<len(text))}
                if mode=='zhuyin' and previous_zero and offset<len(text):
                    kwargs['abort']=False
                case(f'zero-key/{mode}/{text}/{op}/{offset}',**kwargs)(separator_case(text,op,offset))
for mode,text,offset in (('pinyin',"xi'",2),('pinyin',"xi''",2),
                         ('pinyin',"xi''",3),('zhuyin',"xi1'an1",3),
                         ('zhuyin',"xi1'",3)):
    for op in ('key','rest','render'):
        case(f'allocator-zero/{mode}/{text}/{op}/{offset}',mode=mode)(
            separator_case(text,op,offset,0x1aa))
case("allocator-zero/zhuyin/xi1'an1/key/4",mode='zhuyin',abort=False)(
    separator_case("xi1'an1",'key',4,0x1aa))


# #681: the eight "unexecuted" string-getter lines of the allocator registers
# (E3-E10). Each getter's false branch is `0 == key->get_table_index()`
# (pinyin.cpp:2707-2762, zhuyin.cpp:1732-1760); a conforming consumer reaches
# it with the zero key the key getter answers at a separator column. The
# case records the return and the state of every out-parameter (`untouched`
# is the 0xABCDEF sentinel, `null` a NULL write); the control calls the same
# getter on a real key.
def alloc_register_case(mode, text, offset, which):
    def probe(k):
        inst = k.inst
        k.fn('set_options', B, P, U)(k.ctx, 0x1aa)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        out = {'guess_sentence': k.fn('guess_sentence', B, P)(inst)}
        key = P(UNTOUCHED)
        out['key_ret'] = k.fn('get_' + mode + '_key', B, P, Z, C.POINTER(P))(inst, offset, C.byref(key))
        if not out['key_ret'] or not key.value:
            return out
        out['key_bytes'] = C.string_at(key.value, 2).hex()
        if which == 'strings':
            initial, final = (P(UNTOUCHED), P(UNTOUCHED))
            out['ret'] = k.fn('get_pinyin_strings', B, P, P, C.POINTER(P), C.POINTER(P))(inst, key, C.byref(initial), C.byref(final))
            out['shengmu'], out['yunmu'] = owned_text(k, initial), owned_text(k, final)
        else:
            string = P(UNTOUCHED)
            out['ret'] = k.fn('get_' + which + '_string', B, P, P, C.POINTER(P))(inst, key, C.byref(string))
            out['out'] = owned_text(k, string)
        return out
    return probe


for _mode, _text, _zero, _real, _names in (
        ('pinyin', "xi'", 2, 0, ('pinyin', 'zhuyin', 'luoma_pinyin', 'secondary_zhuyin', 'strings')),
        ('zhuyin', "xi1'", 3, 0, ('zhuyin', 'pinyin'))):
    for _name in _names:
        case(f'alloc-register-{_mode}_get_{_name}-zero-key', mode=_mode, control=True)(
            alloc_register_case(_mode, _text, _zero, _name))
        case(f'alloc-register-{_mode}_get_{_name}-real-key', mode=_mode, control=True)(
            alloc_register_case(_mode, _text, _real, _name))



# #695, row 54: parse replaces the matrix even for empty input; every
# candidate guess searches the currently loaded libraries without a reparse.
def row54_empty(parse_name, text):
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn(parse_name, Z, P, S)
        guess = k.fn('guess_sentence', B, P)
        out = {'first_parse': parse(inst, text), 'first_guess': guess(inst)}
        out['empty_parse'] = parse(inst, b'')
        out['empty_guess'] = guess(inst)
        out['reparse'] = parse(inst, text)
        out['recovered_guess'] = guess(inst)
        return out
    return probe


def row54_unload(direction, reparse):
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn('parse_more_full_pinyins', Z, P, S)
        out = {'parse': parse(inst, b'nihao')}
        def rows():
            if k.mode == 'pinyin':
                ret = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
            else:
                ret = k.fn('guess_candidates_' + direction + '_cursor', B, P, Z)(
                    inst, 0 if direction == 'after' else 5)
            count = U(UNTOUCHED)
            counted = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            strings = []
            for index in range(count.value):
                candidate, text = P(), S()
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(candidate))
                assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, candidate, C.byref(text))
                strings.append(text.value.decode())
            return [ret, counted, count.value, strings]
        out['before'] = rows()
        out['unload'] = k.fn('unload_phrase_library', B, P, C.c_ubyte)(k.ctx, 2)
        if reparse:
            out['reparse'] = parse(inst, b'nihao')
        out['after'] = rows()
        out['repeated'] = rows()
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _parser, _text in (('full_pinyins', b'nihao'), ('chewings', b'su3cl3')):
        case(f'row54-empty-{_mode}-{_parser}', mode=_mode)(
            row54_empty('parse_more_' + _parser, _text))
    for _reparse in (False, True):
        case(f'row54-unload-{_mode}' + ('-reparse' if _reparse else ''),
             mode=_mode, control=_reparse)(row54_unload('after', _reparse))
case('row54-empty-pinyin-double')(row54_empty('parse_more_double_pinyins', b'nihk'))
case('row54-unload-zhuyin-before', mode='zhuyin', control=True)(row54_unload('before', False))



# PR #709 review: keep a non-first NORMAL choice through an empty parse
# until a sentence guess validates the constraints against that empty matrix.
def row54_forcing(sequence, restored=b'nihao'):
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn('parse_more_full_pinyins', Z, P, S)
        guess = k.fn('guess_sentence', B, P)
        def sentence():
            text = P(UNTOUCHED)
            if k.mode == 'pinyin':
                ret = k.fn('get_sentence', B, P, U, C.POINTER(P))(inst, 0, C.byref(text))
            else:
                ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(text))
            return [ret, 'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        out = {'parse': parse(inst, b'nihao'), 'initial_guess': guess(inst),
               'initial_sentence': sentence()}
        if k.mode == 'pinyin':
            out['candidates'] = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
        else:
            out['candidates'] = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
        count = U()
        assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
        chosen = None
        for index in range(count.value):
            cand, text, kind = P(), S(), I()
            assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
            assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
            assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
            if kind.value == 2 and text.value == '泥'.encode():
                assert index > 0, 'the chosen NORMAL candidate must not be first'
                out['chosen_index'], out['chosen_text'] = index, text.value.decode()
                chosen = cand
                break
        assert chosen, 'missing non-first NORMAL candidate 泥'
        out['choose'] = k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
        if sequence != 'no-empty':
            out['empty_parse'] = parse(inst, b'')
            if sequence == 'empty-guess':
                out['empty_guess'] = guess(inst)
        out['restored_parse'] = parse(inst, restored)
        out['restored_guess'] = guess(inst)
        out['restored_sentence'] = sentence()
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _sequence in ('empty', 'empty-guess', 'no-empty'):
        case(f'row54-forcing-{_mode}-{_sequence}', mode=_mode,
             control=_sequence != 'empty')(row54_forcing(_sequence))


# #710, row 53 item 2: every zero-consumption parse leaves an empty
# matrix. get_nbest_match returns false before clearing sentence rows
# (074a2219, phonetic_lookup.h:743-748), including the prefix-seeded path.
def zero_consumption_sentence(parser, text, with_prefix):
    def probe(k):
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        out = {}
        for stale in (False, True):
            inst = k.alloc()
            def sentence():
                value = P(UNTOUCHED)
                if k.mode == 'pinyin':
                    ret = k.fn('get_sentence', B, P, U, C.POINTER(P))(inst, 0, C.byref(value))
                else:
                    ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(value))
                return [ret, 'untouched' if value.value == UNTOUCHED else k.text(value.value)]
            result = {}
            if stale:
                result['initial_parse'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
                result['initial_guess'] = k.fn('guess_sentence', B, P)(inst)
                result['initial_sentence'] = sentence()
            result['parse'] = k.fn('parse_more_' + parser, Z, P, S)(inst, text)
            assert result['parse'] == 0, 'the case must leave an empty matrix'
            if with_prefix:
                result['guess'] = k.fn('guess_sentence_with_prefix', B, P, S)(inst, '你'.encode())
            else:
                result['guess'] = k.fn('guess_sentence', B, P)(inst)
            result['sentence'] = sentence()
            out['stale' if stale else 'fresh'] = result
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _parsers = ('full_pinyins', 'chewings', 'double_pinyins') if _mode == 'pinyin' else ('full_pinyins', 'chewings')
    for _parser in _parsers:
        _texts = (b'n', b'!', b'') if _parser == 'full_pinyins' else (b'!', b'')
        for _text in _texts:
            for _prefix in (False, True):
                _label = _text.decode() or 'empty'
                case(f'zero-consumption-{_mode}-{_parser}-{_label}' + ('-prefix' if _prefix else ''),
                     mode=_mode, control=_mode == 'pinyin')(
                    zero_consumption_sentence(_parser, _text, _prefix))


# #710: parsing never touches m_constraints. At 074a2219,
# lookup/phonetic_lookup.cpp:120-160 first resizes to matrix.size(), then
# clears only ONESTEP spans ending >= size or whose pronunciation
# possibility is < FLT_EPSILON. A guess (or a choose) validates; a parse
# alone, including an empty or temporarily incompatible parse, does not.
# Compare complete sentence text, not just guess success or a prefix.
def changed_parse_constraints(initial, choices, replacement, sequence,
                              intermediate=None):
    def probe(k):
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn('parse_more_full_pinyins', Z, P, S)
        guess = k.fn('guess_sentence', B, P)

        def sentence():
            text = P(UNTOUCHED)
            if k.mode == 'pinyin':
                ret = k.fn('get_sentence', B, P, U, C.POINTER(P))(inst, 0, C.byref(text))
            else:
                ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(text))
            return [ret, 'untouched' if text.value == UNTOUCHED else k.text(text.value)]

        def choose(offset, wanted):
            if k.mode == 'pinyin':
                assert k.fn('guess_candidates', B, P, Z, U)(inst, offset, 0x1e)
            else:
                assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, offset)
            count = U()
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            for index in range(count.value):
                cand, text, kind = P(), S(), I()
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
                assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
                assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
                if kind.value == 2 and text.value.decode() == wanted:
                    return [wanted, k.fn('choose_candidate', I, P, Z, P)(inst, offset, cand)]
            raise AssertionError('missing NORMAL candidate: ' + wanted)

        out = {'parse': parse(inst, initial.encode()), 'initial_guess': guess(inst),
               'initial_sentence': sentence()}
        out['choices'] = [choose(offset, text) for offset, text in choices]
        if sequence == 'guess':
            out['chosen_guess'] = guess(inst)
            out['chosen_sentence'] = sentence()
        if sequence in ('empty', 'empty-guess'):
            out['empty_parse'] = parse(inst, b'')
            if sequence == 'empty-guess':
                out['empty_guess'] = guess(inst)
        if intermediate is not None:
            out['intermediate_parse'] = parse(inst, intermediate.encode())
            if sequence == 'intermediate-guess':
                out['intermediate_guess'] = guess(inst)
                out['intermediate_sentence'] = sentence()
        out['replacement_parse'] = parse(inst, replacement.encode())
        out['replacement_guess'] = guess(inst)
        out['replacement_sentence'] = sentence()
        out['cleared_constraints'] = [k.fn('clear_constraint', B, P, Z)(inst, offset)
                                      for offset, _ in choices]
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _name, _initial, _choices, _replacement, _control in (
            ('changed-suffix', 'nihao', ((0, '泥'),), 'nihai', False),
            ('shorter', 'nihao', ((0, '泥'),), 'ni', True),
            ('longer', 'nihao', ((0, '泥'),), 'nihaoma', True),
            ('different-first', 'nihao', ((0, '泥'),), 'nahao', True),
            ('span-past-end', 'nihao', ((2, '浩'),), 'ni', True),
            ('whole-span-past-end', 'nihaoshijie', ((0, '你好'),), 'ni', True),
            ('edit-within-span', 'nihaoshijie', ((0, '你好'),), 'nihaishijie', True),
            ('whole-span-kept', 'nihaoshijie', ((0, '你好'),), 'nihaoshijian', False),
            ('two-choices', 'nihao', ((0, '泥'), (2, '浩')), 'nihai', False)):
        for _sequence in ('direct', 'guess', 'empty', 'empty-guess'):
            case(f'parse-constraints-{_mode}-{_name}-{_sequence}', mode=_mode,
                 control=_control or _sequence == 'empty-guess')(
                changed_parse_constraints(_initial, _choices, _replacement, _sequence))
    # A temporary mismatch or truncation must not destroy a choice until
    # something validates that intermediate matrix.
    for _name, _intermediate in (('temporary-mismatch', 'nahao'),
                                ('temporary-shorter', 'n')):
        for _sequence in ('direct', 'intermediate-guess'):
            case(f'parse-constraints-{_mode}-{_name}-{_sequence}', mode=_mode,
                 control=_sequence == 'intermediate-guess' and not (
                     _mode == 'zhuyin' and _name == 'temporary-shorter'))(
                changed_parse_constraints('nihao', ((0, '泥'),), 'nihai',
                                          _sequence, _intermediate))


# #697, row 58: capture the entire list, including row types and order.
def row58_rows(k, prefix):
    inst = k.inst
    ret = k.fn('guess_predicted_candidates', B, P, S)(inst, prefix.encode())
    n = U()
    assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
    rows = []
    for i in range(n.value):
        cand, kind, text = P(), I(), S()
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        rows.append([kind.value, text.value.decode()])
    return dict(ret=ret, rows=rows)


def row58_seed_bigrams(k):
    # Constrain valid n-best results before training, following the existing
    # training cases. Shanghai has more bigram observations; independently
    # boost Beijing's unigram so those two proposed sort keys disagree.
    k.fn('set_options', B, P, U)(k.ctx, 0x18a | (1 << 9))
    for text, phrase, repeats in ((b'wobeijing', '北京', 1), (b'woshanghai', '上海', 3)):
        inst = k.inst
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, text) == len(text)
        assert k.fn('guess_sentence', B, P)(inst)
        offset = choose_text(k, inst, 0, '我')
        assert offset == 2
        assert choose_text(k, inst, offset, phrase) == len(text)
        assert k.fn('guess_sentence', B, P)(inst)
        for _ in range(repeats):
            assert k.fn('train', B, P, C.c_ubyte)(inst, 0)
        assert k.fn('reset', B, P)(inst)
    beijing, shanghai = tokens_of(k, '北京')[0], tokens_of(k, '上海')[0]
    assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, beijing, 100000)
    return {'北京': unigram_of(k, beijing), '上海': unigram_of(k, shanghai)}


def row58_scoring(dynamic):
    def probe(k):
        counts = row58_seed_bigrams(k)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        return dict(unigrams=counts, prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-bigram-scoring-' + ('on' if _dynamic else 'off'))(row58_scoring(_dynamic))


def row58_choose(k, kind, text, times):
    n = U()
    assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(n))
    for i in range(n.value):
        cand, found_kind, found_text = P(), I(), S()
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, i, C.byref(cand))
        assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(k.inst, cand, C.byref(found_kind))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(k.inst, cand, C.byref(found_text))
        if found_kind.value == kind and found_text.value.decode() == text:
            return [k.fn('choose_predicted_candidate', B, P, P)(k.inst, cand) for _ in range(times)]
    raise AssertionError('missing predicted row: %s %s' % (kind, text))


def row58_prefix(dynamic, text, times):
    def probe(k):
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        token = tokens_of(k, '我' + text)[0]
        before = row58_rows(k, '我')
        count_before = unigram_of(k, token)
        chosen = row58_choose(k, 5, text, times)
        return dict(before=before, count_before=count_before, chosen=chosen,
                    count_after=unigram_of(k, token), after=row58_rows(k, '我'))
    return probe


def row58_bigram_choices(dynamic):
    def probe(k):
        counts = row58_seed_bigrams(k)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        out = dict(unigrams=counts, before=row58_rows(k, '我'))
        # Fixed observation points, not a ranking threshold: each acceptance
        # contributes 483 unigram and 69 bigram independently of options.
        for times in (1, 20, 200):
            out['chosen-%d' % times] = row58_choose(k, 4, '上海', times)
            out['rows-%d' % times] = row58_rows(k, '我')
        return out
    return probe


def row58_import(k, rows):
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert it
    for text, reading, count in rows:
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(it, text.encode(), reading.encode(), count)
    k.fn('end_add_phrases', None, P)(it)


def row58_conversion(dynamic):
    def probe(k):
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        # Unigram = import count * 3. Counts 3/6 quantize to the same
        # amplified integer; 99 ties a baked 100 after amplification.
        # Insert in reverse UCS-4 byte order to distinguish order from token.
        row58_import(k, [('我乙乙', "wo3'yi3'yi3", 33),
                         ('我甲甲', "wo3'jia3'jia3", 33),
                         ('我丙丙', "wo3'bing3'bing3", 1),
                         ('我丁丁', "wo3'ding1'ding1", 2)])
        out = dict(ties=row58_rows(k, '我'))
        row58_import(k, [('我戊戊', "wo3'wu4'wu4", 5592405),
                         ('我己己', "wo3'ji3'ji3", 5592406)])
        # 2^24-1 and 2^24+2: the integer-to-float representability edge.
        out['float-edge'] = row58_rows(k, '我')
        out['unigrams'] = {text: unigram_of(k, tokens_of(k, text)[0])
                           for text in ('我戊戊', '我己己')}
        return out
    return probe


def row58_lambda(dynamic):
    def probe(k):
        system = private_system(k)
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        conf.unlink()
        conf.write_text(re.sub(r'lambda parameter:[^\n]+', 'lambda parameter:1', text))
        k._ctx = k.init(system=system)
        assert k._ctx
        # lambda=1 makes every predicted score zero. A nonuniform unigram
        # overlay must therefore leave only the source collection tie order.
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        token = tokens_of(k, '我是谁')[0]
        assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 100000)
        return dict(prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    _state = 'on' if _dynamic else 'off'
    case('row58-prefix-no-bigram-' + _state)(row58_prefix(_dynamic, '是', 1))
    case('row58-prefix-live-21-' + _state)(row58_prefix(_dynamic, '是谁', 21))
    case('row58-bigram-choices-' + _state)(row58_bigram_choices(_dynamic))
    case('row58-conversions-' + _state)(row58_conversion(_dynamic))
    case('row58-lambda-ties-' + _state)(row58_lambda(_dynamic))


def row58_prefix_overflow(dynamic):
    def probe(k):
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        # Library 7 has only 399 left before guint32::MAX. The next 483
        # cannot fit, but the facade total still advances and wraps. An
        # explicit prefix keeps this refusal observable in the parent too;
        # the conversion cases separately cover implicit prefix index keys.
        row58_import(k, [('我', 'wo3', 1),
                         ('我甲甲', "wo3'jia3'jia3", 1431655631)])
        token = tokens_of(k, '我甲甲')[0]
        before = row58_rows(k, '我')
        count_before = unigram_of(k, token)
        chosen = row58_choose(k, 5, '甲甲', 1)
        return dict(before=before, count_before=count_before, chosen=chosen,
                    count_after=unigram_of(k, token), after=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-prefix-overflow-' + ('on' if _dynamic else 'off'))(row58_prefix_overflow(_dynamic))


def row58_signed_score(dynamic, duplicate=False, denominator=20000000):
    def probe(k):
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        phrase = ('我的心', "wo3'de5'xin1") if duplicate else ('我甲甲', "wo3'jia3'jia3")
        row58_import(k, [(*phrase, 1431655631)])
        # The absent-token add advances only the facade total. Make the
        # denominator 20,000,000: score exceeds gint::MAX; at 10,000,000
        # it also exceeds guint32::MAX. Pin x86-64 converts via gint64
        # then retains the low 32 bits, rather than saturating.
        total = (51051831 + 1431655631 * 3) % (1 << 32)
        delta = (denominator - total) % (1 << 32)
        added = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, 0x01ffffff, delta)
        return dict(absent_add=added, prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    _state = 'on' if _dynamic else 'off'
    case('row58-signed-score-' + _state)(row58_signed_score(_dynamic))
    case('row58-signed-duplicate-' + _state)(row58_signed_score(_dynamic, True))
    case('row58-unsigned-score-' + _state)(row58_signed_score(_dynamic, denominator=10000000))


def row58_zero_total(dynamic, punct=False):
    def probe(k):
        tokens = {token for text in ('我', '北京', '上海') for token in tokens_of(k, text)}
        before = sum(unigram_of(k, token)[1] for token in tokens)
        row58_seed_bigrams(k)
        after = sum(unigram_of(k, token)[1] for token in tokens)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        # Only these three phrases were trained/added: use their live delta,
        # including 我's reselection seed, rather than a guessed threshold.
        total = FACADE_TOTAL + after - before
        added = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, 0x01ffffff,
                                                            (-total) % (1 << 32))
        name = 'guess_predicted_candidates_with_punctuations' if punct else 'guess_predicted_candidates'
        ret = k.fn(name, B, P, S)(k.inst, '我'.encode())
        n = U()
        assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(n))
        if not ret:
            assert n.value == 0
        return dict(add=added, ret=ret, rows=[] if n.value == 0 else ['not empty'])
    return probe


for _dynamic in (False, True):
    for _punct in (False, True):
        name = 'row58-total-zero-' + ('punct-' if _punct else '') + ('on' if _dynamic else 'off')
        case(name, abort=False)(row58_zero_total(_dynamic, _punct))


def row58_prefix_zero_total(dynamic):
    def probe(k):
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        added = wrap_total_to_zero(k, k.inst)
        # Prefix rows bypass pinyin.cpp:1859. The pin's invalid conversion
        # has zero low bits; the complete list retains collection tie order.
        return dict(add=added, prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-prefix-zero-total-' + ('on' if _dynamic else 'off'), control=True)(
        row58_prefix_zero_total(_dynamic))


def row58_lambda_rounding(dynamic):
    def probe(k):
        system = private_system(k)
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        conf.unlink()
        # Just below the f32 midpoint between 1 and its predecessor. Direct
        # %f parsing yields the predecessor; a rational -> f64 -> f32 rounds
        # to 1 and erases every score. The large live user count exposes it.
        conf.write_text(re.sub(r'lambda parameter:[^\n]+',
                              'lambda parameter:0.9999999701976776123036', text))
        k._ctx = k.init(system=system)
        assert k._ctx
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        row58_import(k, [('我甲甲', "wo3'jia3'jia3", 1431655631)])
        return dict(prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-lambda-rounding-' + ('on' if _dynamic else 'off'))(row58_lambda_rounding(_dynamic))



# #712 review 4232017129: an imported system-library item has a complete
# override count, whereas an immutable item still needs its store delta.
def row58_system_import(library, dynamic=False, existing=False):
    def probe(k):
        phrase = '我是谁' if existing else '我甲甲'
        prefix, suffix = '我', ('是谁' if existing else '甲甲')
        if k.mode == 'pinyin':
            reading = "wo3'shi4'shei2" if existing else "wo3'jia3'jia3"
            assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        else:
            reading = 'ㄨㄛˇ ㄕˋ ㄕㄟˊ' if existing else 'ㄨㄛˇ ㄐㄧㄚˇ ㄐㄧㄚˇ'
        initial = tokens_of(k, phrase)
        target_library = (initial[0] >> 24) if existing else library
        assert 1 <= target_library <= 4
        before = [t for t in initial if t >> 24 == target_library]
        assert bool(before) == existing
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, target_library)
        assert it
        added = k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, phrase.encode(), reading.encode(), 33)
        assert added
        k.fn('end_add_phrases', None, P)(it)
        tokens = [t for t in tokens_of(k, phrase) if t >> 24 == target_library]
        assert len(tokens) == 1
        token = tokens[0]
        out = dict(tokens_before=before, added=added, tokens_after=tokens,
                   imported_count=unigram_of(k, token))
        if k.mode == 'pinyin':
            if not existing:
                # Place an immutable competitor between the correct count
                # after three accepts (33*3 + 3*483) and its erroneous double.
                rival = tokens_of(k, '我是谁')[0]
                rival_count = (33 * 3 + 3 * 483) * 3 // 2
                delta = rival_count - unigram_of(k, rival)[1]
                assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, rival, delta)
                out['rival_count'] = unigram_of(k, rival)
            out['rows-0'] = row58_rows(k, prefix)
            # Three acceptances in two snapshots exercise the live override,
            # not only its initial import value. Preserve every candidate.
            for accepted in (1, 2):
                out['chosen-%d' % accepted] = row58_choose(k, 5, suffix, accepted)
                out['count-%d' % accepted] = unigram_of(k, token)
                out['rows-%d' % accepted] = row58_rows(k, prefix)
        return out
    return probe


for _library in range(1, 5):
    for _dynamic in (False, True):
        case('row58-system-import-%d-%s' % (_library, 'on' if _dynamic else 'off'))(
            row58_system_import(_library, _dynamic))
    # The import/count twin exists; libzhuyin has no predicted API twin.
    case('row58-system-import-zhuyin-%d' % _library, mode='zhuyin', control=True)(
        row58_system_import(_library))
for _dynamic in (False, True):
    case('row58-system-existing-' + ('on' if _dynamic else 'off'), control=True)(
        row58_system_import(4, _dynamic, existing=True))
case('row58-system-existing-zhuyin', mode='zhuyin', control=True)(
    row58_system_import(4, existing=True))


# #682/#683, pin 074a2219: forward resplit mutation, then divided, then
# fuzzy (phonetic_key_matrix.cpp:87-228; pinyin.cpp:1497-1524). Every
# offset reads the whole parse's matrix, including newly created columns.
# libzhuyin's full-pinyin seam omits both split passes (zhuyin.cpp:1017-
# 1040); cover that facade's distinct order too. Compare every ordered
# type/text row.
MATRIX_RESPLIT_INPUTS = (
    'banangang', 'baguanangang', 'chenanengang', 'anangang', 'fanangang',
    'lianai', 'xiane', 'liane', 'xianai', 'qiane', 'lian', 'xian',
)


def matrix_resplit_windows(text, options):
    def run(k):
        assert k.fn('set_options', B, P, U)(k.ctx, options)
        inst = k.inst
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        assert parsed == len(text), 'characterisation requires a complete parse'
        windows = []
        for offset in range(len(text) + 1):
            if k.mode == 'pinyin':
                windows.append(guess_rows(k, inst, offset, 0x1e))
            else:
                windows.append(zhuyin_guess_rows(k, inst, offset))
        return {'parsed': parsed, 'windows': windows}
    return run


for _mode in ('pinyin', 'zhuyin'):
    for _tables in (False, True):
        for _fuzzy in (False, True):
            # PINYIN_AMB_AN_ANG: expand the halves produced by the tables.
            _options = (0x18a if _tables else 0x0a) | ((1 << 17) if _fuzzy else 0)
            for _text in MATRIX_RESPLIT_INPUTS:
                case('matrix-resplit-%s-%s-%s-%s' % (
                    _mode, _text, 'tables' if _tables else 'plain',
                    'fuzzy' if _fuzzy else 'exact'), mode=_mode,
                    control=not _tables or _text in ('baguanangang', 'chenanengang')
                    or (_mode == 'pinyin' and _text in ('lian', 'xian')))(
                        matrix_resplit_windows(_text, _options))

# --------------------------------------------------------------------------
# #694 and #525 batch B: the system `table.conf` decides the library set.
# `SystemTableInfo2::load` (`table_info.cpp:194-294` at 074a2219) reads the
# header with five `fscanf`s and the rows as words in groups of six; the
# rows' file types and names drive `pinyin_init`/`zhuyin_init`'s library loop
# (`pinyin.cpp:377-392`), the loads, `_write_files` and `_rename_files`.
# Every case runs on a private copy of the oracle's system directory whose
# `table.conf` is rewritten; the data files are links to the oracle's.
# --------------------------------------------------------------------------

def conf_system(k, *edits):
    """A private system directory whose table.conf has `edits` applied:
    (old, new) replaces, ('+', line) appends a row, ('-', substring) drops
    the lines naming it, ('re', pattern, replacement) substitutes."""
    system = private_system(k)
    path = os.path.join(system, 'table.conf')
    # A name may hold a byte that is not UTF-8 (`\udcff`, round-tripped).
    text = Path(k.data, 'table.conf').read_text(errors='surrogateescape')
    for edit in edits:
        if edit[0] == '+':
            text += edit[1] + '\n'
        elif edit[0] == '-':
            text = ''.join(line for line in text.splitlines(True) if edit[1] not in line)
        elif edit[0] == 're':
            assert re.search(edit[1], text), edit
            text = re.sub(edit[1], edit[2], text)
        else:
            assert edit[0] in text, edit
            text = text.replace(edit[0], edit[1])
    os.unlink(path)
    Path(path).write_text(text, errors='surrogateescape')
    return system


CONF_TEXTS = {'pinyin': (b'nihao', b'yishu'), 'zhuyin': (b'ni3hao3', b'yi4shu4')}


def conf_guess_rows(k, inst, limit=12):
    """`guess_candidates` at offset 0: the call's answer and the first rows."""
    if k.mode == 'pinyin':
        ok = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
    else:
        ok = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    count = U(UNTOUCHED)
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    for index in range(min(count.value, limit)):
        cand, text = P(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        rows.append(text.value.decode())
    return ok, count.value, rows


def conf_sentence(k, inst):
    out = P(UNTOUCHED)
    if k.mode == 'pinyin':
        k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(inst, 0, C.byref(out))
    else:
        k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(out))
    return None if out.value == UNTOUCHED else k.text(out.value)


def conf_view(k, ctx):
    """Sentences and candidate lists for two inputs: the observable the
    loaded library set decides."""
    view = {}
    for text in CONF_TEXTS[k.mode]:
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
        k.fn('guess_sentence', B, P)(inst)
        view[text.decode()] = [conf_sentence(k, inst), *conf_guess_rows(k, inst)]
    return view


def user_listing(k, directory=None):
    """The files of the user directory (or of `directory`): names as bytes,
    `user.conf` by text, library chunks by size, and the DBM files named
    `user_*` by whether a chunk was written over them (a chunk's length word
    is its size less the 8-byte header)."""
    directory = directory or k.user
    listing = {}
    for name in sorted(os.listdir(directory)):
        file = os.path.join(directory, name)
        key = os.fsencode(name).decode('latin-1')
        if os.path.isdir(file):
            listing[key] = 'dir'
        elif name == 'user.conf':
            listing[key] = Path(file).read_text().replace('\n', '|')
        elif name.endswith(('.bin', '.dbin')):
            data = Path(file).read_bytes()
            framed = len(data) >= 8 and int.from_bytes(data[:4], 'little') == len(data) - 8
            listing[key] = len(data) if not name.startswith('user_') else ('chunk' if framed else 'dbm')
        else:
            listing[key] = None
    return listing


def conf_session(k, system):
    """Open on `system`, look up, learn one sentence and one phrase, save:
    what the rows decide to load, to accept and to write."""
    ctx = k.init(system=system)
    assert ctx, 'init failed'
    out = {'view': conf_view(k, ctx)}
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, CONF_TEXTS[k.mode][0])
    k.fn('guess_sentence', B, P)(inst)
    out['train'] = k.fn('train', B, P, C.c_ubyte)(inst, 0) if k.mode == 'pinyin' else k.fn('train', B, P)(inst)
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    reading = b"ce'shi'ci" if k.mode == 'pinyin' else 'ㄘㄜˋ ㄕˋ ㄘˊ'.encode()
    out['add'] = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '测试词'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    out['save'] = k.fn('save', B, P)(ctx)
    k.fn('fini', None, P)(ctx)
    out['files'] = user_listing(k)
    return out


def addon_session(k, system, indexes=(0, 3, 4, 5, 15)):
    """The addon libraries a row set loads, and the candidates they add."""
    ctx = k.init(system=system)
    assert ctx, 'init failed'
    load = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
    out = {'load': {str(index): load(ctx, index) for index in indexes}}
    out['view'] = conf_view(k, ctx)
    k.fn('fini', None, P)(ctx)
    return out


# Files the parent already answered like the pin: the header fields it read,
# the words it never needed, an alias, a row that changes nothing observable.
CONF_CONTROLS = {'addon-duplicate-row', 'addon-files-swapped', 'comment-dropped', 'extra-word-dropped',
                 'lambda-exponent', 'lambda-half', 'lambda-hex', 'lambda-hex-fraction', 'lambda-hex-upper',
                 'lambda-one', 'lambda-tiny', 'lambda-zero',
                 'reserved-as-system-file', 'short-row-dropped', 'source-format-zhuyin', 'tsi-is-gb',
                 'versions-other'}


# The libzhuyin candidate law read the pinned λ, not the file's, so these tie-order
# cases differ from the parent on libzhuyin only.
CONF_ZHUYIN_CHANGED = {'lambda-zero', 'lambda-one', 'lambda-tiny'}


def conf_case(name, *edits, modes=('pinyin', 'zhuyin'), session=conf_session, **options):
    for mode in modes:
        def probe(k, edits=edits):
            return session(k, conf_system(k, *edits))
        control = name in CONF_CONTROLS and not (mode == 'zhuyin' and name in CONF_ZHUYIN_CHANGED)
        case('table-conf-' + name + ('-zhuyin' if mode == 'zhuyin' else ''), mode=mode,
             **{'control': control, **options})(probe)


# Valid files that are not the stock one: the pin follows the rows.
_STOCK_DBIN = 'gb_char.dbin SYSTEM_FILE'
conf_case('row-removed-opengram', ('-', 'OPENGRAM_DICTIONARY'))
conf_case('row-removed-gbk', ('-', 'GBK_DICTIONARY'))
conf_case('row-removed-gb', ('-', 'GB_DICTIONARY'))
conf_case('not-used-opengram', ('opengram.dbin SYSTEM_FILE', 'opengram.dbin NOT_USED'))
conf_case('not-used-gb', (_STOCK_DBIN, 'gb_char.dbin NOT_USED'))
conf_case('not-used-merged', ('merged.dbin SYSTEM_FILE', 'merged.dbin NOT_USED'))
conf_case('not-used-user-dictionary', ('user.bin USER_FILE', 'user.bin NOT_USED'))
conf_case('not-used-network-dictionary', ('network.bin USER_FILE', 'network.bin NOT_USED'))
conf_case('user-files-renamed', ('gb_char.dbin', 'x_gb.dbin'), ('merged.dbin', 'x_merged.dbin'),
          ('user.bin', 'x_user.bin'), ('addon.bin', 'x_addon.bin'))
conf_case('tsi-is-gb', ('default GB_DICTIONARY', 'default TSI_DICTIONARY'))
conf_case('later-row-wins', ('+', 'default OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin NOT_USED'))
conf_case('network-as-system-file', ('default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE',
                                      'default NETWORK_DICTIONARY art.table art.bin network.dbin SYSTEM_FILE'))
conf_case('reserved-as-system-file', ('default RESERVED NULL NULL NULL NOT_USED',
                                       'default RESERVED merged.table merged.bin x_reserved.dbin SYSTEM_FILE'))
# λ spelled inside [0, 1]. The candidate law `(λ·bigram + (1−λ)·unigram)·2²⁴`
# reads the file's λ (it used to be the pinned constant, which listed tied
# libzhuyin rows in another order at λ = 0, 1 and 1e-30).
for _name, _value in (('zero', '0'), ('half', '0.5'), ('one', '1'), ('exponent', '5e-1'), ('tiny', '1e-30')):
    conf_case('lambda-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _value))
# λ outside [0, 1]: below 0 the possibilities exceed one and the costs go
# negative; above 1 `unigram_lambda` is negative, `log` of it is NaN and the
# NaN is sticky through the trellis. The tail sort (`g_ptr_array_sort` over a
# comparator that turns the float difference into a `gint`) then orders the
# sentences.
for _name, _value in (('negative-half', '-0.5'), ('negative-two', '-2'), ('one-and-a-half', '1.5'),
                      ('two', '2'), ('million', '1e6'), ('inf', 'inf'), ('nan', 'nan')):
    conf_case('lambda-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _value))
# The header fields the user marker conforms against.
conf_case('database-format-other', ('re', r'database format:\w+', lambda found: 'database format:' + (
    'BerkeleyDB' if found.group(0).endswith('Tkrzw') else 'Tkrzw')))
conf_case('versions-other', ('binary format version:7', 'binary format version:8'),
          ('model data version:14', 'model data version:15'))
conf_case('source-format-zhuyin', ('source table format:pinyin', 'source table format:zhuyin'))
# Words are read in groups of six: a short row, a comment and a seventh
# word at the end are dropped, and an addon index goes through a `guint8`.
conf_case('short-row-dropped', ('+', 'default GB_DICTIONARY a b c'))
conf_case('comment-dropped', ('+', '# a comment'))
conf_case('extra-word-dropped', ('+', 'default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE EXTRA'))
conf_case('addon-index-wraps', ('+', 'addon 256 art.table art.bin NULL DICTIONARY'),
          modes=('pinyin',), session=addon_session)
# The addon rows decide what `pinyin_load_addon_phrase_library` loads.
conf_case('addon-moved', ('addon 4 art.table', 'addon 3 art.table'), modes=('pinyin',), session=addon_session)
conf_case('addon-files-swapped', ('addon 4 art.table art.bin', 'addon 4 art.table culture.bin'),
          ('addon 5 culture.table culture.bin', 'addon 5 culture.table art.bin'),
          modes=('pinyin',), session=addon_session)
conf_case('addon-removed', ('-', 'addon 4 '), modes=('pinyin',), session=addon_session)
conf_case('addon-not-used', ('art.bin NULL DICTIONARY', 'art.bin NULL NOT_USED'), modes=('pinyin',), session=addon_session)
conf_case('addon-index-zero', ('addon 4 art.table', 'addon 0 art.table'), modes=('pinyin',), session=addon_session)
conf_case('addon-duplicate-row', ('+', 'addon 4 culture.table culture.bin NULL DICTIONARY'),
          modes=('pinyin',), session=addon_session)


# Class (c): the rows the pin dies on. The triggering call fails with one
# warning; `userdir` holds the profile the pin has written by then.
def conf_abort(name, *edits, call='init', userdir=True, modes=('pinyin', 'zhuyin'), ret=False):
    for mode in modes:
        def probe(k, edits=edits):
            system = conf_system(k, *edits)
            if call == 'init':
                return {'ret': bool(k.init(system=system))}
            ctx = k.init(system=system)
            assert ctx, 'init failed'
            kind, index = call
            if kind == 'load':
                return {'ret': k.fn('load_phrase_library', B, P, C.c_ubyte)(ctx, index)}
            return {'ret': k.fn('load_addon_phrase_library', B, P, C.c_ubyte)(ctx, index)}
        case('table-conf-abort-' + name + ('-zhuyin' if mode == 'zhuyin' else ''), mode=mode,
             abort=ret, userdir=userdir)(probe)


# `table_info.cpp` aborts inside `SystemTableInfo2::load`, before
# `check_format`: the user directory is untouched.
conf_abort('source-format-unknown', ('source table format:pinyin', 'source table format:foo'))
conf_abort('database-format-unknown', ('re', r'database format:\w+', 'database format:LMDB'))
conf_abort('target-unknown', ('+', 'foo GB_DICTIONARY a b c SYSTEM_FILE'))
conf_abort('default-name-unknown', ('+', 'default FOO_DICTIONARY a b c SYSTEM_FILE'))
conf_abort('file-type-unknown', ('+', 'default GB_DICTIONARY a b c FOO_FILE'))
conf_abort('addon-index-99', ('+', 'addon 99 x.table x.bin NULL DICTIONARY'))
conf_abort('addon-index-16', ('+', 'addon 16 x.table x.bin NULL DICTIONARY'))
conf_abort('addon-index-negative', ('+', 'addon -1 x.table x.bin NULL DICTIONARY'))
# `UserTableInfo::load` converts the `database format:` token of the user's
# `user.conf` and aborts on one it does not know (`table_info.cpp:132`,
# reached from `check_format`); the system `table.conf` reaches the same
# converter above.
@case('user-conf-abort-database-format-unknown', abort=False, userdir=True)
def _(k):
    _write_user_conf(k)
    path = os.path.join(k.user, 'user.conf')
    with open(path) as user_conf:
        text = user_conf.read()
    text = re.sub(r'database format:\w+', 'database format:LMDB', text)
    assert 'database format:LMDB' in text
    with open(path, 'w') as user_conf:
        user_conf.write(text)
    return {'ret': bool(k.init())}


# The init loop (`pinyin.cpp:377-392`) runs after `check_format`: the profile
# is judged and `user.conf` written when it dies.
conf_abort('default-dictionary', (_STOCK_DBIN, 'gb_char.dbin DICTIONARY'))
conf_abort('default-dictionary-gbk', ('gbk_char.dbin SYSTEM_FILE', 'gbk_char.dbin DICTIONARY'))
conf_abort('system-file-without-chunk', ('opengram.table opengram.bin opengram.dbin SYSTEM_FILE',
                                         'opengram.table NULL opengram.dbin SYSTEM_FILE'))
# A row without a user file makes the pin read the user directory as a chunk
# (`memory_chunk.h:434`): it asserts only where `lseek(SEEK_END)` on a
# directory answers 8 or more, which the file system decides. libpinyin has
# written `user.conf` by then, so its answer is the same everywhere; the
# zhuyin facade writes nothing at init, and an empty directory is 0 bytes on
# some file systems, so it is not held.
conf_abort('system-file-without-user-file', ('merged.table merged.bin merged.dbin SYSTEM_FILE',
                                             'merged.table merged.bin NULL SYSTEM_FILE'), modes=('pinyin',))
conf_abort('user-file-without-user-file', ('user.bin USER_FILE', 'NULL USER_FILE'), modes=('pinyin',))
# A library call on a row the pin asserts on (`pinyin.cpp:457`, `:491`).
conf_abort('load-library-not-used', ('opengram.dbin SYSTEM_FILE', 'opengram.dbin NOT_USED'),
           call=('load', 3), userdir=False)
conf_abort('load-library-row-removed', ('-', 'GBK_DICTIONARY'), call=('load', 2), userdir=False)
conf_abort('load-library-gb-not-used', (_STOCK_DBIN, 'gb_char.dbin NOT_USED'), call=('load', 1), userdir=False)
conf_abort('load-library-user-not-used', ('user.bin USER_FILE', 'user.bin NOT_USED'), call=('load', 7),
           userdir=False)
conf_abort('load-addon-system-file', ('art.bin NULL DICTIONARY', 'art.bin NULL SYSTEM_FILE'),
           call=('addon', 4), userdir=False, modes=('pinyin',))
conf_abort('load-addon-user-file', ('art.bin NULL DICTIONARY', 'art.bin NULL USER_FILE'),
           call=('addon', 4), userdir=False, modes=('pinyin',))
conf_abort('load-addon-without-chunk', ('art.table art.bin NULL DICTIONARY', 'art.table NULL NULL DICTIONARY'),
           call=('addon', 4), userdir=False, modes=('pinyin',))
# Class (b): the header's `source table format:` line is missing, and the pin
# compares a buffer it never wrote (undefined behaviour).
conf_abort('source-format-line-missing', ('-', 'source table format'))


# The pin's ordinary `false`: a header directive that does not match answers
# NULL with the raw `load %s failed!` line and no warning.
def header_probe(*edits):
    def probe(k):
        return {'ctx': bool(k.init(system=conf_system(k, *edits)))}
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    case('table-conf-lambda-line-missing' + _suffix, mode=_mode,
         stderr=True)(header_probe(('-', 'lambda parameter')))
    # glibc's scanner does not back up: an exponent mark with no digit after
    # it, a `0x` with no hex digit and `infin` fail the `%f` conversion.
    for _name, _spelling in (('exponent-without-digits', '1e'), ('hex-without-digits', '0x'),
                             ('hex-exponent-without-digits', '0x1p'), ('infin', 'infin')):
        # The oracle image's glibc rejects `1e` and `0x1p` (2.43); Ubuntu's
        # 2.39 accepts them, and the pin follows the host libc.
        case('table-conf-lambda-' + _name + _suffix, mode=_mode, stderr=True,
             host_scanf=(_spelling,) if _name.endswith('exponent-without-digits') else ())(header_probe(('lambda parameter:0.312699', 'lambda parameter:' + _spelling)))


# Every spelling glibc's `%f` accepts, hexadecimal floats included.
for _name, _spelling in (('hex', '0x1p-1'), ('hex-fraction', '0x.8'), ('hex-upper', '0X1.P-1')):
    conf_case('lambda-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _spelling),
              modes=('pinyin', 'zhuyin'))


# A file name never leaves its directory: `g_build_filename(user_dir,
# "/abs/u.bin")` is `<user_dir>/abs/u.bin`, not `/abs/u.bin`.
def absolute_user_file(k):
    nested = k.user + '/nest'
    os.makedirs(nested)
    os.makedirs(k.user + nested)
    out = conf_session(k, conf_system(k, ('user.bin USER_FILE', nested + '/u.bin USER_FILE')))
    out['beside'] = user_listing(k, nested)
    out['beneath'] = user_listing(k, k.user + nested)
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('table-conf-user-file-absolute' + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode)(absolute_user_file)

# The `strtol` behind `%d` and `atoi` saturates at LONG_MIN on a negative
# overflow, which the stores into an `int` and a `guint8` cut to 0.
conf_case('versions-negative-overflow', ('binary format version:7', 'binary format version:-9223372036854775808'),
          ('model data version:14', 'model data version:-99999999999999999999'))
conf_case('addon-index-negative-overflow', ('+', 'addon -99999999999999999999 art.table art.bin NULL DICTIONARY'),
          modes=('pinyin',), session=addon_session)

# The names are the bytes the word holds.
conf_case('user-file-not-utf8', ('user.bin USER_FILE', 'u\udcff.bin USER_FILE'))

# `_write_files` writes the library files by sub-index, then the two indices
# and the bigram, and `_rename_files` renames in the same order: where a file
# of the set is given two roles one name, the later writer's contents are what
# the first rename moves into place, and the second rename fails.
def stderr_conf_case(name, *edits):
    for mode in ('pinyin', 'zhuyin'):
        def probe(k, edits=edits):
            return conf_session(k, conf_system(k, *edits))
        case('table-conf-' + name + ('-zhuyin' if mode == 'zhuyin' else ''), mode=mode, stderr=True)(probe)


stderr_conf_case('colliding-library-files', ('network.bin USER_FILE', 'shared.bin USER_FILE'),
                 ('user.bin USER_FILE', 'shared.bin USER_FILE'))
stderr_conf_case('library-named-like-an-index', ('user.bin USER_FILE', 'user_pinyin_index.bin USER_FILE'))
stderr_conf_case('log-named-like-a-library', ('gb_char.dbin SYSTEM_FILE', 'user.bin SYSTEM_FILE'))


# Class (b): a library whose rows no longer match the index points the
# candidate listing at items that are not there; the pin's duplicate removal
# compares a string it never set (`pinyin.cpp:1635-1637`, `:2053`, SIGSEGV).
def candidates_probe(*edits):
    def probe(k):
        ctx = k.init(system=conf_system(k, *edits))
        assert ctx, 'init failed'
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, CONF_TEXTS[k.mode][0])
        sentence = k.fn('guess_sentence', B, P)(inst)
        ok = conf_guess_rows(k, inst)[0]
        return {'ret': ok, 'sentence': sentence}
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _domain = WARNING_DOMAIN[_mode]
    for _name, _edits in (
            ('files-swapped', (('gb_char.table gb_char.bin', 'gb_char.table TMP.bin'),
                               ('gbk_char.table gbk_char.bin', 'gbk_char.table gb_char.bin'),
                               ('TMP.bin', 'gbk_char.bin'))),
            ('duplicate-row', (('+', 'default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE'),)),
            ('library-as-user-file', (('default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE',
                                       'default GB_DICTIONARY NULL NULL gb_char.dbin USER_FILE'),))):
        case('table-conf-candidates-' + _name + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode,
             crash={'ret': False, 'logs': [[_domain, 16]]})(candidates_probe(*_edits))



# #594: final-step tails, already ported by 55d861de. The pinyin facade
# exposes up to three indexed sentences; zhuyin exposes its <1, 1> result
# and has no SORT_WITHOUT_SENTENCE_CANDIDATE argument. Keep complete lists.
def final_step_sentences(k, inst):
    if k.mode == 'zhuyin':
        ret, text = zhuyin_sentence_out(k, inst)
        return [text] if ret else []
    sentences = []
    for index in range(3):
        # The pin asserts on a missing index when it has any results
        # (pinyin.cpp:1470-1474). Probe a forked snapshot so count and
        # duplicate texts are measured without killing the case worker.
        read_fd, write_fd = os.pipe()
        pid = os.fork()
        if pid == 0:
            try:
                os.close(read_fd)
                with open(os.devnull, 'w') as quiet:
                    os.dup2(quiet.fileno(), 2)
                with os.fdopen(write_fd, 'w') as output:
                    json.dump(sentence_out(k, inst, index), output)
                os._exit(0)
            except BaseException:
                os._exit(1)
        os.close(write_fd)
        with os.fdopen(read_fd) as source:
            payload = source.read()
        _, status = os.waitpid(pid, 0)
        exit_code = os.waitstatus_to_exitcode(status)
        if exit_code == -signal.SIGABRT:
            break
        assert exit_code == 0 and payload, (
            f'sentence probe failed (exit code {exit_code})')
        ret, text = json.loads(payload)
        if not ret:
            break
        sentences.append(text)
    return sentences


def final_step_surface(text, options, imported=False):
    def probe(k):
        assert k.fn('set_options', B, P, U)(k.ctx, options)
        if imported:
            # USER_DICTIONARY; the zhuyin importer takes zhuyin spelling.
            reading = "ba'kua" if k.mode == 'pinyin' else 'ㄅㄚ ㄎㄨㄚ'
            it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
            assert it
            assert k.fn('iterator_add_phrase', B, P, S, S, I)(
                it, '罢跨'.encode(), reading.encode(), 5)
            k.fn('end_add_phrases', None, P)(it)
        inst = k.inst
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        guessed = k.fn('guess_sentence', B, P)(inst)
        sentences = final_step_sentences(k, inst)
        lists = {}
        if k.mode == 'pinyin':
            for sort in (0, 1, 0x1c, 0x1d, 0x1e, 0x1f):
                window = guess_rows(k, inst, 0, sort)
                # Include the original tail index even when duplicate
                # sentence strings collapse into one displayed row.
                indexes = []
                for ordinal, row in enumerate(window['rows']):
                    index = None
                    if row and row[0] == 1:
                        cand, rank = P(), C.c_ubyte(0xff)
                        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(
                            inst, ordinal, C.byref(cand))
                        assert k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
                            inst, cand, C.byref(rank))
                        index = rank.value
                    indexes.append(index)
                window['nbest_indexes'] = indexes
                lists[hex(sort)] = window
        else:
            lists['after-cursor'] = zhuyin_guess_rows(k, inst, 0)
        return dict(parsed=parsed, guessed=guessed, nbest_count=len(sentences),
                    sentences=sentences, candidates=lists)
    return probe


FINAL_STEP_INPUTS = ("ba'kua", "li'shi", "xi'an", "ni'hao", "ba'ku", "ni'h",
                     "nihao'", 'n', 'nih', 'ni', 'nihao', 'zhongguo')
for _mode in ('pinyin', 'zhuyin'):
    for _options in (1 << 5, (1 << 5) | (1 << 3)):
        for _text in FINAL_STEP_INPUTS:
            case('final-step-%s-%s-%x' % (_mode, _text, _options),
                 mode=_mode, control=True)(final_step_surface(_text, _options))
    case('final-step-%s-import-ba-kua' % _mode, mode=_mode, control=True)(
        final_step_surface("ba'kua", 1 << 5, imported=True))


# #728: mask_out rebuilds loaded system sub-indexes from disk, then merges
# the saved user log with the mask (074a2219 pinyin.cpp:1253-1285,
# zhuyin.cpp:788-820). The separate phrase table keeps the imported token,
# so token_get_phrase, rather than lookup_tokens, observes the reload.
def token_phrase_of(k, token):
    length, text = U(), P()
    ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
        k.inst, token, C.byref(length), C.byref(text))
    return [ret, length.value, k.text(text.value)]


def mask_out_system_reload(k, saved, mask=0xFFFFFFFF, selected=False, atomic=False,
                           headerless=False, dropped=False):
    phrase = '龘龘'
    assert not tokens_of(k, phrase), 'the setup phrase already exists'
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 1)
    assert it, 'the import iterator did not open'
    reading = b"da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ'.encode()
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    assert added, 'the setup phrase did not add'
    tokens = tokens_of(k, phrase)
    assert len(tokens) == 1 and tokens[0] >> 24 == 1, 'the import used the wrong library'

    out = {'added': added, 'before': token_phrase_of(k, tokens[0])}
    if saved:
        out['save'] = k.fn('save', B, P)(k.ctx)
        assert out['save'], 'the setup save failed'
    if headerless:
        # Save emitted the 18-byte MODIFY_HEADER first. Without it,
        # _peek_header returns an empty logger before ADD is re-appended.
        payload = Path(k.user, 'gb_char.dbin').read_bytes()[8:]
        assert int.from_bytes(payload[:4], 'little') == 4
        _write_log(k, payload[18:])
    if atomic:
        # A second library has an unsaved item, so reloading it before the
        # guard would observably change the refused call's state.
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 2)
        other_phrase = '龘龘龘'
        other_reading = b"da2'da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ ㄉㄚˊ'.encode()
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(it, other_phrase.encode(), other_reading, 1)
        k.fn('end_add_phrases', None, P)(it)
        other_token = tokens_of(k, other_phrase)[0]
        other_item = token_phrase_of(k, other_token)
        files = transient_files(k.user)
        assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, 0x01001225, 7)
        frequency = unigram_of(k, 0x01001225)
    value = tokens[0] & 0x00FFFFFF if selected else 0
    out['ret'] = k.fn('mask_out', B, P, U, U)(k.ctx, mask, value)
    out['after'] = token_phrase_of(k, tokens[0])
    out['phrase-table-before'] = tokens
    out['phrase-table-after'] = tokens_of(k, phrase)
    if dropped:
        # A phrase-table token is not a live sub-index item. The pin refuses
        # the item update and skips it when pricing a pronunciation hit
        # (phrase_index.cpp:150-174; phonetic_lookup.h:646-647 at 074a2219).
        out['add-unigram'] = k.fn('token_add_unigram_frequency', B, P, U, U)(
            k.inst, tokens[0], 7)
        out['unigram'] = unigram_of(k, tokens[0])
        out['parsed'] = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b"da2'da2")
        out['guessed'] = k.fn('guess_sentence', B, P)(k.inst)
        # Sentence lookup prices pronunciation hits only after checking
        # get_phrase_item. Candidate-list formatting dereferences the pin's
        # dangling table token, so use this defined lookup path instead.
        ret, sentence = (sentence_out(k, k.inst, 0) if k.mode == 'pinyin' else
                         zhuyin_sentence_out(k, k.inst))
        out['lookup'] = [ret, phrase in (sentence or '')]
    if atomic:
        out.update({'untouched-item': out['before'] == out['after'],
                    'untouched-other-library': token_phrase_of(k, other_token) == other_item,
                    'untouched-phrase-table': tokens == out['phrase-table-after'],
                    'untouched-files': transient_files(k.user) == files,
                    'untouched-frequency': unigram_of(k, 0x01001225) == frequency})
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('mask-out-system-unsaved-' + _mode, mode=_mode)(
        lambda k: mask_out_system_reload(k, False))
    case('mask-out-system-saved-' + _mode, mode=_mode)(
        lambda k: mask_out_system_reload(k, True))
    case('abort-mask-out-surviving-add-' + _mode, mode=_mode, abort=False)(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, atomic=True))
    case('mask-out-system-selected-add-' + _mode, mode=_mode, control=True)(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, selected=True))
    case('mask-out-system-headerless-add-' + _mode, mode=_mode)(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, headerless=True))
    case('mask-out-dropped-add-full-' + _mode, mode=_mode)(
        lambda k: mask_out_system_reload(k, True, dropped=True))
    case('mask-out-dropped-add-headerless-' + _mode, mode=_mode)(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, headerless=True, dropped=True))


def mask_out_system_modify(k, header=True, disk=False):
    # MODIFY passes both non-null pointers supplied by _mask_out_records;
    # ADD does not (phrase_index_logger.h:245, :267-268). Keep the item
    # size and unigram unchanged, replacing only 你's one-character text.
    token = 0x01001225
    payload = Path(k.data, 'gb_char.bin').read_bytes()[8:]
    word = lambda offset: int.from_bytes(payload[offset:offset + 4], 'little')
    total, index, content = word(0), word(4), word(8)
    offset = content + word(index + (token & 0x00FFFFFF) * 4)
    length, prons = payload[offset:offset + 2]
    size = 6 + length * 4 + prons * (length * 2 + 4)
    old = payload[offset:offset + size]
    assert length == 1 and int.from_bytes(old[6:10], 'little') == ord('你')
    new = old[:6] + ord('龘').to_bytes(4, 'little') + old[10:]
    record = ((3).to_bytes(4, 'little') + (token & 0x00FFFFFF).to_bytes(4, 'little') +
              len(old).to_bytes(2, 'little') + len(new).to_bytes(2, 'little') + old + new)
    if disk:
        system = private_system(k)
        k._ctx = k.init(system=system)
        assert k._ctx, 'init failed'
    before = token_phrase_of(k, token)  # initialise before changing disk state
    tokens = tokens_of(k, '你')
    if disk:
        # Replace only a scratch symlink: the reference oracle is read-only.
        path = Path(system, 'gb_char.bin')
        path.unlink()
        path.write_bytes(_chunk(payload[:offset] + new + payload[offset + size:]))
    else:
        _write_log(k, (_modify_header(0, total, total) if header else b'') + record)
    ret = k.fn('mask_out', B, P, U, U)(k.ctx, 0xFFFFFFFF if disk else 0x00FFFFFF, 0)
    return {'ret': ret, 'before': before, 'after': token_phrase_of(k, token),
            'phrase-table-before': tokens, 'phrase-table-after': tokens_of(k, '你')}


for _mode in ('pinyin', 'zhuyin'):
    case('mask-out-system-modify-' + _mode, mode=_mode)(mask_out_system_modify)
    # Without MODIFY_HEADER the pin returns an empty logger, so even a
    # valid MODIFY record is not merged (phrase_index.cpp:855-856).
    case('mask-out-system-no-header-' + _mode, mode=_mode, control=True)(
        lambda k: mask_out_system_modify(k, False))
    case('mask-out-system-disk-replacement-' + _mode, mode=_mode)(
        lambda k: mask_out_system_modify(k, disk=True))


# Batch F (#525): the phrase-index logger's two assert sites. A
# `SYSTEM_FILE`/`DICTIONARY` library's user `.dbin` is a `MemoryChunk` of
# `PhraseIndexLogger` records. `next_record` asserts a `MODIFY_HEADER`
# record's token is `null_token` (`phrase_index_logger.h:202`), reached both
# at init's `merge` and at `mask_out`'s `merge_with_mask`; `_peek_header`
# asserts at most one header (`phrase_index.cpp:745`), reached only by
# `merge_with_mask`, so a multi-header `.dbin` loads at init but refuses a
# mask. `_peek_header` counts the headers it read before `next_record`
# answers false on a malformed record and asserts on that accumulated
# total, so a corrupt tail after two headers aborts too; the mask loop
# only walks libraries `get_range` still holds, so an unloaded library's
# `.dbin` is never merged. A `.dbin` is a backend-independent `MemoryChunk`,
# so those fixtures are the same in every cell; the zhuyin import cases craft
# the user phrase table in the running cell's own format (`_craft_db`).
def _chunk(payload):
    """A `MemoryChunk` image: the length and checksum header words
    (`memory_chunk.h:543-547`) then `payload`."""
    checksum = 0
    aligned = len(payload) & ~0x3
    for i in range(0, aligned, 4):
        checksum ^= int.from_bytes(payload[i:i + 4], 'little')
    for shift, byte in enumerate(payload[aligned:]):
        checksum ^= byte << (8 * shift)
    return (len(payload).to_bytes(4, 'little') +
            (checksum & 0xFFFFFFFF).to_bytes(4, 'little') + payload)


def _modify_header(token, old_total, new_total):
    """One `LOG_MODIFY_HEADER` record: the type (4), the token, a `u16`
    length, then the old and new total runs."""
    return ((4).to_bytes(4, 'little') + token.to_bytes(4, 'little') +
            (4).to_bytes(2, 'little') + old_total.to_bytes(4, 'little') +
            new_total.to_bytes(4, 'little'))


def _write_log(k, payload, name='gb_char.dbin'):
    Path(k.user, name).write_bytes(_chunk(payload))


def logger_init_probe(k, payload):
    """A `.dbin` crafted before the (only) init. `_write_user_conf` keeps
    `check_format` from unlinking it first, as `pinyin_init`'s own rewrite
    already does for the pinyin cases and `zhuyin_init`'s does not."""
    _write_user_conf(k)
    _write_log(k, payload)
    return {'ret': bool(k.init())}


def logger_mask_out_probe(k, payload):
    """A `.dbin` corrupted after a successful init, then a mask: the pin's
    `merge_with_mask` walks the file it re-reads, so this reaches the site
    the load-time snapshot does not."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    _write_log(k, payload)
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0, 0)}


def logger_mask_out_unloaded_probe(k, payload):
    """A `.dbin` for a library the call unloads first: the pin's mask loop
    `continue`s on `get_range`'s `ERROR_NO_SUB_PHRASE_INDEX`
    (`pinyin.cpp:1239-1240`, `zhuyin.cpp:796-797`), so the file is never
    merged and the mask completes where the same payload in a loaded
    library refuses it. Index 2 is the one both facades let go
    (`pinyin_unload_phrase_library`'s `GBK_DICTIONARY`), and its user file
    is `gbk_char.dbin`."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    _write_log(k, payload, 'gbk_char.dbin')
    assert k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2), 'the unload failed'
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0, 0)}


def logger_mask_out_untouched(k, payload):
    """A refused mask leaves prior state byte-for-byte: the subject
    validates every `.dbin` before it changes anything, where the pin's
    partial reloads die with its abort."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    assert added, 'the setup phrase did not add'
    assert k.fn('save', B, P)(ctx)
    k._ctx = ctx
    _write_log(k, payload)
    files = transient_files(k.user)
    tokens = list(tokens_of(k, '你好'))
    out = {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0, 0),
           'untouched-tokens': list(tokens_of(k, '你好')) == tokens,
           'untouched-files': transient_files(k.user) == files}
    return out


_HEADER_TOKEN = _modify_header(0x01000001, 1, 1)
_TWO_HEADERS = _modify_header(0, 1, 1) + _modify_header(0, 1, 2)
# A full record head with an invalid type: `next_record` reads the type and
# the token, hits its `default`, and answers false without advancing the
# header count (`phrase_index_logger.h:214-217`).
_BAD_TAIL = (0x99).to_bytes(4, 'little') + (0).to_bytes(4, 'little')
_TWO_HEADERS_BAD_TAIL = _TWO_HEADERS + _BAD_TAIL
_ONE_HEADER_BAD_TAIL = _modify_header(0, 1, 2) + _BAD_TAIL

for _mode in ('pinyin', 'zhuyin'):
    case('abort-init-log-header-token-' + _mode, mode=_mode, abort=False)(
        lambda k: logger_init_probe(k, _HEADER_TOKEN))
    case('abort-mask-out-log-header-token-' + _mode, mode=_mode, abort=False)(
        lambda k: logger_mask_out_probe(k, _HEADER_TOKEN))
    case('abort-mask-out-multiple-log-headers-' + _mode, mode=_mode, abort=False)(
        lambda k: logger_mask_out_probe(k, _TWO_HEADERS))
    # `_peek_header` counts the headers it read before the malformed tail
    # ends the walk and asserts on the total (`phrase_index.cpp:721-746`),
    # so two headers followed by an invalid record still abort there; the
    # pre-fix Err branch discarded the count and let the mask through.
    case('abort-mask-out-multiple-log-headers-bad-tail-' + _mode, mode=_mode, abort=False)(
        lambda k: logger_mask_out_probe(k, _TWO_HEADERS_BAD_TAIL))
    case('abort-mask-out-log-header-untouched-' + _mode, mode=_mode, abort=False)(
        lambda k: logger_mask_out_untouched(k, _TWO_HEADERS))
    # The mask loop skips a library the call unloaded, so its corrupt
    # `.dbin` is never merged and the mask completes. The parent (batch D)
    # had no log validation at all, so it also completed here: a control,
    # held against a future `validate_system_logs` that drops the filter.
    case('mask-out-unloaded-library-log-' + _mode, mode=_mode, control=True)(
        lambda k: logger_mask_out_unloaded_probe(k, _TWO_HEADERS))
    # The pin applies every header at init's `merge` (it only peeks at
    # mask-out time), so a two-header `.dbin` loads cleanly: a control.
    case('init-multiple-log-headers-' + _mode, mode=_mode, control=True)(
        lambda k: logger_init_probe(k, _TWO_HEADERS))
    # One header before the same tail passes `1 >= header_count`, so the
    # pin and the pre-fix subject both complete: a control that holds the
    # boundary the count-before-error fix must not cross.
    case('mask-out-log-header-bad-tail-' + _mode, mode=_mode, control=True)(
        lambda k: logger_mask_out_probe(k, _ONE_HEADER_BAD_TAIL))


# #851: a journal REMOVE of a system item takes it out of the session. A
# system library's `.dbin` is merged at init by `SubPhraseIndex::merge`
# (`phrase_index.cpp:439-517` at 074a2219); REMOVE (`:460-475`) calls
# `remove_phrase_item` (`:219-236`), which zeroes the item's offset and takes its
# unigram off the library total. From then on `get_phrase_item` fails for the
# token (`:180-206`): the item is not readable, a unigram add is refused, an
# export skips it and a save diffs the removal out as a REMOVE record under a
# lowered header, while the phrase table still holds the token.
# `_load_phrase_library` ignores `merge`'s answer (`pinyin.cpp:276`).
#
# The probe crafts the `.dbin` before the only init, then observes the state
# after replay: the touched items through the API, a trailing ADD (whether a
# later record applied), and the `gb_char.dbin` a following `save` diffs out of
# the visible items and the library total (`phrase_index.cpp:392-436`), whose
# header carries the total the replay left. With `reopen` it then closes the
# context and opens the saved profile again, which replays what the save wrote.
_LOG_ADD, _LOG_REMOVE, _LOG_MODIFY = 1, 2, 3


def _log_record(kind, token, old=b'', new=b''):
    """One record as `PhraseIndexLogger::next_record` reads it
    (`phrase_index_logger.h:160-209`): ADD and REMOVE carry one length and
    one chunk, MODIFY both lengths and both chunks."""
    head = kind.to_bytes(4, 'little') + token.to_bytes(4, 'little')
    if kind == _LOG_ADD:
        return head + len(new).to_bytes(2, 'little') + new
    if kind == _LOG_REMOVE:
        return head + len(old).to_bytes(2, 'little') + old
    return head + len(old).to_bytes(2, 'little') + len(new).to_bytes(2, 'little') + old + new


def _item_size(item):
    """`get_phrase_item`'s length from the header bytes alone."""
    length, prons = item[0], item[1]
    return 6 + 4 * length + prons * (2 * length + 4)


def _system_items(k, name='gb_char.bin'):
    """A system chunk's total and its items, `{slot: bytes}` at their header
    length."""
    payload = Path(k.data, name).read_bytes()[8:]
    word = lambda offset: int.from_bytes(payload[offset:offset + 4], 'little')
    total, index, content = word(0), word(4), word(8)
    items = {}
    for slot in range((content - index - 1) // 4):
        offset = word(index + slot * 4)
        if offset:
            items[slot] = payload[content + offset:content + offset + _item_size(payload[content + offset:])]
    return total, items


def _item_with(item, unigram=None, freq=None):
    """`item` with its unigram and/or its last pronunciation's frequency
    replaced; the size is unchanged."""
    item = bytearray(item)
    if unigram is not None:
        item[2:6] = unigram.to_bytes(4, 'little')
    if freq is not None:
        item[-4:] = freq.to_bytes(4, 'little')
    return bytes(item)


def _flip(item):
    """`item` with its last byte changed: the same size, so only the
    `memcmp` can refuse it."""
    return item[:-1] + bytes((item[-1] ^ 0xFF,))


def _item_text(item):
    return ''.join(chr(int.from_bytes(item[6 + 4 * i:10 + 4 * i], 'little')) for i in range(item[0]))


def _observe_system_item(k, watched, phrase):
    out = {name: [token_phrase_of(k, token), unigram_of(k, token)] for name, token in watched.items()}
    # The phrase table keeps the token whatever the sub-index holds.
    out['phrase-table'] = list(tokens_of(k, phrase))
    out['export'] = remember_export(k, 1, phrase)
    return out


def system_replay(k, build, reopen=False, unigram_add=True):
    """`build(p)` returns the records after the header and the tokens to
    watch; `p` maps `one` and `two` (an item of two or more readings) to
    (token, bytes) of library 1, and `fresh` and `absent` to a token it does
    not hold: one for a trailing ADD, one no record adds."""
    total, items = _system_items(k)
    one = next(slot for slot, item in items.items() if item[0] == 1 and item[1] == 1)
    two = next(slot for slot, item in items.items() if item[0] == 1 and item[1] >= 2)
    fresh = max(items) + 2
    records, watched = build({'one': (0x01000000 | one, items[one]),
                              'two': (0x01000000 | two, items[two]),
                              'fresh': (0x01000000 | fresh, _item_with(items[one], unigram=11)),
                              'absent': (0x01000000 | (fresh + 3), b'')})
    phrase = _item_text(items[one])
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total) + b''.join(records))
    out = {'total': total, 'ctx': bool(k.ctx)}
    out.update(_observe_system_item(k, watched, phrase))
    if unigram_add:
        # A unigram add reaches only an item the sub-index holds.
        out['add-unigram'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, watched['one'], 7)
    # An import marks the context modified, so the save writes the diff.
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    out['save'] = k.fn('save', B, P)(k.ctx)
    out['dbin'] = Path(k.user, 'gb_char.dbin').read_bytes().hex()
    if reopen:
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        out['reopen'] = _observe_system_item(k, watched, phrase)
    return out


def _replay_case(name, build, reopen=False, unigram_add=True, **options):
    for mode in ('pinyin', 'zhuyin'):
        case('replay-' + name + '-' + mode, mode=mode, stderr=True, **options)(
            lambda k, build=build: system_replay(k, build, reopen, unigram_add))


def _watch(p):
    return {'one': p['one'][0], 'fresh': p['fresh'][0]}


def _add_fresh(p):
    return _log_record(_LOG_ADD, p['fresh'][0], new=p['fresh'][1])


# A matching REMOVE, then an ADD: the item is gone, the total is lower and the
# ADD after it applied.
def _matched_remove(p):
    return [_log_record(_LOG_REMOVE, p['one'][0], old=p['one'][1]), _add_fresh(p)], _watch(p)


_replay_case('matched-remove', _matched_remove)
# The same, saved and opened again: the REMOVE the save wrote replays to the
# same state.
_replay_case('matched-remove-reopen', _matched_remove, reopen=True)


# #850: journal replay compares and sizes items as the pin does. `merge`
# compares a record's old chunk with the visible item by `PhraseItem::operator==`
# (equal chunk sizes, then `memcmp`: `phrase_index.h:235-240`); the visible
# item's length comes from its header, `6 + 4L + n(2L + 4)` (`get_phrase_item`,
# `phrase_index.cpp:180-206`), not from the stored payload; `add_phrase_item`
# stores the whole payload, trailing bytes included (`:208-217`). REMOVE deletes
# the item before it compares (`:460-475`), so a mismatch returns false with
# the removal kept. MODIFY compares first (`:477-483`); a chunk longer than the
# visible item is a remove plus an add of the whole new payload, otherwise it
# is `memmove`d over the stored bytes (`:486-500`). The first failure ends the
# merge and the records before it stay applied (`pinyin.cpp:276` ignores the
# answer). The probe is #851's `system_replay`.

def _compare_case(name, build, **options):
    """The probe without its unigram add: a live item's added delta is a
    different path than the compare and sizing these cases hold."""
    _replay_case(name, build, unigram_add=False, **options)


# REMOVE whose old chunk carries a trailing byte: the sizes differ, the item
# is gone already and the merge stops before the ADD.
_compare_case('remove-trailing-old', lambda p: ([
    _log_record(_LOG_REMOVE, p['one'][0], old=p['one'][1] + b'\xaa'), _add_fresh(p)], _watch(p)))

# MODIFY whose old chunk carries a trailing byte: the compare refuses it and
# nothing is changed.
_compare_case('modify-trailing-old', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1] + b'\xaa',
                new=_item_with(p['one'][1], freq=77)), _add_fresh(p)], _watch(p)))


# MODIFY whose new chunk is longer than the visible item by trailing bytes:
# the growing path stores all of it, and the item is still its header's
# length, so the next record compares against the header-length bytes.
def _modify_grow(stored_old=False):
    def build(p):
        token, item = p['one']
        new = _item_with(item, unigram=item[2] + 500, freq=5)
        trailing = b'\x01\x02\x03'
        # The second record's old chunk is the item at its header length, or
        # with the trailing bytes the first record stored.
        second = _item_with(new, freq=9)
        return ([_log_record(_LOG_MODIFY, token, old=item, new=new + trailing),
                 _log_record(_LOG_MODIFY, token, old=new + (trailing if stored_old else b''), new=second),
                 _add_fresh(p)], _watch(p))
    return build


_compare_case('modify-trailing-new-grow', _modify_grow(), control=True)
_compare_case('modify-trailing-new-grow-stored-old', _modify_grow(True))


# The in-place path: a new chunk with a pronunciation fewer and trailing bytes
# that still fits the old size is `memmove`d over the item. Its header now
# gives a shorter item, so the second record compares against that, and not
# against the bytes the memmove left behind it.
def _modify_in_place(stored_old=False):
    def build(p):
        token, item = p['two']
        new = bytes((item[0], item[1] - 1)) + item[2:-6]
        assert _item_size(new) == len(item) - 6
        trailing = b'\x01\x02\x03\x04'
        second = _item_with(new, freq=9)
        return ([_log_record(_LOG_MODIFY, token, old=item, new=new + trailing),
                 _log_record(_LOG_MODIFY, token, old=new + (trailing if stored_old else b''), new=second),
                 _add_fresh(p)], _watch(p))
    return build


_compare_case('modify-trailing-new-in-place', _modify_in_place(), control=True)
_compare_case('modify-trailing-new-in-place-stored-old', _modify_in_place(True))


# An empty phrase is an item of `6 + 4n` bytes. In place, then once more
# against the shorter item.
def _modify_empty(p):
    token, item = p['one']
    empty = bytes((0, 0)) + item[2:6]
    return ([_log_record(_LOG_MODIFY, token, old=item, new=empty),
             _log_record(_LOG_MODIFY, token, old=empty, new=_item_with(empty, unigram=3)),
             _add_fresh(p)], _watch(p))


_compare_case('modify-empty-phrase', _modify_empty)

# A mismatch ends the merge: the record before it applied, the one after it
# did not.
_compare_case('mismatch-then-valid', lambda p: ([
    _log_record(_LOG_MODIFY, p['two'][0], old=p['two'][1], new=_item_with(p['two'][1], freq=21)),
    _log_record(_LOG_MODIFY, p['one'][0], old=_flip(p['one'][1]), new=_item_with(p['one'][1], freq=33)),
    _log_record(_LOG_MODIFY, p['two'][0], old=_item_with(p['two'][1], freq=21),
                new=_item_with(p['two'][1], freq=22)),
    _add_fresh(p)], _watch(p)))

# A REMOVE whose old chunk has the right size and the wrong bytes: the item
# and its unigram are gone from the total already, and the merge stops.
_compare_case('failed-remove', lambda p: ([
    _log_record(_LOG_REMOVE, p['one'][0], old=_flip(p['one'][1])), _add_fresh(p)], _watch(p)))

# A record for a token the library does not hold. `get_phrase_item` fails
# (`ERROR_OUT_OF_RANGE`, `phrase_index.cpp:187-189`) and `merge` goes on with
# what its result leaves behind: REMOVE dereferences the NULL `tmpitem`
# (`:468`, `remove_phrase_item` `:219-224` leaves it unset); MODIFY ignores
# the failure and compares a stale `item` (`:478-483`), the default one with no
# earlier MODIFY, else the last item a MODIFY fetched.
def _watch_absent(p):
    return dict(_watch(p), absent=p['absent'][0])


# The pin dies of SIGSEGV at init; the subject opens, goes on past the
# record and answers the library as it holds it.
_NO_ITEM = [[False, 0, None], [False, 0]]
_compare_case('remove-absent', lambda p: ([
    _log_record(_LOG_REMOVE, p['absent'][0], old=p['one'][1]), _add_fresh(p)], _watch_absent(p)),
    crash={'ctx': True, 'absent': _NO_ITEM, 'one': [[True, 1, '锕'], [True, 3]],
           'fresh': [[True, 1, '锕'], [True, 11]]})

_compare_case('modify-absent', lambda p: ([
    _log_record(_LOG_MODIFY, p['absent'][0], old=p['one'][1],
                new=_item_with(p['one'][1], freq=77)), _add_fresh(p)], _watch_absent(p)))


# After a MODIFY of `two`, the stale `item` is `two`'s bytes. A MODIFY of the
# absent token whose old chunk equals them passes the compare: the same-size
# new chunk is `memmove`d over `two` (`:495-500`), the growing one meets the
# NULL of REMOVE's assert (`:486-491`).
def _modify_absent_stale(grow):
    def build(p):
        token, item = p['two']
        first = _item_with(item, freq=21)
        new = _item_with(first, freq=22)
        return ([_log_record(_LOG_MODIFY, token, old=item, new=first),
                 _log_record(_LOG_MODIFY, p['absent'][0], old=first,
                             new=new + (b'\x01' if grow else b'')),
                 _add_fresh(p)], _watch_absent(p))
    return build


_compare_case('modify-absent-stale', _modify_absent_stale(False))
# The growing form reaches REMOVE's NULL: the pin dies of SIGSEGV, the subject
# stops at the record and keeps what the first MODIFY did.
_compare_case('modify-absent-stale-grow', _modify_absent_stale(True),
             crash={'ctx': True, 'absent': _NO_ITEM, 'fresh': _NO_ITEM,
                    'one': [[True, 1, '锕'], [True, 3]]})


# A removed item and the prefix it extends. `SEARCH_CONTINUED` comes from the
# pinyin table's DB get alone (`chewing_large_table2_bdb.cpp:182-187` at
# 074a2219): a REMOVE clears only the sub-index (`phrase_index.cpp:219-236`), so
# the table still reports the removed phrase's key path as continuing, and the
# item drops out later, where `get_phrase_item` fails (`phonetic_lookup.h:646`,
# `:674`). The probe removes the one four-character phrase of the merged
# library that nothing extends, then looks at what the C ABI shows of a search
# over its reading: the parse and the guessed sentence for the reading, for a
# reading that goes on past it and for the prefix the phrase extends; and the
# candidate list of the prefix. The pin lists the removed token itself through
# a stale item (a SIGSEGV), so no candidate list is taken where it is one.
def _removed_extension(k, candidates, labels=('exact', 'longer', 'prefix'), before=False):
    total, items = _system_items(k, 'merged.bin')

    def readings(item):
        length, prons = item[0], item[1]
        start = 6 + 4 * length
        stride = 2 * length + 4
        return [item[start + stride * i:start + stride * i + 2 * length] for i in range(prons)]

    # The item's key path must be no other phrase's prefix, in any system
    # library: only then does the removal leave nothing that extends it.
    longer = set()
    for name in ('merged.bin', 'opengram.bin', 'gb_char.bin', 'gbk_char.bin'):
        for item in _system_items(k, name)[1].values():
            if item[0] > 4:
                longer.update(reading[:8] for reading in readings(item))
    slot = next(slot for slot, item in items.items()
                if item[0] == 4 and item[1] == 1 and readings(item)[0] not in longer)
    token = 0x04000000 | slot
    # Read the phrase and its reading from the untouched library first.
    phrase = token_phrase_of(k, token)[2]
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    keys = new(0, 0, 2)
    assert k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, 0, keys)
    view = C.cast(keys, C.POINTER(Arr)).contents
    syllables = []
    for i in range(view.len):
        key = C.c_ushort(C.cast(view.data, C.POINTER(C.c_ushort))[i])
        rendered = P(UNTOUCHED)
        assert k.fn('get_pinyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(
            k.inst, C.byref(key), C.byref(rendered))
        syllables.append(k.text(rendered.value))
    reading = ''.join(syllables)
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    k._ctx = k._inst = None
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total) + _log_record(_LOG_REMOVE, token, old=items[slot]),
               'merged.dbin')
    out = {'phrase': phrase, 'reading': reading, 'removed': token_phrase_of(k, token)}
    if k.mode == 'zhuyin':
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    texts = {'exact': reading, 'longer': reading + 'ni', 'prefix': ''.join(syllables[:-1])}
    for label in labels:
        text = texts[label]
        inst = k.alloc()
        out[label] = {'parsed': k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())}
        out[label]['guessed'] = k.fn('guess_sentence', B, P)(inst)
        out[label]['sentence'] = (sentence_out(k, inst, 0) if k.mode == 'pinyin' else
                                  zhuyin_sentence_out(k, inst))
        if candidates and k.mode == 'pinyin':
            out[label]['candidates'] = guess_rows(k, inst, 0, 0x1e)
        elif candidates and before:
            ret = k.fn('guess_candidates_before_cursor', B, P, Z)(inst, out[label]['parsed'])
            out[label]['candidates'] = {'ret': ret, 'rows': candidate_rows(k, inst)}
        elif candidates:
            out[label]['candidates'] = zhuyin_guess_rows(k, inst, 0)
        k.fn('free_instance', None, P)(inst)
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('removed-extension-sentence-' + _mode, mode=_mode, stderr=True)(
        lambda k: _removed_extension(k, False))
    case('removed-extension-candidates-' + _mode, mode=_mode, stderr=True)(
        lambda k: _removed_extension(k, True, ('prefix',)))


# The candidate list over the removed token itself. `_compute_phrase_strings_of_items`
# leaves `m_phrase_string` unset when `_token_get_phrase` finds no item
# (`pinyin.cpp:1635-1637`, result ignored at `:2013-2016`; `zhuyin.cpp:1103-1105`),
# and `_remove_duplicated_items_by_phrase_string` hands the NULL to `strcmp` in
# `compare_indexed_item_with_phrase_string` (`pinyin.cpp:2053`, called from
# `pinyin_guess_candidates` at `:2300`; `zhuyin.cpp:1360`, from
# `zhuyin_guess_candidates_after_cursor` at `:1537`): SIGSEGV. A removed token
# stays in the phrase table, so a search over its reading lists it. oxpinyin
# lists no candidate for an item that does not exist.
def _removed_candidate(k, before=False):
    out = _removed_extension(k, True, ('exact',), before)
    exact = out.pop('exact')
    out['exact-parsed'] = exact['parsed']
    out['exact-guessed'] = exact['guessed']
    out['exact-candidates-ret'] = exact['candidates']['ret']
    out['~exact-candidates'] = exact['candidates']['rows']
    return out


# Row 102's answer for the site: an item a reload freed adds no candidate and
# the call returns normally. A removed item is the same case, a token whose item
# `get_phrase_item` no longer finds, so the calls answer `true`.
for _mode in ('pinyin', 'zhuyin'):
    case('removed-candidate-' + _mode, mode=_mode, stderr=True, crash={
        'removed': [False, 0, None], 'exact-guessed': True, 'exact-candidates-ret': True,
    })(_removed_candidate)
case('removed-candidate-before-cursor-zhuyin', mode='zhuyin', stderr=True, crash={
    'removed': [False, 0, None], 'exact-guessed': True, 'exact-candidates-ret': True,
})(lambda k: _removed_candidate(k, before=True))


# Batch F (#525): `zhuyin.cpp`'s phrase-table walk in `_add_phrase`. A token
# whose sub-index equals the target trips `assert(PHRASE_INDEX_LIBRARY_INDEX
# (token) != index)` (`:440`); a lone match whose phrase-index item text
# differs from the phrase trips the `memcmp` assert (`:457`). The crafted
# `user_phrase_index.bin` is written in the running cell's own format
# (`_craft_db`): a Berkeley DB btree on bdb (the pin's own layout,
# `chewing_large_table2_bdb.cpp:58`), a Kyoto Cabinet snapshot on kc
# (`phrase_large_table3_kyotodb.cpp:109`) and a tkrzw `TreeDBM` file on tkrzw
# (`phrase_large_table3_tkrzwdb.cpp:83`).
def _ucs4_key(text):
    return b''.join(ord(character).to_bytes(4, 'little') for character in text)


@case('abort-zhuyin-add-phrase-duplicate-library-token', mode='zhuyin', abort=False)
def _(k):
    # Two tokens for 你好, both in library 7 (the target): the pin's second
    # in-index candidate asserts at `:440`.
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    value = (0x07000001).to_bytes(4, 'little') + (0x07000002).to_bytes(4, 'little')
    _craft_db(os.path.join(k.user, 'user_phrase_index.bin'), [(_ucs4_key('你好'), value)], 1)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你好'.encode(), 'ㄋㄧˇ ㄏㄠˇ'.encode(), 1)}


@case('abort-zhuyin-add-phrase-index-text-mismatch', mode='zhuyin', abort=False)
def _(k):
    # Persist 你們, then point 你好's phrase-table row at 你們's token: the
    # lone in-library match names an item whose text differs, `:457`.
    ctx = k.ctx
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    assert k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你們'.encode(), 'ㄋㄧˇ ㄇㄣˊ'.encode(), 1), 'the setup phrase did not add'
    k.fn('end_add_phrases', None, P)(it)
    assert k.fn('save', B, P)(ctx)
    token = int(tokens_of(k, '你們')[0])
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(ctx)
    _craft_db(os.path.join(k.user, 'user_phrase_index.bin'),
              [(_ucs4_key('你好'), token.to_bytes(4, 'little'))], 1)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你好'.encode(), 'ㄋㄧˇ ㄏㄠˇ'.encode(), 1)}


# The pinyin import iterator's `_add_phrase` walks the same phrase table
# (`pinyin.cpp:533-571`): two tokens in the target library trip the assert at
# `:554`; a lone one whose phrase-index item has another length trips `:568`,
# one with the same length and other text `:571`. The crafted
# `user_phrase_index.bin` is a Berkeley DB btree, so the cases are bdb only.
def _persist_pinyin_phrase(k, text, reading):
    """Adds `text` to library 7, saves, and returns its token; the context
    is closed."""
    ctx = k.ctx
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    assert k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, text.encode(), reading.encode(), 1), 'the setup phrase did not add'
    k.fn('end_add_phrases', None, P)(it)
    assert k.fn('save', B, P)(ctx)
    token = int(tokens_of(k, text)[0])
    assert token >> 24 == 7, 'the setup phrase is not new to library 7'
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(ctx)
    k._ctx = None
    return token


def _add_pinyin_phrase_after_index(k, value):
    """Points 你好's phrase-table row at `value` and adds 你好 to library 7."""
    _craft_db(os.path.join(k.user, 'user_phrase_index.bin'), [(_ucs4_key('你好'), value)], 1)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), b"ni'hao", 1)}


@case('abort-add-phrase-duplicate-library-token', abort=False, userdir=True, cells=('bdb',))
def _(k):
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    value = (0x07000001).to_bytes(4, 'little') + (0x07000002).to_bytes(4, 'little')
    return _add_pinyin_phrase_after_index(k, value)


@case('abort-add-phrase-index-length-mismatch', abort=False, userdir=True, cells=('bdb',))
def _(k):
    token = _persist_pinyin_phrase(k, '你好嗷', "ni'hao'ao")
    return _add_pinyin_phrase_after_index(k, token.to_bytes(4, 'little'))


@case('abort-add-phrase-index-item-missing', abort=False, userdir=True, cells=('bdb',))
def _(k):
    # The phrase table names library 7's token, but user.bin has no item:
    # get_phrase_item leaves a zero-length PhraseItem, so :568 asserts.
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    return _add_pinyin_phrase_after_index(k, (0x07000001).to_bytes(4, 'little'))


@case('abort-add-phrase-index-text-mismatch', abort=False, userdir=True, cells=('bdb',))
def _(k):
    token = _persist_pinyin_phrase(k, '你們', "ni'men")
    return _add_pinyin_phrase_after_index(k, token.to_bytes(4, 'little'))


# #759 PR1 / #785: phrase records are token arrays, including duplicates.
# The kc/tkrzw mask walk reuses an entry; MemoryChunk::set_content does not
# shrink its length. Compare complete phrase records, independently of the
# still-unported chewing-table half of #785.
def phrase_table_lookup(k, text):
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    k.glib.g_array_free.restype, k.glib.g_array_free.argtypes = P, [P, I]
    arr = new(0, 0, 4)
    ret = k.fn('lookup_tokens', B, P, S, P)(k.inst, text.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    tokens = list(C.cast(view.data, C.POINTER(U))[:view.len])
    k.glib.g_array_free(arr, True)
    return [ret, tokens]


def phrase_table_reopen(k):
    if k._inst:
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    assert k.ctx, 'phrase-table reopen failed'


def phrase_table_mask_records(duplicates, remove_middle=False):
    def probe(k):
        phrases = TOKEN_REUSE_PHRASES[:4]
        for text, reading, _ in phrases:
            _add_user_phrase(k, text, [reading.encode()], 100)
        tokens = [phrase_table_lookup(k, text)[1][0] for text, _, _ in phrases]
        assert k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        # Explicit byte-key order: the long record is visited before the
        # short one on each backend. The long tail survives a no-match mask.
        keys = sorted(text.encode('utf-32-le') for text, _, _ in phrases)
        long_tokens = [tokens[0], tokens[0], tokens[1]] if duplicates else sorted(tokens[:3])
        rows = [(keys[0], b''.join(t.to_bytes(4, 'little') for t in long_tokens)),
                (keys[1], tokens[3].to_bytes(4, 'little'))]
        path = Path(k.user, 'user_phrase_index.bin')
        _craft_db(str(path), rows, 1)
        _write_user_conf(k)
        assert k.ctx
        out = {'loaded': [phrase_table_lookup(k, key.decode('utf-32-le')) for key, _ in rows],
               'export_before': d18_export_phrases(k)}
        masked_token = tokens[1] if remove_middle else 0x0700fffe
        out['masked'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, masked_token)
        # mask_out does not set m_modified. An unconstrained train marks
        # the context dirty without editing either crafted phrase record.
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'wo') == 2
        assert k.fn('guess_sentence', B, P)(k.inst)
        out['trained'] = k.fn('train', B, P, C.c_ubyte)(k.inst, 0)
        out['saved'] = k.fn('save', B, P)(k.ctx)
        out['stored'] = index_file_rows(path)
        out['export_after'] = d18_export_phrases(k)
        phrase_table_reopen(k)
        out['reopened'] = [phrase_table_lookup(k, key.decode('utf-32-le')) for key, _ in rows]
        out['export_reopened'] = d18_export_phrases(k)
        return out
    return probe


# bdb controls the absence of shared-scratch tails. Both cases also detect
# reconstruction from phrase items when saving: these raw records, including
# their key/token associations, must survive without being derived anew.
case('phrase-table-mask-long-short')(
    phrase_table_mask_records(False))
case('phrase-table-mask-duplicate-long-short')(phrase_table_mask_records(True))
# Removing the middle token shrinks the scratch entry before the short row.
# Carry its post-mask length, not the maximum length previously encountered;
# retaining the old maximum would duplicate the surviving final token.
case('phrase-table-mask-post-removal-shrink')(
    phrase_table_mask_records(False, remove_middle=True))


@case('phrase-table-add-search-remove-roundtrip', control=True)
def _(k):
    text, reading = '龘龘', b"da2'da2"
    out = {'absent': phrase_table_lookup(k, text)}
    out['added'] = _add_user_phrase(k, text, [reading], 100)
    out['found'] = phrase_table_lookup(k, text)
    out['added_again'] = _add_user_phrase(k, text, [reading], 1)
    out['found_again'] = phrase_table_lookup(k, text)
    out['saved'] = k.fn('save', B, P)(k.ctx)
    phrase_table_reopen(k)
    out['reopened'] = phrase_table_lookup(k, text)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
    assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
    candidates = _user_candidate_rows(k, k.inst)
    assert candidates
    out['removed'] = k.fn('remove_user_candidate', B, P, P)(k.inst, candidates[0])
    out['after_remove'] = phrase_table_lookup(k, text)
    out['readded'] = _add_user_phrase(k, text, [reading], 100)
    out['after_readd'] = phrase_table_lookup(k, text)
    out['saved_again'] = k.fn('save', B, P)(k.ctx)
    phrase_table_reopen(k)
    out['final'] = phrase_table_lookup(k, text)
    out['export'] = d18_export_phrases(k)
    return out


# #799 review: prepare_tokens only enables instantiated sub-indices
# (074a2219 storage/phrase_index.h:762-772); PhraseTableEntry::search skips
# NULL arrays (storage/phrase_large_table3.cpp:40-42).
@case('phrase-table-never-loaded-library-filter')
def _(k):
    token = (15 << 24) | 1  # No library 15 is loaded by this fixture.
    value = token.to_bytes(4, 'little')
    prefix = '你'.encode('utf-32-le')
    _craft_db(str(Path(k.user, 'user_phrase_index.bin')),
              [(prefix, b''), (prefix + '龘'.encode('utf-32-le'), value),
               ('龘龘'.encode('utf-32-le'), value)], 1)
    _write_user_conf(k)
    return {'lookup': phrase_table_lookup(k, '龘龘'),
            'prediction': row58_rows(k, '你')}


def phrase_table_raw_key_prediction(nonunicode):
    def probe(k):
        _add_user_phrase(k, '龘龘', [b"da2'da2"], 100)
        token = next(t for t in tokens_of(k, '龘龘') if t >> 24 == 7)
        assert k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        prefix = '你'.encode('utf-32-le')
        suffix = (0x110000).to_bytes(4, 'little') if nonunicode else '好'.encode('utf-32-le')
        _craft_db(str(Path(k.user, 'user_phrase_index.bin')),
                  [(prefix, b''), (prefix + suffix, token.to_bytes(4, 'little'))], 1)
        _write_user_conf(k)
        # search_suggestion compares byte prefixes, never decodes the key
        # (phrase_large_table3_bdb.cpp:225-252). The candidate uses the live
        # item length (pinyin.cpp:2391-2395) and strips m_begin characters
        # from its live text (:2018-2022), with no starts_with check. Both
        # fixtures therefore display 龘, despite the table key starting 你.
        return row58_rows(k, '你')
    return probe


case('phrase-table-predict-nonunicode-raw-key')(
    phrase_table_raw_key_prediction(True))
case('phrase-table-predict-mismatched-live-prefix', control=True)(
    phrase_table_raw_key_prediction(False))


# #540 / D-18. Small boundary fixtures, never a repeated-training workload.
# Read at libpinyin 074a2219: phrase_index.cpp:56-91, 150-179;
# pinyin_parser2.cpp:274-360; ngram.cpp:277-358; pinyin.cpp:520-605,
# 896-911, 2597-2650; phonetic_lookup.h:880-929.
def d18_export_phrases(k):
    it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
    rows = []
    while k.fn('iterator_has_next_phrase', B, P)(it):
        phrase, reading, count = P(), P(), I(UNTOUCHED)
        ret = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
            it, C.byref(phrase), C.byref(reading), C.byref(count))
        rows.append([ret, k.text(phrase.value), k.text(reading.value), count.value])
    k.fn('end_get_phrases', None, P)(it)
    return rows


def d18_remember(deltas):
    def probe(k):
        out = {'parsed': consumed(k, 'parse_more_full_pinyins', b'nihaoshijie', fresh=False)}
        rows = []
        for delta in deltas:
            ret = k.fn('remember_user_input', B, P, S, I)(k.inst, '你好世界'.encode(), delta)
            token = next(t for t in tokens_of(k, '你好世界') if t >> 24 == 7)
            rows.append([delta, ret, d18_export_phrases(k), unigram_of(k, token)])
        out['steps'] = rows
        return out
    return probe


case('d18-remember-reported', control=True)(d18_remember((-1, 0, 0x7fffffff)))
case('d18-remember-signed-boundary', control=True)(d18_remember((0x7ffffffe, 1, 1, 0x7fffffff, 1)))
for _count in (715827882, 715827883, 1431655765, 1431655766):
    case(f'd18-remember-multiply-{_count}', control=True)(d18_remember((_count,)))


# #752: reuse the preregistered full-pinyin NUL-middle case below,
# axis-c-encoding-pinyin-remember_user_input-nul-middle. These are its
# transformed-parser twins and a known-character NUL-termination control.
# 074a2219 pinyin.cpp:3115-3121, 3688-3694 refuses a character with no token;
# bytes after the first NUL cannot change the visible phrase.
def remember_nul(parse_name, input_text, phrase):
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, 0x28)
        if parse_name == 'parse_more_double_pinyins':
            assert k.fn('set_double_pinyin_scheme', B, P, I)(k.ctx, 2)
        elif parse_name == 'parse_more_chewings':
            assert k.fn('set_zhuyin_scheme', B, P, I)(k.ctx, 1)
        parsed = k.fn(parse_name, Z, P, S)(k.inst, input_text)
        assert parsed == len(input_text), 'the complete two-syllable input must parse'
        return dict(parsed=parsed,
                    ret=k.fn('remember_user_input', B, P, S, I)(k.inst, phrase, 1),
                    rows=d18_export_phrases(k))
    return probe


case('remember-nul-double-pinyin', stderr=True)(
    remember_nul('parse_more_double_pinyins', b'nihk', b'ni\0hao'))
case('remember-nul-chewing-pinyin', stderr=True)(
    remember_nul('parse_more_chewings', b'su3cl3', b'ni\0hao'))
case('remember-known-nul-suffix-pinyin', control=True, stderr=True)(
    remember_nul('parse_more_full_pinyins', b'nihao', '你好'.encode() + b'\0ignored'))


def d18_unigram(boundary, text='你'):
    def probe(k):
        token = tokens_of(k, text)[0]
        if text == '丠':
            assert token >> 24 == 2, 'the alternate-sub-index control must use library 2'
        out = {'token': token, 'before': unigram_of(k, token)}
        if boundary:
            # Select the token's actual sub-index, not an assumed GB token.
            libraries = {'RESERVED': 0, 'GB_DICTIONARY': 1, 'GBK_DICTIONARY': 2,
                         'OPENGRAM_DICTIONARY': 3, 'MERGED_DICTIONARY': 4,
                         'ADDON_DICTIONARY': 5, 'NETWORK_DICTIONARY': 6, 'USER_DICTIONARY': 7}
            library = token >> 24
            entries = [line.split() for line in Path(k.data, 'table.conf').read_text().splitlines()
                       if line.startswith('default ')]
            entry = next(row for row in entries if libraries[row[1]] == library)
            assert entry[3] != 'NULL', 'boundary fixture needs a system sub-index'
            # MemoryChunk's 8-byte header, then SubPhraseIndex::m_total_freq.
            total = int.from_bytes(Path(k.data, entry[3]).read_bytes()[8:12], 'little')
            deltas = (0xffffffff - total - 1, 1, 1, 0xffffffff)
            out.update(library=library, subindex=entry[3], **{'library-total': total})
        else:
            deltas = (0xffffffff, 0x7fffffff, 0x7fffffff, 0x7fffffff, 0x7fffffff, 0xffffffff)
        add = k.fn('token_add_unigram_frequency', B, P, U, U)
        out['steps'] = [[delta, add(k.inst, token, delta), unigram_of(k, token)] for delta in deltas]
        if boundary:
            assert [row[1] for row in out['steps']] == [True, True, False, False]
            assert out['steps'][1][2] == out['steps'][2][2] == out['steps'][3][2]
        return out
    return probe


def d18_input(k):
    assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    out = {}
    parse = k.fn('parse_more_full_pinyins', Z, P, S)
    for length in (4096, 4097, 32766, 32767, 32768, 65535, 65536, 65537):
        inst = k.alloc()
        text = (b'ni' * ((length + 1) // 2))[:length]
        ret = parse(inst, text)
        stored = k.fn('get_parsed_input_length', Z, P)(inst)
        # A later short parse must replace the retained long matrix/length.
        short = parse(inst, b'hao')
        out[str(length)] = [ret, stored, short, k.fn('get_parsed_input_length', Z, P)(inst)]
        k.fn('free_instance', None, P)(inst)
    for label, text in (('whole-tail', b'ni' * 16382 + b'hao'),
                        ('split-tail', b'ni' * 16383 + b'hao'),
                        ('separator-tail', b'ni' * 16383 + b"'hao"),
                        ('invalid-tail', b'ni' * 16383 + b'!ni')):
        inst = k.alloc()
        out[label] = [parse(inst, text), k.fn('get_parsed_input_length', Z, P)(inst)]
        k.fn('free_instance', None, P)(inst)
    return out


def d18_bigram_db(k, path, rows=None, read_keys=(), allow_missing=False):
    """Independent raw SingleGram fixtures through each installed C DBM API.

    `rows=None` reads an existing fixture; all returned buffers are copied
    before closing the DBM. No installed system/user data is ever written.
    """
    import ctypes.util
    conf = Path(k.data, 'table.conf').read_text()
    if 'database format:BerkeleyDB' in conf or 'database format: BerkeleyDB' in conf:
        return _craft_bdb(str(path), rows, 2, read_keys=read_keys, allow_missing=allow_missing)
    writable = rows is not None
    if writable and Path(path).exists():
        Path(path).unlink()
    if 'KyotoCabinet' in conf:
        db = C.CDLL(ctypes.util.find_library('kyotocabinet'))
        db.kcdbnew.restype = P
        db.kcdbopen.argtypes, db.kcdbopen.restype = [P, S, U], I
        db.kcdbset.argtypes, db.kcdbset.restype = [P, S, Z, S, Z], I
        db.kcdbget.argtypes, db.kcdbget.restype = [P, S, Z, C.POINTER(Z)], P
        db.kcdbloadsnap.argtypes, db.kcdbloadsnap.restype = [P, S], I
        db.kcdbdumpsnap.argtypes, db.kcdbdumpsnap.restype = [P, S], I
        db.kcdbclose.argtypes, db.kcdbclose.restype = [P], I
        db.kcdbdel.argtypes, db.kcfree.argtypes = [P], [P]
        handle = db.kcdbnew()
        # The KC system gram is a HashDB; the user gram is a StashDB
        # snapshot (`074a2219 ngram_kyotodb.cpp:54-90`), not a HashDB file.
        snapshot = Path(path).name == 'user_bigram.db'
        spec = b'-' if snapshot else (str(path) + '#type=kch').encode()
        assert db.kcdbopen(handle, spec, 6 if writable or snapshot else 1)
        try:
            if snapshot and not writable:
                assert db.kcdbloadsnap(handle, str(path).encode())
            for key, value in rows or ():
                assert db.kcdbset(handle, key, len(key), value, len(value))
            if snapshot and writable:
                assert db.kcdbdumpsnap(handle, str(path).encode())
            values = []
            for key in read_keys:
                size = Z()
                value = db.kcdbget(handle, key, len(key), C.byref(size))
                if not value and allow_missing:
                    values.append(None)
                    continue
                assert value
                values.append(C.string_at(value, size.value))
                db.kcfree(value)
            return values
        finally:
            assert db.kcdbclose(handle)
            db.kcdbdel(handle)
    assert 'Tkrzw' in conf, 'unknown fixture backend'
    db = C.CDLL(ctypes.util.find_library('tkrzw'))
    db.tkrzw_dbm_open.argtypes, db.tkrzw_dbm_open.restype = [S, B, S], P
    db.tkrzw_dbm_set.argtypes, db.tkrzw_dbm_set.restype = [P, S, I, S, I, B], B
    db.tkrzw_dbm_get.argtypes, db.tkrzw_dbm_get.restype = [P, S, I, C.POINTER(I)], P
    db.tkrzw_dbm_close.argtypes, db.tkrzw_dbm_close.restype = [P], B
    libc = C.CDLL(None)
    libc.free.argtypes = [P]
    handle = db.tkrzw_dbm_open(str(path).encode(), writable, b'dbm=HashDBM')
    assert handle
    try:
        for key, value in rows or ():
            assert db.tkrzw_dbm_set(handle, key, len(key), value, len(value), True)
        values = []
        for key in read_keys:
            size = I()
            value = db.tkrzw_dbm_get(handle, key, len(key), C.byref(size))
            if not value and allow_missing:
                values.append(None)
                continue
            assert value
            values.append(C.string_at(value, size.value))
            libc.free(value)
        return values
    finally:
        assert db.tkrzw_dbm_close(handle)


def d18_gram(total, token, count):
    return b''.join(n.to_bytes(4, 'little') for n in (total, token, count))


def d18_bigram_fixture(k, system, user, current='号', empty_system=False, extra_successor=False):
    prev, cur, other = (tokens_of(k, text)[0] for text in ('你', current, '我'))
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    if system is not None or empty_system:
        copied = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
        shutil.copytree(k.data, copied, dirs_exist_ok=True)
        k.data = copied
        system_rows = [] if empty_system else [
            (prev.to_bytes(4, 'little'), d18_gram(system[0], cur, system[1]))]
        d18_bigram_db(k, Path(copied, 'bigram.db'), system_rows)
    # More than one predecessor: the export's do/while leaves its last
    # loaded gram for the next has_next call (row 36, already fixed).
    records = [(cur, user[1])] + ([(other, 1)] if extra_successor else [])
    gram = user[0].to_bytes(4, 'little') + b''.join(
        t.to_bytes(4, 'little') + count.to_bytes(4, 'little') for t, count in sorted(records))
    rows = [(p.to_bytes(4, 'little'), gram) for p in (prev, other)]
    rows.append((b'\x01\x00\x00\x00', b'\0' * 4))
    d18_bigram_db(k, Path(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    return prev, cur


def d18_merge_observation(k):
    assert k.fn('set_options', B, P, U)(k.ctx, (1 << 5) | (1 << 9))
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'nihao')
    guessed = k.fn('guess_sentence', B, P)(k.inst)
    sentences = final_step_sentences(k, k.inst)
    first = guess_rows(k, k.inst, 0, 0x1e) if k.mode == 'pinyin' else zhuyin_guess_rows(k, k.inst, 0)
    candidate = row_with_text(k, k.inst, 2, '你')
    assert first['ret'] and candidate, 'missing normal predecessor candidate'
    chosen = k.fn('choose_candidate', I, P, Z, P)(k.inst, 0, candidate)
    constrained = k.fn('guess_sentence', B, P)(k.inst)
    chosen_sentences = final_step_sentences(k, k.inst)
    rows = guess_rows(k, k.inst, 2, 0x1e) if k.mode == 'pinyin' else zhuyin_guess_rows(k, k.inst, 2)
    normal = [row for row in rows['rows'] if row[0] == 2]
    assert normal, 'merged-row fixture must reach normal candidates'
    return dict(parsed=parsed, guessed=guessed, chosen=chosen, constrained=constrained,
                sentences=sentences, chosen_sentences=chosen_sentences,
                ret=rows['ret'], candidates=normal)


def d18_merge(system, user, current='号', empty_system=False, extra_successor=False, lam=None):
    def probe(k):
        d18_bigram_fixture(k, system, user, current=current, empty_system=empty_system,
                          extra_successor=extra_successor)
        if lam is not None:
            conf = Path(k.data, 'table.conf')
            conf.write_text(re.sub(r'(?m)^lambda parameter:.*$', 'lambda parameter:' + lam,
                                   conf.read_text()))
        return d18_merge_observation(k)
    return probe


def d18_bigram_export(count):
    def probe(k):
        d18_bigram_fixture(k, None, (count, count), current='好')
        it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
        out = {'has': k.fn('bigram_iterator_has_next_phrase', B, P)(it)}
        phrase, reading, value = P(), P(), I(UNTOUCHED)
        ret = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
            it, C.byref(phrase), C.byref(reading), C.byref(value))
        out['row'] = [ret, k.text(phrase.value), k.text(reading.value), value.value]
        k.fn('end_get_bigram_phrases', None, P)(it)
        return out
    return probe


def d18_bigram_update(count, predicted=False, user_only=False):
    def probe(k):
        current = '嚎' if user_only else '好'
        prev, cur = d18_bigram_fixture(k, None, (count + int(user_only), count), current=current,
                                     empty_system=user_only, extra_successor=user_only)
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        out = {'before': unigram_of(k, cur)}
        if predicted:
            out['predicted'] = k.fn('guess_predicted_candidates', B, P, S)(k.inst, '你'.encode())
            candidate = row_with_text(k, k.inst, 4, current)
            assert candidate, 'missing predicted bigram candidate'
            out['choose'] = k.fn('choose_predicted_candidate', B, P, P)(k.inst, candidate)
            # A guessed, unconstrained sentence makes train set m_modified
            # without changing counts, so save exposes the predicted update.
            out['parse'] = consumed(k, 'parse_more_full_pinyins', b'wo', fresh=False)
        else:
            out['parse'] = consumed(k, 'parse_more_full_pinyins', b'nihao', fresh=False)
        out['guess'] = k.fn('guess_sentence', B, P)(k.inst)
        if not predicted:
            choices = []
            for offset, text in ((0, '你'), (2, '好')):
                if k.mode == 'pinyin':
                    assert k.fn('guess_candidates', B, P, Z, U)(k.inst, offset, 0x1e)
                else:
                    assert k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, offset)
                candidate = row_with_text(k, k.inst, 2, text)
                assert candidate, 'missing normal candidate: ' + text
                choices.append(k.fn('choose_candidate', I, P, Z, P)(k.inst, offset, candidate))
            out['choices'] = choices
            out['chosen-guess'] = k.fn('guess_sentence', B, P)(k.inst)
        out['train'] = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
                        else k.fn('train', B, P)(k.inst))
        out['after'] = unigram_of(k, cur)
        out['save'] = k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        raw = d18_bigram_db(k, Path(k.user, 'user_bigram.db'), read_keys=(prev.to_bytes(4, 'little'),))[0]
        total = int.from_bytes(raw[:4], 'little')
        records = [[int.from_bytes(raw[i:i+4], 'little'), int.from_bytes(raw[i+4:i+8], 'little')]
                   for i in range(4, len(raw), 8)]
        out['stored'] = [total, records]
        out['reopened-unigram'] = unigram_of(k, cur)
        if user_only:
            out['reopened-guess'] = d18_merge_observation(k)
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    case('d18-unigram-max-' + _mode, mode=_mode, control=True)(d18_unigram(False))
    case('d18-unigram-total-boundary-' + _mode, mode=_mode, control=True)(d18_unigram(True))
    case('d18-input-boundaries-' + _mode, mode=_mode, control=True)(d18_input)
    for _label, _system, _user, _control in (
            ('total-before', (1, 1), (0xfffffffe, 1), True),
            ('total-zero', (1, 1), (0xffffffff, 1), False),
            ('total-after', (2, 1), (0xffffffff, 1), False),
            ('count-before', (0x80000000, 1), (0xffffffff, 0xfffffffe), True),
            ('count-zero', (0x80000000, 1), (0xffffffff, 0xffffffff), False),
            ('count-after', (0x80000000, 2), (0xffffffff, 0xffffffff), False)):
        case(f'd18-merge-{_label}-{_mode}', mode=_mode, control=_control)(
            d18_merge(_system, _user, current='嚎' if _label.startswith('count-') else '号'))
    for _label, _count, _control in (
            ('stored-zero', 0, False), ('seed-before', 0x7fffffff, True), ('seed-zero', 0x80000000, False),
            ('seed-after', 0x80000001, False), ('total-max', 0xffffffff - 22080, True),
            ('total-overflow', 0xffffffff - 22080 + 1, False)):
        case(f'd18-train-{_label}-{_mode}', mode=_mode, control=_control)(d18_bigram_update(_count))
for _label, _count, _control in (('max', 0xffffffff - 69, True),
                                ('zero', 0xffffffff - 68, False), ('after', 0xffffffff - 67, False)):
    case('d18-predicted-total-' + _label, control=_control)(d18_bigram_update(_count, predicted=True))
for _count in (0x3fffffff, 0x40000000, 0x7fffffff, 0x80000000, 0x80000001, 0xffffffff):
    case(f'd18-bigram-export-{_count:x}', control=True)(d18_bigram_export(_count))

# Review regressions: row/record presence survives a wrapped zero total/count
# (074a2219 ngram.cpp:135-175,277-358; phonetic_lookup.h:600-640).
for _mode in ('pinyin', 'zhuyin'):
    case('d18-review-user-only-zero-total-' + _mode, mode=_mode)(
        d18_merge(None, (0, 0xffffffff), current='嚎', empty_system=True, extra_successor=True))
    for _label, _lam in (('default', None), ('half', '0.5'), ('zero-lambda', '0')):
        case(f'd18-review-zero-count-zero-total-{_label}-{_mode}', mode=_mode,
             control=_label != 'default' or _mode == 'pinyin')(
            d18_merge((1, 1), (0xffffffff, 0xffffffff),
                      current='好' if _label == 'default' else '嚎', lam=_lam))
    case('d18-review-unigram-library-two-' + _mode, mode=_mode, control=True)(
        d18_unigram(True, text='丠'))
case('d18-review-predicted-user-only-zero-total')(
    d18_bigram_update(0xffffffff - 69, predicted=True, user_only=True))


# #739 review: BDB save_db refuses unlinking a directory, but zhuyin_save
# still renames the untouched temporary directory (074a2219
# chewing_large_table2_bdb.cpp:138-140, phrase_large_table3_bdb.cpp:153-155;
# zhuyin.cpp:650-679). A regular live index prevents that rename.
def stale_index_tmp(k, index):
    out = {}
    for destination in ('absent', 'empty-directory', 'live-file'):
        k.user = tempfile.mkdtemp(prefix='user-', dir=k.scratch)
        inst = k.inst
        parse, text = ('parse_more_full_pinyins', b'nihao') if k.mode == 'pinyin' else ('parse_more_chewings', b'su3cl3')
        def train():
            assert k.fn(parse, Z, P, S)(inst, text)
            assert k.fn('guess_sentence', B, P)(inst)
            return (k.fn('train', B, P, C.c_ubyte)(inst, 0) if k.mode == 'pinyin'
                    else k.fn('train', B, P)(inst))
        assert train()
        live = Path(k.user, index)
        if destination == 'live-file':
            assert k.fn('save', B, P)(k.ctx)
            before = live.read_bytes()
            assert train()
        elif destination == 'empty-directory':
            live.mkdir()
        tmp = Path(k.user, index + '.tmp')
        tmp.mkdir()
        (tmp / 'stale-marker').write_bytes(b'unchanged temporary directory')
        ret = k.fn('save', B, P)(k.ctx)
        out[destination] = {
            'save': ret,
            'tmp-present': tmp.exists(),
            'stale-moved': (live / 'stale-marker').is_file() and
                (live / 'stale-marker').read_bytes() == b'unchanged temporary directory',
            'live-preserved': destination == 'live-file' and live.read_bytes() == before,
            'messages': list(k.log_messages),
        }
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
    return out


for _mode in ('pinyin', 'zhuyin'):
    for _index in ('user_pinyin_index.bin', 'user_phrase_index.bin'):
        case(f'issue542-stale-{_index}-{_mode}', mode=_mode, stderr=True,
             control=_mode == 'pinyin', cells=('bdb',))(
            lambda k, index=_index: stale_index_tmp(k, index))


# #542's remaining named audit inputs, kept in the existing isolated worker.
def phrase_segment_invalid_utf8(k):
    ret = k.fn('phrase_segment', B, P, S)(k.inst, b'\xff\xfe')
    return {'ret': ret, 'messages': k.log_messages}


def train_no_choose_save(k):
    inst = k.inst
    parse, text = ('parse_more_full_pinyins', b'nihao') if k.mode == 'pinyin' else ('parse_more_chewings', b'su3cl3')
    out = {'parse': k.fn(parse, Z, P, S)(inst, text),
           'guess': k.fn('guess_sentence', B, P)(inst)}
    if k.mode == 'pinyin':
        out['train'] = k.fn('train', B, P, C.c_ubyte)(inst, 0)
    else:
        out['train'] = k.fn('train', B, P)(inst)
    out['save'] = k.fn('save', B, P)(k.ctx)
    out['files'] = sorted(os.listdir(k.user))
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('issue542-phrase-segment-invalid-utf8-' + _mode, mode=_mode)(phrase_segment_invalid_utf8)
    case('issue542-train-no-choose-save-' + _mode, mode=_mode, control=True)(train_no_choose_save)
    case('issue542-parse-full-one-values-' + _mode, mode=_mode, control=True)(
        lambda k: {t.decode(): key_out(k, t) for t in (b'n', b'ni3')})


@case('issue542-alias-prefix-borrowed')
def _(k):
    out = {}
    for predicted_call in (False, True):
        inst = full_inst(k, b'nihao')
        k.fn('guess_sentence', B, P)(inst)
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1E)
        cand, text = P(UNTOUCHED), P(UNTOUCHED)
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, cand, C.byref(text))
        label = 'predicted' if predicted_call else 'with-prefix'
        out[label + ' borrowed text'] = C.string_at(text.value).decode()
        name = 'guess_predicted_candidates' if predicted_call else 'guess_sentence_with_prefix'
        out[label] = k.fn(name, B, P, P)(inst, text)
        if not predicted_call:
            out['sentences'] = [sentence_out(k, inst, i) for i in range(3)]
        else:
            count = U(UNTOUCHED)
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            out['predicted candidate count'] = count.value
            out['predicted sentences'] = [sentence_out(k, inst, i) for i in range(3)]
            # The same text at caller-owned storage must still predict.
            out['owned prefix recovery'] = k.fn('guess_predicted_candidates', B, P, S)(
                inst, '你好'.encode())
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            out['owned prefix candidate count'] = count.value
    return out


# #740: removal subtracts from the stored guint32 total, never from a sum
# of survivors (074a2219 ngram.cpp:76-102, ngram_bdb.cpp:215-254;
# pinyin.cpp:1230,3766; zhuyin.cpp:765). Read persisted rows after save
# and again after reopen so the live container and export paths agree.
def removal_bigram(total, survivor_counts, removed_count, operation, multiple=False, unchanged_total=17):
    def probe(k):
        reading = b"da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ'.encode()
        _add_user_phrase(k, '龘龘', [reading])
        token = next(t for t in tokens_of(k, '龘龘') if t >> 24 == 7)
        predecessors = [tokens_of(k, text)[0] for text in ('你', '我')]
        successors = [tokens_of(k, text)[0] for text in ('好', '号')]
        assert k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None

        records = list(zip(successors, survivor_counts)) + [(token, removed_count)]
        if multiple:
            records.append((token + 1, 1))
        gram = total.to_bytes(4, 'little') + b''.join(
            t.to_bytes(4, 'little') + count.to_bytes(4, 'little') for t, count in sorted(records))
        # The second predecessor is untouched, with a deliberately independent
        # stored total. Removing zero items must leave that total alone too.
        unchanged = d18_gram(unchanged_total, successors[0], 2)
        keys = [p.to_bytes(4, 'little') for p in predecessors]
        path = Path(k.user, 'user_bigram.db')
        d18_bigram_db(k, path, list(zip(keys, (gram, unchanged))))
        _write_user_conf(k)

        if operation == 'remove-candidate':
            assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
            assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
            candidates = _user_candidate_rows(k, k.inst)
            assert candidates, 'missing removable user candidate'
            removed = k.fn('remove_user_candidate', B, P, P)(k.inst, candidates[0])
        else:
            mask, value = (0xff000000, 0x07000000) if multiple else (0xffffffff, token)
            removed = k.fn('mask_out', B, P, U, U)(k.ctx, mask, value)
        # Neither pin removal call sets m_modified. An unconstrained train
        # marks it without changing any bigram, making save observable.
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'wo') == 2
        assert k.fn('guess_sentence', B, P)(k.inst)
        trained = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
                   else k.fn('train', B, P)(k.inst))
        saved = k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        stored = [value.hex() for value in d18_bigram_db(k, path, read_keys=keys)]
        assert k.ctx, 'reopen after removal failed'
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        reopened = [value.hex() for value in d18_bigram_db(k, path, read_keys=keys)]
        return dict(removed=removed, trained=trained, saved=saved, stored=stored, reopened=reopened)
    return probe


for _mode, _operation in (('pinyin', 'mask-out'), ('pinyin', 'remove-candidate'),
                         ('zhuyin', 'mask-out')):
    for _label, _total, _survivors, _removed in (
            ('reported', 1, (0xffffffff, 1), 1),
            ('underflow', 0, (2, 3), 1),
            ('independent-total', 17, (2, 3), 4),
            ('remove-zero', 17, (2, 3), 0),
            ('zero-survivors', 1, (0, 0), 1),
            ('wrap-after', 1, (2, 3), 0xffffffff),
            ('ordinary', 9, (2, 3), 4)):
        case(f'removal-bigram-{_label}-{_operation}-{_mode}', mode=_mode,
             control=_label == 'ordinary')(
            removal_bigram(_total, _survivors, _removed, _operation,
                          unchanged_total=2 if _label == 'ordinary' else 17))
    if _operation == 'mask-out':
        case(f'removal-bigram-multiple-{_mode}', mode=_mode)(
            removal_bigram(1, (0xffffffff, 0), 1, _operation, multiple=True))


# #757 PR 2: the pin keeps the stored SingleGram array as it is and searches
# it with std::lower_bound (074a2219 stl_lite.h:29; ngram.cpp:161, 185, 215,
# 239, 259), so an unsorted array answers deterministically (Q1) and
# duplicated items are kept: get/set act on the first match, mask_out removes
# every match and subtracts each count, retrieve_all yields stored order (Q2,
# ngram.cpp:86-99, 133-151). Training (phonetic_lookup.h:876-909), predicted
# training (pinyin.cpp:2622-2638), mask and removal (pinyin.cpp:1230, 3764-3766;
# ngram_bdb.cpp:217-254 and twins), export (pinyin.cpp:775-787, 885-890) and
# prediction (pinyin.cpp:2321-2341) all run over that raw array.
def _gram_bytes(total, items):
    return total.to_bytes(4, 'little') + b''.join(
        t.to_bytes(4, 'little') + c.to_bytes(4, 'little') for t, c in items)


def _gram_items(raw):
    if raw is None:
        return None
    return [int.from_bytes(raw[:4], 'little'),
            [[int.from_bytes(raw[i:i + 4], 'little'), int.from_bytes(raw[i + 4:i + 8], 'little')]
             for i in range(4, len(raw) - len(raw[4:]) % 8, 8)]]


def _plant_user_grams(k, phrases, grams):
    """Add `phrases` (text, pinyin, zhuyin, count) as user phrases, save and
    close, then replace user_bigram.db with `grams(tokens)` — a list of
    (predecessor, raw value) — in this cell's own format. Answers the
    phrases' user tokens and the planted keys."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    for text, pinyin, zhuyin, count in phrases:
        assert add(it, text.encode(), (pinyin if k.mode == 'pinyin' else zhuyin).encode(), count)
    k.fn('end_add_phrases', None, P)(it)
    users = [next(t for t in tokens_of(k, text) if t >> 24 == 7) for text, _, _, _ in phrases]
    system = {text: tokens_of(k, text)[0] for text in ('你', '好', '号', '我')}
    assert k.fn('save', B, P)(k.ctx)
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    rows = [(p.to_bytes(4, 'little'), value) for p, value in grams(system, users)]
    d18_bigram_db(k, Path(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    return system, users, [key for key, _ in rows]


def _close_and_read_grams(k, keys):
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    values = d18_bigram_db(k, Path(k.user, 'user_bigram.db'), read_keys=keys, allow_missing=True)
    return [_gram_items(value) for value in values]


def _mark_modified_by_unconstrained_train(k):
    # Neither removal call sets m_modified; an unconstrained train marks it
    # without changing a bigram, so the save that follows is observable.
    assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'wo') == 2
    assert k.fn('guess_sentence', B, P)(k.inst)
    return (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
            else k.fn('train', B, P)(k.inst))


_DALONG = ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ', 5)
_BINGBING = ('靐靐', "da3'da3", 'ㄉㄚˇ ㄉㄚˇ', 5)


def user_gram_layout(kind, operation):
    """`kind` 'unsorted': the 你 gram holds [龘龘, 号, 好] in descending token
    order. `kind` 'duplicate': it holds 好 twice and 龘龘 twice, sorted.
    Every count passes the export threshold (68) and the predicted filter."""
    def grams(system, users):
        hao, hao2, dalong = system['好'], system['号'], users[0]
        if kind == 'unsorted':
            items = sorted([(dalong, 90), (hao2, 80), (hao, 70)], reverse=True)
        else:
            items = [(hao, 70), (hao, 80), (dalong, 90), (dalong, 100)]
        total = sum(count for _, count in items)
        return [(system['你'], _gram_bytes(total, items))]

    def probe(k):
        system, users, keys = _plant_user_grams(k, [_DALONG], grams)
        out = {}
        if operation == 'train-save':
            assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
            out['parse'] = consumed(k, 'parse_more_full_pinyins', b'nihao', fresh=False)
            out['guess'] = k.fn('guess_sentence', B, P)(k.inst)
            choices = []
            for offset, text in ((0, '你'), (2, '好')):
                if k.mode == 'pinyin':
                    assert k.fn('guess_candidates', B, P, Z, U)(k.inst, offset, 0x1e)
                else:
                    assert k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, offset)
                candidate = row_with_text(k, k.inst, 2, text)
                assert candidate, 'missing normal candidate: ' + text
                choices.append(k.fn('choose_candidate', I, P, Z, P)(k.inst, offset, candidate))
            out['choices'] = choices
            out['chosen-guess'] = k.fn('guess_sentence', B, P)(k.inst)
            out['train'] = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
                            else k.fn('train', B, P)(k.inst))
            out['save'] = k.fn('save', B, P)(k.ctx)
        elif operation in ('mask-save', 'remove-candidate-save'):
            if operation == 'remove-candidate-save':
                assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
                assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
                rows = _user_candidate_rows(k, k.inst)
                assert rows, 'missing removable user candidate'
                out['removed'] = k.fn('remove_user_candidate', B, P, P)(k.inst, rows[0])
            else:
                # Unsorted: mask 龘龘 out of the gram. Duplicate: mask both 好.
                value = users[0] if kind == 'unsorted' else system['好']
                out['masked'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, value)
            out['trained'] = _mark_modified_by_unconstrained_train(k)
            out['save'] = k.fn('save', B, P)(k.ctx)
        elif operation == 'export':
            it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
            has = k.fn('bigram_iterator_has_next_phrase', B, P)
            nxt = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P),
                       C.POINTER(I))
            steps, falses = [], 0
            # The do/while loads the last predecessor's gram on a `has` that
            # answers false, and scans it on the next one (row 36).
            while falses < 2 and len(steps) < 16:
                if not has(it):
                    falses += 1
                    steps.append('no-next')
                    continue
                falses = 0
                phrase, reading, count = P(), P(), I(UNTOUCHED)
                ret = nxt(it, C.byref(phrase), C.byref(reading), C.byref(count))
                steps.append([ret, k.text(phrase.value), k.text(reading.value), count.value])
            k.fn('end_get_bigram_phrases', None, P)(it)
            out['steps'] = steps
            return out
        out['grams'] = _close_and_read_grams(k, keys)
        return out
    return probe


def user_gram_predicted(kind):
    """Two user phrases with equal counts score equally, so the predicted
    list's stable sort keeps their retrieve_all order. 'unsorted' plants them
    in descending token order; 'duplicate' plants 龘龘 twice, the first above
    the filter (10) and the second below it."""
    def grams(system, users):
        dalong, bingbing = users
        if kind == 'unsorted':
            items = sorted([(dalong, 20), (bingbing, 20)], reverse=True)
        else:
            items = [(dalong, 20), (dalong, 3), (bingbing, 20)]
        return [(system['你'], _gram_bytes(sum(c for _, c in items), items))]

    def probe(k):
        _plant_user_grams(k, [_DALONG, _BINGBING], grams)
        out = {'ret': k.fn('guess_predicted_candidates', B, P, S)(k.inst, '你'.encode())}
        rows = []
        n = U()
        k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(n))
        for i in range(n.value):
            cand, text, kind_ = P(), S(), I()
            k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, i, C.byref(cand))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(k.inst, cand, C.byref(text))
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(k.inst, cand, C.byref(kind_))
            rows.append([kind_.value, text.value.decode() if text.value else None])
        out['rows'] = rows
        return out
    return probe


for _kind in ('unsorted', 'duplicate'):
    for _mode in ('pinyin', 'zhuyin'):
        for _operation in ('train-save', 'mask-save'):
            case(f'user-gram-{_kind}-{_operation}-{_mode}', mode=_mode)(
                user_gram_layout(_kind, _operation))
    case(f'user-gram-{_kind}-remove-candidate-save')(user_gram_layout(_kind, 'remove-candidate-save'))
    case(f'user-gram-{_kind}-export')(user_gram_layout(_kind, 'export'))
    case(f'user-gram-{_kind}-predicted')(user_gram_predicted(_kind))


# Q1's abort (register row 85): training 你 after the sentence start finds
# the start's gram ordered [1, 2, 3, 6, 4] around it (你 = 5). get_freq
# misses, insert_freq appends, and set_freq's second lower_bound lands on the
# larger token, so check_result(user->set_freq(token, freq + seed)) dies
# (074a2219 phonetic_lookup.h:888-903; ngram.cpp:178-205, 252-271).
def train_unsorted_set_freq_miss(k):
    def grams(system, users):
        ni = system['你']
        items = [(ni - 4, 1), (ni - 3, 1), (ni - 2, 1), (ni + 1, 1), (ni - 1, 1)]
        return [(1, _gram_bytes(5, items))]

    _plant_user_grams(k, [], grams)
    assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    assert consumed(k, 'parse_more_full_pinyins', b'nihao', fresh=False) == 5
    assert k.fn('guess_sentence', B, P)(k.inst)
    for offset, text in ((0, '你'), (2, '好')):
        if k.mode == 'pinyin':
            assert k.fn('guess_candidates', B, P, Z, U)(k.inst, offset, 0x1e)
        else:
            assert k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, offset)
        candidate = row_with_text(k, k.inst, 2, text)
        assert candidate, 'missing normal candidate: ' + text
        assert k.fn('choose_candidate', I, P, Z, P)(k.inst, offset, candidate) > 0
    assert k.fn('guess_sentence', B, P)(k.inst)
    ret = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
           else k.fn('train', B, P)(k.inst))
    return {'ret': ret}


for _mode in ('pinyin', 'zhuyin'):
    case(f'abort-train-unsorted-set-freq-miss-{_mode}', mode=_mode, abort=False)(
        train_unsorted_set_freq_miss)


# Q3: an item-less gram under a key the mask leaves alone is loaded,
# `SingleGram::mask_out` removes nothing, and `Bigram::mask_out` skips it
# untouched — `if (0 == num) { delete gram; continue; }` (074a2219
# ngram_bdb.cpp:238-241, ngram_kyotodb.cpp:219-222, ngram_tkrzwdb.cpp:195-198)
# — whatever its total; register row 88 records that the pin completes the
# mask and leaves the state.
def itemless_gram_untouched(operation, total):
    def grams(system, users):
        return [(system['你'], _gram_bytes(140, [(system['好'], 70), (users[0], 70)])),
                (system['我'], total.to_bytes(4, 'little'))]

    def probe(k):
        system, users, keys = _plant_user_grams(k, [_DALONG], grams)
        out = {}
        if operation == 'remove-candidate':
            assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
            assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
            rows = _user_candidate_rows(k, k.inst)
            assert rows, 'missing removable user candidate'
            out['removed'] = k.fn('remove_user_candidate', B, P, P)(k.inst, rows[0])
        else:
            out['masked'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, users[0])
        out['trained'] = _mark_modified_by_unconstrained_train(k)
        out['save'] = k.fn('save', B, P)(k.ctx)
        out['grams'] = _close_and_read_grams(k, keys)
        return out
    return probe


# (name, operation, facade): the name keeps the case ids already cited.
for _name, _operation, _mode in (('pinyin', 'mask-out', 'pinyin'),
                                 ('zhuyin', 'mask-out', 'zhuyin'),
                                 ('remove-candidate', 'remove-candidate', 'pinyin')):
    for _label, _total in (('total-zero', 0), ('total-nonzero', 17)):
        case(f'mask-out-itemless-gram-untouched-{_name}-{_label}', mode=_mode)(
            itemless_gram_untouched(_operation, _total))


# Q4: an item-less user gram still merges its total with the system gram's
# (074a2219 merge_single_gram, ngram.cpp:298-306). The sentence start's user
# gram carries a total that wraps the merged total to 1, so every bigram from
# the sentence start dominates the first phrase. The phrase segmenter merges
# per step (phrase_lookup.cpp:269-293); the n-best guess merges per row and is
# the control. `texts` replaces the guess inputs; `plant=False` leaves the user
# bigram out and keeps the close-and-reopen around it.
def itemless_user_total(operation, texts=(b'nihaoshijie', b'womenshizhongguoren'), plant=True):
    def probe(k):
        system_total = int.from_bytes(
            d18_bigram_db(k, Path(k.data, 'bigram.db'), read_keys=(b'\x01\x00\x00\x00',))[0][:4],
            'little')
        user_total = (1 - system_total) % (1 << 32)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        if plant:
            d18_bigram_db(k, Path(k.user, 'user_bigram.db'),
                          [(b'\x01\x00\x00\x00', user_total.to_bytes(4, 'little'))])
        _write_user_conf(k)
        out = {}
        if operation == 'phrase-segment':
            segment = k.fn('phrase_segment', B, P, S)
            n_phrase = k.fn('get_n_phrase', B, P, C.POINTER(U))
            token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))
            # 今天天气很好 changes its first phrases once the total is
            # merged; the other text keeps its segmentation either way.
            for text in ('今天天气很好', '我们是中国人'):
                ret = segment(k.inst, text.encode())
                count = U(UNTOUCHED)
                n_phrase(k.inst, C.byref(count))
                tokens = []
                for index in range(count.value if count.value != UNTOUCHED else 0):
                    slot = U(UNTOUCHED)
                    token(k.inst, index, C.byref(slot))
                    tokens.append(slot.value)
                out[text] = [ret, tokens]
        else:
            for text in texts:
                inst = k.alloc()
                k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
                out[text.decode()] = [k.fn('guess_sentence', B, P)(inst),
                                      [sentence_out(k, inst, index) for index in range(3)]]
                k.fn('free_instance', None, P)(inst)
        return out
    return probe


case('itemless-user-total-phrase-segment')(itemless_user_total('phrase-segment'))

case('itemless-user-total-guess', control=True)(itemless_user_total('guess'))


# #791: with the sentence start's merged total wrapped to 1, bigram_poss is
# count / 1, the first phrase's blend is above one, and the pin takes its log
# uncapped (074a2219 phonetic_lookup.h:692-693; bigram_poss = freq / (gfloat)
# total, ngram.cpp:171): a positive score. For 研究生命起源 that makes 研究 the
# beam head, so the unigram pass (topresults[0] only, phonetic_lookup.h:546-547)
# stores a second copy of the 生命 -> 起源 chain beside the bigram pass's
# (eval_item has no identity check, phonetic_lookup_heap.h:56-66): the pin's
# rows are 研究生命起源 twice, then 研究生名气元. A blend clamped at certainty
# moved the head, so that copy was never stored. The control is the same input
# with no planted record, where both sides already store both copies.
case('itemless-user-total-guess-above-one')(
    itemless_user_total('guess', texts=(b'yanjiushengmingqiyuan',)))

case('itemless-user-total-guess-above-one-control', control=True)(
    itemless_user_total('guess', texts=(b'yanjiushengmingqiyuan',), plant=False))


# #798: the n-best unigram denominator is the facade's guint32 total
# (074a2219 phrase_index.h:440, returned at :615-617), read at
# phonetic_lookup.h:651 (unigram arm) and :679 (blended arm) and at
# phrase_lookup.cpp:315 and :334. Importing a new user phrase adds the item
# with frequency 0 and then add_unigram_frequency(count * 3)
# (pinyin.cpp:598-605), which adds to the facade total with no overflow check
# before the sub-index checks its own (phrase_index.h:628-634;
# phrase_index.cpp:169-175). `total` is the facade total the import leaves,
# before the guint32 wraps it; the user library's own total stays under 2^32.
def import_facade_total(total):
    def probe(k):
        count, rest = divmod(total - FACADE_TOTAL, 3)
        assert rest == 0 and 3 * count <= 0xFFFFFFFF and count < 2 ** 31, total
        out = {'count': count, 'added': import_rows(k, 7, [('妈', 'ma1', count)])}
        out.update(_guess_out(k, b"ni3'hao3"))
        out['segment'] = _segment_out(k, ('你好',))
        return out
    return probe


# The count #798 names: the total wraps to 51050831. The pin's third n-best
# row is 你好 again; a total that did not wrap ranks 拟好 there.
case('import-wrapped-facade-total-guess')(import_facade_total(2 ** 32 + 51050831))

# A total that wraps to 2000000 also moves the phrase segment: the pin splits
# 你好 into 你 and 好, a total that did not wrap keeps the phrase.
case('import-wrapped-facade-total-segment')(import_facade_total(2 ** 32 + 2000000))

# Just under the boundary: the total is 2^32 - 1 and nothing wraps.
case('import-facade-total-below-wrap', control=True)(import_facade_total(2 ** 32 - 1))


# A library's total is the one its file stores (074a2219 phrase_index.cpp:345),
# and the facade adds it whole at load (:237-248). user.bin's header is set
# here past the sum of its items so the facade wraps to 2000000 on reopen.
# The save after the next import writes the items' sum again: pinyin_save
# compacts first (pinyin.cpp:1139), which rebuilds each library's total from
# its items (phrase_index.cpp:648-675) and leaves the facade's alone.
def stored_header_total(header):
    def probe(k):
        out = {'added': import_rows(k, 7, [('你好', "ni3'hao3", 5)])}
        out['saved'] = k.fn('save', B, P)(k.ctx)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        path = Path(k.user, 'user.bin')
        payload = bytearray(path.read_bytes()[8:])
        out['stored'] = int.from_bytes(payload[:4], 'little')
        payload[:4] = header.to_bytes(4, 'little')
        path.write_bytes(_chunk(bytes(payload)))
        _write_user_conf(k)
        out.update(_guess_out(k, b"ni3'hao3"))
        out['segment'] = _segment_out(k, ('你好',))
        out['import'] = import_rows(k, 7, [('好你', "hao3'ni3", 2)])
        out['resaved'] = k.fn('save', B, P)(k.ctx)
        out['header'] = int.from_bytes(Path(k.user, 'user.bin').read_bytes()[8:12], 'little')
        return out
    return probe


case('stored-header-facade-total')(stored_header_total(2 ** 32 + 2000000 - FACADE_TOTAL))

# A header a little off the items' sum moves nothing visible.
case('stored-header-facade-total-small', control=True)(stored_header_total(7))


# A phrase imported into a system library is saved as an ADD record of
# gb_char.dbin, after a header whose new total already counts it (diff,
# phrase_index.cpp:392-399). On reopen the merge sets the total from that
# header and then adds the item again (add_phrase_item, :453-459), so the
# facade counts its unigram twice. The count makes the doubled total wrap
# to 2000003.
def system_add_reload(count):
    def probe(k):
        out = {'added': import_rows(k, 1, [('龘龘', "da3'da3", count)])}
        out['saved'] = k.fn('save', B, P)(k.ctx)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        out.update(_guess_out(k, b"ni3'hao3"))
        out['segment'] = _segment_out(k, ('你好',))
        return out
    return probe


case('system-add-reload-facade-total')(system_add_reload((2 ** 32 + 2000003 - FACADE_TOTAL) // 6))

# The same flow with a count too small to move the ranking.
case('system-add-reload-facade-total-small', control=True)(system_add_reload(5))


# #801 review. add_unigram_frequency raises the item's own unigram with the
# facade total (074a2219 phrase_index.h:628-634, phrase_index.cpp:150-178), and
# the n-best step reads both from the live item (phonetic_lookup.h:651, :679).
def _stored_library_total(k, name):
    """A system library chunk's stored total, after the MemoryChunk header."""
    return int.from_bytes(Path(k.data, name).read_bytes()[8:12], 'little')


def _facade_total_out(k):
    out = _guess_out(k, b"ni3'hao3")
    out['candidates'] = guess_rows(k, k.inst, 0, 0x1e)['rows'][:8]
    out['segment'] = _segment_out(k, ('你好',))
    return out


def bumped_item_guess(k):
    token = [t for t in tokens_of(k, '号') if t >> 24 == 1][0]
    out = {'before': unigram_of(k, token)}
    out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 5000000)
    out['after'] = unigram_of(k, token)
    out.update(_facade_total_out(k))
    return out


case('token-add-unigram-numerator-guess')(bumped_item_guess)


# Choosing a LONGER row trains its token through the same add, 69 * 7 = 483
# each time (pinyin.cpp:2521-2530). Two thousand choices of 你们 for `ni`
# make the pin read 跟你们 as 根你们.
def longer_choices_guess(k):
    inst = k.inst
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    chosen = 0
    for _ in range(2000):
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        cand = row_of(k, inst, 7)
        if not cand or k.fn('choose_candidate', I, P, Z, P)(inst, 0, cand) != 1:
            break
        chosen += 1
    out = {'chosen': chosen}
    for text in (b'gennimen', b'nimen'):
        fresh = k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(fresh, text)
        k.fn('guess_sentence', B, P)(fresh)
        out[text.decode()] = [final_step_sentences(k, fresh),
                              guess_rows(k, fresh, 0, 0x1e)['rows'][:6]]
    k._inst = None
    out['segment'] = _segment_out(k, ('跟你们',))
    return out


case('longer-choices-numerator-guess')(longer_choices_guess)


# Unloading a library takes its sub-index's whole total out of the facade,
# accepted bumps included, and loading it maps the stored items again
# (phrase_index.cpp:261-269, :237-248). The bump on a GBK token wraps the
# facade total to 2000000 while the library is loaded.
def unload_accepted_bump(k):
    token = 0x02000001
    out = {'gbk': _stored_library_total(k, 'gbk_char.bin'), 'before': unigram_of(k, token)}
    delta = (2 ** 32 + 2000000 - FACADE_TOTAL) % 2 ** 32
    out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, delta)
    out['loaded'] = _facade_total_out(k)
    out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 2)
    k._inst = None
    out['unloaded'] = _facade_total_out(k)
    out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, 2)
    k._inst = None
    out['reloaded'] = _facade_total_out(k)
    out['after'] = unigram_of(k, token)
    return out


case('unload-drops-accepted-bump')(unload_accepted_bump)


# #805: `pinyin_load_phrase_library` / `zhuyin_load_phrase_library` map a
# SYSTEM_FILE library again and merge only its saved `.dbin`
# (074a2219 pinyin.cpp:264-277, zhuyin.cpp:298-312), after `unload` freed
# the sub-index with everything the session had changed in it
# (phrase_index.cpp:261-268). An unsaved training step, an imported reading
# or an imported item is gone after the reload; the user index tables are
# not unloaded, so `lookup_tokens` still answers a dropped item's token while
# the item reads answer false. The pin's `guess_candidates` dereferences
# that dangling token (class (b)): the subject answers it as a missing item.
RELOAD_WATCH = ('礙', '龘龘')


def _reload_pron_keys(k, token):
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    count = U(UNTOUCHED)
    ok = k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
    out = [ok, count.value]
    for nth in range(count.value if ok else 0):
        keys = new(0, 0, 2)
        k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
        view = C.cast(keys, C.POINTER(Arr)).contents
        out.append(C.string_at(view.data, view.len * 2).hex() if view.len else '')
    return out


def _reload_candidates(k, text):
    """The candidate count, the first three rows and where the watched
    phrases sit in the list a fresh guess builds."""
    inst = k.alloc()
    parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
    k.fn(parse, Z, P, S)(inst, text)
    if k.mode == 'pinyin':
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
    else:
        k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    head, where = [], {}
    for i in range(count.value):
        cand, text_out, kind = P(), S(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text_out))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        shown = text_out.value.decode() if text_out.value else None
        if i < 3:
            head.append([shown, kind.value])
        if shown in RELOAD_WATCH:
            where[shown] = [i, kind.value]
    k.fn('free_instance', None, P)(inst)
    return [count.value, head, where]


def _reload_snapshot(k, phrases, inputs):
    k._inst = None
    out = {}
    for phrase in phrases:
        tokens = list(tokens_of(k, phrase))
        out['tokens ' + phrase] = [hex(token) for token in tokens]
        for token in tokens:
            out['unigram %s %s' % (phrase, hex(token))] = unigram_of(k, token)
            out['readings %s %s' % (phrase, hex(token))] = _reload_pron_keys(k, token)
    for text in inputs:
        out['candidates ' + text.decode()] = _reload_candidates(k, text)
    return out


def _reload_probe(change, library, save_first):
    def probe(k):
        phrases, inputs = change(k, False)
        out = {'before': _reload_snapshot(k, phrases, inputs)}
        change(k, True)
        out['changed'] = _reload_snapshot(k, phrases, inputs)
        if save_first:
            out['save'] = bool(k.fn('save', B, P)(k.ctx))
        out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, library)
        k._inst = None
        out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, library)
        k._inst = None
        out['reloaded'] = _reload_snapshot(k, phrases, inputs)
        return out
    return probe


def _reload_train(phrase, token_library, reading):
    def change(k, apply):
        if not apply:
            return [phrase], [reading]
        parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
        assert k.fn(parse, Z, P, S)(k.inst, reading) == len(reading)
        token = [t for t in tokens_of(k, phrase) if t >> 24 == token_library][0]
        train_selected_top(k, phrase, token)
    return change


def _reload_import(library, rows, phrases, inputs):
    def change(k, apply):
        if not apply:
            return phrases, inputs
        import_rows(k, library, rows)
    return change


# 礙 is GBK-only (0x02000005): library 2 is the one library `pinyin_unload_
# phrase_library` lets go, and `癫痫` (library 1) is one it refuses.
_RELOAD_PINYIN = (
    ('train', 2, _reload_train('礙', 2, b'ai4')),
    ('reading', 2, _reload_import(2, [('礙', 'ai1', 50)], ['礙'], [b'ai1'])),
    ('new-item', 2, _reload_import(2, [('龘龘', "da3'da3", 50)], ['龘龘'], [])),
    ('train-library-1', 1, _reload_train('癫痫', 1, b'dian4xian4')),
    ('import-library-1', 1, _reload_import(1, [('癫痫', "dian3'xian4", 50)], ['癫痫'], [b'dian3xian4'])),
)
# The zhuyin facade runs the same path (zhuyin.cpp:298-312) and lets every
# library but TSI_DICTIONARY (1) go. Inputs are standard-layout bopomofo.
_RELOAD_ZHUYIN = (
    ('train', 2, _reload_train('礙', 2, b'94')),
    ('reading', 2, _reload_import(2, [('礙', 'ㄞ', 50)], ['礙'], [b'9'])),
    ('new-item', 2, _reload_import(2, [('龘龘', 'ㄉㄚˇ ㄉㄚˇ', 50)], ['龘龘'], [])),
    ('train-library-1', 1, _reload_train('癫痫', 1, b'2u04vu04')),
)
for _mode, _table in (('pinyin', _RELOAD_PINYIN), ('zhuyin', _RELOAD_ZHUYIN)):
    for _name, _library, _change in _table:
        _tag = '%s-%s' % (_mode, _name)
        if _library == 1:
            # The facade refuses to unload library 1: both calls answer false
            # and the session's change stays.
            case('reload-refused-%s' % _tag, mode=_mode, control=True)(_reload_probe(_change, _library, False))
            continue
        case('reload-drops-session-%s' % _tag, mode=_mode)(_reload_probe(_change, _library, False))
        case('reload-keeps-saved-%s' % _tag, mode=_mode, control=True)(_reload_probe(_change, _library, True))


# Library 7 is the USER_FILE library: `unload` deletes its sub-index and the
# load builds it again from the saved `user.bin` chunk, or an empty sub-index
# when none was saved (zhuyin.cpp:247-265; pinyin twin pinyin.cpp:303-321).
# The pinyin facade never unloads it (pinyin.cpp:470-471). A dropped item's
# token stays in the phrase table, so no input is walked for it (class (b),
# policy row 102).
def _reload_user(setup, change, phrases, inputs, save_setup, save_change):
    def probe(k):
        if setup:
            import_rows(k, 7, setup)
            if save_setup:
                assert k.fn('save', B, P)(k.ctx)
        k._inst = None
        out = {'before': _reload_snapshot(k, phrases, inputs)}
        import_rows(k, 7, change)
        out['changed'] = _reload_snapshot(k, phrases, inputs)
        if save_change:
            out['save'] = bool(k.fn('save', B, P)(k.ctx))
        out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 7)
        k._inst = None
        out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, 7)
        k._inst = None
        out['reloaded'] = _reload_snapshot(k, phrases, inputs)
        return out
    return probe


_RELOAD_USER = {
    'pinyin': (("hao3'ni3", "hao4'ni3"), b'hao3ni3'),
    'zhuyin': (('ㄏㄠˇ ㄋㄧˇ', 'ㄏㄠˋ ㄋㄧˇ'), b'cl3su3'),
}
for _mode, (_readings, _input) in _RELOAD_USER.items():
    _first, _second = _readings
    _item = [('好你', _first, 50)]
    _more = [('好你', _second, 50)]
    # An unsaved item (nothing saved: the sub-index comes back empty) and an
    # unsaved reading on a saved item; the controls save first.
    _user = (
        ('item', [], _item, [], False, False, False),
        ('item-saved', [], _item, [_input], False, True, True),
        ('reading', _item, _more, [_input], True, False, False),
        ('reading-saved', _item, _more, [_input], True, True, True),
    )
    for _name, _setup, _change, _inputs, _save_setup, _save_change, _control in _user:
        _probe = _reload_user(_setup, _change, ['好你'], _inputs, _save_setup, _save_change)
        _tag = '%s-%s' % (_mode, _name)
        if _mode == 'pinyin':
            # The pinyin facade refuses to unload library 7: both calls answer
            # false and the session's change stays.
            case('reload-refused-user-%s' % _tag, mode=_mode, control=True)(_probe)
        else:
            case('reload-user-%s' % _tag, mode=_mode, control=_control)(_probe)


# After the reload the phrase table and the user pinyin index still hold the
# dropped token, and an import takes `get_range(7).m_range_end` as its token
# (zhuyin.cpp:472-478): a different phrase gets the dropped token again, and
# the kept index entry for the old reading now finds it; the same phrase meets
# its own token in the phrase table (zhuyin.cpp:447-468).
def _reload_reuse_setup(k):
    out = {'first': import_rows(k, 7, [('好你', 'ㄏㄠˇ ㄋㄧˇ', 50)])}
    out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 7)
    k._inst = None
    out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, 7)
    k._inst = None
    return out


@case('reload-user-zhuyin-reuse-other', mode='zhuyin')
def _(k):
    out = _reload_reuse_setup(k)
    out['again'] = import_rows(k, 7, [('你好', 'ㄋㄧˇ ㄏㄠˇ', 50)])
    out['after'] = _reload_snapshot(k, ['好你', '你好'], [b'cl3su3', b'su3cl3'])
    return out


# The entries the pin's user pinyin index kept for the dropped token are real
# rows: a save writes them, so a reopen still finds the old reading on the
# phrase that took the token, and a `mask_out` over the library's token range
# removes them, so a phrase that takes the token afterwards is not found under
# the old reading.
_REUSE_INPUTS = [b'cl3su3', b'su3cl3', b'cl3cl3']


@case('reload-user-zhuyin-reuse-other-save-reopen', mode='zhuyin')
def _(k):
    out = _reload_reuse_setup(k)
    out['again'] = import_rows(k, 7, [('你好', 'ㄋㄧˇ ㄏㄠˇ', 50)])
    out['save'] = bool(k.fn('save', B, P)(k.ctx))
    k._inst = None
    k.fn('fini', None, P)(k.ctx)
    k._ctx = None
    out['after'] = _reload_snapshot(k, ['好你', '你好'], _REUSE_INPUTS[:2])
    return out


@case('reload-user-zhuyin-reuse-other-mask', mode='zhuyin')
def _(k):
    out = _reload_reuse_setup(k)
    out['again'] = import_rows(k, 7, [('你好', 'ㄋㄧˇ ㄏㄠˇ', 50)])
    out['mask'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xFF000000, 0x07000000)
    k._inst = None
    out['third'] = import_rows(k, 7, [('好好', 'ㄏㄠˇ ㄏㄠˇ', 50)])
    out['after'] = _reload_snapshot(k, ['好你', '你好', '好好'], _REUSE_INPUTS)
    return out


# The same phrase meets its own token in the phrase table, `get_phrase_item`
# finds no item, and `assert(phrase_length == item.get_phrase_length())`
# (zhuyin.cpp:454) aborts the pin: class (c), the site the import iterator's
# other two asserts (`:440`, `:457`) already answer.
@case('reload-user-zhuyin-reuse-same', mode='zhuyin', abort=False)
def _(k):
    _reload_reuse_setup(k)
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '好你'.encode(), 'ㄏㄠˇ ㄋㄧˇ'.encode(), 50)}


# The dangling token: `lookup_tokens` keeps answering it, the item reads
# answer false, and a candidate walk over it is the pin's SIGSEGV
# (class (b)); the subject adds no candidate for it, as for a missing item.
def _reload_dangling(walk, reading):
    def probe(k):
        import_rows(k, 2, [('龘龘', reading, 50)])
        k._inst = None
        token = [t for t in tokens_of(k, '龘龘') if t >> 24 == 2][0]
        k.fn('unload_phrase_library', B, P, U)(k.ctx, 2)
        k._inst = None
        k.fn('load_phrase_library', B, P, U)(k.ctx, 2)
        k._inst = None
        return {'tokens': [hex(t) for t in tokens_of(k, '龘龘')], 'unigram': unigram_of(k, token),
                'readings': _reload_pron_keys(k, token), 'candidates': _reload_candidates(k, walk)}
    return probe


_RELOAD_DANGLING = {'tokens': ['0x2003cfd'], 'unigram': [False, 0], 'readings': [False, 0],
                    'candidates': [88, [['大大', 2], ['鞑靼', 2], ['达达', 2]], {}]}
case('reload-dangling-token-candidates', crash=_RELOAD_DANGLING)(_reload_dangling(b'dada', "da3'da3"))
case('reload-dangling-token-candidates-zhuyin', mode='zhuyin',
     crash=_RELOAD_DANGLING)(_reload_dangling(b'283283', 'ㄉㄚˇ ㄉㄚˇ'))


# `unload` takes the library's total out of the facade, an unsaved import's
# mass included (phrase_index.cpp:261-268): while library 2 is gone, the
# sentence and candidate order divide by the smaller total, and the reload
# puts the saved state's share back.
def _reload_total_window(k):
    out = {}

    def look(tag):
        k._inst = None
        for text in (b'nihao', b'women', b'xian', b'zhongguo'):
            inst = k.alloc()
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
            k.fn('guess_sentence', B, P)(inst)
            out['%s sentence %s' % (tag, text.decode())] = sentence_out(k, inst, 0)
            out['%s candidates %s' % (tag, text.decode())] = guess_rows(k, inst, 0, 0x1e)['rows'][:12]
            k.fn('free_instance', None, P)(inst)

    look('loaded')
    import_rows(k, 2, [('礙', 'ai1', 2000000000), ('龘龘', "da3'da3", 2000000000)])
    look('imported')
    out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 2)
    look('unloaded')
    out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, 2)
    look('reloaded')
    return out


case('reload-total-while-unloaded')(_reload_total_window)


# A system-library import reloaded from its .dbin is counted twice by the
# merge (phrase_index.cpp:453-459 after the header at :503-514). A mask_out
# that removes it reloads the chunk and merges the masked log, taking out the
# sub-index's current total and putting the new one in (:301-332, :237-248).
# Before any compact, the current total still carries the second count; after
# one (here, an import's end, pinyin.cpp:657) it does not, and the facade keeps
# it. The count's reload offset alone wraps the facade total to 1900001. A
# build with no reload offset at all lands on the pin's total in the first
# case and misses it in the second; one that keeps the load-time offset
# through the mask misses the first.
def mask_out_reload(compact):
    def probe(k):
        count = (2 ** 32 + 1900001 - FACADE_TOTAL) // 3
        assert _stored_library_total(k, 'gb_char.bin') + 3 * count <= 0xFFFFFFFF
        out = {'added': import_rows(k, 1, [('龘龘', "da3'da3", count)])}
        out['saved'] = k.fn('save', B, P)(k.ctx)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        token = [t for t in tokens_of(k, '龘龘') if t >> 24 == 1][0]
        if compact:
            out['import'] = import_rows(k, 7, [('好你', "hao3'ni3", 2)])
        out['mask'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xFFFFFFFF, token)
        k._inst = None
        out.update(_facade_total_out(k))
        return out
    return probe


case('mask-out-replaces-reload-offset', control=True)(mask_out_reload(False))
case('mask-out-after-compact-keeps-reload-offset')(mask_out_reload(True))


# #802. SubPhraseIndex::add_unigram_frequency refuses when m_total_freq +
# delta wraps (074a2219 phrase_index.cpp:170-171), and until the first compact
# m_total_freq is the total stored in the library file (:345), not its items'
# sum. A MODIFY_HEADER log sets library 1's stored total _STORED_GAP short of
# the guint32 wrap: a delta of GAP - 1 is accepted, GAP is refused. A save
# compacts (pinyin.cpp:1130), the total becomes the items' sum, and GAP is
# accepted. A build that checks the item sum accepts every one of them.
_STORED_GAP = 5000000


def stored_total_overflow(refused, after_save):
    def probe(k):
        total = _stored_library_total(k, 'gb_char.bin')
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        _write_user_conf(k)
        _write_log(k, _modify_header(0, total, 2 ** 32 - _STORED_GAP))
        # The refused delta overshoots the wrap by the amount that lands the
        # facade total on 4000000000, which moves the ranking off the unbumped one.
        delta = _STORED_GAP - 1
        if refused:
            delta = _STORED_GAP + (4000000000 - FACADE_TOTAL + total) % 2 ** 32
            assert _STORED_GAP <= delta < 2 ** 32
        token = [t for t in tokens_of(k, '号') if t >> 24 == 1][0]
        add = k.fn('token_add_unigram_frequency', B, P, U, U)
        out = {'before': unigram_of(k, token)}
        out['add'] = add(k.inst, token, delta)
        out['after'] = unigram_of(k, token)
        # A refused delta still raised the facade total (phrase_index.h:633-634),
        # which the guess and the candidate frequencies divide by.
        out['facade'] = _facade_total_out(k)
        if after_save:
            # An import marks the context modified; the save compacts.
            out['import'] = import_rows(k, 7, [('好你', "hao3'ni3", 2)])
            out['saved'] = k.fn('save', B, P)(k.ctx)
            out['add-saved'] = add(k.inst, token, delta)
            out['after-saved'] = unigram_of(k, token)
            out['facade-saved'] = _facade_total_out(k)
        return out
    return probe


case('stored-total-overflow-below', control=True)(stored_total_overflow(False, False))
case('stored-total-overflow-above')(stored_total_overflow(True, False))
case('stored-total-overflow-above-saved')(stored_total_overflow(True, True))
case('stored-total-overflow-below-saved', control=True)(stored_total_overflow(False, True))


# Each MODIFY_HEADER must land on the merge's running total, or the merge
# stops there; one that lands sets the total (phrase_index.cpp:503-514). Two
# headers from the same old total: the second stops the merge. A chained pair:
# both apply. Either way the pin's final total wraps the facade to 2000000.
def log_headers(chained):
    def probe(k):
        total = _stored_library_total(k, 'gb_char.bin')
        shift = (2 ** 32 + 2000000 - FACADE_TOTAL) % 2 ** 32
        if chained:
            first = (total + 1000000000) % 2 ** 32
            log = _modify_header(0, total, first) + _modify_header(0, first, (total + shift) % 2 ** 32)
        else:
            log = _modify_header(0, total, (total + shift) % 2 ** 32) + _modify_header(0, total, total)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        _write_user_conf(k)
        _write_log(k, log)
        return {'total': total, **_facade_total_out(k)}
    return probe


case('log-headers-second-stops-merge')(log_headers(False))
case('log-headers-chained-both-apply')(log_headers(True))


# --------------------------------------------------------------------------
# #757 PR 3, the lookup side. The system and user grams merge as the pin's
# merge_single_gram builds them (074a2219 ngram.cpp:277-352: both absent,
# no row; one absent, its bytes; both present, the two-pointer interleave),
# and the lookups read the merged chunk as the pin does: the n-best step
# with SingleGram::search per range, one step per yielded item
# (phonetic_lookup.h:594-632, ngram.cpp:153-176); DYNAMIC_ADJUST and the
# phrase segmenter with get_freq from lower_bound (pinyin.cpp:1845-1852,
# phrase_lookup.cpp:268-290, ngram.cpp:231-250). Nothing is sorted,
# deduplicated or filtered on a count. The grams sit under the sentence
# start, the predecessor of the first phrase in all three lookups.
# --------------------------------------------------------------------------
_PAST_RANGES = 0x0fffffff
_BOOST = 1 << 31


def _start_gram(kind, token):
    """The sentence start's planted gram. 'unsorted': [past-every-range,
    token], so lower_bound from any range start lands on the first item and
    the walk stops there; the boosted `token` is never met. 'duplicate':
    `token` twice, whose counts wrap to 1 when summed, so only a reader that
    keeps both items sees the boost."""
    if kind == 'unsorted':
        return _gram_bytes(_BOOST + 1, [(_PAST_RANGES, 1), (token, _BOOST)])
    return _gram_bytes(_BOOST, [(token, _BOOST), (token, (1 << 32) - _BOOST + 1)])


def _replace_user_bigram(k, rows):
    """Close the context and replace user_bigram.db with `rows`, a list of
    (predecessor, raw value), in this cell's own format."""
    if k._inst is not None:
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
    k.fn('fini', None, P)(k.ctx)
    k._ctx = None
    d18_bigram_db(k, Path(k.user, 'user_bigram.db'),
                  [(p.to_bytes(4, 'little'), value) for p, value in rows])
    _write_user_conf(k)


def _replace_system_start_row(k, value):
    """A copy of the system tables whose bigram.db holds `value` under the
    sentence start."""
    if k._inst is not None:
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
    k.fn('fini', None, P)(k.ctx)
    k._ctx = None
    copied = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
    shutil.copytree(k.data, copied, dirs_exist_ok=True)
    k.data = copied
    d18_bigram_db(k, Path(copied, 'bigram.db'), [(b'\x01\x00\x00\x00', value)])
    _write_user_conf(k)


def _guess_out(k, text=b'nihao'):
    if k.mode == 'zhuyin':
        # The zhuyin facade parses full pinyin under this option word, as
        # the d18 merge cases do.
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    guessed = k.fn('guess_sentence', B, P)(inst)
    return dict(parsed=parsed, guessed=guessed, sentences=final_step_sentences(k, inst))


def _dynamic_adjust_out(k, text=b'nihao'):
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | (1 << 9))
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    rows = guess_rows(k, inst, 0, 0x1e) if k.mode == 'pinyin' else zhuyin_guess_rows(k, inst, 0)
    return dict(parsed=parsed, **rows)


def _segment_out(k, texts):
    segment = k.fn('phrase_segment', B, P, S)
    n_phrase = k.fn('get_n_phrase', B, P, C.POINTER(U))
    token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))
    out = {}
    for text in texts:
        ret = segment(k.inst, text.encode())
        count = U(UNTOUCHED)
        n_phrase(k.inst, C.byref(count))
        tokens = []
        for index in range(count.value if count.value != UNTOUCHED else 0):
            slot = U(UNTOUCHED)
            token(k.inst, index, C.byref(slot))
            tokens.append(slot.value)
        out[text] = [ret, tokens]
    return out


def _system_start_tokens(k):
    raw = d18_bigram_db(k, Path(k.data, 'bigram.db'), read_keys=(b'\x01\x00\x00\x00',))[0]
    return [token for token, _ in _gram_items(raw)[1]]


def _pin_lower_bound(at, length, token):
    # libstdc++'s __lower_bound, as SingleGram's std_lite::lower_bound runs.
    first = 0
    while length > 0:
        half = length >> 1
        if at(first + half) < token:
            first, length = first + half + 1, length - half - 1
        else:
            length = half
    return first


def _interleave_miss_layout(system, token):
    """A user gram [token, z, a] (a < token < z) that get_freq finds on its
    own but misses once merged with the sorted system gram `system`: the
    merge keeps the user order and places `a` after `z`, out of order, and
    `z` is picked so that the merged lower_bound probes `a`, steps past it
    and lands beyond `token` (ngram.cpp:308-349, :231-250)."""
    assert system == sorted(system), 'the installed start gram is sorted'
    present, below = set(system), sum(1 for item in system if item < token)
    tied = int(token in present)
    a = next(t for t in range(token - 1, 0, -1) if t not in present)
    gaps = [item + 1 for item in system[below:] if item + 1 not in present and item + 1 > token]
    if token + 1 not in present:
        gaps.insert(0, token + 1)
    for z in gaps:
        before_z = sum(1 for item in system if item < z)
        merged = (system[:below] + [token] + system[below + tied:before_z] + [z, a]
                  + system[before_z:])
        start = _pin_lower_bound(lambda i: merged[i], len(merged), token)
        walk = next((t for t in merged[start:] if t >= token), None)
        if walk != token:
            return [(token, _BOOST), (z, 1), (a, 1)]
    raise AssertionError('no interleave misses the token')


# 尼 is a ni reading the n-best and the candidate list rank below 你; the
# boost would put it first for a reader that met it. A boosted 今天 splits
# 今天天气 off the start of 今天天气很好.
def user_gram_lookup(kind, operation):
    def probe(k):
        boosted = tokens_of(k, '今天' if operation == 'phrase-segment' else '尼')[0]
        if operation == 'phrase-segment' and kind == 'unsorted':
            # The phrase segmenter reads each pair, so a gram get_freq
            # misses on its own reads the same merged or not; only the
            # merge's interleave tells the two apart.
            items = _interleave_miss_layout(_system_start_tokens(k), boosted)
            value = _gram_bytes(_BOOST + 2, items)
        else:
            value = _start_gram(kind, boosted)
        _replace_user_bigram(k, [(1, value)])
        if operation == 'guess':
            return _guess_out(k)
        if operation == 'dynamic-adjust':
            return _dynamic_adjust_out(k)
        return _segment_out(k, ('今天天气很好',))
    return probe


for _kind in ('unsorted', 'duplicate'):
    for _operation in ('guess', 'dynamic-adjust', 'phrase-segment'):
        # A duplicated user gram reads the same per pair as merged: the
        # merge sums the first duplicate with the system item and appends
        # the second after it, so get_freq meets the same first match.
        control = (_kind, _operation) == ('duplicate', 'phrase-segment')
        case(f'user-gram-{_kind}-{_operation}', control=control)(
            user_gram_lookup(_kind, _operation))


# PR 0's ranges and SingleGram::search's one-sided bound (ngram.cpp:161-173):
# the walk stops at the first token at or past the range end, and nothing
# checks the range start. The user gram [你, T, T] for a ni reading T below
# 你 merges to [..., 你, T, T, ...] (ngram.cpp:308-349), so the walk over
# 你's range yields both T items, below that range, and the pin steps T
# with each (phonetic_lookup.h:625-629). The two T counts wrap to 1 when
# summed, so a reader that sorts and sums the gram does not boost T.
@case('user-gram-unsorted-range-boundary')
def _(k):
    ni = tokens_of(k, '你')[0]
    below = next(token for token in (tokens_of(k, text)[0] for text in '泥拟逆腻妮倪匿溺尼')
                 if token + 1 < ni)
    items = [(ni, 1), (below, _BOOST), (below, (1 << 32) - _BOOST + 1)]
    _replace_user_bigram(k, [(1, _gram_bytes(_BOOST, items))])
    return _guess_out(k)


# The system gram enters the merge as stored, with no user gram beside it.
# 'duplicate' holds 尼 at 1 and then at the boost: search yields both items,
# so the boosted one steps; a reader that keeps the first match does not.
def system_gram_lookup(kind):
    def probe(k):
        ni2 = tokens_of(k, '尼')[0]
        value = (_start_gram(kind, ni2) if kind == 'unsorted'
                 else _gram_bytes(_BOOST + 1, [(ni2, 1), (ni2, _BOOST)]))
        _replace_system_start_row(k, value)
        return _guess_out(k)
    return probe


for _kind in ('unsorted', 'duplicate'):
    case(f'system-gram-{_kind}-guess')(system_gram_lookup(_kind))


# A stored zero count is an item. The n-best step takes a bigram step for
# each item `search` yields (phonetic_lookup.h:625-629) beside the unigram
# pass's step from the best predecessor, so a one-syllable best sentence
# the start's merged gram holds is answered twice and one it lacks once.
# The start's system gram is replaced by one item past every range, and
# its user gram holds the best sentence's phrase at count 0: the pin's
# merged row holds that item, so the sentence comes twice. A reader that
# drops zero counts answers it once.
@case('user-gram-zero-count-guess')
def _(k):
    _replace_system_start_row(k, _gram_bytes(1, [(_PAST_RANGES, 1)]))
    best = _guess_out(k, b'a')['sentences'][0]
    _replace_user_bigram(k, [(1, _gram_bytes(0, [(tokens_of(k, best)[0], 0)]))])
    return _guess_out(k, b'a')


# N2, register row 85: a user gram shorter than its guint32 total. Beside a
# system gram the merge reads it (ngram.cpp:304, through phonetic_lookup.h:598,
# pinyin.cpp:2214, zhuyin.cpp:1487, phrase_lookup.cpp:272) and the pin dies
# at memory_chunk.h:390; alone, SingleGram::search reads it
# (ngram.cpp:165, through phonetic_lookup.h:625). The sentence start has a
# system gram; a user phrase used as a sentence prefix has none.
_SHORT = b'\x07\x00\x00'


def short_start_guess(k):
    _replace_user_bigram(k, [(1, _SHORT)])
    return {'ret': _guess_out(k)['guessed']}


for _mode in ('pinyin', 'zhuyin'):
    case(f'abort-guess-sentence-short-user-gram-system-row-{_mode}', mode=_mode, abort=False)(
        short_start_guess)


@case('abort-guess-sentence-short-user-gram-no-system-row', abort=False)
def _(k):
    def grams(system, users):
        return [(users[0], _SHORT)]
    _plant_user_grams(k, [_DALONG], grams)
    inst = k.inst
    assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao') == 5
    return {'ret': k.fn('guess_sentence_with_prefix', B, P, S)(inst, _DALONG[0].encode())}


def short_start_dynamic_adjust(k):
    _replace_user_bigram(k, [(1, _SHORT)])
    return {'ret': _dynamic_adjust_out(k)['ret']}


for _mode in ('pinyin', 'zhuyin'):
    case(f'abort-guess-candidates-dynamic-adjust-short-user-gram-{_mode}', mode=_mode,
         abort=False)(short_start_dynamic_adjust)


# The same gram under DYNAMIC_ADJUST, parsed and never guessed: the pin's
# parse merges no gram (pinyin.cpp:1497-1520, zhuyin.cpp:1017-1043), so the
# parse answers its full length.
def short_start_dynamic_adjust_parse(k):
    _replace_user_bigram(k, [(1, _SHORT)])
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | (1 << 9))
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    return dict(parsed=parsed, length=k.fn('get_parsed_input_length', Z, P)(inst))


for _mode in ('pinyin', 'zhuyin'):
    case(f'parse-dynamic-adjust-short-user-gram-{_mode}', mode=_mode)(
        short_start_dynamic_adjust_parse)


@case('abort-phrase-segment-short-user-gram', abort=False)
def _(k):
    _replace_user_bigram(k, [(1, _SHORT)])
    return {'ret': k.fn('phrase_segment', B, P, S)(k.inst, '今天天气很好'.encode())}


# Controls: a short gram the lookup never loads. 我 never precedes a phrase
# of `nihao`; with DYNAMIC_ADJUST clear the candidate list merges nothing
# (pinyin.cpp:2207-2216).
@case('guess-unrelated-short-user-gram', control=True)
def _(k):
    _replace_user_bigram(k, [(tokens_of(k, '我')[0], _SHORT)])
    return _guess_out(k)


@case('guess-candidates-short-user-gram-dynamic-adjust-off', control=True)
def _(k):
    _replace_user_bigram(k, [(1, _SHORT)])
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a)
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    return dict(parsed=parsed, **guess_rows(k, inst, 0, 0x1e))


# --------------------------------------------------------------------------
# #563 axis B (bdb) — the libzhuyin per-entry-point contract. Four exports
# no other case reached (`zhuyin_parse_chewing`,
# `zhuyin_in_chewing_keyboard`, `zhuyin_get_n_phrase`,
# `zhuyin_get_phrase_token`), the optional out-params and the empty-instance
# boundary of `zhuyin_get_zhuyin_key_rest_positions`, and the two pin bodies
# that never read their instance. The NULL-argument cases are the libzhuyin
# half of register row 41 (class (b): the pin dereferences and dies, the
# subject's guard answers). Pin cites are in the PR body; each case names
# the site it holds.
# --------------------------------------------------------------------------


def zy_strv(k, pointer):
    """A `gchar **` a keyboard probe answered; decode and free it."""
    if pointer == UNTOUCHED:
        return 'untouched'
    if not pointer:
        return None
    free = k.glib.g_strfreev
    free.restype, free.argtypes = None, [P]
    array, out, index = C.cast(pointer, C.POINTER(S)), [], 0
    while array[index]:
        out.append(array[index].decode('utf-8', 'replace'))
        index += 1
    free(pointer)
    return out


def parse_chewing_probe(schemes, texts):
    def probe(k):
        out = {}
        for scheme in schemes:
            if scheme is not None:
                k.fn('set_chewing_scheme', B, P, I)(k.ctx, scheme)
            parse = k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))
            row, key = {}, C.c_ushort(UNTOUCHED & 0xFFFF)
            for text in texts:
                key.value = UNTOUCHED & 0xFFFF
                row[repr(text)] = [parse(k.inst, text, C.byref(key)), key.value]
            out[str(scheme)] = row
        return out
    return probe


# `parse_one_key` (Simple `zhuyin_parser2.cpp:163-215`, Discrete `:331-...`,
# DaChen `:789-...`) probes the last byte for a tone under `USE_TONE`, maps
# the rest through the scheme's symbol table and looks the chewing up in
# `zhuyin_index`; `*onekey` is written only on success. `su3cl3` holds two
# keys and is refused as one; the empty string and a tone-only byte are
# refused; every implemented scheme is probed.
case('zhuyin-parse-chewing-key', mode='zhuyin', control=True)(
    parse_chewing_probe((None, 1, 2, 3, 4, 5, 6, 8, 9),
                        (b'su3', b'cl3', b'su', b'', b'zzz', b'su3cl3', b'su3x', b'1', b'3', b'ni')))


def in_chewing_probe(scheme, options, keys):
    def probe(k):
        if scheme is not None:
            k.fn('set_chewing_scheme', B, P, I)(k.ctx, scheme)
        if options is not None:
            k.fn('set_options', B, P, U)(k.ctx, options)
        keyboard = k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))
        row = {}
        for key in keys:
            symbols = P(UNTOUCHED)
            ret = keyboard(k.inst, key, C.byref(symbols))
            row[repr(key)] = [ret, zy_strv(k, symbols.value)]
        return row
    return probe


# `in_chewing_scheme` fixes `*symbols = NULL` first and fills it only when
# the byte maps (Simple `zhuyin_parser2.cpp:301-335`, Discrete `:496-540`,
# DaChen `:798-846`); the tone table is consulted only under `USE_TONE`.
case('zhuyin-in-chewing-keyboard-key', mode='zhuyin', control=True)(
    in_chewing_probe(None, None, (b's', b'u', b'3', b'1', b'a', b'z', b'q', b'!', b'~', b'\x80', b'\x00', b'i')))
case('zhuyin-in-chewing-keyboard-no-tone', mode='zhuyin', control=True)(
    in_chewing_probe(None, 0, (b's', b'3', b'1')))
for _zyscheme in (2, 3, 4, 5, 6, 8, 9):
    case(f'zhuyin-in-chewing-keyboard-scheme-{_zyscheme}', mode='zhuyin', control=True)(
        in_chewing_probe(_zyscheme, None, (b's', b'u', b'i', b'3')))


def key_rest_positions_probe(text, offsets):
    # `zhuyin_get_zhuyin_key_rest` on an empty matrix aborts the pin (a
    # registered class (c) site held by `abort-zhuyin-get-zhuyin-key-rest-
    # empty-matrix`), so the accessor is only reachable on a parsed instance;
    # this case takes a key-rest pointer from the getter the pin's own call
    # returns, then exercises the optional `begin`/`end`.
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, 0x1aa)
        inst = k.inst
        k.fn('parse_more_chewings', Z, P, S)(inst, text.encode())
        rest = k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))
        positions = k.fn('get_zhuyin_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))
        out = {}
        for offset in offsets:
            slot = P(UNTOUCHED)
            got = rest(inst, offset, C.byref(slot))
            row = {'rest_ret': got,
                   'rest': 'untouched' if slot.value == UNTOUCHED else 'null' if not slot.value else 'set'}
            if got and slot.value:
                # `zhuyin.cpp:1930-1941`: both out-params are optional, neither
                # is written when NULL, and the call always answers true.
                begin, end = C.c_ushort(0xABCD), C.c_ushort(0xABCD)
                row['both'] = [positions(inst, slot, C.byref(begin), C.byref(end)), begin.value, end.value]
                begin = C.c_ushort(0xABCD)
                row['begin-only'] = [positions(inst, slot, C.byref(begin), None), begin.value]
                end = C.c_ushort(0xABCD)
                row['end-only'] = [positions(inst, slot, None, C.byref(end)), end.value]
                row['neither'] = positions(inst, slot, None, None)
            out[str(offset)] = row
        return out
    return probe


case('zhuyin-key-rest-positions-out', mode='zhuyin', control=True)(
    key_rest_positions_probe('su3cl3', (0, 1, 2)))


def phrase_tokens_probe(texts):
    def probe(k):
        inst = k.inst
        segment = k.fn('phrase_segment', B, P, S)
        n_phrase = k.fn('get_n_phrase', B, P, C.POINTER(U))
        token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))

        def read(prefix):
            count = U(UNTOUCHED)
            n_ret = n_phrase(inst, C.byref(count))
            rows, index = [], 0
            while index < (count.value if count.value != UNTOUCHED else 0):
                slot = U(0xDEADBEEF)
                rows.append([token(inst, index, C.byref(slot)), slot.value])
                index += 1
            past = []
            for index in (count.value if count.value != UNTOUCHED else 0, 0xFFFFFFFF):
                slot = U(0xDEADBEEF)
                past.append([token(inst, index, C.byref(slot)), slot.value])
            return {prefix + '-n': [n_ret, count.value], prefix + '-tokens': rows, prefix + '-past': past}

        out = {'empty-instance': read('empty')}
        for text in texts:
            row = {'segment': segment(inst, text)}
            row.update(read('row'))
            out[repr(text)] = row
        segment(inst, '你好'.encode())
        k.fn('reset', B, P)(inst)
        out['after-reset'] = read('reset')
        return out
    return probe


# `zhuyin_get_n_phrase` writes `*num` unconditionally and answers true
# (`zhuyin.cpp:2227-2231`); `zhuyin_get_phrase_token` writes the null token
# before its range check, so a refused index still writes 0 (`:2233-2244`).
# `phrase_segment` fills the array, `reset` empties it (`:1723-1729`).
case('zhuyin-phrase-result-tokens', mode='zhuyin', control=True)(
    phrase_tokens_probe(('你好'.encode(), b'', b'123', '你好世界'.encode())))


# The two string getters' whole surface: `zhuyin_get_zhuyin_string`
# (`zhuyin.cpp:1732-1741`) renders the key's zhuyin spelling;
# `zhuyin_get_pinyin_string` (`:1743-1766`) reads the context's scheme and
# dispatches (`FULL_PINYIN_HANYU` 1, `LUOMA` 2, `SECONDARY_ZHUYIN` 3). Both
# NULL the out-param before the `get_table_index() == 0` refusal, so a
# refused zero key leaves NULL, not the caller's sentinel.
@case('zhuyin-string-getters', mode='zhuyin', control=True)
def _(k):
    inst = k.inst
    key = C.c_ushort(0)
    assert k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(inst, b'su3', C.byref(key))
    zero = C.c_ushort(0)
    out = {}
    for scheme in (None, 1, 2, 3):
        if scheme is not None:
            assert k.fn('set_full_pinyin_scheme', B, P, I)(k.ctx, scheme)
        for label, name in (('zhuyin_string', 'get_zhuyin_string'),
                            ('pinyin_string', 'get_pinyin_string')):
            fn = k.fn(name, B, P, C.POINTER(C.c_ushort), C.POINTER(P))
            string = P(UNTOUCHED)
            out['%s scheme %s' % (label, scheme)] = [
                fn(inst, C.byref(key), C.byref(string)), owned_text(k, string)]
            string = P(UNTOUCHED)
            out['%s scheme %s zero-key' % (label, scheme)] = [
                fn(inst, C.byref(zero), C.byref(string)), owned_text(k, string)]
    return out


# `zhuyin_get_zhuyin_offset` in range: the cursor is clamped to the parse end
# and walked back over empty columns to the first key (`zhuyin.cpp:1973-1992`).
# The past-matrix cursor lives in `zhuyin-cursor-neighbours`.
@case('zhuyin-cursor-offset-range', mode='zhuyin', control=True)
def _(k):
    inst = zhuyin_chewing(k)
    fn = k.fn('get_zhuyin_offset', B, P, Z, C.POINTER(Z))
    out = {}
    for cursor in (0, 1, 2, 3, 6):
        res = Z(UNTOUCHED)
        out[str(cursor)] = [fn(inst, cursor, C.byref(res)),
                            'untouched' if res.value == UNTOUCHED else res.value]
    return out


# The five exports whose pin bodies never read their instance:
# `zhuyin_get_zhuyin_string` (`zhuyin.cpp:1732-1741`),
# `zhuyin_get_candidate_type` (`:1858-1863`),
# `zhuyin_get_candidate_string` (`:1865-1870`),
# `zhuyin_get_zhuyin_key_rest_positions` (`:1930-1941`) and
# `zhuyin_get_zhuyin_key_rest_length` (`:1942-1946`). A NULL instance is a
# defined call there, so the subject answers what the pin answers.
@case('zhuyin-null-instance-defined-getters', mode='zhuyin')
def _(k):
    inst = k.inst
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    key = C.c_ushort(0)
    assert k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(inst, b'su3', C.byref(key))
    rest = P(UNTOUCHED)
    assert k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))(inst, 0, C.byref(rest)) and rest.value
    assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    cand = P(UNTOUCHED)
    assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand)) and cand.value
    out = {}
    string = P(UNTOUCHED)
    out['zhuyin_string'] = [k.fn('get_zhuyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(
        None, C.byref(key), C.byref(string)), owned_text(k, string)]
    begin, end = C.c_ushort(0xABCD), C.c_ushort(0xABCD)
    out['key_rest_positions'] = [k.fn('get_zhuyin_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))(
        None, rest, C.byref(begin), C.byref(end)), begin.value, end.value]
    length = C.c_ushort(0xBEEF)
    out['key_rest_length'] = [k.fn('get_zhuyin_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(
        None, rest, C.byref(length)), length.value]
    kind = I(0x7FFFFFFF)
    out['candidate_type'] = [k.fn('get_candidate_type', B, P, P, C.POINTER(I))(
        None, cand, C.byref(kind)), kind.value]
    candidate_string = P(UNTOUCHED)
    out['candidate_string'] = [k.fn('get_candidate_string', B, P, P, C.POINTER(P))(
        None, cand, C.byref(candidate_string)),
        'untouched' if candidate_string.value == UNTOUCHED
        else C.string_at(candidate_string.value).decode('utf-8', 'replace')]
    return out


# `token_get_phrase`'s optional out-params: `_token_get_phrase`
# (`zhuyin.cpp:1095-1113`, the zhuyin twin of the shared pinyin helper)
# writes `*len` and `*utf8_str` only when their pointer is non-NULL, so all
# four combinations are defined calls — on a token that resolves and on one
# that is refused before either write.
@case('zhuyin-token-get-phrase-optional-out', mode='zhuyin')
def _(k):
    fn = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))
    out = {}
    for label, token in (('known', tokens_of(k, '你好')[0]), ('unknown', 0xFFFFFFFF)):
        length, text = U(UNTOUCHED), P(UNTOUCHED)
        both = fn(k.inst, token, C.byref(length), C.byref(text))
        out[label] = [both, 'untouched' if length.value == UNTOUCHED else length.value,
                      'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        length = U(UNTOUCHED)
        out[label + '-len-only'] = [fn(k.inst, token, C.byref(length), None), length.value]
        text = P(UNTOUCHED)
        out[label + '-text-only'] = [fn(k.inst, token, None, C.byref(text)),
                                     'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        out[label + '-neither'] = fn(k.inst, token, None, None)
    return out


# #804: a 256th reading on one token. `PhraseItem::add_pronunciation` stores
# `npron + 1` through a `guint8` (`phrase_index.cpp:28`, `:88` at 074a2219),
# so the count wraps to 0 and the call still answers true. Every read sizes
# the item from that count (`SubPhraseIndex::get_phrase_item`, `:203-204`),
# and the next add copies only that view (`remove_phrase_item`, `:228`): the
# item has no readings, keeps its unigram, and the 257th add leaves exactly
# one reading, the new one. The cases fill libraries 1-7 to 255 readings with
# a first frequency of 11 and 1 after it, add the 256th, and read the item
# live, after `end_add_phrases`, and after save and reopen; the 257th add
# (frequency 9) follows in the same iterator or in the next one. A library the
# pin does not load answers false on the first add and is recorded as such.
_WRAP_PHRASES = {1: '龘', 2: '龖', 3: '龗', 4: '鬱', 5: '鑫', 6: '靐', 7: '麤'}


def _wrap_pinyin_readings():
    initials = ['b', 'p', 'm', 'f', 'd', 't', 'n', 'l', 'g', 'k', 'h', 'j', 'q', 'x', 'zh', 'ch', 'sh',
                'r', 'z', 'c', 's', 'y', 'w', '']
    finals = ['a', 'o', 'e', 'ai', 'ei', 'ao', 'ou', 'an', 'en', 'ang', 'eng', 'i', 'u', 'ia', 'ie',
              'iao', 'iu', 'ian', 'in', 'iang', 'ing', 'ua', 'uo', 'uai', 'ui', 'uan', 'un', 'uang', 'ong']
    return [(i + f + t).encode() for i in initials for f in finals for t in '1234']


def _wrap_zhuyin_readings():
    initials = 'ㄅㄆㄇㄈㄉㄊㄋㄌㄍㄎㄏㄐㄑㄒㄓㄔㄕㄖㄗㄘㄙ'
    finals = ['ㄚ', 'ㄛ', 'ㄜ', 'ㄞ', 'ㄟ', 'ㄠ', 'ㄡ', 'ㄢ', 'ㄣ', 'ㄤ', 'ㄥ', 'ㄧ', 'ㄨ', 'ㄩ', 'ㄧㄚ', 'ㄧㄝ',
              'ㄧㄠ', 'ㄧㄡ', 'ㄧㄢ', 'ㄧㄣ', 'ㄧㄤ', 'ㄧㄥ', 'ㄨㄚ', 'ㄨㄛ', 'ㄨㄞ', 'ㄨㄟ', 'ㄨㄢ', 'ㄨㄣ',
              'ㄨㄤ', 'ㄨㄥ']
    return [(i + f + t).encode() for i in initials for f in finals for t in ('ˊ', 'ˇ', 'ˋ', '')]


def _wrap_distinct(k, candidates):
    """The candidates the facade accepts as a one-syllable reading, found by
    adding each to a fresh one-character phrase on a scratch context: a new
    phrase has no reading to merge with, so only a refused parse answers
    false, and in the case itself a refused add can only be the 256th."""
    scratch = tempfile.mkdtemp(prefix='wrap-', dir=k.scratch)
    ctx = k.init(user=scratch)
    assert ctx, 'scratch context'
    iterator = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    kept = [reading for number, reading in enumerate(candidates)
            if k.fn('iterator_add_phrase', B, P, S, S, I)(iterator, chr(0x4E00 + number).encode(), reading, 1)]
    k.fn('end_add_phrases', None, P)(iterator)
    k.fn('fini', None, P)(ctx)
    assert len(kept) >= 258, len(kept)
    return kept


def _wrap_item_reads(k, lib, text, rows, result, label):
    """Records the item `text` in `lib` as the C API shows it under `label`
    (None if it has no token); what the pin leaves indeterminate goes under
    a `~` key, which the comparison leaves out."""
    tokens = [t for t in tokens_of(k, text) if t >> 24 == lib]
    if not tokens:
        result[label] = None
        return
    token = tokens[0]
    count = U(UNTOUCHED)
    ret = k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
    out, loose = {'token': hex(token), 'n': [ret, count.value]}, {}
    # An item the user store holds in the addon or network library has no
    # unigram through `token_get_unigram_frequency`, which reads the shipped
    # addon table there; that is unrelated to the wrap, so it is recorded but
    # not compared for those two libraries.
    (out if lib not in (5, 6) else loose)['unigram'] = unigram_of(k, token)
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    for nth in (0, 1, 254, 255):
        keys = new(0, 0, 2)
        ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
        view = C.cast(keys, C.POINTER(Arr)).contents
        content = C.string_at(view.data, view.len * 2).hex() if view.len else ''
        # Past the last counted reading the pin hands back an untouched stack
        # buffer; the ruled answer is zeroed keys (class (b) for the content).
        if nth < count.value:
            out['nth %d' % nth] = [ret, view.len, content]
        else:
            out['nth %d' % nth] = [ret, view.len]
            loose['content nth %d' % nth] = content
    if rows:
        listed = rows(text)
        out['export'] = [len(listed), listed[:1], listed[-1:], hashlib.sha256(
            json.dumps(listed, ensure_ascii=False).encode()).hexdigest()]
    result[label] = out
    result['~' + label] = loose


def _wrap_export_rows(k, lib):
    def rows(text):
        out = k.fn('begin_get_phrases', P, P, U)(k.ctx, lib)
        has = k.fn('iterator_has_next_phrase', B, P)
        nxt = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))
        listed = []
        while has(out):
            ph, py, n = P(), P(), I()
            nxt(out, C.byref(ph), C.byref(py), C.byref(n))
            phrase, reading = k.text(ph.value), k.text(py.value)
            if phrase == text:
                listed.append([reading, n.value])
        k.fn('end_get_phrases', None, P)(out)
        return listed
    return rows


def wrap_256th_reading(k, readings, same_iterator):
    readings = _wrap_distinct(k, readings)
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    begin = k.fn('begin_add_phrases', P, P, U)
    end = k.fn('end_add_phrases', None, P)
    out = {}
    for lib, text in _WRAP_PHRASES.items():
        rows = _wrap_export_rows(k, lib) if k.mode == 'pinyin' else None
        label = 'lib %d' % lib
        pending = iter(readings)
        iterator = begin(k.ctx, lib)
        if not iterator:
            out[label] = 'no iterator'
            continue

        def step(frequency):
            reading = next(pending)
            ret = add(iterator, text.encode(), reading, frequency)
            tokens = [t for t in tokens_of(k, text) if t >> 24 == lib]
            count = U(UNTOUCHED)
            if tokens:
                k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, tokens[0], C.byref(count))
            return ret, (count.value if tokens else 0)

        # Fill to 255 readings; a reading that merges into one the item
        # already holds leaves the count where it was.
        ret, count = step(11)
        if not ret:
            out[label] = 'first add refused'
            end(iterator)
            continue
        while count < 255:
            ret, count = step(1)
        _wrap_item_reads(k, lib, text, rows, out, label + ' at 255')
        # The 256th reading: skip any that only merge into an existing one.
        while True:
            before = count
            ret, count = step(1)
            if count != before or not ret:
                break
        out[label + ' 256th'] = [ret, count]
        _wrap_item_reads(k, lib, text, rows, out, label + ' after 256th live')
        if same_iterator:
            out[label + ' 257th add'] = step(9)[0]
            _wrap_item_reads(k, lib, text, rows, out, label + ' after 257th live')
        end(iterator)
        _wrap_item_reads(k, lib, text, rows, out, label + ' after end_add')
        if not same_iterator:
            iterator = begin(k.ctx, lib)
            out[label + ' 257th add'] = step(9)[0]
            _wrap_item_reads(k, lib, text, rows, out, label + ' after 257th live')
            end(iterator)
            _wrap_item_reads(k, lib, text, rows, out, label + ' after 257th end_add')
    out['save'] = k.fn('save', B, P)(k.ctx)
    k.fn('fini', None, P)(k.ctx)
    k._ctx = k._inst = None
    for lib, text in _WRAP_PHRASES.items():
        rows = _wrap_export_rows(k, lib) if k.mode == 'pinyin' else None
        _wrap_item_reads(k, lib, text, rows, out, 'lib %d after reload' % lib)
    return out


@case('wrap-256th-reading')
def _(k):
    return wrap_256th_reading(k, _wrap_pinyin_readings(), same_iterator=False)


@case('wrap-256th-reading-same-iterator')
def _(k):
    return wrap_256th_reading(k, _wrap_pinyin_readings(), same_iterator=True)


@case('wrap-256th-reading-zhuyin', mode='zhuyin')
def _(k):
    return wrap_256th_reading(k, _wrap_zhuyin_readings(), same_iterator=False)


@case('wrap-256th-reading-zhuyin-same-iterator', mode='zhuyin')
def _(k):
    return wrap_256th_reading(k, _wrap_zhuyin_readings(), same_iterator=True)


# Register row 41 (class (b)), the libzhuyin half: the pin dereferences a
# NULL argument and dies of SIGSEGV, the subject's guard answers. One case
# per site so the pin dies at the site under test.
def null_argument_case(call):
    def probe(k):
        return {'ret': call(k)}
    return probe


_ZY_NULL_FALSE = {'ret': False, 'logs': []}
_ZY_NULL_TRUE = {'ret': True, 'logs': []}


def zy_parse_chewing_null_instance(k):
    key = C.c_ushort(0)
    return k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(None, b'su3', C.byref(key))


def zy_parse_chewing_null_text(k):
    key = C.c_ushort(0)
    return k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, None, C.byref(key))


def zy_parse_chewing_null_key(k):
    return k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, b'su3', None)


def zy_in_chewing_null_instance(k):
    symbols = P(UNTOUCHED)
    return k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))(None, b's', C.byref(symbols))


def zy_in_chewing_null_symbols(k):
    return k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))(k.inst, b's', None)


def zy_pinyin_string_null_instance(k):
    key = C.c_ushort(0)
    k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, b'su3', C.byref(key))
    string = P(UNTOUCHED)
    return k.fn('get_pinyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(None, C.byref(key), C.byref(string))


def zy_zhuyin_string_null_key(k):
    string = P(UNTOUCHED)
    return k.fn('get_zhuyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(k.inst, None, C.byref(string))


def zy_zhuyin_string_null_out(k):
    key = C.c_ushort(0)
    k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(k.inst, b'su3', C.byref(key))
    return k.fn('get_zhuyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(k.inst, C.byref(key), None)


def zy_key_rest_positions_null_rest(k):
    begin, end = C.c_ushort(0), C.c_ushort(0)
    return k.fn('get_zhuyin_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))(
        k.inst, None, C.byref(begin), C.byref(end))


def zy_n_phrase_null_instance(k):
    count = U(0)
    return k.fn('get_n_phrase', B, P, C.POINTER(U))(None, C.byref(count))


def zy_n_phrase_null_out(k):
    return k.fn('get_n_phrase', B, P, C.POINTER(U))(k.inst, None)


def zy_phrase_token_null_instance(k):
    slot = U(0)
    return k.fn('get_phrase_token', B, P, U, C.POINTER(U))(None, 0, C.byref(slot))


def zy_phrase_token_null_out(k):
    return k.fn('get_phrase_token', B, P, U, C.POINTER(U))(k.inst, 0, None)


def zy_first_rest(k):
    inst = k.inst
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    rest = P(UNTOUCHED)
    assert k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))(inst, 0, C.byref(rest)) and rest.value
    return inst, rest


def zy_first_candidate(k):
    inst = k.inst
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    cand = P(UNTOUCHED)
    assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand)) and cand.value
    return inst, cand


def zy_key_rest_length_null_rest(k):
    inst, _ = zy_first_rest(k)
    length = C.c_ushort(0)
    return k.fn('get_zhuyin_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(inst, None, C.byref(length))


def zy_key_rest_length_null_length(k):
    inst, rest = zy_first_rest(k)
    return k.fn('get_zhuyin_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(inst, rest, None)


def zy_candidate_type_null_candidate(k):
    inst, _ = zy_first_candidate(k)
    kind = I(0)
    return k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, None, C.byref(kind))


def zy_candidate_type_null_out(k):
    inst, cand = zy_first_candidate(k)
    return k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, None)


def zy_candidate_string_null_candidate(k):
    inst, _ = zy_first_candidate(k)
    text = P(UNTOUCHED)
    return k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, None, C.byref(text))


def zy_candidate_string_null_out(k):
    inst, cand = zy_first_candidate(k)
    return k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, cand, None)


# Register row 43 (class (b)): `zhuyin_begin_add_phrases` stores any `guint8`
# (`zhuyin.cpp:392-398`), so a following `zhuyin_iterator_add_phrase` on
# library 16 reaches `_add_phrase`'s `phrase_index->get_range(index, range)`
# (`zhuyin.cpp:475`), which reads `m_sub_phrase_indices[16]`
# (`storage/phrase_index.cpp:611`) one past the 16-entry array
# (`storage/phrase_index.h:441`) and dies of SIGSEGV. The subject refuses
# every library that is neither a user file nor a loaded system library.
# The pinyin twin's index is not part of the row (the audit recorded library
# 255 accepted there, so that side has no stable crash to hold).
def zy_import_index_out_of_range(k):
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 16)
    if not it:
        return 'no-iterator'
    out = k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '你好'.encode(), 'ㄋㄧˇ ㄏㄠˇ'.encode(), 1)
    k.fn('end_add_phrases', None, P)(it)
    return out


case('zhuyin-add-phrase-library-16', mode='zhuyin', control=True, crash={'ret': False})(
    null_argument_case(zy_import_index_out_of_range))


for _zyname, _zycall, _zyanswer in (
        ('null-instance-parse-chewing', zy_parse_chewing_null_instance, _ZY_NULL_FALSE),
        ('null-text-parse-chewing', zy_parse_chewing_null_text, _ZY_NULL_FALSE),
        ('null-key-parse-chewing', zy_parse_chewing_null_key, _ZY_NULL_FALSE),
        ('null-instance-in-chewing-keyboard', zy_in_chewing_null_instance, _ZY_NULL_FALSE),
        ('null-symbols-in-chewing-keyboard', zy_in_chewing_null_symbols, _ZY_NULL_TRUE),
        ('null-instance-get-pinyin-string', zy_pinyin_string_null_instance, _ZY_NULL_FALSE),
        ('null-key-get-zhuyin-string', zy_zhuyin_string_null_key, _ZY_NULL_FALSE),
        ('null-out-get-zhuyin-string', zy_zhuyin_string_null_out, _ZY_NULL_FALSE),
        ('null-rest-get-zhuyin-key-rest-positions', zy_key_rest_positions_null_rest, _ZY_NULL_FALSE),
        ('null-rest-get-zhuyin-key-rest-length', zy_key_rest_length_null_rest, _ZY_NULL_FALSE),
        ('null-length-get-zhuyin-key-rest-length', zy_key_rest_length_null_length, _ZY_NULL_TRUE),
        ('null-candidate-get-candidate-type', zy_candidate_type_null_candidate, _ZY_NULL_FALSE),
        ('null-out-get-candidate-type', zy_candidate_type_null_out, _ZY_NULL_TRUE),
        ('null-candidate-get-candidate-string', zy_candidate_string_null_candidate, _ZY_NULL_FALSE),
        ('null-out-get-candidate-string', zy_candidate_string_null_out, _ZY_NULL_TRUE),
        ('null-instance-get-n-phrase', zy_n_phrase_null_instance, _ZY_NULL_FALSE),
        ('null-out-get-n-phrase', zy_n_phrase_null_out, _ZY_NULL_TRUE),
        ('null-instance-get-phrase-token', zy_phrase_token_null_instance, _ZY_NULL_FALSE),
        ('null-out-get-phrase-token', zy_phrase_token_null_out, _ZY_NULL_FALSE)):
    case('zhuyin-' + _zyname, mode='zhuyin', control=True, crash=_zyanswer)(
        null_argument_case(_zycall))


def declared_sentence_residuals():
    """Read membership from the frozen gate's §12 declaration, not a copy."""
    root = Path(__file__).resolve().parents[2]
    doc = (root / 'docs/findings/sentence-surface.md').read_text()
    section = doc.split('\n## 12. ', 1)[1].split('\n## 13.', 1)[0]
    declaration = section.split(
        'Only the following six shared-comparison rows are class (a), as ruled.', 1)[1]
    table = declaration.split('| Input | Pin deciding f32 bits | Subject native costs |\n', 1)[1]
    names = set()
    for line in table.splitlines()[1:]:
        if not line.startswith('|'):
            break
        name = line.split('|')[1].strip().strip('`')
        assert name not in names, 'duplicate declared residual'
        names.add(name)
    assert len(names) == 6, 'frozen declared residual count moved'
    return names


def encode_sentence_tie(row, guessed, sentences, nbest, residual_sentences, residual_nbest):
    """Pool each text once per input; all displayed rows have NBEST type 1."""
    texts = list(dict.fromkeys(sentences + residual_sentences))
    ids = {text: index for index, text in enumerate(texts)}
    def indexed(items):
        return ','.join(str(ids[text]) for text in items)
    def displayed(items):
        return ','.join(f'{index}:{ids[text]}' for index, text in items)
    return '\t'.join(row[:3] + [str(int(guessed)), '\x01'.join(texts),
                                indexed(sentences), displayed(nbest),
                                indexed(residual_sentences), displayed(residual_nbest)])


def regenerate_sentence_ties(prefix, fixture):
    """Refresh pin expectations; preserve the six approved exact subject rows.

    The first three columns are historical native-tail selection metadata.
    Text pooling keeps indexed duplicates and complete NBEST rows compact.
    Membership comes only from the frozen §12 table; ordinary cases cannot
    carry subject expectations. No observations or other outputs are saved.
    """
    suspend_offset = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC)

    def fields(path):
        return dict(line.split('=', 1) for line in path.read_text().splitlines()
                    if line and not line.startswith('#') and '=' in line)

    manifest = fields(prefix / 'oracle-pin.txt')
    source_pin = fields(Path(__file__).resolve().parents[2] / 'tools/oracle/oracle-pin.txt')
    expected = (f"libpinyin-{source_pin['libpinyin_tag']}-{source_pin['libpinyin_commit']}"
                f"+model20-{source_pin['model_archive_sha256']}+dbm-bdb")
    assert manifest['pin_ref'] == expected and manifest['dbm'] == 'BerkeleyDB', 'off-pin bdb prefix'
    so = prefix / 'lib/libpinyin.so'
    assert hashlib.sha256(so.read_bytes()).hexdigest() == manifest['shared_object_sha256'], \
        'pin library checksum differs'
    verified_files = 0
    for name, field in (('oracle-data.sha256', 'data_manifest_sha256'),
                        ('oracle-data-unstable.sha256', 'data_unstable_manifest_sha256')):
        raw = (prefix / name).read_bytes()
        assert hashlib.sha256(raw).hexdigest() == manifest[field], 'pin data manifest checksum differs'
        for line in raw.decode().splitlines():
            digest, relative = line.split('  ', 1)
            assert hashlib.sha256((prefix / relative).read_bytes()).hexdigest() == digest, \
                'pin data checksum differs: ' + relative
            verified_files += 1
    print(f'pin verified: library and {verified_files} data files', flush=True)

    raw = fixture.read_text()
    headers = [line for line in raw.splitlines() if line.startswith('#')]
    assert '# format=sentence-tie-order-v1' in headers, 'unsupported tie fixture format'
    cases = [line.split('\t') for line in raw.splitlines() if not line.startswith('#')]
    assert cases and all(len(row) == 9 and int(row[1]) > 0 for row in cases), 'bad tie fixture'
    assert len({row[0] for row in cases}) == len(cases), 'duplicate selected input'
    declared = declared_sentence_residuals()
    recorded = {row[0] for row in cases if row[7] or row[8]}
    assert recorded == declared, 'subject expectations must match the frozen declared membership exactly'
    out = []
    row_count = 0
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    with tempfile.TemporaryDirectory(prefix='sentence-ties-') as scratch:
        k = Kit('pinyin', str(so.resolve()), str(prefix / 'lib/libpinyin/data'), scratch)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a)
        try:
            for ordinal, row in enumerate(cases, 1):
                text = row[0]
                pool = row[4].split('\x01')
                residual_sentences = [pool[int(index)] for index in row[7].split(',')] if row[7] else []
                residual_nbest = [(int(index), pool[int(text_id)])
                                  for index, text_id in (item.split(':') for item in row[8].split(','))] if row[8] else []
                assert k.fn('reset', B, P)(k.inst)
                k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, text.encode())
                guessed = k.fn('guess_sentence', B, P)(k.inst)
                assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0x1e)
                nbest = []
                for index, candidate in enumerate(candidate_rows(k, k.inst)):
                    assert candidate is not None, 'unreadable candidate for ' + repr(text)
                    if candidate[0] == 1:
                        pointer, rank = P(), C.c_ubyte(0xff)
                        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(
                            k.inst, index, C.byref(pointer))
                        assert k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
                            k.inst, pointer, C.byref(rank))
                        nbest.append((rank.value, candidate[1]))
                sentences = final_step_sentences(k, k.inst)
                out.append(encode_sentence_tie(row, guessed, sentences, nbest,
                                              residual_sentences, residual_nbest))
                row_count += len(nbest)
                if ordinal % 1000 == 0:
                    print(f'pin expectations: {ordinal}/{len(cases)}', flush=True)
        finally:
            if k._inst:
                k.fn('free_instance', None, P)(k._inst)
            if k._ctx:
                k.fn('fini', None, P)(k._ctx)
    headers = [line for line in headers if not line.startswith('# pin_ref=')]
    headers.insert(1, '# pin_ref=' + manifest['pin_ref'])
    elapsed_suspend = (time.clock_gettime(time.CLOCK_BOOTTIME) -
                       time.clock_gettime(time.CLOCK_MONOTONIC) - suspend_offset)
    assert abs(elapsed_suspend) < 0.01, 'discard: suspend overlapped pin regeneration'
    fixture.write_text('\n'.join(headers + out) + '\n')
    print(f'regenerated {fixture}: {len(cases)} distinct inputs, '
          f'{sum(int(row[1]) for row in cases)} corpus entries, {row_count} ordered NBEST rows; '
          f'pin-equal expectations {len(cases) - len(declared)}, exact residual expectations {len(declared)}', flush=True)
    print(f'suspend overlap: none (clock delta {elapsed_suspend:.6f} seconds)', flush=True)


def axis_c_definition(*, verify_sources=False):
    """The committed replacement definition, never inferred from results.

    Loading happens at import for every run (ordinary case selection builds
    the Axis C groups too). Source-fixture hashing is deferred to
    `verify_sources=True`, asked for once in the Axis C branch, so an
    ordinary run is not failed by a changed fixture and an Axis C worker,
    which imports this module after the parent validated, does not repeat it.
    """
    root = Path(__file__).resolve().parents[2]
    definition = json.loads((root / 'tools/bisection/axis-c-replacement.json').read_text())
    assert definition['format'] == 'axis-c-replacement-v1'
    if verify_sources:
        for name, digest in definition['sources'].items():
            assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
    return definition


def axis_c_isolated(k, fn):
    """Fork one read-only observation; both pipes stay in memory.

    The context is opened before forking. Children allocate fresh instances,
    never save/train, and exit without teardown, so one abort cannot truncate
    the remaining corpus. Import cases instead use the existing fresh worker.
    """
    k.ctx
    output_read, output_write = os.pipe()
    error_read, error_write = os.pipe()
    pid = os.fork()
    if pid == 0:
        os.close(output_read)
        os.close(error_read)
        os.dup2(error_write, 2)
        os.close(error_write)
        k.logs.clear()
        k.log_messages.clear()
        try:
            result = fn()
            result['logs'] = k.logs
            result['log_messages'] = k.log_messages
            payload = json.dumps(result, sort_keys=True, ensure_ascii=True).encode()
            with os.fdopen(output_write, 'wb') as destination:
                destination.write(payload)
            os._exit(0)
        except BaseException as error:
            os.write(2, repr(error).encode())
            os._exit(1)
    os.close(output_write)
    os.close(error_write)
    # Each child emits at most short diagnostics. Drain both concurrently,
    # including a large complete candidate vector, without capture files.
    import selectors
    selector = selectors.DefaultSelector()
    selector.register(output_read, selectors.EVENT_READ, 'output')
    selector.register(error_read, selectors.EVENT_READ, 'errors')
    chunks = {'output': [], 'errors': []}
    while selector.get_map():
        for key, _ in selector.select():
            chunk = os.read(key.fd, 65536)
            if chunk:
                chunks[key.data].append(chunk)
            else:
                selector.unregister(key.fd)
                os.close(key.fd)
    selector.close()
    _, status = os.waitpid(pid, 0)
    payload = b''.join(chunks['output'])
    return dict(exit=os.waitstatus_to_exitcode(status),
                result=json.loads(payload) if payload else None,
                stderr=b''.join(chunks['errors']).decode('utf-8', 'surrogateescape'))


def axis_c_surface(k, parser, text, instance=None):
    """Parse -> lengths -> sentence rank zero -> complete candidate list."""
    inst = k.alloc() if instance is None else instance
    parsed = k.fn(parser, Z, P, S)(inst, text)
    out = dict(consumed=parsed, parsed=k.fn('get_parsed_input_length', Z, P)(inst))
    guessed = k.fn('guess_sentence', B, P)(inst)
    out['guess_sentence'] = guessed
    if guessed:
        out['sentence'] = (sentence_out(k, inst, 0) if k.mode == 'pinyin'
                           else zhuyin_sentence_out(k, inst))
    out['guess_candidates'] = (k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
                               if k.mode == 'pinyin' else
                               k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0))
    count = U(UNTOUCHED)
    out['get_n_candidate'] = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    out['n'] = count.value
    assert count.value != UNTOUCHED, 'candidate count untouched'
    out['candidates'] = candidate_rows(k, inst)
    out['candidate_multiset'] = sorted(out['candidates'], key=lambda row: json.dumps(row))
    return out


def axis_c_options(word, texts):
    def probe(k):
        set_ok = k.fn('set_options', B, P, U)(k.ctx, word)
        # Every fork gets an independent, never-parsed instance snapshot.
        # The parent never parses it; constructor work is independent of input.
        # Retain the original allocation path for a bytewise equivalence check.
        snapshot = None if os.environ.get('AXIS_C_ALLOCATE_EACH') == '1' else k.alloc()
        return dict(set_options=set_ok, observations={
            str(index): axis_c_isolated(k, lambda text=text: axis_c_surface(
                k, 'parse_more_full_pinyins', text.encode(), snapshot))
            for index, text in enumerate(texts)})
    return probe


def axis_c_lookup(k, text):
    new = k.glib.g_array_new
    new.argtypes, new.restype = [I, I, U], P
    arr = new(0, 0, 4)
    try:
        ret = k.fn('lookup_tokens', B, P, S, P)(k.inst, text, arr)
        view = C.cast(arr, C.POINTER(Arr)).contents
        return dict(ret=ret, tokens=list(C.cast(view.data, C.POINTER(U))[:view.len]))
    finally:
        k.glib.g_array_free.argtypes, k.glib.g_array_free.restype = [P, B], P
        k.glib.g_array_free(arr, True)


def axis_c_import(k, operation, text):
    phrase = text if operation == 'import-phrase' else '你好'.encode()
    reading = text if operation == 'import-reading' else (
        b"ni'hao" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode())
    it = k.fn('begin_add_phrases', P, P, C.c_ubyte)(k.ctx, 7)
    assert it, 'missing import iterator'
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase, reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    out = dict(added=added, lookup=axis_c_lookup(k, phrase),
               saved=k.fn('save', B, P)(k.ctx))
    k.fn('fini', None, P)(k.ctx)
    k._ctx, k._inst = None, None
    # libzhuyin has no export iterator. Reopen either facade's saved profile
    # through the same side's libpinyin export API in a fresh worker process.
    env = dict(os.environ, AXIS_C_REOPEN_USER=k.user)
    side = os.environ['AXIS_C_COMPANION_PINYIN']
    proc = subprocess.run([sys.executable, __file__, '--worker', 'pinyin', side,
                           k.data, 'axis-c-export-user'], capture_output=True, env=env)
    out['reopen_exit'] = proc.returncode
    out['reopen_stderr'] = proc.stderr.decode('utf-8', 'surrogateescape').replace(k.user, 'USER')
    out['export'] = json.loads(proc.stdout) if proc.stdout else None
    return out


@case('axis-c-export-user')
def axis_c_export_user(k):
    k.user = os.environ['AXIS_C_REOPEN_USER']
    return dict(rows=d18_export_phrases(k))


def axis_c_encoding(operation, text):
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, 0x28)
        if operation.startswith('parse_more_'):
            return axis_c_surface(k, operation, text)
        if operation == 'lookup_tokens':
            return axis_c_lookup(k, text)
        if operation == 'phrase_segment':
            ret = k.fn('phrase_segment', B, P, S)(k.inst, text)
            count = U(UNTOUCHED)
            got = k.fn('get_n_phrase', B, P, C.POINTER(U))(k.inst, C.byref(count))
            out = dict(ret=ret, get_n_phrase=got, count=count.value)
            # #751 reuses the six preregistered surrogate cases. The pin
            # returns false with six null slots and no GLib record (the
            # worker already compares domain, level and exact message).
            # Only read out the result; keep the battery's input, options
            # and state-changing operation exactly as preregistered.
            if b'\xed\xa0\x80' in text:
                token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))
                def read(index):
                    slot = U(UNTOUCHED)
                    return [token(k.inst, index, C.byref(slot)), slot.value]
                out['tokens'] = ([read(index) for index in range(count.value)]
                                 if got and count.value <= len(text) else None)
                out['past'] = [read(count.value), read(0xFFFFFFFF)]
            return out
        if operation == 'remember_user_input':
            parsed = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'nihao')
            return dict(parsed=parsed, ret=k.fn('remember_user_input', B, P, S, I)(k.inst, text, 1),
                        rows=d18_export_phrases(k))
        return axis_c_import(k, operation, text)
    return probe


def axis_c_keyboard(k, byte):
    out = P(UNTOUCHED)
    ret = k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))(
        k.inst, bytes([byte]), C.byref(out))
    if out.value == UNTOUCHED:
        return dict(ret=ret, symbols='untouched')
    if not out.value:
        return dict(ret=ret, symbols=None)
    strings = C.cast(out, C.POINTER(S))
    symbols, index = [], 0
    while strings[index] is not None:
        symbols.append(strings[index].decode())
        index += 1
    k.glib.g_strfreev.argtypes, k.glib.g_strfreev.restype = [P], None
    k.glib.g_strfreev(out)
    return dict(ret=ret, symbols=symbols)


def axis_c_layout(scheme, word, definition):
    def probe(k):
        k.fn('set_options', B, P, U)(k.ctx, word)
        def configured(fn):
            setter = 'set_zhuyin_scheme' if k.mode == 'pinyin' else 'set_chewing_scheme'
            ret = k.fn(setter, B, P, I)(k.ctx, scheme)
            return dict(set_scheme=ret, observation=fn() if ret else None)
        rows = {f'ascii-{byte}': axis_c_isolated(k, lambda byte=byte: configured(
            lambda: axis_c_keyboard(k, byte))) for byte in definition['layout_ascii']}
        for index, text in enumerate(definition['layout_inputs']):
            rows[f'input-{index}'] = axis_c_isolated(k, lambda text=text: configured(
                lambda: axis_c_surface(k, 'parse_more_chewings', text.encode())))
        return dict(observations=rows)
    return probe


AXIS_C_GROUPS = {name: [] for name in ('options', 'encoding', 'layouts')}
AXIS_C_DEFINITION = axis_c_definition()
for _kind, _words in AXIS_C_DEFINITION['words'].items():
    for _word in _words:
        _name = f'axis-c-options-{_kind}-{int(_word, 16):08x}'
        case(_name, stderr=True)(axis_c_options(int(_word, 16), AXIS_C_DEFINITION['option_inputs']))
        AXIS_C_GROUPS['options'].append(_name)
for _mode, _operation in AXIS_C_DEFINITION['encoding_operations']:
    for _input in AXIS_C_DEFINITION['encoding_inputs']:
        _name = f"axis-c-encoding-{_mode}-{_operation}-{_input['name']}"
        case(_name, mode=_mode, stderr=True)(axis_c_encoding(_operation, bytes.fromhex(_input['hex'])))
        AXIS_C_GROUPS['encoding'].append(_name)
# The invalid scheme numbers whose abort the pin takes and the register already
# classifies (rows 5c/5d). These layout cases are matched with the classic
# class (c) `abort=` expectation at the nested-observation level and counted as
# covered, not DIFFER, exactly as `abort-zhuyin-chewing-scheme-*` is.
AXIS_C_LAYOUT_COVERED = {}
_covered_rows = {scheme: entry['row'] for entry in AXIS_C_DEFINITION['layout_covered']
                 for scheme in entry['schemes']}
for _mode in ('pinyin', 'zhuyin'):
    for _scheme in AXIS_C_DEFINITION['layout_schemes']:
        for _word in AXIS_C_DEFINITION['layout_words']:
            _name = f'axis-c-layout-{_mode}-{_scheme}-{int(_word, 16):08x}'
            case(_name, mode=_mode, stderr=True)(axis_c_layout(_scheme, int(_word, 16), AXIS_C_DEFINITION))
            AXIS_C_GROUPS['layouts'].append(_name)
            if _scheme in _covered_rows:
                AXIS_C_LAYOUT_COVERED[_name] = _covered_rows[_scheme]


def axis_c_canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(',', ':')).encode()


def axis_c_normalize_diagnostics(value, scratch, field=''):
    """Only generated paths in diagnostics, never candidate/input payloads."""
    if isinstance(value, dict):
        return {key: axis_c_normalize_diagnostics(item, scratch, key) for key, item in value.items()}
    if isinstance(value, list):
        return [axis_c_normalize_diagnostics(item, scratch, field) for item in value]
    if isinstance(value, str) and field in ('stderr', 'reopen_stderr', 'log_messages'):
        return re.sub(r'(user|sys)-(\udcff)?[A-Za-z0-9_]+', r'\1-\2X', value.replace(scratch, 'SCRATCH'))
    return value


def check_axis_c_snapshots(args):
    names = ['axis-c-options-single-00000008', 'axis-c-options-pair-00000060',
             'axis-c-options-boundary-ffffffff']
    offset = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC)
    # The private switch belongs to the allocation half only; clear any caller
    # value while validating the fork-snapshot half and restore it on the way
    # out so an invoking shell is left as found.
    caller_allocate_each = os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
    try:
        with tempfile.TemporaryDirectory(prefix='contract-diff-') as scratch:
            for side in (args.prefix / 'lib/libpinyin.so', args.pinyin_so):
                for name in names:
                    os.environ['AXIS_C_ALLOCATE_EACH'] = '1'
                    original = run_worker('pinyin', side.resolve(), args.prefix / 'lib/libpinyin/data', name, scratch)
                    os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
                    snapshot = run_worker('pinyin', side.resolve(), args.prefix / 'lib/libpinyin/data', name, scratch)
                    # Ignore only incidental directory inventory, as in the battery.
                    assert all(original[key] == snapshot[key] for key in ('exit', 'result', 'stderr')), name
                    print(f'instance snapshot: {side.name} {name} identical', flush=True)
    finally:
        if caller_allocate_each is None:
            os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
        else:
            os.environ['AXIS_C_ALLOCATE_EACH'] = caller_allocate_each
    delta = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC) - offset
    assert abs(delta) < 0.01, 'discard suspend-overlapping snapshot validation'
    print(f'instance snapshot: 6 word/library comparisons, 96 input observations; identical; suspend delta {delta:.6f}', flush=True)
    return 0


def axis_c_covered_abort(pin_row, subject_row, mode):
    """Register-covered class (c) site at the nested-observation level.

    Mirrors the classic ``abort=`` expectation: the pin child dies of SIGABRT
    before returning, and the subject returns normally with the scheme setter
    refused (``set_scheme`` false, no observation) and exactly one warning in
    the facade's GLib domain at level 16. Rows 5c/5d of the register.
    """
    if not isinstance(pin_row, dict) or not isinstance(subject_row, dict):
        return False
    result = subject_row.get('result')
    return (pin_row.get('exit') == -signal.SIGABRT and pin_row.get('result') is None
            and set(pin_row) == set(AXIS_C_ABORT_ROW_FIELDS)
            and subject_row.get('exit') == 0 and isinstance(result, dict)
            and set(result) == set(AXIS_C_REFUSAL_RESULT_FIELDS)
            and result.get('set_scheme') is False and result.get('observation') is None
            and result.get('logs') == [[WARNING_DOMAIN[mode], 16]])


# The exact registered shape of a scheme-setter observation. The abort row
# carries only the worker envelope; the refusal row's result carries exactly
# these four keys, so an extra unregistered field is never licensed.
AXIS_C_ABORT_ROW_FIELDS = ('exit', 'result', 'stderr')
AXIS_C_REFUSAL_RESULT_FIELDS = ('set_scheme', 'observation', 'logs', 'log_messages')


def axis_c_live(row):
    """A worker row that exited cleanly and returned a result.

    A row that died, was signalled, or printed no result observed nothing, so
    two identical failures must never read as an exact match. This mirrors the
    normal runner's rejection (`run_worker` verdicts); a pin abort is an
    explicit class (c) expectation and is matched by `axis_c_covered_abort`,
    never by equality.
    """
    return isinstance(row, dict) and row.get('exit') == 0 and row.get('result') is not None


def axis_c_licensed_remnant(row):
    """A covered row with only its licensed abort/refusal fields removed.

    Anything the row carries beyond those three fields stays and is compared,
    so an unregistered extra field still fails the case.
    """
    return {key: value for key, value in row.items() if key not in AXIS_C_ABORT_ROW_FIELDS}


def axis_c_covered_residual(pin, subject, covered_keys):
    """Both envelopes with only the licensed fields of the covered rows removed.

    Root exit/result/stderr, root `log_messages` and the parent-side call
    results, plus every non-covered observation in full, are retained and
    compared; a covered verdict therefore never hides an unrelated difference.
    """
    pin_residual = copy.deepcopy(pin)
    subject_residual = copy.deepcopy(subject)
    pin_rows = (pin_residual.get('result') or {}).get('observations')
    subject_rows = (subject_residual.get('result') or {}).get('observations')
    for key in covered_keys:
        pin_rows[key] = axis_c_licensed_remnant(pin_rows[key])
        subject_rows[key] = axis_c_licensed_remnant(subject_rows[key])
    return pin_residual, subject_residual


def axis_c_first_difference(left, right, path=''):
    if type(left) is type(right) and isinstance(left, dict):
        for key in sorted(set(left) | set(right)):
            if left.get(key) != right.get(key):
                return axis_c_first_difference(left.get(key), right.get(key), path + '.' + key)
    if type(left) is type(right) and isinstance(left, list):
        for index, (a, b) in enumerate(zip(left, right)):
            if a != b:
                return axis_c_first_difference(a, b, path + f'[{index}]')
    return dict(field=path, pin=left if len(repr(left)) < 300 else repr(left)[:300],
                subject=right if len(repr(right)) < 300 else repr(right)[:300])


def run_axis_c(args, names):
    """Repeat and compare existing worker observations, with no saved output."""
    from collections import Counter
    from concurrent.futures import ThreadPoolExecutor
    offset = time.clock_gettime(time.CLOCK_BOOTTIME) - time.clock_gettime(time.CLOCK_MONOTONIC)
    totals, fingerprints = Counter(), {side: hashlib.sha256() for side in ('pin', 'subject')}
    differences, examples = Counter(), {}
    case_verdicts = {}
    def record_difference(name, key, left, right):
        first = axis_c_first_difference(left, right)
        pin_exit = left.get('exit') if isinstance(left, dict) else None
        subject_exit = right.get('exit') if isinstance(right, dict) else None
        if pin_exit:
            diagnostic = left.get('stderr', '')
            result = (right.get('result') or {}) if isinstance(right, dict) else {}
            signature = json.dumps(dict(pin_exit=pin_exit, subject_exit=subject_exit,
                                        pin_diagnostic=diagnostic,
                                        subject_logs=result.get('log_messages', result.get('logs'))),
                                   sort_keys=True, ensure_ascii=True)
        else:
            signature = json.dumps(first, sort_keys=True, ensure_ascii=True)
        differences[signature] += 1
        examples.setdefault(signature, dict(case=name, row=key, first=first))
    def observe(name, subject, companion):
        spec = CASES[name]
        # Each simultaneous observation owns its scratch/profile inventory.
        # The existing cleanup helper must never see another worker's files.
        with tempfile.TemporaryDirectory(prefix='worker-', dir=scratch) as isolated:
            value = run_worker(spec['mode'], subject, args.prefix / 'lib/libpinyin/data', name, isolated,
                               {'AXIS_C_COMPANION_PINYIN': str(companion)})
            return axis_c_normalize_diagnostics(
                {key: value[key] for key in ('exit', 'result', 'stderr')}, isolated)
    with tempfile.TemporaryDirectory(prefix='contract-diff-') as scratch, ThreadPoolExecutor(max_workers=2) as pool:
        for ordinal, name in enumerate(names, 1):
            mode = CASES[name]['mode']
            if args.axis_c_mutation:
                pin_so = args.pinyin_so if mode == 'pinyin' else args.zhuyin_so
                subject_so = args.axis_c_mutant_pinyin_so if mode == 'pinyin' else args.axis_c_mutant_zhuyin_so
                pin_companion, subject_companion = args.pinyin_so, args.axis_c_mutant_pinyin_so
            else:
                pin_so = args.prefix / ('lib/libpinyin.so' if mode == 'pinyin' else 'lib/libzhuyin.so')
                subject_so = args.pinyin_so if mode == 'pinyin' else args.zhuyin_so
                pin_companion, subject_companion = args.prefix / 'lib/libpinyin.so', args.pinyin_so
            assert pin_so and subject_so and pin_so.is_file() and subject_so.is_file(), name
            futures = [pool.submit(observe, name, so.resolve(), companion.resolve())
                       for so, companion in ((pin_so, pin_companion), (subject_so, subject_companion),
                                             (pin_so, pin_companion), (subject_so, subject_companion))]
            pin, subject, pin_repeat, subject_repeat = [future.result() for future in futures]
            stable = pin == pin_repeat and subject == subject_repeat
            assert stable, 'NOT-ESTABLISHED: unstable case ' + name
            for side, observation in (('pin', pin), ('subject', subject)):
                fingerprints[side].update(axis_c_canonical([name, observation]))
            totals['cases'] += 1
            totals['diagnostics_match' if pin['stderr'] == subject['stderr'] else 'diagnostics_differ'] += 1
            covered_row = AXIS_C_LAYOUT_COVERED.get(name)
            covered_rows, differ_rows = [], 0
            residual_first = None
            pin_rows = (pin['result'] or {}).get('observations')
            subject_rows = (subject['result'] or {}).get('observations')
            if pin_rows is not None and subject_rows is not None:
                for key in pin_rows:
                    left, right = pin_rows[key], subject_rows.get(key)
                    totals['observations'] += 1
                    if axis_c_live(left) and axis_c_live(right) and left == right:
                        totals['payload_match'] += 1
                    elif covered_row and axis_c_covered_abort(left, right, mode):
                        totals['payload_covered'] += 1
                        covered_rows.append(key)
                    else:
                        totals['payload_differ'] += 1
                        differ_rows += 1
                        record_difference(name, key, left, right)
                # First differing row only; all rows were compared in full.
                if differ_rows == 0 and axis_c_live(pin) and axis_c_live(subject) and pin == subject:
                    totals['case_match'] += 1
                    verdict = 'MATCH'
                elif (covered_row and differ_rows == 0 and covered_rows
                      and axis_c_live(pin) and axis_c_live(subject)):
                    # Don't just trust the covered rows: remove only their
                    # licensed fields and require the rest of the envelope to
                    # agree, so a new parent-side warning, root log message or
                    # outer stderr still makes the case DIFFER.
                    residual_pin, residual_subject = axis_c_covered_residual(pin, subject, covered_rows)
                    if residual_pin == residual_subject:
                        totals['covered'] += 1
                        covered_by_row = totals.setdefault('covered_by_row', {})
                        covered_by_row[covered_row] = covered_by_row.get(covered_row, 0) + 1
                        verdict = 'COVERED'
                    else:
                        totals['case_differ'] += 1
                        record_difference(name, 'covered-residual', residual_pin, residual_subject)
                        residual_first = axis_c_first_difference(residual_pin, residual_subject)
                        verdict = 'DIFFER'
                else:
                    totals['case_differ'] += 1
                    if differ_rows == 0:
                        record_difference(name, 'case', pin, subject)
                    verdict = 'DIFFER'
            else:
                totals['observations'] += 1
                if axis_c_live(pin) and axis_c_live(subject) and pin == subject:
                    totals['payload_match'] += 1
                    totals['case_match'] += 1
                    verdict = 'MATCH'
                else:
                    totals['payload_differ'] += 1
                    totals['case_differ'] += 1
                    record_difference(name, 'case', pin, subject)
                    verdict = 'DIFFER'
            case_verdicts[name] = verdict
            if verdict == 'DIFFER':
                print(json.dumps(dict(case=name, verdict='DIFFER',
                                      first=residual_first or axis_c_first_difference(pin, subject)),
                                 ensure_ascii=True), flush=True)
            elif verdict == 'COVERED':
                print(json.dumps(dict(case=name, verdict='COVERED', row=covered_row,
                                      covered_observations=len(covered_rows)), ensure_ascii=True), flush=True)
            if ordinal % 25 == 0:
                print(f'axis C progress: {ordinal}/{len(names)} cases', flush=True)
    elapsed_suspend = (time.clock_gettime(time.CLOCK_BOOTTIME) -
                       time.clock_gettime(time.CLOCK_MONOTONIC) - offset)
    assert abs(elapsed_suspend) < 0.01, 'NOT-ESTABLISHED: discard suspend-overlapping run'
    for signature in sorted(differences):
        print(json.dumps(dict(difference_signature=json.loads(signature), count=differences[signature],
                              example=examples[signature]), ensure_ascii=True), flush=True)
    print(json.dumps(dict(battery=args.axis_c_replacement, mutation=args.axis_c_mutation,
                          counts=dict(totals), repeats='identical', suspend_delta=elapsed_suspend,
                          hashes={side: h.hexdigest() for side, h in fingerprints.items()}), sort_keys=True), flush=True)
    if args.axis_c_mutation:
        controls = AXIS_C_DEFINITION['mutations'][args.axis_c_mutation]['controls']
        for control in controls:
            print(json.dumps(dict(mutation=args.axis_c_mutation, control=control,
                                  verdict=case_verdicts.get(control)), ensure_ascii=True), flush=True)
        undetected = [control for control in controls if case_verdicts.get(control) != 'DIFFER']
        assert not undetected, ('NOT-ESTABLISHED: %s control(s) not detected: %s'
                                % (args.axis_c_mutation, ', '.join(undetected)))
        return 0
    return 1 if totals['case_differ'] else 0

def matches(spec, pin, subject):
    """The verdict predicate shared by the driver and gcov measurement tier."""
    as_abort = spec['abort'] is not NO_ABORT and (not spec['either'] or pin['exit'] == -6)
    if spec['crash'] is not None and not as_abort:
        shown = subject['result'] or {}
        same = pin['exit'] == -signal.SIGSEGV and subject['exit'] == 0 and \
            (spec['stderr_varies'] or pin['stderr'] == subject['stderr']) and \
            all(shown.get(key) == value for key, value in spec['crash'].items())
    elif as_abort:
        shown = subject['result'] or {}
        same = pin['exit'] == -6 and subject['exit'] == 0 and \
            shown.get('ret') == spec['abort'] and shown.get('logs') == [[WARNING_DOMAIN[spec['mode']], 16]] and \
            all(v for key, v in shown.items() if key.startswith('untouched')) and \
            (not spec['userdir'] or pin['userfiles'] == subject['userfiles'])
    else:
        # A side that died or printed nothing observed nothing: two
        # identical failures must not read as a match (nor satisfy
        # --expect-parent as a difference).
        same = all(r['exit'] == 0 and r['result'] is not None for r in (pin, subject)) and \
            observed(pin['result']) == observed(subject['result'])
        if spec['stderr']:
            same = same and pin['stderr'] == subject['stderr']
    return same


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('cell', choices=['bdb', 'kc', 'tkrzw'])
    parser.add_argument('prefix', type=Path)
    parser.add_argument('pinyin_so', type=Path)
    parser.add_argument('--zhuyin-so', type=Path)
    parser.add_argument('--cases', default='')
    parser.add_argument('--expect-parent', action='store_true')
    parser.add_argument('--regenerate-sentence-ties', type=Path,
                        help='regenerate pin expectations in the supplementary #747 source fixture')
    parser.add_argument('--observations', type=Path, default=os.environ.get('CONTRACT_DIFF_OBSERVATIONS'),
                        help='retain both complete observations as JSONL (also CONTRACT_DIFF_OBSERVATIONS)')
    axis_c_mode = parser.add_mutually_exclusive_group()
    axis_c_mode.add_argument('--axis-c-replacement', choices=['options', 'encoding', 'layouts', 'all'])
    axis_c_mode.add_argument('--axis-c-mutation', choices=['C-m1-r', 'C-m3-r'])
    axis_c_mode.add_argument('--axis-c-snapshot-check', action='store_true')
    parser.add_argument('--axis-c-mutant-pinyin-so', type=Path)
    parser.add_argument('--axis-c-mutant-zhuyin-so', type=Path)
    args = parser.parse_args()
    if (args.axis_c_mutant_pinyin_so or args.axis_c_mutant_zhuyin_so) and not args.axis_c_mutation:
        parser.error('--axis-c-mutant-pinyin-so/--axis-c-mutant-zhuyin-so require --axis-c-mutation')
    os.environ['CONTRACT_DIFF_CELL'] = args.cell
    lib = args.prefix / 'lib'
    data = lib / 'libpinyin/data'
    inputs = [args.prefix / 'oracle-pin.txt', lib / 'libpinyin.so', args.pinyin_so, data / 'bigram.db']
    for path in inputs:
        assert path.is_file(), 'missing input: %s' % path
    manifest = (args.prefix / 'oracle-pin.txt').read_text()
    assert 'libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' in manifest
    assert '+dbm-' + args.cell + '\n' in manifest
    if args.axis_c_replacement or args.axis_c_mutation or args.axis_c_snapshot_check:
        assert not (args.observations or args.cases or args.expect_parent or args.regenerate_sentence_ties), \
            'axis C replacement cannot be combined with capture, regeneration or parent modes'
        # Hash the committed source fixtures once, here, before any Axis C
        # worker is launched; the loader no longer does it at import.
        axis_c_definition(verify_sources=True)
        if args.axis_c_snapshot_check:
            return check_axis_c_snapshots(args)
        # The fork-snapshot protocol is preregistered; a caller-provided
        # AXIS_C_ALLOCATE_EACH must never silently switch the battery over to
        # per-child allocation (that is the snapshot check's private half).
        os.environ.pop('AXIS_C_ALLOCATE_EACH', None)
        names = (AXIS_C_DEFINITION['mutations'][args.axis_c_mutation]['controls']
                 if args.axis_c_mutation else
                 sum(AXIS_C_GROUPS.values(), []) if args.axis_c_replacement == 'all' else
                 AXIS_C_GROUPS[args.axis_c_replacement])
        return run_axis_c(args, names)
    if args.regenerate_sentence_ties:
        assert args.cell == 'bdb', '#747 fixture regeneration is bdb only'
        assert args.pinyin_so.resolve() == (lib / 'libpinyin.so').resolve(), \
            'fixture expectations must come from the pin, never the subject'
        assert not (args.cases or args.expect_parent or args.zhuyin_so or args.observations), \
            'fixture regeneration cannot be combined with differential or capture modes'
        regenerate_sentence_ties(args.prefix, args.regenerate_sentence_ties)
        return 0
    # The whole set only when --cases is omitted: an explicit list that
    # names nothing (`--cases ,`) is an error, not a request for everything.
    names = [n for n in args.cases.split(',') if n] if args.cases else sorted(n for n in CASES if not n.startswith('axis-c-'))
    assert names, 'no cases selected'
    for n in names:
        assert n in CASES, 'unknown case: %s' % n
    failures = 0
    if args.observations:
        args.observations.write_text('')
    with tempfile.TemporaryDirectory(prefix='contract-diff-') as scratch:
        for n in names:
            spec = CASES[n]
            if args.cell not in spec['cells']:
                # The case crafts a fixture in another cell's storage format
                # (e.g. a Berkeley DB btree); neither side can reach the site
                # here, so there is nothing to compare.
                print(json.dumps(dict(cell=args.cell, case=n, verdict='SKIP',
                                      expected_ok=True, exit=[0, 0], stderr_lines=[0, 0]),
                                 sort_keys=True), flush=True)
                continue
            if spec['mode'] == 'zhuyin':
                assert args.zhuyin_so and args.zhuyin_so.is_file() and (lib / 'libzhuyin.so').is_file(), \
                    'zhuyin case %s needs --zhuyin-so and the pin libzhuyin' % n
                pin_so, subject_so = lib / 'libzhuyin.so', args.zhuyin_so
            else:
                pin_so, subject_so = lib / 'libpinyin.so', args.pinyin_so
            pin = run_worker(spec['mode'], pin_so.resolve(), data, n, scratch)
            subject = run_worker(spec['mode'], subject_so.resolve(), data, n, scratch)
            if args.observations:
                with args.observations.open('a') as capture:
                    capture.write(json.dumps(dict(cell=args.cell,case=n,pin=pin,subject=subject),ensure_ascii=True)+'\n')
            same = matches(spec, pin, subject)
            expected = same if (not args.expect_parent or spec['control']) else not same
            verdict = 'MATCH' if same else 'DIFFER'
            host_dependent = (not same and not args.expect_parent
                              and any(map(host_scanf_accepts, spec['host_scanf']))
                              and pin_loaded_subject_refused_table_conf(pin, subject))
            if host_dependent:
                verdict = 'HOST-DEPENDENT'
                expected = True
            print(json.dumps(dict(cell=args.cell, case=n, verdict=verdict, expected_ok=expected,
                                  exit=[pin['exit'], subject['exit']],
                                  stderr_lines=[pin['stderr_lines'], subject['stderr_lines']]),
                             sort_keys=True), flush=True)
            if host_dependent:
                continue
            if not expected or (not same and not args.expect_parent):
                failures += 1
                if not same:
                    for side, r in (('pin', pin), ('subject', subject)):
                        print('  %s exit=%s %s' % (side, r['exit'], json.dumps(
                            r['result'] and observed(r['result']), sort_keys=True, ensure_ascii=False)))
                        if spec['userdir']:
                            print('  %s userfiles=%s' % (side, json.dumps(r['userfiles'], sort_keys=True)))
                        if spec['stderr']:
                            print('  %s stderr=%r' % (side, r['stderr']))
    return 1 if failures else 0


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--worker':
        worker(*sys.argv[2:])
    else:
        sys.exit(main())
