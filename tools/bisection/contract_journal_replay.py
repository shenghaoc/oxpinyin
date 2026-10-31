"""Journal replay and removed items cases (inventory row 65, #1175).
"""
import ctypes as C
import hashlib
import os
from pathlib import Path
import time

from contract_abi import Arr, B, I, P, S, U, UNTOUCHED, Z, sentence_out, tokens_of, unigram_of, zhuyin_sentence_out
from contract_case_helpers import (
    _chunk, _dynamic_adjust_out, _facade_total_out, _modify_header, _reload_pron_keys, _write_log, candidate_rows,
    guess_rows, log_walk, phrase_table_reopen, remember_export, token_phrase_of, token_unigram_trained,
    train_selected_top, zhuyin_guess_rows)
from contract_fixtures import _write_user_conf, import_rows, private_system


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


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
    """A system chunk's total, its items `{slot: bytes}` at their header
    length, and the item-area offsets `{slot: offset}`."""
    payload = Path(k.data, name).read_bytes()[8:]
    word = lambda offset: int.from_bytes(payload[offset:offset + 4], 'little')
    total, index, content = word(0), word(4), word(8)
    items, offsets = {}, {}
    for slot in range((content - index - 1) // 4):
        offset = word(index + slot * 4)
        if offset:
            start = content + offset
            # Only the two header bytes size an item; copying the remaining
            # chunk here for every slot makes fixture setup quadratic.
            size = _item_size(payload[start:start + 2])
            items[slot] = payload[start:start + size]
            offsets[slot] = offset
    return total, items, offsets


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
    """Decode the item's counted four-byte little-endian character fields.

    Use PhraseItem::get_phrase_string's layout in src/storage/phrase_index.cpp:103-105
    at pin 074a2219. Do not validate total extent; chr rejects values above
    Unicode's maximum and an absent count byte raises IndexError."""
    return ''.join(chr(int.from_bytes(item[6 + 4 * i:10 + 4 * i], 'little')) for i in range(item[0]))


def _observe_system_item(k, watched, phrase):
    """Collect watched tokens' phrase/unigram outputs plus phrase-table matches and library-one exports."""
    out = {name: [token_phrase_of(k, token), unigram_of(k, token)] for name, token in watched.items()}
    # The phrase table keeps the token whatever the sub-index holds.
    out['phrase-table'] = list(tokens_of(k, phrase))
    out['export'] = remember_export(k, 1, phrase)
    return out


def system_replay(k, build, reopen=False, unigram_add=True, observe_add=False, observe=None,
                   observe_saved=False):
    """`build(p)` returns the records after the header and the tokens to
    watch; `p` maps `one`, `two` (an item of two or more readings) and `last`
    (the item at the highest item-area offset) to (token, bytes) of library 1,
    and `fresh` and `absent` to a token it does not hold: one for a trailing
    ADD, one no record adds."""
    total, items, offsets = _system_items(k)
    one = next(slot for slot, item in items.items() if item[0] == 1 and item[1] == 1)
    two = next(slot for slot, item in items.items() if item[0] == 1 and item[1] >= 2)
    last = max(offsets, key=offsets.get)
    fresh = max(items) + 2
    records, watched = build({'total': total, 'one': (0x01000000 | one, items[one]),
                              'two': (0x01000000 | two, items[two]),
                              'last': (0x01000000 | last, items[last]),
                              'fresh': (0x01000000 | fresh, _item_with(items[one], unigram=11)),
                              'absent': (0x01000000 | (fresh + 3), b''),
                              'items': items, 'offsets': offsets})
    phrase = _item_text(items[one])
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total) + b''.join(records))
    out = {'total': total, 'ctx': bool(k.ctx)}
    out.update(_observe_system_item(k, watched, phrase))
    if observe:
        out['detail'] = observe(k)
    if unigram_add:
        # A unigram add reaches only an item the sub-index holds.
        out['add-unigram'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, watched['one'], 7)
    if observe_add:
        out['after-add'] = _observe_system_item(k, watched, phrase)
        if k.mode == 'pinyin':
            out['facade-after-add'] = _facade_total_out(k)
    # An import marks the context modified, so the save writes the diff.
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    out['save'] = k.fn('save', B, P)(k.ctx)
    out['dbin'] = Path(k.user, 'gb_char.dbin').read_bytes().hex()
    if observe_saved:
        header, counts = log_walk(bytes.fromhex(out['dbin']))
        out['saved-header'] = header
        out['saved-counts'] = counts
    if reopen:
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        out['reopen'] = _observe_system_item(k, watched, phrase)
        if observe:
            out['reopen-detail'] = observe(k)
    return out


def _replay_case(name, build, reopen=False, unigram_add=True, observe_add=False, observe=None,
                 observe_saved=False, extra_issues=(), **options):
    """Register both facade replay cases with the supplied builder and observation options.

    Enable stderr comparison and combine the replay issue IDs with extra_issues;
    the lambdas defer file mutation and native work until case execution."""
    for mode in ('pinyin', 'zhuyin'):
        case('replay-' + name + '-' + mode, mode=mode, stderr=True,
             issues=('851', '850', '870') + tuple(extra_issues), **options)(
            lambda k, build=build: system_replay(k, build, reopen, unigram_add, observe_add,
                                                 observe, observe_saved))


def _watch(p):
    """Return the standard watched-token mapping for the existing one and fresh fixture items."""
    return {'one': p['one'][0], 'fresh': p['fresh'][0]}


# #853: accepted live-item increments survive a later dirty save.
_replay_case('unigram-add-live-item', lambda p: ([], _watch(p)))
_replay_case('unigram-add-live-item-reopen', lambda p: ([], _watch(p)), reopen=True)
_replay_case('unigram-add-live-item-control', lambda p: ([], _watch(p)), unigram_add=False, control=True)
_replay_case('unigram-add-overridden-item-reopen', lambda p: (
    [_log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                 new=_item_with(p['one'][1], unigram=19))], _watch(p)), reopen=True)


