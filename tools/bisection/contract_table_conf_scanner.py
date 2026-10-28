"""Table.conf and scanner cases (inventory row 64, #1175).
"""
import ctypes as C
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from contract_abi import B, I, P, S, U, UNTOUCHED, Z, unigram_of
from contract_case_helpers import token_unigram_trained, token_unigram_user
from contract_fixtures import _write_user_conf, conf_system, private_system, user_listing
from contract_verdict import WARNING_DOMAIN


# #1119: direct KC classes open literal paths (074a2219
# ngram_kyotodb.cpp:117-119; chewing_large_table2_kyotodb.cpp:87-89).
# table_info.cpp:255-284 preserves '#' in filename fields; pinyin.cpp:
# 250-276, 303-319, 939-988, 1042-1084 loads/saves those literal names.
# The DBM filenames are fixed by the facade; table.conf can rename the
# system chunks, user chunks and journals. The store trait laws also
# exercise '#' in actual TreeDB/HashDB filenames, without a symlink.
def hash_path_fixture(k, placement):
    """Put literal #type=kct text into the selected private directory or configured filename.

    placement selects system-directory, user-directory, system-filename or
    user-filename; otherwise raise ValueError. Rename directories or replace
    the private table.conf link, update k.data and optionally k.user, and return
    (system chunk name, journal name, user chunk name). Do not initialize a context."""
    system = private_system(k)
    system_name, journal_name, user_name = 'gb_char.bin', 'gb_char.dbin', 'user.bin'
    if placement == 'system-directory':
        renamed = system + '#type=kct'
        os.rename(system, renamed)
        system = renamed
    elif placement == 'user-directory':
        renamed = k.user + '#type=kct'
        os.rename(k.user, renamed)
        k.user = renamed
    elif placement in ('system-filename', 'user-filename'):
        conf = Path(system, 'table.conf')
        text = conf.read_text()
        if placement == 'system-filename':
            system_name = 'gb#type=kct.bin'
            Path(system, 'gb_char.bin').rename(Path(system, system_name))
            text = text.replace('gb_char.bin', system_name)
        else:
            journal_name, user_name = 'gb#type=kct.dbin', 'user#type=kct.bin'
            text = text.replace('gb_char.dbin', journal_name).replace('user.bin', user_name)
        conf.unlink()  # private_system linked this file to the immutable pin data
        conf.write_text(text)
    else:
        raise ValueError('unknown hash path placement: ' + placement)
    k.data = system
    return system_name, journal_name, user_name


