"""Integer limits D18 cases (inventory row 60, #1175).
"""
import ctypes as C
from pathlib import Path
import re
import shutil
import tempfile

from contract_abi import (
    B, I, P, S, U, UNTOUCHED, Z, consumed, final_step_sentences, row_with_text, tokens_of, unigram_of)
from contract_case_helpers import d18_bigram_db, d18_export_phrases, d18_gram, guess_rows, zhuyin_guess_rows
from contract_fixtures import _write_user_conf


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def d18_remember(deltas):
    """Build successive remember-user-input updates for 你好世界 with the supplied signed deltas."""
    def probe(k):
        """Parse nihaoshijie once, then record each remember flag, user export and resulting unigram.

        Require a matching library-seven token after each call; next raises if none exists."""
        out = {'parsed': consumed(k, 'parse_more_full_pinyins', b'nihaoshijie', fresh=False)}
        rows = []
        for delta in deltas:
            ret = k.fn('remember_user_input', B, P, S, I)(k.inst, '你好世界'.encode(), delta)
            token = next(t for t in tokens_of(k, '你好世界') if t >> 24 == 7)
            rows.append([delta, ret, d18_export_phrases(k), unigram_of(k, token)])
        out['steps'] = rows
        return out
    return probe


case('d18-remember-reported', control=True, issues=('540',))(d18_remember((-1, 0, 0x7fffffff)))
case('d18-remember-signed-boundary', control=True, issues=('540',))(d18_remember((0x7ffffffe, 1, 1, 0x7fffffff, 1)))
for _count in (715827882, 715827883, 1431655765, 1431655766):
    case(f'd18-remember-multiply-{_count}', control=True, issues=('540',))(d18_remember((_count,)))


# #752: reuse the preregistered full-pinyin NUL-middle case below,
# axis-c-encoding-pinyin-remember_user_input-nul-middle. These are its
# transformed-parser twins and a known-character NUL-termination control.
# 074a2219 pinyin.cpp:3115-3121, 3688-3694 refuses a character with no token;
# bytes after the first NUL cannot change the visible phrase.
def remember_nul(parse_name, input_text, phrase):
    """Build a remember-input case using the supplied parser bytes and possibly NUL-containing phrase."""
    def probe(k):
        """Configure the parser, assert complete input consumption and remember phrase with count one.

        Use options 0x28 and scheme two for double pinyin or one for chewing.
        Return parse length, remember flag and user exports; pass phrase bytes unchanged."""
        k.fn('set_options', B, P, U)(k.ctx, 0x28)
        if parse_name == 'parse_more_double_pinyins':
            assert k.fn('set_double_pinyin_scheme', B, P, I)(k.ctx, 2)
        elif parse_name == 'parse_more_chewings':
            assert k.fn('set_zhuyin_scheme', B, P, I)(k.ctx, 1)
        parsed = k.fn(parse_name, Z, P, S)(k.inst, input_text)
        assert parsed == len(input_text), 'the complete two-syllable input must parse'
        return dict(parsed=parsed,
                    ret=k.fn('remember_user_input', B, P, S, I)(k.inst, phrase, 1),
                    rows=d18_export_phrases(k))
    return probe


case('remember-nul-double-pinyin', stderr=True, issues=('752',))(
    remember_nul('parse_more_double_pinyins', b'nihk', b'ni\0hao'))
case('remember-nul-chewing-pinyin', stderr=True, issues=('752',))(
    remember_nul('parse_more_chewings', b'su3cl3', b'ni\0hao'))
case('remember-known-nul-suffix-pinyin', control=True, stderr=True, issues=('752',))(
    remember_nul('parse_more_full_pinyins', b'nihao', '你好'.encode() + b'\0ignored'))


