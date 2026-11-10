"""The REPL launcher isolates mutation and forwards one backend and mode."""
from pathlib import Path
import tempfile
import json
import sys
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from tools.dev import repl


class ReplTests(unittest.TestCase):
    def test_session_writes_never_reach_fixture(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            fixture = root / 'fixtures' / 'w3' / 'db'
            fixture.mkdir(parents=True)
            (fixture / 'table.conf').write_bytes(b'original')
            copies = []

            def run(command, **kwargs):
                if command[0] == 'cargo':
                    self.assertEqual(command[command.index('--features') + 1], 'bdb')
                    kwargs['stdout'].write(json.dumps({'reason': 'compiler-artifact', 'target': {'name': 'oxpinyin-repl'}, 'executable': '/custom/target/triple/debug/oxpinyin-repl'}) + '\n')
                    return 0
                self.assertEqual(command[0], '/custom/target/triple/debug/oxpinyin-repl')
                self.assertEqual(command[1], 'parser2')
                self.assertEqual(command[3:], ['--parser', 'zhuyin'])
                data = Path(command[2])
                self.assertNotEqual(data, fixture)
                copies.append(data)
                (data / 'table.conf').write_bytes(b'changed')
                return 7

            with patch.object(repl, 'ROOT', root), patch.object(repl.slot, 'run_command', side_effect=run):
                self.assertEqual(repl.main(['parser2', '--backend', 'bdb', '--parser', 'zhuyin']), 7)
            self.assertEqual((fixture / 'table.conf').read_bytes(), b'original')
            self.assertFalse(copies[0].exists())

    def test_table_modes_stage_authored_text_without_mutating_w3(self):
        for mode in ('phrase_table', 'chewing_table'):
            with self.subTest(mode=mode):
                def run(command, **kwargs):
                    if command[0] == 'cargo':
                        kwargs['stdout'].write(json.dumps({'reason': 'compiler-artifact',
                            'target': {'name': 'oxpinyin-repl'}, 'executable': '/built/repl'}) + '\n')
                        return 0
                    data = Path(command[2])
                    for table in ('gb_char', 'gbk_char', 'opengram', 'merged'):
                        self.assertEqual((data / f'{table}.table').read_bytes(),
                            (repl.ROOT / 'fixtures/datagen-toned' / f'{table}.table').read_bytes())
                    self.assertEqual((data / 'table.conf').read_bytes(),
                        (repl.ROOT / 'fixtures/w3/tkt/table.conf').read_bytes())
                    return 0
                with patch.object(repl.slot, 'run_command', side_effect=run):
                    self.assertEqual(repl.main([mode]), 0)

    def test_failed_build_does_not_start_session(self):
        with patch.object(repl.slot, 'run_command', return_value=4) as run:
            self.assertEqual(repl.main(['pinyin']), 4)
            self.assertEqual(run.call_count, 1)


if __name__ == "__main__":
    unittest.main()
