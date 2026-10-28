"""Fixtures that read the run's DBM cell, directly or through one another (#1175).

Each definition is the driver's, verbatim, with the driver state it read added as
keyword-only parameters. contract-diff.py keeps a same-signature adapter per helper that
passes its current CELL, crafters and sibling adapters on every call, so tests that patch
them through the driver, and the families that received the adapters, see no change.
"""
import ctypes as C
import os
from pathlib import Path
import subprocess

from contract_abi import Arr, B, I, P, S, U, Z, row_of
from contract_case_helpers import ADDON_PROMOTE_INPUTS, PHRASE_TEXT_WORDS, TOKEN_REUSE_PHRASES, USER_INDEX_FILES, _chunk
from contract_fixtures import _write_user_conf, private_system, zero_item_padding


def _craft_db(path, rows, dbtype, keep=False, *, system_index=False, CELL, _craft_bdb, _craft_kc, _craft_tkrzw):
    """A user-table container of raw `key -> value` records in the running
    cell's own format: a Berkeley DB `dbtype` container on bdb, the Kyoto
    Cabinet snapshot the pin's `load_db` reads on kc (every user table
    there is loaded with `load_snapshot`, `chewing_large_table2_kyotodb.cpp:106`,
    `ngram_kyotodb.cpp:54`, `phrase_large_table3_kyotodb.cpp:109`), or the
    tkrzw `TreeDBM` file the pin's `load_db` opens on tkrzw
    (`chewing_large_table2_tkrzwdb.cpp:83`, `ngram_tkrzwdb.cpp:48`,
    `phrase_large_table3_tkrzwdb.cpp:83`); the bigram is a `HashDBM` there
    (`ngram_tkrzwdb.cpp:50`). `dbtype` picks the container kind (1 btree/tree,
    2 hash); the rows are the same records in every cell. `keep=True` is
    the fixture that adds rows to the copied system index: a native attached
    TreeDB on kc (`chewing_large_table2_kyotodb.cpp:87-89`), a TreeDBM on
    tkrzw (`chewing_large_table2_tkrzwdb.cpp:87-88`), and a btree on bdb.
    `system_index=True` also selects that native format for a fresh index;
    with `keep=False` it replaces the file instead of writing a KC snapshot.

    Rows contain raw byte keys and values. Assert dbtype == 1 for KC when
    keep or system_index selects native format. Without keep, replace the
    target; keep updates existing rows. Delegate native failures to the
    writer's assertions and return its result. tkrzw selects HashDBM for type two
    and TreeDBM for other values."""
    if CELL == 'kc':
        assert not (keep or system_index) or dbtype == 1, 'KC system indexes are btrees'
        if system_index:
            return _craft_kc(path, rows, keep=keep, system_index=True)
        return _craft_kc(path, rows, keep=keep)
    if CELL == 'tkrzw':
        return _craft_tkrzw(path, rows, 'HashDBM' if dbtype == 2 else 'TreeDBM', keep=keep)
    return _craft_bdb(path, rows, dbtype, keep=keep)


def _craft_hash(path, rows, *, _craft_db):
    """A Berkeley DB hash of `key -> value` records (the pin's user bigram
    layout, `ngram_bdb.cpp:56`).

    Dispatch through _craft_db with hash type two and raw byte key/value pairs,
    replacing the current cell's container. The other supported formats are a
    KC snapshot and tkrzw HashDBM."""
    _craft_db(path, rows, 2)  # DB_HASH == 2


