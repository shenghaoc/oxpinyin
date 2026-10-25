"""The hot-stack printer resolves samply sidecars and never invents names."""

import gzip
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('extract-hot-stacks.py')
LIB = 'libpinyin_capi.dylib'
CODE_ID = 'ABC123'


def raw_profile(addresses):
    """A save-only profile shape: raw library-relative addresses, no names."""
    strings = [f'0x{address:x}' for address in addresses]
    return {
        'libs': [{
            'name': LIB,
            'debugName': LIB,
            'codeId': CODE_ID,
            'breakpadId': '000000000000000000000000000000000',
        }],
        'threads': [{
            'stringArray': strings + [LIB],
            'samples': {'stack': [1, 1]},
            'stackTable': {'prefix': [None, 0], 'frame': [0, 1]},
            'frameTable': {'func': [0, 1], 'address': list(addresses)},
            'funcTable': {'name': [0, 1], 'resource': [0, 0]},
            'resourceTable': {
                'lib': [0],
                'name': [len(addresses)],
                'host': [None],
                'type': [1],
            },
        }],
    }


def flat_profile(addresses, samples, names=None):
    """One frame per stack row, so each sample names exactly one stack."""
    names = (list(names) if names is not None
             else [f'0x{address:x}' for address in addresses])
    return {
        'libs': [{
            'name': LIB,
            'debugName': LIB,
            'codeId': CODE_ID,
            'breakpadId': '000000000000000000000000000000000',
        }],
        'threads': [{
            'stringArray': names + [LIB],
            'samples': {'stack': list(samples)},
            'stackTable': {
                'prefix': [None] * len(addresses),
                'frame': list(range(len(addresses))),
            },
            'frameTable': {
                'func': list(range(len(addresses))),
                'address': list(addresses),
            },
            'funcTable': {
                'name': list(range(len(addresses))),
                'resource': [0] * len(addresses),
            },
            'resourceTable': {
                'lib': [0],
                'name': [len(addresses)],
                'host': [None],
                'type': [1],
            },
        }],
    }


def symbol_sidecar():
    """The samply `.syms.json` shape: RVAs into a shared string table."""
    return {
        'string_table': [
            'UNKNOWN',
            'oxpinyin::capi::hot',
            'oxpinyin::capi::leaf',
        ],
        'data': [{
            'debug_name': LIB,
            'debug_id': '00000000-0000-0000-0000-000000000000',
            'code_id': CODE_ID,
            'symbol_table': [
                {'rva': 0x10, 'size': 0x10, 'symbol': 1},
                {'rva': 0x20, 'size': 0x10, 'symbol': 2},
            ],
            'known_addresses': [[0x10, 0], [0x20, 1]],
        }],
    }


def single_symbol_sidecar(rva, name):
    return {
        'string_table': ['UNKNOWN', name],
        'data': [{
            'debug_name': LIB,
            'debug_id': '00000000-0000-0000-0000-000000000000',
            'code_id': CODE_ID,
            'symbol_table': [{'rva': rva, 'size': 0x10, 'symbol': 1}],
            'known_addresses': [[rva, 0]],
        }],
    }


def index_only_thread():
    """One unresolved frame with no library or RVA."""
    return {
        'stringArray': ['0x0'],
        'samples': {'stack': [0]},
        'stackTable': {'prefix': [None], 'frame': [0]},
        'frameTable': {'func': [0]},
        'funcTable': {'name': [0]},
    }


def run_report(profile, sidecar=None, args=()):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / 'profile.json.gz'
        with gzip.open(path, 'wt', encoding='utf-8') as handle:
            json.dump(profile, handle)
        if sidecar is not None:
            sidecar_path = Path(directory) / 'profile.json.syms.json'
            sidecar_path.write_text(json.dumps(sidecar), encoding='utf-8')
        return subprocess.run([sys.executable, str(SCRIPT), *args, str(path)],
                              capture_output=True, text=True)


def run_on_file(content, *args):
    """Run the script on a profile file holding exactly these bytes."""
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / 'profile.json.gz'
        path.write_bytes(content)
        return subprocess.run([sys.executable, str(SCRIPT), *args, str(path)],
                              capture_output=True, text=True)


