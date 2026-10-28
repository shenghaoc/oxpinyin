"""Allocator register key-string cases."""
import ctypes as C

from contract_abi import B, P, S, U, UNTOUCHED, Z, owned_text


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #681: the eight "unexecuted" string-getter lines of the allocator registers
# (E3-E10). Each getter's false branch is `0 == key->get_table_index()`
# (pinyin.cpp:2707-2762, zhuyin.cpp:1732-1760); a conforming consumer reaches
# it with the zero key the key getter answers at a separator column. The
# case records the return and the state of every out-parameter (`untouched`
# is the 0xABCDEF sentinel, `null` a NULL write); the control calls the same
# getter on a real key.
def alloc_register_case(mode, text, offset, which):
    """Build a key-to-string getter case at the selected full-pinyin byte offset.

    mode names the key accessor; which='strings' requests separate pinyin
    initial/final strings, otherwise it names one rendering accessor."""
    def probe(k):
        """Parse and guess, then record raw key bytes and owned string outputs.

        Return early when the key getter fails or writes NULL. Use option word
        0x1aa and preserve untouched output sentinels; free returned strings."""
        inst = k.inst
        k.fn('set_options', B, P, U)(k.ctx, 0x1aa)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, text.encode())
        out = {'guess_sentence': k.fn('guess_sentence', B, P)(inst)}
        key = P(UNTOUCHED)
        out['key_ret'] = k.fn('get_' + mode + '_key', B, P, Z, C.POINTER(P))(inst, offset, C.byref(key))
        if not out['key_ret'] or not key.value:
            return out
        out['key_bytes'] = C.string_at(key.value, 2).hex()
        if which == 'strings':
            initial, final = (P(UNTOUCHED), P(UNTOUCHED))
            out['ret'] = k.fn('get_pinyin_strings', B, P, P, C.POINTER(P), C.POINTER(P))(inst, key, C.byref(initial), C.byref(final))
            out['shengmu'], out['yunmu'] = owned_text(k, initial), owned_text(k, final)
        else:
            string = P(UNTOUCHED)
            out['ret'] = k.fn('get_' + which + '_string', B, P, P, C.POINTER(P))(inst, key, C.byref(string))
            out['out'] = owned_text(k, string)
        return out
    return probe


for _mode, _text, _zero, _real, _names in (
        ('pinyin', "xi'", 2, 0, ('pinyin', 'zhuyin', 'luoma_pinyin', 'secondary_zhuyin', 'strings')),
        ('zhuyin', "xi1'", 3, 0, ('zhuyin', 'pinyin'))):
    for _name in _names:
        case(f'alloc-register-{_mode}_get_{_name}-zero-key', mode=_mode, control=True, issues=('681',))(
            alloc_register_case(_mode, _text, _zero, _name))
        case(f'alloc-register-{_mode}_get_{_name}-real-key', mode=_mode, control=True, issues=('681',))(
            alloc_register_case(_mode, _text, _real, _name))


def register(driver_case):
    """Register the declarations in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
