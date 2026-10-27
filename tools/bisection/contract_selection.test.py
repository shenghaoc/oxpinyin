#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Regressions for contract-diff's case selection (#1175).

The selection rules take the registry as a parameter, so these run them over a
small synthetic one: no driver, no 4,000-case registration. contract-diff.test.py
keeps the checks that go through the driver's CASES."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))

import contract_selection as selection

ALL = ('bdb', 'kc', 'tkrzw')


def entry(mode='pinyin', cells=ALL, issues=()):
    return dict(mode=mode, cells=cells, issues=issues)


REGISTRY = {
    'plain': entry(issues=('532',)),
    'zero-key/b': entry(mode='zhuyin', issues=('640', 'row:54')),
    'axis-c-options': entry(issues=('640',)),
    'zero-key/a': entry(issues=('640',)),
    'kc-only': entry(cells=('kc',), issues=('532',)),
    'separator/1/x': entry(issues=()),
}


class TagTests(unittest.TestCase):
    def test_tags_normalise_but_numbers_and_lane_tags_stay_distinct(self):
        for tag in ('#640', '640', ' 640 ', 640):
            self.assertEqual(selection.normal_tag(tag), '640')
        for tag in ('row:54', 'Row 54', 'row54', 'ROW:54', '#row 54'):
            self.assertEqual(selection.normal_tag(tag), 'row:54')
        self.assertEqual(selection.normal_tag('W11'), 'w11')
        self.assertEqual(selection.normal_tag('rowing'), 'row:ing')

    def test_a_family_is_the_text_before_the_first_slash(self):
        self.assertEqual(selection.family_of('zero-key/a'), 'zero-key')
        self.assertEqual(selection.family_of('separator/1/x'), 'separator')
        self.assertEqual(selection.family_of('/x'), '')
        self.assertIsNone(selection.family_of('plain'))

    def test_the_index_lists_sorted_ids_per_tag_and_family_and_leaves_axis_c_out(self):
        self.assertEqual(selection.tag_index(REGISTRY), {
            '532': ['kc-only', 'plain'],
            '640': ['zero-key/a', 'zero-key/b'],
            'family/separator': ['separator/1/x'],
            'family/zero-key': ['zero-key/a', 'zero-key/b'],
            'row:54': ['zero-key/b'],
        })
        self.assertEqual(selection.tag_index({}), {})

    def test_case_ids_are_split_on_commas_and_kept_as_given(self):
        self.assertEqual(selection.case_names('b,a,b'), ['b', 'a', 'b'])
        self.assertEqual(selection.case_names('a, b'), ['a', ' b'])
        for value in ('', ' ', ',', 'a,', ',a', 'a,,b', 'a, ,b'):
            with self.subTest(value=value), self.assertRaisesRegex(
                    selection.SelectionError, '--cases must not contain an empty value'):
                selection.case_names(value)

    def test_selection_errors_are_value_errors(self):
        self.assertTrue(issubclass(selection.SelectionError, ValueError))


