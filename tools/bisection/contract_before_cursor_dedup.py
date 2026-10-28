"""Before-cursor and longer dedup cases (inventory row 29, #1175).
"""
import ctypes as C

from contract_abi import B, I, P, S, U, Z, tokens_of
from contract_case_helpers import _gram_bytes, _replace_user_bigram, candidate_rows, guess_rows
from contract_fixtures import row58_import


def register(case, *, addon_promote_sequence):
    """Register these cases at their original driver position."""
    # At 074a2219, each before-cursor slice keeps its `m_freq` when appended
    # (`zhuyin.cpp:1607-1612`). The final dedup compares all slices (:1630,
    # :1406-1416). A parse keeps the last sentence result (:1017-1042).
    # Guess `nianhao`, then parse `ni''hao`: result[4] still holds the second
    # token. The separator starts have no predecessor; start 4 has the first
    # token (:1188-1197). Its bigram lets the later slice win. With the bit
    # clear the rows tie.
    def before_cursor_frequency_listing(k, dynamic):
        """Observe before-cursor candidates after a guessed phrase is reparsed with separators.

        Import 甲/乙, save and replace the user bigram with million-count rows.
        Set USE_TONE and optional DYNAMIC_ADJUST, assert complete nianhao and
        ni''hao parses, and choose the first 乙 row at the end cursor. Return rows,
        flags, parsed byte length and selection offset; assert that row was found."""
        row58_import(k, [('甲', 'ㄋㄧㄢˊ', 1000000), ('乙', 'ㄏㄠˇ', 1000000)])
        first, second = (tokens_of(k, text)[-1] for text in ('甲', '乙'))
        assert k.fn('save', B, P)(k.ctx)
        _replace_user_bigram(k, [(1, _gram_bytes(1000000, [(first, 1000000)])),
                                 (first, _gram_bytes(1000000, [(second, 1000000)]))])
        assert k.fn('set_options', B, P, U)(k.ctx, 0x20 | (0x200 if dynamic else 0))
        inst = k.inst
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nianhao') == 7
        guessed = k.fn('guess_sentence', B, P)(inst)
        assert guessed
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b"ni''hao")
        assert parsed == 7
        ret = k.fn('guess_candidates_before_cursor', B, P, Z)(inst, parsed)
        rows = candidate_rows(k, inst)
        chosen = None
        for index, (_, text) in enumerate(rows):
            if text == '乙':
                candidate = P()
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(candidate))
                chosen = k.fn('choose_candidate', I, P, Z, P)(inst, parsed, candidate)
                break
        assert chosen is not None, 'the imported before-cursor row is missing'
        return dict(parsed=parsed, guessed=guessed, ret=ret, rows=rows, chosen=chosen)


    case('before-cursor-frequency-dedup', mode='zhuyin', issues=('913',))(
        lambda k: before_cursor_frequency_listing(k, True))
    case('before-cursor-frequency-tie', mode='zhuyin', control=True, issues=('913',))(
        lambda k: before_cursor_frequency_listing(k, False))


    # The divided table adds xi + an to `xian` (phonetic_key_matrix.cpp:169-217,
    # special_table.h:27). Two libraries hold one text with xi'an and xian'ni
    # readings. New items add both index entries (pinyin.cpp:584-605). The first
    # is NORMAL on the divided path; the second is LONGER on the whole key.
    # LONGER has only a unigram term (pinyin.cpp:1916-1927), while NORMAL gets
    # the planted bigram (:1845-1866). The final dedup sees both (:2292-2300).
    def longer_frequency_listing(k, dynamic, longer=True):
        """Compare normal and longer rows for two imported readings of 甲乙.

        Import into libraries 6 and 7, save and replace the user bigram to favor
        the library-6 token. Enable divided/resplit parsing and optional dynamic
        adjustment, assert a complete xian parse and, when requested, the longer
        row at end. Return candidate lists before/after sentence guessing and with
        sentence rows suppressed, retaining the cached context's mutations."""
        for library, reading in ((6, "xi1'an1"), (7, "xian1'ni1")):
            iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
            assert iterator
            assert k.fn('iterator_add_phrase', B, P, S, S, I)(
                iterator, '甲乙'.encode(), reading.encode(), 10000000)
            k.fn('end_add_phrases', None, P)(iterator)
        token, = (token for token in tokens_of(k, '甲乙') if token >> 24 == 6)
        assert k.fn('save', B, P)(k.ctx)
        _replace_user_bigram(k, [(1, _gram_bytes(1000000, [(token, 1000000)]))])
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | (0x200 if dynamic else 0))
        inst = k.inst
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'xian')
        assert parsed == 4
        sort = 1 if longer else 3
        out = dict(parsed=parsed, at_end=guess_rows(k, inst, parsed, sort))
        if longer:
            assert [7, '甲乙'] in out['at_end']['rows'], 'the LONGER fixture row is missing'
        out['without_sentence'] = guess_rows(k, inst, 0, sort)
        out['guess_sentence'] = k.fn('guess_sentence', B, P)(inst)
        out['with_sentence'] = guess_rows(k, inst, 0, sort & ~1)
        out['sentence_suppressed'] = guess_rows(k, inst, 0, sort)
        return out


    case('longer-normal-frequency-dedup', issues=('913',))(
        lambda k: longer_frequency_listing(k, True))
    # With sentence rows suppressed the old path did no final dedup. Even
    # the tie case must remove the duplicate NORMAL row (:2298-2300).
    case('longer-normal-frequency-tie', issues=('913',))(
        lambda k: longer_frequency_listing(k, False))
    case('longer-normal-disabled', control=True, issues=('913',))(
        lambda k: longer_frequency_listing(k, True, longer=False))

    case('issue762-addon-promote-no-mask', control=True, issues=('762',))(lambda k: addon_promote_sequence(k, None))
    case('issue762-addon-promote-mask-highest', issues=('762',))(lambda k: addon_promote_sequence(k, 0x05000003))
    case('issue762-addon-promote-mask-middle', control=True, issues=('762',))(lambda k: addon_promote_sequence(k, 0x05000002))