def d18_unigram(boundary, text='你'):
    """Build a unigram-increment sequence for the first token matching text.

    boundary derives increments from the token's actual system-library total;
    otherwise use fixed maximum/signed-boundary increments."""
    def probe(k):
        """Record unigram outputs after each increment, checking the derived boundary sequence when requested.

        Resolve the library's system filename from table.conf and read its stored
        total when boundary is true. Require the 丠 control to use library two and
        assert boundary flags True, True, False, False with counts unchanged after
        refusal. Leave the context modified by the attempted calls."""
        token = tokens_of(k, text)[0]
        if text == '丠':
            assert token >> 24 == 2, 'the alternate-sub-index control must use library 2'
        out = {'token': token, 'before': unigram_of(k, token)}
        if boundary:
            # Select the token's actual sub-index, not an assumed GB token.
            libraries = {'RESERVED': 0, 'GB_DICTIONARY': 1, 'GBK_DICTIONARY': 2,
                         'OPENGRAM_DICTIONARY': 3, 'MERGED_DICTIONARY': 4,
                         'ADDON_DICTIONARY': 5, 'NETWORK_DICTIONARY': 6, 'USER_DICTIONARY': 7}
            library = token >> 24
            entries = [line.split() for line in Path(k.data, 'table.conf').read_text().splitlines()
                       if line.startswith('default ')]
            entry = next(row for row in entries if libraries[row[1]] == library)
            assert entry[3] != 'NULL', 'boundary fixture needs a system sub-index'
            # MemoryChunk's 8-byte header, then SubPhraseIndex::m_total_freq.
            total = int.from_bytes(Path(k.data, entry[3]).read_bytes()[8:12], 'little')
            deltas = (0xffffffff - total - 1, 1, 1, 0xffffffff)
            out.update(library=library, subindex=entry[3], **{'library-total': total})
        else:
            deltas = (0xffffffff, 0x7fffffff, 0x7fffffff, 0x7fffffff, 0x7fffffff, 0xffffffff)
        add = k.fn('token_add_unigram_frequency', B, P, U, U)
        out['steps'] = [[delta, add(k.inst, token, delta), unigram_of(k, token)] for delta in deltas]
        if boundary:
            assert [row[1] for row in out['steps']] == [True, True, False, False]
            assert out['steps'][1][2] == out['steps'][2][2] == out['steps'][3][2]
        return out
    return probe


