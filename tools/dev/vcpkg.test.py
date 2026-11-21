"""The vcpkg binary cache and install steps of the Windows CI job."""
import contextlib
import hashlib
import io
import os
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
PACKAGES = 'glib:x64-windows pkgconf:x64-windows tkrzw:x64-windows'


def git(status, stdout=b''):
    return lambda command: SimpleNamespace(status=status, stdout=stdout)


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
                        'packages': hashlib.sha256(PACKAGES.encode()).hexdigest()[:16]}})
        self.assertNotIn('\\', cache + selected['environment']['VCPKG_ROOT'])
        other = vcpkg.binary_cache(self.env, WINDOWS, PACKAGES + ' zlib:x64-windows', capture=git(0, b'abc123\n'))
        self.assertNotEqual(other['outputs']['packages'], selected['outputs']['packages'])
        self.assertNotIn('packages', vcpkg.binary_cache(self.env, WINDOWS, capture=git(0, b'abc123\n'))['outputs'])

    def test_only_a_windows_triplet_has_a_cache_or_install_rule(self):
        # Linux takes Kyoto Cabinet from apt and macOS tkrzw from Homebrew (docs/runbooks/setup.md).
        calls = []
        for triplet in ('x64-linux', 'arm64-osx-oxpinyin'):
            with self.subTest(triplet=triplet):
                with self.assertRaisesRegex(ValueError, 'no vcpkg cache rule'):
                    vcpkg.binary_cache(self.env, triplet, capture=git(0, b'abc123\n'))
                with self.assertRaisesRegex(ValueError, 'no install rule'):
                    vcpkg.runner_install({'VCPKG_ROOT': str(self.root)}, triplet,
                                         run=lambda command, **options: calls.append(command))
        self.assertEqual(calls, [])

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
        self.assertEqual([command for command, _ in calls], [
            [str(self.program), 'install', *PACKAGES.split(), f'--overlay-ports={ROOT / "vcpkg-overlays"}'],
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
        self.assertEqual(calls, [])

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
        self.assertEqual(set(outputs), {'dir', 'prefix', 'key', 'packages'})
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


if __name__ == '__main__':
    unittest.main()
