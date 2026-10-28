"""Helpers shared by several contract case families (#1175).

Each definition is the driver's, moved verbatim. None reads per-run driver state; the
driver imports every name back, so its remaining cases, the family registrations that
receive them and other tools resolve them where they always did.
"""
import ctypes as C
from pathlib import Path
import shutil
import tempfile

from contract_abi import Arr, B, I, P, S, U, UNTOUCHED, Z, final_step_sentences, tokens_of, unigram_of
from contract_fixtures import _craft_bdb, _write_user_conf


def token_unigram_user(k, *, add_frequency=True):
    """Import a phrase into library 7 and observe its unigram, optionally adding seven.

    Use the facade's reading syntax and import count nine. Assert iterator
    creation, successful addition and exactly one matching library-7 token.
    Close the add iterator, then return the token and before/after lookups;
    include an add flag only when add_frequency is true."""
    phrase = '你好你好'
    reading = "ni3'hao3'ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ ㄋㄧˇ ㄏㄠˇ'
    iterator = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert iterator
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(
        iterator, phrase.encode(), reading.encode(), 9)
    k.fn('end_add_phrases', None, P)(iterator)
    assert added
    tokens = [token for token in tokens_of(k, phrase) if token >> 24 == 7]
    assert len(tokens) == 1, tokens
    token = tokens[0]
    out = {'imported': added, 'token': hex(token), 'before': unigram_of(k, token)}
    if add_frequency:
        out['add'] = k.fn('token_add_unigram_frequency', B, P, U, U)(k.inst, token, 7)
    out['after'] = unigram_of(k, token)
    return out


def train_selected_top(k, wanted, token, guess_before_choose=False):
    """Choose a normal phrase, guess n-best, then train result 0 without reset.

    This test-only sequence preserves the runbook's train_top precondition:
    a candidate list alone cannot be trained. The normal selection also
    installs the OneStep constraint that train_result3 needs to write counts.

    Require a parsed cached instance with the wanted normal candidate and
    a user profile for training. Assert candidate/accessor success, positive
    selection cursor, a successful sentence guess and successful training;
    raise AssertionError if no matching kind-2 row exists. Return selection
    and training details plus the token's unigram immediately after choosing.
    With guess_before_choose, also guess and snapshot sentences before the
    selection, using forked sentence reads for pinyin. Selection and training
    mutate the cached instance and its context without resetting them.

    The constraint path is pinyin_choose_candidate at src/pinyin.cpp:2578-2588
    and zhuyin_choose_candidate at src/zhuyin.cpp:1646-1666; count updates are
    in PhoneticLookup::train_result3 at src/lookup/phonetic_lookup.h:874-929
    at pin 074a2219."""
    inst = k.inst
    if k.mode == 'pinyin':
        # SORT_WITHOUT_SENTENCE (0x1) would train during choose instead
        # of installing a constraint (074a2219 pinyin.cpp:2566-2576).
        assert k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0x1e)
    else:
        assert k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
    count = U()
    assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    for index in range(count.value):
        candidate, text, kind = P(), S(), I()
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(
            inst, index, C.byref(candidate))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(
            inst, candidate, C.byref(text))
        assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(
            inst, candidate, C.byref(kind))
        # NORMAL_CANDIDATE / NORMAL_CANDIDATE_AFTER_CURSOR are both 2.
        # The zhuyin BEST_MATCH row installs no constraint (074a2219
        # zhuyin.cpp:1643-1654), so selecting it would make this a no-op.
        if kind.value == 2 and text.value == wanted.encode():
            before = {}
            if guess_before_choose:
                # Retain this NORMAL row before guessing promotes matching
                # text to NBEST in a newly generated candidate list. Guess
                # leaves m_candidates intact (074a2219 pinyin.cpp:1372-1386).
                assert k.fn('guess_sentence', B, P)(inst)
                before['before-choose'] = final_step_sentences(k, inst)
            cursor = k.fn('choose_candidate', I, P, Z, P)(inst, 0, candidate)
            assert cursor > 0, 'normal phrase selection failed'
            after_choose = unigram_of(k, token)
            guessed = k.fn('guess_sentence', B, P)(inst)
            assert guessed, 'training needs a guessed n-best result'
            if k.mode == 'pinyin':
                trained = k.fn('train', B, P, C.c_ubyte)(inst, 0)
            else:
                trained = k.fn('train', B, P)(inst)
            assert trained, 'training n-best result 0 failed'
            return {'chosen': wanted, 'kind': kind.value, 'cursor': cursor,
                    'after-choose': after_choose, 'guess': guessed, 'train': trained, **before}
    raise AssertionError('missing normal candidate: ' + wanted)


