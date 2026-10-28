"""Phrase table mask cases (inventory row 38, #1175).
"""
import ctypes as C
from pathlib import Path

from contract_abi import Arr, B, I, P, S, U, Z, tokens_of
from contract_case_helpers import (
    TOKEN_REUSE_PHRASES, _add_user_phrase, _user_candidate_rows, d18_export_phrases, phrase_table_reopen, row58_rows)
from contract_fixtures import _write_user_conf


def register(case, *, _craft_db, index_file_rows):
    """Register these cases at their original driver position."""
    # #759 PR1 / #785: phrase records are token arrays, including duplicates.
    # The kc/tkrzw mask walk reuses an entry; MemoryChunk::set_content does not
    # shrink its length. Compare complete phrase records, independently of the
    # still-unported chewing-table half of #785.
    def phrase_table_lookup(k, text):
        """Look up a Unicode phrase and return [native flag, token list] in native order.

        Allocate a four-byte-element GArray and free it on normal completion,
        including when lookup returns false. Preserve duplicate tokens."""
        new = k.glib.g_array_new
        new.restype, new.argtypes = P, [I, I, U]
        k.glib.g_array_free.restype, k.glib.g_array_free.argtypes = P, [P, I]
        arr = new(0, 0, 4)
        ret = k.fn('lookup_tokens', B, P, S, P)(k.inst, text.encode(), arr)
        view = C.cast(arr, C.POINTER(Arr)).contents
        tokens = list(C.cast(view.data, C.POINTER(U))[:view.len])
        k.glib.g_array_free(arr, True)
        return [ret, tokens]


    def phrase_table_mask_records(duplicates, remove_middle=False):
        """Build a raw phrase-table mask/save/reopen case with long and short token records.

        duplicates repeats the first token; remove_middle masks the second token
        rather than a nonmatching token. Key order is raw UTF-32LE byte order."""
        def probe(k):
            """Persist four phrases, replace the raw phrase index, mask and compare saved/reopened records.

            Close/reset before writing, preserve deliberately reassigned key/token pairs,
            and use an unconstrained wo training call before save. Return exports,
            lookups and sorted raw rows; leave the reopened context live."""
            phrases = TOKEN_REUSE_PHRASES[:4]
            for text, reading, _ in phrases:
                _add_user_phrase(k, text, [reading.encode()], 100)
            tokens = [phrase_table_lookup(k, text)[1][0] for text, _, _ in phrases]
            assert k.fn('save', B, P)(k.ctx)
            k.fn('free_instance', None, P)(k._inst)
            k._inst = None
            k.fn('fini', None, P)(k._ctx)
            k._ctx = None
            # Explicit byte-key order: the long record is visited before the
            # short one on each backend. The long tail survives a no-match mask.
            keys = sorted(text.encode('utf-32-le') for text, _, _ in phrases)
            long_tokens = [tokens[0], tokens[0], tokens[1]] if duplicates else sorted(tokens[:3])
            rows = [(keys[0], b''.join(t.to_bytes(4, 'little') for t in long_tokens)),
                    (keys[1], tokens[3].to_bytes(4, 'little'))]
            path = Path(k.user, 'user_phrase_index.bin')
            _craft_db(str(path), rows, 1)
            _write_user_conf(k)
            assert k.ctx
            out = {'loaded': [phrase_table_lookup(k, key.decode('utf-32-le')) for key, _ in rows],
                   'export_before': d18_export_phrases(k)}
            masked_token = tokens[1] if remove_middle else 0x0700fffe
            out['masked'] = k.fn('mask_out', B, P, U, U)(k.ctx, 0xffffffff, masked_token)
            # mask_out does not set m_modified. An unconstrained train marks
            # the context dirty without editing either crafted phrase record.
            assert k.fn('set_options', B, P, U)(k.ctx, 1 << 5)
            assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'wo') == 2
            assert k.fn('guess_sentence', B, P)(k.inst)
            out['trained'] = k.fn('train', B, P, C.c_ubyte)(k.inst, 0)
            out['saved'] = k.fn('save', B, P)(k.ctx)
            out['stored'] = index_file_rows(path)
            out['export_after'] = d18_export_phrases(k)
            phrase_table_reopen(k)
            out['reopened'] = [phrase_table_lookup(k, key.decode('utf-32-le')) for key, _ in rows]
            out['export_reopened'] = d18_export_phrases(k)
            return out
        return probe


    # bdb controls the absence of shared-scratch tails. Both cases also detect
    # reconstruction from phrase items when saving: these raw records, including
    # their key/token associations, must survive without being derived anew.
    case('phrase-table-mask-long-short', issues=('759', '785'))(
        phrase_table_mask_records(False))
    case('phrase-table-mask-duplicate-long-short', issues=('759', '785'))(phrase_table_mask_records(True))
    # Removing the middle token shrinks the scratch entry before the short row.
    # Carry its post-mask length, not the maximum length previously encountered;
    # retaining the old maximum would duplicate the surviving final token.
    case('phrase-table-mask-post-removal-shrink', issues=('759', '785'))(
        phrase_table_mask_records(False, remove_middle=True))


    @case('phrase-table-add-search-remove-roundtrip', control=True, issues=('759', '785'))
    def _(k):
        """Observe 龘龘 through add, repeated add, save/reopen, candidate removal and re-add.

        Assert complete dada parsing and a displayed user candidate, then save/reopen
        again and export the final library-seven rows."""
        text, reading = '龘龘', b"da2'da2"
        out = {'absent': phrase_table_lookup(k, text)}
        out['added'] = _add_user_phrase(k, text, [reading], 100)
        out['found'] = phrase_table_lookup(k, text)
        out['added_again'] = _add_user_phrase(k, text, [reading], 1)
        out['found_again'] = phrase_table_lookup(k, text)
        out['saved'] = k.fn('save', B, P)(k.ctx)
        phrase_table_reopen(k)
        out['reopened'] = phrase_table_lookup(k, text)
        assert k.fn('parse_more_full_pinyins', Z, P, S)(k.inst, b'dada') == 4
        assert k.fn('guess_candidates', B, P, Z, U)(k.inst, 0, 0)
        candidates = _user_candidate_rows(k, k.inst)
        assert candidates
        out['removed'] = k.fn('remove_user_candidate', B, P, P)(k.inst, candidates[0])
        out['after_remove'] = phrase_table_lookup(k, text)
        out['readded'] = _add_user_phrase(k, text, [reading], 100)
        out['after_readd'] = phrase_table_lookup(k, text)
        out['saved_again'] = k.fn('save', B, P)(k.ctx)
        phrase_table_reopen(k)
        out['final'] = phrase_table_lookup(k, text)
        out['export'] = d18_export_phrases(k)
        return out


    # #799 review: prepare_tokens only enables instantiated sub-indices
    # (074a2219 storage/phrase_index.h:762-772); PhraseTableEntry::search skips
    # NULL arrays (storage/phrase_large_table3.cpp:40-42).
    @case('phrase-table-never-loaded-library-filter', issues=('799',))
    def _(k):
        """Plant phrase-table rows for an unloaded library-fifteen token and record lookup/prediction outputs.

        Write a conforming marker before lazy initialization and retain an empty
        prefix record alongside the two token records."""
        token = (15 << 24) | 1  # No library 15 is loaded by this fixture.
        value = token.to_bytes(4, 'little')
        prefix = '你'.encode('utf-32-le')
        _craft_db(str(Path(k.user, 'user_phrase_index.bin')),
                  [(prefix, b''), (prefix + '龘'.encode('utf-32-le'), value),
                   ('龘龘'.encode('utf-32-le'), value)], 1)
        _write_user_conf(k)
        return {'lookup': phrase_table_lookup(k, '龘龘'),
                'prediction': row58_rows(k, '你')}


    def phrase_table_raw_key_prediction(nonunicode):
        """Build a prediction case whose phrase-table prefix differs from the live item's text.

        nonunicode selects raw suffix 0x110000; otherwise use 好 after prefix 你."""
        def probe(k):
            """Persist 龘龘, replace its phrase index with the selected raw key and predict after 你.

            Close/reset cached objects before writing the index and marker; reopening is
            lazy. Record the resulting prediction list without decoding the raw suffix."""
            _add_user_phrase(k, '龘龘', [b"da2'da2"], 100)
            token = next(t for t in tokens_of(k, '龘龘') if t >> 24 == 7)
            assert k.fn('save', B, P)(k.ctx)
            k.fn('free_instance', None, P)(k._inst)
            k._inst = None
            k.fn('fini', None, P)(k._ctx)
            k._ctx = None
            prefix = '你'.encode('utf-32-le')
            suffix = (0x110000).to_bytes(4, 'little') if nonunicode else '好'.encode('utf-32-le')
            _craft_db(str(Path(k.user, 'user_phrase_index.bin')),
                      [(prefix, b''), (prefix + suffix, token.to_bytes(4, 'little'))], 1)
            _write_user_conf(k)
            # search_suggestion compares byte prefixes, never decodes the key
            # (phrase_large_table3_bdb.cpp:225-252). The candidate uses the live
            # item length (pinyin.cpp:2391-2395) and strips m_begin characters
            # from its live text (:2018-2022), with no starts_with check. Both
            # fixtures therefore display 龘, despite the table key starting 你.
            return row58_rows(k, '你')
        return probe


    case('phrase-table-predict-nonunicode-raw-key')(
        phrase_table_raw_key_prediction(True))
    case('phrase-table-predict-mismatched-live-prefix', control=True)(
        phrase_table_raw_key_prediction(False))
