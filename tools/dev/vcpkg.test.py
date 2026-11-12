#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Development environment selection must match runtime certification, and CI delegates vcpkg to Python."""
import contextlib
import hashlib
import io
import os
import re
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import vcpkg

WINDOWS = 'x64-windows'
MACOS = 'arm64-osx-oxpinyin'
LINUX = 'x64-linux-oxpinyin'
PACKAGES = 'glib:x64-windows pkgconf:x64-windows tkrzw:x64-windows'


def git(status, stdout=b''):
    return lambda command: SimpleNamespace(status=status, stdout=stdout)


def tools(command):
    """Answer the two probes of a Linux key: the checkout revision and the compiler."""
    return SimpleNamespace(status=0, stdout=b'abc123\n' if command[0] == 'git' else b'c++ (Debian 15.2.0) 15.2.0\n')


def job(name):
    """One job of the nightly workflow, without its comment lines."""
    text = (ROOT / '.github/workflows/verify-nightly.yml').read_text(encoding='utf-8')
    body = re.split(r'(?m)^  [a-z][\w-]*:\n', text[text.index(f'\n  {name}:\n') + 1:])[1]
    return ''.join(line for line in body.splitlines(keepends=True) if not line.lstrip().startswith('#'))


class RunnerCheckout(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(os.path.realpath(directory.name)) / 'vcpkg'
        (self.root / 'versions').mkdir(parents=True)
        (self.root / 'versions/baseline.json').write_text('{"default": {}}')
        self.program = self.root / ('vcpkg.exe' if os.name == 'nt' else 'vcpkg')
        self.program.write_text('#!/bin/sh\n')
        self.program.chmod(0o755)
        self.env = {'ImageOS': 'win25', 'ImageVersion': '20261111.1', 'RUNNER_TEMP': str(self.root.parent / 'tmp'),
                    'VCPKG_INSTALLATION_ROOT': str(self.root)}

    def test_named_root_wins_and_path_lookup_follows_symlinks(self):
        self.assertEqual(vcpkg.runner_checkout({'VCPKG_INSTALLATION_ROOT': str(self.root)}, which=self.fail), self.root)
        link = self.root.parent / 'bin' / self.program.name
        link.parent.mkdir()
        try:
            link.symlink_to(self.program)
        except OSError:
            self.skipTest('this host cannot create a symbolic link')
        self.assertEqual(vcpkg.runner_checkout({'PATH': str(link.parent)}), self.root)

    def test_missing_checkout_is_refused(self):
        with self.assertRaisesRegex(ValueError, 'not on PATH'):
            vcpkg.runner_checkout({}, which=lambda name, path=None: None)
        self.program.unlink()
        with self.assertRaisesRegex(ValueError, 'no vcpkg checkout at'):
            vcpkg.runner_checkout({'VCPKG_INSTALLATION_ROOT': str(self.root)})
        with self.assertRaisesRegex(ValueError, 'no vcpkg checkout at'):
            vcpkg.runner_install({'VCPKG_ROOT': str(self.root)}, WINDOWS, run=self.fail)

    def test_windows_key_names_triplet_revision_image_and_packages(self):
        selected = vcpkg.binary_cache(self.env, WINDOWS, PACKAGES, capture=git(0, b'abc123\n'))
        cache = (self.root.parent / 'tmp/vcpkg-binary-cache').as_posix()
        self.assertEqual(selected, {
            'environment': {'VCPKG_ROOT': self.root.as_posix(),
                            'VCPKG_BINARY_SOURCES': f'clear;files,{cache},readwrite'},
            'outputs': {'dir': cache, 'prefix': 'vcpkg-bin-win25-x64-windows-',
                        'key': 'vcpkg-bin-win25-x64-windows-abc123-win25-20261111.1',
                        'installed': (self.root / 'installed' / WINDOWS).as_posix(),
                        'packages': hashlib.sha256(PACKAGES.encode()).hexdigest()[:16]}})
        self.assertNotIn('\\', cache + selected['environment']['VCPKG_ROOT'])
        other = vcpkg.binary_cache(self.env, WINDOWS, PACKAGES + ' zlib:x64-windows', capture=git(0, b'abc123\n'))
        self.assertNotEqual(other['outputs']['packages'], selected['outputs']['packages'])
        self.assertNotIn('packages', vcpkg.binary_cache(self.env, WINDOWS, capture=git(0, b'abc123\n'))['outputs'])

    def test_macos_key_also_names_the_checkout_path(self):
        env = {**self.env, 'ImageOS': 'macos26'}
        selected = vcpkg.binary_cache(env, MACOS, capture=git(0, b'abc123\n'))
        root = hashlib.sha256(self.root.as_posix().encode()).hexdigest()[:16]
        self.assertEqual(selected['outputs'], {
            'dir': (self.root.parent / 'tmp/vcpkg-binary-cache').as_posix(),
            'prefix': f'vcpkg-bin-macos26-{MACOS}-{root}-',
            'key': f'vcpkg-bin-macos26-{MACOS}-{root}-abc123-macos26-20261111.1',
            'installed': (self.root / 'installed' / MACOS).as_posix()})
        other = self.root.parent / 'other'
        self.root.rename(other)
        moved = vcpkg.binary_cache({**env, 'VCPKG_INSTALLATION_ROOT': str(other)}, MACOS, capture=git(0, b'abc123\n'))
        self.assertNotEqual(moved['outputs']['prefix'], selected['outputs']['prefix'])

    def test_macos_install_uses_the_overlays_and_checks_the_installed_library(self):
        calls = []
        env = {'VCPKG_ROOT': str(self.root), 'PKG_CONFIG_PATH': '/opt/homebrew/lib/pkgconfig'}
        result = vcpkg.runner_install(env, MACOS,
                                      run=lambda command, **options: calls.append((list(map(str, command)), options)))
        installed = self.root / 'installed' / MACOS
        own = (installed / 'lib/pkgconfig').as_posix()
        self.assertEqual(result, {
            'environment': {'PKG_CONFIG_PATH': own + os.pathsep + '/opt/homebrew/lib/pkgconfig'},
            'path': [(installed / 'tools/tkrzw/bin').as_posix()]})
        self.assertEqual([command for command, _ in calls], [
            [str(self.program), 'install', f'tkrzw:{MACOS}', f'--overlay-ports={ROOT / "vcpkg-overlays"}',
             f'--overlay-triplets={ROOT / "triplets"}'],
            ['pkg-config', '--modversion', '--cflags', '--libs', 'tkrzw'],
            ['otool', '-D', str(installed / 'lib/libtkrzw.dylib')]])
        self.assertEqual(calls[1][1]['env']['PKG_CONFIG_PATH'], own)
        self.assertEqual(vcpkg.runner_install({'VCPKG_ROOT': str(self.root)}, MACOS,
                                              run=lambda command, **options: None)['environment'],
                         {'PKG_CONFIG_PATH': own})

    def test_linux_key_names_the_container_toolchain_and_checkout_not_the_runner_image(self):
        release = self.root.parent / 'os-release'
        release.write_text('ID=debian\nVERSION_CODENAME=forky\n')
        env = {'RUNNER_TEMP': self.env['RUNNER_TEMP'], 'VCPKG_ROOT': str(self.root)}
        selected = vcpkg.binary_cache(env, LINUX, capture=tools, os_release=release)
        identity = hashlib.sha256(b'c++ (Debian 15.2.0) 15.2.0\n' + release.read_bytes()
                                  + (self.root.as_posix() + '\n').encode()).hexdigest()[:16]
        self.assertEqual(selected['outputs'], {
            'dir': (self.root.parent / 'tmp/vcpkg-binary-cache').as_posix(),
            'prefix': f'vcpkg-bin-Linux-{LINUX}-', 'key': f'vcpkg-bin-Linux-{LINUX}-abc123-{identity}',
            'installed': (self.root / 'installed' / LINUX).as_posix()})
        self.assertEqual(selected['environment']['VCPKG_ROOT'], self.root.as_posix())
        release.write_text('ID=debian\nVERSION_CODENAME=next\n')
        self.assertNotEqual(vcpkg.binary_cache(env, LINUX, capture=tools, os_release=release)['outputs']['key'],
                            selected['outputs']['key'])
        with self.assertRaisesRegex(ValueError, 'c\\+\\+ --version failed'):
            vcpkg.binary_cache(env, LINUX, os_release=release, capture=lambda command: SimpleNamespace(
                status=0 if command[0] == 'git' else 127, stdout=b'abc123\n'))
        with self.assertRaisesRegex(ValueError, 'RUNNER_TEMP'):
            vcpkg.binary_cache({'VCPKG_ROOT': str(self.root)}, LINUX, capture=tools, os_release=release)

    def test_linux_install_publishes_the_developer_selection(self):
        calls = []
        env = {'VCPKG_ROOT': str(self.root), 'PKG_CONFIG_PATH': '/usr/lib/pkgconfig',
               'OXPINYIN_TKRZW_LIBRARY': '/old/libtkrzw.so.1'}
        result = vcpkg.runner_install(env, LINUX,
                                      run=lambda command, **options: calls.append((list(map(str, command)), options)))
        installed = self.root / 'installed' / LINUX
        self.assertEqual([command for command, _ in calls], [
            [str(self.program), 'install', f'tkrzw:{LINUX}', f'--overlay-ports={ROOT / "vcpkg-overlays"}',
             f'--overlay-triplets={ROOT / "triplets"}'],
            ['pkg-config', '--modversion', '--cflags', '--libs', 'tkrzw']])
        self.assertEqual(calls[1][1]['env']['PKG_CONFIG_PATH'], (installed / 'lib/pkgconfig').as_posix())
        selected = vcpkg.installed_environment(env, installed)
        self.assertEqual(result, {
            'environment': {'PKG_CONFIG_PATH': str(installed / 'lib/pkgconfig') + os.pathsep + '/usr/lib/pkgconfig',
                            'LD_LIBRARY_PATH': str(installed / 'lib'), 'OXPINYIN_TKRZW_PREFIX': str(installed)},
            'path': [(installed / 'tools/tkrzw/bin').as_posix()]})
        self.assertEqual(result['environment']['PKG_CONFIG_PATH'], selected['PKG_CONFIG_PATH'])

    def test_bootstrap_fetches_the_pinned_revision_and_publishes_the_root(self):
        calls = []
        files = {name: self.root.parent / name for name in ('GITHUB_ENV',)}
        env = {'RUNNER_TEMP': str(self.root.parent / 'runner'), 'GITHUB_ENV': str(files['GITHUB_ENV'])}
        status = vcpkg.main(['bootstrap'], env, run=lambda command, **options: calls.append((command, options)))
        self.assertEqual(status, 0)
        destination = self.root.parent / 'runner/vcpkg'
        self.assertEqual([command for command, _ in calls], [
            ['git', 'init', str(destination)],
            ['git', 'fetch', '--depth=1', 'https://github.com/microsoft/vcpkg.git', vcpkg.REVISION],
            ['git', 'checkout', '--detach', 'FETCH_HEAD'],
            ['bash', str(destination / 'bootstrap-vcpkg.sh'), '-disableMetrics']])
        self.assertEqual([options['cwd'] for _, options in calls], [ROOT, destination, destination, destination])
        self.assertEqual(files['GITHUB_ENV'].read_text(), f'VCPKG_ROOT={destination.as_posix()}\n')
        with contextlib.redirect_stderr(io.StringIO()) as errors:
            self.assertEqual(vcpkg.main(['bootstrap'], {}, run=self.fail), 1)
        self.assertIn('::error::runner variable RUNNER_TEMP is unset', errors.getvalue())

    def test_checkout_without_git_metadata_is_keyed_on_its_baseline(self):
        digest = hashlib.sha256(b'{"default": {}}').hexdigest()[:40]
        key = vcpkg.binary_cache(self.env, WINDOWS, capture=git(128))['outputs']['key']
        self.assertIn(f'-baseline-{digest}-win25-', key)

    def test_unknown_runner_image_is_refused(self):
        for name in ('ImageOS', 'ImageVersion', 'RUNNER_TEMP'):
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, name):
                vcpkg.binary_cache({**self.env, name: ''}, WINDOWS, capture=git(0, b'abc123\n'))

    def test_windows_install_uses_the_overlay_and_checks_with_its_own_pkgconf(self):
        calls = []
        env = {'VCPKG_ROOT': str(self.root), 'PKG_CONFIG_PATH': '/elsewhere'}
        result = vcpkg.runner_install(env, WINDOWS, PACKAGES,
                                      run=lambda command, **options: calls.append((list(map(str, command)), options)))
        installed = self.root / 'installed' / WINDOWS
        own = (installed / 'lib/pkgconfig').as_posix()
        triplets = [f'--overlay-triplets={ROOT / "triplets"}']
        self.assertEqual([command for command, _ in calls], [
            [str(self.program), 'install', *PACKAGES.split(), f'--overlay-ports={ROOT / "vcpkg-overlays"}', *triplets],
            [str(installed / 'tools/pkgconf/pkgconf.exe'), '--modversion', '--cflags', '--libs', 'glib-2.0', 'tkrzw']])
        self.assertEqual(calls[1][1]['env']['PKG_CONFIG_PATH'], own)
        # The job's pkg-config reads this tree alone, through vcpkg's own pkgconf.
        self.assertEqual(result, {
            'environment': {'PKG_CONFIG': (installed / 'tools/pkgconf/pkgconf.exe').as_posix(), 'PKG_CONFIG_PATH': own},
            'path': [(installed / 'bin').as_posix()]})
        with self.assertRaisesRegex(ValueError, 'VCPKG_ROOT'):
            vcpkg.runner_install({}, WINDOWS, run=self.fail)
        calls.clear()
        with self.assertRaisesRegex(ValueError, 'no install rule'):
            vcpkg.runner_install(env, 'riscv64-plan9', run=lambda command, **options: calls.append(command))
        self.assertEqual(len(calls), 1)

    def test_commands_publish_to_the_runner_files(self):
        files = {name: self.root.parent / name for name in ('GITHUB_ENV', 'GITHUB_OUTPUT', 'GITHUB_PATH')}
        env = {**self.env, 'PATH': os.environ['PATH'], **{name: str(path) for name, path in files.items()}}
        arguments = ['cache-key', '--triplet', WINDOWS, '--packages', PACKAGES]
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(vcpkg.main(arguments, env), 0)
        self.assertIn('vcpkg packages: ' + PACKAGES, output.getvalue())
        published = dict(line.split('=', 1) for line in files['GITHUB_ENV'].read_text().splitlines())
        self.assertEqual(published['VCPKG_ROOT'], self.root.as_posix())
        outputs = dict(line.split('=', 1) for line in files['GITHUB_OUTPUT'].read_text().splitlines())
        self.assertEqual(set(outputs), {'dir', 'prefix', 'key', 'installed', 'packages'})
        self.assertTrue(Path(outputs['dir']).is_dir())
        self.assertTrue(outputs['key'].startswith(outputs['prefix'] + 'baseline-'))
        with contextlib.redirect_stderr(io.StringIO()) as errors:
            self.assertEqual(vcpkg.main(arguments, {**env, 'ImageOS': ''}), 1)
        self.assertIn('::error::runner variable ImageOS is unset', errors.getvalue())

    def test_report_names_the_requested_and_restored_keys(self):
        env = {**self.env, 'REQUESTED_KEY': 'wanted', 'MATCHED_KEY': 'older'}
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(vcpkg.main(['cache-report'], env), 0)
        self.assertEqual(output.getvalue(), 'vcpkg cache: requested=wanted restored=older image=win25/20261111.1\n')
        with contextlib.redirect_stderr(io.StringIO()), contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(vcpkg.main(['cache-report'], {'REQUESTED_KEY': 'wanted'}), 1)
        # The process environment is read as the runner spells it; Windows stores the names in upper case.
        with mock.patch.dict(os.environ, env), contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(vcpkg.main(['cache-report']), 0)
        self.assertIn('image=win25/20261111.1', output.getvalue())


