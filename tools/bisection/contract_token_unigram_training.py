"""Token unigram and training cases (inventory row 30, #1175).
"""
import ctypes as C

from contract_abi import Arr, B, I, P, S, U, UNTOUCHED, Z, choose_text, tokens_of, unigram_of
from contract_case_helpers import token_unigram_trained, token_unigram_user


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #576: zhuyin.cpp:1813-1826 at 074a2219 reads the phrase item's
# stored unigram, including generation-time +1 and the in-memory overlay.
# e2853126 already ported the getter; keep the pinyin twin as a control.
def token_unigram_system(k):
    """Record a fixed system token's unigram before and after adding seven.

    Use token 0x01001225 from the oracle fixture, retain the add return flag,
    and also query the fixture's absent-item and absent-library IDs. The
    frequency update affects the same context used for all observations."""
    token = 0x01001225  # 你; the stored system field is 52887, not 52888.
    out = {'token': hex(token), 'before': unigram_of(k, token)}
    out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 7)
    out['after'] = unigram_of(k, token)
    out['absent-item'] = unigram_of(k, 0x01FFFFFF)
    out['absent-library'] = unigram_of(k, 0x0FFFFFFF)
    return out


# #809: the ten zhuyin-only syllables (content_table keys that pinyin_index
# lacks) are stored as key words the pin hands back unfiltered
# (074a2219 zhuyin.cpp:1778-1811, phrase_index.cpp:33-44). Libraries 1-4 read
# the replaced item through the spelling round trip, which dropped them;
# libraries 5-7 read the user store and are the controls.
ZHUYIN_ONLY = {
    'chua': 'ㄔㄨㄚ', 'den': 'ㄉㄣ', 'din': 'ㄉㄧㄣ', 'fe': 'ㄈㄜ', 'kei': 'ㄎㄟ',
    'len': 'ㄌㄣ', 'nia': 'ㄋㄧㄚ', 'rua': 'ㄖㄨㄚ', 'yai': 'ㄧㄞ', 'zhei': 'ㄓㄟ',
}
ZHUYIN_ONLY_WORDS = ['龘鱻', '麤靐', '驫齉', '爨馕', '鬻厵', '龘麤', '鱻靐', '驫爨', '齉鬻', '馕厵']


def zhuyin_only_readings(k, libraries):
    """Import plain and third-tone readings for each zhuyin-only syllable and library.

    Use facade-specific spellings and count five, closing each add iterator.
    Return add flags, matching token IDs and up to four raw pronunciations as
    hex-encoded two-byte keys. Count and pronunciation keys share one row per
    library/syllable and are overwritten as matching tokens are visited.
    Accessor failures remain in the observations; allocated native arrays
    are not freed here."""
    out = {}
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    for library in libraries:
        for (pinyin, zhuyin), word in zip(ZHUYIN_ONLY.items(), ZHUYIN_ONLY_WORDS):
            if k.mode == 'pinyin':
                readings = ["%s'%s" % (pinyin, pinyin), "%s3'%s3" % (pinyin, pinyin)]
            else:
                readings = ['%s %s' % (zhuyin, zhuyin), '%sˇ %sˇ' % (zhuyin, zhuyin)]
            iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
            added = [k.fn('iterator_add_phrase', B, P, S, S, I)(iterator, word.encode(), r.encode(), 5)
                     for r in readings]
            k.fn('end_add_phrases', None, P)(iterator)
            row = {'added': added}
            tokens = [token for token in tokens_of(k, word) if token >> 24 == library]
            row['tokens'] = [hex(token) for token in tokens]
            for token in tokens:
                count = U(UNTOUCHED)
                row['n_pron'] = [k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(
                    k.inst, token, C.byref(count)), count.value]
                for nth in range(min(count.value, 4)):
                    keys = new(0, 0, 2)
                    ret = k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
                    view = C.cast(keys, C.POINTER(Arr)).contents
                    row['nth %d' % nth] = [ret, C.string_at(view.data, view.len * 2).hex() if view.len else '']
            out['lib %d %s' % (library, pinyin)] = row
    return out


for _mode in ('pinyin', 'zhuyin'):
    _prefix = 'zhuyin-' if _mode == 'zhuyin' else ''
    case(_prefix + 'system-zhuyin-only-readings', mode=_mode, issues=('809',))(
        lambda k: zhuyin_only_readings(k, (1, 2, 3, 4)))
    case(_prefix + 'user-zhuyin-only-readings', mode=_mode, control=True, issues=('809',))(
        lambda k: zhuyin_only_readings(k, (5, 6, 7)))


