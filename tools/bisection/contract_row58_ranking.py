"""Row 58 ranking cases (inventory row 57, #1175).
"""
import ctypes as C
from pathlib import Path
import re

from contract_abi import B, I, P, S, U, Z, choose_text, tokens_of, unigram_of
from contract_case_helpers import FACADE_TOTAL, row58_rows, wrap_total_to_zero
from contract_fixtures import private_system, row58_import


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def row58_seed_bigrams(k):
    """Seed competing Beijing and Shanghai prediction statistics and return their unigram outputs.

    Train constrained 我北京 once and 我上海 three times with dynamic options,
    reset after each phrase, then add 100000 to Beijing's unigram. Assert parses,
    guesses, choices, training, resets and the frequency update; leave live state changed."""
    # Constrain valid n-best results before training, following the existing
    # training cases. Shanghai has more bigram observations; independently
    # boost Beijing's unigram so those two proposed sort keys disagree.
    k.fn('set_options', B, P, U)(k.ctx, 0x18a | (1 << 9))
    for text, phrase, repeats in ((b'wobeijing', '北京', 1), (b'woshanghai', '上海', 3)):
        inst = k.inst
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, text) == len(text)
        assert k.fn('guess_sentence', B, P)(inst)
        offset = choose_text(k, inst, 0, '我')
        assert offset == 2
        assert choose_text(k, inst, offset, phrase) == len(text)
        assert k.fn('guess_sentence', B, P)(inst)
        for _ in range(repeats):
            assert k.fn('train', B, P, C.c_ubyte)(inst, 0)
        assert k.fn('reset', B, P)(inst)
    beijing, shanghai = tokens_of(k, '北京')[0], tokens_of(k, '上海')[0]
    assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, beijing, 100000)
    return {'北京': unigram_of(k, beijing), '上海': unigram_of(k, shanghai)}


def row58_scoring(dynamic):
    """Build a prediction-order case with seeded competing counts and the chosen dynamic option."""
    def probe(k):
        """Seed bigrams, set the dynamic option and return unigrams plus predictions after 我."""
        counts = row58_seed_bigrams(k)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        return dict(unigrams=counts, prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-bigram-scoring-' + ('on' if _dynamic else 'off'), issues=('697', 'row:58'))(row58_scoring(_dynamic))


def row58_choose(k, kind, text, times):
    """Repeatedly choose the first current predicted row matching kind and text.

    Make times selections. Assert getters, reuse the borrowed row pointer
    without regenerating the list, and return each selection flag.
    Raise AssertionError if the row is absent;
    nonpositive times returns an empty list once a matching row is found."""
    n = U()
    assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(n))
    for i in range(n.value):
        cand, found_kind, found_text = P(), I(), S()
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, i, C.byref(cand))
        assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(k.inst, cand, C.byref(found_kind))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(k.inst, cand, C.byref(found_text))
        if found_kind.value == kind and found_text.value.decode() == text:
            return [k.fn('choose_predicted_candidate', B, P, P)(k.inst, cand) for _ in range(times)]
    raise AssertionError('missing predicted row: %s %s' % (kind, text))


def row58_prefix(dynamic, text, times):
    """Build a repeated PREFIX-row choice case with before/after counts and predictions.

    text is the suffix after 我; times controls consecutive choices and dynamic
    sets the prediction option."""
    def probe(k):
        """Select the requested type-5 suffix repeatedly and snapshot its full-phrase unigram.

        Look up 我 plus text before predicting; return before/after lists and all
        selection flags without saving."""
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        token = tokens_of(k, '我' + text)[0]
        before = row58_rows(k, '我')
        count_before = unigram_of(k, token)
        chosen = row58_choose(k, 5, text, times)
        return dict(before=before, count_before=count_before, chosen=chosen,
                    count_after=unigram_of(k, token), after=row58_rows(k, '我'))
    return probe


def row58_bigram_choices(dynamic):
    """Build a live BIGRAM-row ranking case with successive batches of Shanghai selections."""
    def probe(k):
        """Seed competing counts and select 上海 in batches of one, twenty and two hundred.

        Set the requested dynamic option and snapshot predictions after each batch;
        selections accumulate rather than restarting from the seed."""
        counts = row58_seed_bigrams(k)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        out = dict(unigrams=counts, before=row58_rows(k, '我'))
        # Fixed observation points, not a ranking threshold: each acceptance
        # contributes 483 unigram and 69 bigram independently of options.
        for times in (1, 20, 200):
            out['chosen-%d' % times] = row58_choose(k, 4, '上海', times)
            out['rows-%d' % times] = row58_rows(k, '我')
        return out
    return probe


