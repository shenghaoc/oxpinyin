"""Remember-tone and export cases."""
import ctypes as C

from contract_abi import B, I, P, S, U, Z


def register_tones(case, *, remember_export):
    """Register the first two cases with the shared export reader."""
    def remember_tones(k):
        """Check remember/export API presence and, for pinyin, remember a toned imported phrase.

        Import 你好你好, parse its toned spelling, guess, remember with count three
        and export its rows. Free the cached instance and finalize the context on
        the pinyin path without clearing the kit's pointers."""
        out={'remember_api':hasattr(k.lib,k.mode+'_remember_user_input'),
             'export_api':hasattr(k.lib,k.mode+'_begin_get_phrases')}
        if k.mode=='zhuyin':return out
        it=k.fn('begin_add_phrases',P,P,U)(k.ctx,7)
        out['add']=k.fn('iterator_add_phrase',B,P,S,S,I)(it,'你好你好'.encode(),b"ni3'hao3'ni3'hao3",100000)
        k.fn('end_add_phrases',None,P)(it)
        out['parse']=k.fn('parse_more_full_pinyins',Z,P,S)(k.inst,b'ni3hao3ni3hao3')
        out['guess']=k.fn('guess_sentence',B,P)(k.inst)
        out['remember']=k.fn('remember_user_input',B,P,S,I)(k.inst,'你好你好'.encode(),3)
        out['export']=remember_export(k,7,'你好你好')
        k.fn('free_instance',None,P)(k.inst)
        k.fn('fini',None,P)(k.ctx)
        return out


    case('remember-tones-pinyin')(remember_tones)
    case('remember-api-absent-zhuyin', mode='zhuyin', control=True)(remember_tones)


def register_review(case, *, remember_export):
    """Register the remaining cases at their later driver position."""
    def remember_review(k, double=False):
        """Observe remembered readings after clearing tones or using the double-pinyin parser.

        Use count three, export the target phrase, then free/finalize cached native
        objects without clearing their pointers. The full-pinyin path clears options
        after guessing; the double-pinyin path retains the tone option."""
        k.fn('set_options', B, P, U)(k.ctx, 0x20)
        parse = 'parse_more_double_pinyins' if double else 'parse_more_full_pinyins'
        phrase = '你' if double else '你好'
        out = {'parse': k.fn(parse, Z, P, S)(k.inst, b'ni3' if double else b'ni3hao3')}
        out['guess'] = k.fn('guess_sentence', B, P)(k.inst)
        if not double:
            out['clear-tone'] = k.fn('set_options', B, P, U)(k.ctx, 0)
        out['remember'] = k.fn('remember_user_input', B, P, S, I)(k.inst, phrase.encode(), 3)
        out['export'] = remember_export(k, 7, phrase)
        k.fn('free_instance', None, P)(k.inst)
        k.fn('fini', None, P)(k.ctx)
        return out

    case('remember-cleared-tone-pinyin')(remember_review)
    case('remember-double-tone-pinyin')(lambda k: remember_review(k, True))