# #853 lifecycle: the rules are read from phrase_index.cpp:150-178,
# :237-269, :392-436, :648-675; pinyin.cpp:956, :1132-1145,
# :1253-1285, and phonetic_lookup.h:925-929 at 074a2219.
def unigram_lifecycle(k, variant):
    """Observe a system unigram through the selected mutation, dirty save and reopen sequence.

    Work on private system links and a single-character item (你 for import-system).
    Variants prepare removed, wrapping, overflowing or missing items, repeat adds,
    reload/mask a library, replace its disk bytes, import a system reading or train.
    Record clean/dirty save flags, journal size/hash/records and post-reopen adds.
    timing additionally performs 1000 increments and emits diagnostic nanoseconds;
    repeat performs a second dirty save/reopen. Leave the final context live."""
    system = private_system(k)
    k.data = system
    name = 'gbk_char.bin' if variant == 'reload' else 'gb_char.bin'
    library = 2 if variant == 'reload' else 1
    total, items, offsets = _system_items(k, name)
    slot = next(slot for slot, item in items.items() if item[0] == 1 and item[1] == 1)
    if variant == 'import-system':
        slot = 0x1225  # 你
    token, item = (library << 24) | slot, items[slot]
    _write_user_conf(k)
    if variant == 'removed':
        _write_log(k, _modify_header(0, total, total) +
                   _log_record(_LOG_REMOVE, token, old=item))
    elif variant == 'wrap':
        # A valid layout with an intentionally inconsistent stored total:
        # item arithmetic wraps independently of the guarded library total.
        payload = bytearray(Path(system, name).read_bytes()[8:])
        content = int.from_bytes(payload[8:12], 'little')
        start = content + offsets[slot] + 2
        payload[start:start + 4] = (0xfffffffc).to_bytes(4, 'little')
        path = Path(system, name)
        path.unlink()  # private symlink; never write the source data
        path.write_bytes(_chunk(payload))
    elif variant == 'overflow':
        _write_log(k, _modify_header(0, total, 0xfffffffc))
    assert k.ctx
    if variant == 'import-system':
        reading = 'ni3' if k.mode == 'pinyin' else 'ㄋㄧˇ'
        assert import_rows(k, 1, [('你', reading, 9)]) == [True]
    add = k.fn('token_add_unigram_frequency', B, P, U, U)
    watched = token if variant != 'missing' else ((library << 24) | (max(items) + 20))
    out = {'before': unigram_of(k, watched)}
    out['add'] = add(k.inst, watched, 7)
    out['after'] = unigram_of(k, watched)
    if variant == 'timing':
        started = time.perf_counter_ns()
        for _ in range(1000):
            assert add(k.inst, token, 1)
        out['~increment-ns'] = time.perf_counter_ns() - started
    if variant == 'repeat':
        out['again'] = add(k.inst, watched, 11)
        out['after-again'] = unigram_of(k, watched)
        other = k.alloc()
        frequency = U()
        out['other-instance'] = [k.fn('token_get_unigram_frequency', B, P, U, C.POINTER(U))(
            other, watched, C.byref(frequency)), frequency.value]
        k.fn('free_instance', None, P)(other)
    if variant in ('overflow', 'missing', 'removed') and k.mode == 'pinyin':
        out['facade'] = _facade_total_out(k)
    out['clean-save'] = k.fn('save', B, P)(k.ctx)
    if variant == 'reload':
        out['unload'] = k.fn('unload_phrase_library', B, P, C.c_ubyte)(k.ctx, 2)
        out['unloaded-add'] = add(k.inst, token, 13)
        out['load'] = k.fn('load_phrase_library', B, P, C.c_ubyte)(k.ctx, 2)
        out['after-load'] = unigram_of(k, token)
    elif variant == 'mask':
        out['mask'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, 0)
        out['after-mask'] = unigram_of(k, token)
    elif variant == 'replacement':
        payload = bytearray(Path(system, name).read_bytes()[8:])
        content = int.from_bytes(payload[8:12], 'little')
        start = content + offsets[slot] + 2
        payload[start:start + 4] = (37).to_bytes(4, 'little')
        payload[:4] = (total - int.from_bytes(item[2:6], 'little') + 37).to_bytes(4, 'little')
        path = Path(system, name)
        temporary = Path(str(path) + '.new')
        temporary.write_bytes(_chunk(payload))
        os.replace(temporary, path)
    elif variant == 'import-system':
        out['system-import'] = import_rows(k, 1, [('你', reading, 17)])
    elif variant == 'training':
        trained_token = 0x01003e57
        out['training-add'] = add(k.inst, trained_token, 7)
        out['training'] = token_unigram_trained(k)
    # An import makes save dirty without replacing the watched system item.
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    out['import'] = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    started = time.perf_counter_ns()
    out['save'] = k.fn('save', B, P)(k.ctx)
    if variant == 'timing':
        out['~save-ns'] = time.perf_counter_ns() - started
    log = Path(k.user, name.replace('.bin', '.dbin')).read_bytes()
    out['journal'] = [len(log), hashlib.sha256(log).hexdigest(), log_walk(log)]
    out['after-save'] = unigram_of(k, watched)
    phrase_table_reopen(k)
    out['reopen'] = unigram_of(k, watched)
    out['add-reopened'] = add(k.inst, watched, 5)
    out['after-reopened'] = unigram_of(k, watched)
    if variant == 'repeat':
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
        out['second-import'] = k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, '你好'.encode(), reading, 1)
        k.fn('end_add_phrases', None, P)(it)
        out['second-save'] = k.fn('save', B, P)(k.ctx)
        phrase_table_reopen(k)
        out['second-reopen'] = unigram_of(k, watched)
    return out


for _mode in ('pinyin', 'zhuyin'):
    for _variant in ('repeat', 'replacement', 'wrap', 'overflow', 'missing', 'removed',
                     'reload', 'mask', 'training', 'timing', 'import-system'):
        case('unigram-lifecycle-' + _variant + '-' + _mode, mode=_mode, stderr=True,
             control=_variant in ('overflow', 'missing', 'removed', 'reload', 'mask'), issues=('853', '870'))(
            lambda k, variant=_variant: unigram_lifecycle(k, variant))


def unigram_null_user_save(k):
    """Exercise unigram updates around a save with a literal NULL user directory.

    Replace a private library's stored total with 0xfffffffb, initialize and
    attempt increments of two and four around save. Return count and dynamic
    pricing observations without assuming either update succeeds."""
    # The pin's save guards precede compact (pinyin.cpp:1132-1145,
    # zhuyin.cpp:547-554). Both C facades already enforce those guards;
    # direct reporting-helper coverage is a user-store unit test.
    system = private_system(k)
    k.data = system
    payload = bytearray(Path(system, 'gb_char.bin').read_bytes()[8:])
    payload[:4] = (0xfffffffb).to_bytes(4, 'little')
    path = Path(system, 'gb_char.bin')
    path.unlink()  # replace our private symlink, never the oracle data
    path.write_bytes(_chunk(payload))
    k._ctx = k.init(system, None, literal=True)
    assert k._ctx
    token = 0x01000001
    add = k.fn('token_add_unigram_frequency', B, P, U, U)
    out = {'before': unigram_of(k, token)}
    out['near-bound'] = add(k.inst, token, 2)
    out['before-save'] = unigram_of(k, token)
    out['save'] = k.fn('save', B, P)(k.ctx)
    out['across-bound'] = add(k.inst, token, 4)
    out['after-refusal'] = unigram_of(k, token)
    out['pricing'] = _dynamic_adjust_out(k, b'ni3')
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('unigram-null-user-save-' + _mode, mode=_mode, control=True, stderr=True, issues=('853', '870'))(
        unigram_null_user_save)


