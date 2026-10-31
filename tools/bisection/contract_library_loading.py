"""Library loading and replacement cases (inventory row 63, #1175).
"""
import ctypes as C
import hashlib
import os
from pathlib import Path
import re
import resource

from contract_abi import B, I, P, S, U, Z
from contract_case_helpers import LIBRARY_FACADES, _chunk, _modify_header, _write_log, log_walk, train_once
from contract_fixtures import _write_user_conf, private_system
from contract_verdict import WARNING_DOMAIN


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


SPARSE_CHUNK_SIZE = (1 << 32) + 8


def sparse_chunk(path):
    """The 2^32+8 chunk `MemoryChunk::mmap` truncates to a zero payload.

    `guint32 data_len = file_size - header` (`memory_chunk.h:499`) turns
    2^32 into 0, so the length word and the checksum word both read 0 and
    the pin accepts the all-zero payload. Only the 8-byte header block is
    allocated: the rest is a hole, so the fixture is 4 GiB logically and
    one file-system block physically, and no copy or read materialises it.

    Create the file exclusively, failing if the path exists, and close its
    descriptor even on failure. Assert the logical size and at most 1 MiB
    allocated, using st_blocks in 512-byte units."""
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    try:
        os.write(fd, b'\0' * 8)  # length word 0, checksum 0
        os.ftruncate(fd, SPARSE_CHUNK_SIZE)
    finally:
        os.close(fd)
    stat = os.stat(path)
    assert stat.st_size == SPARSE_CHUNK_SIZE, stat
    # st_blocks is in 512-byte units; one 4 KiB block for the header.
    assert stat.st_blocks * 512 <= 1 << 20, ('the fixture materialised', stat)


def chunk_stats(path):
    """Return logical and allocated file sizes in bytes, converting st_blocks from 512-byte units."""
    stat = os.stat(path)
    return {'fixture_size': stat.st_size, 'fixture_allocated': stat.st_blocks * 512}


def nonzero_layout_chunk():
    """Encode a chunk with total 123 and an invalid second sub-index separator.

    Wrap the 32-byte payload in a valid length/checksum header. The offsets
    follow SubPhraseIndex::load in src/storage/phrase_index.cpp at pin 074a2219;
    its total is read before the separator check."""
    # A valid header and checksum with total_freq 123 and an in-bounds
    # second-separator fault. The pin reads 123 into the sub-index's local
    # total and does not add it to the facade aggregate
    # (`phrase_index.cpp:237-248`).
    payload = bytearray(32)
    payload[0:4] = (123).to_bytes(4, 'little')
    payload[4:8] = (17).to_bytes(4, 'little')
    payload[8:12] = (20).to_bytes(4, 'little')
    payload[12:16] = (24).to_bytes(4, 'little')
    payload[16] = ord('#')
    payload[19] = ord('!')
    return _chunk(bytes(payload))


def break_library(k, system, name, how):
    """Replaces the link to library `name` with a file `MemoryChunk::mmap`
    refuses: none (`missing`), empty (`empty`), shorter than the 8-byte
    header (`short`), a wrong length word (`bad-length`), or a payload whose
    checksum does not verify (`checksum`) — or with the header-only 8-byte
    chunk (`header-only`) or the sparse 2^32+8 chunk (`sparse`), which both
    map but whose sub-index the pin refuses while main refuses the chunk.

    Remove the existing link before examining how. An unsupported how value
    raises AssertionError after that link has been removed. Additional fixture
    choices are oob-layout, short-chunk, nonzero-layout, second-separator,
    third-separator, dir-long and dir-short. Checksum corruption copies source
    bytes, directories contain one file and sparse fixtures retain their holes.
    Layout checks follow MemoryChunk::mmap at src/include/memory_chunk.h:479-518
    and SubPhraseIndex::load at src/storage/phrase_index.cpp:335-362 (pin 074a2219)."""
    path = os.path.join(system, name)
    os.unlink(path)
    if how == 'empty':
        Path(path).write_bytes(b'')
    elif how == 'short':
        Path(path).write_bytes(b'\0\0\0\0')
    elif how == 'bad-length':
        # 16-byte file, payload 8, declared length 7: the pin's truncated
        # length check refuses it before the checksum.
        Path(path).write_bytes((7).to_bytes(4, 'little') + (0).to_bytes(4, 'little') + b'\0' * 8)
    elif how == 'header-only':
        Path(path).write_bytes(b'\0' * 8)
    elif how == 'sparse':
        sparse_chunk(path)
    elif how == 'oob-layout':
        # An in-bounds four-word header whose second separator index lies
        # past the payload. The pin reads the separator outside the mapped
        # chunk (phrase_index.cpp:354-356); oxpinyin refuses the layout.
        payload = bytearray(32)
        payload[0:4] = (123).to_bytes(4, 'little')
        payload[4:8] = (17).to_bytes(4, 'little')
        payload[8:12] = (1 << 28).to_bytes(4, 'little')
        payload[12:16] = (24).to_bytes(4, 'little')
        payload[16] = ord('#')
        Path(path).write_bytes(_chunk(bytes(payload)))
    elif how == 'short-chunk':
        # A payload shorter than the four-word header: the pin's
        # SubPhraseIndex::load reads the index words past the mapped chunk.
        payload = bytearray(8)
        payload[0:4] = (123).to_bytes(4, 'little')
        payload[4:8] = (17).to_bytes(4, 'little')
        Path(path).write_bytes(_chunk(bytes(payload)))
    elif how == 'nonzero-layout':
        Path(path).write_bytes(nonzero_layout_chunk())
    elif how in ('second-separator', 'third-separator'):
        # A chunk with a valid header and checksum whose second or third
        # `SubPhraseIndex::load` separator is in-bounds but is not '#':
        # the pin's checks at `phrase_index.cpp:355-356`.
        payload = bytearray(32)
        payload[4:8] = (17).to_bytes(4, 'little')     # index_one
        payload[8:12] = (20).to_bytes(4, 'little')    # index_two
        payload[12:16] = (21).to_bytes(4, 'little')   # index_three
        payload[16] = ord('#')
        if how == 'second-separator':
            payload[19] = ord('!')                    # index_two - 1
        else:
            payload[19] = ord('#')
            payload[12:16] = (24).to_bytes(4, 'little')
            payload[23] = ord('!')                    # index_three - 1
        Path(path).write_bytes(_chunk(bytes(payload)))
    elif how == 'checksum':
        data = bytearray(Path(k.data, name).read_bytes())
        data[-1] ^= 0xff
        Path(path).write_bytes(bytes(data))
    elif how in ('dir-long', 'dir-short'):
        # A non-empty directory in the library's place. The size the pin
        # reads from `lseek(SEEK_END)` is the file system's: ext4 answers
        # the htree end of file (huge) for any directory, btrfs the summed
        # length of the entry names twice over (2 for `x`, 50 for 25
        # characters), so which `MemoryChunk::mmap` site is reached follows
        # the size (`memory_chunk.h:479-483` against `:493`).
        os.makedirs(path)
        Path(path, 'e' * 25 if how == 'dir-long' else 'x').write_bytes(b'x')
    else:
        assert how == 'missing'


