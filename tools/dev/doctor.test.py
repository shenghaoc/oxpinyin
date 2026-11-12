#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Unit tests for tools.dev.doctor."""
from pathlib import Path
import json
import re
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import doctor, test_support


def completed(text, returncode=0):
    return mock.Mock(returncode=returncode, stdout=text + '\n', stderr='')


def tool(name):
    return next(candidate for candidate in doctor.TOOLS if candidate.name == name)


class DoctorTests(unittest.TestCase):
    def _dependency_lines(self, query_result, native='amd64', which='/usr/bin/dpkg-query'):
        """Run the dependency report with dpkg-query returning QUERY_RESULT and dpkg printing NATIVE."""
        def run(command, *args, **kwargs):
            if isinstance(query_result, Exception) and command[1] != '--print-architecture':
                raise query_result
            if command[1] == '--print-architecture':
                return mock.Mock(returncode=0, stdout=(native or '') + '\n', stderr='')
            return query_result
        with mock.patch.object(doctor.sys, 'platform', 'linux'), \
             mock.patch.object(doctor.shutil, 'which', return_value=which) as found, \
             mock.patch.object(doctor.subprocess, 'run', side_effect=run) as query:
            lines = doctor.oracle_dependency_lines({'PATH': '/supplied'})
        return lines, found, query

    def _manifest_packages(self):
        groups = json.loads((Path(doctor.__file__).resolve().parents[2] / 'tools/oracle/dependencies.json').read_text())
        return [package for group in groups.values() for package in group]

    def test_oracle_dependency_report_uses_manifest_groups(self):
        packages = [package for package in self._manifest_packages() if package != 'autopoint']
        output = ''.join(f'{package}\tamd64\tinstalled\n' for package in packages)
        lines, found, query = self._dependency_lines(mock.Mock(returncode=1, stdout=output, stderr=''))
        found.assert_called_with('dpkg-query', path='/supplied')
        self.assertIn('autopoint', query.call_args_list[0].args[0])
        self.assertIn('oracle full: optional, not installed autopoint', lines)
        self.assertIn('oracle common: installed', lines)
        self.assertIn('oracle tkrzw: installed', lines)

    def test_oracle_dependency_report_survives_a_missing_dpkg_query(self):
        for case in (None, FileNotFoundError(2, 'No such file')):
            with self.subTest(case=case):
                lines, _, _ = self._dependency_lines(case if case else None, which=None if case is None else '/x')
                self.assertEqual(len(lines), 1)
                self.assertIn('Debian package check unavailable', lines[0])

    def test_oracle_dependency_report_treats_status_two_as_unavailable(self):
        lines, _, _ = self._dependency_lines(mock.Mock(returncode=2, stdout='', stderr='dpkg-query: error: cannot read database\n'))
        self.assertEqual(len(lines), 1)
        self.assertIn('unavailable', lines[0])
        self.assertIn('cannot read database', lines[0])

    def test_oracle_dependency_report_counts_only_native_or_all_architectures(self):
        packages = self._manifest_packages()
        output = ''.join(f'{package}\t{"i386" if index == 0 else "all" if index == 1 else "amd64"}\tinstalled\n'
                         for index, package in enumerate(packages))
        lines, _, _ = self._dependency_lines(mock.Mock(returncode=0, stdout=output, stderr=''))
        text = '\n'.join(lines)
        self.assertIn(packages[0], text)
        self.assertNotIn(packages[1], text)
        self.assertNotIn(packages[2], text)

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
        self.assertTrue(any(re.fullmatch(r'interpreter: cpython 3\.14\.\d+ \(minimum 3\.14\.0, below 3\.15\)', line)
                            for line in lines), lines)
        self.assertTrue(any(re.fullmatch(r'uv: \d+\.\d+\.\d+ \(minimum 0\.11\.32\)', line) for line in lines), lines)
        self.assertTrue(any(line.startswith('executable: ') for line in lines))

    def test_free_threaded_build_is_rejected(self):
        original = doctor.sysconfig.get_config_var
        def config(name):
            return 1 if name == 'Py_GIL_DISABLED' else original(name)
        with mock.patch.object(doctor.sysconfig, 'get_config_var', side_effect=config):
            with self.assertRaisesRegex(doctor.DoctorError, 'free-threaded'):
                doctor.identity()

    def test_python_outside_the_series_is_refused(self):
        for numbers, level, found in (((3, 13, 7), 'final', '3.13.7'),
                                      ((3, 16, 0), 'final', '3.16.0'),
                                      ((3, 17, 2), 'final', '3.17.2'),
                                      ((3, 14, 0), 'candidate', '3.14.0 (candidate)'),
                                      ((3, 14, 0), 'beta', '3.14.0 (beta)')):
            with self.subTest(numbers=numbers, level=level):
                with self.assertRaisesRegex(
                        doctor.DoctorError,
                        r'^CPython 3\.14\.0 or newer, below 3\.15, required; found '
                        + re.escape(found) + '$'):
                    doctor.check_python(numbers, level)

    def test_missing_uv_is_named(self):
        with mock.patch.object(doctor, '_run', return_value=None), \
                self.assertRaisesRegex(doctor.DoctorError, r'^missing uv 0\.11\.32 or newer;'):
            doctor.uv_version({'PATH': '/nonexistent'})

    def test_unparseable_uv_version_is_rejected(self):
        for output in ('uv', 'uv version unknown', 'uv 0.13', 'uv 0.13.1-', 'uv v0.13.1', '', 'ruff 0.13.1'):
            result = mock.Mock(returncode=0, stdout=output, stderr='')
            with self.subTest(output=output), \
                    mock.patch.object(doctor, '_run', return_value=result):
                with self.assertRaisesRegex(doctor.DoctorError,
                                            r'^unparseable uv version output: .*; '
                                            r'uv 0\.11\.32 or newer required$'):
                    doctor.uv_version({'PATH': '/tools'})

    def test_collect_reports_an_unsupported_tool(self):
        info = dict(uv='uv 0.13.1', implementation='cpython', version='3.14.0',
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
    'pwsh': ('PowerShell {}', '7.4.99', doctor.PWSH_MINIMUM, '7.6.1'),
    'bash': ('GNU bash, version {} (1)-release (x86_64-pc-linux-gnu)', '4.4.23', '5.0.0', '5.2.37'),
    'cargo-nextest': ('cargo-nextest {} (60fa45f63 2026-08-04)', '0.9.114', '0.9.115', '0.10.0'),
    'cargo-c': ('cargo-c {}+cargo-0.98.0', '0.10.23', '0.10.24', '0.10.26'),
    'cargo-fuzz': ('cargo-fuzz {}', '0.13.1', '0.13.2', '0.14.0'),
    'cargo-llvm-cov': ('cargo-llvm-cov {}', '0.8.9', '0.9.0', '0.10.1'),
    'GNU ld': ('GNU ld (GNU Binutils for Debian) {}', '2.42', '2.43', '2.47'),
    'gcov': ('gcov (Debian 16.2.0-3) {}', '13.3.0', '14.0.0', '16.2.0'),
    'valgrind': ('valgrind-{}', '3.21.0', '3.22.0', '3.27.1'),
    'shellcheck': ('ShellCheck - shell script analysis tool\nversion: {}', '0.10.0', '0.11.0', '0.12.0'),
}


