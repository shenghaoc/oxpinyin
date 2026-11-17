"""The shared child-process helpers in tools/dev/process.py."""
import io
from pathlib import Path
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))
from tools.dev import process


class CaptureTests(unittest.TestCase):
    def test_a_captured_childs_stderr_is_forwarded(self):
        # wslc and fetch-contract-data write diagnostics to stderr while their stdout is captured.
        done = mock.Mock(stdout=b'out', stderr=b'diagnostic\n', status=0)
        stream = io.TextIOWrapper(io.BytesIO(), encoding='utf-8')
        with mock.patch.object(process.bench, 'capture', return_value=done), mock.patch.object(sys, 'stderr', stream):
            self.assertIs(process.capture(['child']), done)
            stream.flush()
            self.assertEqual(stream.buffer.getvalue(), b'diagnostic\n')


if __name__ == '__main__':
    unittest.main()
