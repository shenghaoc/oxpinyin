#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Shipping-size recipe key, export check, report and orchestration; no builds."""
import contextlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import cell, ship_sizes

VER = '''PINYIN {
  global:
    pinyin_init;
    pinyin_free;
  local:
    *;
};
'''
NM = '''0000000000001000 T pinyin_init@@LIBPINYIN
0000000000001010 T pinyin_free@@LIBPINYIN
0000000000001020 T _init
'''


class Repo:
    def __init__(self, directory):
        self.root = Path(directory)
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'user.name', 'Shenghao Chen')
        self.git('config', 'user.email', 'shenghaoc@outlook.com')

    def git(self, *args):
        return subprocess.check_output(['git', '-C', str(self.root), *args], text=True).strip()

    def commit(self, files=None):
        for name, text in (files or {}).items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        self.git('add', '-A')
        self.git('commit', '-q', '--allow-empty', '-m', 'change')
        return self.git('rev-parse', 'HEAD')



class KeyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.repo = Repo(self.directory.name)
        self.facts = {'rustc': '1.97.1', 'cc': 'gcc 15'}

    def tearDown(self):
        self.directory.cleanup()

    def key(self, revision='HEAD', facts=None):
        return ship_sizes.recipe_key(revision, facts or self.facts, self.repo.root)

    def test_key_follows_inputs_not_commits(self):
        first = self.repo.commit({'crates/oxpinyin-core/lib': 'a\n'})
        key = self.key()
        self.repo.commit({'docs/note': 'x\n'})
        self.assertEqual(self.key(), key)
        self.assertEqual(self.key(first), key)
        self.assertRegex(key, r'^ship-sizes-v1-[0-9a-f]{32}$')

    def test_input_or_environment_change_moves_the_key(self):
        self.repo.commit({'crates/oxpinyin-core/lib': 'a\n', 'Cargo.lock': 'l\n'})
        key = self.key()
        self.assertNotEqual(self.key(facts={**self.facts, 'rustc': '1.98.0'}), key)
        # Any crate under crates/ moves the key, not only the C API closure.
        for path in ('crates/oxpinyin-core/lib', 'crates/oxpinyin-corpus/lib', 'Cargo.lock', 'tools/dev/packaging.py'):
            with self.subTest(path=path):
                before = self.repo.commit({path: 'new ' + path + '\n'})
                self.assertNotEqual(self.key(), key)
                self.repo.git('reset', '-q', '--hard', 'HEAD~1')
                self.assertEqual(self.key(), key, before)


class ExportTests(unittest.TestCase):
    def test_missing_and_extra_symbols_fail(self):
        for text, label in ((NM.replace('pinyin_free@@LIBPINYIN', 'x'), 'MISSING pinyin_free'),
                            (NM + '1 T pinyin_extra@@LIBPINYIN\n', 'EXTRA   pinyin_extra'),
                            (NM + '1 T oxpinyin_extra\n', 'EXTRA   oxpinyin_extra')):
            with self.subTest(label=label), self.assertRaises(ship_sizes.Failure) as raised:
                ship_sizes.check_exports('libpinyin', text, VER)
            self.assertIn(label, str(raised.exception))


CELL = cell.Settings.read({}, ROOT)


def result(revision, pinyin, zhuyin):
    return {'schema': 1, 'revision': revision, 'backend': CELL.feature,
            'libraries': {'libpinyin': pinyin, 'libzhuyin': zhuyin}}


class ReportTests(unittest.TestCase):
    def test_table_reports_bytes_difference_and_ratio(self):
        text = ship_sizes.table(result('a' * 40, 1000, 2000), result('b' * 40, 1100, 1900), 'cached')
        self.assertIn('| library | base bytes | PR bytes | difference | ratio |', text)
        self.assertIn('| `libpinyin.so.15.0.0` | 1000 | 1100 | +100 | 1.1000 |', text)
        self.assertIn('| `libzhuyin.so.15.0.0` | 2000 | 1900 | -100 | 0.9500 |', text)
        self.assertIn('Increase over base: libpinyin.', text)