def register(case, *, CASES, WORKER):
    """Register these cases at their original driver position."""
    CONF_TEXTS = {'pinyin': (b'nihao', b'yishu'), 'zhuyin': (b'ni3hao3', b'yi4shu4')}


    def conf_system_bytes(k, mutate):
        """A private system copy whose table.conf is `mutate` of the raw stock
        bytes. Bytes and NUL are preserved, unlike `conf_system`'s text edits."""
        system = private_system(k)
        path = os.path.join(system, 'table.conf')
        data = Path(k.data, 'table.conf').read_bytes()
        os.unlink(path)
        Path(path).write_bytes(mutate(data))
        return system


    def conf_guess_rows(k, inst, limit=12):
        """`guess_candidates` at offset 0: the call's answer and the first rows."""
        if k.mode == 'pinyin':
            ok = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        else:
            ok = k.fn('guess_candidates_after_cursor', B, P, Z)(inst, 0)
        count = U(UNTOUCHED)
        k.fn('get_n_candidate', B, P, C.POINTER(U))(inst, C.byref(count))
        rows = []
        for index in range(min(count.value, limit)):
            cand, text = P(), S()
            k.fn('get_candidate', B, P, U, C.POINTER(P))(inst, index, C.byref(cand))
            k.fn('get_candidate_string', B, P, P, C.POINTER(S))(inst, cand, C.byref(text))
            rows.append(text.value.decode())
        return ok, count.value, rows


    def conf_sentence(k, inst):
        """Read the first sentence, returning None for both untouched output and NULL.

        Ignore the native flag; decode/free returned text with Kit.text."""
        out = P(UNTOUCHED)
        if k.mode == 'pinyin':
            k.fn('get_sentence', B, P, C.c_ubyte, C.POINTER(P))(inst, 0, C.byref(out))
        else:
            k.fn('get_sentence', B, P, C.POINTER(P))(inst, C.byref(out))
        return None if out.value == UNTOUCHED else k.text(out.value)


    def conf_view(k, ctx):
        """Sentences and candidate lists for two inputs: the observable the
        loaded library set decides."""
        view = {}
        for text in CONF_TEXTS[k.mode]:
            inst = k.fn('alloc_instance', P, P)(ctx)
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
            k.fn('guess_sentence', B, P)(inst)
            view[text.decode()] = [conf_sentence(k, inst), *conf_guess_rows(k, inst)]
        return view


    def conf_session(k, system):
        """Open on `system`, look up, learn one sentence and one phrase, save:
        what the rows decide to load, to accept and to write."""
        ctx = k.init(system=system)
        if not ctx:
            return {'ctx': False}
        out = {'view': conf_view(k, ctx)}
        inst = k.fn('alloc_instance', P, P)(ctx)
        k.fn('parse_more_full_pinyins', Z, P, S)(inst, CONF_TEXTS[k.mode][0])
        k.fn('guess_sentence', B, P)(inst)
        out['train'] = k.fn('train', B, P, C.c_ubyte)(inst, 0) if k.mode == 'pinyin' else k.fn('train', B, P)(inst)
        it = k.fn('begin_add_phrases', P, P, U)(ctx, 7)
        reading = b"ce'shi'ci" if k.mode == 'pinyin' else 'ㄘㄜˋ ㄕˋ ㄘˊ'.encode()
        out['add'] = k.fn('iterator_add_phrase', B, P, S, S, I)(it, '测试词'.encode(), reading, 1)
        k.fn('end_add_phrases', None, P)(it)
        out['save'] = k.fn('save', B, P)(ctx)
        k.fn('fini', None, P)(ctx)
        out['files'] = user_listing(k)
        return out


    def hash_path_session(placement):
        """Build a training/import/save/reopen case using the selected literal-hash path placement."""
        def probe(k):
            """Assert trained and imported counts survive saving through the renamed paths.

            Check expected files and absence of replaced names, reopen after clearing
            cached pointers, and compare unigram outputs. Close again, then launch the
            companion pinyin library's export worker with AXIS_C_REOPEN_USER. Require
            zero exit and nonempty JSON export rows; the subprocess has no timeout here."""
            system_name, journal_name, user_name = hash_path_fixture(k, placement)
            out = {'training': token_unigram_trained(k),
                   'import': token_unigram_user(k, add_frequency=False)}
            out['save'] = k.fn('save', B, P)(k.ctx)
            assert out['save'], 'the trained profile must save'
            out['files'] = user_listing(k)
            assert Path(k.data, system_name).is_file(), 'the configured system file must exist'
            assert Path(k.user, journal_name).is_file(), 'the trained journal must be written'
            assert Path(k.user, user_name).is_file(), 'the imported library must be written'
            if placement == 'system-filename':
                assert not Path(k.data, 'gb_char.bin').exists()
            if placement == 'user-filename':
                assert not Path(k.user, 'gb_char.dbin').exists()
                assert not Path(k.user, 'user.bin').exists()
            k.fn('free_instance', None, P)(k.inst)
            k.fn('fini', None, P)(k.ctx)
            k._ctx = k._inst = None
            out['reopen'] = bool(k.ctx)
            out['trained-after-reopen'] = unigram_of(k, int(out['training']['token'], 16))
            assert out['trained-after-reopen'] == out['training']['after']
            out['import-after-reopen'] = unigram_of(k, int(out['import']['token'], 16))
            assert out['import-after-reopen'] == out['import']['after']
            k.fn('free_instance', None, P)(k.inst)
            k.fn('fini', None, P)(k.ctx)
            k._ctx = k._inst = None
            # libzhuyin has no export iterator (zhuyin.h at 074a2219).
            # Reuse the existing export worker on this side's companion
            # libpinyin, as axis_c_import does for either facade's saved profile.
            library = Path(k.lib._name)
            companion = library.with_name(library.name.replace('libzhuyin', 'libpinyin', 1))
            proc = subprocess.run([sys.executable, str(WORKER), 'pinyin', str(companion),
                                   k.data, 'axis-c-export-user'], capture_output=True,
                                  env=dict(os.environ, AXIS_C_REOPEN_USER=k.user))
            assert proc.returncode == 0, proc.stderr.decode('utf-8', 'surrogateescape')
            out['export'] = json.loads(proc.stdout)
            assert out['export']['rows'], 'reopen must preserve the imported phrase for export'
            return out
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        for _placement in ('system-directory', 'user-directory', 'system-filename', 'user-filename'):
            case('hash-path/' + _placement + '-' + _mode, mode=_mode, stderr=True,
                 issues=('1119',))(hash_path_session(_placement))


    def addon_session(k, system, indexes=(0, 3, 4, 5, 15)):
        """The addon libraries a row set loads, and the candidates they add."""
        ctx = k.init(system=system)
        assert ctx, 'init failed'
        load = k.fn('load_addon_phrase_library', B, P, C.c_ubyte)
        out = {'load': {str(index): load(ctx, index) for index in indexes}}
        out['view'] = conf_view(k, ctx)
        k.fn('fini', None, P)(ctx)
        return out


    # Files the parent already answered like the pin: the header fields it read,
    # the words it never needed, an alias, a row that changes nothing observable.
    CONF_CONTROLS = {'addon-duplicate-row', 'addon-files-swapped', 'comment-dropped', 'extra-word-dropped',
                     'lambda-exponent', 'lambda-half', 'lambda-hex', 'lambda-hex-fraction', 'lambda-hex-upper',
                     'lambda-one', 'lambda-tiny', 'lambda-zero',
                     'reserved-as-system-file', 'short-row-dropped', 'source-format-zhuyin', 'tsi-is-gb',
                     'versions-other'}


    # The libzhuyin candidate law read the pinned λ, not the file's, so these tie-order
    # cases differ from the parent on libzhuyin only.
    CONF_ZHUYIN_CHANGED = {'lambda-zero', 'lambda-one', 'lambda-tiny'}


    def conf_case(name, *edits, modes=('pinyin', 'zhuyin'), session=conf_session,
                  issues=('694', '525'), **options):
        """Register edited-config session cases for the selected facades.

        Compute control status from the named control sets, allowing options to
        override it. Each registered callable builds a private system when run;
        registration itself does not initialize native state."""
        for mode in modes:
            def probe(k, edits=edits):
                """Apply the captured config edits and return the selected session's observations."""
                return session(k, conf_system(k, *edits))
            probe.config_edits = edits
            control = name in CONF_CONTROLS and not (mode == 'zhuyin' and name in CONF_ZHUYIN_CHANGED)
            case('table-conf-' + name + ('-zhuyin' if mode == 'zhuyin' else ''), mode=mode,
                 **{'control': control, **options}, issues=issues)(probe)


    # Valid files that are not the stock one: the pin follows the rows.
    _STOCK_DBIN = 'gb_char.dbin SYSTEM_FILE'
    conf_case('row-removed-opengram', ('-', 'OPENGRAM_DICTIONARY'))
    conf_case('row-removed-gbk', ('-', 'GBK_DICTIONARY'))
    conf_case('row-removed-gb', ('-', 'GB_DICTIONARY'))
    conf_case('not-used-opengram', ('opengram.dbin SYSTEM_FILE', 'opengram.dbin NOT_USED'))
    conf_case('not-used-gb', (_STOCK_DBIN, 'gb_char.dbin NOT_USED'))
    conf_case('not-used-merged', ('merged.dbin SYSTEM_FILE', 'merged.dbin NOT_USED'))
    conf_case('not-used-user-dictionary', ('user.bin USER_FILE', 'user.bin NOT_USED'))
    conf_case('not-used-network-dictionary', ('network.bin USER_FILE', 'network.bin NOT_USED'))
    conf_case('user-files-renamed', ('gb_char.dbin', 'x_gb.dbin'), ('merged.dbin', 'x_merged.dbin'),
              ('user.bin', 'x_user.bin'), ('addon.bin', 'x_addon.bin'))
    conf_case('tsi-is-gb', ('default GB_DICTIONARY', 'default TSI_DICTIONARY'))
    conf_case('later-row-wins', ('+', 'default OPENGRAM_DICTIONARY opengram.table opengram.bin opengram.dbin NOT_USED'))
    conf_case('network-as-system-file', ('default NETWORK_DICTIONARY NULL NULL network.bin USER_FILE',
                                          'default NETWORK_DICTIONARY art.table art.bin network.dbin SYSTEM_FILE'))
    conf_case('reserved-as-system-file', ('default RESERVED NULL NULL NULL NOT_USED',
                                           'default RESERVED merged.table merged.bin x_reserved.dbin SYSTEM_FILE'))
    # λ spelled inside [0, 1]. The candidate law `(λ·bigram + (1−λ)·unigram)·2²⁴`
    # reads the file's λ (it used to be the pinned constant, which listed tied
    # libzhuyin rows in another order at λ = 0, 1 and 1e-30).
    for _name, _value in (('zero', '0'), ('half', '0.5'), ('one', '1'), ('exponent', '5e-1'), ('tiny', '1e-30')):
        conf_case('lambda-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _value))
    # λ outside [0, 1]: below 0 the possibilities exceed one and the costs go
    # negative; above 1 `unigram_lambda` is negative, `log` of it is NaN and the
    # NaN is sticky through the trellis. The tail sort (`g_ptr_array_sort` over a
    # comparator that turns the float difference into a `gint`) then orders the
    # sentences.
    for _name, _value in (('negative-half', '-0.5'), ('negative-two', '-2'), ('one-and-a-half', '1.5'),
                          ('two', '2'), ('million', '1e6'), ('inf', 'inf'), ('nan', 'nan')):
        conf_case('lambda-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _value))
    # The header fields the user marker conforms against.
    conf_case('database-format-other', ('re', r'database format:\w+', lambda found: 'database format:' + (
        'BerkeleyDB' if found.group(0).endswith('Tkrzw') else 'Tkrzw')))
    conf_case('versions-other', ('binary format version:7', 'binary format version:8'),
              ('model data version:14', 'model data version:15'))
    conf_case('source-format-zhuyin', ('source table format:pinyin', 'source table format:zhuyin'))
    # Words are read in groups of six: a short row, a comment and a seventh
    # word at the end are dropped, and an addon index goes through a `guint8`.
    conf_case('short-row-dropped', ('+', 'default GB_DICTIONARY a b c'))
    conf_case('comment-dropped', ('+', '# a comment'))
    conf_case('extra-word-dropped', ('+', 'default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE EXTRA'))
    conf_case('addon-index-wraps', ('+', 'addon 256 art.table art.bin NULL DICTIONARY'),
              modes=('pinyin',), session=addon_session)
    # The addon rows decide what `pinyin_load_addon_phrase_library` loads.
    conf_case('addon-moved', ('addon 4 art.table', 'addon 3 art.table'), modes=('pinyin',), session=addon_session)
    conf_case('addon-files-swapped', ('addon 4 art.table art.bin', 'addon 4 art.table culture.bin'),
              ('addon 5 culture.table culture.bin', 'addon 5 culture.table art.bin'),
              modes=('pinyin',), session=addon_session)
    conf_case('addon-removed', ('-', 'addon 4 '), modes=('pinyin',), session=addon_session)
    conf_case('addon-not-used', ('art.bin NULL DICTIONARY', 'art.bin NULL NOT_USED'), modes=('pinyin',), session=addon_session)
    conf_case('addon-index-zero', ('addon 4 art.table', 'addon 0 art.table'), modes=('pinyin',), session=addon_session)
    conf_case('addon-duplicate-row', ('+', 'addon 4 culture.table culture.bin NULL DICTIONARY'),
              modes=('pinyin',), session=addon_session)


    # Class (c): the rows the pin dies on. The triggering call fails with one
    # warning; `userdir` holds the profile the pin has written by then.
    def conf_abort(name, *edits, call='init', userdir=True, modes=('pinyin', 'zhuyin'), ret=False,
                   dir_chunk=None, control=False, issues=('694', '525')):
        """Register expected-abort config cases for initialization or a library load.

        call='init' tests initialization; otherwise unpack (kind, index), using
        'load' for a normal library and any other kind for an addon. Forward expected
        ret, userdir, directory-chunk and control metadata to case registration."""
        for mode in modes:
            def probe(k, edits=edits):
                """Initialize with the captured config edits and perform the selected load if requested.

                Return its truth/flag as ret; loads require successful initialization and
                leave the context live."""
                system = conf_system(k, *edits)
                if call == 'init':
                    return {'ret': bool(k.init(system=system))}
                ctx = k.init(system=system)
                assert ctx, 'init failed'
                kind, index = call
                if kind == 'load':
                    return {'ret': k.fn('load_phrase_library', B, P, C.c_ubyte)(ctx, index)}
                return {'ret': k.fn('load_addon_phrase_library', B, P, C.c_ubyte)(ctx, index)}
            case('table-conf-abort-' + name + ('-zhuyin' if mode == 'zhuyin' else ''), mode=mode,
                 abort=ret, userdir=userdir, dir_chunk=dir_chunk, control=control, issues=issues)(probe)


    # `table_info.cpp` aborts inside `SystemTableInfo2::load`, before
    # `check_format`: the user directory is untouched.
    conf_abort('source-format-unknown', ('source table format:pinyin', 'source table format:foo'))
    conf_abort('database-format-unknown', ('re', r'database format:\w+', 'database format:LMDB'))
    conf_abort('target-unknown', ('+', 'foo GB_DICTIONARY a b c SYSTEM_FILE'))
    conf_abort('default-name-unknown', ('+', 'default FOO_DICTIONARY a b c SYSTEM_FILE'))
    conf_abort('file-type-unknown', ('+', 'default GB_DICTIONARY a b c FOO_FILE'))
    conf_abort('addon-index-99', ('+', 'addon 99 x.table x.bin NULL DICTIONARY'))
    conf_abort('addon-index-16', ('+', 'addon 16 x.table x.bin NULL DICTIONARY'))
    conf_abort('addon-index-negative', ('+', 'addon -1 x.table x.bin NULL DICTIONARY'))
    # `UserTableInfo::load` converts the `database format:` token of the user's
    # `user.conf` and aborts on one it does not know (`table_info.cpp:132`,
    # reached from `check_format`); the system `table.conf` reaches the same
    # converter above.
    @case('user-conf-abort-database-format-unknown', abort=False, userdir=True, issues=('694', '525'))
    def _(k):
        """Write an LMDB database-format marker into user.conf and record initialization truth.

        Assert the replacement matched before overwriting the profile marker."""
        _write_user_conf(k)
        path = os.path.join(k.user, 'user.conf')
        with open(path) as user_conf:
            text = user_conf.read()
        text = re.sub(r'database format:\w+', 'database format:LMDB', text)
        assert 'database format:LMDB' in text
        with open(path, 'w') as user_conf:
            user_conf.write(text)
        return {'ret': bool(k.init())}


    # The init loop (`pinyin.cpp:377-392`) runs after `check_format`: the profile
    # is judged and `user.conf` written when it dies.
    conf_abort('default-dictionary', (_STOCK_DBIN, 'gb_char.dbin DICTIONARY'))
    conf_abort('default-dictionary-gbk', ('gbk_char.dbin SYSTEM_FILE', 'gbk_char.dbin DICTIONARY'))
    # A row without a file name makes the pin read that directory as a chunk
    # (`g_build_filename` stops at the NULL). `MemoryChunk` asserts on the
    # `read` of a directory (`memory_chunk.h:434` for `load`, `:493` for `mmap`)
    # only where `lseek(SEEK_END)` on it answers 8 or more, which the file system
    # of the scratch directory decides (#1011): ext4, fuse-overlayfs and a new
    # directory in an overlay answer more, tmpfs refuses `SEEK_END` on a
    # directory. `dir_chunk` holds each case to the side of that condition the
    # host is on (`case`, `directory_reads_as_a_chunk`). Without a system chunk
    # the pin reads the system directory: on tmpfs `mmap` refuses it
    # (`memory_chunk.h:486-489`), the pin writes `mmap <dir> failed!`
    # (`pinyin.cpp:255-256`, `:289-290`, `zhuyin.cpp:199-200`) and dies of
    # SIGSEGV on the empty chunk in `SubPhraseIndex::load` (`phrase_index.cpp:354`);
    # the subject writes the same line and gives the abort site's answer.
    conf_abort('system-file-without-chunk', ('opengram.table opengram.bin opengram.dbin SYSTEM_FILE',
                                             'opengram.table NULL opengram.dbin SYSTEM_FILE'), dir_chunk='system')
    # Without a user file the pin reads the user directory, which holds the
    # `user.conf` libpinyin saved in `check_format` (`pinyin.cpp:187`); on tmpfs
    # the read is an ordinary `false`, and the pin and the subject both carry
    # on. The zhuyin facade writes nothing at init, and an empty directory is 0
    # bytes on some file systems, so it is not held.
    conf_abort('system-file-without-user-file', ('merged.table merged.bin merged.dbin SYSTEM_FILE',
                                                 'merged.table merged.bin NULL SYSTEM_FILE'), modes=('pinyin',),
               dir_chunk='user')
    conf_abort('user-file-without-user-file', ('user.bin USER_FILE', 'NULL USER_FILE'), modes=('pinyin',),
               dir_chunk='user')
    # A library call on a row the pin asserts on (`pinyin.cpp:457`, `:491`).
    conf_abort('load-library-not-used', ('opengram.dbin SYSTEM_FILE', 'opengram.dbin NOT_USED'),
               call=('load', 3), userdir=False)
    conf_abort('load-library-row-removed', ('-', 'GBK_DICTIONARY'), call=('load', 2), userdir=False)
    conf_abort('load-library-gb-not-used', (_STOCK_DBIN, 'gb_char.dbin NOT_USED'), call=('load', 1), userdir=False)
    conf_abort('load-library-user-not-used', ('user.bin USER_FILE', 'user.bin NOT_USED'), call=('load', 7),
               userdir=False)
    conf_abort('load-addon-system-file', ('art.bin NULL DICTIONARY', 'art.bin NULL SYSTEM_FILE'),
               call=('addon', 4), userdir=False, modes=('pinyin',))
    conf_abort('load-addon-user-file', ('art.bin NULL DICTIONARY', 'art.bin NULL USER_FILE'),
               call=('addon', 4), userdir=False, modes=('pinyin',))
    conf_abort('load-addon-without-chunk', ('art.table art.bin NULL DICTIONARY', 'art.table NULL NULL DICTIONARY'),
               call=('addon', 4), userdir=False, modes=('pinyin',), dir_chunk='system')
    # Class (b): the header's `source table format:` line is missing, and the pin
    # compares a buffer it never wrote (undefined behaviour).
    conf_abort('source-format-line-missing', ('-', 'source table format'))


    # The pin's ordinary `false`: a header directive that does not match answers
    # NULL with the raw `load %s failed!` line and no warning.
    def header_probe(*edits, issues=('694', '525')):
        """Build an initialization-only probe with text config edits.

        The issues argument is unused here; registration owns issue metadata."""
        def probe(k):
            """Create the edited private system and return the new context's truth value."""
            return {'ctx': bool(k.init(system=conf_system(k, *edits)))}
        probe.config_edits = edits
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
        case('table-conf-lambda-line-missing' + _suffix, mode=_mode,
             stderr=True, issues=('694', '525'))(header_probe(('-', 'lambda parameter')))
        # glibc's scanner does not back up: an exponent mark with no digit after
        # it, a `0x` with no hex digit and `infin` fail the `%f` conversion.
        for _name, _spelling in (('exponent-without-digits', '1e'), ('hex-without-digits', '0x'),
                                 ('hex-exponent-without-digits', '0x1p'), ('infin', 'infin')):
            # Each host's libc decides acceptance identically for both facades.
            case('table-conf-lambda-' + _name + _suffix, mode=_mode, stderr=True,
                 issues=('694', '525'))(header_probe(('lambda parameter:0.312699', 'lambda parameter:' + _spelling)))


    # Every spelling glibc's `%f` accepts, hexadecimal floats included.
    for _name, _spelling in (('hex', '0x1p-1'), ('hex-fraction', '0x.8'), ('hex-upper', '0X1.P-1')):
        conf_case('lambda-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _spelling),
                  modes=('pinyin', 'zhuyin'))


    # #919: table.conf scanner partitions. The reference image uses glibc 2.41; the
    # cases cover every partition the source walk distinguishes, not only the
    # spellings the stock file uses. `control=True` cases already agree with the
    # parent; the three incomplete-exponent spellings are the mutation targets.
    def conf_919_case(name, mutate, **options):
        """Register both facade sessions with a raw-byte config mutation under issue 919.

        Forward options to case registration; each callable applies mutate only when run."""
        for mode in ('pinyin', 'zhuyin'):
            def probe(k, mutate=mutate):
                """Apply the captured byte mutation to private table.conf and run the standard config session."""
                return conf_session(k, conf_system_bytes(k, mutate))
            probe.config_mutate = mutate
            case('table-conf-919-' + name + ('-zhuyin' if mode == 'zhuyin' else ''),
                 mode=mode, issues=('919',), **options)(probe)


    def one_line_conf(data):
        """Join the first six newline-delimited config fields with spaces and retain the remaining bytes.

        Insert two newlines before the remainder; fewer than six newlines raises IndexError."""
        parts = data.split(b'\n', 6)
        return b' '.join(parts[:6]) + b'\n\n' + parts[6]


    conf_919_case('header-one-line', one_line_conf, control=True)
    conf_919_case('header-crlf', lambda data: data.replace(b'\n', b'\r\n'), control=True)
    conf_919_case('header-no-final-newline', lambda data: data.rstrip(b'\n'), control=True)
    conf_919_case('header-indented-lambda',
                  lambda data: data.replace(b'\nlambda parameter:', b'\n   lambda parameter:'),
                  control=True)
    conf_919_case('word-nul-truncates',
                  lambda data: data.replace(
                      b'gb_char.table gb_char.bin gb_char.dbin',
                      b'gb_char.table gb_char.bin\x00junk gb_char.dbin'),
                  control=True)

    conf_919_case('source-token-nul',
                  lambda data: data.replace(b'source table format:pinyin', b'source table format:pinyin\x00suffix'))
    conf_919_case('database-token-nul',
                  lambda data: data.replace(b'database format:BerkeleyDB', b'database format:BerkeleyDB\x00suffix')
                      .replace(b'database format:KyotoCabinet', b'database format:KyotoCabinet\x00suffix')
                      .replace(b'database format:Tkrzw', b'database format:Tkrzw\x00suffix'))


    def header_919(name, *edits, control=True):
        """Register initialization-only header cases for both facades with stderr."""
        for mode in ('pinyin', 'zhuyin'):
            case('table-conf-919-' + name + ('-zhuyin' if mode == 'zhuyin' else ''),
                 mode=mode, stderr=True, issues=('919',), control=control)(header_probe(*edits))


    header_919('lambda-dot-only', ('lambda parameter:0.312699', 'lambda parameter:.'))
    header_919('header-leading-space', ('re', r'\Abinary', ' binary'))
    header_919('binary-version-line-missing', ('-', 'binary format version'))
    header_919('model-version-line-missing', ('-', 'model data version'))

    conf_abort('919-database-format-line-missing', ('-', 'database format'), issues=('919',), control=True)
    conf_abort('919-source-format-value-missing',
               ('re', r'source table format:\w+', 'source table format:'), issues=('919',), control=True)
    conf_abort('919-database-format-value-missing',
               ('re', r'database format:\w+', 'database format:'), issues=('919',), control=True)
    conf_abort('919-source-format-line-missing', ('-', 'source table format'), issues=('919',), control=True)
    conf_abort('919-word-width-overlong', ('+', 'x' * 300 + ' x.table x.bin x.dbin SYSTEM_FILE'),
               issues=('919',), control=True)

    conf_case('919-versions-positive-overflow',
              ('binary format version:7', 'binary format version:9223372036854775808'),
              ('model data version:14', 'model data version:99999999999999999999'),
              issues=('919',), control=False)
    conf_case('919-versions-int-wrap',
              ('binary format version:7', 'binary format version:4294967296'),
              ('model data version:14', 'model data version:2147483648'),
              issues=('919',), control=False)
    conf_case('919-versions-plus-sign',
              ('binary format version:7', 'binary format version:+7'),
              ('model data version:14', 'model data version:+14'),
              issues=('919',), control=True)
    conf_case('919-versions-negative',
              ('binary format version:7', 'binary format version:-1'),
              ('model data version:14', 'model data version:-2'),
              issues=('919',), control=False)
    header_919('versions-no-digits', ('binary format version:7', 'binary format version:x'))

    for _name, _spelling in (
            ('lambda-hex-no-exponent', '0x1'),
            ('lambda-dot-leading', '.5'),
            ('lambda-exponent-plus', '1e-1'),
            ('lambda-infinity', 'infinity'),
            ('lambda-nan-payload', 'nan(1)'),
            ('lambda-hex-overflow', '0x1p9999'),
            ('lambda-decimal-underflow', '1e-5000'),
            ('lambda-positive-sign', '+0.5')):
        conf_case('919-' + _name, ('lambda parameter:0.312699', 'lambda parameter:' + _spelling),
                  issues=('919',), control=True)

    for _name, _spelling in (
            ('lambda-exponent-sign-without-digits', '1e+'),
            ('lambda-hex-exponent-sign-without-digits', '0x1p+'),
            ('lambda-hex-exponent-minus-without-digits', '0x1p-'),
            ('lambda-hex-dot-without-digits', '0x.'),
            ('lambda-nan-unclosed', 'nan(')):
        for _mode in ('pinyin', 'zhuyin'):
            _suffix = '-zhuyin' if _mode == 'zhuyin' else ''
            case('table-conf-919-' + _name + _suffix, mode=_mode, stderr=True,
                 issues=('919',), control=True)(
                     header_probe(('lambda parameter:0.312699', 'lambda parameter:' + _spelling)))

    # The five-word row plus the next row's first word form a valid six-word
    # row with a library/table mismatch; the pin then reads a NULL item and
    # SIGSEGVs. The subject answers the same request without the invalid read:
    # class (b), recorded as the declared result rather than reproduced.
    conf_case('919-short-row-merged',
              ('+', 'default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin'),
              ('+', 'SYSTEM_FILE default RESERVED NULL NULL NULL'),
              issues=('919',), control=True, crash={'train': True, 'save': True},
              stderr_varies=True)
    conf_case('919-addon-atoi-nondigit',
              ('+', 'addon abc art.table art.bin NULL DICTIONARY'),
              modes=('pinyin',), session=addon_session, issues=('919',), control=True)
    conf_abort('919-addon-positive-overflow',
               ('+', 'addon 99999999999999999999 art.table art.bin NULL DICTIONARY'),
               modes=('pinyin',), issues=('919',), control=True)

    # #1297 Phase 2 accepted these fixtures as policy divergences of a Rust
    # scanner. External-I/O stage 1 reads table.conf with the pin's own
    # fscanf/atoi calls, so they run their original sessions and MATCH.
    def scanner_initialization(original):
        """Keep the fixture while observing the header parser before user-state work."""
        def probe(k):
            """Observe header acceptance with an already conforming user marker."""
            if hasattr(original, 'config_edits'):
                system = conf_system(k, *original.config_edits)
            else:
                system = conf_system_bytes(k, original.config_mutate)
            _write_user_conf(k)
            return {'ctx': bool(k.init(system=system))}
        return probe

    # Maintainer ruling 2026-10-28: both sides read lambda with the same host
    # libc. Preserve current main's initialization observations for these scanner
    # fixtures; every spelling must MATCH, including version-sensitive failures.
    for _name in ('lambda-hex', 'lambda-hex-fraction', 'lambda-hex-upper', 'lambda-inf', 'lambda-nan',
                  '919-lambda-hex-no-exponent', '919-lambda-infinity', '919-lambda-nan-payload',
                  '919-lambda-hex-overflow'):
        for _suffix in ('', '-zhuyin'):
            _spec = CASES['table-conf-' + _name + _suffix]
            _spec['fn'] = scanner_initialization(_spec['fn'])
            _spec['issues'] = tuple(dict.fromkeys((*_spec['issues'], '919')))
    for _name in ('lambda-exponent-without-digits', 'lambda-hex-exponent-without-digits',
                  '919-lambda-exponent-sign-without-digits',
                  '919-lambda-hex-exponent-sign-without-digits',
                  '919-lambda-hex-exponent-minus-without-digits',
                  '919-lambda-hex-dot-without-digits'):
        for _suffix in ('', '-zhuyin'):
            _spec = CASES['table-conf-' + _name + _suffix]
            _spec['issues'] = tuple(dict.fromkeys((*_spec['issues'], '919')))


    # Maintainer ruling 2026-10-28 UTC (D3): `SystemTableInfo2::load` is the
    # first step, and a table.conf it cannot open fails the init however
    # complete the rest of the directory is (074a2219 table_info.cpp:199-202,
    # pinyin.cpp:335-340, zhuyin.cpp:279-284).
    def missing_conf_with_dbms(k):
        """Initialize a private system copy that has every file but table.conf."""
        system = private_system(k)
        os.unlink(os.path.join(system, 'table.conf'))
        return {'ctx': bool(k.init(system=system))}


    for _mode in ('pinyin', 'zhuyin'):
        case('table-conf-missing-with-dbms' + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode,
             stderr=True, issues=('545',))(missing_conf_with_dbms)


    # A file name never leaves its directory: `g_build_filename(user_dir,
    # "/abs/u.bin")` is `<user_dir>/abs/u.bin`, not `/abs/u.bin`.
    def absolute_user_file(k):
        """Observe where a configured absolute-looking user filename is written.

        Create both the named nested directory and its user-root-prefixed counterpart,
        run a trained config session and return listings of both destinations."""
        nested = k.user + '/nest'
        os.makedirs(nested)
        os.makedirs(k.user + nested)
        out = conf_session(k, conf_system(k, ('user.bin USER_FILE', nested + '/u.bin USER_FILE')))
        out['beside'] = user_listing(k, nested)
        out['beneath'] = user_listing(k, k.user + nested)
        return out


    for _mode in ('pinyin', 'zhuyin'):
        case('table-conf-user-file-absolute' + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode, issues=('694', '525'))(absolute_user_file)

    # The `strtol` behind `%d` and `atoi` saturates at LONG_MIN on a negative
    # overflow, which the stores into an `int` and a `guint8` cut to 0.
    conf_case('versions-negative-overflow', ('binary format version:7', 'binary format version:-9223372036854775808'),
              ('model data version:14', 'model data version:-99999999999999999999'))
    conf_case('addon-index-negative-overflow', ('+', 'addon -99999999999999999999 art.table art.bin NULL DICTIONARY'),
              modes=('pinyin',), session=addon_session)

    # The names are the bytes the word holds.
    conf_case('user-file-not-utf8', ('user.bin USER_FILE', 'u\udcff.bin USER_FILE'))

    # `_write_files` writes the library files by sub-index, then the two indices
    # and the bigram, and `_rename_files` renames in the same order: where a file
    # of the set is given two roles one name, the later writer's contents are what
    # the first rename moves into place, and the second rename fails.
    def stderr_conf_case(name, *edits):
        """Register both facade config sessions with stderr comparison enabled."""
        for mode in ('pinyin', 'zhuyin'):
            def probe(k, edits=edits):
                """Build the captured edited system and return the standard session's observations."""
                return conf_session(k, conf_system(k, *edits))
            case('table-conf-' + name + ('-zhuyin' if mode == 'zhuyin' else ''), mode=mode, stderr=True, issues=('694', '525'))(probe)


    stderr_conf_case('colliding-library-files', ('network.bin USER_FILE', 'shared.bin USER_FILE'),
                     ('user.bin USER_FILE', 'shared.bin USER_FILE'))
    stderr_conf_case('library-named-like-an-index', ('user.bin USER_FILE', 'user_pinyin_index.bin USER_FILE'))
    stderr_conf_case('log-named-like-a-library', ('gb_char.dbin SYSTEM_FILE', 'user.bin SYSTEM_FILE'))
    # libpinyin's check_format saves the marker, then wipes a non-conforming
    # profile (074a2219 pinyin.cpp:187, :194-215): a library whose user file
    # is user.conf takes the fresh marker with it.
    stderr_conf_case('user-library-named-user-conf', ('user.bin USER_FILE', 'user.conf USER_FILE'))


    # Class (b): a library whose rows no longer match the index points the
    # candidate listing at items that are not there; the pin's duplicate removal
    # compares a string it never set (`pinyin.cpp:1635-1637`, `:2053`, SIGSEGV).
    def candidates_probe(*edits):
        """Build a candidate-guess case against the supplied config edits."""
        def probe(k):
            """Initialize the edited system, parse its first configured input and record sentence/candidate flags.

            Assert init and collect candidate rows through conf_guess_rows even though
            only its flag is returned. Leave the instance and context live."""
            ctx = k.init(system=conf_system(k, *edits))
            assert ctx, 'init failed'
            inst = k.fn('alloc_instance', P, P)(ctx)
            k.fn('parse_more_full_pinyins', Z, P, S)(inst, CONF_TEXTS[k.mode][0])
            sentence = k.fn('guess_sentence', B, P)(inst)
            ok = conf_guess_rows(k, inst)[0]
            return {'ret': ok, 'sentence': sentence}
        return probe


    for _mode in ('pinyin', 'zhuyin'):
        _domain = WARNING_DOMAIN[_mode]
        for _name, _edits in (
                ('files-swapped', (('gb_char.table gb_char.bin', 'gb_char.table TMP.bin'),
                                   ('gbk_char.table gbk_char.bin', 'gbk_char.table gb_char.bin'),
                                   ('TMP.bin', 'gbk_char.bin'))),
                ('duplicate-row', (('+', 'default GB_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE'),)),
                ('library-as-user-file', (('default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE',
                                           'default GB_DICTIONARY NULL NULL gb_char.dbin USER_FILE'),))):
            case('table-conf-candidates-' + _name + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode,
                 crash={'ret': False, 'logs': [[_domain, 16]]}, issues=('694', '525'))(candidates_probe(*_edits))
