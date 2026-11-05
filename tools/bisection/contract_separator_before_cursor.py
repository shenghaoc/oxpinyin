"""Separator and before-cursor cases (inventory row 67, #1175).
"""
import ctypes as C

from contract_abi import B, I, P, S, U, UNTOUCHED, Z, owned_text


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #640 leading separators, #698 retained before-cursor windows, #681 zero keys.
# Calls reuse the existing isolated worker and distinguish NULL from untouched.
def borrowed_text(pointer):
    """Decode a borrowed C string without freeing it, preserving untouched and NULL outputs.

    Return 'untouched' for the sentinel and None for NULL; invalid UTF-8 raises."""
    if pointer.value==UNTOUCHED:
        return 'untouched'
    return C.string_at(pointer.value).decode() if pointer.value else None


def separator_case(text, op, off, options=0x18a):
    """Build a parse-and-query case for a full-pinyin string and byte offset.

    options is passed to set_options. op selects parse, sentence, candidates,
    before, left, right, offset, character, rest or render; other values query
    a key. The returned callable records output sentinels and borrowed data."""

    def probe(k):
        """Parse with the selected options and collect the requested operation's outputs.

        Guess a sentence unless op is parse. Preserve numeric and pointer sentinels,
        record candidate type/text/rank where available, and copy key/rest bytes.
        Render operations decode/free owned strings; candidate strings are borrowed.
        Character queries choose 阿, 你好 or 你 from the input. No native objects
        are finalized here."""
        inst = k.inst
        k.fn('set_options', B, P, U)(k.ctx, options)
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        out = {'parse_return_consumed': parsed, 'get_parsed_input_length': k.fn('get_parsed_input_length', Z, P)(inst)}
        if op == 'parse':
            return out
        guessed = k.fn('guess_sentence', B, P)(inst)
        out['guess_sentence'] = guessed
        if op == 'sentence':
            ptr = P(UNTOUCHED)
            if k.mode == 'pinyin':
                ret = k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(inst, 0, C.byref(ptr))
            else:
                ret = k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(ptr))
            out.update(ret=ret, sentence='untouched' if ptr.value == UNTOUCHED else k.text(ptr.value))
            return out
        if op in ['candidates', 'before']:
            if k.mode == 'pinyin':
                ret = k.fn('guess_candidates', B, P, Z, U)(inst, off, 0x1e)
            else:
                ret = k.fn('guess_candidates_before_cursor' if op == 'before' else 'guess_candidates_after_cursor', B, P, Z)(inst, off)
            n = U(UNTOUCHED)
            got = k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
            out.update(ret=ret, get_n=got, n=n.value)
            rows = []
            if got:
                for index in range(n.value):
                    candidate, kind, string = (P(UNTOUCHED), I(UNTOUCHED), P(UNTOUCHED))
                    fetched = k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(candidate))
                    typed = k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, candidate, C.byref(kind))
                    rendered = k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, candidate, C.byref(string))
                    row = [fetched, typed, kind.value, rendered, borrowed_text(string)]
                    if k.mode == 'pinyin' and kind.value == 1:
                        nbest = C.c_ubyte(0xab)
                        row.extend([k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(inst, candidate, C.byref(nbest)), nbest.value])
                    rows.append(row)
            out['rows'] = rows
            return out
        outval = Z(UNTOUCHED)
        if op in ['left', 'right', 'offset']:
            name = 'get_' + ('' + k.mode + '_offset' if op == 'offset' else op + '_' + k.mode + '_offset')
            ret = k.fn(name, B, P, Z, C.POINTER(Z))(inst, off, C.byref(outval))
            out.update(ret=ret, out=outval.value, untouched_output=outval.value == UNTOUCHED if not ret else True)
            return out
        if op == 'character':
            phrase = '阿' if text == "'a" else '你好' if 'hao' in text else '你'
            ret = k.fn('get_character_offset', B, P, S, Z, C.POINTER(Z))(inst, phrase.encode(), off, C.byref(outval))
            out.update(ret=ret, out=outval.value, phrase=phrase, untouched_output=outval.value == UNTOUCHED if not ret else True)
            return out
        ptr = P(UNTOUCHED)
        suffix = 'key_rest' if op == 'rest' else 'key'
        ret = k.fn('get_' + k.mode + '_' + suffix, B, P, Z, C.POINTER(P))(inst, off, C.byref(ptr))
        out.update(ret=ret, pointer='untouched' if ptr.value == UNTOUCHED else 'null' if not ptr.value else 'key')
        if not ret:
            out['untouched_or_null_pointer'] = ptr.value in (None, UNTOUCHED)
        if ret and ptr.value:
            out['bytes'] = C.string_at(ptr.value, 4 if op == 'rest' else 2).hex()
            if op == 'rest':
                begin, end, length = (C.c_ushort(0xbeef), C.c_ushort(0xbeef), C.c_ushort(0xbeef))
                out['positions_ret'] = k.fn('get_' + k.mode + '_key_rest_positions', B, P, P, C.POINTER(C.c_ushort), C.POINTER(C.c_ushort))(inst, ptr, C.byref(begin), C.byref(end))
                out['positions'] = [begin.value, end.value]
                out['length_ret'] = k.fn('get_' + k.mode + '_key_rest_length', B, P, P, C.POINTER(C.c_ushort))(inst, ptr, C.byref(length))
                out['length'] = length.value
            if op == 'render':
                for name in ['pinyin', 'zhuyin', 'luoma_pinyin', 'secondary_zhuyin'] if k.mode == 'pinyin' else ['zhuyin', 'pinyin']:
                    string = P(UNTOUCHED)
                    rendered = k.fn('get_' + name + '_string', B, P, P, C.POINTER(P))(inst, ptr, C.byref(string))
                    out[name + '_string'] = [rendered, owned_text(k, string)]
                if k.mode == 'pinyin':
                    initial, final = (P(UNTOUCHED), P(UNTOUCHED))
                    rendered = k.fn('get_pinyin_strings', B, P, P, C.POINTER(P), C.POINTER(P))(inst, ptr, C.byref(initial), C.byref(final))
                    out['pinyin_strings'] = [rendered, owned_text(k, initial), owned_text(k, final)]
        return out
    return probe

