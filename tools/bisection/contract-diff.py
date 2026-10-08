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

A class (c) case (`abort=`) holds when the pin dies of SIGABRT and the subject
answers the declared value with exactly one warning in its own library's domain
(`libpinyin`, or `libzhuyin` for the zhuyin facade).

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
import subprocess
import sys
import tempfile

P, U, I, S, B, Z = C.c_void_p, C.c_uint, C.c_int, C.c_char_p, C.c_bool, C.c_size_t
UNTOUCHED = 0xABCDEF  # out-param sentinel

CASES = {}


NO_ABORT = object()

# Each facade logs under its own library's domain.
WARNING_DOMAIN = {'pinyin': 'libpinyin', 'zhuyin': 'libzhuyin'}


def case(name, mode='pinyin', control=False, abort=NO_ABORT, stderr=False):
    """Registers a case. `stderr=True` also compares what the library wrote
    to stderr (raw `fprintf`s of the pin; GLib logs are the `logs` field),
    with the scratch directory names normalised. `abort=<value>` marks a
    class (c) site: the pin
    must die of SIGABRT, and the subject must return `<value>` in its `ret`
    field with exactly one GLib warning in the facade's domain (`libpinyin`,
    or `libzhuyin` for the zhuyin facade; level 16)."""
    def register(fn):
        CASES[name] = dict(fn=fn, mode=mode, control=control, abort=abort, stderr=stderr)
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


# One past the reserved slot is an ordinary lookup that finds nothing.
@case('guess-candidates-past-the-reserved-slot')
def _(k):
    out = {}
    inst = full_inst(k, b'nihao')
    for offset in (5, 6):
        # The returns only: at these offsets the pin still lists the LONGER
        # row (policy row 65), which oxpinyin does not.
        out['offset %d' % offset] = k.fn('guess_candidates', B, P, Z, U)(inst, offset, 0)
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


# --------------------------------------------------------------------------

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


def run_worker(mode, so, data, name, scratch):
    env = dict(os.environ, TMPDIR=str(scratch))
    proc = subprocess.run([sys.executable, __file__, '--worker', mode, str(so), str(data), name],
                          capture_output=True, env=env)
    stdout = proc.stdout.decode('utf-8', 'replace')
    # stderr keeps invalid UTF-8 apart from a real U+FFFD (`surrogateescape`),
    # since the pin writes paths as their bytes.
    stderr = proc.stderr.decode('utf-8', 'surrogateescape')
    lines = [json.loads(line) for line in stdout.splitlines() if line.startswith('{')]
    return dict(exit=proc.returncode, result=lines[-1] if lines else None,
                stderr_lines=len(stderr.splitlines()),
                stderr=re.sub(r'(user|sys)-(\udcff)?[A-Za-z0-9_]+', r'\1-\2X', stderr))


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
            if spec['abort'] is not NO_ABORT:
                shown = subject['result'] or {}
                same = pin['exit'] == -6 and subject['exit'] == 0 and \
                    shown.get('ret') == spec['abort'] and shown.get('logs') == [[WARNING_DOMAIN[spec['mode']], 16]] and \
                    all(v for key, v in shown.items() if key.startswith('untouched'))
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
                        if spec['stderr']:
                            print('  %s stderr=%r' % (side, r['stderr']))
    return 1 if failures else 0


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--worker':
        worker(*sys.argv[2:])
    else:
        sys.exit(main())
