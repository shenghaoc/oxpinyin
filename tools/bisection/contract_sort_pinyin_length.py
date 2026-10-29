"""Sort and pinyin length cases (inventory row 35, #1175).
"""
from contract_abi import B, P, S, U, Z, final_step_sentences
from contract_case_helpers import candidate_rows, guess_rows, zhuyin_guess_rows


def register(case, *, CASES):
    """Register these cases at their original driver position."""
    # #921, #927: every sort that orders a C-visible list is GLib's merge
    # (`g_sort_array`, glib 2.84.4 `gqsort.c:69-193`) over a comparator that is
    # not a total order. The tails compare `-(lhs - rhs)` of two `gfloat`
    # possibilities as a `gint` (074a2219 `phonetic_lookup.h:174-178,337`), so
    # scores under one natural-log unit apart tie and ordinary sentences take the
    # merge's order; the candidate and predicted lists (`pinyin.cpp:1678-1709,
    # 2286,2443`; `zhuyin.cpp:1129-1144,1529,1610`) tie on equal truncated
    # frequencies and keep the search order. Controls: the parent already sorts
    # with the same merge.
    GLIB_SORT_TAIL_INPUTS = ('xian', 'fangan', 'shishi', 'yiyi', 'jiandan', 'tiantian', 'woaini',
                             'nihaoshijie', 'zhongguoren', 'shijie', 'dada', 'mama', 'lilun', 'jiqi',
                             'shuxue', 'diannao', 'yuyan', 'jintian', 'mingtian', 'shangwu')
    GLIB_SORT_LIST_INPUTS = ('n', 'zh', 'a', 'de', 'yi', 'shi', 'ji')


    def glib_sort_zhuyin_options(k):
        """Set USE_TONE without FORCE_TONE on the zhuyin facade, so it takes a toneless key."""
        if k.mode == 'zhuyin':
            assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)


    def glib_sort_tails(k, texts=GLIB_SORT_TAIL_INPUTS, options=None):
        """Parse each tail input on a fresh instance, guess and read every sentence rank."""
        glib_sort_zhuyin_options(k)
        if options is not None:
            assert k.fn('set_options', B, P, U)(k.ctx, options)
        out = {}
        for text in texts:
            inst = k.alloc()
            parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
            guessed = k.fn('guess_sentence', B, P)(inst)
            out[text] = dict(parsed=parsed, guessed=guessed, sentences=final_step_sentences(k, inst))
        return out


    def glib_sort_candidates(k):
        """Full candidate lists of each list input: three pinyin sort words, or both zhuyin cursors."""
        glib_sort_zhuyin_options(k)
        out = {}
        for text in GLIB_SORT_LIST_INPUTS:
            inst = k.alloc()
            parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
            if k.mode == 'pinyin':
                out[text] = {hex(sort): guess_rows(k, inst, 0, sort) for sort in (0x14, 0x1c, 0x1e)}
            else:
                after = zhuyin_guess_rows(k, inst, 0)
                before = k.fn('guess_candidates_before_cursor', B, P, Z)(inst, parsed)
                out[text] = {'after': after, 'before': {'ret': before, 'rows': candidate_rows(k, inst)}}
        return out


    @case('glib-sort-predicted', control=True, issues=('927',))
    def _(k):
        """Full plain predicted lists (no punctuation) after common one-character prefixes."""
        out = {}
        for prefix in ('你', '我', '中', '的', '一', '是'):
            ret = k.fn('guess_predicted_candidates', B, P, S)(k.inst, prefix.encode())
            out[prefix] = {'ret': ret, 'rows': candidate_rows(k, k.inst)}
        return out


    for _mode in ('pinyin', 'zhuyin'):
        case('glib-sort-tails-' + _mode, mode=_mode, control=True, stderr=True, issues=('921',))(glib_sort_tails)
        case('glib-sort-candidates-' + _mode, mode=_mode, control=True, issues=('927',))(glib_sort_candidates)


    # S3's registered finite triple: the captured qian pre-sort tails, plus
    # the same input with the divided/resplit tables disabled. Reuse the
    # complete-rank observer; no diagnostic pin or new driver is needed.
    for _enabled, _options in (('on', 0x1fe00198), ('off', 0x1fe00018)):
        case('trellis-tail-sort/qian-' + _enabled, control=True, stderr=True, issues=('1347', '921'))(
            lambda k, options=_options: glib_sort_tails(k, ('qian',), options))


    # S1/S2 of the witnessed algorithm-precondition amendment (2026-10-28 UTC).
    # Reuse the complete-rank observer for the captured witnesses and table-off controls.
    for _site, _text in (('resplit', 'jianai'), ('divided', 'bian')):
        for _enabled, _options in (('on', 0x1fe00198), ('off', 0x1fe00018)):
            case('trellis-adjusted-score/%s-%s-%s' % (_site, _text, _enabled),
                 control=True, issues=('1347',))(
                     lambda k, text=_text, options=_options: glib_sort_tails(k, (text,), options))

    # Keep the full diagnostic envelope when registering the intentional S1/S2 differences:
    # the special-cost completion (lambda above one, and negative lambda since #1400) and
    # the S2 tie rule (the gb table.conf cases, #1400).
    for _name in ('nbest-user-table-order-pinyin', 'table-conf-lambda-one-and-a-half',
                  'table-conf-lambda-two', 'table-conf-lambda-million',
                  'table-conf-lambda-negative-half', 'table-conf-lambda-negative-two',
                  'table-conf-row-removed-gb', 'table-conf-not-used-gb'):
        CASES[_name].update(stderr=True, policy_record='trellis-adjusted-cost-s1-s2.md')


    # #1378 applies the 2026-10-29 length/raw-score/batch zhuyin key.
    for _name in ('nbest-user-table-order-zhuyin',
                  'table-conf-lambda-one-and-a-half-zhuyin',
                  'table-conf-lambda-two-zhuyin', 'table-conf-lambda-million-zhuyin'):
        CASES[_name].update(stderr=True, policy_record='1378-zhuyin-selection.md')

    # #1275: the pin's SORT_BY_PINYIN_LENGTH key is `guint8 len = m_end -
    # m_begin` (074a2219 `pinyin.cpp:1692-1694`). Any run of apostrophes parses
    # (`storage/pinyin_parser2.cpp:238-251`) into zero keys the matrix search
    # steps over without counting them (`storage/phonetic_key_matrix.cpp:69-80,
    # 383-393`), so `ni`, N apostrophes and `hao` give 你好 a span of 5 + N bytes
    # at offset 0 (`pinyin.cpp:2246`) while the one-syllable rows span 2. From
    # 256 bytes the key wraps; a sort word carrying SORT_BY_PHRASE_LENGTH still
    # ranks 你好 first, so those words are controls inside every case.
    PINYIN_LENGTH_SORTS = (0x08, 0x09, 0x0a, 0x0c, 0x18, 0x1a, 0x1c, 0x1e)


    def pinyin_length_rows(apostrophes):
        """Full offset-0 candidate lists of `ni`, apostrophes, `hao` under each pinyin-length sort word."""
        def probe(k):
            """Parse the whole input on a fresh instance, list every sort word and require a non-empty list."""
            text = b'ni' + b"'" * apostrophes + b'hao'
            inst = k.alloc()
            parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
            assert parsed == len(text), parsed
            out = {'span': len(text), 'parsed': parsed}
            for sort in PINYIN_LENGTH_SORTS:
                rows = guess_rows(k, inst, 0, sort)
                assert rows['ret'] and rows['rows'], hex(sort)
                out[hex(sort)] = rows
            return out
        return probe


    for _apostrophes, _control in ((0, True), (10, True), (250, True), (251, False), (252, False), (507, False)):
        case('pinyin-length-wrap/span-%d' % (5 + _apostrophes), control=_control, issues=('1275',))(
            pinyin_length_rows(_apostrophes))