class ToolCase(unittest.TestCase):
    def check(self, subject, output, returncode=0):
        with mock.patch.object(doctor.shutil, 'which', return_value=f'/tools/{subject.executable}'), \
                mock.patch.object(doctor, '_run', return_value=completed(output, returncode)) as run:
            line = doctor.check_tool(subject, {'PATH': '/tools'})
        test_support.assert_calls(self, run, [mock.call([f'/tools/{subject.executable}', *subject.arguments],
                                                        {'PATH': '/tools'})])
        return line

    def assert_found(self, subject, output, shown):
        suffix = ' (advisory)' if subject.optional else ''
        self.assertEqual(self.check(subject, output),
                         f'{subject.name}: {shown} at /tools/{subject.executable} '
                         f'(minimum {subject.minimum}){suffix}')

    def assert_too_old(self, subject, output):
        with self.assertRaises(doctor.DoctorError) as caught:
            self.check(subject, output)
        parsed = subject.format.parse(output)
        shown = parsed.text if parsed else output
        self.assertEqual(str(caught.exception),
                         f'{subject.name} {subject.minimum} or later required; '
                         f'found {shown} at /tools/{subject.executable}')


class ToolMinimumTests(ToolCase):
    def test_a_tool_at_or_above_its_minimum_passes(self):
        for candidate in doctor.TOOLS:
            template, _, at, above = SAMPLES[candidate.name]
            for version in (at, above):
                with self.subTest(tool=candidate.name, version=version):
                    line = self.check(candidate, template.format(version))
                    self.assertRegex(line, rf'^{re.escape(candidate.name)}: {re.escape(version)}\S* '
                                           rf'at /tools/{re.escape(candidate.executable)} '
                                           rf'\(minimum {re.escape(candidate.minimum)}\)$')

    def test_a_tool_below_its_minimum_fails_with_both_versions(self):
        for candidate in doctor.TOOLS:
            template, below, _, _ = SAMPLES[candidate.name]
            with self.subTest(tool=candidate.name):
                self.assert_too_old(candidate, template.format(below))

    def test_unreadable_output_fails(self):
        for candidate in doctor.TOOLS:
            if candidate.identity:
                continue  # another program under the name is reported, not failed
            with self.subTest(tool=candidate.name):
                with self.assertRaisesRegex(
                        doctor.DoctorError,
                        'or later required; found Apple LLVM version 21 at '
                        + re.escape(f'/tools/{candidate.executable}') + '$'):
                    self.check(candidate, 'Apple LLVM version 21')

    def test_a_failing_command_fails(self):
        for candidate in doctor.TOOLS:
            template, _, at, _ = SAMPLES[candidate.name]
            with self.subTest(tool=candidate.name):
                with self.assertRaisesRegex(doctor.DoctorError, 'or later required'):
                    self.check(candidate, template.format(at), returncode=1)

    def test_a_missing_tool_is_reported_with_its_install_step(self):
        for candidate in doctor.TOOLS:
            with self.subTest(tool=candidate.name), \
                    mock.patch.object(doctor.shutil, 'which', return_value=None), \
                    mock.patch.object(doctor, '_run') as run:
                if candidate.required:
                    with self.assertRaisesRegex(doctor.DoctorError, 'missing .*install:'):
                        doctor.check_tool(candidate, {'PATH': '/tools'})
                    continue
                line = doctor.check_tool(candidate, {'PATH': '/tools'})
            test_support.assert_not_called(self, run)
            self.assertEqual(line, f'{candidate.name}: missing (requires {candidate.minimum} or later; '
                                   f'install: {candidate.install})')

    def test_another_linker_under_the_name_ld_is_not_a_failure(self):
        ld = tool('GNU ld')
        for output in ('GNU gold (GNU Binutils 2.43) 1.16', 'LLD 20.1.0 (compatible with GNU linkers)',
                       'mold 2.40.0 (compatible with GNU ld)'):
            with self.subTest(output=output):
                line = self.check(ld, output)
                self.assertTrue(line.startswith(f'GNU ld: not found, ld is {output} at /tools/ld '), line)
                self.assertIn('requires 2.43 or later', line)

    def test_linux_only_tools_are_skipped_elsewhere(self):
        linux_only = {candidate.name for candidate in doctor.TOOLS if candidate.platform == 'linux'}
        self.assertEqual(linux_only, {'GNU ld', 'gcov', 'valgrind'})
        for platform_name, expected in (('darwin', set(SAMPLES) - linux_only - {'pwsh'}),
                                        ('linux', set(SAMPLES) - {'pwsh'})):
            with self.subTest(platform=platform_name), \
                    mock.patch.object(doctor.sys, 'platform', platform_name), \
                    mock.patch.object(doctor.shutil, 'which', return_value=None):
                lines, failures = doctor.tool_lines({'PATH': '/tools'})
            self.assertEqual(failures, [])
            self.assertEqual({line.split(':')[0] for line in lines}, expected)

    def test_tool_lines_collects_every_required_failure(self):
        required = [candidate for candidate in doctor.TOOLS
                    if not candidate.optional and not candidate.platform]
        with mock.patch.object(doctor.sys, 'platform', 'darwin'), \
                mock.patch.object(doctor.shutil, 'which', return_value='/tools/x'), \
                mock.patch.object(doctor, '_run', return_value=completed('unrecognised version output')):
            lines, failures = doctor.tool_lines({'PATH': '/tools'})
        self.assertEqual(len(failures), len(required))
        self.assertTrue(all(line == f'FAIL: {failure}' for line, failure in zip(lines, failures)))
        self.assertTrue(any(line.startswith('advisory: shellcheck ') for line in lines), lines)


