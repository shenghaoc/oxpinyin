"""Library reload cases (inventory row 55, #1175).
"""
import ctypes as C

from contract_abi import B, I, P, S, U, Z, sentence_out, tokens_of, unigram_of
from contract_case_helpers import (
    FACADE_TOTAL, _facade_total_out, _reload_pron_keys, _stored_library_total, guess_rows, train_selected_top)
from contract_fixtures import import_rows


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


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
    """Collect token, unigram, reading and candidate observations for a reload step.

    Clear the cached instance before querying; per-token keys include hexadecimal
    token IDs and candidate keys use decoded input bytes.
    """
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
    """Build a before/change/reload probe with an optional save before unloading.

    change(k, False) supplies phrases and inputs; change(k, True) mutates the
    session. The system-library reload path reads the saved log in
    _load_phrase_library at 074a2219 pinyin.cpp:264-277 and zhuyin.cpp:208-221.
    """
    def probe(k):
        """Snapshot the session around a change and a library unload/load pair.

        Record save, unload and load results without requiring success; clear the
        cached instance between library calls and return all three snapshots.
        """
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
    """Build a reload change that selects and trains a phrase in one library.

    The returned callback reports the watched phrase and input when apply is
    false; applying it requires the entire reading to parse and a matching token.
    """
    def change(k, apply):
        """Return the watched inputs, or parse the reading and train the selected token."""
        if not apply:
            return [phrase], [reading]
        parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
        assert k.fn(parse, Z, P, S)(k.inst, reading) == len(reading)
        token = [t for t in tokens_of(k, phrase) if t >> 24 == token_library][0]
        train_selected_top(k, phrase, token)
    return change


def _reload_import(library, rows, phrases, inputs):
    """Build a reload change that imports rows and exposes its watched inputs."""
    def change(k, apply):
        """Return the supplied watch lists, or import rows into the requested library."""
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
            case('reload-refused-%s' % _tag, mode=_mode, control=True, issues=('805',))(_reload_probe(_change, _library, False))
            continue
        case('reload-drops-session-%s' % _tag, mode=_mode, issues=('805',))(_reload_probe(_change, _library, False))
        case('reload-keeps-saved-%s' % _tag, mode=_mode, control=True, issues=('805',))(_reload_probe(_change, _library, True))


# Library 7 is the USER_FILE library: `unload` deletes its sub-index and the
# load builds it again from the saved `user.bin` chunk, or an empty sub-index
# when none was saved (zhuyin.cpp:247-265; pinyin twin pinyin.cpp:303-321).
# The pinyin facade never unloads it (pinyin.cpp:470-471). A dropped item's
# token stays in the phrase table, so no input is walked for it (class (b),
# policy row 102).
def _reload_user(setup, change, phrases, inputs, save_setup, save_change):
    """Build a user-library reload probe with independent setup/change saves.

    setup and change are import rows for library 7. Snapshots follow the supplied
    phrases and inputs. The USER_FILE load path reads user.bin or creates an
    empty sub-index at 074a2219 zhuyin.cpp:247-265 and pinyin.cpp:303-321.
    """
    def probe(k):
        """Import the optional baseline, change it, and record user-library reload state.

        Require the setup save to succeed when requested; record the later save and
        unload/load results, clearing cached instances before fresh snapshots.
        """
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
            case('reload-refused-user-%s' % _tag, mode=_mode, control=True, issues=('805',))(_probe)
        else:
            case('reload-user-%s' % _tag, mode=_mode, control=_control, issues=('805',))(_probe)


# After the reload the phrase table and the user pinyin index still hold the
# dropped token, and an import takes `get_range(7).m_range_end` as its token
# (zhuyin.cpp:472-478): a different phrase gets the dropped token again, and
# the kept index entry for the old reading now finds it; the same phrase meets
# its own token in the phrase table (zhuyin.cpp:447-468).
def _reload_reuse_setup(k):
    """Import an unsaved zhuyin user phrase, then unload and reload its library.

    Return import and reload results with the cached instance cleared, leaving
    the fixture ready for a second import.
    """
    out = {'first': import_rows(k, 7, [('好你', 'ㄏㄠˇ ㄋㄧˇ', 50)])}
    out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 7)
    k._inst = None
    out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, 7)
    k._inst = None
    return out


@case('reload-user-zhuyin-reuse-other', mode='zhuyin', issues=('805',))
def _(k):
    """Import a different phrase after user-library reload and inspect both readings.

    Exercise token reuse through _add_phrase at 074a2219 zhuyin.cpp:470-485;
    return both import results and the resulting phrase/candidate snapshot.
    """
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


