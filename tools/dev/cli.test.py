#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Unit tests for tools.dev CLI diagnostics and exit statuses."""
import contextlib
import io
import json
import os
from pathlib import Path
import re
import sys
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import cli, host, registry, slot, test_support

IMPLEMENTATION = 'legacy:tools/install-python.sh'
COMMAND = ['bash', '{root}/tools/install-python.sh']


class RealJustSmokeTests(unittest.TestCase):
    """Use CI's pinned real just and managed Python; no command mocks."""

    def invoke(self, *args):
        return subprocess.run(['just', '--justfile', str(ROOT / 'justfile'), *args],
                              cwd=ROOT, capture_output=True, text=True, timeout=30)

    def test_real_justfile_lists_its_public_recipes(self):
        result = self.invoke('--list')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('test-tools', result.stdout)
        self.assertIn('audit-glib-call-sites', result.stdout)
        self.assertNotIn('_test-argv', result.stdout)

    def test_real_recipe_preserves_arguments_and_nonzero_exit(self):
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / 'must-not-exist'
            values = ['path with spaces', '', 'literal;$HOME', '*?[abc]',
                      'double" and single\' quotes', r'back\slash', 'line\nbreak',
                      f'$(touch "{marker}")', f'`touch "{marker}"', '--flag']
            for status in (0, 23):
                with self.subTest(status=status):
                    result = self.invoke('_test-argv', str(status), *values)
                    self.assertEqual(result.returncode, status, result.stderr)
                    self.assertEqual(json.loads(result.stdout), values)
                    self.assertFalse(marker.exists(), 'shell expanded a literal argument')

    def test_every_uv_run_recipe_except_doctor_depends_on_python(self):
        result = self.invoke('--dump', '--dump-format', 'json')
        self.assertEqual(result.returncode, 0, result.stderr)
        recipes = json.loads(result.stdout)['recipes']
        users = {name for name, recipe in recipes.items()
                 if any('uv run' in fragment for line in recipe['body'] for fragment in line)}
        self.assertIn('check', users)
        self.assertIn('heavy', users)
        for name in sorted(users - {'doctor'}):
            with self.subTest(recipe=name):
                self.assertIn('python', [dependency['recipe'] for dependency in recipes[name]['dependencies']])
        self.assertEqual(recipes['doctor']['dependencies'], [])

    def test_every_platform_variant_of_a_uv_run_recipe_depends_on_python(self):
        header = re.compile(r'^(@?[A-Za-z_][\w-]*)(?: [^:]*)?:(.*)$')
        blocks = []
        for line in (ROOT / 'justfile').read_text().splitlines():
            match = header.match(line)
            if match and not line.startswith(('set ', 'default')):
                blocks.append([match.group(1), match.group(2), []])
            elif blocks and line.startswith(' '):
                blocks[-1][2].append(line)
        users = [(name, dependencies) for name, dependencies, body in blocks
                 if name != 'doctor' and any('uv run' in line for line in body)]
        self.assertGreaterEqual(len(users), 20)
        for name, dependencies in users:
            with self.subTest(recipe=name):
                self.assertIn('python', dependencies.split())

    def test_doctor_without_the_interpreter_names_just_python_and_installs_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'uv.log'
            fake = Path(directory) / 'uv'
            fake.write_text(f'#!/bin/sh\necho "$@" >> {log}\nexit 2\n')
            fake.chmod(0o755)
            env = {**os.environ, 'PATH': f'{directory}{os.pathsep}{os.environ["PATH"]}'}
            result = subprocess.run(['just', '--justfile', str(ROOT / 'justfile'), 'doctor'],
                                    cwd=ROOT, capture_output=True, text=True, timeout=30, env=env)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('the managed interpreter in .python-version is not installed: run `just python`', result.stderr)
            self.assertEqual(log.read_text().splitlines(),
                             ['python find --managed-python'])


