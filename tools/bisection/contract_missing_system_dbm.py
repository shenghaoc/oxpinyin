"""Missing system DBM attachments (#1516), derived from pin 074a2219.

Pinyin ignores six attaches (pinyin.cpp:357,369,396,421,429,440);
zhuyin opens only the main indexes and bigram (zhuyin.cpp:301,312,338).
Each case removes one link from its own system directory. The ordinary
worker pool keeps cases independent and parallel. Every cell retains the
failed native object: BDB and KC since 553866c, tkrzw since the
concrete-class binding (the pin's TreeDBM or HashDBM answers
PRECONDITION_ERROR and writes nothing to stderr).
"""
import ctypes as C
from pathlib import Path

from contract_abi import B, I, P, S, U, Z, sentence_out, tokens_of, zhuyin_sentence_out
from contract_case_helpers import candidate_rows
from contract_fixtures import _write_user_conf, private_system


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


def register(case):
    """Register both facades, isolated missing files, and intact controls."""
    for facade in ('pinyin', 'zhuyin'):
        for table in TABLES:
            for operation in ('lifecycle', 'operations'):
                case(f'missing-system-dbm/{facade}-{table}-{operation}', mode=facade,
                     stderr=True, userdir=True, issues=('1516',))(
                         missing_case(table, operation == 'operations'))
            case(f'missing-system-dbm/{facade}-{table}-overlay', mode=facade,
                 stderr=True, userdir=True, issues=('1516',))(
                     missing_case(table, True, overlay=True))
        case(f'missing-system-dbm/{facade}-intact', mode=facade, control=True,
             stderr=True, userdir=True, issues=('1516',))(missing_case(None, True))
        case(f'missing-system-dbm/{facade}-pinyin-unloaded', mode=facade,
             stderr=True, userdir=True, issues=('1516',))(
                 missing_case('pinyin', True, unload=True))
