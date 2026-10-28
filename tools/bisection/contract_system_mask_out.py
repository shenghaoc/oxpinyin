"""System mask out cases (inventory row 36, #1175).
"""
from pathlib import Path

from contract_abi import B, I, P, S, U, Z, sentence_out, tokens_of, unigram_of, zhuyin_sentence_out
from contract_case_helpers import _chunk, _modify_header, _write_log, token_phrase_of, transient_files
from contract_fixtures import private_system


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def mask_out_system_reload(k, saved, mask=0xFFFFFFFF, selected=False, atomic=False,
                           headerless=False, dropped=False):
    """Observe an imported system item across save/log rewriting and mask-out.

    Import 龘龘 into library one and assert its unique token. saved writes a
    baseline; headerless strips its leading 18-byte MODIFY_HEADER. selected
    uses the token's low bits as mask value. atomic also imports an unsaved
    library-two item and checks files, tokens and a bumped frequency afterward.
    dropped additionally observes a unigram add and sentence lookup. Return
    native flags and before/after observations without closing cached objects."""
    phrase = '龘龘'
    assert not tokens_of(k, phrase), 'the setup phrase already exists'
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 1)
    assert it, 'the import iterator did not open'
    reading = b"da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ'.encode()
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, phrase.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    assert added, 'the setup phrase did not add'
    tokens = tokens_of(k, phrase)
    assert len(tokens) == 1 and tokens[0] >> 24 == 1, 'the import used the wrong library'

    out = {'added': added, 'before': token_phrase_of(k, tokens[0])}
    if saved:
        out['save'] = k.fn('save', B, P)(k.ctx)
        assert out['save'], 'the setup save failed'
    if headerless:
        # Save emitted the 18-byte MODIFY_HEADER first. Without it,
        # _peek_header returns an empty logger before ADD is re-appended.
        payload = Path(k.user, 'gb_char.dbin').read_bytes()[8:]
        assert int.from_bytes(payload[:4], 'little') == 4
        _write_log(k, payload[18:])
    if atomic:
        # A second library has an unsaved item, so reloading it before the
        # guard would observably change the refused call's state.
        it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 2)
        other_phrase = '龘龘龘'
        other_reading = b"da2'da2'da2" if k.mode == 'pinyin' else 'ㄉㄚˊ ㄉㄚˊ ㄉㄚˊ'.encode()
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(it, other_phrase.encode(), other_reading, 1)
        k.fn('end_add_phrases', None, P)(it)
        other_token = tokens_of(k, other_phrase)[0]
        other_item = token_phrase_of(k, other_token)
        files = transient_files(k.user)
        assert k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, 0x01001225, 7)
        frequency = unigram_of(k, 0x01001225)
    value = tokens[0] & 0x00FFFFFF if selected else 0
    out['ret'] = k.fn('mask_out', B, P, U, U)(k.ctx, mask, value)
    out['after'] = token_phrase_of(k, tokens[0])
    out['phrase-table-before'] = tokens
    out['phrase-table-after'] = tokens_of(k, phrase)
    if dropped:
        # A phrase-table token is not a live sub-index item. The pin refuses
        # the item update and skips it when pricing a pronunciation hit
        # (phrase_index.cpp:150-174; phonetic_lookup.h:646-647 at 074a2219).
        out['add-unigram'] = k.fn('token_add_unigram_frequency', B, P, U, U)(
            k.inst, tokens[0], 7)
        out['unigram'] = unigram_of(k, tokens[0])
        out['parsed'] = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b"da2'da2")
        out['guessed'] = k.fn('guess_sentence', B, P)(k.inst)
        # Sentence lookup prices pronunciation hits only after checking
        # get_phrase_item. Candidate-list formatting dereferences the pin's
        # dangling table token, so use this defined lookup path instead.
        ret, sentence = (sentence_out(k, k.inst, 0) if k.mode == 'pinyin' else
                         zhuyin_sentence_out(k, k.inst))
        out['lookup'] = [ret, phrase in (sentence or '')]
    if atomic:
        out.update({'untouched-item': out['before'] == out['after'],
                    'untouched-other-library': token_phrase_of(k, other_token) == other_item,
                    'untouched-phrase-table': tokens == out['phrase-table-after'],
                    'untouched-files': transient_files(k.user) == files,
                    'untouched-frequency': unigram_of(k, 0x01001225) == frequency})
    return out


