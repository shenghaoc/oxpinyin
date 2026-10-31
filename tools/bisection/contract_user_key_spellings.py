"""User readings whose key is not a typed-input spelling (#1418).
"""
import ctypes as C
from pathlib import Path

from contract_abi import B, I, P, S, U, UNTOUCHED, Z, tokens_of, unigram_of
from contract_case_helpers import _chunk, _reload_pron_keys, _ucs4_key, phrase_text_bytes
from contract_fixtures import import_rows

LIBRARY = 7

# 074a2219 pinyin_custom2.h:34,36,79: PINYIN_INCOMPLETE, USE_TONE, PINYIN_CORRECT_ALL.
PINYIN_LOOKUP_OPTIONS = (1 << 3) | (1 << 5) | (0xFF << 21)

# (case tag, mode, phrase, import reading, typed lookup, expected key spelling).
# libpinyin's import parses under PINYIN_CORRECT_ALL | USE_TONE
# (pinyin.cpp:630-638), so `egn` and `emg` resolve to content row 88, `eng`,
# and `nuen` to row 248, `nun` (pinyin_parser_table.h:132,134,375).
# libzhuyin's import parses `ㄉㄣ` to row 68, `den`, and `ㄥ` to row 88,
# `eng` (zhuyin.cpp:515-523; pinyin_parser_table.h:1591,2518); a missing
# tone symbol defaults to the first tone (zhuyin_parser2.cpp:870).
READINGS = [
    ('egn', 'pinyin', '嗯', 'egn', b'egn', 'eng'),
    ('emg', 'pinyin', '嗯', 'emg', b'emg', 'eng'),
    ('nuen', 'pinyin', '嫩', 'nuen', b'nuen', 'nun'),
    ('den', 'zhuyin', '扽', 'ㄉㄣ', 'ㄉㄣ'.encode(), 'den'),
    ('den-tone4', 'zhuyin', '扽', 'ㄉㄣˋ', 'ㄉㄣˋ'.encode(), 'den'),
    ('eng', 'zhuyin', '嗯', 'ㄥ', 'ㄥ'.encode(), 'eng'),
]


