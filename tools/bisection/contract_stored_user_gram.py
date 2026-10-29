"""Stored user gram cases (inventory row 50, #1175).
"""
import ctypes as C
from pathlib import Path

from contract_abi import B, I, P, S, U, UNTOUCHED, Z, consumed, row_with_text, sentence_out
from contract_case_helpers import (
    _DALONG, _gram_bytes, _gram_items, _plant_user_grams, _user_candidate_rows, d18_bigram_db)
from contract_fixtures import _write_user_conf


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def _close_and_read_grams(k, keys):
    """Close/reset cached objects and decode the requested persisted user-gram rows in key order.

    Pass keys as database bytes; map failed lookups to None. Do not save pending
    changes or reopen the native context."""
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    values = d18_bigram_db(k, Path(k.user, 'user_bigram.db'), read_keys=keys, allow_missing=True)
    return [_gram_items(value) for value in values]


def _mark_modified_by_unconstrained_train(k):
    """Train an unconstrained wo guess to make a subsequent save observable.

    Assert tone-option setup, the two-byte parse and sentence guessing, then
    return the facade's training flag. Leave cached objects live."""
    # Neither removal call sets m_modified; an unconstrained train marks it
    # without changing a bigram, so the save that follows is observable.
    assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'wo') == 2
    assert k.fn('guess_sentence', B, P)(k.inst)
    return (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
            else k.fn('train', B, P)(k.inst))


_BINGBING = ('靐靐', "da3'da3", 'ㄉㄚˇ ㄉㄚˇ', 5)


def user_gram_layout(kind, operation):
    """`kind` 'unsorted': the 你 gram holds [龘龘, 号, 好] in descending token
    order. `kind` 'duplicate': it holds 好 twice and 龘龘 twice, sorted.
    Every count passes the export threshold (68) and the predicted filter."""
    def grams(system, users):
        """Encode the captured layout under 你, retaining item order and storing the sum of counts."""
        hao, hao2, dalong = system['好'], system['号'], users[0]
        if kind == 'unsorted':
            items = sorted([(dalong, 90), (hao2, 80), (hao, 70)], reverse=True)
        else:
            items = [(hao, 70), (hao, 80), (dalong, 90), (dalong, 100)]
        total = sum(count for _, count in items)
        return [(system['你'], _gram_bytes(total, items))]

    def probe(k):
        """Plant the captured gram, perform the selected operation and return its observations.

        Save after training or removal, then close/reset caches and decode persisted
        rows. Export instead records at most sixteen iterator steps, stopping after
        two consecutive false has-next results; it closes the iterator but leaves
        the context live."""
        system, users, keys = _plant_user_grams(k, [_DALONG], grams)
        out = {}
        if operation == 'train-save':
            assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
            out['parse'] = consumed(k, 'parse_more_full_pinyins', b'nihao', fresh=False)
            out['guess'] = k.fn('guess_sentence', B, P)(k.inst)
            choices = []
            for offset, text in ((0, '你'), (2, '好')):
                if k.mode == 'pinyin':
                    assert k.fn('guess_candidates', B, P, Z, U)(k.inst, offset, 0x1e)
                else:
                    assert k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, offset)
                candidate = row_with_text(k, k.inst, 2, text)
                assert candidate, 'missing normal candidate: ' + text
                choices.append(k.fn('choose_candidate', I, P, Z, P)(k.inst, offset, candidate))
            out['choices'] = choices
            out['chosen-guess'] = k.fn('guess_sentence', B, P)(k.inst)
            out['train'] = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
                            else k.fn('train', B, P)(k.inst))
            out['save'] = k.fn('save', B, P)(k.ctx)
        elif operation in ('mask-save', 'remove-candidate-save'):
            if operation == 'remove-candidate-save':
                assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
                assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
                rows = _user_candidate_rows(k, k.inst)
                assert rows, 'missing removable user candidate'
                out['removed'] = k.fn('remove_user_candidate', B, P, P)(k.inst, rows[0])
            else:
                # Unsorted: mask 龘龘 out of the gram. Duplicate: mask both 好.
                value = users[0] if kind == 'unsorted' else system['好']
                out['masked'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, value)
            out['trained'] = _mark_modified_by_unconstrained_train(k)
            out['save'] = k.fn('save', B, P)(k.ctx)
        elif operation == 'export':
            it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
            has = k.fn('bigram_iterator_has_next_phrase', B, P)
            nxt = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P),
                       C.POINTER(I))
            steps, falses = [], 0
            # The do/while loads the last predecessor's gram on a `has` that
            # answers false, and scans it on the next one (row 36).
            while falses < 2 and len(steps) < 16:
                if not has(it):
                    falses += 1
                    steps.append('no-next')
                    continue
                falses = 0
                phrase, reading, count = P(), P(), I(UNTOUCHED)
                ret = nxt(it, C.byref(phrase), C.byref(reading), C.byref(count))
                steps.append([ret, k.text(phrase.value), k.text(reading.value), count.value])
            k.fn('end_get_bigram_phrases', None, P)(it)
            out['steps'] = steps
            return out
        out['grams'] = _close_and_read_grams(k, keys)
        return out
    return probe


