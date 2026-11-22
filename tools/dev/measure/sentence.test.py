"""Native ABI call-order and campaign refusal tests for #1348; no model needed."""
import contextlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.measure import bench, sentence
from tools.dev.runners import Finished


class ObservationTests(unittest.TestCase):
    def test_elapsed_ns_accepts_zero_and_rejects_negative_or_non_int(self):
        args = types.SimpleNamespace(workload='sentence', iterations=1, input='ni')
        row = dict(pid=123, workload='sentence', iterations=1, observations=[[dict(
            input='ni', parsed=2, guess_sentence=True, get_sentence=True, sentence='x')]])
        self.assertEqual(sentence.observation(json.dumps(dict(row, elapsed_ns=0)), args)['elapsed_ns'], 0)
        for bad in (-1, 1.0, '0', True, None):
            with self.subTest(elapsed_ns=bad), self.assertRaises(ValueError):
                sentence.observation(json.dumps(dict(row, elapsed_ns=bad)), args)


class CampaignTests(unittest.TestCase):
    def native_fixture(self, *, changed=False, elapsed=1000):
        root = Path(self.enterContext(tempfile.TemporaryDirectory(prefix='sentence-native-')))
        paths = {name: root / name for name in ('base-plain', 'pr-plain', 'pin')}
        for name, path in paths.items():
            path.write_text(name)
        args = types.SimpleNamespace(out=root, workload='typing-sequence', input='ni', cell='bdb',
            iterations=1, rounds=2, native_iterations=2, native_rounds=2, cpu=0,
            main_shipping=paths['base-plain'], pr_shipping=paths['pr-plain'], pin=paths['pin'],
            system_dir=Path('/data'))
        commands = []

        def capture(command, **kwargs):
            self.assertEqual(command[:3], ['taskset', '-c', '0'])
            self.assertNotIn('valgrind', command)
            self.assertEqual(command[-1], '2')
            commands.append(command)
            arm = Path(command[-4]).parent.name
            steps = [dict(input=i, parsed=len(i), guess_sentence=True, get_sentence=True, sentence=i,
                          guess_candidates=True, candidates=[]) for i in ('n', 'ni')]
            if changed and arm == 'pr__':
                steps[1]['sentence'] = 'changed'
            row = dict(pid=123, workload=args.workload, iterations=2, elapsed_ns=elapsed,
                       observations=[steps, steps])
            return types.SimpleNamespace(status=0, stdout=json.dumps(row).encode(), stderr=b'')

        with mock.patch.object(bench, 'capture', side_effect=capture), \
                mock.patch.object(bench, 'guard'), \
                contextlib.redirect_stdout(io.StringIO()):
            result = sentence.native_campaign(args, {}, root / 'bisect')
        return root, commands, result

    def test_native_rounds_use_plain_copies_rotate_and_report_paired_time(self):
        root, commands, result = self.native_fixture()
        self.assertEqual([Path(c[-4]).parent.name for c in commands],
                         list(sentence.ARMS) + list(sentence.ARMS[1:] + sentence.ARMS[:1]))
        self.assertEqual((root / 'native/main/libpinyin.so').read_bytes(), b'base-plain')
        self.assertEqual((root / 'native/pr__/libpinyin.so').read_bytes(), b'pr-plain')
        self.assertEqual((root / 'native/mctl/libpinyin.so').read_bytes(), b'base-plain')
        self.assertEqual(result['pr_over_main'], dict(median=1.0, ci95=[1.0, 1.0]))
        self.assertEqual(result['ns_per_invocation']['main'], [500, 500])
        self.assertEqual(result['verdict'], 'VALID')
        self.assertTrue((root / 'native/main.0.stdout').is_file())
        self.assertTrue((root / 'native/main.1.stdout').is_file())

    def test_native_refuses_changed_observations_and_zero_time(self):
        for options in (dict(changed=True), dict(elapsed=0)):
            with self.subTest(options=options), self.assertRaises(Finished):
                self.native_fixture(**options)

    def test_native_short_round_request_is_rejected_before_inputs(self):
        with mock.patch.object(os, 'sched_getaffinity', return_value={0}, create=True), \
                contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            sentence.arguments(['--workload', 'sentence', '--cell', 'bdb', '--main', '/missing',
                '--pr', '/missing', '--pin', '/missing', '--main-shipping', '/missing',
                '--pr-shipping', '/missing', '--system-dir', '/missing', '--out', '/missing',
                '--native-rounds', '2'])

    def fake_campaign(self, *, changed_answer=False, varying_control=False, workload='typing-sequence', changed=False):
        temporary = self.enterContext(tempfile.TemporaryDirectory(prefix='sentence-campaign-'))
        root = Path(temporary)
        libraries = {arm: root / arm / 'libpinyin.so' for arm in sentence.ARMS}
        for path in libraries.values():
            path.parent.mkdir()
            path.write_bytes(b'library')
        args = types.SimpleNamespace(out=root, workload=workload, input='ni', cell='bdb',
                                     iterations=1, rounds=2, cpu=0, main=Path('/main'), pr=Path('/pr'),
                                     pin=Path('/pin'), main_shipping=Path('/ship-main'),
                                     pr_shipping=Path('/ship-pr'), system_dir=Path('/data'))
        commands = []

        def capture(command, **kwargs):
            if command[0] == 'nm':
                return types.SimpleNamespace(status=0, stdout=b'1 T bisect_sentence_unit\n2 T bisect_typing_unit\n')
            self.assertEqual(command[:3], ['taskset', '-c', '0'])
            commands.append(command)
            # The anchor's client requests bracket the count; a --toggle-collect would invert it.
            self.assertIn('--collect-atstart=no', command)
            self.assertFalse([part for part in command if part.startswith('--toggle-collect')])
            self.assertEqual(command[-5:], ['--perf-' + workload, command[-4], str(Path('/data')), 'ni', '1'])
            arm = Path(command[-4]).parent.name
            output = Path(next(c.split('=', 1)[1] for c in command if c.startswith('--callgrind-out-file=')))
            count = 101 if varying_control and arm == 'pctl' else 100
            costs = {'f': count}
            if changed and arm in ('pr__', 'pctl'):
                costs['g'] = 7  # a deterministic main/PR difference, identical in both PR copies
            total = sum(costs.values())
            body = ''.join(f'fn={name}\n1 {cost}\n' for name, cost in costs.items())
            output.write_text(f'creator: callgrind-3.26\npid: 123\ncmd: {root / "bisect"} --perf-typing-sequence x\n'
                              f'events: Ir\nsummary: {total}\nob=/lib.so\n{body}totals: {total}\n', newline='\n')
            steps = [dict(input=i, parsed=len(i), guess_sentence=True, get_sentence=True, sentence=i,
                          guess_candidates=True, candidates=[])
                     for i in (('n', 'ni') if workload == 'typing-sequence' else ('ni',))]
            if changed_answer and arm == 'pr__':
                steps[-1]['sentence'] = 'changed'
            row = dict(pid=123, workload=args.workload, iterations=1, elapsed_ns=1000, observations=[steps])
            return types.SimpleNamespace(status=0, stdout=json.dumps(row).encode(), stderr=b'')

        with mock.patch.object(sentence, 'prepare', return_value=libraries), \
                mock.patch.object(bench, 'build_harness'), mock.patch.object(bench, 'guard') as guard, \
                mock.patch.object(bench, 'capture', side_effect=capture), \
                mock.patch.object(sentence.identity, 'cpu_model', return_value='test'), \
                contextlib.redirect_stdout(io.StringIO()):
            code = sentence.campaign(args, {})
        self.assertEqual(guard.call_count, 10)
        self.assertEqual([Path(c[-4]).parent.name for c in commands],
                         list(sentence.ARMS) + list(sentence.ARMS[1:] + sentence.ARMS[:1]))
        return code, json.loads((root / 'result.json').read_text())

    def test_campaign_rotates_serial_arms_and_checks_the_complete_answers(self):
        code, result = self.fake_campaign(changed=True)
        self.assertEqual(code, 0)
        self.assertEqual(result['verdict'], 'VALID')
        self.assertEqual(result['main_minus_pr_bounds'], [-7, -7])
        self.assertTrue(result['main_pr_observations_equal'])
        self.assertTrue(result['main_pin_observations_equal'])

    def test_changed_answer_or_same_bytes_instruction_variation_invalidates_campaign(self):
        for options in (dict(changed_answer=True), dict(varying_control=True)):
            with self.subTest(options=options):
                code, result = self.fake_campaign(**options)
                self.assertEqual(code, 3)
                self.assertEqual(result['verdict'], 'INVALID')
                self.assertNotIn('Ir_per_invocation', result)

    def test_controls_reject_both_within_arm_and_between_copy_variation(self):
        counts = {arm: [100, 100] for arm in sentence.ARMS}
        self.assertTrue(all(c['valid'] for c in sentence.instruction_controls(counts).values()))
        counts['mctl'][1] = 101
        counts['pctl'] = [99, 99]
        controls = sentence.instruction_controls(counts)
        self.assertEqual(controls['main'], dict(valid=False, spread=1))
        self.assertEqual(controls['pr__'], dict(valid=False, spread=1))

    def test_profile_requires_identity_events_positive_and_reconciled_counts(self):
        bisect = Path('/tmp/bisect')
        raw = (f'creator: callgrind-3.26\npid: 123\ncmd: {bisect} --perf-sentence x\n'
               'events: Ir\nsummary: 7\nob=/lib.so\nfn=f\n1 7\ntotals: 7\n').encode()
        self.assertEqual(sentence.checked_profile(raw, bisect, 123, Path('/lib.so'))['total'], 7)
        for bad in (raw.replace(b'pid: 123', b'pid: 124'), raw.replace(b'events: Ir', b'events: Ir Dr'),
                    raw.replace(b'1 7', b'1 6'), raw.replace(b'totals: 7', b'totals: 6')):
            with self.subTest(raw=bad), self.assertRaises(ValueError):
                sentence.checked_profile(bad, bisect, 123, Path('/lib.so'))

    def test_wrong_cell_is_refused_before_output_or_workers(self):
        args = types.SimpleNamespace(main=Path('/main'), pr=Path('/pr'), pin=Path('/pin'), cell='bdb')
        with mock.patch.object(sentence.identity, 'library_backend', return_value='tkrzw'), \
                mock.patch.object(bench, 'capture') as capture, contextlib.redirect_stderr(io.StringIO()), \
                self.assertRaises(Finished):
            sentence.prepare(args, {})
        capture.assert_not_called()


if __name__ == '__main__':
    unittest.main()