def d18_input(k):
    """Compare consumed and retained byte lengths for long pinyin inputs and a subsequent short parse.

    Set tone options, use the listed 4096..65537-byte lengths and four tail shapes,
    and free each temporary instance after reading its outputs."""
    assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    out = {}
    parse = k.fn('parse_more_full_pinyins', Z, P, S)
    for length in (4096, 4097, 32766, 32767, 32768, 65535, 65536, 65537):
        inst = k.alloc()
        text = (b'ni' * ((length + 1) // 2))[:length]
        ret = parse(inst, text)
        stored = k.fn('get_parsed_input_length', Z, P)(inst)
        # A later short parse must replace the retained long matrix/length.
        short = parse(inst, b'hao')
        out[str(length)] = [ret, stored, short, k.fn('get_parsed_input_length', Z, P)(inst)]
        k.fn('free_instance', None, P)(inst)
    for label, text in (('whole-tail', b'ni' * 16382 + b'hao'),
                        ('split-tail', b'ni' * 16383 + b'hao'),
                        ('separator-tail', b'ni' * 16383 + b"'hao"),
                        ('invalid-tail', b'ni' * 16383 + b'!ni')):
        inst = k.alloc()
        out[label] = [parse(inst, text), k.fn('get_parsed_input_length', Z, P)(inst)]
        k.fn('free_instance', None, P)(inst)
    return out


def d18_bigram_fixture(k, system, user, current='号', empty_system=False, extra_successor=False):
    """Replace bigram fixtures and return predecessor/current tokens after closing cached objects.

    system and user are (total, count) pairs. A non-None system or empty_system
    copies the whole data tree to scratch and replaces bigram.db; otherwise
    retain the current system tree. Write two user predecessors with sorted
    successors, an empty start row and a conforming marker. extra_successor adds
    我 with count one. Reopening is deferred."""
    prev, cur, other = (tokens_of(k, text)[0] for text in ('你', current, '我'))
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    if system is not None or empty_system:
        copied = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
        shutil.copytree(k.data, copied, dirs_exist_ok=True)
        k.data = copied
        system_rows = [] if empty_system else [
            (prev.to_bytes(4, 'little'), d18_gram(system[0], cur, system[1]))]
        d18_bigram_db(k, Path(copied, 'bigram.db'), system_rows)
    # More than one predecessor: the export's do/while leaves its last
    # loaded gram for the next has_next call (row 36, already fixed).
    records = [(cur, user[1])] + ([(other, 1)] if extra_successor else [])
    gram = user[0].to_bytes(4, 'little') + b''.join(
        t.to_bytes(4, 'little') + count.to_bytes(4, 'little') for t, count in sorted(records))
    rows = [(p.to_bytes(4, 'little'), gram) for p in (prev, other)]
    rows.append((b'\x01\x00\x00\x00', b'\0' * 4))
    d18_bigram_db(k, Path(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    return prev, cur


def d18_merge_observation(k):
    """Collect sentence and candidate observations before/after constraining nihao to 你.

    Enable tone/dynamic options, assert a NORMAL predecessor row, choose at byte
    zero and list NORMAL rows at byte two. Assert a nonempty final normal list;
    read sentence ranks through final_step_sentences."""
    assert k.fn('set_options', B, P, U)(k.ctx, (1 << 5) | (1 << 9))
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'nihao')
    guessed = k.fn('guess_sentence', B, P)(k.inst)
    sentences = final_step_sentences(k, k.inst)
    first = guess_rows(k, k.inst, 0, 0x1e) if k.mode == 'pinyin' else zhuyin_guess_rows(k, k.inst, 0)
    candidate = row_with_text(k, k.inst, 2, '你')
    assert first['ret'] and candidate, 'missing normal predecessor candidate'
    chosen = k.fn('choose_candidate', I, P, Z, P)(k.inst, 0, candidate)
    constrained = k.fn('guess_sentence', B, P)(k.inst)
    chosen_sentences = final_step_sentences(k, k.inst)
    rows = guess_rows(k, k.inst, 2, 0x1e) if k.mode == 'pinyin' else zhuyin_guess_rows(k, k.inst, 2)
    normal = [row for row in rows['rows'] if row[0] == 2]
    assert normal, 'merged-row fixture must reach normal candidates'
    return dict(parsed=parsed, guessed=guessed, chosen=chosen, constrained=constrained,
                sentences=sentences, chosen_sentences=chosen_sentences,
                ret=rows['ret'], candidates=normal)


def d18_merge(system, user, current='号', empty_system=False, extra_successor=False, lam=None):
    """Build a merged-bigram observation with optional system/user fixture and lambda override.

    When lam is supplied, the callable overwrites k.data/table.conf; callers
    must choose a fixture that copies the system tree before that edit."""
    def probe(k):
        """Install the captured bigram fixtures, optionally rewrite lambda and collect constrained guesses."""
        d18_bigram_fixture(k, system, user, current=current, empty_system=empty_system,
                          extra_successor=extra_successor)
        if lam is not None:
            conf = Path(k.data, 'table.conf')
            conf.write_text(re.sub(r'(?m)^lambda parameter:.*$', 'lambda parameter:' + lam,
                                   conf.read_text()))
        return d18_merge_observation(k)
    return probe


def d18_bigram_export(count):
    """Build a one-row bigram export observation with equal supplied total and item count."""
    def probe(k):
        """Plant the 好 successor, record has-next and fetch one export row regardless of that flag.

        Decode/free both returned strings and end the bigram iterator on normal completion."""
        d18_bigram_fixture(k, None, (count, count), current='好')
        it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
        out = {'has': k.fn('bigram_iterator_has_next_phrase', B, P)(it)}
        phrase, reading, value = P(), P(), I(UNTOUCHED)
        ret = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
            it, C.byref(phrase), C.byref(reading), C.byref(value))
        out['row'] = [ret, k.text(phrase.value), k.text(reading.value), value.value]
        k.fn('end_get_bigram_phrases', None, P)(it)
        return out
    return probe


def d18_bigram_update(count, predicted=False, user_only=False):
    """Build a bigram update/save/reopen case using training or a predicted selection.

    user_only empties system bigrams, uses 嚎 and adds a second user successor;
    otherwise use 好. count seeds the user's item count."""
    def probe(k):
        """Plant the gram, choose/train as configured and observe the saved predecessor record.

        Predicted mode chooses a BIGRAM row then guesses wo before training;
        ordinary mode constrains nihao to 你 and 好. Record unigram and save flags,
        close/reset caches, read raw total/items and lazily reopen for a final count.
        user_only also records a reopened constrained guess."""
        current = '嚎' if user_only else '好'
        prev, cur = d18_bigram_fixture(k, None, (count + int(user_only), count), current=current,
                                     empty_system=user_only, extra_successor=user_only)
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
        out = {'before': unigram_of(k, cur)}
        if predicted:
            out['predicted'] = k.fn('guess_predicted_candidates', B, P, S)(k.inst, '你'.encode())
            candidate = row_with_text(k, k.inst, 4, current)
            assert candidate, 'missing predicted bigram candidate'
            out['choose'] = k.fn('choose_predicted_candidate', B, P, P)(k.inst, candidate)
            # A guessed, unconstrained sentence makes train set m_modified
            # without changing counts, so save exposes the predicted update.
            out['parse'] = consumed(k, 'parse_more_full_pinyins', b'wo', fresh=False)
        else:
            out['parse'] = consumed(k, 'parse_more_full_pinyins', b'nihao', fresh=False)
        out['guess'] = k.fn('guess_sentence', B, P)(k.inst)
        if not predicted:
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
        out['after'] = unigram_of(k, cur)
        out['save'] = k.fn('save', B, P)(k.ctx)
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
        raw = d18_bigram_db(k, Path(k.user, 'user_bigram.db'), read_keys=(prev.to_bytes(4, 'little'),))[0]
        total = int.from_bytes(raw[:4], 'little')
        records = [[int.from_bytes(raw[i:i+4], 'little'), int.from_bytes(raw[i+4:i+8], 'little')]
                   for i in range(4, len(raw), 8)]
        out['stored'] = [total, records]
        out['reopened-unigram'] = unigram_of(k, cur)
        if user_only:
            out['reopened-guess'] = d18_merge_observation(k)
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    case('d18-unigram-max-' + _mode, mode=_mode, control=True, issues=('540',))(d18_unigram(False))
    case('d18-unigram-total-boundary-' + _mode, mode=_mode, control=True, issues=('540',))(d18_unigram(True))
    case('d18-input-boundaries-' + _mode, mode=_mode, control=True, issues=('540',))(d18_input)
    for _label, _system, _user, _control in (
            ('total-before', (1, 1), (0xfffffffe, 1), True),
            ('total-zero', (1, 1), (0xffffffff, 1), False),
            ('total-after', (2, 1), (0xffffffff, 1), False),
            ('count-before', (0x80000000, 1), (0xffffffff, 0xfffffffe), True),
            ('count-zero', (0x80000000, 1), (0xffffffff, 0xffffffff), False),
            ('count-after', (0x80000000, 2), (0xffffffff, 0xffffffff), False)):
        case(f'd18-merge-{_label}-{_mode}', mode=_mode, control=_control, issues=('540',))(
            d18_merge(_system, _user, current='嚎' if _label.startswith('count-') else '号'))
    for _label, _count, _control in (
            ('stored-zero', 0, False), ('seed-before', 0x7fffffff, True), ('seed-zero', 0x80000000, False),
            ('seed-after', 0x80000001, False), ('total-max', 0xffffffff - 22080, True),
            ('total-overflow', 0xffffffff - 22080 + 1, False)):
        case(f'd18-train-{_label}-{_mode}', mode=_mode, control=_control, issues=('540',))(d18_bigram_update(_count))
for _label, _count, _control in (('max', 0xffffffff - 69, True),
                                ('zero', 0xffffffff - 68, False), ('after', 0xffffffff - 67, False)):
    case('d18-predicted-total-' + _label, control=_control, issues=('540',))(d18_bigram_update(_count, predicted=True))
for _count in (0x3fffffff, 0x40000000, 0x7fffffff, 0x80000000, 0x80000001, 0xffffffff):
    case(f'd18-bigram-export-{_count:x}', control=True, issues=('540',))(d18_bigram_export(_count))

# Review regressions: row/record presence survives a wrapped zero total/count
# (074a2219 ngram.cpp:135-175,277-358; phonetic_lookup.h:600-640).
for _mode in ('pinyin', 'zhuyin'):
    case('d18-review-user-only-zero-total-' + _mode, mode=_mode, issues=('540',))(
        d18_merge(None, (0, 0xffffffff), current='嚎', empty_system=True, extra_successor=True))
    for _label, _lam in (('default', None), ('half', '0.5'), ('zero-lambda', '0')):
        case(f'd18-review-zero-count-zero-total-{_label}-{_mode}', mode=_mode,
             control=_label != 'default' or _mode == 'pinyin', issues=('540',))(
            d18_merge((1, 1), (0xffffffff, 0xffffffff),
                      current='好' if _label == 'default' else '嚎', lam=_lam))
    case('d18-review-unigram-library-two-' + _mode, mode=_mode, control=True, issues=('540',))(
        d18_unigram(True, text='丠'))
case('d18-review-predicted-user-only-zero-total', issues=('540',))(
    d18_bigram_update(0xffffffff - 69, predicted=True, user_only=True))


# #739 review: save_db refuses removing a directory on all three cells
# (074a2219 chewing_large_table2_bdb.cpp:138-140,
# phrase_large_table3_bdb.cpp:153-155; chewing_large_table2_kyotodb.cpp:125-127,
# phrase_large_table3_kyotodb.cpp:140-142; chewing_large_table2_tkrzwdb.cpp:115-118,
# phrase_large_table3_tkrzwdb.cpp:106-109), but zhuyin_save still renames the
# untouched temporary directory (zhuyin.cpp:650-679). A regular live index
# prevents that rename. Both facades and all destination variants are
# verified on bdb, kc and tkrzw (#1120).
def stale_index_tmp(k, index):
    """Record save results with a stale temporary index directory and three destination shapes.

    Use a fresh user directory for each absent, empty-directory or live-file
    destination, train before saving and preserve a marker in index + '.tmp'.
    Report marker movement, live-file preservation and copied log messages;
    free the instance and close/reset the context after each destination."""
    out = {}
    for destination in ('absent', 'empty-directory', 'live-file'):
        k.user = tempfile.mkdtemp(prefix='user-', dir=k.scratch)
        inst = k.inst
        parse, text = ('parse_more_full_pinyins', b'nihao') if k.mode == 'pinyin' else ('parse_more_chewings', b'su3cl3')
        def train():
            """Assert parsing and guessing of the facade's nihao input, then return its training flag."""
            assert k.fn(parse, Z, P, S)(inst, text)
            assert k.fn('guess_sentence', B, P)(inst)
            return (k.fn('train', B, P, C.c_ubyte)(inst, 0) if k.mode == 'pinyin'
                    else k.fn('train', B, P)(inst))
        assert train()
        live = Path(k.user, index)
        if destination == 'live-file':
            assert k.fn('save', B, P)(k.ctx)
            before = live.read_bytes()
            assert train()
        elif destination == 'empty-directory':
            live.mkdir()
        tmp = Path(k.user, index + '.tmp')
        tmp.mkdir()
        (tmp / 'stale-marker').write_bytes(b'unchanged temporary directory')
        ret = k.fn('save', B, P)(k.ctx)
        out[destination] = {
            'save': ret,
            'tmp-present': tmp.exists(),
            'stale-moved': (live / 'stale-marker').is_file() and
                (live / 'stale-marker').read_bytes() == b'unchanged temporary directory',
            'live-preserved': destination == 'live-file' and live.read_bytes() == before,
            'messages': list(k.log_messages),
        }
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
        k.fn('fini', None, P)(k._ctx)
        k._ctx = None
    return out


for _mode in ('pinyin', 'zhuyin'):
    for _index in ('user_pinyin_index.bin', 'user_phrase_index.bin'):
        case(f'issue542-stale-{_index}-{_mode}', mode=_mode, stderr=True,
             control=_mode == 'pinyin', issues=('542',))(
            lambda k, index=_index: stale_index_tmp(k, index))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
