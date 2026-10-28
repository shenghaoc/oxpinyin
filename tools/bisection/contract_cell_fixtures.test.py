#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the fixtures that read the run's DBM cell (#1175).

The implementations take the cell, the crafters and their sibling fixtures as
keyword-only parameters; contract-modules.test.py checks that the driver's adapters
pass its current bindings. contract-diff.test.py covers the crafters themselves."""
import inspect
import unittest
from unittest.mock import Mock, call

import contract_cell_fixtures as fixtures


class CellFixtureTests(unittest.TestCase):
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
