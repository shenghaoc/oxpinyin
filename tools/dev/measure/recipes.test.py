"""Tests for the measure recipe execution checks."""
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
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev.measure import bench, identity, prepare, recipes
from tools.dev.runners import Finished


def run(operation, argv=(), **options):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        status = recipes.run(operation, argv, {'GITHUB_ACTIONS': 'true'}, **options)
    return status, out.getvalue(), err.getvalue()


class RecipeTests(unittest.TestCase):
    def test_the_shared_failure_ladder_maps_every_ending(self):
        def raises(error):
            def recipe(argv, env):
                raise error
            return recipe
        self.assertEqual(run(raises(Finished(9)))[0], 9)
        self.assertEqual(run(raises(FileNotFoundError('taskset')))[0], 127)
        status, _, err = run(raises(ValueError('bad')))
        self.assertEqual((status, err), (1, 'FAIL: bad\n'))


    def test_a_pin_comparison_is_refused_before_running_the_operation(self):
        operation = mock.Mock()
        status, _, err = run(operation, pin_comparison=True)
        self.assertEqual(status, 2)
        self.assertIn('pin comparisons are local-only', err)
        operation.assert_not_called()

    def test_a_local_pin_comparison_runs_with_the_vcpkg_tkrzw_first(self):
        from tools.dev import vcpkg
        operation = mock.Mock(return_value=0)
        selected = {'OXPINYIN_TKRZW_PREFIX': '/cache/vcpkg/installed/x'}
        with mock.patch.object(vcpkg, 'environment', return_value=selected) as environment, \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(recipes.run(operation, ['a'], {'OXPINYIN_CACHE': '/cache'}, pin_comparison=True), 0)
        environment.assert_called_once_with({'OXPINYIN_CACHE': '/cache'})
        operation.assert_called_once_with(['a'], selected)


class NativeCompareTests(unittest.TestCase):
    def test_zero_cpu_sample_drops_only_that_ratio_and_the_campaign_completes(self):
        operations = recipes.workload_operations()
        row = dict(pid=1, cycles=1, operations=operations, fingerprint='a'*16,
                   wall_seconds=1., cpu_seconds=.25, loop_cpu_cycles=1000,
                   peak_private_commit_bytes=100, peak_working_set_bytes=200)
        # One zero GetProcessTimes reading, in a control sample, among eight.
        samples = [dict(row, cpu_seconds=0.) if index == 3 else dict(row) for index in range(8)]
        args = types.SimpleNamespace(pairs=2, control_pairs=2, cycles=1, budget=180, windows=True)
        arms = {'A': {}, 'B': {}, recipes.CONTROL: {}}
        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(recipes, 'native_sample', side_effect=samples) as sample, \
             contextlib.redirect_stdout(io.StringIO()):
            recipes.native_compare(args, arms, directory, {}, Path(directory))
            self.assertEqual(sample.call_count, 8)
            self.assertEqual(len((Path(directory)/'runs.jsonl').read_text().splitlines()), 8)
            summary = json.loads((Path(directory)/'summary.json').read_text())
        for kind in ('comparison', 'control'):
            cpu = summary[kind]['cpu_seconds']
            self.assertNotIn('B_over_A', cpu)
            self.assertIn('below the 1/64-second', cpu['no_ratio'])
            self.assertEqual(cpu['A']['n'], 2)
            for metric in ('loop_cpu_cycles', 'wall_seconds'):
                self.assertEqual(summary[kind][metric]['B_over_A']['median'], 1)
                self.assertNotIn('no_ratio', summary[kind][metric])
            self.assertEqual(summary[kind]['peak_private_commit_bytes']['B_minus_A_bytes']['median'], 0)
        # A zero cycle delta is still a broken capture and stops the campaign at that sample.
        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(recipes, 'native_sample', return_value=dict(row, loop_cpu_cycles=0)) as sample:
            with self.assertRaisesRegex(ValueError, 'broken required capture: loop_cpu_cycles'):
                recipes.native_compare(args, arms, directory, {}, Path(directory))
            sample.assert_called_once()


if sys.platform == 'win32':
    class WindowsNativeTests(unittest.TestCase):
        def test_one_build_identical_drivers_and_cleanup_on_success_and_failure(self):
            from tools.dev import contract_data
            for outcome in (None, ValueError('sample failed')):
                with self.subTest(outcome=outcome), tempfile.TemporaryDirectory() as directory:
                    destinations=[];digests=[];staged={}
                    def build(sha, destination, env, **options):
                        destinations.append(destination)
                        destination.mkdir(parents=True)
                        dll=destination/'oxpinyin_capi.dll';dll.write_bytes(b'product')
                        return dict(revision=sha,dylib=dll,uuid='hash',windows=True)
                    def driver(arm, output, env):
                        output.write_bytes(b'identical driver')
                        return identity.sha256(output)
                    def compare(args, arms, data, env, output):
                        digests.extend(identity.sha256(arm['driver']) for arm in arms.values())
                        staged.update({label: (arm['dylib'], arm['dylib'].read_bytes(), arm['driver'].parent)
                                       for label, arm in arms.items()})
                        if outcome: raise outcome
                    with mock.patch('tools.dev.check.msvc_environment',side_effect=lambda env:env), \
                         mock.patch.object(bench,'capture',return_value=types.SimpleNamespace(status=0,stdout=b'a'*40+b'\n')), \
                         mock.patch.object(prepare,'native_build',side_effect=build) as builds, \
                         mock.patch.object(prepare,'native_driver',side_effect=driver), \
                         mock.patch.object(identity,'windows_preflight',return_value=[]), \
                         mock.patch.object(identity,'expected_cow_commit',return_value={}), \
                         mock.patch.object(contract_data,'data_identity',return_value='data'), \
                         mock.patch.object(recipes,'native_compare',side_effect=compare), \
                         contextlib.redirect_stdout(io.StringIO()):
                        args=['compare','--base','HEAD','--head','HEAD','--data',directory]
                        env=dict(OXPINYIN_CACHE=directory,VCPKG_ROOT=directory,PATH=os.environ['PATH'])
                        if outcome:
                            with self.assertRaises(ValueError): recipes.native(args,env)
                        else:
                            self.assertEqual(recipes.native(args,env),0)
                    builds.assert_called_once()
                    self.assertEqual(len(set(digests)),1)
                    # A' is a second copy of A: its own prefix, the DLL beside its driver, the same bytes.
                    self.assertEqual(set(staged), {'A', 'B', recipes.CONTROL})
                    self.assertEqual(len({path for path, _, _ in staged.values()}), 3)
                    self.assertEqual(staged[recipes.CONTROL][1], staged['A'][1])
                    self.assertEqual(staged[recipes.CONTROL][0].parent, staged[recipes.CONTROL][2])
                    self.assertEqual(staged[recipes.CONTROL][0].parents[1].name, 'C')
                    # One length for every staged prefix: the same bytes sit at same-length paths.
                    self.assertEqual({len(str(path)) for path, _, _ in staged.values()}, {len(str(staged['A'][0]))})
                    self.assertFalse(destinations[0].parent.exists())

        def test_unsupported_modes_fail_before_preparation(self):
            for argv in (['profile','--template','Allocations'],['neutrality']):
                with mock.patch.object(prepare,'native_build') as build, self.assertRaises(Finished):
                    recipes.native(argv,{})
                build.assert_not_called()


if __name__ == '__main__':
    unittest.main()
