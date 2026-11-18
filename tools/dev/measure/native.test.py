"""Native measurement contracts, using synthetic inputs rather than product measurements."""
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
from tools.dev.measure import bench, identity, neutrality, prepare, recipes, reports
from tools.dev.runners import Finished
from tools.dev.test_support import capi_metadata, native_row as sample


class NativeCampaignTests(unittest.TestCase):
    def test_balanced_schedule_keeps_outlier_and_diagnostic_control(self):
        args = recipes.native_arguments(['compare', '--base', 'x', '--head', 'x'])
        order = list(recipes.schedule(args.pairs, args.control_pairs))
        self.assertEqual(order[:8], [('comparison', 0, 'A'), ('comparison', 0, 'B'),
            ('control', 0, 'A'), ('control', 0, 'B'), ('comparison', 1, 'B'),
            ('comparison', 1, 'A'), ('control', 1, 'B'), ('control', 1, 'A')])
        observations = [sample() for _ in order]
        observations[3]['wall_seconds'] = 999
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(recipes, 'native_sample', side_effect=observations), contextlib.redirect_stdout(io.StringIO()):
            recipes.native_compare(args, {'A': 1, 'B': 2, recipes.CONTROL: 3}, 'data', {}, Path(directory))
            rows = [json.loads(line) for line in (Path(directory)/'runs.jsonl').read_text().splitlines()]
            self.assertEqual(len(rows), 24)
            self.assertEqual(rows[3]['wall_seconds'], 999)
            summary = (Path(directory)/'summary.json').read_text()
            self.assertEqual(json.loads(summary)['control']['wall_seconds']['B']['maximum'], 999)
            control = json.loads(summary)['control']
            self.assertEqual(list(control)[:2], ['loop_instructions', 'wall_seconds'])
            for counter in reports.COUNTS:
                self.assertEqual(control[counter]['B_over_A']['median'], 1)
            self.assertNotIn('B_over_A', control['loop_runnable_seconds'])
            self.assertAlmostEqual(control['loop_runnable_seconds']['A']['over_cpu_seconds']['median'], .09 / .08)
            self.assertEqual(set(json.loads(summary)['units']), set(reports.MEASURES))
            self.assertNotIn('verdict', summary)
            self.assertNotIn('stable', summary)
        with self.assertRaises(ValueError):
            list(recipes.schedule(3, 2))

    def test_failure_retains_completed_runs_without_retry(self):
        args = recipes.native_arguments(['compare', '--base', 'x', '--head', 'y'])
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(recipes, 'native_sample', side_effect=[sample(), ValueError('failed')]) as launch:
            with self.assertRaises(ValueError):
                recipes.native_compare(args, {'A': 1, 'B': 2, recipes.CONTROL: 3}, 'data', {}, Path(directory))
            self.assertEqual(launch.call_count, 2)
            self.assertEqual(len((Path(directory)/'runs.jsonl').read_text().splitlines()), 1)
            self.assertFalse((Path(directory)/'summary.json').exists())

    def test_budget_stops_without_retry(self):
        args = recipes.native_arguments(['compare', '--base', 'x', '--head', 'y'])
        with tempfile.TemporaryDirectory() as directory, mock.patch('time.monotonic', side_effect=[0, 181]), mock.patch.object(recipes, 'native_sample') as launch:
            with self.assertRaisesRegex(ValueError, 'budget'):
                recipes.native_compare(args, {}, 'data', {}, Path(directory))
            launch.assert_not_called()

    def test_child_timeout_reports_budget_and_retains_completed_run(self):
        args = recipes.native_arguments(['compare', '--base', 'x', '--head', 'y'])
        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(recipes, 'native_sample', side_effect=[sample(), subprocess.TimeoutExpired('driver', 1)]) as launch, \
             mock.patch.object(recipes.runners, 'emit') as emit:
            output = Path(directory)
            status = recipes.run(lambda argv, env: recipes.native_compare(args, {'A': 1, 'B': 2, recipes.CONTROL: 3}, 'data', env, output), [], {})
            self.assertEqual(status, 1)
            emit.assert_called_once_with('FAIL: measurement budget exhausted; campaign incomplete', error=True)
            self.assertEqual(launch.call_count, 2)
            self.assertEqual(len((output/'runs.jsonl').read_text().splitlines()), 1)
            self.assertFalse((output/'summary.json').exists())

    def test_profile_timeout_reports_budget_without_finalizing_or_retrying(self):
        args = recipes.native_arguments(['profile', '--template', 'Allocations'])
        arm = dict(driver='driver', dylib='library', uuid='a'*32)
        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(bench, 'run', side_effect=subprocess.TimeoutExpired('xctrace', args.budget)) as record, \
             mock.patch.object(recipes, 'finish_profile') as finish, \
             mock.patch.object(recipes.runners, 'emit') as emit:
            status = recipes.run(lambda argv, env: recipes.native_profile(args, arm, 'data', env, Path(directory)), [], {})
            self.assertEqual(status, 1)
            emit.assert_called_once_with('FAIL: profile budget exhausted before xctrace finished', error=True)
            record.assert_called_once()
            self.assertEqual(record.call_args.kwargs['timeout'], args.budget)
            finish.assert_not_called()

    if sys.platform == 'darwin':
        def test_distinct_sha_builds_once_and_no_backend_override(self):
            arm = {'revision': 'a'*40, 'dylib': Path('library'), 'uuid': '0'*32}
            with tempfile.TemporaryDirectory() as directory, \
                 mock.patch('platform.machine', return_value='arm64'), \
                 mock.patch.object(bench, 'capture', return_value=types.SimpleNamespace(status=0, stdout=b'a'*40+b'\n')), \
                 mock.patch.object(prepare, 'native_build', return_value=arm) as build, \
                 mock.patch.object(prepare, 'native_driver', side_effect=RuntimeError('end preparation')):
                with self.assertRaisesRegex(RuntimeError, 'end preparation'):
                    recipes.native(['compare', '--base', 'main', '--head', 'main'], {'OXPINYIN_CACHE': directory})
                build.assert_called_once()
                self.assertEqual(build.call_args.args[0], 'a'*40)
                self.assertEqual(build.call_args.args[2], {'OXPINYIN_CACHE': directory, 'RUSTUP_TOOLCHAIN': 'a'*40})
                self.assertEqual(build.call_args.kwargs, {})

        def test_second_invocation_builds_again_in_a_fresh_directory(self):
            from tools.dev import contract_data
            builds = []
            def build(revision, destination, env, **options):
                builds.append(destination)
                library = destination / 'prefix/lib/libpinyin.15.0.0.dylib'
                Path(str(library) + '.dSYM').mkdir(parents=True)
                library.write_bytes(f'build {len(builds)}'.encode())
                return dict(revision=revision, dylib=library, uuid='0'*32)
            def driver(arm, output, env):
                output.write_bytes(b'signed driver')
                return identity.sha256(output)
            with tempfile.TemporaryDirectory() as directory, \
                 mock.patch('platform.machine', return_value='arm64'), \
                 mock.patch.object(bench, 'capture', return_value=types.SimpleNamespace(status=0, stdout=b'a'*40+b'\n')), \
                 mock.patch.object(prepare, 'native_build', side_effect=build), \
                 mock.patch.object(prepare, 'native_driver', side_effect=driver), \
                 mock.patch.object(contract_data, 'data_identity', return_value={}), \
                 mock.patch.object(recipes, 'native_compare'), \
                 contextlib.redirect_stdout(io.StringIO()):
                for _ in range(2):
                    self.assertEqual(recipes.native(['compare', '--base', 'main', '--head', 'main', '--data', directory],
                                                    {'OXPINYIN_CACHE': directory}), 0)
                # Each invocation has its own session, and its staged A holds the bytes of its own build.
                sessions = sorted((Path(directory) / 'measure/runs').iterdir(), key=lambda path: path.stat().st_mtime_ns)
                self.assertEqual(len(sessions), 2)
                staged = [Path(json.loads((session / 'session.json').read_text())['staged']['A']) for session in sessions]
                for number, (session, prefix) in enumerate(zip(sessions, staged), 1):
                    self.assertEqual(prefix.parent, session)
                    self.assertEqual((prefix / 'lib/libpinyin.15.0.0.dylib').read_bytes(), f'build {number}'.encode())

        def test_control_labels_run_from_separate_identical_prefixes(self):
            from tools.dev import contract_data
            def build(revision, destination, env, **options):
                library = destination / 'prefix/lib/libpinyin.15.0.0.dylib'
                symbols = Path(str(library) + '.dSYM')
                symbols.mkdir(parents=True)
                library.write_bytes(b'product ' + revision.encode())
                (symbols / 'dwarf').write_bytes(b'symbols')
                return dict(revision=revision, dylib=library, uuid='0'*32)
            def driver(arm, output, env):
                output.write_bytes(b'signed driver')
                return identity.sha256(output)
            launched = []
            def launch(arm, data, cycles, env, timeout):
                launched.append(arm)
                return sample()
            def contents(prefix):
                return {path.relative_to(prefix): path.read_bytes()
                        for path in sorted(prefix.rglob('*')) if path.is_file()}
            # A and B share one revision here, so every prefix must hold the same bytes.
            with tempfile.TemporaryDirectory() as directory, \
                 mock.patch('platform.machine', return_value='arm64'), \
                 mock.patch.object(bench, 'capture', return_value=types.SimpleNamespace(status=0, stdout=b'a'*40+b'\n')), \
                 mock.patch.object(prepare, 'native_build', side_effect=build) as built, \
                 mock.patch.object(prepare, 'native_driver', side_effect=driver), \
                 mock.patch.object(contract_data, 'data_identity', return_value={}), \
                 mock.patch.object(recipes, 'native_sample', side_effect=launch), \
                 contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(recipes.native(['compare', '--base', 'main', '--head', 'main', '--pairs', '2',
                                                 '--control-pairs', '2', '--data', directory],
                                                {'OXPINYIN_CACHE': directory}), 0)
                built.assert_called_once()
                session = next((Path(directory) / 'measure/runs').iterdir())
                rows = [json.loads(line) for line in (session / 'runs.jsonl').read_text().splitlines()]
                self.assertEqual(len(rows), len(launched))
                prefixes = {}
                for row, arm in zip(rows, launched):
                    self.assertEqual(arm['driver'].parents[1], arm['dylib'].parents[1])
                    prefixes.setdefault((row['kind'], row['arm']), set()).add(arm['dylib'].parents[1])
                self.assertEqual({key: len(value) for key, value in prefixes.items()},
                                 dict.fromkeys([('comparison', 'A'), ('comparison', 'B'),
                                                ('control', 'A'), ('control', 'B')], 1))
                first, second = (next(iter(prefixes['control', label])) for label in 'AB')
                self.assertNotEqual(first, second)
                self.assertEqual(first, next(iter(prefixes['comparison', 'A'])))
                self.assertNotEqual(second, next(iter(prefixes['comparison', 'B'])))
                self.assertEqual(sorted(map(str, contents(first))),
                                 ['bin/bisect', 'lib/libpinyin.15.0.0.dylib',
                                  'lib/libpinyin.15.0.0.dylib.dSYM/dwarf'])
                self.assertEqual(contents(first), contents(second))
                self.assertEqual(json.loads((session / 'session.json').read_text())['staged'][recipes.CONTROL],
                                 str(second))

        def test_staged_product_bytes_must_match_the_build(self):
            with tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                library = root / 'libpinyin.dylib'
                library.write_bytes(b'product')
                Path(str(library) + '.dSYM').mkdir()
                (root / 'bisect').write_bytes(b'driver')
                digest = identity.sha256(root / 'bisect')
                real = __import__('shutil').copy2
                def corrupt(source, target):
                    real(source, target)
                    if Path(target).name == library.name:
                        Path(target).write_bytes(b'other')
                with mock.patch('shutil.copy2', side_effect=corrupt), \
                     self.assertRaisesRegex(ValueError, 'staged product bytes differ'):
                    recipes.stage_arm(root / 'A', {'dylib': library}, root / 'bisect', digest)

        def test_invocation_builds_removed_on_success_failure_and_timeout(self):
            from tools.dev import contract_data
            for mode in ('compare', 'profile', 'neutrality'):
                for outcome in (None, ValueError('failed'), subprocess.TimeoutExpired('child', 1)):
                    with self.subTest(mode=mode, outcome=outcome), tempfile.TemporaryDirectory() as directory:
                        destinations = []
                        def build(revision, destination, env, **options):
                            destination.mkdir(parents=True)
                            (destination/'artifact').write_text('build artifact')
                            destinations.append(destination)
                            return dict(revision=revision, dylib=destination/'library', uuid='0'*32)
                        with mock.patch('platform.machine', return_value='arm64'), \
                             mock.patch.object(bench, 'capture', return_value=types.SimpleNamespace(status=0, stdout=b'a'*40+b'\n')), \
                             mock.patch.object(prepare, 'native_build', side_effect=build), \
                             mock.patch.object(prepare, 'native_driver', return_value='digest'), \
                             mock.patch('shutil.copy2'), mock.patch('shutil.copytree'), \
                             mock.patch.object(identity, 'sha256', return_value='digest'), \
                             mock.patch.object(contract_data, 'data_identity', return_value={}), \
                             mock.patch.object(recipes, 'native_compare', side_effect=outcome), \
                             mock.patch.object(recipes, 'native_profile', side_effect=outcome), \
                             mock.patch.object(neutrality, 'compare_macho', side_effect=outcome), \
                             contextlib.redirect_stdout(io.StringIO()):
                            argv = [mode, '--base', 'main', '--head', 'main', '--template', 'Allocations', '--data', directory]
                            if outcome is None:
                                self.assertEqual(recipes.native(argv, {'OXPINYIN_CACHE': directory}), 0)
                            else:
                                with self.assertRaises(type(outcome)):
                                    recipes.native(argv, {'OXPINYIN_CACHE': directory})
                        self.assertTrue(destinations)
                        for destination in destinations:
                            self.assertFalse(destination.parent.exists())
                            self.assertTrue(destination.parent.parent.is_dir(), 'retain session output')



