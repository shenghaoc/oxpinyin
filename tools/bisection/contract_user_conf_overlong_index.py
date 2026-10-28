"""User configuration and overlong index cases (inventory row 56, #1175).
"""
import ctypes as C
import os
from pathlib import Path
import re
import shutil

from contract_abi import B, P, S, U, Z, unigram_of
from contract_case_helpers import token_unigram_trained, token_unigram_user
from contract_fixtures import _write_user_conf, private_system, user_listing


def register(case, *, _craft_db):
    """Register these cases at their original driver position."""
    def _craft_btree(path, keys):
        """A Berkeley DB btree of `key -> empty` records.

        The helper dispatches through _craft_db with tree type one, replacing the
        current cell's index container with byte keys mapped to empty values.
        That dispatch also supports KC snapshots and tkrzw TreeDBM; native failures
        follow the selected writer's assertions."""
        _craft_db(path, [(key, b'') for key in keys], 1)  # DB_BTREE == 1


    # #920: user.conf integer-overflow and state-preservation cases. The fixtures
    # derive every value from the running cell's table.conf; no database-format
    # literal is written. A conforming marker must retain the trained/imported
    # state, while a non-conforming one follows check_format's wipe.
    def _user_conf_values(k):
        """Read the available format-version integers from the cell's table.conf.

        Later occurrences replace earlier ones; absent keys are omitted and malformed
        integer values raise ValueError.
        """
        values = {}
        for line in Path(k.data, 'table.conf').read_text().splitlines():
            for key in ('binary format version:', 'model data version:'):
                if line.startswith(key):
                    values[key] = int(line[len(key):].strip())
        return values


    def _replace_conf_value(data, key, value):
        """Replace the first complete byte line beginning with key.

        The replacement is the encoded key followed by str(value); a missing key
        leaves data unchanged.
        """
        pattern = rb'(?m)^' + re.escape(key.encode()) + rb'.*$'
        return re.sub(pattern, key.encode() + str(value).encode(), data, count=1)


    def _edit_user_conf_versions(k, extra):
        """Return user.conf bytes with each table version increased by 2**32 + extra.

        Read the reference versions from this cell's table.conf; do not write either
        file. Both reference version keys must exist.
        """
        data = Path(k.user, 'user.conf').read_bytes()
        values = _user_conf_values(k)
        for key in ('binary format version:', 'model data version:'):
            data = _replace_conf_value(data, key, values[key] + (1 << 32) + extra)
        return data


    def _edit_user_conf_counter(k, value):
        """Return user.conf bytes with the first open-counter line replaced."""
        return _replace_conf_value(
            Path(k.user, 'user.conf').read_bytes(), 'open counter:', value)


    def _edit_user_conf_nul(k):
        """Insert a NUL and junk after the first database-format value in copied bytes.

        Read user.conf without modifying it; leave unmatched input unchanged.
        """
        data = Path(k.user, 'user.conf').read_bytes()
        return re.sub(
            rb'(?m)^(database format:)([^\r\n\x00]+)',
            lambda match: match.group(1) + match.group(2) + b'\x00junk',
            data, count=1)


    def _edit_user_conf_missing_database(k):
        """Return user.conf bytes with database-format and open-counter lines removed."""
        data = Path(k.user, 'user.conf').read_bytes()
        data = re.sub(rb'(?m)^database format:.*$', b'', data)
        return re.sub(rb'(?m)^open counter:.*$', b'', data)


    def _edit_user_conf_missing_counter(k):
        """Return user.conf bytes with every open-counter line removed."""
        data = Path(k.user, 'user.conf').read_bytes()
        return re.sub(rb'(?m)^open counter:.*$', b'', data)


    def _edit_user_conf_negative_overflow(k):
        """Return user.conf bytes with both version fields set to signed 64-bit minimum.

        Only existing first occurrences are replaced; the file is not written.
        """
        data = Path(k.user, 'user.conf').read_bytes()
        for key in ('binary format version:', 'model data version:'):
            data = _replace_conf_value(data, key, -9223372036854775808)
        return data


    def _user_conf_920_session(edit, keep_state):
        """Build a saved-profile reopen case with an edited user.conf.

        The callback edit(k) supplies replacement bytes. keep_state asserts retained
        frequencies only if reopening succeeds; the result still records a failed
        reopen. This exercises check_format at 074a2219 pinyin.cpp:172-218 and
        zhuyin.cpp:126-163.
        """
        def probe(k):
            """Train and import, save, replace the marker, and observe the reopened profile.

            Close the old instance and context before writing. Assert save success and,
            when requested and reopen succeeds, unchanged trained/imported frequencies;
            return those observations and the resulting user-file inventory.
            """
            training = token_unigram_trained(k)
            imported = token_unigram_user(k, add_frequency=False)
            saved = k.fn('save', B, P)(k.ctx)
            assert saved, 'the trained profile must save'
            k.fn('free_instance', None, P)(k.inst)
            k.fn('fini', None, P)(k.ctx)
            k._ctx = k._inst = None
            Path(k.user, 'user.conf').write_bytes(edit(k))
            ctx = k.init()
            out = {'reopen': bool(ctx), 'training': training, 'import': imported,
                   'save': saved}
            if ctx:
                k._ctx = ctx
                out['trained-after'] = unigram_of(k, int(training['token'], 16))
                out['import-after'] = unigram_of(k, int(imported['token'], 16))
                if keep_state:
                    assert out['trained-after'] == training['after'], 'trained state was wiped'
                    assert out['import-after'] == imported['after'], 'imported state was wiped'
            out['files'] = user_listing(k)
            return out
        return probe


    def _user_conf_920_case(name, edit, keep_state, control):
        """Register the edited-marker case for both facades under issue 920.

        Append -zhuyin to that facade's case name and pass control through to the
        registry; both cases share the requested edit and state-retention check.
        """
        for mode in ('pinyin', 'zhuyin'):
            suffix = '-zhuyin' if mode == 'zhuyin' else ''
            case('user-conf-920-' + name + suffix, mode=mode, issues=('920',),
                 control=control)(_user_conf_920_session(edit, keep_state))


    _user_conf_920_case('nul-database-token', _edit_user_conf_nul, True, False)
    _user_conf_920_case('version-overflow-conforms',
                         lambda k: _edit_user_conf_versions(k, 0), True, True)
    _user_conf_920_case('version-overflow-nonconforms',
                         lambda k: _edit_user_conf_versions(k, 1), False, True)
    _user_conf_920_case('counter-overflow-conforms',
                         lambda k: _edit_user_conf_counter(k, 1 << 32), True, True)
    _user_conf_920_case('counter-overflow-nonconforms',
                         lambda k: _edit_user_conf_counter(k, (1 << 32) + 7), False, True)
    _user_conf_920_case('negative-overflow-nonconforms',
                         _edit_user_conf_negative_overflow, False, True)
    _user_conf_920_case('database-missing-at-eof',
                         _edit_user_conf_missing_database, False, True)
    _user_conf_920_case('counter-missing',
                         _edit_user_conf_missing_counter, True, True)


    # Maintainer ruling 2026-10-28 UTC (D4): check_format ignores the marker
    # save's answer (074a2219 pinyin.cpp:187) and the context keeps its user
    # state. A directory named user.conf opens for reading and reads as end of
    # input, and cannot be opened for writing.
    def _unwritable_marker(k):
        """Train and save over a user dir whose user.conf is a directory."""
        os.mkdir(os.path.join(k.user, 'user.conf'))
        training = token_unigram_trained(k)
        saved = k.fn('save', B, P)(k.ctx)
        return {'training': training, 'save': saved, 'files': user_listing(k)}


    for _mode in ('pinyin', 'zhuyin'):
        case('user-conf-marker-unwritable' + ('-zhuyin' if _mode == 'zhuyin' else ''), mode=_mode,
             stderr=True, issues=('920',))(_unwritable_marker)


    def overlong_index_context(k, parse=None, rows=None, options=None):
        """A context whose user dir carries crafted `user_pinyin_index.bin` rows,
        by default the packed first syllable of `parse` and a 17-syllable key that
        extends it. The first init writes the conforming `user.conf`; the crafted
        keys are then added and the profile reopened, so the longer-candidate walk
        reaches them. `rows(first)` overrides the crafted key list — e.g. the exact
        prefix row can be dropped, or the over-long row's trailing byte made odd.
        `options` sets the option word of both the build context and the reopened
        one (e.g. `PINYIN_INCOMPLETE = 8`, so a partial first syllable projects to
        a zero-initial word).

        When parse is absent, use bytes 00 01 as the first two-byte key. The default
        rows are that key and its 17-fold repetition. Explicitly
        rewrite the container and user.conf before reopening. Assert initialization
        and key extraction, but not option-setting success. The returned context
        is not cached on the kit and must be finalized separately."""
        ctx = k.init()
        assert ctx, 'the first init failed'
        if options is not None:
            k.fn('set_options', B, P, U)(ctx, options)
        if parse is None:
            first = b'\x00\x01'
        else:
            inst = k.fn('alloc_instance', P, P)(ctx)
            assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, parse)
            key = C.c_void_p()
            assert k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(inst, 0, C.byref(key))
            first = C.string_at(key.value, 2)
            k.fn('free_instance', None, P)(inst)
        k.fn('fini', None, P)(ctx)
        if rows is None:
            rows = [first, first * 17]
        else:
            rows = rows(first)
        _craft_btree(os.path.join(k.user, 'user_pinyin_index.bin'), rows)
        _write_user_conf(k)
        ctx = k.init()
        assert ctx, 'the reopen failed'
        if options is not None:
            k.fn('set_options', B, P, U)(ctx, options)
        return ctx


    @case('abort-mask-out-overlong-index-key', abort=False, issues=('525', 'row:84'))
    def _(k):
        """Mask a reopened profile containing a 17-word user index key and record the flag."""
        ctx = overlong_index_context(k)
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    @case('abort-zhuyin-mask-out-overlong-index-key', mode='zhuyin', abort=False, issues=('525', 'row:84'))
    def _(k):
        """Run zhuyin mask_out against a reopened profile with a 17-word user index key."""
        # libzhuyin masks `m_pinyin_table` too (`zhuyin.cpp:763`), the same user
        # chewing table, so the same :529 walk aborts there.
        ctx = overlong_index_context(k)
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    @case('abort-guess-candidates-overlong-index-key', abort=False, issues=('525', 'row:84'))
    def _(k):
        """Guess ni candidates against a user index containing its key and a 17-word extension.

        Assert a nonempty parse, then enable the longer-candidate walk with sort zero."""
        ctx = overlong_index_context(k, parse=b'ni')
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
        # sort 0 clears SORT_WITHOUT_LONGER_CANDIDATE, so the walk runs and
        # reaches the crafted extension (`pinyin.cpp:2292-2293`).
        return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


    @case('abort-guess-candidates-overlong-odd-index-key', abort=False, issues=('525', 'row:84'))
    def _(k):
        """Guess ni candidates with a 35-byte user index key extending its parsed syllable.

        Plant the exact prefix as well, reopen and assert parsing before guessing."""
        # A 35-byte key is not one upstream writes, but `phrase_length =
        # db_key.size / sizeof(ChewingKey)` is integer division
        # (`chewing_large_table2_bdb.cpp:470`), so the pin reads it as 17 words
        # and its `default: abort()` dies. The subject classifies the complete-word
        # prefix the same way and drops the trailing byte only after.
        ctx = overlong_index_context(
            k, parse=b'ni', rows=lambda first: [first, first * 17 + b'\x07'])
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
        return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


    @case('abort-mask-out-overlong-odd-index-key', abort=False, issues=('525', 'row:84'))
    def _(k):
        """Mask a user index containing a normal key and a 17-word key with one trailing byte."""
        ctx = overlong_index_context(
            k, rows=lambda first: [first, first * 17 + b'\x07'])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    # A key of no whole word (empty, or one byte) reads as `size / 2 == 0` words
    # and `mask_out`'s `switch` has no such case: `default: abort()`
    # (`chewing_large_table2_bdb.cpp:529`, the KC twin :499, tkrzw :466).
    @case('abort-mask-out-empty-index-key', abort=False, userdir=True, issues=('525', 'row:84'))
    def _(k):
        """Mask a reopened user index containing an empty key and one normal key."""
        ctx = overlong_index_context(k, rows=lambda first: [b'', first])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    @case('abort-mask-out-one-byte-index-key', abort=False, userdir=True, issues=('525', 'row:84'))
    def _(k):
        """Mask a reopened user index containing a normal key and a one-byte key."""
        ctx = overlong_index_context(k, rows=lambda first: [first, b'\x07'])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    @case('abort-zhuyin-mask-out-empty-index-key', mode='zhuyin', abort=False, userdir=True, issues=('525', 'row:84'))
    def _(k):
        """Run zhuyin mask_out against an empty-key user index after reopening the profile."""
        ctx = overlong_index_context(k, rows=lambda first: [b'', first])
        return {'ret': k.fn('mask_out', B, P, U, U)(ctx, 0xFFFFFFFF, 0)}


    # The system `pinyin_index.bin` is the same `ChewingLargeTable2` btree, and
    # `FacadeChewingTable2::search_suggestion` walks it too
    # (`chewing_large_table2_bdb.cpp:282`): a key past 16 words that extends the
    # query dies at the same `default: abort()`. The stock index has none, so the
    # fixture is a private copy (never a link into the oracle) with one row added
    # in its native attached format (KC aborts at :269, tkrzw at :252).
    def system_overlong_index_context(k, trailing=b''):
        """Return a context on a private system index extended with a 17-word ni key.

        Extract ni's two-byte key, finalize its context and replace the private
        index's oracle symlink with a file copy. Add the repeated key plus trailing
        bytes without dropping existing records. Assert reopening against that
        private system directory; leave the returned context uncached on the kit."""
        ctx = k.init()
        assert ctx, 'the first init failed'
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
        key = C.c_void_p()
        assert k.fn('get_pinyin_key', B, P, Z, C.POINTER(P))(inst, 0, C.byref(key))
        first = C.string_at(key.value, 2)
        k.fn('free_instance', None, P)(inst)
        k.fn('fini', None, P)(ctx)
        system = private_system(k)
        path = os.path.join(system, 'pinyin_index.bin')
        os.unlink(path)  # the link into the oracle's data
        shutil.copyfile(os.path.join(k.data, 'pinyin_index.bin'), path)
        _craft_db(path, [(first * 17 + trailing, b'')], 1, keep=True)
        ctx = k.init(system=system)
        assert ctx, 'the open on the private system dir failed'
        return ctx


    @case('abort-guess-candidates-overlong-system-index-key', abort=False, userdir=True, issues=('525', 'row:84'))
    def _(k):
        """Guess ni candidates after appending a 17-word key to a copied system index."""
        ctx = system_overlong_index_context(k)
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
        return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


    @case('abort-guess-candidates-overlong-odd-system-index-key', abort=False, userdir=True, issues=('525', 'row:84'))
    def _(k):
        """Guess ni candidates after appending a 35-byte key to a copied system index."""
        ctx = system_overlong_index_context(k, trailing=b'\x07')
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
        return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


    @case('guess-after-overlong-index-key-without-prefix-answers-true', issues=('525', 'row:84'))
    def _(k):
        """Guess ni with an overlong user key present but its exact prefix record absent.

        Reopen the crafted profile and assert parsing, then record the guess flag."""
        # The pin's `search_suggestion` probes the exact query key first
        # (`cursorp->c_get(..., DB_SET)`, `chewing_large_table2_bdb.cpp:576`) and
        # answers SEARCH_NONE when it is absent, so an over-long key whose prefix
        # row was never written walks nothing and the abort is unreachable.
        ctx = overlong_index_context(k, parse=b'ni', rows=lambda first: [first * 17])
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
        return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}


    @case('reparse-after-guess-ignores-overlong-index-key', issues=('525', 'row:84'))
    def _(k):
        """Guess hao, then reparse ni in a profile carrying ni's overlong index extension.

        Return the first guess flag and consumed bytes from reparsing; do not guess
        candidates again after the replacement parse."""
        # `_prepend_longer_candidates` runs only from `pinyin_guess_candidates`
        # (`pinyin.cpp:2292-2293`); a parse refreshes the cached list without it,
        # so the pin cannot reach the abort from the re-parse.
        ctx = overlong_index_context(k, parse=b'ni')
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'hao')
        guessed = k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)
        reparsed = k.fn('parse_more_full_pinyins', Z, P, S)(inst, b'ni')
        return {'guess_hao': guessed, 'reparse_ni': reparsed}


    @case('abort-guess-candidates-overlong-incomplete-index-key', abort=False, issues=('525', 'row:84'))
    def _(k):
        """Guess n'an against crafted incomplete-key prefixes with PINYIN_INCOMPLETE enabled.

        Plant the two-word prefix and its 17-word extension, reopen with option
        word eight and assert parsing before guessing at zero with sort zero."""
        # `compute_incomplete_chewing_index` sets only `m_initial`, so the partial
        # `n` projects to its initial and the vowel-initial `an` to the zero
        # initial. The 17-word key extends that two-word projection, so the pin's
        # `default: abort()` dies at the guess.
        ctx = overlong_index_context(
            k,
            parse=b'n',
            options=8,  # PINYIN_INCOMPLETE
            rows=lambda first: [
                first + b'\x00\x00',
                first + b'\x00\x00' + b'\x01\x00' * 15,
            ],
        )
        inst = k.fn('alloc_instance', P, P)(ctx)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(inst, b"n'an")
        return {'ret': k.fn('guess_candidates', B, P, Z, U)(inst, 0, 0)}
