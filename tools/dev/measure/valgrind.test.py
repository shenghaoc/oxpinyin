#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the valgrind recipes: the callgrind differential's eight stages and system-import-profile.

The differential runs in the fake world (fake_world.py): its stages, banners, files and stopping
rules are pinned without valgrind, binutils or a PMU, so the file runs on macOS and Linux. The
neutrality comparison it calls is replaced by a fake module (it has its own tests).
"""
import contextlib
import io
import os
from pathlib import Path
import shutil
import signal
import sys
import threading
import time
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import host, pool, slot
from tools.dev.measure import bench, identity, valgrind
from tools.dev import runners
from tools.dev.measure.fake_world import Stream, World

NM = ('0000000000001130 t bisect_perf_cold_unit\n0000000000001140 t bisect_perf_steady_unit\n'
      '0000000000001150 T main\n')


class ControlTests(World):
    def setUp(self):
        super().setUp()
        self.profile, self.shipping = self.root / 'profile', self.root / 'shipping'
        for stage in (self.profile, self.shipping):
            stage.mkdir()
            (stage / 'libpinyin.so').write_bytes(b'library')

    def control(self):
        return self.execute(valgrind.callgrind_control, [str(self.profile), str(self.shipping)], {})

    def test_dwarf_is_checked_and_the_neutrality_failure_propagates(self):
        with mock.patch.object(valgrind.bench, 'capture', return_value=pool.Completed(
                0, b'  Compilation Unit @ offset 0:\n', b'')) as readelf, mock.patch(
                'tools.dev.measure.neutrality.compare', return_value=(1, 'FAIL (c)\n')) as compare:
            status, out, err = self.control()
        self.assertEqual((status, out, err), (1, 'profiled DWARF compilation units: 1\nFAIL (c)\n', ''))
        readelf.assert_called_once_with(['readelf', '--debug-dump=info', self.profile / 'libpinyin.so'], env={})
        compare.assert_called_once_with(self.profile / 'libpinyin.so', self.shipping / 'libpinyin.so')
        for stage in (self.profile, self.shipping):
            self.assertEqual((stage / 'libpinyin.so').read_bytes(), b'library')

    def test_missing_dwarf_and_failed_readelf_stop_before_comparison(self):
        for code, error in ((0, 'callgrind-control: profiled library has no DWARF compilation units\n'),
                            (7, 'readelf failed\n')):
            with self.subTest(code=code), mock.patch.object(valgrind.bench, 'capture', return_value=pool.Completed(
                    code, b'', b'readelf failed\n' if code else b'')), mock.patch(
                    'tools.dev.measure.neutrality.compare') as compare:
                self.assertEqual(self.control(), (code or 1, '', error))
                compare.assert_not_called()

    def test_bad_arguments_and_either_missing_library_fail(self):
        self.assertEqual(self.execute(valgrind.callgrind_control, [], {})[0], 2)
        for index in range(2):
            stages = [str(self.profile), str(self.shipping)]
            stages[index] = str(self.root / 'missing')
            self.assertEqual(self.execute(valgrind.callgrind_control, stages, {})[0], 1)


class TableTests(World):
    def nm(self, text):
        return lambda command: pool.Completed(0, text.encode(), b'')

    def test_distinct_anchors_pass_and_name_the_toggle(self):
        code, text = valgrind.anchors_table(Path('bisect'), run=self.nm(NM))
        self.assertEqual(code, 0)
        self.assertEqual(text, '=== collection anchors ===\n0000000000001130 t bisect_perf_cold_unit\n'
                               '0000000000001140 t bisect_perf_steady_unit\n'
                               'PASS: distinct addresses, so --toggle-collect=bisect_perf_steady_unit\n'
                               '      excludes process init and the cold cycle\n')

    def test_folded_anchors_fail_the_run(self):
        folded = NM.replace('1140 t', '1130 t')
        code, text = valgrind.anchors_table(Path('bisect'), run=self.nm(folded))
        self.assertEqual(code, 1)
        self.assertTrue(text.endswith('FAIL: anchors folded to one address; the toggle would collect the cold cycle\n'))

    def test_no_anchor_symbol_at_all_ends_the_group_after_its_header(self):
        self.assertEqual(valgrind.anchors_table(Path('bisect'), run=self.nm('0 T main\n')),
                         (1, '=== collection anchors ===\n'))

    def test_nms_own_complaint_reaches_stderr(self):
        err = Stream()
        with contextlib.redirect_stderr(err):
            result = valgrind.anchors_table(
                Path('bisect'), run=lambda command: pool.Completed(1, b'', b"nm: 'bisect': No such file\n"))
        self.assertEqual((result, err.text()), ((1, '=== collection anchors ===\n'), "nm: 'bisect': No such file\n"))

    def test_one_missing_anchor_passes_as_the_shell_did(self):
        code, text = valgrind.anchors_table(Path('bisect'), run=self.nm('0000000000001140 t bisect_perf_steady_unit\n'))
        self.assertEqual(code, 0)

    def test_lines_of_selects_like_grep_and_complains_about_a_missing_file(self):
        path = self.root / 'f'
        path.write_text('ob=(1) /usr/lib/vgpreload_memcheck.so\nfn=main\nfn=MALLOC\nREDIR: x\n')
        self.assertEqual(valgrind.lines_of(path, '^(ob|fl|fn)='), ['ob=(1) /usr/lib/vgpreload_memcheck.so', 'fn=main', 'fn=MALLOC'])
        self.assertEqual(valgrind.lines_of(path, 'malloc', ignore_case=True), ['fn=MALLOC'])
        err = Stream()
        with contextlib.redirect_stderr(err):
            self.assertEqual(valgrind.lines_of(self.root / 'missing', 'x'), [])
        self.assertEqual(err.text(), f'grep: {self.root}/missing: No such file or directory\n')

    def test_the_environment_table_names_the_host_the_tools_and_each_artifact(self):
        artifact = self.root / 'lib.so'
        artifact.write_bytes(b'abc')
        with mock.patch.object(identity, 'uname', lambda flags='srm': {'m': 'aarch64', 'srvm': 'Linux 6 #1 aarch64'}[flags]):
            text = valgrind.environment_table([artifact], run=lambda command: pool.Completed(0, b'tool 1\n', b''))
        lines = text.splitlines()
        self.assertEqual(lines[:4], ['=== environment ===', 'captured_utc: 2026-10-27T00:00:00Z', 'container_arch: aarch64',
                                     'kernel: Linux 6 #1 aarch64'])
        self.assertIn('valgrind: tool 1.0', lines)
        self.assertIn('perf_event_paranoid: 2', lines)
        self.assertIn('libpinyin_pkgversion: Version: 2.11.92', lines)
        self.assertIn(f'artifact: ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  {artifact} 3 bytes', lines)
        self.assertEqual(lines[-1], f'data_dir: {valgrind.DATA}')

    def test_a_symlinked_artifact_reports_the_links_own_size_as_stat_does(self):
        target = self.root / 'libpinyin.so.15.0.0'
        target.write_bytes(b'x' * 1000)
        link = self.root / 'libpinyin.so'
        link.symlink_to(target.name)
        text = valgrind.environment_table([link], run=lambda command: pool.Completed(0, b'', b''))
        hashed = identity.sha256(target)
        self.assertIn(f'artifact: {hashed}  {link} {len(target.name)} bytes\n', text)


class LibrariesTests(World):
    def roots(self, present):
        roots = {}
        for key in ('lp', 'ox', 'nodebug', 'alloc'):
            root = self.root / key
            if present:
                (root / 'lib').mkdir(parents=True)
                if present is True:
                    (root / 'lib/libpinyin.so').write_bytes(b'')
            roots[key] = root
        self.enterContext(mock.patch.object(valgrind, 'ROOTS', roots))
        self.enterContext(mock.patch.object(valgrind, 'DATA', self.root / 'data'))
        (self.root / 'data').mkdir()
        return roots

    def libraries(self):
        err = Stream()
        with contextlib.redirect_stderr(err):
            try:
                return valgrind.libraries(), None, err.text()
            except runners.Finished as stop:
                return None, stop.status, err.text()

    def test_all_four_libraries_are_found_below_their_roots(self):
        roots = self.roots(True)
        found, status, _ = self.libraries()
        self.assertEqual((status, {key: path for key, path in found.items()}),
                         (None, {key: root / 'lib/libpinyin.so' for key, root in roots.items()}))

    def test_a_root_that_is_not_there_is_findings_own_complaint(self):
        roots = self.roots(None)
        found, status, err = self.libraries()
        self.assertEqual((found, status, err), (None, 1, f"find: '{roots['lp']}': No such file or directory\n"))

    def test_a_root_without_the_library_is_a_missing_artifact_with_nothing_to_name(self):
        self.roots('empty')
        _, status, err = self.libraries()
        self.assertEqual((status, err), (1, "fatal: missing artifact: ''\n"))

    def test_a_missing_data_directory_is_named_last(self):
        self.roots(True)
        (self.root / 'data').rmdir()
        _, status, err = self.libraries()
        self.assertEqual((status, err), (1, f'fatal: data dir {self.root / "data"} missing\n'))


class AnnotateTests(World):
    """annotate takes heavy-slot and memory admission: the real one, on a fake host."""

    def setUp(self):
        super().setUp()
        locks = self.root / 'locks'
        locks.mkdir()
        environment = {name: value for name, value in os.environ.items()
                       if name not in ('CARGO_BUILD_JOBS', 'RUST_TEST_THREADS', slot.HEAVY_SLOT_HELD)}
        environment.update(OXPINYIN_HEAVY_SLOTS='1', OXPINYIN_RESOURCE_WAIT_SECONDS='0')
        self.enterContext(mock.patch.dict(os.environ, environment, clear=True))
        self.enterContext(mock.patch.object(host, 'host_lock_dir', return_value=locks))
        self.enterContext(mock.patch.object(slot.CANCELLED, 'is_set', lambda: False))
        self.enterContext(contextlib.redirect_stderr(io.StringIO()))
        self.fake_host(available=64000, cpus=4)

    def fake_host(self, available, cpus, total=16384):
        """A host with AVAILABLE MiB free of TOTAL and CPUS CPUs, all of them allowed."""
        snapshot = dict(available_mib=available, total_mib=total, memory_limit_mib=None, cpus=cpus,
                        cargo_cpus=cpus, notes=[f'synthetic {available} MiB, {cpus} CPUs'])
        self.enterContext(mock.patch.object(host, 'allowed_cpus', lambda: set(range(cpus))))
        self.enterContext(mock.patch.object(host, 'resource_snapshot', lambda *args, **kwargs: dict(snapshot)))

    def annotate_four_profiles(self, expected=0):
        """Annotate four profiles, eight annotations; return the most that ran at once.

        With EXPECTED, each annotation waits until that many run together, so a narrower pool cannot pass.
        """
        for index in range(4):
            (self.root / f'callgrind.p{index}.r1.out').write_bytes(b'')
        lock, together, running, peak = threading.Lock(), threading.Event(), [0], [0]
        self.ran = []

        def capture(command, **kwargs):
            with lock:
                self.ran.append(command)
                running[0] += 1
                peak[0] = max(peak[0], running[0])
                if running[0] == expected:
                    together.set()
            if expected:
                self.assertTrue(together.wait(5), 'the annotations did not overlap')
            time.sleep(0.02)
            with lock:
                running[0] -= 1
            return pool.Completed(0, b'annotated\n', b'')
        with mock.patch.object(bench, 'capture', capture):
            valgrind.annotate(self.root)
        return peak[0]

    def test_a_host_short_of_memory_annotates_one_profile_at_a_time(self):
        # 3,000 MiB less 1,638 MiB of headroom holds one 1,024 MiB annotation, not two; four CPUs would run four.
        self.fake_host(available=3000, cpus=4)
        self.assertEqual(self.annotate_four_profiles(), 1)
        self.assertEqual(len(self.ran), 8)

    def test_a_roomy_host_annotates_up_to_its_cpu_count_at_once(self):
        self.fake_host(available=64000, cpus=4)
        self.assertEqual(self.annotate_four_profiles(expected=4), 4)
        self.assertEqual(len(self.ran), 8)

    def test_a_host_that_cannot_hold_one_annotation_refuses_the_pool_and_starts_none(self):
        self.fake_host(available=2000, cpus=4)    # 362 MiB after the headroom
        with self.assertRaises(host.ResourceBlocked):
            self.annotate_four_profiles()
        self.assertEqual(self.ran, [])

    def test_each_profile_is_annotated_twice_and_the_redirection_probe_is_skipped(self):
        for name in ('callgrind.libpinyin-tkrzw.r1.out', 'callgrind.redir-probe.out', 'callgrind.oxpinyin-tkrzw.r1.out', 'other.out'):
            (self.root / name).write_bytes(b'')
        calls = []

        def capture(command, merge_stderr=False, **kw):
            calls.append((command, merge_stderr))
            return pool.Completed(0, b'annotated\n', b'')
        with mock.patch.object(bench, 'capture', capture):
            valgrind.annotate(self.root)
        profile = self.root / 'callgrind.libpinyin-tkrzw.r1.out'
        # Independent post-processing on a pool: no order between the units.
        self.assertIn(['callgrind_annotate', '--threshold=99.5', str(profile)], [command for command, _ in calls])
        self.assertIn(['callgrind_annotate', '--threshold=99.5', '--inclusive=yes', str(profile)],
                      [command for command, _ in calls])
        self.assertEqual(len(calls), 4)
        self.assertEqual({merged for _, merged in calls}, {True})
        self.assertEqual((self.root / 'callgrind.libpinyin-tkrzw.r1.out.annotated.txt').read_bytes(), b'annotated\n')
        self.assertTrue((self.root / 'callgrind.oxpinyin-tkrzw.r1.out.inclusive.txt').exists())
        self.assertFalse(list(self.root.glob('*redir-probe*.txt')))

    def test_a_missing_annotate_tool_is_noted_in_the_file_and_is_not_a_failure(self):
        (self.root / 'callgrind.a.r1.out').write_bytes(b'')

        def missing(command, **kw):
            raise FileNotFoundError(2, 'No such file or directory', 'callgrind_annotate')
        with mock.patch.object(bench, 'capture', missing):
            valgrind.annotate(self.root)
        self.assertEqual((self.root / 'callgrind.a.r1.out.annotated.txt').read_text(), 'callgrind_annotate: No such file or directory\n')


class InterceptionTests(World):
    def setUp(self):
        super().setUp()
        self.arm = bench.Arm('oxpinyin-tkrzw', Path('/ox/libpinyin.so'), Path('/data'), Path('/ox'))
        self.worker = bench.Bench('run-callgrind-differential.sh', {}, Path('/h/bisect'), '0', cell='tkrzw')

    def probe(self, stderr=b'', profile='', status=0, refuse=False):
        def launch(arm, mode, cycles, **options):
            self.launched = (arm.label, mode, cycles, options)
            if refuse:
                bench.fail('REFUSED: x: subject: libtkrzw resolved to /usr/lib')
            (self.root / 'callgrind.redir-probe.out').write_text(profile)
            return pool.Completed(status, b'row\n', stderr)
        with mock.patch.object(self.worker, 'launch', launch, create=True):
            return valgrind.malloc_interception(self.worker, self.arm, self.root)

    def test_the_report_lists_the_redirections_and_the_credited_objects_sorted_and_unique(self):
        credited = ''.join(f'ob=(2) /usr/lib/vgpreload_{name}.so\n' for name in ('memcheck', 'callgrind', 'callgrind')) + 'fn=ordinary\n'
        text = self.probe(b'REDIR: a\nnothing here\nmalloc replaced\n' + b'free x\n' * 60, credited)
        lines = text.splitlines()
        self.assertEqual(lines[0], '=== valgrind malloc interception ===')
        self.assertEqual(lines[1], '--- redirections naming malloc/free (from --trace-redir) ---')
        self.assertEqual(lines[2:4], ['REDIR: a', 'malloc replaced'])
        self.assertEqual(len([line for line in lines if line.startswith(('REDIR', 'malloc', 'free'))]), 40)
        marker = lines.index('--- objects credited in the profile ---')
        self.assertEqual(lines[marker + 1:marker + 3], ['ob=(2) /usr/lib/vgpreload_callgrind.so', 'ob=(2) /usr/lib/vgpreload_memcheck.so'])
        self.assertEqual(lines[marker + 3], '')
        self.assertTrue(lines[-1].endswith('Call counts are unaffected.'))
        self.assertEqual(self.launched[:3], ('probe', 'speed', '2'))
        self.assertEqual(self.launched[3]['repeats'], 1)
        self.assertEqual(self.launched[3]['prefix'][:5], ['valgrind', '--tool=callgrind', '--collect-atstart=no',
                                                         '--toggle-collect=bisect_perf_steady_unit', '--trace-redir=yes'])
        self.assertEqual((self.root / 'redir-probe.stdout').read_bytes(), b'row\n')

    def test_a_failing_probe_does_not_stop_the_capture(self):
        text = self.probe(b'valgrind: error\n', '', status=1)
        self.assertIn('--- objects credited in the profile ---', text)

    def test_a_host_without_taskset_leaves_the_shells_complaint_in_the_probes_stderr_file(self):
        def launch(arm, mode, cycles, **options):
            raise FileNotFoundError(2, 'No such file or directory', 'taskset')
        with mock.patch.object(self.worker, 'launch', launch, create=True):
            err = Stream()
            with contextlib.redirect_stderr(err):
                text = valgrind.malloc_interception(self.worker, self.arm, self.root)
        self.assertEqual((self.root / 'redir-probe.stderr').read_text(), 'taskset: command not found\n')
        self.assertEqual((self.root / 'redir-probe.stdout').read_bytes(), b'')
        self.assertTrue(text.startswith('=== valgrind malloc interception ===\n'))
        self.assertTrue(text.rstrip().endswith('Call counts are unaffected.'))
        self.assertIn(f'grep: {self.root}/callgrind.redir-probe.out: No such file or directory', err.text())

    def test_a_refusal_inside_the_probe_goes_to_its_stderr_file(self):
        text = self.probe(refuse=True)
        self.assertEqual((self.root / 'redir-probe.stdout').read_bytes(), b'')
        self.assertEqual((self.root / 'redir-probe.stderr').read_text(), 'REFUSED: x: subject: libtkrzw resolved to /usr/lib\n')
        self.assertTrue(text.startswith('=== valgrind malloc interception ===\n'))


class DifferentialTests(World):
    def setUp(self):
        super().setUp()
        self.out = self.root / 'out'
        self.libs = {}
        for key, base in (('lp', 'pin'), ('ox', 'ox'), ('nodebug', 'nodebug'), ('alloc', 'alloc')):
            library = self.root / base / 'lib/libpinyin.so'
            library.parent.mkdir(parents=True)
            library.write_bytes(base.encode())
            self.libs[key] = library
        (self.root / 'data').mkdir()
        self.enterContext(mock.patch.object(valgrind, 'libraries', lambda: dict(self.libs)))
        self.enterContext(mock.patch.object(valgrind, 'DATA', self.root / 'data'))
        self.nm_output = NM
        self.neutral = (0, 'PASS (a): identical .text\n')
        fake = types.SimpleNamespace(compare=lambda first, second: self.neutral)
        self.enterContext(mock.patch.dict(sys.modules, {'tools.dev.measure.neutrality': fake}))
        self.enterContext(mock.patch('tools.dev.measure.neutrality', fake, create=True))
        original = self.fake_capture

        def capture(command, *, env=None, cwd=None, merge_stderr=False):
            if list(command)[:1] == ['nm']:
                return pool.Completed(0, self.nm_output.encode(), b'')
            if '--perf' in [str(part) for part in command] and env.get('PERF_HW') == 'instructions':
                self.log.append(('hardware',))
            return original(command, env=env, cwd=cwd, merge_stderr=merge_stderr)
        self.enterContext(mock.patch.object(bench, 'capture', capture))
        self.enterContext(mock.patch.object(valgrind, 'annotate', lambda out: self.log.append(('annotate', out.name))))
        self.env = {'CG_OUT': str(self.out), 'CG_ROUNDS': '2', 'T_RUNS': '2', 'HW_RUNS': '2', 'PERF_CPU': '1'}
        self.worker_row = {}

    def test_the_eight_stages_run_in_order_and_leave_their_files(self):
        status, out, err = self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual(status, 0, err)
        stages = [line for line in out.splitlines() if line.startswith(('=== ', '--- '))]
        self.assertEqual(stages, [
            '=== environment ===', '=== collection anchors ===', '=== debug-info neutrality ===',
            '=== valgrind malloc interception ===', '--- redirections naming malloc/free (from --trace-redir) ---',
            '--- objects credited in the profile ---',
            '--- callgrind: libpinyin-tkrzw round 1 ---', '--- callgrind: oxpinyin-tkrzw round 1 ---',
            '--- callgrind: libpinyin-tkrzw round 2 ---', '--- callgrind: oxpinyin-tkrzw round 2 ---',
            '=== allocations per steady cycle (oxpinyin, alloc-count build) ===',
            '--- timing: 2 alternating runs x 8 cycles x 1 passes ---', f'=== captures written to {self.out} ==='])
        for name in ('environment.txt', 'anchors.txt', 'debuginfo-neutrality.txt', 'malloc-interception.txt',
                     'cg.libpinyin-tkrzw.r1.stdout', 'cg.oxpinyin-tkrzw.r2.stderr', 'hw-instructions.jsonl', 'hw-instructions.md',
                     'hw.err', 'alloc-count.txt', 'speed.jsonl', 'speed.err', 'timing.md'):
            self.assertTrue((self.out / name).exists(), name)
        self.assertEqual((self.out / 'debuginfo-neutrality.txt').read_text(), '=== debug-info neutrality ===\nPASS (a): identical .text\n')
        self.assertEqual((self.out / 'timing.md').read_text(), 'REPORT perf-ci.py\n')
        self.assertTrue(out.rstrip().endswith('timing.md'))

    def test_workers_are_pinned_to_perf_cpu_and_profiled_with_the_callgrind_options(self):
        self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual({event[1] for event in self.kinds('taskset')}, {'1'})
        profiled = [w for w in self.workers_of() if 'valgrind' in w[6]]
        # the redirection probe, then two rounds of both cells
        self.assertEqual(len(profiled), 5)
        cell = profiled[1][6]
        self.assertEqual(cell[cell.index('valgrind'):cell.index('valgrind') + 7],
                         ['valgrind', '--tool=callgrind', '--collect-atstart=no', '--toggle-collect=bisect_perf_steady_unit',
                          '--cache-sim=yes', '--branch-sim=yes', '--separate-threads=no'])
        self.assertIn(f'--callgrind-out-file={self.out}/callgrind.libpinyin-tkrzw.r1.out', cell)
        self.assertEqual((profiled[1][1], profiled[1][3], profiled[1][4]), ('libpinyin-tkrzw', '4', '1'))
        native = [w for w in self.workers_of() if 'valgrind' not in w[6]]
        # hardware: 2 runs x 2 cells with PERF_HW, the allocation run, then the timing runs
        self.assertEqual([w[1] for w in native], ['libpinyin-tkrzw', 'oxpinyin-tkrzw'] * 2 + ['oxpinyin-alloccount']
                         + ['libpinyin-tkrzw', 'oxpinyin-nodebug'] * 2)
        self.assertEqual({w[3] for w in native}, {'8'})
        self.assertEqual(len(self.kinds('hardware')), 4)
        self.assertEqual([check[2] for check in self.kinds('check')[:4]], ['pin', 'subject', 'subject', 'subject'])

    def test_a_bad_count_is_named_before_anything_is_made(self):
        for name in ('CG_ROUNDS', 'T_RUNS', 'HW_RUNS'):
            with self.subTest(name=name):
                status, out, err = self.execute(valgrind.callgrind_differential, [], {**self.env, name: '1.5'})
                self.assertEqual((status, out, self.out.exists()), (2, '', False))
                self.assertEqual(err, f"run-callgrind-differential.sh: {name} must be a decimal count (got '1.5')\n")

    def test_folded_anchors_stop_after_their_table_is_kept(self):
        self.nm_output = NM.replace('1140 t', '1130 t')
        status, out, _ = self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual(status, 1)
        self.assertIn('FAIL: anchors folded', (self.out / 'anchors.txt').read_text())
        self.assertFalse((self.out / 'debuginfo-neutrality.txt').exists())
        self.assertEqual(self.workers_of(), [])

    def test_different_code_stops_the_run_before_any_profile(self):
        self.neutral = (1, 'FAIL (c): instruction multiset differs\n')
        status, out, _ = self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual(status, 1)
        self.assertIn('FAIL (c)', out)
        self.assertFalse((self.out / 'malloc-interception.txt').exists())
        self.assertEqual(self.workers_of(), [])

    def test_a_failing_callgrind_run_stops_after_keeping_its_streams(self):
        self.worker_status = {2: 5}
        status, _, _ = self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual(status, 5)
        self.assertTrue((self.out / 'cg.libpinyin-tkrzw.r1.stderr').exists())
        self.assertFalse((self.out / 'hw-instructions.jsonl').exists())
        self.assertNotIn(('annotate', 'out'), self.log)

    def test_a_failing_allocation_run_keeps_its_partial_table_without_the_trailer(self):
        self.worker_status = {5 + 4 + 1: 3}
        status, _, _ = self.execute(valgrind.callgrind_differential, [], self.env)
        self.assertEqual(status, 3)
        self.assertNotIn('steady_alloc_count', (self.out / 'alloc-count.txt').read_text())
        self.assertFalse((self.out / 'speed.jsonl').exists())


class SystemImportProfileTests(World):
    def setUp(self):
        super().setUp()
        self.pin, self.ox, self.data = (self.root / name for name in ('pin.so', 'ox.so', 'data'))
        for path in (self.pin, self.ox):
            path.write_bytes(b'')
        self.data.mkdir()
        self.out = self.root / 'out'
        self.calls = []
        profile = types.SimpleNamespace(main=lambda argv: self.calls.append(argv) or 0)
        self.enterContext(mock.patch.object(bench, 'script', lambda relative: profile))

    def run_recipe(self, *arguments):
        return self.execute(valgrind.system_import_profile, [str(self.pin), str(self.ox), str(self.data), str(self.out), *arguments])

    def test_the_default_workload_is_a_thousand_iterations_and_the_driver_is_compiled_first(self):
        status, out, err = self.run_recipe()
        self.assertEqual(status, 0, err)
        self.assertEqual(self.calls, [['--', str(self.pin.resolve()), str(self.ox.resolve()), str(self.data.resolve()),
                                       str(self.out.resolve()), '1000']])
        compile_command = self.kinds('run')[0][1]
        self.assertEqual(compile_command[:6], ['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2'])
        self.assertEqual(compile_command[-3:], ['-ldl', '-o', str(self.out.resolve() / 'driver')])

    def test_the_workload_argument_and_cold_open_come_before_the_options(self):
        for arguments, workload, jobs in ((['30'], '30', []), (['--cold-open'], '--cold-open', []),
                                           (['--jobs', '2'], '1000', ['--jobs', '2']),
                                           (['5', '--jobs', '1'], '5', ['--jobs', '1'])):
            with self.subTest(arguments=arguments):
                self.calls.clear()
                shutil.rmtree(self.out, ignore_errors=True)
                status, _, err = self.run_recipe(*arguments)
                self.assertEqual(status, 0, err)
                self.assertEqual(self.calls[0][:len(jobs)], jobs)
                self.assertEqual(self.calls[0][-1], workload)

    def test_jobs_needs_a_count(self):
        for arguments in (['--jobs'], ['--jobs', '']):
            with self.subTest(arguments=arguments):
                status, out, err = self.run_recipe(*arguments)
                self.assertEqual((status, out, err), (2, '', '--jobs needs a count\n'))
        self.assertFalse(self.out.exists())

    def test_missing_operands_are_named_with_the_runner(self):
        for given, name in ((0, 'pin-so'), (1, 'ox-so'), (2, 'system-data-dir'), (3, 'new-output-dir')):
            status, _, err = self.execute(valgrind.system_import_profile, [str(self.pin)] * given, {})
            self.assertEqual((status, err), (1, f'run-system-import-profile.sh: {given + 1}: {name}\n'))

    def test_the_collections_run_under_their_own_script_name(self):
        seen = []
        profile = types.SimpleNamespace(main=lambda argv: seen.append(Path(sys.argv[0]).name) or 0)
        self.enterContext(mock.patch.object(bench, 'script', lambda relative: profile))
        argv = sys.argv
        status, _, err = self.run_recipe()
        self.assertEqual((status, seen), (0, ['system_import_profile.py']), err)
        self.assertIs(sys.argv, argv)

    def test_a_cancelled_collection_ends_with_the_status_of_the_signal(self):
        def cancelled(argv):
            slot.CANCELLED.set()
            slot.CANCELLED_SIGNAL = signal.SIGTERM
            raise SystemExit('cancelled')
        self.addCleanup(slot.CANCELLED.clear)
        self.addCleanup(setattr, slot, 'CANCELLED_SIGNAL', None)
        self.enterContext(mock.patch.object(bench, 'script', lambda relative: types.SimpleNamespace(main=cancelled)))
        status, _, err = self.run_recipe()
        self.assertEqual((status, err), (128 + signal.SIGTERM, 'cancelled\n'))

    def test_any_other_exit_of_the_collections_is_left_alone(self):
        def refused(argv):
            raise SystemExit('pin: failed workload or user-store open')
        self.enterContext(mock.patch.object(bench, 'script', lambda relative: types.SimpleNamespace(main=refused)))
        with self.assertRaises(SystemExit) as stop:
            self.run_recipe()
        self.assertEqual(stop.exception.code, 'pin: failed workload or user-store open')

    def test_an_existing_output_directory_is_refused_before_the_driver_is_built(self):
        self.out.mkdir()
        status, _, err = self.run_recipe()
        self.assertEqual(status, 1)
        self.assertEqual(err, f"mkdir: cannot create directory '{self.out}': File exists\n")
        self.assertEqual(self.kinds('run'), [])
        self.assertEqual(self.calls, [])

    def test_an_argument_error_comes_before_the_directory_is_made(self):
        status, _, err = self.run_recipe('30', '--bogus')
        self.assertEqual((status, err), (2, 'unknown argument: --bogus\n'))
        self.assertFalse(self.out.exists())


if __name__ == '__main__':
    unittest.main()
