"""The token and gram helpers the local cross-open cases share (#1175).

These are the definitions the surviving cases reach: the token phrases, the
candidate reader, the raw SingleGram encoders and the user-bigram replacer.
"""

import ctypes as C
from pathlib import Path

from contract_abi import B, I, P, S, U, Z
from contract_fixtures import _craft_bdb, _write_user_conf

TOKEN_REUSE_PHRASES = (
    ('龘龘', "da2'da2", 'ㄉㄚˊ ㄉㄚˊ'), ('龖龖', "ta4'ta4", 'ㄊㄚˋ ㄊㄚˋ'),
    ('靐靐', "bing3'bing3", 'ㄅㄧㄥˇ ㄅㄧㄥˇ'), ('鱻鱻', "xian1'xian1", 'ㄒㄧㄢ ㄒㄧㄢ'),
    ('麤麤', "cu1'cu1", 'ㄘㄨ ㄘㄨ'), ('厵厵', "yuan2'yuan2", 'ㄩㄢˊ ㄩㄢˊ'),
)

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

