"""Scheme, library and iterator refusal cases."""
import ctypes as C

from contract_abi import B, I, P, U, UNTOUCHED


def ctx_call(name, res, *args):
    """Build a context-call case from a return type and (ctypes type, value) pairs.

    Capture the export name and arguments without loading a library or calling it."""
    def run(k):
        """Call the captured export on the cached context and return its result as ret."""
        return {'ret': k.fn(name, res, P, *[t for t, _ in args])(k.ctx, *[v for _, v in args])}
    return run


def register_pinyin(case):
    """Register the pinyin refusals at their original position."""
    for _name, _call in (
            ('set-full-pinyin-scheme-0', ctx_call('set_full_pinyin_scheme', B, (I, 0))),
            ('set-full-pinyin-scheme-4', ctx_call('set_full_pinyin_scheme', B, (I, 4))),
            ('set-double-pinyin-scheme-30', ctx_call('set_double_pinyin_scheme', B, (I, 30))),
            ('set-zhuyin-scheme-0', ctx_call('set_zhuyin_scheme', B, (I, 0))),
            ('set-zhuyin-scheme-10', ctx_call('set_zhuyin_scheme', B, (I, 10))),
            ('set-zhuyin-scheme-7', ctx_call('set_zhuyin_scheme', B, (I, 7))),
            ('load-phrase-library-0', ctx_call('load_phrase_library', B, (C.c_ubyte, 0))),
            ('load-phrase-library-8', ctx_call('load_phrase_library', B, (C.c_ubyte, 8))),
            ('unload-phrase-library-16', ctx_call('unload_phrase_library', B, (C.c_ubyte, 16))),
            ('unload-addon-phrase-library-16', ctx_call('unload_addon_phrase_library', B, (C.c_ubyte, 16)))):
        case('abort-' + _name, abort=False, sigabrt=True, issues=('525',))(_call)


    @case('abort-iterator-get-next-phrase', abort=False, sigabrt=True, issues=('525',))
    def _(k):
        """Request a phrase from an empty library-7 iterator and record both iterator flags.

        Call get-next even when has-next is false; output pointers are not observed
        or freed, and the iterator is left allocated for the isolated worker."""
        # An empty user dictionary: the begin probes no next token.
        it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
        phrase, pinyin, count = P(), P(), I()
        return {'has next': k.fn('iterator_has_next_phrase', B, P)(it),
                'ret': k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
                    it, C.byref(phrase), C.byref(pinyin), C.byref(count))}


    # #681, row 62: the bigram twin. A fresh user dir has an empty user bigram,
    # so the iterator's predecessor is still null_token and the pin asserts
    # (`pinyin.cpp:902`) before it writes anything. A control: the parent build
    # already answers this way; the case registers behaviour, it changes none.
    @case('abort-bigram-iterator-get-next-phrase', abort=False, sigabrt=True, issues=('681', 'row:62'))
    def _(k):
        """Request a row from a fresh bigram iterator and check that all outputs stay untouched.

        Record has-next, the get-next flag and separate sentinel checks for phrase,
        reading and count, even when no next row is reported."""
        it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
        phrase, pinyin, count = P(UNTOUCHED), P(UNTOUCHED), I(UNTOUCHED)
        has_next = k.fn('bigram_iterator_has_next_phrase', B, P)(it)
        ret = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
            it, C.byref(phrase), C.byref(pinyin), C.byref(count))
        return {'has next': has_next, 'ret': ret,
                'untouched phrase': phrase.value == UNTOUCHED,
                'untouched pinyin': pinyin.value == UNTOUCHED,
                'untouched count': count.value == UNTOUCHED}


    # The in-range neighbours answer without a warning, as at the pin.
    @case('scheme-and-library-neighbours', control=True, issues=('525',))
    def _(k):
        """Record scheme changes and library load/unload flags for the listed boundary neighbors.

        Apply all operations in order to the same context, retaining its mutations."""
        out = {}
        for label, name, res, arg in (
                ('full 1', 'set_full_pinyin_scheme', B, (I, 1)), ('full 3', 'set_full_pinyin_scheme', B, (I, 3)),
                ('double 6', 'set_double_pinyin_scheme', B, (I, 6)), ('double 99', 'set_double_pinyin_scheme', B, (I, 99)),
                ('zhuyin 1', 'set_zhuyin_scheme', B, (I, 1)), ('zhuyin 9', 'set_zhuyin_scheme', B, (I, 9)),
                ('load 1', 'load_phrase_library', B, (C.c_ubyte, 1)), ('load 7', 'load_phrase_library', B, (C.c_ubyte, 7)),
                ('load 16', 'load_phrase_library', B, (C.c_ubyte, 16)), ('load 255', 'load_phrase_library', B, (C.c_ubyte, 255)),
                ('unload 1', 'unload_phrase_library', B, (C.c_ubyte, 1)), ('unload 15', 'unload_phrase_library', B, (C.c_ubyte, 15)),
                ('unload addon 15', 'unload_addon_phrase_library', B, (C.c_ubyte, 15)),
                ('load addon 16', 'load_addon_phrase_library', B, (C.c_ubyte, 16))):
            out[label] = k.fn(name, res, P, arg[0])(k.ctx, arg[1])
        return out


def register_zhuyin(case):
    """Register the zhuyin refusals at their original position."""
    # batch2 group 12f: the zhuyin twins (PR 12f, #525)
    for _name, _call in (
            ('chewing-scheme-0', ctx_call('set_chewing_scheme', B, (I, 0))),
            ('chewing-scheme-7', ctx_call('set_chewing_scheme', B, (I, 7))),
            ('chewing-scheme-10', ctx_call('set_chewing_scheme', B, (I, 10))),
            ('chewing-scheme-minus-1', ctx_call('set_chewing_scheme', B, (I, -1))),
            ('full-pinyin-scheme-0', ctx_call('set_full_pinyin_scheme', B, (I, 0))),
            ('full-pinyin-scheme-4', ctx_call('set_full_pinyin_scheme', B, (I, 4))),
            ('load-phrase-library-0', ctx_call('load_phrase_library', B, (C.c_ubyte, 0))),
            ('load-phrase-library-8', ctx_call('load_phrase_library', B, (C.c_ubyte, 8))),
            ('unload-phrase-library-16', ctx_call('unload_phrase_library', B, (C.c_ubyte, 16))),
            ('unload-phrase-library-255', ctx_call('unload_phrase_library', B, (C.c_ubyte, 255)))):
        case('abort-zhuyin-' + _name, mode='zhuyin', abort=False, issues=('525',))(_call)