class AllocationDetails(unittest.TestCase):
    toc = '<trace-toc><run number="1"><info><target><process pid="17" name="bisect" type="launched" return-exit-status="0"/></target></info><processes><process pid="17" path="driver"/></processes><tracks><track name="Allocations"><details><detail name="Allocations List" kind="table"/><detail name="Statistics" kind="table"/></details></track></tracks></run></trace-toc>'
    xml = '<trace-query-result><node><row address="0x1234" size="64" category="pthread_mutex_t" timestamp="00:01.310.951" live="true"><backtrace><frame id="0" name="0x1020" addr="0x1020"><binary id="1" UUID="aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa" load-addr="0x1000"/></frame></backtrace></row><row address="0x1240" size="32" category="Malloc 32 Bytes" timestamp="00:01.311.091" live="true"><backtrace><frame ref="0"/></backtrace></row></node></trace-query-result>'

    def test_toc_binds_the_list_to_the_successful_driver(self):
        path = reports.allocation_path(self.toc, pid=17, driver=Path('driver'))
        self.assertEqual(path, '/trace-toc/run[@number="1"]/tracks/track[@name="Allocations"]/details/detail[@name="Allocations List"]')
        for broken in (self.toc.replace('pid="17"', 'pid="18"'),
                       self.toc.replace('path="driver"', 'path="other"'),
                       self.toc.replace('status="0"', 'status="1"'),
                       self.toc.replace('Allocations List', 'Statistics')):
            with self.subTest(broken=broken), self.assertRaises(ValueError):
                reports.allocation_path(broken, pid=17, driver=Path('driver'))

    def test_allocation_attributes_references_uuid_and_dsym_resolution(self):
        with tempfile.TemporaryDirectory() as directory:
            library = Path(directory)/'lib.dylib'
            dwarf = Path(str(library)+'.dSYM')/'Contents/Resources/DWARF/lib.dylib'
            dwarf.parent.mkdir(parents=True)
            dwarf.touch()
            with mock.patch.object(identity, 'macho_uuid', return_value='a'*32), \
                 mock.patch.object(bench, 'capture', return_value=types.SimpleNamespace(status=0, stdout=b'pinyin_init (in lib.dylib) (context.rs:78)\n')) as atos:
                result = reports.allocation_rows(self.xml, uuid='a'*32, dylib=library)
                self.assertEqual(result['allocation_rows'], 2)
                self.assertEqual(result['live_rows'], 2)
                self.assertEqual(result['product_rows'], 2)
                self.assertEqual(result['resolved_product_functions'], ['pinyin_init'])
                self.assertEqual(atos.call_args.args[0], ['xcrun', 'atos', '-arch', 'arm64', '-o', dwarf, '-l', '0x1000', '0x1020'])
                for broken in (self.xml.replace('size="64"', 'size="0"'),
                               self.xml.replace('category="pthread_mutex_t"', 'category=""'),
                               self.xml.replace('<backtrace>', '<absent>').replace('</backtrace>', '</absent>'),
                               self.xml.replace('aaaaaaaa-', 'bbbbbbbb-'),
                               self.xml.replace('ref="0"', 'ref="missing"')):
                    with self.subTest(broken=broken), self.assertRaises(ValueError):
                        reports.allocation_rows(broken, uuid='a'*32, dylib=library)
                atos.return_value.stdout = b'0x1020\n'
                with self.assertRaisesRegex(ValueError, 'known product'):
                    reports.allocation_rows(self.xml, uuid='a'*32, dylib=library)
            with mock.patch.object(identity, 'macho_uuid', return_value='b'*32):
                with self.assertRaisesRegex(ValueError, 'UUID'):
                    reports.allocation_rows(self.xml, uuid='a'*32, dylib=library)

    statistics = '<result><row category="All Heap &amp; Anonymous VM" count-total="3" count-transient="2" count-persistent="1" total-bytes="96" transient-bytes="64" persistent-bytes="32"/><row category="Malloc 32 Bytes" count-total="3" count-transient="2" count-persistent="1" total-bytes="96" transient-bytes="64" persistent-bytes="32"/></result>'

    def test_statistics_requires_counts_bytes_and_consistent_totals(self):
        result = reports.allocation_statistics(self.statistics)
        self.assertEqual(result['overall']['count-total'], 3)
        self.assertEqual(result['categories']['Malloc 32 Bytes']['persistent-bytes'], 32)
        for broken in (self.statistics.replace('count-total="3"', 'count-total="4"'),
                       self.statistics.replace('total-bytes="96"', 'total-bytes="95"'),
                       self.statistics.replace('count-transient="2"', ''),
                       self.statistics.replace('persistent-bytes="32"', 'persistent-bytes="-1"'),
                       self.statistics.replace('All Heap &amp; Anonymous VM', 'Other'),
                       self.statistics.replace('Malloc 32 Bytes', 'All Heap Allocations')):
            with self.subTest(broken=broken), self.assertRaises(ValueError):
                reports.allocation_statistics(broken)
        with self.assertRaisesRegex(ValueError, 'TOC'):
            reports.allocation_path(self.toc.replace('name="Statistics"', 'name="Absent"'),
                                    pid=17, driver=Path('driver'), detail_name='Statistics')

    def test_profile_exports_the_toc_listed_detail(self):
        args = recipes.native_arguments(['profile', '--template', 'Allocations'])
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            (output/'driver.json').write_text(json.dumps(sample()))
            arm = dict(driver=Path('driver'), dylib=Path('lib.dylib'), uuid='a'*32)
            with mock.patch.object(bench, 'run'), \
                 mock.patch.object(bench, 'capture', side_effect=[types.SimpleNamespace(status=0, stdout=self.toc.encode()), types.SimpleNamespace(status=0, stdout=self.xml.encode()), types.SimpleNamespace(status=0, stdout=self.statistics.encode())]) as capture, \
                 mock.patch.object(reports, 'allocation_rows', return_value={'allocation_rows': 2}) as validate, \
                 contextlib.redirect_stdout(io.StringIO()):
                recipes.finish_profile(args, arm, output)
                self.assertEqual(capture.call_args.args[0][-1], reports.allocation_path(self.toc, pid=17, driver=Path('driver'), detail_name='Statistics'))
                self.assertTrue((output/'statistics.xml').is_file())
                validate.assert_called_once_with(self.xml.encode(), uuid='a'*32, dylib=Path('lib.dylib'))


