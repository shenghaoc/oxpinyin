"""Pinyin and zhuyin candidate lookup offset cases."""
from contract_abi import B, P, S, U, Z, full_inst


GUESS_CANDIDATE_INPUTS = (b'ni', b'n', b'nihao', b'nih', b"ni'hao",
                          b'hao', b'women', b'zhongguo', b'woshi', b'xian')

GUESS_CANDIDATE_SORTS = (0, 0x1c, 0x1e)

GUESS_CANDIDATE_ZHUYIN_INPUTS = (b'su3', b'su', b'su3cl3', b'su3cl',
                                 b"su3'cl3", b'cl3', b'ji3', b'ji3cl3',
                                 b'zhong1', b'xian1')


def register(case, *, guess_rows, zhuyin_guess_rows):
    """Register the cases with the shared candidate readers."""
    # The characterisation set: single and multi-syllable, a bare partial tail,
    # apostrophes, and inputs whose longer phrases exist (nihao -> 你好吗,
    # women -> 我们…) and do not (woshi, xian). Every matrix offset of each input
    # — 0 through the reserved slot — three sort words — 0 (LONGER kept,
    # unsorted) plus the two sorted words with and without the LONGER bit — and
    # with and without a prior sentence lookup, which is what stores the n-best
    # rows. Full lists, not returns.
    #
    # The pin's matrix holds `parsed + 1` columns for the pinyin facade; an
    # offset past that reads through `_check_offset`'s `get_column_size` assert
    # (`pinyin.cpp:2163-2182`, `:2226`) and aborts — the class (c) sites
    # `abort-guess-candidates-past-matrix` and its zhuyin twin hold. The walk
    # therefore stops at `parsed` (the reserved slot) and `parsed + 1`.


    @case('guess-candidates-lookup-offset', issues=('696', 'row:65'))
    def _(k):
        """Collect ordered pinyin candidate lists across inputs, sort words and byte offsets.

        For each fresh parse, optionally guess a sentence first, then query offsets
        zero through parsed length plus one without reparsing between guesses."""
        out = {}
        for text in GUESS_CANDIDATE_INPUTS:
            for previous in (0, 1):
                inst = k.alloc()
                parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
                if previous:
                    k.fn('guess_sentence', B, P)(inst)
                for sort in GUESS_CANDIDATE_SORTS:
                    for offset in range(parsed + 2):
                        label = '%s prev=%d sort=%#x off=%d' % (
                            text.decode(), previous, sort, offset)
                        out[label] = guess_rows(k, inst, offset, sort)
        return out


    # The zhuyin facade has no LONGER prepend (`zhuyin.cpp:1460-1539` prepends
    # only the sentence rows at `:1533`), so the same full-list walk must already
    # match at every offset; it is the control that bounds the rule to the pinyin
    # facade. Its reserved slot is the LAST valid column (`parsed`); one past it
    # aborts (row 66,
    # `abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one`).


    @case('guess-candidates-lookup-offset-zhuyin', mode='zhuyin', control=True, issues=('696', 'row:65'))
    def _(k):
        """Collect zhuyin candidate lists through each parse's reserved final column.

        Compare fresh parses with and without a preceding sentence guess; offsets
        run from zero through the returned consumed length, inclusive."""
        out = {}
        for text in GUESS_CANDIDATE_ZHUYIN_INPUTS:
            for previous in (0, 1):
                inst = k.alloc()
                parsed = k.fn('parse_more_chewings', Z, P, S)(inst, text)
                if previous:
                    k.fn('guess_sentence', B, P)(inst)
                for offset in range(parsed + 1):
                    label = '%s prev=%d off=%d' % (text.decode(), previous, offset)
                    out[label] = zhuyin_guess_rows(k, inst, offset)
        return out


    # One past the reserved slot is an ordinary lookup that finds nothing —
    # except the LONGER row, which the pin still lists there (row 65). The full
    # list is compared, not the return alone.
    @case('guess-candidates-past-the-reserved-slot', issues=('696', 'row:65'))
    def _(k):
        """Record nihao candidate lists at byte offsets five and six with sort word zero."""
        out = {}
        inst = full_inst(k, b'nihao')
        for offset in (5, 6):
            out['offset %d' % offset] = guess_rows(k, inst, offset, 0)
        return out
