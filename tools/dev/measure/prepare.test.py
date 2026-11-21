"""Tests for the Valgrind recipes' input preparation: prepared pin prefixes are reused only when
every recorded input still matches, and are rebuilt otherwise."""
import contextlib
import io
import shutil
from pathlib import Path
import platform
import re
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import runners
from tools.dev.measure import bench, identity, prepare
from tools.dev.test_support import capi_metadata
from tools.oracle import image, versions


class PreparationTests(unittest.TestCase):
    def prepared_prefix(self, prefix):
        """A prepared PREFIX that passes every reuse check; answer its record's text."""
        import hashlib
        data = prefix / 'lib/libpinyin/data'
        data.mkdir(parents=True)
        pins = image.fields(ROOT / 'tools/oracle/oracle-pin.txt')
        installed = mock.patch.object(versions, 'package_version', return_value='1.0')
        installed.start()
        self.addCleanup(installed.stop)
        (prefix / 'versions.txt').write_text(prepare.running_inventory())
        fields = dict(libpinyin_commit=pins['libpinyin_commit'], ibus_libpinyin_commit=pins['ibus_libpinyin_commit'],
                      model_sha256=pins['model_archive_sha256'],
                      recipe_sha256=image.recipe_hash(), architecture=platform.machine(),
                      dbm=image.cell.BACKEND_FORMATS[image.cell.CELL_TO_BACKEND['kc']],
                      versions_sha256=image.digest_file(prefix / 'versions.txt'))
        for library, key, content in (('libpinyin.so', 'shared_object_sha256', b'pinyin'),
                                      ('libzhuyin.so', 'zhuyin_shared_object_sha256', b'zhuyin')):
            (prefix / 'lib' / library).write_bytes(content)
            fields[key] = hashlib.sha256(content).hexdigest()
        text = ''.join(f'{key}={value}\n' for key, value in fields.items())
        for manifest, key, file in (('oracle-data.sha256', 'data_manifest_sha256', 'table.conf'),
                                    ('oracle-data-unstable.sha256', 'data_unstable_manifest_sha256', 'bigram.db')):
            (data / file).write_bytes(b'data')
            content = f'{hashlib.sha256(b"data").hexdigest()}  lib/libpinyin/data/{file}\n'.encode()
            (prefix / manifest).write_bytes(content)
            text += f'{key}={hashlib.sha256(content).hexdigest()}\n'
        (prefix / 'oracle-pin.txt').write_text(text)
        return text

    def test_prepared_data_reuse_checks_prefix_ok_and_file_hashes(self):
        with tempfile.TemporaryDirectory() as directory:
            prefix = Path(directory) / 'prepared'
            text = self.prepared_prefix(prefix)
            data = prefix / 'lib/libpinyin/data'
            self.assertTrue(prepare.prepared_pin_matches(prefix, 'kc'))
            (data / 'bigram.db').write_bytes(b'damaged')
            self.assertFalse(prepare.prepared_pin_matches(prefix, 'kc'))
            (data / 'bigram.db').write_bytes(b'data')
            (prefix / 'oracle-pin.txt').write_text(re.sub(r'recipe_sha256=\w+', 'recipe_sha256=other', text))
            self.assertFalse(prepare.prepared_pin_matches(prefix, 'kc'))

    @staticmethod
    def environment(root):
        return {'OXPINYIN_CACHE': str(root / 'cache')}

    def prepare_oracle(self, root):
        """Run `prepare oracle --cells kc` with the cache root under ROOT; answer the build mock.

        The libraries build publishes ROOT/seed/kc as the prefix, as build.py --libs-only would;
        the data build changes nothing."""
        from tools.dev import model

        def build(command, env=None):
            command = [str(part) for part in command]
            # The distribution's Kyoto Cabinet is on the default paths: no overlay, no run path.
            self.assertNotIn('OXPINYIN_TKRZW_PREFIX', env)
            self.assertNotIn('LDFLAGS', env, 'the pin is linked as its recipe links it, with no run path')
            prefix = Path(command[command.index('--prefix') + 1])
            if '--libs-only' in command:
                self.assertFalse(prefix.exists(), 'build.py needs an empty prefix')
                shutil.copytree(root / 'seed/kc', prefix)
            else:
                # build.py's LD_LIBRARY_PATH is the prefix's lib directory; the prefix holds no library link.
                self.assertFalse((prefix / 'lib/libtkrzw.so.1').exists())
            return 0
        with mock.patch.object(model, 'fetch', return_value=root / 'model'), \
             mock.patch.object(bench, 'heavy', side_effect=build) as builds, \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(prepare.prepare(['oracle', '--cells', 'kc'], self.environment(root)), 0)
        return builds

    def test_a_matching_prefix_is_reused_without_a_build(self):
        with tempfile.TemporaryDirectory() as directory:
            self.prepared_and_seed(directory)
            self.prepare_oracle(Path(directory)).assert_not_called()

    def test_a_rebuild_runs_build_py_for_the_libraries_then_the_data_under_the_cache_root(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prefix, _, _ = self.prepared_and_seed(directory)
            shutil.rmtree(prefix)
            builds = [[str(part) for part in call.args[0]] for call in self.prepare_oracle(root).call_args_list]
            self.assertEqual([command[1] for command in builds], [str(bench.ROOT / 'tools/oracle/build.py')] * 2)
            self.assertIn('--libs-only', builds[0])
            self.assertEqual(builds[1][builds[1].index('--model-dir') + 1], str(root / 'model'))
            work = root / 'cache/measure/oracle-kc'
            self.assertEqual([command[command.index('--work-dir') + 1] for command in builds],
                             [str(work / 'libs'), str(work / 'data')])
            self.assertEqual({command[command.index('--prefix') + 1] for command in builds}, {str(prefix)})
            self.assertEqual([command[command.index('--dbm') + 1] for command in builds], ['kc', 'kc'])
            self.assertTrue(prepare.prepared_pin_matches(prefix, 'kc'))

    def test_build_pin_runs_the_libraries_then_the_data_build_in_the_callers_environment(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            prefix = root / 'kc'
            calls = []

            def build(command, env=None):
                command = [str(part) for part in command]
                calls.append((command, env))
                if '--libs-only' in command:
                    (prefix / 'lib').mkdir(parents=True)
                return 0
            environment = {'OXPINYIN_CACHE': str(root / 'cache'), 'PATH': '/bin'}
            with mock.patch.object(Path, 'symlink_to') as link, mock.patch.object(bench, 'heavy', side_effect=build):
                prepare.build_pin(prefix, 'kc', root / 'model', root / 'work', environment)
            link.assert_not_called()
            self.assertEqual([env for _, env in calls], [environment, environment])
            self.assertEqual([command[command.index('--dbm') + 1] for command, _ in calls], ['kc', 'kc'])
            self.assertIn('--libs-only', calls[0][0])
            self.assertIn('--regen-data', calls[1][0])

    def test_tkrzw_is_no_linux_measurement_cell(self):
        from tools.dev import model
        from tools.dev.measure import recipes
        with mock.patch.object(recipes, 'require_local_measurement'), \
             mock.patch.object(model, 'fetch') as fetch, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                prepare.prepare(['oracle', '--cells', 'tkrzw'], {})
            self.assertEqual(error.exception.code, 2)
            fetch.assert_not_called()

    def prepared_and_seed(self, directory):
        """A valid prepared prefix and an identical seed under DIRECTORY; answer both and the record's text."""
        prefix = prepare.pin_prefix(self.environment(Path(directory)), 'kc')
        seed = Path(directory) / 'seed/kc'
        text = self.prepared_prefix(prefix)
        shutil.copytree(prefix, seed)
        return prefix, seed, text

    def test_a_cached_library_with_changed_bytes_is_refused_and_rebuilt(self):
        for library, content in (('libpinyin.so', b'pinyin'), ('libzhuyin.so', b'zhuyin')):
            with self.subTest(library=library), tempfile.TemporaryDirectory() as directory:
                prefix, _, _ = self.prepared_and_seed(directory)
                record = (prefix / 'oracle-pin.txt').read_bytes()
                (prefix / 'lib' / library).write_bytes(b'changed')
                self.assertEqual((prefix / 'oracle-pin.txt').read_bytes(), record)
                self.assertFalse(prepare.prepared_pin_matches(prefix, 'kc'))
                self.assertIn('--regen-data', self.prepare_oracle(Path(directory)).call_args.args[0])
                self.assertEqual((prefix / 'lib' / library).read_bytes(), content)

    def test_a_prefix_with_another_hosts_versions_is_refused_and_rebuilt(self):
        with tempfile.TemporaryDirectory() as directory:
            prefix, seed, text = self.prepared_and_seed(directory)
            self.assertTrue(prepare.prepared_pin_matches(prefix, 'kc'))
            with mock.patch.object(versions, 'package_version', return_value='2.0'):
                current = prepare.running_inventory()
                (seed / 'versions.txt').write_text(current)
                (seed / 'oracle-pin.txt').write_text(re.sub(
                    r'versions_sha256=\w+', 'versions_sha256=' + image.digest_file(seed / 'versions.txt'), text))
                self.assertTrue(image.prefix_ok(prefix, 'kc'), 'the recorded inventory is intact')
                self.assertFalse(prepare.prepared_pin_matches(prefix, 'kc'))
                self.assertIn('--regen-data', self.prepare_oracle(Path(directory)).call_args.args[0])
            self.assertEqual((prefix / 'versions.txt').read_text(), current)

    def test_a_prefix_without_the_pinned_model_is_refused_and_rebuilt(self):
        for label, change in (('missing', lambda text: re.sub(r'model_sha256=\w+\n', '', text)),
                              ('different', lambda text: re.sub(r'model_sha256=\w+', 'model_sha256=' + '0' * 64, text))):
            with self.subTest(label), tempfile.TemporaryDirectory() as directory:
                prefix, _, text = self.prepared_and_seed(directory)
                (prefix / 'oracle-pin.txt').write_text(change(text))
                self.assertTrue(image.prefix_ok(prefix, 'kc'), 'prefix_ok does not check the model')
                self.assertFalse(prepare.prepared_pin_matches(prefix, 'kc'))
                self.assertIn('--regen-data', self.prepare_oracle(Path(directory)).call_args.args[0])
                self.assertEqual((prefix / 'oracle-pin.txt').read_text(), text)

    def test_a_rebuild_that_still_does_not_match_fails_preparation(self):
        def another_model(seed, text):
            (seed / 'oracle-pin.txt').write_text(re.sub(r'model_sha256=\w+', 'model_sha256=' + '0' * 64, text))

        for label, damage, installed, named in (
            ('model', another_model, '1.0', 'model_sha256'),
            ('library', lambda seed, text: (seed / 'lib/libpinyin.so').write_bytes(b'changed'), '1.0', 'prefix_ok'),
            ('data', lambda seed, text: (seed / 'lib/libpinyin/data/bigram.db').write_bytes(b'changed'), '1.0', 'bigram.db'),
            ('inventory', lambda seed, text: None, '2.0', "this host's inventory"),
        ):
            with self.subTest(label), tempfile.TemporaryDirectory() as directory:
                prefix, seed, text = self.prepared_and_seed(directory)
                shutil.rmtree(prefix)
                damage(seed, text)
                with mock.patch.object(versions, 'package_version', return_value=installed), \
                     contextlib.redirect_stderr(io.StringIO()) as stderr, self.assertRaises(runners.Finished):
                    self.prepare_oracle(Path(directory))
                self.assertIn('preparation: the rebuilt kc prefix', stderr.getvalue())
                self.assertIn(named, stderr.getvalue())

    def test_cells_is_only_accepted_for_oracle_preparation(self):
        from tools.dev import model
        from tools.dev.measure import recipes
        for mode in ('callgrind',):
            with self.subTest(mode=mode), mock.patch.object(recipes, 'require_local_measurement'), \
                 mock.patch.object(model, 'fetch') as fetch, contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as error:
                    prepare.prepare([mode, '--cells', 'kc'], {})
                self.assertEqual(error.exception.code, 2)
                fetch.assert_not_called()

    def test_preparation_refuses_on_github_actions_before_fetching_model_data(self):
        from tools.dev import model
        with mock.patch.object(model, 'fetch') as fetch, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(runners.Finished):
                prepare.prepare(['oracle'], {'GITHUB_ACTIONS': 'true'})
            fetch.assert_not_called()


if sys.platform == 'win32':
    class WindowsBuildTests(unittest.TestCase):
        def build(self,directory,target):
            """native_build of a revision whose C crate names its library target TARGET."""
            def metadata(root,env=None):
                return capi_metadata(root,features={'default':['tkrzw']},targets={'pinyin':target})
            root=Path(directory);pc=root/'installed/x64-windows/lib/pkgconfig'
            pc.mkdir(parents=True);(pc/'tkrzw.pc').touch()
            with mock.patch('tools.dev.check.msvc_environment',side_effect=lambda env:env), \
                 mock.patch.object(bench,'run') as run, \
                 mock.patch('tools.dev.packaging.cargo_metadata',side_effect=metadata) as read, \
                 mock.patch.object(identity,'sha256',return_value='digest'):
                arm=prepare.native_build('a'*40,root/'build',dict(VCPKG_ROOT=directory,
                    RUSTFLAGS='bad',CARGO_ENCODED_RUSTFLAGS='bad',CARGO_PROFILE_RELEASE_DEBUG='2'),windows=True)
            return arm,run,read,pc

        def test_release_build_has_no_overrides(self):
            with tempfile.TemporaryDirectory() as directory:
                arm,run,read,pc=self.build(directory,'oxpinyin_capi')
                command=next(call for call in run.call_args_list if call.args[0][0]=='cargo')
                self.assertEqual(command.args[0],['cargo','build','--locked','--release','-p','oxpinyin-capi'])
                self.assertFalse(any(key in command.kwargs['env'] for key in
                                    ('RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','CARGO_PROFILE_RELEASE_DEBUG')))
                self.assertEqual(arm['dylib'].name,'oxpinyin_capi.dll')
                self.assertEqual(arm['default_features'],['tkrzw'])
                self.assertEqual(command.kwargs['env']['PKG_CONFIG_PATH'],str(pc.resolve()))

        def test_the_library_files_take_the_name_of_the_revisions_own_target(self):
            # A revision names its library target itself: pinyin_capi before the default name, oxpinyin_capi after.
            for target in ('pinyin_capi','oxpinyin_capi'):
                with self.subTest(target=target),tempfile.TemporaryDirectory() as directory:
                    arm,run,read,_=self.build(directory,target)
                    release=Path(directory)/'build/target/release'
                    self.assertEqual(arm['dylib'],release/(target+'.dll'))
                    self.assertEqual(arm['import_library'],release/(target+'.dll.lib'))
                    # The metadata is read once, in the product source, before the build.
                    self.assertEqual([call.args[0] for call in read.call_args_list],[Path(directory)/'build/source'])


if __name__ == '__main__':
    unittest.main()
