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

from tools.dev import doctor, test_support


def completed(text, returncode=0):
    return mock.Mock(returncode=returncode, stdout=text + '\n', stderr='')


def tool(name):
    return next(candidate for candidate in doctor.TOOLS if candidate.name == name)


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

    def test_collect_shows_a_newer_prerelease_uv_as_printed(self):
        info = dict(uv='uv 0.14.0-beta.1 (abc123def 2026-10-14 x86_64-unknown-linux-gnu)',
                    implementation='cpython', version='3.15.0',
                    managed=True, gil_enabled=True, executable='/managed/python')
        with mock.patch.object(doctor, 'host_lines', return_value=[]), \
                mock.patch.object(doctor, 'identity', return_value=info), \
                mock.patch.object(doctor, 'tool_lines', return_value=([], [])):
            lines, status = doctor.collect()
        self.assertEqual(status, 0)
        self.assertIn('uv: 0.14.0-beta.1 (minimum 0.13.1)', lines)

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
        for output in ('uv', 'uv version unknown', 'uv 0.13', 'uv 0.13.1-', 'uv v0.13.1', '', 'ruff 0.13.1'):
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


class UvVersionTests(unittest.TestCase):
    """uv prints a semantic version, so a pre-release of the minimum is older than it."""

    def uv(self, version):
        text = f'uv {version} (abc123def 2026-10-14 x86_64-unknown-linux-gnu)'
        result = mock.Mock(returncode=0, stdout=text + '\n', stderr='')
        return text, mock.patch.object(doctor, '_run', return_value=result)

    def test_a_prerelease_at_the_minimum_fails(self):
        for version in ('0.13.1-beta.1', '0.13.1-rc.1', '0.13.1-alpha', '0.13.1-0', '0.13.1-rc.1+build.5'):
            text, run = self.uv(version)
            with self.subTest(version=version), run:
                with self.assertRaises(doctor.DoctorError) as caught:
                    doctor.uv_version({'PATH': '/tools'})
                self.assertEqual(str(caught.exception), f'uv 0.13.1 or newer required; found {text}')

    def test_the_release_and_a_release_with_build_metadata_pass(self):
        for version in ('0.13.1', '0.13.1+build.5', '0.13.1+20261014', '0.13.2'):
            text, run = self.uv(version)
            with self.subTest(version=version), run:
                self.assertEqual(doctor.uv_version({'PATH': '/tools'}), text)

    def test_a_newer_prerelease_passes(self):
        for version in ('0.14.0-beta.1', '0.13.2-rc.1', '1.0.0-alpha', '0.13.2-rc.1+build.5'):
            text, run = self.uv(version)
            with self.subTest(version=version), run:
                self.assertEqual(doctor.uv_version({'PATH': '/tools'}), text)

    def test_a_prerelease_of_an_older_release_fails(self):
        for version in ('0.13.0-rc.1', '0.12.9-beta.1'):
            text, run = self.uv(version)
            with self.subTest(version=version), run:
                with self.assertRaises(doctor.DoctorError) as caught:
                    doctor.uv_version({'PATH': '/tools'})
                self.assertEqual(str(caught.exception), f'uv 0.13.1 or newer required; found {text}')


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


class ToolCase(unittest.TestCase):
    def check(self, subject, output, returncode=0):
        with mock.patch.object(doctor.shutil, 'which', return_value=f'/tools/{subject.executable}'), \
                mock.patch.object(doctor, '_run', return_value=completed(output, returncode)) as run:
            line = doctor.check_tool(subject, {'PATH': '/tools'})
        test_support.assert_calls(self, run, [mock.call([f'/tools/{subject.executable}', *subject.arguments],
                                                        {'PATH': '/tools'})])
        return line

    def assert_found(self, subject, output, shown):
        self.assertEqual(self.check(subject, output), f'{subject.name}: {shown} (minimum {subject.minimum})')

    def assert_too_old(self, subject, output):
        with self.assertRaises(doctor.DoctorError) as caught:
            self.check(subject, output)
        self.assertEqual(str(caught.exception),
                         f'{subject.name} {subject.minimum} or later required; found {output}')


