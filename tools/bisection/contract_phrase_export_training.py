"""Phrase export and training cases (inventory row 51, #1175).
"""
import ctypes as C
from pathlib import Path

from contract_abi import Arr, B, I, P, S, U, UNTOUCHED, Z, key_out, tokens_of
from contract_case_helpers import (
    _PAST_RANGES, _gram_bytes, _guess_out, _modify_header, _replace_system_start_row, _replace_user_bigram, _write_log,
    remember_export, train_selected_top)
from contract_fixtures import _write_user_conf, import_rows


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #759: the export iterator is the pin's cursor (pinyin.cpp:662-768 at
# 074a2219). Every get_next re-reads the item it is at and re-probes the
# range (:748-767), so an import that lands between two calls is exported:
# a reading merged into the item under the cursor, and a token appended
# past it. The parent buffered each probed item's rows at the probe.
def export_rows(k, it, limit=None):
    """Consume up to limit export rows without closing the caller's iterator.

    Return [flag, phrase, reading, count] rows and decode/free the returned
    strings. None consumes to exhaustion; even a reached limit still calls
    has_next first. Native getter failure does not stop iteration."""
    rows = []
    while k.fn('iterator_has_next_phrase', B, P)(it) and (limit is None or len(rows) < limit):
        ph, py, n = P(), P(), I(UNTOUCHED)
        ret = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
            it, C.byref(ph), C.byref(py), C.byref(n))
        rows.append([ret, k.text(ph.value), k.text(py.value), n.value])
    return rows


def export_open_across_import_system(k):
    """Keep a system export cursor open while importing another reading for its next token.

    Use library one's first two tokens and assert their getter results. Consume
    the first token's rows, add the second token's tone-one reading, then read
    its original count plus one. Close the iterator and free/finalize cached
    objects on success; the pronunciation array remains allocated."""
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
    """Observe a live user export cursor across insertion of a new token and reading.

    Seed 你好, open/probe the iterator, import 好你 and a toned 你好 reading,
    then consume remaining rows. Close the iterator and finalize the context
    without clearing the cached pointer."""
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
    """Compare exports of 你 around the selected training and persistence sequence.

    Actions no-train and unconstrained only guess or train without selection;
    others train the selected token. open-before keeps an export cursor live;
    save-reopen and replayed-train finalize and clear caches, with the latter
    training again after lazy reopening. Return before/after rows and call results."""
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


case('issue790-export-after-train-system', issues=('790',))(issue790_export)
for _action in ('no-train', 'unconstrained', 'open-before', 'save-reopen', 'replayed-train'):
    case('issue790-export-' + _action + '-system',
         control=_action in ('no-train', 'unconstrained', 'save-reopen'), issues=('790',))(
             lambda k, action=_action: issue790_export(k, action))


def issue790_readings(k, phrase, readings, unigram=None):
    """Replay a system-item MODIFY using the existing logger fixture
    helpers; only the named item's readings and optional unigram change.

    readings contains (pinyin spelling, count) pairs encoded as 16-bit keys
    and 32-bit counts. Preserve the unigram unless supplied and wrap the library
    total modulo 2**32. Assert library one and a single-character item, free
    and finalize cached objects, clear pointers, write the marker/log and return
    the token; reopening is deferred. Layouts follow PhraseItem::get_nth_pronunciation
    at src/storage/phrase_index.cpp:33-43, PhraseItem::get_unigram_frequency at
    src/storage/phrase_index.h:124-126 and PhraseIndexLogger::next_record at
    src/storage/phrase_index_logger.h:158-211 (pin 074a2219)."""
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
    """Seed 你 with the supplied ni3 count and two zero-count readings, then compare exports after training.

    Assert ni parses and train its selected token after the rewritten log reopens."""
    token = issue790_readings(k, '你', [('ni3', count), ('ni2', 0), ('na4', 0)])
    out = {'before': remember_export(k, 1, '你')}
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'ni')
    out['training'] = train_selected_top(k, '你', token, guess_before_choose=True)
    out['after'] = remember_export(k, 1, '你')
    return out


case('issue790-export-zero-and-signed-system', issues=('790',))(
    lambda k: issue790_bounded_counts(k, (1 << 31) - 69))
case('issue790-export-prefix-overflow-system', issues=('790',))(
    lambda k: issue790_bounded_counts(k, (1 << 32) - 76))


def issue790_imported(k, appended=False):
    """Import a system reading or appended four-character phrase and compare exports around training.

    appended chooses 你你你你; otherwise add ni2 to 你. Parse the imported
    reading and train the first matching token, recording import and training outputs."""
    phrase, reading = ('你你你你', "ni'ni'ni'ni") if appended else ('你', 'ni2')
    out = {'imported': import_rows(k, 1, [(phrase, reading, 5)]),
           'before': remember_export(k, 1, phrase)}
    token = tokens_of(k, phrase)[0]
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, reading.encode())
    out['training'] = train_selected_top(k, phrase, token, guess_before_choose=True)
    out['after'] = remember_export(k, 1, phrase)
    return out


case('issue790-export-override-only-system', issues=('790',))(issue790_imported)
case('issue790-export-appended-system', issues=('790',))(lambda k: issue790_imported(k, appended=True))


@case('issue790-guess-after-train-system', issues=('790',))
def _(k):
    """Compare ni guesses before and after training a low-mass reading of 尼.

    Rewrite its pronunciation counts and system/user start rows, train the selected
    token, clear the constraint and collect a fresh guess without saving."""
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


@case('issue790-character-offset-after-train-system', issues=('790',))
def _(k):
    """Compare character-offset output before and after training an initially zero-count na reading.

    Seed 哪 with na3/nei3, train through incomplete n, clear its constraint,
    and query again without saving or reopening."""
    # Both readings have shipped index entries. An incomplete n path
    # trains na (initially zero) and nei; the public offset call then tests
    # na without saving/reopening. An unindexed positive reading would
    # instead exercise incomplete-key expansion's membership semantics.
    token = issue790_readings(k, '哪', [('na3', 0), ('nei3', 100)])
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a)
    get = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))
    def offset():
        """Parse na and return the offset-call flag and sentinel-initialized output for 哪 at byte one."""
        assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'na')
        value = Z(UNTOUCHED)
        return [get(k.inst, '哪'.encode(), 1, C.byref(value)), value.value]
    out = {'before': offset()}
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'n')
    out['training'] = train_selected_top(k, '哪', token, guess_before_choose=True)
    out['clear'] = k.fn('clear_constraint', B, P, Z)(k.inst, 0)
    out['after'] = offset()
    return out


case('issue759-export-open-across-import-system', issues=('759',))(export_open_across_import_system)
case('issue759-export-open-across-import-user', issues=('759',))(export_open_across_import_user)


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
