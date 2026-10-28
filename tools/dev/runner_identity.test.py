#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for the old/new C driver identity recipe."""
from pathlib import Path
import shutil
import string
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from tools.dev import runner_identity as identity
from tools.dev.cell import CellError, Settings
from tools.dev.runner_recipes import RECIPES
from tools.dev.system_data import SystemData

# Darwin declares mkdtemp(3) in <unistd.h>, and only under _DARWIN_C_SOURCE, as in
# bisect.c and startup.c.
PROBE = r'''
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#endif
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
int main(int argc, char **argv) {
    char path[4096];
    snprintf(path, sizeof path, "%s-XXXXXX", argv[1]);
    if (!mkdtemp(path))
        return 3;
    printf("made %s in %s\n", path, getenv("TMPDIR"));
    fprintf(stderr, "user=%s\n", argv[2]);
    return argc > 3 ? atoi(argv[3]) : 0;
}
'''


class Services:
    def run(self, argv, obj, *, runner, side, cwd, environment):
        return subprocess.run(argv, cwd=cwd, env=environment, capture_output=True)


class Data:
    """SystemData over ENVIRONMENT that records each lookup."""
    def __init__(self, environment):
        self.system, self.lookups = SystemData(Settings.read(environment)), []

    def resolve(self, variable, label):
        self.lookups.append(variable)
        return self.system.resolve(variable, label)


