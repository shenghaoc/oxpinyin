"""Punctuation cases (inventory row 39, #1175).
"""
import ctypes as C
from pathlib import Path
import shutil
import tempfile

from contract_abi import B, I, P, S, U, UNTOUCHED, Z, full_inst, sentence_out, tokens_of
from contract_fixtures import _write_user_conf


def register(case, *, _craft_db):
    """Register these cases at their original driver position."""
    # punct.bin decoding (#1144). `PunctTableEntry::unescape` appends the
    # word at the cursor, then each nonzero word after it up to a zero or the
    # value's last whole word, and the walk steps one word past that run
    # (`src/storage/punct_table.cpp:57-94` at pin 074a2219). The pin hands each
    # run to C through `g_string_append_unichar`, so a non-scalar takes GLib's
    # historical form and a zero first word cuts to an empty string. Each case
    # writes the system `punct.bin` in the running cell's native format and
    # records the punctuation rows' raw bytes.
    def punct_words(*words):
        """Encode u32 words little-endian, the `punct.bin` value layout."""
        return b''.join(word.to_bytes(4, 'little') for word in words)


    def punct_fixture(k, rows_for):
        """Replace the system `punct.bin` with crafted rows and reopen lazily.

        `rows_for(k)` runs on the original data and returns `(text, value)`
        pairs; every token of `text` gets `value`. Copy the system tree to
        scratch, write the table natively (`_craft_db` with `system_index`: a
        btree on bdb, a TreeDB on kc, a TreeDBM on tkrzw, as each pin's
        `PunctTable::attach` opens it) and close the cached instance and context."""
        rows = [(token.to_bytes(4, 'little'), value)
                for text, value in rows_for(k) for token in tokens_of(k, text)]
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        copied = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
        shutil.copytree(k.data, copied, dirs_exist_ok=True)
        k.data = copied
        _craft_db(str(Path(copied, 'punct.bin')), rows, 1, system_index=True)


    def punct_rows(k, prefix):
        """Run the punctuation prediction for `prefix` and record its rows.

        Return the call's result, the punctuation rows' bytes as hex in list
        order, the kinds of the first rows and the other rows' count."""
        inst = k.inst
        ret = k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, prefix.encode())
        count = U()
        assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
        kinds, puncts = [], []
        for i in range(count.value):
            cand, kind, text = P(), I(), S()
            k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
            kinds.append(kind.value)
            if kind.value == 8:
                k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
                puncts.append(None if text.value is None else text.value.hex())
        return dict(ret=ret, puncts=puncts, kinds=kinds[:len(puncts) + 2],
                    others=len(kinds) - len(puncts))


    def punct_decode_case(value):
        """A case that stores `value` under 我 and predicts after 我."""
        def probe(k):
            """Craft the table, then record the punctuation rows for 我."""
            if value in (PUNCT_DECODE_CASES['misaligned-terminated'], PUNCT_DECODE_CASES['misaligned-short']):
                _write_user_conf(k)
            punct_fixture(k, lambda _k: [('我', value)])
            out = punct_rows(k, '我')
            if value in (PUNCT_DECODE_CASES['misaligned-terminated'], PUNCT_DECODE_CASES['misaligned-short']):
                return {'ret': out['ret'], 'puncts': out['puncts']}
            return out
        return probe


    PUNCT_DECODE_CASES = {
        # Controls: identical before and after #1144.
        'valid': punct_words(0xFF0C, 0, 0x3002, 0),
        'zero-length': b'',
        # Non-scalars: a surrogate, the first value above Unicode, the first
        # five- and six-byte forms and the largest word.
        'surrogate': punct_words(0xD800, 0, 0xFF0C, 0xDFFF, 0),
        'above-unicode': punct_words(0x11_0000, 0, 0x20_0000, 0, 0x400_0000, 0, 0xFFFF_FFFF, 0),
        # No final zero: the pin reads one word past the value either way.
        'unterminated': punct_words(0xFF0C, 0, 0x3002, 0x3001),
        'unterminated-zero': punct_words(0xFF0C, 0, 0),
        # Length not a multiple of four: whole words, then one empty
        # punctuation after a terminated run.
        'misaligned-terminated': punct_words(0xFF0C, 0) + b'A',
        'misaligned-unterminated': punct_words(0xFF0C) + b'AB',
        'misaligned-short': b'ABC',
        # A zero first word is an empty punctuation that still takes the
        # nonzero words after it.
        'empty': punct_words(0, 0, 0xFF0C, 0),
        'empty-leading': punct_words(0, 0xFF0C, 0, 0x3002, 0),
    }
    for _name, _value in PUNCT_DECODE_CASES.items():
        case('punct-decode/' + _name, control=_name in ('valid', 'zero-length'),
             issues=('1144',), policy_record='1297-punct-words.md' if _name in ('misaligned-terminated', 'misaligned-short') else None)(punct_decode_case(_value))


    # Punctuation order and duplicates (#928). The pin appends each prefix
    # token's punctuations in `m_prefixes` order, skips a string already taken
    # (`g_strv_contains`, `g_str_equal` over the C strings) and prepends the
    # result in reverse, so the rows keep first-occurrence order ahead of the
    # predicted rows, which take no part in the test (`src/pinyin.cpp:2463-2494`
    # at pin 074a2219).
    def punct_dedup_case(rows, prefix='我'):
        """A case that stores `rows` (`(text, value)` pairs) and predicts after `prefix`."""
        def probe(k):
            """Craft the table, then record the punctuation rows for the prefix."""
            punct_fixture(k, lambda _k: rows)
            return punct_rows(k, prefix)
        return probe


    PUNCT_DEDUP_CASES = {
        # Control: no repeated string.
        'distinct': [('我', punct_words(0xFF0C, 0, 0x3002, 0, 0xFF01, 0))],
        'within': [('我', punct_words(0xFF0C, 0, 0x3002, 0, 0xFF0C, 0, 0x3002, 0))],
        # 们's tokens come before 我们's: suffixes are walked shortest first.
        'across-prefixes': [('们', punct_words(0xFF0C, 0, 0xFF01, 0)),
                            ('我们', punct_words(0x3002, 0, 0xFF0C, 0))],
        'empty': [('我', punct_words(0, 0, 0, 0, 0xFF0C, 0))],
        # Different stored words, one empty C string.
        'empty-forms': [('我', punct_words(0, 0, 0, 0xFF0C, 0, 0x3002, 0))],
        'non-scalar': [('们', punct_words(0xD800, 0, 0x11_0000, 0)),
                       ('我们', punct_words(0x11_0000, 0, 0xD800, 0))],
    }
    for _name, _rows in PUNCT_DEDUP_CASES.items():
        _prefix = '我们' if any(text == '我们' for text, _ in _rows) else '我'
        case('punct-dedup/' + _name, control=_name == 'distinct',
             issues=('928',))(punct_dedup_case(_rows, _prefix))


    @case('punct-dedup/candidate', control=True, issues=('928',))
    def _(k):
        """Store the first predicted phrase for 我 as its punctuation.

        The punctuation row and the predicted row both stay: the pin compares
        punctuations only with each other. Record the punctuation rows and the
        first predicted rows' bytes."""
        inst = k.inst
        assert k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())
        cand, text = P(), S()
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        first = text.value.decode()
        punct_fixture(k, lambda _k: [('我', punct_words(*(ord(ch) for ch in first), 0, 0xFF0C, 0))])
        out = punct_rows(k, '我')
        inst = k.inst
        texts = []
        for i in range(len(out['puncts']), len(out['puncts']) + 3):
            cand, text = P(), S()
            if k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand)):
                k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
                texts.append(None if text.value is None else text.value.hex())
        out['first predicted'] = texts
        return out


    # An aliased prefix for the punctuation entry (#1272). The entry first
    # calls `pinyin_guess_predicted_candidates` (`src/pinyin.cpp:2459` at pin
    # 074a2219), which frees every candidate string (`_free_candidates`,
    # `:1335-1344`) and empties `m_prefixes` (`:2423`) before `_compute_prefixes`
    # reads `prefix` (`:2424`): when `prefix` is one of those strings, that read
    # is a use-after-free, so the pin is never driven with it (`pin_not_run`).
    # The defined parts are the answer `true` (`:2498`), the emptied list and
    # the sentence rows, which neither call touches; with #542's no-prefix
    # outcome the list stays empty. The owned controls pass a copy of the same
    # text and compare with the pin.
    def punct_alias_case(source, aliased):
        """A case that passes a candidate string, or a copy of it, as the prefix.

        `source` picks the list the string comes from: `candidates` (the first
        row of `pinyin_guess_candidates` after nihao) or `predicted` (the first
        non-punctuation row of a punctuation prediction after 我 whose own text,
        predicted on a second instance, gives rows)."""
        def probe(k):
            """Guess nihao, build the list, then predict with punctuation after the row's text.

            Record the answer, the candidate count, the first kinds and whether
            the three sentence rows are unchanged."""
            inst = full_inst(k, b'nihao')
            k.fn('guess_sentence', B, P)(inst)
            index = 0
            if source == 'candidates':
                k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1E)
            else:
                k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, '我'.encode())
                probe_inst = k.alloc()
                kind, n = I(), U()
                while True:
                    cand, text = P(), S()
                    assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
                    k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
                    if kind.value != 8:
                        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
                        k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(probe_inst, text.value)
                        assert k.fn('get_n_candidate', B, P, C.POINTER(U))(probe_inst, C.byref(n))
                        if n.value:
                            break
                    index += 1
            before = [sentence_out(k, inst, i) for i in range(3)]
            cand, text = P(UNTOUCHED), P(UNTOUCHED)
            assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
            assert k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, cand, C.byref(text))
            owned = C.create_string_buffer(C.string_at(text.value))
            prefix = text.value if aliased else C.cast(owned, P).value
            ret = k.fn('guess_predicted_candidates_with_punctuations', B, P, P)(inst, prefix)
            count = U(UNTOUCHED)
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            kinds = []
            for i in range(min(count.value, 4)):
                cand, kind = P(), I()
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
                k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
                kinds.append(kind.value)
            after = [sentence_out(k, inst, i) for i in range(3)]
            return {'ret': ret, 'candidates': count.value, 'kinds': kinds,
                    'sentences kept': before == after}
        return probe


    PUNCT_ALIAS_DEFINED = {'ret': True, 'candidates': 0, 'kinds': [], 'sentences kept': True}
    for _source in ('candidates', 'predicted'):
        case('punct-alias/borrowed-' + _source, pin_not_run=PUNCT_ALIAS_DEFINED,
             issues=('1272', '542'))(punct_alias_case(_source, True))
        case('punct-alias/owned-' + _source, control=True,
             issues=('1272',))(punct_alias_case(_source, False))
