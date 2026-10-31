#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the training and constraints cases (#1175)."""
from pathlib import Path
import sys

import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev.test_support import collect_declarations

import contract_training_constraints as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('abort-zhuyin-train-stale-forcing',
             (),
             {'abort': False, 'issues': ('525', 'row:6'), 'mode': 'zhuyin'}),
            ('abort-train-stale-forcing', (), {'sigabrt': True, 'abort': False, 'issues': ('525', 'row:6')}),
            ('abort-train-index-past-results',
             (),
             {'sigabrt': True, 'abort': False, 'issues': ('525', 'row:6'), 'userdir': True}),
            ('train-forcing-after-lookup', (), {'control': True, 'issues': ('525', 'row:6')}),
            ('constraint-survives-zhonghuarenmin', (), {'control': True, 'issues': ('825',)}),
            ('constraint-survives-zhonghuarm', (), {'control': True, 'issues': ('825',)}),
            ('constraint-survives-zhhrenmin', (), {'control': True, 'issues': ('825',)}),
            ('constraint-survives-zhonghrm', (), {'control': False, 'issues': ('825',)}),
            ('constraint-survives-zhhrm', (), {'control': False, 'issues': ('825',)}),
            ('constraint-survives-zhhrmghg', (), {'control': False, 'issues': ('825',)}),
            ('constraint-survives-reconcile-juezhonghuarm', (), {'control': True, 'issues': ('825',)}),
            ('constraint-survives-reconcile-juezhhrm', (), {'control': False, 'issues': ('825',)}),
            ('constraint-stale-item-keeps-nini', (), {'issues': ('825',)}),
            ('constraint-stale-item-drops-nihao', (), {'control': True, 'issues': ('825',)}),
            ('abort-train-after-reparse-past-matrix-end', (), {'sigabrt': True, 'abort': False, 'issues': ('525', 'row:82')}),
            ('abort-train-after-reparse-empty-start-column',
             (),
             {'sigabrt': True, 'abort': False, 'issues': ('525', 'row:82')}),
            ('train-after-reparse-internal-zero-key', (), {'control': True, 'issues': ('525', 'row:82')}),
            ('abort-train-after-reparse-empty-end-column', (), {'sigabrt': True, 'abort': False, 'issues': ('525', 'row:82')}),
            ('train-after-choose-lookup-control', (), {'control': True, 'issues': ('525', 'row:82')})]


def declarations():
    return collect_declarations(family.register)


class TrainingConstraintsTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations()
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)

if __name__ == '__main__':
    unittest.main()