# #853/#876 overlap: unigram ownership must not erase a replayed reading's
# count, whether the sub-index accepts or refuses the increment. The facade
# still takes the refused delta before dispatch (phrase_index.h:628-636).
_replay_case('unigram-add-changed-reading-reopen', lambda p: (
    [_log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                 new=_item_with(p['one'][1], unigram=19, freq=250))], _watch(p)),
    reopen=True, observe_add=True)
_replay_case('unigram-refused-changed-reading-reopen', lambda p: (
    [_log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                 new=_item_with(p['one'][1], freq=250)),
     _modify_header(0, p['total'], 0xfffffffc)], _watch(p)),
    reopen=True, observe_add=True, control=True)


def _add_fresh(p):
    """Encode an ADD for the builder's fresh token and item bytes."""
    return _log_record(_LOG_ADD, p['fresh'][0], new=p['fresh'][1])


# A matching REMOVE, then an ADD: the item is gone, the total is lower and the
# ADD after it applied.
_replay_case('unigram-add-appended-item-reopen', lambda p: (
    [_add_fresh(p)], {'one': p['fresh'][0], 'original': p['one'][0]}), reopen=True)


def _matched_remove(p):
    """Build a matching REMOVE followed by a fresh ADD and return the standard watched tokens."""
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


# #870: merge walks the journal in record order, while a later diff walks
# ascending slots (074a2219 phrase_index.cpp:392-436,439-517). Repeated,
# out-of-order writes leave each slot's final value, including after reopen.
def _touched_order(p):
    """Build descending-token MODIFY records and a repeated update to the lower token.

    Set last pronunciation frequencies to 77 then 99 and watch both source items
    plus the fresh token; do not execute a replay."""
    rows = sorted((p['one'], p['two']), reverse=True, key=lambda row: row[0])
    records = [_log_record(_LOG_MODIFY, token, old=item, new=_item_with(item, freq=77))
               for token, item in rows]
    token, item = rows[-1]
    records.append(_log_record(_LOG_MODIFY, token, old=_item_with(item, freq=77),
                               new=_item_with(item, freq=99)))
    return records, dict(_watch(p), two=p['two'][0])


_compare_case('touched-order-duplicates-reopen', _touched_order, reopen=True)


# #870: unigram candidates are per slot, not a whole-library zero-change
# proof. Counts stay absolute in the pin's mapped items (phrase_index.h:124-126).
def _seed_mixed(p):
    """Build one frequency override and two successive unigram overrides on the other item.

    Encode 77 for one's last frequency and 71 then 93 for two's unigram;
    return the records and watched tokens."""
    token, item = p['two']
    return [
        _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                    new=_item_with(p['one'][1], freq=77)),
        _log_record(_LOG_MODIFY, token, old=item, new=_item_with(item, unigram=71)),
        _log_record(_LOG_MODIFY, token, old=_item_with(item, unigram=71),
                    new=_item_with(item, unigram=93)),
    ], dict(_watch(p), two=token)


_compare_case('seed-mixed-repeated-unigrams-reopen', _seed_mixed, reopen=True)
_compare_case('seed-restored-unigram-reopen', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                new=_item_with(p['one'][1], unigram=99)),
    _log_record(_LOG_MODIFY, p['one'][0], old=_item_with(p['one'][1], unigram=99),
                new=_item_with(p['one'][1], freq=77)),
], _watch(p)), reopen=True)
_compare_case('seed-add-remove-unigrams-reopen', lambda p: ([
    _add_fresh(p),
    _log_record(_LOG_REMOVE, p['one'][0], old=p['one'][1]),
    _log_record(_LOG_MODIFY, p['two'][0], old=p['two'][1],
                new=_item_with(p['two'][1], unigram=91)),
], dict(_watch(p), two=p['two'][0])), reopen=True)
_compare_case('seed-header-total-reopen', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                new=_item_with(p['one'][1], freq=77)),
    _modify_header(0, p['total'], 123456),
], _watch(p)), reopen=True)
_compare_case('seed-stale-storage-unigram-reopen', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                new=_item_with(p['one'][1], freq=77)),
    _log_record(_LOG_MODIFY, p['absent'][0], old=_item_with(p['one'][1], freq=77),
                new=_item_with(p['one'][1], unigram=99, freq=77)),
], _watch(p)), reopen=True)


def _touched_sparse_edits(p):
    """Build relocation, replacement and sparse-slot add/remove records in a fixed sequence.

    Use high token 0x01040000, retain a trailing-byte growth fixture, and return
    records plus watched tokens without touching files."""
    token, item = p['one']
    moved = _item_with(item, freq=77)
    high = 0x01040000  # sparse slot 262144; no full-token-range fixture
    records = [
        _log_record(_LOG_MODIFY, token, old=item, new=moved + b'\x01\x02\x03'),
        _log_record(_LOG_MODIFY, token, old=moved, new=_item_with(item, freq=99)),
        _log_record(_LOG_ADD, high, new=p['fresh'][1]),
        _log_record(_LOG_REMOVE, token, old=_item_with(item, freq=99)),
        _log_record(_LOG_ADD, token, new=item),
        _log_record(_LOG_REMOVE, high, old=p['fresh'][1]),
        _log_record(_LOG_ADD, high, new=_item_with(p['fresh'][1], freq=55)),
    ]
    return records, dict(_watch(p), high=high)


_compare_case('touched-sparse-add-remove-relocate-reopen', _touched_sparse_edits, reopen=True)


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
    """Build a record factory for growing an item with trailing bytes before modifying it again.

    stored_old decides whether the second record compares those trailing bytes
    or only the first replacement's header-sized content."""
    def build(p):
        """Encode two modifications of one followed by a fresh ADD.

        Set the first unigram to the original low unigram byte plus 500, append three
        bytes, then change its last frequency from five to nine. Return watched tokens."""
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
    """Build a record factory that drops one reading but stores four trailing bytes.

    stored_old includes those bytes in the second record's expected old payload."""
    def build(p):
        """Shorten two by six bytes, append a four-byte tail and encode a second frequency update.

        Assert the new header size, append a fresh ADD, and return records and watched tokens."""
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
    """Encode two replacements of one with a six-byte empty-phrase item, then a fresh ADD.

    Keep its initial unigram for the first record and use three for the second."""
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
    """Extend the standard watched-token mapping with the absent fixture token."""
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
    """Build records that modify two, then address an absent token using two's replacement bytes.

    grow appends one byte to the absent-token replacement; both forms finish
    with a fresh ADD and the absent-token watch set."""
    def build(p):
        """Encode frequency 21 on two, then frequency 22 through the absent-token record.

        Optionally grow that replacement by one byte; return records without replaying them."""
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


