"""System phrase text cases (inventory row 46, #1175).
"""
import ctypes as C

from contract_abi import B, I, P, S, U, UNTOUCHED, Z, unigram_of
from contract_case_helpers import PHRASE_TEXT_WORDS, phrase_text_bytes


def register(case, *, phrase_text_system):
    """Register these cases at their original driver position."""
    def phrase_text_rows(k):
        """Export the private system library's two rows: text and reading hex, count."""
        it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 1)
        rows = []
        while k.fn('iterator_has_next_phrase', B, P)(it):
            assert len(rows) < 2, 'fixture exported an unexpected extra row'
            text, reading, count = P(UNTOUCHED), P(UNTOUCHED), I(UNTOUCHED)
            ret = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
                it, C.byref(text), C.byref(reading), C.byref(count))
            rows.append([ret, phrase_text_bytes(k, text.value),
                         phrase_text_bytes(k, reading.value), count.value])
        k.fn('end_get_phrases', None, P)(it)
        return rows


    def phrase_text_choose(k):
        """Choose the stored phrase, read the sentence, then commit it.

        074a2219: the choose writes only a token constraint and returns the cursor
        (pinyin.cpp:2577-2587, zhuyin.cpp:1646-1666); the sentence decodes under
        it (pinyin.cpp:1372-1387, zhuyin.cpp:895-910) and converts each phrase
        from its stored words (lookup/lookup.cpp:39-68). Neither facade has a
        commit call: a client commits the sentence it read and trains it
        (train_result3, lookup/phonetic_lookup.h:844-935), which trains each
        phrase with its stored length (storage/phonetic_key_matrix.cpp:610-620).
        """
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'da1da1da1ba1')
        if k.mode == 'pinyin':
            # SORT_WITHOUT_SENTENCE (0x1) would train at choose time instead of
            # installing a constraint (074a2219 pinyin.cpp:2566-2576).
            listed = k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0x1e)
        else:
            listed = k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, 0)
        count = U(UNTOUCHED)
        assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(count))
        chosen = None
        for index in range(count.value):
            candidate, text, kind = P(), P(UNTOUCHED), I(UNTOUCHED)
            assert k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, index, C.byref(candidate))
            assert k.fn('get_candidate_string', B, P, P, C.POINTER(P))(
                k.inst, candidate, C.byref(text))
            assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(
                k.inst, candidate, C.byref(kind))
            # NORMAL_CANDIDATE / NORMAL_CANDIDATE_AFTER_CURSOR are both 2; the
            # stored phrase is the fixture's only one over da'da'da.
            if kind.value == 2:
                chosen = (candidate, phrase_text_bytes(k, text.value, owned=False))
                break
        assert chosen is not None, 'missing the stored phrase candidate'
        cursor = k.fn('choose_candidate', I, P, Z, P)(k.inst, 0, chosen[0])
        guessed = k.fn('guess_sentence', B, P)(k.inst)
        sentence = P(UNTOUCHED)
        if k.mode == 'pinyin':
            read = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(k.inst, 0, C.byref(sentence))
            trained = k.fn('train', B, P, C.c_ubyte)(k.inst, 0)
        else:
            read = k.fn('get_sentence', B, P, C.POINTER(P))(k.inst, C.byref(sentence))
            trained = k.fn('train', B, P)(k.inst)
        out = {'parsed': parsed, 'listed': listed, 'chosen': chosen[1], 'cursor': cursor,
               'guessed': guessed, 'read': read, 'sentence': phrase_text_bytes(k, sentence.value),
               'trained': trained,
               'unigrams': [unigram_of(k, 0x01000000 | slot) for slot in (1, 2)]}
        if k.mode == 'pinyin':
            # The counts are the trained pronunciation frequencies
            # (pinyin.cpp:722-737); zhuyin exports no iterator.
            out['rows'] = phrase_text_rows(k)
        return out


    def phrase_text_probe(variant, surface):
        """Build a phrase-text observation for an isolated raw-word fixture.

        surface selects export, sentence, candidate or choose output. Keep text as
        byte hex so invalid UTF-8 is not replaced; for zhuyin export, report symbol
        availability because that facade has no export iterator. choose selects
        the stored phrase, reads the sentence and commits it (phrase_text_choose).
        """
        def probe(k):
            """Install the chosen word variant and return its text through the selected API.

            Bound pinyin export to two rows, release owned text, and leave candidate text
            borrowed. Sentence/candidate observations include parse and guess results.
            """
            phrase_text_system(k, variant)
            if surface == 'export' and k.mode == 'zhuyin':
                # zhuyin.h/libzhuyin.ver export no iterator. Never borrow a
                # pinyin context or invent an unsupported zhuyin C function.
                return {'exports': {name: hasattr(k.lib, 'zhuyin_' + name) for name in
                                    ('begin_get_phrases', 'iterator_has_next_phrase',
                                     'iterator_get_next_phrase', 'end_get_phrases')}}
            assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
            if surface == 'export':
                return {'rows': phrase_text_rows(k)}
            if surface == 'choose':
                return phrase_text_choose(k)
            spelling = b'da1da1da1' + (b'ba1' if surface == 'sentence' else b'')
            parsed = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, spelling)
            if surface == 'sentence':
                guessed = k.fn('guess_sentence', B, P)(k.inst)
                text = P(UNTOUCHED)
                if k.mode == 'pinyin':
                    ret = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(k.inst, 0, C.byref(text))
                else:
                    ret = k.fn('get_sentence', B, P, C.POINTER(P))(k.inst, C.byref(text))
                return {'parsed': parsed, 'guessed': guessed, 'ret': ret,
                        'text': phrase_text_bytes(k, text.value)}
            if k.mode == 'pinyin':
                ret = k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0x1f)
            else:
                ret = k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, 0)
            count = U(UNTOUCHED)
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(count))
            rows = []
            for index in range(count.value):
                candidate, text, kind = P(), P(UNTOUCHED), I(UNTOUCHED)
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, index, C.byref(candidate))
                assert k.fn('get_candidate_string', B, P, P, C.POINTER(P))(
                    k.inst, candidate, C.byref(text))
                assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(
                    k.inst, candidate, C.byref(kind))
                rows.append([kind.value, phrase_text_bytes(k, text.value, owned=False)])
            return {'parsed': parsed, 'ret': ret, 'rows': rows}
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        for _variant in PHRASE_TEXT_WORDS:
            for _surface in ('candidate', 'sentence', 'export', 'choose'):
                # The malformed stored-text cases are ordinary parity cases: the
                # outward bytes are reproduced, so they MATCH and are expected to
                # differ against a parent build. Scalar controls and the zhuyin
                # export-symbol absence checks are controls.
                _fixed = _variant != 'valid' and not (_mode == 'zhuyin' and _surface == 'export')
                case(f'phrase-text/{_mode}-{_variant}-{_surface}', mode=_mode,
                     control=not _fixed, issues=('894',))(
                         phrase_text_probe(_variant, _surface))
