"""Unit tests for tools/oracle/versions.py."""
import importlib.util
import io
import json
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest.mock import patch

MODULE_PATH = Path(__file__).with_name('versions.py')
SPEC = importlib.util.spec_from_file_location('oracle_versions', MODULE_PATH)
versions = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(versions)


def manifest(components):
    return {'schema': versions.SCHEMA, 'components': components}


def component(name, package, major_minor):
    return {'name': name, 'package': package, 'major_minor': major_minor,
            'basis': 'test basis'}


class ManifestTest(unittest.TestCase):
    def test_checked_in_manifest_is_valid_and_complete(self):
        loaded = versions.load_manifest(versions.DEFAULT_MANIFEST)
        names = [entry['name'] for entry in loaded['components']]
        for expected in ('glibc', 'glib', 'Berkeley DB 5.3', 'Kyoto Cabinet',
                         'tkrzw', 'C compiler', 'C++ compiler', 'binutils',
                         'autoconf', 'automake', 'libtool', 'pkg-config'):
            self.assertIn(expected, names)
        # Debian's libtkrzw needs no distro-probe.sh; no component asks for one.
        self.assertFalse(any('probe' in entry for entry in loaded['components']))

    def test_duplicate_component_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp, 'manifest.json')
            path.write_text(json.dumps(manifest([
                component('glibc', 'libc6', '2.41'),
                component('glibc', 'libc6', '2.41')])))
            with self.assertRaises(ValueError):
                versions.load_manifest(path)


class CheckTest(unittest.TestCase):
    def run_check(self, entries, installed):
        with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
            return versions.check(manifest(entries), resolve=lambda package: installed[package])

    def test_installed_version_is_recorded(self):
        self.assertEqual(self.run_check([component('glibc', 'libc6', '2.41')],
                                        {'libc6': '2.41-12+deb13u4'}), 0)

    def test_version_drift_is_recorded_without_rejection(self):
        stderr = io.StringIO()
        with redirect_stdout(io.StringIO()) as stdout, redirect_stderr(stderr):
            status = versions.check(manifest([component('C compiler', 'gcc-14', '14.2')]),
                                    resolve=lambda package: '15.2.0-1')
        self.assertEqual(status, 0)
        self.assertIn('C compiler\tgcc-14\t15.2.0-1', stdout.getvalue())
        self.assertEqual(stderr.getvalue(), '')

    def test_a_missing_package_fails_the_build_and_no_probe_runs(self):
        def missing(package):
            raise LookupError(f'{package}: not installed')
        with patch.object(versions.subprocess, 'run', side_effect=AssertionError('a child was started')), \
                redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()) as stderr:
            status = versions.check(manifest([component('tkrzw', 'libtkrzw1t64', '1.0')]), resolve=missing)
        self.assertEqual(status, 1)
        self.assertIn('FAIL: tkrzw: libtkrzw1t64: not installed', stderr.getvalue())

    def test_the_probe_option_is_gone(self):
        with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
            versions.main(['--probe', 'tools/tkrzw/distro-probe.sh'])


class PrintModeTest(unittest.TestCase):
    def test_print_records_fallbacks_and_missing_components_without_a_probe(self):
        entries = [component('glibc', 'libc6', '2.41'),
                   component('C compiler', 'gcc-14', '14.2'),
                   component('pkg-config', 'pkg-config', '1.8'),
                   component('tkrzw', 'libtkrzw1t64', '1.0')]
        packages = {'libc6': '2.41-12+deb13u4'}
        commands = {('gcc', '--version'): '15.2.0'}

        def package_version(package):
            if package not in packages:
                raise LookupError(f'{package}: not installed')
            return packages[package]

        def command_version(command):
            if command not in commands:
                raise LookupError(f'{command[0]} is unavailable')
            return commands[command]

        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp, 'manifest.json')
            path.write_text(json.dumps(manifest(entries)))
            record = Path(tmp, 'versions.txt')
            stdout = io.StringIO()
            with patch.object(versions, 'package_version', side_effect=package_version), \
                    patch.object(versions, 'command_version', side_effect=command_version), \
                    redirect_stdout(stdout), redirect_stderr(io.StringIO()):
                status = versions.main(['--print', '--manifest', str(path),
                                        '--record', str(record)])
            recorded = record.read_text(encoding='utf-8')
        # Print mode checks no range: the out-of-range
        # fallback version is recorded, and a missing package keeps its own
        # error even when its fallback command also fails.
        expected = ('glibc\tlibc6\t2.41-12+deb13u4\n'
                    'C compiler\tgcc-14\t15.2.0\n'
                    'pkg-config\tpkg-config\tMISSING (pkg-config: not installed)\n'
                    'tkrzw\tlibtkrzw1t64\tMISSING (libtkrzw1t64: not installed)\n')
        self.assertEqual(status, 0)
        self.assertEqual(stdout.getvalue(), expected)
        self.assertEqual(recorded, expected)


if __name__ == '__main__':
    unittest.main()
