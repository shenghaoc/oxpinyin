#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The pinned contract system data: package extraction and the identity digest."""
import io
import os
from pathlib import Path
import stat
import sys
import tarfile
import tempfile
import unittest
import urllib.error
from unittest.mock import patch
import hashlib

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))

from tools.dev import contract_data


def deb(files):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w:xz') as archive:
        for name, payload in files.items():
            info = tarfile.TarInfo('./' + name)
            info.size = len(payload)
            archive.addfile(info, io.BytesIO(payload))
    data = buffer.getvalue()
    member = lambda name, blob: name.ljust(16).encode() + b'0'.ljust(12) + b'0'.ljust(6) * 2 + b'100644'.ljust(8) \
        + str(len(blob)).ljust(10).encode() + b'`\n' + blob + (b'\n' if len(blob) % 2 else b'')
    return b'!<arch>\n' + member('debian-binary', b'2.0\n') + member('data.tar.xz', data)


class PinTests(unittest.TestCase):
    def test_native_windows_amd64_uses_the_pinned_amd64_package(self):
        payload = deb({'usr/lib/x/libpinyin/data/table.conf': b'c'})
        with tempfile.TemporaryDirectory() as directory:
            expected = Path(directory, 'expected')
            contract_data.extract(payload, 'usr/lib/x/libpinyin/data', expected)
            pin = dict(version='test', content_sha256=contract_data.content_digest(expected),
                       archives={'amd64': dict(snapshot_sha1='a' * 40,
                                               sha256=hashlib.sha256(payload).hexdigest(),
                                               data_dir='usr/lib/x/libpinyin/data')})
            with patch.object(contract_data.platform, 'machine', return_value='AMD64'), \
                    patch.object(contract_data.urllib.request, 'urlopen', return_value=io.BytesIO(payload)):
                target = contract_data.fetch(Path(directory, 'cache'), pin=pin)
            self.assertEqual((target / 'table.conf').read_bytes(), b'c')

    def test_extraction_keeps_only_the_data_directory_and_both_digests_agree(self):
        payload = deb({'usr/lib/x/libpinyin/data/table.conf': b'c', 'usr/lib/x/libpinyin/data/a.bin': b'1',
                       'usr/share/doc/copyright': b'no'})
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory, 'data')
            contract_data.extract(payload, 'usr/lib/x/libpinyin/data', target)
            self.assertEqual(sorted(file.name for file in target.iterdir()), ['a.bin', 'table.conf'])
            self.assertEqual(contract_data.content_digest(target), contract_data.data_identity(target))

    def test_staged_files_are_read_only_and_a_damaged_read_only_copy_is_replaced(self):
        files = {'usr/lib/x/libpinyin/data/table.conf': b'c', 'usr/lib/x/libpinyin/data/a.bin': b'1'}
        payload = deb(files)
        with tempfile.TemporaryDirectory() as directory:
            expected = Path(directory, 'expected')
            contract_data.extract(payload, 'usr/lib/x/libpinyin/data', expected)
            pin = dict(version='test', content_sha256=contract_data.content_digest(expected),
                       archives={'amd64': dict(snapshot_sha1='a' * 40, sha256=hashlib.sha256(payload).hexdigest(),
                                               data_dir='usr/lib/x/libpinyin/data')})
            write_bits = stat.S_IWUSR | stat.S_IWGRP | stat.S_IWOTH
            with patch.object(contract_data.urllib.request, 'urlopen', return_value=io.BytesIO(payload)) as open_url:
                target = contract_data.fetch(Path(directory, 'cache'), 'amd64', pin)
                self.assertEqual(open_url.call_count, 1)
            for name in ('table.conf', 'a.bin'):
                self.assertEqual(os.lstat(target / name).st_mode & write_bits, 0, name)
            with patch.object(contract_data.urllib.request, 'urlopen') as open_url:
                self.assertEqual(contract_data.fetch(Path(directory, 'cache'), 'amd64', pin), target)
                open_url.assert_not_called()
            # Damage one read-only file as a writer with the owner's bit would, then
            # protect it again: the next fetch finds the digest wrong and replaces the
            # copy whole, read-only files included.
            damaged = target / 'a.bin'
            os.chmod(damaged, stat.S_IMODE(os.lstat(damaged).st_mode) | stat.S_IWUSR)
            damaged.write_bytes(b'2')
            os.chmod(damaged, stat.S_IMODE(os.lstat(damaged).st_mode) & ~write_bits)
            with patch.object(contract_data.urllib.request, 'urlopen', return_value=io.BytesIO(payload)) as open_url:
                self.assertEqual(contract_data.fetch(Path(directory, 'cache'), 'amd64', pin), target)
                self.assertEqual(open_url.call_count, 1)
            self.assertEqual(damaged.read_bytes(), b'1')
            self.assertEqual(contract_data.content_digest(target), pin['content_sha256'])
            for name in ('table.conf', 'a.bin'):
                self.assertEqual(os.lstat(target / name).st_mode & write_bits, 0, name)

    def test_fetch_retries_only_transient_errors_and_keeps_checksum_gate(self):
        payload = deb({'usr/lib/x/libpinyin/data/table.conf': b'c'})
        pin = dict(version='test', content_sha256=hashlib.sha256(b'wrong').hexdigest(),
                   archives={'amd64': dict(snapshot_sha1='a' * 40, sha256=hashlib.sha256(payload).hexdigest(),
                                            data_dir='usr/lib/x/libpinyin/data')})
        with tempfile.TemporaryDirectory() as directory, patch.object(contract_data.time, 'sleep') as sleep:
            response = io.BytesIO(payload)
            with patch.object(contract_data.urllib.request, 'urlopen', side_effect=[
                    urllib.error.HTTPError('url', 503, 'temporary', {}, None), TimeoutError(), response]) as open_url:
                with self.assertRaisesRegex(SystemExit, 'pinned content digest'):
                    contract_data.fetch(directory, 'amd64', pin)
                self.assertEqual(open_url.call_count, 3)
                self.assertEqual(sleep.call_count, 2)
            with patch.object(contract_data.urllib.request, 'urlopen', side_effect=urllib.error.HTTPError(
                    'url', 404, 'missing', {}, None)) as open_url:
                with self.assertRaises(urllib.error.HTTPError):
                    contract_data.fetch(directory, 'amd64', pin)
                self.assertEqual(open_url.call_count, 1)


if __name__ == '__main__':
    unittest.main()