def crafted_bigram_context(k, rows, *, _craft_hash):
    """A context whose user dir carries crafted `user_bigram.db` rows
    (`(key_bytes, value_bytes)` records). The first init writes the
    conforming `user.conf`; the rows are then written and the profile
    reopened.

    Write the raw records in the current cell's format and explicitly
    overwrite user.conf before reopening. Assert both initializations;
    the returned context is not cached or finalized here."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    k.fn('fini', None, P)(ctx)
    _craft_hash(os.path.join(k.user, 'user_bigram.db'), rows)
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    return ctx


def predicted_bigram_context(k, phrase, value, *, _craft_hash):
    """A context whose user bigram carries `value` under every
    phrase-table token of `phrase`, so `_compute_prefixes` reaches it. The
    token is read from this side's own table, so the fixture matches the
    pin and the subject alike.

    UTF-8 encode phrase, assert token lookup and use four-byte little-endian
    tokens as keys. Finalize the initial context and replace user_bigram.db
    and user.conf before returning a separately initialized context. Free
    the lookup instance but leave its GArray allocated; do not update kit caches."""
    ctx = k.init()
    assert ctx, 'the first init failed'
    inst = k.fn('alloc_instance', P, P)(ctx)
    new = k.glib.g_array_new
    new.restype, new.argtypes = P, [I, I, U]
    arr = new(0, 0, 4)
    assert k.fn('lookup_tokens', B, P, S, P)(inst, phrase.encode(), arr)
    view = C.cast(arr, C.POINTER(Arr)).contents
    tokens = list(C.cast(view.data, C.POINTER(U))[:view.len])
    k.fn('free_instance', None, P)(inst)
    k.fn('fini', None, P)(ctx)
    _craft_hash(os.path.join(k.user, 'user_bigram.db'),
                [(token.to_bytes(4, 'little'), value) for token in tokens])
    _write_user_conf(k)
    ctx = k.init()
    assert ctx, 'the reopen failed'
    return ctx


def token_reuse_sequence(k, steps, bigram_rows=None, library=7, *, crafted_bigram_context, token_reuse_files):
    """Runs `steps` over the user library and reports what a caller can see:
    each step's return, the user token of every phrase imported so far, and
    the user files left behind. `('add', n)` imports phrase n, `('save',)`,
    `('mask', mask, value)` and `('reopen',)` are the facade calls.
    `bigram_rows` seeds `user_bigram.db` first, with rows that name the token
    the first import will take.

    Use the chosen library and facade reading column with import count 100.
    A truthy bigram_rows replaces the seeded profile; otherwise use the cached
    context. Reopen finalizes without implicitly saving or replacing the kit's
    cache. Return action results, per-step token mappings and normalized files.
    Unrecognized actions still produce a token snapshot."""
    ctx = crafted_bigram_context(k, bigram_rows) if bigram_rows else k.ctx
    column = 1 if k.mode == 'pinyin' else 2
    out, seen = [], []

    def tokens():
        """Look up every seen phrase with a temporary instance and return hexadecimal token lists.

        Free that instance after lookup; the per-phrase GArrays remain allocated."""
        inst = k.fn('alloc_instance', P, P)(ctx)
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        found = {}
        for n in seen:
            arr = new(0, 0, 4)
            k.fn('lookup_tokens', B, P, S, P)(inst, TOKEN_REUSE_PHRASES[n][0].encode(), arr)
            view = C.cast(arr, C.POINTER(Arr)).contents
            found[TOKEN_REUSE_PHRASES[n][0]] = [hex(t) for t in C.cast(view.data, C.POINTER(U))[:view.len]]
        k.fn('free_instance', None, P)(inst)
        return found

    for step in steps:
        if step[0] == 'add':
            it = k.fn('begin_add_phrases', P, P, U)(ctx, library)
            assert it, 'the import iterator'
            text, reading = TOKEN_REUSE_PHRASES[step[1]][0], TOKEN_REUSE_PHRASES[step[1]][column]
            out.append(['add', bool(k.fn('iterator_add_phrase', B, P, S, S, I)(it, text.encode(), reading.encode(), 100))])
            k.fn('end_add_phrases', None, P)(it)
            seen.append(step[1])
        elif step[0] == 'save':
            out.append(['save', bool(k.fn('save', B, P)(ctx))])
        elif step[0] == 'mask':
            out.append(['mask', bool(k.fn('mask_out', B, P, U, U)(ctx, step[1], step[2]))])
        elif step[0] == 'reopen':
            k.fn('fini', None, P)(ctx)
            ctx = k.init()
            assert ctx, 'the reopen failed'
        out.append(tokens())
    return {'steps': out, 'files': token_reuse_files(k)}


def token_reuse_files(k, *, CELL, index_file_rows):
    """The user files a save leaves, by content. The two user index
    containers are compared record by record, a key whose value is empty
    included ([`index_file_rows`]); a Berkeley DB user bigram is read back
    through `db_dump` and only its non-empty rows are compared (on the other
    backends only that the bigram container exists), `user.bin` is compared
    whole except for the 4 timestamp bytes of its header, and every other file
    is compared as written.

    Skip non-regular entries and .tmp files; normalize pinyin-item padding
    in index rows. Recognize BDB-magic containers for db5.3_dump. The header
    bytes zeroed are 4..7, also in addon.bin and network.bin; other file content
    is hex encoded. File, reader and subprocess failures propagate."""
    files = {}
    for name in sorted(os.listdir(k.user)):
        path = Path(k.user, name)
        if not path.is_file() or name.endswith('.tmp'):
            continue
        raw = path.read_bytes()
        if name in USER_INDEX_FILES:
            rows = index_file_rows(path)
            if name == 'user_pinyin_index.bin':
                rows = [[key, zero_item_padding(key, value)] for key, value in rows]
            files[name] = rows
        elif CELL != 'bdb' and name == 'user_bigram.db':
            files[name] = len(raw) > 0  # another backend's container: only its presence is compared
        elif raw[12:16] in (b'\x62\x31\x05\x00', b'\x61\x15\x06\x00'):
            dump = subprocess.run(['db5.3_dump', str(path)], check=True, capture_output=True).stdout.decode()
            body = dump.split('DATA=END')[0].split('HEADER=END\n')[1].split('\n')[:-1]
            files[name] = [[body[i].strip(), body[i + 1].strip()] for i in range(0, len(body), 2) if body[i + 1].strip()]
        elif name in ('user.bin', 'addon.bin', 'network.bin'):
            files[name] = (raw[:4] + b'\0\0\0\0' + raw[8:]).hex()
        else:
            files[name] = raw.hex()
    return files


def addon_promote_sequence(k, mask_value, *, token_reuse_files):
    """Promotes the addon candidates in turn (`pinyin_choose_candidate`); with
    `mask_value`, masks that token after the third and before the fourth, with
    no reopen between.

    Load addon libraries four and five, save after the first three promotions
    and again after the fourth. Assert library loads and candidate presence;
    return selection/mask/save results, token observations and normalized files.
    The cached context retains all changes."""
    ctx = k.ctx
    for index in (4, 5):
        assert k.fn('load_addon_phrase_library', B, P, C.c_ubyte)(ctx, index)
    out, texts = [], []

    def promote(pinyin):
        """Choose the first addon row for pinyin, append its result and free the temporary instance.

        Assert a kind-6 candidate exists after parsing and guessing at byte zero."""
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, pinyin.encode())
        k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        row = row_of(k, inst, 6)
        assert row, 'no addon candidate for ' + pinyin
        out.append(['promote', pinyin, k.fn('choose_candidate', I, P, Z, P)(inst, 0, row)])
        k.fn('free_instance', None, P)(inst)

    def tokens():
        """Return token IDs for all promoted texts using a temporary native instance.

        Free the instance after lookup but retain the allocated GArrays."""
        inst = k.fn('alloc_instance', P, P)(ctx)
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        found = {}
        for text in texts:
            arr = new(0, 0, 4)
            k.fn('lookup_tokens', B, P, S, P)(inst, text.encode(), arr)
            view = C.cast(arr, C.POINTER(Arr)).contents
            found[text] = [hex(t) for t in C.cast(view.data, C.POINTER(U))[:view.len]]
        k.fn('free_instance', None, P)(inst)
        return found

    for pinyin, text in ADDON_PROMOTE_INPUTS[:3]:
        promote(pinyin)
        texts.append(text)
        out.append(tokens())
    out.append(['save', bool(k.fn('save', B, P)(ctx))])
    if mask_value is not None:
        out.append(['mask', bool(k.fn('mask_out', B, P, U, U)(ctx, 0xffffffff, mask_value))])
        out.append(tokens())
    promote(ADDON_PROMOTE_INPUTS[3][0])
    texts.append(ADDON_PROMOTE_INPUTS[3][1])
    out.append(tokens())
    out.append(['save', bool(k.fn('save', B, P)(ctx))])
    return {'steps': out, 'files': token_reuse_files(k)}


def phrase_text_system(k, variant, *, items_system):
    """Two native items: 甲<word>乙 / da1'da1'da1, then 丙 / ba1.

    Layout: phrase_index.cpp's SubPhraseIndex; PinyinIndexItem2<L> at
    pinyin_phrase3.h:181-197 (four-byte aligned token then packed keys).
    ChewingKey at chewing_key.h:41-47 has tone at bit 12; da = 4|1<<7,
    ba = 1|1<<7 (chewing_enum.h:16,19,65). No API import repairs the words.
    """
    word = PHRASE_TEXT_WORDS[variant]
    items_system(k, [((0x7532, word, 0x4e59), (0x1084,) * 3, 100), ((0x4e19,), (0x1081,), 100)])


def items_system(k, items, total=200, *, _craft_db):
    """A private system whose library 1 holds exactly `items`, tokens 1, 2, ….

    Each item is `(words, keys, unigram)`, written raw: no API import
    repairs a non-scalar word. Items that share a reading share one
    `pinyin_index.bin` value, in token order; every proper prefix of a
    reading or a text is an empty marker row, as import writes them.
    Replace the private links to gb_char.bin, pinyin_index.bin and
    phrase_index.bin, write user.conf and point the kit at the system."""
    system = private_system(k)
    content, offsets = bytearray(8), [0]
    phonetic, phrases = {}, {}
    for slot, (text, reading, unigram) in enumerate(items, 1):
        token = 0x01000000 | slot
        raw_text = b''.join(value.to_bytes(4, 'little') for value in text)
        raw_keys = b''.join(value.to_bytes(2, 'little') for value in reading)
        offsets.append(len(content))
        content += bytes((len(text), 1)) + unigram.to_bytes(4, 'little') + raw_text
        content += raw_keys + (10).to_bytes(4, 'little')
        index = b''.join((value & 0x0fff).to_bytes(2, 'little') for value in reading)
        for end in range(2, len(index), 2):
            phonetic.setdefault(index[:end], b'')
        item = token.to_bytes(4, 'little') + raw_keys
        phonetic[index] = phonetic.get(index, b'') + item + bytes((-len(item)) % 4)
        for end in range(4, len(raw_text), 4):
            phrases.setdefault(raw_text[:end], b'')
        phrases[raw_text] = phrases.get(raw_text, b'') + token.to_bytes(4, 'little')
    index_two = 17 + len(offsets) * 4 + 1
    payload = b''.join(value.to_bytes(4, 'little') for value in
                       (total, 17, index_two, index_two + len(content) + 1))
    payload += b'#' + b''.join(value.to_bytes(4, 'little') for value in offsets)
    payload += b'#' + content + b'#'
    path = Path(system, 'gb_char.bin')
    path.unlink()  # replace the private symlink, never the oracle's file
    path.write_bytes(_chunk(payload))
    for name, rows in (('pinyin_index.bin', phonetic), ('phrase_index.bin', phrases)):
        path = Path(system, name)
        path.unlink()
        _craft_db(str(path), list(rows.items()), 1, system_index=True)
    _write_user_conf(k)
    k.data = system
