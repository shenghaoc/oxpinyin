"""The hot-stack printer resolves samply sidecars and never invents names."""

import gzip
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


def run_report(profile, sidecar=None):
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory) / 'profile.json.gz'
        with gzip.open(path, 'wt', encoding='utf-8') as handle:
            json.dump(profile, handle)
        if sidecar is not None:
            sidecar_path = Path(directory) / 'profile.json.syms.json'
            sidecar_path.write_text(json.dumps(sidecar), encoding='utf-8')
        return subprocess.run([sys.executable, str(SCRIPT), str(path)],
                              capture_output=True, text=True)


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
        self.assertIn('# symbols: unresolved (no sidecar at', result.stdout)
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


if __name__ == '__main__':
    unittest.main()
