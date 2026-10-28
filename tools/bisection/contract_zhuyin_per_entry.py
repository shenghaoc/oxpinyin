"""Zhuyin per-entry contract cases."""
import ctypes as C

from contract_abi import B, I, P, S, U, UNTOUCHED, Z, owned_text, tokens_of, zhuyin_chewing


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def zy_strv(k, pointer):
    """A `gchar **` a keyboard probe answered; decode and free it."""
    if pointer == UNTOUCHED:
        return 'untouched'
    if not pointer:
        return None
    free = k.glib.g_strfreev
    free.restype, free.argtypes = None, [P]
    array, out, index = C.cast(pointer, C.POINTER(S)), [], 0
    while array[index]:
        out.append(array[index].decode('utf-8', 'replace'))
        index += 1
    free(pointer)
    return out


def parse_chewing_probe(schemes, texts):
    """Build a single-key parser probe across the supplied schemes and byte strings.

    A None scheme preserves the current setting. Record the boolean result and
    the output key initialized to the low sixteen bits of UNTOUCHED.
    """
    def probe(k):
        """Set each requested scheme and collect parse_chewing results with fresh sentinels."""
        out = {}
        for scheme in schemes:
            if scheme is not None:
                k.fn('set_chewing_scheme', B, P, I)(k.ctx, scheme)
            parse = k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))
            row, key = {}, C.c_ushort(UNTOUCHED & 0xFFFF)
            for text in texts:
                key.value = UNTOUCHED & 0xFFFF
                row[repr(text)] = [parse(k.inst, text, C.byref(key)), key.value]
            out[str(scheme)] = row
        return out
    return probe


# `parse_one_key` (Simple `zhuyin_parser2.cpp:163-215`, Discrete `:331-...`,
# DaChen `:789-...`) probes the last byte for a tone under `USE_TONE`, maps
# the rest through the scheme's symbol table and looks the chewing up in
# `zhuyin_index`; `*onekey` is written only on success. `su3cl3` holds two
# keys and is refused as one; the empty string and a tone-only byte are
# refused; every implemented scheme is probed.
case('zhuyin-parse-chewing-key', mode='zhuyin', control=True, issues=('563',))(
    parse_chewing_probe((None, 1, 2, 3, 4, 5, 6, 8, 9),
                        (b'su3', b'cl3', b'su', b'', b'zzz', b'su3cl3', b'su3x', b'1', b'3', b'ni')))


def in_chewing_probe(scheme, options, keys):
    """Build a keyboard-membership probe with optional scheme and option changes.

    Return each byte's status and decoded symbol vector, preserving untouched and
    null outputs distinctly and freeing returned vectors through zy_strv.
    """
    def probe(k):
        """Apply optional keyboard settings and record every requested byte's symbols."""
        if scheme is not None:
            k.fn('set_chewing_scheme', B, P, I)(k.ctx, scheme)
        if options is not None:
            k.fn('set_options', B, P, U)(k.ctx, options)
        keyboard = k.fn('in_chewing_keyboard', B, P, C.c_char, C.POINTER(P))
        row = {}
        for key in keys:
            symbols = P(UNTOUCHED)
            ret = keyboard(k.inst, key, C.byref(symbols))
            row[repr(key)] = [ret, zy_strv(k, symbols.value)]
        return row
    return probe


# `in_chewing_scheme` fixes `*symbols = NULL` first and fills it only when
# the byte maps (Simple `zhuyin_parser2.cpp:301-335`, Discrete `:496-540`,
# DaChen `:798-846`); the tone table is consulted only under `USE_TONE`.
case('zhuyin-in-chewing-keyboard-key', mode='zhuyin', control=True, issues=('563',))(
    in_chewing_probe(None, None, (b's', b'u', b'3', b'1', b'a', b'z', b'q', b'!', b'~', b'\x80', b'\x00', b'i')))
case('zhuyin-in-chewing-keyboard-no-tone', mode='zhuyin', control=True, issues=('563',))(
    in_chewing_probe(None, 0, (b's', b'3', b'1')))
