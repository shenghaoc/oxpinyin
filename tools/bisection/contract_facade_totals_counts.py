"""Facade totals and counts cases (inventory row 48, #1175).
"""
from pathlib import Path

from contract_abi import B, I, P, S, U, Z, final_step_sentences, row_of, tokens_of, unigram_of
from contract_case_helpers import (
    FACADE_TOTAL, _chunk, _facade_total_out, _guess_out, _segment_out, _stored_library_total, guess_rows)
from contract_fixtures import _write_user_conf, import_rows


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #798: the n-best unigram denominator is the facade's guint32 total
# (074a2219 phrase_index.h:440, returned at :615-617), read at
# phonetic_lookup.h:651 (unigram arm) and :679 (blended arm) and at
# phrase_lookup.cpp:315 and :334. Importing a new user phrase adds the item
# with frequency 0 and then add_unigram_frequency(count * 3)
# (pinyin.cpp:598-605), which adds to the facade total with no overflow check
# before the sub-index checks its own (phrase_index.h:628-634;
# phrase_index.cpp:169-175). `total` is the facade total the import leaves,
# before the guint32 wraps it; the user library's own total stays under 2^32.
def import_facade_total(total):
    """Build an import-and-guess case whose import count is derived from total minus FACADE_TOTAL."""
    def probe(k):
        """Import 妈 at the derived count and record ni3'hao3 guesses and 你好 segmentation.

        Require the difference to be divisible by three, three times the count to
        fit below 2**32 and count to be below 2**31. These assertions impose no
        lower bound; leave the modified context live."""
        count, rest = divmod(total - FACADE_TOTAL, 3)
        assert rest == 0 and 3 * count <= 0xFFFFFFFF and count < 2 ** 31, total
        out = {'count': count, 'added': import_rows(k, 7, [('妈', 'ma1', count)])}
        out.update(_guess_out(k, b"ni3'hao3"))
        out['segment'] = _segment_out(k, ('你好',))
        return out
    return probe


# The count #798 names: the total wraps to 51050831. The pin's third n-best
# row is 你好 again; a total that did not wrap ranks 拟好 there.
case('import-wrapped-facade-total-guess', issues=('798',))(import_facade_total(2 ** 32 + 51050831))

# A total that wraps to 2000000 also moves the phrase segment: the pin splits
# 你好 into 你 and 好, a total that did not wrap keeps the phrase.
case('import-wrapped-facade-total-segment', issues=('798',))(import_facade_total(2 ** 32 + 2000000))

# Just under the boundary: the total is 2^32 - 1 and nothing wraps.
case('import-facade-total-below-wrap', control=True, issues=('798',))(import_facade_total(2 ** 32 - 1))


# A library's total is the one its file stores (074a2219 phrase_index.cpp:345),
# and the facade adds it whole at load (:237-248). user.bin's header is set
# here past the sum of its items so the facade wraps to 2000000 on reopen.
# The save after the next import writes the items' sum again: pinyin_save
# compacts first (pinyin.cpp:1139), which rebuilds each library's total from
# its items (phrase_index.cpp:648-675) and leaves the facade's alone.
def stored_header_total(header):
    """Build a save/reopen case that replaces the user library's stored total with header."""
    def probe(k):
        """Save an imported 你好, overwrite its chunk total, reopen and record guesses and a second save.

        Encode header as an unsigned 32-bit little-endian value and rebuild chunk
        framing. Record the original total, segmentation, a subsequent 好你 import
        and the resaved header. Reset cached pointers before editing the file;
        leave the reopened context live."""
        out = {'added': import_rows(k, 7, [('你好', "ni3'hao3", 5)])}
        out['saved'] = k.fn('save', B, P)(k.ctx)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        path = Path(k.user, 'user.bin')
        payload = bytearray(path.read_bytes()[8:])
        out['stored'] = int.from_bytes(payload[:4], 'little')
        payload[:4] = header.to_bytes(4, 'little')
        path.write_bytes(_chunk(bytes(payload)))
        _write_user_conf(k)
        out.update(_guess_out(k, b"ni3'hao3"))
        out['segment'] = _segment_out(k, ('你好',))
        out['import'] = import_rows(k, 7, [('好你', "hao3'ni3", 2)])
        out['resaved'] = k.fn('save', B, P)(k.ctx)
        out['header'] = int.from_bytes(Path(k.user, 'user.bin').read_bytes()[8:12], 'little')
        return out
    return probe


case('stored-header-facade-total')(stored_header_total(2 ** 32 + 2000000 - FACADE_TOTAL))

# A header a little off the items' sum moves nothing visible.
case('stored-header-facade-total-small', control=True)(stored_header_total(7))


