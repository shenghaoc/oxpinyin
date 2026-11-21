"""Linux compare and profile contracts; synthetic rows and a stub library, not product measurements."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import pool
from tools.dev.measure import bench, identity, prepare, recipes
from tools.dev.runners import Finished
from tools.dev.test_support import PERF_LIBRARY as LIBRARY, capi_metadata, perf_row


class CampaignFixture:
    """A comparison whose guard and driver processes are replaced; shared with the Linux-only cases."""

    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.arms = {}
        for label, name in recipes.PREFIXES.items():
            library = self.root / name / 'lib/libpinyin.so.15.0.0'
            self.arms[label] = dict(label=name, dylib=library, driver=self.root / name / 'bin/bisect',
                                    data=self.root / 'data', backend='kc')
        self.launches = []
        self.refused = True
        self.ir = lambda label: 5000
        self.enterContext(mock.patch.object(bench, 'guard', side_effect=self.guard))
        self.enterContext(mock.patch.object(bench, 'measured_process', side_effect=self.process))
        self.enterContext(mock.patch.object(os, 'sched_getaffinity', return_value={3, 5}, create=True))
        self.guarded = []

    def guard(self, label, cell, side, obj, command, *, env, cwd=None):
        self.guarded.append((cell, str(obj), env['LD_LIBRARY_PATH']))

    def process(self, command, *, env, timeout, usage=True):
        command = list(map(str, command))
        self.launches.append((command, dict(env), usage))
        library = command[command.index('--perf') + 1]
        row = json.loads(perf_row(int(env['PERF_CYCLES']), so=library, backend=env['PERF_BACKEND'], pid=len(self.launches)))
        if 'PERF_HW' not in env or self.refused:
            del row['steady_hw_cycles'], row['steady_hw_instructions']
            if 'PERF_HW' in env:
                row['hw_errno'] = 2
        for part in command:
            if part.startswith('--callgrind-out-file='):
                total = self.ir(env['PERF_BACKEND'])
                Path(part.split('=', 1)[1]).write_text(
                    f'creator: callgrind-3.22.0\npid: {row["pid"]}\ncmd: {command[command.index("--perf") - 1]} --perf x\n'
                    f'events: Ir\nob=(1) {library}\nfn=(1) steady\n0 {total}\nsummary: {total}\ntotals: {total}\n')
        return json.dumps(row).encode(), None

    def compare(self, **options):
        args = types.SimpleNamespace(pairs=2, control_pairs=2, cycles=4, budget=180, **options)
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return recipes.linux_compare(args, self.arms, {'LD_LIBRARY_PATH': '/host/lib'}, self.root)


class LinuxCampaign(CampaignFixture, unittest.TestCase):
    """The runner around the driver: what it launches, in which order, and what it keeps."""

    def test_a_granted_counter_needs_no_valgrind_and_a_later_refusal_fails(self):
        self.refused = False
        self.assertEqual(self.compare(), 0)
        self.assertEqual(len(self.launches), 9)
        self.assertTrue(all(env['PERF_HW'] == 'instructions' for _, env, _ in self.launches))
        summary = json.loads((self.root / 'summary.json').read_text())
        self.assertEqual(summary['instruction_source'], 'perf_event_open')
        self.assertNotIn('loop_callgrind_ir', summary['comparison'])
        real = self.process

        def refuse_later(command, **options):
            self.refused = len(self.launches) >= 3
            return real(command, **options)
        self.launches.clear()
        with mock.patch.object(bench, 'measured_process', side_effect=refuse_later), \
             self.assertRaisesRegex(ValueError, 'never mixed'):
            self.compare()
        # The completed runs stay; nothing is retried and no summary is written.
        self.assertEqual(len((self.root / 'runs.jsonl').read_text().splitlines()), 2)
        self.assertEqual(len(self.launches), 4)

    def test_the_budget_covers_both_passes(self):
        # Two readings for each run: the schedule's check, then the launcher's after its preflight.
        with mock.patch('time.monotonic', side_effect=[0, *[1] * 16, 181]), \
             self.assertRaisesRegex(ValueError, 'budget'):
            self.compare()
        self.assertEqual(len((self.root / 'runs.jsonl').read_text().splitlines()), 8)
        self.assertFalse((self.root / 'summary.json').exists())

    def test_a_slow_preflight_shortens_the_worker_bound_and_cannot_launch_past_the_deadline(self):
        clock, timeouts = [0.0], []
        real_guard, real_process = self.guard, self.process

        def slow_guard(*arguments, **options):
            # The probe is check 1. Check 3 takes 30 seconds; check 4 outlasts the 180-second budget.
            clock[0] += {3: 30, 4: 200}.get(len(self.guarded) + 1, 0)
            return real_guard(*arguments, **options)

        def process(command, **options):
            timeouts.append(options['timeout'])
            # A worker that starts in time and succeeds is kept, however long it then runs.
            clock[0] += 5
            return real_process(command, **options)
        with mock.patch('time.monotonic', side_effect=lambda: clock[0]), \
             mock.patch.object(bench, 'guard', side_effect=slow_guard), \
             mock.patch.object(bench, 'measured_process', side_effect=process), \
             self.assertRaisesRegex(ValueError, 'measurement budget exhausted; campaign incomplete'):
            self.compare()
        # The probe and two runs were launched; the third was checked and never started.
        self.assertEqual((len(self.guarded), len(self.launches)), (4, 3))
        # The budget starts after the probe. Run 1 has all of it; run 2 loses run 1's 5 seconds and its own 30-second check.
        self.assertEqual(timeouts, [180, 180, 145])
        self.assertEqual(len((self.root / 'runs.jsonl').read_text().splitlines()), 2)
        self.assertFalse((self.root / 'summary.json').exists())

    def profile(self, template):
        args = types.SimpleNamespace(template=template, cycles=4, budget=60)
        out = io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(io.StringIO()):
            status = recipes.linux_profile(args, self.arms['A'], {}, self.root)
        return status, json.loads(out.getvalue()), self.launches[-1][0]

    def test_profiles_use_the_existing_callgrind_and_massif_recipes(self):
        from tools.dev.measure import valgrind
        annotated = []
        real = self.process

        def massif(command, **options):
            for part in map(str, command):
                if part.startswith('--massif-out-file='):
                    Path(part.split('=', 1)[1]).write_text('mem_heap_B=10\nmem_heap_B=4096\n')
            return real(command, **options)
        with mock.patch.object(valgrind, 'annotate', side_effect=annotated.append), \
             mock.patch.object(bench, 'measured_process', side_effect=massif):
            status, report, command = self.profile('Callgrind')
            self.assertEqual(status, 0)
            self.assertEqual(command[3:3 + len(valgrind.CALLGRIND_OPTIONS)], list(valgrind.CALLGRIND_OPTIONS))
            self.assertEqual(annotated, [self.root])
            self.assertEqual(report['capture']['subject_Ir'], 5000)
            status, report, command = self.profile('Massif')
            self.assertEqual(command[3:8], ['valgrind', '--tool=massif', '--time-unit=i', '--heap=yes', '--stacks=no'])
            self.assertEqual(command.count('--massif-out-file=' + str(self.root / 'massif.out')), 1)
            self.assertEqual(report['capture']['heap'], ['A peak heap: 4096 bytes'])
        # A profile that credits nothing to the measured library is not an attribution.
        self.ir = lambda label: 0
        with mock.patch.object(valgrind, 'annotate'), self.assertRaises(ValueError):
            self.profile('Callgrind')


class PreparationFixture:
    def native(self, argv, env=None, **patches):
        with mock.patch.object(sys, 'platform', 'linux'), contextlib.redirect_stdout(io.StringIO()), \
             contextlib.redirect_stderr(io.StringIO()):
            return recipes.native(argv, env or {})


class LinuxPreparation(PreparationFixture, unittest.TestCase):

    def test_modes_linux_does_not_have_fail_before_any_build(self):
        for argv in (['neutrality'], ['profile', '--template', 'Allocations'],
                     ['profile', '--template', 'Massif', '--instrument', 'VM Tracker'],
                     ['compare', '--base', 'x', '--head', 'x', '--cycles', '1']):
            with self.subTest(argv=argv), mock.patch.object(prepare, 'native_build') as build, \
                 self.assertRaises(Finished) as stopped:
                self.native(argv)
            self.assertEqual(stopped.exception.status, 2)
            build.assert_not_called()
        # The Valgrind templates belong to Linux alone.
        with mock.patch.object(sys, 'platform', 'darwin'), mock.patch('platform.machine', return_value='arm64'), \
             mock.patch.object(prepare, 'native_build') as build, contextlib.redirect_stderr(io.StringIO()), \
             self.assertRaises(Finished):
            recipes.native(['profile', '--template', 'Callgrind'], {})
        build.assert_not_called()

    def test_the_build_is_the_local_cell_from_the_distribution(self):
        def metadata(root, env=None):
            return capi_metadata(root, features={'default': ['declared']})
        answer = {'status': 1}

        def capture(command, **kwargs):
            self.assertEqual(command, ['pkg-config', '--exists', 'kyotocabinet'])
            return mock.Mock(status=answer['status'])
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(bench, 'run'), \
             mock.patch.object(bench, 'capture', side_effect=capture), \
             mock.patch('tools.dev.packaging.cargo_metadata', side_effect=metadata), \
             mock.patch('tools.dev.packaging.install_library') as install, \
             mock.patch.object(identity, 'sha256', return_value='digest'), \
             mock.patch.object(identity, 'library_backend', return_value='kc') as backend:
            env = dict(RUSTFLAGS='bad', CARGO_PROFILE_RELEASE_DEBUG='2', OXPINYIN_DSYM='1', PKG_CONFIG_PATH='/other')
            with self.assertRaisesRegex(ValueError, 'install libkyotocabinet-dev before measuring'):
                prepare.native_build('a' * 40, Path(directory) / 'none', env, linux=True, symbols=False)
            install.assert_not_called()
            answer['status'] = 0
            arm = prepare.native_build('a' * 40, Path(directory) / 'plain', env, linux=True, symbols=False)
            arguments, tools = install.call_args.args
            self.assertEqual(arguments[2:], ['--', '--locked', '--release',
                                             '--no-default-features', '--features', 'kyotocabinet'])
            for name in ('RUSTFLAGS', 'CARGO_PROFILE_RELEASE_DEBUG', 'OXPINYIN_DSYM', 'OXPINYIN_TKRZW_PREFIX'):
                self.assertNotIn(name, tools.env)
            self.assertEqual(tools.env['PKG_CONFIG_PATH'], '/other')
            self.assertEqual((arm['dylib'].name, arm['uuid'], arm['backend'], arm['linux']),
                             ('libpinyin.so.15.0.0', 'digest', 'kc', True))
            self.assertEqual(arm['build_options']['features'], ['kyotocabinet'])
            prepare.native_build('a' * 40, Path(directory) / 'profiled', env, linux=True)
            self.assertEqual(install.call_args.args[1].env['RUSTFLAGS'], '-Cdebuginfo=1 -Cstrip=none')
            self.assertNotIn('OXPINYIN_DSYM', install.call_args.args[1].env)
            backend.return_value = 'tkrzw'
            with self.assertRaises(Finished):
                prepare.native_build('a' * 40, Path(directory) / 'other', env, linux=True)

    def test_launch_timeout_bounds_the_worker(self):
        worker = bench.Bench('just measure', {}, Path('/stage/bisect'), '0', required=True)
        arm = bench.Arm('A', Path(LIBRARY), Path('/data'), Path(LIBRARY).parent)
        with mock.patch.object(bench, 'guard'), \
             mock.patch.object(bench, 'measured_process', return_value=(b'{}', None)) as bounded, \
             mock.patch.object(bench, 'capture', return_value=pool.Completed(0, b'[]', b'')) as unbounded:
            self.assertEqual(worker.launch(arm, 'speed', 2, timeout=7).stdout, b'{}')
            self.assertEqual((bounded.call_args.kwargs['timeout'], bounded.call_args.kwargs['usage']), (7, False))
            unbounded.assert_not_called()
            self.assertEqual(worker.launch(arm, 'speed', 2).stdout, b'[]')
            unbounded.reset_mock()
            # A deadline replaces the bound with the time left after the check; with none left nothing is launched.
            with mock.patch('time.monotonic', return_value=100):
                worker.launch(arm, 'speed', 2, deadline=103)
                self.assertEqual(bounded.call_args.kwargs['timeout'], 3)
                launched = bounded.call_count
                with self.assertRaises(subprocess.TimeoutExpired):
                    worker.launch(arm, 'speed', 2, deadline=100)
            self.assertEqual(bounded.call_count, launched)
            unbounded.assert_not_called()


if sys.platform == 'linux':
    # These read Callgrind profiles by their Linux paths and run the Linux cell's arms.
    class LinuxCallgrindCampaign(CampaignFixture, unittest.TestCase):
        """The Callgrind pass of the runner, which parses a profile written for Linux paths."""

        def test_a_refused_counter_selects_callgrind_for_the_whole_comparison(self):
            self.assertEqual(self.compare(), 0)
            commands = [command for command, _, _ in self.launches]
            # One unmeasured probe, the native schedule, then the same schedule under Callgrind.
            self.assertEqual([('valgrind' in command) for command in commands], [False] * 9 + [True] * 8)
            probe, native, profiled = self.launches[0], self.launches[1], self.launches[9]
            self.assertEqual((probe[1]['PERF_HW'], probe[1]['PERF_CYCLES'], probe[1]['PERF_MODE']), ('instructions', '2', 'speed'))
            self.assertNotIn('PERF_HW', native[1])
            self.assertEqual((native[1]['PERF_MODE'], native[1]['PERF_WORKLOAD_CHECKSUM'], native[2]), ('rss-diag', '1', False))
            # The existing launcher: pinned to the first allowed CPU, the arm's directory first on the loader path.
            self.assertEqual(native[0][:3], ['taskset', '-c', '3'])
            self.assertEqual(native[1]['LD_LIBRARY_PATH'], str(self.arms['A']['dylib'].parent) + ':/host/lib')
            self.assertIn(('kc', str(self.arms['A']['dylib']), native[1]['LD_LIBRARY_PATH']), self.guarded)
            # No --toggle-collect: the anchor's own client requests bracket the count, and a toggle would invert it.
            self.assertEqual(profiled[0][3:9], ['valgrind', '--tool=callgrind', '--collect-atstart=no', '--cache-sim=no',
                                                '--branch-sim=no', '--separate-threads=no'])
            self.assertEqual(profiled[1]['PERF_MODE'], 'speed')
            # Both passes walk the shared schedule; the control's B label is A' from its own prefix.
            order = [env['PERF_BACKEND'] for _, env, _ in self.launches[1:9]]
            self.assertEqual(order, ['A', 'B', 'A', 'C', 'B', 'A', 'C', 'A'])
            self.assertEqual(order, [env['PERF_BACKEND'] for _, env, _ in self.launches[9:]])
            runs = [json.loads(line) for line in (self.root / 'runs.jsonl').read_text().splitlines()]
            self.assertEqual([run['phase'] for run in runs], ['native'] * 8 + ['callgrind'] * 8)
            summary = json.loads((self.root / 'summary.json').read_text())
            self.assertEqual((summary['instruction_source'], summary['verdicts']['loop_callgrind_ir']), ('callgrind', 'VALID'))
            self.assertNotIn('loop_hw_instructions', summary['comparison'])

        def test_an_invalid_instruction_control_keeps_its_record_and_fails(self):
            self.ir = lambda label: 5000 + (label == 'C')
            self.assertEqual(self.compare(), 3)
            summary = json.loads((self.root / 'summary.json').read_text())
            self.assertEqual(summary['verdicts']['loop_callgrind_ir'], 'INVALID')


    class LinuxStaging(PreparationFixture, unittest.TestCase):
        """The native recipe up to staging, on the Linux cell."""

        def test_one_plain_build_and_three_identical_prefixes_of_one_length(self):
            from tools.dev import contract_data
            for mode, outcome in (('compare', 0), ('compare', ValueError('sample failed')), ('profile', 0)):
                with self.subTest(mode=mode, outcome=outcome), tempfile.TemporaryDirectory() as directory:
                    destinations, staged, environments = [], {}, []

                    def build(sha, destination, env, **options):
                        destinations.append((destination, options))
                        library = destination / 'prefix/lib/libpinyin.so.15.0.0'
                        library.parent.mkdir(parents=True)
                        library.write_bytes(b'product')
                        return dict(revision=sha, dylib=library, uuid='hash', linux=True, backend='kc')

                    def driver(arm, output, env):
                        output.write_bytes(b'driver')
                        return identity.sha256(output)

                    def measure(args, arms, env, output):
                        arms = arms if isinstance(arms, dict) and 'A' in arms else {'A': arms}
                        staged.update({label: (arm['dylib'], arm['dylib'].read_bytes(), arm['driver'], arm['label'])
                                       for label, arm in arms.items()})
                        environments.append(env)
                        if isinstance(outcome, Exception):
                            raise outcome
                        return outcome
                    with mock.patch.object(bench, 'capture', return_value=types.SimpleNamespace(status=0, stdout=b'a' * 40 + b'\n')), \
                         mock.patch.object(identity, 'first_output_line', return_value='tool 1.0'), \
                         mock.patch.object(prepare, 'native_build', side_effect=build) as builds, \
                         mock.patch.object(prepare, 'native_driver', side_effect=driver), \
                         mock.patch.object(contract_data, 'data_identity', return_value='data'), \
                         mock.patch.object(recipes, 'linux_compare', side_effect=measure), \
                         mock.patch.object(recipes, 'linux_profile', side_effect=measure):
                        argv = ([mode, '--base', 'HEAD', '--head', 'HEAD'] if mode == 'compare'
                                else [mode, '--template', 'Massif']) + ['--data', directory]
                        env = dict(OXPINYIN_CACHE=directory, PATH='/bin')
                        if isinstance(outcome, Exception):
                            with self.assertRaises(ValueError):
                                self.native(argv, env)
                        else:
                            self.assertEqual(self.native(argv, env), 0)
                    builds.assert_called_once()
                    # A comparison measures the plain shipping build; only a profile adds debug information.
                    self.assertEqual(destinations[0][1], dict(linux=True, symbols=mode == 'profile'))
                    self.assertEqual(set(staged), {'A', 'B', recipes.CONTROL} if mode == 'compare' else {'A'})
                    self.assertEqual({content for _, content, _, _ in staged.values()}, {b'product'})
                    self.assertEqual(len({path for path, _, _, _ in staged.values()}), len(staged))
                    self.assertEqual({len(str(path)) for path, _, _, _ in staged.values()}, {len(str(staged['A'][0]))})
                    self.assertEqual([label for _, _, _, label in staged.values()],
                                     [recipes.PREFIXES[label] for label in staged])
                    for path, _, executable, _ in staged.values():
                        self.assertEqual(executable, path.parents[1] / 'bin/bisect')
                    # Every launch runs in the caller's environment: the local cell comes from the
                    # distribution. The build tree is removed either way.
                    self.assertEqual(environments[0]['OXPINYIN_CACHE'], directory)
                    self.assertNotIn('OXPINYIN_TKRZW_PREFIX', environments[0])
                    self.assertNotIn('LD_LIBRARY_PATH', environments[0])
                    self.assertFalse(destinations[0][0].parent.exists())
                    self.assertTrue(destinations[0][0].parent.parent.is_dir(), 'retain session output')

        def test_the_linux_arms_read_the_prepared_host_cell_data(self):
            # Debian's libpinyin-data is tkrzw data; the Linux arms read the pin data that
            # `prepare oracle` wrote for the host's cell (tools/dev/cell.py).
            from tools.dev import cell, contract_data
            with tempfile.TemporaryDirectory() as directory:
                received = []

                def build(sha, destination, env, **options):
                    library = destination / 'prefix/lib/libpinyin.so.15.0.0'
                    library.parent.mkdir(parents=True)
                    library.write_bytes(b'product')
                    return dict(revision=sha, dylib=library, uuid='hash', linux=True, backend='kc')

                def driver(arm, output, env):
                    output.write_bytes(b'driver')
                    return identity.sha256(output)

                def measure(args, arms, env, output):
                    received.extend(arm['data'] for arm in arms.values())
                    return 0
                env = dict(OXPINYIN_CACHE=directory, PATH='/bin')
                prepared = prepare.pin_prefix(env, 'kc') / prepare.DATA_SUBDIRECTORY
                argv = ['compare', '--base', 'HEAD', '--head', 'HEAD']
                with mock.patch.object(bench, 'capture', return_value=types.SimpleNamespace(status=0, stdout=b'a' * 40 + b'\n')), \
                     mock.patch.object(identity, 'first_output_line', return_value='tool 1.0'), \
                     mock.patch.object(prepare, 'native_build', side_effect=build) as builds, \
                     mock.patch.object(prepare, 'native_driver', side_effect=driver) as drivers, \
                     mock.patch.object(contract_data, 'fetch') as fetch, \
                     mock.patch.object(contract_data, 'data_identity', return_value='data'), \
                     mock.patch.object(cell.platform, 'system', return_value='Linux'), \
                     mock.patch.object(recipes, 'linux_compare', side_effect=measure):
                    # Without the prepared data the recipe refuses before any revision or driver build.
                    with self.assertRaisesRegex(ValueError, 'no prepared kc pin data'):
                        self.native(argv, env)
                    builds.assert_not_called()
                    drivers.assert_not_called()
                    self.assertEqual(received, [])
                    prepared.mkdir(parents=True)
                    (prepared / 'table.conf').write_text('fixture\n')
                    self.assertEqual(self.native(argv, env), 0)
                fetch.assert_not_called()
                self.assertEqual(set(received), {prepared})


if __name__ == '__main__':
    unittest.main()
