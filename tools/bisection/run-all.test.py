#!/usr/bin/env python3
"""Regressions for run-all.py's issue resolution, runner timing and artifact selection."""
import json
from pathlib import Path
import runpy
import unittest

run_all = runpy.run_path(str(Path(__file__).with_name('run-all.py')))


class IssueResolutionTests(unittest.TestCase):
    index = {'tags': {'532': ['default-option-parse', 'explicit-use-tone-parse'], '640': ['separator/pinyin/0/parse/0'],
                      'family/separator': ['separator/pinyin/0/parse/0']},
             'modes': {'default-option-parse': 'pinyin', 'explicit-use-tone-parse': 'pinyin',
                       'separator/pinyin/0/parse/0': 'pinyin'}}

    def test_tags_normalise_like_the_contract_driver(self):
        self.assertEqual(run_all['normal_tag']('#W11'), 'w11')
        self.assertEqual(run_all['normal_tag']('Row 33'), 'row:33')

    def test_requested_tags_resolve_to_exact_case_ids(self):
        chosen = run_all['contract_cases_for']({'532', '640', '9999'}, self.index)
        self.assertEqual(chosen, {'default-option-parse': ['532'], 'explicit-use-tone-parse': ['532'],
                                  'separator/pinyin/0/parse/0': ['640']})
        self.assertEqual(run_all['contract_cases_for']({'9999'}, self.index), {})

    def test_plan_names_runners_inputs_cases_and_facades(self):
        row = dict(name='contract-diff', input_paths=['{capi}', '{zhuyin}'])
        other = dict(name='pc-version', oracle=False, input_paths=[])
        jobs = [(row, None, {}, dict(capi='/t/libpinyin_capi.so', zhuyin='/t/libzhuyin_capi.so')),
                (other, None, {}, {})]
        chosen = run_all['contract_cases_for']({'532'}, self.index)
        lines = run_all['plan_lines']('bdb', jobs, chosen, {'contract-diff': '--issue 532: 2 contract cases',
                                                            'pc-version': '--runners'}, self.index)
        self.assertEqual(lines[0], 'plan (bdb): 2 runners')
        self.assertIn('contract-diff: --issue 532: 2 contract cases; oracle: yes; inputs: /t/libpinyin_capi.so, /t/libzhuyin_capi.so', lines[1])
        self.assertIn('2 contract cases; facades: pinyin 2', lines[2])
        self.assertTrue(lines[3].strip().startswith('default-option-parse') and 'issue 532' in lines[3])
        self.assertIn('pc-version: --runners; oracle: no; inputs: none', lines[-1])

    def test_plan_resolves_env_placeholders_like_the_runner(self):
        row = dict(name='user-dir-round-trip', input_paths=['{capi}', '{env[OXPINYIN_USER_RT_TEST]}'],
                   env={'OXPINYIN_USER_RT_TEST': '{root}/target/rt-test'})
        substitutions = dict(root='/w', capi='/w/libpinyin_capi.so')
        self.assertEqual(run_all['planned_inputs'](row, {}, substitutions),
                         ['/w/libpinyin_capi.so', '/w/target/rt-test'])
        unset = dict(name='pc-metadata', input_paths=['{env[OXPINYIN_METADATA_PIN_ROOT]}'])
        self.assertEqual(run_all['planned_inputs'](unset, {'PATH': '/bin'}, {}),
                         ['{env[OXPINYIN_METADATA_PIN_ROOT]}'])
        self.assertEqual(run_all['planned_inputs'](unset, {'OXPINYIN_METADATA_PIN_ROOT': '/pin'}, {}), ['/pin'])
        lines = run_all['plan_lines']('bdb', [(unset, None, {'OXPINYIN_METADATA_PIN_ROOT': '/pin'}, {})], {},
                                      {'pc-metadata': 'default'}, {'tags': {}, 'modes': {}})
        self.assertIn('pc-metadata: default; oracle: yes; inputs: /pin', lines[1])


class TimingTests(unittest.TestCase):
    def test_runner_timing_line_is_read_from_its_output(self):
        output = 'noise\n{"case": "x", "verdict": "MATCH"}\n' + json.dumps(dict(timing=dict(cases=1, seconds={}))) + '\n'
        self.assertEqual(run_all['timing_of'](output), dict(cases=1, seconds={}))
        self.assertIsNone(run_all['timing_of']('pc-version: ok\n'))
        self.assertIsNone(run_all['timing_of']('{"timing": nonsense\n'))
class ArtifactUnionTests(unittest.TestCase):
    def test_union_is_ordered_and_deduplicated(self):
        rows = [dict(name='a', artifacts=['zhuyin-capi', 'pinyin-capi']), dict(name='b', artifacts=['pinyin-capi', 'dictool']),
                dict(name='c', artifacts=[])]
        self.assertEqual(run_all['artifact_union'](rows), ['pinyin-capi', 'zhuyin-capi', 'dictool'])

    def test_a_subjectless_selection_needs_no_build(self):
        self.assertEqual(run_all['artifact_union']([dict(name='pc-version', artifacts=[])]), [])

    def test_a_row_without_a_declaration_is_a_registry_error(self):
        with self.assertRaises(ValueError):
            run_all['artifact_union']([dict(name='x')])
        with self.assertRaises(ValueError):
            run_all['artifact_union']([dict(name='x', artifacts=['libfoo'])])

    def test_every_registered_gate_declares_its_set(self):
        rows = json.loads((Path(__file__).with_name('runners.json')).read_text())
        for row in rows:
            with self.subTest(runner=row['name']):
                self.assertIsInstance(row.get('artifacts'), list)
                self.assertFalse(set(row['artifacts']) - set(run_all['ARTIFACTS']))
                blob = json.dumps(row['command']) + json.dumps(row.get('input_paths', []))
                if '{capi}' in blob:
                    self.assertIn('pinyin-capi', row['artifacts'])
                if '{zhuyin}' in blob:
                    self.assertIn('zhuyin-capi', row['artifacts'])
        self.assertEqual(next(r for r in rows if r['name'] == 'pc-version')['artifacts'], [])
        self.assertEqual(next(r for r in rows if r['name'] == 'user-dir-round-trip')['artifacts'],
                         ['pinyin-capi', 'user-rt-test'])
        self.assertEqual(next(r for r in rows if r['name'] == 'import-diff')['artifacts'], ['pinyin-capi', 'dictool'])


if __name__ == '__main__':
    unittest.main()
