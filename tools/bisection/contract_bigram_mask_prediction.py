"""The #1114 handoff cases (inventory row 34, #1175).

The two #1114 cases are also Rust tests; they stay registered for
`verify --reverse-cross-open`, which lets the pin read the profile oxpinyin wrote.
The family's other cases are Rust tests (`bigram_mask_prediction` in the capi crates).
"""
import ctypes as C
import hashlib
import os
from pathlib import Path

from contract_abi import B, I, P, S, U
from contract_verdict import tkrzw_saved_bigram_bytes


def register(case, *, _craft_hash, crafted_bigram_context, index_file_rows):
    """Register these cases at their original driver position."""
    def issue1114_collision_saves(k):
        """Pin-derived load/save witness with three keys in TinyDBM bucket 74121.

        Save and reopen the three-row profile twice, checking rows and saved bytes.
        CONTRACT_1114_PROFILE replaces k.user when set. With CONTRACT_1114_RELOAD=1,
        open it as-is and compare both saves against its original bytes; otherwise
        create the fixed rows. Import one phrase before each save and assert unchanged
        rows and, for reloads, bytes after the timestamp mask. Return each file's
        byte length, masked SHA-256 and decoded rows. Finalize/reopen after each
        save, then finalize the last context."""
        rows = []
        for token, degree in ((0x01000d46, 1), (0x01004404, 6), (0x01004dd8, 100)):
            value = (69 * degree).to_bytes(4, 'little') + b''.join(
                item.to_bytes(4, 'little') + (69).to_bytes(4, 'little')
                for item in range(0x01000001, 0x01000001 + degree))
            rows.append((token.to_bytes(4, 'little'), value))
        # The runner supplies one donor directory (the pin-written profile in a reference). The receiver
        # must open that exact profile, without crafting a replacement file.
        handoff = os.environ.get('CONTRACT_1114_PROFILE')
        if handoff:
            k.user = handoff
        reloading = os.environ.get('CONTRACT_1114_RELOAD') == '1'
        ctx = k.init() if reloading else crafted_bigram_context(k, rows)
        assert ctx
        path = Path(k.user, 'user_bigram.db')
        original = path.read_bytes() if reloading else None
        expected_rows = sorted([[key.hex(), value.hex()] for key, value in rows])
        saved = []
        for text, pinyin, zhuyin in (('达达', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'),
                                    ('你你', "ni3'ni3", 'ㄋㄧˇ ㄋㄧˇ')):
            iterator = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
            assert iterator
            reading = pinyin if k.mode == 'pinyin' else zhuyin
            assert k.fn('iterator_add_phrase', B, P, S, S, I)(
                iterator, text.encode(), reading.encode(), 100)
            k.fn('end_add_phrases', None, P)(iterator)
            assert k.fn('save', B, P)(ctx)
            raw = path.read_bytes()
            # HashDBM bytes include the bucket pointers and record links. Compare
            # every byte except the authorized timestamp, including byte 56.
            if original is not None:
                assert tkrzw_saved_bigram_bytes(raw) == tkrzw_saved_bigram_bytes(original), 'cross-reload changed saved bytes'
            actual_rows = index_file_rows(path, kind='hash')
            assert actual_rows == expected_rows, 'cross-reload changed a key or value'
            saved.append(dict(bytes=len(raw), sha256=hashlib.sha256(tkrzw_saved_bigram_bytes(raw)).hexdigest(),
                              rows=actual_rows))
            k.fn('fini', None, P)(ctx)
            ctx = k.init()
            assert ctx
        k.fn('fini', None, P)(ctx)
        return {'saved_bigram': saved}


    for _issue1114_mode in ('pinyin', 'zhuyin'):
        case('issue1114-collision-load-save-' + _issue1114_mode, mode=_issue1114_mode,
             cells=('tkrzw',), issues=('1114',))(issue1114_collision_saves)
