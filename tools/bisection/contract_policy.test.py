#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the compact discrepancy-record entries (contract_policy)."""
import hashlib
import json
from pathlib import Path
import signal
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))

import contract_policy as policy

PIN = dict(exit=0, result={'ret': True, 'rows': [[2, 'a'], [2, 'b'], [2, 'c']], 'logs': []}, stderr='')
SUBJECT = dict(exit=0, result={'ret': True, 'rows': [[1, 'x'], [2, 'a'], [2, 'c']], 'logs': []}, stderr='')


class CanonicalTests(unittest.TestCase):
    def test_the_canonical_text_ignores_key_order_and_escapes_everything(self):
        self.assertEqual(policy.canonical({'b': 1, 'a': [True, None, '甲']}), '{"a":[true,null,"\\u7532"],"b":1}')
        self.assertEqual(policy.canonical({'a': 1, 'b': 2}), policy.canonical({'b': 2, 'a': 1}))
        self.assertNotEqual(policy.canonical({'ret': True}), policy.canonical({'ret': 1}))
        self.assertEqual(policy.canonical('\udcff'), '"\\udcff"', 'a surrogate-escaped stderr byte stays ASCII')

    def test_the_digest_is_sha256_of_the_canonical_text(self):
        self.assertEqual(policy.digest(PIN), hashlib.sha256(policy.canonical(PIN).encode('ascii')).hexdigest())
        self.assertEqual(len(policy.digest(PIN)), 64)
        value = {'甲\udcff': ''.join(chr(code) for code in range(0xd800, 0xe000))}
        self.assertEqual(policy.digest(value), hashlib.sha256(policy.canonical(value).encode('ascii')).hexdigest())
        self.assertNotEqual(policy.digest('\udcff'), policy.digest('\ufffd'))

    def test_field_digests_cover_every_top_level_field_and_each_result_member(self):
        fields = policy.field_digests(dict(PIN, userfiles={'user.conf': 'x'}))
        self.assertEqual(sorted(fields), ['exit', 'result.logs', 'result.ret', 'result.rows', 'stderr', 'userfiles'])
        self.assertTrue(all(len(value) == policy.FIELD_DIGEST_LENGTH for value in fields.values()))
        self.assertEqual(policy.field_digests(dict(PIN, result=None))['result'], policy.digest(None)[:16])

    def test_the_stable_observation_keeps_userfiles_only_for_a_user_directory_case(self):
        side = dict(PIN, userfiles={'a': 'b'}, stderr_lines=3)
        self.assertEqual(policy.stable_observation(side, False), PIN)
        self.assertEqual(policy.stable_observation(side, True), dict(PIN, userfiles={'a': 'b'}))


class ObservationProblemTests(unittest.TestCase):
    def test_a_normal_termination_with_a_complete_document_is_recordable(self):
        self.assertIsNone(policy.observation_problem(PIN))
        self.assertIsNone(policy.observation_problem(dict(PIN, userfiles={'a': 'b'})))

    def test_a_registered_pin_abort_is_recordable_only_without_a_document(self):
        aborted = dict(PIN, exit=-signal.SIGABRT, result=None)
        self.assertIsNone(policy.observation_problem(aborted, pin_abort=True))
        self.assertIn('registered pin abort', policy.observation_problem(aborted))
        self.assertIn('cannot have a result', policy.observation_problem(dict(aborted, result={'logs': []}), pin_abort=True))

    def test_other_failures_and_incomplete_documents_are_refused(self):
        for bad, reason in ((dict(PIN, exit=1), 'normally terminate'), (dict(PIN, result=None), 'normally terminate'),
                            (dict(PIN, result={'ret': True}), 'logs'), (dict(PIN, result={'logs': [], '~x': 1}), '~'),
                            (dict(PIN, stderr=None), 'stderr'), (dict(PIN, exit='TIMEOUT'), 'integer'),
                            (dict(PIN, userfiles={'a': 1}), 'listing'), ('text', 'observation')):
            with self.subTest(bad=bad):
                self.assertIn(reason, policy.observation_problem(bad))


