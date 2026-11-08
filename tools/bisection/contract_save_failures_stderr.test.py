#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the save failures and stderr cases (#1175)."""
from pathlib import Path
import sys

import collections
import unittest
from unittest.mock import Mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_save_failures_stderr as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('save-929-logical-cross-open-pinyin', (), {'issues': ('929',), 'mode': 'pinyin'}),
            ('save-929-logical-cross-open-zhuyin', (), {'issues': ('929',), 'mode': 'zhuyin'})]
INJECTED = ('CASES', 'crafted_bigram_context', 'index_file_rows')


def stub(name):
    return collections.defaultdict(dict) if name == 'CASES' else Mock(name=name)


def declarations(state):
    return collect_declarations(family.register, CASES=state['CASES'], crafted_bigram_context=state['crafted_bigram_context'], index_file_rows=state['index_file_rows'])


class SaveFailuresStderrTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
