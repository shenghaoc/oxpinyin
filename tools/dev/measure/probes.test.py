#!/usr/bin/env -S uv run --no-project --managed-python python -X lazy_imports=normal
"""Tests for probes.py: reading a crate's artifact name, the egress table, and the debug-info probe's flow.

The probes run here against stub tools on PATH (a cargo that builds nothing, a readelf that prints canned
sections) and a stand-in for curl, so every test runs on macOS and Linux. One test runs the real curl against
a port nothing listens on; it needs curl.
"""
import contextlib
import hashlib
import io
import json
import os
from pathlib import Path
import shutil
import socket
import sys
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT))
from tools.dev import runners
from tools.dev.measure import probes


class Streams:
    """Replace the standard streams with byte-backed text wrappers and keep what was written."""

    def __enter__(self):
        self.out_bytes, self.err_bytes = io.BytesIO(), io.BytesIO()
        self.out = io.TextIOWrapper(self.out_bytes, encoding='utf-8', write_through=True)
        self.err = io.TextIOWrapper(self.err_bytes, encoding='utf-8', write_through=True)
        self.stack = contextlib.ExitStack()
        self.stack.enter_context(mock.patch.object(sys, 'stdout', self.out))
        self.stack.enter_context(mock.patch.object(sys, 'stderr', self.err))
        return self

    def __exit__(self, *exc):
        self.stack.close()

    @property
    def stdout(self):
        return self.out_bytes.getvalue().decode()

    @property
    def stderr(self):
        return self.err_bytes.getvalue().decode()


class ManifestTests(unittest.TestCase):
    def test_the_package_name_is_the_first_name_line_whatever_its_section(self):
        lines = ['[dependencies]', 'name = "other"', '[package]', 'name = "oxpinyin-capi"']
        self.assertEqual(probes.package_name(lines), 'other')

    def test_what_follows_the_closing_quote_stays(self):
        self.assertEqual(probes.package_name(['name = "oxpinyin-capi" # the crate']), 'oxpinyin-capi # the crate')
        self.assertEqual(probes.package_name(['name = "a" # "b"']), 'a" # "b')

    def test_a_name_without_spaces_around_the_equals_sign_is_not_read(self):
        self.assertEqual(probes.package_name(['name="x"', ' name = "y"']), '')

    def test_the_library_name_wins_inside_its_section_only(self):
        lines = ['[package]', 'name = "oxpinyin-capi"', '[lib]', 'name = "pinyin"', 'crate-type = ["cdylib"]',
                 '[dependencies]', 'name = "ignored"']
        self.assertEqual(probes.library_name(lines), 'pinyin')

    def test_a_library_name_without_spaces_is_one_word_and_yields_nothing(self):
        self.assertEqual(probes.library_name(['[lib]', 'name="pinyin"']), '')

    def test_quotes_and_commas_go_before_the_words_are_read(self):
        self.assertEqual(probes.library_name(['[lib]', 'name = "pinyin",']), 'pinyin')

    def test_a_section_header_with_a_comment_still_opens_the_section(self):
        self.assertEqual(probes.library_name(['[lib] # the cdylib', 'name = "pinyin"']), 'pinyin')

    def test_another_header_closes_it(self):
        self.assertEqual(probes.library_name(['[lib]', '[[bin]]', 'name = "pinyin"']), '')

    def test_several_names_are_several_lines_and_trailing_newlines_go(self):
        self.assertEqual(probes.library_name(['[lib]', 'name = "a"', 'name = "b"']), 'a\nb')
        self.assertEqual(probes.library_name(['[lib]', 'name = "a"', 'name = "b"', 'name="c"']), 'a\nb')
        self.assertEqual(probes.library_name(['[lib]', 'name="c"']), '')

    def test_debug_sections_are_the_lines_that_name_one(self):
        listing = (b'  [27] .debug_aranges    PROGBITS         0000000000000000  0013b0e6\n'
                   b'       0000000000000030  0000000000000000           0     0     1\n'
                   b'  [28] .debug_info       PROGBITS         0000000000000000  0013b116\n'
                   b'  [29] .text             PROGBITS         0000000000001000  00001000\n')
        self.assertEqual(probes.debug_sections(listing), 2)
        self.assertEqual(probes.debug_sections(b''), 0)