# A MODIFY's new chunk is not checked against its own header: only the old
# chunk is compared (`phrase_index.cpp:481-483`), and a new chunk no longer
# than the visible item is `memmove`d over the stored bytes (`:493-500`). The
# stored item keeps its length, so its header still names the whole item.
_compare_case('modify-short-in-place', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                new=p['one'][1][:2] + (200).to_bytes(4, 'little')),
    _add_fresh(p)], _watch(p)))


# The default `PhraseItem` is six zero bytes (`phrase_index.h:81-84`), not an
# empty item: a MODIFY of an absent token whose old chunk is those zeros passes
# the compare with no earlier MODIFY, and a new chunk that fits the six bytes
# is written into `item`'s own buffer. No entry is created and the merge goes
# on; a later compare sees what the write left there.
_ZERO_ITEM = bytes(6)


def _modify_absent_default(twice=False):
    """Build an absent-token MODIFY sequence beginning with a six-zero-byte expected item.

    twice adds a second six-byte replacement before the trailing fresh ADD."""
    def build(p):
        """Encode absent-token unigram 50 and optionally 60, then return a fresh ADD and watches."""
        first = bytes((0, 0)) + (50).to_bytes(4, 'little')
        records = [_log_record(_LOG_MODIFY, p['absent'][0], old=_ZERO_ITEM, new=first)]
        if twice:
            second = bytes((0, 0)) + (60).to_bytes(4, 'little')
            records.append(_log_record(_LOG_MODIFY, p['absent'][0], old=first, new=second))
        records.append(_add_fresh(p))
        return records, _watch_absent(p)
    return build


_compare_case('modify-absent-default', _modify_absent_default())
_compare_case('modify-absent-default-twice', _modify_absent_default(True))
# A new chunk longer than the six-byte default reaches the NULL of REMOVE's
# assert (`:486-491`): the pin dies of SIGSEGV, the subject stops at the record.
_compare_case('modify-absent-default-grow', lambda p: ([
    _log_record(_LOG_MODIFY, p['absent'][0], old=_ZERO_ITEM,
                new=bytes((0, 0)) + (50).to_bytes(4, 'little') + b'\x00'),
    _add_fresh(p)], _watch_absent(p)),
    crash={'ctx': True, 'absent': _NO_ITEM, 'fresh': _NO_ITEM,
           'one': [[True, 1, '锕'], [True, 3]]}, control=True)


# A small over-long header on a non-last item: the compare passes, the
# six-byte chunk is written in place, and the next read takes the twenty-two
# bytes its header names, six of them from the following item. That read ends
# inside the content buffer, so the pin's answer is reproduced.
_compare_case('modify-header-overlong-small', lambda p: ([
    _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                new=bytes((2, 1)) + (200).to_bytes(4, 'little')),
    _add_fresh(p)], _watch(p)))


# #981: the same borrowed bytes, one record later. A first MODIFY leaves a
# six-byte payload whose header claims two readings (30 bytes), so slot A's
# header-derived item spans the adjacent slot B. A second in-place MODIFY of A
# compares the 30 bytes and rewrites them with B's unigram changed; B has no
# record of its own. The replay view exposes only the slots it touched
# (`ReplayedLibrary::contains`), so a reader that falls back to the original
# item for B answers B's original unigram, while the pin's `get_phrase_item`
# reads the changed bytes. A is a one-character, one-reading slot so the fixed
# 30-byte header always spans B; B's item must hold those 14 bytes. In the
# pinned gb_char.bin the pair is slot 1 (token 0x01000001, 锕, unigram 3) and
# slot 2 (token 0x01000002, 吖, two readings, unigram 11).
def _cross_item_pair(p):
    """The first adjacent pair whose A is a one-character, one-reading item
    and whose B holds the 14 bytes A's (2, 2) header borrows."""
    ordered = sorted(p['offsets'].items(), key=lambda pair: pair[1])
    for (slot_a, offset_a), (slot_b, offset_b) in zip(ordered, ordered[1:]):
        item_a = p['items'][slot_a]
        item_b = p['items'][slot_b]
        if item_a[0] != 1 or item_a[1] != 1 or offset_b != offset_a + len(item_a):
            continue
        # The synthesized second phrase character is A's first pronunciation
        # key plus its frequency's low half; keep it a Unicode scalar so the
        # pin's `g_ucs4_to_utf8` has a value to render.
        second = int.from_bytes(item_a[10:14], 'little')
        if second > 0x10FFFF or 0xD800 <= second <= 0xDFFF or len(item_b) < 14:
            continue
        return (0x01000000 | slot_a, item_a, 0x01000000 | slot_b, item_b)
    raise AssertionError('no adjacent one-reading items the header can span')


def _cross_item_records(p):
    """Encode a 30-byte claimed A item whose second MODIFY changes B's overlapping unigram.

    Assert the borrowed extent fits B, choose 170 or 171 to differ from its
    original value, append a fresh ADD and watch all three tokens."""
    token_a, item_a, token_b, item_b = _cross_item_pair(p)
    expanded = bytes((2, 2)) + (200).to_bytes(4, 'little')
    claimed = _item_size(expanded[:2])
    borrowed = claimed - len(item_a)
    assert claimed == 30 and 0 < borrowed <= len(item_b)
    # The first in-place write copies only the six-byte payload, so the rest
    # of A's slot keeps its bytes and the read borrows B's first 14.
    old = expanded + item_a[6:] + item_b[:borrowed]
    assert len(old) == claimed
    at = len(item_a) + 2  # B's unigram, relative to A
    original = int.from_bytes(old[at:at + 4], 'little')
    changed = 170 if original != 170 else 171
    new = old[:at] + changed.to_bytes(4, 'little') + old[at + 4:]
    return ([_log_record(_LOG_MODIFY, token_a, old=item_a, new=expanded),
             _log_record(_LOG_MODIFY, token_a, old=old, new=new),
             _add_fresh(p)],
            {'a': token_a, 'b': token_b, 'fresh': p['fresh'][0]})