class DivergenceTests(unittest.TestCase):
    def assert_budget(self, lines):
        # Match the deepest schema-2 case/backend field's actual indentation,
        # including field name, JSON escaping, commas, newlines and the marker.
        nested = {'cases': {'case': {'cells': {'bdb': {'divergence': lines}}}}}
        serialized = policy.render(nested, indent=2)
        field = serialized[serialized.index('          "divergence":'):]
        field = field[:field.index('\n          ]') + len('\n          ]')]
        self.assertLessEqual(len(field.encode('utf-8')), policy.DIVERGENCE_BYTES)
        self.assertEqual(json.loads(serialized), nested)
        self.assertEqual(lines[-1], policy.OMITTED)

    def test_oversized_list_and_string_previews_do_not_weaken_exact_acceptance(self):
        for field in ('rows', 'text'):
            with self.subTest(field=field):
                if field == 'rows':
                    before = [f'甲-{i}' for i in range(128)]
                    after = ['乙-0', *before[1:]]
                else:
                    before = '甲"\\\udcff' * 256
                    after = '乙"\\\udcfe' * 256
                pin = dict(PIN, result={'logs': [], field: before})
                subject = dict(PIN, result={'logs': [], field: after})
                kept = policy.entry(pin, subject)
                self.assert_budget(kept['divergence'])
                text = '\n'.join(kept['divergence'])
                self.assertIn(f'result.{field}', text)
                self.assertIn('length', text)
                self.assertIn('128' if field == 'rows' else '1024', text)
                self.assertIn('甲', text)
                self.assertIn('乙', text)
                self.assertNotIn(before[-1] if field == 'rows' else before, text)
                self.assertIs(policy.check_entry(kept), kept)
                self.assertTrue(policy.accepted(kept, dict(pin=pin, subject=subject)))
                for side, observation in (('pin', pin), ('subject', subject)):
                    original = observation['result'][field]
                    mutation = [*original[:-1], 'hidden mutation'] if field == 'rows' else original[:-1] + 'X'
                    changed = dict(observation, result={'logs': [], field: mutation})
                    actual = dict(pin=pin, subject=subject)
                    actual[side] = changed
                    self.assertFalse(policy.accepted(kept, actual))

    def test_many_escaped_diagnostics_stop_at_the_serialized_byte_budget(self):
        pin = {f'{i:04d}-甲"\\\udcff': '甲"\\\udcff' * 8 for i in range(128)}
        subject = {key: '乙"\\\udcfe' * 8 for key in pin}
        with patch.object(policy, '_preview', wraps=policy._preview) as preview:
            lines = policy.divergence(pin, subject)
        self.assert_budget(lines)
        self.assertLess(preview.call_count, 2 * len(pin))
        self.assertNotIn('0127', '\n'.join(lines))

    def test_large_values_are_excerpted_before_rendering_or_list_alignment(self):
        pin = {'rows': ['甲' * 1024] * 128, 'text': '甲' * 1024, 'count': 2 ** 4096}
        subject = {'rows': ['乙' * 1024] * 128, 'text': '乙' * 1024, 'count': 2 ** 4097}
        original = policy.render

        def bounded(value, **kwargs):
            if isinstance(value, str):
                self.assertLess(len(value), 1024)
            if isinstance(value, (list, dict)):
                self.assertLess(len(value), 128)
            if isinstance(value, int):
                self.assertLessEqual(value.bit_length(), 128)
            return original(value, **kwargs)

        with patch.object(policy, 'render', side_effect=bounded), \
                patch.object(policy, 'canonical', side_effect=AssertionError('unbounded list alignment')):
            lines = policy.divergence(pin, subject)
        self.assert_budget(lines)
        self.assertIn('first change [0]', '\n'.join(lines))

    def test_list_edits_are_described_with_the_pin_indices(self):
        self.assertEqual(policy.divergence(PIN, SUBJECT),
                         ['result.rows[0]: subject inserts [[1, "x"]]', 'result.rows[1:2]: subject drops [[2, "b"]]'])

    def test_scalars_and_keys_on_one_side_are_described_in_sorted_key_order(self):
        pin = dict(exit=0, result={'logs': [], 'ret': True, 'only': 1}, stderr='a')
        subject = dict(exit=1, result={'logs': [], 'ret': False, 'extra': [1]}, stderr='b')
        self.assertEqual(policy.divergence(pin, subject),
                         ['exit: pin 0; subject 1', 'result.extra: subject only [1]', 'result.only: pin only 1',
                          'result.ret: pin true; subject false', 'stderr: pin "a"; subject "b"'])

    def test_an_edit_of_nested_documents_is_described_member_by_member(self):
        pin = {'rows': [{'a': 1, 'b': 2}, {'a': 3}]}
        subject = {'rows': [{'a': 1, 'b': 5}, {'a': 3}]}
        self.assertEqual(policy.divergence(pin, subject), ['rows[0].b: pin 2; subject 5'])
        self.assertEqual(policy.divergence({'v': [1, 2]}, {'v': [3]}), ['v[0:2]: pin [1, 2]; subject [3]'])
        self.assertEqual(policy.divergence({'v': 1}, {'v': 1.0}), ['v: pin 1; subject 1.0'],
                         'the canonical text keeps the number form, so the divergence does too')
        self.assertEqual(policy.divergence({'v': True}, {'v': 1}), ['v: pin true; subject 1'])
        self.assertEqual(policy.divergence({'v': [-0.0]}, {'v': [0.0]}),
                         ['v[0:1]: pin [-0.0]; subject [0.0]'])

    def test_non_ascii_text_is_written_as_is(self):
        self.assertEqual(policy.divergence({'t': '甲'}, {'t': '乙'}), ['t: pin "甲"; subject "乙"'])

    def test_surrogates_in_values_and_paths_are_escaped_for_utf8_output(self):
        pin = {'甲\udcff': {'\ud800': ['乙\udcff']}, 'removed\udfff': True}
        subject = {'甲\udcff': {'\ud800': ['丙\udcfe']}, 'added\ud801': False}
        lines = policy.divergence(pin, subject)
        self.assertEqual(lines, ['added\\ud801: subject only false', 'removed\\udfff: pin only true',
                                 '甲\\udcff.\\ud800[0:1]: pin ["乙\\udcff"]; subject ["丙\\udcfe"]'])
        text = policy.render(policy.entry(pin, subject))
        text.encode('utf-8')
        self.assertIn('甲', text)
        self.assertEqual(json.loads(text), policy.entry(pin, subject))