class EgressTests(unittest.TestCase):
    def setUp(self):
        self.streams = self.enterContext(Streams())
        self.calls = []

    def fetch(self, codes=None, status=b'', default=b'200'):
        """A curl stand-in: the code asked for per URL, STATUS for the proxy page."""
        codes = codes or {}

        def curl(arguments, env):
            self.calls.append((list(arguments), dict(env)))
            url = arguments[-1]
            if url.endswith('/__agentproxy/status'):
                return 0, status
            return 0, codes.get(url, default)
        return mock.patch.object(probes, 'curl', curl)

    def probe(self, env=None, **fetch):
        with self.fetch(**fetch):
            return probes.probe_egress([], env or {})

    def test_every_host_answering_is_exit_0(self):
        status = self.probe(codes={'https://mirror.gcr.io/v2/': b'401', 'https://index.crates.io/config.json': b'301'})
        self.assertEqual(status, 0)
        rows = self.streams.stdout.splitlines()
        self.assertEqual(rows[-2:], ['', 'RESULT: every host a perf image build needs is reachable.'])
        self.assertEqual(len(self.calls), 8)
        self.assertEqual(self.calls[0][0], ['-s', '-o', '/dev/null', '-w', '%{http_code}', '--max-time', '20',
                                            'https://deb.debian.org/debian/dists/stable/Release'])
        self.assertEqual(self.streams.stderr, '')

    def test_the_table_has_its_columns_at_fixed_offsets(self):
        self.probe()
        rows = self.streams.stdout.splitlines()
        self.assertEqual(rows[0], 'HOST' + ' ' * 21 + 'CODE   VERDICT  NEEDED-FOR')
        self.assertEqual(rows[1], '-----------------------' + ' ' * 2 + '-----  -------  ----------')
        need = 'Debian packages (libtkrzw-dev, libkyotocabinet-dev, glib, valgrind)'
        self.assertEqual(rows[2], 'deb.debian.org' + ' ' * 11 + '200    ok       ' + need)
        self.assertEqual(len(rows), 2 + 8 + 2)

    def test_000_403_and_407_are_denied_and_the_rest_answered(self):
        urls = [url for url, _, _ in probes.EGRESS_PROBES]
        codes = dict(zip(urls, (b'000', b'403', b'407', b'200', b'401', b'301', b'500', b'404')))
        status = self.probe(codes=codes)
        self.assertEqual(status, 1)
        verdicts = [row.split()[2] for row in self.streams.stdout.splitlines()[2:10]]
        self.assertEqual(verdicts, ['DENIED', 'DENIED', 'DENIED', 'ok', 'ok', 'ok', 'ok', 'ok'])
        self.assertEqual(self.streams.stdout.splitlines()[-4:],
                         ['', 'RESULT: at least one required host is unreachable or denied.',
                          '        A perf image build will fail. Report the blocked host;',
                          '        do not route around an egress policy.'])

    def test_one_denial_is_enough(self):
        self.assertEqual(self.probe(codes={'https://github.com/libpinyin/libpinyin.git/info/refs'
                                           '?service=git-upload-pack': b'000'}), 1)

    def test_a_trailing_newline_in_the_code_goes(self):
        self.probe(default=b'200\n')
        self.assertIn(' 200    ok ', self.streams.stdout)

    def test_no_curl_leaves_every_code_empty_and_every_verdict_ok(self):
        # `2>/dev/null` hid the shell's "command not found", and an empty code is no denial.
        with mock.patch.object(probes.shutil, 'which', return_value=None):
            status = probes.probe_egress([], {'PATH': ''})
        self.assertEqual(status, 0)
        self.assertEqual(self.streams.stdout.splitlines()[2].split(), ['deb.debian.org', 'ok', 'Debian', 'packages',
                                                                         '(libtkrzw-dev,', 'libkyotocabinet-dev,',
                                                                         'glib,', 'valgrind)'])

    def test_without_a_proxy_there_is_no_proxy_block(self):
        self.probe()
        self.assertEqual([call for call in self.calls if call[0][0] == '-sS'], [])
        self.assertNotIn('proxy-recorded', self.streams.stdout)

    def test_the_proxy_status_page_lists_each_denial_once(self):
        status = json.dumps({'recentRelayFailures': [
            {'host': 'deb.debian.org', 'kind': 'denied', 'detail': 'not on the allowlist'},
            {'host': 'deb.debian.org', 'kind': 'denied', 'detail': 'again'},
            {'host': 'deb.debian.org', 'kind': 'timeout', 'detail': 'slow'},
            {'kind': 'denied', 'detail': 'no host'},
            {'host': 'github.com', 'detail': 'no kind'}]}).encode()
        self.probe(env={'HTTPS_PROXY': 'http://proxy:3128'}, status=status)
        self.assertEqual(self.calls[-1][0], ['-sS', 'http://proxy:3128/__agentproxy/status'])
        rows = self.streams.stdout.splitlines()
        start = rows.index('--- proxy-recorded denials (deduplicated) ---')
        self.assertEqual(rows[start - 1], '')
        self.assertEqual(rows[start + 1:start + 4],
                         ['  ' + 'deb.debian.org'.ljust(42) + ' denied: not on the allowlist',
                          '  ' + 'deb.debian.org'.ljust(42) + ' timeout: slow',
                          '  ' + 'github.com'.ljust(42) + ' None: no kind'])
        self.assertEqual(rows[start + 4], '')

    def test_a_reply_that_is_not_json_has_no_rows(self):
        for status in (b'', b'not json', b'\xff\xfe'):
            with self.subTest(status=status):
                self.streams.out_bytes.seek(0), self.streams.out_bytes.truncate()
                self.probe(env={'HTTPS_PROXY': 'http://proxy:3128'}, status=status)
                self.assertIn('--- proxy-recorded denials (deduplicated) ---\n\nRESULT', self.streams.stdout)

    def test_a_reply_of_another_shape_ends_the_rows_quietly(self):
        self.assertEqual(probes.denials(b'[1, 2]'), [])
        self.assertEqual(probes.denials(b'{"recentRelayFailures": 5}'), [])
        self.assertEqual(probes.denials(b'{"recentRelayFailures": [{"host": "a", "kind": "k", "detail": "d"}, 3]}'),
                         ['  ' + 'a'.ljust(42) + ' k: d'])
        self.assertEqual(probes.denials(b'{}'), [])

    def test_the_environment_reaches_curl(self):
        self.probe(env={'HTTPS_PROXY': 'http://proxy:3128', 'MARKER': 'seen'})
        self.assertTrue(all(call[1].get('MARKER') == 'seen' for call in self.calls))

    def test_the_probe_list_matches_the_perf_images_needs(self):
        hosts = [host for _, host, _ in probes.EGRESS_PROBES]
        self.assertEqual(hosts, ['deb.debian.org', 'cloudfront.docker.com', 'registry-1.docker.io', 'mirror.gcr.io',
                                 'static.rust-lang.org', 'index.crates.io', 'static.crates.io', 'github.com'])
        self.assertTrue(all(url.startswith('https://') and not url.endswith('.com') for url, _, _ in probes.EGRESS_PROBES))


