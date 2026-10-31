"""C string inputs cases (inventory row 61, #1175).
"""
import ctypes as C

from contract_abi import Arr, B, I, P, S, U, UNTOUCHED, Z, tokens_of
from contract_case_helpers import PREFIX_WORDS, glib_utf8


def register(case, *, items_system):
    """Register these cases at their original driver position."""
    # 074a2219 storage/pinyin_parser2.cpp:169-174 checks raw bytes for an
    # apostrophe before zeroing the key, including after invalid UTF-8 (#1279).
    def c_string_full_key_apostrophe(spelling):
        """Observe the assertion refusal and whether the output key is untouched."""
        def probe(k):
            key = C.c_uint16(UNTOUCHED & 0xffff)
            ret = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(
                k.inst, spelling, C.byref(key))
            return {'ret': ret, 'untouched key': key.value == UNTOUCHED & 0xffff}
        return probe


    def c_string_full_key(spelling):
        """Record the flag and the key a malformed spelling leaves behind."""
        def probe(k):
            key = C.c_uint16(UNTOUCHED & 0xffff)
            ret = k.fn('parse_full_pinyin', B, P, S, C.POINTER(C.c_uint16))(
                k.inst, spelling, C.byref(key))
            return {'ret': ret, 'key': 'untouched' if key.value == UNTOUCHED & 0xffff else key.value}
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        # pinyin_parser2.cpp:169-175: no apostrophe, so the key is zeroed, then
        # the index search fails on the malformed byte.
        # Both match the parent: regression guards for the covered refusal.
        for _variant, _spelling in (('valid', b'ni3'), ('malformed', b'n\xff3')):
            case('c-string-input/%s-full-key-%s' % (_mode, _variant), mode=_mode,
                 control=True, issues=('1279',))(c_string_full_key(_spelling))


    for _mode in ('pinyin', 'zhuyin'):
        for _variant, _spelling in (('valid', b"n'i"), ('invalid', b"ni\xff'")):
            case('c-string-input/%s-full-key-apostrophe-%s' % (_mode, _variant),
                 mode=_mode, abort=False, sigabrt=True, issues=('1279',))(
                     c_string_full_key_apostrophe(_spelling))


    # U conversion accepts raw words; _add_phrase writes them verbatim
    # (074a2219 pinyin.cpp:628,585-607; zhuyin.cpp:513,472-494).
    def c_string_words(operation, variant):
        """Exercise accepted input words through import, lookup or remember."""
        def probe(k):
            # g_utf8_to_ucs4 fails on malformed input with zero items written
            # (gutf8.c:938-1000); NULL is refused by the entry's own guard
            # (pinyin.cpp:620-622,3674-3675; zhuyin.cpp:466-468).
            word = PREFIX_WORDS.get(variant, PREFIX_WORDS['valid'])
            words = (0x7532, word, 0x4e59)
            items_system(k, [(words, (0x1084,) * 3, 100)] +
                         [((value,), (0x1084,), 100) for value in words], total=400)
            text = {'invalid': '甲'.encode() + b'\xff' + '乙'.encode(), 'null': None}.get(
                variant, None if variant == 'null' else glib_utf8(words))
            if operation == 'import':
                iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
                reading = b"da1'da1'da1" if k.mode == 'pinyin' else 'ㄉㄚ ㄉㄚ ㄉㄚ'.encode()
                ret = k.fn('iterator_add_phrase', B, P, S, S, I)(iterator, text, reading, 5)
                k.fn('end_add_phrases', None, P)(iterator)
            elif operation == 'remember':
                assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
                parsed = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'da1da1da1')
                assert parsed == 9
                ret = k.fn('remember_user_input', B, P, S, I)(k.inst, text, 5)
            else:
                ret = None
            if text is None:
                return {'ret': ret}
            new = k.glib.g_array_new
            new.restype, new.argtypes = P, [I, I, U]
            array = new(0, 0, 4)
            try:
                found = k.fn('lookup_tokens', B, P, S, P)(k.inst, text, array)
                view = C.cast(array, C.POINTER(Arr)).contents
                tokens = list(C.cast(view.data, C.POINTER(U))[:view.len])
            finally:
                k.glib.g_array_free.argtypes = [P, I]
                k.glib.g_array_free(array, 1)
            return {'ret': ret, 'found': found, 'tokens': tokens}
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        for _operation in ('lookup', 'import', 'remember'):
            if _operation == 'remember' and _mode == 'zhuyin':
                continue
            for _variant in (*PREFIX_WORDS, 'invalid', 'null'):
                if _variant == 'null' and _operation == 'lookup':
                    continue  # NULL lookup is the UB case in c_string_null_state
                case('c-string-input/%s-%s-%s' % (_mode, _operation, _variant),
                     mode=_mode, control=_variant in ('valid', 'invalid', 'null'), issues=('1279',))(
                         c_string_words(_operation, _variant))


    def c_string_direct_reading(variant):
        """A failed later segment retains the earlier direct-zhuyin key."""
        def probe(k):
            reading = {'valid': 'ㄉㄚ'.encode(),
                       'later-invalid': 'ㄉㄚ '.encode() + b'\xff',
                       'same-invalid': 'ㄉㄚ'.encode() + b'\xff'}[variant]
            iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
            ret = k.fn('iterator_add_phrase', B, P, S, S, I)(
                iterator, '甲'.encode(), reading, 5)
            k.fn('end_add_phrases', None, P)(iterator)
            return {'ret': ret, 'tokens': tokens_of(k, '甲')}
        return probe


    for _variant in ('valid', 'later-invalid', 'same-invalid'):
        case('c-string-input/zhuyin-direct-' + _variant, mode='zhuyin',
             control=_variant != 'later-invalid', issues=('1279',))(
                 c_string_direct_reading(_variant))


    def c_string_null_state(operation, is_null):
        """Seed visible state before testing a NULL or non-empty valid argument."""
        def probe(k):
            assert k.fn('phrase_segment', B, P, S)(k.inst, '你好'.encode())
            if operation == 'segment':
                ret = k.fn('phrase_segment', B, P, S)(k.inst, None if is_null else '你好'.encode())
                count = U(UNTOUCHED)
                k.fn('get_n_phrase', B, P, C.POINTER(U))(k.inst, C.byref(count))
                return {'ret': ret, 'count': count.value}
            new = k.glib.g_array_new
            new.restype, new.argtypes = P, [I, I, U]
            array = new(0, 0, 4)
            try:
                assert k.fn('lookup_tokens', B, P, S, P)(k.inst, '你'.encode(), array)
                ret = k.fn('lookup_tokens', B, P, S, P)(k.inst, None if is_null else '你'.encode(), array)
                return {'ret': ret, 'count': C.cast(array, C.POINTER(Arr)).contents.len}
            finally:
                k.glib.g_array_free.argtypes = [P, I]
                k.glib.g_array_free(array, 1)
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        for _operation in ('segment', 'lookup'):
            for _null in (False, True):
                # g_utf8_strlen's critical (gutf8.c:240) precedes the NULL
                # dereference at gutf8.c:950; both sides' GLib-domain line is
                # compared (GLib 2.84.4 CRITICAL = 8).
                _expected = {'ret': _operation == 'segment', 'count': 0,
                             'logs': [['GLib', 8]] if _operation == 'segment' else []}
                case('c-string-input/%s-%s-state-%s' % (_mode, _operation, 'null' if _null else 'valid'),
                     mode=_mode, control=not _null, pin_not_run=_expected if _null else None,
                     issues=('1279',))(c_string_null_state(_operation, _null))


    def c_string_parse_null(entry, is_null):
        """Seed parse state and a key sentinel before a NULL or valid call."""
        def probe(k):
            seed, seed_text = (('parse_more_full_pinyins', b'nihao') if k.mode == 'pinyin'
                               else ('parse_more_chewings', b'su3cl3'))
            assert k.fn(seed, Z, P, S)(k.inst, seed_text) > 0
            if entry.startswith('parse_more_'):
                text = b'su3' if entry == 'parse_more_chewings' else b'ni3'
                ret = k.fn(entry, Z, P, S)(k.inst, None if is_null else text)
                length = k.fn('get_parsed_input_length', Z, P)(k.inst)
                return {'ret': ret, 'length': length}
            key = C.c_uint16(UNTOUCHED & 0xffff)
            text = b'su3' if entry == 'parse_chewing' else b'ni3'
            ret = k.fn(entry, B, P, S, C.POINTER(C.c_uint16))(
                k.inst, None if is_null else text, C.byref(key))
            return {'ret': ret, 'untouched key': key.value == UNTOUCHED & 0xffff}
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        _entries = ['parse_full_pinyin', 'parse_chewing',
                    'parse_more_full_pinyins', 'parse_more_chewings']
        if _mode == 'pinyin':
            _entries += ['parse_double_pinyin', 'parse_more_double_pinyins']
        for _entry in _entries:
            for _null in (False, True):
                _expected = ({'ret': 0, 'length': 0, 'logs': []}
                             if _entry.startswith('parse_more_') else
                             {'ret': False, 'untouched key': True, 'logs': []})
                case('c-string-input/%s-%s-%s' % (_mode, _entry, 'null' if _null else 'valid'),
                     mode=_mode, control=True,
                     pin_not_run=_expected if _null else None, issues=('1279',))(
                         c_string_parse_null(_entry, _null))


    # pinyin.cpp:3216-3243 / zhuyin.cpp:2183-2209 convert the phrase with
    # g_utf8_to_ucs4 and look up each converted word, so a canonical non-scalar
    # word reaches the offset walk like any scalar (#1279).
    def c_string_character_offset(variant):
        """Offset of a two-word phrase whose first word is `variant`'s."""
        def probe(k):
            word = PREFIX_WORDS.get(variant, PREFIX_WORDS['valid'])
            items_system(k, [((word,), (0x3084,), 100), ((0x4e19,), (0x3084,), 100)], total=200)
            inst = k.inst
            parse, typed = 'parse_more_full_pinyins', b'da3da3'
            assert k.fn(parse, Z, P, S)(inst, typed) > 0
            phrase = {'invalid': '甲'.encode() + b'\xff', 'null': None}.get(
                variant.split('-past')[0], glib_utf8((word, 0x4e19)))
            length = Z(UNTOUCHED)
            past = variant.endswith('-past-matrix')
            ret = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(
                inst, phrase, 99 if past else 5, C.byref(length))
            if past:  # the pin's offset asserts fire before the phrase is read
                return {'ret': ret}
            return {'ret': ret, 'length': 'untouched' if length.value == UNTOUCHED else length.value}
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        for _variant in ('valid', 'invalid', 'null', 'surrogate', 'above-unicode', 'five-byte', 'six-byte'):
            case('c-string-input/%s-character-offset-%s' % (_mode, _variant), mode=_mode,
                 control=_variant in ('valid', 'invalid', 'null'), issues=('1279',))(
                     c_string_character_offset(_variant))
        for _variant in ('null-past-matrix', 'invalid-past-matrix'):
            # pinyin.cpp:3200-3214 / zhuyin.cpp:2155-2167: assert(offset < size).
            case('c-string-input/%s-character-offset-%s' % (_mode, _variant), mode=_mode,
                 abort=False, sigabrt=True, issues=('1279',))(c_string_character_offset(_variant))
