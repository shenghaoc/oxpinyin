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
SEPARATOR_ABORTS = {
    'zhuyin/3/before': (4,),
    'zhuyin/4/before': (4,),
    'pinyin/0/right': (3,),
    'pinyin/0/character': (0, 1, 2, 3),
    'pinyin/1/right': (4,),
    'pinyin/1/character': (0, 1, 2, 3, 4),
    'pinyin/2/right': (6,),
    'pinyin/2/character': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/3/left': (4,),
    'pinyin/3/right': (4, 7),
    'pinyin/3/character': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/4/left': (4,),
    'pinyin/4/right': (3, 4),
    'pinyin/4/character': (0, 1, 2, 3, 4),
    'pinyin/5/right': (2,),
    'pinyin/5/character': (0, 1, 2),
    'zhuyin/0/right': (3,),
    'zhuyin/0/character': (0, 1, 2, 3),
    'zhuyin/1/right': (4,),
    'zhuyin/1/character': (0, 1, 2, 3, 4),
    'zhuyin/2/right': (6,),
    'zhuyin/2/character': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/3/candidates': (4,),
    'zhuyin/3/left': (4, 7),
    'zhuyin/3/right': (4, 7),
    'zhuyin/3/key': (4,),
    'zhuyin/3/rest': (4,),
    'zhuyin/3/character': (0, 1, 2, 3, 4, 5, 6, 7),
    'zhuyin/4/candidates': (4,),
    'zhuyin/4/left': (4,),
    'zhuyin/4/right': (3, 4),
    'zhuyin/4/character': (0, 1, 2, 3, 4),
    'zhuyin/5/right': (2,),
    'zhuyin/5/character': (0, 1, 2),
}
SEPARATOR_CONTROLS = {
    'pinyin/0/parse': (0,),
    'pinyin/0/offset': (0, 1, 2, 3),
    'pinyin/0/left': (0, 1, 2, 3),
    'pinyin/0/right': (0, 1, 2, 3),
    'pinyin/0/key': (1, 2, 3),
    'pinyin/0/rest': (1, 2, 3),
    'pinyin/0/character': (0, 1, 2, 3),
    'pinyin/1/parse': (0,),
    'pinyin/1/offset': (0, 1, 2, 3, 4),
    'pinyin/1/left': (0, 1, 2, 3, 4),
    'pinyin/1/right': (0, 1, 2, 3, 4),
    'pinyin/1/key': (2, 3, 4),
    'pinyin/1/rest': (2, 3, 4),
    'pinyin/1/character': (0, 1, 2, 3, 4),
    'pinyin/2/parse': (0,),
    'pinyin/2/offset': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/2/left': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/2/right': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/2/key': (1, 2, 3, 4, 5, 6),
    'pinyin/2/rest': (1, 2, 3, 4, 5, 6),
    'pinyin/2/character': (0, 1, 2, 3, 4, 5, 6),
    'pinyin/3/parse': (0,),
    'pinyin/3/offset': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/left': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/right': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/key': (1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/rest': (1, 2, 3, 4, 5, 6, 7),
    'pinyin/3/character': (0, 1, 2, 3, 4, 5, 6, 7),
    'pinyin/4/parse': (0,),
    'pinyin/4/offset': (0, 1, 2, 3, 4),
    'pinyin/4/left': (0, 1, 2, 3, 4),
    'pinyin/4/right': (0, 1, 2, 3, 4),
    'pinyin/4/key': (1, 2, 4),
    'pinyin/4/rest': (1, 2, 4),
    'pinyin/4/character': (0, 1, 2, 3, 4),
    'pinyin/5/parse': (0,),
    'pinyin/5/offset': (0, 1, 2),
    'pinyin/5/left': (0, 1, 2),
    'pinyin/5/right': (0, 1, 2),
    'pinyin/5/key': (1, 2),
    'pinyin/5/rest': (1, 2),
    'pinyin/5/character': (0, 1, 2),
    'zhuyin/0/parse': (0,),
    'zhuyin/0/offset': (0, 1, 2, 3),
    'zhuyin/0/left': (0, 1, 2, 3),
    'zhuyin/0/right': (0, 1, 2, 3),
    'zhuyin/0/key': (1, 2, 3),
    'zhuyin/0/rest': (1, 2, 3),
    'zhuyin/0/character': (0, 1, 2),
    'zhuyin/1/parse': (0,),
    'zhuyin/1/offset': (0, 1, 2, 3, 4),
    'zhuyin/1/left': (0, 1, 2, 3, 4),
    'zhuyin/1/right': (0, 1, 2, 3, 4),
    'zhuyin/1/key': (2, 3, 4),
    'zhuyin/1/rest': (2, 3, 4),
    'zhuyin/1/character': (0, 1, 2, 3),
    'zhuyin/2/parse': (0,),
    'zhuyin/2/offset': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/2/left': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/2/right': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/2/key': (1, 2, 3, 4, 5, 6),
    'zhuyin/2/rest': (1, 2, 3, 4, 5, 6),
    'zhuyin/2/character': (0, 1, 2, 3, 4, 5),
    'zhuyin/3/parse': (0,),
    'zhuyin/3/offset': (0, 1, 2, 3, 4, 5, 6, 7),
    'zhuyin/3/left': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/3/right': (0, 1, 2, 3, 4, 5, 6, 7),
    'zhuyin/3/key': (1, 2, 5, 6, 7),
    'zhuyin/3/rest': (1, 2, 5, 6, 7),
    'zhuyin/3/character': (0, 1, 2, 3, 4, 5, 6),
    'zhuyin/4/parse': (0,),
    'zhuyin/4/offset': (0, 1, 2, 3, 4),
    'zhuyin/4/left': (0, 1, 2, 3, 4),
    'zhuyin/4/right': (0, 1, 2, 3, 4),
    'zhuyin/4/key': (1, 2, 4),
    'zhuyin/4/rest': (1, 2, 4),
    'zhuyin/4/character': (0, 1, 2, 3),
    'zhuyin/5/parse': (0,),
    'zhuyin/5/offset': (0, 1, 2),
    'zhuyin/5/left': (0, 1, 2),
    'zhuyin/5/right': (0, 1, 2),
    'zhuyin/5/key': (1, 2),
    'zhuyin/5/rest': (1, 2),
    'zhuyin/5/character': (0, 1),
}
for mode in ('pinyin','zhuyin'):
    for index,text in enumerate(SEPARATOR_INPUTS):
        operations=['parse','sentence','candidates','offset','left','right','key','rest','character']
        if mode=='zhuyin':
            operations.append('before')
        for op in operations:
            for offset in ([0] if op in ('parse','sentence') else range(len(text)+1)):
                identity=f'{mode}/{index}/{op}/{offset}'
                kwargs={'mode':mode,'control':offset in SEPARATOR_CONTROLS.get(f'{mode}/{index}/{op}', ())}
                if offset in SEPARATOR_ABORTS.get(f"{mode}/{index}/{op}", ()):
                    kwargs['abort']=False
                case('separator/'+identity,**kwargs, issues=('640',))(separator_case(text,op,offset))
