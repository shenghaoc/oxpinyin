"""Missing system DBM attachments (#1516), derived from pin 074a2219.

Pinyin ignores six attaches (pinyin.cpp:357,369,396,421,429,440);
zhuyin opens only the main indexes and bigram (zhuyin.cpp:301,312,338).
Each case removes one link from its own system directory. The ordinary
worker pool keeps cases independent and parallel. KC and tkrzw retain the
failed native object: KC since 553866c, tkrzw since the concrete-class
binding (the pin's TreeDBM or HashDBM answers PRECONDITION_ERROR and writes
nothing to stderr). BDB closes the failed handle, as Berkeley DB requires
after a failed DB->open, so the pin's later calls on it and libdb's BDB1565
lines have no counterpart (compatibility register, Berkeley DB failed
handles).
"""
import ctypes as C
from pathlib import Path

from contract_abi import B, I, P, S, U, Z, sentence_out, tokens_of, zhuyin_sentence_out
from contract_case_helpers import candidate_rows
from contract_fixtures import _write_user_conf, private_system


# The tables each facade attaches (pinyin.cpp:357,369,396,421,429,440;
# zhuyin.cpp:301,312,338). Only these reach a failed BDB handle.
ATTACHED = {
    'pinyin': ('pinyin', 'phrase', 'bigram', 'punctuation', 'addon-pinyin', 'addon-phrase'),
    'zhuyin': ('pinyin', 'phrase', 'bigram'),
}

TABLES = {
    'pinyin': 'pinyin_index.bin',
    'phrase': 'phrase_index.bin',
    'bigram': 'bigram.db',
    'punctuation': 'punct.bin',
    'addon-pinyin': 'addon_pinyin_index.bin',
    'addon-phrase': 'addon_phrase_index.bin',
}


def exercise(k):
    """Exercise read, prediction, learning and save without assuming a hit.

    Search and suggestion fail before record parsing on every backend.
    Bigram misses preserve unigram decoding; training follows a fresh
    guess_sentence, as Session::train_top does, and writes only user data
    (phonetic_lookup.h:878-904). Save never visits the failed system DBMs.
    """
    ctx, inst = k.ctx, k.inst
    k.fn('set_options', B, P, U)(ctx, (1 << 5) | (1 << 9))  # USE_TONE | DYNAMIC_ADJUST
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni3hao3')
    guessed = k.fn('guess_sentence', B, P)(inst)
    sentence = sentence_out(k, inst, 0) if k.mode == 'pinyin' else zhuyin_sentence_out(k, inst)
    tokens = tokens_of(k, '你好')
    segmented = k.fn('phrase_segment', B, P, S)(inst, '你好'.encode())
    if k.mode == 'pinyin':
        candidates = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1C)
    else:
        candidates = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    rows = candidate_rows(k, inst)
    out = dict(parsed=parsed, guessed=guessed, sentence=sentence,
               tokens=tokens, segmented=segmented, candidates=candidates, rows=rows)
    length = Z(99)
    out['character_offset'] = [k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
        inst, '你好'.encode(), 0, C.byref(length)), length.value]
    out['prefix_guess'] = k.fn('guess_sentence_with_prefix', B, P, S)(inst, '你好'.encode())
    if k.mode == 'pinyin':
        out['predicted'] = k.fn('guess_predicted_candidates', B, P, S)(inst, '我们'.encode())
        out['predicted_rows'] = candidate_rows(k, inst)
        out['punctuation'] = k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, '我们'.encode())
        out['punctuation_rows'] = candidate_rows(k, inst)
    # Training consumes n-best results, never the candidate list.
    out['train_guess'] = k.fn('guess_sentence', B, P)(inst)
    out['trained'] = (k.fn('train', B, P, C.c_ubyte)(inst, 0) if k.mode == 'pinyin'
                      else k.fn('train', B, P)(inst))
    out['saved'] = k.fn('save', B, P)(ctx)
    return out


def missing_case(table, operations, overlay=False, unload=False):
    """Remove exactly one system DBM, initialize and explicitly finalize."""
    def probe(k):
        k.data = private_system(k)
        if table is not None:
            Path(k.data, TABLES[table]).unlink()
        _write_user_conf(k)
        k._ctx = k.init()
        out = {'init': bool(k._ctx)}
        if not k._ctx:
            return out
        if unload:
            out['unloaded'] = k.fn('unload_phrase_library', B, P, C.c_ubyte)(k.ctx, 2)
        if overlay:
            # _add_phrase first searches the phrase index, then writes only
            # the user indexes (pinyin.cpp:540,637-649; zhuyin.cpp:426,516-528).
            iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
            reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
            out['added'] = k.fn('iterator_add_phrase', B, P, S, S, I)(
                iterator, '你好'.encode(), reading, 100000)
            k.fn('end_add_phrases', None, P)(iterator)
        if operations:
            out.update(exercise(k))
        if k._inst:
            k.fn('free_instance', None, P)(k._inst)
            k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        out['fini'] = True
        return out
    return probe


# The bdb pin's later calls on its failed handle print libdb's BDB1565 lines,
# which the subject does not make (compatibility register, row 113).
RECORD = 'bdb-failed-handle.md'


def register(case):
    """Register both facades, isolated missing files, and intact controls.

    A missing table the facade attaches runs on kc and tkrzw under its
    name, and on bdb as a `bdb-` twin that names the record of the BDB1565
    difference. A table the facade never attaches runs on every cell.
    """
    def missing(name, facade, table, probe):
        if table not in ATTACHED[facade]:
            case(f'missing-system-dbm/{name}', mode=facade,
                 stderr=True, userdir=True, issues=('1516',))(probe)
            return
        case(f'missing-system-dbm/{name}', mode=facade, cells=('kc', 'tkrzw'),
             stderr=True, userdir=True, issues=('1516',))(probe)
        case(f'missing-system-dbm/bdb-{name}', mode=facade, cells=('bdb',), policy_record=RECORD,
             stderr=True, userdir=True, issues=('1516',))(probe)

    # A lifecycle case is the clean init and fini with no operations. It is not an `operations` case
    # without the exercise: the exercise trains and saves, so the lifecycle leaves one file in the user
    # directory where the operations case leaves eleven, in the frozen rows. Every lifecycle result is
    # the same, since the attach of a missing table is ignored, so each facade keeps one, on a different
    # kind of table: libpinyin's bigram and libzhuyin's phrase index. A table libzhuyin never attaches
    # (`zhuyin.cpp:301,312,338` open three) is never opened, so removing it leaves `zhuyin-intact`'s
    # outcome in every frozen row; one overlay stays, the import that `zhuyin-intact` does not make.
    LIFECYCLE_KEPT = (('pinyin', 'bigram'), ('zhuyin', 'phrase'))
    for facade in ('pinyin', 'zhuyin'):
        for table in TABLES:
            unattached = table not in ATTACHED[facade]
            if (facade, table) in LIFECYCLE_KEPT:
                missing(f'{facade}-{table}-lifecycle', facade, table, missing_case(table, False))
            if not unattached:
                missing(f'{facade}-{table}-operations', facade, table, missing_case(table, True))
            if not unattached or table == 'punctuation':
                missing(f'{facade}-{table}-overlay', facade, table,
                        missing_case(table, True, overlay=True))
        case(f'missing-system-dbm/{facade}-intact', mode=facade, control=True,
             stderr=True, userdir=True, issues=('1516',))(missing_case(None, True))
        missing(f'{facade}-pinyin-unloaded', facade, 'pinyin',
                missing_case('pinyin', True, unload=True))
