#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the fixtures that read the run's DBM cell (#1175).

The implementations take the cell, the crafters and their sibling fixtures as
keyword-only parameters; contract-modules.test.py checks that the driver's adapters
pass its current bindings. contract-diff.test.py covers the crafters themselves."""
import inspect
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, call, patch

import contract_cell_fixtures as fixtures


class CellFixtureTests(unittest.TestCase):
    def test_isolated_items_replace_inherited_libraries_addon_indexes_and_bigrams(self):
        with tempfile.TemporaryDirectory() as root:
            source, system = Path(root, 'source'), Path(root, 'system')
            source.mkdir()
            system.mkdir()
            conf = ('binary format version:7\nmodel data version:14\ndatabase format:BerkeleyDB\n'
                    'lambda parameter:0.312699\n'
                    'default GB_DICTIONARY gb_char.table gb_char.bin gb_char.dbin SYSTEM_FILE\n'
                    'default GBK_DICTIONARY gbk_char.table gbk_char.bin gbk_char.dbin SYSTEM_FILE\n'
                    'default USER_DICTIONARY NULL NULL user.bin USER_FILE\n'
                    'addon 4 art.table art.bin NULL DICTIONARY\n')
            for name in ('gb_char.bin', 'pinyin_index.bin', 'phrase_index.bin', 'table.conf',
                         'addon_pinyin_index.bin', 'addon_phrase_index.bin', 'bigram.db'):
                Path(source, name).write_text(conf if name == 'table.conf' else 'original')
                Path(system, name).symlink_to(Path(source, name))
            k, craft, bigram = SimpleNamespace(data=str(source)), Mock(), Mock()
            with patch.object(fixtures, 'private_system', return_value=str(system)), \
                    patch.object(fixtures, '_write_user_conf'), \
                    patch.object(fixtures, 'd18_bigram_db', bigram):
                fixtures.items_system(k, [((0x7532, 0x4e59), (0x1034, 0x1180), 100),
                                         ((0x7532, 0x4e59), (0x11b4, 0x102b), 100)],
                                      isolated=True, _craft_db=craft)
            self.assertEqual(k.data, str(system))
            self.assertEqual(Path(system, 'gb_char.bin').stat().st_size, 91)
            self.assertEqual(Path(source, 'table.conf').read_text(), conf)
            isolated_conf = Path(system, 'table.conf').read_text()
            self.assertIn('default GB_DICTIONARY ', isolated_conf)
            self.assertNotIn('GBK_DICTIONARY', isolated_conf)
            self.assertNotIn('USER_DICTIONARY', isolated_conf)
            self.assertNotIn('addon 4', isolated_conf)
            self.assertEqual(craft.call_args_list[-2:], [
                call(str(Path(system, 'addon_pinyin_index.bin')), [], 1, system_index=True),
                call(str(Path(system, 'addon_phrase_index.bin')), [], 1, system_index=True)])
            bigram.assert_called_once_with(k, Path(system, 'bigram.db'), [])
            for name in ('gb_char.bin', 'pinyin_index.bin', 'phrase_index.bin',
                         'addon_pinyin_index.bin', 'addon_phrase_index.bin', 'bigram.db'):
                self.assertFalse(Path(system, name).is_symlink())
                self.assertEqual(Path(source, name).read_text(), 'original')

    def crafters(self):
        return {name: Mock(name=name, return_value=name) for name in ('_craft_bdb', '_craft_kc', '_craft_tkrzw')}

    def test_craft_db_dispatches_on_the_cell_it_is_given(self):
        for cell, dbtype, crafter, expected in (
                ('bdb', 2, '_craft_bdb', call('p', [(b'k', b'v')], 2, keep=True)),
                ('kc', 1, '_craft_kc', call('p', [(b'k', b'v')], keep=True)),
                ('tkrzw', 2, '_craft_tkrzw', call('p', [(b'k', b'v')], 'HashDBM', keep=True))):
            with self.subTest(cell=cell):
                crafters = self.crafters()
                self.assertEqual(fixtures._craft_db('p', [(b'k', b'v')], dbtype, True, CELL=cell, **crafters), crafter)
                self.assertEqual(crafters[crafter].call_args, expected)
                for other in set(crafters) - {crafter}:
                    crafters[other].assert_not_called()

    def test_kc_system_indexes_must_be_btrees(self):
        with self.assertRaisesRegex(AssertionError, 'btrees'):
            fixtures._craft_db('p', [], 2, system_index=True, CELL='kc', **self.crafters())

    def test_second_tier_fixtures_reach_the_fixture_they_are_given(self):
        craft_db = Mock()
        fixtures._craft_hash('p', [(b'k', b'v')], _craft_db=craft_db)
        craft_db.assert_called_once_with('p', [(b'k', b'v')], 2)
        items_system = Mock()
        fixtures.phrase_text_system('kit', 'valid', items_system=items_system)
        self.assertEqual(items_system.call_args.args[0], 'kit')
        self.assertEqual(items_system.call_args.args[1][0][0], (0x7532, 0x4e01, 0x4e59))

    def test_every_fixture_takes_its_driver_state_keyword_only(self):
        state = {'_craft_db': ('CELL', '_craft_bdb', '_craft_kc', '_craft_tkrzw'), '_craft_hash': ('_craft_db',),
                 'crafted_bigram_context': ('_craft_hash',), 'predicted_bigram_context': ('_craft_hash',),
                 'token_reuse_sequence': ('crafted_bigram_context', 'token_reuse_files'),
                 'token_reuse_files': ('CELL', 'index_file_rows'), 'addon_promote_sequence': ('token_reuse_files',),
                 'phrase_text_system': ('items_system',), 'items_system': ('_craft_db',)}
        for name, names in state.items():
            with self.subTest(name=name):
                params = inspect.signature(getattr(fixtures, name)).parameters.values()
                keyword_only = [p.name for p in params if p.kind is p.KEYWORD_ONLY and p.default is p.empty]
                self.assertEqual(keyword_only, list(names))


if __name__ == '__main__':
    unittest.main()
