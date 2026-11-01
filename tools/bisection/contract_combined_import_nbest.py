"""Combined import and n-best ordering cases."""
import ctypes as C

from contract_abi import B, I, P, S, U, Z, sentence_out, tokens_of, unigram_of, zhuyin_sentence_out


def register(case, *, remember_export, transient_files):
    """Register these cases at their original driver position."""
    # #525: reduce_tokens asserts on the sixth add, with five existing tokens.
    def combined_import_bound(k, guard=True, sixth=None):
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
        ret = k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), sixth or reading, 100000)
        # The pin aborts on the sixth add (row 74) or, when the add refuses
        # at the parse bound, in the token read below; these fields prove a
        # surviving refusal left the earlier libraries untouched.
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

    # The pin's full-pinyin parse keeps its steps in `gint16`s
    # (`pinyin_parser2.cpp:67-70`, `:248`, `:292`), so an import's keys are
    # those of its first 32767 bytes; oxpinyin parses the whole reading
    # (#1501, `1501-gint16-input-cap.md`). Here both readings give the two
    # keys, and both sides die in the add (`pinyin.cpp:541`).
    case('combined-library-sixth-add-long-reading-pinyin', abort=False, sigabrt=True,
         issues=('525', 'row:74'))(
        lambda k: combined_import_bound(k, sixth=b"ni3'hao3" + b"'" * 70000))
    # Row 74 through the read, settled 2026-11-01 UTC: at the pin the
    # 40007-byte reading parses to one key (the gint16 bound,
    # `pinyin_parser2.cpp:67-70`, `:353-357`), so the sixth add refuses at
    # `pinyin.cpp:640` and leaks its keys. The case's first read, `tokens_of`
    # below, then calls `pinyin_lookup_tokens` (`pinyin.cpp:2664`), and
    # `reduce_tokens`' `assert(0 <= num && num <= 4)`
    # (`phrase_large_table3.h:95`) sees the five libraries that already hold
    # 你好. oxpinyin parses the whole reading (#1501): two keys, so the add
    # runs and `_add_phrase`'s own `reduce_tokens` aborts first
    # (`pinyin.cpp:541`). Both sides die of SIGABRT before writing anything
    # else, so the observations are the same; only the pin's assertion line
    # differs (`pin-abort-assert-line.md`).
    case('combined-library-sixth-add-key-past-parse-bound-pinyin',
         abort=False, sigabrt=True,
         issues=('525', 'row:74'))(
        lambda k: combined_import_bound(k, sixth=b'ni3' + b"'" * 40000 + b'hao3'))
    # `ZhuyinDirectParser2::parse` has `int` indices and no bound
    # (`zhuyin_parser2.cpp:914-961`) and skips each run of separators: the
    # long reading keeps its two keys and reaches the same assert.
    case('combined-library-sixth-add-long-reading-zhuyin', mode='zhuyin', abort=False, sigabrt=True,
         issues=('525', 'row:74'))(
        lambda k: combined_import_bound(k, sixth='ㄋㄧˇ ㄏㄠˇ'.encode() + b' ' * 70000))


    # The other validating `reduce_tokens` callers (`phrase_large_table3.h:95`):
    # `_compute_prefixes` (`pinyin.cpp:1415`, reached from the prefix guess
    # at `:1434` and the prediction at `:2424`) and `_pre_compute_tokens`'
    # `get_first_token` (`:3117`, reached from the character offset at
    # `:3221` and from `pinyin_remember_user_input` at `:3688`). Each dies
    # on a search that holds more than four tokens, before anything it
    # would observe. Five libraries holding the phrase reach it; four do not.
    def hold_in_libraries(k, phrase, pinyin, zhuyin, libraries):
        """Import phrase with its facade's reading into libraries 1..libraries, asserting each add."""
        reading = (pinyin if k.mode == 'pinyin' else zhuyin).encode()
        added = []
        for library in range(1, libraries + 1):
            it = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
            added.append(k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading, 100000))
            k.fn('end_add_phrases', None, P)(it)
        assert added == [True] * libraries, added

    def prefix_guess(libraries):
        """Guess a sentence after the prefix 你好 held in the given number of libraries."""
        def run(k):
            """Hold 你好, then answer the prefix guess's flag on a new instance."""
            hold_in_libraries(k, '你好', "ni3'hao3", 'ㄋㄧˇ ㄏㄠˇ', libraries)
            return {'ret': k.fn('guess_sentence_with_prefix', B, P, S)(k.alloc(), '你好'.encode())}
        return run

    def character_offset(libraries):
        """Ask the character offset of 你 over the parse ni, with 你 held in the given number of libraries."""
        def run(k):
            """Hold 你, parse ni and answer the offset call's flag and length."""
            hold_in_libraries(k, '你', 'ni3', 'ㄋㄧˇ', libraries)
            if k.mode == 'zhuyin':
                k.fn('set_options', B, P, U)(k.ctx, 0x18a)
            inst = k.alloc()
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
            length = Z(0)
            ret = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(inst, '你'.encode(), 0, C.byref(length))
            return {'ret': ret, 'length': length.value}
        return run

    case('prefix-guess-five-libraries-pinyin', abort=False, sigabrt=True, issues=('525', 'row:74'))(
        prefix_guess(5))
    case('prefix-guess-four-libraries-pinyin', control=True, issues=('525', 'row:74'))(prefix_guess(4))

    def predicted_candidates(k):
        """Predict after the prefix 你好 held in five libraries."""
        hold_in_libraries(k, '你好', "ni3'hao3", 'ㄋㄧˇ ㄏㄠˇ', 5)
        return {'ret': k.fn('guess_predicted_candidates', B, P, S)(k.alloc(), '你好'.encode())}

    case('predicted-candidates-five-libraries-pinyin', abort=False, sigabrt=True,
         issues=('525', 'row:74'))(predicted_candidates)
    case('character-offset-five-libraries-pinyin', abort=False, sigabrt=True,
         issues=('525', 'row:74'))(character_offset(5))
    case('character-offset-four-libraries-pinyin', control=True, issues=('525', 'row:74'))(
        character_offset(4))

    def remember_input(parse):
        """Remember 你 held in five libraries, after the parse ni or with no parse."""
        def run(k):
            """Hold 你, optionally parse ni, and answer the remember call's flag."""
            hold_in_libraries(k, '你', 'ni3', 'ㄋㄧˇ', 5)
            inst = k.alloc()
            if parse:
                k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
            return {'ret': k.fn('remember_user_input', B, P, S, I)(inst, '你'.encode(), 1)}
        return run

    # `_pre_compute_tokens` runs before anything reads the matrix
    # (`pinyin.cpp:3685-3700`), so the abort needs no parse.
    case('remember-user-input-five-libraries-pinyin', abort=False, sigabrt=True,
         issues=('525', 'row:74'))(remember_input(True))
    case('remember-user-input-five-libraries-unparsed-pinyin', abort=False, sigabrt=True,
         issues=('525', 'row:74'))(remember_input(False))

    # The zhuyin twins: `zhuyin_lookup_tokens` (`zhuyin.cpp:1690`),
    # `_compute_prefixes` (`:933`, before the prefix guess's matrix is read,
    # `:953`) and `_pre_compute_tokens` (`:2080`, from the character offset
    # at `:2176`). The sixth add's one-key reading refuses at the length gate
    # (`zhuyin.cpp:525`), and the case's first read, `tokens_of`, then
    # meets the five libraries that hold 你好.
    case('combined-library-sixth-add-one-key-zhuyin', mode='zhuyin', abort=False, sigabrt=True,
         issues=('525', 'row:74'))(
        lambda k: combined_import_bound(k, sixth='ㄋㄧˇ'.encode()))
    case('prefix-guess-five-libraries-zhuyin', mode='zhuyin', abort=False, sigabrt=True,
         issues=('525', 'row:74'))(prefix_guess(5))
    case('prefix-guess-four-libraries-zhuyin', mode='zhuyin', control=True, issues=('525', 'row:74'))(
        prefix_guess(4))
    case('character-offset-five-libraries-zhuyin', mode='zhuyin', abort=False, sigabrt=True,
         issues=('525', 'row:74'))(character_offset(5))
    case('character-offset-four-libraries-zhuyin', mode='zhuyin', control=True,
         issues=('525', 'row:74'))(character_offset(4))

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