# #808: the default facade's USER_FILE libraries 5 (ADDON), 6 (NETWORK) and 7
# (USER) are read from m_phrase_index (074a2219 pinyin.cpp:2821-2834 via
# phrase_index.h:646-652): true with the stored field for a real token, false
# with 0 for an id that owns no item. The pinyin getter used to read the addon
# facade for 5 and 6 and never checked the item for 7.
def token_unigram_user_libraries(k):
    """Plant phrases in libraries 5, 6 and 7 and observe unigram updates and nearby IDs.

    Assert each import succeeds and yields exactly one token in its requested
    library. Record that token's lookup, three presumed-absent neighbors, the
    return flag from adding seven and the subsequent lookup. Imports use
    count nine and close their add iterators; all libraries share the context."""
    out = {}
    plants = {5: '你好你好', 6: '再见再见', 7: '你好你好呀'}
    readings = {
        'pinyin': {5: "ni3'hao3'ni3'hao3", 6: "zai4'jian4'zai4'jian4", 7: "ni3'hao3'ni3'hao3'ya1"},
        'zhuyin': {5: 'ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ', 6: 'ㄗㄞˋ ㄐㄧㄢˋ ㄗㄞˋ ㄐㄧㄢˋ',
                   7: 'ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ ㄧㄚ'},
    }[k.mode]
    for library, phrase in plants.items():
        iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, library)
        assert iterator
        added = k.fn('iterator_add_phrase', B, P, S, S, I)(
            iterator, phrase.encode(), readings[library].encode(), 9)
        k.fn('end_add_phrases', None, P)(iterator)
        assert added
        tokens = [token for token in tokens_of(k, phrase) if token >> 24 == library]
        assert len(tokens) == 1, tokens
        token = tokens[0]
        out['lib %d token' % library] = hex(token)
        out['lib %d item' % library] = unigram_of(k, token)
        for delta in (0, 2, 100):
            absent = (library << 24) | ((token & 0xFFFFFF) + delta) if delta else token - 1
            out['lib %d absent %#x' % (library, absent)] = unigram_of(k, absent)
        out['lib %d add' % library] = k.fn(
            'token_add_unigram_frequency', B, P, U, U)(k.inst, token, 7)
        out['lib %d after' % library] = unigram_of(k, token)
    return out


for _mode in ('pinyin', 'zhuyin'):
    _prefix = 'zhuyin-' if _mode == 'zhuyin' else ''
    case(_prefix + 'token-unigram-system', mode=_mode, control=_mode == 'pinyin', issues=('808',))(
        token_unigram_system)
    case(_prefix + 'token-unigram-user', mode=_mode, control=_mode == 'pinyin', issues=('808',))(
        token_unigram_user)
    case(_prefix + 'token-unigram-trained', mode=_mode, control=_mode == 'pinyin', issues=('808',))(
        token_unigram_trained)
    case(_prefix + 'token-unigram-user-libraries', mode=_mode, control=_mode == 'zhuyin', issues=('808',))(
        token_unigram_user_libraries)


@case('train-unigram-total', issues=('540',))
def _(k):
    """Record unigram growth and training return flags at cumulative training checkpoints.

    Parse nihao, choose 你 and 好 where available, and guess again on the
    cached instance. Train result zero 30,000 times without resetting, recording
    at 1, 10,000, 20,000 and 30,000 calls. Each checkpoint stores sorted distinct
    return flags and both unigram lookups; selection results are recorded
    rather than asserted."""
    # Once the library's guint32 total would overflow the item stops growing:
    # at the pin 你 freezes at 2122897296 after about 20000 trainings.
    inst = k.inst
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    ni, hao = tokens_of(k, '你')[0], tokens_of(k, '好')[0]
    first = choose_text(k, inst, 0, '你')
    second = choose_text(k, inst, first, '好') if first else None
    k.fn('guess_sentence', B, P)(inst)
    out = {'chosen': [first, second], 'before': [unigram_of(k, ni), unigram_of(k, hao)]}
    train = k.fn('train', B, P, C.c_ubyte)
    for step, count in ((1, 1), (2, 9999), (3, 10000), (4, 10000)):
        rets = {train(inst, 0) for _ in range(count)}
        out['after %d' % sum(c for s, c in ((1, 1), (2, 9999), (3, 10000), (4, 10000)) if s <= step)] = [
            sorted(rets), unigram_of(k, ni), unigram_of(k, hao)]
    return out


@case('predicted-unigram-overflow', issues=('540',))
def _(k):
    """Count repeated predicted-candidate acceptances after 20,000 training calls.

    Seed the cached instance with nihao, choose 你 and 好 where available and
    train result zero. Generate predictions for 你 and find the first kind-5
    row whose first text token belongs to library 1. Return chosen offsets
    and whether a row was found; if found, count successful selections up to
    the first false return or 1,000 successes. The reported count can therefore
    be capped without observing a refusal. Training and selection mutate the
    shared context, and the same candidate pointer is reused."""
    # Training 你 and 好 fills library 1 to within one seed of its guint32
    # total; accepted predicted candidates (483 each) then run it out, and
    # the pin answers false from the call that no longer fits.
    inst = k.inst
    k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
    k.fn('guess_sentence', B, P)(inst)
    first = choose_text(k, inst, 0, '你')
    second = choose_text(k, inst, first, '好') if first else None
    k.fn('guess_sentence', B, P)(inst)
    train = k.fn('train', B, P, C.c_ubyte)
    for _ in range(20000):
        train(inst, 0)
    k.fn('guess_predicted_candidates', B, P, S)(inst, '你'.encode())
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    pick = None
    for i in range(count.value):
        cand, kind, text = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        if kind.value == 5 and (tokens_of(k, text.value.decode()) or [0])[0] >> 24 == 1:
            pick = cand
            break
    out = {'chosen': [first, second], 'found': pick is not None}
    if pick is None:
        return out
    accepted = 0
    choose = k.fn('choose_predicted_candidate', B, P, P)
    while accepted < 1000 and choose(inst, pick):
        accepted += 1
    out['accepted before the first refusal'] = accepted
    return out


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