for _mode in ('pinyin', 'zhuyin'):
    case('mask-out-system-unsaved-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_reload(k, False))
    case('mask-out-system-saved-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_reload(k, True))
    case('abort-mask-out-surviving-add-' + _mode, mode=_mode, abort=False, issues=('728',))(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, atomic=True))
    case('mask-out-system-selected-add-' + _mode, mode=_mode, control=True, issues=('728',))(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, selected=True))
    case('mask-out-system-headerless-add-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, headerless=True))
    case('mask-out-dropped-add-full-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_reload(k, True, dropped=True))
    case('mask-out-dropped-add-headerless-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_reload(k, True, 0x00FFFFFF, headerless=True, dropped=True))


def mask_out_system_modify(k, header=True, disk=False):
    """Replace 你 with 龘 in a journal or private system chunk, then observe mask-out.

    Assert the fixed library-one item is a single 你. disk initializes a private
    system first and replaces its chunk; otherwise write a MODIFY log with an
    optional header. Retain item size/unigram using SubPhraseIndex::get_phrase_item
    in src/storage/phrase_index.cpp:180-205 and PhraseIndexLogger::next_record in
    src/storage/phrase_index_logger.h:158-211 at pin 074a2219. Return phrase and table
    observations before/after; leave the context live."""
    # MODIFY passes both non-null pointers supplied by _mask_out_records;
    # ADD does not (phrase_index_logger.h:245, :267-268). Keep the item
    # size and unigram unchanged, replacing only 你's one-character text.
    token = 0x01001225
    payload = Path(k.data, 'gb_char.bin').read_bytes()[8:]
    word = lambda offset: int.from_bytes(payload[offset:offset + 4], 'little')
    total, index, content = word(0), word(4), word(8)
    offset = content + word(index + (token & 0x00FFFFFF) * 4)
    length, prons = payload[offset:offset + 2]
    size = 6 + length * 4 + prons * (length * 2 + 4)
    old = payload[offset:offset + size]
    assert length == 1 and int.from_bytes(old[6:10], 'little') == ord('你')
    new = old[:6] + ord('龘').to_bytes(4, 'little') + old[10:]
    record = ((3).to_bytes(4, 'little') + (token & 0x00FFFFFF).to_bytes(4, 'little') +
              len(old).to_bytes(2, 'little') + len(new).to_bytes(2, 'little') + old + new)
    if disk:
        system = private_system(k)
        k._ctx = k.init(system=system)
        assert k._ctx, 'init failed'
    before = token_phrase_of(k, token)  # initialise before changing disk state
    tokens = tokens_of(k, '你')
    if disk:
        # Replace only a scratch symlink: the reference oracle is read-only.
        path = Path(system, 'gb_char.bin')
        path.unlink()
        path.write_bytes(_chunk(payload[:offset] + new + payload[offset + size:]))
    else:
        _write_log(k, (_modify_header(0, total, total) if header else b'') + record)
    ret = k.fn('mask_out', B, P, U, U)(k.ctx, 0xFFFFFFFF if disk else 0x00FFFFFF, 0)
    return {'ret': ret, 'before': before, 'after': token_phrase_of(k, token),
            'phrase-table-before': tokens, 'phrase-table-after': tokens_of(k, '你')}


for _mode in ('pinyin', 'zhuyin'):
    case('mask-out-system-modify-' + _mode, mode=_mode, issues=('728',))(mask_out_system_modify)
    # Without MODIFY_HEADER the pin returns an empty logger, so even a
    # valid MODIFY record is not merged (phrase_index.cpp:855-856).
    case('mask-out-system-no-header-' + _mode, mode=_mode, control=True, issues=('728',))(
        lambda k: mask_out_system_modify(k, False))
    case('mask-out-system-disk-replacement-' + _mode, mode=_mode, issues=('728',))(
        lambda k: mask_out_system_modify(k, disk=True))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