class ToolMinimumTests(ToolCase):
    def test_the_table_has_every_tool_once(self):
        names = [candidate.name for candidate in doctor.TOOLS]
        self.assertEqual(sorted(names), sorted(SAMPLES))
        self.assertEqual(len(names), len(set(names)))
        for candidate in doctor.TOOLS:
            self.assertRegex(candidate.minimum, r'^\d+(\.\d+){0,2}$', candidate.name)

    def test_a_tool_at_or_above_its_minimum_passes(self):
        for candidate in doctor.TOOLS:
            template, _, at, above = SAMPLES[candidate.name]
            for version in (at, above):
                with self.subTest(tool=candidate.name, version=version):
                    line = self.check(candidate, template.format(version))
                    self.assertRegex(line, rf'^{re.escape(candidate.name)}: {re.escape(version)}\S* '
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
                with self.assertRaisesRegex(doctor.DoctorError,
                                            'or later required; found Apple LLVM version 21$'):
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
                self.assertTrue(line.startswith(f'GNU ld: not found, ld is {output} '), line)
                self.assertIn('requires 2.43 or later', line)

    def test_linux_only_tools_are_skipped_elsewhere(self):
        linux_only = {candidate.name for candidate in doctor.TOOLS if candidate.platform == 'linux'}
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


SEMVER_TOOLS = ('just', 'cargo-nextest', 'cargo-c', 'cargo-fuzz', 'cargo-llvm-cov')


def semver_line(subject, version):
    """The first line `<tool> --version` prints for VERSION. cargo-nextest adds a commit and a date."""
    trailer = ' (60fa45f63 2026-08-04)' if subject.name == 'cargo-nextest' else ''
    return f'{subject.name} {version}{trailer}'


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
    """just, cargo-nextest, cargo-c, cargo-fuzz and cargo-llvm-cov print semantic versions
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

    def test_a_prerelease_at_the_minimum_fails(self):
        for candidate in self.semver_tools():
            for suffix in ('-beta.1', '-rc.1', '-alpha', '-0', '-rc.1+build.5'):
                with self.subTest(tool=candidate.name, version=candidate.minimum + suffix):
                    self.assert_fails(candidate, candidate.minimum + suffix)

    def test_the_release_at_the_minimum_passes(self):
        for candidate in self.semver_tools():
            with self.subTest(tool=candidate.name):
                self.assert_passes(candidate, candidate.minimum)

    def test_a_release_with_build_metadata_passes(self):
        for candidate in self.semver_tools():
            for suffix in ('+build.5', '+20261023', '+cargo-0.98.0'):
                with self.subTest(tool=candidate.name, version=candidate.minimum + suffix):
                    self.assert_passes(candidate, candidate.minimum + suffix)

    def test_a_newer_prerelease_passes(self):
        for candidate in self.semver_tools():
            for version in (next_minor(candidate.minimum) + '-beta.1',
                            next_patch(candidate.minimum) + '-rc.1',
                            next_patch(candidate.minimum) + '-rc.1+build.5'):
                with self.subTest(tool=candidate.name, version=version):
                    self.assert_passes(candidate, version)

    def test_a_prerelease_of_an_older_release_fails(self):
        for candidate in self.semver_tools():
            with self.subTest(tool=candidate.name):
                self.assert_fails(candidate, previous(candidate.minimum) + '-rc.1')

    def test_text_that_is_not_a_semantic_version_fails(self):
        for candidate in self.semver_tools():
            for version in (candidate.minimum.rsplit('.', 1)[0], candidate.minimum + '-',
                            'v' + candidate.minimum, candidate.minimum + '-beta..1'):
                with self.subTest(tool=candidate.name, version=version):
                    self.assert_fails(candidate, version)

    def test_cargo_c_prints_the_embedded_cargo_as_build_metadata(self):
        cargo_c = tool('cargo-c')
        self.assert_found(cargo_c, 'cargo-c 0.10.24+cargo-0.98.0', '0.10.24+cargo-0.98.0')
        self.assert_found(cargo_c, 'cargo-c 0.10.25-rc.1+cargo-0.99.0', '0.10.25-rc.1+cargo-0.99.0')
        self.assert_too_old(cargo_c, 'cargo-c 0.10.24-rc.1+cargo-0.98.0')

    def test_precedence_follows_the_specification(self):
        ordered = ['1.0.0-alpha', '1.0.0-alpha.1', '1.0.0-alpha.beta', '1.0.0-beta', '1.0.0-beta.2',
                   '1.0.0-beta.11', '1.0.0-rc.1', '1.0.0', '1.0.1-0', '1.1.0-alpha', '2.0.0-rc.1']
        keys = [doctor.semver_key(version) for version in ordered]
        self.assertEqual(keys, sorted(keys))
        self.assertEqual(len(set(keys)), len(keys))
        self.assertEqual(doctor.semver_key('1.0.0+a'), doctor.semver_key('1.0.0+b'))
        self.assertEqual(doctor.semver_key('1.0.0-rc.1+build'), doctor.semver_key('1.0.0-rc.1'))
        for text in ('1.0', '1.0.0-', '1.0.0+', 'v1.0.0', '1.0.0-a..b', ''):
            with self.subTest(text=text):
                with self.assertRaises(ValueError):
                    doctor.semver_key(text)


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

    def test_a_hyphen_suffix_is_not_a_prerelease_for_these_tools(self):
        # semver would read -15.fc42 and -3 as pre-releases and call 2.43.1-15.fc42 older than 2.43.1.
        self.assert_found(tool('GNU ld'), 'GNU ld version 2.43-1.fc42', '2.43')
        self.assert_found(tool('gcov'), 'gcov (GCC) 14.0.0 20230101 (Red Hat 14.0.0-0.3)', '14.0.0')
        self.assert_found(tool('valgrind'), 'valgrind-3.22.0-1', '3.22.0')

    def test_a_development_build_is_not_older_than_its_release(self):
        self.assert_found(tool('valgrind'), 'valgrind-3.22.0.GIT', '3.22.0.GIT')
        self.assert_found(tool('gcov'), 'gcov (GCC) 14.0.0 20230101 (experimental)', '14.0.0 experimental')
        self.assert_found(tool('GNU ld'), 'GNU ld (GNU Binutils) 2.43.0.20240101', '2.43.0.20240101')

    def test_a_snapshot_date_never_outweighs_an_earlier_component(self):
        ld = tool('GNU ld')
        self.assert_too_old(ld, 'GNU ld (GNU Binutils) 2.42.0.99999999')
        self.assert_found(ld, 'GNU ld (GNU Binutils) 2.43.0.1', '2.43.0.1')


def read(relative):
    return (ROOT / relative).read_text()


class MinimumsAgreeWithTheirOtherHomes(unittest.TestCase):
    """A minimum that lives in a tool's own configuration must match doctor's."""

    def minimum(self, name):
        return tool(name).minimum

    def test_uv_floor_is_uv_toml(self):
        match = re.search(r'^required-version = ">=(\d+\.\d+\.\d+)"$', read('uv.toml'), re.M)
        self.assertIsNotNone(match)
        self.assertEqual(match.group(1), doctor.UV_MINIMUM)

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
        for variable, expected in (('uv_minimum', doctor.UV_MINIMUM),
                                   ('valgrind_minimum', self.minimum('valgrind'))):
            with self.subTest(variable=variable):
                match = re.search(rf'^{variable}=(\S+)$', setup, re.M)
                self.assertIsNotNone(match, f'setup.sh sets no {variable}')
                self.assertEqual(match.group(1), expected)


if __name__ == '__main__':
    unittest.main()