class RunTests(unittest.TestCase):
    def invoke(self, argv, builds, **environment):
        stdout = io.StringIO()
        with mock.patch.object(ship_sizes, 'build_side', side_effect=builds) as build, \
                mock.patch.dict(os.environ, environment), \
                contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(io.StringIO()):
            status = ship_sizes.main(argv)
        return status, stdout.getvalue(), build

    def test_cached_base_is_not_rebuilt(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory, 'sizes.json')
            ship_sizes.write_result(cache, result('a' * 40, 1000, 2000))
            status, out, build = self.invoke(['run', '--base', 'B', '--base-cache', str(cache),
                                              '--work', directory], [result('b' * 40, 1500, 2000)])
        self.assertEqual(status, 0)
        self.assertEqual([call.args[0] for call in build.call_args_list], ['HEAD'])
        self.assertIn('cached by recipe key', out)
        self.assertIn('+500', out)

    def test_missing_or_invalid_cache_builds_base_and_writes_it(self):
        for text in (None, 'not json', json.dumps({'schema': 1})):
            with self.subTest(text=text), tempfile.TemporaryDirectory() as directory:
                cache = Path(directory, 'sizes.json')
                if text is not None:
                    cache.write_text(text)
                status, out, build = self.invoke(
                    ['run', '--base', 'B', '--base-cache', str(cache), '--work', directory],
                    [result('a' * 40, 10, 10), result('b' * 40, 10, 10)])
                self.assertEqual(status, 0)
                self.assertEqual([call.args[0] for call in build.call_args_list], ['B', 'HEAD'])
                self.assertEqual(ship_sizes.read_result(cache), result('a' * 40, 10, 10))
                self.assertIn('built in this run', out)

    def test_size_increase_never_fails_the_run(self):
        with tempfile.TemporaryDirectory() as directory:
            status, out, _ = self.invoke(['run', '--base', 'B', '--work', directory],
                                         [result('a', 1000, 1000), result('b', 9000, 9000)])
        self.assertEqual(status, 0)
        self.assertIn('Increase over base: libpinyin, libzhuyin.', out)

    def test_failed_build_fails_the_run_and_reports_no_table(self):
        with tempfile.TemporaryDirectory() as directory:
            for failing in (0, 1):
                builds = [result('a', 1, 1), ship_sizes.Failure('install broke')]
                if failing == 0:
                    builds = [ship_sizes.Failure('install broke')]
                with self.subTest(failing=failing):
                    status, out, _ = self.invoke(['run', '--base', 'B', '--work', directory], builds)
                    self.assertEqual(status, 1)
                    self.assertNotIn('| library |', out)


