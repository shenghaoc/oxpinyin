"""Path selection and aggregate regressions."""
import contextlib
import io
import json
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import ci_tree

HEAD = 'b2ae9126' + '0' * 32


class AggregateTests(unittest.TestCase):
    def invoke_main(self, outputs, results, changes='success'):
        needs = {name: {'result': result} for name, result in results.items()}
        needs['changes'] = {'result': changes, 'outputs': outputs}
        with mock.patch.object(sys, 'argv', ['ci_tree.py', 'aggregate']), \
                mock.patch.dict(ci_tree.os.environ, {'NEEDS': json.dumps(needs)}, clear=True), \
                contextlib.redirect_stdout(io.StringIO()):
            return ci_tree.main()

    def test_each_lane_follows_its_own_flag(self):
        # Exercise each job selected alone, as well as all-selected and none-selected.
        selections = [set(), set(ci_tree.JOB_FLAGS), *({job} for job in ci_tree.JOB_FLAGS)]
        for selected in selections:
            outputs = {flag: 'true' if job in selected else 'false'
                       for job, flag in ci_tree.JOB_FLAGS.items()}
            results = {job: 'success' if job in selected else 'skipped' for job in ci_tree.JOB_FLAGS}
            with self.subTest(selected=selected):
                self.assertEqual(self.invoke_main(outputs, results), 0)
                for job in results:
                    for result in ('success', 'skipped', 'failure', 'cancelled', ''):
                        if result != results[job]:
                            self.assertEqual(self.invoke_main(outputs, {**results, job: result}), 1)

    def test_invalid_or_failed_gate_never_passes(self):
        outputs = dict.fromkeys(ci_tree.JOB_FLAGS.values(), 'false')
        results = dict.fromkeys(ci_tree.JOB_FLAGS, 'skipped')
        for status in ('failure', 'cancelled', 'skipped', ''):
            self.assertEqual(self.invoke_main(outputs, results, changes=status), 1)
        for flag in outputs:
            for invalid in ('', 'wat'):
                self.assertEqual(self.invoke_main({**outputs, flag: invalid}, results), 1)
            self.assertEqual(self.invoke_main({key: value for key, value in outputs.items() if key != flag},
                                              results), 1)
        self.assertFalse(ci_tree.aggregate('success', {}, results))


class GateTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.root = Path(self.directory.name)
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'user.name', 'Shenghao Chen')
        self.git('config', 'user.email', 'shenghaoc@outlook.com')
        self.base = self.commit({'README.md': 'r\n', 'capi/lib': 'a\n'})

    def tearDown(self):
        self.directory.cleanup()

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.root), *args], text=True).strip()

    def commit(self, files):
        for name, text in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        self.git('add', '-A')
        self.git('commit', '-q', '-m', 'change')
        return self.git('rev-parse', 'HEAD')

    def selected(self, path):
        self.git('reset', '-q', '--hard', self.base)
        self.commit({path: 'changed\n'})
        flags = ci_tree.gate(self.base, root=self.root)
        return {name for name, value in flags.items() if value}

    def test_a_missing_or_unknown_base_selects_every_job(self):
        everything = set(ci_tree.CI_INPUTS)
        for base in ('', '0' * 40, 'HEAD'):
            with self.subTest(base=base):
                self.assertEqual({name for name, value in ci_tree.gate(base, root=self.root).items() if value},
                                 everything)

    def test_paths_select_the_jobs_that_read_them(self):
        cases = {
            # The Windows tkrzw pin is a tools/ file: the Rust lanes import tools.dev, the harness tests it.
            'tools/tkrzw/pin.json': {'rust', 'harness'},
            'docs/note.md': set(),
            'docs/findings/compatibility-policy.md': set(),
            'AGENTS.md': set(),
            # The store gate script and its arguments are harness inputs.
            'tools/bisection/cells-for-diff.py': {'harness'},
            '.github/workflows/verify-nightly.yml': set(),
            'tools/bisection/contract_axis_c.py': {'harness'},
            'tools/dev/ci_tree.test.py': {'harness'},
            'tools/oracle/build.py': {'rust', 'harness'},
            'tools/dev/lint-crate-cfg-placement.test.py': {'rust', 'harness'},
            'tools/dev/abi.py': {'rust', 'harness'},
            'tools/dev/new_module.py': {'rust', 'harness'},
            'tools/oracle/oracle-pin.txt': {'rust', 'harness', 'ship'},
            'tools/measurement/src/workload.rs': {'rust', 'harness'},
            # A Rust-only change does not select the harness. Only the root library and the
            # two C API crates ship.
            'repl/src/lib.rs': {'rust'},
            'utils/src/lib.rs': {'rust'},
            'src/lib.rs': {'rust', 'ship'},
            'capi/src/lib.rs': {'rust', 'ship'},
            'zhuyin-capi/src/lib.rs': {'rust', 'ship'},
            'codegen/store.rs': {'rust', 'harness', 'ship'},
            'build.rs': {'rust', 'harness', 'ship'},
            'tests/suite.rs': {'rust'},
            'capi/tests/it/main.rs': {'rust'},
            'zhuyin-capi/tests/it/main.rs': {'rust'},
            'src/table_data/mod.rs': {'rust', 'ship'},
            'benches/new.rs': {'rust'},
            'examples/new.rs': {'rust'},
            # The files directly in a crate's directory are harness inputs: packaging.test.py copies them.
            'capi/build.rs': {'rust', 'harness', 'ship'},
            'zhuyin-capi/build.rs': {'rust', 'harness', 'ship'},
            'repl/Cargo.toml': {'rust', 'harness'},
            'capi/libpinyin.ver': {'rust', 'harness', 'ship'},
            'zhuyin-capi/novel_types.h': {'rust', 'harness', 'ship'},
            'fixtures/w3/tkt/bigram.db': {'rust', 'harness'},
            'data/table.conf.in': {'harness'},
            'fuzz/Cargo.lock': {'rust', 'harness'},
            'fuzz/fuzz_targets/x.rs': {'rust'},
            'fuzz/seeds/capi-commands/choose': {'rust', 'harness'},
            'fuzz/dictionaries/table-conf.dict': {'rust', 'harness'},
            'fuzz/rust-toolchain.toml': {'rust', 'harness'},
            'deny.toml': {'rust'},
            'clippy.toml': {'rust'},
            '.config/nextest.toml': {'rust'},
            'Cargo.lock': {'rust', 'harness', 'ship'},
            'rust-toolchain.toml': {'rust', 'harness', 'ship'},
            '.python-version': {'rust', 'harness'},
            # Every job that runs repository Python enters the uv project.
            'pyproject.toml': {'rust', 'harness', 'ship'},
            # Only the jobs that run a dependency group read the lockfile: Lizard in rust, Ruff in
            # the harness.
            'uv.lock': {'rust', 'harness'},
            'justfile': {'rust', 'harness', 'ship'},
            '.github/actions/bootstrap/action.yml': {'rust', 'harness', 'ship'},
            '.github/workflows/ci.yml': {'rust', 'harness', 'ship'},
            'scripts/new.sh': set(),
            'tools/bisection/契约.py': {'harness'},
        }
        for path, jobs in cases.items():
            with self.subTest(path=path):
                self.assertEqual(self.selected(path), jobs)
        with self.subTest(rename='capi/lib -> docs/lib'):
            self.git('reset', '-q', '--hard', self.base)
            self.git('rm', '-q', 'capi/lib')
            self.commit({'docs/lib': 'a\n'})
            self.assertEqual(self.git('diff', '--name-status', self.base, 'HEAD'),
                             'R100\tcapi/lib\tdocs/lib')
            flags = ci_tree.gate(self.base, root=self.root)
            self.assertEqual({name for name, value in flags.items() if value}, {'rust', 'harness', 'ship'})

    def test_crate_dirs_are_the_workspace_members(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.toml').write_text('[package]\nname = "a"\n\n'
                                             '[workspace]\nmembers = ["one", "nested/two"]\n')
            self.assertEqual(ci_tree.workspace_members(root), ('one', 'nested/two'))
        self.assertEqual(ci_tree.CRATE_DIRS, ci_tree.workspace_members(ROOT))
        for directory in ci_tree.CRATE_DIRS:
            with self.subTest(directory=directory):
                self.assertTrue((ROOT / directory / 'Cargo.toml').is_file())
                ship = {'ship'} if directory in ci_tree.SHIP_CRATES else set()
                self.assertEqual(self.selected(f'{directory}/Cargo.toml'),
                                 {'rust', 'harness'} | ship)
        self.assertLessEqual(set(ci_tree.SHIP_CRATES), set(ci_tree.CRATE_DIRS))

    def test_every_generated_table_selects_the_harness(self):
        from tools.dev import rust_tables
        self.assertEqual(set(ci_tree.GENERATED_TABLES), set(rust_tables.render()),
                         'GENERATED_TABLES must name exactly the files rust-tables writes')
        self.assertEqual(len(ci_tree.GENERATED_TABLES), 13)
        for path in ci_tree.GENERATED_TABLES:
            with self.subTest(path=path):
                self.assertTrue((ROOT / path).is_file())
                self.assertEqual(self.selected(path), {'rust', 'harness', 'ship'})

    def test_no_step_of_a_lane_depends_on_a_path_flag(self):
        workflow = (ROOT / '.github/workflows/ci.yml').read_text()
        outputs = set(re.findall(r'needs\.changes\.outputs\.(\w+)', workflow))
        self.assertEqual(outputs, set(ci_tree.JOB_FLAGS.values()))
        self.assertEqual({*ci_tree.CI_INPUTS, 'store'}, outputs)
        self.assertEqual(set(re.findall(r'^  ([a-z-]+):$', workflow.split('\njobs:\n', 1)[1], re.MULTILINE)),
                         {'changes', *ci_tree.JOB_FLAGS, 'ci-aggregate'})
        for job, flag in ci_tree.JOB_FLAGS.items():
            block = re.split(r'\n  [a-z-]+:\n', workflow.split('\n  ' + job + ':\n', 1)[1], maxsplit=1)[0]
            self.assertIn("    if: needs.changes.outputs." + flag + " == 'true'\n", block)
        steps = re.findall(r'^ {6,}(?:- )?if: (.*)$', workflow, re.MULTILINE)
        for condition in steps:
            with self.subTest(condition=condition):
                self.assertNotRegex(condition, r'needs\.changes\.outputs\.')

    def test_the_store_gate_runs_the_backend_rule_with_its_infrastructure(self):
        self.assertTrue(ci_tree.STORE_GATE_SCRIPT.is_file())
        for path in ('src/store/mod.rs', 'src/lib.rs', 'Cargo.lock', 'utils/tests/cell.rs', 'tools/dev/test_runner.py',
                     'capi/libpinyin.alloc', 'zhuyin-capi/zhuyin.h', 'tools/abi/tests/abi.rs', 'tools/dev/abi.py',
                     '.github/workflows/ci.yml', 'fuzz/rust-toolchain.toml', 'clippy.toml'):
            with self.subTest(path=path):
                self.git('reset', '-q', '--hard', self.base)
                self.commit({path: 'changed\n'})
                self.assertTrue(ci_tree.store_gate(self.base, root=self.root))
        for path in ('docs/note.md', 'repl/src/main.rs'):
            with self.subTest(path=path):
                self.git('reset', '-q', '--hard', self.base)
                self.commit({path: 'changed\n'})
                self.assertFalse(ci_tree.store_gate(self.base, root=self.root))
        for base in ('', '0' * 40):
            with self.subTest(base=base):
                self.assertTrue(ci_tree.store_gate(base, root=self.root))

    def test_event_base(self):
        self.assertEqual(ci_tree.event_base('pull_request', {'pull_request': {'base': {'sha': HEAD}}}), HEAD)
        self.assertEqual(ci_tree.event_base('push', {'before': HEAD}), HEAD)
        self.assertEqual(ci_tree.event_base('push', {}), '')


if __name__ == '__main__':
    unittest.main()
