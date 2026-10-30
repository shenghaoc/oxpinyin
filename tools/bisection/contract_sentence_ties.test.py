#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the sentence-tie fixture regeneration (#1175)."""
import inspect
from pathlib import Path
import tempfile
import unittest
from unittest import mock

import contract_sentence_ties as ties


class SentenceTieTests(unittest.TestCase):
    def test_the_declared_residuals_are_read_from_the_frozen_gate(self):
        names = ties.declared_sentence_residuals()
        self.assertEqual(len(names), 156)
        self.assertTrue(all(isinstance(name, str) and name for name in names))

    def test_a_row_pools_each_text_once_and_encodes_rows_by_pool_index(self):
        row = ['id', 'input', '7', 'ignored']
        encoded = ties.encode_sentence_tie(row, True, ['a', 'b'], [(0, 'a'), (1, 'c')], ['c'], [(2, 'b')])
        self.assertEqual(encoded.split('\t'), ['id', 'input', '7', '1', 'a\x01b\x01c', '0,1', '0:0,1:2', '2', '2:1'])

    def test_absent_subject_vectors_have_no_trailing_whitespace(self):
        encoded = ties.encode_sentence_tie(['id', '1', '7'], True,
                                           ['a'], [(0, 'a')], [], [])
        self.assertEqual(encoded.split('\t')[-2:], ['-', '-'])
        self.assertEqual(encoded, encoded.rstrip())

    def test_regeneration_takes_the_driver_kit_keyword_only(self):
        params = inspect.signature(ties.regenerate_sentence_ties).parameters
        self.assertEqual(list(params), ['prefix', 'fixture', 'Kit'])
        self.assertIs(params['Kit'].kind, inspect.Parameter.KEYWORD_ONLY)

    def test_regeneration_refuses_a_prefix_outside_the_reference_cell(self):
        source = (Path(__file__).resolve().parents[2] / 'tools/oracle/oracle-pin.txt').read_text()
        pin = dict(line.split('=', 1) for line in source.splitlines() if line and not line.startswith('#') and '=' in line)
        base = f"libpinyin-{pin['libpinyin_tag']}-{pin['libpinyin_commit']}+model20-{pin['model_archive_sha256']}"
        with tempfile.TemporaryDirectory() as scratch:
            prefix = Path(scratch)
            (prefix / 'oracle-pin.txt').write_text(f'pin_ref={base}+dbm-bdb\ndbm=BerkeleyDB\n')
            # CLOCK_BOOTTIME is Linux-only; the check under test runs before it matters.
            with mock.patch.object(ties.time, 'CLOCK_BOOTTIME', ties.time.CLOCK_MONOTONIC, create=True), \
                    self.assertRaisesRegex(AssertionError, '^off-pin tkrzw prefix$'):
                ties.regenerate_sentence_ties(prefix, prefix / 'fixture', Kit=None)


if __name__ == '__main__':
    unittest.main()
