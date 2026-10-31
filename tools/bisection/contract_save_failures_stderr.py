"""Save failures and stderr cases (inventory row 59, #1175).
"""
import ctypes as C
import functools
import os
from pathlib import Path
import resource
import shutil
import signal
import tempfile

from contract_abi import Arr, B, I, P, S, U, Z
from contract_case_helpers import TOKEN_REUSE_PHRASES, d18_gram


# Later #929 boundaries: source-derived at 074a2219 src/pinyin.cpp:922-1146,
# 1194-1200, src/zhuyin.cpp:547-698,741-757, and src/storage/table_info.cpp:377-396.
# GLib 2.89.4 glib/gstdio.h:60-82 makes rename/fsync direct Unix calls.
def save_929_physical_files(directory):
    """Observe every regular file's complete bytes, without normalization.

    Hex bytes plus length compare physical files without masks, including chunk
    checksums, DBM metadata, marker contents and any retained staging files.
    Nonregular entries are an error rather than an omitted observation.
    """
    files = {}
    for path in sorted(Path(directory).iterdir()):
        if not path.is_file() or path.is_symlink():
            raise ValueError('nonregular saved profile entry: ' + path.name)
        raw = path.read_bytes()
        files[path.name] = dict(size=len(raw), bytes=raw.hex())
    return files


def save_929_marker_interrupt(path, operation):
    """Kill a child at the marker's write-open, notified by a Linux file lease.

    The read lease prevents the write-open from completing. SIGIO is a kernel
    lease-break notification, not a timed filesystem poll. Kill and reap the
    child before releasing the lease; the bounded signal wait fails if the
    intended boundary is never reached. No production hook or preload shim.
    """
    import fcntl
    old_mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGIO})
    fd = os.open(path, os.O_RDONLY)
    pid = None
    try:
        fcntl.fcntl(fd, fcntl.F_SETLEASE, fcntl.F_RDLCK)
        pid = os.fork()
        if pid == 0:
            os.close(fd)
            try:
                operation()
            finally:
                os._exit(74)  # reaching this means the boundary was missed
        notification = signal.sigtimedwait({signal.SIGIO}, 10)
        assert notification is not None, 'marker write-open did not request a lease break'
        os.kill(pid, signal.SIGKILL)
        _, status = os.waitpid(pid, 0)
        pid = None
        assert os.waitstatus_to_exitcode(status) == -signal.SIGKILL
        return 'marker-open'
    finally:
        if pid is not None and pid > 0:
            os.kill(pid, signal.SIGKILL)
            os.waitpid(pid, 0)
        fcntl.fcntl(fd, fcntl.F_SETLEASE, fcntl.F_UNLCK)
        os.close(fd)
        signal.pthread_sigmask(signal.SIG_SETMASK, old_mask)