@unittest.skipUnless(shutil.which('curl'), 'needs curl')
class RealCurlTests(unittest.TestCase):
    def test_a_refused_connection_prints_000(self):
        with socket.socket() as probe:
            probe.bind(('127.0.0.1', 0))
            port = probe.getsockname()[1]
        # A proxy in the environment would answer for the loopback address; this checks curl's own refusal.
        env = {key: value for key, value in os.environ.items() if 'proxy' not in key.lower()}
        status, output = probes.curl(['-s', '-o', '/dev/null', '-w', '%{http_code}', '--max-time', '20',
                                      f'http://127.0.0.1:{port}/'], env)
        self.assertEqual(output, b'000')
        self.assertNotEqual(status, 0)


STUB_CARGO = '''#!/bin/bash
echo "cargo $*" >> "$STUB_LOG"
echo "  CARGO_TERM_QUIET=${CARGO_TERM_QUIET-unset} RUSTFLAGS=${RUSTFLAGS-unset} PROFILE_DEBUG=${CARGO_PROFILE_RELEASE_DEBUG-unset}" >> "$STUB_LOG"
echo "  cwd=$PWD" >> "$STUB_LOG"
[ crates/oxpinyin-capi/src/lib.rs -nt "$STUB_MARKER" ] && echo "  lib.rs touched" >> "$STUB_LOG" || echo "  lib.rs stale" >> "$STUB_LOG"
echo "stdout noise"; echo "stderr noise" >&2
[ "${STUB_CARGO_STATUS:-0}" = 0 ] || exit "$STUB_CARGO_STATUS"
mkdir -p target/release
if [ -n "${CARGO_PROFILE_RELEASE_DEBUG-}" ]; then printf 'profile-build' > "target/release/${STUB_ARTIFACT:-liboxpinyin-capi.so}"
elif [ -n "${RUSTFLAGS-}" ] && [ -n "${STUB_RUSTFLAGS_DIFFERS-}" ]; then printf 'rustflags!' > "target/release/${STUB_ARTIFACT:-liboxpinyin-capi.so}"
else printf 'plain-build' > "target/release/${STUB_ARTIFACT:-liboxpinyin-capi.so}"; fi
'''
STUB_READELF = '''#!/bin/bash
echo "readelf $*" >> "$STUB_LOG"
echo "  [ 1] .text             PROGBITS         0000000000001000  00001000"
if grep -q profile-build "$2"; then
  echo "  [27] .debug_aranges    PROGBITS         0000000000000000  0013b0e6"
  echo "  [28] .debug_info       PROGBITS         0000000000000000  0013b116"
fi
'''
STUB_OBJCOPY = '''#!/bin/bash
echo "objcopy $*" >> "$STUB_LOG"
cp "$4" "$5"
'''
STUB_RUSTC = '''#!/bin/bash
echo "rustc 1.97.1 (stub 2026-10-27)"
'''