class BashFloorTests(ToolCase):
    """The Bash floor does not apply on Windows."""

    def test_bash_is_not_applicable_on_windows(self):
        # An old Bash on PATH (Git Bash, MSYS) is neither run nor failed there.
        executed = []

        def fake_run(command, environ):
            executed.append(command[0])
            if command[0] == '/tools/bash':
                return completed('GNU bash, version 3.2.57(1)-release (x86_64-pc-msys)')
            return completed('unrecognised version output')

        with mock.patch.object(doctor.sys, 'platform', 'win32'), \
                mock.patch.object(doctor.shutil, 'which', side_effect=lambda name, path: f'/tools/{name}'), \
                mock.patch.object(doctor, '_run', side_effect=fake_run):
            lines, failures = doctor.tool_lines({'PATH': '/tools'})
        self.assertIn('bash: not applicable on win32', lines)
        self.assertNotIn('/tools/bash', executed)
        self.assertFalse(any(failure.startswith('bash ') for failure in failures), failures)

class AdvisoryToolTests(unittest.TestCase):
    """ShellCheck is reported against 0.11.0 without blocking work that does not need it."""

    def lines_for(self, shellcheck):
        def fake_check(subject, environ=None):
            if subject.name == 'shellcheck':
                if isinstance(shellcheck, Exception):
                    raise shellcheck
                return shellcheck
            return f'{subject.name}: ok'

        with mock.patch.object(doctor, 'check_tool', side_effect=fake_check):
            return doctor.tool_lines({'PATH': '/tools'})

    def test_an_old_shellcheck_is_reported_without_failing(self):
        failure = 'shellcheck 0.11.0 or later required; found 0.10.0 at /tools/shellcheck'
        lines, failures = self.lines_for(doctor.DoctorError(failure))
        self.assertEqual(failures, [])
        self.assertIn(f'advisory: {failure}', lines)


