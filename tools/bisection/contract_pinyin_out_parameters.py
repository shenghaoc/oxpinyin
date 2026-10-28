"""Pinyin return and output-parameter cases."""
import ctypes as C

from contract_abi import Arr, B, I, P, S, U, UNTOUCHED, Z, sentence_out, tokens_of


def register(case, *, Rest):
    """Register the cases at their original position in the driver."""
    # lane C, #542 rows 2, 6, 7, 20: out-params the pin leaves alone, or writes.
    @case('empty-parse-sentence', issues=('542',))
    def _(k):
        """Record sentence access after empty or invalid parsing and on an unparsed instance.

        Attempt a sentence guess in each state and retain its return flag plus
        sentence output; parsed states also include the consumed byte count."""
        out = {}
        for t in (b'', b'!'):
            inst = k.alloc()
            n = k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
            guessed = k.fn('guess_sentence', B, P)(inst)
            out[repr(t)] = [n, guessed, sentence_out(k, inst, 0)]
        inst = k.alloc()
        out['fresh'] = [k.fn('guess_sentence', B, P)(inst), sentence_out(k, inst, 0)]
        return out


    @case('character-offset-out', issues=('542',))
    def _(k):
        """Record character-offset return flags and whether the output length was written.

        Use fresh full- or double-pinyin parses for matching, missing, empty and
        keyless phrase queries. Seed each length with UNTOUCHED and replace only
        an unchanged sentinel with the string "untouched" in the returned mapping."""
        fn = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))
        out = {}
        full, double = 'parse_more_full_pinyins', 'parse_more_double_pinyins'
        for label, parse, text, phrase, offset in (
                ('recursion-fails-1', full, b'nihao', '啊', 3), ('recursion-fails-2', full, b'nihao', '你', 5),
                ('recursion-fails-3', full, b'nihao', '好', 5), ('hit', full, b'nihao', '你好', 5),
                ('no-token', full, b'nihao', 'x', 2), ('empty-phrase', full, b'nihao', '', 2),
                ('no-parse', full, b'', '你好', 0), ('keyless', full, b"'", '你好', 0),
                ('double-fails', double, b'nihk', '啊', 2), ('double-hit', double, b'nihk', '你好', 4)):
            inst = k.alloc()
            k.fn(parse, Z, P, S)(inst, text)
            length = Z(UNTOUCHED)
            ret = fn(inst, phrase.encode(), offset, C.byref(length))
            out[label] = [ret, 'untouched' if length.value == UNTOUCHED else length.value]
        return out


    @case('token-get-phrase-out', issues=('542',))
    def _(k):
        """Query selected token IDs and preserve independent length and text sentinels.

        Return each lookup flag, length output and decoded phrase; free any
        returned non-NULL phrase without dereferencing an untouched pointer."""
        out = {}
        for token in (0xFFFFFFFF, 0x0DEADBEE, 0x01000000):
            length, text = U(UNTOUCHED), P(UNTOUCHED)
            ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
                k.inst, token, C.byref(length), C.byref(text))
            out[hex(token)] = [ret, 'untouched' if length.value == UNTOUCHED else length.value,
                               'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        return out


    @case('nth-pronunciation-range', issues=('542',))
    def _(k):
        """Record pronunciation-array results at valid, boundary and out-of-range indices.

        Use the first token for 你好 and report its pronunciation count. Include
        raw two-byte key contents in ordinary observations only for in-range
        indices; retain every content dump under a tilde-prefixed diagnostic key,
        which observed excludes from comparison. Finally query an unknown token
        using a seeded array and record the resulting length. Native arrays are
        not freed here."""
        token = tokens_of(k, '你好')[0]
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        count = U(UNTOUCHED)
        k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
        out = {'n': count.value}
        for nth in (0, count.value - 1, count.value, count.value + 1, 0xFFFFFFFF):
            keys = new(0, 0, 2)
            ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
            view = C.cast(keys, C.POINTER(Arr)).contents
            content = C.string_at(view.data, view.len * 2).hex() if view.len else ''
            in_range = nth < count.value
            out['nth %d' % nth] = [ret, view.len] + ([content] if in_range else [])
            # Past the last reading the pin returns whatever its stack held; the
            # ruled answer is zeroed keys (class (b) for the content).
            out['~content nth %d' % nth] = content
        # An unknown token still empties the caller's array first.
        append = k.glib.g_array_append_vals
        append.restype, append.argtypes = P, [P, P, U]
        keys = new(0, 0, 2)
        seed = C.c_ushort(0x1234)
        append(keys, C.byref(seed), 1)
        ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, 0xFFFFFFFF, 0, keys)
        out['unknown token'] = [ret, C.cast(keys, C.POINTER(Arr)).contents.len]
        return out


    # lane C, #542 rows 14 and 19: the pin's function-static slots and u16 length.
    @case('static-key-slots', issues=('542',))
    def _(k):
        """Observe key-pointer sharing and contents across two instances and one free.

        Parse ni and hao separately, fetch both key and rest pointers, compare
        addresses and snapshot the first pointers after the second lookup. Free
        the first instance and read those pointers again. This exercises the
        function-static slots in pinyin_get_pinyin_key and pinyin_get_pinyin_key_rest
        in src/pinyin.cpp at pin 074a2219; it assumes the returned pointers can
        be read for two and four bytes respectively."""
        first, second = k.alloc(), k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(first, b'ni')
        k.fn('parse_more_full_pinyins', Z, P, S)(second, b'hao')
        key = k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))
        rest = k.fn('get_pinyin_key_rest', B, P, Z, C.POINTER(P))
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


    @case('key-rest-length', issues=('542',))
    def _(k):
        """Record return flags and 16-bit lengths for ordered, reversed and extreme offsets.

        Seed each length with 0xBEEF before calling get_pinyin_key_rest_length.
        The tested native calculation is _ChewingKeyRest::length in
        src/storage/chewing_key.h, exposed by pinyin_get_pinyin_key_rest_length
        in src/pinyin.cpp at pin 074a2219."""
        fn = k.fn('get_pinyin_key_rest_length', B, P, C.POINTER(Rest), C.POINTER(C.c_ushort))
        out = {}
        for begin, end in ((0, 2), (5, 2), (2, 2), (0, 65535), (65535, 0), (1, 65535)):
            length = C.c_ushort(0xBEEF)
            rest = Rest(begin, end)
            out['%d..%d' % (begin, end)] = [fn(k.inst, C.byref(rest), C.byref(length)), length.value]
        return out


    # lane C, #542 rows 1, 3, 10, 21: return values of sentence and unload.
    @case('sentence-before-guess', issues=('542',))
    def _(k):
        """Observe sentence outputs as one instance is parsed, guessed, reset and reparsed.

        Query indices zero and 255 before the first guess, then index zero after
        each later transition, including a new parse following a guess."""
        out = {}
        inst = k.alloc()
        out['fresh'] = [sentence_out(k, inst, 0), sentence_out(k, inst, 255)]
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        out['parsed'] = [sentence_out(k, inst, 0), sentence_out(k, inst, 255)]
        k.fn('guess_sentence', B, P)(inst)
        out['guessed'] = sentence_out(k, inst, 0)
        k.fn('reset', B, P)(inst)
        out['reset'] = sentence_out(k, inst, 0)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        out['reparsed'] = sentence_out(k, inst, 0)
        # A parse alone keeps the rows of the last guess.
        k.fn('guess_sentence', B, P)(inst)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'xian')
        out['guessed then reparsed'] = sentence_out(k, inst, 0)
        return out


    @case('sentence-before-guess-schemes', issues=('542',))
    def _(k):
        """Read sentence zero after double-pinyin and chewing parses without guessing.

        Use a separate new instance for each parsing scheme."""
        out = {}
        inst = k.alloc()
        k.fn('parse_more_double_pinyins', Z, P, S)(inst, b'nihk')
        out['double'] = sentence_out(k, inst, 0)
        inst = k.alloc()
        k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
        out['chewing'] = sentence_out(k, inst, 0)
        return out


    @case('guess-sentence-keyless', issues=('542',))
    def _(k):
        """Record sentence guesses and outputs for separators, invalid input and a valid prefix.

        Use fresh instances for initial cases, then separately observe keyless
        reparsing after a guess and keyless guessing with the prefix 你. Return
        consumed byte counts for initial cases and preserve sentence sentinels."""
        out = {}
        for t in (b"'", b"''", b'', b'!', b"nihao'"):
            inst = k.alloc()
            n = k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
            ret = k.fn('guess_sentence', B, P)(inst)
            out[repr(t)] = [n, ret, sentence_out(k, inst, 0)]
        # A keyless guess answers false before it clears anything: the rows of
        # the last guess stay.
        for t in (b"'", b'!'):
            inst = k.alloc()
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
            first = k.fn('guess_sentence', B, P)(inst)
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
            out['stale after ' + repr(t)] = [first, k.fn('guess_sentence', B, P)(inst), sentence_out(k, inst, 0)]
            inst = k.alloc()
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
            out['with-prefix ' + repr(t)] = [k.fn('guess_sentence_with_prefix', B, P, S)(inst, '你'.encode()),
                                             sentence_out(k, inst, 0)]
        return out


    @case('guess-candidates-keyless', issues=('542',))
    def _(k):
        """Record candidate-guess flags and whether any candidates remain after parsing.

        For each separator, invalid, empty or valid-prefix input, compare a fresh
        instance with one that has already parsed ni and attempted a sentence
        guess. Guess at byte offset zero with option mask 0x1E; reduce the returned
        candidate count to a nonzero boolean."""
        # `0 == matrix.size()`: the candidates are freed and the answer is
        # false, whatever sentence rows an earlier guess left.
        out = {}
        for t in (b"'", b"'''", b'!', b'', b"ni'"):
            for stale in (False, True):
                inst = k.alloc()
                if stale:
                    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
                    k.fn('guess_sentence', B, P)(inst)
                k.fn('parse_more_full_pinyins', Z, P, S)(inst, t)
                ret = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1E)
                count = U(UNTOUCHED)
                k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
                out['%r stale=%s' % (t, stale)] = [ret, count.value > 0]
        return out


    @case('unload-phrase-library-repeat', issues=('542',))
    def _(k):
        """Return unload flags for repeated library 2 requests interleaved with other IDs.

        Issue all requests against the same cached context in the listed order."""
        unload = k.fn('unload_phrase_library', B, P, U)
        return {'unload': [unload(k.ctx, i) for i in (2, 2, 2, 1, 3, 15, 2)]}


    @case('sentence-with-prefix-bytes', issues=('542',))
    def _(k):
        """Observe prefix-based guessing with valid, empty and invalid UTF-8 prefixes.

        Parse nihao on a fresh instance for each prefix, then return the guess
        flag and sentence outputs at indices zero and one."""
        out = {}
        for label, prefix in (('invalid', b'\xff\xfe'), ('valid', '你'.encode()), ('empty', b''),
                              ('tail-invalid', '你'.encode() + b'\xff')):
            inst = k.alloc()
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
            ret = k.fn('guess_sentence_with_prefix', B, P, S)(inst, prefix)
            out[label] = [ret, sentence_out(k, inst, 0), sentence_out(k, inst, 1)]
        return out