class Workflow(unittest.TestCase):
    def test_windows_job_delegates_to_this_module(self):
        text = job('test-windows')
        for command in ('cache-key --triplet x64-windows --packages "$VCPKG_PACKAGES"', 'cache-report',
                        'install --triplet x64-windows --packages "$VCPKG_PACKAGES"'):
            self.assertIn(f'        run: uv run --no-project --managed-python python -X lazy_imports=normal'
                          f' -m tools.dev.vcpkg {command}\n', text)
        for shell in ('sha256sum', ' vcpkg install', 'specs', 'GITHUB_ENV', 'GITHUB_OUTPUT', 'GITHUB_PATH', 'rev-parse'):
            self.assertNotIn(shell, text)

    def test_macos_job_delegates_to_this_module(self):
        text = job('test-macos')
        for command in ('cache-key', 'install'):
            self.assertIn(f'        run: uv run --no-project --managed-python python -X lazy_imports=normal'
                          f' -m tools.dev.vcpkg {command} --triplet "$VCPKG_TRIPLET"\n', text)
        for shell in ('shasum', 'readlink', '"$VCPKG_ROOT/vcpkg" install', 'vcpkg_cache', 'GITHUB_ENV'):
            self.assertNotIn(shell, text)

    def test_linux_action_delegates_to_this_module(self):
        text = (ROOT / '.github/actions/tkrzw/action.yml').read_text(encoding='utf-8')
        for command in ('bootstrap', f'cache-key --triplet {LINUX}', f'install --triplet {LINUX}'):
            self.assertIn(f'      run: python3 -m tools.dev.vcpkg {command}\n', text)
        for shell in ('run: |', 'set -euo', 'sha256sum', 'GITHUB_ENV', 'GITHUB_OUTPUT', 'GITHUB_PATH', 'git -C', 'git init'):
            self.assertNotIn(shell, text)

    def test_ci_commands_need_only_the_standard_library(self):
        # The container jobs run them with the image's Python: no managed-interpreter guard, no process service.
        source = (ROOT / 'tools/dev/vcpkg.py').read_text(encoding='utf-8')
        self.assertNotIn('require_normal_imports', source)
        self.assertEqual(source.count('from .process import run'), 1)