LIBRARY_BREAKS = ('missing', 'empty', 'short', 'bad-length', 'checksum')


def library_probe(site, how):
    """Build a case that breaks a private library before the selected native operation.

    site selects init, init-fresh, init-nonconform, load, addon, save or mask-out;
    how is passed to break_library. The returned callable performs all I/O."""
    def probe(k):
        """Prepare the selected profile state and collect flags around the broken library.

        Init variants optionally train/save a conforming profile or change its
        model version to 999. Other sites initialize first, then break the library
        before loading, saving or masking it. Leave contexts live."""
        name, index, parse, text, train_args = LIBRARY_FACADES[k.mode]
        system = private_system(k)
        if site in ('init', 'init-fresh', 'init-nonconform'):
            # The pin judges the user profile before it loads a library, so
            # the failing init writes the profile's line first when the
            # profile is fresh or non-conforming and nothing of it when the
            # profile conforms: the three user dirs below.
            if site != 'init-fresh':
                settled = k.init()
                assert settled, 'init failed'
                train_once(k, settled, parse, text, train_args)
                assert k.fn('save', B, P)(settled), 'save failed'
                k.fn('fini', None, P)(settled)
            if site == 'init-nonconform':
                conf = Path(k.user, 'user.conf')
                conf.write_text(re.sub(r'(model data version:)\d+', r'\g<1>999', conf.read_text()))
            break_library(k, system, name, how)
            return {'ctx': bool(k.init(system=system))}
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        load = k.fn('load_phrase_library', B, P, C.c_ubyte)
        if site == 'load':
            # GBK is the one library `pinyin_unload_phrase_library` lets go
            # (TSI_DICTIONARY, 1, is the one `zhuyin_unload_phrase_library`
            # keeps); index 2 is unloadable on both facades.
            out = {'unload': k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2)}
            break_library(k, system, 'gbk_char.bin', how)
            out['load'] = load(ctx, 2)
            return out
        if site == 'addon':
            addon = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
            out = {'unload': k.fn('unload_addon_phrase_library', B, P, C.c_ubyte)(ctx, 4)}
            break_library(k, system, 'art.bin', how)
            out['load'] = addon(ctx, 4)
            return out
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        break_library(k, system, name, how)
        if site == 'save':
            out['save'] = k.fn('save', B, P)(ctx)
        else:
            assert site == 'mask-out'
            out['mask_out'] = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        return out
    return probe


def library_expected(site, how, mode):
    """What the subject answers at a site where the pin dies: its answers
    from before the lines were added, held unchanged. An init that fails on a
    library logs one warning in its own domain and answers NULL.

    A missing-library init is the success-without-logs exception. Other init
    faults return NULL with a facade warning; non-init sites use fixed result
    dictionaries with no logs. Unsupported non-init sites or required warning
    modes raise KeyError."""
    if site.startswith('init'):
        failed = how != 'missing'
        return {'ctx': not failed, 'logs': [[WARNING_DOMAIN[mode], 16]] if failed else []}
    return dict(logs=[], **{
        'load': {'unload': True, 'load': True},
        'addon': {'unload': True, 'load': False},
        'save': {'train': True, 'save': True},
        'mask-out': {'train': True, 'mask_out': True},
    }[site])


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    for _how in LIBRARY_BREAKS:
        for _site in ('init', 'load', 'save', 'mask-out') + (('addon',) if _mode == 'pinyin' else ()):
            case(f'stderr-library-{_site}-{_how}' + _suffix, mode=_mode,
                 crash=library_expected(_site, _how, _mode), issues=('545',))(library_probe(_site, _how))
        # Both lines in the pin's order: the profile's, then the library's.
        for _site, _label in (('init-fresh', 'fresh-user-dir'), ('init-nonconform', 'nonconforming-user-dir')):
            case(f'stderr-library-init-{_label}-{_how}' + _suffix, mode=_mode,
                 crash=library_expected(_site, _how, _mode), issues=('545',))(library_probe(_site, _how))


