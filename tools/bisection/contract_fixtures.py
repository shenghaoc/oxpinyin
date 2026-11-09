"""User-profile and DBM-container fixtures the cross-open cases share.

Builders for the files a case plants before it opens a facade (a conforming
user.conf, a user table container in each backend's own format) and a reader
for what a run left behind. Nothing here reads a per-run state: the cell,
which the container reader needs, is a parameter.
"""
import ctypes as C
import os
import subprocess

from contract_abi import B, I, P, S, U, Z


def _write_user_conf(k):
    """A conforming `user.conf` for the cell's system tables.

    The pin writes one at `pinyin_save` / `zhuyin_save` only, but
    `check_format` (`pinyin.cpp:172`, `zhuyin.cpp:126`) wipes the user
    tables when it is missing or stale. `pinyin_init`'s check_format
    rewrites it on every open (so the pinyin cases survive without this);
    `zhuyin_init`'s does not, so a crafted zhuyin index needs the marker
    present before the reopen or it is unlinked.

    Overwrite user.conf with binary/model/database fields read from table.conf
    and an open counter of zero. Missing fields raise KeyError after the
    output file has been opened."""
    keys = ('binary format version:', 'model data version:', 'database format:')
    values = {}
    with open(os.path.join(k.data, 'table.conf')) as table_conf:
        for line in table_conf:
            for key in keys:
                if line.startswith(key):
                    values[key] = line[len(key):].strip()
    with open(os.path.join(k.user, 'user.conf'), 'w') as user_conf:
        for key in keys:
            user_conf.write(key + values[key] + '\n')
        user_conf.write('open counter:0\n')





def _craft_kc(path, rows, keep=False, *, system_index=False):
    """A Kyoto Cabinet snapshot of the records (`BasicDB::dump_snapshot`),
    written through the C API from an in-memory database. `keep=True`
    instead adds to the native TreeDB the pin attaches as a system index.
    `system_index=True` creates that native format even with `keep=False`.

    Unlink an existing path unless keep is true. Assert native operations,
    then close/delete the handle on success; failures have no finally-based
    cleanup path."""
    kc = C.CDLL('libkyotocabinet.so.16')
    kc.kcdbnew.restype = C.c_void_p
    kc.kcdbdel.argtypes = [C.c_void_p]
    kc.kcdbopen.argtypes = [C.c_void_p, C.c_char_p, C.c_uint32]
    kc.kcdbset.argtypes = [C.c_void_p, C.c_char_p, C.c_size_t, C.c_char_p, C.c_size_t]
    kc.kcdbdumpsnap.argtypes = [C.c_void_p, C.c_char_p]
    kc.kcdbclose.argtypes = [C.c_void_p]
    if os.path.exists(path) and not keep:
        os.unlink(path)
    db = kc.kcdbnew()
    assert db
    # Existing system TreeDB: KCOWRITER, explicit type because its suffix
    # is .bin. Fresh system TreeDB and user memory DB: also KCOCREATE.
    native = keep or system_index
    target = (path + '#type=kct').encode() if native else b'-'
    mode = (1 << 1) if keep else (1 << 1) | (1 << 2)
    assert kc.kcdbopen(db, target, mode)
    for key, value in rows:
        assert kc.kcdbset(db, key, len(key), value, len(value))
    if not native:
        assert kc.kcdbdumpsnap(db, path.encode())
    assert kc.kcdbclose(db)
    kc.kcdbdel(db)


FACADE = None  # the facade library being verified; the worker sets it before any case runs


def tkrzw_handle(library=None):
    """The handle tkrzw's C API resolves through.

    LIBRARY is a tkrzw path certified against the worker's anchor object. Without it the verified
    facade's own handle is used: dlsym searches the library and the libraries it depends on, so the
    tkrzw it links is the one used, on every platform."""
    target = library if library is not None else FACADE
    if target is None:
        raise RuntimeError('no facade library to resolve tkrzw through')
    return C.CDLL(str(target))