class NativePreparation(unittest.TestCase):
    def test_packaged_default_release_and_matching_dsym(self):
        def metadata(root, env=None):
            return capi_metadata(root, features={'default': ['declared']})
        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(bench, 'run'), \
             mock.patch('tools.dev.packaging.cargo_metadata', side_effect=metadata), \
             mock.patch('tools.dev.packaging.install_library') as install, \
             mock.patch.object(identity, 'macho_uuid', return_value='a'*32):
            destination = Path(directory)/'build'
            pkgconfig = Path(directory)/'installed/arm64-osx-oxpinyin/lib/pkgconfig'
            pkgconfig.mkdir(parents=True)
            (pkgconfig/'tkrzw.pc').touch()
            env = {'RUSTUP_TOOLCHAIN': 'selected', 'VCPKG_ROOT': directory, 'PKG_CONFIG_PATH': '/other'}
            arm = prepare.native_build('a'*40, destination, env)
            arguments, tools = install.call_args.args
            self.assertEqual(arguments, ['libpinyin', '--prefix='+str(destination/'prefix'), '--',
                                         '--locked', '--release'])
            self.assertEqual(tools.env['RUSTFLAGS'], '-Cdebuginfo=1 -Cstrip=none')
            self.assertEqual(tools.env['RUSTUP_TOOLCHAIN'], 'selected')
            self.assertEqual(tools.env['OXPINYIN_DSYM'], '1')
            self.assertEqual(tools.env['PKG_CONFIG_PATH'], str(pkgconfig.resolve()) + os.pathsep + '/other')
            self.assertEqual(arm['default_features'], ['declared'])
            with mock.patch.object(identity, 'macho_uuid', side_effect=['a'*32, 'b'*32]):
                with self.assertRaises(Finished):
                    prepare.native_build('b'*40, Path(directory)/'bad', env)

    def test_native_build_requires_the_vcpkg_triplet(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(bench, 'run') as run:
            for env in ({}, {'VCPKG_ROOT': directory}):
                with self.subTest(env=env), self.assertRaises(ValueError):
                    prepare.native_build('a'*40, Path(directory)/'build', env)
            run.assert_not_called()

    def test_profile_passes_environment_and_propagates_invalid_capture(self):
        args = recipes.native_arguments(['profile', '--template', 'CPU Profiler'])
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            arm = dict(driver=Path('driver'), dylib=Path('lib.dylib'), uuid='a'*32)
            def command(argv, **kwargs):
                if 'record' in argv:
                    row = sample()
                    del row['peak_rss_bytes']
                    (output/'driver.json').write_text(json.dumps(row))
            with mock.patch.object(bench, 'run', side_effect=command) as run, \
                 mock.patch.object(bench, 'capture', return_value=types.SimpleNamespace(status=0, stdout=b'<empty/>')), \
                 mock.patch.object(reports, 'capture_rows', side_effect=ValueError('wrong process')) as validate:
                with self.assertRaisesRegex(ValueError, 'wrong process'):
                    recipes.native_profile(args, arm, 'data', {}, output)
                self.assertEqual(run.call_args_list[0].kwargs['timeout'], args.budget)
                record = run.call_args_list[0].args[0]
                self.assertEqual(record[record.index('--env')+1], 'PERF_CYCLES=20')
                self.assertEqual(record[-6:], ['--', Path('driver'), '--measure', Path('lib.dylib'), 'a'*32, 'data'])
                self.assertEqual(validate.call_args.kwargs, dict(pid=17, uuid='a'*32, allocations=False))


if __name__ == '__main__':
    unittest.main()