# A directory where a system library should be (register row 96). A size of
# 8 or more reads as a header-sized file, `read` then fails with EISDIR and
# `assert(ret_len == sizeof(length))` kills the pin (`memory_chunk.h:493`,
# class (c)); a smaller size is `MemoryChunk::mmap`'s ordinary `false` and
# the empty chunk the pin dies on later (row 76, class (b)). The long entry
# name reaches `:493` on ext4 and on btrfs; the one-character entry reaches
# it on ext4 and row 76 on btrfs, so both shapes match whichever site the
# pin's own signal names.
LIBRARY_FINAL = {'init': 'ctx', 'load': 'load', 'save': 'save', 'mask-out': 'mask_out', 'addon': 'load'}


def library_directory_probe(site, how):
    """Wrap a library-failure case so its final operation is also exposed as ret."""
    inner = library_probe(site, how)

    def probe(k):
        """Run the configured library fault and alias the site's final flag as ret."""
        out = inner(k)
        out['ret'] = out[LIBRARY_FINAL[site]]
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    # The zhuyin facade has no addon entry point.
    for _site in (s for s in LIBRARY_FINAL if s != 'addon' or _mode == 'pinyin'):
        case(f'abort-library-directory-long-entry-{_site}' + _suffix, mode=_mode,
             abort=False, crash=library_expected(_site, 'short', _mode), either=True, issues=('545',))(library_directory_probe(_site, 'dir-long'))
        case(f'library-directory-one-entry-{_site}' + _suffix, mode=_mode, abort=False,
             crash=library_expected(_site, 'short', _mode), either=True, issues=('545',))(library_directory_probe(_site, 'dir-short'))


def library_directory_guard(site, guard):
    """Build a save or mask case with a directory at a reserved or unloaded library.

    The callable rewrites a private table.conf and trains before applying the
    fault. site='save' saves; other site values mask library four."""
    def probe(k):
        """Prepare the guarded library, replace its link with a directory and record the operation.

        For guard='reserved', configure index zero; for 'unloaded', unload index two.
        Assert initialization and training and leave the private context live."""
        system = private_system(k)
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        name = 'gbk_char.bin' if guard == 'unloaded' else 'gb_char.bin'
        if guard == 'reserved':
            text = text.replace('default RESERVED NULL NULL NULL NOT_USED',
                                'default RESERVED gb_char.table reserved.bin reserved.dbin SYSTEM_FILE')
            name = 'reserved.bin'
            os.symlink(os.path.join(k.data, 'gb_char.bin'), os.path.join(system, name))
        conf.unlink()
        conf.write_text(text)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, phrase, train_args = LIBRARY_FACADES[k.mode]
        assert train_once(k, ctx, parse, phrase, train_args), 'train failed'
        if guard == 'unloaded':
            assert k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2), 'unload failed'
        break_library(k, system, name, 'dir-long')
        if site == 'save':
            ret = k.fn('save', B, P)(ctx)
        else:
            ret = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        return {'ret': ret}
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _guard in ('reserved', 'unloaded'):
        for _site in ('save', 'mask-out'):
            case(f'library-directory-guard-{_guard}-{_site}' + ('-zhuyin' if _mode == 'zhuyin' else ''),
                 mode=_mode, control=True, issues=('545',))(library_directory_guard(_site, _guard))


# The guards that keep the pin silent with the same broken library: the
# library is already loaded (a load asks for nothing), it is unloaded (a
# save and a mask-out skip it), nothing is modified (a save writes nothing),
# or it is whole (a reload maps it).
def library_control(site):
    """Build a control case for a loaded, unloaded, unmodified or intact system library.

    Supported site names select repeated load/addon load, whole reload,
    unmodified save, or trained save/mask after unloading."""
    def probe(k):
        """Apply the control's library state, record operation flags and finalize the context.

        Use checksum corruption where a broken file is needed; initialization must
        succeed. Unsupported fallback sites assert."""
        name, index, parse, text, train_args = LIBRARY_FACADES[k.mode]
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        unload = k.fn('unload_phrase_library', B, P, C.c_ubyte)
        load = k.fn('load_phrase_library', B, P, C.c_ubyte)
        out = {}
        if site == 'load-already-loaded':
            break_library(k, system, 'gbk_char.bin', 'checksum')
            out['load'] = load(ctx, 2)
        elif site == 'load-whole':
            out['unload'] = unload(ctx, 2)
            out['load'] = load(ctx, 2)
        elif site == 'addon-already-loaded':
            addon = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
            out['first'] = addon(ctx, 4)
            break_library(k, system, 'art.bin', 'checksum')
            out['again'] = addon(ctx, 4)
        elif site == 'save-unmodified':
            break_library(k, system, name, 'checksum')
            out['save'] = k.fn('save', B, P)(ctx)
        else:
            assert site in ('save-unloaded', 'mask-out-unloaded')
            out['train'] = train_once(k, ctx, parse, text, train_args)
            out['unload'] = unload(ctx, 2)
            break_library(k, system, 'gbk_char.bin', 'checksum')
            if site == 'save-unloaded':
                out['save'] = k.fn('save', B, P)(ctx)
            else:
                out['mask_out'] = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        k.fn('fini', None, P)(ctx)
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    for _site in ('load-already-loaded', 'load-whole', 'save-unmodified', 'save-unloaded', 'mask-out-unloaded') + \
            (('addon-already-loaded',) if _mode == 'pinyin' else ()):
        case(f'stderr-library-control-{_site}' + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode,
             control=True, stderr=True, issues=('545',))(library_control(_site))


