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


def component(name, package, major_minor, probe=False):
    entry = {'name': name, 'package': package, 'major_minor': major_minor,
             'basis': 'test basis'}
    if probe:
        entry['probe'] = True
    return entry


class ManifestTest(unittest.TestCase):
    def test_checked_in_manifest_is_valid_and_complete(self):
        loaded = versions.load_manifest(versions.DEFAULT_MANIFEST)
        names = [entry['name'] for entry in loaded['components']]
        for expected in ('glibc', 'glib', 'Berkeley DB 5.3', 'Kyoto Cabinet',
                         'tkrzw', 'C compiler', 'C++ compiler', 'binutils',
                         'autoconf', 'automake', 'libtool', 'pkg-config'):
            self.assertIn(expected, names)
        tkrzw = next(entry for entry in loaded['components'] if entry['name'] == 'tkrzw')
        self.assertTrue(tkrzw.get('probe'))

    def test_duplicate_component_is_rejected(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp, 'manifest.json')
            path.write_text(json.dumps(manifest([
                component('glibc', 'libc6', '2.41'),
                component('glibc', 'libc6', '2.41')])))
            with self.assertRaises(ValueError):
                versions.load_manifest(path)


class CheckTest(unittest.TestCase):
    def run_check(self, entries, installed, probe=None):
        with tempfile.TemporaryDirectory() as tmp:
            probe_path = Path(tmp, 'probe.sh')
            probe_path.write_text('#!/bin/sh\necho "RESULT : healthy"\n')
            path = probe if probe is not None else str(probe_path)
            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                status = versions.check(manifest(entries), resolve=lambda package: installed[package],
                                        probe=path)
        return status

    def test_installed_version_is_recorded(self):
        self.assertEqual(self.run_check([component('glibc', 'libc6', '2.41')],
                                        {'libc6': '2.41-12+deb13u4'}), 0)

    def test_version_drift_is_recorded_without_rejection(self):
        stderr = io.StringIO()
        with redirect_stdout(io.StringIO()) as stdout, redirect_stderr(stderr):
            status = versions.check(manifest([component('C compiler', 'gcc-14', '14.2')]),
                                    resolve=lambda package: '15.2.0-1', probe='/bin/true')
        self.assertEqual(status, 0)
        self.assertIn('C compiler\tgcc-14\t15.2.0-1', stdout.getvalue())
        self.assertEqual(stderr.getvalue(), '')

    def test_failed_tkrzw_probe_fails_the_build(self):
        with tempfile.TemporaryDirectory() as tmp:
            probe = Path(tmp, 'probe.sh')
            probe.write_text('#!/bin/sh\necho "RESULT : pointer identity is broken"\nexit 1\n')
            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                status = versions.check(
                    manifest([component('tkrzw', 'libtkrzw1t64', '1.0', probe=True)]),
                    resolve=lambda package: '1.0.32-1+b1', probe=str(probe))
        self.assertEqual(status, 1)


class PrintModeTest(unittest.TestCase):
    def test_print_records_fallbacks_and_missing_components_without_a_probe(self):
        entries = [component('glibc', 'libc6', '2.41'),
                   component('C compiler', 'gcc-14', '14.2'),
                   component('pkg-config', 'pkg-config', '1.8'),
                   component('tkrzw', 'libtkrzw1t64', '1.0', probe=True)]
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
                                        '--probe', str(Path(tmp, 'absent-probe.sh')),
                                        '--record', str(record)])
            recorded = record.read_text(encoding='utf-8')
        # Print mode checks no range and runs no probe: the out-of-range
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
