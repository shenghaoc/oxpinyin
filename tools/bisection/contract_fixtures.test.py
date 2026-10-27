#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for the fixture builders the contract-diff cases share (#1175)."""

import os
from pathlib import Path
import subprocess
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
    """Kit.fn hands back the Python function registered for the facade symbol."""

    def __init__(self, data='', user='', scratch='', **functions):
        self.data, self.user, self.scratch = data, user, scratch
        self.ctx = 'context'
        self.functions = functions

    def fn(self, name, res, *args):
        return self.functions[name]


class TemporaryTree(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.data = self.root / 'data'
        self.user = self.root / 'user'
        self.scratch = self.root / 'scratch'
        for directory in (self.data, self.user, self.scratch):
            directory.mkdir()
        (self.data / 'table.conf').write_text(TABLE_CONF)
        (self.data / 'gb_char.bin').write_bytes(b'system chunk')
        self.kit = FakeKit(data=str(self.data), user=str(self.user), scratch=str(self.scratch))


class SystemDirectoryTests(TemporaryTree):
    def test_private_system_links_every_entry_and_leaves_the_source_alone(self):
        system = Path(fixtures.private_system(self.kit))
        self.assertEqual(system.parent, self.scratch)
        self.assertTrue(system.name.startswith('sys-'))
        self.assertEqual(sorted(entry.name for entry in system.iterdir()), ['gb_char.bin', 'table.conf'])
        for entry in system.iterdir():
            self.assertEqual(Path(os.readlink(entry)), self.data / entry.name)

    def test_conf_system_applies_each_kind_of_edit_to_a_real_copy_of_table_conf(self):
        system = Path(fixtures.conf_system(
            self.kit, ('BerkeleyDB', 'KyotoCabinet'), ('+', 'extra row'), ('-', 'model data'),
            ('re', r'version:(\d)', r'version=\1')))
        text = (system / 'table.conf').read_text()
        self.assertFalse((system / 'table.conf').is_symlink())
        self.assertEqual(text, 'binary format version=7\ndatabase format:KyotoCabinet\n'
                               'default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\n'
                               'extra row\n')
        self.assertEqual((self.data / 'table.conf').read_text(), TABLE_CONF)

    def test_conf_system_refuses_an_edit_that_changes_nothing(self):
        for edit in (('absent text', 'x'), ('re', 'absent', 'x')):
            with self.subTest(edit=edit), self.assertRaises(AssertionError):
                fixtures.conf_system(self.kit, edit)


class UserDirectoryTests(TemporaryTree):
    def test_write_user_conf_copies_the_three_format_fields_and_zeroes_the_counter(self):
        fixtures._write_user_conf(self.kit)
        self.assertEqual((self.user / 'user.conf').read_text(),
                         'binary format version:7\nmodel data version:3\ndatabase format:BerkeleyDB\nopen counter:0\n')

    def test_write_user_conf_needs_every_field_in_table_conf(self):
        (self.data / 'table.conf').write_text('binary format version:7\n')
        with self.assertRaises(KeyError):
            fixtures._write_user_conf(self.kit)

    def test_user_listing_describes_each_kind_of_file(self):
        (self.user / 'user.conf').write_text('a:1\nb:2\n')
        (self.user / 'phrase.dbin').write_bytes(b'abc')
        (self.user / 'user_pinyin_index.bin').write_bytes((3).to_bytes(4, 'little') + bytes(4) + b'abc')
        (self.user / 'user_phrase_index.bin').write_bytes(b'not framed')
        (self.user / 'notes.txt').write_text('x')
        (self.user / 'sub').mkdir()
        self.assertEqual(fixtures.user_listing(self.kit), {
            'notes.txt': None, 'phrase.dbin': 3, 'sub': 'dir', 'user.conf': 'a:1|b:2|',
            'user_phrase_index.bin': 'dbm', 'user_pinyin_index.bin': 'chunk'})
        other = self.root / 'other'
        other.mkdir()
        (other / 'x.bin').write_bytes(b'12')
        self.assertEqual(fixtures.user_listing(self.kit, other), {'x.bin': 2})


class ImportTests(unittest.TestCase):
    def kit(self, accepted=True):
        self.calls = []

        def begin(ctx, library):
            self.calls.append(('begin', ctx, library))
            return 'iterator'

        def add(it, phrase, reading, count):
            self.calls.append(('add', it, phrase, reading, count))
            return accepted

        return FakeKit(begin_add_phrases=begin, iterator_add_phrase=add,
                       end_add_phrases=lambda it: self.calls.append(('end', it)))

    def test_import_rows_returns_the_add_flags_without_asserting_them(self):
        kit = self.kit(accepted=False)
        self.assertEqual(fixtures.import_rows(kit, 7, [('你', 'ni', 3), ('好', 'hao', 4)]), [False, False])
        self.assertEqual(self.calls, [('begin', 'context', 7), ('add', 'iterator', '你'.encode(), b'ni', 3),
                                      ('add', 'iterator', '好'.encode(), b'hao', 4), ('end', 'iterator')])

    def test_row58_import_uses_library_seven_and_asserts_each_add(self):
        kit = self.kit()
        fixtures.row58_import(kit, [('龘', 'da', 1)])
        self.assertEqual(self.calls, [('begin', 'context', 7), ('add', 'iterator', '龘'.encode(), b'da', 1),
                                      ('end', 'iterator')])
        with self.assertRaises(AssertionError):
            fixtures.row58_import(self.kit(accepted=False), [('龘', 'da', 1)])


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
            self.assertEqual(load.call_args_list[0].args, ('/anchor/libtkrzw.so.1',))
            self.assertEqual(library.tkrzw_dbm_open.call_args.args[2], b'dbm=TreeDBM')
            with self.assertRaisesRegex(ValueError, 'tkrzw profile read-only open failed'):
                fixtures.index_file_rows('x.bin', cell='tkrzw', kind='hash')
            self.assertEqual(library.tkrzw_dbm_open.call_args.args[2], b'dbm=HashDBM')

    def test_an_unknown_container_kind_is_refused_before_any_reader_opens(self):
        with patch.object(fixtures.C, 'CDLL', side_effect=AssertionError('opened')):
            for cell in ('bdb', 'kc', 'tkrzw'):
                with self.subTest(cell=cell), self.assertRaisesRegex(ValueError, 'unknown profile container kind: tuple'):
                    fixtures.index_file_rows('x.bin', cell=cell, kind='tuple')

    def test_the_cell_is_not_defaulted(self):
        with self.assertRaises(TypeError):
            fixtures.index_file_rows('x.bin')

    def test_zero_item_padding_clears_only_the_alignment_bytes_of_odd_key_items(self):
        key_hex = '61626364' + '6162'   # three 2-byte key words: odd, so each item carries 2 padding bytes
        stride = 4 + 2 * 3 + 2
        value = bytes(range(1, 1 + 2 * stride + 3))
        expected = bytearray(value)
        for start in (0, stride):
            expected[start + stride - 2:start + stride] = b'\0\0'
        self.assertEqual(fixtures.zero_item_padding(key_hex, value.hex()), expected.hex())
        even = '6162636465666768'
        self.assertEqual(fixtures.zero_item_padding(even, 'aabb'), 'aabb')
        self.assertEqual(fixtures.zero_item_padding(key_hex, ''), '')

    def test_craft_tkrzw_replaces_the_file_and_writes_every_record(self):
        with tempfile.TemporaryDirectory() as scratch:
            path = Path(scratch, 'index.bin')
            path.write_bytes(b'old')
            library = MagicMock()
            library.tkrzw_dbm_open.return_value = 99
            library.tkrzw_dbm_set.return_value = True
            library.tkrzw_dbm_close.return_value = True
            with patch.object(fixtures.C, 'CDLL', return_value=library):
                fixtures._craft_tkrzw(str(path), [(b'k', b'v')], 'TreeDBM')
            self.assertFalse(path.exists())
            library.tkrzw_dbm_open.assert_called_once_with(str(path).encode(), True, b'dbm=TreeDBM')
            library.tkrzw_dbm_set.assert_called_once_with(99, b'k', 1, b'v', 1, True)
            library.tkrzw_dbm_close.assert_called_once_with(99)
            path.write_bytes(b'kept')
            with patch.object(fixtures.C, 'CDLL', return_value=library):
                fixtures._craft_tkrzw(str(path), [], 'HashDBM', keep=True)
            self.assertEqual(path.read_bytes(), b'kept')


if __name__ == '__main__':
    unittest.main()