# The pin's ignored `SubPhraseIndex::load` failure. `MemoryChunk::mmap`
# accepts a header whose length word matches `(file_size - 8)` truncated to
# `guint32` (`memory_chunk.h:499`) and whose checksum verifies; the pinned
# `SubPhraseIndex::load` then rejects the layout with a `g_return_val_if_fail`
# critical (`phrase_index.cpp:354`), which `_load_phrase_library` ignores
# (`pinyin.cpp:264-277`, `zhuyin.cpp:208-221`) and the caller reports as
# success. Main refuses the chunk outright. The sparse 2^32+8 chunk is one
# instance (its truncated payload is 0); the eight-byte header-only chunk is
# the control that separates the truncation from the ignored layout failure.
# The file lives in the private copy of the system directory and replaces
# merged.bin, gbk_char.bin (the one unloadable system library) or art.bin
# (the pinyin facade's addon entry; the zhuyin facade has none).
def chunk_limits():
    """Limit this worker to 12 GiB address space and 60 CPU seconds and disable core dumping.

    Set soft and hard resource limits without restoration and assert Linux
    PR_SET_DUMPABLE succeeds. These settings are not restored."""
    # The pin's init maps 2^32+8 and its save/mask/reinit map it again while
    # the earlier mapping is still held (freemem unmaps only 8 bytes). 8 GiB
    # makes those calls fail on `mmap` — a limit artifact, not pin behaviour.
    # 12 GiB admits the pin's init plus one re-map and the retained mapping
    # (three 4 GiB mappings would not fit, but no case needs a third). The
    # safe subject maps only the 8+data_len chunk (8 bytes for this
    # fixture). The CPU bound and dumpable=0 keep a surprise inside this
    # worker.
    resource.setrlimit(resource.RLIMIT_AS, (12 << 30, 12 << 30))
    resource.setrlimit(resource.RLIMIT_CPU, (60, 60))
    libc = C.CDLL(None, use_errno=True)
    libc.prctl.argtypes, libc.prctl.restype = [I, C.c_ulong, C.c_ulong, C.c_ulong, C.c_ulong], I
    assert libc.prctl(4, 0, 0, 0, 0) == 0, ('PR_SET_DUMPABLE failed', C.get_errno())


def chunk_init_probe(site, how):
    """Build an initialization case with a malformed merged.bin and optional saved profile.

    site='init-settled' trains and saves first; other values use a fresh profile.
    The callable applies chunk_limits and reports fixture sizes in bytes."""
    def probe(k):
        """Install the merged.bin fixture, initialize and report context truth and file sizes.

        For a settled profile, assert its initial training/save and finalize that
        context before the tested init. Leave the final context live."""
        chunk_limits()
        system = private_system(k)
        if site == 'init-settled':
            # A consumer's second run: the user dir carries the profile and
            # a per-library merged.dbin, which init merges after the failed
            # sub-index load.
            settled = k.init()
            assert settled, 'settled init failed'
            _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
            assert train_once(k, settled, parse, text, train_args), 'train failed'
            assert k.fn('save', B, P)(settled), 'save failed'
            k.fn('fini', None, P)(settled)
        path = os.path.join(system, 'merged.bin')
        break_library(k, system, 'merged.bin', how)
        out = {'ctx': bool(k.init(system=system))}
        return out | chunk_stats(path)
    return probe


def chunk_addon_probe(how, reload):
    """Build an addon-load case with a malformed art.bin and optional unload/reload."""
    def probe(k):
        """Apply worker limits, replace art.bin and record addon load flags and fixture sizes.

        Assert initial context creation; reload controls a second load after unload.
        The context remains live."""
        chunk_limits()
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        path = os.path.join(system, 'art.bin')
        break_library(k, system, 'art.bin', how)
        addon = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
        out = {'first': bool(addon(ctx, 4))}
        if reload:
            out['unload'] = bool(k.fn('unload_addon_phrase_library', B, P, C.c_ubyte)(ctx, 4))
            out['again'] = bool(addon(ctx, 4))
        return out | chunk_stats(path)
    return probe


def chunk_load_probe(how):
    """Build a case that unloads library two and reloads it from a malformed gbk_char.bin."""
    def probe(k):
        """Apply worker limits and record unload/load flags after replacing gbk_char.bin.

        Assert context initialization and leave the context live."""
        chunk_limits()
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        unload = k.fn('unload_phrase_library', B, P, C.c_ubyte)
        load = k.fn('load_phrase_library', B, P, C.c_ubyte)
        out = {'unload': bool(unload(ctx, 2))}
        break_library(k, system, 'gbk_char.bin', how)
        out['load'] = bool(load(ctx, 2))
        return out
    return probe