def token_unigram_trained(k):
    """Record a fixed phrase's unigram around one constrained training operation.

    Require the oracle token for 癫痫 and a complete parse of the facade's
    input bytes. Select its normal candidate and train result zero through
    train_selected_top, asserting that selection alone did not alter the
    unigram. Return the consumed byte count, training details and before/after
    lookups; training modifies the context's learned state."""
    token = 0x01003e57  # 癫痫: one constrained train adds 69 * 7 = 483.
    assert tokens_of(k, '癫痫') == [token]
    parse = 'parse_more_full_pinyins' if k.mode == 'pinyin' else 'parse_more_chewings'
    text = b'dian4xian4' if k.mode == 'pinyin' else b'2u04vu04'
    parsed = k.fn(parse, Z, P, S)(k.inst, text)
    assert parsed == len(text), 'the whole training input must parse'
    out = {'token': hex(token), 'parsed': parsed, 'before': unigram_of(k, token)}
    out['training'] = train_selected_top(k, '癫痫', token)
    assert out['training']['after-choose'] == out['before'], 'selection must not train'
    out['after'] = unigram_of(k, token)
    return out


def predicted_rows(k, inst, prefix, limit=12):
    """The first rows of a predicted list: (type, text).

    Generate predictions including punctuation, UTF-8 encoding prefix and
    replacing inst's candidate list. Return (rows, full native count), with
    at most limit [type, decoded text] rows and none for a nonpositive limit.
    Native success flags are unchecked; text decoding is strict UTF-8."""
    k.fn('guess_predicted_candidates_with_punctuations', B, P, S)(inst, prefix.encode())
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    for i in range(min(count.value, limit)):
        cand, kind, text = P(), I(), S()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        rows.append([kind.value, text.value.decode()])
    return rows, count.value


FACADE_TOTAL = 51051831   # the oracle data's loaded libraries, summed


def wrap_total_to_zero(k, inst):
    """Adds the delta that wraps the facade `guint32` total to zero.

    Use token (1 << 24) | 1 and return the native add flag. Assume the loaded
    context total equals FACADE_TOTAL; do not measure it or verify the result.
    FacadePhraseIndex::add_unigram_frequency at src/storage/phrase_index.h:628-634
    updates its unsigned total before delegating to SubPhraseIndex::add_unigram_frequency
    at src/storage/phrase_index.cpp:150-177 (pin 074a2219), so sub-index refusal
    need not undo the facade-total change."""
    add = k.fn('token_add_unigram_frequency', B, P, U, U)
    return add(inst, (1 << 24) | 1, (2 ** 32 - FACADE_TOTAL) % 2 ** 32)


