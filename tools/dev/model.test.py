#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
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

from tools.dev import model


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
        self.cache = self.work / 'cache'
        self.bin = self.work / 'bin'
        self.bin.mkdir()
        self.env = dict(os.environ, PATH=f'{self.bin}:{os.environ.get("PATH", os.defpath)}')
        self.env.pop('PINYIN_MODEL_CACHE', None)
        self.env.pop('PINYIN_MODEL_URL', None)

    def fake(self, name, body):
        path = self.bin / name
        path.write_text('#!/bin/sh\n' + body)
        path.chmod(0o755)
        return path

    def main(self, *argv, env=None):
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            status = model.main(list(argv), env=self.env if env is None else env)
        return status, out.getvalue(), err.getvalue()

    def pinned(self, archive):
        return mock.patch.object(model, 'MODEL20_SHA256', write_archive(archive))

    def test_help_and_option_errors(self):
        status, out, err = self.main('--help')
        self.assertEqual((status, err), (0, ''))
        self.assertIn('--cache-dir DIR', out)
        status, out, err = self.main('--bogus')
        self.assertEqual((status, out), (2, ''))
        self.assertTrue(err.startswith('unknown option: --bogus\nUsage:'), err)
        status, out, err = self.main('--cache-dir')
        self.assertEqual(status, 2)
        self.assertTrue(err.startswith('option needs a value: --cache-dir\n'), err)

    def test_checksum_mismatch_names_expected_and_actual(self):
        archive = self.work / 'corrupt.tar.gz'
        archive.write_bytes(b'not-the-pinned-archive')
        actual = hashlib.sha256(b'not-the-pinned-archive').hexdigest()
        status, out, err = self.main('--cache-dir', str(self.cache), '--archive', str(archive))
        self.assertEqual((status, out), (1, ''))
        self.assertEqual(err, f'checksum mismatch: model20.text.tar.gz\n  expected: {model.MODEL20_SHA256}\n'
                              f'  actual:   {actual}\n')
        self.assertFalse((self.cache / 'extracted').exists())

    def test_missing_local_archive(self):
        status, _, err = self.main('--cache-dir', str(self.cache), '--archive', str(self.work / 'absent'))
        self.assertEqual((status, err), (1, f'local archive is not a file: {self.work / "absent"}\n'))

    def test_failed_download_names_the_url_and_leaves_no_part_file(self):
        self.fake('curl', 'for a; do prev=$out; out=$a; done\n: > "$prev"; exit 7\n')
        url = 'https://127.0.0.1:1/model20.text.tar.gz'
        status, out, err = self.main('--cache-dir', str(self.cache), '--url', url)
        self.assertEqual((status, out), (1, ''))
        self.assertEqual(err, f'downloading {url}\nfailed to download model20.text.tar.gz from {url}\n')
        self.assertEqual(list((self.cache / 'downloads').iterdir()), [])

    def test_url_defaults_to_the_environment_then_the_pin(self):
        log = self.work / 'curl.log'
        self.fake('curl', f'printf "%s\\n" "$@" > {log}; exit 22\n')
        self.main('--cache-dir', str(self.cache))
        self.assertEqual(log.read_text().splitlines()[-1], model.MODEL_URL)
        self.main('--cache-dir', str(self.cache), env=dict(self.env, PINYIN_MODEL_URL='https://example.invalid/m'))
        self.assertEqual(log.read_text().splitlines()[-1], 'https://example.invalid/m')

    def test_download_extracts_and_a_second_run_skips(self):
        source = self.work / 'served.tar.gz'
        with self.pinned(source):
            self.fake('curl', f'for a; do prev=$out; out=$a; done\ncp {source} "$prev"\n')
            status, out, err = self.main('--cache-dir', str(self.cache))
            self.assertEqual(status, 0, err)
            extracted = self.cache / 'extracted'
            self.assertEqual(out.splitlines()[-1], str(extracted))
            self.assertTrue(model.extraction_complete(extracted))
            self.assertTrue((self.cache / 'downloads/model20.text.tar.gz').is_file())
            self.assertEqual((self.cache / 'verified').read_text(),
                             f'schema=pinyin-model-cache-v1\nurl={model.MODEL_URL}\nsha256={model.MODEL20_SHA256}\n')
            self.assertEqual([path.name for path in self.cache.iterdir() if path.name.startswith('.extract.')], [])
            self.fake('curl', 'exit 99\n')
            status, out, err = self.main('--cache-dir', str(self.cache))
            self.assertEqual((status, out, err), (0, f'{extracted}\n', 'verified model cache present; skipping fetch\n'))
            # The cached download is reused without curl until --force discards it.
            (self.cache / 'verified').unlink()
            self.assertEqual(self.main('--cache-dir', str(self.cache))[0], 0)
            self.assertEqual(self.main('--cache-dir', str(self.cache), '--force')[0], 1)

    def test_local_archive_with_a_missing_file_names_it(self):
        archive = self.work / 'short.tar.gz'
        names = [name for name in model.EXPECTED_MODEL_FILES if name not in ('art.table', 'punct.table')]
        with mock.patch.object(model, 'MODEL20_SHA256', write_archive(archive, names)):
            status, _, err = self.main('--cache-dir', str(self.cache), '--archive', str(archive))
        self.assertEqual((status, err), (1, 'extracted archive is missing art.table\n'
                                            'extracted archive is missing punct.table\n'))
        self.assertEqual(list(self.cache.iterdir()), [])

    def test_force_replaces_an_existing_extraction(self):
        archive = self.work / 'pinned.tar.gz'
        stale = self.cache / 'extracted' / 'stale'
        stale.parent.mkdir(parents=True)
        stale.touch()
        with self.pinned(archive):
            status, out, err = self.main('--cache-dir', str(self.cache), '--archive', str(archive), '--force')
        self.assertEqual(status, 0, err)
        self.assertFalse(stale.exists())
        self.assertEqual(out, f'{self.cache / "extracted"}\n')

    def test_refuses_a_tracked_cache_path(self):
        cache = ROOT / 'tools/dev/__must_not_vendor_model20'
        status, _, err = self.main('--cache-dir', str(cache), '--archive', '/dev/null')
        self.assertFalse(cache.exists())
        self.assertEqual((status, err), (1, f'refusing to write model bytes to a path git would track: {cache}\n'
                                            'use a build cache under target/ (the default) or another ignored path\n'))

    def test_refuses_the_repository_when_git_is_unavailable(self):
        self.fake('git', 'exit 1\n')
        cache = ROOT / 'tools/dev/__must_not_vendor_model20_nogit'
        status, _, err = self.main('--cache-dir', str(cache), '--archive', '/dev/null')
        self.assertFalse(cache.exists())
        self.assertEqual((status, err), (1, 'refusing to write model bytes under the repository without git '
                                            f'ignore checks: {cache}\n'))
        # Without git, target/ is still accepted.
        status, _, err = self.main('--cache-dir', str(ROOT / 'target/model20-nogit-test'), '--archive',
                                   str(self.work / 'absent'))
        self.assertEqual(status, 1)
        self.assertTrue(err.startswith('local archive is not a file'), err)

    def test_cli_entry_prints_only_the_extracted_directory(self):
        archive = self.work / 'pinned.tar.gz'
        digest = write_archive(archive)
        code = ('import sys; from tools.dev import model; model.MODEL20_SHA256 = sys.argv[1]; '
                'from tools.dev import cli; raise SystemExit(cli.main(sys.argv[2:]))')
        done = subprocess.run([sys.executable, '-X', 'lazy_imports=normal', '-c', code, digest, 'fetch-model',
                               '--cache-dir', str(self.cache), '--archive', str(archive)],
                              cwd=ROOT, env=self.env, capture_output=True, text=True, timeout=60)
        self.assertEqual((done.returncode, done.stdout, done.stderr), (0, f'{self.cache / "extracted"}\n', ''))


if __name__ == '__main__':
    unittest.main()