def _cross_item_records_past_area(p):
    """Encode an ADD followed by two A modifications claiming a 38-byte cross-item view.

    Assemble the comparison bytes from A, all of B and a fresh-item prefix;
    assert the resulting extent. Change the overlapping B unigram to 170 or
    171 and return A/B/fresh watches without replaying records."""
    token_a, item_a, token_b, item_b = _cross_item_pair(p)
    expanded = bytes((2, 3)) + (200).to_bytes(4, 'little')
    claimed = _item_size(expanded[:2])
    assert claimed == 38
    fresh = p['fresh'][1]
    borrowed = claimed - len(item_a)
    old = expanded + item_a[6:] + item_b + fresh[:borrowed - len(item_b)]
    assert len(old) == claimed
    at = len(item_a) + 2
    original = int.from_bytes(old[at:at + 4], 'little')
    changed = 170 if original != 170 else 171
    new = old[:at] + changed.to_bytes(4, 'little') + old[at + 4:]
    # The ADD comes first so the last original item B is followed by the fresh
    # item; A's 34-byte header reaches past the original area into it.
    return ([_add_fresh(p),
             _log_record(_LOG_MODIFY, token_a, old=item_a, new=expanded),
             _log_record(_LOG_MODIFY, token_a, old=old, new=new)],
            {'a': token_a, 'b': token_b, 'fresh': p['fresh'][0]})


def _observe_replayed_item(k, token):
    """Collect a replayed token's phrase, pronunciation keys, unigram and matching export rows.

    Render the token as eight hexadecimal digits; use 'NO PHRASE' when its
    phrase getter fails or yields empty text."""
    phrase = token_phrase_of(k, token)
    text = phrase[2] if phrase[0] else None
    return {'token': '0x%08x' % token,
            'phrase': phrase,
            'n-pron': _reload_pron_keys(k, token),
            'unigram': unigram_of(k, token),
            'export': remember_export(k, 1, text) if text else 'NO PHRASE'}


def _observe_cross_item(k):
    """Select the adjacent fixture pair from the original system bytes and observe both live tokens."""
    _total, items, offsets = _system_items(k)
    token_a, _item_a, token_b, _item_b = _cross_item_pair({'items': items, 'offsets': offsets})
    return {'a': _observe_replayed_item(k, token_a), 'b': _observe_replayed_item(k, token_b)}


_compare_case('cross-item-unigram-unrecorded', _cross_item_records,
              observe=_observe_cross_item, observe_saved=True, extra_issues=('981',))
_compare_case('cross-item-unigram-unrecorded-reopen', _cross_item_records, reopen=True,
              observe=_observe_cross_item, observe_saved=True, extra_issues=('981',))
_compare_case('cross-item-unigram-unrecorded-past-area', _cross_item_records_past_area,
              observe=_observe_cross_item, observe_saved=True, extra_issues=('981',))


# The pin does not check a new chunk's header against its size: the compare is
# on the old chunk only. A stored item whose header then names more bytes than
# were written is read past by a later `get_phrase_item` (class (b); row 107).
# The last item of the item area sits at the end of the pin's `m_phrase_content`
# copy once the trailing ADD has appended: a 132,096-byte header (255, 255)
# reads past the item, the copy and its allocation, and the pin dies of
# SIGSEGV at the first read. The subject keeps its safe answer: it applies the
# write and the ADD, and the over-read item is unreadable at the finish.
def _watch_last(p):
    """Extend standard watched tokens with the item at the highest content offset."""
    return dict(_watch(p), last=p['last'][0])


_compare_case('modify-header-overlong', lambda p: ([
    _log_record(_LOG_MODIFY, p['last'][0], old=p['last'][1],
                new=bytes((255, 255)) + (200).to_bytes(4, 'little')),
    _add_fresh(p)], _watch_last(p)),
    crash={'ctx': True, 'fresh': [[True, 1, '锕'], [True, 11]],
           'one': [[True, 1, '锕'], [True, 3]],
           'last': [[True, 14, '中华人民共和国中央军事委员会'], [True, 62]]})


# Same row-107 refusal after a duplicate touch. The pin still over-reads the
# last slot; the safe finish reports it once and keeps that slot's baseline.
_compare_case('touched-duplicate-invalid', lambda p: ([
    _log_record(_LOG_MODIFY, p['last'][0], old=p['last'][1],
                new=_item_with(p['last'][1], freq=77)),
    _log_record(_LOG_MODIFY, p['last'][0], old=_item_with(p['last'][1], freq=77),
                new=bytes((255, 255)) + (200).to_bytes(4, 'little')),
    _add_fresh(p)], _watch_last(p)),
    crash={'ctx': True, 'fresh': [[True, 1, '锕'], [True, 11]],
           'one': [[True, 1, '锕'], [True, 3]],
           'last': [[True, 14, '中华人民共和国中央军事委员会'], [True, 62]]})


# Many row-107 invalidations, in descending order and each after an earlier
# valid touch. Finalization must emit each diagnostic once in ascending order;
# membership and item reads fall back to the original for every rejected slot.
def touched_many_invalid(k):
    """Replay repeated invalidating writes to the last 256 system items in reverse offset order.

    For each selected slot, change its last frequency then write a six-byte
    header claiming 255 characters/readings. Append a fresh ADD and observe/save
    through system_replay with unigram increments disabled."""
    _, items, offsets = _system_items(k)
    slots = sorted(offsets, key=offsets.get)[-256:]

    def build(p):
        """Encode paired frequency and oversized-header modifications for the captured slots.

        Return those records plus a fresh ADD and the last-item watch mapping."""
        records = []
        for slot in reversed(slots):
            token, item = 0x01000000 | slot, items[slot]
            changed = _item_with(item, freq=77)
            records.extend([
                _log_record(_LOG_MODIFY, token, old=item, new=changed),
                _log_record(_LOG_MODIFY, token, old=changed,
                            new=bytes((255, 255)) + (200).to_bytes(4, 'little')),
            ])
        return records + [_add_fresh(p)], _watch_last(p)

    return system_replay(k, build, unigram_add=False)


for _mode in ('pinyin', 'zhuyin'):
    case('replay-touched-many-invalid-' + _mode, mode=_mode, stderr=True,
         issues=('850', '851', '870'),
         crash={'ctx': True, 'fresh': [[True, 1, '锕'], [True, 11]],
                'one': [[True, 1, '锕'], [True, 3]],
                'last': [[True, 14, '中华人民共和国中央军事委员会'], [True, 62]]})(
                    touched_many_invalid)


# An ADD payload shorter than the six bytes `add_phrase_item` reads its
# unigram from: `PhraseItem::get_unigram_frequency` (`phrase_index.h:124-126`)
# reads four bytes at offset 2 through `MemoryChunk::get_content<guint32>`
# (`memory_chunk.h:388-392`), whose `check_result` is an `assert` in the oracle
# build, so `pinyin_init`/`zhuyin_init` dies of SIGABRT (row 108). oxpinyin's
# open stops at the record and the C adapters abort (member 5a).
def _short_add(payload):
    """Build a replay case with a supplied ADD payload between a valid MODIFY and ADD."""
    def build(p):
        """Encode frequency 77 on one, the supplied fresh ADD payload, and a valid fresh ADD."""
        return ([
            _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                        new=_item_with(p['one'][1], freq=77)),
            _log_record(_LOG_ADD, p['fresh'][0], new=payload),
            _add_fresh(p)], _watch(p))

    def probe(k):
        """Replay the short-payload sequence without a unigram add and alias init truth as ret."""
        out = system_replay(k, build, unigram_add=False)
        out['ret'] = out['ctx']
        return out
    return probe


