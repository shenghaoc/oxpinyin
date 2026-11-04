#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""The pinned contract system data: package extraction and the identity digest."""
import io
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
import urllib.error
from unittest.mock import patch
import hashlib

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
sys.path.insert(0, str(ROOT / 'tools/bisection'))

from tools.dev import contract_data
import contract_reference


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
    def test_the_pin_names_one_exact_version_and_both_architectures(self):
        pin = contract_data.read_pin()
        self.assertEqual(pin['version'], '2.11.92-1')
        self.assertEqual(set(pin['archives']), {'amd64', 'arm64'})
        for entry in pin['archives'].values():
            self.assertRegex(entry['sha256'], r'^[0-9a-f]{64}$')
            self.assertRegex(entry['snapshot_sha1'], r'^[0-9a-f]{40}$')

    def test_extraction_keeps_only_the_data_directory_and_both_digests_agree(self):
        payload = deb({'usr/lib/x/libpinyin/data/table.conf': b'c', 'usr/lib/x/libpinyin/data/a.bin': b'1',
                       'usr/share/doc/copyright': b'no'})
        with tempfile.TemporaryDirectory() as directory:
            target = Path(directory, 'data')
            contract_data.extract(payload, 'usr/lib/x/libpinyin/data', target)
            self.assertEqual(sorted(file.name for file in target.iterdir()), ['a.bin', 'table.conf'])
            self.assertEqual(contract_data.content_digest(target), contract_reference.data_identity(target))

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
