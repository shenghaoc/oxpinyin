"""Changed parse constraints cases (inventory row 49, #1175).
"""
import ctypes as C

from contract_abi import B, I, P, S, U, UNTOUCHED, Z


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #710: parsing never touches m_constraints. At 074a2219,
# lookup/phonetic_lookup.cpp:120-160 first resizes to matrix.size(), then
# clears only ONESTEP spans ending >= size or whose pronunciation
# possibility is < FLT_EPSILON. A guess (or a choose) validates; a parse
# alone, including an empty or temporarily incompatible parse, does not.
# Compare complete sentence text, not just guess success or a prefix.
def changed_parse_constraints(initial, choices, replacement, sequence,
                              intermediate=None):
    """Build a sentence comparison across selected constraints and replacement parses.

    choices contains (byte offset, NORMAL-candidate text) pairs. sequence may
    guess after choosing, parse empty with or without guessing, or guess an
    optional intermediate input. Always guess the replacement, record its
    sentence, then clear each chosen offset."""
    def probe(k):
        """Choose the requested rows, perform the parse sequence and record full sentence outputs.

        Assert tone options and candidate lookup, encode input strings as UTF-8,
        and record native parse/guess/clear flags without asserting success."""
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        inst = k.inst
        parse = k.fn('parse_more_full_pinyins', Z, P, S)
        guess = k.fn('guess_sentence', B, P)

        def sentence():
            """Read and free the current first sentence, distinguishing untouched output from NULL."""
            text = P(UNTOUCHED)
            if k.mode == 'pinyin':
                ret = k.fn('get_sentence', B, P, U, C.POINTER(P))(inst, 0, C.byref(text))
            else:
                ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(text))
            return [ret, 'untouched' if text.value == UNTOUCHED else k.text(text.value)]

        def choose(offset, wanted):
            """Choose the first NORMAL row matching wanted at the given byte offset.

            Assert candidate generation and getters; return [wanted, signed selection
            result] or raise AssertionError when no matching row is displayed."""
            if k.mode == 'pinyin':
                assert k.fn('guess_candidates', B, P, Z, U)(inst, offset, 0x1e)
            else:
                assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, offset)
            count = U()
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            for index in range(count.value):
                cand, text, kind = P(), S(), I()
                assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
                assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
                assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
                if kind.value == 2 and text.value.decode() == wanted:
                    return [wanted, k.fn('choose_candidate', I, P, Z, P)(inst, offset, cand)]
            raise AssertionError('missing NORMAL candidate: ' + wanted)

        out = {'parse': parse(inst, initial.encode()), 'initial_guess': guess(inst),
               'initial_sentence': sentence()}
        out['choices'] = [choose(offset, text) for offset, text in choices]
        if sequence == 'guess':
            out['chosen_guess'] = guess(inst)
            out['chosen_sentence'] = sentence()
        if sequence in ('empty', 'empty-guess'):
            out['empty_parse'] = parse(inst, b'')
            if sequence == 'empty-guess':
                out['empty_guess'] = guess(inst)
        if intermediate is not None:
            out['intermediate_parse'] = parse(inst, intermediate.encode())
            if sequence == 'intermediate-guess':
                out['intermediate_guess'] = guess(inst)
                out['intermediate_sentence'] = sentence()
        out['replacement_parse'] = parse(inst, replacement.encode())
        out['replacement_guess'] = guess(inst)
        out['replacement_sentence'] = sentence()
        out['cleared_constraints'] = [k.fn('clear_constraint', B, P, Z)(inst, offset)
                                      for offset, _ in choices]
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _name, _initial, _choices, _replacement, _control in (
            ('changed-suffix', 'nihao', ((0, '泥'),), 'nihai', False),
            ('shorter', 'nihao', ((0, '泥'),), 'ni', True),
            ('longer', 'nihao', ((0, '泥'),), 'nihaoma', True),
            ('different-first', 'nihao', ((0, '泥'),), 'nahao', True),
            ('span-past-end', 'nihao', ((2, '浩'),), 'ni', True),
            ('whole-span-past-end', 'nihaoshijie', ((0, '你好'),), 'ni', True),
            ('edit-within-span', 'nihaoshijie', ((0, '你好'),), 'nihaishijie', True),
            ('whole-span-kept', 'nihaoshijie', ((0, '你好'),), 'nihaoshijian', False),
            ('two-choices', 'nihao', ((0, '泥'), (2, '浩')), 'nihai', False)):
        for _sequence in ('direct', 'guess', 'empty', 'empty-guess'):
            case(f'parse-constraints-{_mode}-{_name}-{_sequence}', mode=_mode,
                 control=_control or _sequence == 'empty-guess', issues=('710',))(
                changed_parse_constraints(_initial, _choices, _replacement, _sequence))
    # A temporary mismatch or truncation must not destroy a choice until
    # something validates that intermediate matrix.
    for _name, _intermediate in (('temporary-mismatch', 'nahao'),
                                ('temporary-shorter', 'n')):
        for _sequence in ('direct', 'intermediate-guess'):
            case(f'parse-constraints-{_mode}-{_name}-{_sequence}', mode=_mode,
                 control=_sequence == 'intermediate-guess' and not (
                     _mode == 'zhuyin' and _name == 'temporary-shorter'), issues=('710',))(
                changed_parse_constraints('nihao', ((0, '泥'),), 'nihai',
                                          _sequence, _intermediate))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