def chunk_save_probe(how):
    """Build a trained-save case whose merged.bin is malformed before initialization."""
    def probe(k):
        """Install the chunk fixture and, if initialization succeeds, train and save.

        Apply worker limits and always include fixture sizes in bytes. Leave any
        successful context live."""
        chunk_limits()
        system = private_system(k)
        path = os.path.join(system, 'merged.bin')
        break_library(k, system, 'merged.bin', how)
        ctx = k.init(system=system)
        out = {'ctx': bool(ctx)}
        if not ctx:
            return out | chunk_stats(path)
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out['train'] = train_once(k, ctx, parse, text, train_args)
        out['save'] = k.fn('save', B, P)(ctx)
        return out | chunk_stats(path)
    return probe


def chunk_mask_probe(how):
    """Build a library-four mask case with merged.bin malformed before initialization."""
    def probe(k):
        """Install the chunk fixture and, if initialization succeeds, train and mask library four.

        Apply worker limits and include fixture sizes in bytes on either return path."""
        chunk_limits()
        system = private_system(k)
        path = os.path.join(system, 'merged.bin')
        break_library(k, system, 'merged.bin', how)
        ctx = k.init(system=system)
        out = {'ctx': bool(ctx)}
        if not ctx:
            return out | chunk_stats(path)
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out['train'] = train_once(k, ctx, parse, text, train_args)
        out['mask_out'] = k.fn('mask_out', B, P, U, U)(ctx, 0xff000000, 0x04000000)
        return out | chunk_stats(path)
    return probe


def chunk_reinit_probe():
    """Build a case that initializes twice against a sparse merged.bin."""
    def probe(k):
        """Apply worker limits, initialize twice, and finalize each non-NULL context.

        Return both context truth values and sparse fixture sizes in bytes."""
        chunk_limits()
        system = private_system(k)
        path = os.path.join(system, 'merged.bin')
        break_library(k, system, 'merged.bin', 'sparse')
        first = k.init(system=system)
        if first:
            k.fn('fini', None, P)(first)
        second = k.init(system=system)
        out = {'ctxs': [bool(first), bool(second)]}
        if second:
            k.fn('fini', None, P)(second)
        return out | chunk_stats(path)
    return probe


def chunk_candidates_probe():
    """Build a candidate-observation case with a sparse merged.bin."""
    def probe(k):
        """Guess through the sparse-library context and capture at most eight candidate rows.

        Apply worker limits; return early on NULL init. Otherwise record parse,
        sentence/candidate flags, total count and [type, text] rows, replacing
        invalid UTF-8 and treating a missing or empty string as None."""
        chunk_limits()
        system = private_system(k)
        break_library(k, system, 'merged.bin', 'sparse')
        ctx = k.init(system=system)
        out = {'ctx': bool(ctx)}
        if not ctx:
            return out
        inst = k.fn('alloc_instance', P, P)(ctx)
        _, _, parse, text, _ = LIBRARY_FACADES[k.mode]
        out['parse'] = k.fn(parse, Z, P, S)(inst, text)
        out['guess_sentence'] = bool(k.fn('guess_sentence', B, P)(inst))
        if k.mode == 'pinyin':
            out['guess_candidates'] = bool(k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0))
        else:
            out['guess_candidates'] = bool(k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0))
        n = U()
        k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(n))
        out['n'] = n.value
        rows = []
        for i in range(min(n.value, 8)):
            cand, ctype, string = P(), I(), S()
            k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, i, C.byref(cand))
            k.fn('get_candidate_type', B, P, P, C.POINTER(I))(inst, cand, C.byref(ctype))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(string))
            rows.append([ctype.value, string.value.decode('utf-8', 'replace') if string.value else None])
        out['rows'] = rows
        return out
    return probe


def chunk_token_probe():
    """Build a token-getter case for the first two items of a sparse library four."""
    def probe(k):
        """Read phrase, pronunciation count and unigram outputs after sparse-library init.

        Apply worker limits and return early on NULL init. For pinyin, also open
        and close an export iterator to record whether library four has a next row.
        Decode/free returned phrase strings; leave the instance and context live."""
        chunk_limits()
        system = private_system(k)
        break_library(k, system, 'merged.bin', 'sparse')
        ctx = k.init(system=system)
        out = {'ctx': bool(ctx)}
        if not ctx:
            return out
        inst = k.fn('alloc_instance', P, P)(ctx)
        # The empty sub-index owns no item: every token read refuses, and
        # the library's export range is empty.
        for token in (0x04000001, 0x04000002):
            length, text = U(0), P(0)
            n, freq = U(0), U(0)
            out[hex(token)] = [
                bool(k.fn('token_get_phrase', B, P, U, C.POINTER(U), C.POINTER(P))(
                    inst, token, C.byref(length), C.byref(text))),
                length.value,
                None if not text.value else k.text(text.value),
                bool(k.fn('token_get_n_pronunciation', B, P, U, C.POINTER(U))(inst, token, C.byref(n))),
                n.value,
                bool(k.fn('token_get_unigram_frequency', B, P, U, C.POINTER(U))(inst, token, C.byref(freq))),
                freq.value,
            ]
        if k.mode == 'pinyin':
            it = k.fn('begin_get_phrases', P, P, U)(ctx, 4)
            out['has_next'] = bool(k.fn('iterator_has_next_phrase', B, P)(it))
            k.fn('end_get_phrases', None, P)(it)
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    for _site, _label in (('init', ''), ('init-settled', '-settled')):
        case(f'sparse-library-init{_label}{_suffix}', mode=_mode,
             stderr=True)(chunk_init_probe(_site, 'sparse'))
    case(f'sparse-library-load{_suffix}', mode=_mode, stderr=True)(chunk_load_probe('sparse'))
    case(f'sparse-library-save{_suffix}', mode=_mode, stderr=True,
         userdir=True)(chunk_save_probe('sparse'))
    case(f'sparse-library-mask-out{_suffix}', mode=_mode, stderr=True,
         userdir=True)(chunk_mask_probe('sparse'))
    case(f'sparse-library-reinit{_suffix}', mode=_mode, stderr=True)(chunk_reinit_probe())
    case(f'sparse-library-candidates{_suffix}', mode=_mode, stderr=True)(chunk_candidates_probe())