def user_gram_predicted(kind):
    """Two user phrases with equal counts score equally, so the predicted
    list's stable sort keeps their retrieve_all order. 'unsorted' plants them
    in descending token order; 'duplicate' plants 龘龘 twice, the first above
    the filter (10) and the second below it."""
    def grams(system, users):
        """Encode the captured user-token order with a stored total equal to its item-count sum."""
        dalong, bingbing = users
        if kind == 'unsorted':
            items = sorted([(dalong, 20), (bingbing, 20)], reverse=True)
        else:
            items = [(dalong, 20), (dalong, 3), (bingbing, 20)]
        return [(system['你'], _gram_bytes(sum(c for _, c in items), items))]

    def probe(k):
        """Plant the gram, predict after 你 and return the flag plus candidate type/text rows.

        Getter flags are unchecked; empty or NULL text becomes None and other text
        must decode as UTF-8. Leave the reopened context and instance live."""
        _plant_user_grams(k, [_DALONG, _BINGBING], grams)
        out = {'ret': k.fn('guess_predicted_candidates', B, P, S)(k.inst, '你'.encode())}
        rows = []
        n = U()
        k.fn('get_n_candidate', B, P, C.POINTER(U))(k.inst, C.byref(n))
        for i in range(n.value):
            cand, text, kind_ = P(), S(), I()
            k.fn('get_candidate', B, P, U, C.POINTER(P))(k.inst, i, C.byref(cand))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(k.inst, cand, C.byref(text))
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(k.inst, cand, C.byref(kind_))
            rows.append([kind_.value, text.value.decode() if text.value else None])
        out['rows'] = rows
        return out
    return probe


for _kind in ('unsorted', 'duplicate'):
    for _mode in ('pinyin', 'zhuyin'):
        for _operation in ('train-save', 'mask-save'):
            case(f'user-gram-{_kind}-{_operation}-{_mode}', mode=_mode, issues=('757',))(
                user_gram_layout(_kind, _operation))
    case(f'user-gram-{_kind}-remove-candidate-save', issues=('757',))(user_gram_layout(_kind, 'remove-candidate-save'))
    case(f'user-gram-{_kind}-export', strjoinv_over_read=True, issues=('757', 'row:1'))(
        user_gram_layout(_kind, 'export'))
    case(f'user-gram-{_kind}-predicted', issues=('757',))(user_gram_predicted(_kind))