for _zyscheme in (2, 3, 4, 5, 6, 8, 9):
    case(f'zhuyin-in-chewing-keyboard-scheme-{_zyscheme}', mode='zhuyin', control=True, issues=('563',))(
        in_chewing_probe(_zyscheme, None, (b's', b'u', b'i', b'3')))


def key_rest_positions_probe(text, offsets):
    """Build a probe of key-rest offsets and all optional begin/end combinations.

    Only call the position accessor after a successful non-null rest lookup.
    zhuyin_get_zhuyin_key_rest_positions permits either output to be null at
    074a2219 zhuyin.cpp:1930-1939.
    """
    # `zhuyin_get_zhuyin_key_rest` on an empty matrix aborts the pin (a
    # registered class (c) site held by `abort-zhuyin-get-zhuyin-key-rest-
    # empty-matrix`), so the accessor is only reachable on a parsed instance;
    # this case takes a key-rest pointer from the getter the pin's own call
    # returns, then exercises the optional `begin`/`end`.
    def probe(k):
        """Parse the chewing text and record rest lookup and optional position outputs.

        Set option word 0x1aa; distinguish untouched, null and set rest pointers for
        each offset and initialize each numeric output to 0xABCD.
        """
        k.fn('set_options', B, P, U)(k.ctx, 0x1aa)
        inst = k.inst
        k.fn('parse_more_chewings', Z, P, S)(inst, text.encode())
        rest = k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))
        positions = k.fn('get_zhuyin_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))
        out = {}
        for offset in offsets:
            slot = P(UNTOUCHED)
            got = rest(inst, offset, C.byref(slot))
            row = {'rest_ret': got,
                   'rest': 'untouched' if slot.value == UNTOUCHED else 'null' if not slot.value else 'set'}
            if got and slot.value:
                # `zhuyin.cpp:1930-1941`: both out-params are optional, neither
                # is written when NULL, and the call always answers true.
                begin, end = C.c_ushort(0xABCD), C.c_ushort(0xABCD)
                row['both'] = [positions(inst, slot, C.byref(begin), C.byref(end)), begin.value, end.value]
                begin = C.c_ushort(0xABCD)
                row['begin-only'] = [positions(inst, slot, C.byref(begin), None), begin.value]
                end = C.c_ushort(0xABCD)
                row['end-only'] = [positions(inst, slot, None, C.byref(end)), end.value]
                row['neither'] = positions(inst, slot, None, None)
            out[str(offset)] = row
        return out
    return probe


case('zhuyin-key-rest-positions-out', mode='zhuyin', control=True, issues=('563',))(
    key_rest_positions_probe('su3cl3', (0, 1, 2)))


def phrase_tokens_probe(texts):
    """Build a phrase-result probe covering empty, segmented and reset instances.

    Record count/token results plus end and UINT_MAX reads. The getters write
    their outputs as shown at 074a2219 zhuyin.cpp:2227-2245.
    """
    def probe(k):
        """Observe each segmentation and a final reset on the same instance."""
        inst = k.inst
        segment = k.fn('phrase_segment', B, P, S)
        n_phrase = k.fn('get_n_phrase', B, P, C.POINTER(U))
        token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))

        def read(prefix):
            """Read count, in-range tokens and two out-of-range slots under prefixed keys.

            An untouched count suppresses the in-range loop; return call results together
            with numeric output values so unchanged sentinels remain visible.
            """
            count = U(UNTOUCHED)
            n_ret = n_phrase(inst, C.byref(count))
            rows, index = [], 0
            while index < (count.value if count.value != UNTOUCHED else 0):
                slot = U(0xDEADBEEF)
                rows.append([token(inst, index, C.byref(slot)), slot.value])
                index += 1
            past = []
            for index in (count.value if count.value != UNTOUCHED else 0, 0xFFFFFFFF):
                slot = U(0xDEADBEEF)
                past.append([token(inst, index, C.byref(slot)), slot.value])
            return {prefix + '-n': [n_ret, count.value], prefix + '-tokens': rows, prefix + '-past': past}

        out = {'empty-instance': read('empty')}
        for text in texts:
            row = {'segment': segment(inst, text)}
            row.update(read('row'))
            out[repr(text)] = row
        segment(inst, '你好'.encode())
        k.fn('reset', B, P)(inst)
        out['after-reset'] = read('reset')
        return out
    return probe


