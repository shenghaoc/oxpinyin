#!/usr/bin/env -S uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal
"""Unit tests for tools.dev.doctor."""
from pathlib import Path
import re
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import doctor


def completed(text, returncode=0):
    return mock.Mock(returncode=returncode, stdout=text + '\n', stderr='')


class DoctorTests(unittest.TestCase):
    def test_current_interpreter_identity(self):
        info = doctor.identity()
        self.assertEqual(info['implementation'], 'cpython')
        self.assertRegex(info['version'], r'^3\.15\.\d+$')
        self.assertTrue(info['gil_enabled'])
        self.assertFalse(info['gil_disabled'])
        self.assertTrue(info['managed'])

    def test_selection_overrides_are_rejected(self):
        for name in ('UV_PYTHON', 'PYTHONHOME', 'PYTHONPATH',
                     'UV_PYTHON_VERSION', 'UV_PYTHON_IMPLEMENTATION',
                     'UV_PYTHON_VARIANT', 'UV_PYTHON_ARCH'):
            with self.subTest(name=name):
                with self.assertRaisesRegex(doctor.DoctorError, name):
                    doctor.reject_overrides(dict(doctor.os.environ, **{name: '/x'}))

    def test_collect_reports_the_interpreter(self):
        with mock.patch.object(doctor, 'tool_lines', return_value=([], [])):
            lines, status = doctor.collect()
        self.assertEqual(status, 0)
        self.assertTrue(any(re.fullmatch(r'interpreter: cpython 3\.15\.\d+ \(minimum 3\.15\.0, below 3\.16\)', line)
                            for line in lines), lines)
        self.assertTrue(any(re.fullmatch(r'uv: \d+\.\d+\.\d+ \(minimum 0\.13\.1\)', line) for line in lines), lines)
        self.assertTrue(any(line.startswith('executable: ') for line in lines))

    def test_free_threaded_build_is_rejected(self):
        original = doctor.sysconfig.get_config_var
        def config(name):
            return 1 if name == 'Py_GIL_DISABLED' else original(name)
        with mock.patch.object(doctor.sysconfig, 'get_config_var', side_effect=config):
            with self.assertRaisesRegex(doctor.DoctorError, 'free-threaded'):
                doctor.identity()

    def test_python_series_accepts_3_15_0_and_later_patch_releases(self):
        for numbers in ((3, 15, 0), (3, 15, 1), (3, 15, 12)):
            with self.subTest(numbers=numbers):
                doctor.check_python(numbers, 'final')

    def test_python_outside_the_series_is_refused(self):
        for numbers, level, found in (((3, 14, 7), 'final', '3.14.7'),
                                      ((3, 16, 0), 'final', '3.16.0'),
                                      ((3, 17, 2), 'final', '3.17.2'),
                                      ((3, 15, 0), 'candidate', '3.15.0 (candidate)'),
                                      ((3, 15, 0), 'beta', '3.15.0 (beta)')):
            with self.subTest(numbers=numbers, level=level):
                with self.assertRaisesRegex(
                        doctor.DoctorError,
                        r'^CPython 3\.15\.0 or newer, below 3\.16, required; found '
                        + re.escape(found) + '$'):
                    doctor.check_python(numbers, level)

    def test_missing_uv_is_named(self):
        with self.assertRaisesRegex(doctor.DoctorError, r'^missing uv 0\.13\.1 or newer;'):
            doctor.uv_version({'PATH': '/nonexistent'})

    def test_uv_minimum_version(self):
        for version, accepted in (('0.13.1', True), ('0.13.4', True), ('0.14.0', True),
                                  ('0.13.0', False), ('0.12.9', False)):
            text = f'uv {version} (x86_64-unknown-linux-gnu)'
            result = mock.Mock(returncode=0, stdout=text + '\n', stderr='')
            with self.subTest(version=version), \
                    mock.patch.object(doctor, '_run', return_value=result):
                if accepted:
                    self.assertEqual(doctor.uv_version({'PATH': '/tools'}), text)
                else:
                    with self.assertRaisesRegex(doctor.DoctorError,
                                                r'^uv 0\.13\.1 or newer required; found '):
                        doctor.uv_version({'PATH': '/tools'})

    def test_unparseable_uv_version_is_rejected(self):
        for output in ('uv', 'uv version unknown', 'uv 0.13', '', 'ruff 0.13.1'):
            result = mock.Mock(returncode=0, stdout=output, stderr='')
            with self.subTest(output=output), \
                    mock.patch.object(doctor, '_run', return_value=result):
                with self.assertRaisesRegex(doctor.DoctorError,
                                            r'^unparseable uv version output: .*; '
                                            r'uv 0\.13\.1 or newer required$'):
                    doctor.uv_version({'PATH': '/tools'})

    def test_collect_reports_an_unsupported_tool(self):
        info = dict(uv='uv 0.13.1', implementation='cpython', version='3.15.0',
                    managed=True, gil_enabled=True, executable='/managed/python')
        failure = 'cargo-nextest 0.9.115 or later required; found cargo-nextest 0.9.114'
        with mock.patch.object(doctor, 'host_lines', return_value=[]), \
                mock.patch.object(doctor, 'identity', return_value=info), \
                mock.patch.object(doctor, 'tool_lines',
                                  return_value=([f'FAIL: {failure}'], [failure])):
            lines, status = doctor.collect()
        self.assertEqual(status, 1)
        self.assertIn(f'FAIL: {failure}', lines)