def register(case, *, CASES, crafted_bigram_context, index_file_rows, token_reuse_sequence):
    """Register these cases at their original driver position."""
    # batch2 group 15: the pin's stderr and the save report (PR 15, #545)
    def save_after_training(k, remove_dir, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
        """Train a guessed sentence and return training and save flags.

        Use the specified parser and training signature, passing rank zero when
        train_args is nonempty. If remove_dir is true, delete the user directory
        before saving. Leave the context and allocated instance live."""
        ctx = k.ctx
        inst = k.alloc()
        k.fn(parse, Z, P, S)(inst, text)
        k.fn('guess_sentence', B, P)(inst)
        out = {'train': k.fn('train', B, P, *train_args)(inst, *((0,) if train_args else ()))}
        if remove_dir:
            shutil.rmtree(k.user)
        out['save'] = k.fn('save', B, P)(ctx)
        return out


    def blocked_save(k, blocked, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
        """A second, modified save with one path of the file set blocked by a
        non-empty directory: what each side prints and which files it updates.

        Move an existing blocked path to .moved. Classify remaining post-save
        entries as new, changed or same using directory names or a short content
        hash; missing post-save entries are omitted. Return both save flags and
        leave the filesystem and native objects for worker cleanup."""
        import hashlib
        ctx = k.ctx
        inst = k.alloc()
        k.fn(parse, Z, P, S)(inst, text)
        k.fn('guess_sentence', B, P)(inst)
        train = k.fn('train', B, P, *train_args)
        train(inst, *((0,) if train_args else ()))
        out = {'first': k.fn('save', B, P)(ctx)}

        def snapshot():
            """Snapshot immediate user entries as sorted directory contents or eight MD5 hex digits."""
            files = {}
            for name in sorted(os.listdir(k.user)):
                path = os.path.join(k.user, name)
                files[name] = ('dir:' + ','.join(sorted(os.listdir(path)))) if os.path.isdir(path) \
                    else hashlib.md5(open(path, 'rb').read()).hexdigest()[:8]
            return files
        before = snapshot()
        train(inst, *((0,) if train_args else ()))
        path = os.path.join(k.user, blocked)
        if os.path.isdir(path) or os.path.exists(path):
            os.replace(path, path + '.moved')
        os.mkdir(path)
        open(os.path.join(path, 'keep'), 'w').write('x')
        out['second'] = k.fn('save', B, P)(ctx)
        after = snapshot()
        out['files'] = {n: ('new' if n not in before else 'changed' if before[n] != after[n] else 'same')
                        for n in after if not n.endswith('.moved')}
        return out


    # The pin writes and renames file by file (`pinyin.cpp:940-1130`): with one
    # path blocked every other file is updated, one line is printed and the
    # profile mixes two saves.
    @case('stderr-save-one-tmp-blocked', stderr=True, issues=('545',))
    def _(k):
        """Save with user_pinyin_index.bin.tmp blocked by a nonempty directory."""
        return blocked_save(k, 'user_pinyin_index.bin.tmp')


    @case('stderr-save-one-final-blocked', stderr=True, issues=('545',))
    def _(k):
        """Save with the final user_phrase_index.bin path blocked by a nonempty directory."""
        return blocked_save(k, 'user_phrase_index.bin')


    @case('stderr-save-one-tmp-blocked-zhuyin', mode='zhuyin', stderr=True, issues=('545',))
    def _(k):
        """Repeat the blocked temporary pinyin-index save through the zhuyin facade."""
        return blocked_save(k, 'user_pinyin_index.bin.tmp', 'parse_more_chewings', b'su3cl3', ())


    # #929: expectations read at 074a2219 pinyin.cpp:922-1146 and
    # zhuyin.cpp:547-698 before differential execution. Ordinary rename failures
    # continue, retain the failed .tmp and clear dirty; successful saves survive fini.
    def save_929_failure(k, blocked):
        """Block a final name, then check the soft failure and clean-save guard."""
        if k.mode == 'zhuyin':
            out = blocked_save(k, blocked, 'parse_more_chewings', b'su3cl3', ())
        else:
            out = blocked_save(k, blocked)
        out['clean_save'] = bool(k.fn('save', B, P)(k.ctx))
        out['staged_retained'] = Path(k.user, blocked + '.tmp').is_file()
        out['destination_retained'] = Path(k.user, blocked, 'keep').read_text()
        return out


    def save_929_roundtrip(k):
        """Saved imports survive reopen; an unsaved suffix does not survive fini."""
        return token_reuse_sequence(k, [
            ('add', 0), ('save',), ('reopen',), ('add', 1), ('reopen',),
            ('add', 2), ('save',), ('reopen',),
        ])


    def save_929_sync_rejecting(k):
        """Chunk writes to /dev/null succeed; its ignored sync error cannot stop save.

        memory_chunk.h:555-557 ignores g_fsync and close returns. This witnesses
        outward continuation, not whether the subject actually issues a sync.
        """
        fd = os.open('/dev/null', os.O_WRONLY)
        try:
            try:
                os.fsync(fd)
            except OSError:
                pass
            else:
                raise AssertionError('/dev/null must reject sync on this host')
        finally:
            os.close(fd)
        if k.mode == 'zhuyin':
            ctx = _dirty_for_save(k, 'parse_more_chewings', b'su3cl3', ())
        else:
            ctx = _dirty_for_save(k)
        Path(k.user, 'user.bin.tmp').symlink_to('/dev/null')
        saved = bool(k.fn('save', B, P)(ctx))
        clean = bool(k.fn('save', B, P)(ctx))
        return {'saved': saved, 'clean': clean,
                'renamed_target': (os.readlink(Path(k.user, 'user.bin'))
                                   if Path(k.user, 'user.bin').is_symlink() else None),
                'bigram_exists': Path(k.user, 'user_bigram.db').is_file()}


    for _save929_mode in ('pinyin', 'zhuyin'):
        for _save929_name in ('user.bin', 'user_pinyin_index.bin',
                              'user_phrase_index.bin', 'user_bigram.db'):
            case('save-929-rename-' + _save929_name + '-' + _save929_mode,
                 mode=_save929_mode, stderr=True, issues=('929',))(
                functools.partial(save_929_failure, blocked=_save929_name))
        case('save-929-reload-' + _save929_mode, mode=_save929_mode,
             issues=('929',))(save_929_roundtrip)
        case('save-929-sync-rejecting-' + _save929_mode, mode=_save929_mode,
             issues=('929',))(save_929_sync_rejecting)


    def save_929_import(k, ctx, number, count=100):
        """Import one existing test phrase through the selected C facade."""
        entry = TOKEN_REUSE_PHRASES[number]
        iterator = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        assert iterator
        assert k.fn('iterator_add_phrase', B, P, S, S, I)(
            iterator, entry[0].encode(), entry[1 if k.mode == 'pinyin' else 2].encode(), count)
        k.fn('end_add_phrases', None, P)(iterator)


    def save_929_tokens(k, ctx):
        """Observe the saved prefix and unsaved suffix after restart."""
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert inst
        k.glib.g_array_new.restype, k.glib.g_array_new.argtypes = P, [I, I, U]
        k.glib.g_array_free.restype, k.glib.g_array_free.argtypes = P, [P, I]
        result = []
        try:
            for text, _, _ in TOKEN_REUSE_PHRASES[:3]:
                arr = k.glib.g_array_new(0, 0, 4)
                try:
                    k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
                    view = C.cast(arr, C.POINTER(Arr)).contents
                    result.append(list(C.cast(view.data, C.POINTER(U))[:view.len]))
                finally:
                    k.glib.g_array_free(arr, 1)
        finally:
            k.fn('free_instance', None, P)(inst)
        return result


    def save_929_late_boundary(k, boundary):
        """Save/fini through C, interrupt at a named boundary, then reopen finals.

        src/pinyin.cpp:1141-1146 and src/zhuyin.cpp:695-698 put the marker after
        data renames. Fini never saves dirty dictionaries; only pinyin writes a
        marker (src/pinyin.cpp:1194-1200; src/zhuyin.cpp:741-757).
        """
        ctx = k.ctx
        save_929_import(k, ctx, 0)
        assert k.fn('save', B, P)(ctx)
        marker = Path(k.user, 'user.conf')
        marker_before = marker.read_bytes()
        if boundary == 'fini-marker':
            save_929_import(k, ctx, 1)
            assert k.fn('save', B, P)(ctx)

        def child():
            """Commit a second phrase and leave a third unsaved before fini."""
            if boundary != 'fini-marker':
                save_929_import(k, ctx, 1)
                assert k.fn('save', B, P)(ctx)
            save_929_import(k, ctx, 2)
            if boundary in ('fini-marker', 'after-fini'):
                k.fn('fini', None, P)(ctx)

        if boundary in ('save-marker', 'fini-marker'):
            stopped = save_929_marker_interrupt(marker, child)
            assert marker.read_bytes() == marker_before
        else:
            pid = os.fork()
            if pid == 0:
                try:
                    child()
                except BaseException:
                    os._exit(74)
                os._exit(73)  # deliberately bypass fini/unwinding at after-save
            _, status = os.waitpid(pid, 0)
            assert os.waitstatus_to_exitcode(status) == 73
            stopped = 'exit-73'
        saved = save_929_physical_files(k.user)
        assert not any(name.endswith('.tmp') for name in saved)
        # The parent's inherited context is stale; never fini it over the child's
        # marker. The worker exits without teardown, as other contract cases do.
        k._ctx = None
        reopened = k.ctx
        tokens = save_929_tokens(k, reopened)
        assert tokens == [[0x07000001], [0x07000002], []], tokens
        after_open = save_929_physical_files(k.user)
        k.fn('fini', None, P)(reopened)
        k._ctx = None
        after_fini = save_929_physical_files(k.user)
        assert saved.keys() == after_open.keys() == after_fini.keys()
        assert all(saved[n] == after_open[n] == after_fini[n] for n in saved if n != 'user.conf')
        return dict(boundary=stopped, tokens=tokens,
                    files=sorted(saved), marker=marker_before.decode(),
                    data_unchanged_on_open={n: saved[n] == after_open.get(n) for n in saved if n != 'user.conf'},
                    data_unchanged_on_fini={n: saved[n] == after_fini.get(n) for n in saved if n != 'user.conf'},
                    final_marker=marker.read_text())


    def save_929_logical_roundtrip(k):
        """Dump exact key/value records and re-save through each receiving facade.

        The authorized pin-only control at a4dd7637 showed independent BDB saves
        are not byte-identical. No fields are masked here: index_file_rows keeps
        complete key/value bytes, including empty values. Fresh cross-open workers
        receive the donor's exact profile through the existing handoff mechanism.
        """
        handoff = os.environ.get('CONTRACT_929_PROFILE')
        if handoff:
            k.user = handoff
        bigram_key = (0x07000001).to_bytes(4, 'little')
        bigram_value = d18_gram(37, 0x07000002, 37)
        if os.environ.get('CONTRACT_929_RELOAD') != '1':
            # Bigram::load_db/save_db copy full rows (ngram_bdb.cpp:47-110).
            k._ctx = crafted_bigram_context(k, [(bigram_key, bigram_value)])
            ctx = k.ctx
            save_929_import(k, ctx, 0)
            save_929_import(k, ctx, 1)
            assert k.fn('save', B, P)(ctx)
            save_929_import(k, ctx, 2)
            k.fn('fini', None, P)(ctx)
            k._ctx = None

        def rows():
            """Read every record, including the nonempty bigram row."""
            return {name: index_file_rows(Path(k.user, name),
                                         kind='hash' if name == 'user_bigram.db' else 'tree',
                                         allow_empty=True)
                    for name in ('user_pinyin_index.bin', 'user_phrase_index.bin', 'user_bigram.db')}

        before = rows()
        assert before['user_bigram.db'] == [[bigram_key.hex(), bigram_value.hex()]]
        ctx = k.ctx
        tokens = save_929_tokens(k, ctx)
        assert tokens == [[0x07000001], [0x07000002], []], tokens
        # Re-adding an existing reading with count zero changes no logical value:
        # src/pinyin.cpp:561-582; src/zhuyin.cpp:449-469 at 074a2219.
        # Re-save witnesses that the C facade loaded the donor's records, rather
        # than merely comparing untouched files with an external dump utility.
        save_929_import(k, ctx, 0, count=0)
        assert k.fn('save', B, P)(ctx)
        after = rows()
        assert before == after, 'facade reopen/save changed logical records'
        k.fn('fini', None, P)(ctx)
        k._ctx = None
        return dict(rows=after, tokens=tokens)


    for _save929_mode in ('pinyin', 'zhuyin'):
        for _save929_boundary in ('save-marker', 'after-save', 'after-fini') + (
                ('fini-marker',) if _save929_mode == 'pinyin' else ()):
            _save929_case = 'save-929-boundary-' + _save929_boundary + '-' + _save929_mode
            case(_save929_case, mode=_save929_mode, issues=('929',))(
                functools.partial(save_929_late_boundary, boundary=_save929_boundary))
            CASES[_save929_case].update(platforms=('linux/amd64', 'linux/arm64'),
                                      platform_skip_reason='#929 interruption uses Linux file leases and fork')
        case('save-929-logical-cross-open-' + _save929_mode, mode=_save929_mode,
             issues=('929',))(save_929_logical_roundtrip)


    # Batch A (#525): `MemoryChunk::save`'s header writes (`memory_chunk.h:543`,
    # `:547`). The pin writes the header as two `guint32`s and `assert`s each, so
    # a filesystem that refuses a header word kills it mid-save. The save is the
    # one failure the pin does not carry past: oxpinyin fails the call and logs
    # one warning in the facade's own domain.
    def _dirty_for_save(k, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
        """Parse, guess and train the configured input, returning the cached context.

        Use rank zero for a nonempty training signature. Ignore operation flags
        and leave the newly allocated instance live."""
        ctx = k.ctx
        inst = k.alloc()
        k.fn(parse, Z, P, S)(inst, text)
        k.fn('guess_sentence', B, P)(inst)
        train = k.fn('train', B, P, *train_args)
        train(inst, *((0,) if train_args else ()))
        return ctx


    def save_header_dev_full(k, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
        """`user.bin.tmp` symlinked to `/dev/full`: its every write fails, so the
        very first header word does (`memory_chunk.h:543`,
        `ret_len == sizeof(length)`).

        Train before saving, leave the symlink in place, and return the save flag
        if the call returns."""
        ctx = _dirty_for_save(k, parse, text, train_args)
        Path(k.user, 'user.bin.tmp').symlink_to('/dev/full')
        return {'ret': k.fn('save', B, P)(ctx)}


    def save_header_rlimit(k, parse='parse_more_full_pinyins', text=b'nihao', train_args=(C.c_ubyte,)):
        """`RLIMIT_FSIZE` of one header word: the length word fits (file size 4)
        and the checksum word past it is refused (`memory_chunk.h:547`,
        `ret_len == sizeof(checksum)`).

        Train before saving. Ignore SIGXFSZ and set both file-size limits to four
        bytes without restoring them. Return the save flag if the call returns."""
        ctx = _dirty_for_save(k, parse, text, train_args)
        signal.signal(signal.SIGXFSZ, signal.SIG_IGN)
        resource.setrlimit(resource.RLIMIT_FSIZE, (4, 4))
        return {'ret': k.fn('save', B, P)(ctx)}


    @case('abort-save-chunk-header-length', abort=False, sigabrt=True, userdir=True, issues=('525',))
    def _(k):
        """Attempt a pinyin save whose first chunk-header write targets /dev/full."""
        return save_header_dev_full(k)


    @case('abort-save-chunk-header-checksum', abort=False, sigabrt=True, userdir=True, issues=('525',))
    def _(k):
        """Attempt a pinyin save with a four-byte process file-size limit."""
        return save_header_rlimit(k)


    @case('abort-zhuyin-save-chunk-header-length', mode='zhuyin', abort=False, sigabrt=True, userdir=True, issues=('525',))
    def _(k):
        """Attempt a zhuyin save whose first chunk-header write targets /dev/full."""
        return save_header_dev_full(k, 'parse_more_chewings', b'su3cl3', ())


    @case('abort-zhuyin-save-chunk-header-checksum', mode='zhuyin', abort=False, sigabrt=True, userdir=True, issues=('525',))
    def _(k):
        """Attempt a zhuyin save with a four-byte process file-size limit."""
        return save_header_rlimit(k, 'parse_more_chewings', b'su3cl3', ())


    @case('stderr-fresh-user-dir', stderr=True, issues=('545',))
    def _(k):
        """Initialize and finalize a fresh pinyin profile, returning the original context's truth value."""
        ctx = k.ctx
        k.fn('fini', None, P)(ctx)
        return {'init': bool(ctx)}


    @case('stderr-fresh-user-dir-zhuyin', mode='zhuyin', stderr=True, issues=('545',))
    def _(k):
        """Initialize and finalize a fresh zhuyin profile, returning the original context's truth value."""
        ctx = k.ctx
        k.fn('fini', None, P)(ctx)
        return {'init': bool(ctx)}


    @case('stderr-init-missing-system-dir', stderr=True, issues=('545',))
    def _(k):
        """Record pinyin initialization with /nonexistent-system-dir as the system path."""
        return {'ctx': bool(k.init(system='/nonexistent-system-dir'))}


    @case('stderr-init-missing-system-dir-zhuyin', mode='zhuyin', stderr=True, issues=('545',))
    def _(k):
        """Record zhuyin initialization with /nonexistent-system-dir as the system path."""
        return {'ctx': bool(k.init(system='/nonexistent-system-dir'))}


    @case('stderr-init-system-dir-without-table-conf', stderr=True, issues=('545',))
    def _(k):
        """Initialize against an empty scratch system directory with two trailing slashes."""
        empty = tempfile.mkdtemp(prefix='sys-', dir=k.scratch)
        return {'ctx': bool(k.init(system=empty + '//'))}


    @case('stderr-save-dir-removed', stderr=True, issues=('545',))
    def _(k):
        """Train pinyin, remove the user directory and record the save result."""
        return save_after_training(k, True)


    @case('stderr-save-dir-removed-zhuyin', mode='zhuyin', stderr=True, issues=('545',))
    def _(k):
        """Train zhuyin, remove the user directory and record the save result."""
        return save_after_training(k, True, 'parse_more_chewings', b'su3cl3', ())


    @case('stderr-fini-dir-removed', stderr=True, issues=('545',))
    def _(k):
        """Remove a fresh pinyin profile directory before finalizing its cached context."""
        ctx = k.ctx
        shutil.rmtree(k.user)
        k.fn('fini', None, P)(ctx)
        return {}


    @case('stderr-fini-dir-removed-zhuyin', mode='zhuyin', stderr=True, issues=('545',))
    def _(k):
        """Remove a fresh zhuyin profile directory before finalizing its cached context."""
        ctx = k.ctx
        shutil.rmtree(k.user)
        k.fn('fini', None, P)(ctx)
        return {}


    # The paths that stay quiet at the pin: a save that has nothing to save, a
    # save that works, a reopen of the profile it wrote.
    # Paths in the pin's lines are the caller's bytes, not text.
    @case('stderr-non-utf8-user-dir', stderr=True, issues=('545',))
    def _(k):
        """Train in a user directory containing 0xFF, remove it, then save and finalize.

        Return training and save flags while leaving diagnostics to the worker."""
        user = os.path.join(os.fsencode(k.scratch), b'user-\xff' + os.path.basename(k.user.encode())[5:])
        os.mkdir(user)
        ctx = k.init(user=user)
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'nihao')
        k.fn('guess_sentence', B, P)(inst)
        out = {'train': k.fn('train', B, P, C.c_ubyte)(inst, 0)}
        shutil.rmtree(user)
        out['save'] = k.fn('save', B, P)(ctx)
        k.fn('fini', None, P)(ctx)
        return out


    @case('stderr-quiet-saves', stderr=True, issues=('545',))
    def _(k):
        """Record unmodified and trained saves, then reopen and save the persisted profile.

        Finalize both contexts on the normal path; the kit's cached pointers are
        not cleared."""
        ctx = k.ctx
        out = {'unmodified save': k.fn('save', B, P)(ctx)}
        out.update(save_after_training(k, False))
        k.fn('fini', None, P)(ctx)
        ctx2 = k.init()
        out['reopen'] = bool(ctx2)
        out['second save'] = k.fn('save', B, P)(ctx2)
        k.fn('fini', None, P)(ctx2)
        return out


    @case('stderr-unmodified-save-dir-removed', stderr=True, issues=('545',))
    def _(k):
        """Remove an unmodified pinyin profile directory and record its save result."""
        ctx = k.ctx
        shutil.rmtree(k.user)
        return {'save': k.fn('save', B, P)(ctx)}
