"""256th pronunciation reading cases."""
import ctypes as C
import hashlib
import json
import tempfile

from contract_abi import Arr, B, I, P, S, U, UNTOUCHED, tokens_of, unigram_of


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


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
    """Enumerate candidate pinyin syllable bytes in initial/final/tone product order.

    Include tones 1-4 and an empty initial; parsing and reading deduplication are
    left to the caller.
    """
    initials = ['b', 'p', 'm', 'f', 'd', 't', 'n', 'l', 'g', 'k', 'h', 'j', 'q', 'x', 'zh', 'ch', 'sh',
                'r', 'z', 'c', 's', 'y', 'w', '']
    finals = ['a', 'o', 'e', 'ai', 'ei', 'ao', 'ou', 'an', 'en', 'ang', 'eng', 'i', 'u', 'ia', 'ie',
              'iao', 'iu', 'ian', 'in', 'iang', 'ing', 'ua', 'uo', 'uai', 'ui', 'uan', 'un', 'uang', 'ong']
    return [(i + f + t).encode() for i in initials for f in finals for t in '1234']


def _wrap_zhuyin_readings():
    """Enumerate UTF-8 zhuyin reading candidates in initial/final/tone product order.

    Try second, third, fourth and unmarked tones for each listed combination;
    the returned combinations are not prevalidated as syllables.
    """
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
    """Build a reader that exports one phrase's readings from a pinyin library."""
    def rows(text):
        """Walk the whole export iterator and retain [reading, frequency] rows for text.

        Decode and free each returned phrase/reading string and close the iterator;
        preserve export order, including repeated readings.
        """
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
    """Observe the 256th and 257th reading additions across all configured libraries.

    Filter candidate readings on a scratch profile, fill each available item to
    255 readings, then observe live, end-of-import and saved/reopened states.
    same_iterator chooses where the 257th add occurs. This exercises the guint8
    count in PhraseItem::add_pronunciation at
    074a2219 phrase_index.cpp:56-91 and phrase_index.cpp:28-30. Exhausting readings
    propagates StopIteration; unavailable libraries are recorded and skipped.
    """
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
            """Consume one reading, add it at frequency, and return status and current count.

            Group add, token lookup and count retrieval in one per-call step; return
            zero for the count if the phrase has no token in this library.
            """
            reading = next(pending)
            with k.per_call_step():
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


@case('wrap-256th-reading', issues=('804',))
def _(k):
    """Observe pinyin reading-count wrap with the 257th add in a new iterator."""
    return wrap_256th_reading(k, _wrap_pinyin_readings(), same_iterator=False)


@case('wrap-256th-reading-same-iterator', issues=('804',))
def _(k):
    """Observe pinyin reading-count wrap with the 257th add in the same iterator."""
    return wrap_256th_reading(k, _wrap_pinyin_readings(), same_iterator=True)


@case('wrap-256th-reading-zhuyin', mode='zhuyin', issues=('804',))
def _(k):
    """Observe zhuyin reading-count wrap with the 257th add in a new iterator."""
    return wrap_256th_reading(k, _wrap_zhuyin_readings(), same_iterator=False)


@case('wrap-256th-reading-zhuyin-same-iterator', mode='zhuyin', issues=('804',))
def _(k):
    """Observe zhuyin reading-count wrap with the 257th add in the same iterator."""
    return wrap_256th_reading(k, _wrap_zhuyin_readings(), same_iterator=True)


def register(driver_case):
    """Register the declarations in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