def _add_user_phrase(k, text, readings, count=5):
    """Add UTF-8 phrase text with byte-string readings to library 7 and return add flags.

    Assert iterator creation and every addition, using the supplied signed
    count. Close the iterator before checking all add flags; mutations affect
    the cached context and are not saved here."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    assert it
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    added = [add(it, text.encode(), reading, count) for reading in readings]
    k.fn('end_add_phrases', None, P)(it)
    assert all(added)
    return added


def _user_candidate_rows(k, inst):
    """The NORMAL user-dictionary candidates of the current guess list.

    Return borrowed kind-2 pointers in list order without regenerating the list.
    The instance owns the pointers; native accessor flags are unchecked."""
    rows = []
    n = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
    for i in range(n.value):
        cand, kind = P(), I()
        k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        if kind.value == 2 and k.fn('is_user_candidate', B, P, P)(inst, cand):
            rows.append(cand)
    return rows


TOKEN_REUSE_PHRASES = (
    ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'), ('龖龖', "ta4'ta4", 'ㄊㄚˋ ㄊㄚˋ'),
    ('靐靐', "bing3'bing3", 'ㄅㄧㄥˇ ㄅㄧㄥˇ'), ('鱻鱻', "xian1'xian1", 'ㄒㄧㄢ ㄒㄧㄢ'),
    ('麤麤', "cu1'cu1", 'ㄘㄨ ㄘㄨ'), ('厵厵', "yuan2'yuan2", 'ㄩㄢˊ ㄩㄢˊ'),
)


USER_INDEX_FILES = ('user_phrase_index.bin', 'user_pinyin_index.bin')


USER_ALL = (0x0f000000, 0x07000000)


ADDON_PROMOTE_INPUTS = (('shuxue', '书学'), ('wuji', '武技'), ('wusheng', '武生'), ('wuxi', '武戏'))


def candidate_rows(k, inst):
    """The instance's full candidate list as ordered [type, string] rows.

    Use None when candidate fetching fails and replace invalid UTF-8 when
    decoding its borrowed string. Count, type and string getter flags are
    unchecked; a successful fetch is expected to supply a non-NULL string."""
    count = U()
    k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
    rows = []
    for i in range(count.value):
        cand, kind, text = P(), I(), S()
        if k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand)):
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
            rows.append([kind.value, text.value.decode('utf-8', 'replace')])
        else:
            rows.append(None)
    return rows


def guess_rows(k, inst, offset, sort):
    """One `pinyin_guess_candidates` call: the return and the full list.

    Use the supplied byte offset and sort word. Read the complete current
    candidate list even when guessing fails."""
    ret = k.fn('guess_candidates', B, P, Z, U)(inst, offset, sort)
    return {'ret': ret, 'rows': candidate_rows(k, inst)}


def zhuyin_guess_rows(k, inst, offset):
    """One `zhuyin_guess_candidates_after_cursor` call: return and list.

    Use the supplied byte offset. Read the complete current candidate list
    even when guessing returns false."""
    ret = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, offset)
    return {'ret': ret, 'rows': candidate_rows(k, inst)}


def train_once(k, ctx, parse, text, train_args):
    """Allocate an instance, parse and guess, then return one training call's flag.

    Use rank zero when train_args is nonempty. Ignore parse/guess flags and
    leave the instance live on the caller's context."""
    inst = k.fn('alloc_instance', P, P)(ctx)
    k.fn(parse, Z, P, S)(inst, text)
    k.fn('guess_sentence', B, P)(inst)
    return k.fn('train', B, P, *train_args)(inst, *((0,) if train_args else ()))


# mode -> (library file, index, parse call, text, train args)
LIBRARY_FACADES = {
    'pinyin': ('merged.bin', 4, 'parse_more_full_pinyins', b'nihao', (C.c_ubyte,)),
    'zhuyin': ('merged.bin', 4, 'parse_more_chewings', b'su3cl3', ()),
}