BEFORE_CURSOR_INPUTS=('ni','hao','wo','shi','zhong','guo','xian','tian',
    'nihao','zhongguo','beijing','shanghai','renmin','nihaoshijie',
    'zhonghuarenmin','beijingdaxue','shijieheping','nih','zhongg',
    'beijin','nihaoshij','ni3','ni3hao3','zhong1guo2')
BEFORE_CURSOR_CONTROLS=(('ni', 0), ('ni', 2), ('hao', 0), ('hao', 3), ('wo', 0), ('wo', 2), ('shi', 0), ('shi', 3), ('zhong', 0), ('zhong', 5), ('guo', 0), ('guo', 3), ('xian', 0), ('tian', 0), ('nihao', 0), ('nihao', 2), ('nihao', 5), ('zhongguo', 0), ('zhongguo', 5), ('zhongguo', 8), ('beijing', 0), ('beijing', 3), ('beijing', 7), ('shanghai', 0), ('shanghai', 5), ('shanghai', 8), ('renmin', 0), ('renmin', 3), ('renmin', 6), ('nihaoshijie', 0), ('nihaoshijie', 2), ('nihaoshijie', 5), ('nihaoshijie', 8), ('zhonghuarenmin', 0), ('zhonghuarenmin', 5), ('zhonghuarenmin', 8), ('zhonghuarenmin', 11), ('zhonghuarenmin', 14), ('beijingdaxue', 0), ('beijingdaxue', 3), ('beijingdaxue', 7), ('beijingdaxue', 9), ('beijingdaxue', 12), ('shijieheping', 0), ('shijieheping', 3), ('shijieheping', 8), ('shijieheping', 12), ('nih', 0), ('nih', 2), ('nih', 3), ('zhongg', 0), ('zhongg', 5), ('zhongg', 6), ('beijin', 0), ('beijin', 3), ('beijin', 6), ('nihaoshij', 0), ('nihaoshij', 2), ('nihaoshij', 5), ('nihaoshij', 8), ('nihaoshij', 9), ('ni3', 0), ('ni3', 3), ('ni3hao3', 0), ('ni3hao3', 3), ('ni3hao3', 7), ('zhong1guo2', 0), ('zhong1guo2', 6), ('zhong1guo2', 10))
for text in BEFORE_CURSOR_INPUTS:
    options=0x1aa if any(char.isdigit() for char in text) else 0x18a
    for offset in range(len(text)+1):
        case(f'before-cursor/{text}/{offset}',mode='zhuyin',
             control=(text,offset) in BEFORE_CURSOR_CONTROLS, issues=('698',))(separator_case(text,'before',offset,options))
        for op in ('candidates','key','rest'):
            case(f'separator-free-control/{text}/{op}/{offset}',control=True, issues=('640', '698'))(
                separator_case(text,op,offset,options))
    case(f'separator-free-control/{text}/sentence',control=True, issues=('640', '698'))(
        separator_case(text,'sentence',0,options))
for mode in ('pinyin','zhuyin'):
    for text in ("ni'","ni'hao","ni''hao","ni'hao'"):
        for op in ('key','rest'):
            for offset in range(len(text)+1):
                previous_zero=offset>0 and text[offset-1]=="'"
                is_zero=offset<len(text) and text[offset]=="'"
                tail_zero=is_zero and all(char=="'" for char in text[offset:])
                kwargs={'mode':mode,'control':not (tail_zero if mode=='pinyin' else is_zero) and not (mode=='zhuyin' and previous_zero and offset<len(text))}
                if mode=='zhuyin' and previous_zero and offset<len(text):
                    kwargs['abort']=False
                case(f'zero-key/{mode}/{text}/{op}/{offset}',**kwargs, issues=('681',))(separator_case(text,op,offset))
for mode,text,offset in (('pinyin',"xi'",2),('pinyin',"xi''",2),
                         ('pinyin',"xi''",3),('zhuyin',"xi1'an1",3),
                         ('zhuyin',"xi1'",3)):
    for op in ('key','rest','render'):
        case(f'allocator-zero/{mode}/{text}/{op}/{offset}',mode=mode, issues=('681',))(
            separator_case(text,op,offset,0x1aa))
case("allocator-zero/zhuyin/xi1'an1/key/4",mode='zhuyin',abort=False, issues=('681',))(
    separator_case("xi1'an1",'key',4,0x1aa))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
