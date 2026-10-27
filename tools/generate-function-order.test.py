#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Source regressions for the in-tree policy generator and measurement reader.

Synthetic nm, Callgrind, smaps and linker records exercise parsing, scoping,
normalization and reconciliation. They need no compiler, profiler or worker.
"""

import ast
import contextlib
import hashlib
import io
import os
from pathlib import Path
import re
import runpy
import sys
import tempfile
import threading
import unittest
from types import SimpleNamespace
from unittest import mock


HERE = Path(__file__).resolve().parent
GENERATOR = runpy.run_path(str(HERE / "generate-function-order.py"))
MEASUREMENT = runpy.run_path(str(HERE / "measure-function-order.py"))
TEARDOWN = runpy.run_path(str(HERE / "bisection/teardown.py"))
LIBRARY = Path("/synthetic/libpinyin.so")
HEADER = "oxpinyin-function-order-v1\tlibpinyin.so.15\n"


def row(name, section, address="0000000000000000"):
    return f"{name}|{address}| t | FUNC |0000000000000010| |{section}\n"


def profile(body, total):
    return f"version: 1\npositions: instr line\nevents: Ir\n{body}totals: {total}\n"


class GenerationPreflightTests(unittest.TestCase):
    @staticmethod
    def tree(root):
        return {str(path.relative_to(root)): path.read_bytes() if path.is_file() else None
                for path in root.rglob('*')}

    def refusal(self, platform, missing, expected, expected_lookups):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('work', 'build'):
                (root / name).mkdir()
                (root / name / 'keep').write_bytes(b'unchanged')
            campaign = root / 'campaign.json'
            campaign.write_text('[]')
            before = self.tree(root)
            variants = (
                ['--bisect', str(root / 'worker'), '--work-dir', str(root / 'new-work'),
                 '--output-dir', str(root / 'out')],
                ['--campaign', str(campaign)],
                ['--campaign', str(campaign), '--compare-base', str(root / 'base.py')],
                ['--output-dir', str(root / 'out')],
            )
            for argv in variants:
                with self.subTest(argv=argv):
                    looked_up, executed = [], []
                    def which(tool):
                        looked_up.append(tool)
                        return None if tool == missing else f'/tools/{tool}'
                    def execute(args, parser):
                        # All admission, worker launches and directory changes
                        # belong to this execution phase, after the CLI guard.
                        executed.append('generation')
                        self.fail('generation started before platform/tool refusal')
                    err = io.StringIO()
                    status = GENERATOR['main'](argv, platform=platform, which=which,
                                               execute=execute, err=err)
                    self.assertEqual(status, 2)
                    self.assertEqual(err.getvalue(), expected + '\n')
                    self.assertEqual(looked_up, expected_lookups)
                    self.assertEqual(executed, [])
                    self.assertEqual(self.tree(root), before)

    def test_macos_refuses_before_tool_lookup_or_generation(self):
        self.refusal('darwin', None, 'function ordering: generation requires Linux (host: darwin)', [])

    def test_linux_without_taskset_refuses_before_generation(self):
        self.refusal('linux', 'taskset', 'function ordering: generation requires taskset on PATH', ['taskset'])

    def test_linux_without_valgrind_refuses_before_generation(self):
        self.refusal('linux', 'valgrind', 'function ordering: generation requires valgrind on PATH',
                     ['taskset', 'valgrind'])

    def test_supported_host_dispatches_once_after_checking_both_tools(self):
        looked_up, executed = [], []
        def which(tool):
            looked_up.append(tool)
            return f'/tools/{tool}'
        def execute(args, parser):
            executed.append(args.campaign)
            return 0
        err = io.StringIO()
        self.assertEqual(GENERATOR['main'](['--campaign', 'campaign.json'], platform='linux', which=which,
                                          execute=execute, err=err), 0)
        self.assertEqual(looked_up, ['taskset', 'valgrind'])
        self.assertEqual(executed, [Path('campaign.json')])
        self.assertEqual(err.getvalue(), '')


class MeasurementTests(unittest.TestCase):
    def steady_campaign(self, guard):
        tree = ast.parse((HERE / 'measure-function-order.py').read_text())
        loops = [node for node in ast.walk(tree)
                 if isinstance(node, ast.For) and isinstance(node.target, ast.Name)
                 and node.target.id == 'facade' and isinstance(node.body[0], ast.Assign)
                 and any(isinstance(target, ast.Name) and target.id == 'measurements'
                         for target in node.body[0].targets)]
        self.assertEqual(len(loops), 1)
        libraries = {(arm, facade): Path(f'/original/{arm}/lib{facade}.so')
                     for arm in MEASUREMENT['ARMS'] for facade in MEASUREMENT['FACADES']}
        def launch(command, environment):
            guard.assert_called_with('auto', 'measure-function-order.py',
                                     command[-2].parent.name, command[-2], env=environment)
            self.assertIs(guard.call_args.kwargs['env'], environment)
            self.assertEqual(environment['LD_LIBRARY_PATH'], '/verified/runtime')
            self.assertEqual(environment['PERF_FACADE'], command[-2].stem.removeprefix('lib'))
            return b'{"cycles_ns":[1,1,1,1,1,1,1,1],"workload_checksum":"same"}', b''
        run = mock.Mock(side_effect=launch)
        scope = {**MEASUREMENT, 'libraries': libraries, 'check_tkrzw': guard, 'run': run,
                 'env': {'LD_LIBRARY_PATH': '/verified/runtime'},
                 'args': SimpleNamespace(cpu=2, bisect='/fixture/bisect',
                                         system_dir=Path('/tables'), rounds=1)}
        code = compile(ast.Module(body=loops, type_ignores=[]), str(HERE / 'measure-function-order.py'), 'exec')
        with contextlib.redirect_stdout(io.StringIO()):
            exec(code, scope)
        return run

    def test_final_steady_launch_checks_original_library_and_exact_environment(self):
        guard = mock.Mock()
        run = self.steady_campaign(guard)
        self.assertEqual(run.call_count, 24)
        self.assertEqual(guard.call_count, run.call_count)

    def test_final_steady_refusal_prevents_launch(self):
        guard = mock.Mock(side_effect=RuntimeError('wrong tkrzw runtime'))
        with self.assertRaisesRegex(RuntimeError, 'wrong tkrzw runtime'):
            self.steady_campaign(guard)

    def test_time_sample_takes_the_cycle_row_before_the_closed_row(self):
        sample = MEASUREMENT["time_sample"](
            '{"teardown":"sample","cycle":1,"observations":[],"times":{"init_ns":{"wall_ns":5}}}\n'
            '{"teardown":"closed","status":0,"times":{"dlclose_ns":{"wall_ns":1}}}\n')
        self.assertEqual(sample["times"]["init_ns"]["wall_ns"], 5)
        with self.assertRaisesRegex(ValueError, "one teardown sample row, got 0"):
            MEASUREMENT["time_sample"]('{"teardown":"closed","status":0,"times":{}}\n')

    def test_unordered_arm_is_refused_before_any_measurement(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'lib').mkdir()
            for facade in ('pinyin', 'zhuyin'):
                (root / 'lib' / f'lib{facade}.so.15.0.0').write_bytes(b'not an ordered ELF')
            for profile in ('phrases', 'journals'):
                (root / profile).mkdir()
                (root / profile / 'profile.manifest').write_text('')
            argv = ['measure-function-order.py']
            for arm in ('base', 'pr', 'control', 'pin'):
                argv += ['--subject', f'{arm}={root}']
            for option in ('bisect', 'system-dir', 'profiles-dir', 'work-dir'):
                argv += ['--' + option, str(root)]
            argv += ['--cpu', '0', '--system-identity', hashlib.sha256(b'').hexdigest()]
            with mock.patch.object(sys, 'argv', argv), \
                    mock.patch.dict(MEASUREMENT['main'].__globals__, run=lambda *a: (b'ext4', b'')), \
                    mock.patch.dict(MEASUREMENT['GEN'], manifest=lambda *a: '',
                                    collection_scratch=mock.Mock(side_effect=AssertionError('measurement started'))):
                with self.assertRaisesRegex(ValueError, 'function ordering: arm base/pinyin refused'):
                    MEASUREMENT['main']()

    def setUp(self):
        # These are 4 KiB Linux ELF fixtures, independent of the test host's
        # native page size (macOS arm64 uses 16 KiB).
        pages = mock.patch.dict(MEASUREMENT["executed_profile"].__globals__, PAGE=4096)
        pages.start()
        self.addCleanup(pages.stop)

    def test_residency_below_pin_and_the_exact_upper_boundary_meet_target(self):
        meets = MEASUREMENT["meets_residency_target"]
        self.assertTrue(meets(356, 452, 0))
        self.assertTrue(meets(368, 436, 0))
        self.assertTrue(meets(500, 452, 48))
        self.assertFalse(meets(501, 452, 0))
        self.assertFalse(meets(356, 452, 49))

    def test_instruction_positions_include_call_row_bases_and_cross_page_bytes(self):
        text = profile(
            "ob=(1) /synthetic/libpinyin.so\nfn=(1) f\n0xfff 1 2\n"
            "cob=(2) /foreign.so\ncfn=(2) target\ncalls=1 0x50 2\n"
            "0x1001 2 999\n+1 * 3\nob=(2)\nfn=(3) foreign\n0x80 4 5\n", 10)
        row = MEASUREMENT["executed_profile"](text.encode(), LIBRARY, {0xfff: 2, 0x1002: 1})
        self.assertEqual(row, {"ir": 10, "pages": {0, 1}, "bytes": 3})

    def test_a_toggle_outside_the_library_body_profiles_as_empty(self):
        text = profile("ob=(1) /foreign.so\nfn=(1) frame_dummy\n0x80 1 4\n", 4)
        row = MEASUREMENT["executed_profile"](text.encode(), LIBRARY, {0x80: 1})
        self.assertEqual(row, {"ir": 4, "pages": set(), "bytes": 0})

    def test_executed_addresses_must_match_original_elf(self):
        text = profile("ob=/synthetic/libpinyin.so\nfn=f\n0x123 1 2\n", 2)
        with self.assertRaisesRegex(ValueError, "does not match ELF"):
            MEASUREMENT["executed_profile"](text.encode(), LIBRARY, {0x124: 1})

    def test_only_executable_mappings_of_the_selected_inode_count(self):
        text = ("1000-4000 r-xp 0000 08:01 42 /selected.so\nRss: 8 kB\nPss: 6 kB\n"
                "4000-8000 r--p 3000 08:01 42 /selected.so\nRss: 4 kB\nPss: 4 kB\n"
                "8000-9000 r-xp 0000 08:01 43 /foreign.so\nRss: 4 kB\nPss: 4 kB\n")
        self.assertEqual(MEASUREMENT["text_memory"](text, 42), {"text_pages": 2, "text_pss_kib": 6})


class ParallelCollectionTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.campaigns = []
        for backend in ('bdb', 'kc', 'tkrzw'):
            root = self.root / backend
            root.mkdir()
            data = root / 'system'
            data.mkdir()
            (data / 'table.conf').write_text('system')
            for profile in GENERATOR['PROFILES']:
                user = root / 'profiles' / profile / 'user'
                user.mkdir(parents=True)
                (user / 'state').write_text('original')
                (user.parent / 'profile.manifest').write_text(GENERATOR['manifest'](user))
            args = SimpleNamespace(bisect=Path('/worker'), system_dir=data,
                                   system_identity=GENERATOR['STARTUP_PROFILE']['identity'](data),
                                   profiles_dir=root / 'profiles', work_dir=root, init=[], steady=[],
                                   output_dir=root / 'output', repeat_output_dir=root / 'repeat')
            libraries = {facade: root / f'lib{facade}.so' for facade in GENERATOR['FACADES']}
            self.campaigns.append((backend, args, libraries, libraries))

    def run_collections(self, toggle, *, workers=6, steady=None):
        from tools.dev import pool
        @contextlib.contextmanager
        def admitted(*args, **kwargs):
            yield pool.Budget(workers, {'ADMITTED': 'yes', pool.POOL_WORKERS: str(workers)}, (42,))
        collect = GENERATOR['collect_campaigns']
        with mock.patch.dict(collect.__globals__, teardown_exports=lambda *a: '',
                             startup_collections=lambda *a: [('phrases', 'a'), ('journals', 'b')],
                             collect_toggle=toggle, collect_steady=steady or (lambda *a: {'hot', 'shared'}),
                             library_symbols=lambda *a: {}, archive_paths=lambda *a: {},
                             resolved_paths=lambda functions, *a: functions), \
                mock.patch.object(GENERATOR['host'], 'allowed_cpus', return_value=set(range(6))), \
                mock.patch.object(pool, 'admitted', side_effect=admitted) as admission, \
                mock.patch.object(pool, 'run_admitted', wraps=pool.run_admitted) as runner:
            result = collect(self.campaigns, jobs=workers)
        return result, admission, runner

    def test_three_backends_and_two_passes_share_admission_and_private_cpu_leases(self):
        barrier = threading.Barrier(6)
        lock = threading.Lock()
        active, visited, first = set(), [], []
        def toggle(args, facade, library, user, name, environment):
            self.assertEqual((user / 'state').read_text(), 'original')
            (user / 'state').write_text('mutated')
            self.assertEqual(environment['ADMITTED'], 'yes')
            self.assertEqual(args.context.fds, (42,))
            with lock:
                self.assertNotIn(args.cpu, active)
                active.add(args.cpu)
                visited.append(user)
                initial = len(visited) <= 6
                if initial:
                    first.append(library.parent.name)
            if initial:
                barrier.wait(timeout=5)
            with lock:
                active.remove(args.cpu)
            return {name, 'shared'}
        result, admission, runner = self.run_collections(toggle)
        self.assertEqual(admission.call_count, 1)
        self.assertEqual(runner.call_count, 1)
        units = runner.call_args.args[0]
        self.assertEqual([name.split('-pass')[0] for name, _ in units[:6]],
                         ['bdb', 'kc', 'tkrzw', 'bdb', 'kc', 'tkrzw'])
        self.assertEqual(sorted(first), ['bdb', 'bdb', 'kc', 'kc', 'tkrzw', 'tkrzw'])
        self.assertEqual(len(set(visited)), 24)
        self.assertTrue(all(not path.exists() for path in visited))
        for backend in result:
            self.assertEqual(backend[0], backend[1])
            self.assertIn('init\ta\ninit\tb\ninit\tshared\nsteady\thot\n', backend[0]['libpinyin.order'])
        for _, args, _, _ in self.campaigns:
            for profile in GENERATOR['PROFILES']:
                self.assertEqual((args.profiles_dir / profile / 'user/state').read_text(), 'original')

    def test_worker_failure_propagates_and_removes_all_scratch(self):
        def failed(*args):
            raise ValueError('bad stream')
        with self.assertRaisesRegex(ValueError, 'bad stream'):
            self.run_collections(failed, workers=2)
        self.assertEqual(list(self.root.glob('*/function-order-*')), [])
        self.assertTrue(all(not args.output_dir.exists() for _, args, _, _ in self.campaigns))

    def test_fixture_changed_during_collection_is_rejected_before_publication(self):
        changed = self.campaigns[0][1].profiles_dir / 'phrases/user/state'
        def toggle(*args):
            changed.write_text('changed')
            return {'function'}
        with self.assertRaisesRegex(ValueError, 'profile changed during collection'):
            self.run_collections(toggle, workers=1)
        self.assertTrue(all(not args.output_dir.exists() for _, args, _, _ in self.campaigns))

    def test_single_cpu_still_uses_shared_admission(self):
        from tools.dev import pool
        with mock.patch.object(GENERATOR['host'], 'allowed_cpus', return_value={2, 5}), \
                mock.patch.object(pool, 'run_admitted', side_effect=ValueError('reached shared runner')) as runner, \
                mock.patch.dict(GENERATOR['collect_campaigns'].__globals__, collection_units=lambda *a: []):
            with self.assertRaisesRegex(ValueError, 'reached shared runner'):
                GENERATOR['collect_campaigns'](self.campaigns, cpu=5, jobs=1)
            self.assertEqual(runner.call_args.kwargs['cpus'], [5])
            self.assertEqual(runner.call_args.kwargs['max_workers'], 1)
            self.assertNotIn('serial', runner.call_args.kwargs)

    def test_comparison_uses_serial_base_and_one_then_two_candidate_passes(self):
        compare = GENERATOR['compare_base']
        baseline = self.root / 'base.py'
        baseline.touch()
        wanted = {'libpinyin.order': 'p', 'libzhuyin.order': 'z'}
        def run_base(units, label, **kwargs):
            self.assertEqual(kwargs['max_workers'], 1)
            self.assertEqual(kwargs['cpus'], [2])
            self.assertEqual(len(units), 3)
            from tools.dev import pool
            def launch(context, command):
                self.assertEqual(command[command.index('--cpu') + 1], '2')
                self.assertNotIn('--repeat-output-dir', command)
                self.assertEqual(command[3], str(baseline))
                destination = Path(command[command.index('--output-dir') + 1])
                destination.mkdir()
                for name, contents in wanted.items():
                    (destination / name).write_text(contents)
                return pool.Completed(0, b'', b'')
            with mock.patch.object(pool.UnitContext, 'launch', launch):
                return [function(pool.UnitContext(name, threading.Event(), cpu=2))
                        for name, function in units]
        def collect(campaigns, **kwargs):
            kwargs['report']['workers'] = 3
            return [[wanted.copy() for _ in range(2 if args.repeat_output_dir else 1)]
                    for _, args, _, _ in campaigns]
        with mock.patch.object(GENERATOR['host'], 'allowed_cpus', return_value={2, 5}), \
                mock.patch.object(GENERATOR['pool'], 'run_admitted', side_effect=run_base), \
                mock.patch.dict(compare.__globals__, collect_campaigns=collect), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            compare(self.campaigns, baseline)
        summary = GENERATOR['json'].loads(output.getvalue())
        self.assertEqual(summary['policies'], 6)
        self.assertEqual(summary['parallel_workers'], 3)
        for _, args, _, _ in self.campaigns:
            self.assertEqual((args.output_dir / 'libpinyin.order').read_text(), 'p')
            self.assertEqual((args.repeat_output_dir / 'libpinyin.order').read_text(), 'p')

    def test_one_backend_comparison_uses_the_selected_cpu_and_admitted_environment(self):
        from tools.dev import pool
        compare = GENERATOR['compare_base']
        baseline = self.root / 'base.py'
        baseline.touch()
        locks = self.root / 'locks'
        locks.mkdir()
        wanted = {'libpinyin.order': 'p', 'libzhuyin.order': 'z'}
        launched = []
        def launch(context, command):
            launched.append(command)
            self.assertEqual(command[command.index('--cpu') + 1], '5')
            self.assertEqual(context.cpu, 5)
            for name in ('PATH', 'HOME'):
                self.assertEqual(context.environment.get(name), os.environ[name])
            self.assertEqual(Path(context.environment['OXPINYIN_LOCK_DIR']), locks)
            marker = GENERATOR['json'].loads(context.environment['OXPINYIN_HEAVY_SLOT_HELD'])
            self.assertEqual(context.fds, (marker['fd'],))
            os.fstat(marker['fd'])
            destination = Path(command[command.index('--output-dir') + 1])
            destination.mkdir()
            for name, contents in wanted.items():
                (destination / name).write_text(contents)
            return pool.Completed(0, b'', b'')
        def collect(campaigns, **kwargs):
            kwargs['report']['workers'] = 1
            return [[wanted.copy() for _ in range(2 if args.repeat_output_dir else 1)]
                    for _, args, _, _ in campaigns]
        snapshot = dict(available_mib=12000, total_mib=16384, cpus=4, cargo_cpus=4, notes=[])
        with mock.patch.object(GENERATOR['host'], 'allowed_cpus', return_value={2, 5}), \
                mock.patch.object(GENERATOR['host'], 'host_lock_dir', return_value=locks), \
                mock.patch.object(GENERATOR['host'], 'resource_snapshot', return_value=snapshot), \
                mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.dict(compare.__globals__, collect_campaigns=collect), \
                contextlib.redirect_stdout(io.StringIO()) as output:
            compare(self.campaigns[:1], baseline, cpu=5)
        self.assertEqual(len(launched), 1)
        self.assertEqual(GENERATOR['json'].loads(output.getvalue())['policies'], 2)
        for _, args, _, _ in self.campaigns[:1]:
            self.assertEqual((args.output_dir / 'libpinyin.order').read_text(), 'p')
            self.assertEqual((args.repeat_output_dir / 'libpinyin.order').read_text(), 'p')

    def test_comparison_rejects_byte_difference_without_publishing(self):
        compare = GENERATOR['compare_base']
        baseline = self.root / 'base.py'
        baseline.touch()
        wanted = {f'lib{facade}.order': b'line\r\n' for facade in GENERATOR['FACADES']}
        def collect(campaigns, **kwargs):
            kwargs['report']['workers'] = 2
            return [[{f'lib{facade}.order': 'line\n' for facade in GENERATOR['FACADES']}]
                    for _ in campaigns]
        with mock.patch.object(GENERATOR['host'], 'allowed_cpus', return_value={2}), \
                mock.patch.object(GENERATOR['pool'], 'run_admitted', return_value=[wanted] * 3), \
                mock.patch.dict(compare.__globals__, collect_campaigns=collect):
            with self.assertRaisesRegex(ValueError, 'serial/parallel policies differ'):
                compare(self.campaigns, baseline)
        self.assertTrue(all(not args.output_dir.exists() for _, args, _, _ in self.campaigns))

    def test_campaign_paths_and_duplicate_outputs_are_checked(self):
        row = {'backend': 'bdb', 'library': {'pinyin': 'p.so', 'zhuyin': 'z.so'},
               'staticlib': {'pinyin': 'p.a', 'zhuyin': 'z.a'}, 'bisect': 'worker',
               'system_dir': 'system', 'system_identity': 'id', 'profiles_dir': 'profiles',
               'work_dir': 'work', 'output_dir': 'out', 'repeat_output_dir': 'again'}
        # Exercise canonical paths even when the host's temp directory is not
        # symlinked (macOS normally reaches /private/var through /var).
        linked = self.root / 'linked'
        linked.symlink_to(self.root, target_is_directory=True)
        path = linked / 'campaign.json'
        path.write_text(GENERATOR['json'].dumps([row]))
        campaigns = GENERATOR['read_campaign'](path)
        self.assertEqual(campaigns[0][1].bisect, (self.root / 'worker').resolve())
        path.write_text(GENERATOR['json'].dumps([row, dict(row, backend='kc')]))
        with self.assertRaisesRegex(ValueError, 'output directories must be distinct'):
            GENERATOR['read_campaign'](path)
        path.write_text(GENERATOR['json'].dumps([dict(row, unknown='bad')]))
        with self.assertRaisesRegex(ValueError, 'missing or unknown fields'):
            GENERATOR['read_campaign'](path)


class GeneratorTests(unittest.TestCase):
    def parse(self, text):
        return GENERATOR["callgrind_functions"](io.StringIO(text), LIBRARY)

    def test_exclusive_costs_compressed_names_and_exact_object(self):
        text = profile(
            "ob=(1) /synthetic/libpinyin.so\nfn=(1) ancestor\ncfn=(2) callee\n"
            "calls=1 0x10 1\n0x1 1 90\n"
            "fn=(2)\n0x10 1 7\n+1 * 2\n"
            "ob=(2) /synthetic/libpinyin.so.extra\nfn=(3) foreign\n0x20 2 4\n"
            "ob=(1)\nfn=(4) not_executed\n0x30 3 0\n", 13)
        self.assertEqual(self.parse(text), {"callee"})

    def test_missing_or_inconsistent_totals_are_errors(self):
        prefix = "positions: instr\nevents: Ir\nob=/synthetic/libpinyin.so\nfn=f\n0x1 2\n"
        for text in (prefix, prefix + "totals: 3\n", prefix + "calls=1 0x2\ntotals: 2\n"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                self.parse(text)

    def test_ir_event_column_and_omitted_trailing_zero(self):
        text = ("positions: instr\nevents: Dr Ir Dw\nob=/synthetic/libpinyin.so\n"
                "fn=f\n0x1 8 2\nfn=zero_ir\n0x2 3\ntotals: 11 2 0\n")
        self.assertEqual(self.parse(text), {"f"})

    def test_unresolved_compression_and_wrong_object_fail(self):
        for body in ("ob=(9)\n", "ob=/synthetic/other.so\nfn=f\n0x1 1 2\n"):
            with self.subTest(body=body), self.assertRaises(ValueError):
                self.parse(profile(body, 2))

    def test_policy_order_is_lexical_with_init_precedence(self):
        render = GENERATOR["policy"]
        first = render("pinyin", {"z", "a"}, {"b", "z"})
        second = render("pinyin", set(reversed(["a", "z"])), {"z", "b"})
        self.assertEqual(first, HEADER + "init\ta\ninit\tz\nsteady\tb\n")
        self.assertEqual(first, second)

    def test_paths_preserve_generics_and_closures(self):
        name = "<crate::Store<T>>::open::{closure#1}"
        self.assertEqual(GENERATOR["canonical"](name + "::h0123456789abcdef.llvm.abcdef"), name)
        self.assertNotEqual(name, "<crate::Store<U>>::open::{closure#1}")

    def test_instantiating_crate_disambiguates_without_hash_identity(self):
        name = "crate::open::<T>"
        identity = GENERATOR["stable_identity"]
        first = identity(name, "_RIfakeECsOLD_16oxpinyin_runtime")
        self.assertEqual(first, identity(name, "_RIfakeECsNEW_16oxpinyin_runtime"))
        self.assertEqual(first, name + " [instantiating crate: oxpinyin_runtime]")
        self.assertNotEqual(first, identity(name, "_RIfakeECsNEW_15oxpinyin_facade"))

    def test_both_facades_and_profiles_are_mandatory(self):
        with self.assertRaises(ValueError):
            GENERATOR["pairs"](["pinyin=one"], ("pinyin", "zhuyin"), "--library")
        with self.assertRaises(ValueError):
            GENERATOR["pairs"](["pinyin:phrases=one"],
                               tuple(f"{f}:{p}" for f in ("pinyin", "zhuyin")
                                     for p in ("phrases", "journals")), "--init")

    def test_exact_unnamed_crt_start_resolves_without_address_guessing(self):
        reader = GENERATOR["library_symbols"]
        data = row("deregister_tm_clones", ".text", "00000000000647e0")
        with mock.patch.dict(reader.__globals__, run=lambda _: data):
            table = reader(LIBRARY)
        self.assertEqual(table["0x00000000000647e0"], ("deregister_tm_clones", ".text"))
        self.assertNotIn("0x00000000000647e1", table)

    def test_unknown_or_ambiguous_measured_paths_fail(self):
        measured = GENERATOR["measured_paths"]
        table = {"_Rraw": ("crate::open", ".text")}
        with mock.patch.dict(measured.__globals__, callgrind_functions=lambda *_: {"_Rraw"}):
            for available in ({}, {"crate::open": {".text.one", ".text.two"}}):
                with self.subTest(available=available), self.assertRaises(ValueError):
                    measured(Path("-"), LIBRARY, table, available)

    def test_rust_and_cxx_mangling_are_told_apart(self):
        legacy = "_ZN4core3ptr13drop_in_place17h0123456789abcdefE"
        for name in (legacy, legacy + ".llvm.42", "_RNvCs1234_5crate4open"):
            with self.subTest(name=name):
                self.assertTrue(GENERATOR["rust_mangled"](name))
                self.assertFalse(GENERATOR["cxx_mangled"](name))
        for name in ("_ZN12kyotocabinet6HashDB3getEv", "_ZNK12kyotocabinet6HashDB5countEv",
                     "_ZN12_GLOBAL__N_1L4failEP7oxkc_db.isra.0"):
            with self.subTest(name=name):
                self.assertTrue(GENERATOR["cxx_mangled"](name))
        self.assertFalse(GENERATOR["cxx_mangled"]("oxkc_db_new"))

    def test_cxx_functions_are_orderable_and_unlisted_ones_stay_fixed(self):
        measured = GENERATOR["measured_paths"]
        cxx, unlisted = "_ZN12kyotocabinet6HashDB3getEv", "_ZN12_GLOBAL__N_1L4failEv.isra.0"
        table = {"_Rraw": ("crate::open", ".text"), cxx: (cxx, ".text"), unlisted: (unlisted, ".text")}
        available = {"crate::open": {".text.one"}, cxx: {".text." + cxx}}
        with mock.patch.dict(measured.__globals__, callgrind_functions=lambda *_: set(table)):
            self.assertEqual(measured(Path("-"), LIBRARY, table, available), {"crate::open", cxx})

    def init_array_fixture(self, values, relocations=(), symbols=()):
        size = 8 * len(values)
        section = ("  [20] .init_array       INIT_ARRAY      "
                   f"0000000000002000 002000 {size:06x} 08 WAo  0   0  8\n")
        words = [f"{value.to_bytes(8, 'little').hex()[:8]} "
                 f"{value.to_bytes(8, 'little').hex()[8:]}" for value in values]
        dump = ("\nHex dump of section '.init_array':\n"
                "  0x0000000000002000 " + " ".join(words) + " ................\n\n")
        relocs = ("Relocation section '.rela.dyn' at offset 0x1 contains entries:\n"
                  "    Offset             Info             Type               "
                  "Symbol's Value  Symbol's Name + Addend\n"
                  + "".join(f"{offset:016x}  0000000000000008 R_X86_64_RELATIVE"
                            f"                         {addend:x}\n"
                            for offset, addend in relocations))
        nm = "".join(f"{address:016x} T {name}\n" for address, name in symbols)

        def dispatch(command):
            if command[:2] == ["readelf", "-SW"]:
                return section
            if command[:2] == ["readelf", "-x"]:
                return dump
            if command[:2] == ["readelf", "-rW"]:
                return relocs
            return nm

        return dispatch

    def test_init_array_symbols_use_exact_constructor_targets(self):
        reader = GENERATOR["init_array_symbols"]
        run = self.init_array_fixture([0x1000, 0x2000], symbols=[(0x1000, "first_ctor"), (0x2000, "second_ctor")])
        with mock.patch.dict(reader.__globals__, run=run):
            self.assertEqual(reader(LIBRARY), ["first_ctor", "second_ctor"])

    def test_init_array_nonzero_unresolved_slot_raises(self):
        reader = GENERATOR["init_array_symbols"]
        run = self.init_array_fixture([0x4000], symbols=[(0x1000, "first_ctor")])
        with mock.patch.dict(reader.__globals__, run=run), self.assertRaisesRegex(ValueError, "0x4000"):
            reader(LIBRARY)

    def test_init_array_zero_slot_without_relocation_is_exempt(self):
        reader = GENERATOR["init_array_symbols"]
        run = self.init_array_fixture([0x0, 0x1000], symbols=[(0x1000, "first_ctor")])
        with mock.patch.dict(reader.__globals__, run=run):
            self.assertEqual(reader(LIBRARY), ["first_ctor"])

    def test_init_array_relocated_zero_slot_resolves_from_addend(self):
        reader = GENERATOR["init_array_symbols"]
        run = self.init_array_fixture([0x0], relocations=[(0x2000, 0x2000)],
                                      symbols=[(0x2000, "relocated_ctor")])
        with mock.patch.dict(reader.__globals__, run=run):
            self.assertEqual(reader(LIBRARY), ["relocated_ctor"])

    def test_init_array_relocated_target_missing_raises(self):
        reader = GENERATOR["init_array_symbols"]
        run = self.init_array_fixture([0x0], relocations=[(0x2000, 0x3000)],
                                      symbols=[(0x1000, "first_ctor")])
        with mock.patch.dict(reader.__globals__, run=run), self.assertRaisesRegex(ValueError, "0x3000"):
            reader(LIBRARY)

    def test_capture_uses_shared_launcher_and_preserves_deadline(self):
        from tools.dev import pool
        context = pool.UnitContext('collection', threading.Event(), cpu=2, fds=(42,))
        def launch(context, command, *, env):
            self.assertEqual(context.fds, (42,))
            context.check()
            return pool.Completed(0, b'out', b'err')
        capture = GENERATOR['capture']
        with mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.object(GENERATOR['time'], 'monotonic', side_effect=[0, 1]):
            self.assertEqual(capture(['worker'], {'KEY': 'value'}, context), (b'out', b'err'))
        with mock.patch.object(pool.UnitContext, 'launch', launch), \
                mock.patch.object(GENERATOR['time'], 'monotonic', side_effect=[0, 1801]):
            with self.assertRaisesRegex(ValueError, 'measurement child timed out'):
                capture(['worker'], {}, context)

    def test_teardown_exports_keep_only_the_selected_facade(self):
        reader = GENERATOR["teardown_exports"]
        with mock.patch.dict(reader.__globals__, run=lambda _: (
                "00000001 T pinyin_init@@LIBPINYIN\n"
                "00000002 T zhuyin_init@@LIBZHUYIN\n"
                "00000003 T g_free\n")):
            self.assertEqual(reader(LIBRARY, "pinyin"), "pinyin_init")
        with self.assertRaises(ValueError):
            with mock.patch.dict(reader.__globals__, run=lambda _: "00000001 T g_free\n"):
                reader(LIBRARY, "pinyin")

    def test_first_candidate_toggles_list_every_facade_export(self):
        toggles = GENERATOR["first_candidate_toggles"]
        names = GENERATOR["FIRST_CANDIDATE"]["zhuyin"]
        exports = ",".join(["zhuyin_init", "pinyin_guess_candidates",
                            *(f"zhuyin_{name}" for name in names)])
        self.assertEqual(toggles("zhuyin", exports), [f"zhuyin_{name}" for name in names])
        with self.assertRaisesRegex(ValueError, "lacks the first-candidate exports "
                                                "zhuyin_guess_candidates_after_cursor$"):
            toggles("zhuyin", exports.replace(",zhuyin_guess_candidates_after_cursor", ""))

    def test_generator_and_measurement_use_the_same_startup_toggles(self):
        exports = ",".join(f"pinyin_{name}" for name in ("init", *GENERATOR["FIRST_CANDIDATE"]["pinyin"]))
        constructors = ["ctor_a", "ctor_b"]
        collections = GENERATOR['startup_collections']
        with mock.patch.dict(collections.__globals__, init_array_symbols=lambda library: constructors), \
                mock.patch.dict(MEASUREMENT["GEN"], init_array_symbols=lambda library: constructors):
            collected = [toggle for profile, toggle in collections("pinyin", LIBRARY, exports)]
            measured = MEASUREMENT["startup_toggles"](LIBRARY, "pinyin", exports)
        self.assertEqual(list(dict.fromkeys(collected)), list(measured))
        self.assertIn("pinyin_get_candidate_nbest_index", measured)

    def test_first_candidate_matches_the_worker_path_to_m1(self):
        """set_options, alloc_instance, td_first and td_read_candidate in teardown.h."""
        source = (HERE / "bisection/teardown.h").read_text()

        def body(signature):
            start = source.index("{", source.index(signature))
            depth = 0
            for end in range(start, len(source)):
                depth += {"{": 1, "}": -1}.get(source[end], 0)
                if not depth:
                    return source[start:end]
            raise AssertionError(signature)

        resolve = body("static void td_resolve(")
        common, branches = resolve.split("if (api->zhuyin) {")
        zhuyin, pinyin = branches.split("} else {")
        pattern = re.compile(r'TD_RESOLVE\((\w+), "(\w+)"\)|api->(\w+)\) = td_symbol'
                             r'\(library, facade, "(\w+)"\)')

        def resolved(text):
            return {(field or other): (suffix or suffix2)
                    for field, suffix, other, suffix2 in pattern.findall(text)}

        scope = {"pinyin": resolved(common) | resolved(pinyin),
                 "zhuyin": resolved(common) | resolved(zhuyin)}
        calls = set(re.findall(r"api->(?:common\.)?(\w+)\(",
            body("bool bisect_teardown_first_candidate_unit(") + body("static bool td_read_candidate(")))
        run = body("static int run_teardown_mode(")
        path = run[run.index("api.common.init("):run.index('td_checkpoint("M1"')]
        for field in ("set_options", "alloc_instance"):
            self.assertIn(f"api.common.{field}(", path)
        for facade, fields in scope.items():
            expected = {"set_options", "alloc_instance"} | {
                fields[field] for field in calls if field in fields}
            self.assertEqual(set(GENERATOR["FIRST_CANDIDATE"][facade]), expected, facade)
            self.assertEqual(len(GENERATOR["FIRST_CANDIDATE"][facade]), len(expected), facade)

    def test_teardown_command_has_the_worker_boundary_arguments(self):
        args = SimpleNamespace(bisect=Path("/bin/bisect"), system_dir=Path("/data"))
        command = GENERATOR["teardown_command"](args, "pinyin", Path("/lib.so"), Path("/user"))
        self.assertEqual(command, [str(args.bisect), "--teardown", "pinyin", "/lib.so",
                                   "/data", "/user", "time", "0", "1", "1",
                                   "first-candidate", "-"])


class ProfileManifestTests(unittest.TestCase):
    """The generator reads the manifests the teardown provisioner writes (section 7.4)."""

    class Reached(Exception):
        pass

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.system = self.root / "data"
        self.system.mkdir()
        (self.system / "table.conf").write_bytes(b"conf\n")
        self.profiles = self.root / "profiles"
        for name in TEARDOWN["PROFILES"]:
            user = self.profiles / name / "user"
            (user / "nested" / "deeper").mkdir(parents=True)
            (user / "a.txt").write_bytes(name.encode())
            (user / "nested" / "b.bin").write_bytes(b"\x00\x01")
            (user / "nested" / "deeper" / "c").write_bytes(b"")
            (user / "nested.txt").write_bytes(b"sibling")
            self.write_teardown_manifest(user)

    @staticmethod
    def write_teardown_manifest(user):
        # teardown.py's provision() format, spelled out independently of the
        # shared implementation: size<TAB>sha256<TAB>relative path.
        rows = ["oxpinyin-startup-profile-v1"]
        for path in sorted(user.rglob("*")):
            if path.is_file():
                data = path.read_bytes()
                rows.append(f"{len(data)}\t{hashlib.sha256(data).hexdigest()}\t{path.relative_to(user)}")
        (user.parent / "profile.manifest").write_text("\n".join(rows) + "\n")

    def collect(self):
        collect = GENERATOR["collect_policies"]
        args = SimpleNamespace(
            bisect=Path("/bin/bisect"), system_dir=self.system, profiles_dir=self.profiles,
            work_dir=self.root, cpu=0, init=[], steady=[],
            system_identity=GENERATOR["STARTUP_PROFILE"]["identity"](self.system))

        def reached(_root):
            raise self.Reached

        with mock.patch.dict(collect.__globals__, collection_scratch=reached):
            collect(args, {}, {})

    def rejected(self, message):
        with self.assertRaisesRegex(ValueError, message):
            self.collect()

    def test_teardown_format_with_nested_directories_is_accepted(self):
        user = self.profiles / "phrases" / "user"
        self.assertEqual((user.parent / "profile.manifest").read_text(), GENERATOR["manifest"](user))
        with self.assertRaises(self.Reached):
            self.collect()
        # The provisioner's own existing-tree check accepts the same files.
        self.assertEqual(set(TEARDOWN["provision"](None, None, self.profiles)), set(TEARDOWN["PROFILES"]))

    def test_measurement_and_provisioner_write_the_same_manifest(self):
        user = self.profiles / "journals" / "user"
        recorded = (user.parent / "profile.manifest").read_text()
        self.assertEqual(MEASUREMENT["GEN"]["manifest"](user), recorded)
        self.assertEqual(TEARDOWN["STARTUP_PROFILE"].manifest(user), recorded)

    def test_changed_file_is_rejected(self):
        (self.profiles / "phrases" / "user" / "nested" / "b.bin").write_bytes(b"\x00\x02")
        self.rejected(r"differs from .*phrases/user: changed nested/b\.bin$")

    def test_added_file_is_rejected(self):
        (self.profiles / "journals" / "user" / "nested" / "deeper" / "new").write_bytes(b"x")
        self.rejected(r"differs from .*journals/user: added nested/deeper/new$")

    def test_removed_file_is_rejected(self):
        (self.profiles / "phrases" / "user" / "a.txt").unlink()
        self.rejected(r"differs from .*phrases/user: removed a\.txt$")

    def test_symlink_is_rejected(self):
        (self.profiles / "phrases" / "user" / "link").symlink_to("a.txt")
        self.rejected(r"manifest input contains a symlink: .*phrases/user/link$")

    def test_tab_or_newline_in_a_path_is_rejected(self):
        for name in ("new\nline", "tab\tname"):
            with self.subTest(name=name):
                user = self.root / "control" / name.encode().hex()
                user.mkdir(parents=True)
                try:
                    (user / name).write_bytes(b"x")
                except OSError as error:
                    self.skipTest(f"file system refuses {name!r}: {error}")
                with self.assertRaisesRegex(ValueError, "path contains a tab or newline: " + re.escape(repr(name))):
                    GENERATOR["manifest"](user)

    def test_old_four_column_manifest_is_rejected(self):
        manifest = self.profiles / "phrases" / "profile.manifest"
        lines = manifest.read_text().split("\n")
        manifest.write_text("\n".join([lines[0], *("file\t" + line for line in lines[1:-1]), ""]))
        self.rejected(r"line 2 is not size<TAB>sha256<TAB>path: .*phrases/profile\.manifest$")

    def test_identity_of_a_fixed_directory(self):
        # SHA-256 of the header plus a.txt, nested/b.bin, nested/deeper/c and
        # nested.txt rows, in that order, computed outside Python.
        self.assertEqual(GENERATOR["STARTUP_PROFILE"]["identity"](self.profiles / "empty" / "user"),
                         "dc34a1df1c7365ad0dbdc19b51a9a7c20d64a6f0954877e90c16feac9ddef08f")


if __name__ == "__main__":
    unittest.main()
