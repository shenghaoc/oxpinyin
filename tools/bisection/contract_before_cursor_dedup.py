"""The S2 n-best witness of the before-cursor and longer dedup family (#1175).

The family's other cases are Rust tests in `crates/oxpinyin-capi/tests` and
`crates/oxpinyin-zhuyin-capi/tests` (`contract_data/before_cursor_dedup.rs`).
This bdb-only case has no frozen reference yet; it waits for the next pin capture.
"""
from contract_abi import B, I, P, S, U, Z, tokens_of
from contract_case_helpers import _gram_bytes, _replace_user_bigram, guess_rows


def register(case):
    """Register the case at its original driver position."""
    # The divided table adds xi + an to `xian` (phonetic_key_matrix.cpp:169-217,
    # special_table.h:27). Two libraries hold one text with xi'an and xian'ni
    # readings. New items add both index entries (pinyin.cpp:584-605). The first
    # is NORMAL on the divided path; the second is LONGER on the whole key.
    # LONGER has only a unigram term (pinyin.cpp:1916-1927), while NORMAL gets
    # the planted bigram (:1845-1866). The final dedup sees both (:2292-2300).
    def s2_nbest_precedence(k):
        """Compare normal and longer rows for two imported readings of 甲乙.

        Import into libraries 6 and 7, save and replace the user bigram to favor
        the library-6 token. Enable divided/resplit parsing and dynamic
        adjustment, assert a complete xian parse and the longer
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
        assert k.fn('set_options', B, P, U)(k.ctx, 0x38a)
        inst = k.inst
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'xian')
        assert parsed == 4
        sort = 1
        out = dict(parsed=parsed, at_end=guess_rows(k, inst, parsed, sort))
        assert [7, '甲乙'] in out['at_end']['rows'], 'the LONGER fixture row is missing'
        out['without_sentence'] = guess_rows(k, inst, 0, sort)
        out['guess_sentence'] = k.fn('guess_sentence', B, P)(inst)
        out['with_sentence'] = guess_rows(k, inst, 0, sort & ~1)
        out['sentence_suppressed'] = guess_rows(k, inst, 0, sort)
        return out

    # Preserve the former full-model observation separately. Its sentence tails
    # exercise S2; NBEST precedence can then replace a same-text phrase row.
    # This one witness retains full lists to register the complete policy pair.
    case('s2-nbest-longer-normal-precedence', cells=('bdb',), issues=('1398', '1347'), stderr=True,
         policy_record='trellis-adjusted-cost-s1-s2.md')(s2_nbest_precedence)