def log_walk(data):
    """The `.dbin` chunk's first MODIFY_HEADER totals and record counts.
    Record layout: type(4), token(4), then ADD/REMOVE len(2)+data,
    MODIFY oldlen(2)+newlen(2)+old+new, MODIFY_HEADER len(2)+old+new
    (`phrase_index_logger.h`, `next_record`).

    Skip the eight-byte framing header without validation; decode fields
    little endian. Stop on unknown types, fewer than ten remaining bytes or
    an incomplete header record. ADD/REMOVE/MODIFY counts can include a
    truncated final record; only header payload bounds are checked. Return
    None for totals if no complete header record was seen. The pin layout is
    PhraseIndexLogger::next_record at src/storage/phrase_index_logger.h:158-211
    at 074a2219."""
    payload = data[8:]
    pos = 0
    header = None
    counts = {'add': 0, 'remove': 0, 'modify': 0, 'header': 0}
    while pos + 10 <= len(payload):
        typ = int.from_bytes(payload[pos:pos + 4], 'little')
        if typ == 4:
            ln = int.from_bytes(payload[pos + 8:pos + 10], 'little')
            if pos + 10 + 2 * ln > len(payload):
                break
            if header is None:
                header = [int.from_bytes(payload[pos + 10:pos + 14], 'little'),
                          int.from_bytes(payload[pos + 14:pos + 18], 'little')]
            counts['header'] += 1
            pos += 10 + 2 * ln
        elif typ in (1, 2):
            ln = int.from_bytes(payload[pos + 8:pos + 10], 'little')
            counts['add' if typ == 1 else 'remove'] += 1
            pos += 10 + ln
        elif typ == 3:
            old = int.from_bytes(payload[pos + 8:pos + 10], 'little')
            new = int.from_bytes(payload[pos + 10:pos + 12], 'little')
            counts['modify'] += 1
            pos += 12 + old + new
        else:
            break
    return header, counts


def remember_export(k, library, phrase, adjacent=False):
    """The export rows of `phrase` in `library`. `adjacent=True` stops at the
    first row of another phrase after the phrase's own: one item's readings
    are consecutive, so for a text held by one token this is the same list
    without walking the rest of a 93,000-item library in Python.

    Rows contain [flag, phrase, reading, count]. Release the iterator and
    returned strings on the normal path; return 'NO EXPORT API' for zhuyin.
    If one text belongs to noncontiguous tokens, adjacent can omit later matches."""
    if k.mode == 'zhuyin':
        return 'NO EXPORT API'
    it=k.fn('begin_get_phrases',P,P,U)(k.ctx,library)
    rows=[]
    while k.fn('iterator_has_next_phrase',B,P)(it):
        ph,py,n=P(),P(),I()
        ret=k.fn('iterator_get_next_phrase',B,P,C.POINTER(P),C.POINTER(P),C.POINTER(I))(it,C.byref(ph),C.byref(py),C.byref(n))
        text,reading=k.text(ph.value),k.text(py.value)
        if text==phrase: rows.append([ret,text,reading,n.value])
        elif adjacent and rows: break
    k.fn('end_get_phrases',None,P)(it)
    return rows


def transient_files(root):
    """Snapshot recursive file entries as relative-path keys with mode bits and raw bytes.

    Follow is_file/stat/read_bytes behavior for symlinks; omit directories and
    let filesystem errors propagate."""
    return {str(p.relative_to(root)): (p.stat().st_mode, p.read_bytes())
            for p in Path(root).rglob('*') if p.is_file()}


# #697, row 58: capture the entire list, including row types and order.
def row58_rows(k, prefix):
    """Guess predictions for a Unicode prefix and return the flag and ordered [type, text] rows.

    Assert count and candidate getters, decode borrowed strings as strict UTF-8,
    and read the list even if the prediction call returns false."""
    inst = k.inst
    ret = k.fn('guess_predicted_candidates', B, P, S)(inst, prefix.encode())
    n = U()
    assert k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
    rows = []
    for i in range(n.value):
        cand, kind, text = P(), I(), S()
        assert k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
        assert k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(kind))
        assert k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
        rows.append([kind.value, text.value.decode()])
    return dict(ret=ret, rows=rows)


