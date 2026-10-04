#!/usr/bin/env python3
"""Requested-row training differential; isolated libraries and aborting calls.

Usage: train-index-diff.py CELL PREFIX PINYIN_SO ZHUYIN_SO [--expect-parent]
Needs native backend dump tools. Missing inputs and zero cases fail.
Captures live only in a temporary directory, removed even on failure.
"""
import argparse
import ctypes as C
import importlib.util
import hashlib
import json
from pathlib import Path
import shutil
import resource
import subprocess
import sys
import tempfile


def worker(mode, so, data, profile, case):
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    lib = C.CDLL(so)
    ptr, uint, boolean = C.c_void_p, C.c_uint, C.c_bool

    def api(name, result, *args):
        fn = getattr(lib, mode + '_' + name)
        fn.restype, fn.argtypes = result, list(args)
        return fn

    warnings = []
    callback_type = C.CFUNCTYPE(None, C.c_char_p, uint, C.c_char_p, ptr)

    def warning(domain, level, message, unused):
        record = dict(domain=domain.decode(), level=level, message=message.decode())
        warnings.append(record)
        print(json.dumps(record), file=sys.stderr, flush=True)

    callback = callback_type(warning)
    glib = C.CDLL('libglib-2.0.so.0')
    glib.g_log_set_handler.argtypes = [C.c_char_p, uint, callback_type, ptr]
    glib.g_log_set_handler.restype = uint
    glib.g_log_set_handler(b'libpinyin', 16, callback, None)
    context = api('init', ptr, C.c_char_p, C.c_char_p)(data.encode(), profile.encode())
    assert context
    instance = api('alloc_instance', ptr, ptr)(context)
    assert instance
    rows = []
    if case != 'fresh':
        parse = 'parse_more_full_pinyins' if mode == 'pinyin' else 'parse_more_chewings'
        text = b'nihao' if case == 'seed' else b'jintian' if mode == 'pinyin' else b'su3cl3'
        assert api(parse, C.c_size_t, ptr, C.c_char_p)(instance, text)
        if case != 'choose-only':
            assert api('guess_sentence', boolean, ptr)(instance)
        if case == 'seed':
            assert api('train', boolean, ptr, C.c_ubyte)(instance, 0)
            assert api('save', boolean, ptr)(context)
            api('free_instance', None, ptr)(instance)
            api('fini', None, ptr)(context)
            print(json.dumps(dict(seed=True)), flush=True)
            return
        if mode == 'pinyin':
            assert api('guess_candidates', boolean, ptr, C.c_size_t, uint)(instance, 0, 0x1e)
        else:
            assert api('guess_candidates_after_cursor', boolean, ptr, C.c_size_t)(instance, 0)
        size = uint()
        assert api('get_n_candidate', boolean, ptr, C.POINTER(uint))(instance, C.byref(size))
        desired = '今' if mode == 'pinyin' else '你'
        chosen = False
        for i in range(size.value):
            candidate, kind, value = ptr(), C.c_int(), C.c_char_p()
            assert api('get_candidate', boolean, ptr, uint, C.POINTER(ptr))(instance, i, C.byref(candidate))
            assert api('get_candidate_type', boolean, ptr, ptr, C.POINTER(C.c_int))(instance, candidate, C.byref(kind))
            assert api('get_candidate_string', boolean, ptr, ptr, C.POINTER(C.c_char_p))(instance, candidate, C.byref(value))
            if kind.value == 2 and value.value.decode() == desired:
                assert api('choose_candidate', C.c_int, ptr, C.c_size_t, ptr)(instance, 0, candidate) > 0
                chosen = True
                break
        assert chosen, desired
        if case != 'choose-only':
            assert api('guess_sentence', boolean, ptr)(instance)
            if mode == 'pinyin':
                assert api('guess_candidates', boolean, ptr, C.c_size_t, uint)(instance, 0, 0x1e)
                assert api('get_n_candidate', boolean, ptr, C.POINTER(uint))(instance, C.byref(size))
                indices = []
                for i in range(size.value):
                    candidate, kind, row_index = ptr(), C.c_int(), C.c_ubyte()
                    assert api('get_candidate', boolean, ptr, uint, C.POINTER(ptr))(instance, i, C.byref(candidate))
                    assert api('get_candidate_type', boolean, ptr, ptr, C.POINTER(C.c_int))(instance, candidate, C.byref(kind))
                    if kind.value == 1:
                        assert api('get_candidate_nbest_index', boolean, ptr, ptr, C.POINTER(C.c_ubyte))(instance, candidate, C.byref(row_index))
                        indices.append(row_index.value)
                indices = sorted(set(indices))
                assert indices == list(range(len(indices))) and indices
            else:
                indices = [0]
            for i in indices:
                value = C.c_char_p()
                if mode == 'pinyin':
                    assert api('get_sentence', boolean, ptr, C.c_ubyte, C.POINTER(C.c_char_p))(instance, i, C.byref(value))
                else:
                    assert api('get_sentence', boolean, ptr, C.POINTER(C.c_char_p))(instance, C.byref(value))
                rows.append(value.value.decode())
                glib.g_free.argtypes = [ptr]
                glib.g_free.restype = None
                glib.g_free(value)
    print(json.dumps(dict(rows=rows)), flush=True)
    if case == 'rows':
        api('free_instance', None, ptr)(instance)
        api('fini', None, ptr)(context)
        return
    index = len(rows) if case == 'len' else int(case) if case.isdigit() else 0
    trained = api('train', boolean, ptr, C.c_ubyte)(instance, index) if mode == 'pinyin' else api('train', boolean, ptr)(instance)
    saved = api('save', boolean, ptr)(context)
    api('free_instance', None, ptr)(instance)
    api('fini', None, ptr)(context)
    print(json.dumps(dict(trained=trained, saved=saved, warnings=warnings)), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('cell', choices=['bdb', 'kc', 'tkrzw'])
    parser.add_argument('prefix', type=Path)
    parser.add_argument('pinyin_so', type=Path)
    parser.add_argument('zhuyin_so', type=Path)
    parser.add_argument('--expect-parent', action='store_true')
    args = parser.parse_args()
    pin = args.prefix / 'lib'
    data = pin / 'libpinyin/data'
    for path in [args.prefix / 'oracle-pin.txt', pin / 'libpinyin.so', pin / 'libzhuyin.so', args.pinyin_so, args.zhuyin_so, data / 'bigram.db']:
        assert path.is_file(), path
    manifest = (args.prefix / 'oracle-pin.txt').read_text()
    assert '074a2219c90feaf962d0d24f034514033ece5f99' in manifest
    assert '+dbm-' + args.cell + '\n' in manifest
    assert '+patches-' not in manifest, 'training requires the unmodified pin'
    spec = importlib.util.spec_from_file_location('state', Path(__file__).with_name('compare-nbest-training-state.py'))
    state = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(state)
    count = 0
    with tempfile.TemporaryDirectory(prefix='train-index-') as scratch:
        root = Path(scratch)
        # A pin-produced, constraint-free save creates compatible empty stores.
        seed = root / 'seed'
        seed.mkdir()
        result = subprocess.run([sys.executable, __file__, '--worker', 'pinyin', str(pin / 'libpinyin.so'), str(data), str(seed), 'seed'], capture_output=True, text=True)
        assert result.returncode == 0, result.stderr
        for mode, subject in [('pinyin', args.pinyin_so), ('zhuyin', args.zhuyin_so)]:
            preflight = []
            for side, library in [('pin', pin / ('lib' + mode + '.so')), ('subject', subject)]:
                profile = root / (mode + '-rows-' + side)
                shutil.copytree(seed, profile)
                output = subprocess.run([sys.executable, __file__, '--worker', mode, str(library.resolve()), str(data), str(profile), 'rows'], capture_output=True, text=True)
                assert output.returncode == 0 and not output.stderr, output.stderr
                preflight.append(json.loads(output.stdout)['rows'])
            expected_rows = ['今天', '今田', '今添'] if mode == 'pinyin' else ['你好']
            assert preflight[0] == preflight[1] == expected_rows
            print(json.dumps(dict(cell=args.cell, mode=mode, pre_training_rows=preflight, identical=True)), flush=True)
            cases = ['0', '1', '2', 'len', '255', 'fresh', 'choose-only'] if mode == 'pinyin' else ['0', 'fresh', 'choose-only']
            for case in cases:
                results, profiles = [], []
                for side, library in [('pin', pin / ('lib' + mode + '.so')), ('subject', subject)]:
                    profile = root / (mode + '-' + case + '-' + side)
                    shutil.copytree(seed, profile)
                    output = subprocess.run([sys.executable, __file__, '--worker', mode, str(library.resolve()), str(data), str(profile), case], capture_output=True, text=True)
                    lines = [json.loads(line) for line in output.stdout.splitlines()]
                    assert lines, output.stderr
                    results.append((output, lines))
                    profiles.append(profile)
                expected_rows = ['今天', '今田', '今添'] if mode == 'pinyin' else ['你好']
                rows = results[0][1][0]['rows']
                assert rows == results[1][1][0]['rows']
                assert rows == ([] if case in ['fresh', 'choose-only'] else expected_rows)
                bounds = case in ['len', '255']
                if bounds:
                    assert results[0][0].returncode == -6, results[0][0].stderr
                else:
                    assert results[0][0].returncode == 0, results[0][0].stderr
                assert results[1][0].returncode == 0, results[1][0].stderr
                if not bounds:
                    assert all(not result[0].stderr for result in results), [result[0].stderr for result in results]
                actual = results[1][1][-1]
                if bounds and not args.expect_parent:
                    assert actual['trained'] is False and actual['saved'] is False
                    assert len(actual['warnings']) == 1
                    assert actual['warnings'][0]['domain'] == 'libpinyin'
                    assert actual['warnings'][0]['level'] == 16
                    assert len(results[1][0].stderr.splitlines()) == 1
                elif bounds:
                    assert actual['trained'] and actual['saved'] and not actual['warnings']
                elif args.expect_parent and case == 'choose-only':
                    assert actual['trained'] and not results[0][1][-1]['trained']
                else:
                    assert actual['trained'] == results[0][1][-1]['trained']
                    assert actual['saved'] == results[0][1][-1]['saved']
                    assert not actual['warnings']
                def records(profile):
                    result = {}
                    for path in profile.glob('*.dbin'):
                        result[path.name] = path.read_bytes()
                    if (profile / 'user_bigram.db').exists():
                        result['bigram'] = state.bigrams(args.cell, profile)
                    for name in ['user.bin', 'user_addon.bin', 'user_network.bin']:
                        path = profile / name
                        if path.exists():
                            result[name] = path.read_bytes()
                    for name in ['user_pinyin_index.bin', 'user_phrase_index.bin']:
                        path = profile / name
                        if path.exists():
                            dump_profile = root / 'index-dump'
                            if dump_profile.exists():
                                shutil.rmtree(dump_profile)
                            dump_profile.mkdir()
                            shutil.copyfile(path, dump_profile / 'user_bigram.db')
                            result[name] = state.bigrams(args.cell, dump_profile)
                    return result
                subject_state = records(profiles[1])
                reference_state = records(seed if bounds else profiles[0])
                equal = subject_state == reference_state
                expected_equal = not (args.expect_parent and (bounds or case in ['1', '2', 'choose-only']))
                assert equal == expected_equal, (mode, case, equal, expected_equal)
                print(json.dumps(dict(cell=args.cell, mode=mode, case=case, rows=rows, pin_return='SIGABRT' if bounds else results[0][1][-1]['trained'], subject_return=actual['trained'], records_and_dbin_equal=equal, stderr_lines=[len(r[0].stderr.splitlines()) for r in results], warnings=actual['warnings'], exported_bigram_records=[(key.hex(), value.hex()) for key, value in subject_state.get('bigram', [])], dbin_sha256={name: hashlib.sha256(value).hexdigest() for name, value in subject_state.items() if name.endswith('.dbin')}, raw_index_equal={name: (profiles[0] / name).read_bytes() == (profiles[1] / name).read_bytes() for name in ['user_pinyin_index.bin', 'user_phrase_index.bin'] if not bounds and (profiles[0] / name).exists()})), flush=True)
                count += 1
    assert count == 10


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--worker':
        worker(*sys.argv[2:])
    else:
        main()
