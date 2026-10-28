"""Stored pronunciation-count header cases."""
import ctypes as C
from pathlib import Path

from contract_abi import B, P, U, UNTOUCHED
from contract_fixtures import _write_user_conf, conf_system


def register(case, *, _chunk):
    """Register the cases at their original position in the driver."""
    # #882: get_phrase_item checks the offset and two header bytes only
    # (074a2219 phrase_index.cpp:180-205). Neither getter decodes text or keys
    # (pinyin.cpp:2786-2798; zhuyin.cpp:1778-1790).
    def stored_count_headers(user):
        """Build a pronunciation-count probe using only planted item headers.

        Choose a private system library or user.bin, with invalid offsets, alias
        slots and malformed payloads. SubPhraseIndex::get_phrase_item reads only the
        offset and two header bytes before constructing a view at
        074a2219 phrase_index.cpp:180-205; the count getter reads that header at
        zhuyin.cpp:1778-1790.
        """
        def probe(k):
            """Write the header fixture and record count outputs for slots, tags and a missing library.

            Require a fresh context, initialize counts to UNTOUCHED, and close the instance
            and context after collecting the results.
            """
            item = bytes([1, 3]) + (9).to_bytes(4, 'little') + (0xd800).to_bytes(4, 'little') + b'\xff' * 18
            content = b'\0' * 8 + item + b'\x01'
            offsets = [0, 8, len(content) - 1, 0, len(content), 8]
            index_two = 17 + len(offsets) * 4 + 1
            payload = b''.join(value.to_bytes(4, 'little') for value in
                               (9, 17, index_two, index_two + len(content) + 1))
            payload += b'#' + b''.join(value.to_bytes(4, 'little') for value in offsets) + b'#' + content + b'#'
            if user:
                _write_user_conf(k)
                Path(k.user, 'user.bin').write_bytes(_chunk(payload))
                ctx = k.init()
                library = 7
            else:
                system = conf_system(k, ('gb_char.bin', 'count.bin'))
                Path(system, 'count.bin').write_bytes(_chunk(payload))
                ctx = k.init(system=system)
                library = 1
            assert ctx, 'count fixture init'
            inst = k.fn('alloc_instance', P, P)(ctx)
            count = k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))
            out = {}
            for slot in (0, 1, 2, 3, 4, 5, 6, 0xffffff):
                number = U(UNTOUCHED)
                ret = count(inst, (library << 24) | slot, C.byref(number))
                out[str(slot)] = [ret, number.value]
            for tag in (0x10000000, 0xf0000000):
                number = U(UNTOUCHED)
                token = tag | (library << 24) | 1
                out['tag ' + hex(tag)] = [count(inst, token, C.byref(number)), number.value]
            number = U(UNTOUCHED)
            out['no sub-index'] = [count(inst, 0x0f000001, C.byref(number)), number.value]
            k.fn('free_instance', None, P)(inst)
            k.fn('fini', None, P)(ctx)
            return out
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        for _user in (False, True):
            case('stored-count-headers-' + ('user-' if _user else 'system-') + _mode,
                 mode=_mode, issues=('882',))(stored_count_headers(_user))
