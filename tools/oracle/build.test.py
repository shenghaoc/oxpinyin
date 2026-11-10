"""The oracle builder's refusals, environment, provenance records and data publication.

git, autoreconf, configure, make and patch are replaced by one fake that installs the files
the real targets install; nothing here compiles or downloads.
"""
import contextlib
import hashlib
import io
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import types
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
from tools.dev import cell  # noqa: E402
from tools.oracle import build, image  # noqa: E402

PIN = build.fields(ROOT / 'tools/oracle/oracle-pin.txt')
EPOCH = '1788427579'


def sha(data):
    return hashlib.sha256(data).hexdigest()


class Upstream:
    """Stands in for every build command, records them, and fails where FAIL says."""

    def __init__(self, fail=None, head=None):
        self.fail, self.head = fail or {}, head
        self.calls, self.environments = [], []

    def __call__(self, command, **options):
        command = [str(part) for part in command]
        self.calls.append(command)
        self.environments.append(options.get('env'))
        for words, status in self.fail.items():
            if all(word in command for word in words):
                raise subprocess.CalledProcessError(status, command)
        stdout = ''
        if command[0] == 'git' and 'rev-parse' in command:
            stdout = (self.head or command[2].rsplit('-', 1)[1]) + '\n'
        if command[0] == 'git' and 'log' in command:
            stdout = EPOCH + '\n'
        if command[0] == './configure':
            self.prefix = Path(next(part for part in command if part.startswith('--prefix=')).split('=', 1)[1])
            self.zhuyin = '--enable-libzhuyin' in command
        if command[0] == 'make' and 'install' in command:
            stage = next((part.split('=', 1)[1] for part in command if part.startswith('DESTDIR=')), None)
            root = Path(stage) / self.prefix.relative_to(self.prefix.anchor) if stage else self.prefix
            where = command[command.index('-C') + 1] if '-C' in command else 'all'
            if where in ('src', 'all'):
                self.write(root / 'lib/libpinyin.so.15.0.0', b'pinyin')
                self.write(root / f'include/libpinyin-{PIN["libpinyin_tag"]}/pinyin.h', b'header')
                if self.zhuyin:
                    self.write(root / 'lib/libzhuyin.so', b'zhuyin')
            if where in ('data', 'all'):
                for name in (*build.DATA_UNSTABLE_FILES, 'table.conf', 'gb_char.bin'):
                    self.write(root / build.DATA_DIRECTORY / name, name.encode())
        if command[0] == 'versions':
            Path(command[-1]).write_text('host versions\n')
        return types.SimpleNamespace(stdout=stdout, returncode=0)

    @staticmethod
    def write(path, data):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)

    def environment(self, program):
        return next(env for call, env in zip(self.calls, self.environments) if call[0] == program)


class BuildTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path(self.enterContext(tempfile.TemporaryDirectory())).resolve()
        self.work, self.prefix = self.directory / 'work', self.directory / 'prefix'
        versions = self.directory / 'image-versions.txt'
        versions.write_bytes(b'image versions\n')
        self.enterContext(mock.patch.object(build, 'IMAGE_VERSIONS', versions))
        self.enterContext(mock.patch.object(build.shutil, 'which', side_effect=lambda name: name))
        for name in ('CFLAGS', 'CXXFLAGS', 'LDFLAGS', 'PKG_CONFIG_PATH'):
            self.enterContext(mock.patch.dict(os.environ)), os.environ.pop(name, None)

    def invoke(self, *args, upstream=None):
        """Run main with ARGS; return (status, stdout, stderr, the fake upstream)."""
        upstream = upstream or Upstream()
        out, err = io.StringIO(), io.StringIO()
        with mock.patch.object(build, 'run', upstream), contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            try:
                status = build.main([str(arg) for arg in args])
            except SystemExit as exit:
                status = exit.code
        return status, out.getvalue(), err.getvalue(), upstream

    def libs(self, *args, **kwargs):
        return self.invoke('--libs-only', '--enable-libzhuyin', '--work-dir', self.work, '--prefix', self.prefix,
                           *args, **kwargs)

    def model(self, marker=PIN['model_archive_sha256'], omit=()):
        extracted = self.directory / 'model/extracted'
        extracted.mkdir(parents=True)
        for name in build.MODEL_FILES:
            if name not in omit:
                (extracted / name).write_text(name)
        (extracted / 'stray.table').write_text('stray')
        if marker:
            (extracted.parent / 'verified').write_text(f'schema=pinyin-model-cache-v1\nsha256={marker}\n')
        return extracted

    def record(self):
        return build.fields(self.prefix / 'oracle-pin.txt')

    def test_usage_refusals_exit_2_before_any_command(self):
        for args in (['--libs-only', '--regen-data'], ['--apply-patches', self.directory], ['--dbm', 'gdbm'],
                     ['--jobs', '0'], ['--jobs', 'four'], ['--jobs', '-1'], ['--unknown'],
                     ['--regen-data', '--apply-patches', self.directory, '--prefix', self.prefix]):
            with self.subTest(args=args):
                status, out, err, upstream = self.invoke(*args)
                self.assertEqual((status, out, upstream.calls), (2, '', []))
                self.assertTrue(err)

    def test_the_pin_record_supplies_the_default_backend_and_every_backend_spelling(self):
        self.assertEqual(build.DBM_NAMES[build.default_dbm(PIN)], PIN['dbm'])
        self.assertEqual(build.DBM_NAMES, {name: cell.BACKEND_FORMATS[backend] for name, backend in cell.CELL_TO_BACKEND.items()})
        for dbm, name in build.DBM_NAMES.items():
            with self.subTest(dbm=dbm):
                prefix = self.directory / dbm
                status, _, _, upstream = self.invoke('--libs-only', '--dbm', dbm, '--work-dir', self.work, '--prefix', prefix)
                self.assertEqual(status, 0)
                self.assertIn(f'--with-dbm={name}', next(call for call in upstream.calls if call[0] == './configure'))
                record = build.fields(prefix / 'oracle-pin.txt')
                self.assertEqual(record['dbm'], name)
                self.assertTrue(record['pin_ref'].endswith('+dbm-' + dbm))

    def test_libraries_build_records_the_installed_bytes_and_prints_the_library_last(self):
        status, out, _, upstream = self.libs('--jobs', '3')
        self.assertEqual(status, 0)
        library = self.prefix / 'lib/libpinyin.so.15.0.0'
        self.assertEqual(out.splitlines()[-1], str(library))
        commit, source = PIN['libpinyin_commit'], self.work / 'src' / f'libpinyin-{PIN["libpinyin_commit"]}'
        self.assertIn(['git', '-C', str(source), 'fetch', '--quiet', '--depth=1', build.LIBPINYIN_GIT_URL, commit], upstream.calls)
        self.assertFalse(any('ibus' in part for call in upstream.calls for part in call))
        self.assertEqual([call for call in upstream.calls if call[0] == 'make'],
                         [['make', '-C', 'src', '-j3'], ['make', '-C', 'src', 'install'], ['make', 'install-pkgconfigDATA']])
        self.assertEqual(self.record(), {
            'schema': 'pinyin-oracle-v1',
            'pin_ref': f'libpinyin-{PIN["libpinyin_tag"]}-{commit}+model20-{PIN["model_archive_sha256"]}+dbm-tkrzw',
            'recipe_sha256': image.recipe_hash(), 'architecture': build.platform.machine(),
            'versions_sha256': sha(b'image versions\n'), 'libpinyin_tag': PIN['libpinyin_tag'], 'libpinyin_commit': commit,
            'ibus_libpinyin_tag': PIN['ibus_libpinyin_tag'], 'ibus_libpinyin_commit': PIN['ibus_libpinyin_commit'],
            'model_sha256': PIN['model_archive_sha256'], 'dbm': 'Tkrzw', 'header_sha256': sha(b'header'),
            'shared_object_sha256': sha(b'pinyin'), 'zhuyin_shared_object_sha256': sha(b'zhuyin')})
        self.assertEqual((self.prefix / 'versions.txt').read_bytes(), b'image versions\n')
        self.assertFalse((self.prefix / 'oracle-data.sha256').exists())

    def test_reference_environment_is_derived_from_the_checkout_and_the_prefix(self):
        with mock.patch.dict(os.environ, {'LD_LIBRARY_PATH': '/caller/lib', 'SOURCE_DATE_EPOCH': '1', 'TZ': 'Asia/Tokyo'}):
            env = self.libs()[3].environment('./configure')
        source = self.work / 'src' / f'libpinyin-{PIN["libpinyin_commit"]}'
        remap = f'-ffile-prefix-map={source}=/usr/src/libpinyin -fdebug-prefix-map={source}=/usr/src/libpinyin'
        self.assertEqual(env['SOURCE_DATE_EPOCH'], EPOCH)  # git log of the checkout, never the caller's value
        self.assertEqual((env['TZ'], env['LC_ALL']), ('UTC', 'C'))
        self.assertEqual((env['CFLAGS'], env['CXXFLAGS']), ('-g -O2 ' + remap, '-g -O2 ' + remap))
        self.assertEqual(env['LDFLAGS'], '-Wl,--build-id=none')
        self.assertEqual(env['PKG_CONFIG_PATH'], f'{self.prefix}/lib/pkgconfig:{self.prefix}/lib64/pkgconfig')
        self.assertEqual(env['LD_LIBRARY_PATH'], f'{self.prefix}/lib:{self.prefix}/lib64')

    def test_caller_flags_pass_through_and_an_empty_value_is_not_the_default(self):
        caller = {'CFLAGS': '', 'CXXFLAGS': '-O0', 'LDFLAGS': '-L/x', 'PKG_CONFIG_PATH': '/usr/local/lib/pkgconfig', 'CC': 'clang'}
        with mock.patch.dict(os.environ, caller):
            env = self.libs()[3].environment('make')
        self.assertTrue(env['CFLAGS'].startswith(' -ffile-prefix-map='))
        self.assertTrue(env['CXXFLAGS'].startswith('-O0 -ffile-prefix-map='))
        self.assertEqual(env['LDFLAGS'], '-L/x -Wl,--build-id=none')
        self.assertTrue(env['PKG_CONFIG_PATH'].endswith('/lib64/pkgconfig:/usr/local/lib/pkgconfig'))
        self.assertEqual(env['CC'], 'clang')

    def test_occupied_or_non_directory_prefix_is_refused(self):
        (self.prefix / 'lib').mkdir(parents=True)
        status, _, err, upstream = self.libs()
        self.assertEqual((status, upstream.calls), (1, []))
        self.assertIn('installation prefix must be empty', err)
        blocker = self.directory / 'file'
        blocker.write_text('')
        status, _, err, _ = self.invoke('--libs-only', '--prefix', blocker)
        self.assertEqual(status, 1)
        self.assertIn('not a directory', err)

    def test_source_identity_failures(self):
        status, _, err, _ = self.libs(upstream=Upstream(fail={('git', 'fetch'): 128}))
        self.assertEqual(status, 1)
        self.assertIn(f'git fetch of libpinyin {PIN["libpinyin_commit"]} failed', err)
        status, _, err, upstream = self.libs(upstream=Upstream(head='0' * 40))
        self.assertEqual(status, 1)
        self.assertIn('libpinyin commit mismatch: fetched ' + '0' * 40, err)
        self.assertFalse(any(call[0] == 'autoreconf' for call in upstream.calls))
        # Any other git failure keeps its own status.
        self.assertEqual(self.libs(upstream=Upstream(fail={('git', 'checkout'): 129}))[0], 129)
        self.assertFalse((self.prefix / 'oracle-pin.txt').exists())

    def test_failed_library_build_leaves_no_provenance_record(self):
        status, out, _, _ = self.libs(upstream=Upstream(fail={('make', 'src', 'install'): 2}))
        self.assertEqual((status, out), (2, ''))
        self.assertFalse((self.prefix / 'oracle-pin.txt').exists())

    def test_model_directory_needs_the_inventory_and_the_pinned_marker(self):
        cases = (({'omit': ('punct.table', 'art.table')}, '--model-dir is missing: art.table punct.table'),
                 ({'marker': ''}, 'lacks a matching provenance marker'),
                 ({'marker': '0' * 64}, 'lacks a matching provenance marker'))
        for number, (options, message) in enumerate(cases):
            with self.subTest(options=options):
                self.directory = self.directory / str(number)
                status, _, err, upstream = self.invoke('--work-dir', self.work, '--prefix', self.prefix,
                                                       '--model-dir', self.model(**options))
                self.assertEqual((status, upstream.calls), (1, []))
                self.assertIn(message, err)
                self.directory = self.directory.parent
        status, _, err, _ = self.invoke('--prefix', self.prefix, '--model-dir', self.directory / 'absent')
        self.assertEqual(status, 1)
        self.assertIn('--model-dir is not a directory', err)

    def archive(self, members):
        path = self.work / 'downloads' / build.MODEL_ARCHIVE
        path.parent.mkdir(parents=True, exist_ok=True)
        with tarfile.open(path, 'w:gz') as tar:
            for info, data in members:
                tar.addfile(info, io.BytesIO(data) if data is not None else None)
        return sha(path.read_bytes())

    def test_downloaded_archive_is_checked_and_only_its_regular_inventory_is_extracted(self):
        def regular(name, data=b'model'):
            info = tarfile.TarInfo(name)
            info.size = len(data)
            return info, data
        link = tarfile.TarInfo('link.table')
        link.type, link.linkname = tarfile.SYMTYPE, '/etc/passwd'
        members = [regular('./' + name) for name in build.MODEL_FILES]
        digest = self.archive([*members, regular('../escape.table'), regular('extra.table'), (link, None)])
        destination = self.directory / 'data'
        with mock.patch.object(build, 'run', Upstream()) as upstream:
            build.download_model(self.work, digest, destination)
            self.assertEqual(upstream.calls, [])  # a cached archive is not downloaded again
            self.assertEqual(sorted(path.name for path in destination.iterdir()), sorted(build.MODEL_FILES))
            self.assertFalse((self.directory / 'escape.table').exists())
            with self.assertRaisesRegex(build.BuildError, 'checksum mismatch: model20.text.tar.gz'):
                build.download_model(self.work, '0' * 64, destination)
            art = tarfile.TarInfo('art.table')
            art.type, art.linkname = tarfile.SYMTYPE, 'culture.table'
            digest = self.archive([*members[1:], (art, None)])
            with self.assertRaisesRegex(build.BuildError, 'the pinned archive is missing: art.table'):
                build.download_model(self.work, digest, self.directory / 'other')
        self.assertEqual(list(self.work.glob('tmp*')), [])

    def test_missing_archive_is_downloaded_with_curl_and_a_failed_download_propagates(self):
        with mock.patch.object(build, 'run', Upstream(fail={('curl',): 22})) as upstream:
            (self.work / 'downloads').mkdir(parents=True)
            with self.assertRaises(subprocess.CalledProcessError):
                build.download_model(self.work, '0' * 64, self.directory / 'data')
        self.assertEqual(upstream.calls, [['curl', '--fail', '--location', '--retry', '3', '--show-error', build.MODEL_URL,
                                           '--output', str(self.work / 'downloads' / build.MODEL_ARCHIVE)]])

    def patches(self):
        directory = self.directory / 'patches'
        directory.mkdir()
        for name in ('b.patch', 'a.patch', 'notes.txt'):
            (directory / name).write_text(name)
        (directory / 'c.patch').mkdir()
        return directory

    def test_patches_apply_in_name_order_and_mark_the_build(self):
        directory = self.patches()
        status, _, err, upstream = self.libs('--apply-patches', directory)
        self.assertEqual(status, 0)
        self.assertEqual([call for call in upstream.calls if call[0] == 'patch'],
                         [['patch', '-p1', '--forward', '--no-backup-if-mismatch']] * 2)
        self.assertLess(err.index('a.patch'), err.index('b.patch'))
        manifest = f'{sha(b"a.patch")}  ./a.patch\n{sha(b"b.patch")}  ./b.patch\n'.encode()
        self.assertEqual((self.prefix / 'oracle-patches.sha256').read_bytes(), manifest)
        record = self.record()
        self.assertEqual(record['patches_manifest_sha256'], sha(manifest))
        self.assertTrue(record['pin_ref'].endswith('+dbm-tkrzw+patches-' + sha(manifest)))

    def test_patch_failures_stop_the_build(self):
        status, _, _, upstream = self.libs('--apply-patches', self.patches(), upstream=Upstream(fail={('patch',): 1}))
        self.assertEqual(status, 1)
        self.assertFalse(any(call[0] == 'autoreconf' for call in upstream.calls))
        self.assertFalse((self.prefix / 'oracle-pin.txt').exists())
        empty = self.directory / 'empty'
        empty.mkdir()
        status, _, err, _ = self.invoke('--libs-only', '--work-dir', self.work, '--prefix', self.directory / 'p2',
                                        '--apply-patches', empty)
        self.assertEqual(status, 1)
        self.assertIn('no *.patch files', err)

    def test_full_build_installs_data_builds_ibus_and_records_both_manifests(self):
        status, _, err, upstream = self.invoke('--work-dir', self.work, '--prefix', self.prefix, '--model-dir', self.model())
        self.assertEqual(status, 0)
        self.assertIn('reusing pre-extracted model export', err)
        source = self.work / 'src' / f'libpinyin-{PIN["libpinyin_commit"]}'
        self.assertEqual(sorted(path.name for path in (source / 'data').iterdir()), sorted(build.MODEL_FILES))
        ibus = [call for call in upstream.calls if PIN['ibus_libpinyin_commit'] in ' '.join(call)]
        self.assertIn(build.IBUS_LIBPINYIN_GIT_URL, ibus[1])
        data = build.DATA_DIRECTORY
        self.assertEqual((self.prefix / 'oracle-data.sha256').read_bytes().decode(),
                         f'{sha(b"gb_char.bin")}  {data}/gb_char.bin\n{sha(b"table.conf")}  {data}/table.conf\n')
        self.assertEqual((self.prefix / 'oracle-data-unstable.sha256').read_bytes().decode(),
                         ''.join(f'{sha(name.encode())}  {data}/{name}\n' for name in build.DATA_UNSTABLE_FILES))
        record = self.record()
        self.assertEqual(record['data_manifest_sha256'], sha((self.prefix / 'oracle-data.sha256').read_bytes()))
        self.assertEqual(record['data_unstable_manifest_sha256'], sha((self.prefix / 'oracle-data-unstable.sha256').read_bytes()))

    def regenerate(self, **kwargs):
        return self.invoke('--regen-data', '--work-dir', self.work, '--prefix', self.prefix, '--model-dir',
                           self.directory / 'model/extracted', **kwargs)

    def test_data_regeneration_publishes_into_a_matching_libraries_prefix(self):
        self.assertEqual(self.libs()[0], 0)
        (self.prefix / 'versions.txt').write_bytes(b'the libraries versions\n')
        libraries = self.record()
        self.model()
        status, out, _, upstream = self.regenerate()
        self.assertEqual(status, 0)
        self.assertEqual(out.splitlines()[-1], str(self.prefix / 'lib/libpinyin.so.15.0.0'))
        self.assertIn(['make', '-C', 'data', 'install', mock.ANY], upstream.calls)
        self.assertFalse(any(call[:2] == ['make', 'install'] for call in upstream.calls))
        self.assertTrue((self.prefix / build.DATA_DIRECTORY / 'bigram.db').is_file())
        record = self.record()
        libraries['versions_sha256'] = sha(b'the libraries versions\n')
        self.assertEqual({key: record[key] for key in libraries}, libraries)
        self.assertEqual(record['data_manifest_sha256'], sha((self.prefix / 'oracle-data.sha256').read_bytes()))
        self.assertEqual((self.prefix / 'versions.txt').read_bytes(), b'the libraries versions\n')
        self.assertEqual(sorted(path.name for path in self.directory.iterdir() if '.stage.' in path.name), [])

    def test_failed_data_regeneration_leaves_the_prefix_as_it_was(self):
        self.assertEqual(self.libs()[0], 0)
        self.model()
        old = self.prefix / build.DATA_DIRECTORY / 'kept.bin'
        Upstream.write(old, b'old')
        before = sorted((str(path), path.read_bytes() if path.is_file() else None) for path in self.prefix.rglob('*'))
        for fail in ({('make', 'data', 'install'): 2}, {('make', '-j1'): 130}):
            with self.subTest(fail=fail):
                status = self.regenerate(upstream=Upstream(fail=fail))[0]
                self.assertEqual(status, next(iter(fail.values())))
                after = sorted((str(path), path.read_bytes() if path.is_file() else None) for path in self.prefix.rglob('*'))
                self.assertEqual(after, before)
                self.assertEqual([path.name for path in self.directory.iterdir() if '.stage.' in path.name], [])

    def test_interrupted_data_regeneration_removes_its_stage_and_propagates(self):
        self.assertEqual(self.libs()[0], 0)
        self.model()

        def interrupt(command, **options):
            if str(command[0]) == 'autoreconf':
                raise KeyboardInterrupt
            return upstream(command, **options)
        upstream = Upstream()
        with self.assertRaises(KeyboardInterrupt):
            self.regenerate(upstream=interrupt)
        self.assertEqual([path.name for path in self.directory.iterdir() if '.stage.' in path.name], [])
        self.assertNotIn('data_manifest_sha256', self.record())

    def test_data_regeneration_refuses_a_prefix_of_another_recipe_backend_or_none(self):
        self.model()
        status, _, err, upstream = self.regenerate()
        self.assertEqual((status, upstream.calls), (1, []))
        self.assertIn('is not a libs-only prefix of this pin, recipe and dbm', err)
        self.assertFalse(self.prefix.exists())
        self.assertEqual(self.libs()[0], 0)
        status, _, _, upstream = self.invoke('--regen-data', '--dbm', 'kc', '--work-dir', self.work, '--prefix', self.prefix,
                                             '--model-dir', self.directory / 'model/extracted')
        self.assertEqual((status, upstream.calls), (1, []))
        pin = self.prefix / 'oracle-pin.txt'
        pin.write_bytes(pin.read_bytes().replace(b'recipe_sha256=', b'recipe_sha256=0'))
        status, _, _, upstream = self.regenerate()
        self.assertEqual((status, upstream.calls), (1, []))


