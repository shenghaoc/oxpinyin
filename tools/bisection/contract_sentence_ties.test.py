#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the sentence-tie fixture regeneration (#1175)."""
import inspect
import unittest

import contract_sentence_ties as ties


class SentenceTieTests(unittest.TestCase):
    def test_the_declared_residuals_are_read_from_the_frozen_gate(self):
        names = ties.declared_sentence_residuals()
        self.assertEqual(len(names), 6)
        self.assertTrue(all(isinstance(name, str) and name for name in names))

    def test_a_row_pools_each_text_once_and_encodes_rows_by_pool_index(self):
        row = ['id', 'input', '7', 'ignored']
        encoded = ties.encode_sentence_tie(row, True, ['a', 'b'], [(0, 'a'), (1, 'c')], ['c'], [(2, 'b')])
        self.assertEqual(encoded.split('\t'), ['id', 'input', '7', '1', 'a\x01b\x01c', '0,1', '0:0,1:2', '2', '2:1'])

    def test_regeneration_takes_the_driver_kit_keyword_only(self):
        params = inspect.signature(ties.regenerate_sentence_ties).parameters
        self.assertEqual(list(params), ['prefix', 'fixture', 'Kit'])
        self.assertIs(params['Kit'].kind, inspect.Parameter.KEYWORD_ONLY)


if __name__ == '__main__':
    unittest.main()