class IdentityTests(unittest.TestCase):
    def setUp(self):
        self.root = Path(self.enterContext(tempfile.TemporaryDirectory()))

    def test_recipe_is_dispatched(self):
        self.assertIs(RECIPES['driver-identity'], identity.identity)

    def test_normalise_names_run_directories_longest_first(self):
        run = self.root / 'run-1'
        paths = {'run': run, 'user': run / 'user', 'tmp': run / 'tmp'}
        text = f'{run}/user/a {run}/tmp/b {run}/x'.encode()
        self.assertEqual(identity.normalise(text, paths, ()), b'{user}/a {tmp}/b {run}/x')

    def test_normalise_blanks_exact_mkdtemp_suffixes_only(self):
        templates = ('/tmp/residue-mech-', '/tmp/residue-mech-b-', '{tmp}/bisect-user-')
        text = (b'/tmp/residue-mech-Ab09zZ /tmp/residue-mech-b-QwErTy '
                b'/tmp/residue-mech-abc {tmp}/bisect-user-123456')
        self.assertEqual(identity.normalise(text, {}, templates),
                         b'/tmp/residue-mech-XXXXXX /tmp/residue-mech-b-XXXXXX '
                         b'/tmp/residue-mech-abc {tmp}/bisect-user-XXXXXX')

    def test_differing_names_each_field(self):
        base = identity.Observation(0, b'a', b'b', ())
        self.assertEqual(identity.differing(base, base), [])
        self.assertEqual(identity.differing(base, identity.Observation(1, b'a', b'c', ('x',))),
                         ['status', 'stderr', 'left'])

    def test_declared_drivers_and_cases_are_well_formed(self):
        values = {name: name for name in identity.PLACEHOLDERS}
        for stem, driver in identity.DRIVERS.items():
            with self.subTest(stem=stem):
                self.assertTrue((ROOT / 'tools/bisection' / f'{stem}.c').is_file())
                names = [case.name for case in driver.cases]
                self.assertEqual(len(names), len(set(names)))
                self.assertIn(driver.facade, ('pinyin', 'zhuyin'))
                self.assertIn(driver.pin_tables, ('pin', 'subject'))
                for template in driver.templates:
                    self.assertTrue(template.endswith('-') and ' ' not in template, template)
                for case in driver.cases:
                    self.assertTrue(case.sides and set(case.sides) <= set(identity.SIDES), case)
                    for text in (*case.args, *(value for _, value in case.env)):
                        fields = {field for _, field, _, _ in string.Formatter().parse(text) if field}
                        self.assertLessEqual(fields, set(identity.PLACEHOLDERS), text)
                        text.format(**values)

    def test_extract_keeps_the_c_sources_of_a_revision(self):
        ctx = SimpleNamespace(settings=SimpleNamespace(root=ROOT), write=lambda *a, **k: None,
                              command=lambda argv, capture: subprocess.run(list(map(str, argv)),
                                                                           capture_output=True))
        tree = identity.extract(ctx, 'HEAD', self.root / 'tree')
        self.assertTrue((tree / 'bisect.c').is_file())
        self.assertTrue((tree / 'teardown.h').is_file())
        self.assertFalse(any(tree.glob('*.py')))

    def test_libraries_resolves_subject_tables_only_when_a_side_reads_them(self):
        tables, absent = self.root / 'tables', self.root / 'absent'
        tables.mkdir()
        for name in ('gb_char.bin', 'pinyin_index.bin', 'phrase_index.bin', 'bigram.db'):
            (tables / name).touch()
        (tables / 'table.conf').write_text('database format:BerkeleyDB\n')
        pin_so, pin_data = Path('/pin/lib/libpinyin.so'), Path('/pin/lib/libpinyin/data')

        def libraries(stem, sides, environment):
            ctx = SimpleNamespace(pin=lambda require_data: (pin_so, pin_data), data=Data(environment),
                                  artifact=lambda facade: Path(f'/subject/{facade}.so'))
            return identity.libraries(ctx, identity.DRIVERS[stem], sides, self.root), ctx.data.lookups

        # A refusal-only selection (scheme-diff --case usage) opens no tables.
        for environment in ({}, {'OXPINYIN_SYSTEM_DIR': str(absent)}):
            with self.subTest(environment=environment):
                inputs, lookups = libraries('scheme-diff', {'none'}, environment)
                self.assertEqual(lookups, [])
                self.assertEqual(inputs, {'pin': (pin_so, pin_data), 'none': (None, pin_data)})
        inputs, lookups = libraries('scheme-diff', {'subject'}, {'OXPINYIN_SYSTEM_DIR': str(tables)})
        self.assertEqual(lookups, ['OXPINYIN_SYSTEM_DIR'])
        self.assertEqual(inputs['subject'], (Path('/subject/pinyin.so'), tables))
        self.assertEqual(inputs['pin'], (pin_so, pin_data))
        # zhuyin's pin reads the subject's tables, so even its refusal cases resolve them.
        inputs, lookups = libraries('zhuyin-diff', {'none'}, {'OXPINYIN_SYSTEM_DIR': str(tables)})
        self.assertEqual(lookups, ['OXPINYIN_SYSTEM_DIR'])
        self.assertEqual(inputs, {'pin': (pin_so.parent / 'libzhuyin.so.15.0.0', tables), 'none': (None, tables)})
        with self.assertRaisesRegex(CellError, 'system directory is incomplete'):
            libraries('zhuyin-diff', {'none'}, {'OXPINYIN_SYSTEM_DIR': str(absent)})

    @unittest.skipUnless(shutil.which('cc'), 'needs a C compiler')
    def test_observe_normalises_fresh_directories_and_reports_leaks(self):
        source = self.root / 'probe.c'
        source.write_text(PROBE)
        binary = self.root / 'probe'
        subprocess.run(['cc', '-o', str(binary), str(source)], check=True)
        template = str(self.root / 'leak-')
        driver = identity.Driver(cases=(), templates=(template,))
        stage = self.root / 'stage'
        stage.mkdir()
        ctx = SimpleNamespace(env={'PATH': '/usr/bin:/bin'}, name='driver-identity', services=Services(),
                              here=ROOT / 'tools/bisection')
        runs = self.root / 'runs'
        runs.mkdir()
        seen = [identity.observe(ctx, driver, binary, stage / 'probe',
                                 identity.Case('leak', ('none',), (template.removesuffix('-'), '{user}', status)),
                                 'none', (None, self.root), self.root, runs)
                for status in ('0', '0', '4')]
        self.assertEqual(seen[0], seen[1])
        self.assertEqual(seen[0].stdout, f'made {template}XXXXXX in {{tmp}}\n'.encode())
        self.assertEqual(seen[0].stderr, b'user={user}\n')
        self.assertEqual(seen[0].left, (template + 'XXXXXX',))
        self.assertEqual(identity.differing(seen[0], seen[2]), ['status'])
        self.assertEqual(list(self.root.glob('leak-*')), [])


if __name__ == '__main__':
    unittest.main()
