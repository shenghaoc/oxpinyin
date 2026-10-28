"""Prefix conversion cases (inventory row 47, #1175).
"""
import ctypes as C
from pathlib import Path

from contract_abi import B, I, P, S, U, UNTOUCHED, Z
from contract_case_helpers import PREFIX_WORDS, d18_bigram_db, d18_gram, glib_utf8, phrase_text_bytes


def register(case, *, items_system):
    """Register these cases at their original driver position."""
    def prefix_system(k, word):
        """The private system and bigram above, for the middle word `word`."""
        prefix_text = (0x7532, word, 0x4e59)
        items_system(k, [(prefix_text, (0x1084,) * 3, 100), ((0x4e19,), (0x1081,), 1000),
                         ((0x620c,), (0x1081,), 10), (prefix_text + (0x4e19,), (0x1084,) * 3 + (0x1081,), 100)],
                     total=2000)
        path = Path(k.data, 'bigram.db')
        path.unlink()
        d18_bigram_db(k, path, [(0x01000001.to_bytes(4, 'little'), d18_gram(1000, 0x01000003, 1000))])


    def prefix_argument(variant):
        """The prefix bytes for `variant`, or None for the NULL case."""
        if variant == 'null':
            return None
        if variant == 'invalid':
            return '甲'.encode() + b'\xff' + '乙'.encode()
        return glib_utf8((0x7532, PREFIX_WORDS[variant], 0x4e59))


    def prefix_prediction_case(entry, variant):
        """Fill the candidate list, then predict after the prefix of `variant`.

        Record the answer and every row's type and bytes."""
        def probe(k):
            """Build the system, list the ba1 candidates, then call the entry."""
            prefix_system(k, PREFIX_WORDS.get(variant, PREFIX_WORDS['valid']))
            inst = k.inst
            assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ba1')
            k.fn('guess_sentence', B, P)(inst)
            k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
            ret = k.fn(entry, B, P, S)(inst, prefix_argument(variant))
            count = U(UNTOUCHED)
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            rows = []
            for index in range(count.value):
                candidate, text, kind = P(), P(UNTOUCHED), I(UNTOUCHED)
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(candidate))
                k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, candidate, C.byref(text))
                k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, candidate, C.byref(kind))
                rows.append([kind.value, phrase_text_bytes(k, text.value, owned=False)])
            return {'ret': ret, 'rows': rows}
        return probe


    def prefix_sentence_case(variant):
        """Guess ba1 with or without a converted prefix, then with `variant`'s.

        A converted non-scalar prefix seeds 戌. For NULL and invalid input the
        first guess is seeded with the valid prefix, so the no-prefix answer 丙
        shows the sentence was guessed again. Record the answer and sentence."""
        def probe(k):
            """Build the system, guess once, then guess with the prefix."""
            prefix_system(k, PREFIX_WORDS.get(variant, PREFIX_WORDS['valid']))
            inst = k.inst
            assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ba1')
            if variant in ('null', 'invalid'):
                seeded = k.fn('guess_sentence_with_prefix', B, P, S)(inst, prefix_argument('valid'))
            else:
                seeded = k.fn('guess_sentence', B, P)(inst)
            ret = k.fn('guess_sentence_with_prefix', B, P, S)(inst, prefix_argument(variant))
            sentence = P(UNTOUCHED)
            if k.mode == 'pinyin':
                read = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(inst, 0, C.byref(sentence))
            else:
                read = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(sentence))
            return {'seeded': seeded, 'ret': ret, 'sentence': [read, phrase_text_bytes(k, sentence.value)]}
        return probe


    # 丙 and 戌 as the C strings the getters return.
    _BING, _XU = '丙'.encode().hex(), '戌'.encode().hex()
    for _variant in ('valid', 'invalid', 'surrogate', 'above-unicode', 'five-byte', 'six-byte', 'null'):
        for _entry, _null in (('guess_predicted_candidates', {'ret': False, 'rows': []}),
                              ('guess_predicted_candidates_with_punctuations', {'ret': True, 'rows': []})):
            _name = 'prefix-convert/%s-%s' % ('predicted' if _entry == 'guess_predicted_candidates'
                                              else 'punctuation', _variant)
            # The punctuation entry already gave no prefix for invalid and NULL.
            _control = _variant == 'valid' or (_entry != 'guess_predicted_candidates' and _variant in ('invalid', 'null'))
            case(_name, control=_control, crash=_null if _variant == 'null' else None,
                 issues=('1277',))(prefix_prediction_case(_entry, _variant))
        for _mode in ('pinyin', 'zhuyin'):
            # The pinyin sentence entry already guessed on no prefix for invalid input.
            _control = _variant == 'valid' or (_mode == 'pinyin' and _variant == 'invalid')
            _null = {'seeded': True, 'ret': True, 'sentence': [True, _BING]}
            case('prefix-convert/%s-sentence-%s' % (_mode, _variant), mode=_mode, control=_control,
                 crash=_null if _variant == 'null' else None,
                 issues=('1277',))(prefix_sentence_case(_variant))