# `zhuyin_get_n_phrase` writes `*num` unconditionally and answers true
# (`zhuyin.cpp:2227-2231`); `zhuyin_get_phrase_token` writes the null token
# before its range check, so a refused index still writes 0 (`:2233-2244`).
# `phrase_segment` fills the array, `reset` empties it (`:1723-1729`).
case('zhuyin-phrase-result-tokens', mode='zhuyin', control=True, issues=('563',))(
    phrase_tokens_probe(('你好'.encode(), b'', b'123', '你好世界'.encode())))


# The two string getters' whole surface: `zhuyin_get_zhuyin_string`
# (`zhuyin.cpp:1732-1741`) renders the key's zhuyin spelling;
# `zhuyin_get_pinyin_string` (`:1743-1766`) reads the context's scheme and
# dispatches (`FULL_PINYIN_HANYU` 1, `LUOMA` 2, `SECONDARY_ZHUYIN` 3). Both
# NULL the out-param before the `get_table_index() == 0` refusal, so a
# refused zero key leaves NULL, not the caller's sentinel.
@case('zhuyin-string-getters', mode='zhuyin', control=True, issues=('563',))
def _(k):
    """Compare zhuyin and pinyin string outputs for a parsed key and a zero key.

    Try the current scheme and schemes 1-3, decoding and freeing owned strings.
    The getters clear the output before refusing a zero key at
    074a2219 zhuyin.cpp:1732-1766.
    """
    inst = k.inst
    key = C.c_ushort(0)
    assert k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(inst, b'su3', C.byref(key))
    zero = C.c_ushort(0)
    out = {}
    for scheme in (None, 1, 2, 3):
        if scheme is not None:
            assert k.fn('set_full_pinyin_scheme', B, P, I)(k.ctx, scheme)
        for label, name in (('zhuyin_string', 'get_zhuyin_string'),
                            ('pinyin_string', 'get_pinyin_string')):
            fn = k.fn(name, B, P, C.POINTER(C.c_ushort), C.POINTER(P))
            string = P(UNTOUCHED)
            out['%s scheme %s' % (label, scheme)] = [
                fn(inst, C.byref(key), C.byref(string)), owned_text(k, string)]
            string = P(UNTOUCHED)
            out['%s scheme %s zero-key' % (label, scheme)] = [
                fn(inst, C.byref(zero), C.byref(string)), owned_text(k, string)]
    return out


# `zhuyin_get_zhuyin_offset` in range: the cursor is clamped to the parse end
# and walked back over empty columns to the first key (`zhuyin.cpp:1973-1992`).
# The past-matrix cursor lives in `zhuyin-cursor-neighbours`.
@case('zhuyin-cursor-offset-range', mode='zhuyin', control=True, issues=('563',))
def _(k):
    """Read five cursor offsets from a parsed zhuyin instance with sentinel outputs."""
    inst = zhuyin_chewing(k)
    fn = k.fn('get_zhuyin_offset', B, P, Z, C.POINTER(Z))
    out = {}
    for cursor in (0, 1, 2, 3, 6):
        res = Z(UNTOUCHED)
        out[str(cursor)] = [fn(inst, cursor, C.byref(res)),
                            'untouched' if res.value == UNTOUCHED else res.value]
    return out


