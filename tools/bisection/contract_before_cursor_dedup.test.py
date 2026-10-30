#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the before-cursor and longer dedup cases (#1175)."""
import unittest
from unittest.mock import Mock, patch

import contract_before_cursor_dedup as family


# (case ID, positional options, keyword options) in registration order.
EXPECTED = [('before-cursor-frequency-dedup', (), {'issues': ('913',), 'mode': 'zhuyin'}),
            ('before-cursor-frequency-tie', (), {'control': True, 'issues': ('913',), 'mode': 'zhuyin'}),
            ('longer-normal-frequency-dedup', (), {'issues': ('913',)}),
            ('longer-normal-frequency-tie', (), {'issues': ('913',)}),
            ('longer-normal-disabled', (), {'control': True, 'issues': ('913',)}),
            ('s2-nbest-longer-normal-precedence', (), {'cells': ('bdb',), 'issues': ('1398', '1347'), 'stderr': True,
                                                     'policy_record': 'trellis-adjusted-cost-s1-s2.md'}),
            ('issue762-addon-promote-no-mask', (), {'control': True, 'issues': ('762',)}),
            ('issue762-addon-promote-mask-highest', (), {'issues': ('762',)}),
            ('issue762-addon-promote-mask-middle', (), {'control': True, 'issues': ('762',)})]
INJECTED = ('addon_promote_sequence', 'items_system')


def stub(name):
    return Mock(name=name)


def declarations(state):
    cases = []

    def case(name, *args, **kwargs):
        def collect(fn):
            cases.append((name, args, kwargs, fn))
            return fn
        return collect

    family.register(case, **state)
    return cases


class BeforeCursorDedupTests(unittest.TestCase):
    def test_reduced_cases_check_complete_typed_lists_and_isolate_the_model(self):
        for name, dynamic, longer in (('longer-normal-frequency-dedup', True, True),
                                      ('longer-normal-frequency-tie', False, True),
                                      ('longer-normal-disabled', True, False)):
            with self.subTest(name=name):
                state = {n: stub(n) for n in INJECTED}
                fn = next(fn for case_name, _, _, fn in declarations(state) if case_name == name)
                k = Mock(ctx='ctx', inst='inst')
                returns = {'save': True, 'set_options': True, 'parse_more_full_pinyins': 4,
                           'guess_sentence': True}
                k.fn.side_effect = lambda name, *args: Mock(return_value=returns[name])
                normal = 2 if dynamic or not longer else 7
                expected = [([7, '甲乙'],) if longer else (), ([normal, '甲乙'],),
                            ([1, '甲乙'],), ([normal, '甲乙'],)]
                observations = [dict(ret=True, rows=list(rows)) for rows in expected]
                with patch.object(family, '_replace_user_bigram') as replace, \
                        patch.object(family, 'guess_rows', side_effect=observations):
                    out = fn(k)
                self.assertEqual(out['sentence_suppressed'], observations[-1])
                state['items_system'].assert_called_once_with(
                    k, [((0x7532, 0x4e59), (0x1034, 0x1180), 100),
                        ((0x7532, 0x4e59), (0x11b4, 0x102b), 100)], isolated=True)
                replace.assert_called_once_with(k, [(1, family._gram_bytes(1, [(0x01000001, 1)]))])
                # A leaked model row or the wrong surviving candidate type must fail.
                for wrong in ([[normal, '甲乙'], [2, '外']], [[7 if normal == 2 else 2, '甲乙']]):
                    broken = [dict(value) for value in observations]
                    broken[-1] = dict(ret=True, rows=wrong)
                    with patch.object(family, '_replace_user_bigram'), \
                            patch.object(family, 'guess_rows', side_effect=broken), \
                            self.assertRaises(AssertionError):
                        fn(k)

    def test_case_ids_order_and_metadata(self):
        cases = declarations({n: stub(n) for n in INJECTED})
        self.assertEqual([(name, args, kwargs) for name, args, kwargs, _ in cases], EXPECTED)


if __name__ == '__main__':
    unittest.main()
