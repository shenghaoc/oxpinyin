"""User stored words cases (inventory row 41, #1175).
"""
import ctypes as C
from pathlib import Path

from contract_abi import B, I, P, S, U, UNTOUCHED, Z, tokens_of, unigram_of
from contract_case_helpers import PHRASE_TEXT_WORDS, _chunk, _ucs4_key, phrase_text_bytes
from contract_fixtures import import_rows


def register(case, *, phrase_text_system):
    """Register these cases at their original driver position."""
    # #1198: USER_FILE load copies raw words (074a2219 pinyin.cpp:303-318,
    # phrase_index.cpp:335-389); journal ADD copies the same item bytes
    # (phrase_index.cpp:439-459). No scalar validation occurs on either path.
    # Token text has declared length three (pinyin.cpp:1627-1645), even when
    # GLib's conversion stops at zero. Sentence conversion cuts each component
    # independently (lookup/lookup.cpp:39-68). Ranking keeps the stored counts
    # and length (pinyin.cpp:1678-1708,1795-1867), dedup compares outward text
    # (:2041-2095), and export converts text without shortening the reading
    # (:711-739). Save copies stored content, not outward text (:978-988).
    def user_stored_words(variant, library):
        """Import a scalar fixture, replace its stored words, then load/save/reload.

        Reuse the existing private-system, import, chunk and journal builders.
        Library one exercises journal ADD replay; five through seven are the
        default facade's addon, network and user files. Observe bytes losslessly.
        """
        def probe(k):
            phrase_text_system(k, 'valid')
            reading = "da1'da1'da1" if k.mode == 'pinyin' else 'ㄉㄚ ㄉㄚ ㄉㄚ'
            assert import_rows(k, library, [('甲戊乙', reading, 1000)]) == [True]
            token = next(token for token in tokens_of(k, '甲戊乙') if token >> 24 == library)
            assert k.fn('save', B, P)(k.ctx)
            k.fn('free_instance', None, P)(k.inst)
            k.fn('fini', None, P)(k.ctx)
            k._ctx = k._inst = None
            filename = {1: 'gb_char.dbin', 5: 'addon.bin', 6: 'network.bin', 7: 'user.bin'}[library]
            path = Path(k.user, filename)
            original = _ucs4_key('甲戊乙')
            words = (0x7532, PHRASE_TEXT_WORDS[variant], 0x4e59)
            replacement = b''.join(word.to_bytes(4, 'little') for word in words)
            payload = path.read_bytes()[8:]
            assert payload.count(original) == 1, 'one imported item to replace'
            path.write_bytes(_chunk(payload.replace(original, replacement)))

            def observe():
                length, text = U(UNTOUCHED), P(UNTOUCHED)
                ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
                    k.inst, token, C.byref(length), C.byref(text))
                result = {'token': [ret, length.value, phrase_text_bytes(k, text.value)],
                          'unigram': unigram_of(k, token)}
                if k.mode == 'pinyin':
                    iterator = k.fn('begin_get_phrases', P, P, U)(k.ctx, library)
                    rows = []
                    while k.fn('iterator_has_next_phrase', B, P)(iterator):
                        assert len(rows) < 4, 'bounded fixture export'
                        phrase, pinyin, count = P(UNTOUCHED), P(UNTOUCHED), I(UNTOUCHED)
                        got = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
                            iterator, C.byref(phrase), C.byref(pinyin), C.byref(count))
                        rows.append([got, phrase_text_bytes(k, phrase.value),
                                     phrase_text_bytes(k, pinyin.value), count.value])
                    k.fn('end_get_phrases', None, P)(iterator)
                    result['export'] = rows
                assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
                result['parsed'] = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'da1da1da1ba1')
                result['guessed'] = k.fn('guess_sentence', B, P)(k.inst)
                sentence = P(UNTOUCHED)
                if k.mode == 'pinyin':
                    read = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(k.inst, 0, C.byref(sentence))
                    listed = k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0x1e)
                else:
                    read = k.fn('get_sentence', B, P, C.POINTER(P))(k.inst, C.byref(sentence))
                    listed = k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, 0)
                result['sentence'] = [read, phrase_text_bytes(k, sentence.value)]
                count = U(UNTOUCHED)
                assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(count))
                rows = []
                for index in range(count.value):
                    candidate, phrase, kind = P(), P(UNTOUCHED), I(UNTOUCHED)
                    assert k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, index, C.byref(candidate))
                    assert k.fn('get_candidate_string', B, P, P, C.POINTER(P))(k.inst, candidate, C.byref(phrase))
                    assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(k.inst, candidate, C.byref(kind))
                    rows.append([kind.value, phrase_text_bytes(k, phrase.value, owned=False)])
                result['candidates'] = [listed, rows]
                return result

            out = {'loaded': observe()}
            # Ending an import batch arms save without changing an item.
            assert import_rows(k, 7, []) == []
            out['saved'] = k.fn('save', B, P)(k.ctx)
            out['native'] = path.read_bytes().hex()
            k.fn('free_instance', None, P)(k.inst)
            k.fn('fini', None, P)(k.ctx)
            k._ctx = k._inst = None
            out['reloaded'] = observe()
            return out
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        for _library in (1, 5, 6, 7):
            for _variant in PHRASE_TEXT_WORDS:
                case(f'user-stored-words/{_mode}-{_library}-{_variant}', mode=_mode,
                     control=_variant in ('valid', 'nul'), issues=('1198',))(
                         user_stored_words(_variant, _library))
