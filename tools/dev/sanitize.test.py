"""Sanitizer selection, backend flags and host-runtime fixtures."""
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import native, test_runner

NIGHTLY = test_runner.sanitizer_nightly()


class FakeTools:
    """Record each child launch; answer the toolchain and runtime questions."""

    def __init__(self, *, nightly=f'{NIGHTLY}-x86_64-unknown-linux-gnu',
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
    # Runtime discovery belongs to each fixture, not the invoking shell.
    for name in ('OXPINYIN_TKRZW_LIBRARY', 'OXPINYIN_TKRZW_PREFIX', 'OXPINYIN_TKRZW_PREFIXES'):
        env.pop(name, None)
    env.update(environment or {})
    status = test_runner.sanitize(list(args), out=out, err=err, environment=env,
                                  tools=tools)
    return status, out.getvalue(), err.getvalue()


class ToolchainTests(unittest.TestCase):
    def test_tools_dev_starts_without_the_file_and_only_sanitizer_commands_name_it(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            shutil.copytree(ROOT / 'tools', root / 'tools',
                            ignore=shutil.ignore_patterns('__pycache__'))
            # tools.dev reads the interpreter request when it starts.
            shutil.copyfile(ROOT / '.python-version', root / '.python-version')
            missing = root / 'fuzz' / 'rust-toolchain.toml'
            environment = {name: value for name, value in os.environ.items()
                           if name != 'OXPINYIN_SANITIZER_TOOLCHAIN'}

            def python(*arguments):
                return subprocess.run([sys.executable, '-X', 'utf8', *arguments],
                                      cwd=root, env=environment, capture_output=True,
                                      text=True, timeout=60)

            imported = python('-c', 'from tools.dev import test_runner; '
                                    'print(test_runner.SANITIZER_TOOLCHAIN_FILE)')
            self.assertEqual((imported.returncode, imported.stdout.strip()),
                             (0, str(missing)), imported.stderr)
            # --check never runs a gate.
            result = python('-c', 'import sys; from tools.dev.test_runner import sanitize; '
                                  'raise SystemExit(sanitize(sys.argv[1:]))', '--check', 'bdb')
            self.assertEqual((result.returncode, result.stdout), (1, ''), result.stderr)
            self.assertEqual(result.stderr, f'sanitize: {missing} is missing; it names the sanitizer nightly.\n')


class SelectionTests(unittest.TestCase):
    def test_default_is_address(self):
        status, out, err = run('--check', 'bdb')
        self.assertEqual((status, err), (0, ''))
        self.assertIn('backend=bdb sanitizer=address host=Darwin/arm64', out)

    def test_memory_is_refused_on_darwin_and_linux(self):
        status, _, err = run('bdb', 'memory')
        self.assertEqual(status, 1)
        self.assertIn('MemorySanitizer is unsupported on macOS', err)
        status, _, err = run('bdb', 'memory', host='Linux', arch='x86_64')
        self.assertEqual(status, 1)
        self.assertIn('no in-tree wrapper', err)

    def test_invalid_arguments_are_usage_status_two(self):
        for args in (('bdb', 'bogus'), ('bogus',), ('bdb', 'address', 'extra'),
                     ('--help',)):
            with self.subTest(args=args):
                status, out, err = run(*args)
                self.assertEqual((status, out), (2, ''))
                self.assertTrue(err.startswith('usage: just sanitize '))

    def test_missing_toolchain_streams_and_allow_skip(self):
        tools = FakeTools(nightly='')
        status, out, err = run('bdb', tools=tools)
        self.assertEqual(status, 1)
        self.assertEqual(out, '')
        self.assertIn(f'{NIGHTLY} is not installed.', err)
        self.assertIn('ERROR: the sanitizer gate cannot run.', err)
        status, out, err = run('kc', tools=tools)
        self.assertEqual(status, 1)
        self.assertIn(f'{NIGHTLY} is not installed.', out)
        self.assertEqual(err, '')
        status, out, err = run('bdb', tools=tools,
                               environment={'OXPINYIN_SANITIZER_ALLOW_SKIP': '1'})
        self.assertEqual(status, 0)
        self.assertIn('SKIP: OXPINYIN_SANITIZER_ALLOW_SKIP is set', err)


class BackendTests(unittest.TestCase):
    def environment(self, **values):
        return {'OXPINYIN_SANITIZER_TARGET': 'x86_64-unknown-linux-gnu',
                'OXPINYIN_SANITIZER_TOOLCHAIN': NIGHTLY, **values}

    def test_abi_uses_address_with_leak_detection_on_the_pinned_nightly(self):
        settings = test_runner.cell.Settings.read({'PINYIN_ORACLE_DBM': 'kc'})
        tools = FakeTools()
        environment = {'ASAN_OPTIONS': 'detect_leaks=0', 'CARGO_ENCODED_RUSTFLAGS': '-Cdebuginfo=0'}
        with mock.patch.object(native, 'Tools', return_value=tools), \
                mock.patch.object(test_runner.slot, 'run_command', return_value=0) as launch:
            status = test_runner.sanitize_abi(settings, environment)
        self.assertEqual(status, 0)
        command, = launch.call_args.args
        env = launch.call_args.kwargs['env']
        self.assertEqual(command[:4], ['cargo', f'+{NIGHTLY}', 'nextest', 'run'])
        self.assertEqual(command[command.index('--test') + 1], 'abi')
        self.assertNotIn('CARGO_ENCODED_RUSTFLAGS', env)
        self.assertIn('-Zsanitizer=address', env['RUSTFLAGS'].split())
        self.assertIn('detect_leaks=1', env['ASAN_OPTIONS'].split(':'))

    def test_bdb_runs_the_rust_test_alone(self):
        tools = FakeTools()
        with mock.patch.object(test_runner.platform, 'system', return_value='Darwin'):
            status, out, err = run('bdb', tools=tools,
                                   environment=self.environment(
                                       OXPINYIN_LIBPINYIN_DATA_DIR='/fixture/data',
                                       SYSTEM_DEPS_BDB_INCLUDE='/include',
                                       SYSTEM_DEPS_BDB_SEARCH_NATIVE='/library'))
        self.assertEqual((status, err), (0, ''), out)
        commands = [call[0] for call in tools.calls]
        test = next(command for command in commands
                    if command[:4] == ['cargo', f'+{NIGHTLY}', 'nextest', 'run'])
        self.assertIn('--features', test)
        self.assertEqual(test[test.index('--features') + 1], 'bdb')
        self.assertFalse(any(command[0] == 'gcc' for command in commands))
        test_environment = next(env for command, env in tools.calls
                                if command[:4] == ['cargo', f'+{NIGHTLY}', 'nextest', 'run'])
        self.assertEqual(test_environment['RUSTFLAGS'],
                         '-Zsanitizer=address -Cdebuginfo=1')
        self.assertIn('sanitizers: clean', out)

    def test_kc_thread_uses_the_darwin_deadlock_option(self):
        tools = FakeTools()
        with mock.patch.object(test_runner.platform, 'system', return_value='Darwin'):
            status, _, err = run('kc', 'thread', tools=tools,
                                 environment=self.environment())
        self.assertEqual((status, err), (0, ''))
        _, env = next(call for call in tools.calls if call[0][:4] ==
                      ['cargo', f'+{NIGHTLY}', 'nextest', 'run'])
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

    def test_tkrzw_checks_every_binary_against_the_homebrew_library(self):
        # The tkrzw gate is a macOS gate: Linux runs no tkrzw code.
        tools = FakeTools(host_library='/fixture/host/libtkrzw.dylib')
        with mock.patch.object(test_runner.platform, 'system', return_value='Darwin'), \
                mock.patch.object(test_runner.platform, 'machine', return_value='arm64'):
            status, out, err = run('tkrzw', tools=tools, environment=self.environment())
        self.assertEqual((status, err), (0, ''), out)
        build = next(call[0] for call in tools.calls if '--no-run' in call[0])
        self.assertEqual(build[:3], ['cargo', f'+{NIGHTLY}', 'test'])
        test = next(call[0] for call in tools.calls
                    if call[0][:4] == ['cargo', f'+{NIGHTLY}', 'nextest', 'run']
                    and 'oxpinyin' in call[0])
        self.assertEqual(test[4:], [*build[3:-2], '-E', 'test(/^store::/)'])
        self.assertEqual(tools.checks, [(Path('/fixture/test-binary'), 'subject')])
        self.assertIn('libtkrzw: 1 test binaries resolve it to ' + str(Path('/fixture/libtkrzw.so.1')), out)
        lifetime, environment = next(call for call in tools.calls if 'test(outlives_context)' in call[0])
        self.assertEqual(lifetime[:4], ['cargo', f'+{NIGHTLY}', 'nextest', 'run'])
        self.assertIn('oxpinyin-capi', lifetime)
        self.assertIn('oxzhuyin-capi', lifetime)
        self.assertEqual(lifetime[-3:], ['--lib', '-E', 'test(outlives_context)'])
        self.assertIn('-Zsanitizer=address', environment['RUSTFLAGS'])
        self.assertEqual(environment['OXPINYIN_TKRZW_LIBRARY'], str(Path('/fixture/host/libtkrzw.dylib')))

    def test_every_gate_runs_its_tests_under_nextest(self):
        cases = (('bdb', 'address', 'Darwin', {'SYSTEM_DEPS_BDB_INCLUDE': '/include'}),
                 ('kc', 'thread', 'Darwin', {}),
                 ('tkrzw', 'address', 'Darwin', {}))
        for backend, sanitizer, host, values in cases:
            with self.subTest(backend=backend):
                tools = FakeTools(host_library='/fixture/host/libtkrzw.so.1')
                with mock.patch.object(test_runner.platform, 'system', return_value=host), \
                        mock.patch.object(test_runner.platform, 'machine', return_value='x86_64'):
                    status, out, err = run(backend, sanitizer, tools=tools,
                                           environment=self.environment(**values))
                self.assertEqual((status, err), (0, ''), out)
                cargo = [call[0][call[0].index('cargo'):] for call in tools.calls
                         if 'cargo' in call[0]]
                plain = [command for command in cargo
                         if command[2:3] == ['test'] and '--no-run' not in command]
                self.assertEqual(plain, [])
                self.assertTrue(any(command[2:4] == ['nextest', 'run'] for command in cargo))

    def test_tkrzw_reports_a_missing_verified_runtime(self):
        tools = FakeTools()
        with mock.patch.object(test_runner.platform, 'system', return_value='Darwin'):
            status, _, err = run('tkrzw', host='Darwin', tools=tools, environment=self.environment())
        self.assertEqual(status, 1)
        self.assertIn('ERROR: no verified libtkrzw; the sanitizer gate did NOT run.', err)
        self.assertIn('Install tkrzw with Homebrew.', err)

    def test_no_backend_selects_the_hosts_cell(self):
        from tools.dev import cell
        for host, selected in (('Linux', 'kc'), ('Darwin', 'tkrzw')):
            with self.subTest(host=host), mock.patch.object(cell.platform, 'system', return_value=host):
                status, out, err = run('--check', '', host=host)
                self.assertEqual((status, err), (0, ''), out)
                self.assertIn(f'sanitize: check backend={selected} ', out)
                # A named backend stays as named.
                status, out, _ = run('--check', 'bdb', host=host)
                self.assertEqual(status, 0)
                self.assertIn('sanitize: check backend=bdb ', out)

    def test_tkrzw_is_no_linux_gate(self):
        # Linux runs no tkrzw code (tools/dev/cell.py): the gate is refused, and `all` means the Linux cell.
        tools = FakeTools(host_library='/fixture/host/libtkrzw.so.1')
        with mock.patch.object(test_runner.platform, 'system', return_value='Linux'), \
                mock.patch.object(test_runner.platform, 'machine', return_value='x86_64'):
            status, out, err = run('tkrzw', host='Linux', tools=tools,
                                   environment=self.environment(OXPINYIN_TKRZW_LIBRARY='/fixture/libtkrzw.so.1'))
            self.assertEqual(status, 1)
            self.assertIn('sanitize: tkrzw is no Linux cell: Linux uses Kyoto Cabinet', err)
            self.assertEqual(tools.calls, [])
            status, out, err = run('all', host='Linux', tools=tools, environment=self.environment())
        self.assertEqual((status, err), (0, ''), out)
        self.assertEqual([line for line in out.splitlines() if line.startswith('=== sanitize ')],
                         ['=== sanitize kc ==='])

    def test_macos_tkrzw_ignores_peer_link_search_paths(self):
        tools = FakeTools(host_library='/fixture/host/libtkrzw.dylib')
        with mock.patch.object(test_runner.platform, 'system', return_value='Darwin'):
            status, _, err = run('tkrzw', host='Darwin', tools=tools,
                                 environment=self.environment(LIBRARY_PATH='/peer/lib', CPATH='/peer/include',
                                                              PKG_CONFIG_PATH='/overlay/pkgconfig',
                                                              OXPINYIN_TKRZW_PREFIX='/overlay'))
        self.assertEqual((status, err), (0, ''))
        cargo = [env for command, env in tools.calls if 'cargo' in command]
        self.assertTrue(cargo)
        for env in cargo:
            for name in ('LIBRARY_PATH', 'CPATH', 'PKG_CONFIG_PATH', 'OXPINYIN_TKRZW_PREFIX'):
                self.assertNotIn(name, env)


if __name__ == '__main__':
    unittest.main()
