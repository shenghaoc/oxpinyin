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
        self.assertTrue(any(re.fullmatch(r'uv: \d+\.\d+\.\d+.*', line) for line in lines), lines)
        self.assertTrue(any(line.startswith('executable: ') for line in lines))

    def test_free_threaded_build_is_rejected(self):
        original = doctor.sysconfig.get_config_var
        def config(name):
            return 1 if name == 'Py_GIL_DISABLED' else original(name)
        with mock.patch.object(doctor.sysconfig, 'get_config_var', side_effect=config):
            with self.assertRaisesRegex(doctor.DoctorError, 'free-threaded'):
                doctor.identity()

    def test_cargo_c_fallback_is_separate_from_the_installed_floor(self):
        self.assertEqual(doctor.cargo_tool('cargo-c').minimum, '0.10.11')
        self.assertEqual(doctor.CARGO_TOOL_PINS['cargo-c'], '0.10.25')

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
                self.assertRaisesRegex(doctor.DoctorError, r'^uv did not run; install it as '):
            doctor.uv_version({'PATH': '/nonexistent'})

    def test_uv_version_is_reported_without_a_comparison(self):
        # uv refuses a version below uv.toml's required-version itself.
        for output in ('uv 0.0.1', 'uv 99.0.0-rc.1 (abc 2026-11-01 x86_64-unknown-linux-gnu)', 'unrecognised'):
            with self.subTest(output=output), \
                    mock.patch.object(doctor, '_run', return_value=completed(output)):
                self.assertEqual(doctor.uv_version({'PATH': '/tools'}), output)

    def test_collect_reports_an_unsupported_tool(self):
        info = dict(uv='uv 0.13.1', implementation='cpython', version='3.14.0',
                    managed=True, gil_enabled=True, executable='/managed/python')
        failure = 'cargo-c 0.10.11 or later required; found 0.10.10 at /tools/cargo-cinstall'
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
    'cargo-c': ('cargo-c {}+cargo-0.98.0', '0.10.10', '0.10.11', '0.10.25'),
    'cargo-fuzz': ('cargo-fuzz {}', '0.13.1', '0.13.2', '0.14.0'),
    'cargo-llvm-cov': ('cargo-llvm-cov {}', '0.8.9', '0.9.0', '0.10.1'),
    'GNU ld': ('GNU ld (GNU Binutils for Debian) {}', '2.41', '2.42', '2.47'),
    'gcov': ('gcov (Debian 16.2.0-3) {}', '12.3.0', '13.0.0', '16.2.0'),
    'valgrind': ('valgrind-{}', '3.21.0', '3.22.0', '3.27.1'),
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
        self.assertEqual(self.check(subject, output),
                         f'{subject.name}: {shown} at /tools/{subject.executable} '
                         f'(minimum {subject.minimum})')

    def assert_too_old(self, subject, output):
        with self.assertRaises(doctor.DoctorError) as caught:
            self.check(subject, output)
        parsed = subject.format.parse(output)
        shown = parsed.text if parsed else output
        self.assertEqual(str(caught.exception),
                         f'{subject.name} {subject.minimum} or later required; '
                         f'found {shown} at /tools/{subject.executable}')


# The tools whose version doctor compares; a tool with a verdict answers for itself.
VERSIONED = tuple(candidate for candidate in doctor.TOOLS if candidate.verdict is None)


