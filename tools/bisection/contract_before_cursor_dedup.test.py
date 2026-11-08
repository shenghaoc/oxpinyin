#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the S2 n-best witness of the before-cursor and longer dedup family (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_before_cursor_dedup as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('s2-nbest-longer-normal-precedence', (), {'cells': ('bdb',), 'issues': ('1398', '1347'), 'stderr': True,
                                                     'policy_record': 'trellis-adjusted-cost-s1-s2.md'})]


class BeforeCursorDedupTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = collect_declarations(family.register)
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