for _name, _payload in (('one-byte', b'\x01'), ('five-bytes', bytes((1, 1, 7, 0, 0)))):
    for _mode in ('pinyin', 'zhuyin'):
        _suffix = '-zhuyin' if _mode == 'zhuyin' else '-pinyin'
        case('replay-short-add-' + _name + _suffix,
             mode=_mode, abort=True, sigabrt=True, userdir=True, stderr=True,
             issues=('850', '870', 'row:108'))(_short_add(_payload))


# Row 91's replay order: `merge` returns at a mismatched REMOVE or MODIFY
# (`phrase_index.cpp:468-471`, `:483-484`) before `next_record` reads the
# non-null-token `MODIFY_HEADER` behind it, so init completes with the
# records before the stop applied (a REMOVE's removal included) and the
# assert is never reached.
_HEADER_TOKEN_RECORD = _modify_header(0x01000001, 1, 1)


def _stop_before_header_token(stop):
    """Build a replay: a valid MODIFY of `one`, the stopping record `stop(p)`,
    the bad header, then a fresh ADD the merge never reaches."""
    def build(p):
        """Encode frequency 77 on one, the stop, the bad header and an ADD."""
        return ([
            _log_record(_LOG_MODIFY, p['one'][0], old=p['one'][1],
                        new=_item_with(p['one'][1], freq=77)),
            stop(p),
            _HEADER_TOKEN_RECORD,
            _add_fresh(p)], dict(_watch(p), two=p['two'][0]))
    return lambda k: system_replay(k, build, unigram_add=False)


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else '-pinyin'
    case('replay-stop-before-header-token-remove' + _suffix, mode=_mode, issues=('row:91',))(
        _stop_before_header_token(
            lambda p: _log_record(_LOG_REMOVE, p['two'][0], old=_flip(p['two'][1]))))
    case('replay-stop-before-header-token-modify' + _suffix, mode=_mode, issues=('row:91',))(
        _stop_before_header_token(
            lambda p: _log_record(_LOG_MODIFY, p['two'][0], old=_flip(p['two'][1]),
                                  new=p['two'][1])))


# #871: the pronunciation counts of a replayed system item. A MODIFY record
# changes the item the sub-index holds in place (`phrase_index.cpp:477-501`);
# the item is then a list of (keys, frequency) entries in the file's order,
# and the pin trains (`phrase_index.cpp:114-143`, every entry in turn), merges
# an imported reading (`:122-139`) and diffs the list back out at save
# (`:392-436`) without ever keying the entries by their keys. Two entries with
# the same keys are two entries. The probe replays one journalled item
# (癫痫, one reading `dian4 xian4` at 100), then runs the item through the
# lifecycle a session sees: train it, import a reading, save, open the saved
# profile, train again, save and open once more. It reports the C ABI's view
# after each step and the bytes `gb_char.dbin` holds after each save. The
# export (pinyin only) lists one row per entry, so entries that were merged
# into one show there.
_PRON_TOKEN = 0x01003e57
_PRON_PHRASE = '癫痫'


def _pron_entries(item, entries):
    """`item` with its readings replaced by `entries`, each (keys, freq):
    the header's count and the unigram stay, the rest is rebuilt."""
    length = item[0]
    head = bytes((length, len(entries))) + item[2:6 + 4 * length]
    return head + b''.join(keys + freq.to_bytes(4, 'little') for keys, freq in entries)


def _pron_shape(entries, unigram=None):
    """Build an item transformer replacing readings and optionally its unigram.

    Each (keys, frequency) pair uses the original first reading's keys when keys
    is None. Preserve entry order and duplicates."""
    def new_item(item):
        """Rebuild captured pronunciation entries, substituting the item's first keys for None."""
        keys = item[6 + 4 * item[0]:6 + 4 * item[0] + 2 * item[0]]
        built = _pron_entries(item, [(keys if name is None else name, freq) for name, freq in entries])
        return _item_with(built, unigram=unigram) if unigram is not None else built
    return new_item


# Another reading's keys (two syllables, as the item's own), taken from a
# two-reading item of the same library: the pin does not check them.
_PRON_OTHER_KEYS = bytes.fromhex('81024702')


def pron_lifecycle(k, new_item_of, steps):
    """`steps` is `train`, `import` or `mask`: the lifecycle that follows the
    replay. `train` trains the item and imports a reading before the first
    save; `import` only imports (twice, the second time onto the reading the
    first one added, after a reopen); `mask` masks the library the journal
    changed before the import. Every variant saves and opens the saved
    profile at least once."""
    total, items, _offsets = _system_items(k)
    token = _PRON_TOKEN
    item = items[token & 0xFFFFFF]
    assert _item_text(item) == _PRON_PHRASE and item[1] == 1, item.hex()
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total) +
               _log_record(_LOG_MODIFY, token, old=item, new=new_item_of(item)))
    out = {'ctx': bool(k.ctx)}
    pinyin = k.mode == 'pinyin'
    parse = 'parse_more_full_pinyins' if pinyin else 'parse_more_chewings'
    typed = b'dian4xian4' if pinyin else b'2u04vu04'
    imported = "dian3'xian4" if pinyin else 'ㄉㄧㄢˇ ㄒㄧㄢˋ'

    def look(tag):
        """Store adjacent export rows, unigram and phrase outputs under the requested checkpoint tag."""
        out[tag] = {'export': remember_export(k, 1, _PRON_PHRASE, adjacent=True), 'unigram': unigram_of(k, token),
                    'phrase': token_phrase_of(k, token)}

    def train():
        """Assert complete parsing of 癫痫's facade input and train its selected token."""
        assert k.fn(parse, Z, P, S)(k.inst, typed) == len(typed)
        return train_selected_top(k, _PRON_PHRASE, token)

    def save(tag):
        """Record the save flag and complete gb_char.dbin bytes as hex under tag-derived keys."""
        out[tag + '-save'] = k.fn('save', B, P)(k.ctx)
        out[tag + '-dbin'] = Path(k.user, 'gb_char.dbin').read_bytes().hex()

    def reopen(tag):
        """Free/finalize cached objects, clear their pointers and observe the lazily reopened profile."""
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        look(tag)

    look('opened')
    if steps == 'unigram':
        add = k.fn('token_add_unigram_frequency', B, P, U, U)
        out['added-unigram'] = add(k.inst, token, 7)
        look('after-unigram')
        out['trained'] = train()
        look('after-train')
        save('first')
        reopen('reopened')
        out['added-again'] = add(k.inst, token, 5)
        look('after-unigram-again')
        # A unigram increment does not set m_modified. Dirty an unrelated
        # user library before the second save, preserving both live readings.
        out['dirty'] = import_rows(k, 7, [('你好', "ni3'hao3" if pinyin else 'ㄋㄧˇ ㄏㄠˇ', 1)])
        save('second')
        reopen('reopened-again')
        return out

    if steps == 'train':
        out['trained'] = train()
        look('after-train')
        out['imported'] = import_rows(k, 1, [(_PRON_PHRASE, imported, 50)])
        look('after-import')
        save('first')
        reopen('reopened')
        out['trained-again'] = train()
        look('after-train-again')
        save('second')
        reopen('reopened-again')
    elif steps == 'import':
        out['imported'] = import_rows(k, 1, [(_PRON_PHRASE, imported, 50)])
        look('after-import')
        save('first')
        reopen('reopened')
        out['imported-again'] = import_rows(k, 1, [(_PRON_PHRASE, imported, 7), (_PRON_PHRASE, "dian4'xian4" if pinyin else 'ㄉㄧㄢˋ ㄒㄧㄢˋ', 9)])
        look('after-import-again')
        save('second')
        reopen('reopened-again')
    else:
        assert steps == 'mask'
        # Library 1 as a whole: its journal is skipped when the libraries
        # are merged again, so the item is the shipped one.
        out['mask'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xFF000000, 0x01000000)
        look('after-mask')
        out['imported'] = import_rows(k, 1, [(_PRON_PHRASE, imported, 50)])
        look('after-import')
        save('first')
        reopen('reopened')
    return out


