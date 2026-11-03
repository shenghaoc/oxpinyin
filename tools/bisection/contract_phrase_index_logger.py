"""Phrase-index logger cases (inventory row 37, #1175).
"""
import ctypes as C
import os

from contract_abi import B, I, P, S, U
from contract_case_helpers import _modify_header, _write_log
from contract_fixtures import _write_user_conf, private_system


_REGISTRATIONS = []


def case(name, *args, **kwargs):
    """Collect a declaration without binding it to a particular driver."""
    def collect(fn):
        _REGISTRATIONS.append((name, args, kwargs, fn))
        return fn
    return collect


def logger_init_probe(k, payload):
    """A `.dbin` crafted before the (only) init. `_write_user_conf` keeps
    `check_format` from unlinking it first, as `pinyin_init`'s own rewrite
    already does for the pinyin cases and `zhuyin_init`'s does not."""
    _write_user_conf(k)
    _write_log(k, payload)
    return {'ret': bool(k.init())}


def logger_mask_out_probe(k, payload, mask=0, value=0):
    """A `.dbin` corrupted after a successful init, then a mask: the pin's
    `merge_with_mask` walks the file it re-reads, so this reaches the site
    the load-time snapshot does not."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    _write_log(k, payload)
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, mask, value)}


def logger_mask_out_later_unmappable(k, payload, later='merged.bin'):
    """`gb_char.dbin` (library 1) holds `payload`; library 4's system chunk
    is gone after the init. The pin's mask loop maps, loads and merges one
    library at a time (`pinyin.cpp:1234-1285`), so library 1's merge asserts
    before the loop reaches library 4, and no `mmap %s failed!` line for it
    is written."""
    system = private_system(k)
    _write_user_conf(k)
    ctx = k.init(system=system)
    assert ctx, 'init failed'
    _write_log(k, payload)
    os.unlink(os.path.join(system, later))
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0, 0)}


def logger_later_directory(k, payload, site):
    """`gb_char.dbin` (library 1) holds `payload`, library 2's system chunk
    is gone, and library 4's is a directory with a long entry, which
    `MemoryChunk::mmap` asserts on (row 96, `memory_chunk.h:493`, on ext4 and
    btrfs alike). Init's library loop (`pinyin.cpp:376-391` into
    `_load_phrase_library` `:246-276`, `zhuyin.cpp:318-334`) and the mask
    loop (`pinyin.cpp:1234-1285`, `zhuyin.cpp:769-826`) map, load and merge
    one library at a time, so library 1's merge asserts (row 91) before
    either loop maps library 2: its `mmap %s failed!` line, which a loop
    that reached library 4 would have written, is never written. `site` is
    'init' (the files are in place before the only init) or 'mask-out' (they
    change after it)."""
    system = private_system(k)
    _write_user_conf(k)

    def break_library():
        os.unlink(os.path.join(system, 'gbk_char.bin'))
        os.unlink(os.path.join(system, 'merged.bin'))
        os.makedirs(os.path.join(system, 'merged.bin'))
        with open(os.path.join(system, 'merged.bin', 'e' * 25), 'wb') as entry:
            entry.write(b'x')

    if site == 'init':
        _write_log(k, payload)
        break_library()
        return {'ret': bool(k.init(system=system))}
    ctx = k.init(system=system)
    assert ctx, 'init failed'
    _write_log(k, payload)
    break_library()
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0, 0)}


def logger_mask_out_unloaded_probe(k, payload):
    """A `.dbin` for a library the call unloads first: the pin's mask loop
    `continue`s on `get_range`'s `ERROR_NO_SUB_PHRASE_INDEX`
    (`pinyin.cpp:1239-1240`, `zhuyin.cpp:796-797`), so the file is never
    merged and the mask completes where the same payload in a loaded
    library refuses it. Index 2 is the one both facades let go
    (`pinyin_unload_phrase_library`'s `GBK_DICTIONARY`), and its user file
    is `gbk_char.dbin`."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    _write_log(k, payload, 'gbk_char.dbin')
    assert k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2), 'the unload failed'
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0, 0)}


def logger_mask_out_after_save(k, payload):
    """A saved profile, then a `.dbin` the mask's reload aborts on: the pin
    dies inside `merge_with_mask` with the saved files as they were, and the
    subject must leave the same user directory."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    reading = b"ni3'hao3" if k.mode == 'pinyin' else 'ㄋㄧˇ ㄏㄠˇ'.encode()
    it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
    added = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '你好'.encode(), reading, 1)
    k.fn('end_add_phrases', None, P)(it)
    assert added, 'the setup phrase did not add'
    assert k.fn('save', B, P)(ctx)
    k._ctx = ctx
    _write_log(k, payload)
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0, 0)}


def logger_reload_probe(k, payload):
    """`*_load_phrase_library` of the unloaded GBK library (index 2) merges
    its saved `gbk_char.dbin` (`pinyin.cpp:276`, `zhuyin.cpp:219`), so the
    merge's assertions are reached on this path as at init."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    assert k.fn('unload_phrase_library', B, P, C.c_ubyte)(ctx, 2), 'the unload failed'
    _write_log(k, payload, 'gbk_char.dbin')
    return {'ret': k.fn('load_phrase_library', B, P, C.c_ubyte)(ctx, 2)}