class DebugInfoProbeTests(unittest.TestCase):
    def setUp(self):
        self.streams = self.enterContext(Streams())
        directory = Path(self.enterContext(tempfile.TemporaryDirectory()))
        self.root = directory / 'repo'
        (self.root / 'crates/oxpinyin-capi/src').mkdir(parents=True)
        (self.root / 'crates/oxpinyin-capi/Cargo.toml').write_text(
            '[package]\nname = "oxpinyin-capi"\n[lib]\nname = "pinyin"\n')
        (self.root / 'crates/oxpinyin-capi/src/lib.rs').write_text('')
        self.stubs = directory / 'stubs'
        self.stubs.mkdir()
        # Nothing else is on PATH, so a tool a test removes is missing whatever this host has installed.
        self.support = directory / 'support'
        self.support.mkdir()
        for name in ('cp', 'grep', 'mkdir'):
            (self.support / name).symlink_to(shutil.which(name))
        for name, body in (('cargo', STUB_CARGO), ('readelf', STUB_READELF), ('objcopy', STUB_OBJCOPY),
                           ('rustc', STUB_RUSTC)):
            (self.stubs / name).write_text(body)
            (self.stubs / name).chmod(0o755)
        self.log = directory / 'stub.log'
        self.log.touch()
        self.marker = directory / 'marker'
        self.marker.touch()
        for path in (self.marker, self.root / 'crates/oxpinyin-capi/src/lib.rs'):
            os.utime(path, (1_000_000_000, 1_000_000_000))
        self.env = {'PATH': f'{self.stubs}:{self.support}', 'STUB_LOG': str(self.log), 'STUB_MARKER': str(self.marker),
                    'STUB_ARTIFACT': 'libpinyin.so'}
        self.compared = []
        self.report = 'REPORT LINE\n'
        self.verdict = 0
        for patch in (mock.patch.object(probes, 'ROOT', self.root),
                      mock.patch.object(probes.bench, 'utc_now', return_value='2026-10-27T01:02:03Z'),
                      mock.patch.object(probes.neutrality, 'compare', self.compare)):
            self.enterContext(patch)

    def compare(self, first, second):
        self.compared.append((Path(first), Path(second), Path(first).read_bytes(), Path(second).read_bytes()))
        return self.verdict, self.report

    def probe(self, argv=(), **env):
        return runners.guarded(lambda: probes.probe_debuginfo_neutrality(list(argv), {**self.env, **env}))

    def test_the_whole_probe_prints_what_the_shell_script_did(self):
        status = self.probe()
        self.assertEqual(status, 0)
        text_digest = {tag: hashlib.sha256(content).hexdigest()[:16] for tag, content in
                       (('stock', b'plain-build'), ('rustflags', b'plain-build'), ('profile', b'profile-build'))}
        self.assertEqual(self.streams.stdout,
                         '=== debug-info neutrality: oxpinyin-capi, backend bdb ===\n'
                         'captured_utc: 2026-10-27T01:02:03Z\n'
                         'rustc: rustc 1.97.1 (stub 2026-10-27)\n'
                         '\n'
                         + 'stock'.ljust(10) + ' ' + '11 B'.ljust(12) + ' debug sections: 0\n'
                         + 'rustflags'.ljust(10) + ' ' + '11 B'.ljust(12) + ' debug sections: 0\n'
                         + 'profile'.ljust(10) + ' ' + '13 B'.ljust(12) + ' debug sections: 2\n'
                         '\n'
                         '=== .text ===\n'
                         + 'stock'.ljust(10) + ' ' + '11'.rjust(10) + f' B  sha256 {text_digest["stock"]}…\n'
                         + 'rustflags'.ljust(10) + ' ' + '11'.rjust(10) + f' B  sha256 {text_digest["rustflags"]}…\n'
                         + 'profile'.ljust(10) + ' ' + '13'.rjust(10) + f' B  sha256 {text_digest["profile"]}…\n'
                         '\n'
                         'CONFIRMED: RUSTFLAGS=-Cdebuginfo=1 produced a byte-identical\n'
                         '           artifact — cargo stripped it. Use the profile.\n'
                         '\n'
                         '=== verdict: stock vs profile-debug ===\n'
                         'REPORT LINE\n')
        self.assertEqual(self.streams.stderr, '')

    def test_the_status_is_the_comparisons(self):
        self.verdict, self.report = 1, 'FAIL (c): different\n'
        self.assertEqual(self.probe(), 1)
        self.assertTrue(self.streams.stdout.endswith('=== verdict: stock vs profile-debug ===\nFAIL (c): different\n'))

    def test_each_build_is_a_cargo_build_with_its_own_setting_and_a_touched_lib_rs(self):
        self.probe(['tkrzw'])
        built = self.log.read_text().split('cargo ')[1:]
        self.assertEqual(len(built), 3)
        flags = '--locked --release -p oxpinyin-capi --no-default-features --features tkrzw'
        self.assertEqual([entry.splitlines()[0] for entry in built], ['build ' + flags] * 3)
        self.assertEqual([entry.splitlines()[1].strip() for entry in built],
                         ['CARGO_TERM_QUIET=true RUSTFLAGS=unset PROFILE_DEBUG=unset',
                          'CARGO_TERM_QUIET=unset RUSTFLAGS=-Cdebuginfo=1 PROFILE_DEBUG=unset',
                          'CARGO_TERM_QUIET=unset RUSTFLAGS=unset PROFILE_DEBUG=1'])
        self.assertEqual({entry.splitlines()[2].strip() for entry in built}, {f'cwd={self.root.resolve()}'})
        self.assertEqual([entry.splitlines()[3].strip() for entry in built], ['lib.rs touched'] * 3)
        self.assertIn('backend tkrzw ===', self.streams.stdout)

    def test_cargo_prints_nothing_to_the_probes_output(self):
        self.probe()
        self.assertNotIn('noise', self.streams.stdout + self.streams.stderr)

    def test_an_empty_backend_argument_is_the_default(self):
        self.probe([''])
        self.assertIn('backend bdb ===', self.streams.stdout)

    def test_pkg_names_the_crate(self):
        other = self.root / 'crates/other'
        (other / 'src').mkdir(parents=True)
        (other / 'Cargo.toml').write_text('[package]\nname = "other"\n')
        (other / 'src/lib.rs').write_text('')
        status = self.probe(PKG='other', STUB_ARTIFACT='libother.so')
        self.assertEqual(status, 0)
        self.assertIn('=== debug-info neutrality: other, backend bdb ===', self.streams.stdout)
        self.assertIn('-p other ', self.log.read_text())

    def test_a_crate_without_a_library_section_names_its_file_after_the_package(self):
        (self.root / 'crates/oxpinyin-capi/Cargo.toml').write_text('[package]\nname = "oxpinyin-capi"\n')
        self.assertEqual(self.probe(STUB_ARTIFACT='liboxpinyin-capi.so'), 0)

    def test_a_different_rustflags_artifact_is_a_note(self):
        self.assertEqual(self.probe(STUB_RUSTFLAGS_DIFFERS='1'), 0)
        self.assertIn('NOTE: RUSTFLAGS=-Cdebuginfo=1 did change the artifact on this\n'
                      '      toolchain; the strip-back behaviour above may have been fixed.\n',
                      self.streams.stdout)
        self.assertNotIn('CONFIRMED', self.streams.stdout)

    def test_the_comparison_sees_the_stock_and_profile_builds_and_they_are_gone_afterwards(self):
        self.probe()
        first, second, first_bytes, second_bytes = self.compared[0]
        self.assertEqual((first.name, second.name, first_bytes, second_bytes),
                         ('stock.so', 'profile.so', b'plain-build', b'profile-build'))
        self.assertFalse(first.parent.exists())

    def test_the_text_sections_are_extracted_with_objcopy(self):
        self.probe()
        extracted = [line for line in self.log.read_text().splitlines() if line.startswith('objcopy')]
        self.assertEqual(len(extracted), 3)
        self.assertTrue(all(line.startswith('objcopy -O binary --only-section=.text ') for line in extracted))

    def test_a_failing_build_ends_the_probe_with_its_status_and_no_message(self):
        self.assertEqual(self.probe(STUB_CARGO_STATUS='101'), 101)
        self.assertEqual(self.streams.stderr, '')
        self.assertNotIn('stock ', self.streams.stdout)
        self.assertEqual(len(self.log.read_text().split('cargo ')) - 1, 1)

    def test_a_missing_manifest_is_the_seds_complaint_and_status_2(self):
        (self.root / 'crates/oxpinyin-capi/Cargo.toml').unlink()
        self.assertEqual(self.probe(), 2)
        self.assertEqual(self.streams.stderr,
                         "sed: can't read crates/oxpinyin-capi/Cargo.toml: No such file or directory\n")
        self.assertIn('rustc: rustc 1.97.1 (stub 2026-10-27)', self.streams.stdout)

    def test_a_missing_lib_rs_is_the_touch_complaint_and_status_1(self):
        shutil.rmtree(self.root / 'crates/oxpinyin-capi/src')
        self.assertEqual(self.probe(), 1)
        self.assertEqual(self.streams.stderr,
                         "touch: cannot touch 'crates/oxpinyin-capi/src/lib.rs': No such file or directory\n")

    def test_a_missing_artifact_is_the_cp_complaint_and_status_1(self):
        self.assertEqual(self.probe(STUB_ARTIFACT='libelsewhere.so'), 1)
        self.assertEqual(self.streams.stderr,
                         "cp: cannot stat 'target/release/libpinyin.so': No such file or directory\n")

    def test_no_cargo_is_status_127_without_a_message(self):
        (self.stubs / 'cargo').unlink()
        self.assertEqual(self.probe(), 127)
        self.assertEqual(self.streams.stderr, '')

    def test_no_rustc_is_noted_and_the_probe_goes_on(self):
        (self.stubs / 'rustc').unlink()
        self.assertEqual(self.probe(), 0)
        self.assertEqual(self.streams.stderr, 'rustc: command not found\n')
        self.assertIn('rustc: \n', self.streams.stdout)

    def test_no_readelf_counts_no_debug_sections(self):
        (self.stubs / 'readelf').unlink()
        self.assertEqual(self.probe(), 0)
        self.assertIn('debug sections: 0\n', self.streams.stdout)
        self.assertNotIn('debug sections: 2', self.streams.stdout)

    def test_a_missing_objcopy_stops_before_the_table(self):
        (self.stubs / 'objcopy').unlink()
        self.assertEqual(self.probe(), 127)
        self.assertNotIn('=== .text ===', self.streams.stdout)

    def test_the_probe_leaves_the_working_directory_as_it_found_it(self):
        before = os.getcwd()
        self.probe()
        self.assertEqual(os.getcwd(), before)


class RecipeTests(unittest.TestCase):
    def test_the_recipes_are_the_two_probes(self):
        self.assertEqual(sorted(probes.RECIPES), ['probe-debuginfo-neutrality', 'probe-egress'])
        self.assertIs(probes.RECIPES['probe-egress'], probes.probe_egress)


if __name__ == '__main__':
    unittest.main()