class SelectTests(unittest.TestCase):
    def select(self, *args, **kwargs):
        return selection.select_cases(REGISTRY, *args, **kwargs)

    def test_with_no_selector_every_case_but_axis_c_is_chosen_in_sorted_order(self):
        names, reasons = self.select()
        self.assertEqual(names, ['kc-only', 'plain', 'separator/1/x', 'zero-key/a', 'zero-key/b'])
        self.assertEqual(set(map(tuple, reasons.values())), {('default suite',)})
        self.assertEqual(self.select(None), (names, reasons))
        self.assertEqual(self.select(None, (), ()), (names, reasons))

    def test_explicit_ids_keep_their_order_once_and_may_name_axis_c(self):
        names, reasons = self.select('zero-key/b,plain,zero-key/b,axis-c-options')
        self.assertEqual(names, ['zero-key/b', 'plain', 'axis-c-options'])
        self.assertEqual(reasons['zero-key/b'], ['--cases', '--cases'])

    def test_issues_and_families_add_sorted_ids_and_every_reason(self):
        names, reasons = self.select('plain', issues=['#532', 'Row 54', '640'], families=['zero-key'])
        self.assertEqual(names, ['plain', 'kc-only', 'zero-key/b', 'zero-key/a'])
        self.assertEqual(reasons, {'plain': ['--cases', 'issue 532'], 'kc-only': ['issue 532'],
                                   'zero-key/b': ['issue row:54', 'issue 640', 'family zero-key'],
                                   'zero-key/a': ['issue 640', 'family zero-key']})

    def test_a_repeated_selector_repeats_its_reason(self):
        names, reasons = self.select(issues=['640', '#640'])
        self.assertEqual(names, ['zero-key/a', 'zero-key/b'])
        self.assertEqual(reasons['zero-key/a'], ['issue 640', 'issue 640'])

    def test_an_axis_c_case_is_never_chosen_by_tag(self):
        self.assertNotIn('axis-c-options', self.select(issues=['640'])[0])

    def test_unknown_selectors_name_what_is_registered_and_never_fall_back(self):
        with self.assertRaises(selection.SelectionError) as caught:
            self.select(issues=['9999'])
        self.assertEqual(str(caught.exception),
                         "unknown issue tag '9999'; registered tags: 532, 640, row:54")
        with self.assertRaises(selection.SelectionError) as caught:
            self.select(issues=['family/zero-key'])
        self.assertNotIn('family/', str(caught.exception).split('registered tags: ')[1])
        with self.assertRaises(selection.SelectionError) as caught:
            self.select(families=['nope'])
        self.assertEqual(str(caught.exception), "unknown family 'nope'; registered families: separator, zero-key")
        with self.assertRaisesRegex(selection.SelectionError, 'unknown case: nope'):
            self.select('plain,nope')

    def test_an_empty_component_is_refused_even_beside_other_selectors(self):
        for cases in ('', ',', 'plain,'):
            for selectors in ({}, {'issues': ['532']}, {'families': ['zero-key']}):
                with self.subTest(cases=cases, selectors=selectors), self.assertRaisesRegex(
                        selection.SelectionError, '--cases must not contain an empty value'):
                    self.select(cases, **selectors)

    def test_an_empty_registry_selects_nothing(self):
        with self.assertRaisesRegex(selection.SelectionError, 'no cases selected'):
            selection.select_cases({})

    def test_the_registry_passed_is_the_one_selected_from(self):
        other = {'only': entry(issues=('1',))}
        self.assertEqual(selection.select_cases(other)[0], ['only'])
        self.assertEqual(selection.select_cases(other, issues=['1'])[0], ['only'])
        with self.assertRaises(selection.SelectionError):
            selection.select_cases(other, 'plain')


class PlanTests(unittest.TestCase):
    PREFIX = Path('/pin')

    def plan(self, names, reasons, zhuyin_so=None, cell='bdb'):
        return selection.plan_lines(REGISTRY, names, reasons, Path('/s/libpinyin_capi.so'), zhuyin_so, self.PREFIX, cell)

    def test_the_default_suite_is_a_count_and_the_artifacts_with_no_per_case_lines(self):
        names, reasons = selection.select_cases(REGISTRY)
        self.assertEqual(self.plan(names, reasons), [
            'plan: 5 cases (default suite: every registered non-axis-C case); cell bdb; facades: pinyin 4, zhuyin 1',
            '  1 case skip on this cell (fixture in another backend\'s format)',
            '  artifacts: subject /s/libpinyin_capi.so; pin /pin/lib/libpinyin.so; system tables /pin/lib/libpinyin/data'
            '; subject --zhuyin-so (missing); pin /pin/lib/libzhuyin.so'])
        self.assertEqual(len(self.plan(names, reasons, cell='kc')), 2)

    def test_an_explicit_selection_lists_each_id_with_its_facade_and_reasons(self):
        names, reasons = selection.select_cases(REGISTRY, 'plain,zero-key/b', issues=['640'])
        lines = self.plan(names, reasons, zhuyin_so=Path('/s/libzhuyin_capi.so'))
        self.assertEqual(lines[0], 'plan: 3 cases; cell bdb; facades: pinyin 2, zhuyin 1')
        self.assertEqual(lines[1], '  artifacts: subject /s/libpinyin_capi.so; pin /pin/lib/libpinyin.so; '
                                   'system tables /pin/lib/libpinyin/data; subject /s/libzhuyin_capi.so; '
                                   'pin /pin/lib/libzhuyin.so')
        self.assertEqual(lines[2:], ['  plain       pinyin  --cases',
                                     '  zero-key/b  zhuyin  --cases, issue 640',
                                     '  zero-key/a  pinyin  issue 640'])

    def test_a_single_case_is_singular(self):
        self.assertTrue(self.plan(['plain'], {'plain': ['--cases']})[0].startswith('plan: 1 case;'))


if __name__ == '__main__':
    unittest.main()