case('sparse-library-addon', stderr=True)(chunk_addon_probe('sparse', False))
case('sparse-library-addon-reload', stderr=True)(chunk_addon_probe('sparse', True))
case('sparse-library-tokens', stderr=True)(chunk_token_probe())
case('sparse-library-tokens-zhuyin', mode='zhuyin', stderr=True)(chunk_token_probe())
for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    case(f'header-only-library-init{_suffix}', mode=_mode,
         stderr=True)(chunk_init_probe('init', 'header-only'))
    case(f'header-only-library-load{_suffix}', mode=_mode,
         stderr=True)(chunk_load_probe('header-only'))
case('header-only-library-save', stderr=True, userdir=True)(chunk_save_probe('header-only'))


def nonzero_total_candidates_probe():
    """Build a candidate guess with a nonzero-total malformed sub-index and attempted total wrap."""
    def probe(k):
        """Derive the wrap delta from three intact libraries and guess with malformed merged.bin.

        Apply worker limits and assert initialization. Sum stored totals modulo
        2**32, attempt the compensating token update, and return parse/guess flags;
        do not assume the update succeeded."""
        chunk_limits()
        system = private_system(k)
        break_library(k, system, 'merged.bin', 'nonzero-layout')
        # The loaded valid libraries' stored totals; the failed merged is
        # excluded by the pin, and (before the correction) added by main.
        total = 0
        for name in ('gb_char.bin', 'gbk_char.bin', 'opengram.bin'):
            total = (total + int.from_bytes(Path(system, name).read_bytes()[8:12], 'little')) & 0xFFFFFFFF
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        inst = k.fn('alloc_instance', P, P)(ctx)
        delta = (1 << 32) - total
        add = k.fn('token_add_unigram_frequency', B, P, U, U)
        out = {'add': bool(add(inst, 0x04000001, delta & 0xFFFFFFFF))}
        _, _, parse, text, _ = LIBRARY_FACADES[k.mode]
        out['parse'] = k.fn(parse, Z, P, S)(inst, text)
        out['guess_sentence'] = bool(k.fn('guess_sentence', B, P)(inst))
        if k.mode == 'pinyin':
            out['ret'] = bool(k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0))
        else:
            out['ret'] = bool(k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0))
        return out
    return probe


def nonzero_total_save_probe():
    """Build a save case that observes the log header from a malformed sub-index with total 123."""
    def probe(k):
        """Train and save with nonzero-layout merged.bin, then read two fixed log-header totals.

        Apply worker limits and assert init. Return training/save flags plus file
        length in bytes and the two little-endian words, without validating the log."""
        chunk_limits()
        system = private_system(k)
        break_library(k, system, 'merged.bin', 'nonzero-layout')
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        out['save'] = k.fn('save', B, P)(ctx)
        # The local sub-index total survives in the failed library's log:
        # a MODIFY_HEADER record carries the same old/new 123.
        data = Path(k.user, 'merged.dbin').read_bytes()
        payload = data[8:]
        out['dbin'] = [len(data), int.from_bytes(payload[10:14], 'little'),
                       int.from_bytes(payload[14:18], 'little')]
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    # The pinyin facade reproduces the `0 < total_freq` abort (row 61); the
    # zhuyin twin still refuses after the init critical.
    case(f'nonzero-total-candidates{_suffix}', mode=_mode, abort=False, userdir=True,
         **(dict(sigabrt=True) if _mode == 'pinyin'
            else dict(abort_logs=[[None, 8], [WARNING_DOMAIN[_mode], 16]])))(
             nonzero_total_candidates_probe())
    case(f'nonzero-total-save{_suffix}', mode=_mode, stderr=True)(nonzero_total_save_probe())


def crash_layout_init_probe(how, library='gbk_char.bin'):
    """Build an initialization case for the selected broken library and layout."""
    def probe(k):
        """Apply worker limits, replace the private library and return initialization truth."""
        chunk_limits()
        system = private_system(k)
        break_library(k, system, library, how)
        return {'init': bool(k.init(system=system))}
    return probe


