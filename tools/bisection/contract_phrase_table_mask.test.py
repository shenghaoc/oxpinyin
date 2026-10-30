#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the phrase table mask cases (#1175)."""
import unittest
from unittest.mock import Mock

import contract_phrase_table_mask as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('phrase-table-mask-long-short', (), {'issues': ('759', '785')}),
            ('phrase-table-mask-duplicate-long-short', (), {'issues': ('759', '785')}),
            ('phrase-table-mask-post-removal-shrink', (), {'issues': ('759', '785')}),
            ('phrase-table-add-search-remove-roundtrip', (), {'control': True, 'issues': ('759', '785')}),
            ('phrase-table-never-loaded-library-filter', (), {'issues': ('799',)}),
            ('phrase-table-predict-nonunicode-raw-key', (), {}),
            ('phrase-table-predict-mismatched-live-prefix', (), {'control': True})]
INJECTED = ('_craft_db', 'index_file_rows')


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, _craft_db=state['_craft_db'], index_file_rows=state['index_file_rows'])
    return cases


class PhraseTableMaskTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
