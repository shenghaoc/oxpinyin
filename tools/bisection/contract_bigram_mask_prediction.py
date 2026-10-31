"""Bigram mask and prediction cases (inventory row 34, #1175).
"""
import ctypes as C
import hashlib
import os
from pathlib import Path

from contract_abi import Arr, B, I, P, S, U, Z, row_of
from contract_case_helpers import d18_bigram_db, d18_gram
from contract_fixtures import _write_user_conf
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
        # run_case supplies one donor directory per direction. The receiver
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


    def predicted_bigram_row_context(k, phrase, successor, count=20):
        """A context whose user bigram lists `successor` after `phrase` with
        `count` (the pin lists a predicted-bigram row from a count of 10 and a
        one- or two-word successor, `pinyin.cpp:2331-2368`). Both tokens are read
        from this side's own table.

        Use the first successor token and encode count as unsigned four-byte
        little-endian total and item count. Assert lookups and reopening, replacing
        the bigram file and marker. Free the lookup instance but not its arrays;
        return the uncached context. The helper does not validate the count or
        phrase-length eligibility for prediction itself."""
        ctx = k.init()
        assert ctx, 'the first init failed'
        inst = k.fn('alloc_instance', P, P)(ctx)
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        found = []
        for text in (phrase, successor):
            arr = new(0, 0, 4)
            assert k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
            view = C.cast(arr, C.POINTER(Arr)).contents
            found.append(list(C.cast(view.data, C.POINTER(U))[:view.len]))
        k.fn('free_instance', None, P)(inst)
        k.fn('fini', None, P)(ctx)
        value = count.to_bytes(4, 'little') + found[1][0].to_bytes(4, 'little') + count.to_bytes(4, 'little')
        _craft_hash(os.path.join(k.user, 'user_bigram.db'), [(token.to_bytes(4, 'little'), value) for token in found[0]])
        _write_user_conf(k)
        ctx = k.init()
        assert ctx, 'the reopen failed'
        return ctx


    # `pinyin_choose_candidate` takes no predicted row of either kind
    # (`pinyin.cpp:2507`); the prefix arm is `abort-choose-candidate-predicted-prefix-row`.
    # BDB-only coverage inherited from e7ad3ab9: the predicted-bigram row and
    # pinyin.cpp:2507 abort were verified there. _craft_hash now supports kc and
    # tkrzw too, but this case's row selection has not been verified on them;
    # their exclusion is a coverage limit, not an unreadable fixture format.
    # #1231 follow-up: the pin's `is_user_candidate` rejects every predicted row
    # before it reads the token library (`pinyin.cpp:3712-3715`), so a predicted
    # bigram whose successor token lives in USER_DICTIONARY still answers false.
    # Import the successor into library 7, plant the user bigram through the
    # existing builder, then query the predicted-bigram row. The parent build
    # answered true because it tested only the token nibble.
    @case('predicted-bigram-is-user-candidate', issues=('1231',))
    def _(k):
        """Import 龘龘 into library 7, predict it as a bigram successor, and query user status."""
        ctx = k.init()
        assert ctx, 'the import context failed'
        iterator = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        assert iterator
        added = k.fn('iterator_add_phrase', B, P, S, S, I)(
            iterator, '龘龘'.encode(), "da2'da2".encode(), 5)
        k.fn('end_add_phrases', None, P)(iterator)
        assert added, 'the user successor did not import'
        assert k.fn('save', B, P)(ctx), 'the user successor did not save'
        k.fn('fini', None, P)(ctx)
        ctx = predicted_bigram_row_context(k, '我', '龘龘')
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, '我'.encode())
        row = row_of(k, inst, 4)
        assert row, 'the planted gram listed no predicted-bigram row'
        kind, text = I(), S()
        typed = k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, row, C.byref(kind))
        rendered = k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, row, C.byref(text))
        return {'type': [typed, kind.value],
                'text': [rendered, None if text.value is None else text.value.decode('utf-8', 'replace')],
                'is-user': k.fn('is_user_candidate', B, P, P)(inst, row)}


    @case('abort-choose-candidate-predicted-bigram-row', abort=0, sigabrt=True, cells=('bdb',))
    def _(k):
        """Plant 我-to-你好 predictions and pass a kind-4 row to ordinary candidate selection.

        Assert the predicted-bigram row exists; record the signed selection result."""
        ctx = predicted_bigram_row_context(k, '我', '你好')
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, '我'.encode())
        row = row_of(k, inst, 4)
        assert row, 'the planted gram listed no predicted-bigram row'
        return {'ret': k.fn('choose_candidate', I, P, Z, P)(inst, 0, row)}


    @case('abort-mask-out-short-bigram-value', abort=False)
    def _(k):
        """Mask a profile whose token-one bigram value is three bytes long."""
        # The pin's `mask_out` loads the gram and `get_total_freq` asserts on
        # the three-byte value (`memory_chunk.h:390`, `ngram.cpp:80`).
        ctx = crafted_bigram_context(k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    @case('abort-zhuyin-mask-out-short-bigram-value', mode='zhuyin', abort=False)
    def _(k):
        """Run zhuyin mask_out with a three-byte bigram value under token one."""
        ctx = crafted_bigram_context(k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    @case('mask-out-short-bigram-value-masked-key', control=True)
    def _(k):
        """Mask token one itself in a profile carrying its three-byte bigram value.

        Record the native flag for deleting the key rather than editing its items."""
        # A short value whose key the mask erases wholesale is never loaded, so
        # the pin erases it and completes (`ngram_bdb.cpp:231-238`); the
        # `get_total_freq` assert is reached only for a key the mask leaves
        # alone. A control: the guard must not over-refuse here, so pin and
        # subject both complete and the parent build (no bigram checks) matches.
        ctx = crafted_bigram_context(k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 1)}


    @case('abort-mask-out-non-token-bigram-key', abort=False, sigabrt=True)
    def _(k):
        """Mask a profile with a two-byte bigram key and a four-byte total value."""
        # The pin's `mask_out` walks the container with `get_all_items`, whose
        # `key.size == sizeof(phrase_token_t)` assert dies on the two-byte key
        # (`ngram_bdb.cpp:199`).
        ctx = crafted_bigram_context(k, [(b'\x00\x01', b'\x07\x00\x00\x00')])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    @case('abort-zhuyin-mask-out-non-token-bigram-key', mode='zhuyin', abort=False)
    def _(k):
        """Run zhuyin mask_out against a two-byte bigram key with a four-byte value."""
        ctx = crafted_bigram_context(k, [(b'\x00\x01', b'\x07\x00\x00\x00')])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    # A gram whose `total_freq` is not covered by its items (total 7, one item of
    # frequency 0): the mask removes the item (token 0 == value 0) but the key is
    # left alone, so `SingleGram::mask_out` leaves a residual total and
    # `Bigram::mask_out`'s `get_length` assert dies (`ngram.cpp:70`,
    # `ngram_bdb.cpp:243`).
    @case('abort-mask-out-residual-bigram-gram', abort=False, sigabrt=True)
    def _(k):
        """Mask successor zero from a planted gram with total seven and a zero-count item."""
        ctx = crafted_bigram_context(
            k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00\x00' + b'\x00' * 8)])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    @case('abort-zhuyin-mask-out-residual-bigram-gram', mode='zhuyin', abort=False)
    def _(k):
        """Run the residual-total gram mask through zhuyin and record its return flag."""
        ctx = crafted_bigram_context(
            k, [(b'\x01\x00\x00\x00', b'\x07\x00\x00\x00' + b'\x00' * 8)])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    @case('abort-begin-bigram-phrases-non-token-key', abort=False, sigabrt=True)
    def _(k):
        """Begin bigram export after planting a two-byte key, returning iterator-pointer truthiness.

        The iterator is not advanced or closed if creation returns."""
        # `pinyin_begin_get_bigram_phrases` walks the same container through
        # `get_all_items` (`pinyin.cpp:776-787`), so the two-byte key aborts at
        # `ngram_bdb.cpp:199` before any row is exported.
        ctx = crafted_bigram_context(k, [(b'\x00\x01', b'\x07\x00\x00\x00')])
        return {'ret': bool(k.fn('begin_get_bigram_phrases', P, P)(ctx))}


    def issue677_sequence(k):
        """The #677 sequence over a valid bigram row that the mask removes.

        Create a profile with one library-seven predecessor, import three phrases
        across five saves and mask user-library tokens after the first save.
        Return operation flags, the file inventory and the seeded key's raw value
        or absence after masking and at the end. Backend get results remain
        observable; the reopened context is retained for the worker."""
        ctx = crafted_bigram_context(
            k, [(b'\x01\x00\x00\x07', d18_gram(100, 0x01000001, 100))])

        def add(text, reading):
            """Import a phrase with count 100 into the captured context's user library.

            Encode text and reading as UTF-8, assert iterator/add success and close it."""
            it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
            assert it, 'the import iterator'
            assert k.fn('iterator_add_phrase', B, P, S, S, I)(
                it, text.encode(), reading.encode(), 100)
            k.fn('end_add_phrases', None, P)(it)

        def save():
            """Save the captured native context and return its boolean result."""
            return bool(k.fn('save', B, P)(ctx))

        reading = (lambda text: text) if k.mode == 'pinyin' else (lambda text: text)
        add('达达', "da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ')
        ret1 = save()
        mask = bool(k.fn('mask_out', B, P, U, U)(ctx, 0x0f000000, 0x07000000))
        ret2 = save()
        path = Path(k.user, 'user_bigram.db')
        key = b'\x01\x00\x00\x07'
        after_mask = d18_bigram_db(k, path, read_keys=[key], allow_missing=True)[0]
        add('你你', "ni3'ni3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄋㄧˇ')
        ret3 = save()
        ret4 = save()
        add('好好', "hao3'hao3" if k.mode == 'pinyin' else 'ㄏㄠˇ ㄏㄠˇ')
        ret5 = save()
        result = {
            'ret1': ret1, 'mask': mask, 'ret2': ret2, 'ret3': ret3,
            'ret4': ret4, 'ret5': ret5,
            'files': sorted(os.listdir(k.user)),
            'removed_key_after_mask': after_mask.hex() if after_mask is not None else None,
        }
        # The raw DBM contract is the backend's own `Get` result. On a
        # library that retains a removed row, its value remains observable;
        # on a library that deletes it, the result is absent. Compare that
        # value, the return sequence and the user-file inventory in every cell.
        final = d18_bigram_db(k, path, read_keys=[key], allow_missing=True)[0]
        result['removed_key_final'] = final.hex() if final is not None else None
        return result


    for _issue677_mode in ('pinyin', 'zhuyin'):
        case('issue677-mask-save-sequence-' + _issue677_mode, mode=_issue677_mode, issues=('677',))(issue677_sequence)