SEPARATOR_INPUTS=("'ni","''ni","'nihao","'ni'hao","'ni'","'a")
# The kept offsets of each (mode, input index, operation); an operation absent here has no case.
SEPARATOR_OFFSETS = {
    'pinyin/0/character': (0, 1, 2, 3),
    'pinyin/0/right': (3,),
    'pinyin/1/candidates': (0, 1, 2, 3, 4),
    'pinyin/1/key': (0, 1, 2, 3, 4),
    'pinyin/1/left': (0, 1, 2, 3, 4),
    'pinyin/1/right': (0, 1, 3),
    'pinyin/3/candidates': (1, 3, 6, 7),
    'pinyin/3/key': (0, 3, 6),
    'pinyin/3/left': (1, 4, 7),
    'pinyin/3/offset': (0,),
    'pinyin/3/parse': (0,),
    'pinyin/3/rest': (1,),
    'pinyin/3/right': (0, 4),
    'pinyin/3/sentence': (0,),
    'pinyin/4/candidates': (0, 1, 2, 3, 4),
    'pinyin/4/key': (0, 1),
    'pinyin/4/left': (0, 1, 2, 3),
    'pinyin/4/parse': (0,),
    'pinyin/4/right': (0, 2, 3),
    'pinyin/4/sentence': (0,),
    'pinyin/5/candidates': (0, 1, 2),
    'pinyin/5/key': (0, 1, 2),
    'pinyin/5/left': (0, 1, 2),
    'pinyin/5/parse': (0,),
    'pinyin/5/right': (0,),
    'pinyin/5/sentence': (0,),
    'zhuyin/0/character': (0, 1, 2, 3),
    'zhuyin/0/right': (3,),
    'zhuyin/1/before': (2, 4),
    'zhuyin/1/parse': (0,),
    'zhuyin/1/sentence': (0,),
    'zhuyin/3/before': (1, 4, 7),
    'zhuyin/3/candidates': (0, 4),
    'zhuyin/3/key': (3,),
    'zhuyin/3/left': (4, 6, 7),
    'zhuyin/3/rest': (3, 4),
    'zhuyin/3/right': (4, 6),
    'zhuyin/4/before': (1, 3),
    'zhuyin/4/offset': (4,),
    'zhuyin/4/rest': (4,),
    'zhuyin/4/right': (3,),
    'zhuyin/5/before': (1, 2),
}
SEPARATOR_ABORTS = {
    'zhuyin/3/before': (4,),
    'pinyin/0/right': (3,),
    'pinyin/0/character': (0, 1, 2, 3),
    'pinyin/3/left': (4,),
    'pinyin/3/right': (4,),
    'pinyin/4/right': (3,),
    'zhuyin/0/right': (3,),
    'zhuyin/0/character': (0, 1, 2, 3),
    'zhuyin/3/candidates': (4,),
    'zhuyin/3/left': (4, 7),
    'zhuyin/3/right': (4,),
    'zhuyin/3/rest': (4,),
    'zhuyin/4/right': (3,),
}
SEPARATOR_CONTROLS = {
    'pinyin/0/right': (3,),
    'pinyin/0/character': (0, 1, 2, 3),
    'pinyin/1/left': (0, 1, 2, 3, 4),
    'pinyin/1/right': (0, 1, 3),
    'pinyin/1/key': (2, 3, 4),
    'pinyin/3/parse': (0,),
    'pinyin/3/offset': (0,),
    'pinyin/3/left': (1, 4, 7),
    'pinyin/3/right': (0, 4),
    'pinyin/3/key': (3, 6),
    'pinyin/3/rest': (1,),
    'pinyin/4/parse': (0,),
    'pinyin/4/left': (0, 1, 2, 3),
    'pinyin/4/right': (0, 2, 3),
    'pinyin/4/key': (1,),
    'pinyin/5/parse': (0,),
    'pinyin/5/left': (0, 1, 2),
    'pinyin/5/right': (0,),
    'pinyin/5/key': (1, 2),
    'zhuyin/0/right': (3,),
    'zhuyin/0/character': (0, 1, 2),
    'zhuyin/1/parse': (0,),
    'zhuyin/3/left': (4, 6),
    'zhuyin/3/right': (4, 6),
    'zhuyin/4/offset': (4,),
    'zhuyin/4/right': (3,),
    'zhuyin/4/rest': (4,),
}
for mode in ('pinyin','zhuyin'):
    for index,text in enumerate(SEPARATOR_INPUTS):
        operations=['parse','sentence','candidates','offset','left','right','key','rest','character']
        if mode=='zhuyin':
            operations.append('before')
        for op in operations:
            for offset in SEPARATOR_OFFSETS.get(f'{mode}/{index}/{op}', ()):
                identity=f'{mode}/{index}/{op}/{offset}'
                kwargs={'mode':mode,'control':offset in SEPARATOR_CONTROLS.get(f'{mode}/{index}/{op}', ())}
                if offset in SEPARATOR_ABORTS.get(f"{mode}/{index}/{op}", ()):
                    # Both facades reproduce these aborts (2026-10-31 UTC,
                    # pin-aborts-pinyin-facade.md, pin-aborts-zhuyin-facade.md).
                    kwargs.update(abort=False, sigabrt=True, control=False)
                case('separator/'+identity,**kwargs, issues=('640',))(separator_case(text,op,offset))