def default_level():
    """The level the script applies when --min-samples is not given."""
    spec = importlib.util.spec_from_file_location('extract_hot_stacks', SCRIPT)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.MIN_SAMPLES


def hot_samples(count):
    """`count` samples, all on one named frame."""
    return flat_profile([0x10], [0] * count, names=['hot'])


class ReportTests(unittest.TestCase):
    def test_reports_count_and_percentage_for_a_ranked_stack(self):
        profile = {
            'threads': [{
                'stringArray': ['root', 'leaf'],
                'samples': {'stack': [1, 1]},
                'stackTable': {'prefix': [None, 0], 'frame': [0, 1]},
                'frameTable': {'func': [0, 1]},
                'funcTable': {'name': [0, 1]},
            }],
        }
        result = run_report(profile)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(' 1.      2  (100.0%)  leaf', result.stdout)


class SidecarTests(unittest.TestCase):
    def test_sidecar_names_raw_oxpinyin_frames(self):
        result = run_report(raw_profile([0x10, 0x20]), symbol_sidecar())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('oxpinyin::capi::hot', result.stdout)
        self.assertIn('oxpinyin::capi::leaf', result.stdout)
        self.assertNotIn('0x10', result.stdout)
        self.assertNotIn('0x20', result.stdout)

    def test_missing_sidecar_labels_each_unresolved_frame(self):
        result = run_report(raw_profile([0x10, 0x20]))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('# symbol sidecar: missing at', result.stdout)
        self.assertIn(
            'unresolved (libpinyin_capi.dylib+0x10)', result.stdout)
        self.assertIn(
            'unresolved (libpinyin_capi.dylib+0x20)', result.stdout)

    def test_sidecar_miss_labels_unresolved_frame(self):
        result = run_report(raw_profile([0x10, 0x30]), symbol_sidecar())
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('oxpinyin::capi::hot', result.stdout)
        self.assertIn(
            'unresolved (libpinyin_capi.dylib+0x30)', result.stdout)

    def test_sidecar_offset_is_not_used_as_a_name(self):
        sidecar = symbol_sidecar()
        sidecar['string_table'][1] = '0x10'
        result = run_report(raw_profile([0x10, 0x20]), sidecar)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('oxpinyin::capi::leaf', result.stdout)
        self.assertIn(
            'unresolved (libpinyin_capi.dylib+0x10)', result.stdout)
        self.assertNotIn('       0x10\n', result.stdout)

    def test_distinct_unresolved_stacks_are_not_merged(self):
        result = run_report(flat_profile([0x10, 0x20], [0, 1]))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            ' 1.      1  ( 50.0%)  unresolved '
            '(libpinyin_capi.dylib+0x10)', result.stdout)
        self.assertIn(
            ' 2.      1  ( 50.0%)  unresolved '
            '(libpinyin_capi.dylib+0x20)', result.stdout)
        self.assertNotIn('100.0%', result.stdout)

    def test_frame_index_fallback_is_per_thread(self):
        profile = {'threads': [index_only_thread(), index_only_thread()]}
        result = run_report(profile)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            ' 1.      1  ( 50.0%)  unresolved (thread 0 frame 0)',
            result.stdout)
        self.assertIn(
            ' 2.      1  ( 50.0%)  unresolved (thread 1 frame 0)',
            result.stdout)
        self.assertNotIn('100.0%', result.stdout)

    def test_named_stack_outranks_scattered_unresolved_samples(self):
        profile = flat_profile([0x10, 0x20, 0x30], [0, 1, 2, 2])
        result = run_report(
            profile, single_symbol_sidecar(0x30, 'oxpinyin::capi::hot'))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            ' 1.      2  ( 50.0%)  oxpinyin::capi::hot', result.stdout)
        self.assertIn(
            ' 2.      1  ( 25.0%)  unresolved '
            '(libpinyin_capi.dylib+0x10)', result.stdout)
        self.assertIn(
            ' 3.      1  ( 25.0%)  unresolved '
            '(libpinyin_capi.dylib+0x20)', result.stdout)


