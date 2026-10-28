"""Predicted-candidate type selection cases."""
import ctypes as C

from contract_abi import B, I, P, S, U


def choose_predicted_of(k, inst, want_kind, skip=0):
    """Choose a matching kind from the existing candidate list and return its boolean result.

    Skip the first skip matching rows; return None if no eligible row remains.
    A negative skip keeps skipping all matches. This helper does not regenerate
    the list or check accessor flags; choosing can update learned state."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for i in range(count.value):
        cand, kind = P(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        if kind.value == want_kind:
            if skip:
                skip -= 1
                continue
            return k.fn('choose_predicted_candidate', B, P, P)(inst, cand)
    return None


def register(case, *, predicted_rows):
    """Register the case with the shared prediction reader."""
    @case('choose-predicted-types', issues=('542', '540'))
    def _(k):
        """Record predictions for 我, then select punctuation twice and a prefix row.

        Keep the initial rows/count and each selection result. All selections
        reuse the cached instance's generated candidate list, with kinds 8 and 5."""
        # The return table: a punctuation row has no frequency and answers true,
        # a prefix row answers true. (The prefix row's bigram is not compared:
        # the pin trains its unigram only.)
        out = {}
        inst = k.inst
        out['before'] = predicted_rows(k, inst, '我')
        out['punctuation'] = choose_predicted_of(k, inst, 8)
        out['punctuation again'] = choose_predicted_of(k, inst, 8)
        out['prefix'] = choose_predicted_of(k, inst, 5)
        return out