class BuildSideTests(unittest.TestCase):
    """The install recipe, strip, export check and size read, with a fake process boundary."""

    def run_build(self, nm_text=NM, strip_status=0):
        calls = []
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory, 'work')

            def fake_extract(revision, destination, root=ship_sizes.ROOT):
                for library, (crate, prefix) in ship_sizes.LIBRARIES.items():
                    ver = destination / 'crates' / crate / (library + '.ver')
                    ver.parent.mkdir(parents=True, exist_ok=True)
                    ver.write_text(VER.replace('pinyin', prefix))

            def fake_run(argv, *, cwd=None, env=None, capture=True):
                argv = [str(word) for word in argv]
                calls.append((argv, cwd, env))
                if 'install' in argv:
                    library = argv[argv.index('install') + 1]
                    destdir = next(word for word in argv if word.startswith('--destdir=')).split('=', 1)[1]
                    so = Path(destdir, 'usr/lib', library + '.so.15.0.0')
                    so.parent.mkdir(parents=True, exist_ok=True)
                    so.write_bytes(b'x' * 100)
                elif argv[0] == 'strip':
                    if strip_status:
                        raise ship_sizes.Failure('strip failed')
                    Path(argv[-1]).write_bytes(b'x' * 60)
                elif argv[0] == 'nm':
                    return nm_text.replace('pinyin', 'zhuyin') if 'libzhuyin' in argv[-1] else nm_text
                elif argv[:2] == ['git', 'rev-parse']:
                    return 'c' * 40 + '\n'
                return ''

            with mock.patch.object(ship_sizes, 'extract', fake_extract), \
                    mock.patch.object(ship_sizes, 'run', fake_run), \
                    mock.patch.object(ship_sizes, 'platform_system', return_value='Linux'):
                outcome = ship_sizes.build_side('HEAD', work, Path(directory, 'target'))
        return outcome, calls

    def test_release_recipe_arguments_and_cwd(self):
        outcome, calls = self.run_build()
        installs = [call for call in calls if 'install' in call[0]]
        self.assertEqual(len(installs), 2)
        for argv, cwd, env in installs:
            self.assertEqual(argv[argv.index('-m') + 1:argv.index('-m') + 3], ['tools.dev.packaging', 'install'])
            self.assertEqual(cwd.name, 'src')
            self.assertIn('--release', argv)
            self.assertIn('--locked', argv)
            self.assertIn('--no-default-features', argv)
            self.assertEqual(env['PINYIN_ORACLE_DBM'], CELL.cell)
            self.assertIn(f'--remap-path-prefix={cwd}=/oxpinyin', env['RUSTFLAGS'])
        features = {call[0][call[0].index('install') + 1]: call[0][call[0].index('--features') + 1] for call in installs}
        self.assertEqual(features, {'libpinyin': CELL.feature + ',shipped', 'libzhuyin': CELL.feature})
        strips = [call[0] for call in calls if call[0][0] == 'strip']
        self.assertEqual([call[:2] for call in strips], [['strip', '--strip-unneeded']] * 2)
        self.assertEqual(outcome['libraries'], {'libpinyin': 60, 'libzhuyin': 60})
        self.assertEqual(outcome['backend'], CELL.feature)

    def test_export_mismatch_strip_failure_and_other_systems_fail(self):
        with self.assertRaises(ship_sizes.Failure):
            self.run_build(nm_text=NM + '1 T pinyin_leak@@LIBPINYIN\n')
        with self.assertRaises(ship_sizes.Failure):
            self.run_build(strip_status=1)
        with mock.patch.object(ship_sizes, 'platform_system', return_value='Darwin'), \
                self.assertRaisesRegex(ship_sizes.Failure, 'Linux'):
            ship_sizes.build_side('HEAD', Path('/nonexistent'), Path('/nonexistent'))


class ExtractTests(unittest.TestCase):
    def test_extracted_files_are_newer_than_their_commit(self):
        with tempfile.TemporaryDirectory() as directory:
            Path(directory, 'repo').mkdir()
            repo = Repo(Path(directory, 'repo'))
            repo.commit({'src/lib': 'a\n'})
            old = repo.root / 'src/lib'
            os.utime(old, (1, 1))
            destination = Path(directory, 'out')
            ship_sizes.extract('HEAD', destination, repo.root)
            self.assertTrue((destination / 'src/lib').is_file())
            self.assertGreater((destination / 'src/lib').stat().st_mtime, 1_000_000_000)
            ship_sizes.extract('HEAD', destination, repo.root)

    def test_a_link_keeps_the_mtime_of_its_target(self):
        with tempfile.TemporaryDirectory() as directory:
            outside = Path(directory, 'outside')
            (outside / 'inner').mkdir(parents=True)
            (outside / 'file').write_text('a\n')
            tree = Path(directory, 'tree')
            tree.mkdir()
            (tree / 'plain').write_text('b\n')
            os.symlink(outside, tree / 'directory-link', target_is_directory=True)
            os.symlink(outside / 'file', tree / 'file-link')
            junction = tree / 'junction'
            if sys.platform == 'win32':
                subprocess.run(['cmd', '/c', 'mklink', '/J', str(junction), str(outside)], check=True, stdout=subprocess.DEVNULL)
            for path in (outside, outside / 'inner', outside / 'file', tree / 'plain'):
                os.utime(path, (1, 1))
            ship_sizes.touch_tree(tree)
            for path in (outside, outside / 'inner', outside / 'file'):
                self.assertEqual(path.stat().st_mtime, 1, path)
            self.assertGreater((tree / 'plain').stat().st_mtime, 1_000_000_000)


if __name__ == '__main__':
    unittest.main()