class SampleTotalTests(unittest.TestCase):
    """A report says how many samples stand behind its order."""

    def test_report_gives_the_total_at_the_level(self):
        result = run_report(hot_samples(3), args=['--min-samples', '3'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            '# samples: 3 (at least the 3 needed to rank hot stacks)',
            result.stdout)
        self.assertEqual(result.stderr, '')

    def test_report_warns_below_the_level_and_still_ranks(self):
        result = run_report(hot_samples(2), args=['--min-samples', '3'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(
            '# samples: 2 (fewer than the 3 needed to rank hot stacks)',
            result.stdout)
        self.assertIn(
            'warning: the profile has 2 of the 3 samples needed to rank '
            'hot stacks', result.stderr)
        self.assertIn(' 1.      2  (100.0%)  hot', result.stdout)

    def test_total_prints_only_the_sample_line(self):
        result = run_report(hot_samples(3),
                            args=['--total', '--min-samples', '3'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stdout,
            'samples: 3 (at least the 3 needed to rank hot stacks)\n')
        self.assertEqual(result.stderr, '')

    def test_total_below_the_level_warns_without_failing(self):
        result = run_report(hot_samples(2),
                            args=['--total', '--min-samples', '3'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stdout,
            'samples: 2 (fewer than the 3 needed to rank hot stacks)\n')
        self.assertIn('warning: the profile has 2 of the 3 samples needed',
                      result.stderr)
        self.assertIn('PERF_CYCLES', result.stderr)

    def test_default_level_applies_without_the_option(self):
        level = default_level()
        short = run_report(hot_samples(level - 1), args=['--total'])
        self.assertIn(f'(fewer than the {level} needed', short.stdout)
        self.assertIn('warning:', short.stderr)
        enough = run_report(hot_samples(level), args=['--total'])
        self.assertIn(f'(at least the {level} needed', enough.stdout)
        self.assertEqual(enough.stderr, '')

    def test_total_counts_every_thread(self):
        profile = hot_samples(2)
        profile['threads'].append(hot_samples(3)['threads'][0])
        result = run_report(profile, args=['--total', '--min-samples', '5'])
        self.assertEqual(
            result.stdout,
            'samples: 5 (at least the 5 needed to rank hot stacks)\n')

    def test_total_skips_samples_without_a_stack(self):
        profile = hot_samples(2)
        profile['threads'][0]['samples']['stack'] += [None, -1, 99]
        result = run_report(profile, args=['--total', '--min-samples', '2'])
        self.assertEqual(
            result.stdout,
            'samples: 2 (at least the 2 needed to rank hot stacks)\n')

    def test_total_of_a_profile_without_samples_is_zero_and_warns(self):
        result = run_report({'threads': []}, args=['--total'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('samples: 0 (fewer than the', result.stdout)
        self.assertIn('warning: the profile has 0 of the', result.stderr)

    def test_report_of_a_profile_without_samples_still_fails(self):
        result = run_report({'threads': []})
        self.assertEqual(result.returncode, 1)
        self.assertIn('no samples', result.stdout)

    def test_zero_level_never_warns(self):
        result = run_report({'threads': []},
                            args=['--total', '--min-samples', '0'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, '')

    def test_negative_level_is_a_usage_error(self):
        result = run_report(hot_samples(1), args=['--min-samples', '-1'])
        self.assertEqual(result.returncode, 2)
        self.assertIn('must not be negative', result.stderr)

    def test_unreadable_profile_is_fatal(self):
        cases = {
            'not gzip': b'not gzip',
            'not json': gzip.compress(b'not json'),
            'not an object': gzip.compress(b'[1, 2]'),
            'truncated gzip': gzip.compress(b'{"threads": []}')[:12],
            'corrupt deflate': (b'\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\xff'
                                b'\x07\x00\x00\x00'),
        }
        for name, content in cases.items():
            with self.subTest(name):
                result = run_on_file(content, '--total')
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('fatal: cannot read the profile at',
                              result.stderr)
                self.assertEqual(result.stdout, '')


if __name__ == '__main__':
    unittest.main()
