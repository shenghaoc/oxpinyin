"""Candidate removals and refusals cases (inventory row 32, #1175).
"""
import ctypes as C
import os

from contract_abi import Arr, B, I, P, S, U, Z, full_inst, guessed, row_of
from contract_case_helpers import _add_user_phrase, _user_candidate_rows
from contract_fixtures import _write_user_conf


def register(case, *, _craft_hash):
    """Register these cases at their original driver position."""
    def predicted(k, prefix='我'):
        """Return a new instance with predictions and punctuation for the UTF-8 prefix.

        Ignore the generation flag and leave the instance allocated for the caller."""
        inst = k.alloc()
        k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, prefix.encode())
        return inst


    @case('abort-get-candidate-nbest-index-normal-row', abort=False, issues=('525',))
    def _(k):
        """Request an n-best index from a normal candidate and record whether 0xAB survives."""
        inst = guessed(k)
        out = C.c_ubyte(0xAB)
        ret = k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
            inst, row_of(k, inst, 2), C.byref(out))
        # The pin dies before it writes `*index`; an `untouched*` field must stay true.
        return {'ret': ret, 'untouched index': out.value == 0xAB}


    @case('abort-remove-user-candidate-normal-system-token', abort=False, issues=('525',))
    def _(k):
        """Attempt user-candidate removal on the first normal row from a system nihao guess."""
        inst = guessed(k)
        return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, row_of(k, inst, 2))}


    @case('abort-remove-user-candidate-sentence-row', abort=False, issues=('525',))
    def _(k):
        """Attempt user-candidate removal on the sentence row from a nihao guess."""
        inst = guessed(k)
        return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, row_of(k, inst, 1))}


    # lane #525 batch C: the pin asserts when pinyin_remove_user_candidate cannot
    # remove a user phrase (class (c): pinyin.cpp:3743/3750/3759). The subject
    # fails the call with false and one warning in the libpinyin domain.


    def _guessed_user_phrase(k, text, readings, parse=b'dada'):
        """Import a user phrase, parse its input on a new instance and return instance and rows.

        Generate candidates at byte zero with sort zero and assert that at least
        one normal user row exists; do not free the returned instance or its rows."""
        _add_user_phrase(k, text, readings)
        inst = k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        rows = _user_candidate_rows(k, inst)
        assert rows
        return inst, rows


    @case('abort-remove-user-candidate-twice', abort=False, issues=('525',))
    def _(k):
        """Remove one imported candidate twice, asserting the first removal succeeds.

        Reuse the same borrowed pointer and return the first flag and the second
        call's flag if both calls return."""
        inst, rows = _guessed_user_phrase(k, '龘龘', [b"da2'da2"])
        first = k.fn('remove_user_candidate', B, P, P)(inst, rows[0])
        # The first removal must complete, or the second call would assert for
        # the wrong reason (the phrase still present).
        assert first, 'the first removal did not complete'
        # The phrase is gone from the user dictionary: the pin asserts at :3743.
        return {'first': first, 'ret': k.fn('remove_user_candidate', B, P, P)(inst, rows[0])}


    @case('abort-remove-user-candidate-missing-phrase-table', abort=False, issues=('525',))
    def _(k):
        """Remove a user candidate after saving and renaming its phrase-table file away.

        Finalize the cached context, move user_phrase_index.bin to a .gone path,
        and reopen independently. Assert reopening and a normal user row, then
        record removal; the kit's cached context is not replaced."""
        _add_user_phrase(k, '龘龘', [b"da2'da2"])
        assert k.fn('save', B, P)(k.ctx)
        k.fn('fini', None, P)(k.ctx)
        # user_phrase_index.bin holds the phrase table; user.bin still holds the
        # phrase index, so :3743 passes and :3750 fires.
        index = os.path.join(k.user, 'user_phrase_index.bin')
        os.replace(index, index + '.gone')
        ctx = k.init()
        assert ctx
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'dada')
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        rows = _user_candidate_rows(k, inst)
        assert rows
        return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, rows[0])}


    @case('abort-remove-user-candidate-multi-pron', abort=False, issues=('525',))
    def _(k):
        """Import two readings for one phrase and attempt to remove its first normal user row."""
        # Two readings for one phrase: the second is merged without a pinyin
        # index entry, so :3759 fires when the phrase is removed.
        inst, rows = _guessed_user_phrase(k, '龘龘', [b"da2'da2", b"da1'da1"])
        return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, rows[0])}


    # batch 525-D: `pinyin_remove_user_candidate` also drives `Bigram::mask_out`
    # (`pinyin.cpp:3766`, mask `PHRASE_INDEX_LIBRARY_MASK|PHRASE_MASK`), so all
    # three corrupt user-bigram shapes reach it. The pin's phrase-index removals
    # are in memory only and die with the abort, so a failed call leaves the store
    # unchanged; the subject refuses before it removes anything.
    def _removable_user_bigram_context(k, rows_for):
        """A context with one persisted user phrase and a crafted `user_bigram.db`
        carrying `rows_for(token)`. The phrase makes the candidate row acceptable
        (the batch-C checks pass), so the pin reaches `user_bigram->mask_out`
        (`pinyin.cpp:3766`). `rows_for` gets the phrase's user token, which the
        residual shape's item must equal — the pin's mask selects nothing
        otherwise.

        Import and save 龘龘, finalize the cached context, replace user_bigram.db
        and write the profile marker. Assert reopening and candidate generation.
        Return the new instance and borrowed removable candidate without updating
        the kit's cached context. The allocated lookup array is not freed here."""
        _add_user_phrase(k, '龘龘', [b"da2'da2"])
        assert k.fn('save', B, P)(k.ctx)
        probe = k.alloc()
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        arr = new(0, 0, 4)
        assert k.fn('lookup_tokens', B, P, S, P)(probe, '龘龘'.encode(), arr)
        view = C.cast(arr, C.POINTER(Arr)).contents
        token = list(C.cast(view.data, C.POINTER(U))[:view.len])[0]
        k.fn('free_instance', None, P)(probe)
        k.fn('fini', None, P)(k.ctx)
        _craft_hash(os.path.join(k.user, 'user_bigram.db'), rows_for(token))
        _write_user_conf(k)
        ctx = k.init()
        assert ctx, 'the reopen failed'
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'dada')
        assert k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        candidates = _user_candidate_rows(k, inst)
        assert candidates, 'no user candidate'
        return inst, candidates[0]


    @case('abort-remove-user-candidate-short-bigram-value', abort=False, issues=('525',))
    def _(k):
        """Remove a persisted user phrase with a three-byte value planted in its bigram store."""
        inst, cand = _removable_user_bigram_context(
            k, lambda _token: [(b'\x01\x00\x00\x00', b'\x07\x00\x00')])
        return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, cand)}


    @case('abort-remove-user-candidate-non-token-bigram-key', abort=False, issues=('525',))
    def _(k):
        """Remove a persisted user phrase with a two-byte key planted in its bigram store."""
        inst, cand = _removable_user_bigram_context(
            k, lambda _token: [(b'\x00\x01', b'\x07\x00\x00\x00')])
        return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, cand)}


    @case('abort-remove-user-candidate-residual-bigram-gram', abort=False, issues=('525',))
    def _(k):
        """Remove a user phrase whose planted bigram total exceeds its sole item's count.

        Replace and reopen the profile through _removable_user_bigram_context,
        then return the removal flag if the native call completes."""
        # `total_freq` 7 with one item of the removed token and frequency 3:
        # `SingleGram::mask_out` drops the item and leaves 4, so the pin's
        # `get_length` assert dies (`ngram.cpp:70`, `ngram_bdb.cpp:243`).
        def rows(token):
            """Encode a little-endian gram with total seven and one item of count three.

            Use the removed token as the successor and token one as the container key."""
            value = b'\x07\x00\x00\x00' + token.to_bytes(4, 'little') + (3).to_bytes(4, 'little')
            return [(b'\x01\x00\x00\x00', value)]

        inst, cand = _removable_user_bigram_context(k, rows)
        return {'ret': k.fn('remove_user_candidate', B, P, P)(inst, cand)}


    @case('abort-choose-candidate-predicted-prefix-row', abort=0, issues=('525',))
    def _(k):
        """Pass a predicted-prefix row to ordinary candidate selection at byte zero."""
        inst = predicted(k)
        return {'ret': k.fn('choose_candidate', I, P, Z, P)(inst, 0, row_of(k, inst, 5))}


    @case('abort-choose-predicted-candidate-normal-row', abort=False, issues=('525',))
    def _(k):
        """Pass a normal nihao candidate to the predicted-candidate selection API."""
        inst = guessed(k)
        return {'ret': k.fn('choose_predicted_candidate', B, P, P)(inst, row_of(k, inst, 2))}


    @case('abort-choose-predicted-candidate-sentence-row', abort=False, issues=('525',))
    def _(k):
        """Pass a nihao sentence row to the predicted-candidate selection API."""
        inst = guessed(k)
        return {'ret': k.fn('choose_predicted_candidate', B, P, P)(inst, row_of(k, inst, 1))}


    # `pinyin_choose_candidate` under SORT_WITHOUT_SENTENCE_CANDIDATE asserts
    # `0 == offset` on the NORMAL leg (`pinyin.cpp:2566`, register row 45).
    def choose_without_sentence(k, offset):
        """Choose a normal nihao row at the supplied offset with sentence candidates disabled.

        Generate the list at zero with sort word 0x1, assert a normal row exists and
        return the signed selection result. The supplied offset is not validated."""
        inst = full_inst(k, b'nihao')
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1)
        row = row_of(k, inst, 2)
        assert row, 'no NORMAL row'
        return {'ret': k.fn('choose_candidate', I, P, Z, P)(inst, offset, row)}


    @case('abort-choose-candidate-without-sentence-offset-1', abort=0, issues=('525',))
    def _(k):
        """Select at byte one from a list generated at zero with sentence rows disabled."""
        return choose_without_sentence(k, 1)


    # At offset 0 the leg trains the unigram and answers 1 on both sides.
    @case('choose-candidate-without-sentence-offset-0', control=True, issues=('525',))
    def _(k):
        """Select at byte zero with sentence rows disabled and record the selection result."""
        return choose_without_sentence(k, 0)


    # The rows the guards leave alone answer without a warning.
    @case('candidate-type-neighbours', control=True, issues=('525',))
    def _(k):
        """Record accepted candidate-type operations on guessed and predicted lists.

        Read a sentence row's n-best index, query user status, choose a normal row,
        and choose predicted-prefix and punctuation rows on a separate instance."""
        out = {}
        inst = guessed(k)
        nbest = C.c_ubyte(0xAB)
        out['nbest of sentence row'] = [k.fn('get_candidate_nbest_index', B, P, P, C.POINTER(C.c_ubyte))(
            inst, row_of(k, inst, 1), C.byref(nbest)), nbest.value]
        out['is user'] = [k.fn('is_user_candidate', B, P, P)(inst, row_of(k, inst, kind)) for kind in (1, 2)]
        out['choose normal'] = k.fn('choose_candidate', I, P, Z, P)(inst, 0, row_of(k, inst, 2))
        pred = predicted(k)
        out['choose predicted prefix'] = k.fn('choose_predicted_candidate', B, P, P)(pred, row_of(k, pred, 5))
        out['choose predicted punctuation'] = k.fn('choose_predicted_candidate', B, P, P)(pred, row_of(k, pred, 8))
        return out