def save_replacement_probe(initial, replacement):
    """Build a save case with merged.bin replaced after its original bytes were loaded.

    initial='invalid' installs a malformed layout before init. replacement is
    same, invalid, valid, or any other value to use gbk_char.bin as the replacement.
    The callable reports the saved log's byte length, SHA-256, totals and counts."""
    def probe(k):
        """Train, atomically replace the private merged.bin when requested, and inspect its saved log.

        Apply worker limits and assert init. Replacement uses a .new sibling and
        os.replace; native training/save flags are recorded rather than asserted."""
        chunk_limits()
        system = private_system(k)
        if initial == 'invalid':
            break_library(k, system, 'merged.bin', 'nonzero-layout')
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        if replacement != 'same':
            path = Path(system, 'merged.bin')
            if replacement == 'invalid':
                data = nonzero_layout_chunk()
            elif replacement == 'valid':
                data = Path(k.data, 'merged.bin').read_bytes()
            else:
                # A different valid chunk: the gbk sub-index's bytes under
                # the merged path. Its items and total differ from merged's.
                data = Path(k.data, 'gbk_char.bin').read_bytes()
            tmp = Path(str(path) + '.new')
            tmp.write_bytes(data)
            os.replace(tmp, path)
        out['save'] = bool(k.fn('save', B, P)(ctx))
        data = Path(k.user, 'merged.dbin').read_bytes()
        header, counts = log_walk(data)
        out['merged'] = [len(data), hashlib.sha256(data).hexdigest(), header]
        out['records'] = counts
        return out
    return probe


def save_changed_with_override_probe():
    """A loaded user override of a gb_char item plus a different valid gb_char
    chunk on disk: the save diff must union the disk, the loaded original and
    the user override (`diff_records_against`).

    Return a callable that changes token 0x01001225 from 你 to 龘 in a user
    log, loads it, then replaces gb_char.bin with gbk_char.bin before saving.
    Record the new log's byte length, hash, decoded header and record counts."""
    def probe(k):
        """Write the single-character override, train, replace the system chunk and summarize the saved log.

        Assert the fixed token names 你 and initialization succeeds. Use the layouts
        from SubPhraseIndex::load and PhraseItem::get_phrase_string in
        src/storage/phrase_index.cpp and PhraseIndexLogger::next_record in
        src/storage/phrase_index_logger.h at pin 074a2219. Apply worker limits and
        replace only private system links."""
        chunk_limits()
        system = private_system(k)
        payload = Path(k.data, 'gb_char.bin').read_bytes()[8:]
        word = lambda offset: int.from_bytes(payload[offset:offset + 4], 'little')
        total, index, content = word(0), word(4), word(8)
        token = 0x01001225
        offset = content + word(index + (token & 0x00FFFFFF) * 4)
        length, prons = payload[offset:offset + 2]
        size = 6 + length * 4 + prons * (length * 2 + 4)
        old = payload[offset:offset + size]
        assert length == 1 and int.from_bytes(old[6:10], 'little') == ord('你')
        new = old[:6] + ord('龘').to_bytes(4, 'little') + old[10:]
        record = ((3).to_bytes(4, 'little') +
                  (token & 0x00FFFFFF).to_bytes(4, 'little') +
                  len(old).to_bytes(2, 'little') + len(new).to_bytes(2, 'little') +
                  old + new)
        _write_user_conf(k)
        _write_log(k, _modify_header(0, total, total) + record, 'gb_char.dbin')
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        data = Path(k.data, 'gbk_char.bin').read_bytes()
        path = Path(system, 'gb_char.bin')
        tmp = Path(str(path) + '.new')
        tmp.write_bytes(data)
        os.replace(tmp, path)
        out['save'] = bool(k.fn('save', B, P)(ctx))
        data = Path(k.user, 'gb_char.dbin').read_bytes()
        header, counts = log_walk(data)
        out['dbin'] = [len(data), hashlib.sha256(data).hexdigest(), header]
        out['records'] = counts
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    # Class (b) row 76 extensions: the same SubPhraseIndex::load separator
    # reads run past the mapped chunk; the subject refuses without the read.
    # The pin SIGSEGVs, so these are crash controls (MATCH on the parent too).
    case('crash-oob-layout' + _suffix, mode=_mode, crash={'init': False},
         control=True)(crash_layout_init_probe('oob-layout'))
    # A short chunk keeps the pin's observable empty sub-index + critical;
    # the out-of-object index reads behind it are not reproduced (class (b)
    # arm of row 76), so the answer is the matched control.
    case('short-chunk-layout' + _suffix, mode=_mode, stderr=True,
         control=True)(crash_layout_init_probe('short-chunk'))
    case('save-replacement-valid-to-different-valid-with-user-override' +
         ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode, stderr=True,
         userdir=True)(save_changed_with_override_probe())

for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    for _name, _initial, _replacement in (
            ('save-replacement-valid-to-invalid', 'valid', 'invalid'),
            ('save-replacement-invalid-to-valid', 'invalid', 'valid'),
            ('save-replacement-valid-to-different-valid', 'valid', 'different'),
            ('save-unchanged-valid', 'valid', 'same'),
            ('save-unchanged-layout-failed', 'invalid', 'same')):
        case(_name + _suffix, mode=_mode, stderr=True, userdir=True)(
            save_replacement_probe(_initial, _replacement))
    # `_write_files` emits the critical synchronously; under fatal mode the
    # pin dies before the failing library's log and after the earlier
    # libraries' writes (pinyin `.tmp`s; zhuyin already-renamed `.dbin`s).
    # The subject writes no critical and completes the save (#1297, D1).
    case('save-fatal-valid-to-invalid' + _suffix, mode=_mode, fatal=True,
         stderr=True, userdir=True)(
        save_replacement_probe('valid', 'invalid'))