class ToolMinimumTests(ToolCase):
    def test_a_tool_at_or_above_its_minimum_passes(self):
        for candidate in VERSIONED:
            template, _, at, above = SAMPLES[candidate.name]
            for version in (at, above):
                with self.subTest(tool=candidate.name, version=version):
                    line = self.check(candidate, template.format(version))
                    self.assertRegex(line, rf'^{re.escape(candidate.name)}: {re.escape(version)}\S* '
                                           rf'at /tools/{re.escape(candidate.executable)} '
                                           rf'\(minimum {re.escape(candidate.minimum)}\)$')

    def test_a_tool_below_its_minimum_fails_with_both_versions(self):
        for candidate in VERSIONED:
            template, below, _, _ = SAMPLES[candidate.name]
            with self.subTest(tool=candidate.name):
                self.assert_too_old(candidate, template.format(below))

    def test_unreadable_output_fails(self):
        for candidate in VERSIONED:
            if candidate.identity:
                continue  # another program under the name is reported, not failed
            with self.subTest(tool=candidate.name):
                with self.assertRaisesRegex(
                        doctor.DoctorError,
                        'or later required; found Apple LLVM version 21 at '
                        + re.escape(f'/tools/{candidate.executable}') + '$'):
                    self.check(candidate, 'Apple LLVM version 21')

    def test_a_failing_command_fails(self):
        for candidate in VERSIONED:
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
            self.assertEqual(line, f'{candidate.name}: missing (requires {doctor.requirement(candidate)}; '
                                   f'install: {candidate.install})')

    def test_another_linker_under_the_name_ld_is_not_a_failure(self):
        ld = tool('GNU ld')
        for output in ('GNU gold (GNU Binutils 2.43) 1.16', 'LLD 20.1.0 (compatible with GNU linkers)',
                       'mold 2.40.0 (compatible with GNU ld)'):
            with self.subTest(output=output):
                line = self.check(ld, output)
                self.assertTrue(line.startswith(f'GNU ld: not found, ld is {output} at /tools/ld '), line)
                self.assertIn('requires 2.42 or later', line)

    def test_linux_only_tools_are_skipped_elsewhere(self):
        linux_only = {candidate.name for candidate in doctor.TOOLS if candidate.platform == 'linux'}
        self.assertEqual(linux_only, {'GNU ld', 'gcov', 'valgrind'})
        reported = set(SAMPLES) | {'cargo-nextest'}
        for platform_name, expected in (('darwin', reported - linux_only - {'pwsh'}),
                                        ('linux', reported - {'pwsh'})):
            with self.subTest(platform=platform_name), \
                    mock.patch.object(doctor.sys, 'platform', platform_name), \
                    mock.patch.object(doctor.shutil, 'which', return_value=None):
                lines, failures = doctor.tool_lines({'PATH': '/tools'})
            self.assertEqual(failures, [])
            self.assertEqual({line.split(':')[0] for line in lines}, expected)

    def test_tool_lines_collects_every_required_failure(self):
        # nextest answers for itself, and its fake answer here is a success.
        required = [candidate for candidate in VERSIONED if not candidate.platform]
        with mock.patch.object(doctor.sys, 'platform', 'darwin'), \
                mock.patch.object(doctor.shutil, 'which', return_value='/tools/x'), \
                mock.patch.object(doctor, '_run', return_value=completed('unrecognised version output')):
            lines, failures = doctor.tool_lines({'PATH': '/tools'})
        self.assertEqual(len(failures), len(required))
        self.assertEqual([line for line in lines if line.startswith('FAIL: ')],
                         [f'FAIL: {failure}' for failure in failures])


SEMVER_TOOLS = ('pwsh', 'cargo-c', 'cargo-fuzz', 'cargo-llvm-cov')