def token_phrase_of(k, token):
    """Return a token's phrase getter flag, length and decoded owned text.

    Initialize outputs to zero, ignore failure for decoding, and free non-NULL text."""
    length, text = U(), P()
    ret = k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
        k.inst, token, C.byref(length), C.byref(text))
    return [ret, length.value, k.text(text.value)]


def _chunk(payload):
    """A `MemoryChunk` image: the length and checksum header words
    (`memory_chunk.h:543-547`) then `payload`."""
    checksum = 0
    aligned = len(payload) & ~0x3
    for i in range(0, aligned, 4):
        checksum ^= int.from_bytes(payload[i:i + 4], 'little')
    for shift, byte in enumerate(payload[aligned:]):
        checksum ^= byte << (8 * shift)
    return (len(payload).to_bytes(4, 'little') +
            (checksum & 0xFFFFFFFF).to_bytes(4, 'little') + payload)


def _modify_header(token, old_total, new_total):
    """One `LOG_MODIFY_HEADER` record: the type (4), the token, a `u16`
    length, then the old and new total runs."""
    return ((4).to_bytes(4, 'little') + token.to_bytes(4, 'little') +
            (4).to_bytes(2, 'little') + old_total.to_bytes(4, 'little') +
            new_total.to_bytes(4, 'little'))


def _write_log(k, payload, name='gb_char.dbin'):
    """Overwrite the named user log with payload framed by the chunk length and checksum."""
    Path(k.user, name).write_bytes(_chunk(payload))


def _ucs4_key(text):
    """Encode each Python character as an unsigned four-byte little-endian phrase-table key unit.

    Add no terminator and perform no scalar validation. This is the key layout
    used by PhraseLargeTable3::search in src/storage/phrase_large_table3_bdb.cpp:180-192
    at pin 074a2219."""
    return b''.join(ord(character).to_bytes(4, 'little') for character in text)


def phrase_table_reopen(k):
    """Free any cached instance, finalize the cached context and assert lazy reopening succeeds.

    Clear cached pointers before reopening; pass the existing context pointer
    to fini even when it is NULL."""
    if k._inst:
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    assert k.ctx, 'phrase-table reopen failed'


def d18_export_phrases(k):
    """Export all library-seven rows as [flag, phrase, reading, count] in iterator order.

    Decode/free returned strings and end the iterator on normal completion;
    getter failure flags do not stop the loop."""
    it = k.fn('begin_get_phrases', P, P, U)(k.ctx, 7)
    rows = []
    while k.fn('iterator_has_next_phrase', B, P)(it):
        phrase, reading, count = P(), P(), I(UNTOUCHED)
        ret = k.fn('iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))(
            it, C.byref(phrase), C.byref(reading), C.byref(count))
        rows.append([ret, k.text(phrase.value), k.text(reading.value), count.value])
    k.fn('end_get_phrases', None, P)(it)
    return rows


