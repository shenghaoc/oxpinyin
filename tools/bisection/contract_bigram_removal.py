"""Bigram removal cases (inventory row 31, #1175).
"""
import ctypes as C
from pathlib import Path

from contract_abi import B, P, S, U, Z, tokens_of
from contract_case_helpers import _add_user_phrase, _user_candidate_rows, d18_bigram_db, d18_gram
from contract_fixtures import _write_user_conf


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #740: removal subtracts from the stored guint32 total, never from a sum
# of survivors (074a2219 ngram.cpp:76-102, ngram_bdb.cpp:215-254;
# pinyin.cpp:1230,3766; zhuyin.cpp:765). Read persisted rows after save
# and again after reopen so the live container and export paths agree.
def removal_bigram(total, survivor_counts, removed_count, operation, multiple=False, unchanged_total=17):
    """Build a removal/save/reopen case with independently supplied bigram totals and counts.

    Pair survivor_counts with 好 and 号 by zip. multiple adds token + 1 and
    makes mask-out target all of library seven; operation='remove-candidate'
    uses candidate removal instead. unchanged_total seeds a second predecessor."""
    def probe(k):
        """Import 龘龘, replace user grams, perform removal and compare persisted bytes across reopen.

        Sort the planted successor records, mark the context modified by an
        unconstrained wo training call, and record operation/save flags plus raw
        hex rows. Close and reset cached objects around file access, require reopen
        to succeed and close it again before the second read."""
        reading = b"da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ'.encode()
        _add_user_phrase(k, '龘龘', [reading])
        token = next(t for t in tokens_of(k, '龘龘') if t >> 24 == 7)
        predecessors = [tokens_of(k, text)[0] for text in ('你', '我')]
        successors = [tokens_of(k, text)[0] for text in ('好', '号')]
        assert k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None

        records = list(zip(successors, survivor_counts)) + [(token, removed_count)]
        if multiple:
            records.append((token + 1, 1))
        gram = total.to_bytes(4, 'little') + b''.join(
            t.to_bytes(4, 'little') + count.to_bytes(4, 'little') for t, count in sorted(records))
        # The second predecessor is untouched, with a deliberately independent
        # stored total. Removing zero items must leave that total alone too.
        unchanged = d18_gram(unchanged_total, successors[0], 2)
        keys = [p.to_bytes(4, 'little') for p in predecessors]
        path = Path(k.user, 'user_bigram.db')
        d18_bigram_db(k, path, list(zip(keys, (gram, unchanged))))
        _write_user_conf(k)

        if operation == 'remove-candidate':
            assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
            assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
            candidates = _user_candidate_rows(k, k.inst)
            assert candidates, 'missing removable user candidate'
            removed = k.fn('remove_user_candidate', B, P, P)(k.inst, candidates[0])
        else:
            mask, value = (0xff000000, 0x07000000) if multiple else (0xffffffff, token)
            removed = k.fn('mask_out', B, P, U, U)(k.ctx, mask, value)
        # Neither pin removal call sets m_modified. An unconstrained train
        # marks it without changing any bigram, making save observable.
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'wo') == 2
        assert k.fn('guess_sentence', B, P)(k.inst)
        trained = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
                   else k.fn('train', B, P)(k.inst))
        saved = k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        stored = [value.hex() for value in d18_bigram_db(k, path, read_keys=keys)]
        assert k.ctx, 'reopen after removal failed'
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        reopened = [value.hex() for value in d18_bigram_db(k, path, read_keys=keys)]
        return dict(removed=removed, trained=trained, saved=saved, stored=stored, reopened=reopened)
    return probe


for _mode, _operation in (('pinyin', 'mask-out'), ('pinyin', 'remove-candidate'),
                         ('zhuyin', 'mask-out')):
    for _label, _total, _survivors, _removed in (
            ('reported', 1, (0xffffffff, 1), 1),
            ('underflow', 0, (2, 3), 1),
            ('independent-total', 17, (2, 3), 4),
            ('remove-zero', 17, (2, 3), 0),
            ('zero-survivors', 1, (0, 0), 1),
            ('wrap-after', 1, (2, 3), 0xffffffff),
            ('ordinary', 9, (2, 3), 4)):
        case(f'removal-bigram-{_label}-{_operation}-{_mode}', mode=_mode,
             control=_label == 'ordinary', issues=('740',))(
            removal_bigram(_total, _survivors, _removed, _operation,
                          unchanged_total=2 if _label == 'ordinary' else 17))
    if _operation == 'mask-out':
        case(f'removal-bigram-multiple-{_mode}', mode=_mode, issues=('740',))(
            removal_bigram(1, (0xffffffff, 0), 1, _operation, multiple=True))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
