"""Pinyin and zhuyin duplicate phrase-add cases."""
import os

from contract_abi import B, I, P, S, U, tokens_of
from contract_fixtures import _write_user_conf


def register(case, *, _craft_db, _ucs4_key):
    """Register the cases at their original position in the driver."""
    @case('abort-zhuyin-add-phrase-duplicate-library-token', mode='zhuyin', abort=False, sigabrt=True, issues=('525',))
    def _(k):
        """Plant two library-seven tokens for 你好, reopen zhuyin and attempt importing that phrase.

        Replace the user phrase index and marker; assert both initializations and
        leave the final import iterator/context live if the call returns."""
        # Two tokens for 你好, both in library 7 (the target): the pin's second
        # in-index candidate asserts at `:440`.
        ctx = k.init()
        assert ctx, 'the first init failed'
        k.fn('fini', None, P)(ctx)
        value = (0x07000001).to_bytes(4, 'little') + (0x07000002).to_bytes(4, 'little')
        _craft_db(os.path.join(k.user, 'user_phrase_index.bin'), [(_ucs4_key('你好'), value)], 1)
        _write_user_conf(k)
        ctx = k.init()
        assert ctx, 'the reopen failed'
        it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, '你好'.encode(), 'ㄋㄧˇ ㄏㄠˇ'.encode(), 1)}


    @case('abort-zhuyin-add-phrase-index-text-mismatch', mode='zhuyin', abort=False, sigabrt=True, issues=('525',))
    def _(k):
        """Persist 你們, redirect 你好 to its user token and attempt zhuyin import after reopening.

        Assert setup import/save, replace the phrase index and marker, and free the
        cached instance before finalizing. Leave the final iterator/context live."""
        # Persist 你們, then point 你好's phrase-table row at 你們's token: the
        # lone in-library match names an item whose text differs, `:457`.
        ctx = k.ctx
        it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, '你們'.encode(), 'ㄋㄧˇ ㄇㄣˊ'.encode(), 1), 'the setup phrase did not add'
        k.fn('end_add_phrases', None, P)(it)
        assert k.fn('save', B, P)(ctx)
        token = int(tokens_of(k, '你們')[0])
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(ctx)
        _craft_db(os.path.join(k.user, 'user_phrase_index.bin'),
                  [(_ucs4_key('你好'), token.to_bytes(4, 'little'))], 1)
        _write_user_conf(k)
        ctx = k.init()
        assert ctx, 'the reopen failed'
        it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, '你好'.encode(), 'ㄋㄧˇ ㄏㄠˇ'.encode(), 1)}


    # The pinyin import iterator's `_add_phrase` walks the same phrase table
    # (`pinyin.cpp:533-571`): two tokens in the target library trip the assert at
    # `:554`; a lone one whose phrase-index item has another length trips `:568`,
    # one with the same length and other text `:571`. `_craft_db` writes
    # `user_phrase_index.bin` in the running cell's format: a BDB btree, KC
    # snapshot or tkrzw TreeDBM. All three cells are verified (#1120).
    def _persist_pinyin_phrase(k, text, reading):
        """Adds `text` to library 7, saves, and returns its token; the context
        is closed."""
        ctx = k.ctx
        it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, text.encode(), reading.encode(), 1), 'the setup phrase did not add'
        k.fn('end_add_phrases', None, P)(it)
        assert k.fn('save', B, P)(ctx)
        token = int(tokens_of(k, text)[0])
        assert token >> 24 == 7, 'the setup phrase is not new to library 7'
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(ctx)
        k._ctx = None
        return token


    def _add_pinyin_phrase_after_index(k, value):
        """Points 你好's phrase-table row at `value` and adds 你好 to library 7."""
        _craft_db(os.path.join(k.user, 'user_phrase_index.bin'), [(_ucs4_key('你好'), value)], 1)
        _write_user_conf(k)
        ctx = k.init()
        assert ctx, 'the reopen failed'
        it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), b"ni'hao", 1)}


    # Row 104 through libzhuyin: a lone in-library token whose item has
    # another length, or no item at all (a zero-length `PhraseItem`), trips
    # `assert(phrase_length == item.get_phrase_length())` (`zhuyin.cpp:454`).
    def _add_zhuyin_phrase_after_index(k, value):
        """Points 你好's phrase-table row at `value` and adds 你好 to library 7 through libzhuyin."""
        _craft_db(os.path.join(k.user, 'user_phrase_index.bin'), [(_ucs4_key('你好'), value)], 1)
        _write_user_conf(k)
        ctx = k.init()
        assert ctx, 'the reopen failed'
        it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        return {'ret': k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, '你好'.encode(), 'ㄋㄧˇ ㄏㄠˇ'.encode(), 1)}


    @case('abort-zhuyin-add-phrase-index-length-mismatch', mode='zhuyin', abort=False, sigabrt=True,
          issues=('525', 'row:104'))
    def _(k):
        """Persist 你好嗷 through libzhuyin, redirect 你好 to its token and attempt a shorter import."""
        token = _persist_pinyin_phrase(k, '你好嗷', 'ㄋㄧˇ ㄏㄠˇ ㄠˊ')
        return _add_zhuyin_phrase_after_index(k, token.to_bytes(4, 'little'))


    @case('abort-zhuyin-add-phrase-index-item-missing', mode='zhuyin', abort=False, sigabrt=True,
          issues=('525', 'row:104'))
    def _(k):
        """Plant a library-seven token without an item and attempt a zhuyin import of 你好."""
        ctx = k.init()
        assert ctx, 'the first init failed'
        k.fn('fini', None, P)(ctx)
        return _add_zhuyin_phrase_after_index(k, (0x07000001).to_bytes(4, 'little'))


    @case('abort-add-phrase-duplicate-library-token', abort=False, sigabrt=True, userdir=True, issues=('525',))
    def _(k):
        """Initialize/finalize, plant two library-seven tokens for 你好 and attempt importing it."""
        ctx = k.init()
        assert ctx, 'the first init failed'
        k.fn('fini', None, P)(ctx)
        value = (0x07000001).to_bytes(4, 'little') + (0x07000002).to_bytes(4, 'little')
        return _add_pinyin_phrase_after_index(k, value)


    @case('abort-add-phrase-index-length-mismatch', abort=False, sigabrt=True, userdir=True, issues=('525',))
    def _(k):
        """Persist 你好嗷, redirect 你好 to its token and attempt an import with a shorter phrase."""
        token = _persist_pinyin_phrase(k, '你好嗷', "ni'hao'ao")
        return _add_pinyin_phrase_after_index(k, token.to_bytes(4, 'little'))


    @case('abort-add-phrase-index-item-missing', abort=False, sigabrt=True, userdir=True, issues=('525',))
    def _(k):
        """Plant a phrase-table token without a library item and attempt importing 你好 after reopening."""
        # The phrase table names library 7's token, but user.bin has no item:
        # get_phrase_item leaves a zero-length PhraseItem, so :568 asserts.
        ctx = k.init()
        assert ctx, 'the first init failed'
        k.fn('fini', None, P)(ctx)
        return _add_pinyin_phrase_after_index(k, (0x07000001).to_bytes(4, 'little'))


    @case('abort-add-phrase-index-text-mismatch', abort=False, sigabrt=True, userdir=True, issues=('525',))
    def _(k):
        """Persist 你們, redirect 你好 to its token and attempt an equal-length mismatched-text import."""
        token = _persist_pinyin_phrase(k, '你們', "ni'men")
        return _add_pinyin_phrase_after_index(k, token.to_bytes(4, 'little'))