def logger_mask_out_two_libraries(k, first, second, mask=0, value=0):
    """`mask_out` reloads library 1, then library 2, in the mask loop's
    order (`pinyin.cpp:1234-1291`, `zhuyin.cpp:769-826`): `first` is
    `gb_char.dbin`, `second` is `gbk_char.dbin`."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    _write_log(k, first)
    _write_log(k, second, 'gbk_char.dbin')
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, mask, value)}


_HEADER_TOKEN = _modify_header(0x01000001, 1, 1)
_TWO_HEADERS = _modify_header(0, 1, 1) + _modify_header(0, 1, 2)
# A full record head with an invalid type: `next_record` reads the type and
# the token, hits its `default`, and answers false without advancing the
# header count (`phrase_index_logger.h:214-217`).
_BAD_TAIL = (0x99).to_bytes(4, 'little') + (0).to_bytes(4, 'little')
_TWO_HEADERS_BAD_TAIL = _TWO_HEADERS + _BAD_TAIL
_ONE_HEADER_BAD_TAIL = _modify_header(0, 1, 2) + _BAD_TAIL
# A header, a complete ADD that a mask on library 1's slot 9 keeps, then the
# invalid record: `_peek_header` sets `m_error` there, `rewind()` keeps it
# (`phrase_index_logger.h:128-129`, `:141-144`), and the later passes read
# nothing (`phrase_index.cpp:851-869`), so the kept ADD never reaches
# `append_record`'s assert (`phrase_index_logger.h:245`).
_KEPT_ADD = ((1).to_bytes(4, 'little') + (0x01000005).to_bytes(4, 'little') +
             (8).to_bytes(2, 'little') + bytes((1, 0, 7, 0, 0, 0, 0, 0)))
_HEADER_KEPT_ADD_BAD_TAIL = _modify_header(0, 1, 1) + _KEPT_ADD + _BAD_TAIL
_MASK_SLOT_9 = (0x0FFFFFFF, 0x01000009)

# Library 2's records: a one-byte ADD (row 108) and a non-null header token
# (row 91), for the reload path.
_GBK_SHORT_ADD = ((1).to_bytes(4, 'little') + (0x02000005).to_bytes(4, 'little') +
                  (1).to_bytes(2, 'little') + b'\x01')
_GBK_HEADER_TOKEN = _modify_header(0x02000001, 1, 1)

for _mode in ('pinyin', 'zhuyin'):
    # Row 91: init's library loop merges `gb_char.dbin` and `next_record`
    # asserts on the token (`phrase_index_logger.h:202`) after `check_format`
    # wrote its marker.
    case('abort-init-log-header-token-' + _mode, mode=_mode, abort=False, sigabrt=True, userdir=True,
         issues=('525', 'row:91'))(
        lambda k: logger_init_probe(k, _HEADER_TOKEN))
    case('abort-mask-out-log-header-token-' + _mode, mode=_mode, abort=False, sigabrt=True,
         issues=('525', 'row:91'))(
        lambda k: logger_mask_out_probe(k, _HEADER_TOKEN))
    case('abort-mask-out-multiple-log-headers-' + _mode, mode=_mode, abort=False, sigabrt=True,
         issues=('525', 'row:92'))(
        lambda k: logger_mask_out_probe(k, _TWO_HEADERS))
    # `_peek_header` counts the headers it read before the malformed tail
    # ends the walk and asserts on the total (`phrase_index.cpp:721-746`),
    # so two headers followed by an invalid record still abort there.
    case('abort-mask-out-multiple-log-headers-bad-tail-' + _mode, mode=_mode, abort=False, sigabrt=True,
         issues=('525', 'row:92'))(
        lambda k: logger_mask_out_probe(k, _TWO_HEADERS_BAD_TAIL))
    # The abort comes after a save: both sides leave the saved profile.
    case('abort-mask-out-log-header-untouched-' + _mode, mode=_mode, abort=False, sigabrt=True, userdir=True,
         issues=('525', 'row:92'))(
        lambda k: logger_mask_out_after_save(k, _TWO_HEADERS))
    # The mask loop skips a library the call unloaded, so its corrupt
    # `.dbin` is never merged and the mask completes.
    case('mask-out-unloaded-library-log-' + _mode, mode=_mode, control=True, issues=('525',))(
        lambda k: logger_mask_out_unloaded_probe(k, _TWO_HEADERS))
    # The pin applies every header at init's `merge` (it only peeks at
    # mask-out time), so a two-header `.dbin` loads cleanly: a control.
    case('init-multiple-log-headers-' + _mode, mode=_mode, control=True, issues=('525',))(
        lambda k: logger_init_probe(k, _TWO_HEADERS))
    if _mode == 'pinyin':
        case('abort-mask-out-log-header-token-later-unmappable-' + _mode, mode=_mode, abort=False,
             sigabrt=True, stderr=True, issues=('row:91',))(
            lambda k: logger_mask_out_later_unmappable(k, _HEADER_TOKEN))
    # Row 91 in library 1 before row 96 in library 4: the earlier journal
    # fault is the one reached, at init and at `mask_out`, with no line for
    # the missing library 2 between them.
    case('abort-init-log-header-token-later-directory-' + _mode, mode=_mode, abort=False,
         sigabrt=True, stderr=True, userdir=True, issues=('row:91', 'row:96'))(
        lambda k: logger_later_directory(k, _HEADER_TOKEN, 'init'))
    case('abort-mask-out-log-header-token-later-directory-' + _mode, mode=_mode, abort=False,
         sigabrt=True, stderr=True, issues=('row:91', 'row:96'))(
        lambda k: logger_later_directory(k, _HEADER_TOKEN, 'mask-out'))
    case('mask-out-log-kept-add-bad-tail-' + _mode, mode=_mode, control=True, issues=('row:97',))(
        lambda k: logger_mask_out_probe(k, _HEADER_KEPT_ADD_BAD_TAIL, *_MASK_SLOT_9))
    # One header before the same tail passes `1 >= header_count`, so the
    # pin completes: a control that holds the count's boundary.
    case('mask-out-log-header-bad-tail-' + _mode, mode=_mode, control=True, issues=('525',))(
        lambda k: logger_mask_out_probe(k, _ONE_HEADER_BAD_TAIL))
    # Rows 91 and 108 on the reload path: the GBK library's saved log
    # reaches `next_record`'s assert or `add_phrase_item`'s unigram read
    # (`memory_chunk.h:390`) when the library is loaded again.
    case('abort-reload-log-header-token-' + _mode, mode=_mode, abort=False, sigabrt=True, userdir=True,
         issues=('row:91',))(
        lambda k: logger_reload_probe(k, _GBK_HEADER_TOKEN))
    case('abort-reload-short-add-' + _mode, mode=_mode, abort=False, sigabrt=True, userdir=True,
         issues=('row:108',))(
        lambda k: logger_reload_probe(k, _GBK_SHORT_ADD))
    # The mask reloads library 1 (a header-only log the merge stops at)
    # and then dies in library 2's `_peek_header` (`phrase_index.cpp:745`).
    case('abort-mask-out-second-library-multiple-log-headers-' + _mode, mode=_mode, abort=False,
         sigabrt=True, userdir=True, issues=('row:92',))(
        lambda k: logger_mask_out_two_libraries(k, _modify_header(0, 1, 1), _TWO_HEADERS))
    # The same logs under a mask whose library bits name library 1:
    # The journal has already been loaded; `merge_with_mask` returns for
    # library 2 before parsing its records (`phrase_index.cpp:309-313`).
    case('mask-out-second-library-other-bits-' + _mode, mode=_mode, control=True, issues=('row:92',))(
        lambda k: logger_mask_out_two_libraries(k, _modify_header(0, 1, 1), _TWO_HEADERS,
                                                0x0F000000, 0x01000000))



def mask_excluded_journal_header(k):
    """The excluded library's journal is loaded before its mask is checked."""
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'init failed'
    path = os.path.join(k.user, 'gb_char.dbin')
    if os.path.lexists(path):
        os.unlink(path)
    os.mkdir(path)
    with open(os.path.join(path, 'e' * 25), 'wb') as entry:
        entry.write(b'x')
    # Library 1 is excluded, but MemoryChunk::load's header read occurs
    # before merge_with_mask tests the bits (pinyin.cpp:1280-1285).
    return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0x0f000000, 0x02000000)}


for _mode in ('pinyin', 'zhuyin'):
    case('chunk-frame-excluded-journal-header-' + _mode, mode=_mode, abort=False,
         sigabrt=True, dir_chunk='user', issues=('1576',))(mask_excluded_journal_header)

def register(driver_case):
    """Register the declarations with the driver's current decorator, in their original order."""
    for name, args, kwargs, fn in _REGISTRATIONS:
        driver_case(name, *args, **kwargs)(fn)