# Each shape is the item the MODIFY record leaves: (keys, frequency) entries,
# `None` standing for the item's own keys. The first two leave one reading;
# the dup- shapes carry the item's keys twice; `other` adds a second reading.
_PRON_SHAPES = {
    'raised': ([(None, 250)], None),
    'unigram-only': ([(None, 100)], 500),
    'lowered': ([(None, 40)], None),
    'dup-raised-last': ([(None, 100), (None, 160)], None),
    'dup-raised-first': ([(None, 160), (None, 100)], None),
    'dup-both': ([(None, 160), (None, 220)], None),
    'dup-equal': ([(None, 100), (None, 100)], 500),
    'other-reading': ([(None, 100), (_PRON_OTHER_KEYS, 130)], None),
    'other-first': ([(_PRON_OTHER_KEYS, 130), (None, 250)], None),
}
for _mode in ('pinyin', 'zhuyin'):
    case('replay-pron-unigram-combined-' + _mode, mode=_mode, stderr=True, issues=('871', '876', '870'))(
        lambda k: pron_lifecycle(k, _pron_shape([(None, 160), (None, 220)]), 'unigram'))

for _shape, (_entries, _unigram) in _PRON_SHAPES.items():
    for _steps in ('train', 'import', 'mask'):
        for _mode in ('pinyin', 'zhuyin'):
            case('replay-pron-' + _steps + '-' + _shape + '-' + _mode, mode=_mode, stderr=True, issues=('871', '876', '870'))(
                lambda k, e=_entries, u=_unigram, st=_steps: pron_lifecycle(k, _pron_shape(e, u), st))


# The same lifecycle over a profile with all of it at once: the journal
# changes 癫痫's reading and unigram and appends a fresh item to the library
# (its reading is seeded, the shipped item's is not), a user phrase is
# imported into library 7, and 癫痫 is trained, which also trains a gram.
# Each save is followed by a reopen; the second round trains again.
def pron_mixed(k):
    """Replay mixed system-item overrides and a fresh ADD, then train/import/save twice.

    Use the fixed 癫痫 token with last frequency 250 and low unigram byte plus
    400, a fresh item at max slot plus two, and a library-seven 龘龘 import.
    Record live exports/counts and complete gb_char.dbin/user.bin hex around
    both reopens; leave final native state cached."""
    total, items, _offsets = _system_items(k)
    token = _PRON_TOKEN
    item = items[token & 0xFFFFFF]
    one = next(slot for slot, it in items.items() if it[0] == 1 and it[1] == 1)
    fresh = 0x01000000 | (max(items) + 2)
    appended = _item_with(items[one], unigram=11)
    changed = _item_with(item, unigram=item[2] + 400, freq=250)
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total + 400 + 11) +
               _log_record(_LOG_MODIFY, token, old=item, new=changed) +
               _log_record(_LOG_ADD, fresh, new=appended))
    out = {'ctx': bool(k.ctx)}
    pinyin = k.mode == 'pinyin'
    parse = 'parse_more_full_pinyins' if pinyin else 'parse_more_chewings'
    typed = b'dian4xian4' if pinyin else b'2u04vu04'
    user_row = ('龘龘', "da2'da2" if pinyin else 'ㄉㄚˊ ㄉㄚˊ', 20)

    def look(tag):
        """Record system/user exports and unigram outputs for the overridden and appended items.

        Scan all rows for the appended item's shared phrase text."""
        out[tag] = {'export': remember_export(k, 1, _PRON_PHRASE, adjacent=True),
                    'user-export': remember_export(k, 7, user_row[0], adjacent=True),
                    'unigram': unigram_of(k, token), 'fresh': unigram_of(k, fresh),
                    'fresh-phrase': token_phrase_of(k, fresh),
                    # The appended item shares its text with slot `one`: walk every row.
                    'fresh-export': remember_export(k, 1, token_phrase_of(k, fresh)[2])}

    def train():
        """Assert complete facade parsing of 癫痫 and train its selected token."""
        assert k.fn(parse, Z, P, S)(k.inst, typed) == len(typed)
        return train_selected_top(k, _PRON_PHRASE, token)

    def save(tag):
        """Save and record complete system-journal and user-library bytes as hex."""
        out[tag + '-save'] = k.fn('save', B, P)(k.ctx)
        for name in ('gb_char.dbin', 'user.bin'):
            out[tag + '-' + name] = Path(k.user, name).read_bytes().hex()

    def reopen(tag):
        """Close cached native objects, clear pointers and record observations after lazy reopening."""
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = k._inst = None
        look(tag)

    look('opened')
    out['imported-user'] = import_rows(k, 7, [user_row])
    out['trained'] = train()
    look('after-train')
    out['imported'] = import_rows(k, 1, [(_PRON_PHRASE, "dian3'xian4" if pinyin else 'ㄉㄧㄢˇ ㄒㄧㄢˋ', 50)])
    look('after-import')
    save('first')
    reopen('reopened')
    out['trained-again'] = train()
    look('after-train-again')
    save('second')
    reopen('reopened-again')
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('replay-pron-mixed-' + _mode, mode=_mode, stderr=True, issues=('871', '876', '870'))(pron_mixed)


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
    """Remove a four-character system phrase by journal and observe its search paths.

    Choose a single-reading merged item whose key sequence prefixes no longer
    phrase in four system libraries. Obtain its rendered reading, close/reset
    caches, write a REMOVE log and reopen. labels selects exact, longer or
    prefix input; candidates optionally records lists, with before selecting
    zhuyin's before-cursor path. Free per-input instances but retain the context
    and lookup GArray. Missing fixture shapes raise through next/assertions."""
    total, items, _offsets = _system_items(k, 'merged.bin')

    def readings(item):
        """Slice raw pronunciation key sequences using the item's character count and entry stride.

        Follow PhraseItem::get_nth_pronunciation in src/storage/phrase_index.cpp:33-43 at
        pin 074a2219; do not validate slice lengths."""
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
    """Flatten exact-reading observations after removal, retaining candidate rows as diagnostic-only data.

    Delegate fixture setup and optional zhuyin before-cursor lookup to
    _removed_extension; omit its exact sentence field from the returned mapping."""
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
    }, issues=('row:102',))(_removed_candidate)