def save_missing_system_probe():
    """A modified save after the first system chunk is removed. The pin
    prints `mmap %s failed!` and dies in the empty chunk's
    SubPhraseIndex::load (row 76); the subject prints the line and saves,
    the registered row 76 answer.

    Return a callable that records training and save flags if the native
    calls return; crash expectations come from case registration."""
    def probe(k):
        """Apply worker limits, train a private context, unlink gb_char.bin and record save.

        Assert initialization; leave the context live for the isolated worker."""
        chunk_limits()
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        Path(system, 'gb_char.bin').unlink()
        out['save'] = bool(k.fn('save', B, P)(ctx))
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    case('crash-save-missing-system' + _suffix, mode=_mode,
         crash={'save': True}, control=True)(save_missing_system_probe())


def save_unloaded_replacement_probe():
    """Unload a system library, replace its file with a different valid
    chunk, then save. `_write_files` skips a library whose `get_range` has no
    sub-index (`pinyin.cpp:928-933`, `zhuyin.cpp:563-568`), so the pin never
    remaps the unloaded library; the subject must answer `Keep` for it.

    Return a callable that records flags after unloading library two and
    replacing its private gbk_char.bin. The zhuyin skip is in zhuyin_save
    at src/zhuyin.cpp:560-568 (pin 074a2219)."""
    def probe(k):
        """Train, unload library two, replace gbk_char.bin with merged.bin and save.

        Apply worker limits, assert init, and use a .new sibling plus os.replace.
        Record operation flags without asserting unload, train or save."""
        chunk_limits()
        system = private_system(k)
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        _, _, parse, text, train_args = LIBRARY_FACADES[k.mode]
        out = {'train': train_once(k, ctx, parse, text, train_args)}
        out['unload'] = bool(k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2))
        path = Path(system, 'gbk_char.bin')
        tmp = Path(str(path) + '.new')
        tmp.write_bytes(Path(k.data, 'merged.bin').read_bytes())
        os.replace(tmp, path)
        out['save'] = bool(k.fn('save', B, P)(ctx))
        return out
    return probe


for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    case('save-unloaded-replacement' + _suffix, mode=_mode, stderr=True,
         userdir=True)(save_unloaded_replacement_probe())
for _mode in ('pinyin', 'zhuyin'):
    _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
    for _how in ('second-separator', 'third-separator'):
        case(f'{_how}-library-init{_suffix}', mode=_mode,
             stderr=True)(chunk_init_probe('init', _how))


# The cases whose fixture reaches a `SubPhraseIndex::load` layout check: the
# pin logs its `g_return_val_if_fail` critical (and aborts there under a
# fatal mask); the subject logs nothing. Bound by the #1297 D1 record.
LAYOUT_CRITICAL_RECORD = '1297-layout-criticals.md'
LAYOUT_CRITICAL_CASES = frozenset((
    'header-only-library-init',
    'header-only-library-init-zhuyin',
    'header-only-library-load',
    'header-only-library-load-zhuyin',
    'header-only-library-save',
    'nonzero-total-candidates-zhuyin',
    'nonzero-total-save',
    'nonzero-total-save-zhuyin',
    'save-fatal-valid-to-invalid',
    'save-fatal-valid-to-invalid-zhuyin',
    'save-replacement-invalid-to-valid',
    'save-replacement-invalid-to-valid-zhuyin',
    'save-replacement-valid-to-invalid',
    'save-replacement-valid-to-invalid-zhuyin',
    'save-unchanged-layout-failed',
    'save-unchanged-layout-failed-zhuyin',
    'second-separator-library-init',
    'second-separator-library-init-zhuyin',
    'short-chunk-layout',
    'short-chunk-layout-zhuyin',
    'sparse-library-addon',
    'sparse-library-addon-reload',
    'sparse-library-candidates',
    'sparse-library-candidates-zhuyin',
    'sparse-library-init',
    'sparse-library-init-settled',
    'sparse-library-init-settled-zhuyin',
    'sparse-library-init-zhuyin',
    'sparse-library-load',
    'sparse-library-load-zhuyin',
    'sparse-library-mask-out',
    'sparse-library-mask-out-zhuyin',
    'sparse-library-reinit',
    'sparse-library-reinit-zhuyin',
    'sparse-library-save',
    'sparse-library-save-zhuyin',
    'sparse-library-tokens',
    'sparse-library-tokens-zhuyin',
    'third-separator-library-init',
    'third-separator-library-init-zhuyin',
))


def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order.

    A case in LAYOUT_CRITICAL_CASES is bound to LAYOUT_CRITICAL_RECORD."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        if name in LAYOUT_CRITICAL_CASES:
            kwargs = dict(kwargs, policy_record=LAYOUT_CRITICAL_RECORD)
        driver_case(name, *args, **kwargs)(fn)