def row58_conversion(dynamic):
    """Build prediction fixtures around score ties and integer-to-float count boundaries."""
    def probe(k):
        """Import equal-count 乙乙 before 甲甲, then compare tie and large-count prediction lists.

        Use import counts 33/33/1/2, followed by 5592405/5592406, and report the last
        two full-phrase unigrams. Apply the requested dynamic option without saving."""
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        # Unigram = import count * 3. Counts 3/6 quantize to the same
        # amplified integer; 99 ties a baked 100 after amplification.
        # Insert in reverse UCS-4 byte order to distinguish order from token.
        row58_import(k, [('我乙乙', "wo3'yi3'yi3", 33),
                         ('我甲甲', "wo3'jia3'jia3", 33),
                         ('我丙丙', "wo3'bing3'bing3", 1),
                         ('我丁丁', "wo3'ding1'ding1", 2)])
        out = dict(ties=row58_rows(k, '我'))
        row58_import(k, [('我戊戊', "wo3'wu4'wu4", 5592405),
                         ('我己己', "wo3'ji3'ji3", 5592406)])
        # 2^24-1 and 2^24+2: the integer-to-float representability edge.
        out['float-edge'] = row58_rows(k, '我')
        out['unigrams'] = {text: unigram_of(k, tokens_of(k, text)[0])
                           for text in ('我戊戊', '我己己')}
        return out
    return probe