def d18_bigram_db(k, path, rows=None, read_keys=(), allow_missing=False):
    """Independent raw SingleGram fixtures through each installed C DBM API.

    `rows=None` reads an existing fixture; all returned buffers are copied
    before closing the DBM. No installed system/user data is ever written.
    """
    import ctypes.util
    conf = Path(k.data, 'table.conf').read_text()
    if 'database format:BerkeleyDB' in conf or 'database format: BerkeleyDB' in conf:
        return _craft_bdb(str(path), rows, 2, read_keys=read_keys, allow_missing=allow_missing)
    writable = rows is not None
    if writable and Path(path).exists():
        Path(path).unlink()
    if 'KyotoCabinet' in conf:
        db = C.CDLL(ctypes.util.find_library('kyotocabinet'))
        db.kcdbnew.restype = P
        db.kcdbopen.argtypes, db.kcdbopen.restype = [P, S, U], I
        db.kcdbset.argtypes, db.kcdbset.restype = [P, S, Z, S, Z], I
        db.kcdbget.argtypes, db.kcdbget.restype = [P, S, Z, C.POINTER(Z)], P
        db.kcdbloadsnap.argtypes, db.kcdbloadsnap.restype = [P, S], I
        db.kcdbdumpsnap.argtypes, db.kcdbdumpsnap.restype = [P, S], I
        db.kcdbclose.argtypes, db.kcdbclose.restype = [P], I
        db.kcdbdel.argtypes, db.kcfree.argtypes = [P], [P]
        handle = db.kcdbnew()
        # The KC system gram is a HashDB; the user gram is a StashDB
        # snapshot (`074a2219 ngram_kyotodb.cpp:54-90`), not a HashDB file.
        snapshot = Path(path).name == 'user_bigram.db'
        spec = b'-' if snapshot else (str(path) + '#type=kch').encode()
        assert db.kcdbopen(handle, spec, 6 if writable or snapshot else 1)
        try:
            if snapshot and not writable:
                assert db.kcdbloadsnap(handle, str(path).encode())
            for key, value in rows or ():
                assert db.kcdbset(handle, key, len(key), value, len(value))
            if snapshot and writable:
                assert db.kcdbdumpsnap(handle, str(path).encode())
            values = []
            for key in read_keys:
                size = Z()
                value = db.kcdbget(handle, key, len(key), C.byref(size))
                if not value and allow_missing:
                    values.append(None)
                    continue
                assert value
                values.append(C.string_at(value, size.value))
                db.kcfree(value)
            return values
        finally:
            assert db.kcdbclose(handle)
            db.kcdbdel(handle)
    assert 'Tkrzw' in conf, 'unknown fixture backend'
    db = C.CDLL(ctypes.util.find_library('tkrzw'))
    db.tkrzw_dbm_open.argtypes, db.tkrzw_dbm_open.restype = [S, B, S], P
    db.tkrzw_dbm_set.argtypes, db.tkrzw_dbm_set.restype = [P, S, I, S, I, B], B
    db.tkrzw_dbm_get.argtypes, db.tkrzw_dbm_get.restype = [P, S, I, C.POINTER(I)], P
    db.tkrzw_dbm_close.argtypes, db.tkrzw_dbm_close.restype = [P], B
    libc = C.CDLL(None)
    libc.free.argtypes = [P]
    handle = db.tkrzw_dbm_open(str(path).encode(), writable, b'dbm=HashDBM')
    assert handle
    try:
        for key, value in rows or ():
            assert db.tkrzw_dbm_set(handle, key, len(key), value, len(value), True)
        values = []
        for key in read_keys:
            size = I()
            value = db.tkrzw_dbm_get(handle, key, len(key), C.byref(size))
            if not value and allow_missing:
                values.append(None)
                continue
            assert value
            values.append(C.string_at(value, size.value))
            libc.free(value)
        return values
    finally:
        assert db.tkrzw_dbm_close(handle)


def d18_gram(total, token, count):
    """Encode one SingleGram total/token/count triple as unsigned 32-bit little-endian words.

    Follow SingleGram::get_total_freq and ::retrieve_all in src/storage/ngram.cpp:48-50,133-151
    at pin 074a2219; out-of-range integers fail through to_bytes."""
    return b''.join(n.to_bytes(4, 'little') for n in (total, token, count))


def _gram_bytes(total, items):
    """Encode a stored SingleGram total and ordered token/count pairs as unsigned 32-bit words.

    Use little endian and retain duplicates and supplied order without checking
    the total against the items. This is the layout read by SingleGram::get_total_freq
    and ::retrieve_all in src/storage/ngram.cpp:48-50,133-151 at pin 074a2219. Integer range
    errors propagate from to_bytes."""
    return total.to_bytes(4, 'little') + b''.join(
        t.to_bytes(4, 'little') + c.to_bytes(4, 'little') for t, c in items)