class EnvironmentTests(unittest.TestCase):
    def test_explicit_checkout_wins_and_replaces_stale_certification(self):
        with tempfile.TemporaryDirectory() as directory:
            env = dict(VCPKG_ROOT=directory, OXPINYIN_CACHE='/unused', PATH='/host/bin',
                       LD_LIBRARY_PATH='/host/lib', PKG_CONFIG_PATH='/host/pkgconfig',
                       OXPINYIN_TKRZW_LIBRARY='/old/libtkrzw.so.1', OXPINYIN_TKRZW_PREFIXES='/old')
            child = vcpkg.environment(env)
            prefix = Path(directory).resolve() / 'installed' / vcpkg.triplet()
            self.assertEqual(child['OXPINYIN_TKRZW_PREFIX'], str(prefix))
            self.assertEqual(child['PATH'], str(prefix / 'tools/tkrzw/bin') + os.pathsep + '/host/bin')
            self.assertEqual(child['LD_LIBRARY_PATH'], str(prefix / 'lib') + os.pathsep + '/host/lib')
            self.assertEqual(child['PKG_CONFIG_PATH'], str(prefix / 'lib/pkgconfig') + os.pathsep + '/host/pkgconfig')
            self.assertNotIn('OXPINYIN_TKRZW_LIBRARY', child)
            self.assertNotIn('OXPINYIN_TKRZW_PREFIXES', child)
            self.assertEqual(env['OXPINYIN_TKRZW_LIBRARY'], '/old/libtkrzw.so.1')