# The five exports whose pin bodies never read their instance:
# `zhuyin_get_zhuyin_string` (`zhuyin.cpp:1732-1741`),
# `zhuyin_get_candidate_type` (`:1858-1863`),
# `zhuyin_get_candidate_string` (`:1865-1870`),
# `zhuyin_get_zhuyin_key_rest_positions` (`:1930-1941`) and
# `zhuyin_get_zhuyin_key_rest_length` (`:1942-1946`). A NULL instance is a
# defined call there, so the subject answers what the pin answers.
@case('zhuyin-null-instance-defined-getters', mode='zhuyin', issues=('563',))
def _(k):
    """Call five instance-independent zhuyin getters with a null instance.

    Obtain real key, rest and candidate pointers first. These getters do not read
    the instance at 074a2219 zhuyin.cpp:1732-1741, zhuyin.cpp:1858-1870 and
    zhuyin.cpp:1930-1946; return their statuses and output values.
    """
    inst = k.inst
    k.fn('parse_more_chewings', Z, P, S)(inst, b'su3cl3')
    key = C.c_ushort(0)
    assert k.fn('parse_chewing', B, P, S, C.POINTER(C.c_ushort))(inst, b'su3', C.byref(key))
    rest = P(UNTOUCHED)
    assert k.fn('get_zhuyin_key_rest', B, P, Z, C.POINTER(P))(inst, 0, C.byref(rest)) and rest.value
    assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    cand = P(UNTOUCHED)
    assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand)) and cand.value
    out = {}
    string = P(UNTOUCHED)
    out['zhuyin_string'] = [k.fn('get_zhuyin_string', B, P, C.POINTER(C.c_ushort), C.POINTER(P))(
        None, C.byref(key), C.byref(string)), owned_text(k, string)]
    begin, end = C.c_ushort(0xABCD), C.c_ushort(0xABCD)
    out['key_rest_positions'] = [k.fn('get_zhuyin_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))(
        None, rest, C.byref(begin), C.byref(end)), begin.value, end.value]
    length = C.c_ushort(0xBEEF)
    out['key_rest_length'] = [k.fn('get_zhuyin_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(
        None, rest, C.byref(length)), length.value]
    kind = I(0x7FFFFFFF)
    out['candidate_type'] = [k.fn('get_candidate_type', B, P, P, C.POINTER(I))(
        None, cand, C.byref(kind)), kind.value]
    candidate_string = P(UNTOUCHED)
    out['candidate_string'] = [k.fn('get_candidate_string', B, P, P, C.POINTER(P))(
        None, cand, C.byref(candidate_string)),
        'untouched' if candidate_string.value == UNTOUCHED
        else C.string_at(candidate_string.value).decode('utf-8', 'replace')]
    return out


# `token_get_phrase`'s optional out-params: `_token_get_phrase`
# (`zhuyin.cpp:1095-1113`, the zhuyin twin of the shared pinyin helper)
# writes `*len` and `*utf8_str` only when their pointer is non-NULL, so all
# four combinations are defined calls — on a token that resolves and on one
# that is refused before either write.
@case('zhuyin-token-get-phrase-optional-out', mode='zhuyin', issues=('563',))
def _(k):
    """Exercise all optional phrase length/text outputs for known and unknown tokens.

    Preserve untouched markers and release returned text. _token_get_phrase checks
    each output pointer only after finding the item at
    074a2219 zhuyin.cpp:1096-1113.
    """
    fn = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))
    out = {}
    for label, token in (('known', tokens_of(k, '你好')[0]), ('unknown', 0xFFFFFFFF)):
        length, text = U(UNTOUCHED), P(UNTOUCHED)
        both = fn(k.inst, token, C.byref(length), C.byref(text))
        out[label] = [both, 'untouched' if length.value == UNTOUCHED else length.value,
                      'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        length = U(UNTOUCHED)
        out[label + '-len-only'] = [fn(k.inst, token, C.byref(length), None), length.value]
        text = P(UNTOUCHED)
        out[label + '-text-only'] = [fn(k.inst, token, None, C.byref(text)),
                                     'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        out[label + '-neither'] = fn(k.inst, token, None, None)
    return out


def register(driver_case):
    """Register the declarations in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
