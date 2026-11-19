"""Unit tests for tools.dev.fuzz: the rules of `just fuzz` that need no fuzzer build."""
from pathlib import Path
import contextlib
import io
import os
import re
import shutil
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.dev import fuzz


def completed(stdout, returncode=0):
    return mock.Mock(returncode=returncode, stdout=stdout, stderr='')


class OptionTests(unittest.TestCase):
    def test_options_that_break_the_recipe_are_refused(self):
        refused = {
            ('-f', 'testcase'): r"^refused '-f': -f makes afl-fuzz write each input to a file",
            ('-ftestcase',): r"^refused '-ftestcase': -f ",
            ('-df', 'testcase'): r"^refused '-df': -f ",
            ('-x', 'dict@@'): r"^refused 'dict@@': @@ passes the target a file name",
            ('-V', '60', '@@'): r"^refused '@@': @@ ",
            ('--',): r"^refused '--': the recipe supplies the target command after --\.$",
            ('-V', '60', '--', 'other'): r"^refused '--'",
            ('target-command',): r"^refused 'target-command': afl-fuzz would take it as the target command",
            ('-M', 'main'): r"^refused '-M': -M names a main instance; the recipe runs one afl-fuzz instance",
            ('-Sworker',): r"^refused '-Sworker': -S names a secondary instance",
            ('-dM', 'main'): r"^refused '-dM': -M ",
        }
        for args, message in refused.items():
            with self.subTest(args=args), self.assertRaisesRegex(fuzz.FuzzError, message):
                fuzz.refuse_options(list(args))

    def test_ordinary_options_and_their_values_pass(self):
        # A value is not read as an option: `-x -f` names a dictionary called -f.
        for args in ([], ['-V', '60'], ['-V60'], ['-d'], ['-x', '-f'], ['-V', '-M'], ['-G', '167681', '-V', '20'],
                     ['-p', 'explore', '-Q']):
            with self.subTest(args=args):
                fuzz.refuse_options(args)

    def test_the_option_string_is_afl_fuzz_440c(self):
        letters, valued = fuzz._option_letters()
        self.assertTrue({'f', 'M', 'S', 'V', 'x', 'c', 'i', 'o'} <= valued)
        self.assertTrue({'d', 'D', 'Q', 'h'} <= letters - valued)


class CorpusTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.directory)

    def test_a_seed_update_leaves_an_older_queue_entry_unchanged(self):
        seeds = self.directory / 'seeds' / 'probe'
        seeds.mkdir(parents=True)
        corpus = self.directory / 'corpus' / 'probe'
        (seeds / 'one').write_bytes(b'first')
        fuzz.seed_corpus('probe', corpus, fuzz=self.directory)
        queue_entry = self.directory / 'queue-entry'
        os.link(corpus / 'one', queue_entry)  # afl-fuzz links its seed entries to the corpus
        (seeds / 'one').write_bytes(b'second')
        fuzz.seed_corpus('probe', corpus, fuzz=self.directory)
        self.assertEqual((corpus / 'one').read_bytes(), b'second')
        self.assertEqual(queue_entry.read_bytes(), b'first')
        self.assertEqual(sorted(path.name for path in corpus.iterdir()), ['one'])

    def test_an_empty_corpus_gets_one_byte(self):
        corpus = self.directory / 'corpus' / 'empty'
        fuzz.seed_corpus('empty', corpus, fuzz=self.directory)
        self.assertEqual((corpus / 'one-byte').read_bytes(), b'\0')

    # AFL++ names queue entries and findings with ":" (id:000000,...), which a
    # Windows file name cannot hold. Fuzzing runs on Linux only, so these tests
    # are defined on POSIX only.
    if os.name == 'posix':
        def test_queue_promotion_takes_every_entry_and_changes_no_corpus_file(self):
            queue = self.directory / 'queue'
            (queue / '.state').mkdir(parents=True)
            corpus = self.directory / 'corpus'
            corpus.mkdir()
            (queue / 'id:000000,orig:seed').write_bytes(b'seed')
            (queue / '0123456789abcdef0123456789abcdef01234567').write_bytes(b'hash-named')
            (queue / 'id:000001,src:000000').write_bytes(b'hash-named')
            # The seed is in the corpus under its own name, as afl-fuzz read it.
            present = corpus / 'seed'
            present.write_bytes(b'seed')
            inode = present.stat().st_ino
            self.assertEqual(fuzz.promote_queue(queue, corpus), 1)
            self.assertEqual(present.stat().st_ino, inode)
            self.assertEqual(sorted(path.name for path in corpus.iterdir()),
                             sorted(['seed', fuzz.content_name(queue / 'id:000001,src:000000')]))
            self.assertEqual(fuzz.promote_queue(queue, corpus), 0)

        def test_every_finding_counts_except_the_readme(self):
            crashes = self.directory / 'crashes'
            crashes.mkdir()
            (crashes / 'README.txt').write_text('AFL++ notes')
            self.assertEqual(fuzz.count_findings(crashes), 0)
            (crashes / '3f786850e387550fdab836ed7e6dc881de23001b').write_bytes(b'x')  # AFL_SHA1_FILENAMES=1
            (crashes / 'id:000000,sig:06').write_bytes(b'y')
            self.assertEqual(fuzz.count_findings(crashes), 2)
            self.assertEqual(fuzz.count_findings(self.directory / 'absent'), 0)

        def test_a_seed_issue_keeps_the_named_queue_entry(self):
            run = self.directory / 'run'
            queue = run / 'default' / 'queue'
            queue.mkdir(parents=True)
            (queue / 'id:000001,time:0,execs:0,orig:CRASH').write_bytes(b'CRASH')
            log = ("[*] Attempting dry run with 'id:000001,time:0,execs:0,orig:CRASH'...\n"
                   "[!] WARNING: Test case 'id:000001,time:0,execs:0,orig:CRASH' results in a crash, skipping\n"
                   "[-] PROGRAM ABORT : As AFL_EXIT_ON_SEED_ISSUES is set, afl-fuzz exits.\n")
            self.assertEqual(fuzz.keep_seed_issues(log, run / 'default', run), 1)
            self.assertEqual((run / 'seed-issues' / 'id:000001,time:0,execs:0,orig:CRASH').read_bytes(), b'CRASH')
            self.assertEqual(fuzz.keep_seed_issues("Test case 'gone' results in a timeout\n", run / 'default', run), 0)
            self.assertIsNone(fuzz.SEED_ISSUE.search('AFL_EXIT_ON_SEED_ISSUES is set\n'))

    def test_fuzzer_stats_are_read_by_key(self):
        stats = self.directory / 'fuzzer_stats'
        stats.write_text('afl_version       : ++4.40c\nexecs_done        : 807\ncommand_line      : a -- b\n')
        self.assertEqual(fuzz.read_stats(stats),
                         {'afl_version': '++4.40c', 'execs_done': '807', 'command_line': 'a -- b'})