class RecipeTests(unittest.TestCase):
    def test_every_recipe_input_changes_the_hash_and_the_image_reuses_it(self):
        self.assertIs(image.recipe_hash, build.recipe_hash)
        self.assertEqual(build.recipe_hash(), image.recipe_hash(ROOT))
        with tempfile.TemporaryDirectory() as directory:
            copied = Path(directory) / 'tools/oracle'
            copied.mkdir(parents=True)
            for name in build.RECIPE_INPUTS:
                (copied / name).write_bytes((ROOT / 'tools/oracle' / name).read_bytes())
            self.assertEqual(build.recipe_hash(directory), build.recipe_hash())
            for name in build.RECIPE_INPUTS:
                with self.subTest(name=name):
                    original = (copied / name).read_bytes()
                    (copied / name).write_bytes(original + b'\n')
                    self.assertNotEqual(build.recipe_hash(directory), build.recipe_hash())
                    (copied / name).write_bytes(original)

    def test_the_image_carries_every_recipe_input_beside_the_builder(self):
        docker = (ROOT / 'tools/oracle/Dockerfile.oracle').read_text()
        copy = next(line for line in docker.splitlines() if line.startswith('COPY ') and line.endswith(' /opt/oracle-recipe/tools/oracle/'))
        self.assertEqual(copy.split()[1:-1], ['tools/oracle/' + name for name in build.RECIPE_INPUTS])
        self.assertIn('python3 /opt/oracle-recipe/tools/oracle/build.py --libs-only', docker)

    def test_builder_imports_only_the_standard_library(self):
        # Debian's python3 runs it in the image before any managed interpreter or tools.dev exists.
        source = (ROOT / 'tools/oracle/build.py').read_text()
        self.assertNotRegex(source, r'(?m)^\s*(from|import)\s+(tools|\.)')
        result = subprocess.run([sys.executable, '-I', str(ROOT / 'tools/oracle/build.py'), '--help'],
                                capture_output=True, text=True, cwd=tempfile.gettempdir(), check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        for option in ('--work-dir', '--prefix', '--jobs', '--apply-patches', '--model-dir', '--enable-libzhuyin',
                       '--dbm', '--libs-only', '--regen-data'):
            self.assertIn(option, result.stdout)


if __name__ == '__main__':
    unittest.main()