def _gram_items(raw):
    """Decode a raw SingleGram into [total, [[token, count], ...]], preserving stored order.

    Return None for None; read little-endian words, tolerate a short total and
    ignore any trailing partial item. Follow SingleGram::get_total_freq and
    ::retrieve_all in src/storage/ngram.cpp:48-50,133-151 at pin 074a2219 without validating
    framing, sorting or consistency."""
    if raw is None:
        return None
    return [int.from_bytes(raw[:4], 'little'),
            [[int.from_bytes(raw[i:i + 4], 'little'), int.from_bytes(raw[i + 4:i + 8], 'little')]
             for i in range(4, len(raw) - len(raw[4:]) % 8, 8)]]


def _plant_user_grams(k, phrases, grams):
    """Add `phrases` (text, pinyin, zhuyin, count) as user phrases, save and
    close, then replace user_bigram.db with `grams(tokens)` — a list of
    (predecessor, raw value) — in this cell's own format. Answers the
    phrases' user tokens and the planted keys."""
    it = k.fn('begin_add_phrases', P, P, U)(k.ctx, 7)
    add = k.fn('iterator_add_phrase', B, P, S, S, I)
    for text, pinyin, zhuyin, count in phrases:
        assert add(it, text.encode(), (pinyin if k.mode == 'pinyin' else zhuyin).encode(), count)
    k.fn('end_add_phrases', None, P)(it)
    users = [next(t for t in tokens_of(k, text) if t >> 24 == 7) for text, _, _, _ in phrases]
    system = {text: tokens_of(k, text)[0] for text in ('你', '好', '号', '我')}
    assert k.fn('save', B, P)(k.ctx)
    k.fn('free_instance', None, P)(k._inst)
    k._inst = None
    k.fn('fini', None, P)(k._ctx)
    k._ctx = None
    rows = [(p.to_bytes(4, 'little'), value) for p, value in grams(system, users)]
    d18_bigram_db(k, Path(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    return system, users, [key for key, _ in rows]


_DALONG = ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ', 5)


def _stored_library_total(k, name):
    """A system library chunk's stored total, after the MemoryChunk header."""
    return int.from_bytes(Path(k.data, name).read_bytes()[8:12], 'little')


def _facade_total_out(k):
    """Return ni3'hao3 sentence observations, up to eight leading candidates and 你好 segmentation.

    Use the cached instance for candidate guessing with sort flags 0x1e;
    this observes ranking rather than returning a numeric facade total."""
    out = _guess_out(k, b"ni3'hao3")
    out['candidates'] = guess_rows(k, k.inst, 0, 0x1e)['rows'][:8]
    out['segment'] = _segment_out(k, ('你好',))
    return out


def _reload_pron_keys(k, token):
    """Read a token's pronunciation count and hex-encoded two-byte key arrays.

    Return [count-call success, count, ...readings], skipping the reading calls
    on failure. Each reading allocates a GArray that this helper does not free.
    """
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    count = U(UNTOUCHED)
    ok = k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(k.inst, token, C.byref(count))
    out = [ok, count.value]
    for nth in range(count.value if ok else 0):
        keys = new(0, 0, 2)
        k.fn('token_get_nth_pronunciation', B, P, U, U, P)(k.inst, token, nth, keys)
        view = C.cast(keys, C.POINTER(Arr)).contents
        out.append(C.string_at(view.data, view.len * 2).hex() if view.len else '')
    return out


_PAST_RANGES = 0x0fffffff


def _replace_user_bigram(k, rows):
    """Close the context and replace user_bigram.db with `rows`, a list of
    (predecessor, raw value), in this cell's own format."""
    if k._inst is not None:
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
    k.fn('fini', None, P)(k.ctx)
    k._ctx = None
    d18_bigram_db(k, Path(k.user, 'user_bigram.db'),
                  [(p.to_bytes(4, 'little'), value) for p, value in rows])
    _write_user_conf(k)


def _replace_system_start_row(k, value):
    """A copy of the system tables whose bigram.db holds `value` under the
    sentence start."""
    if k._inst is not None:
        k.fn('free_instance', None, P)(k._inst)
        k._inst = None
    k.fn('fini', None, P)(k.ctx)
    k._ctx = None
    copied = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
    shutil.copytree(k.data, copied, dirs_exist_ok=True)
    k.data = copied
    d18_bigram_db(k, Path(copied, 'bigram.db'), [(b'\x01\x00\x00\x00', value)])
    _write_user_conf(k)


def _guess_out(k, text=b'nihao'):
    """Parse the input and return guess status plus all final-step sentences.

    For zhuyin, first require the full-pinyin option word to be accepted; use the
    cached instance for both parse and guess.
    """
    if k.mode == 'zhuyin':
        # The zhuyin facade parses full pinyin under this option word, as
        # the d18 merge cases do.
        assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    guessed = k.fn('guess_sentence', B, P)(inst)
    return dict(parsed=parsed, guessed=guessed, sentences=final_step_sentences(k, inst))


def _dynamic_adjust_out(k, text=b'nihao'):
    """Enable dynamic adjustment and return parse length with facade-specific rows.

    Require option word 0x38a to succeed and guess from offset zero on the
    cached instance.
    """
    assert k.fn('set_options', B, P, U)(k.ctx, 0x18a | (1 << 9))
    inst = k.inst
    parsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
    rows = guess_rows(k, inst, 0, 0x1e) if k.mode == 'pinyin' else zhuyin_guess_rows(k, inst, 0)
    return dict(parsed=parsed, **rows)


def _segment_out(k, texts):
    """Segment each UTF-8 text and return its status and resulting token list.

    Reuse the cached instance. An untouched count produces an empty list;
    token output slots retain the sentinel if their getters do not write them.
    """
    segment = k.fn('phrase_segment', B, P, S)
    n_phrase = k.fn('get_n_phrase', B, P, C.POINTER(U))
    token = k.fn('get_phrase_token', B, P, U, C.POINTER(U))
    out = {}
    for text in texts:
        ret = segment(k.inst, text.encode())
        count = U(UNTOUCHED)
        n_phrase(k.inst, C.byref(count))
        tokens = []
        for index in range(count.value if count.value != UNTOUCHED else 0):
            slot = U(UNTOUCHED)
            token(k.inst, index, C.byref(slot))
            tokens.append(slot.value)
        out[text] = [ret, tokens]
    return out


PHRASE_TEXT_WORDS = dict(valid=0x4e01, surrogate=0xd800, **{'above-unicode': 0x110000}, nul=0)


def phrase_text_bytes(k, ptr, *, owned=True):
    """Lossless text observation: never replace malformed UTF-8 with U+FFFD."""
    if ptr == UNTOUCHED:
        return 'untouched'
    if not ptr:
        return None
    value = C.string_at(ptr).hex()
    if owned:
        k.glib.g_free(ptr)
    return value


def glib_utf8(words):
    """`g_unichar_to_utf8`'s one- to six-byte forms (gutf8.c:546-596)."""
    out = bytearray()
    for word in words:
        for limit, first, length in ((0x80, 0, 1), (0x800, 0xc0, 2), (0x10000, 0xe0, 3),
                                     (0x200000, 0xf0, 4), (0x4000000, 0xf8, 5), (1 << 32, 0xfc, 6)):
            if word < limit:
                break
        tail = []
        for _ in range(length - 1):
            tail.append(0x80 | (word & 0x3f))
            word >>= 6
        out += bytes([first | word] + tail[::-1])
    return bytes(out)


PREFIX_WORDS = {'valid': 0x4e01, 'surrogate': 0xd800, 'above-unicode': 0x110000,
                'five-byte': 0x200000, 'six-byte': 0x4000000}
