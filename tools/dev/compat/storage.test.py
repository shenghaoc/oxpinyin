"""Storage selection must never include unrelated volume names."""
from pathlib import Path
import subprocess
import tempfile
import sys
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev.compat import storage


class StorageTests(unittest.TestCase):
    def test_only_exact_compat_names_are_selected(self):
        names = storage.volume_names('NAME TYPE\noxpinyin-compat-0123456789abcdef named\n'
                                     'other-volume named\noxpinyin-compat-0123456789abcdef-extra named\n')
        self.assertEqual(names, ['oxpinyin-compat-0123456789abcdef'])

    def test_labels_identify_active_or_orphaned_owner_and_legacy(self):
        owner = '0123456789abcdef'
        name = 'oxpinyin-compat-v2-' + owner
        metadata = {'labels': {storage.PROJECT: 'oxpinyin', storage.OWNER: owner,
                               storage.PURPOSE: storage.PURPOSE_VALUE}}
        self.assertEqual(storage.classify(name, metadata, {owner}), 'active owner ' + owner)
        self.assertEqual(storage.classify(name, metadata, set()), 'orphaned owner ' + owner)
        self.assertEqual(storage.classify('oxpinyin-compat-' + owner, {'labels': {}}, set()), 'legacy, unlabelled')
        self.assertEqual(storage.classify(name, {'labels': {}}, set()), 'unowned or invalid labels')
        self.assertEqual(storage.classify(name, {'labels': dict(metadata['labels'], **{storage.OWNER: 'f'*16})}, set()),
                         'unowned or invalid labels')

    def test_clean_guest_removes_only_selected_owned_targets(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            targets = root / 'container/0123456789abcdef/targets'
            targets.mkdir(parents=True)
            (targets / 'artifact').write_text('data')
            stable_targets = root / 'targets/image/triple/dev-opt1/tkrzw'
            stable_targets.mkdir(parents=True)
            (stable_targets / 'artifact').write_text('data')
            model = root / 'model20'
            model.mkdir()
            (model / 'verified').write_text('keep')
            result = subprocess.run([sys.executable, '-c', storage.CLEAN_GUEST, str(root), 'targets'],
                                    capture_output=True, text=True, input='\n')
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse(targets.exists())
            self.assertFalse((root / 'targets').exists())
            self.assertTrue((model / 'verified').is_file())

    @unittest.skipIf(sys.platform == 'win32', 'fcntl locks are Unix-only')
    def test_clean_guest_holds_lock_until_reclamation_finishes(self):
        import fcntl
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'work').mkdir()
            process = subprocess.Popen([sys.executable, '-u', '-c', storage.CLEAN_GUEST, str(root), 'work'],
                                       stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
            try:
                lines = iter(process.stdout.readline, '')
                self.assertTrue(any(line.strip() == 'OXPINYIN_CLEAN_READY' for line in lines))
                with (root / 'run.lock').open('r+') as lock:
                    with self.assertRaises(BlockingIOError):
                        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                process.stdin.write('\n')
                process.stdin.flush()
                self.assertEqual(process.wait(timeout=5), 0)
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()
                process.stdin.close()
                process.stdout.close()

    def test_clean_guest_refuses_redirected_or_unowned_namespace(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            outside = root / 'outside'
            outside.mkdir()
            (outside / 'keep').write_text('keep')
            containers = root / 'container'
            containers.mkdir()
            (containers / '0123456789abcdef').symlink_to(outside, target_is_directory=True)
            result = subprocess.run([sys.executable, '-c', storage.CLEAN_GUEST, str(root), 'targets'],
                                    capture_output=True, text=True, input='\n')
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue((outside / 'keep').is_file())



if __name__ == '__main__':
    unittest.main()