@case('reload-user-zhuyin-reuse-other-save-reopen', mode='zhuyin', issues=('805',))
def _(k):
    """Save and reopen a second phrase imported after user-library reload.

    Observe both old and new readings after closing the context, so the snapshot
    includes persisted index entries rather than only the live session.
    """
    out = _reload_reuse_setup(k)
    out['again'] = import_rows(k, 7, [('你好', 'ㄋㄧˇ ㄏㄠˇ', 50)])
    out['save'] = bool(k.fn('save', B, P)(k.ctx))
    k._inst = None
    k.fn('fini', None, P)(k.ctx)
    k._ctx = None
    out['after'] = _reload_snapshot(k, ['好你', '你好'], _REUSE_INPUTS[:2])
    return out


@case('reload-user-zhuyin-reuse-other-mask', mode='zhuyin', issues=('805',))
def _(k):
    """Mask the reused user-library range before importing a third phrase.

    Return the import and mask results plus snapshots of all three phrases and
    readings; mask 0xFF000000 selects library 7.
    """
    out = _reload_reuse_setup(k)
    out['again'] = import_rows(k, 7, [('你好', 'ㄋㄧˇ ㄏㄠˇ', 50)])
    out['mask'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xFF000000, 0x07000000)
    k._inst = None
    out['third'] = import_rows(k, 7, [('好好', 'ㄏㄠˇ ㄏㄠˇ', 50)])
    out['after'] = _reload_snapshot(k, ['好你', '你好', '好好'], _REUSE_INPUTS)
    return out


# The same phrase meets its own token in the phrase table, `get_phrase_item`
# finds no item, and `assert(phrase_length == item.get_phrase_length())`
# (zhuyin.cpp:454) aborts the pin, and oxpinyin aborts there too (row 104;
# class (c) retired 2026-10-31 UTC), as at the iterator's `:440` and `:457`.
@case('reload-user-zhuyin-reuse-same', mode='zhuyin', abort=False, sigabrt=True, issues=('805',))
def _(k):
    """Re-import the same phrase after dropping its unsaved user-library item.

    Return the add result at the missing-item assertion site in _add_phrase,
    074a2219 zhuyin.cpp:447-457; the iterator remains open for worker teardown.
    """
    _reload_reuse_setup(k)
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(
        it, '好你'.encode(), 'ㄏㄠˇ ㄋㄧˇ'.encode(), 50)}


# The dangling token: `lookup_tokens` keeps answering it, the item reads
# answer false, and a candidate walk over it is the pin's SIGSEGV
# (class (b)); the subject adds no candidate for it, as for a missing item.
def _reload_dangling(walk, reading):
    """Build a candidate walk over an imported token after its library reload.

    The import goes into library 2 without saving; report retained token lookup,
    item reads and candidates after unloading and loading that library.
    """
    def probe(k):
        """Import a watched phrase, reload library 2, and inspect its token and candidates.

        Require the import to expose a library-2 token; clear cached instances between
        library changes before reading the same token again.
        """
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
case('reload-dangling-token-candidates', crash=_RELOAD_DANGLING, issues=('805',))(_reload_dangling(b'dada', "da3'da3"))
case('reload-dangling-token-candidates-zhuyin', mode='zhuyin',
     crash=_RELOAD_DANGLING, issues=('805',))(_reload_dangling(b'283283', 'ㄉㄚˇ ㄉㄚˇ'))


# `unload` takes the library's total out of the facade, an unsaved import's
# mass included (phrase_index.cpp:261-268): while library 2 is gone, the
# sentence and candidate order divide by the smaller total, and the reload
# puts the saved state's share back.
def _reload_total_window(k):
    """Record sentence and candidate order across import, unload and reload.

    Add two large-frequency library-2 items before unloading. The total removed
    by FacadePhraseIndex::unload includes that library's current mass
    (074a2219 phrase_index.cpp:261-268).
    """
    out = {}

    def look(tag):
        """Append four fresh-instance sentence and candidate snapshots under tag.

        Keep only the first twelve candidates and free each temporary instance.
        """
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


case('reload-total-while-unloaded', issues=('805',))(_reload_total_window)


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
    """Build a mask-out probe with or without compaction after a saved-log reload.

    Use an imported count chosen to expose the reload offset in the facade total;
    the relevant merge and replacement paths are SubPhraseIndex::merge and
    FacadePhraseIndex::merge_with_mask at 074a2219 phrase_index.cpp:453-459,
    phrase_index.cpp:503-514 and phrase_index.cpp:301-332.
    """
    def probe(k):
        """Save and reopen a large import, optionally compact, then mask its exact token.

        Assert the chosen count fits the stored library total; report the import,
        save, optional compaction-triggering import, mask and facade observations.
        """
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


case('mask-out-replaces-reload-offset', control=True, issues=('805',))(mask_out_reload(False))
case('mask-out-after-compact-keeps-reload-offset', issues=('805',))(mask_out_reload(True))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
