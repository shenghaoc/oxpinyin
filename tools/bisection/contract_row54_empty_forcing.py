"""Empty parse, unload and forcing cases for row 54."""
import ctypes as C

from contract_abi import B, I, P, S, U, UNTOUCHED, Z


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #695, row 54: parse replaces the matrix even for empty input; every
# candidate guess searches the currently loaded libraries without a reparse.
def row54_empty(parse_name, text):
    """Build a case that guesses after the original, empty and restored byte input."""
    def probe(k):
        """Set tone options and record three parse/guess pairs on one instance.

        Use the supplied parse entry point and retain native failure flags."""
        k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn(parse_name, Z, P, S)
        guess = k.fn('guess_sentence', B, P)
        out = {'first_parse': parse(inst, text), 'first_guess': guess(inst)}
        out['empty_parse'] = parse(inst, b'')
        out['empty_guess'] = guess(inst)
        out['reparse'] = parse(inst, text)
        out['recovered_guess'] = guess(inst)
        return out
    return probe


def row54_unload(direction, reparse):
    """Build a candidate-list comparison around unloading library two.

    Optionally reparse nihao before collecting two post-unload lists. direction
    selects the zhuyin cursor side; pinyin always guesses at offset zero."""
    def probe(k):
        """Parse nihao, collect candidates, unload library two and collect two more lists.

        Use tone options and optional reparse; keep the cached context and instance live."""
        k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn('parse_more_full_pinyins', Z, P, S)
        out = {'parse': parse(inst, b'nihao')}
        def rows():
            """Guess and return flags, count and ordered candidate strings for the selected cursor side.

            Zhuyin before-cursor uses byte five, after-cursor byte zero; pinyin uses
            sort word 0x1e at zero. Assert each candidate/string fetch and decode UTF-8
            strictly without freeing borrowed strings."""
            if k.mode == 'pinyin':
                ret = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
            else:
                ret = k.fn('guess_candidates_' + direction + '_cursor', B, P, Z)(
                    inst, 0 if direction == 'after' else 5)
            count = U(UNTOUCHED)
            counted = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            strings = []
            for index in range(count.value):
                candidate, text = P(), S()
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(candidate))
                assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, candidate, C.byref(text))
                strings.append(text.value.decode())
            return [ret, counted, count.value, strings]
        out['before'] = rows()
        out['unload'] = k.fn('unload_phrase_library', B, P, C.c_ubyte)(k.ctx, 2)
        if reparse:
            out['reparse'] = parse(inst, b'nihao')
        out['after'] = rows()
        out['repeated'] = rows()
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _parser, _text in (('full_pinyins', b'nihao'), ('chewings', b'su3cl3')):
        case(f'row54-empty-{_mode}-{_parser}', mode=_mode, issues=('695', 'row:54'))(
            row54_empty('parse_more_' + _parser, _text))
    for _reparse in (False, True):
        case(f'row54-unload-{_mode}' + ('-reparse' if _reparse else ''),
             mode=_mode, control=_reparse, issues=('695', 'row:54'))(row54_unload('after', _reparse))
case('row54-empty-pinyin-double', issues=('695', 'row:54'))(row54_empty('parse_more_double_pinyins', b'nihk'))
case('row54-unload-zhuyin-before', mode='zhuyin', control=True, issues=('695', 'row:54'))(row54_unload('before', False))



# PR #709 review: keep a non-first NORMAL choice through an empty parse
# until a sentence guess validates the constraints against that empty matrix.
def row54_forcing(sequence, restored=b'nihao'):
    """Build a constraint-retention case choosing non-first NORMAL 泥 before reparsing.

    sequence='no-empty' omits the empty parse; 'empty-guess' guesses after it;
    other values parse empty without guessing. restored is the final byte input."""
    def probe(k):
        """Choose displayed 泥, perform the requested empty-parse sequence and record the restored sentence.

        Assert candidate getters and that 泥 is a non-first NORMAL row. Use tone
        options and return initial, choice and restored parse/guess observations."""
        k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn('parse_more_full_pinyins', Z, P, S)
        guess = k.fn('guess_sentence', B, P)
        def sentence():
            """Return the current first sentence and flag, preserving untouched output and freeing returned text."""
            text = P(UNTOUCHED)
            if k.mode == 'pinyin':
                ret = k.fn('get_sentence', B, P, U, C.POINTER(P))(inst, 0, C.byref(text))
            else:
                ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(text))
            return [ret, 'untouched' if text.value == UNTOUCHED else k.text(text.value)]
        out = {'parse': parse(inst, b'nihao'), 'initial_guess': guess(inst),
               'initial_sentence': sentence()}
        if k.mode == 'pinyin':
            out['candidates'] = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
        else:
            out['candidates'] = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
        count = U()
        assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
        chosen = None
        for index in range(count.value):
            cand, text, kind = P(), S(), I()
            assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
            assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
            assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
            if kind.value == 2 and text.value == '泥'.encode():
                assert index > 0, 'the chosen NORMAL candidate must not be first'
                out['chosen_index'], out['chosen_text'] = index, text.value.decode()
                chosen = cand
                break
        assert chosen, 'missing non-first NORMAL candidate 泥'
        out['choose'] = k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
        if sequence != 'no-empty':
            out['empty_parse'] = parse(inst, b'')
            if sequence == 'empty-guess':
                out['empty_guess'] = guess(inst)
        out['restored_parse'] = parse(inst, restored)
        out['restored_guess'] = guess(inst)
        out['restored_sentence'] = sentence()
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _sequence in ('empty', 'empty-guess', 'no-empty'):
        case(f'row54-forcing-{_mode}-{_sequence}', mode=_mode,
             control=_sequence != 'empty', issues=('709', '695', 'row:54'))(row54_forcing(_sequence))


def register(driver_case):
    """Register the declarations in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
