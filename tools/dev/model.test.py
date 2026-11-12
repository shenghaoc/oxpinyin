#!/usr/bin/env -S uv run --no-project --managed-python python -X utf8
"""Contract of the model20 fetch: options, refusals, verification, extraction and the cache marker."""
import contextlib
import hashlib
import io
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools import cachedir
from tools.dev import model, test_support


def write_archive(path, names=model.EXPECTED_MODEL_FILES):
    """Write a gzip tarball holding NAMES at its top level; return its SHA-256."""
    with tarfile.open(path, 'w:gz') as archive:
        for name in names:
            data = f'{name}\n'.encode()
            info = tarfile.TarInfo(name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


class FetchTests(unittest.TestCase):
    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='oxpinyin-model-')
        self.addCleanup(scratch.cleanup)
        self.work = Path(scratch.name)
        self.pin = self.work / 'oracle-pin.txt'
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        self.env = dict(os.environ, PATH=f'{self.bin}{os.pathsep}{os.environ.get("PATH", os.defpath)}')
        self.env['OXPINYIN_CACHE'] = str(self.work / 'root')
        self.env.pop('PINYIN_MODEL_URL', None)

    @property
    def cache(self):
        """The model20 entry under this test's cache root, for the pin in force."""
        return model.default_cache(self.env)

    def fake(self, name, source):
        """Put a Python program NAME first on PATH, so a test never starts the real curl or git."""
        return test_support.python_tool(self.bin, name, source)

    def main(self, *argv, env=None):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            status = model.main(list(argv), env=self.env if env is None else env)
        return status, out.getvalue(), err.getvalue()

    def pinned(self, archive, names=model.EXPECTED_MODEL_FILES):
        digest = write_archive(archive, names)
        self.pin.write_text(f'# fixture pin\nmodel_archive_sha256={digest}\n')
        return mock.patch.object(model, 'PIN_FILE', self.pin)

    def test_help_and_option_errors(self):
        status, out, err = self.main('--help')
        self.assertEqual((status, err), (0, ''))
        self.assertIn('--archive PATH', out)
        self.assertNotIn('--cache-dir', out)
        status, out, err = self.main('--bogus')
        self.assertEqual((status, out), (2, ''))
        self.assertTrue(err.startswith('unknown option: --bogus\nUsage:'), err)
        status, out, err = self.main('--url')
        self.assertEqual(status, 2)
        self.assertTrue(err.startswith('option needs a value: --url\n'), err)
        self.assertEqual(self.main('--cache-dir', str(self.work))[0], 2)

    def test_the_cache_is_the_digest_keyed_entry_of_an_absolute_root(self):
        self.assertEqual(self.cache, self.work / 'root' / f'model20-{model.archive_sha256()[:12]}')
        for value in (None, '', 'relative/root'):
            env = {name: text for name, text in self.env.items() if name != 'OXPINYIN_CACHE'}
            if value is not None:
                env['OXPINYIN_CACHE'] = value
            with self.subTest(value=value), self.assertRaisesRegex(SystemExit, 'OXPINYIN_CACHE must name an absolute'):
                self.main('--archive', os.devnull, env=env)

    def test_checksum_mismatch_names_expected_and_actual(self):
        archive = self.work / 'corrupt.tar.gz'
        archive.write_bytes(b'not-the-pinned-archive')
        actual = hashlib.sha256(b'not-the-pinned-archive').hexdigest()
        status, out, err = self.main('--archive', str(archive))
        self.assertEqual((status, out), (1, ''))
        self.assertEqual(err, f'checksum mismatch: model20.text.tar.gz\n  expected: {model.archive_sha256()}\n'
                              f'  actual:   {actual}\n')
        self.assertFalse((self.cache / 'extracted').exists())

    def test_failed_download_names_the_url_and_leaves_no_part_file(self):
        self.fake('curl', 'import sys\n'
                          'open(sys.argv[sys.argv.index("--output") + 1], "w").close()\n'
                          'sys.exit(7)\n')
        url = 'https://127.0.0.1:1/model20.text.tar.gz'
        status, out, err = self.main('--url', url)
        self.assertEqual((status, out), (1, ''))
        self.assertEqual(err, f'downloading {url}\nfailed to download model20.text.tar.gz from {url}\n')
        self.assertEqual(list(self.cache.iterdir()), [])

    def test_url_defaults_to_the_environment_then_the_pin(self):
        log = self.work / 'curl.log'
        self.fake('curl', 'import sys\n'
                          f'open({str(log)!r}, "w").write("\\n".join(sys.argv[1:]) + "\\n")\n'
                          'sys.exit(22)\n')
        self.main()
        self.assertEqual(log.read_text().splitlines()[-1], model.MODEL_URL)
        self.main(env=dict(self.env, PINYIN_MODEL_URL='https://example.invalid/m'))
        self.assertEqual(log.read_text().splitlines()[-1], 'https://example.invalid/m')

    def test_download_extracts_and_a_second_run_skips(self):
        source = self.work / 'served.tar.gz'
        with self.pinned(source):
            self.fake('curl', 'import shutil, sys\n'
                              f'shutil.copyfile({str(source)!r}, sys.argv[sys.argv.index("--output") + 1])\n')
            status, out, err = self.main()
            self.assertEqual(status, 0, err)
            extracted = self.cache / 'extracted'
            self.assertEqual(out.splitlines()[-1], str(extracted))
            self.assertTrue(model.extraction_complete(extracted))
            self.assertTrue((self.cache / 'downloads/model20.text.tar.gz').is_file())
            self.assertEqual((self.cache / 'verified').read_text(),
                             f'schema=pinyin-model-cache-v1\nurl={model.MODEL_URL}\nsha256={model.archive_sha256()}\n')
            self.assertEqual([path.name for path in self.cache.iterdir() if path.name.startswith('.extract.')], [])
            self.fake('curl', 'import sys\nsys.exit(99)\n')
            status, out, err = self.main()
            self.assertEqual((status, out, err), (0, f'{extracted}\n', 'verified model cache present; skipping fetch\n'))
            # The cached download is reused without curl unless --force fetches again.
            (self.cache / 'verified').unlink()
            self.assertEqual(self.main()[0], 0)
            self.assertEqual(self.main('--force')[0], 1)
            cached = self.cache / 'downloads/model20.text.tar.gz'
            self.assertEqual(cached.read_bytes(), source.read_bytes())
            self.assertEqual(self.main()[0], 0)
            self.fake('curl', 'import sys\n'
                              'open(sys.argv[sys.argv.index("--output") + 1], "wb").write(b"wrong")\n')
            status, _, err = self.main('--force')
            self.assertEqual(status, 1)
            self.assertIn('checksum mismatch', err)
            self.assertEqual(cached.read_bytes(), source.read_bytes())

    def test_pin_change_refuses_the_previous_marker_and_archive(self):
        archive = self.work / 'old.tar.gz'
        options = ('--archive', str(archive))
        with self.pinned(archive):
            marker = self.cache / 'verified'
            old_digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            status, _, err = self.main(*options)
            self.assertEqual(status, 0, err)
            self.assertIn(f'sha256={old_digest}', marker.read_text().splitlines())

            new_digest = 'f' * 64
            self.pin.write_text(f'model_archive_sha256={new_digest}\n')
            status, out, err = self.main(*options)
            self.assertEqual((status, out), (1, ''))
            self.assertIn(f'expected: {new_digest}', err)
            self.assertIn(f'actual:   {old_digest}', err)
            # The new pin has its own entry; the previous one is as it was.
            self.assertNotEqual(self.cache, marker.parent)
            self.assertFalse((self.cache / 'extracted').exists())
            self.assertIn(f'sha256={old_digest}', marker.read_text().splitlines())

    def test_local_archive_with_a_missing_file_names_it(self):
        archive = self.work / 'short.tar.gz'
        names = [name for name in model.EXPECTED_MODEL_FILES if name not in ('art.table', 'punct.table')]
        with self.pinned(archive, names):
            status, _, err = self.main('--archive', str(archive))
            self.assertEqual(list(self.cache.iterdir()), [])
        self.assertEqual((status, err), (1, 'extracted archive is missing art.table\n'
                                            'extracted archive is missing punct.table\n'))

    def test_force_replaces_an_existing_extraction(self):
        archive = self.work / 'pinned.tar.gz'
        with self.pinned(archive):
            stale = self.cache / 'extracted' / 'stale'
            stale.parent.mkdir(parents=True)
            stale.touch()
            status, out, err = self.main('--archive', str(archive), '--force')
            self.assertEqual(out, f'{self.cache / "extracted"}\n')
        self.assertEqual(status, 0, err)
        self.assertFalse(stale.exists())

    def test_a_concurrent_force_cannot_touch_the_cache_during_publication(self):
        archive = self.work / 'pinned.tar.gz'
        with self.pinned(archive):
            options = ('--archive', str(archive))
            self.assertEqual(self.main(*options)[0], 0)
            extracted = self.cache / 'extracted'
            marker = self.cache / 'verified'
            real_replace = Path.replace

            def replace(path, destination):
                if destination == marker:
                    self.assertTrue(model.extraction_complete(extracted))
                    status, out, err = self.main(*options, '--force')
                    self.assertEqual((status, out), (1, ''))
                    self.assertIn('cache entry is busy', err)
                return real_replace(path, destination)

            with mock.patch.object(Path, 'replace', replace):
                self.assertEqual(self.main(*options, '--force')[0], 0)
            self.assertTrue(model.marker_matches(marker))

    def test_killing_the_fetch_lock_holder_releases_the_lock(self):
        entry = self.cache
        lock = entry.with_name(f'.{entry.name}.fetch-lock')
        hold_lock = (
            'from pathlib import Path\n'
            'import sys\n'
            'from tools.cachedir import exclusive_fetch\n'
            'with exclusive_fetch(Path(sys.argv[1])):\n'
            '    print("locked", flush=True)\n'
            '    sys.stdin.read()\n'
        )
        with subprocess.Popen([sys.executable, '-c', hold_lock, str(entry)], cwd=ROOT,
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True) as child:
            try:
                self.assertEqual(child.stdout.readline().strip(), 'locked')
                self.assertTrue(lock.is_file())
            finally:
                child.kill()
                child.wait(timeout=10)
        with cachedir.exclusive_fetch(entry):
            self.assertTrue(lock.is_file())
        self.assertTrue(lock.is_file())

    def test_marker_publication_failure_restores_the_previous_extraction(self):
        archive = self.work / 'pinned.tar.gz'
        with self.pinned(archive):
            options = ('--archive', str(archive))
            self.assertEqual(self.main(*options)[0], 0)
            marker = self.cache / 'verified'
            old_marker = marker.read_bytes()
            old_tree = self.cache / 'extracted' / 'previous'
            old_tree.touch()
            with mock.patch.object(Path, 'replace', side_effect=PermissionError('marker blocked')):
                status, _, err = self.main(*options, '--force')
            self.assertEqual(status, 1)
            self.assertIn('marker blocked', err)
            self.assertTrue(old_tree.is_file())
            self.assertEqual(marker.read_bytes(), old_marker)

    def test_refuses_a_tracked_cache_path(self):
        root = ROOT / 'tools/dev/__must_not_vendor_model20'
        self.env['OXPINYIN_CACHE'] = str(root)
        status, _, err = self.main('--archive', os.devnull)
        self.assertFalse(root.exists())
        self.assertEqual((status, err), (1, f'refusing to write model bytes to a path git would track: {self.cache}\n'
                                            'set OXPINYIN_CACHE to an ignored path or a path outside the repository\n'))

    def test_refuses_the_repository_when_git_is_unavailable(self):
        self.fake('git', 'import sys\nsys.exit(1)\n')
        root = ROOT / 'tools/dev/__must_not_vendor_model20_nogit'
        self.env['OXPINYIN_CACHE'] = str(root)
        status, _, err = self.main('--archive', os.devnull)
        self.assertFalse(root.exists())
        self.assertEqual((status, err), (1, 'refusing to write model bytes under the repository without git '
                                            f'ignore checks: {self.cache}\n'))
        # Without git, target/ is still accepted.
        self.env['OXPINYIN_CACHE'] = str(ROOT / 'target/model20-nogit-test')
        status, _, err = self.main('--archive', str(self.work / 'absent'))
        self.assertEqual(status, 1)
        self.assertTrue(err.startswith('local archive is not a file'), err)


if __name__ == '__main__':
    unittest.main()