class EntryTests(unittest.TestCase):
    def test_an_entry_holds_both_digests_their_fields_and_the_divergence(self):
        entry = policy.entry(PIN, SUBJECT)
        self.assertEqual(set(entry), {'pin', 'subject', 'divergence'})
        self.assertEqual(entry['pin'], {'sha256': policy.digest(PIN), 'fields': policy.field_digests(PIN)})
        self.assertEqual(entry['divergence'], policy.divergence(PIN, SUBJECT))
        self.assertIs(policy.check_entry(entry), entry)
        self.assertEqual(json.loads(json.dumps(entry)), entry)

    def test_parity_cannot_be_recorded(self):
        with self.assertRaisesRegex(ValueError, 'difference, not parity'):
            policy.entry(PIN, dict(PIN))
        broken = policy.entry(PIN, SUBJECT)
        broken['subject']['sha256'] = broken['pin']['sha256']
        with self.assertRaisesRegex(ValueError, 'difference, not parity'):
            policy.check_entry(broken)

    def test_an_incomplete_or_malformed_entry_is_refused(self):
        entry = policy.entry(PIN, SUBJECT)
        for change in ('pin', 'subject', 'divergence', 'sha256', 'fields', 'hex', 'short', 'field-hex', 'lines'):
            with self.subTest(change=change):
                broken = json.loads(json.dumps(entry))
                if change in ('pin', 'subject', 'divergence'):
                    del broken[change]
                elif change in ('sha256', 'fields'):
                    del broken['pin'][change]
                elif change == 'hex':
                    broken['pin']['sha256'] = 'G' + broken['pin']['sha256'][1:]
                elif change == 'short':
                    broken['subject']['sha256'] = broken['subject']['sha256'][:63]
                elif change == 'field-hex':
                    broken['subject']['fields']['exit'] = 'xyz'
                else:
                    broken['divergence'] = []
                with self.assertRaises(ValueError):
                    policy.check_entry(broken)
        with self.assertRaises(ValueError):
            policy.check_entry('not an entry')

    def test_acceptance_needs_both_digests_and_the_diagnosis_names_the_moved_fields(self):
        entry = policy.entry(PIN, SUBJECT)
        self.assertTrue(policy.accepted(entry, dict(pin=PIN, subject=SUBJECT)))
        moved = dict(pin=PIN, subject=dict(SUBJECT, result=dict(SUBJECT['result'], ret=False)))
        self.assertFalse(policy.accepted(entry, moved))
        report = policy.diagnose(entry, moved)
        self.assertTrue(report['pin']['matches'])
        self.assertEqual(report['subject'], dict(matches=False, moved=['result.ret'], added=[], missing=[]))
        self.assertEqual(policy.describe(report), 'pin matches; subject moved in result.ret')
        both = dict(pin=dict(PIN, stderr='x', result=dict(PIN['result'], new=1)),
                    subject=dict(SUBJECT, result={'logs': []}))
        report = policy.diagnose(entry, both)
        self.assertEqual(report['pin'], dict(matches=False, moved=['stderr'], added=['result.new'], missing=[]))
        self.assertEqual(report['subject'],
                         dict(matches=False, moved=[], added=[], missing=['result.ret', 'result.rows']))
        self.assertEqual(policy.describe(report),
                         'pin moved in stderr, +result.new; subject moved in -result.ret, -result.rows')


if __name__ == '__main__':
    unittest.main()