SEMVER_TOOLS = ('pwsh', 'cargo-nextest', 'cargo-c', 'cargo-fuzz', 'cargo-llvm-cov')


def semver_line(subject, version):
    """The first line `<tool> --version` prints for VERSION. cargo-nextest adds a commit and a date."""
    trailer = ' (60fa45f63 2026-08-04)' if subject.name == 'cargo-nextest' else ''
    name = 'PowerShell' if subject.name == 'pwsh' else subject.name
    return f'{name} {version}{trailer}'


def numbers_of(version):
    return [int(part) for part in version.split('.')]


def next_minor(minimum):
    major, minor, _ = numbers_of(minimum)
    return f'{major}.{minor + 1}.0'


def next_patch(minimum):
    major, minor, patch = numbers_of(minimum)
    return f'{major}.{minor}.{patch + 1}'


def previous(minimum):
    major, minor, patch = numbers_of(minimum)
    return f'{major}.{minor}.{patch - 1}' if patch else f'{major}.{minor - 1}.99'


class SemverToolTests(ToolCase):
    """PowerShell, cargo-nextest, cargo-c, cargo-fuzz and cargo-llvm-cov print semantic versions
    (semver.org 2.0.0): a pre-release is older than the release it precedes, a pre-release of a
    newer release is newer than the minimum, and build metadata does not count."""

    def semver_tools(self):
        found = [candidate for candidate in doctor.TOOLS if candidate.name in SEMVER_TOOLS]
        self.assertEqual({candidate.name for candidate in found}, set(SEMVER_TOOLS))
        return found

    def assert_passes(self, subject, version):
        self.assert_found(subject, semver_line(subject, version), version)

    def assert_fails(self, subject, version):
        self.assert_too_old(subject, semver_line(subject, version))

    def test_tool_semver_minimums(self):
        subjects = [None, *self.semver_tools()]
        for candidate in subjects:
            name = candidate.name if candidate else 'uv'
            minimum = candidate.minimum if candidate else doctor.UV_MINIMUM
            cases = [(minimum + suffix, False)
                     for suffix in ('-beta.1', '-rc.1', '-alpha', '-0', '-rc.1+build.5')]
            cases += [(minimum + suffix, True)
                      for suffix in ('', '+build.5', '+20261023', '+cargo-0.98.0')]
            cases += [(version, True) for version in (
                next_minor(minimum) + '-beta.1', next_patch(minimum) + '-rc.1',
                next_patch(minimum) + '-rc.1+build.5')]
            cases += [(version, False) for version in (
                previous(minimum) + '-rc.1', minimum.rsplit('.', 1)[0], minimum + '-',
                'v' + minimum, minimum + '-beta..1')]
            if candidate is None:
                cases = [(version, False) for version in (
                    '0.11.32-beta.1', '0.11.32-rc.1', '0.11.32-alpha', '0.11.32-0',
                    '0.11.32-rc.1+build.5', '0.11.31-rc.1', '0.10.9-beta.1')]
                cases += [(version, True) for version in (
                    '0.11.32', '0.11.32+build.5', '0.11.32+20261014', '0.13.2',
                    '0.14.0-beta.1', '0.13.2-rc.1', '1.0.0-alpha', '0.13.2-rc.1+build.5')]
            for version, accepted in cases:
                with self.subTest(tool=name, version=version):
                    if candidate is None:
                        text = f'uv {version} (abc123def 2026-10-14 x86_64-unknown-linux-gnu)'
                        with mock.patch.object(doctor, '_run', return_value=completed(text)):
                            if accepted:
                                self.assertEqual(doctor.uv_version({'PATH': '/tools'}), text)
                            else:
                                with self.assertRaises(doctor.DoctorError) as caught:
                                    doctor.uv_version({'PATH': '/tools'})
                                self.assertEqual(str(caught.exception),
                                                 f'uv {minimum} or newer required; found {text}')
                    elif accepted:
                        self.assert_passes(candidate, version)
                    else:
                        self.assert_fails(candidate, version)


