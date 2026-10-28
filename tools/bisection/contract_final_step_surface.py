"""Final step surface cases (inventory row 43, #1175).
"""
import ctypes as C

from contract_abi import B, I, P, S, U, Z, final_step_sentences
from contract_case_helpers import guess_rows, zhuyin_guess_rows


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def final_step_surface(text, options, imported=False):
    """Build a complete sentence/candidate observation with optional user phrase 罢跨."""
    def probe(k):
        """Apply options, optionally import 罢跨, then parse, guess and collect available sentence ranks.

        Pinyin records six sort-word lists and each displayed NBEST row's original
        rank; zhuyin records one after-cursor list. Assert setup and rank getters;
        leave the cached native objects live."""
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
                 mode=_mode, control=True, issues=('594',))(final_step_surface(_text, _options))
    case('final-step-%s-import-ba-kua' % _mode, mode=_mode, control=True, issues=('594',))(
        final_step_surface("ba'kua", 1 << 5, imported=True))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
