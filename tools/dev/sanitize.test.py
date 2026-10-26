#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Sanitizer selection, backend flags and host-runtime fixtures."""
import io
import os
from pathlib import Path
import subprocess
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import native, test_runner


class FakeTools:
    """Record each child launch; answer the toolchain and runtime questions."""

    def __init__(self, *, nightly='nightly-2026-09-07-x86_64-unknown-linux-gnu',
                 host_library=None):
        self.calls = []
        self.checks = []
        self.nightly = nightly
        self.host_library = host_library

    def run(self, argv, *, environment=None, cwd=None, input=None, text=True,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE):
        argv = [str(part) for part in argv]
        self.calls.append((argv, dict(environment or {})))
        output = ''
        if argv[:3] == ['rustup', 'toolchain', 'list'] and self.nightly:
            output = self.nightly + '\n'
        elif argv[0] == 'rustc':
            output = 'host: x86_64-unknown-linux-gnu\n'
        elif '--no-run' in argv:
            output = '{"reason":"compiler-artifact","executable":"/fixture/test-binary"}\n'
        return subprocess.CompletedProcess(argv, 0, output, '')

    def tkrzw(self, obj, *, runner, side, cwd, environment=None):
        self.checks.append((Path(obj), side))
        return Path('/fixture/libtkrzw.so.1') if self.host_library else None

    def tkrzw_host(self, *, environment=None):
        if self.host_library is None:
            raise native.NativeRefused('no verified host')
        return Path(self.host_library)


def run(*args, host='Darwin', arch='arm64', tools=None, environment=None):
    out, err = io.StringIO(), io.StringIO()
    env = dict(os.environ, OXPINYIN_SANITIZER_HOST_OS=host,
               OXPINYIN_SANITIZER_HOST_ARCH=arch)
    env.update(environment or {})
    status = test_runner.sanitize(list(args), out=out, err=err, environment=env,
                                  tools=tools)
    return status, out.getvalue(), err.getvalue()


class SelectionTests(unittest.TestCase):
    def test_default_is_address(self):
        status, out, err = run('--check', 'bdb')
        self.assertEqual((status, err), (0, ''))
        self.assertIn('backend=bdb sanitizer=address host=Darwin/arm64', out)

    def test_aliases_and_thread_selection(self):
        for alias in ('tsan', 'thread'):
            with self.subTest(alias=alias):
                status, out, _ = run('--check', 'bdb', alias)
                self.assertEqual(status, 0)
                self.assertIn('sanitizer=thread', out)

    def test_memory_is_refused_on_darwin_and_linux(self):
        status, _, err = run('bdb', 'memory')
        self.assertEqual(status, 1)
        self.assertIn('MemorySanitizer is unsupported on macOS', err)
        status, _, err = run('bdb', 'memory', host='Linux', arch='x86_64')
        self.assertEqual(status, 1)
        self.assertIn('no in-tree wrapper', err)

    def test_leak_refusals(self):
        status, _, err = run('bdb', 'leak')
        self.assertEqual(status, 1)
        self.assertIn('standalone LeakSanitizer is unsupported on macOS arm64', err)
        for host, arch in (('Darwin', 'arm64'), ('Linux', 'x86_64')):
            with self.subTest(host=host, arch=arch):
                status, _, err = run('tkrzw', 'leak', host=host, arch=arch)
                self.assertEqual(status, 1)
                self.assertIn('tkrzw LeakSanitizer is unsupported', err)
                self.assertIn('process-lifetime allocations', err)
        status, _, err = run('all', 'leak', host='Linux', arch='x86_64')
        self.assertEqual(status, 1)
        self.assertIn('tkrzw LeakSanitizer is unsupported', err)

    def test_invalid_arguments_are_usage_status_two(self):
        for args in (('bdb', 'bogus'), ('bogus',), ('bdb', 'address', 'extra'),
                     ('--help',)):
            with self.subTest(args=args):
                status, out, err = run(*args)
                self.assertEqual((status, out), (2, ''))
                self.assertTrue(err.startswith('usage: sanitize.sh '))

    def test_missing_toolchain_streams_and_allow_skip(self):
        tools = FakeTools(nightly='')
        status, out, err = run('bdb', tools=tools)
        self.assertEqual(status, 1)
        self.assertEqual(out, '')
        self.assertIn('nightly-2026-09-07 is not installed.', err)
        self.assertIn('ERROR: the sanitizer gate cannot run.', err)
        status, out, err = run('kc', tools=tools)
        self.assertEqual(status, 1)
        self.assertIn('nightly-2026-09-07 is not installed.', out)
        self.assertEqual(err, '')
        status, out, err = run('bdb', tools=tools,
                               environment={'OXPINYIN_SANITIZER_ALLOW_SKIP': '1'})
        self.assertEqual(status, 0)
        self.assertIn('SKIP: OXPINYIN_SANITIZER_ALLOW_SKIP is set', err)