# GNU ld, gcov and Valgrind do not use semantic versioning, so doctor reads each one's own format and
# compares numbers; a development build counts as its number and is never "older" for being one.
# Each row: the tool, the first line it prints, and what a report shows (None when it is too old).
#
# Observed on 2026-10-23 by installing the distribution's packages in a stock container: Debian testing
# (binutils 2.47, gcc 16.2.0, valgrind 3.27.1), Ubuntu 24.04.5 and Fedora Linux 44. Fedora's gcov prints
# a second line, "JSON format version: 2", which doctor does not read.
OBSERVED = (
    ('GNU ld', 'GNU ld (GNU Binutils for Debian) 2.47', '2.47'),
    ('gcov', 'gcov (Debian 16.2.0-3) 16.2.0', '16.2.0'),
    ('valgrind', 'valgrind-3.27.1', '3.27.1'),
    ('GNU ld', 'GNU ld (GNU Binutils for Ubuntu) 2.42', None),
    ('gcov', 'gcov (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0', None),
    ('valgrind', 'valgrind-3.22.0', '3.22.0'),
    ('GNU ld', 'GNU ld version 2.46.1-1.fc44', '2.46.1'),   # -1.fc44 is Fedora's package release
    ('gcov', 'gcov (GCC) 16.2.1 20260819 (Red Hat 16.2.1-2)', '16.2.1'),
)

