"""Combined import and n-best ordering cases."""
from contract_abi import B, I, P, S, U, Z, sentence_out, tokens_of, unigram_of, zhuyin_sentence_out


def register(case, *, remember_export, transient_files):
    """Register these cases at their original driver position."""
    # #525: reduce_tokens asserts on the sixth add, with five existing tokens.
    def combined_import_bound(k, guard=True):
        """Import the same phrase into five libraries, optionally attempting a sixth with state checks.

        Assert the first five adds. With guard false, return their flags immediately.
        Otherwise save a baseline, attempt library six, then compare fixed fixture
        tokens, frequencies, exports, dirty/save status and files. Finalize native
        objects after the guarded attempt; the fixture assumes the listed token IDs."""
        phrase = '你好'
        reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
        added = []
        for library in range(1, 6):
            it = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
            added.append(k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading, 100000))
            k.fn('end_add_phrases', None, P)(it)
        assert added == [True] * 5
        if not guard:
            return {'added': added}
        assert k.fn('save', B, P)(k.ctx)
        files = transient_files(k.user)
        tokens = [16802309, 33570045, 50359794, 67109866, 83886081]
        frequencies = [unigram_of(k, token) for token in tokens]
        exports = [remember_export(k, library, phrase) for library in range(1, 8)]
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 6)
        ret = k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading, 100000)
        # The pin aborts above. These fields are checked by the existing class(c)
        # driver and prove the subject refused before changing prior state.
        out = {'ret': ret,
               'untouched-tokens': tokens_of(k, phrase) == tokens,
               'untouched-frequencies': [unigram_of(k, token) for token in tokens] == frequencies,
               'untouched-readings': [remember_export(k, library, phrase) for library in range(1, 8)] == exports,
               'untouched-dirty': not k.fn('save', B, P)(k.ctx),
               'untouched-files': transient_files(k.user) == files}
        k.fn('end_add_phrases', None, P)(it)
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        return out

    for _mode in ('pinyin', 'zhuyin'):
        case('combined-library-sixth-add-' + _mode, mode=_mode, abort=False, sigabrt=True,
             issues=('525',))(combined_import_bound)
        case('combined-library-fifth-add-' + _mode, mode=_mode, control=True, issues=('525',))(
            lambda k: combined_import_bound(k, guard=False))


    def import_review(k, invalid=False):
        """Attempt an invalid-reading or unused-library import after populating five libraries.

        invalid selects a one-syllable reading for 你好 in library six; otherwise
        use its complete reading in library eight. Record the add flag and
        free/finalize cached native objects without clearing their pointers."""
        out = combined_import_bound(k, guard=False)
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 6 if invalid else 8)
        reading = b'ni3' if invalid else b"ni3'hao3"
        if k.mode == 'zhuyin':
            reading = ('ㄋㄧˇ' if invalid else 'ㄋㄧˇ ㄏㄠˇ').encode()
        out['ret'] = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 3)
        k.fn('end_add_phrases', None, P)(it)
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        return out

    for _mode in ('pinyin', 'zhuyin'):
        case('combined-library-invalid-reading-' + _mode, mode=_mode, control=True, issues=('525',))(lambda k: import_review(k, True))
        case('combined-library-unused-index-' + _mode, mode=_mode, abort=False, sigabrt=True,
             issues=('525',))(import_review)


    # #757 PR 0: two user phrases with equal counts whose user-table order
    # differs from token order. add_index keeps the user table ordered by
    # pinyin_exact_compare2 keys, then token (074a2219 chewing_large_table2.h:
    # 208-231, pinyin_phrase3.h:33-63), so 靐靐 (da2'da2, added second) comes
    # before 龘龘 (da3'da3, added first). A toneless parse matches both with
    # equal unigram and pronunciation possibility: the n-best tie goes to the
    # phrase the trellis walk meets first (phonetic_lookup.h:560-572).
    def nbest_user_table_order(k):
        """Compare sentence ordering for two equally counted user phrases inserted opposite to reading order.

        Assert unique increasing library-seven tokens for 龘龘 and 靐靐. Parse
        toneless dada; record three pinyin sentence ranks or one zhuyin sentence.
        Zhuyin options retain USE_TONE without FORCE_TONE."""
        phrases = (('龘龘', "da3'da3", 'ㄉㄚˇ ㄉㄚˇ'), ('靐靐', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'))
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
        assert it
        add = k.fn('iterator_add_phrase', B, P, S, S, I)
        added = [add(it, text.encode(), (pinyin if k.mode == 'pinyin' else zhuyin).encode(), 100000)
                 for text, pinyin, zhuyin in phrases]
        k.fn('end_add_phrases', None, P)(it)
        assert all(added), added
        tokens = [[token for token in tokens_of(k, text) if token >> 24 == 7] for text, _, _ in phrases]
        assert all(len(found) == 1 for found in tokens), tokens
        assert tokens[0][0] < tokens[1][0], 'the first phrase added holds the lower token'
        out = {'tokens': [hex(found[0]) for found in tokens]}
        if k.mode == 'zhuyin':
            # USE_TONE without FORCE_TONE, so the zhuyin facade takes a toneless key.
            assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.alloc()
        out['parsed'] = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'dada')
        out['guess'] = k.fn('guess_sentence', B, P)(inst)
        if k.mode == 'pinyin':
            out['sentences'] = [sentence_out(k, inst, index) for index in range(3)]
        else:
            out['sentence'] = zhuyin_sentence_out(k, inst)
        return out


    # Controls: the parent build already met the user items in this order, so
    # PR 0 preserves the output here.
    for _mode in ('pinyin', 'zhuyin'):
        case('nbest-user-table-order-' + _mode, mode=_mode, control=True, issues=('757',))(nbest_user_table_order)