# For each tool: the first line of its version output with a placeholder, then
# a version below the minimum, the minimum itself and a newer one. The output
# shapes are the ones the tools print on the hosts this repository runs on.
SAMPLES = {
    'just': ('just {}', '1.58.9', '1.59.0', '1.61.2'),
    'cargo-nextest': ('cargo-nextest {} (60fa45f63 2026-08-04)', '0.9.114', '0.9.115', '0.10.0'),
    'cargo-c': ('cargo-c {}+cargo-0.98.0', '0.10.23', '0.10.24', '0.10.26'),
    'cargo-fuzz': ('cargo-fuzz {}', '0.13.1', '0.13.2', '0.14.0'),
    'cargo-llvm-cov': ('cargo-llvm-cov {}', '0.8.9', '0.9.0', '0.10.1'),
    'GNU ld': ('GNU ld (GNU Binutils for Debian) {}', '2.42', '2.43', '2.47'),
    'gcov': ('gcov (Debian 16.2.0-3) {}', '13.3.0', '14.0.0', '16.2.0'),
    'valgrind': ('valgrind-{}', '3.21.0', '3.22.0', '3.27.1'),
}


class ToolMinimumTests(unittest.TestCase):
    def check(self, tool, output, returncode=0):
        with mock.patch.object(doctor.shutil, 'which', return_value=f'/tools/{tool.executable}'), \
                mock.patch.object(doctor, '_run', return_value=completed(output, returncode)) as run:
            line = doctor.check_tool(tool, {'PATH': '/tools'})
        run.assert_called_once_with([f'/tools/{tool.executable}', *tool.arguments], {'PATH': '/tools'})
        return line

    def test_the_table_has_every_tool_once(self):
        names = [tool.name for tool in doctor.TOOLS]
        self.assertEqual(sorted(names), sorted(SAMPLES))
        self.assertEqual(len(names), len(set(names)))
        for tool in doctor.TOOLS:
            self.assertRegex(tool.minimum, r'^\d+(\.\d+){0,2}$', tool.name)

    def test_a_tool_at_or_above_its_minimum_passes(self):
        for tool in doctor.TOOLS:
            template, _, at, above = SAMPLES[tool.name]
            for version in (at, above):
                with self.subTest(tool=tool.name, version=version):
                    line = self.check(tool, template.format(version))
                    self.assertEqual(line, f'{tool.name}: {version} (minimum {tool.minimum})')

    def test_a_tool_below_its_minimum_fails_with_both_versions(self):
        for tool in doctor.TOOLS:
            template, below, _, _ = SAMPLES[tool.name]
            with self.subTest(tool=tool.name):
                with self.assertRaises(doctor.DoctorError) as caught:
                    self.check(tool, template.format(below))
                self.assertEqual(
                    str(caught.exception),
                    f'{tool.name} {tool.minimum} or later required; found {template.format(below)}')

    def test_unreadable_output_fails(self):
        for tool in doctor.TOOLS:
            if tool.identity:
                continue  # another program under the name is reported, not failed
            with self.subTest(tool=tool.name):
                with self.assertRaisesRegex(doctor.DoctorError,
                                            'or later required; found Apple LLVM version 21$'):
                    self.check(tool, 'Apple LLVM version 21')

    def test_a_failing_command_fails(self):
        for tool in doctor.TOOLS:
            template, _, at, _ = SAMPLES[tool.name]
            with self.subTest(tool=tool.name):
                with self.assertRaisesRegex(doctor.DoctorError, 'or later required'):
                    self.check(tool, template.format(at), returncode=1)

    def test_a_missing_tool_is_reported_with_its_install_step(self):
        for tool in doctor.TOOLS:
            with self.subTest(tool=tool.name), \
                    mock.patch.object(doctor.shutil, 'which', return_value=None), \
                    mock.patch.object(doctor, '_run') as run:
                line = doctor.check_tool(tool, {'PATH': '/tools'})
            run.assert_not_called()
            self.assertEqual(line, f'{tool.name}: missing (requires {tool.minimum} or later; '
                                   f'install: {tool.install})')

    def test_another_linker_under_the_name_ld_is_not_a_failure(self):
        ld = next(tool for tool in doctor.TOOLS if tool.name == 'GNU ld')
        for output in ('GNU gold (GNU Binutils 2.43) 1.16', 'LLD 20.1.0 (compatible with GNU linkers)',
                       'mold 2.40.0 (compatible with GNU ld)'):
            with self.subTest(output=output):
                line = self.check(ld, output)
                self.assertTrue(line.startswith(f'GNU ld: not found, ld is {output} '), line)
                self.assertIn('requires 2.43 or later', line)

    def test_other_distributions_version_lines_parse(self):
        by_name = {tool.name: tool for tool in doctor.TOOLS}
        for name, output, parsed in (
                ('GNU ld', 'GNU ld (GNU Binutils for Ubuntu) 2.44', '2.44'),
                ('GNU ld', 'GNU ld version 2.43.1-15.fc42', '2.43.1'),
                ('GNU ld', 'GNU ld (GNU Binutils) 2.45.1.20251027', '2.45.1'),
                ('gcov', 'gcov (Ubuntu 15.2.0-4ubuntu4) 15.2.0', '15.2.0'),
                ('gcov', 'gcov (GCC) 15.2.1 20260101 (Red Hat 15.2.1-7)', '15.2.1'),
                ('valgrind', 'valgrind-3.28.0.GIT', '3.28.0'),
                ('cargo-c', 'cargo-c 0.10.25+cargo-0.99.0', '0.10.25')):
            with self.subTest(output=output):
                self.assertEqual(self.check(by_name[name], output),
                                 f'{name}: {parsed} (minimum {by_name[name].minimum})')

    def test_linux_only_tools_are_skipped_elsewhere(self):
        linux_only = {tool.name for tool in doctor.TOOLS if tool.platform == 'linux'}
        self.assertEqual(linux_only, {'GNU ld', 'gcov', 'valgrind'})
        for platform_name, expected in (('darwin', set(SAMPLES) - linux_only), ('linux', set(SAMPLES))):
            with self.subTest(platform=platform_name), \
                    mock.patch.object(doctor.sys, 'platform', platform_name), \
                    mock.patch.object(doctor.shutil, 'which', return_value=None):
                lines, failures = doctor.tool_lines({'PATH': '/tools'})
            self.assertEqual(failures, [])
            self.assertEqual({line.split(':')[0] for line in lines}, expected)

    def test_tool_lines_collects_every_failure(self):
        with mock.patch.object(doctor.sys, 'platform', 'darwin'), \
                mock.patch.object(doctor.shutil, 'which', return_value='/tools/x'), \
                mock.patch.object(doctor, '_run', return_value=completed('just 1.0.0')):
            lines, failures = doctor.tool_lines({'PATH': '/tools'})
        self.assertEqual(len(failures), len(lines))
        self.assertTrue(all(line == f'FAIL: {failure}' for line, failure in zip(lines, failures)))