# A phrase imported into a system library is saved as an ADD record of
# gb_char.dbin, after a header whose new total already counts it (diff,
# phrase_index.cpp:392-399). On reopen the merge sets the total from that
# header and then adds the item again (add_phrase_item, :453-459), so the
# facade counts its unigram twice. The count makes the doubled total wrap
# to 2000003.
def system_add_reload(count):
    """Build an import/save/reopen observation for 龘龘 in system library one at the supplied count."""
    def probe(k):
        """Import and save the captured system phrase, then reopen lazily for guesses and segmentation.

        Record flags without asserting success, finalize/reset cached pointers
        after saving and leave the reopened context live."""
        out = {'added': import_rows(k, 1, [('龘龘', "da3'da3", count)])}
        out['saved'] = k.fn('save', B, P)(k.ctx)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        out.update(_guess_out(k, b"ni3'hao3"))
        out['segment'] = _segment_out(k, ('你好',))
        return out
    return probe


case('system-add-reload-facade-total')(system_add_reload((2 ** 32 + 2000003 - FACADE_TOTAL) // 6))

# The same flow with a count too small to move the ranking.
case('system-add-reload-facade-total-small', control=True)(system_add_reload(5))


# #801 review. add_unigram_frequency raises the item's own unigram with the
# facade total (074a2219 phrase_index.h:628-634, phrase_index.cpp:150-178), and
# the n-best step reads both from the live item (phonetic_lookup.h:651, :679).


def bumped_item_guess(k):
    """Add five million to library one's first 号 token and record unigram and ranking observations.

    Require a matching token through list indexing; record the add flag and
    leave the modified context live without saving."""
    token = [t for t in tokens_of(k, '号') if t >> 24 == 1][0]
    out = {'before': unigram_of(k, token)}
    out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 5000000)
    out['after'] = unigram_of(k, token)
    out.update(_facade_total_out(k))
    return out


case('token-add-unigram-numerator-guess', issues=('801',))(bumped_item_guess)


# Choosing a LONGER row trains its token through the same add, 69 * 7 = 483
# each time (pinyin.cpp:2521-2530). Two thousand choices of 你们 for `ni`
# make the pin read 跟你们 as 根你们.
def longer_choices_guess(k):
    """Choose a LONGER row for ni up to two thousand times, then observe guesses and segmentation.

    Stop early if no row exists or choose does not return one. Allocate separate
    instances for gennimen and nimen, recording sentence ranks and up to six
    candidates. Discard the cached instance pointer before segmentation without
    freeing it or the separately allocated instances."""
    inst = k.inst
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
    chosen = 0
    for _ in range(2000):
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        cand = row_of(k, inst, 7)
        if not cand or k.fn('choose_candidate', I, P, Z, P)(inst, 0, cand) != 1:
            break
        chosen += 1
    out = {'chosen': chosen}
    for text in (b'gennimen', b'nimen'):
        fresh = k.alloc()
        k.fn('parse_more_full_pinyins', Z, P, S)(fresh, text)
        k.fn('guess_sentence', B, P)(fresh)
        out[text.decode()] = [final_step_sentences(k, fresh),
                              guess_rows(k, fresh, 0, 0x1e)['rows'][:6]]
    k._inst = None
    out['segment'] = _segment_out(k, ('跟你们',))
    return out


case('longer-choices-numerator-guess', issues=('801',))(longer_choices_guess)


# Unloading a library takes its sub-index's whole total out of the facade,
# accepted bumps included, and loading it maps the stored items again
# (phrase_index.cpp:261-269, :237-248). The bump on a GBK token wraps the
# facade total to 2000000 while the library is loaded.
def unload_accepted_bump(k):
    """Bump a library-two token, then compare ranking before unload and after reload.

    Derive the unsigned increment from FACADE_TOTAL to target a wrapped total
    of two million. Record operation flags and token frequencies; discard cached
    instance pointers between library operations without freeing them. Leave
    the reloaded context live and do not save."""
    token = 0x02000001
    out = {'gbk': _stored_library_total(k, 'gbk_char.bin'), 'before': unigram_of(k, token)}
    delta = (2 ** 32 + 2000000 - FACADE_TOTAL) % 2 ** 32
    out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, delta)
    out['loaded'] = _facade_total_out(k)
    out['unload'] = k.fn('unload_phrase_library', B, P, U)(k.ctx, 2)
    k._inst = None
    out['unloaded'] = _facade_total_out(k)
    out['load'] = k.fn('load_phrase_library', B, P, U)(k.ctx, 2)
    k._inst = None
    out['reloaded'] = _facade_total_out(k)
    out['after'] = unigram_of(k, token)
    return out


case('unload-drops-accepted-bump', issues=('801',))(unload_accepted_bump)


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