class InstallTests(unittest.TestCase):
    def test_native_linux_triplet_selection_and_unsupported_host(self):
        with tempfile.TemporaryDirectory() as directory, mock.patch.object(vcpkg.platform, 'system', return_value='Linux'):
            env = {'OXPINYIN_CACHE': directory}
            for architecture, target in (('x86_64', 'x64'), ('AMD64', 'x64'),
                                         ('aarch64', 'arm64'), ('arm64', 'arm64')):
                with self.subTest(architecture=architecture), mock.patch.object(vcpkg.platform, 'machine', return_value=architecture):
                    self.assertEqual(vcpkg.prefix(env).name, f'{target}-linux-oxpinyin')
            with mock.patch.object(vcpkg.platform, 'machine', return_value='riscv64'), \
                    mock.patch('tools.dev.process.run') as run, \
                    self.assertRaisesRegex(ValueError, 'unsupported Linux architecture.*riscv64'):
                vcpkg.install(env, ROOT, jobs=1)
            run.assert_not_called()
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_interrupted_checkout_is_not_published_and_retry_succeeds(self):
        with tempfile.TemporaryDirectory() as directory:
            env = {'OXPINYIN_CACHE': directory}
            target = vcpkg.checkout(env)
            interrupted = True
            installs = []

            def run(command, **kwargs):
                nonlocal interrupted
                if command[:2] == ['git', 'init']:
                    Path(command[2]).mkdir()
                elif command[:2] == ['git', 'fetch'] and interrupted:
                    interrupted = False
                    raise KeyboardInterrupt('interrupted fetch')
                elif command[0] == 'bash':
                    self.assertFalse(target.exists())
                    self.assertEqual(kwargs['env']['VCPKG_ROOT'], str(kwargs['cwd']))
                    (kwargs['cwd'] / 'vcpkg').touch()
                elif command[1] == 'install':
                    self.assertTrue((target / 'vcpkg').is_file())
                    installs.append(command)

            with mock.patch('tools.dev.process.run', side_effect=run):
                with self.assertRaises(KeyboardInterrupt):
                    vcpkg.install(env, ROOT, jobs=1)
                self.assertFalse(target.exists())
                self.assertEqual(list(Path(directory).iterdir()), [])
                vcpkg.install(env, ROOT, jobs=1)
            self.assertEqual(len(installs), 1)
            self.assertEqual(installs[0][2], f'tkrzw:{vcpkg.triplet()}')
            self.assertFalse(list(target.parent.glob('.vcpkg-*')))

    def test_explicit_root_must_already_be_bootstrapped(self):
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory) / 'user-vcpkg'
            with mock.patch('tools.dev.process.run') as run:
                with self.assertRaisesRegex(ValueError, 'VCPKG_ROOT needs a bootstrapped'):
                    vcpkg.install({'VCPKG_ROOT': str(target)}, ROOT, jobs=1)
            run.assert_not_called()
            self.assertFalse(target.exists())

    def test_explicit_root_updates_packages_without_changing_checkout(self):
        for outdated in (False, True):
            with self.subTest(outdated=outdated), tempfile.TemporaryDirectory() as directory:
                target = Path(directory) / 'user-vcpkg'
                target.mkdir()
                (target / 'vcpkg').write_text('user-owned')
                env = {'VCPKG_ROOT': str(target), 'OXPINYIN_CACHE': str(Path(directory) / 'cache')}
                current = not outdated
                operations = []

                def run(command, **kwargs):
                    nonlocal current
                    # The module resolves the root: /var is a link to /private/var on macOS.
                    self.assertEqual(command[0], (target / 'vcpkg').resolve())
                    self.assertIn('--classic', command)
                    self.assertIn(f'--overlay-ports={ROOT / "vcpkg-overlays"}', command)
                    self.assertIn(f'--overlay-triplets={ROOT / "triplets"}', command)
                    self.assertEqual(command[2], f'tkrzw:{vcpkg.triplet()}')
                    operations.append(command[1])
                    if command[1] == 'install':
                        # Classic install preserves an already installed old version.
                        self.assertEqual(current, not outdated)
                    elif command[1] == 'upgrade':
                        self.assertIn('--no-dry-run', command)
                        current = True
                    else:
                        self.fail(f'unexpected checkout operation: {command}')

                with mock.patch('tools.dev.process.run', side_effect=run):
                    vcpkg.install(env, ROOT, jobs=1)
                self.assertEqual(operations, ['install', 'upgrade'])
                self.assertTrue(current)
                self.assertEqual((target / 'vcpkg').read_text(), 'user-owned')

    def competing_checkout(self, complete):
        with tempfile.TemporaryDirectory() as directory:
            env = {'OXPINYIN_CACHE': directory}
            target = vcpkg.checkout(env)
            operations = []

            def run(command, **kwargs):
                if command[:2] == ['git', 'init']:
                    Path(command[2]).mkdir()
                elif command[0] == 'bash':
                    (kwargs['cwd'] / 'vcpkg').touch()
                    # Publish a populated destination immediately before our rename.
                    target.mkdir()
                    (target / 'vcpkg' if complete else target / 'partial').write_text('other run')
                elif command[1] in ('install', 'upgrade'):
                    operations.append(command[1])

            with mock.patch('tools.dev.process.run', side_effect=run):
                if complete:
                    vcpkg.install(env, ROOT, jobs=1)
                else:
                    with self.assertRaises(OSError):
                        vcpkg.install(env, ROOT, jobs=1)
            self.assertEqual(operations, ['install', 'upgrade'] if complete else [])
            self.assertFalse(list(target.parent.glob('.vcpkg-*')))
            self.assertEqual((target / ('vcpkg' if complete else 'partial')).read_text(), 'other run')

    def test_concurrent_complete_checkout_is_used(self):
        self.competing_checkout(True)

    def test_concurrent_incomplete_checkout_is_rejected(self):
        self.competing_checkout(False)



if __name__ == '__main__':
    unittest.main()
