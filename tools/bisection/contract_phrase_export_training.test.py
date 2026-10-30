#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the phrase export and training cases (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_phrase_export_training as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('issue790-export-after-train-system', (), {'issues': ('790',)}),
            ('issue790-export-no-train-system', (), {'control': True, 'issues': ('790',)}),
            ('issue790-export-unconstrained-system', (), {'control': True, 'issues': ('790',)}),
            ('issue790-export-open-before-system', (), {'control': False, 'issues': ('790',)}),
            ('issue790-export-save-reopen-system', (), {'control': True, 'issues': ('790',)}),
            ('issue790-export-replayed-train-system', (), {'control': False, 'issues': ('790',)}),
            ('issue790-export-zero-and-signed-system', (), {'issues': ('790',)}),
            ('issue790-export-prefix-overflow-system', (), {'issues': ('790',)}),
            ('issue790-export-override-only-system', (), {'issues': ('790',)}),
            ('issue790-export-appended-system', (), {'issues': ('790',)}),
            ('issue790-guess-after-train-system', (), {'issues': ('790',)}),
            ('issue790-character-offset-after-train-system', (), {'issues': ('790',)}),
            ('issue759-export-open-across-import-system', (), {'issues': ('759',)}),
            ('issue759-export-open-across-import-user', (), {'issues': ('759',)}),
            ('user-library-training-bytes-pinyin', (), {'mode': 'pinyin'}),
            ('user-library-training-bytes-zhuyin', (), {'mode': 'zhuyin'})]


def declarations():
    return collect_declarations(family.register)


class PhraseExportTrainingTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
