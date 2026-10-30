#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the gram lookup cases (#1175)."""
import hashlib
import json
import unittest
from unittest.mock import Mock, patch

import contract_gram_lookup as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('user-gram-unsorted-guess', (), {'control': False, 'issues': ('757',)}),
            ('user-gram-unsorted-dynamic-adjust', (), {'control': False, 'issues': ('757',)}),
            ('user-gram-unsorted-phrase-segment', (), {'control': False, 'issues': ('757',), 'policy_record': '1297-gram-partition-segmentation.md'}),
            ('user-gram-duplicate-guess', (), {'control': False, 'issues': ('757',)}),
            ('user-gram-duplicate-dynamic-adjust', (), {'control': False, 'issues': ('757',)}),
            ('user-gram-duplicate-phrase-segment', (), {'control': True, 'issues': ('757',)}),
            ('user-gram-unsorted-range-boundary', (), {'issues': ('757',)}),
            ('system-gram-unsorted-guess', (), {'issues': ('757',)}),
            ('system-gram-duplicate-guess', (), {'issues': ('757',)}),
            ('user-gram-zero-count-guess', (), {'issues': ('757',)}),
            ('abort-guess-sentence-short-user-gram-system-row-pinyin',
             (),
             {'abort': False, 'issues': ('757',), 'mode': 'pinyin'}),
            ('abort-guess-sentence-short-user-gram-system-row-zhuyin',
             (),
             {'abort': False, 'issues': ('757',), 'mode': 'zhuyin'}),
            ('abort-guess-sentence-short-user-gram-no-system-row', (), {'abort': False, 'issues': ('757',)}),
            ('abort-guess-candidates-dynamic-adjust-short-user-gram-pinyin',
             (),
             {'abort': False, 'issues': ('757',), 'mode': 'pinyin'}),
            ('abort-guess-candidates-dynamic-adjust-short-user-gram-zhuyin',
             (),
             {'abort': False, 'issues': ('757',), 'mode': 'zhuyin'}),
            ('parse-dynamic-adjust-short-user-gram-pinyin', (), {'issues': ('757',), 'mode': 'pinyin'}),
            ('parse-dynamic-adjust-short-user-gram-zhuyin', (), {'issues': ('757',), 'mode': 'zhuyin'}),
            ('abort-phrase-segment-short-user-gram', (), {'abort': False, 'issues': ('757',)}),
            ('abort-phrase-segment-prefix-short-user-gram-pinyin',
             (),
             {'abort': False, 'issues': ('918',), 'mode': 'pinyin'}),
            ('phrase-segment-prefix-sound-user-gram-pinyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'pinyin', 'stderr': True}),
            ('phrase-segment-nonscalar-before-short-user-gram-pinyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'pinyin', 'stderr': True}),
            ('phrase-segment-prefix-unrelated-short-user-gram-pinyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'pinyin', 'stderr': True}),
            ('abort-phrase-segment-prefix-short-user-gram-zhuyin',
             (),
             {'abort': False, 'issues': ('918',), 'mode': 'zhuyin'}),
            ('phrase-segment-prefix-sound-user-gram-zhuyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'zhuyin', 'stderr': True}),
            ('phrase-segment-nonscalar-before-short-user-gram-zhuyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'zhuyin', 'stderr': True}),
            ('phrase-segment-prefix-unrelated-short-user-gram-zhuyin',
             (),
             {'control': True, 'issues': ('918',), 'mode': 'zhuyin', 'stderr': True}),
            ('phrase-segment-nonscalar-raw-key-pinyin',
             (),
             {'issues': ('918',), 'mode': 'pinyin', 'stderr': True}),
            ('phrase-segment-nonscalar-raw-key-zhuyin',
             (),
             {'issues': ('918',), 'mode': 'zhuyin', 'stderr': True}),
            ('guess-unrelated-short-user-gram', (), {'control': True, 'issues': ('757',)}),
            ('guess-candidates-short-user-gram-dynamic-adjust-off', (), {'control': True, 'issues': ('757',)})]
INJECTED = ('_craft_db',)


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, _craft_db=state['_craft_db'])
    return cases


class GramLookupTests(unittest.TestCase):
    def test_loading_the_fixture_preserves_its_stored_order(self):
        fixture = json.loads(family._INTERLEAVE_FIXTURE.read_text())
        # A small synthetic system sequence tests loading independently of
        # the installed oracle data. Keep the captured user layout intact.
        system = [fixture['boosted_token']]
        fixture['system_token_count'] = len(system)
        fixture['system_tokens_sha256'] = hashlib.sha256(
            system[0].to_bytes(4, 'little')).hexdigest()
        source = Mock()
        source.read_text.return_value = json.dumps(fixture)
        with patch.object(family, '_INTERLEAVE_FIXTURE', source):
            self.assertEqual(family._interleave_miss_layout(system, system[0]),
                             [tuple(item) for item in fixture['items']])
        self.assertEqual(system, [fixture['boosted_token']])

    def test_interleave_fixture_keeps_the_captured_layout_and_provenance(self):
        fixture = json.loads(family._INTERLEAVE_FIXTURE.read_text())
        self.assertEqual(fixture['case'], 'user-gram-unsorted-phrase-segment')
        self.assertEqual(fixture['items'], [[0x10059f5, 1 << 31], [0x10059fc, 1], [0x10059f4, 1]])
        self.assertEqual(fixture['total'], (1 << 31) + 2)
        self.assertEqual(fixture['provenance']['libpinyin_commit'],
                         '074a2219c90feaf962d0d24f034514033ece5f99')
        self.assertEqual(fixture['provenance']['pin_sites'], [
            'src/storage/ngram.cpp:239', 'src/storage/ngram.cpp:308-349', 'src/include/stl_lite.h:29'])
        self.assertEqual(fixture['provenance']['pin_derivation'], {
            'merged_length': 49739, 'lower_bound_index': 14878,
            'first_walk_token': 0x10059fe, 'finds_boosted_token': False})

    def test_interleave_fixture_refuses_changed_inputs_without_reselection(self):
        with self.assertRaisesRegex(AssertionError, 'boosted phrase token changed'):
            family._interleave_miss_layout([], 0)
        with self.assertRaisesRegex(AssertionError, 'system start gram changed'):
            family._interleave_miss_layout([], 0x10059f5)
        fixture = json.loads(family._INTERLEAVE_FIXTURE.read_text())
        fixture['system_token_count'] = 1
        with patch.object(family.json, 'loads', return_value=fixture):
            with self.assertRaisesRegex(AssertionError, 'stored system token sequence changed'):
                family._interleave_miss_layout([1], 0x10059f5)

    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
