#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Fixture tests for tools/perf-gate/callgrind-ir.py.

Each fixture isolates one property of the callgrind format that the first version
of this parser got wrong. The expected numbers are hand-computed from the fixture
text, which is the point: they are checkable by reading. Every fixture is read by
the function and by the command line, whose output is the integer or `null`.
"""
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().with_name('callgrind-ir.py')

# name -> (what it proves, fixture text, object substring, expected total or None)
FIXTURES = {
    'plain': ('plain self costs sum', '''positions: line
events: Ir
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 100
11 200
''', 'libpinyin', 300),
    'two-obs': ('another object is excluded', '''positions: line
events: Ir
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 100
ob=/lib/libc.so.6
fl=/src/c.c
fn=memcpy
20 9999
''', 'libpinyin', 100),
    # The first version tested the line text for "libpinyin", so it counted the 100
    # under the definition and silently dropped the 250 under `ob=(1)`.
    'compressed': ('compressed ob back-reference counts', '''positions: line
events: Ir
ob=(1) /usr/lib/libpinyin.so.15
fl=(1) /src/a.c
fn=(1) alpha
10 100
ob=(2) /lib/libc.so.6
fl=(2) /src/c.c
fn=(2) memcpy
20 9999
ob=(1)
fl=(1)
fn=(3) beta
30 250
''', 'libpinyin', 350),
    # Naive summing adds the 5000 inclusive cost and reports 5150.
    'calls': ('call-inclusive cost is skipped', '''positions: line
events: Ir
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 100
cfn=callee
calls=3 42
10 5000
11 50
''', 'libpinyin', 150),
    # With Ir second, a fixed first-column reader returns the Dr values (7+9=16).
    'events': ('Ir column chosen from events:', '''positions: line
events: Dr Ir Dw
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 7 100 3
11 9 200 4
''', 'libpinyin', 300),
    'instr': ('two position columns', '''positions: instr line
events: Ir
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
0x1000 10 100
0x1004 11 200
''', 'libpinyin', 300),
    'cob': ('cob= does not retarget self cost', '''positions: line
events: Ir
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 100
cob=/lib/libc.so.6
cfn=memcpy
calls=1 0
10 4000
11 25
''', 'libpinyin', 125),
    'nomatch': ('no records for the object is null', '''positions: line
events: Ir
ob=/lib/libc.so.6
fl=/src/c.c
fn=memcpy
20 42
''', 'libpinyin', None),
    'noir': ('a file with no Ir event is null', '''positions: line
events: Dr Dw
ob=/usr/lib/libpinyin.so.15
fl=/src/a.c
fn=alpha
10 7 3
''', 'libpinyin', None),
}


def load():
    """Load the script as a module; its name has a hyphen, so it cannot be imported."""
    spec = importlib.util.spec_from_file_location('callgrind_ir', SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.work = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.module = load()

    def fixture(self, name):
        path = self.work / (name + '.out')
        path.write_text(FIXTURES[name][1])
        return path

    def command(self, *arguments):
        return subprocess.run([sys.executable, '-X', 'lazy_imports=normal', str(SCRIPT), *map(str, arguments)],
                              capture_output=True, text=True)

    def test_every_fixture_gives_its_hand_computed_total(self):
        for name, (label, _, want_object, expected) in FIXTURES.items():
            with self.subTest(fixture=name, proves=label):
                self.assertEqual(self.module.object_ir(str(self.fixture(name)), want_object), expected)

    def test_the_command_line_prints_the_total_or_null(self):
        for name, (label, _, want_object, expected) in FIXTURES.items():
            with self.subTest(fixture=name, proves=label):
                result = self.command(self.fixture(name), want_object)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, ('null' if expected is None else str(expected)) + '\n')

    def test_the_wrong_number_of_arguments_is_a_usage_error(self):
        for arguments in ((), ('only-one',), ('a', 'b', 'c')):
            with self.subTest(arguments=arguments):
                result = self.command(*arguments)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(result.stdout, '')
                self.assertIn('Usage: callgrind-ir.py <callgrind-out-file> <object-substring>', result.stderr)

    def test_an_unreadable_file_is_reported_not_counted(self):
        result = self.command(self.work / 'missing.out', 'libpinyin')
        self.assertEqual(result.returncode, 2)
        self.assertEqual(result.stdout, '')
        self.assertTrue(result.stderr.startswith('callgrind-ir: '), result.stderr)


if __name__ == '__main__':
    unittest.main()
