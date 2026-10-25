#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The oracle image's build stage runs tools/dev/pool.py with the deps stage's python3 (#1234).

Checks that the stage copies exactly the files the module imports, that the image tag hashes them, and
that the stage's own command line runs the pool with a stub build-oracle.sh in a tree holding only those
files. No platform is skipped.
"""
import ast
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
DOCKERFILE = (ROOT / 'tools/oracle/Dockerfile.oracle').read_text()
BUILD = DOCKERFILE[DOCKERFILE.index('FROM deps AS build'):DOCKERFILE.index('FROM --platform=$ORACLE_PLATFORM scratch')]

STUB = '''#!/bin/sh
dbm= work= jobs=
while [ $# -gt 0 ]; do
    case $1 in
    --dbm) dbm=$2; shift ;;
    --work-dir) work=$2; shift ;;
    --jobs) jobs=$2; shift ;;
    esac
    shift
done
mkdir -p "$work"
echo "$dbm $jobs" >> "$STUB_LOG"
'''


def import_closure(name='pool'):
    """The tools/dev modules NAME imports at module level, transitively, by relative imports.

    An import inside a function (pool.py loads slot.py only when it holds the host heavy
    slot, which the image build never does) is outside the closure."""
    seen, queue = set(), [name]
    while queue:
        module = queue.pop()
        if module in seen:
            continue
        seen.add(module)
        for node in ast.parse((ROOT / 'tools/dev' / f'{module}.py').read_text()).body:
            if isinstance(node, ast.ImportFrom) and node.level == 1:
                queue += [alias.name for alias in node.names] if node.module is None else [node.module]
    return seen


def copied_files():
    """The tools/dev files the build stage copies."""
    return {Path(path).name for path in re.findall(r'tools/dev/\S+\.py', BUILD)}


class BuildStageTests(unittest.TestCase):
    def test_the_stage_copies_exactly_the_modules_the_pool_imports(self):
        self.assertEqual(copied_files(), {f'{name}.py' for name in import_closure()} | {'__init__.py'})

    def test_the_image_tag_hashes_every_copied_file(self):
        tag = (ROOT / 'tools/oracle/image-tag.sh').read_text()
        for name in copied_files():
            self.assertIn(f'"$dev/{name}"', tag, name)

    def test_the_oracle_image_workflow_runs_when_a_copied_file_changes(self):
        workflow = (ROOT / '.github/workflows/oracle-image.yml').read_text()
        for name in copied_files():
            self.assertEqual(workflow.count(f'tools/dev/{name}\n'), 2, name)    # push and pull_request paths
        regex = re.search(r"grep -E '(\^\(.*?\)\$)'", workflow).group(1)
        for name in copied_files():
            self.assertTrue(re.search(regex, f'tools/dev/{name}'), name)

    def test_the_stage_command_runs_the_pool_in_a_tree_of_only_those_files(self):
        block = re.search(r'RUN set -eu; cd /opt/oxpinyin-pool; \\\n(.*?)\n\n', BUILD, re.S).group(1)
        words = shlex.split(block.replace('\\\n', ' '))
        self.assertEqual(words[:3], ['python3', '-m', 'tools.dev.pool'])
        with tempfile.TemporaryDirectory() as directory:
            tree = Path(directory) / 'tree'
            (tree / 'tools/dev').mkdir(parents=True)
            for name in copied_files():
                shutil.copy(ROOT / 'tools/dev' / name, tree / 'tools/dev' / name)
            bin_dir = Path(directory) / 'bin'
            bin_dir.mkdir()
            stub = bin_dir / 'build-oracle.sh'
            stub.write_text(STUB)
            stub.chmod(0o755)
            log = Path(directory) / 'calls'
            build = Path(directory) / 'build'
            command = [sys.executable, *words[1:]]
            command = [word.replace('/build/', f'{build}/') for word in command]
            env = dict(os.environ, PATH=f'{bin_dir}{os.pathsep}{os.environ["PATH"]}', STUB_LOG=str(log))
            result = subprocess.run(command, cwd=tree, env=env, capture_output=True, text=True, timeout=60)
            self.assertEqual(result.returncode, 0, result.stderr)
            calls = sorted(line.split() for line in log.read_text().splitlines())
            self.assertEqual([call[0] for call in calls], ['bdb', 'kc', 'tkrzw'])
            self.assertTrue(all(int(call[1]) >= 1 for call in calls), calls)
            self.assertFalse([path for path in build.iterdir() if path.is_dir()], 'work directories remain')
            self.assertIn('backends: bdb kc tkrzw with', result.stderr)


if __name__ == '__main__':
    unittest.main()