# Forms other distributions and the projects use, not run here. Development builds: binutils calls
# master after 2.43 branched 2.43.50 (newer than 2.43) and before it 2.42.50; GCC calls trunk snapshots
# "(experimental)" and release-branch snapshots "(prerelease)"; Valgrind adds .GIT to the number of the
# release a git build leads up to.
OTHER_FORMS = (
    ('GNU ld', 'GNU ld (Gentoo 2.43 p3) 2.43.1', '2.43.1'),
    ('GNU ld', 'GNU ld (GNU Binutils; SUSE Linux Enterprise 15) 2.41.0.20231213-150100.7.46', None),
    ('GNU ld', 'GNU ld (GNU Binutils) 2.43.50.20240101', '2.43.50.20240101'),
    ('GNU ld', 'GNU ld (GNU Binutils) 2.42.50.20240101', None),
    ('gcov', 'gcov (GCC) 14.2.1 20240910', '14.2.1'),
    ('gcov', 'gcov (Alpine 14.2.0) 14.2.0', '14.2.0'),
    ('gcov', 'gcov (Homebrew GCC 14.2.0_1) 14.2.0', '14.2.0'),
    ('gcov', 'gcov (GCC) 15.0.0 20240101 (experimental)', '15.0.0 experimental'),
    ('gcov', 'gcov (Debian 14-20240330-1) 14.0.1 20240330 (experimental) [master r14-9766-g6a4b5c9d1e2f]',
     '14.0.1 experimental'),
    ('gcov', 'gcov (GCC) 14.2.1 20240801 (prerelease)', '14.2.1 prerelease'),
    ('gcov', 'gcov (GCC) 13.0.1 20230101 (experimental)', None),
    ('gcov', 'gcov (GCC) 13.3.1 20240801 (prerelease)', None),
    ('valgrind', 'valgrind-3.18.1', None),
    ('valgrind', 'valgrind-3.21.0', None),
    ('valgrind', 'valgrind-3.22.0.GIT', '3.22.0.GIT'),
    ('valgrind', 'valgrind-3.23.0.GIT', '3.23.0.GIT'),
    ('valgrind', 'valgrind-3.21.0.GIT', None),
)


class OtherFormatTests(ToolCase):
    def test_each_tool_reads_its_own_format(self):
        for name, output, shown in OBSERVED + OTHER_FORMS:
            subject = tool(name)
            with self.subTest(tool=name, output=output):
                if shown is None:
                    self.assert_too_old(subject, output)
                else:
                    self.assert_found(subject, output, shown)


class WindowsCompilerTests(unittest.TestCase):
    def test_installer_finds_compiler_without_path(self):
        for architecture, folder in (('AMD64', 'x64'), ('ARM64', 'arm64')):
            with self.subTest(architecture=architecture), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                version = root / 'VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt'
                version.parent.mkdir(parents=True)
                version.write_text('14.44.35207\n')
                compiler = root / f'VC/Tools/MSVC/14.44.35207/bin/Host{folder}/{folder}/cl.exe'
                compiler.parent.mkdir(parents=True)
                compiler.touch()
                env = {'ProgramFiles(x86)': str(root / 'Program Files (x86)'), 'PATH': ''}
                with mock.patch.object(doctor, '_run', return_value=completed(str(root))) as run, \
                        mock.patch.object(doctor.platform, 'machine', return_value=architecture):
                    line = doctor.windows_compiler(env)
                self.assertEqual(line, f'MSVC 14.44.35207 at {compiler}')
                self.assertEqual(run.call_args.args, ([str(root / 'Program Files (x86)' /
                    'Microsoft Visual Studio/Installer/vswhere.exe'), '-latest', '-products', '*',
                    '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
                    '-property', 'installationPath'], env))

    def test_missing_or_failed_installer_reports_missing_compiler(self):
        for result in (None, completed('', 1), completed('')):
            with self.subTest(result=result), mock.patch.object(doctor, '_run', return_value=result):
                self.assertIn('missing', doctor.windows_compiler({}))


if __name__ == '__main__':
    unittest.main()