def row58_lambda(dynamic):
    """Build a prediction-order case with lambda one and a boosted competing phrase."""
    def probe(k):
        """Replace a private table.conf lambda with one, initialize and boost 我是谁 by 100000.

        Cache the new context, assert setup and return predictions after 我 under
        the requested dynamic option. Leave the private directory and context live."""
        system = private_system(k)
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        conf.unlink()
        conf.write_text(re.sub(r'lambda parameter:[^\n]+', 'lambda parameter:1', text))
        k._ctx = k.init(system=system)
        assert k._ctx
        # lambda=1 makes every predicted score zero. A nonuniform unigram
        # overlay must therefore leave only the source collection tie order.
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        token = tokens_of(k, '我是谁')[0]
        assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 100000)
        return dict(prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    _state = 'on' if _dynamic else 'off'
    case('row58-prefix-no-bigram-' + _state, issues=('697', 'row:58'))(row58_prefix(_dynamic, '是', 1))
    case('row58-prefix-live-21-' + _state, issues=('697', 'row:58'))(row58_prefix(_dynamic, '是谁', 21))
    case('row58-bigram-choices-' + _state, issues=('697', 'row:58'))(row58_bigram_choices(_dynamic))
    case('row58-conversions-' + _state, issues=('697', 'row:58'))(row58_conversion(_dynamic))
    case('row58-lambda-ties-' + _state, issues=('697', 'row:58'))(row58_lambda(_dynamic))


def row58_prefix_overflow(dynamic):
    """Build a PREFIX-choice case near the user library's 32-bit total limit."""
    def probe(k):
        """Import 我 and a high-count suffix phrase, choose it once and compare counts and rows.

        Use import counts 1 and 1431655631 with the requested dynamic option. Return
        selection flags and live observations without assuming the update succeeded."""
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        # Library 7 has only 399 left before guint32::MAX. The next 483
        # cannot fit, but the facade total still advances and wraps. An
        # explicit prefix keeps this refusal observable in the parent too;
        # the conversion cases separately cover implicit prefix index keys.
        row58_import(k, [('我', 'wo3', 1),
                         ('我甲甲', "wo3'jia3'jia3", 1431655631)])
        token = tokens_of(k, '我甲甲')[0]
        before = row58_rows(k, '我')
        count_before = unigram_of(k, token)
        chosen = row58_choose(k, 5, '甲甲', 1)
        return dict(before=before, count_before=count_before, chosen=chosen,
                    count_after=unigram_of(k, token), after=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-prefix-overflow-' + ('on' if _dynamic else 'off'), issues=('697', 'row:58'))(row58_prefix_overflow(_dynamic))


def row58_signed_score(dynamic, duplicate=False, denominator=20000000):
    """Build a large-score prediction case using a fixed-fixture total and requested denominator.

    duplicate selects the existing phrase 我的心 instead of 我甲甲. The callable
    uses a modulo-2**32 absent-token update derived from base total 51051831."""
    def probe(k):
        """Import the high-count phrase, attempt the compensating total update and collect predictions.

        Set the dynamic option and record the absent-token add flag. The delta
        assumes import count 1431655631 contributes three times that count to the
        fixed system total; it is not recalculated from live library files.
        The factor follows _add_phrase in src/pinyin.cpp at pin 074a2219."""
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        phrase = ('我的心', "wo3'de5'xin1") if duplicate else ('我甲甲', "wo3'jia3'jia3")
        row58_import(k, [(*phrase, 1431655631)])
        # The absent-token add advances only the facade total. Make the
        # denominator 20,000,000: score exceeds gint::MAX; at 10,000,000
        # it also exceeds guint32::MAX. Pin x86-64 converts via gint64
        # then retains the low 32 bits, rather than saturating.
        total = (51051831 + 1431655631 * 3) % (1 << 32)
        delta = (denominator - total) % (1 << 32)
        added = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, 0x01ffffff, delta)
        return dict(absent_add=added, prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    _state = 'on' if _dynamic else 'off'
    case('row58-signed-score-' + _state, issues=('697', 'row:58'))(row58_signed_score(_dynamic))
    case('row58-signed-duplicate-' + _state, issues=('697', 'row:58'))(row58_signed_score(_dynamic, True))
    case('row58-unsigned-score-' + _state, issues=('697', 'row:58'))(row58_signed_score(_dynamic, denominator=10000000))


def row58_zero_total(dynamic, punct=False):
    """Build a zero-total bigram prediction case, optionally including punctuation candidates."""
    def probe(k):
        """Seed bigrams, derive the three trained phrases' live delta and attempt to wrap the total.

        Use FACADE_TOTAL plus that delta and an absent-token update modulo 2**32.
        Record the selected prediction flag and whether its list is empty; assert
        that a false prediction has no rows. Do not return candidate contents."""
        tokens = {token for text in ('我', '北京', '上海') for token in tokens_of(k, text)}
        before = sum(unigram_of(k, token)[1] for token in tokens)
        row58_seed_bigrams(k)
        after = sum(unigram_of(k, token)[1] for token in tokens)
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        # Only these three phrases were trained/added: use their live delta,
        # including 我's reselection seed, rather than a guessed threshold.
        total = FACADE_TOTAL + after - before
        added = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, 0x01ffffff,
                                                            (-total) % (1 << 32))
        name = 'guess_predicted_candidates_with_punctuations' if punct else 'guess_predicted_candidates'
        ret = k.fn(name, B, P, S)(k.inst, '我'.encode())
        n = U()
        assert k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(n))
        if not ret:
            assert n.value == 0
        return dict(add=added, ret=ret, rows=[] if n.value == 0 else ['not empty'])
    return probe


for _dynamic in (False, True):
    for _punct in (False, True):
        name = 'row58-total-zero-' + ('punct-' if _punct else '') + ('on' if _dynamic else 'off')
        case(name, abort=False, issues=('697', 'row:58'))(row58_zero_total(_dynamic, _punct))


def row58_prefix_zero_total(dynamic):
    """Build a PREFIX prediction case after attempting to wrap the facade total to zero."""
    def probe(k):
        """Set the dynamic option, apply wrap_total_to_zero and return the complete prediction list."""
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        added = wrap_total_to_zero(k, k.inst)
        # Prefix rows bypass pinyin.cpp:1859. The pin's invalid conversion
        # has zero low bits; the complete list retains collection tie order.
        return dict(add=added, prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-prefix-zero-total-' + ('on' if _dynamic else 'off'), control=True, issues=('697', 'row:58'))(
        row58_prefix_zero_total(_dynamic))


def row58_lambda_rounding(dynamic):
    """Build a prediction case with a decimal lambda just below the binary32 midpoint beneath one."""
    def probe(k):
        """Write lambda 0.9999999701976776123036 into a private config and predict with a large user count.

        Cache and assert the new context, apply the dynamic option and import
        我甲甲 with count 1431655631. Leave the context and private files live."""
        system = private_system(k)
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        conf.unlink()
        # Just below the f32 midpoint between 1 and its predecessor. Direct
        # %f parsing yields the predecessor; a rational -> f64 -> f32 rounds
        # to 1 and erases every score. The large live user count exposes it.
        conf.write_text(re.sub(r'lambda parameter:[^\n]+',
                              'lambda parameter:0.9999999701976776123036', text))
        k._ctx = k.init(system=system)
        assert k._ctx
        assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        row58_import(k, [('我甲甲', "wo3'jia3'jia3", 1431655631)])
        return dict(prediction=row58_rows(k, '我'))
    return probe