def _craft_tkrzw(path, rows, kind, keep=False):
    """A tkrzw `kind` file (`TreeDBM` for the chewing and phrase tables,
    `HashDBM` for the bigram, as each pin `load_db` opens it) of the records,
    written through the C API with the default tuning the pin's `save_db`
    uses. `keep=True` adds to an existing system index without unlinking it.

    Unlink an existing file unless keep is true, overwrite matching records,
    and assert open, set and close success. No failure cleanup is installed.
    The load_db/save_db formats are ChewingLargeTable2 at
    src/storage/chewing_large_table2_tkrzwdb.cpp:92-130, PhraseLargeTable3 at
    src/storage/phrase_large_table3_tkrzwdb.cpp:83-121 and Bigram at
    src/storage/ngram_tkrzwdb.cpp:48-83 (pin 074a2219)."""
    tk = tkrzw_handle()
    tk.tkrzw_dbm_open.restype = C.c_void_p
    tk.tkrzw_dbm_open.argtypes = [C.c_char_p, C.c_bool, C.c_char_p]
    tk.tkrzw_dbm_set.restype = C.c_bool
    tk.tkrzw_dbm_set.argtypes = [C.c_void_p, C.c_char_p, C.c_int32, C.c_char_p, C.c_int32, C.c_bool]
    tk.tkrzw_dbm_close.restype = C.c_bool
    tk.tkrzw_dbm_close.argtypes = [C.c_void_p]
    if os.path.exists(path) and not keep:
        os.unlink(path)
    db = tk.tkrzw_dbm_open(path.encode(), True, ('dbm=' + kind).encode())
    assert db
    for key, value in rows:
        assert tk.tkrzw_dbm_set(db, key, len(key), value, len(value), True)
    assert tk.tkrzw_dbm_close(db)


def _craft_bdb(path, rows, dbtype, keep=False, read_keys=(), allow_missing=False):
    """A Berkeley DB `dbtype` container; `keep=True` adds to existing rows.

    rows=None opens read-only; other rows values enable creation and unlink
    an existing file unless keep is true. Store byte pairs, then return copied
    values for read_keys in order. With allow_missing, any nonzero get status
    becomes None, not just not-found; otherwise assert success. Assert create,
    open, put and close results. Handles close only on the success path, and
    libdb must expose the dynamically resolved __db_*_pp entry points."""
    import ctypes.util

    class DBT(C.Structure):
        """Mirror the native DBT record passed to Berkeley DB's get and put entry points.

        Include app_data before flags so pointer alignment matches the C layout;
        data points to buffers whose lifetime is managed separately by the caller."""
        # 40 bytes on x86_64: the `app_data` pointer between `doff` and
        # `flags` (omitting it misaligns `flags`, and `__db_put_pp` then
        # rejects the flag argument).
        _fields_ = [
            ('data', C.c_void_p),
            ('size', C.c_uint),
            ('ulen', C.c_uint),
            ('dlen', C.c_uint),
            ('doff', C.c_uint),
            ('app_data', C.c_void_p),
            ('flags', C.c_uint),
        ]

    db = C.CDLL(ctypes.util.find_library('db'))
    db.db_create.argtypes = [C.POINTER(C.c_void_p), C.c_void_p, C.c_uint]
    db.db_create.restype = C.c_int
    db.__db_open_pp.argtypes = [C.c_void_p, C.c_void_p, C.c_char_p, C.c_char_p,
                                C.c_int, C.c_uint, C.c_int]
    db.__db_open_pp.restype = C.c_int
    db.__db_put_pp.argtypes = [C.c_void_p, C.c_void_p, C.POINTER(DBT), C.POINTER(DBT), C.c_uint]
    db.__db_put_pp.restype = C.c_int
    db.__db_close_pp.argtypes = [C.c_void_p, C.c_uint]
    db.__db_close_pp.restype = C.c_int
    db.__db_get_pp.argtypes = [P, P, C.POINTER(DBT), C.POINTER(DBT), U]
    db.__db_get_pp.restype = I
    if rows is not None and os.path.exists(path) and not keep:
        os.unlink(path)
    handle = C.c_void_p()
    assert db.db_create(C.byref(handle), None, 0) == 0
    # DB_CREATE == 0x1; the pin opens its tables the same way
    # (`chewing_large_table2_bdb.cpp:90`, `ngram_bdb.cpp:56`).
    flags = 0x1 if rows is not None else 0x400  # DB_CREATE / DB_RDONLY
    assert db.__db_open_pp(handle, None, path.encode(), None, dbtype, flags, 0o600) == 0
    live = []
    for key, value in rows or ():
        kbuf = C.create_string_buffer(key)
        vbuf = C.create_string_buffer(value)
        live += [kbuf, vbuf]
        kd = DBT(C.cast(kbuf, C.c_void_p), len(key), 0, 0, 0, None, 0)
        vd = DBT(C.cast(vbuf, C.c_void_p), len(value), 0, 0, 0, None, 0)
        assert db.__db_put_pp(handle, None, C.byref(kd), C.byref(vd), 0) == 0
    values = []
    for key in read_keys:
        kbuf = C.create_string_buffer(key)
        kd = DBT(C.cast(kbuf, P), len(key), 0, 0, 0, None, 0)
        vd = DBT()
        status = db.__db_get_pp(handle, None, C.byref(kd), C.byref(vd), 0)
        if status != 0 and allow_missing:
            values.append(None)
            continue
        assert status == 0
        values.append(C.string_at(vd.data, vd.size))
    assert db.__db_close_pp(handle, 0) == 0
    del live
    return values