def read(relative):
    return (ROOT / relative).read_text()


def numbers(text):
    return tuple(int(part) for part in text.split('.'))


class MinimumsAgreeWithTheirOtherHomes(unittest.TestCase):
    """A minimum that lives in a tool's own configuration must match doctor's."""

    def minimum(self, name):
        return next(tool.minimum for tool in doctor.TOOLS if tool.name == name)

    def test_uv_floor_is_uv_toml(self):
        match = re.search(r'^required-version = ">=(\d+\.\d+\.\d+)"$', read('uv.toml'), re.M)
        self.assertIsNotNone(match)
        self.assertEqual(numbers(match.group(1)), doctor.UV_MINIMUM)

    def test_just_minimum_is_the_justfiles(self):
        match = re.search(r'^set minimum-version := "([\d.]+)"$', read('justfile'), re.M)
        self.assertIsNotNone(match)
        self.assertEqual(match.group(1), self.minimum('just'))

    def test_nextest_minimum_is_its_config(self):
        match = re.search(r'^nextest-version = "([\d.]+)"$', read('.config/nextest.toml'), re.M)
        self.assertIsNotNone(match)
        self.assertEqual(match.group(1), self.minimum('cargo-nextest'))

    def test_the_managed_request_is_the_checkouts_python_version(self):
        self.assertEqual(read('.python-version').strip(), doctor.MANAGED_REQUEST)

    def test_setup_script_provisions_with_the_same_minimums(self):
        setup = read('tools/cloud/setup.sh')
        for variable, expected in (('uv_minimum', '.'.join(map(str, doctor.UV_MINIMUM))),
                                   ('valgrind_minimum', self.minimum('valgrind'))):
            with self.subTest(variable=variable):
                match = re.search(rf'^{variable}=(\S+)$', setup, re.M)
                self.assertIsNotNone(match, f'setup.sh sets no {variable}')
                self.assertEqual(match.group(1), expected)


if __name__ == '__main__':
    unittest.main()
