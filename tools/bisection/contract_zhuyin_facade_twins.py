"""Zhuyin facade twin contract cases."""
import ctypes as C

from contract_abi import Arr, B, I, P, S, U, UNTOUCHED, Z, tokens_of, zhuyin_chewing, zhuyin_sentence_out


def register(case, *, Rest):
    """Register these cases at their original driver position."""
    # batch2 group 14: the zhuyin twins of the batch-1 contracts (PR 14, #542)
    # A populated USER_FILE token follows the facade's unload contract. Pinyin
    # refuses index 7; zhuyin unloads it and its phrase-item reads fail until reload
    # (074a2219 pinyin.cpp:466-474, zhuyin.cpp:378-388, phrase_index.h:646-657).
    def user_library_token_unload(k):
        """Snapshot a populated library-7 token before unloading and after reloading it.

        Import 你好 with the facade's reading, assert one user token, save, and
        record phrase and pronunciation getters around unload/load. Lookup arrays
        remain allocated; the kit retains its context and instance."""
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
        reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
        added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
        k.fn('end_add_phrases', None, P)(it)
        tokens = [token for token in tokens_of(k, '你好') if token >> 24 == 7]
        assert added and len(tokens) == 1, 'a populated index-7 token is required'
        token = tokens[0]
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]

        def reads():
            """Read the seeded token's phrase and pronunciation outputs, retaining sentinels.

            Copy 16-bit pronunciation keys as hex and decode/free returned phrase text.
            The pronunciation GArray is not freed."""
            keys = new(0, 0, 2)
            ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, 0, keys)
            view = C.cast(keys, C.POINTER(Arr)).contents
            content = C.string_at(view.data, view.len * 2).hex() if view.len else ''
            length, phrase = U(UNTOUCHED), P(UNTOUCHED)
            phrase_ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
                k.inst, token, C.byref(length), C.byref(phrase))
            count = U(UNTOUCHED)
            count_ret = k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(
                k.inst, token, C.byref(count))
            return dict(nth=[ret, view.len, content],
                        phrase=[phrase_ret, length.value,
                                'untouched' if phrase.value == UNTOUCHED else k.text(phrase.value)],
                        n=[count_ret, count.value])

        out = {'before': reads(), 'save': k.fn('save', B, P)(k.ctx)}
        out['unload'] = k.fn('unload_phrase_library', B, P, C.c_ubyte)(k.ctx, 7)
        out['unloaded'] = reads()
        out['load'] = k.fn('load_phrase_library', B, P, C.c_ubyte)(k.ctx, 7)
        out['reloaded'] = reads()
        return out


    case('user-library-token-unload-pinyin', control=True, issues=('542',))(user_library_token_unload)
    case('user-library-token-unload-zhuyin', mode='zhuyin', issues=('542',))(user_library_token_unload)


    @case('zhuyin-sentence-before-guess', mode='zhuyin', issues=('542',))
    def _(k):
        """Compare zhuyin sentence outputs before parse, after guess, after reset and on a keyless parse."""
        out = {}
        inst = k.alloc()
        out['fresh'] = zhuyin_sentence_out(k, inst)
        k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
        out['parsed'] = zhuyin_sentence_out(k, inst)
        k.fn('guess_sentence', B, P)(inst)
        out['guessed'] = zhuyin_sentence_out(k, inst)
        k.fn('reset', B, P)(inst)
        out['reset'] = zhuyin_sentence_out(k, inst)
        inst = k.alloc()
        k.fn('parse_more_chewings', Z, P, S)(inst, b"'")
        out['keyless'] = [k.fn('guess_sentence', B, P)(inst), zhuyin_sentence_out(k, inst)]
        return out


    @case('zhuyin-character-offset-out', mode='zhuyin', issues=('542',))
    def _(k):
        """Record zhuyin character-offset flags and output sentinels for the listed parse/phrase pairs."""
        fn = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))
        out = {}
        for label, text, phrase, offset in (
                ('recursion-fails-1', b'su3cl3', '啊', 3), ('recursion-fails-2', b'su3cl3', '你', 6),
                ('hit', b'su3cl3', '你好', 5), ('reserved-slot', b'su3cl3', '你好', 6), ('reserved-slot-1', b'su3cl3', '你', 6), ('no-token', b'su3cl3', 'x', 2), ('empty-phrase', b'su3cl3', '', 2),
                ('no-parse', b'', '你好', 0), ('keyless', b"'", '你好', 0)):
            inst = k.alloc()
            k.fn('parse_more_chewings', Z, P, S)(inst, text)
            length = Z(UNTOUCHED)
            ret = fn(inst, phrase.encode(), offset, C.byref(length))
            out[label] = [ret, 'untouched' if length.value == UNTOUCHED else length.value]
        return out


    @case('zhuyin-token-get-phrase-out', mode='zhuyin', issues=('542',))
    def _(k):
        """Read zhuyin phrase outputs for three invalid tokens, preserving untouched sentinels.

        Decode and free any string actually returned."""
        out = {}
        for token in (0xFFFFFFFF, 0x0DEADBEE, 0x01000000):
            length, text = U(UNTOUCHED), P(UNTOUCHED)
            ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
                k.inst, token, C.byref(length), C.byref(text))
            out[hex(token)] = [ret, 'untouched' if length.value == UNTOUCHED else length.value,
                               'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        return out


    @case('zhuyin-nth-pronunciation-range', mode='zhuyin', issues=('542',))
    def _(k):
        """Observe zhuyin pronunciation bounds and an unknown-token call on a seeded array.

        Compare returned key bytes only for in-range indices; retain other bytes
        under diagnostic-only keys. The allocated GArrays remain live."""
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        arr = new(0, 0, 4)
        k.fn('lookup_tokens', B, P, S, P)(k.inst, '你好'.encode(), arr)
        view = C.cast(arr, C.POINTER(Arr)).contents
        token = C.cast(view.data, C.POINTER(U))[:view.len][0]
        count = U(UNTOUCHED)
        k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
        out = {'n': count.value}
        for nth in (0, count.value - 1, count.value, count.value + 1, 0xFFFFFFFF):
            keys = new(0, 0, 2)
            ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
            kview = C.cast(keys, C.POINTER(Arr)).contents
            content = C.string_at(kview.data, kview.len * 2).hex() if kview.len else ''
            in_range = nth < count.value
            out['nth %d' % nth] = [ret, kview.len] + ([content] if in_range else [])
            out['~content nth %d' % nth] = content
        append = k.glib.g_array_append_vals
        append.restype, append.argtypes = P, [P, P, U]
        keys = new(0, 0, 2)
        seed = C.c_ushort(0x1234)
        append(keys, C.byref(seed), 1)
        ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, 0xFFFFFFFF, 0, keys)
        out['unknown token'] = [ret, C.cast(keys, C.POINTER(Arr)).contents.len]
        return out


    @case('zhuyin-static-key-slots', mode='zhuyin', issues=('542',))
    def _(k):
        """Compare key/rest pointers across two zhuyin instances and read them after freeing the first.

        Copy two key bytes and four rest bytes without taking ownership of either pointer."""
        first, second = k.alloc(), k.alloc()
        k.fn('parse_more_chewings', Z, P, S)(first, b'su3')
        k.fn('parse_more_chewings', Z, P, S)(second, b'cl3')
        key = k.fn('get_zhuyin_key', B, P, Z, C.POINTER(P))
        rest = k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))
        a, b, ra, rb = P(), P(), P(), P()
        out = {'ret': [key(first, 0, C.byref(a)), key(second, 0, C.byref(b)),
                       rest(first, 0, C.byref(ra)), rest(second, 0, C.byref(rb))]}
        out['key same pointer'] = a.value == b.value
        out['rest same pointer'] = ra.value == rb.value
        out['first key now'] = C.string_at(a.value, 2).hex()
        out['first rest now'] = C.string_at(ra.value, 4).hex()
        k.fn('free_instance', None, P)(first)
        out['key after free'] = C.string_at(a.value, 2).hex()
        out['rest after free'] = C.string_at(ra.value, 4).hex()
        return out


    @case('zhuyin-key-rest-length', mode='zhuyin', issues=('542',))
    def _(k):
        """Record 16-bit zhuyin rest lengths for forward, reversed and boundary endpoint pairs."""
        fn = k.fn('get_zhuyin_key_rest_length', B, P, C.POINTER(Rest), C.POINTER(C.c_ushort))
        out = {}
        for begin, end in ((0, 2), (5, 2), (2, 2), (0, 65535), (65535, 0), (1, 65535)):
            length = C.c_ushort(0xBEEF)
            rest = Rest(begin, end)
            out['%d..%d' % (begin, end)] = [fn(k.inst, C.byref(rest), C.byref(length)), length.value]
        return out


    @case('zhuyin-unload-phrase-library-repeat', mode='zhuyin', issues=('542',))
    def _(k):
        """Record zhuyin unload results for libraries zero through fifteen, then library two twice."""
        unload = k.fn('unload_phrase_library', B, P, U)
        return {'unload': [unload(k.ctx, i) for i in range(0, 16)] + [unload(k.ctx, i) for i in (2, 2)]}


    def zhuyin_candidates_at_start(k):
        """Return the reported candidate count after guessing at the start of su3cl3.

        Allocate a fresh parsed zhuyin instance and leave it live; getter flags are
        not checked."""
        inst = zhuyin_chewing(k)
        k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
        count = U()
        k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
        return count.value


    @case('zhuyin-unload-phrase-library-effect', mode='zhuyin', issues=('542',))
    def _(k):
        """Compare fresh zhuyin candidate counts around unloading and reloading libraries 3, 2 and 4."""
        unload = k.fn('unload_phrase_library', B, P, C.c_ubyte)
        load = k.fn('load_phrase_library', B, P, C.c_ubyte)
        out = {'before': zhuyin_candidates_at_start(k)}
        for index in (3, 2, 4):
            out['unload %d' % index] = [unload(k.ctx, index), zhuyin_candidates_at_start(k)]
            out['load %d' % index] = [load(k.ctx, index), zhuyin_candidates_at_start(k)]
            out['load %d again' % index] = load(k.ctx, index)
        return out


    @case('zhuyin-alloc-instance-after-fini', mode='zhuyin', issues=('542',))
    def _(k):
        """Attempt zhuyin instance allocation after finalizing its context.

        Record the live allocation flag and keep the post-finalization result under
        a diagnostic-only key; the attempted allocation's pointer is not freed."""
        # As the pinyin case: the pin reads the freed context and survives by
        # chance, the ruled answer is NULL (class (b)); the case holds the exit
        # status and leaves the answer out.
        context = k.init()
        live = k.fn('alloc_instance', P, P)(context)
        k.fn('free_instance', None, P)(live)
        k.fn('fini', None, P)(context)
        after = k.fn('alloc_instance', P, P)(context)
        return {'live instance': bool(live), '~instance after fini': bool(after)}