BEFORE_CURSOR_INPUTS=('guo', 'tian', 'nihaoshijie', 'beijingdaxue', 'nih', 'beijin', 'nihaoshij', 'ni3', 'ni3hao3')
# The kept offsets of each input's zhuyin before-cursor cases, of its separator-free controls by operation, and the inputs whose sentence control is kept.
BEFORE_CURSOR_OFFSETS={'guo': (2,), 'beijingdaxue': (0, 11, 12), 'nih': (0, 2, 3), 'beijin': (1,), 'nihaoshij': (1, 8), 'ni3': (0, 1, 3), 'ni3hao3': (3, 6)}
SEPARATOR_FREE_OFFSETS={
    'tian': {'key': (2,)},
    'nihaoshijie': {'key': (10,)},
    'beijingdaxue': {'candidates': (0, 12), 'key': (0, 1, 11, 12)},
    'nih': {'candidates': (0, 1), 'key': (0, 1, 2, 3)},
    'beijin': {'key': (5,)},
    'nihaoshij': {'candidates': (1,), 'key': (8,)},
    'ni3': {'candidates': (0, 1, 2), 'key': (0, 1, 2, 3)},
    'ni3hao3': {'candidates': (7,), 'key': (3,)},
}
SEPARATOR_FREE_SENTENCES=('beijingdaxue', 'nih', 'ni3',)
BEFORE_CURSOR_CONTROLS=(('beijingdaxue', 0), ('beijingdaxue', 12), ('nih', 0), ('nih', 2), ('nih', 3), ('nihaoshij', 8), ('ni3', 0), ('ni3', 3), ('ni3hao3', 3),)
for text in BEFORE_CURSOR_INPUTS:
    options=0x1aa if any(char.isdigit() for char in text) else 0x18a
    for offset in range(len(text)+1):
        if offset in BEFORE_CURSOR_OFFSETS.get(text, ()):
            case(f'before-cursor/{text}/{offset}',mode='zhuyin',
                 control=(text,offset) in BEFORE_CURSOR_CONTROLS, issues=('698',))(separator_case(text,'before',offset,options))
        for op in ('candidates','key','rest'):
            if offset in SEPARATOR_FREE_OFFSETS.get(text, {}).get(op, ()):
                case(f'separator-free-control/{text}/{op}/{offset}',control=True, issues=('640', '698'))(
                    separator_case(text,op,offset,options))
    if text in SEPARATOR_FREE_SENTENCES:
        case(f'separator-free-control/{text}/sentence',control=True, issues=('640', '698'))(
            separator_case(text,'sentence',0,options))
