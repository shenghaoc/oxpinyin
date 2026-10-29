#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the helpers several contract case families share (#1175).

The encoders and walkers run here on literal bytes; the helpers that call the
facades are covered through the families that use them."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import Mock, patch

import contract_case_helpers as helpers


class ByteLayoutTests(unittest.TestCase):
    def test_gram_bytes_and_items_round_trip_in_stored_order_with_duplicates(self):
        raw = helpers._gram_bytes(9, [(3, 1), (2, 5), (3, 2)])
        self.assertEqual(raw[:4], (9).to_bytes(4, 'little'))
        self.assertEqual(helpers._gram_items(raw), [9, [[3, 1], [2, 5], [3, 2]]])
        self.assertEqual(helpers._gram_items(raw + b'\x01\x02\x03'), [9, [[3, 1], [2, 5], [3, 2]]])
        self.assertIsNone(helpers._gram_items(None))

    def test_chunk_frames_the_payload_with_its_length_and_xor_checksum(self):
        payload = bytes(range(1, 7))
        checksum = int.from_bytes(payload[:4], 'little') ^ payload[4] ^ (payload[5] << 8)
        self.assertEqual(helpers._chunk(payload), (6).to_bytes(4, 'little') + checksum.to_bytes(4, 'little') + payload)
        self.assertEqual(helpers._chunk(b''), bytes(8))

    def test_modify_header_record_and_log_walk_read_the_first_header(self):
        record = helpers._modify_header(0, 10, 20)
        self.assertEqual(record, (4).to_bytes(4, 'little') + bytes(4) + (4).to_bytes(2, 'little')
                         + (10).to_bytes(4, 'little') + (20).to_bytes(4, 'little'))
        header, counts = helpers.log_walk(helpers._chunk(record + helpers._modify_header(0, 20, 30)))
        self.assertEqual(header, [10, 20])
        self.assertEqual(counts, {'add': 0, 'remove': 0, 'modify': 0, 'header': 2})
        self.assertEqual(helpers.log_walk(helpers._chunk(record[:-1])), (None, dict(counts, header=0)))

    def test_write_log_writes_a_framed_chunk_under_the_user_directory(self):
        with tempfile.TemporaryDirectory() as user:
            helpers._write_log(Mock(user=user), b'abcd', name='x.dbin')
            self.assertEqual(Path(user, 'x.dbin').read_bytes(), helpers._chunk(b'abcd'))

    def test_ucs4_key_encodes_each_character_as_four_little_endian_bytes(self):
        self.assertEqual(helpers._ucs4_key('a你'), b'a\x00\x00\x00' + (0x4f60).to_bytes(4, 'little'))
        self.assertEqual(helpers._ucs4_key(''), b'')

    def test_glib_utf8_writes_every_form_from_one_to_six_bytes(self):
        self.assertEqual(helpers.glib_utf8([0x41, 0x4e01]), b'A' + '丁'.encode())
        self.assertEqual(helpers.glib_utf8([0xd800]), b'\xed\xa0\x80')
        self.assertEqual(len(helpers.glib_utf8([0x110000])), 4)
        self.assertEqual(len(helpers.glib_utf8([0x200000])), 5)
        self.assertEqual(len(helpers.glib_utf8([0x4000000])), 6)


class BigramContainerTests(unittest.TestCase):
    def test_kc_system_bigram_is_native_hash_and_user_bigram_is_snapshot(self):
        with tempfile.TemporaryDirectory() as root:
            Path(root, 'table.conf').write_text('database format:KyotoCabinet\n')
            kit = Mock(data=root)
            for name in ('bigram.db', 'user_bigram.db'):
                for writable in (False, True):
                    with self.subTest(name=name, writable=writable):
                        path, db = Path(root, name), Mock()
                        db.kcdbnew.return_value = 123
                        with patch('ctypes.util.find_library', return_value='libkyotocabinet.so.16'), \
                                patch.object(helpers.C, 'CDLL', return_value=db):
                            helpers.d18_bigram_db(kit, path, [] if writable else None)
                        snapshot = name == 'user_bigram.db'
                        target = b'-' if snapshot else (str(path) + '#type=kch').encode()
                        db.kcdbopen.assert_called_once_with(123, target, 6 if writable or snapshot else 1)
                        if snapshot and writable:
                            db.kcdbdumpsnap.assert_called_once_with(123, str(path).encode())
                        else:
                            db.kcdbdumpsnap.assert_not_called()
                        if snapshot and not writable:
                            db.kcdbloadsnap.assert_called_once_with(123, str(path).encode())
                        else:
                            db.kcdbloadsnap.assert_not_called()
                        db.kcdbclose.assert_called_once_with(123)
                        db.kcdbdel.assert_called_once_with(123)

    def test_tkrzw_system_bigram_opens_as_native_hash(self):
        with tempfile.TemporaryDirectory() as root:
            Path(root, 'table.conf').write_text('database format:Tkrzw\n')
            path, db, libc = Path(root, 'bigram.db'), Mock(), Mock()
            db.tkrzw_dbm_open.return_value = 123
            with patch('ctypes.util.find_library', return_value='libtkrzw.so.1'), \
                    patch.object(helpers.C, 'CDLL', side_effect=[db, libc]):
                helpers.d18_bigram_db(Mock(data=root), path, [])
            db.tkrzw_dbm_open.assert_called_once_with(str(path).encode(), True, b'dbm=HashDBM')
            db.tkrzw_dbm_close.assert_called_once_with(123)


class ObservationTests(unittest.TestCase):
    def test_transient_files_snapshots_nested_files_with_mode_and_bytes(self):
        with tempfile.TemporaryDirectory() as root:
            Path(root, 'sub').mkdir()
            Path(root, 'sub', 'a').write_bytes(b'1')
            snapshot = helpers.transient_files(root)
            self.assertEqual(list(snapshot), ['sub/a'])
            self.assertEqual(snapshot['sub/a'][1], b'1')

    def test_phrase_text_bytes_reports_untouched_null_and_frees_owned_text(self):
        kit = Mock()
        self.assertEqual(helpers.phrase_text_bytes(kit, helpers.UNTOUCHED), 'untouched')
        self.assertIsNone(helpers.phrase_text_bytes(kit, None))
        kit.glib.g_free.assert_not_called()

    def test_shared_constants_keep_their_values(self):
        self.assertEqual(helpers.FACADE_TOTAL, 51051831)
        self.assertEqual(helpers.USER_INDEX_FILES, ('user_phrase_index.bin', 'user_pinyin_index.bin'))
        self.assertEqual(helpers.USER_ALL, (0x0f000000, 0x07000000))


if __name__ == '__main__':
    unittest.main()
