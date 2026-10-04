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
