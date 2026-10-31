"""Remaining issue 542 audit cases."""
import ctypes as C
import os

from contract_abi import B, P, S, U, UNTOUCHED, Z, full_inst, key_out, sentence_out


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


# #542's remaining named audit inputs, kept in the existing isolated worker.
def phrase_segment_invalid_utf8(k):
    """Segment two invalid UTF-8 bytes and return the flag with the kit's live log-message list."""
    ret = k.fn('phrase_segment', B, P, S)(k.inst, b'\xff\xfe')
    return {'ret': ret, 'messages': k.log_messages}


def train_no_choose_save(k):
    """Parse, guess, train without choosing a row, then report save and immediate user filenames.

    Use nihao for pinyin or su3cl3 for zhuyin; record flags without asserting
    success and leave the cached context and instance live."""
    inst = k.inst
    parse, text = ('parse_more_full_pinyins', b'nihao') if k.mode == 'pinyin' else ('parse_more_chewings', b'su3cl3')
    out = {'parse': k.fn(parse, Z, P, S)(inst, text),
           'guess': k.fn('guess_sentence', B, P)(inst)}
    if k.mode == 'pinyin':
        out['train'] = k.fn('train', B, P, C.c_ubyte)(inst, 0)
    else:
        out['train'] = k.fn('train', B, P)(inst)
    out['save'] = k.fn('save', B, P)(k.ctx)
    out['files'] = sorted(os.listdir(k.user))
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('issue542-phrase-segment-invalid-utf8-' + _mode, mode=_mode, issues=('542', '1297'),
         policy_record='1297-segment-critical.md')(phrase_segment_invalid_utf8)
    case('issue542-train-no-choose-save-' + _mode, mode=_mode, control=True, issues=('542',))(train_no_choose_save)
    case('issue542-parse-full-one-values-' + _mode, mode=_mode, control=True, issues=('542',))(
        lambda k: {t.decode(): key_out(k, t) for t in (b'n', b'ni3')})


@case('issue542-alias-prefix-borrowed', issues=('542',))
def _(k):
    """Reuse a borrowed candidate string as a guess prefix and record subsequent outputs.

    For each prefix API, allocate a nihao instance and borrow its first candidate's
    text. Record three sentence ranks and, for prediction, candidate counts
    before and after retrying with caller-owned 你好 bytes. Assert candidate
    getters; leave the allocated instances live."""
    out = {}
    for predicted_call in (False, True):
        inst = full_inst(k, b'nihao')
        k.fn('guess_sentence', B, P)(inst)
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1E)
        cand, text = P(UNTOUCHED), P(UNTOUCHED)
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, 0, C.byref(cand))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(P))(inst, cand, C.byref(text))
        label = 'predicted' if predicted_call else 'with-prefix'
        out[label + ' borrowed text'] = C.string_at(text.value).decode()
        name = 'guess_predicted_candidates' if predicted_call else 'guess_sentence_with_prefix'
        out[label] = k.fn(name, B, P, P)(inst, text)
        if not predicted_call:
            out['sentences'] = [sentence_out(k, inst, i) for i in range(3)]
        else:
            count = U(UNTOUCHED)
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            out['predicted candidate count'] = count.value
            out['predicted sentences'] = [sentence_out(k, inst, i) for i in range(3)]
            # The same text at caller-owned storage must still predict.
            out['owned prefix recovery'] = k.fn('guess_predicted_candidates', B, P, S)(
                inst, '你好'.encode())
            assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
            out['owned prefix candidate count'] = count.value
    return out


def register(driver_case):
    """Register the declarations in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