for _dynamic in (False, True):
    case('row58-lambda-rounding-' + ('on' if _dynamic else 'off'), issues=('697', 'row:58'))(row58_lambda_rounding(_dynamic))


# #712 review 4232017129: an imported system-library item has a complete
# override count, whereas an immutable item still needs its store delta.
def row58_system_import(library, dynamic=False, existing=False):
    """Build a prediction/count case for an imported system-library phrase.

    existing selects 我是谁 and its first token's library, ignoring library;
    otherwise import 我甲甲 into library. dynamic affects pinyin only; zhuyin
    records import/count outputs without prediction calls."""
    def probe(k):
        """Import count 33 into a validated system library and observe live count changes.

        Assert the expected token presence and one resulting token. For a new
        pinyin phrase, adjust 我是谁 as a competitor; select the imported PREFIX
        row once then twice, snapshotting counts and complete lists after each
        batch. For zhuyin, stop after the import observations."""
        phrase = '我是谁' if existing else '我甲甲'
        prefix, suffix = '我', ('是谁' if existing else '甲甲')
        if k.mode == 'pinyin':
            reading = "wo3'shi4'shei2" if existing else "wo3'jia3'jia3"
            assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | ((1 << 9) if dynamic else 0))
        else:
            reading = 'ㄨㄛˇ ㄕˋ ㄕㄟˊ' if existing else 'ㄨㄛˇ ㄐㄧㄚˇ ㄐㄧㄚˇ'
        initial = tokens_of(k, phrase)
        target_library = (initial[0] >> 24) if existing else library
        assert 1 <= target_library <= 4
        before = [t for t in initial if t >> 24 == target_library]
        assert bool(before) == existing
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, target_library)
        assert it
        added = k.fn('iterator_add_phrase', B, P, S, S, I)(
            it, phrase.encode(), reading.encode(), 33)
        assert added
        k.fn('end_add_phrases', None, P)(it)
        tokens = [t for t in tokens_of(k, phrase) if t >> 24 == target_library]
        assert len(tokens) == 1
        token = tokens[0]
        out = dict(tokens_before=before, added=added, tokens_after=tokens,
                   imported_count=unigram_of(k, token))
        if k.mode == 'pinyin':
            if not existing:
                # Place an immutable competitor between the correct count
                # after three accepts (33*3 + 3*483) and its erroneous double.
                rival = tokens_of(k, '我是谁')[0]
                rival_count = (33 * 3 + 3 * 483) * 3 // 2
                delta = rival_count - unigram_of(k, rival)[1]
                assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, rival, delta)
                out['rival_count'] = unigram_of(k, rival)
            out['rows-0'] = row58_rows(k, prefix)
            # Three acceptances in two snapshots exercise the live override,
            # not only its initial import value. Preserve every candidate.
            for accepted in (1, 2):
                out['chosen-%d' % accepted] = row58_choose(k, 5, suffix, accepted)
                out['count-%d' % accepted] = unigram_of(k, token)
                out['rows-%d' % accepted] = row58_rows(k, prefix)
        return out
    return probe


for _library in range(1, 5):
    for _dynamic in (False, True):
        case('row58-system-import-%d-%s' % (_library, 'on' if _dynamic else 'off'), issues=('712', '697', 'row:58'))(
            row58_system_import(_library, _dynamic))
    # The import/count twin exists; libzhuyin has no predicted API twin.
    case('row58-system-import-zhuyin-%d' % _library, mode='zhuyin', control=True, issues=('712', '697', 'row:58'))(
        row58_system_import(_library))
for _dynamic in (False, True):
    case('row58-system-existing-' + ('on' if _dynamic else 'off'), control=True, issues=('712', '697', 'row:58'))(
        row58_system_import(4, _dynamic, existing=True))
case('row58-system-existing-zhuyin', mode='zhuyin', control=True, issues=('712', '697', 'row:58'))(
    row58_system_import(4, existing=True))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