def index_file_rows(path, *, cell, kind='tree', allow_empty=False, library=None):
    """Every key/value of a user container, sorted, including empty values.

    The default remains the user-index reader. Teardown also selects `hash`
    for user_bigram.db: KC stores all three files as snapshots; tkrzw saves
    indexes as TreeDBM and the bigram as HashDBM (ngram_tkrzwdb.cpp:48-81 at
    074a2219). BDB keeps db_dump's type detection and historical output.
    `library` is the exact tkrzw path certified against the worker's anchor
    object, avoiding a second, possibly different bare-name resolution.
    `cell` names the backend, bdb, kc or tkrzw, whose container `path` is.

    Return [key hex, value hex] rows. kind must be tree or hash, otherwise
    raise ValueError. KC reads snapshots through an in-memory database; tkrzw
    opens the selected DBM kind read-only. Checked DB/cursor failures raise
    ValueError; opened readers close in finally blocks. Only tkrzw uses allow_empty: an initially
    empty iterator is an error unless it is true; KC and BDB already permit
    empty containers. BDB invokes db5.3_dump; process, decoding and dump-parsing
    failures propagate."""
    if kind not in ('tree', 'hash'):
        raise ValueError('unknown profile container kind: ' + kind)
    rows = []
    if cell == 'kc':
        kc = C.CDLL('libkyotocabinet.so.16')
        kc.kcdbnew.restype = P
        kc.kcdbdel.argtypes = [P]
        kc.kcdbopen.argtypes = [P, S, C.c_uint32]
        kc.kcdbloadsnap.argtypes = [P, S]
        kc.kcdbcursor.restype, kc.kcdbcursor.argtypes = P, [P]
        kc.kccurjump.argtypes = [P]
        kc.kccurget.restype = P
        kc.kccurget.argtypes = [P, C.POINTER(Z), C.POINTER(P), C.POINTER(Z), I]
        kc.kccurecode.argtypes, kc.kccurecode.restype = [P], I
        kc.kccurdel.argtypes = [P]
        kc.kcfree.argtypes = [P]
        kc.kcdbclose.argtypes = [P]
        db = kc.kcdbnew()
        if not db:
            raise ValueError('KC profile allocation failed: ' + str(path))
        opened, cur = False, None
        try:
            if not kc.kcdbopen(db, b'+', (1 << 1) | (1 << 2)):
                raise ValueError('KC profile memory open failed: ' + str(path))
            opened = True
            if not kc.kcdbloadsnap(db, str(path).encode()):
                raise ValueError('KC profile snapshot load failed: ' + str(path))
            cur = kc.kcdbcursor(db)
            if not cur:
                raise ValueError('KC profile cursor allocation failed: ' + str(path))
            if kc.kccurjump(cur):
                while True:
                    ksiz, vbuf, vsiz = Z(), P(), Z()
                    kbuf = kc.kccurget(cur, C.byref(ksiz), C.byref(vbuf), C.byref(vsiz), 1)
                    if not kbuf:
                        if kc.kccurecode(cur) != 7:  # KCENOREC, not a partial walk.
                            raise ValueError('KC profile cursor read failed: ' + str(path))
                        break
                    try:
                        rows.append([C.string_at(kbuf, ksiz.value).hex(), C.string_at(vbuf, vsiz.value).hex()])
                    finally:
                        kc.kcfree(kbuf)
            elif kc.kccurecode(cur) != 7:
                raise ValueError('KC profile cursor first failed: ' + str(path))
        finally:
            if cur:
                kc.kccurdel(cur)
            try:
                if opened and not kc.kcdbclose(db):
                    raise ValueError('KC profile close failed: ' + str(path))
            finally:
                kc.kcdbdel(db)
    elif cell == 'tkrzw':
        tk = tkrzw_handle(library)
        libc = C.CDLL(None)
        libc.free.argtypes = [P]
        tk.tkrzw_dbm_open.restype, tk.tkrzw_dbm_open.argtypes = P, [S, B, S]
        tk.tkrzw_dbm_make_iterator.restype, tk.tkrzw_dbm_make_iterator.argtypes = P, [P]
        tk.tkrzw_dbm_iter_first.restype, tk.tkrzw_dbm_iter_first.argtypes = B, [P]
        tk.tkrzw_dbm_iter_get.restype = B
        tk.tkrzw_dbm_iter_get.argtypes = [P, C.POINTER(P), C.POINTER(C.c_int32), C.POINTER(P), C.POINTER(C.c_int32)]
        tk.tkrzw_dbm_iter_next.restype, tk.tkrzw_dbm_iter_next.argtypes = B, [P]
        tk.tkrzw_dbm_iter_free.argtypes = [P]
        tk.tkrzw_dbm_close.restype, tk.tkrzw_dbm_close.argtypes = B, [P]
        tk.tkrzw_get_last_status_code.restype = I
        tk.tkrzw_get_last_status_code.argtypes = []
        params = b'dbm=HashDBM' if kind == 'hash' else b'dbm=TreeDBM'
        db = tk.tkrzw_dbm_open(str(path).encode(), False, params)
        if not db:
            raise ValueError('tkrzw profile read-only open failed: ' + str(path))
        it = None
        try:
            it = tk.tkrzw_dbm_make_iterator(db)
            if not it:
                raise ValueError('tkrzw profile iterator allocation failed: ' + str(path))
            if not tk.tkrzw_dbm_iter_first(it):
                if not allow_empty or tk.tkrzw_get_last_status_code() != 7:
                    raise ValueError('tkrzw profile iterator first failed: ' + str(path))
                return []
            while True:
                kptr, ksiz, vptr, vsiz = P(), C.c_int32(), P(), C.c_int32()
                if not tk.tkrzw_dbm_iter_get(it, C.byref(kptr), C.byref(ksiz), C.byref(vptr), C.byref(vsiz)):
                    if tk.tkrzw_get_last_status_code() != 7:
                        raise ValueError('tkrzw profile iterator read failed: ' + str(path))
                    break
                try:
                    rows.append([C.string_at(kptr, ksiz.value).hex(), C.string_at(vptr, vsiz.value).hex()])
                finally:
                    libc.free(kptr)
                    libc.free(vptr)
                if not tk.tkrzw_dbm_iter_next(it):
                    if tk.tkrzw_get_last_status_code() != 7:
                        raise ValueError('tkrzw profile iterator next failed: ' + str(path))
                    break
        finally:
            if it:
                tk.tkrzw_dbm_iter_free(it)
            if not tk.tkrzw_dbm_close(db):
                raise ValueError('tkrzw profile close failed: ' + str(path))
    else:
        dump = subprocess.run(['db5.3_dump', str(path)], check=True, capture_output=True).stdout.decode()
        body = dump.split('DATA=END')[0].split('HEADER=END\n')[1].split('\n')[:-1]
        rows = [[body[i].strip(), body[i + 1].strip()] for i in range(0, len(body), 2)]
    return sorted(rows)
