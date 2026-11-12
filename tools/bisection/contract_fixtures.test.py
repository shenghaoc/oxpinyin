#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Regressions for the fixture builders the cross-open cases share (#1175)."""

from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import MagicMock, patch

sys.path.insert(0, str(Path(__file__).resolve().parent))

import contract_fixtures as fixtures

TABLE_CONF = ('binary format version:7\n'
              'model data version:3\n'
              'database format:BerkeleyDB\n'
              'default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\n')


class FakeKit:
    """The kit's data and user directories, all that _write_user_conf reads."""

    def __init__(self, data, user):
        self.data, self.user = data, user


class TemporaryTree(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.data = self.root / 'data'
        self.user = self.root / 'user'
        for directory in (self.data, self.user):
            directory.mkdir()
        (self.data / 'table.conf').write_text(TABLE_CONF)
        self.kit = FakeKit(data=str(self.data), user=str(self.user))


class UserDirectoryTests(TemporaryTree):
    def test_write_user_conf_copies_the_three_format_fields_and_zeroes_the_counter(self):
        fixtures._write_user_conf(self.kit)
        self.assertEqual((self.user / 'user.conf').read_text(),
                         'binary format version:7\nmodel data version:3\ndatabase format:BerkeleyDB\nopen counter:0\n')


class ContainerTests(unittest.TestCase):
    DUMP = ('VERSION=3\nformat=bytevalue\ntype=btree\nHEADER=END\n'
            ' 6162\n 0102\n 6161\n 00\nDATA=END\n')

    def test_index_file_rows_reads_bdb_through_db_dump_in_sorted_order(self):
        with patch.object(fixtures.subprocess, 'run', return_value=SimpleNamespace(stdout=self.DUMP.encode())) as run:
            rows = fixtures.index_file_rows('user.bin', cell='bdb')
        self.assertEqual(rows, [['6161', '00'], ['6162', '0102']])
        self.assertEqual(run.call_args.args[0], ['db5.3_dump', 'user.bin'])

    def test_index_file_rows_picks_the_reader_of_the_cell_it_is_given(self):
        library = MagicMock()
        library.kcdbnew.return_value = 0
        with patch.object(fixtures.C, 'CDLL', return_value=library) as load:
            with self.assertRaisesRegex(ValueError, 'KC profile allocation failed: x.bin'):
                fixtures.index_file_rows('x.bin', cell='kc')
            self.assertEqual(load.call_args.args, ('libkyotocabinet.so.16',))
        library = MagicMock()
        library.tkrzw_dbm_open.return_value = 0
        with patch.object(fixtures.C, 'CDLL', return_value=library) as load:
            with self.assertRaisesRegex(ValueError, 'tkrzw profile read-only open failed: x.bin'):
                fixtures.index_file_rows('x.bin', cell='tkrzw', library=Path('/anchor/libtkrzw.so.1'))
            self.assertEqual(load.call_args_list[0].args, (str(Path('/anchor/libtkrzw.so.1')),))
            self.assertEqual(library.tkrzw_dbm_open.call_args.args[2], b'dbm=TreeDBM')
            # Without a certified library, tkrzw resolves through the facade under test.
            load.reset_mock()
            with patch.object(fixtures, 'FACADE', '/facade.so'), \
                    self.assertRaisesRegex(ValueError, 'tkrzw profile read-only open failed'):
                fixtures.index_file_rows('x.bin', cell='tkrzw', kind='hash')
            self.assertEqual(load.call_args_list[0].args, ('/facade.so',))
            self.assertEqual(library.tkrzw_dbm_open.call_args.args[2], b'dbm=HashDBM')

    def test_craft_tkrzw_replaces_the_file_and_writes_every_record(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch, 'index.bin')
            path.write_bytes(b'old')
            library = MagicMock()
            library.tkrzw_dbm_open.return_value = 99
            library.tkrzw_dbm_set.return_value = True
            library.tkrzw_dbm_close.return_value = True
            with patch.object(fixtures, 'FACADE', '/facade.so'), \
                    patch.object(fixtures.C, 'CDLL', return_value=library) as load:
                fixtures._craft_tkrzw(str(path), [(b'k', b'v')], 'TreeDBM')
            load.assert_called_once_with('/facade.so')
            self.assertFalse(path.exists())
            library.tkrzw_dbm_open.assert_called_once_with(str(path).encode(), True, b'dbm=TreeDBM')
            library.tkrzw_dbm_set.assert_called_once_with(99, b'k', 1, b'v', 1, True)
            library.tkrzw_dbm_close.assert_called_once_with(99)
            path.write_bytes(b'kept')
            with patch.object(fixtures, 'FACADE', '/facade.so'), patch.object(fixtures.C, 'CDLL', return_value=library):
                fixtures._craft_tkrzw(str(path), [], 'HashDBM', keep=True)
            self.assertEqual(path.read_bytes(), b'kept')


if __name__ == '__main__':
    unittest.main()