class CliTests(unittest.TestCase):
    def setUp(self):
        self.enterContext(mock.patch.dict(os.environ, {'PATH': os.environ['PATH']}, clear=True))
        self.launch = self.enterContext(mock.patch.object(
            slot.subprocess, 'Popen', side_effect=AssertionError('CLI unit test launched a process')))

    def test_expected_slot_failures_print_one_line_without_a_traceback(self):
        failures = (
            (host.ResourceBlocked('resource-blocked: available memory unknown'), 1),
            (slot.Cancelled('cancelled; child process groups terminated and reaped'), 130),
            (ValueError('OXPINYIN_LOCK_DIR must be an absolute path'), 1),
            (ValueError('Invalid OXPINYIN_HEAVY_SLOT_HELD'), 1),
            (OSError('child launch\nfailed'), 1),
        )
        for error, expected in failures:
            with self.subTest(error=error), mock.patch.object(
                    slot, 'run_command', side_effect=error), contextlib.redirect_stderr(io.StringIO()) as err:
                self.assertEqual(cli.main(['slot', 'run', '--', 'child']), expected)
            self.assertEqual(err.getvalue().splitlines(),
                             ['slot run: ' + ' '.join(str(error).splitlines())])

    def test_negative_child_status_uses_the_shell_signal_convention(self):
        for signum in (15, 9):
            with self.subTest(signal=signum), mock.patch.object(
                    slot, 'run_command', return_value=-signum):
                self.assertEqual(cli.main(['slot', 'run', '--', 'child']), 128 + signum)

    def test_nonnegative_child_status_and_arguments_are_preserved(self):
        for status in (0, 1, 7, 77):
            with self.subTest(status=status), mock.patch.object(
                    slot, 'run_command', return_value=status) as run:
                command = ['child', '--flag', '', '/path with spaces', 'literal;$value']
                self.assertEqual(cli.main(['slot', 'run', '--', *command]), status)
                test_support.assert_calls(self, run, [mock.call(command)])

    def test_test_commands_forward_raw_arguments_before_argparse(self):
        selections = (['-p', 'oxpinyin-core'], ['--workspace'],
                      ['--test-threads=2'], ['-P', 'ci'], ['--', '-p', 'oxpinyin-core'])
        for command, target in (('test', 'run'), ('test-doc', 'run_doc')):
            for selection in selections:
                with self.subTest(command=command, selection=selection), \
                        mock.patch.object(cli.test_runner, target, return_value=7) as run:
                    self.assertEqual(cli.main([command, *selection]), 7)
                    run.assert_called_once_with(selection)
        test_support.assert_not_called(self, self.launch)

    def test_tools_dispatches_to_shared_harness_runner(self):
        with mock.patch.object(cli.test_runner, 'run_tools', return_value=1) as run:
            self.assertEqual(cli.main(['test-tools']), 1)
        run.assert_called_once_with([], None, None)

    def test_tools_forwards_selection_options(self):
        cases = ((['tools/dev/test_runner.test.py', '-k', 'HarnessTests'],
                  (['tools/dev/test_runner.test.py'], 'HarnessTests', None)),
                 (['--changed'], ([], None, 'HEAD')),
                 (['--changed', 'origin/main'], ([], None, 'origin/main')))
        for argv, expected in cases:
            with self.subTest(argv=argv), \
                    mock.patch.object(cli.test_runner, 'run_tools', return_value=7) as run:
                self.assertEqual(cli.main(['test-tools', *argv]), 7)
                run.assert_called_once_with(*expected)

    def test_coverage_forwards_raw_arguments_before_argparse(self):
        args = ['--locked', '-p', 'oxpinyin-chewing', '--', '', '/path with spaces', 'literal;$value']
        with mock.patch.object(cli.coverage, 'run', return_value=77) as run:
            self.assertEqual(cli.main(['coverage', *args]), 77)
            run.assert_called_once_with(args)
        test_support.assert_not_called(self, self.launch)

    def test_test_options_reach_the_slot_without_a_separator(self):
        for selection in (['-p', 'oxpinyin-core'], ['--workspace'],
                          ['--test-threads=2'], ['-P', 'ci']):
            with self.subTest(selection=selection), \
                    mock.patch.object(slot, 'run_command', return_value=0) as run:
                self.assertEqual(cli.main(['test', *selection]), 0)
                self.assertEqual(run.call_args.args[0][-len(selection):], selection)
        test_support.assert_not_called(self, self.launch)

    def test_main_reads_raw_arguments_from_sys_argv(self):
        with mock.patch.object(sys, 'argv', ['tools.dev', 'test', '--workspace']), \
                mock.patch.object(cli.test_runner, 'run', return_value=7) as run:
            self.assertEqual(cli.main(), 7)
            run.assert_called_once_with(['--workspace'])
        test_support.assert_not_called(self, self.launch)

    def test_registry_commands_reject_empty_unknown_and_unmatched_selections(self):
        selections = ([], ['--runners', ''], ['--runners', ','],
                      ['--runners', 'bisect,'], ['--runners', 'bisect,missing'],
                      ['--cells', ''], ['--cells', 'bdb,missing'],
                      ['--issue', ''], ['--issue', 'missing'],
                      ['--kind', 'missing'],
                      ['--runners', 'bisect', '--issue', 'W11'])
        for selection in selections:
            with self.subTest(selection=selection), \
                    contextlib.redirect_stdout(io.StringIO()) as output, \
                    contextlib.redirect_stderr(io.StringIO()) as error:
                self.assertEqual(cli.main(['plan', *selection]), 2)
                self.assertEqual(output.getvalue(), '')
                self.assertTrue(error.getvalue().startswith('plan: '))
                self.assertNotIn('Traceback', error.getvalue())
        test_support.assert_not_called(self, self.launch)

    def test_registry_listing_filters_static_issues_kind_and_heavy_state(self):
        for selector in ('row:33', 'Row 33'):
            with self.subTest(selector=selector), \
                    contextlib.redirect_stdout(io.StringIO()) as output:
                self.assertEqual(cli.main(['list', '--issue', selector, '--kind', 'gate']), 0)
                self.assertEqual([line.split('\t')[0] for line in output.getvalue().splitlines()],
                                 ['abi-probe-diff', 'residue-a-tail-diff'])
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(cli.main(['list', '--runners', 'bisect', '--heavy']), 0)
            self.assertEqual(output.getvalue(), 'bisect\tgate\tbdb,kc,tkrzw\theavy\t'
                             'python:tools.dev\n')
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(cli.main(['list', '--runners', 'perf-baseline']), 0)
        self.assertEqual(output.getvalue().split('\t')[-1], 'python:tools.dev\n')
        self.assertNotIn('registered', output.getvalue())
        self.assertNotIn('measurement recipe', output.getvalue())
        test_support.assert_not_called(self, self.launch)

    def test_plan_describes_templates_without_resolving_or_executing_them(self):
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(cli.main(['plan', '--runners', 'pc-metadata', '--cells', 'kc']), 0)
        plan = output.getvalue()
        self.assertIn('executes nothing', plan)
        self.assertIn('cells: kc;', plan)
        self.assertNotIn('cells: bdb,kc,tkrzw;', plan)
        self.assertIn('{env[OXPINYIN_METADATA_PIN_ROOT]}', plan)
        self.assertIn('command template:', plan)
        self.assertIn('\n  implementation: python:tools.dev\n', plan)
        test_support.assert_not_called(self, self.launch)

    def test_plan_prints_the_implementation_with_each_row(self):
        with contextlib.redirect_stdout(io.StringIO()) as output:
            self.assertEqual(cli.main(['plan', '--runners', 'bisect,pc-metadata']), 0)
        lines = [line for line in output.getvalue().splitlines() if line.startswith('  implementation: ')]
        self.assertEqual(lines, ['  implementation: python:tools.dev',
                                 '  implementation: python:tools.dev'])
        test_support.assert_not_called(self, self.launch)

    def test_registry_commands_report_a_missing_implementation_without_tracebacks(self):
        rows = [dict(name='fixture', command=['unused'], cells=['bdb'])]
        for command, selection in (('list', []), ('plan', ['--runners', 'fixture'])):
            with self.subTest(command=command), \
                    mock.patch.object(Path, 'read_text', return_value=json.dumps(rows)), \
                    contextlib.redirect_stdout(io.StringIO()) as output, \
                    contextlib.redirect_stderr(io.StringIO()) as error:
                self.assertEqual(cli.main([command, *selection]), 2)
                self.assertEqual(output.getvalue(), '')
                self.assertEqual(error.getvalue(),
                                 f"{command}: registry row 'fixture' has no implementation\n")
        test_support.assert_not_called(self, self.launch)

    def test_registry_read_failure_has_a_nonzero_diagnostic(self):
        for command, selection in (('list', []), ('plan', ['--runners', 'bisect'])):
            with self.subTest(command=command), \
                    mock.patch.object(registry, 'load', side_effect=registry.RegistryError('bad registry')), \
                    contextlib.redirect_stderr(io.StringIO()) as error:
                self.assertEqual(cli.main([command, *selection]), 2)
                self.assertEqual(error.getvalue(), f'{command}: bad registry\n')
        test_support.assert_not_called(self, self.launch)

    def test_registry_load_rejects_numeric_issue_labels(self):
        for issues in ([541], ['541', 542], 541):
            rows = [dict(name='fixture', command=COMMAND, cells=['bdb'], implementation=IMPLEMENTATION, issues=issues)]
            with self.subTest(issues=issues), mock.patch.object(
                    Path, 'read_text', return_value=json.dumps(rows)):
                with self.assertRaisesRegex(registry.RegistryError,
                                            "registry row 'fixture' issues must be a list of strings"):
                    registry.load()
        test_support.assert_not_called(self, self.launch)

    def test_registry_commands_report_numeric_issue_labels_without_tracebacks(self):
        rows = [dict(name='fixture', command=COMMAND, cells=['bdb'], implementation=IMPLEMENTATION, issues=[541])]
        for command, selection in (('list', []), ('plan', ['--runners', 'fixture'])):
            with self.subTest(command=command), \
                    mock.patch.object(Path, 'read_text', return_value=json.dumps(rows)), \
                    contextlib.redirect_stdout(io.StringIO()) as output, \
                    contextlib.redirect_stderr(io.StringIO()) as error:
                self.assertEqual(cli.main([command, *selection]), 2)
                self.assertEqual(output.getvalue(), '')
                self.assertEqual(error.getvalue(),
                                 f"{command}: registry row 'fixture' issues must be a list of strings\n")
        test_support.assert_not_called(self, self.launch)

    def test_registry_numeric_string_issue_labels_remain_selectable(self):
        rows = [dict(name='fixture', command=COMMAND, cells=['bdb'], implementation=IMPLEMENTATION, issues=['541', 'Row 33'])]
        with mock.patch.object(Path, 'read_text', return_value=json.dumps(rows)):
            loaded = registry.load()
        self.assertEqual(loaded, rows)
        for issue in ('541', '#541', '  #541  ', 'row:33', 'Row 33'):
            with self.subTest(issue=issue):
                self.assertEqual(registry.select(loaded, issues=[issue]), rows)
        test_support.assert_not_called(self, self.launch)


if __name__ == '__main__':
    unittest.main()