class ToolsRuleTests(unittest.TestCase):
    def setUp(self):
        self.data = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.data)
        self.environ = {'PATH': '/tools', 'XDG_DATA_HOME': str(self.data)}
        self.pin = fuzz.read_tool_versions()['CARGO_AFL_VERSION']

    def test_the_pinned_cargo_afl_with_plugins_is_accepted(self):
        line = f'cargo-afl {self.pin} (AFL++ version 4.40c with plugins)'
        run = mock.Mock(return_value=completed(line + '\n'))
        self.assertEqual(fuzz.tools_rule(self.environ, run=run), line)
        run.assert_called_once_with(['cargo', 'afl', '--version'], capture_output=True, text=True,
                                    env=self.environ, cwd=fuzz.FUZZ, check=False)

    def test_other_installations_are_refused(self):
        for found in ('cargo-afl 99.0.0 (AFL++ version 4.40c with plugins)',
                      f'cargo-afl {self.pin} (AFL++ version 4.40c)', ''):
            with self.subTest(found=found), \
                    self.assertRaisesRegex(fuzz.FuzzError, r"expected 'cargo-afl .*'; run just fuzz-setup\.$"):
                fuzz.tools_rule(self.environ, run=mock.Mock(return_value=completed(found)))
        with self.assertRaisesRegex(fuzz.FuzzError, "found 'no cargo-afl'"):
            fuzz.tools_rule(self.environ, run=mock.Mock(side_effect=FileNotFoundError))

    def test_a_checkout_is_refused_and_kept(self):
        checkout = self.data / 'afl.rs/AFLplusplus'
        (checkout / '.git').mkdir(parents=True)
        (checkout / 'local-work').write_text('kept')
        line = f'cargo-afl {self.pin} (AFL++ version 4.40c with plugins)'
        path = re.escape(str(checkout))
        message = rf'^{path} is an AFL\+\+ git checkout .* Remove {path}, then run just fuzz-setup\.$'
        with self.assertRaisesRegex(fuzz.FuzzError, message):
            fuzz.tools_rule(self.environ, run=mock.Mock(return_value=completed(line)))
        # setup refuses before it runs any command, and deletes nothing.
        with mock.patch.object(fuzz.sys, 'platform', 'linux'), \
                mock.patch.object(fuzz.subprocess, 'run') as ran, \
                self.assertRaisesRegex(fuzz.FuzzError, message):
            fuzz.setup(self.environ)
        ran.assert_not_called()
        self.assertEqual((checkout / 'local-work').read_text(), 'kept')

    def test_fuzzing_is_refused_off_linux(self):
        refusal = ('fuzz: AFL++ fuzzing is supported on Linux only; on another host, fuzz in a native Linux '
                   'container (docs/runbooks/setup.md#fuzzing).\n')
        for host in ('darwin', 'win32'):
            for argv in (['setup'], ['run', 'chewing-parser'], ['run', 'chewing-parser', '-f', 'x']):
                stderr = io.StringIO()
                with self.subTest(host=host, argv=argv), mock.patch.object(fuzz.sys, 'platform', host), \
                        mock.patch.object(fuzz.subprocess, 'run') as ran, contextlib.redirect_stderr(stderr):
                    self.assertEqual(fuzz.main(argv, self.environ), 1)
                    self.assertEqual(stderr.getvalue(), refusal)
                    ran.assert_not_called()

    def test_usage(self):
        stderr = io.StringIO()
        with contextlib.redirect_stderr(stderr):
            self.assertEqual(fuzz.main([], self.environ), 2)
            self.assertEqual(fuzz.main(['run'], self.environ), 2)
        self.assertIn(fuzz.USAGE, stderr.getvalue())


if __name__ == '__main__':
    unittest.main()
