"""Zero-key guess cases (inventory row 44, #1175).
"""
import ctypes as C

from contract_abi import B, P, S, U, UNTOUCHED, Z, owned_text


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #902: separators consume bytes without keys (074a2219,
# storage/pinyin_parser2.cpp:238-250,360-378). fill_matrix clears the matrix
# before refusing zero keys (storage/phonetic_key_matrix.cpp:34-38).
# Sentence lookup refuses before clearing results (lookup/phonetic_lookup.h:
# 743-748); candidate lookup clears candidates before refusing (pinyin.cpp:
# 2193-2196; zhuyin.cpp:1468-1471,1548-1551). Observe each export separately,
# both fresh and after a successful guess, including its subsequent state.
def zero_key_guess_case(text, options, operation):
    """Build a case comparing a guess operation on fresh and previously guessed instances.

    Capture the input bytes, optional context options and operation name.
    The returned case supports sentence guesses and the facade-specific
    candidate guesses used by the registrations below."""
    def probe(k):
        """Record parse, guess, sentence and candidate state for both seeding conditions.

        Assert an explicitly requested option update and the seed sentence guess.
        Parse text on a new instance for each condition, then call the selected
        guess export at offset zero or with the prefix 你. Return consumed and
        parsed lengths in bytes, the guess flag, and the sentence/count accessor
        outputs, preserving untouched sentinels and freeing returned sentence text.
        Neither instance is freed here."""
        if options is not None:
            assert k.fn('set_options', B, P, U)(k.ctx, options)
        out = {}
        for seeded in (False, True):
            inst = k.alloc()
            if seeded:
                k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni3hao3')
                assert k.fn('guess_sentence', B, P)(inst)
            parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
            if operation == 'guess_sentence':
                ret = k.fn(operation, B, P)(inst)
            elif operation == 'guess_sentence_with_prefix':
                ret = k.fn(operation, B, P, S)(inst, '你'.encode())
            elif operation == 'guess_candidates':
                ret = k.fn(operation, B, P, Z, U)(inst, 0, 0x1e)
            else:
                ret = k.fn(operation, B, P, Z)(inst, 0)
            # Neither facade exports a sentence-count accessor. Read its
            # public sentence result and candidate count without inventing one.
            sentence = P(UNTOUCHED)
            if k.mode == 'pinyin':
                got_sentence = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(
                    inst, 0, C.byref(sentence))
            else:
                got_sentence = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(sentence))
            count = U(UNTOUCHED)
            got_count = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            out['seeded' if seeded else 'fresh'] = dict(
                consumed=parsed,
                parsed_length=k.fn('get_parsed_input_length', Z, P)(inst),
                ret=ret, sentence=[got_sentence, owned_text(k, sentence)],
                candidates=[got_count, count.value])
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _operations = ('guess_sentence', 'guess_sentence_with_prefix') + (
        ('guess_candidates',) if _mode == 'pinyin' else
        ('guess_candidates_after_cursor', 'guess_candidates_before_cursor'))
    for _options in (None, 0x20, 0x18a):
        for _text in (b"'", b"''", b"'''"):
            for _operation in _operations:
                _label = 'default' if _options is None else hex(_options)
                case(f'zero-key-guess/{_mode}/{_label}/{_text.decode()}/{_operation}',
                     mode=_mode, control=_mode == 'pinyin', issues=('902',))(
                    zero_key_guess_case(_text, _options, _operation))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