class BackendTests(unittest.TestCase):
    def environment(self, **values):
        return {'OXPINYIN_SANITIZER_TARGET': 'x86_64-unknown-linux-gnu',
                'OXPINYIN_SANITIZER_TOOLCHAIN': 'nightly-2026-09-07', **values}

    def test_bdb_runs_the_rust_test_then_the_c_harness(self):
        tools = FakeTools()
        with mock.patch.object(test_runner.platform, 'system', return_value='Darwin'):
            status, out, err = run('bdb', tools=tools,
                                   environment=self.environment(
                                       OXPINYIN_LIBPINYIN_DATA_DIR='/fixture/data',
                                       OXPINYIN_BDB_INCLUDE_DIR='/include',
                                       OXPINYIN_BDB_LIB_DIR='/library'))
        self.assertEqual((status, err), (0, ''), out)
        commands = [call[0] for call in tools.calls]
        test = next(command for command in commands
                    if command[:3] == ['cargo', '+nightly-2026-09-07', 'test'])
        self.assertIn('--features', test)
        self.assertEqual(test[test.index('--features') + 1], 'bdb')
        self.assertTrue(any(command[0] == 'gcc' and '-fsanitize=address,undefined' in command
                            for command in commands))
        test_environment = next(env for command, env in tools.calls
                                if command[:3] == ['cargo', '+nightly-2026-09-07', 'test'])
        self.assertEqual(test_environment['RUSTFLAGS'],
                         '-Zsanitizer=address -Cdebuginfo=1')
        self.assertIn('--- 2/2: the libdb call pattern under ASan + UBSan (C) ---', out)
        self.assertIn('sanitizers: clean', out)

    def test_kc_thread_uses_the_darwin_deadlock_option(self):
        tools = FakeTools()
        with mock.patch.object(test_runner.platform, 'system', return_value='Darwin'):
            status, _, err = run('kc', 'thread', tools=tools,
                                 environment=self.environment())
        self.assertEqual((status, err), (0, ''))
        _, env = next(call for call in tools.calls if call[0][:3] ==
                      ['cargo', '+nightly-2026-09-07', 'test'])
        self.assertEqual(env['TSAN_OPTIONS'], 'detect_deadlocks=0:abort_on_error=1')
        self.assertEqual(env['RUSTFLAGS'],
                         '-Zsanitizer=thread -Cunsafe-allow-abi-mismatch=sanitizer')

    def test_a_command_that_cannot_be_executed_is_status_126(self):
        tools = FakeTools()
        run_child = tools.run

        def refuse_cargo(argv, **streams):
            if str(argv[0]) == 'cargo':
                raise PermissionError(13, 'Permission denied', 'cargo')
            return run_child(argv, **streams)

        tools.run = refuse_cargo
        with mock.patch.object(test_runner.platform, 'system', return_value='Darwin'):
            status, _, err = run('kc', tools=tools, environment=self.environment())
        self.assertEqual(status, 126)
        self.assertIn("cargo: [Errno 13] Permission denied: 'cargo'\n", err)

    def test_tkrzw_checks_every_binary_and_uses_setarch_on_linux(self):
        tools = FakeTools(host_library='/fixture/host/libtkrzw.so.1')
        with mock.patch.object(test_runner.platform, 'system', return_value='Linux'), \
                mock.patch.object(test_runner.platform, 'machine', return_value='x86_64'):
            status, out, err = run('tkrzw', tools=tools,
                                   environment=self.environment(
                                       OXPINYIN_TKRZW_LIBRARY='/fixture/verified/libtkrzw.so.1'))
        self.assertEqual((status, err), (0, ''), out)
        build = next(call[0] for call in tools.calls if '--no-run' in call[0])
        self.assertEqual(build[:4], ['setarch', 'x86_64', '-R', 'cargo'])
        self.assertEqual(tools.checks, [(Path('/fixture/test-binary'), 'subject')])
        self.assertIn('libtkrzw: 1 test binaries resolve it to /fixture/libtkrzw.so.1', out)

    def test_tkrzw_reports_a_missing_verified_runtime(self):
        tools = FakeTools()
        status, _, err = run('tkrzw', tools=tools,
                             environment=self.environment())
        self.assertEqual(status, 1)
        self.assertIn('ERROR: no verified libtkrzw; the sanitizer gate did NOT run.', err)
        self.assertIn("Ubuntu's packaged libtkrzw fails it", err)


if __name__ == '__main__':
    unittest.main()
