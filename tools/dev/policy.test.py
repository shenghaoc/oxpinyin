#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the policy-record command (tools.dev.policy)."""
import contextlib
import io
import json
from pathlib import Path
import re
import signal
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
if str(ROOT / 'tools/bisection') not in sys.path:
    sys.path.insert(0, str(ROOT / 'tools/bisection'))

from tools.bisection import contract_policy
from tools.dev import policy
import contract_verdict

PIN = dict(exit=0, result={'ret': True, 'rows': [[2, 'a'], [2, 'b']], 'logs': []}, stderr='open SCRATCH/user-X/user.conf failed.',
           stderr_lines=1, userfiles={'user.conf': 'x'}, measurement_roots=[])
SUBJECT = dict(PIN, result={'ret': True, 'rows': [[2, 'b']], 'logs': []})


def row(case='family/case', cell='bdb', pin=PIN, subject=SUBJECT, userdir=False, abort=False):
    return dict(cell=cell, case=case, userdir=userdir, abort=abort, policy_record='rec.md', pin=pin, subject=subject)


def spec(**overrides):
    return dict(dict(mode='pinyin', control=False, abort=contract_verdict.NO_ABORT, stderr=True, crash=None,
                     userdir=False, cells=('bdb', 'kc', 'tkrzw'), either=False, host_scanf=(), dir_chunk=None,
                     stderr_varies=False, abort_logs=None, fatal=False, both_abort=False, strjoinv_over_read=False,
                     issues=(), policy_record='rec.md', case_id='family/case', pin_not_run=None), **overrides)


class PolicyRecordTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.record = self.root / 'rec.md'
        self.record.write_text('# Record\n\nProse.\n')
        (self.root / 'upstream-divergences.md').write_text('[record](rec.md)\n')
        self.capture = self.root / 'obs.jsonl'

    def write_rows(self, *rows, path=None):
        path = path or self.capture
        path.write_text(''.join(json.dumps(r) + '\n' for r in rows))
        return path

    def run_main(self, *argv):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            status = policy.main([str(self.record), *argv])
        return status, out.getvalue(), err.getvalue()

    def block(self):
        blocks = re.findall(r'^```contract-diff-policy\n(.*?)\n```$', self.record.read_text(), re.M | re.S)
        self.assertEqual(len(blocks), 1)
        return json.loads(blocks[0])

    def test_a_capture_is_recorded_into_a_new_block_the_verdict_accepts(self):
        self.write_rows(row(), row(cell='kc'))
        status, out, err = self.run_main(str(self.capture))
        self.assertEqual((status, err), (0, ''))
        self.assertIn('recorded family/case [bdb]: pin', out)
        self.assertIn('recorded family/case [kc]:', out)
        self.assertIn('1 divergence line(s)', out)
        self.assertIn('rec.md: ', out)
        self.assertNotIn('notice', out)
        record = self.block()
        self.assertEqual(record['schema'], contract_policy.SCHEMA)
        self.assertEqual((record['record'], record['status'], record['disposition']), ('rec', 'current', 'POLICY-ACCEPTED'))
        self.assertEqual(list(record['cases']['family/case']['cells']), ['bdb', 'kc'])
        self.assertEqual(record['cases']['family/case']['cells']['bdb']['divergence'],
                         ['result.rows[0:1]: subject drops [[2, "a"]]'])
        self.assertTrue(self.record.read_text().startswith('# Record\n\nProse.\n\n```contract-diff-policy\n'))
        for cell in ('bdb', 'kc'):
            self.assertEqual(contract_verdict.verdict_for(spec(), PIN, SUBJECT, False, policy_root=self.root, cell=cell,
                                                          directory_probe=None), ('POLICY-ACCEPTED', True, False))
        moved = dict(SUBJECT, result=dict(SUBJECT['result'], ret=False))
        self.assertEqual(contract_verdict.verdict_for(spec(), PIN, moved, False, policy_root=self.root, cell='bdb',
                                                      directory_probe=None), ('DIFFER', False, False))
        self.assertEqual(contract_verdict.policy_diagnosis(spec(), PIN, moved, policy_root=self.root, cell='bdb')['summary'],
                         'pin matches; subject moved in result.ret')

    def test_recording_again_replaces_only_that_cell_and_keeps_the_prose(self):
        self.write_rows(row(), row(cell='kc'))
        self.assertEqual(self.run_main(str(self.capture))[0], 0)
        self.record.write_text(self.record.read_text().replace('Prose.', 'Prose, edited.'))
        changed = dict(SUBJECT, result={'ret': False, 'rows': [], 'logs': []})
        self.write_rows(row(subject=changed))
        status, out, _ = self.run_main(str(self.capture))
        self.assertEqual(status, 0)
        cells = self.block()['cases']['family/case']['cells']
        self.assertEqual(cells['bdb'], contract_policy.entry(contract_policy.stable_observation(PIN, False),
                                                            contract_policy.stable_observation(changed, False)))
        self.assertEqual(cells['kc'], contract_policy.entry(contract_policy.stable_observation(PIN, False),
                                                           contract_policy.stable_observation(SUBJECT, False)))
        self.assertIn('Prose, edited.', self.record.read_text())
        self.assertEqual(self.record.read_text().count('```contract-diff-policy'), 1)

    def test_a_selection_records_those_cases_and_names_a_case_without_a_row(self):
        self.write_rows(row(), row(case='family/other'))
        status, out, _ = self.run_main(str(self.capture), '--cases', 'family/other')
        self.assertEqual(status, 0)
        self.assertEqual(list(self.block()['cases']), ['family/other'])
        status, _, err = self.run_main(str(self.capture), '--cases', 'family/other,family/missing')
        self.assertEqual(status, 1)
        self.assertIn('no capture row for family/missing', err)

    def test_user_directory_and_registered_abort_rows_record_what_the_verdict_compares(self):
        aborted = dict(PIN, exit=-signal.SIGABRT, result=None)
        self.write_rows(row(pin=aborted, userdir=True, abort=True))
        self.assertEqual(self.run_main(str(self.capture))[0], 0)
        entry = self.block()['cases']['family/case']['cells']['bdb']
        self.assertEqual(sorted(entry['pin']['fields']), ['exit', 'result', 'stderr', 'userfiles'])
        self.assertEqual(contract_verdict.verdict_for(spec(userdir=True, abort=False), aborted, SUBJECT, False,
                                                      policy_root=self.root, cell='bdb', directory_probe=None),
                         ('POLICY-ACCEPTED', True, False))
        self.write_rows(row(pin=aborted, userdir=True, abort=False))
        status, _, err = self.run_main(str(self.capture))
        self.assertEqual(status, 1)
        self.assertIn('pin must normally terminate', err)

    def test_parity_incomplete_rows_and_unknown_cells_are_refused_before_writing(self):
        before = self.record.read_text()
        for bad, reason in ((row(subject=PIN), 'difference, not parity'),
                            (row(pin=dict(PIN, result={'ret': True})), 'pin result must carry logs'),
                            (row(cell='sqlite'), 'unknown cell'),
                            (dict(row(), userdir='no'), 'booleans'),
                            ({k: v for k, v in row().items() if k != 'abort'}, 'taken again')):
            with self.subTest(reason=reason):
                self.write_rows(bad)
                status, _, err = self.run_main(str(self.capture))
                self.assertEqual(status, 1)
                self.assertIn(reason, err)
                self.assertEqual(self.record.read_text(), before)
        self.capture.write_text('{not json\n')
        status, _, err = self.run_main(str(self.capture))
        self.assertEqual((status, 'not JSON' in err), (1, True))

    def test_a_schema_1_block_is_converted_only_on_request_and_bare_entries_stay_bare(self):
        pin = contract_policy.stable_observation(PIN, False)
        subject = contract_policy.stable_observation(SUBJECT, False)
        old = dict(schema=1, record='rec', status='current', disposition='POLICY-ACCEPTED',
                   cases={'family/case': dict(pin=pin, subject=subject),
                          'family/cells': {'cells': {'kc': dict(pin=pin, subject=subject)}}})
        self.record.write_text('# Record\n\n```contract-diff-policy\n' + json.dumps(old) + '\n```\n\nTail.\n')
        self.write_rows(row(case='family/new'))
        status, _, err = self.run_main(str(self.capture))
        self.assertEqual(status, 1)
        self.assertIn('pass --migrate', err)
        status, out, err = self.run_main('--migrate')
        self.assertEqual((status, err), (0, ''))
        self.assertIn('converted family/case\n', out)
        self.assertIn('converted family/cells [kc]\n', out)
        record = self.block()
        self.assertEqual(record['schema'], contract_policy.SCHEMA)
        self.assertEqual(record['cases']['family/case'], contract_policy.entry(pin, subject))
        self.assertEqual(record['cases']['family/cells'], {'cells': {'kc': contract_policy.entry(pin, subject)}})
        self.assertTrue(self.record.read_text().endswith('\n```\n\nTail.\n'))
        for cell in ('bdb', 'kc', 'tkrzw'):
            self.assertEqual(contract_verdict.verdict_for(spec(), PIN, SUBJECT, False, policy_root=self.root, cell=cell,
                                                          directory_probe=None), ('POLICY-ACCEPTED', True, False))
        self.write_rows(row(cell='tkrzw'))
        status, out, _ = self.run_main(str(self.capture))
        self.assertEqual(status, 0)
        self.assertIn('notice: family/case had one entry for every cell', out)
        self.assertEqual(list(self.block()['cases']['family/case']['cells']), ['tkrzw'])

    def test_an_unregistered_record_and_an_empty_selection_are_reported(self):
        (self.root / 'upstream-divergences.md').write_text('')
        self.write_rows(row())
        status, out, _ = self.run_main(str(self.capture))
        self.assertEqual(status, 0)
        self.assertIn('not linked from upstream-divergences.md', out)
        self.write_rows()
        status, _, err = self.run_main(str(self.capture))
        self.assertEqual(status, 1)
        self.assertIn('nothing to record', err)
        with self.assertRaises(SystemExit):
            with contextlib.redirect_stderr(io.StringIO()):
                policy.main([str(self.record)])


if __name__ == '__main__':
    unittest.main()
