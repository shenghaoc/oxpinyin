"""Source-keyed target recovery after interrupted compatibility runs."""
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.compat import guest


class SourceTargetTests(unittest.TestCase):
    def test_interrupted_target_is_discarded_and_completed_target_is_kept(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / 'target'
            marker = guest.prepare_source_target(target)
            (target / 'object').write_text('unsealed')
            marker = guest.prepare_source_target(target)
            self.assertFalse((target / 'object').exists())
            (target / 'object').write_text('sealed')
            marker.unlink()
            marker = guest.prepare_source_target(target)
            self.assertEqual((target / 'object').read_text(), 'sealed')
            self.assertTrue(marker.exists())

    def test_completed_failed_run_allows_reuse(self):
        with tempfile.TemporaryDirectory() as temp:
            marker = guest.prepare_source_target(Path(temp) / 'target')
            guest.finish_source_target(marker, 1)
            self.assertFalse(marker.exists())
            self.assertTrue(guest.prepare_source_target(Path(temp) / 'target').exists())

    def test_redirected_marker_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            target = Path(temp) / 'target'
            target.mkdir()
            (target / '.compat-incomplete').symlink_to(Path(temp) / 'elsewhere')
            with self.assertRaisesRegex(ValueError, 'symlink'):
                guest.prepare_source_target(target)


if __name__ == '__main__':
    unittest.main()
