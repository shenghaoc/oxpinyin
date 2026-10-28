"""Token reuse and addon import cases (inventory row 45, #1175).
"""
import ctypes as C

from contract_abi import B, I, P, S, U, Z
from contract_case_helpers import USER_ALL, candidate_rows


def register(case, *, token_reuse_sequence):
    """Register these cases at their original driver position."""
    TOKEN_REUSE_SEQUENCES = {
        # name: (control, steps). A control holds on the parent too: it has no
        # mask, or reopens first (the load seeds the cursor from the saved items,
        # which already reproduced the pin). A mask below the top of the range
        # hands out the token the parent did, but the save after it writes the
        # masked phrase's index keys back empty (#784).
        'no-mask': (True, [('add', 0), ('add', 1), ('save',), ('add', 2), ('save',)]),
        'mask-all': (False, [('add', 0), ('save',), ('mask',) + USER_ALL, ('save',), ('add', 1), ('save',)]),
        'mask-highest': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000003),
                                 ('save',), ('add', 3), ('save',)]),
        'mask-middle': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000002),
                                ('save',), ('add', 3), ('save',)]),
        'mask-twice': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000003),
                               ('mask', 0xffffffff, 0x07000002), ('add', 3), ('add', 4), ('save',)]),
        'mask-import-mask': (False, [('add', 0), ('add', 1), ('save',), ('mask', 0xffffffff, 0x07000002), ('add', 2),
                                     ('mask', 0xffffffff, 0x07000002), ('add', 3), ('save',)]),
        'mask-reopen': (True, [('add', 0), ('add', 1), ('save',), ('mask', 0xffffffff, 0x07000002), ('save',),
                                ('reopen',), ('add', 2), ('save',)]),
        'mask-unsaved-reopen': (True, [('add', 0), ('add', 1), ('save',), ('mask', 0xffffffff, 0x07000002),
                                        ('reopen',), ('add', 2), ('save',)]),
        'mask-reopen-middle': (True, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x07000002),
                                      ('save',), ('reopen',), ('add', 3), ('save',)]),
    }
    # A user bigram row under the token and another that lists it as a successor:
    # the pin's `mask_out` drops both, so the reused token starts clean.
    TOKEN_REUSE_BIGRAM_ROWS = [
        (b'\x01\x00\x00\x07', b''.join(n.to_bytes(4, 'little') for n in (100, 0x01000001, 100))),
        (b'\x01\x00\x00\x01', b''.join(n.to_bytes(4, 'little') for n in (100, 0x07000001, 100))),
    ]
    for _mode in ('pinyin', 'zhuyin'):
        for _name, (_control, _steps) in TOKEN_REUSE_SEQUENCES.items():
            case('issue762-token-reuse-%s-%s' % (_name, _mode), mode=_mode, control=_control, issues=('762',))(
                lambda k, steps=_steps: token_reuse_sequence(k, steps))
        # BDB only: token_reuse_files compares the saved bigram's non-empty rows
        # through db_dump; kc/tkrzw report only its presence, which cannot check
        # that the seeded stale successor was removed.
        case('issue762-token-reuse-stale-bigram-%s' % _mode, mode=_mode, cells=('bdb',), issues=('762',))(
            lambda k: token_reuse_sequence(k, TOKEN_REUSE_SEQUENCES['mask-all'][1], TOKEN_REUSE_BIGRAM_ROWS))
        # The no-mask control needs the same BDB row comparison to establish
        # that the seeded bigram survives; kc/tkrzw presence alone cannot do so.
        case('issue762-token-reuse-stale-bigram-no-mask-%s' % _mode, mode=_mode, control=True, cells=('bdb',), issues=('762',))(
            lambda k: token_reuse_sequence(k, TOKEN_REUSE_SEQUENCES['no-mask'][1], TOKEN_REUSE_BIGRAM_ROWS))


    # The pin takes the next token from the trimmed range end for every index
    # (`_add_phrase`, `pinyin.cpp:586-592`) and for addon promotion
    # (`pinyin.cpp:2541-2544`, which has no `token++` over a zero low word), so the
    # ADDON_DICTIONARY sub-index (5) frees its top the same way the user one does.
    ADDON_ALL = (0x0f000000, 0x05000000)
    ADDON_IMPORT_SEQUENCES = {
        'no-mask': (True, [('add', 0), ('add', 1), ('add', 2), ('save',), ('add', 3), ('save',)]),
        'mask-highest': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x05000003),
                                 ('add', 3), ('save',)]),
        'mask-middle': (False, [('add', 0), ('add', 1), ('add', 2), ('save',), ('mask', 0xffffffff, 0x05000002),
                                ('add', 3), ('save',)]),
    }
    for _mode in ('pinyin', 'zhuyin'):
        for _name, (_control, _steps) in ADDON_IMPORT_SEQUENCES.items():
            case('issue762-addon-import-%s-%s' % (_name, _mode), mode=_mode, control=_control, issues=('762',))(
                lambda k, steps=_steps: token_reuse_sequence(k, steps, library=5))


    # A promoted addon phrase leaves two rows with one string in the next list:
    # the NORMAL row of the new nibble-5 token and the ADDON row. The phrase-string
    # dedup keeps the row with the larger `m_freq`, and a tie keeps the earlier row
    # (`_remove_duplicated_items_by_phrase_string`, `pinyin.cpp:2128-2138`). The
    # ADDON row is scaled by the addon index total (`:1828-1842`) and the NORMAL row
    # by the whole index total (`:1855-1866`), so the ADDON row survives.
    def addon_promoted_listing(k, sort_option):
        """Compare 书学 candidate rows before and after promoting its addon entry.

        Load addon library 4 and generate fresh lists with sort_option. Assert a
        matching kind-6 row, record its selection result, both counts and matching
        rows, and the entire final list. Free the two instances on normal completion."""
        ctx = k.ctx
        assert k.fn('load_addon_phrase_library', B, P, C.c_ubyte)(ctx, 4)
        out = {}

        def listing(label):
            """Parse shuxue on a new instance and store the list size and 书学 rows under label.

            Return the live instance and complete rows for the caller to inspect and free."""
            inst = k.fn('alloc_instance', P, P)(ctx)
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'shuxue')
            k.fn('guess_candidates', B, P, Z, U)(inst, 0, sort_option)
            rows = candidate_rows(k, inst)
            out[label] = [len(rows), [row for row in rows if row[1] == '书学']]
            return inst, rows

        inst, rows = listing('before')
        count = U()
        k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
        chosen = None
        for i in range(count.value):
            cand, kind, text = P(), I(), S()
            k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
            if kind.value == 6 and text.value.decode() == '书学':
                chosen = cand
                break
        assert chosen, 'no addon candidate for shuxue'
        out['choose'] = k.fn('choose_candidate', I, P, Z, P)(inst, 0, chosen)
        k.fn('free_instance', None, P)(inst)
        inst, rows = listing('after')
        k.fn('free_instance', None, P)(inst)
        out['rows after'] = rows
        return out


    # `sort_option` 0 sorts by nothing, so the two rows stay in collection order
    # (NORMAL first) and only the dedup picks the ADDON row. 0x1c is
    # `SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY`: the sort
    # puts the larger `m_freq` (the ADDON row) first, so a keep-first dedup also
    # answers the pin's row; that case is the control.
    case('addon-promoted-listing-unsorted', mode='pinyin', issues=('913',))(
        lambda k: addon_promoted_listing(k, 0))
    case('addon-promoted-listing-by-frequency', mode='pinyin', control=True, issues=('913',))(
        lambda k: addon_promoted_listing(k, 0x1c))