case('removed-candidate-before-cursor-zhuyin', mode='zhuyin', stderr=True, crash={
    'removed': [False, 0, None], 'exact-guessed': True, 'exact-candidates-ret': True,
}, issues=('row:102',))(lambda k: _removed_candidate(k, before=True))


# #1231: a system journal REMOVE leaves the phrase-table token, and the
# prediction walk (`_compute_predicted_prefix_candidates`) reads its item
# without checking the result (`pinyin.cpp:2389-2405` at 074a2219). The
# loop-wide `PhraseItem` keeps the previous success's length, so a missing
# token can pass or be skipped; an appended one carries a NULL phrase
# string. With a sole row both dedup comparisons are skipped and the NULL
# survives; with two or more rows `strcmp` reads it (row 106's class (b)).
# The fixture removes one merged.bin phrase whose prefix is a stored phrase
# and observes the plain prediction API. The stale case uses the reference
# cell's byte-lexical cursor order: a library-one long phrase is read first,
# raises the stale length past the bound, and the missing merged phrase is
# skipped without ever being appended. Other cells' hash order can put the
# missing row first, so those three cases are bdb only.
def _removed_predict_setup(k, phrase):
    """Remove one merged.bin phrase named by its rendered text and reopen.

    Find the slot whose item text is phrase, close the cached context and
    instance, write a REMOVE log for that token and reopen lazily. Return the
    removed token. The phrase table keeps the token."""
    total, items, _offsets = _system_items(k, 'merged.bin')
    slot = next(slot for slot, item in items.items() if _item_text(item) == phrase)
    token = 0x04000000 | slot
    k.fn('free_instance', None, P)(k.inst)
    k.fn('fini', None, P)(k.ctx)
    k._ctx = k._inst = None
    _write_user_conf(k)
    _write_log(k, _modify_header(0, total, total) +
               _log_record(_LOG_REMOVE, token, old=items[slot]), 'merged.dbin')
    return token


def _predicted_surface(k, inst, prefix):
    """Call the plain prediction API and return its flag, count and rows.

    Each row records every getter's flag, the type, and the string with NULL
    kept distinct from an empty string. Return the candidate pointers too, for
    the choose case; they are instance-borrowed and not part of the result."""
    ret = k.fn('guess_predicted_candidates', B, P, S)(inst, prefix.encode())
    count = U()
    got = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    pointers = []
    for index in range(count.value):
        cand, kind, string = P(), I(), S()
        fetched = k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
        typed = k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        rendered = k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(string))
        text = None if string.value is None else string.value.decode('utf-8', 'replace')
        rows.append([fetched, typed, kind.value, rendered, text])
        pointers.append(cand)
    return {'ret': ret, 'get-count': got, 'count': count.value, 'rows': rows}, pointers


def _predicted_sole(k):
    """Remove 不仅仅是 and predict from 不仅仅: the sole pre-dedup row.

    Record the sole row's getters, user status, and the token's failed phrase
    read with both outputs left unchanged."""
    token = _removed_predict_setup(k, '不仅仅是')
    inst = k.inst
    surface, pointers = _predicted_surface(k, inst, '不仅仅')
    length, text = U(UNTOUCHED), P(UNTOUCHED)
    phrase_ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
        inst, token, C.byref(length), C.byref(text))
    out = {'predicted': surface,
           'token-phrase': [phrase_ret, length.value == UNTOUCHED,
                            text.value == UNTOUCHED],
           'is-user': k.fn('is_user_candidate', B, P, P)(inst, pointers[0]) if pointers else None}
    return out


def _predicted_choose(k):
    """Choose the sole missing row, then read the token's frequency.

    The pin advances the loaded facade total by 483 and leaves the missing
    item unwritten; the later unigram read still fails with its seeded output
    unchanged. (The facade total has no C getter; the runtime's own
    amplified-total assertion covers that side effect.)"""
    token = _removed_predict_setup(k, '不仅仅是')
    inst = k.inst
    surface, pointers = _predicted_surface(k, inst, '不仅仅')
    before = unigram_of(k, token)
    chosen = k.fn('choose_predicted_candidate', B, P, P)(inst, pointers[0]) if pointers else None
    after = unigram_of(k, token)
    return {'predicted': surface, 'choose': chosen,
            'unigram-before': before, 'unigram-after': after}


def _predicted_class_b(k):
    """Remove 这样一个 and predict from 这样一: the missing row and a live row.

    The pin reaches the NULL phrase string in dedup (SIGSEGV); the subject
    drops the missing row and keeps the live prefix candidate."""
    _removed_predict_setup(k, '这样一个')
    surface, _pointers = _predicted_surface(k, k.inst, '这样一')
    return {'ret': surface['ret'], 'count': surface['count'], 'rows': surface['rows']}


def _predicted_stale_skip(k):
    """Remove 明显的 and predict from 明显 in reference-cell cursor order.

    gb_char's 明显的句法线索 (length 7) is read first and is skipped by the
    bound; the failed 明显的 read keeps that length, so it is skipped too.
    The subject drops both; the parent would keep the fallback text."""
    _removed_predict_setup(k, '明显的')
    surface, _pointers = _predicted_surface(k, k.inst, '明显')
    return {'ret': surface['ret'], 'count': surface['count'], 'rows': surface['rows']}


case('removed-predict-sole-pinyin', stderr=True, issues=('1231',))(_predicted_sole)
case('removed-predict-choose-pinyin', stderr=True, issues=('1231',))(_predicted_choose)
case('removed-predict-class-b-pinyin', stderr=True, crash={'ret': True, 'count': 1},
     issues=('1231',))(_predicted_class_b)
case('removed-predict-stale-skip-pinyin', stderr=True, cells=('bdb',),
     issues=('1231',))(_predicted_stale_skip)


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