def register(case):
    """Register these cases at their driver position."""
    def user_key_spelling(mode, phrase, reading, typed):
        """Import one reading, save, reopen and observe the stored key.

        The observation covers the token's stored key words, its unigram, the
        pinyin export of the library, and the candidates a typed lookup of the
        same reading reaches. The same observation is taken before the save and
        after the reopen; the pin keeps the reading, so the two sides must agree."""
        def observe(k):
            tokens = [token for token in tokens_of(k, phrase) if token >> 24 == LIBRARY]
            result = {'tokens': tokens,
                      'readings': [_reload_pron_keys(k, token) for token in tokens],
                      'unigrams': [unigram_of(k, token) for token in tokens]}
            if mode == 'pinyin':
                iterator = k.fn('begin_get_phrases', P, P, U)(k.ctx, LIBRARY)
                rows = []
                while k.fn('iterator_has_next_phrase', B, P)(iterator):
                    assert len(rows) < 4, 'bounded fixture export'
                    text, spelling, count = P(UNTOUCHED), P(UNTOUCHED), I(UNTOUCHED)
                    got = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
                        iterator, C.byref(text), C.byref(spelling), C.byref(count))
                    rows.append([got, phrase_text_bytes(k, text.value),
                                 phrase_text_bytes(k, spelling.value), count.value])
                k.fn('end_get_phrases', None, P)(iterator)
                result['export'] = rows
                assert k.fn('set_options', B, P, U)(k.ctx, PINYIN_LOOKUP_OPTIONS)
                result['parsed'] = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, typed)
                result['listed'] = k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0x1e)
            else:
                result['parsed'] = k.fn('parse_more_chewings', Z, P, S)(k.inst, typed)
                result['listed'] = k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, 0)
            count = U(UNTOUCHED)
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(count))
            rows = []
            for index in range(count.value):
                candidate, text, kind = P(), P(UNTOUCHED), I(UNTOUCHED)
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, index, C.byref(candidate))
                assert k.fn('get_candidate_string', B, P, P, C.POINTER(P))(k.inst, candidate, C.byref(text))
                assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(k.inst, candidate, C.byref(kind))
                found = phrase_text_bytes(k, text.value, owned=False)
                if found == phrase.encode().hex():
                    rows.append([index, kind.value, found])
            result['candidates'] = rows
            return result

        def probe(k):
            assert import_rows(k, LIBRARY, [(phrase, reading, 5)]) == [True]
            out = {'imported': observe(k)}
            out['saved'] = k.fn('save', B, P)(k.ctx)
            # Compare the complete chunk bytes, including the compacted
            # offsets and packed key words (074a2219 phrase_index.cpp:365-390,
            # :648-675; pinyin.cpp:978-993, :1139).
            out['saved_chunk'] = Path(k.user, 'user.bin').read_bytes().hex()
            k.fn('free_instance', None, P)(k.inst)
            k.fn('fini', None, P)(k.ctx)
            k._ctx = k._inst = None
            out['reopened'] = observe(k)
            return out
        return probe

    for tag, mode, phrase, reading, typed, _spelling in READINGS:
        case(f'user-key-spelling/{mode}-{tag}', mode=mode, issues=('1418',))(
            user_key_spelling(mode, phrase, reading, typed))

    def user_stored_key(mode, variant):
        """Replace one stored key word in user.bin, reopen and read it back.

        The pin's import writes only parser keys, so these words come from a
        crafted file. SubPhraseIndex::load checks no key (074a2219
        phrase_index.cpp:335-362) and get_nth_pronunciation copies the item's
        words verbatim (phrase_index.cpp:33-50, pinyin.cpp:2801-2818,
        zhuyin.cpp:1793-1810). Only the token getters are observed: export and
        lookup would render or price the word."""
        def probe(k):
            reading = 'ni3' if mode == 'pinyin' else 'ㄋㄧˇ'
            assert import_rows(k, LIBRARY, [('你', reading, 5)]) == [True]
            token = next(token for token in tokens_of(k, '你') if token >> 24 == LIBRARY)
            out = {'imported': _reload_pron_keys(k, token)}
            assert k.fn('save', B, P)(k.ctx)
            k.fn('free_instance', None, P)(k.inst)
            k.fn('fini', None, P)(k.ctx)
            k._ctx = k._inst = None
            path = Path(k.user, 'user.bin')
            payload = bytearray(path.read_bytes()[8:])
            text = _ucs4_key('你')
            assert payload.count(text) == 1, 'one imported item to patch'
            # The item's first reading follows its text (phrase_index.h:56,139-144).
            at = payload.index(text) + len(text)
            word = int.from_bytes(payload[at:at + 2], 'little')
            assert out['imported'][2] == payload[at:at + 2].hex()
            # ChewingKey bits: initial 0..5, middle 5..7, final 7..12, tone 12..15.
            if variant == 'tone6':
                word = (word & ~0x7000) | (6 << 12)
            elif variant == '0x8000':
                # The padding bit alone: no syllable and no tone field.
                word = 0x8000
            else:
                word = (word & 0x7000) | 0x1f
            payload[at:at + 2] = word.to_bytes(2, 'little')
            path.write_bytes(_chunk(bytes(payload)))
            out['stored'] = word.to_bytes(2, 'little').hex()
            length, phrase = U(UNTOUCHED), P(UNTOUCHED)
            ok = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
                k.inst, token, C.byref(length), C.byref(phrase))
            out['phrase'] = [ok, length.value, phrase_text_bytes(k, phrase.value)]
            out['reopened'] = _reload_pron_keys(k, token)
            return out
        return probe

    for _mode in ('pinyin', 'zhuyin'):
        for _variant in ('tone6', 'no-syllable', '0x8000'):
            case(f'user-stored-key/{_mode}-{_variant}', mode=_mode, issues=('1418',))(
                user_stored_key(_mode, _variant))