# The kept offsets of each (mode, input, operation) of the zero-key cases.
ZERO_KEY_OFFSETS={
    ('pinyin', "ni''hao", 'key'): (0, 1, 3, 4, 6, 7),
    ('pinyin', "ni'hao'", 'key'): (0, 1, 6, 7),
    # zhuyin_get_zhuyin_key and its rest getter read the column at the offset itself, where
    # libpinyin first moves past zero keys (`_compute_pinyin_start`, pinyin.cpp:2931).
    ('zhuyin', "ni''hao", 'key'): (2,),
    ('zhuyin', "ni''hao", 'rest'): (2,),
}
for mode in ('pinyin','zhuyin'):
    for text in ("ni'","ni'hao","ni''hao","ni'hao'"):
        for op in ('key','rest'):
            for offset in range(len(text)+1):
                if offset not in ZERO_KEY_OFFSETS.get((mode, text, op), ()):
                    continue
                previous_zero=offset>0 and text[offset-1]=="'"
                is_zero=offset<len(text) and text[offset]=="'"
                tail_zero=is_zero and all(char=="'" for char in text[offset:])
                kwargs={'mode':mode,'control':not (tail_zero if mode=='pinyin' else is_zero) and not (mode=='zhuyin' and previous_zero and offset<len(text))}
                if mode=='zhuyin' and previous_zero and offset<len(text):
                    kwargs.update(abort=False, sigabrt=True)
                case(f'zero-key/{mode}/{text}/{op}/{offset}',**kwargs, issues=('681',))(separator_case(text,op,offset))
# (mode, input, operation, offset) of the kept allocator-zero cases.
ALLOCATOR_ZERO=(('pinyin',"xi''",'render',3),('zhuyin',"xi1'an1",'render',3),('zhuyin',"xi1'",'key',3))
for mode,text,op,offset in ALLOCATOR_ZERO:
    case(f'allocator-zero/{mode}/{text}/{op}/{offset}',mode=mode, issues=('681',))(
        separator_case(text,op,offset,0x1aa))
case("allocator-zero/zhuyin/xi1'an1/key/4",mode='zhuyin',abort=False,sigabrt=True, issues=('681',))(
    separator_case("xi1'an1",'key',4,0x1aa))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