# Q1's abort (register row 85): training 你 after the sentence start finds
# the start's gram ordered [1, 2, 3, 6, 4] around it (你 = 5). get_freq
# misses, insert_freq appends, and set_freq's second lower_bound lands on the
# larger token, so check_result(user->set_freq(token, freq + seed)) dies
# (074a2219 phonetic_lookup.h:888-903; ngram.cpp:178-205, 252-271).
def train_unsorted_set_freq_miss(k):
    """Train a constrained 你好 guess against a deliberately unsorted sentence-start gram.

    Assert setup and positive choices of NORMAL rows at byte offsets zero and
    two. Return the training flag if the native call returns; do not save."""
    def grams(system, users):
        """Encode five count-one successors around 你 in relative order -4, -3, -2, +1, -1.

        Use sentence-start predecessor one and stored total five; users is unused."""
        ni = system['你']
        items = [(ni - 4, 1), (ni - 3, 1), (ni - 2, 1), (ni + 1, 1), (ni - 1, 1)]
        return [(1, _gram_bytes(5, items))]

    _plant_user_grams(k, [], grams)
    assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    assert consumed(k, 'parse_more_full_pinyins', b'nihao', fresh=False) == 5
    assert k.fn('guess_sentence', B, P)(k.inst)
    for offset, text in ((0, '你'), (2, '好')):
        if k.mode == 'pinyin':
            assert k.fn('guess_candidates', B, P, Z, U)(k.inst, offset, 0x1e)
        else:
            assert k.fn('guess_candidates_after_cursor', B, P, Z)(k.inst, offset)
        candidate = row_with_text(k, k.inst, 2, text)
        assert candidate, 'missing normal candidate: ' + text
        assert k.fn('choose_candidate', I, P, Z, P)(k.inst, offset, candidate) > 0
    assert k.fn('guess_sentence', B, P)(k.inst)
    ret = (k.fn('train', B, P, C.c_ubyte)(k.inst, 0) if k.mode == 'pinyin'
           else k.fn('train', B, P)(k.inst))
    return {'ret': ret}


for _mode in ('pinyin', 'zhuyin'):
    case(f'abort-train-unsorted-set-freq-miss-{_mode}', mode=_mode, abort=False, issues=('757',),
         policy_record='1297-gram-partition-training.md')(
        train_unsorted_set_freq_miss)


# Q3: an item-less gram under a key the mask leaves alone is loaded,
# `SingleGram::mask_out` removes nothing, and `Bigram::mask_out` skips it
# untouched — `if (0 == num) { delete gram; continue; }` (074a2219
# ngram_bdb.cpp:238-241, ngram_kyotodb.cpp:219-222, ngram_tkrzwdb.cpp:195-198)
# — whatever its total; register row 88 records that the pin completes the
# mask and leaves the state.
def itemless_gram_untouched(operation, total):
    """Build a removal/save case with a separate itemless 我 gram carrying the supplied total."""
    def grams(system, users):
        """Encode the removable 你 successors and an independent four-byte 我 total with no items."""
        return [(system['你'], _gram_bytes(140, [(system['好'], 70), (users[0], 70)])),
                (system['我'], total.to_bytes(4, 'little'))]

    def probe(k):
        """Remove the imported phrase, train wo, save and decode both persisted gram rows.

        operation='remove-candidate' removes the first user candidate for dada;
        otherwise mask the imported token. Record flags, then close/reset cached
        objects before reading the database."""
        system, users, keys = _plant_user_grams(k, [_DALONG], grams)
        out = {}
        if operation == 'remove-candidate':
            assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
            assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
            rows = _user_candidate_rows(k, k.inst)
            assert rows, 'missing removable user candidate'
            out['removed'] = k.fn('remove_user_candidate', B, P, P)(k.inst, rows[0])
        else:
            out['masked'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, users[0])
        out['trained'] = _mark_modified_by_unconstrained_train(k)
        out['save'] = k.fn('save', B, P)(k.ctx)
        out['grams'] = _close_and_read_grams(k, keys)
        return out
    return probe


# (name, operation, facade): the name keeps the case ids already cited.
for _name, _operation, _mode in (('pinyin', 'mask-out', 'pinyin'),
                                 ('zhuyin', 'mask-out', 'zhuyin'),
                                 ('remove-candidate', 'remove-candidate', 'pinyin')):
    for _label, _total in (('total-zero', 0), ('total-nonzero', 17)):
        case(f'mask-out-itemless-gram-untouched-{_name}-{_label}', mode=_mode, issues=('757',))(
            itemless_gram_untouched(_operation, _total))


