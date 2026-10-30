#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the facade totals and counts cases (#1175)."""
from pathlib import Path
import sys

import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_facade_totals_counts as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('import-wrapped-facade-total-guess', (), {'issues': ('798',)}),
            ('import-wrapped-facade-total-segment', (), {'issues': ('798',)}),
            ('import-facade-total-below-wrap', (), {'control': True, 'issues': ('798',)}),
            ('stored-header-facade-total', (), {}),
            ('stored-header-facade-total-small', (), {'control': True}),
            ('system-add-reload-facade-total', (), {}),
            ('system-add-reload-facade-total-small', (), {'control': True}),
            ('token-add-unigram-numerator-guess', (), {'issues': ('801',)}),
            ('longer-choices-numerator-guess', (), {'issues': ('801',)}),
            ('unload-drops-accepted-bump', (), {'issues': ('801',)}),
            ('token-add-unigram-denominator-candidates', (), {'issues': ('540',)})]


def declarations():
    return collect_declarations(family.register, items_system=Mock(), _craft_db=Mock())


class FacadeTotalsCountsTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
