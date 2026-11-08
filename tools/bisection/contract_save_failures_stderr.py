"""The #929 logical cross-open cases (inventory row 59, #1175).

The two logical cross-open cases are also Rust tests; they stay registered for
`verify --reverse-cross-open`, which lets the pin read the profile oxpinyin wrote.
The family's other cases, the seven save-boundary cases among them, are Rust
tests (`save_failures_stderr` in the capi crates).
"""
import ctypes as C
import os
from pathlib import Path

from contract_abi import Arr, B, I, P, S, U
from contract_case_helpers import TOKEN_REUSE_PHRASES, d18_gram


def register(case, *, CASES, crafted_bigram_context, index_file_rows):
    """Register these cases at their original driver position."""
    def save_929_import(k, ctx, number, count=100):
        """Import one existing test phrase through the selected C facade."""
        entry = TOKEN_REUSE_PHRASES[number]
        iterator = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        assert iterator
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(
            iterator, entry[0].encode(), entry[1 if k.mode == 'pinyin' else 2].encode(), count)
        k.fn('end_add_phrases', None, P)(iterator)


    def save_929_tokens(k, ctx):
        """Observe the saved prefix and unsaved suffix after restart."""
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert inst
        k.glib.g_array_new.restype, k.glib.g_array_new.argtypes = P, [I, I, U]
        k.glib.g_array_free.restype, k.glib.g_array_free.argtypes = P, [P, I]
        result = []
        try:
            for text, _, _ in TOKEN_REUSE_PHRASES[:3]:
                arr = k.glib.g_array_new(0, 0, 4)
                try:
                    k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
                    view = C.cast(arr, C.POINTER(Arr)).contents
                    result.append(list(C.cast(view.data, C.POINTER(U))[:view.len]))
                finally:
                    k.glib.g_array_free(arr, 1)
        finally:
            k.fn('free_instance', None, P)(inst)
        return result


    def save_929_logical_roundtrip(k):
        """Dump exact key/value records and re-save through each receiving facade.

        The authorized pin-only control at a4dd7637 showed independent BDB saves
        are not byte-identical. No fields are masked here: index_file_rows keeps
        complete key/value bytes, including empty values. Fresh cross-open workers
        receive the donor's exact profile through the existing handoff mechanism.
        """
        handoff = os.environ.get('CONTRACT_929_PROFILE')
        if handoff:
            k.user = handoff
        bigram_key = (0x07000001).to_bytes(4, 'little')
        bigram_value = d18_gram(37, 0x07000002, 37)
        if os.environ.get('CONTRACT_929_RELOAD') != '1':
            # Bigram::load_db/save_db copy full rows (ngram_bdb.cpp:47-110).
            k._ctx = crafted_bigram_context(k, [(bigram_key, bigram_value)])
            ctx = k.ctx
            save_929_import(k, ctx, 0)
            save_929_import(k, ctx, 1)
            assert k.fn('save', B, P)(ctx)
            save_929_import(k, ctx, 2)
            k.fn('fini', None, P)(ctx)
            k._ctx = None

        def rows():
            """Read every record, including the nonempty bigram row."""
            return {name: index_file_rows(Path(k.user, name),
                                         kind='hash' if name == 'user_bigram.db' else 'tree',
                                         allow_empty=True)
                    for name in ('user_pinyin_index.bin', 'user_phrase_index.bin', 'user_bigram.db')}

        before = rows()
        assert before['user_bigram.db'] == [[bigram_key.hex(), bigram_value.hex()]]
        ctx = k.ctx
        tokens = save_929_tokens(k, ctx)
        assert tokens == [[0x07000001], [0x07000002], []], tokens
        # Re-adding an existing reading with count zero changes no logical value:
        # src/pinyin.cpp:561-582; src/zhuyin.cpp:449-469 at 074a2219.
        # Re-save witnesses that the C facade loaded the donor's records, rather
        # than merely comparing untouched files with an external dump utility.
        save_929_import(k, ctx, 0, count=0)
        assert k.fn('save', B, P)(ctx)
        after = rows()
        assert before == after, 'facade reopen/save changed logical records'
        k.fn('fini', None, P)(ctx)
        k._ctx = None
        return dict(rows=after, tokens=tokens)


    for _save929_mode in ('pinyin', 'zhuyin'):
        case('save-929-logical-cross-open-' + _save929_mode, mode=_save929_mode,
             issues=('929',))(save_929_logical_roundtrip)
