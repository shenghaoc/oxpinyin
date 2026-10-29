"""Gram lookup cases (inventory row 62, #1175).
"""
import ctypes as C
from pathlib import Path

from contract_abi import B, P, S, U, Z, tokens_of
from contract_case_helpers import (
    _DALONG, _PAST_RANGES, _chunk, _dynamic_adjust_out, _gram_bytes, _gram_items, _guess_out, _plant_user_grams,
    _replace_system_start_row, _replace_user_bigram, _segment_out, d18_bigram_db, guess_rows)
from contract_fixtures import _write_user_conf, private_system


def register(case, *, _craft_db):
    """Register these cases at their original driver position."""
    # --------------------------------------------------------------------------
    # #757 PR 3, the lookup side. The system and user grams merge as the pin's
    # merge_single_gram builds them (074a2219 ngram.cpp:277-352: both absent,
    # no row; one absent, its bytes; both present, the two-pointer interleave),
    # and the lookups read the merged chunk as the pin does: the n-best step
    # with SingleGram::search per range, one step per yielded item
    # (phonetic_lookup.h:594-632, ngram.cpp:153-176); DYNAMIC_ADJUST and the
    # phrase segmenter with get_freq from lower_bound (pinyin.cpp:1845-1852,
    # phrase_lookup.cpp:268-290, ngram.cpp:231-250). Nothing is sorted,
    # deduplicated or filtered on a count. The grams sit under the sentence
    # start, the predecessor of the first phrase in all three lookups.
    # --------------------------------------------------------------------------
    _BOOST = 1 << 31


    def _start_gram(kind, token):
        """The sentence start's planted gram. 'unsorted': [past-every-range,
        token], so lower_bound from any range start lands on the first item and
        the walk stops there; the boosted `token` is never met. 'duplicate':
        `token` twice, whose counts wrap to 1 when summed, so only a reader that
        keeps both items sees the boost."""
        if kind == 'unsorted':
            return _gram_bytes(_BOOST + 1, [(_PAST_RANGES, 1), (token, _BOOST)])
        return _gram_bytes(_BOOST, [(token, _BOOST), (token, (1 << 32) - _BOOST + 1)])


    def _system_start_tokens(k):
        """Read stored successor tokens from the system bigram's sentence-start row.

        Decode the row without sorting or deduplicating its items.
        """
        raw = d18_bigram_db(k, Path(k.data, 'bigram.db'), read_keys=(b'\x01\x00\x00\x00',))[0]
        return [token for token, _ in _gram_items(raw)[1]]


    def _pin_lower_bound(at, length, token):
        """Return the index reached by libstdc++'s halving lower-bound search.

        Read values through at(index) over [0, length), preserving the search path
        even for unsorted input. This models the std::lower_bound call used by
        SingleGram::get_freq at 074a2219 ngram.cpp:239, exposed through stl_lite.h:29;
        it does not sort or validate the sequence.
        """
        # libstdc++'s __lower_bound, as SingleGram's std_lite::lower_bound runs.
        first = 0
        while length > 0:
            half = length >> 1
            if at(first + half) < token:
                first, length = first + half + 1, length - half - 1
            else:
                length = half
        return first


    def _interleave_miss_layout(system, token):
        """A user gram [token, z, a] (a < token < z) that get_freq finds on its
        own but misses once merged with the sorted system gram `system`: the
        merge keeps the user order and places `a` after `z`, out of order, and
        `z` is picked so that the merged lower_bound probes `a`, steps past it
        and lands beyond `token` (ngram.cpp:308-349, :231-250)."""
        assert system == sorted(system), 'the installed start gram is sorted'
        present, below = set(system), sum(1 for item in system if item < token)
        tied = int(token in present)
        a = next(t for t in range(token - 1, 0, -1) if t not in present)
        gaps = [item + 1 for item in system[below:] if item + 1 not in present and item + 1 > token]
        if token + 1 not in present:
            gaps.insert(0, token + 1)
        for z in gaps:
            before_z = sum(1 for item in system if item < z)
            merged = (system[:below] + [token] + system[below + tied:before_z] + [z, a]
                      + system[before_z:])
            start = _pin_lower_bound(lambda i: merged[i], len(merged), token)
            walk = next((t for t in merged[start:] if t >= token), None)
            if walk != token:
                return [(token, _BOOST), (z, 1), (a, 1)]
        raise AssertionError('no interleave misses the token')


    # 尼 is a ni reading the n-best and the candidate list rank below 你; the
    # boost would put it first for a reader that met it. A boosted 今天 splits
    # 今天天气 off the start of 今天天气很好.
    def user_gram_lookup(kind, operation):
        """Build a lookup case using an unsorted or duplicate user sentence-start gram.

        operation selects sentence guessing, dynamic adjustment or segmentation.
        The fixture distinguishes stored-order merging in merge_single_gram at
        074a2219 ngram.cpp:308-349 from sorting or combining duplicates first.
        """
        def probe(k):
            """Replace the user bigram and perform the selected lookup with a boosted token.

            Use an interleave-miss layout for unsorted segmentation; otherwise plant the
            standard unsorted/duplicate layout and return the operation's observations.
            """
            boosted = tokens_of(k, '今天' if operation == 'phrase-segment' else '尼')[0]
            if operation == 'phrase-segment' and kind == 'unsorted':
                # The phrase segmenter reads each pair, so a gram get_freq
                # misses on its own reads the same merged or not; only the
                # merge's interleave tells the two apart.
                items = _interleave_miss_layout(_system_start_tokens(k), boosted)
                value = _gram_bytes(_BOOST + 2, items)
            else:
                value = _start_gram(kind, boosted)
            _replace_user_bigram(k, [(1, value)])
            if operation == 'guess':
                return _guess_out(k)
            if operation == 'dynamic-adjust':
                return _dynamic_adjust_out(k)
            return _segment_out(k, ('今天天气很好',))
        return probe


    for _kind in ('unsorted', 'duplicate'):
        for _operation in ('guess', 'dynamic-adjust', 'phrase-segment'):
            # A duplicated user gram reads the same per pair as merged: the
            # merge sums the first duplicate with the system item and appends
            # the second after it, so get_freq meets the same first match.
            control = (_kind, _operation) == ('duplicate', 'phrase-segment')
            policy = ({'policy_record': '1297-gram-partition-segmentation.md'}
                      if (_kind, _operation) == ('unsorted', 'phrase-segment') else {})
            case(f'user-gram-{_kind}-{_operation}', control=control, issues=('757',), **policy)(
                user_gram_lookup(_kind, _operation))


    # PR 0's ranges and SingleGram::search's one-sided bound (ngram.cpp:161-173):
    # the walk stops at the first token at or past the range end, and nothing
    # checks the range start. The user gram [你, T, T] for a ni reading T below
    # 你 merges to [..., 你, T, T, ...] (ngram.cpp:308-349), so the walk over
    # 你's range yields both T items, below that range, and the pin steps T
    # with each (phonetic_lookup.h:625-629). The two T counts wrap to 1 when
    # summed, so a reader that sorts and sums the gram does not boost T.
    @case('user-gram-unsorted-range-boundary', issues=('757',))
    def _(k):
        """Plant a user gram whose merged range yields duplicated lower tokens.

        Choose a ni reading below the token for 你 and observe sentence guessing.
        SingleGram::search checks only the upper bound while walking at
        074a2219 ngram.cpp:167-173.
        """
        ni = tokens_of(k, '你')[0]
        below = next(token for token in (tokens_of(k, text)[0] for text in '泥拟逆腻妮倪匿溺尼')
                     if token + 1 < ni)
        items = [(ni, 1), (below, _BOOST), (below, (1 << 32) - _BOOST + 1)]
        _replace_user_bigram(k, [(1, _gram_bytes(_BOOST, items))])
        return _guess_out(k)


    # The system gram enters the merge as stored, with no user gram beside it.
    # 'duplicate' holds 尼 at 1 and then at the boost: search yields both items,
    # so the boosted one steps; a reader that keeps the first match does not.
    def system_gram_lookup(kind):
        """Build a sentence-guess case with an unsorted or duplicate system start row.

        Replace the row only in a private system-table copy; no user gram is planted.
        """
        def probe(k):
            """Boost 尼 in a copied system gram and return the nihao sentence observations."""
            ni2 = tokens_of(k, '尼')[0]
            value = (_start_gram(kind, ni2) if kind == 'unsorted'
                     else _gram_bytes(_BOOST + 1, [(ni2, 1), (ni2, _BOOST)]))
            _replace_system_start_row(k, value)
            return _guess_out(k)
        return probe


    for _kind in ('unsorted', 'duplicate'):
        case(f'system-gram-{_kind}-guess', issues=('757',))(system_gram_lookup(_kind))


    # A stored zero count is an item. The n-best step takes a bigram step for
    # each item `search` yields (phonetic_lookup.h:625-629) beside the unigram
    # pass's step from the best predecessor, so a one-syllable best sentence
    # the start's merged gram holds is answered twice and one it lacks once.
    # The start's system gram is replaced by one item past every range, and
    # its user gram holds the best sentence's phrase at count 0: the pin's
    # merged row holds that item, so the sentence comes twice. A reader that
    # drops zero counts answers it once.
    @case('user-gram-zero-count-guess', issues=('757',))
    def _(k):
        """Plant a zero-count user item for the best one-syllable sentence and guess again.

        First replace the system start row with an out-of-range token. The zero item
        remains visible to SingleGram::search at 074a2219 ngram.cpp:167-173.
        """
        _replace_system_start_row(k, _gram_bytes(1, [(_PAST_RANGES, 1)]))
        best = _guess_out(k, b'a')['sentences'][0]
        _replace_user_bigram(k, [(1, _gram_bytes(0, [(tokens_of(k, best)[0], 0)]))])
        return _guess_out(k, b'a')


    # N2, register row 85: a user gram shorter than its guint32 total. Beside a
    # system gram the merge reads it (ngram.cpp:304, through phonetic_lookup.h:598,
    # pinyin.cpp:2214, zhuyin.cpp:1487, phrase_lookup.cpp:272) and the pin dies
    # at memory_chunk.h:390; alone, SingleGram::search reads it
    # (ngram.cpp:165, through phonetic_lookup.h:625). The sentence start has a
    # system gram; a user phrase used as a sentence prefix has none.
    _SHORT = b'\x07\x00\x00'


    def short_start_guess(k):
        """Guess with a three-byte user start gram and return only the guess status."""
        _replace_user_bigram(k, [(1, _SHORT)])
        return {'ret': _guess_out(k)['guessed']}


    for _mode in ('pinyin', 'zhuyin'):
        case(f'abort-guess-sentence-short-user-gram-system-row-{_mode}', mode=_mode, abort=False, issues=('757',))(
            short_start_guess)


    @case('abort-guess-sentence-short-user-gram-no-system-row', abort=False, issues=('757',))
    def _(k):
        """Guess with an imported phrase prefix whose user gram lacks a complete total.

        Require nihao to parse completely, then call guess_sentence_with_prefix with
        the imported phrase; report its return value.
        """
        def grams(system, users):
            """Place the short gram under the first imported user token, ignoring system rows."""
            return [(users[0], _SHORT)]
        _plant_user_grams(k, [_DALONG], grams)
        inst = k.inst
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao') == 5
        return {'ret': k.fn('guess_sentence_with_prefix', B, P, S)(inst, _DALONG[0].encode())}


    def short_start_dynamic_adjust(k):
        """Request dynamically adjusted candidates with a three-byte user start gram."""
        _replace_user_bigram(k, [(1, _SHORT)])
        return {'ret': _dynamic_adjust_out(k)['ret']}


    for _mode in ('pinyin', 'zhuyin'):
        case(f'abort-guess-candidates-dynamic-adjust-short-user-gram-{_mode}', mode=_mode,
             abort=False, issues=('757',))(short_start_dynamic_adjust)


    # The same gram under DYNAMIC_ADJUST, parsed and never guessed: the pin's
    # parse merges no gram (pinyin.cpp:1497-1520, zhuyin.cpp:1017-1043), so the
    # parse answers its full length.
    def short_start_dynamic_adjust_parse(k):
        """Parse with dynamic adjustment enabled and a short start gram, without guessing.

        Return the parser's consumed byte count and the stored parsed-input length.
        """
        _replace_user_bigram(k, [(1, _SHORT)])
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | (1 << 9))
        inst = k.inst
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        return dict(parsed=parsed, length=k.fn('get_parsed_input_length', Z, P)(inst))


    for _mode in ('pinyin', 'zhuyin'):
        case(f'parse-dynamic-adjust-short-user-gram-{_mode}', mode=_mode, issues=('757',))(
            short_start_dynamic_adjust_parse)


    @case('abort-phrase-segment-short-user-gram', abort=False, issues=('757',))
    def _(k):
        """Segment a fixed phrase after replacing the user start gram with three bytes."""
        _replace_user_bigram(k, [(1, _SHORT)])
        return {'ret': k.fn('phrase_segment', B, P, S)(k.inst, '今天天气很好'.encode())}


    # #918: 074a2219 phrase_lookup.cpp:137-149 searches the scalar span and
    # reads its predecessor before the later surrogate prevents a full path.
    # ngram.cpp:303-304 reads the three-byte user's total beside the system row
    # and asserts at memory_chunk.h:390. The sound twin reaches final_step:
    # false, two zero tokens, no diagnostic. No whole-store preflight.
    def phrase_prefix_gram(short, *, before=False, unrelated=False):
        """Build a segmentation case varying where a short predecessor gram is reachable.

        short selects a truncated total; before moves the surrogate before the scalar
        span, and unrelated places the gram under another predecessor. The reachable
        prefix search is PhraseLookup::get_best_match at
        074a2219 phrase_lookup.cpp:135-146.
        """
        def probe(k):
            """Seed an old segmentation result and check the selected surrogate/gram case.

            Assert either the exact short-gram diagnostic and report old-result retention,
            or two null tokens and no diagnostics. The returned status is the segmenter's
            actual result, not a predicted success value.
            """
            predecessor = tokens_of(k, '好')[0] if unrelated else 1
            value = b'\x01\x00\x00' if short else _gram_bytes(1, [])
            _replace_user_bigram(k, [(predecessor, value)])
            inst = k.inst

            def result():
                """Read all phrase tokens and require the first out-of-range slot to become zero."""
                count = U(0xABCDEF)
                assert k.fn('get_n_phrase', B, P, C.POINTER(U))(inst, C.byref(count))
                tokens = []
                for index in range(count.value):
                    token = U(0xABCDEF)
                    assert k.fn('get_phrase_token', B, P, U, C.POINTER(U))(
                        inst, index, C.byref(token))
                    tokens.append(token.value)
                token = U(0xABCDEF)
                assert not k.fn('get_phrase_token', B, P, U, C.POINTER(U))(
                    inst, count.value, C.byref(token))
                assert token.value == 0
                return tokens

            # No scalar span is reachable: seed a distinguishable old result
            # without reading the corrupt start gram on either implementation.
            assert not k.fn('phrase_segment', B, P, S)(inst, b'\xed\xa0\x80' * 3)
            old = result()
            assert old == [0, 0, 0]
            surrogate = b'\xed\xa0\x80'
            text = surrogate + '你'.encode() if before else '你'.encode() + surrogate
            ret = k.fn('phrase_segment', B, P, S)(inst, text)
            tokens = result()
            if short and not before and not unrelated:
                assert k.log_messages == [
                    f'{k.mode}_phrase_segment: a user bigram value is shorter than a guint32 '
                    'total_freq (upstream aborts, memory_chunk.h:390)'
                ]
                return dict(ret=ret, untouched_phrase_result=tokens == old)
            assert tokens == [0, 0]
            assert not k.log_messages
            return dict(ret=ret, tokens=tokens)
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        case(f'abort-phrase-segment-prefix-short-user-gram-{_mode}', mode=_mode,
             abort=False, issues=('918',))(phrase_prefix_gram(True))
        case(f'phrase-segment-prefix-sound-user-gram-{_mode}', mode=_mode,
             control=True, stderr=True, issues=('918',))(phrase_prefix_gram(False))
        case(f'phrase-segment-nonscalar-before-short-user-gram-{_mode}', mode=_mode,
             control=True, stderr=True, issues=('918',))(phrase_prefix_gram(True, before=True))
        case(f'phrase-segment-prefix-unrelated-short-user-gram-{_mode}', mode=_mode,
             control=True, stderr=True, issues=('918',))(phrase_prefix_gram(True, unrelated=True))


    # #918 ruling: the phrase table compares the raw UCS-4 words the pin's
    # converter produces. A table key `[0xd800]` and a bounded one-character
    # item make `phrase_segment("\xed\xa0\x80")` return true with that token;
    # no `char` or `str` exists for the word on either side.
    def phrase_segment_nonscalar_raw_key(k):
        """Install a private raw-surrogate phrase key and observe segmentation of it.

        Replace private library/index links with a one-character item and raw UCS-4
        key 0xd800, seed an earlier failed result, then return status and tokens for
        the matching UTF-8 byte sequence. No Python Unicode scalar represents the key.
        """
        system = private_system(k)
        word = 0xd800
        token = 0x01000001
        raw_text = word.to_bytes(4, 'little')
        reading = (0x1084).to_bytes(2, 'little')
        content = bytearray(8)
        offsets = [0, 8]
        content += bytes((1, 1)) + (100).to_bytes(4, 'little') + raw_text
        content += reading + (10).to_bytes(4, 'little')
        index_two = 17 + len(offsets) * 4 + 1
        payload = b''.join(value.to_bytes(4, 'little') for value in
                           (200, 17, index_two, index_two + len(content) + 1))
        payload += b'#' + b''.join(value.to_bytes(4, 'little') for value in offsets)
        payload += b'#' + content + b'#'
        path = Path(system, 'gb_char.bin')
        path.unlink()
        path.write_bytes(_chunk(payload))
        path = Path(system, 'phrase_index.bin')
        path.unlink()
        _craft_db(str(path), [(raw_text, token.to_bytes(4, 'little'))], 1,
                  system_index=True)
        _write_user_conf(k)
        k.data = system
        inst = k.inst
        assert not k.fn('phrase_segment', B, P, S)(inst, b'\xed\xbf\xbf' * 3)
        ret = k.fn('phrase_segment', B, P, S)(inst, b'\xed\xa0\x80')
        count = U(0xABCDEF)
        assert k.fn('get_n_phrase', B, P, C.POINTER(U))(inst, C.byref(count))
        tokens = []
        for index in range(count.value):
            value = U(0xABCDEF)
            assert k.fn('get_phrase_token', B, P, U, C.POINTER(U))(
                inst, index, C.byref(value))
            tokens.append(value.value)
        return dict(ret=ret, tokens=tokens)


    for _mode in ('pinyin', 'zhuyin'):
        case(f'phrase-segment-nonscalar-raw-key-{_mode}', mode=_mode,
             stderr=True, issues=('918',))(phrase_segment_nonscalar_raw_key)


    # Controls: a short gram the lookup never loads. 我 never precedes a phrase
    # of `nihao`; with DYNAMIC_ADJUST clear the candidate list merges nothing
    # (pinyin.cpp:2207-2216).
    @case('guess-unrelated-short-user-gram', control=True, issues=('757',))
    def _(k):
        """Guess nihao with a short user gram attached to the unrelated token for 我."""
        _replace_user_bigram(k, [(tokens_of(k, '我')[0], _SHORT)])
        return _guess_out(k)


    @case('guess-candidates-short-user-gram-dynamic-adjust-off', control=True, issues=('757',))
    def _(k):
        """Guess candidates with a short start gram while dynamic adjustment is disabled.

        Require option word 0x18a to succeed and return the parse length and rows.
        """
        _replace_user_bigram(k, [(1, _SHORT)])
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a)
        inst = k.inst
        parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        return dict(parsed=parsed, **guess_rows(k, inst, 0, 0x1e))
