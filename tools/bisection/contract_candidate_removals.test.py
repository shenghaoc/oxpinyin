#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the candidate removals and refusals cases (#1175)."""
import unittest
from unittest.mock import Mock

import contract_candidate_removals as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('abort-get-candidate-nbest-index-normal-row', (), {'abort': False, 'issues': ('525',)}),
            ('abort-remove-user-candidate-normal-system-token', (), {'abort': False, 'issues': ('525',)}),
            ('abort-remove-user-candidate-sentence-row', (), {'abort': False, 'issues': ('525',)}),
            ('abort-remove-user-candidate-twice', (), {'abort': False, 'issues': ('525',)}),
            ('abort-remove-user-candidate-missing-phrase-table', (), {'abort': False, 'issues': ('525',)}),
            ('abort-remove-user-candidate-multi-pron', (), {'abort': False, 'issues': ('525',)}),
            ('abort-remove-user-candidate-short-bigram-value', (), {'abort': False, 'issues': ('525',)}),
            ('abort-remove-user-candidate-non-token-bigram-key', (), {'abort': False, 'issues': ('525',)}),
            ('abort-remove-user-candidate-residual-bigram-gram', (), {'abort': False, 'issues': ('525',)}),
            ('abort-choose-candidate-predicted-prefix-row', (), {'abort': 0, 'issues': ('525',)}),
            ('abort-choose-predicted-candidate-normal-row', (), {'abort': False, 'issues': ('525',)}),
            ('abort-choose-predicted-candidate-sentence-row', (), {'abort': False, 'issues': ('525',)}),
            ('abort-choose-candidate-without-sentence-offset-1', (), {'abort': 0, 'issues': ('525',)}),
            ('choose-candidate-without-sentence-offset-0', (), {'control': True, 'issues': ('525',)}),
            ('candidate-type-neighbours', (), {'control': True, 'issues': ('525',)})]
INJECTED = ('_craft_hash',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, _craft_hash=state['_craft_hash'])
    return cases


class CandidateRemovalsTests(unittest.TestCase):
    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
