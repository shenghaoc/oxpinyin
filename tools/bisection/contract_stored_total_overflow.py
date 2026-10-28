"""Stored total overflow and log-header cases."""
from contract_abi import B, P, U, tokens_of, unigram_of
from contract_fixtures import _write_user_conf, import_rows


def register(case, *, FACADE_TOTAL, _stored_library_total, _facade_total_out, _modify_header, _write_log):
    """Register these cases at their original driver position."""
    # #802. SubPhraseIndex::add_unigram_frequency refuses when m_total_freq +
    # delta wraps (074a2219 phrase_index.cpp:170-171), and until the first compact
    # m_total_freq is the total stored in the library file (:345), not its items'
    # sum. A MODIFY_HEADER log sets library 1's stored total _STORED_GAP short of
    # the guint32 wrap: a delta of GAP - 1 is accepted, GAP is refused. A save
    # compacts (pinyin.cpp:1130), the total becomes the items' sum, and GAP is
    # accepted. A build that checks the item sum accepts every one of them.
    _STORED_GAP = 5000000


    def stored_total_overflow(refused, after_save):
        """Build a unigram-overflow case against a planted near-wrap stored total.

        refused chooses an overshooting delta; after_save repeats it after a save
        that compacts the index. SubPhraseIndex::add_unigram_frequency checks the
        stored total at 074a2219 phrase_index.cpp:170-171, loaded at
        phrase_index.cpp:345.
        """
        def probe(k):
            """Replace the saved log, add the chosen delta, and observe item and facade totals.

            Close the original context before planting the header. If requested, import
            a user phrase to mark it modified, save, and repeat the same increment.
            """
            total = _stored_library_total(k, 'gb_char.bin')
            k.fn('fini', None, P)(k.ctx)
            k._ctx = None
            k._inst = None
            _write_user_conf(k)
            _write_log(k, _modify_header(0, total, 2 ** 32 - _STORED_GAP))
            # The refused delta overshoots the wrap by the amount that lands the
            # facade total on 4000000000, which moves the ranking off the unbumped one.
            delta = _STORED_GAP - 1
            if refused:
                delta = _STORED_GAP + (4000000000 - FACADE_TOTAL + total) % 2 ** 32
                assert _STORED_GAP <= delta < 2 ** 32
            token = [t for t in tokens_of(k, '号') if t >> 24 == 1][0]
            add = k.fn('token_add_unigram_frequency', B, P, U, U)
            out = {'before': unigram_of(k, token)}
            out['add'] = add(k.inst, token, delta)
            out['after'] = unigram_of(k, token)
            # A refused delta still raised the facade total (phrase_index.h:633-634),
            # which the guess and the candidate frequencies divide by.
            out['facade'] = _facade_total_out(k)
            if after_save:
                # An import marks the context modified; the save compacts.
                out['import'] = import_rows(k, 7, [('好你', "hao3'ni3", 2)])
                out['saved'] = k.fn('save', B, P)(k.ctx)
                out['add-saved'] = add(k.inst, token, delta)
                out['after-saved'] = unigram_of(k, token)
                out['facade-saved'] = _facade_total_out(k)
            return out
        return probe


    case('stored-total-overflow-below', control=True, issues=('802',))(stored_total_overflow(False, False))
    case('stored-total-overflow-above', issues=('802',))(stored_total_overflow(True, False))
    case('stored-total-overflow-above-saved', issues=('802',))(stored_total_overflow(True, True))
    case('stored-total-overflow-below-saved', control=True, issues=('802',))(stored_total_overflow(False, True))


    # Each MODIFY_HEADER must land on the merge's running total, or the merge
    # stops there; one that lands sets the total (phrase_index.cpp:503-514). Two
    # headers from the same old total: the second stops the merge. A chained pair:
    # both apply. Either way the pin's final total wraps the facade to 2000000.
    def log_headers(chained):
        """Build a reload case with two chained or conflicting MODIFY_HEADER records.

        SubPhraseIndex::merge requires each old total to equal the running total
        (074a2219 phrase_index.cpp:503-514); both layouts use the same net shift.
        """
        def probe(k):
            """Write two total-changing log records and observe their effect after reopening.

            With chained true the second record starts from the first's new total;
            otherwise both claim the original total. Return the original stored total
            and facade-ranking observations.
            """
            total = _stored_library_total(k, 'gb_char.bin')
            shift = (2 ** 32 + 2000000 - FACADE_TOTAL) % 2 ** 32
            if chained:
                first = (total + 1000000000) % 2 ** 32
                log = _modify_header(0, total, first) + _modify_header(0, first, (total + shift) % 2 ** 32)
            else:
                log = _modify_header(0, total, (total + shift) % 2 ** 32) + _modify_header(0, total, total)
            k.fn('fini', None, P)(k.ctx)
            k._ctx = None
            k._inst = None
            _write_user_conf(k)
            _write_log(k, log)
            return {'total': total, **_facade_total_out(k)}
        return probe


    case('log-headers-second-stops-merge')(log_headers(False))
    case('log-headers-chained-both-apply')(log_headers(True))
