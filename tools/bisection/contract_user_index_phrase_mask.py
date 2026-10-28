"""User index and phrase mask cases (inventory row 52, #1175).
"""
import ctypes as C

from contract_abi import Arr, B, I, P, S, U, Z
from contract_case_helpers import TOKEN_REUSE_PHRASES, USER_ALL, _user_candidate_rows, predicted_rows


def register(case, *, predicted_bigram_context, token_reuse_files):
    """Register these cases at their original driver position."""
    # #784: the pin never deletes a key from its user index tables. `mask_out`
    # rewrites every record in place (`phrase_large_table3_bdb.cpp:391-405` and
    # `chewing_large_table2_bdb.cpp:488-537` put with `DB_CURRENT`; the kc visitors
    # return the masked value, `phrase_large_table3_kyotodb.cpp:361-370` and
    # `chewing_large_table2_kyotodb.cpp:459-506`; the tkrzw processors return it,
    # `phrase_large_table3_tkrzwdb.cpp:288-296` and
    # `chewing_large_table2_tkrzwdb.cpp:429-473`), and `pinyin_remove_user_candidate`'s
    # `remove_index` calls store the emptied entry back
    # (`phrase_large_table3_bdb.cpp:353-368`, `chewing_large_table2_bdb.cpp:417-432`,
    # `phrase_large_table3_kyotodb.cpp:334-345`, `phrase_large_table3_tkrzwdb.cpp:265-274`).
    # A key whose last record left stays with an empty value, and every later save
    # copies it. Each step that saves records both index files key by key.
    INDEX_KEY_READINGS = {
        # The facade's own parse input for each TOKEN_REUSE_PHRASES reading.
        'pinyin': ('dada', 'tata', 'bingbing', 'xianxian', 'cucu', 'yuanyuan'),
        'zhuyin': ('286286', 'w84w84', '1u/31u/3', 'vu0 vu0 ', 'hj hj ', 'm06m06'),
    }


    def index_key_sequence(k, steps, phrases=None, readings=None):
        """Runs `steps` over the user library and records each save's index files.
        `('add', n)`, `('save',)`, `('mask', mask, value)`, `('reopen',)` as in
        `token_reuse_sequence`; `('remove', n)` removes phrase n through its user
        candidate (`pinyin_remove_user_candidate`); `('lookup', n)` records what a
        fresh instance shows for phrase n: the candidates of its reading, the
        tokens of its text and of its first character, and (pinyin) the predicted
        rows after its first character. `('guess', pinyin, zhuyin)` parses and
        guesses that input. `phrases` and `readings` default to
        `TOKEN_REUSE_PHRASES` and `INDEX_KEY_READINGS`.

        Also support ('unload',) and ('load',), targeting library seven. Imports
        use count 100; removal asserts the first normal user candidate exists.
        The guess action populates candidate observations without calling the
        sentence-guess API. Unrecognized steps are ignored. Saves snapshot normalized
        files; reopening finalizes without implicitly saving or updating kit caches.
        Free temporary instances after observations; lookup arrays remain allocated.
        Return the ordered steps list under the steps key."""
        ctx = k.ctx
        phrases = TOKEN_REUSE_PHRASES if phrases is None else phrases
        readings = INDEX_KEY_READINGS if readings is None else readings
        column = 1 if k.mode == 'pinyin' else 2
        parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
        out = []

        def tokens(inst, text):
            """Return lookup status and hexadecimal tokens for UTF-8 text in a newly allocated GArray.

            Copy the values into Python lists without freeing the native array."""
            arr = k.glib.g_array_new(0, 0, 4)
            ret = k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
            view = C.cast(arr, C.POINTER(Arr)).contents
            return [ret, [hex(t) for t in C.cast(view.data, C.POINTER(U))[:view.len]]]

        def candidates(inst, limit=12):
            """Return total candidate count and up to limit [kind, UTF-8 text] rows.

            Use the current list, ignore accessor flags and decode text strictly."""
            count = U()
            k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            rows = []
            for i in range(min(count.value, limit)):
                cand, kind, text = P(), I(), S()
                k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
                k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
                k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
                rows.append([kind.value, text.value.decode()])
            return [count.value, rows]

        def guess(inst, n):
            """Parse an explicit string or indexed fixture reading and populate candidates at zero.

            Choose the facade's parser and candidate API; do not guess a sentence or
            assert native return flags."""
            reading = (n if isinstance(n, str) else readings[k.mode][n]).encode()
            k.fn(parse, Z, P, S)(inst, reading)
            if k.mode == 'pinyin':
                k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
            else:
                k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)

        k.glib.g_array_new.restype, k.glib.g_array_new.argtypes = P, [I, I, U]
        for step in steps:
            if step[0] == 'add':
                it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
                assert it, 'the import iterator'
                text, reading = phrases[step[1]][0], phrases[step[1]][column]
                out.append(['add', bool(k.fn('iterator_add_phrase', B, P, S, S, I)(it, text.encode(), reading.encode(), 100))])
                k.fn('end_add_phrases', None, P)(it)
            elif step[0] == 'save':
                out.append(['save', bool(k.fn('save', B, P)(ctx)), token_reuse_files(k)])
            elif step[0] == 'mask':
                out.append(['mask', bool(k.fn('mask_out', B, P, U, U)(ctx, step[1], step[2]))])
            elif step[0] == 'reopen':
                k.fn('fini', None, P)(ctx)
                ctx = k.init()
                assert ctx, 'the reopen failed'
            elif step[0] in ('unload', 'load'):
                out.append([step[0], bool(k.fn(step[0] + '_phrase_library', B, P, U)(ctx, 7))])
            elif step[0] == 'remove':
                inst = k.fn('alloc_instance', P, P)(ctx)
                guess(inst, step[1])
                rows = _user_candidate_rows(k, inst)
                assert rows, 'no user candidate to remove'
                out.append(['remove', bool(k.fn('remove_user_candidate', B, P, P)(inst, rows[0]))])
                k.fn('free_instance', None, P)(inst)
            elif step[0] == 'guess':
                inst = k.fn('alloc_instance', P, P)(ctx)
                guess(inst, step[1] if k.mode == 'pinyin' else step[2])
                out.append(['guess', step[1], candidates(inst)])
                k.fn('free_instance', None, P)(inst)
            elif step[0] == 'lookup':
                text = phrases[step[1]][0]
                inst = k.fn('alloc_instance', P, P)(ctx)
                guess(inst, step[1])
                seen = {'candidates': candidates(inst), 'tokens': tokens(inst, text)}
                # #785: on kc and tkrzw a partial mask leaves the previous record's
                # post-mask tail in the first character's phrase-table key, and this
                # lookup returns it; bdb is the control that carries nothing.
                seen['prefix tokens'] = tokens(inst, text[0])
                if k.mode == 'pinyin':
                    seen['predicted'] = predicted_rows(k, inst, text[0])
                out.append(['lookup', text, seen])
                k.fn('free_instance', None, P)(inst)
        return {'steps': out}


    MIDDLE, HIGHEST = (0xffffffff, 0x07000002), (0xffffffff, 0x07000003)
    SAVED = [('add', 0), ('add', 1), ('add', 2), ('save',)]
    INDEX_KEY_SEQUENCES = {
        # name: (control, steps). A control holds on the parent too: no key is
        # emptied, or no save writes the files after one is. Neither `mask_out`
        # nor `remove_user_candidate` marks the context modified, so a save right
        # after one answers false and writes nothing, at the pin and here alike;
        # the import after it is what makes the next save write the tables.
        'no-mask': (True, SAVED + [('add', 3), ('save',)]),
        'mask-save': (True, SAVED + [('mask',) + MIDDLE, ('save',)]),
        'mask-middle': (False, SAVED + [('mask',) + MIDDLE, ('add', 3), ('save',)]),
        # The highest token is handed out again (#762), under another text.
        'mask-highest': (False, SAVED + [('mask',) + HIGHEST, ('add', 3), ('save',)]),
        'mask-middle-highest': (False, SAVED + [('mask',) + MIDDLE, ('mask',) + HIGHEST, ('add', 3), ('save',)]),
        'mask-unsaved': (False, [('add', 0), ('add', 1), ('add', 2), ('mask',) + MIDDLE, ('save',)]),
        'mask-all': (False, [('add', 0), ('add', 1), ('save',), ('mask',) + USER_ALL, ('add', 2), ('save',)]),
        # The emptied keys come back from the file: the next session's save keeps
        # them, and the masked phrase imported again fills its own keys.
        'mask-reopen': (False, SAVED + [('mask',) + MIDDLE, ('add', 3), ('save',), ('reopen',), ('add', 4),
                                        ('save',), ('reopen',), ('add', 1), ('save',)]),
        # What a fresh instance shows after the mask: in the masking session, and
        # after a reopen that loaded the emptied keys from the file.
        'mask-lookup': (True, SAVED + [('mask',) + MIDDLE, ('lookup', 1), ('lookup', 0)]),
        'mask-reopen-lookup': (False, SAVED + [('mask',) + MIDDLE, ('add', 3), ('save',), ('reopen',),
                                               ('lookup', 1), ('lookup', 0)]),
    }
    for _mode in ('pinyin', 'zhuyin'):
        for _name, (_control, _steps) in INDEX_KEY_SEQUENCES.items():
            case('issue784-index-keys-%s-%s' % (_name, _mode), mode=_mode, control=_control, issues=('784',))(
                lambda k, steps=_steps: index_key_sequence(k, steps))
    # `pinyin_remove_user_candidate` (the zhuyin facade has none).
    REMOVE_SEQUENCES = {
        'remove-save': (True, SAVED + [('remove', 1), ('save',)]),
        'remove-middle': (False, SAVED + [('remove', 1), ('add', 3), ('save',)]),
        'remove-unsaved': (False, [('add', 0), ('add', 1), ('add', 2), ('remove', 2), ('save',)]),
        'remove-reopen': (False, [('add', 0), ('add', 1), ('save',), ('remove', 0), ('add', 2), ('save',),
                                  ('reopen',), ('add', 3), ('save',)]),
        'remove-lookup': (True, [('add', 0), ('add', 1), ('save',), ('remove', 1), ('lookup', 1), ('lookup', 0)]),
        'remove-reopen-lookup': (False, [('add', 0), ('add', 1), ('save',), ('remove', 1), ('add', 2), ('save',),
                                         ('reopen',), ('lookup', 1), ('lookup', 0)]),
    }
    for _name, (_control, _steps) in REMOVE_SEQUENCES.items():
        case('issue784-index-keys-%s' % _name, control=_control, issues=('784',))(
            lambda k, steps=_steps: index_key_sequence(k, steps))


    # #785: on kc and tkrzw the user pinyin table's mask walk starts every key
    # length from the table entry the last operation of that length left. Two
    # phrases share the initials `d d`, so that record holds two items where the
    # others hold one; a search hit or an `add_index` on it leaves both in the
    # entry, and the first record the walk meets in that length picks up the
    # second as its tail. The single-syllable phrase gives length 1 a record
    # that the empty prefix markers after it inherit whole.
    TAIL_PHRASES = (
        ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'), ('龖龖', "ta4'ta4", 'ㄊㄚˋ ㄊㄚˋ'),
        ('靐靐', "bing3'bing3", 'ㄅㄧㄥˇ ㄅㄧㄥˇ'), ('鬥鬥', "dou4'dou4", 'ㄉㄡˋ ㄉㄡˋ'),
        ('龘', 'da2', 'ㄉㄚˊ'), ('鱻鱻', "xian1'xian1", 'ㄒㄧㄢ ㄒㄧㄢ'),
        ('龗龗', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'),
    )
    TAIL_READINGS = {
        'pinyin': ('dada', 'tata', 'bingbing', 'doudou', 'da', 'xianxian', 'dada'),
        'zhuyin': ('286286', 'w84w84', '1u/31u/3', '2.42.4', '286', 'vu0 vu0 ', '286286'),
    }
    DD, D = ('guess', 'dada', '286286'), ('guess', 'da', '286')
    SHARED = [('add', 0), ('add', 6), ('add', 3), ('add', 1), ('add', 2)]
    # A mask that leaves no item behind, and a lookup that reads no tail, hold on
    # the parent as well.
    TAIL_CONTROLS = ('mask-all', 'lookup')
    MASK_TA, MASK_DA2 = ('mask', 0xffffffff, 0x07000004), ('mask', 0xffffffff, 0x07000002)
    TAIL_SEQUENCES = {
        # The add of the second `da da` item leaves that record in the length-2 entry.
        'add': SHARED + [MASK_TA, ('save',)],
        # A search hit on the record does the same.
        'hit': SHARED + [('save',), DD, MASK_TA, ('add', 5), ('save',)],
        'hit-shrink': SHARED + [('save',), DD, MASK_DA2, ('add', 5), ('save',)],
        # A suggestion walk empties the entries of the longer lengths.
        'suggestion': SHARED + [DD, D, MASK_TA, ('save',)],
        # The entry the first walk leaves is where the second starts.
        'mask-twice': SHARED + [DD, MASK_TA, MASK_DA2, ('save',)],
        # A hit on a record that carries a tail.
        'hit-tailed': SHARED + [DD, MASK_DA2, DD, MASK_TA, ('save',)],
        # One guess hits two different length-2 records, so the last hit the
        # engine makes is the one the walk starts from.
        'hit-order': SHARED + [('save',), ('guess', 'dadatata', '286286w84w84'), MASK_TA, ('add', 5),
                               ('save',)],
        'hit-order-reverse': SHARED + [('save',), ('guess', 'tatadada', 'w84w84286286'), MASK_TA, ('add', 5),
                                       ('save',)],
        # A removal leaves the edited record in the entry.
        'remove': SHARED + [('remove', 3), MASK_TA, ('save',)],
        # Length 1: the prefix markers after the non-empty record.
        'markers': [('add', 4), ('add', 0), ('add', 1), ('add', 2), MASK_TA, ('save',)],
        'markers-hit': [('add', 4), ('add', 0), ('add', 1), ('add', 2), ('save',),
                        ('guess', 'da', '286'), MASK_TA, ('add', 5), ('save',)],
        # The tails come back from the file.
        'reopen': SHARED + [DD, MASK_TA, ('save',), ('reopen',), ('add', 5), ('save',)],
        'reopen-mask': SHARED + [DD, MASK_TA, ('save',), ('reopen',), MASK_DA2, ('add', 5), ('save',)],
        # What a fresh instance shows over the tailed records.
        'lookup': SHARED + [DD, MASK_DA2, ('lookup', 0), ('lookup', 3)],
        # A mask that removes nothing still rewrites every record.
        'mask-none': SHARED + [DD, ('mask', 0xffffffff, 0x07000009), ('save',)],
        'mask-all': SHARED + [DD, ('mask',) + USER_ALL, ('add', 5), ('save',)],
    }
    for _mode in ('pinyin', 'zhuyin'):
        for _name, _steps in TAIL_SEQUENCES.items():
            if _mode == 'zhuyin' and _name.startswith('remove'):
                continue  # the zhuyin facade has no `remove_user_candidate`
            case('issue785-pinyin-tails-%s-%s' % (_name, _mode), mode=_mode, control=_name in TAIL_CONTROLS, issues=('785',))(
                lambda k, steps=_steps: index_key_sequence(k, steps, TAIL_PHRASES, TAIL_READINGS))


    # #814 meets #785: unsaved imports leave their index entries behind a library
    # 7 unload and reload; the item that takes the first token again sits among
    # them in the `da da` record (two items, a tail in the mask walk), and the
    # mask removes an entry no item reading backs. The zhuyin facade only: the
    # pinyin facade never unloads library 7. BDB replaces each mask entry's chunk
    # (chewing_large_table2_bdb.cpp:498-500 at 074a2219), so it carries no #785
    # tail. It is not a parent control: at 7fa8ce3^, unload/reload retains the
    # unsaved items and the new phrase takes token 6, while the pin recreates an
    # empty user library and reuses token 1 (zhuyin.cpp:247-260, :379-388 at
    # 074a2219). The case also covers that #814 defect on BDB.
    case('issue785-pinyin-tails-kept-entry-zhuyin', mode='zhuyin', issues=('785', '814'))(
        lambda k: index_key_sequence(
            k, SHARED + [DD, ('unload',), ('load',), ('add', 5), MASK_DA2, ('save',)],
            TAIL_PHRASES, TAIL_READINGS))


    @case('abort-guess-predicted-short-bigram-value', abort=False)
    def _(k):
        """Predict after 我 with a three-byte value planted under each of its prefix tokens."""
        # `_compute_predicted_bigram_candidates` merges the three-byte gram and
        # `get_total_freq` asserts (`memory_chunk.h:390`, `pinyin.cpp:2330`).
        ctx = predicted_bigram_context(k, '我', b'\x07\x00\x00')
        inst = k.fn('alloc_instance', P, P)(ctx)
        return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


    @case('abort-guess-predicted-empty-bigram-gram', abort=False)
    def _(k):
        """Predict after 我 with a total-seven gram containing no items under its prefix tokens."""
        # An item-less gram with a residual total merges to `total_freq != 0`
        # and `SingleGram::get_length` asserts (`ngram.cpp:70`,
        # `pinyin.cpp:2332`).
        ctx = predicted_bigram_context(k, '我', b'\x07\x00\x00\x00')
        inst = k.fn('alloc_instance', P, P)(ctx)
        return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


    # `_compute_predicted_bigram_candidates` loads only the resolved prefixes'
    # own grams (`pinyin.cpp:2322-2330`), so a faulty row under a token the
    # prediction never resolves to is never read: the call completes. The
    # `我` prefixes do not include `你`'s token, so these are controls for the
    # two faults above — the prefix-scoped guard must not over-refuse.
    @case('guess-predicted-unrelated-short-bigram-value', control=True)
    def _(k):
        """Predict after 我 while the three-byte bigram values are planted under 你's tokens."""
        ctx = predicted_bigram_context(k, '你', b'\x07\x00\x00')
        inst = k.fn('alloc_instance', P, P)(ctx)
        return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}


    @case('guess-predicted-unrelated-empty-bigram-gram', control=True)
    def _(k):
        """Predict after 我 while itemless total-seven grams are planted under 你's tokens."""
        ctx = predicted_bigram_context(k, '你', b'\x07\x00\x00\x00')
        inst = k.fn('alloc_instance', P, P)(ctx)
        return {'ret': k.fn('guess_predicted_candidates', B, P, S)(inst, '我'.encode())}