def semver_line(subject, version):
    """The first line `<tool> --version` prints for VERSION."""
    name = 'PowerShell' if subject.name == 'pwsh' else subject.name
    return f'{name} {version}'


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
    """PowerShell, cargo-c, cargo-fuzz and cargo-llvm-cov print semantic versions
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
        for candidate in self.semver_tools():
            name, minimum = candidate.name, candidate.minimum
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
            for version, accepted in cases:
                with self.subTest(tool=name, version=version):
                    if accepted:
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
    ('GNU ld', 'GNU ld (GNU Binutils for Ubuntu) 2.42', '2.42'),
    ('gcov', 'gcov (Ubuntu 13.3.0-6ubuntu2~24.04.1) 13.3.0', '13.3.0'),
    ('valgrind', 'valgrind-3.22.0', '3.22.0'),
    ('GNU ld', 'GNU ld version 2.46.1-1.fc44', '2.46.1'),   # -1.fc44 is Fedora's package release
    ('gcov', 'gcov (GCC) 16.2.1 20260819 (Red Hat 16.2.1-2)', '16.2.1'),
)

# Forms other distributions and the projects use, not run here. Development builds: binutils calls
# master after 2.42 branched 2.42.50 (newer than 2.42) and before it 2.41.50; GCC calls trunk snapshots
# "(experimental)" and release-branch snapshots "(prerelease)"; Valgrind adds .GIT to the number of the
# release a git build leads up to.
OTHER_FORMS = (
    ('GNU ld', 'GNU ld (Gentoo 2.43 p3) 2.43.1', '2.43.1'),
    ('GNU ld', 'GNU ld (GNU Binutils; SUSE Linux Enterprise 15) 2.41.0.20231213-150100.7.46', None),
    ('GNU ld', 'GNU ld (GNU Binutils) 2.42.50.20240101', '2.42.50.20240101'),
    ('GNU ld', 'GNU ld (GNU Binutils) 2.41.50.20230801', None),
    ('gcov', 'gcov (GCC) 14.2.1 20240910', '14.2.1'),
    ('gcov', 'gcov (Alpine 14.2.0) 14.2.0', '14.2.0'),
    ('gcov', 'gcov (Homebrew GCC 14.2.0_1) 14.2.0', '14.2.0'),
    ('gcov', 'gcov (GCC) 15.0.0 20240101 (experimental)', '15.0.0 experimental'),
    ('gcov', 'gcov (Debian 14-20240330-1) 14.0.1 20240330 (experimental) [master r14-9766-g6a4b5c9d1e2f]',
     '14.0.1 experimental'),
    ('gcov', 'gcov (GCC) 14.2.1 20240801 (prerelease)', '14.2.1 prerelease'),
    ('gcov', 'gcov (GCC) 13.0.1 20230101 (experimental)', '13.0.1 experimental'),
    ('gcov', 'gcov (GCC) 13.3.1 20240801 (prerelease)', '13.3.1 prerelease'),
    ('gcov', 'gcov (GCC) 12.0.1 20220101 (experimental)', None),
    ('gcov', 'gcov (GCC) 12.4.1 20240801 (prerelease)', None),
    ('valgrind', 'valgrind-3.18.1', None),
    ('valgrind', 'valgrind-3.21.0', None),
    ('valgrind', 'valgrind-3.22.0.GIT', '3.22.0.GIT'),
    ('valgrind', 'valgrind-3.23.0.GIT', '3.23.0.GIT'),
    ('valgrind', 'valgrind-3.21.0.GIT', None),
)


class NextestVerdictTests(unittest.TestCase):
    """nextest enforces the nextest-version in .config/nextest.toml: doctor reports its answer."""

    def verdict(self, result):
        subject = tool('cargo-nextest')
        with mock.patch.object(doctor.shutil, 'which', return_value='/tools/cargo-nextest'), \
                mock.patch.object(doctor, '_run', return_value=result) as run:
            try:
                return doctor.check_tool(subject, {'PATH': '/tools'})
            finally:
                test_support.assert_calls(self, run, [mock.call(
                    ['/tools/cargo-nextest', 'nextest', 'show-config', 'version'], {'PATH': '/tools'},
                    cwd=doctor.ROOT)])

    def report(self, result):
        return 'current nextest version: N.N.N\nevaluation result: ' + result

    def test_a_met_or_only_recommended_requirement_passes(self):
        for status, result in ((0, 'ok'), (10, 'does not meet recommended version')):
            with self.subTest(status=status):
                self.assertEqual(self.verdict(completed(self.report(result), status)),
                                 'cargo-nextest: N.N.N at /tools/cargo-nextest '
                                 '(the nextest-version in .config/nextest.toml: met)')

    def test_a_missed_required_version_fails(self):
        with self.assertRaises(doctor.DoctorError) as caught:
            self.verdict(completed(self.report('does not meet required version'), 92))
        self.assertEqual(str(caught.exception), 'cargo-nextest N.N.N at /tools/cargo-nextest does not meet '
                                                'the nextest-version in .config/nextest.toml')

    def test_a_check_that_cannot_run_fails(self):
        for result, status in ((completed('error: could not find Cargo.toml', 102), 'exit 102'),
                               (None, 'did not start')):
            with self.subTest(status=status), self.assertRaises(doctor.DoctorError) as caught:
                self.verdict(result)
            self.assertEqual(str(caught.exception), 'cargo-nextest at /tools/cargo-nextest could not check '
                                                    f'the nextest-version in .config/nextest.toml ({status})')

    def test_doctor_compares_no_nextest_version(self):
        self.assertNotIn('nextest', ''.join(SAMPLES))
        self.assertIsNone(tool('cargo-c').verdict)
        self.assertIsNotNone(tool('cargo-nextest').verdict)


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
