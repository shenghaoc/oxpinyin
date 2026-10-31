"""Zero-facade-total candidate cases."""
import ctypes as C

from contract_abi import B, P, S, U, Z


def register(case, *, wrap_total_to_zero):
    """Register the cases at their original position in the driver."""
    # The zero total aborts the pin only while a searched row is ranked
    # (`_compute_frequency_of_items`, `pinyin.cpp:1859`): a window with no phrase
    # row — the reserved slot, a mid-key offset — answers true with no warning.
    # One case each, since the wrapped total is the context's.
    for _label, _offset, _sort in (('end-sort-3', 5, 3), ('mid-key-sort-3', 3, 3), ('end-sort-2', 5, 2)):
        def _no_rank(offset, sort):
            """Capture the byte offset and sort options for a zero-total candidate case."""
            def run(k):
                """Attempt candidate guessing after parsing nihao and applying the total-wrap delta.

                Allocate a fresh instance, ignore the add flag, and return the guess flag
                and candidate count for the captured offset and sort. The context's
                frequency state is changed and the instance is not freed here."""
                inst = k.alloc()
                k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
                wrap_total_to_zero(k, inst)
                count = U()
                ret = k.fn('guess_candidates', B, P, Z, U)(inst, offset, sort)
                k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
                return {'ret': ret, 'rows': count.value}
            return run
        case('facade-total-zero-ranks-nothing-' + _label, control=True, issues=('540',))(_no_rank(_offset, _sort))


    @case('facade-total-zero', abort=False, sigabrt=True, issues=('540',))
    def _(k):
        """Apply the total-wrap delta and record candidate guessing at byte offset zero.

        Return the add and guess flags if the native calls return. The selected
        window reaches _compute_frequency_of_items in src/pinyin.cpp at pin
        074a2219, where a zero total asserts; the registered comparison expects
        SIGABRT from the pin and a false guess result from the subject."""
        # `assert(0 < total_freq)` while ranking a candidate (pinyin.cpp:1859).
        inst = k.inst
        added = wrap_total_to_zero(k, inst)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        return {'add': added, 'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1E)}
