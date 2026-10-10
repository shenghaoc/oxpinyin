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
import ctypes as C
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
         cells=('bdb', 'kc', 'tkrzw')):
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
    a case skipped on the others prints `SKIP` instead of a verdict."""
    def register(fn):
        CASES[name] = dict(fn=fn, mode=mode, control=control, abort=abort, stderr=stderr or crash is not None,
                           crash=crash, userdir=userdir, cells=cells)
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


@case('train-forcing-after-lookup', control=True)
def _(k):
    inst, chosen = forced_sentence(k, True)
    return {'chosen': chosen, 'train': k.fn('train', B, P, C.c_ubyte)(inst, 0),
            'again': k.fn('train', B, P, C.c_ubyte)(inst, 0)}


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
def _craft_db(path, rows, dbtype):
    """A user-table container of raw `key -> value` records in the running
    cell's own format: a Berkeley DB `dbtype` container on bdb, the Kyoto
    Cabinet snapshot the pin's `load_db` reads on kc (every user table
    there is loaded with `load_snapshot`, `chewing_large_table2_kyotodb.cpp:106`,
    `ngram_kyotodb.cpp:54`, `phrase_large_table3_kyotodb.cpp:109`), or the
    tkrzw `TreeDBM` file the pin's `load_db` opens on tkrzw
    (`chewing_large_table2_tkrzwdb.cpp:83`, `ngram_tkrzwdb.cpp:48`,
    `phrase_large_table3_tkrzwdb.cpp:83`); the bigram is a `HashDBM` there
    (`ngram_tkrzwdb.cpp:50`). `dbtype` picks the container kind (1 btree/tree,
    2 hash); the rows are the same records in every cell."""
    if CELL == 'kc':
        return _craft_kc(path, rows)
    if CELL == 'tkrzw':
        return _craft_tkrzw(path, rows, 'HashDBM' if dbtype == 2 else 'TreeDBM')
    return _craft_bdb(path, rows, dbtype)


def _craft_kc(path, rows):
    """A Kyoto Cabinet snapshot of the records (`BasicDB::dump_snapshot`),
    written through the C API from an in-memory database."""
    kc = C.CDLL('libkyotocabinet.so.16')
    kc.kcdbnew.restype = C.c_void_p
    kc.kcdbdel.argtypes = [C.c_void_p]
    kc.kcdbopen.argtypes = [C.c_void_p, C.c_char_p, C.c_uint32]
    kc.kcdbset.argtypes = [C.c_void_p, C.c_char_p, C.c_size_t, C.c_char_p, C.c_size_t]
    kc.kcdbdumpsnap.argtypes = [C.c_void_p, C.c_char_p]
    kc.kcdbclose.argtypes = [C.c_void_p]
    if os.path.exists(path):
        os.unlink(path)
    db = kc.kcdbnew()
    assert db
    # KCOWRITER | KCOCREATE on the in-memory database "-".
    assert kc.kcdbopen(db, b'-', (1 << 1) | (1 << 2))
    for key, value in rows:
        assert kc.kcdbset(db, key, len(key), value, len(value))
    assert kc.kcdbdumpsnap(db, path.encode())
    assert kc.kcdbclose(db)
    kc.kcdbdel(db)


def _craft_tkrzw(path, rows, kind):
    """A tkrzw `kind` file (`TreeDBM` for the chewing and phrase tables,
    `HashDBM` for the bigram, as each pin `load_db` opens it) of the records,
    written through the C API with the default tuning the pin's `save_db`
    uses."""
    tk = C.CDLL('libtkrzw.so.1')
    tk.tkrzw_dbm_open.restype = C.c_void_p
    tk.tkrzw_dbm_open.argtypes = [C.c_char_p, C.c_bool, C.c_char_p]
    tk.tkrzw_dbm_set.restype = C.c_bool
    tk.tkrzw_dbm_set.argtypes = [C.c_void_p, C.c_char_p, C.c_int32, C.c_char_p, C.c_int32, C.c_bool]
    tk.tkrzw_dbm_close.restype = C.c_bool
    tk.tkrzw_dbm_close.argtypes = [C.c_void_p]
    if os.path.exists(path):
        os.unlink(path)
    db = tk.tkrzw_dbm_open(path.encode(), True, ('dbm=' + kind).encode())
    assert db
    for key, value in rows:
        assert tk.tkrzw_dbm_set(db, key, len(key), value, len(value), True)
    assert tk.tkrzw_dbm_close(db)


def _craft_bdb(path, rows, dbtype):
    """A Berkeley DB `dbtype` container of raw `key -> value` records."""
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
    if os.path.exists(path):
        os.unlink(path)
    handle = C.c_void_p()
    assert db.db_create(C.byref(handle), None, 0) == 0
    # DB_CREATE == 0x1; the pin opens its tables the same way
    # (`chewing_large_table2_bdb.cpp:90`, `ngram_bdb.cpp:56`).
    assert db.__db_open_pp(handle, None, path.encode(), None, dbtype, 0x1, 0o600) == 0
    live = []
    for key, value in rows:
        kbuf = C.create_string_buffer(key)
        vbuf = C.create_string_buffer(value)
        live += [kbuf, vbuf]
        kd = DBT(C.cast(kbuf, C.c_void_p), len(key), 0, 0, 0, None, 0)
        vd = DBT(C.cast(vbuf, C.c_void_p), len(value), 0, 0, 0, None, 0)
        assert db.__db_put_pp(handle, None, C.byref(kd), C.byref(vd), 0) == 0
    assert db.__db_close_pp(handle, 0) == 0
    del live


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


def run_worker(mode, so, data, name, scratch):
    env = dict(os.environ, TMPDIR=str(scratch))
    before = set(os.listdir(scratch))
    proc = subprocess.run([sys.executable, __file__, '--worker', mode, str(so), str(data), name],
                          capture_output=True, env=env)
    stdout = proc.stdout.decode('utf-8', 'replace')
    # stderr keeps invalid UTF-8 apart from a real U+FFFD (`surrogateescape`),
    # since the pin writes paths as their bytes.
    stderr = proc.stderr.decode('utf-8', 'surrogateescape')
    lines = [json.loads(line) for line in stdout.splitlines() if line.startswith('{')]
    return dict(exit=proc.returncode, result=lines[-1] if lines else None,
                userfiles=user_dir_listing(scratch, before),
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
        case('table-conf-lambda-' + _name + _suffix, mode=_mode,
             stderr=True)(header_probe(('lambda parameter:0.312699', 'lambda parameter:' + _spelling)))


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


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('cell', choices=['bdb', 'kc', 'tkrzw'])
    parser.add_argument('prefix', type=Path)
    parser.add_argument('pinyin_so', type=Path)
    parser.add_argument('--zhuyin-so', type=Path)
    parser.add_argument('--cases', default='')
    parser.add_argument('--expect-parent', action='store_true')
    parser.add_argument('--observations', type=Path, default=os.environ.get('CONTRACT_DIFF_OBSERVATIONS'),
                        help='retain both complete observations as JSONL (also CONTRACT_DIFF_OBSERVATIONS)')
    args = parser.parse_args()
    os.environ['CONTRACT_DIFF_CELL'] = args.cell
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
            if spec['crash'] is not None:
                shown = subject['result'] or {}
                same = pin['exit'] == -signal.SIGSEGV and subject['exit'] == 0 and \
                    pin['stderr'] == subject['stderr'] and \
                    all(shown.get(key) == value for key, value in spec['crash'].items())
            elif spec['abort'] is not NO_ABORT:
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