# Q4: an item-less user gram still merges its total with the system gram's
# (074a2219 merge_single_gram, ngram.cpp:298-306). The sentence start's user
# gram carries a total that wraps the merged total to 1, so every bigram from
# the sentence start dominates the first phrase. The phrase segmenter merges
# per step (phrase_lookup.cpp:269-293); the n-best guess merges per row and is
# the control. `texts` replaces the guess inputs; `plant=False` leaves the user
# bigram out and keeps the close-and-reopen around it.
def itemless_user_total(operation, texts=(b'nihaoshijie', b'womenshizhongguoren'), plant=True):
    """Build a segmentation or sentence-guess observation with an optional itemless user start gram.

    Choose its total as (1 - system start total) modulo 2**32. plant=False skips
    the database replacement but still closes the context and writes its marker.
    texts supplies guess inputs only; segmentation uses two fixed Chinese strings."""
    def probe(k):
        """Install the captured start-total fixture and record segmentation tokens or three sentence ranks.

        Finalize/reset cached pointers before writing. Segment on the cached instance,
        ignoring getter flags but avoiding iteration over an untouched count; for
        guessing, allocate and free an instance per input. Leave the context open."""
        system_total = int.from_bytes(
            d18_bigram_db(k, Path(k.data, 'bigram.db'), read_keys=(b'\x01\x00\x00\x00',))[0][:4],
            'little')
        user_total = (1 - system_total) % (1 << 32)
        k.fn('fini', None, P)(k.ctx)
        k._ctx = None
        k._inst = None
        if plant:
            d18_bigram_db(k, Path(k.user, 'user_bigram.db'),
                          [(b'\x01\x00\x00\x00', user_total.to_bytes(4, 'little'))])
        _write_user_conf(k)
        out = {}
        if operation == 'phrase-segment':
            segment = k.fn('phrase_segment', B, P, S)
            n_phrase = k.fn('get_n_phrase', B, P, C.POINTER(U))
            token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))
            # 今天天气很好 changes its first phrases once the total is
            # merged; the other text keeps its segmentation either way.
            for text in ('今天天气很好', '我们是中国人'):
                ret = segment(k.inst, text.encode())
                count = U(UNTOUCHED)
                n_phrase(k.inst, C.byref(count))
                tokens = []
                for index in range(count.value if count.value != UNTOUCHED else 0):
                    slot = U(UNTOUCHED)
                    token(k.inst, index, C.byref(slot))
                    tokens.append(slot.value)
                out[text] = [ret, tokens]
        else:
            for text in texts:
                inst = k.alloc()
                k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
                out[text.decode()] = [k.fn('guess_sentence', B, P)(inst),
                                      [sentence_out(k, inst, index) for index in range(3)]]
                k.fn('free_instance', None, P)(inst)
        return out
    return probe


case('itemless-user-total-phrase-segment', issues=('757',))(itemless_user_total('phrase-segment'))

case('itemless-user-total-guess', control=True, issues=('757',))(itemless_user_total('guess'))


# #791: with the sentence start's merged total wrapped to 1, bigram_poss is
# count / 1, the first phrase's blend is above one, and the pin takes its log
# uncapped (074a2219 phonetic_lookup.h:692-693; bigram_poss = freq / (gfloat)
# total, ngram.cpp:171): a positive score. For 研究生命起源 that makes 研究 the
# beam head, so the unigram pass (topresults[0] only, phonetic_lookup.h:546-547)
# stores a second copy of the 生命 -> 起源 chain beside the bigram pass's
# (eval_item has no identity check, phonetic_lookup_heap.h:56-66): the pin's
# rows are 研究生命起源 twice, then 研究生名气元. A blend clamped at certainty
# moved the head, so that copy was never stored. The control is the same input
# with no planted record, where both sides already store both copies.
case('itemless-user-total-guess-above-one', issues=('791',))(
    itemless_user_total('guess', texts=(b'yanjiushengmingqiyuan',)))

case('itemless-user-total-guess-above-one-control', control=True, issues=('791',))(
    itemless_user_total('guess', texts=(b'yanjiushengmingqiyuan',), plant=False))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
