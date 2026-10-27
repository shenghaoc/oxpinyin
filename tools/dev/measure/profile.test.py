#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the W8 profile controller (tools/dev/measure/profile.py).

The run takes its host name, program lookup and program launcher as seams, so the
Darwin and the Linux paths both run here, on either OS, against scripted programs.
A few tests start real stub programs through the real launcher and the real CLI
status ladder; taskset, the one Linux mechanism, is skipped where it is absent.
"""
import contextlib
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import runners, slot, test_support
from tools.dev.measure import bench, profile
from tools.dev.runners import Finished, write

EXTRACTOR = 'tools/profile/extract-hot-stacks.py'
SAMPLY_CYCLES = '2048'
FALLBACK_CYCLES = '32'
HELP_WITH_FLAG = 'USAGE: samply record\n    --unstable-presymbolicate  write a symbol sidecar\n'
HELP_WITHOUT_FLAG = 'USAGE: samply record\n'


def default_level():
    """The level extract-hot-stacks.py applies without --min-samples."""
    return bench.script(EXTRACTOR).MIN_SAMPLES


def profile_gz(samples):
    """A gzipped profile holding `samples` samples on one named frame."""
    document = {'threads': [{
        'stringArray': ['hot'],
        'samples': {'stack': [0] * samples},
        'stackTable': {'prefix': [None], 'frame': [0]},
        'frameTable': {'func': [0]},
        'funcTable': {'name': [0]},
    }]}
    return gzip.compress(json.dumps(document).encode())


def staged(call):
    """The directory a cargo cinstall call was told to install into."""
    return Path(next(part for part in call.command if part.startswith('--destdir=')).split('=', 1)[1])


def option(command, flag):
    """The value after FLAG in COMMAND, or None."""
    return command[command.index(flag) + 1] if flag in command else None


class Result:
    """What a run left on the two streams, and its status."""

    def __init__(self, status, stdout, stderr):
        self.status, self.stdout, self.stderr = status, stdout, stderr


class Call:
    """One program start the fake launcher saw: how it was started, what, and the variables it would see."""

    def __init__(self, kind, command, env, destination=None):
        self.kind, self.command, self.env, self.destination = kind, list(command), dict(env), destination

    @property
    def program(self):
        """The command without the taskset pin in front of it."""
        return self.command[3:] if self.command[:2] == ['taskset', '-c'] else self.command

    @property
    def name(self):
        return Path(self.program[0]).name


class FakeCommands:
    """Scripted programs for a scenario.

    A program's status comes from `statuses` (default 0); `effects` stand in for what it
    leaves on disk; `outputs` are what `output` returns. The worker a profiler wraps prints
    the PERF_CYCLES it was given, so a test sees which cycle count each profiler received.
    """

    def __init__(self, scenario):
        self.scenario = scenario
        self.calls = []
        self.statuses = {}
        self.outputs = {}
        self.effects = {}
        self.refusal = None
        self.samply_help = HELP_WITH_FLAG

    def of(self, kind=None, name=None):
        return [call for call in self.calls
                if (kind is None or call.kind == kind) and (name is None or call.name == name)]

    def shape(self):
        return [f'{call.kind}:{call.name}' for call in self.calls]

    def start(self, kind, command, env, destination=None):
        call = Call(kind, command, env, destination)
        self.calls.append(call)
        effect = self.effects.get(call.name)
        if effect is not None:
            effect(call)
        return call

    def status_of(self, call):
        status = self.statuses.get(call.name, 0)
        return status(call) if callable(status) else status

    def probe(self, command, env):
        call = self.start('probe', command, env)
        if call.name == 'samply':
            return self.status_of(call), self.samply_help
        return self.status_of(call), ''

    def run(self, command, env):
        return self.status_of(self.start('run', command, env))

    def output(self, command, env):
        call = self.start('output', command, env)
        answer = self.outputs.get(call.name, b'')
        return self.status_of(call), answer(call) if callable(answer) else answer

    def tee(self, command, env, destination):
        call = self.start('tee', command, env, destination)
        # The worker prints the cycle count it was given, as bisect --perf's JSON would.
        text = f'worker PERF_CYCLES={call.env.get("PERF_CYCLES")}\n'.encode()
        write(text)
        Path(destination).write_bytes(text)
        return self.status_of(call)

    def pipe(self, first, second, env, destination):
        call = self.start('pipe', first, env, destination)
        call.second = list(second)
        Path(destination).write_bytes(b'<svg/>')
        return self.status_of(call)

    def guard(self, obj, env):
        call = self.start('guard', [str(obj)], env)
        if self.refusal is not None:
            bench.fail('REFUSED: ' + self.refusal)


class Scenario:
    """A temporary tree with a tkrzw export and a verified model cache, and a run against fake programs."""

    def __init__(self, test, system='Linux', tools=('samply',), env=None):
        self.test = test
        self.root = Path(test.enterContext(tempfile.TemporaryDirectory()))
        self.profile = self.root / 'profile'
        self.export = self.root / 'export'
        self.cache = self.root / 'cache'
        self.model = self.cache / 'extracted'
        self.export.mkdir()
        (self.export / 'datagen-manifest.txt').write_text('format=1\nbackend=tkt\n')
        for name in ('pinyin_index.bin', 'phrase_index.bin', 'bigram.db', 'table.conf', 'extra.bin', 'notes.txt'):
            (self.export / name).write_text(name)
        self.model.mkdir(parents=True)
        for name in profile.EXPECTED_MODEL_FILES:
            (self.model / name).write_text(name)
        (self.cache / 'verified').write_text(f'sha256={profile.MODEL20_SHA256}\n')
        self.system = system
        self.tools = set(tools)
        self.env = {'OXPINYIN_PROFILE_DIR': str(self.profile), 'PINYIN_EXPORT_DIR': str(self.export),
                    'PINYIN_MODEL_DIR': str(self.model), 'PATH': '/nowhere', **(env or {})}
        self.commands = FakeCommands(self)
        self.profile_bytes = profile_gz(1)
        self.sidecar = True
        self.libdirs = []
        self.commands.effects.update(cargo=self.cargo, cc=self.compile, gcc=self.compile, samply=self.samply)
        self.commands.outputs['pkg-config'] = self.pkg_config

    @property
    def stage(self):
        return self.profile / 'stage'

    def which(self, name):
        return f'/stub/{name}' if name in self.tools else None

    # What the programs leave behind.
    def cargo(self, call):
        if call.kind != 'run':
            return
        destdir = staged(call)
        if self.system == 'Darwin':
            (destdir / 'usr/lib/pkgconfig').mkdir(parents=True)
            (destdir / 'usr/lib/pkgconfig/libpinyin.pc').write_text('pc')
            (destdir / 'usr/lib/libpinyin.15.dylib').write_text('dylib')
        else:
            (destdir / 'usr/lib64/pkgconfig').mkdir(parents=True)
            (destdir / 'usr/lib64/pkgconfig/oxpinyin.pc').write_text('pc')
            (destdir / 'usr/lib64/libpinyin_capi.so').write_text('so')

    def compile(self, call):
        Path(option(call.command, '-o')).write_text('bisect')

    def samply(self, call):
        output = option(call.command, '-o')
        if output is not None:
            Path(output).write_bytes(self.profile_bytes)
            if self.sidecar:
                Path(output[:output.rindex('.')] + '.syms.json').write_text('{}')

    def pkg_config(self, call):
        sysroot = call.env['PKG_CONFIG_SYSROOT_DIR']
        self.libdirs.append(call.command[-1])
        return (f'{sysroot}/usr/lib\n' if self.system == 'Darwin' else f'{sysroot}/usr/lib64\n').encode()

    def invoke(self, argv=()):
        """Run the recipe against the fakes; return the status and both streams."""
        out = io.TextIOWrapper(io.BytesIO(), encoding='utf-8', write_through=True)
        err = io.TextIOWrapper(io.BytesIO(), encoding='utf-8', write_through=True)
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            try:
                status = profile.profile_w8(list(argv), self.env, system=self.system, which=self.which,
                                            commands=self.commands)
            except Finished as stop:
                status = stop.status
        return Result(status, out.buffer.getvalue().decode(), err.buffer.getvalue().decode())


class ScenarioTest(unittest.TestCase):
    system = 'Linux'
    tools = ('samply',)

    def setUp(self):
        self.scenario = Scenario(self, self.system, self.tools)
        self.commands = self.scenario.commands
        self.env = self.scenario.env

    def invoke(self, argv=()):
        return self.scenario.invoke(argv)

    @staticmethod
    def lines(text):
        return text.splitlines()

    def assertStatus(self, result, status):
        self.assertEqual(result.status, status, result.stderr)


class CheckTests(ScenarioTest):
    """--check reports the qualification and stops before any input is read."""

    def test_reports_the_samply_default_cycle_count(self):
        result = self.invoke(['--check'])
        self.assertStatus(result, 0)
        self.assertIn(f'samply cycles: {SAMPLY_CYCLES}\n', result.stdout)

    def test_reports_a_cycle_count_override(self):
        self.env['PERF_CYCLES'] = '7'
        result = self.invoke(['--check'])
        self.assertIn('samply cycles: 7\n', result.stdout)

    def test_reports_presymbolicate_availability(self):
        result = self.invoke(['--check'])
        self.assertIn('samply presymbolicate: available\n', result.stdout)
        self.assertNotIn('--unstable-presymbolicate', result.stderr)

    def test_reports_missing_presymbolicate(self):
        self.commands.samply_help = HELP_WITHOUT_FLAG
        result = self.invoke(['--check'])
        self.assertStatus(result, 0)
        self.assertIn('samply presymbolicate: unavailable\n', result.stdout)
        self.assertEqual(result.stderr.splitlines()[0],
                         'warning: samply has no --unstable-presymbolicate; the hot-stack report '
                         'will show unresolved frames.')

    def test_says_nothing_about_samply_when_there_is_none(self):
        self.scenario.tools.clear()
        result = self.invoke(['--check'])
        self.assertStatus(result, 0)
        self.assertNotIn('presymbolicate', result.stdout + result.stderr)
        self.assertEqual(self.commands.shape(), [])

    def test_linux_reports_both_defaults(self):
        result = self.invoke(['--check'])
        self.assertEqual(self.lines(result.stdout),
                         [f'samply cycles: {SAMPLY_CYCLES}', 'samply presymbolicate: available',
                          'profile platform: Linux; profiler fallback chain: samply, flamegraph, perf, valgrind',
                          f'fallback cycles: {FALLBACK_CYCLES}'])
        self.assertEqual(result.stderr, '')

    def test_linux_applies_a_requested_count_to_both(self):
        self.env['PERF_CYCLES'] = '7'
        result = self.invoke(['--check'])
        self.assertIn('samply cycles: 7\n', result.stdout)
        self.assertIn('fallback cycles: 7\n', result.stdout)

    def test_creates_nothing_and_starts_only_the_help_probe(self):
        result = self.invoke(['--check'])
        self.assertStatus(result, 0)
        self.assertFalse(self.scenario.profile.exists())
        self.assertEqual(self.commands.shape(), ['probe:samply'])
        self.assertEqual(self.commands.calls[0].command, ['samply', 'record', '--help'])

    def test_only_the_first_argument_asks_for_a_check(self):
        (self.scenario.export / 'datagen-manifest.txt').write_text('backend=bdb\n')
        result = self.invoke(['--other', '--check'])
        self.assertStatus(result, 2)
        self.assertIn('is not a tkrzw export', result.stderr)
        result = self.invoke(['--check', '--other'])
        self.assertStatus(result, 0)


class DarwinTests(ScenarioTest):
    system = 'Darwin'

    def setUp(self):
        super().setUp()
        self.env['OXPINYIN_TKRZW_PREFIX'] = '/opt/tkrzw'

    def test_refuses_perf_cpu_with_the_affinity_reason(self):
        self.env['PERF_CPU'] = '0'
        result = self.invoke()
        self.assertStatus(result, 2)
        self.assertEqual(result.stderr,
                         'fatal: PERF_CPU is unsupported on macOS: Mach affinity is a cache hint, '
                         'not CPU pinning; unset PERF_CPU.\n')
        self.assertEqual(result.stdout, '')
        self.assertEqual(self.commands.shape(), [])

    def test_an_empty_perf_cpu_is_not_a_request(self):
        self.env['PERF_CPU'] = ''
        self.assertStatus(self.invoke(['--check']), 0)

    def test_refuses_missing_samply(self):
        self.scenario.tools.clear()
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(result.stderr,
                         'fatal: samply is required on macOS; install it (for example: cargo install samply) '
                         'or put it on PATH.\n')

    def test_check_names_the_linux_only_parts(self):
        result = self.invoke(['--check'])
        self.assertStatus(result, 0)
        self.assertEqual(self.lines(result.stdout),
                         [f'samply cycles: {SAMPLY_CYCLES}', 'samply presymbolicate: available',
                          'profile platform: Darwin; profiler=samply'])
        self.assertEqual(self.lines(result.stderr),
                         ['note: taskset and PERF_CPU are unsupported on macOS: Mach affinity is a cache hint, '
                          'not CPU pinning',
                          'note: perf record and the perf_event_paranoid hint are Linux-only; skipped on macOS'])

    def test_refuses_missing_cargo_c_before_reading_inputs(self):
        self.commands.statuses['cargo'] = 101
        self.env['PINYIN_EXPORT_DIR'] = str(self.scenario.root / 'absent')
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(result.stderr,
                         'fatal: cargo-c is required on macOS; install cargo-cinstall or put it on PATH.\n')
        self.assertEqual(self.commands.of('probe', 'cargo')[0].command, ['cargo', 'cinstall', '--version'])
        self.assertFalse(self.scenario.profile.exists())

    def test_refuses_an_unnamed_tkrzw_runtime(self):
        del self.env['OXPINYIN_TKRZW_PREFIX']
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(result.stderr,
                         'fatal: set OXPINYIN_TKRZW_PREFIX or OXPINYIN_TKRZW_LIBRARY so runtime.sh '
                         'can verify libtkrzw.\n')
        self.assertFalse(self.scenario.profile.exists())

    def test_a_named_runtime_library_is_enough(self):
        del self.env['OXPINYIN_TKRZW_PREFIX']
        self.env['OXPINYIN_TKRZW_LIBRARY'] = '/opt/tkrzw/lib/libtkrzw.dylib'
        self.assertStatus(self.invoke(), 0)

    def test_a_cycle_note_has_no_fallback_clause(self):
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertIn(f'  cycles: {SAMPLY_CYCLES}\n', result.stdout)

    def test_a_failed_record_has_no_fallback_and_reports_no_total(self):
        self.commands.statuses['samply'] = 1
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(self.lines(result.stderr)[-1],
                         'error: samply failed; macOS has no perf or Callgrind fallback.')
        self.assertNotIn('samples:', result.stdout)
        self.assertEqual(self.commands.of('tee', 'samply')[0].name, 'samply')
        self.assertEqual(len(self.commands.of('tee')), 1)

    def test_a_successful_record_names_what_was_skipped(self):
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertEqual(self.lines(result.stderr)[-2:],
                         ['note: perf record and the perf_event_paranoid hint are Linux-only; skipped on macOS',
                          'note: flamegraph and Valgrind/Callgrind fallbacks are not used on macOS; '
                          'samply is required'])
        self.assertEqual(len(self.commands.of('tee')), 1)

    def test_stages_with_the_tkrzw_feature_and_opts_the_dylib_out_of_the_shared_cache(self):
        self.env['RUSTFLAGS'] = '-C target-cpu=native'
        self.assertStatus(self.invoke(), 0)
        install = self.commands.of('run', 'cargo')[0]
        self.assertEqual(install.command, [
            'cargo', 'cinstall', '--locked', '--profile', 'profiling', '--no-default-features', '--features',
            'tkrzw', '-p', 'oxpinyin-capi', f'--destdir={self.scenario.stage}', '--prefix=/usr',
            '--manifest-path', str(ROOT / 'Cargo.toml')])
        self.assertEqual(install.env['RUSTFLAGS'],
                         '-C target-cpu=native -C link-arg=-Wl,-not_for_dyld_shared_cache')

    def test_rustflags_are_only_the_opt_out_when_none_were_set(self):
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(self.commands.of('run', 'cargo')[0].env['RUSTFLAGS'],
                         '-C link-arg=-Wl,-not_for_dyld_shared_cache')

    def test_looks_the_library_directory_up_by_libpinyin_then_oxpinyin(self):
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(self.scenario.libdirs, ['libpinyin'])
        lookup = self.commands.of('output', 'pkg-config')[0]
        self.assertEqual(lookup.command, ['pkg-config', '--variable=libdir', 'libpinyin'])
        stage = self.scenario.stage
        self.assertEqual(lookup.env['PKG_CONFIG_PATH'], f'{stage}/usr/lib/pkgconfig:{stage}/usr/lib64/pkgconfig')
        self.assertEqual(lookup.env['PKG_CONFIG_SYSROOT_DIR'], str(stage))

    def test_an_empty_libpinyin_answer_falls_back_to_oxpinyin(self):
        answers = [b'\n', f'{self.scenario.stage}/usr/lib\n'.encode()]
        self.commands.outputs['pkg-config'] = lambda call: answers.pop(0)
        self.assertStatus(self.invoke(), 0)
        packages = [call.command[-1] for call in self.commands.of('output', 'pkg-config')]
        self.assertEqual(packages, ['libpinyin', 'oxpinyin'])

    def test_a_failed_lookup_ends_the_run_with_its_status(self):
        self.commands.statuses['pkg-config'] = 3
        result = self.invoke()
        self.assertStatus(result, 3)
        self.assertEqual(len(self.commands.of('output', 'pkg-config')), 1)

    def test_picks_the_first_candidate_dylib_in_the_documented_order(self):
        def stage_all(call):
            if call.kind != 'run':
                return
            destdir = staged(call)
            (destdir / 'usr/lib/pkgconfig').mkdir(parents=True)
            (destdir / 'usr/lib/pkgconfig/libpinyin.pc').write_text('pc')
            for name in ('libpinyin_capi.dylib', 'libpinyin.15.0.0.dylib', 'libpinyin.15.dylib'):
                (destdir / 'usr/lib' / name).write_text(name)
        self.commands.effects['cargo'] = stage_all
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(self.commands.of('guard')[0].command, [f'{self.scenario.stage}/usr/lib/libpinyin.15.dylib'])

    def test_no_candidate_dylib_is_a_fatal_with_an_empty_name(self):
        def stage_none(call):
            if call.kind != 'run':
                return
            destdir = staged(call)
            (destdir / 'usr/lib/pkgconfig').mkdir(parents=True)
            (destdir / 'usr/lib/pkgconfig/libpinyin.pc').write_text('pc')
        self.commands.effects['cargo'] = stage_none
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(self.lines(result.stderr)[-1], 'fatal:  not found')
        self.assertEqual(self.commands.of('guard'), [])

    def test_reports_the_dylib_and_copies_the_whole_export(self):
        result = self.invoke()
        self.assertStatus(result, 0)
        data = self.scenario.stage / 'usr/share/oxpinyin'
        self.assertIn(f'installed capi dylib: {self.scenario.stage}/usr/lib/libpinyin.15.dylib\n', result.stdout)
        self.assertIn(f'installed capi data: {data}\n', result.stdout)
        self.assertEqual(sorted(path.name for path in data.iterdir()),
                         ['bigram.db', 'extra.bin', 'interpolation2.text', 'phrase_index.bin',
                          'pinyin_index.bin', 'table.conf'])

    def test_a_missing_table_conf_is_not_an_error(self):
        (self.scenario.export / 'table.conf').unlink()
        self.assertStatus(self.invoke(), 0)
        self.assertFalse((self.scenario.stage / 'usr/share/oxpinyin/table.conf').exists())

    def test_the_harness_builds_with_cc_and_without_libdl(self):
        self.assertStatus(self.invoke(), 0)
        build = self.commands.of('run', 'cc')[0]
        self.assertEqual(build.command, ['cc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2', '-g', '-o',
                                         f'{self.scenario.profile}/bisect', str(ROOT / 'tools/bisection/bisect.c')])

    def test_the_loader_path_is_left_alone(self):
        self.env['LD_LIBRARY_PATH'] = '/inherited'
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(self.commands.of('tee', 'samply')[0].env['LD_LIBRARY_PATH'], '/inherited')
        del self.env['LD_LIBRARY_PATH']
        self.commands.calls.clear()
        self.assertStatus(self.invoke(), 0)
        self.assertNotIn('LD_LIBRARY_PATH', self.commands.of('tee', 'samply')[0].env)

    def test_stage_reuse_needs_the_pc_file_a_dylib_and_the_request(self):
        stage = self.scenario.stage
        (stage / 'usr/lib/pkgconfig').mkdir(parents=True)
        (stage / 'usr/lib/pkgconfig/libpinyin.pc').write_text('pc')
        (stage / 'usr/lib/libpinyin.dylib').write_text('dylib')
        (stage / 'old').write_text('stale')
        self.env['PROFILE_REUSE_STAGE'] = '1'
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertIn(f'--- reusing stage: {stage} ---\n', result.stdout)
        self.assertEqual(self.commands.of('run', 'cargo'), [])
        self.assertTrue((stage / 'old').exists())
        self.assertNotIn('RUSTFLAGS', self.commands.of('output', 'pkg-config')[0].env)

    def test_a_stage_without_a_dylib_is_not_reused(self):
        stage = self.scenario.stage
        (stage / 'usr/lib/pkgconfig').mkdir(parents=True)
        (stage / 'usr/lib/pkgconfig/libpinyin.pc').write_text('pc')
        (stage / 'old').write_text('stale')
        self.env['PROFILE_REUSE_STAGE'] = '1'
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertIn(f'--- cargo cinstall --profile profiling -> {stage} ---\n', result.stdout)
        self.assertFalse((stage / 'old').exists())


class LinuxStageTests(ScenarioTest):
    def test_stages_with_the_default_features(self):
        self.assertStatus(self.invoke(), 0)
        install = self.commands.of('run', 'cargo')[0]
        self.assertEqual(install.command, [
            'cargo', 'cinstall', '--locked', '--profile', 'profiling', '-p', 'oxpinyin-capi',
            f'--destdir={self.scenario.stage}', '--prefix=/usr', '--manifest-path', str(ROOT / 'Cargo.toml')])
        self.assertNotIn('RUSTFLAGS', install.env)

    def test_exports_the_pkg_config_search_path_and_sysroot(self):
        self.env['PKG_CONFIG_PATH'] = '/inherited/pkgconfig'
        self.assertStatus(self.invoke(), 0)
        lookup = self.commands.of('output', 'pkg-config')[0]
        self.assertEqual(lookup.command, ['pkg-config', '--variable=libdir', 'oxpinyin'])
        self.assertEqual(lookup.env['PKG_CONFIG_PATH'],
                         f'{self.scenario.stage}/usr/lib64/pkgconfig:/inherited/pkgconfig')
        self.assertEqual(lookup.env['PKG_CONFIG_SYSROOT_DIR'], str(self.scenario.stage))

    def test_removes_the_old_stage_before_staging(self):
        (self.scenario.stage).mkdir(parents=True)
        (self.scenario.stage / 'old').write_text('stale')
        seen = []
        self.commands.effects['cargo'] = lambda call: (seen.append((self.scenario.stage / 'old').exists()),
                                                       self.scenario.cargo(call))
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(seen, [False])

    def test_a_cargo_failure_ends_the_run_with_its_status(self):
        self.commands.statuses['cargo'] = 101
        result = self.invoke()
        self.assertStatus(result, 101)
        self.assertEqual(self.commands.of('output'), [])

    def test_the_reuse_test_looks_at_the_pc_file_and_the_library(self):
        stage = self.scenario.stage
        (stage / 'usr/lib64/pkgconfig').mkdir(parents=True)
        (stage / 'usr/lib64/pkgconfig/oxpinyin.pc').write_text('pc')
        (stage / 'usr/lib64/libpinyin_capi.so').write_text('so')
        for value, reused in (('1', True), ('0', False), ('', False), ('yes', False)):
            with self.subTest(request=value):
                self.commands.calls.clear()
                if value:
                    self.env['PROFILE_REUSE_STAGE'] = value
                else:
                    self.env.pop('PROFILE_REUSE_STAGE', None)
                self.assertStatus(self.invoke(), 0)
                self.assertEqual(not self.commands.of('run', 'cargo'), reused)

    def test_a_stage_with_only_the_pc_file_is_not_reused(self):
        stage = self.scenario.stage
        (stage / 'usr/lib64/pkgconfig').mkdir(parents=True)
        (stage / 'usr/lib64/pkgconfig/oxpinyin.pc').write_text('pc')
        self.env['PROFILE_REUSE_STAGE'] = '1'
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(len(self.commands.of('run', 'cargo')), 1)

    def test_a_missing_library_is_fatal_naming_it(self):
        self.commands.effects['cargo'] = lambda call: None
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(self.lines(result.stderr)[-1],
                         f'fatal: {self.scenario.stage}/usr/lib64/libpinyin_capi.so not found')

    def test_copies_the_three_tables_and_the_model_interpolation(self):
        result = self.invoke()
        self.assertStatus(result, 0)
        data = self.scenario.stage / 'usr/share/oxpinyin'
        self.assertEqual(sorted(path.name for path in data.iterdir()),
                         ['bigram.db', 'interpolation2.text', 'phrase_index.bin', 'pinyin_index.bin'])
        self.assertEqual((data / 'interpolation2.text').read_text(), 'interpolation2.text')
        self.assertIn(f'installed capi .so:  {self.scenario.stage}/usr/lib64/libpinyin_capi.so\n', result.stdout)

    def test_the_harness_builds_with_gcc_and_libdl(self):
        self.assertStatus(self.invoke(), 0)
        build = self.commands.of('run', 'gcc')[0]
        self.assertEqual(build.command, ['gcc', '-std=gnu11', '-Wall', '-Wextra', '-Werror', '-O2', '-g', '-o',
                                         f'{self.scenario.profile}/bisect', str(ROOT / 'tools/bisection/bisect.c'),
                                         '-ldl'])
        self.assertIn('--- building bisect --perf harness ---\n', self.invoke().stdout)

    def test_a_compiler_failure_stops_before_the_subject_is_checked(self):
        self.commands.statuses['gcc'] = 1
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(self.commands.of('guard'), [])

    def test_exports_the_loader_path_and_the_worker_settings(self):
        self.env['LD_LIBRARY_PATH'] = '/inherited'
        self.assertStatus(self.invoke(), 0)
        launch = self.commands.of('tee', 'samply')[0]
        self.assertEqual(launch.env['LD_LIBRARY_PATH'], f'{self.scenario.stage}/usr/lib64:/inherited')
        self.assertEqual(launch.env['PERF_BACKEND'], 'oxpinyin')
        self.assertEqual(launch.env['PERF_MODE'], 'speed')
        self.assertEqual(self.commands.of('guard')[0].env['LD_LIBRARY_PATH'],
                         f'{self.scenario.stage}/usr/lib64:/inherited')

    def test_the_loader_path_is_just_the_library_directory_when_none_was_inherited(self):
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(self.commands.of('tee', 'samply')[0].env['LD_LIBRARY_PATH'],
                         f'{self.scenario.stage}/usr/lib64')


class InputTests(ScenarioTest):
    def test_the_banner_names_the_pin_the_model_and_the_cycles(self):
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertEqual(self.lines(result.stdout)[:6], [
            'oxpinyin Stage-2 W8-cycle profile',
            f'  pin: {profile.PIN_REF}',
            f'  model20 SHA-256: {profile.MODEL20_SHA256}',
            f'  model dir: {os.path.realpath(self.scenario.model)}',
            f'  artifacts: {self.scenario.profile}',
            f'  cycles: {SAMPLY_CYCLES} (samply), {FALLBACK_CYCLES} (other profilers)'])

    def test_one_requested_count_is_the_whole_note(self):
        self.env['PERF_CYCLES'] = '7'
        self.assertIn('  cycles: 7\n', self.invoke().stdout)

    def test_an_empty_count_takes_the_defaults(self):
        self.env['PERF_CYCLES'] = ''
        self.assertIn(f'  cycles: {SAMPLY_CYCLES} (samply), {FALLBACK_CYCLES} (other profilers)\n',
                      self.invoke().stdout)

    def test_a_non_tkrzw_export_is_refused_with_status_2(self):
        (self.scenario.export / 'datagen-manifest.txt').write_text('backend=bdb\n')
        result = self.invoke()
        self.assertStatus(result, 2)
        self.assertEqual(result.stderr,
                         f'fatal: {self.scenario.export} is not a tkrzw export; rebuild it with --features tkrzw\n')

    def test_the_backend_line_must_stand_alone(self):
        for text in ('backend=tkt2\n', ' backend=tkt\n', 'backend=tkt\r\n', 'xbackend=tkt\n'):
            with self.subTest(text=text):
                (self.scenario.export / 'datagen-manifest.txt').write_text(text, newline='')
                self.assertStatus(self.invoke(), 2)
        (self.scenario.export / 'datagen-manifest.txt').write_text('a=1\nbackend=tkt')
        self.assertStatus(self.invoke(), 0)

    def test_a_missing_manifest_is_named_before_the_refusal(self):
        (self.scenario.export / 'datagen-manifest.txt').unlink()
        result = self.invoke()
        self.assertStatus(result, 2)
        self.assertEqual(self.lines(result.stderr)[0],
                         f'grep: {self.scenario.export}/datagen-manifest.txt: No such file or directory')
        self.assertIn('is not a tkrzw export', self.lines(result.stderr)[1])

    def test_a_missing_table_is_named(self):
        (self.scenario.export / 'bigram.db').unlink()
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(result.stderr, f'fatal: exported table missing at {self.scenario.export}/bigram.db\n')

    def test_the_profile_directory_exists_before_the_inputs_are_read(self):
        (self.scenario.export / 'bigram.db').unlink()
        self.invoke()
        self.assertTrue(self.scenario.profile.is_dir())

    def test_a_model_directory_that_is_not_one_is_refused(self):
        self.env['PINYIN_MODEL_DIR'] = str(self.scenario.root / 'nowhere')
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(result.stderr, f'fatal: model dir is not a directory: {self.scenario.root}/nowhere\n'
                                        '  run: just fetch-model\n')

    def test_a_missing_model_file_is_named(self):
        (self.scenario.model / 'merged.table').unlink()
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(result.stderr, f'fatal: model dir {self.scenario.model} is missing merged.table\n'
                                        '  run: just fetch-model\n')

    def test_the_model_cache_names_the_extracted_directory(self):
        del self.env['PINYIN_MODEL_DIR']
        self.env['PINYIN_MODEL_CACHE'] = str(self.scenario.cache)
        self.assertStatus(self.invoke(), 0)

    def test_the_model_directory_wins_over_the_cache(self):
        self.env['PINYIN_MODEL_CACHE'] = str(self.scenario.root / 'nowhere')
        self.assertStatus(self.invoke(), 0)

    def test_a_directory_without_the_marker_or_archive_is_unverified(self):
        (self.scenario.cache / 'verified').unlink()
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(result.stderr,
                         f'fatal: {self.scenario.model} is not a verified model20 cache '
                         f'(no sha256={profile.MODEL20_SHA256} marker)\n  run: just fetch-model\n')

    def test_a_marker_for_another_digest_is_not_a_marker(self):
        (self.scenario.cache / 'verified').write_text('sha256=' + '0' * 64 + '\n')
        self.assertStatus(self.invoke(), 1)

    def test_an_unreadable_marker_is_named_as_grep_names_it_and_is_no_match(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            self.assertFalse(profile.W8.marker_matches(self.scenario.root / 'absent' / 'verified'))
        self.assertEqual(err.getvalue(), f'grep: {self.scenario.root}/absent/verified: No such file or directory\n')

    def test_the_marker_must_be_a_whole_line(self):
        (self.scenario.cache / 'verified').write_text(f'x sha256={profile.MODEL20_SHA256}\n')
        self.assertStatus(self.invoke(), 1)
        (self.scenario.cache / 'verified').write_text(f'other\nsha256={profile.MODEL20_SHA256}')
        self.assertStatus(self.invoke(), 0)

    def archive(self, content):
        (self.scenario.cache / 'verified').unlink()
        (self.scenario.cache / 'downloads').mkdir()
        path = self.scenario.cache / 'downloads/model20.text.tar.gz'
        path.write_bytes(content)
        return path

    def test_the_archive_digest_verifies_the_cache_without_a_marker(self):
        content = b'the archive'
        with mock.patch.object(profile, 'MODEL20_SHA256', hashlib.sha256(content).hexdigest()):
            self.archive(content)
            self.assertStatus(self.invoke(), 0)

    def test_a_different_archive_is_refused_with_both_digests(self):
        self.archive(b'another archive')
        path = Path(os.path.realpath(self.scenario.cache), 'downloads/model20.text.tar.gz')
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(result.stderr,
                         f'fatal: archive {path} is not the pinned model20\n'
                         f'  expected: {profile.MODEL20_SHA256}\n'
                         f'  actual:   {hashlib.sha256(b"another archive").hexdigest()}\n')

    def test_a_linked_model_directory_is_resolved_for_the_rest_of_the_run(self):
        link = self.scenario.root / 'link'
        link.symlink_to(self.scenario.model)
        self.env['PINYIN_MODEL_DIR'] = str(link)
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertIn(f'  model dir: {os.path.realpath(self.scenario.model)}\n', result.stdout)
        self.assertIn(f'model dir: {os.path.realpath(self.scenario.model)}\n', result.stdout)

    def test_the_header_records_the_pin_the_library_and_the_cycles(self):
        self.assertStatus(self.invoke(), 0)
        stage = self.scenario.stage
        self.assertEqual((self.scenario.profile / 'header.txt').read_text(),
                         f'pin: {profile.PIN_REF}\nmodel20_sha256: {profile.MODEL20_SHA256}\n'
                         f'so: {stage}/usr/lib64/libpinyin_capi.so\ndata: {stage}/usr/share/oxpinyin\n'
                         f'cycles: {SAMPLY_CYCLES} (samply), {FALLBACK_CYCLES} (other profilers)\n'
                         'profile: profiling (release + line-tables-only + thin LTO)\n')


class GuardTests(ScenarioTest):
    """The dlopened subject is checked before any profiler sees the worker."""

    def test_profiler_receives_the_worker_after_the_subject_preflight(self):
        result = self.invoke()
        self.assertStatus(result, 0)
        shape = self.commands.shape()
        self.assertLess(shape.index('guard:' + str(self.commands.of('guard')[0].name)), shape.index('tee:samply'))
        so = f'{self.scenario.stage}/usr/lib64/libpinyin_capi.so'
        self.assertEqual(self.commands.of('guard')[0].command, [so])
        launch = self.commands.of('tee', 'samply')[0]
        self.assertEqual(launch.command, [
            'samply', 'record', '--save-only', '--unstable-presymbolicate', '-o',
            f'{self.scenario.profile}/w8-cycle.profile.json.gz', '--',
            f'{self.scenario.profile}/bisect', '--perf', so, f'{self.scenario.stage}/usr/share/oxpinyin'])

    def test_the_check_is_the_runtime_check_for_the_subject_in_the_run_environment(self):
        with mock.patch.object(bench, 'runtime_check') as check:
            profile.Commands().guard('/the/subject.so', {'A': '1'})
        test_support.assert_calls(self, check, [mock.call('run-w8-cycle.sh', 'auto', 'subject', '/the/subject.so',
                                                          env={'A': '1'})])

    def test_a_refusal_prevents_the_profiler_and_the_header(self):
        self.commands.refusal = 'run-w8-cycle.sh: subject: libtkrzw resolved to /usr/lib/libtkrzw.so'
        result = self.invoke()
        self.assertStatus(result, 1)
        self.assertEqual(self.lines(result.stderr)[-1],
                         'REFUSED: run-w8-cycle.sh: subject: libtkrzw resolved to /usr/lib/libtkrzw.so')
        self.assertEqual(self.commands.of('tee'), [])
        self.assertFalse((self.scenario.profile / 'header.txt').exists())


class ChainTests(ScenarioTest):
    """The profilers form a fallback chain; each runs only when its predecessors failed or are missing."""
    tools = ()

    def have(self, *names):
        self.scenario.tools.update(names)

    def test_nothing_installed_writes_the_timing_only(self):
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertEqual(self.lines(result.stderr)[-1],
                         'warning: samply, flamegraph, perf, and valgrind all missing; writing timing only')
        launch = self.commands.of('tee')[0]
        self.assertEqual(launch.command[0], f'{self.scenario.profile}/bisect')
        self.assertEqual(launch.env['PERF_CYCLES'], FALLBACK_CYCLES)
        self.assertEqual(self.lines(result.stdout)[-3:],
                         [f'timing: {self.scenario.profile}/w8-cycle-timing.json',
                          f'header: {self.scenario.profile}/header.txt', 'done.'])
        self.assertEqual((self.scenario.profile / 'w8-cycle-timing.json').read_text(),
                         f'worker PERF_CYCLES={FALLBACK_CYCLES}\n')

    def test_a_failing_worker_ends_the_timing_only_run_with_its_status(self):
        self.commands.statuses['bisect'] = 9
        result = self.invoke()
        self.assertStatus(result, 9)
        self.assertNotIn('done.', result.stdout)

    def test_flamegraph_wraps_the_worker_and_stops_the_chain(self):
        self.have('flamegraph', 'perf', 'valgrind')
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertIn('--- flamegraph wrapper (cargo-flamegraph) ---\n', result.stdout)
        self.assertIn(f'flamegraph: {self.scenario.profile}/w8-cycle.svg\n', result.stdout)
        launch = self.commands.of('tee')[0]
        self.assertEqual(launch.command[:4], ['flamegraph', '-o', f'{self.scenario.profile}/w8-cycle.svg', '--'])
        self.assertEqual(len(self.commands.of('tee')), 1)

    def test_a_failed_flamegraph_falls_back_to_perf(self):
        self.have('flamegraph', 'perf')
        self.commands.statuses['flamegraph'] = 1
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertIn('warning: flamegraph failed; falling back\n', result.stderr)
        self.assertEqual([call.name for call in self.commands.of('tee')], ['flamegraph', 'perf'])

    def test_perf_records_with_dwarf_call_graphs_and_keeps_the_first_80_report_lines(self):
        self.have('perf')
        report = b''.join(f'line {number}\n'.encode() for number in range(100))
        self.commands.outputs['perf'] = report
        result = self.invoke()
        self.assertStatus(result, 0)
        record = self.commands.of('tee')[0]
        self.assertEqual(record.command[:9], ['perf', 'record', '--call-graph', 'dwarf', '-F', '999', '-o',
                                              f'{self.scenario.profile}/perf.data', '--'])
        reading = self.commands.of('output', 'perf')[0]
        self.assertEqual(reading.command, ['perf', 'report', '--stdio', '-i', f'{self.scenario.profile}/perf.data',
                                           '--no-children', '--sort', 'overhead'])
        kept = (self.scenario.profile / 'perf-report.txt').read_bytes()
        self.assertEqual(kept, b''.join(f'line {number}\n'.encode() for number in range(80)))
        self.assertIn(f'perf data: {self.scenario.profile}/perf.data\n', result.stdout)
        self.assertEqual(self.commands.of('pipe'), [])
        self.assertNotIn('flamegraph:', result.stdout)

    def test_inferno_turns_the_perf_script_into_a_flamegraph(self):
        self.have('perf', 'inferno-flamegraph')
        result = self.invoke()
        self.assertStatus(result, 0)
        pipe = self.commands.of('pipe')[0]
        self.assertEqual(pipe.command, ['perf', 'script', '-i', f'{self.scenario.profile}/perf.data'])
        self.assertEqual(pipe.second, ['inferno-flamegraph'])
        self.assertEqual(pipe.destination, f'{self.scenario.profile}/w8-cycle.svg')
        lines = self.lines(result.stdout)
        self.assertLess(lines.index(f'flamegraph: {self.scenario.profile}/w8-cycle.svg'),
                        lines.index(f'perf data: {self.scenario.profile}/perf.data'))

    def test_a_failed_flamegraph_pipeline_ends_the_run(self):
        self.have('perf', 'inferno-flamegraph')
        self.commands.statuses['perf'] = lambda call: 4 if call.kind == 'pipe' else 0
        result = self.invoke()
        self.assertStatus(result, 4)
        self.assertNotIn('perf data:', result.stdout)

    def test_a_failed_perf_report_costs_only_the_report(self):
        self.have('perf')
        self.commands.statuses['perf'] = lambda call: 1 if call.kind == 'output' else 0
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertEqual((self.scenario.profile / 'perf-report.txt').read_bytes(), b'')
        self.assertIn('perf data:', result.stdout)

    def test_a_failed_perf_record_falls_back_to_valgrind(self):
        self.have('perf', 'valgrind')
        self.commands.statuses['perf'] = 1
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertIn('warning: perf record failed; falling back\n', result.stderr)
        self.assertEqual([call.name for call in self.commands.of('tee')], ['perf', 'valgrind'])

    def test_valgrind_runs_callgrind_on_the_worker_with_the_fallback_cycle_count(self):
        self.have('valgrind')
        result = self.invoke()
        self.assertStatus(result, 0)
        launch = self.commands.of('tee')[0]
        out = f'{self.scenario.profile}/callgrind.out'
        self.assertEqual(launch.command[:5], ['valgrind', '--tool=callgrind', '--error-exitcode=0',
                                              '--trace-children=no', f'--callgrind-out-file={out}'])
        self.assertEqual(launch.env['PERF_CYCLES'], FALLBACK_CYCLES)
        self.assertIn('--- valgrind callgrind (no perf_events needed) ---\n', result.stdout)
        self.assertIn(f'callgrind: {out}\n', result.stdout)
        self.assertNotIn('callgrind annotate', result.stdout)

    def test_valgrind_with_a_requested_count(self):
        self.have('valgrind')
        self.env['PERF_CYCLES'] = '7'
        self.invoke()
        self.assertEqual(self.commands.of('tee')[0].env['PERF_CYCLES'], '7')

    def test_callgrind_annotate_writes_its_report_when_there_is_a_profile(self):
        self.have('valgrind', 'callgrind_annotate')
        out = self.scenario.profile / 'callgrind.out'
        self.commands.effects['valgrind'] = lambda call: out.write_text('events: Ir\n')
        self.commands.outputs['callgrind_annotate'] = b'annotated\n'
        result = self.invoke()
        self.assertStatus(result, 0)
        note = self.commands.of('output', 'callgrind_annotate')[0]
        self.assertEqual(note.command, ['callgrind_annotate', '--auto=no', '--inclusive=no', '--threshold=1',
                                        str(out)])
        self.assertEqual((self.scenario.profile / 'callgrind-annotate.txt').read_bytes(), b'annotated\n')
        lines = self.lines(result.stdout)
        self.assertLess(lines.index(f'callgrind annotate: {self.scenario.profile}/callgrind-annotate.txt'),
                        lines.index(f'callgrind: {out}'))

    def test_an_empty_callgrind_profile_is_not_annotated(self):
        self.have('valgrind', 'callgrind_annotate')
        (self.scenario.profile / 'callgrind.out').parent.mkdir(parents=True, exist_ok=True)
        self.commands.effects['valgrind'] = lambda call: (self.scenario.profile / 'callgrind.out').write_text('')
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(self.commands.of('output', 'callgrind_annotate'), [])

    def test_a_failed_callgrind_ends_the_run_with_its_status(self):
        self.have('valgrind')
        self.commands.statuses['valgrind'] = 3
        result = self.invoke()
        self.assertStatus(result, 3)
        self.assertNotIn('callgrind:', result.stdout)
        self.assertNotIn('done.', result.stdout)

    def test_a_failed_record_leaves_the_chain_to_report_everything_missing(self):
        self.have('samply')
        self.commands.statuses['samply'] = 1
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertIn('warning: samply failed (often /proc/sys/kernel/perf_event_paranoid > 1); falling back\n',
                      result.stderr)
        self.assertNotIn('samples:', result.stdout)
        self.assertEqual(self.lines(result.stderr)[-1],
                         'warning: samply, flamegraph, perf, and valgrind all missing; writing timing only')
        self.assertEqual([call.name for call in self.commands.of('tee')], ['samply', f'bisect'])

    def test_one_timing_file_is_rewritten_by_each_attempt(self):
        self.have('samply', 'flamegraph')
        self.commands.statuses['samply'] = 1
        self.invoke()
        self.assertEqual((self.scenario.profile / 'w8-cycle-timing.json').read_text(),
                         f'worker PERF_CYCLES={FALLBACK_CYCLES}\n')


class PinningTests(ScenarioTest):
    tools = ('samply', 'taskset')

    def test_a_requested_cpu_pins_the_profiler_on_linux(self):
        self.env['PERF_CPU'] = '2'
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(self.commands.of('tee')[0].command[:5], ['taskset', '-c', '2', 'samply', 'record'])

    def test_no_requested_cpu_is_no_pin(self):
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(self.commands.of('tee')[0].command[0], 'samply')
        self.env['PERF_CPU'] = ''
        self.commands.calls.clear()
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(self.commands.of('tee')[0].command[0], 'samply')

    def test_without_taskset_the_request_is_not_an_error(self):
        self.scenario.tools.discard('taskset')
        self.env['PERF_CPU'] = '2'
        self.assertStatus(self.invoke(), 0)
        self.assertEqual(self.commands.of('tee')[0].command[0], 'samply')

    def test_every_profiler_is_pinned_the_same_way(self):
        self.scenario.tools.update({'flamegraph', 'perf', 'valgrind'})
        self.env['PERF_CPU'] = '1'
        for name, statuses in (('flamegraph', {}), ('perf', {'flamegraph': 1}),
                               ('valgrind', {'flamegraph': 1, 'perf': 1})):
            with self.subTest(profiler=name):
                self.commands.calls.clear()
                self.commands.statuses = dict(statuses, samply=1)
                self.assertStatus(self.invoke(), 0)
                launch = self.commands.of('tee')[-1]
                self.assertEqual(launch.command[:4], ['taskset', '-c', '1', name])

    def test_the_timing_only_run_is_pinned_too(self):
        self.scenario.tools = {'taskset'}
        self.env['PERF_CPU'] = '3'
        self.assertStatus(self.invoke(), 0)
        launch = self.commands.of('tee')[0]
        self.assertEqual(launch.command[:3], ['taskset', '-c', '3'])
        self.assertEqual(launch.command[3], f'{self.scenario.profile}/bisect')


class SampleTotalTests(ScenarioTest):
    """A samply record reports its sample total, and warns when it is short."""

    def record(self, content, system='Linux', status=0, cycles=None):
        self.scenario.system = system
        self.scenario.profile_bytes = content
        self.commands.statuses['samply'] = status
        if cycles is not None:
            self.env['PERF_CYCLES'] = cycles
        return self.invoke()

    def test_a_short_profile_prints_the_total_and_warns(self):
        result = self.record(profile_gz(1))
        self.assertStatus(result, 0)
        self.assertIn(f'samples: 1 (fewer than the {default_level()} needed', result.stdout)
        self.assertIn('warning: the profile has 1 of the', result.stderr)
        self.assertEqual(self.lines(result.stdout)[-1], 'done.')
        self.assertEqual(len(self.commands.of('tee')), 1)

    def test_a_profile_at_the_level_prints_the_total_without_a_warning(self):
        level = default_level()
        result = self.record(profile_gz(level))
        self.assertStatus(result, 0)
        self.assertIn(f'samples: {level} (at least the {level} needed', result.stdout)
        self.assertNotIn('needed to rank hot stacks; their order', result.stderr)

    def test_an_unreadable_profile_warns_and_keeps_the_run(self):
        result = self.record(b'not gzip')
        self.assertStatus(result, 0)
        profile_json = f'{self.scenario.profile}/w8-cycle.profile.json.gz'
        self.assertIn(f'warning: could not read the sample total from {profile_json}', result.stderr)
        self.assertEqual(self.lines(result.stdout)[-1], 'done.')

    def test_a_failed_record_on_linux_reports_no_total(self):
        result = self.record(profile_gz(1), status=1)
        self.assertStatus(result, 0)
        self.assertNotIn('samples:', result.stdout)

    def test_a_failed_record_on_darwin_reports_no_total_and_fails(self):
        self.env['OXPINYIN_TKRZW_PREFIX'] = '/opt/tkrzw'
        result = self.record(profile_gz(1), system='Darwin', status=1)
        self.assertStatus(result, 1)
        self.assertNotIn('samples:', result.stdout)

    def test_samply_gets_its_own_default_cycle_count(self):
        self.record(profile_gz(1))
        self.assertEqual(self.commands.of('tee', 'samply')[0].env['PERF_CYCLES'], SAMPLY_CYCLES)
        self.assertEqual((self.scenario.profile / 'w8-cycle-timing.json').read_text(),
                         f'worker PERF_CYCLES={SAMPLY_CYCLES}\n')

    def test_samply_gets_the_requested_cycle_count(self):
        self.record(profile_gz(1), cycles='7')
        self.assertEqual(self.commands.of('tee', 'samply')[0].env['PERF_CYCLES'], '7')

    def test_the_timing_is_saved_and_printed(self):
        result = self.record(profile_gz(1))
        self.assertIn(f'worker PERF_CYCLES={SAMPLY_CYCLES}\n', result.stdout)
        self.assertEqual((self.scenario.profile / 'w8-cycle-timing.json').read_text(),
                         f'worker PERF_CYCLES={SAMPLY_CYCLES}\n')

    def test_the_profile_path_and_sidecar_are_announced(self):
        result = self.record(profile_gz(1))
        profile_json = f'{self.scenario.profile}/w8-cycle.profile.json.gz'
        lines = self.lines(result.stdout)
        self.assertIn(f'samply profile: {profile_json}', lines)
        self.assertIn(f'samply symbols: {self.scenario.profile}/w8-cycle.profile.json.syms.json', lines)
        self.assertIn('--- samply record (preferred) ---', lines)

    def test_a_record_without_a_sidecar_warns_that_frames_stay_unresolved(self):
        self.scenario.sidecar = False
        result = self.record(profile_gz(1))
        self.assertIn('warning: samply wrote no symbol sidecar; the hot-stack report will show unresolved frames.',
                      result.stderr)
        self.assertNotIn('samply symbols:', result.stdout)

    def test_a_stale_profile_and_sidecar_are_removed_before_the_record(self):
        self.scenario.profile.mkdir(parents=True)
        stale = [self.scenario.profile / 'w8-cycle.profile.json.gz',
                 self.scenario.profile / 'w8-cycle.profile.json.syms.json']
        for path in stale:
            path.write_text('stale')
        self.scenario.sidecar = False
        self.commands.effects['samply'] = lambda call: None
        result = self.invoke()
        self.assertStatus(result, 0)
        self.assertFalse(any(path.exists() for path in stale))
        self.assertIn('could not read the sample total', result.stderr)

    def test_without_presymbolicate_the_flag_is_not_passed(self):
        self.commands.samply_help = HELP_WITHOUT_FLAG
        self.assertStatus(self.invoke(), 0)
        self.assertNotIn('--unstable-presymbolicate', self.commands.of('tee', 'samply')[0].command)


class HeadTests(unittest.TestCase):
    def test_head_keeps_whole_lines(self):
        self.assertEqual(profile.head(b'a\nb\nc\n', 2), b'a\nb\n')
        self.assertEqual(profile.head(b'a\nb\nc\n', 3), b'a\nb\nc\n')
        self.assertEqual(profile.head(b'a\nb\nc\n', 80), b'a\nb\nc\n')
        self.assertEqual(profile.head(b'a\nb\nc', 80), b'a\nb\nc')
        self.assertEqual(profile.head(b'', 80), b'')
        self.assertEqual(profile.head(b'a\r\nb\rc\n', 1), b'a\r\n')


class HostTests(unittest.TestCase):
    def test_the_host_name_comes_from_the_override_then_the_machine(self):
        with mock.patch.object(profile.platform, 'system', return_value='Darwin'):
            self.assertTrue(profile.W8([], {}).darwin)
            self.assertFalse(profile.W8([], {'OXPINYIN_PROFILE_OS': 'Linux'}).darwin)
            self.assertTrue(profile.W8([], {'OXPINYIN_PROFILE_OS': ''}).darwin)
        with mock.patch.object(profile.platform, 'system', return_value='Linux'):
            self.assertFalse(profile.W8([], {}).darwin)
            self.assertTrue(profile.W8([], {'OXPINYIN_PROFILE_OS': 'Darwin'}).darwin)
            self.assertEqual(profile.W8([], {}, system='Darwin').system, 'Darwin')

    def test_any_other_name_takes_the_linux_path(self):
        self.assertFalse(profile.W8([], {'OXPINYIN_PROFILE_OS': 'FreeBSD'}).darwin)

    def test_the_run_starts_in_tools_profile(self):
        directories = []
        scenario = Scenario(self)
        scenario.commands.effects['samply'] = lambda call: directories.append(os.getcwd())
        before = os.getcwd()
        self.assertEqual(scenario.invoke(['--check']).status, 0)
        scenario.commands.calls.clear()
        scenario.invoke()
        self.assertEqual(os.getcwd(), before)
        self.assertTrue(directories and all(Path(path).samefile(ROOT / 'tools/profile') for path in directories))

    def test_a_relative_directory_resolves_against_tools_profile(self):
        scenario = Scenario(self)
        relative = Path(os.path.relpath(scenario.profile, ROOT / 'tools/profile'))
        scenario.env['OXPINYIN_PROFILE_DIR'] = str(relative)
        self.assertEqual(scenario.invoke().status, 0)
        self.assertTrue((scenario.profile / 'header.txt').is_file())

    def test_the_recipe_is_registered(self):
        self.assertEqual(sorted(profile.RECIPES), ['profile-w8'])


def stub_tool(directory, name, body):
    """A program that is a shell script, so a real start of it works on either OS."""
    path = Path(directory) / name
    path.write_text('#!/bin/sh\n' + body + '\n')
    path.chmod(0o755)
    return path


class LauncherTests(unittest.TestCase):
    """The real launcher: its streams, statuses and process-group cleanup."""

    def setUp(self):
        self.directory = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.commands = profile.Commands()
        self.env = {'PATH': f'{self.directory}:/usr/bin:/bin'}

    def test_a_missing_program_probes_as_127_and_says_nothing(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            self.assertEqual(self.commands.probe(['no-such-program-w8'], self.env), (127, ''))
        self.assertEqual(err.getvalue(), '')

    def test_a_probe_merges_the_two_streams(self):
        stub_tool(self.directory, 'tool', 'echo out; echo err >&2; exit 3')
        status, text = self.commands.probe(['tool'], self.env)
        self.assertEqual(status, 3)
        self.assertEqual(sorted(text.split()), ['err', 'out'])

    def test_output_returns_stdout_and_the_status(self):
        stub_tool(self.directory, 'tool', 'printf "a\\nb\\n"; exit 5')
        self.assertEqual(self.commands.output(['tool'], self.env), (5, b'a\nb\n'))

    def test_a_missing_program_is_left_to_the_shared_ladder_as_127(self):
        err = io.StringIO()
        with contextlib.redirect_stderr(err):
            status = runners.guarded(lambda: self.commands.output(['no-such-program-w8'], self.env))
        self.assertEqual(status, 127)
        self.assertIn('no-such-program-w8', err.getvalue())

    def test_tee_copies_stdout_to_the_console_and_the_file(self):
        stub_tool(self.directory, 'tool', 'echo one; echo two')
        saved = self.directory / 'saved.json'
        out = io.TextIOWrapper(io.BytesIO(), encoding='utf-8', write_through=True)
        with contextlib.redirect_stdout(out):
            status = self.commands.tee(['tool'], self.env, str(saved))
        self.assertEqual(status, 0)
        self.assertEqual(saved.read_bytes(), b'one\ntwo\n')
        self.assertEqual(out.buffer.getvalue(), b'one\ntwo\n')

    def test_tee_creates_the_file_even_when_the_program_prints_nothing_and_fails(self):
        stub_tool(self.directory, 'tool', 'exit 7')
        saved = self.directory / 'saved.json'
        self.assertEqual(self.commands.tee(['tool'], self.env, str(saved)), 7)
        self.assertEqual(saved.read_bytes(), b'')

    def test_tee_passes_the_environment_to_the_program(self):
        stub_tool(self.directory, 'tool', 'echo "$PERF_CYCLES"')
        saved = self.directory / 'saved.json'
        out = io.TextIOWrapper(io.BytesIO(), encoding='utf-8', write_through=True)
        with contextlib.redirect_stdout(out):
            self.commands.tee(['tool'], {**self.env, 'PERF_CYCLES': '11'}, str(saved))
        self.assertEqual(saved.read_bytes(), b'11\n')

    def test_a_pipe_connects_the_two_programs_and_writes_the_file(self):
        stub_tool(self.directory, 'first', 'printf "hello\\n"')
        stub_tool(self.directory, 'second', 'tr a-z A-Z')
        destination = self.directory / 'result'
        self.assertEqual(self.commands.pipe(['first'], ['second'], self.env, str(destination)), 0)
        self.assertEqual(destination.read_bytes(), b'HELLO\n')

    def test_a_pipe_reports_the_rightmost_nonzero_status(self):
        # No producer reads its stdin: the test's own stdin may never end.
        stub_tool(self.directory, 'make0', 'echo y')
        stub_tool(self.directory, 'make3', 'echo x; exit 3')
        stub_tool(self.directory, 'take0', 'cat >/dev/null')
        stub_tool(self.directory, 'take2', 'cat >/dev/null; exit 2')
        destination = str(self.directory / 'result')
        for first, second, status in (('make0', 'take0', 0), ('make3', 'take0', 3), ('make0', 'take2', 2),
                                      ('make3', 'take2', 2)):
            with self.subTest(first=first, second=second):
                self.assertEqual(self.commands.pipe([first], [second], self.env, destination), status)

    def test_a_cancellation_stops_the_program_and_reaps_its_group(self):
        pidfile = self.directory / 'pid'
        stub_tool(self.directory, 'sleeper', f'echo $$ > {pidfile}; exec sleep 30')
        timer = threading.Timer(0.5, slot.CANCELLED.set)
        timer.start()
        self.addCleanup(slot.CANCELLED.clear)
        self.addCleanup(timer.cancel)
        started = time.monotonic()
        with self.assertRaises(slot.Cancelled):
            self.commands.tee(['sleeper'], self.env, str(self.directory / 'timing'))
        self.assertLess(time.monotonic() - started, 20)
        pid = int(pidfile.read_text())
        with self.assertRaises(ProcessLookupError):
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                os.kill(pid, 0)
                time.sleep(0.05)
            os.kill(pid, 0)


@unittest.skipUnless(
    sys.platform.startswith('linux') and shutil.which('taskset') and hasattr(os, 'sched_getaffinity'),
    'needs taskset (util-linux) and CPU affinity')
class TasksetTests(unittest.TestCase):
    """The pinned launch against the real taskset: the program sees exactly the requested CPU."""

    def test_the_profiler_runs_on_the_requested_cpu(self):
        cpu = min(os.sched_getaffinity(0))
        directory = Path(self.enterContext(tempfile.TemporaryDirectory()))
        stub_tool(directory, 'worker', 'grep Cpus_allowed_list: /proc/self/status')
        env = {'PATH': f'{directory}:/usr/bin:/bin', 'PERF_CPU': str(cpu)}
        run = profile.W8([], env, system='Linux')
        command = run.pinned(['worker'])
        self.assertEqual(command[:3], ['taskset', '-c', str(cpu)])
        status, answer = profile.Commands().output(command, env)
        self.assertEqual(status, 0)
        self.assertEqual(answer.split()[-1], str(cpu).encode())


class StatusTests(unittest.TestCase):
    """Real processes through the real CLI status ladder: streams and statuses as a caller sees them."""

    def setUp(self):
        self.directory = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.bin = self.directory / 'bin'
        self.bin.mkdir()
        # The system tools last, so a developer's samply or cargo is never found by accident.
        self.path = f'{self.bin}:/usr/bin:/bin'

    def stub(self, name, body):
        return stub_tool(self.bin, name, body)

    def samply(self, help_text):
        self.stub('samply', f'echo "{help_text}"')

    def run_recipe(self, *argv, system=None, **environment):
        env = {'PATH': self.path, 'HOME': str(self.directory), **environment}
        if system is not None:
            env['OXPINYIN_PROFILE_OS'] = system
        code = ('import os, sys; sys.path.insert(0, sys.argv[1]); '
                'from tools.dev import runners; from tools.dev.measure import profile; '
                'sys.exit(runners.guarded(lambda: profile.profile_w8(sys.argv[2:], os.environ)))')
        return subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', code, str(ROOT), *argv],
                              env=env, capture_output=True, text=True, timeout=120)

    def test_the_check_reports_a_stub_samply_on_both_host_names(self):
        self.samply('record --unstable-presymbolicate')
        for system in ('Darwin', 'Linux'):
            with self.subTest(system=system):
                result = self.run_recipe('--check', system=system)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue(result.stdout.startswith(f'samply cycles: {SAMPLY_CYCLES}\n'
                                                         'samply presymbolicate: available\n'))
                self.assertEqual(result.stdout.splitlines()[-1], 'profile platform: Darwin; profiler=samply'
                                 if system == 'Darwin' else f'fallback cycles: {FALLBACK_CYCLES}')

    def test_a_stub_samply_without_the_flag_warns(self):
        self.samply('record')
        result = self.run_recipe('--check', system='Linux')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('samply presymbolicate: unavailable\n', result.stdout)
        self.assertEqual(result.stderr,
                         'warning: samply has no --unstable-presymbolicate; the hot-stack report '
                         'will show unresolved frames.\n')

    def test_darwin_refuses_perf_cpu_with_status_2(self):
        result = self.run_recipe(system='Darwin', PERF_CPU='0')
        self.assertEqual(result.returncode, 2)
        self.assertIn('PERF_CPU is unsupported on macOS', result.stderr)
        self.assertIn('Mach affinity is a cache hint, not CPU pinning', result.stderr)

    def test_darwin_refuses_a_path_without_samply(self):
        result = self.run_recipe(system='Darwin')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('samply is required on macOS', result.stderr)

    def test_darwin_refuses_a_path_without_cargo_c_before_reading_inputs(self):
        self.samply('record')
        result = self.run_recipe(system='Darwin', PINYIN_EXPORT_DIR=str(self.directory / 'absent'))
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('cargo-c is required on macOS', result.stderr)

    def test_a_cargo_that_cannot_cinstall_is_the_same_refusal(self):
        self.samply('record')
        self.stub('cargo', 'exit 101')
        result = self.run_recipe(system='Darwin')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('cargo-c is required on macOS', result.stderr)

    def test_a_cargo_that_can_cinstall_reaches_the_tkrzw_refusal(self):
        self.samply('record')
        self.stub('cargo', 'exit 0')
        result = self.run_recipe(system='Darwin')
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('set OXPINYIN_TKRZW_PREFIX or OXPINYIN_TKRZW_LIBRARY', result.stderr)

    def test_a_missing_export_is_refused_with_status_2(self):
        result = self.run_recipe(system='Linux', PINYIN_EXPORT_DIR=str(self.directory / 'absent'),
                                 OXPINYIN_PROFILE_DIR=str(self.directory / 'profile'))
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(result.stderr.splitlines()[0],
                         f'grep: {self.directory}/absent/datagen-manifest.txt: No such file or directory')
        self.assertTrue((self.directory / 'profile').is_dir())


if __name__ == '__main__':
    unittest.main()
